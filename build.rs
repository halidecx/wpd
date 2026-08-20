use std::path::{Path, PathBuf};
use std::{env, fs};

fn repo_root() -> PathBuf {
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
}

fn compiles(snippet: &str, name: &str) -> bool {
    let out =
        PathBuf::from(env::var("OUT_DIR").unwrap()).join(format!("probe_{name}.c"));
    fs::write(&out, snippet).unwrap();
    cc::Build::new()
        .file(&out)
        .cargo_metadata(false)
        .cargo_warnings(false)
        .warnings(false)
        .try_compile(&format!("wpd_probe_{name}"))
        .is_ok()
}

fn nasm_common(root: &Path, x86_64: bool) -> nasm_rs::Build {
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let vendor = env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();

    let mut b = nasm_rs::Build::new();
    b.include(root.join("src"))
        .flag("-w-label-orphan")
        .flag("-w-implicit-abs-deprecated")
        .define("PIC", Some("1"))
        .define("HAVE_ALIGNED_STACK", Some("1"))
        .define("HAVE_X86_SSE2AVX", Some("0"))
        .define("ARCH_X86_64", Some(if x86_64 { "1" } else { "0" }))
        .define("ARCH_X86_32", Some(if x86_64 { "0" } else { "1" }));
    if vendor == "apple" || (os == "windows" && !x86_64) {
        b.define("PREFIX", Some("1"));
    }
    b
}

fn build_x86(root: &Path, x86_64: bool) {
    let mut avx2 = nasm_common(root, x86_64);
    avx2.define("HAVE_AVX2_EXTERNAL", Some("1"));
    for f in ["vp8l.asm", "vp8dsp.asm", "vp8_intrapred.asm", "filters.asm"] {
        avx2.file(root.join("src/x86").join(f));
    }
    avx2.compile("wpd_asm_avx2").expect("nasm failed");
    println!("cargo:rustc-link-lib=static=wpd_asm_avx2");

    let mut rest = nasm_common(root, x86_64);
    for f in ["vp8dsp_loopfilter.asm", "yuvdsp.asm"] {
        rest.file(root.join("src/x86").join(f));
    }
    rest.compile("wpd_asm_x86").expect("nasm failed");
    println!("cargo:rustc-link-lib=static=wpd_asm_x86");
}

fn build_aarch64(root: &Path) {
    let mut build = cc::Build::new();
    build
        .include(root.join("src/aarch64"))
        .include(root.join("src"));

    for (name, instr) in [
        ("dotprod", "sdot v0.4s, v0.16b, v0.16b"),
        ("i8mm", "usdot v0.4s, v0.16b, v0.16b"),
    ] {
        let directive = compiles(
            &format!("__asm__ (\".arch_extension {name}\\n\");\n"),
            &format!("archext_{name}"),
        );
        let mut code = String::from("__asm__ (");
        if directive {
            code.push_str(&format!("\".arch_extension {name}\\n\""));
        }
        code.push_str(&format!("\"{instr}\\n\");\n"));
        let have = compiles(&code, name);

        let upper = name.to_uppercase();
        build.define(
            &format!("HAVE_AS_ARCHEXT_{upper}_DIRECTIVE"),
            if directive { "1" } else { "0" },
        );
        build.define(&format!("HAVE_{upper}"), if have { "1" } else { "0" });
        println!("cargo:archext_{name}={}", u8::from(directive));
        println!("cargo:have_{name}={}", u8::from(have));
        if have {
            println!("cargo:rustc-cfg=wpd_asm_{name}");
        }
    }

    for f in [
        "vp8l_neon.S",
        "vp8dsp_neon.S",
        "vp8pred_neon.S",
        "yuvdsp_neon.S",
        "yuvdsp_dotprod.S",
    ] {
        build.file(root.join("src/aarch64").join(f));
    }
    build.compile("wpd_asm_aarch64");
}

fn build_arm(root: &Path) {
    let armv6 = compiles(
        "#if !defined(__ARM_ARCH) || __ARM_ARCH < 6\n\
         #error no ARMv6 on this target\n#endif\n\
         int wpd_probe(void) { return 0; }\n",
        "armv6",
    );
    let armv6t2 = compiles(
        "#if !defined(__ARM_ARCH_6T2__) && (!defined(__ARM_ARCH) || __ARM_ARCH < 7)\n\
         #error no Thumb-2 on this target\n#endif\n\
         int wpd_probe(void) { return 0; }\n",
        "armv6t2",
    );
    let armv6t2_external = armv6t2
        && compiles(
            "void wpd_probe(void) { __asm__(\".syntax unified\\nsbfx r0, r0, #0, #1\"); }\n",
            "armv6t2_ext",
        );

    let mut build = cc::Build::new();
    build
        .include(root.join("src/arm"))
        .include(root.join("src"));
    build.file(root.join("src/arm/vp8dsp_neon.S"));
    build.file(root.join("src/arm/vp8pred_neon.S"));
    println!("cargo:armv6={}", u8::from(armv6));
    println!("cargo:armv6t2={}", u8::from(armv6t2));
    println!("cargo:armv6t2_external={}", u8::from(armv6t2_external));
    if armv6 {
        build.define("WPD_ARM_ARMV6_ASM", "1");
        build.file(root.join("src/arm/vp8dsp_armv6.S"));
        println!("cargo:rustc-cfg=wpd_asm_armv6");
    }
    if armv6t2 {
        build.define("WPD_ARM_ARMV6T2_ASM", "1");
    }
    if armv6t2_external {
        build.define("WPD_ARM_ARMV6T2_EXTERNAL_ASM", "1");
    }
    build.compile("wpd_asm_arm");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(wpd_asm_dotprod, wpd_asm_i8mm, wpd_asm_armv6)");

    if cfg!(not(feature = "asm")) {
        return;
    }

    let root = repo_root();
    println!("cargo:rerun-if-changed={}", root.join("src").display());

    match env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "x86_64" => build_x86(&root, true),
        "x86" => build_x86(&root, false),
        "aarch64" => build_aarch64(&root),
        "arm" => build_arm(&root),
        _ => {}
    }
}
