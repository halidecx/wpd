//! Probes the target and compiles the constant tables the x86 assembly reads.
//!
//! No decoder logic is built here any more: the port took the last of it. What
//! is left is the configuration `crates/wpd`'s assembly needs and the one C
//! translation unit that holds the SIMD constants.

use std::path::PathBuf;
use std::{env, fs};

fn repo_root() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../..")
        .canonicalize()
        .unwrap()
}

/// Whether the target has `func`, declared by `prefix`. Meson asks the same
/// question with `cc.has_function()`.
fn has_function(prefix: &str, func: &str) -> bool {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join(format!("has_{func}.c"));
    fs::write(
        &out,
        format!("{prefix}\nvoid wpd_probe(void) {{ (void)&{func}; }}\n"),
    )
    .unwrap();
    cc::Build::new()
        .file(&out)
        .cargo_metadata(false)
        .cargo_warnings(false)
        .warnings(false)
        .try_compile(&format!("wpd_has_{func}"))
        .is_ok()
}

/// A value `crates/wpd`'s build script published through its `links` key.
fn dep(name: &str) -> Option<String> {
    env::var(format!("DEP_WPDASM_{name}")).ok()
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(wpd_asm_dotprod, wpd_asm_i8mm)");

    let root = repo_root();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let asm = cfg!(feature = "asm")
        && matches!(arch.as_str(), "x86_64" | "x86" | "aarch64" | "arm");

    let mut build = cc::Build::new();
    build
        .std("c11")
        .include(&root)
        .include(root.join("include"))
        .include(root.join("src"))
        .flag_if_supported("-fvisibility=hidden")
        .flag_if_supported("-fomit-frame-pointer")
        .flag_if_supported("-Wno-unused-parameter")
        .define("WPD_BUILDING", None)
        .define("WPD_HAVE_ASM", if asm { "1" } else { "0" })
        .define(
            "WPD_HAVE_GETAUXVAL",
            if has_function("#include <sys/auxv.h>", "getauxval") {
                "1"
            } else {
                "0"
            },
        )
        .define(
            "WPD_HAVE_ELF_AUX_INFO",
            if has_function("#include <sys/auxv.h>", "elf_aux_info") {
                "1"
            } else {
                "0"
            },
        );

    if cfg!(feature = "trim_dsp") {
        build.define("WPD_TRIM_DSP_FUNCTIONS", "1");
    }
    if cfg!(feature = "force_rac32") {
        build.define("WPD_FORCE_RAC32", None);
    }

    let mut c_files = 0;

    /* A header change reaches every translation unit that includes it, and a
    stale object from before a struct grew is a mismatch no test can see. */
    for entry in fs::read_dir(root.join("src")).unwrap() {
        let path = entry.unwrap().path();

        if path.extension().is_some_and(|e| e == "h") {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    if asm {
        match arch.as_str() {
            "x86_64" | "x86" => {
                build.file(root.join("src/x86/wpd_simd_constants.c"));
                c_files += 1;
            }
            "aarch64" => {
                build.include(root.join("src/aarch64"));
                for name in ["dotprod", "i8mm"] {
                    let upper = name.to_uppercase();
                    build.define(
                        &format!("HAVE_AS_ARCHEXT_{upper}_DIRECTIVE"),
                        dep(&format!("ARCHEXT_{upper}")).as_deref(),
                    );
                    build.define(
                        &format!("HAVE_{upper}"),
                        dep(&format!("HAVE_{upper}")).as_deref(),
                    );
                    if dep(&format!("HAVE_{upper}")).as_deref() == Some("1") {
                        println!("cargo:rustc-cfg=wpd_asm_{name}");
                    }
                }
            }
            "arm" => {
                build.include(root.join("src/arm"));
                for (var, define) in [
                    ("ARMV6", "WPD_ARM_ARMV6_ASM"),
                    ("ARMV6T2", "WPD_ARM_ARMV6T2_ASM"),
                    ("ARMV6T2_EXTERNAL", "WPD_ARM_ARMV6T2_EXTERNAL_ASM"),
                ] {
                    if dep(var).as_deref() == Some("1") {
                        build.define(define, "1");
                    }
                }
            }
            _ => {}
        }
    }

    /* The port has taken every C source but the assembly's constant tables,
    and those only exist on x86. Handing cc an empty file list is an error. */
    if c_files > 0 {
        build.compile("wpd_c");
    }
}
