//! Passes on what `crates/wpd`'s build script learned about the assembler.
//!
//! Nothing is compiled here and no C compiler is invoked. The decoder's C went
//! with the port, and the last translation unit — a handful of SIMD constants —
//! moved into the `.asm` files that read them. What is left is the two aarch64
//! extension probes: `crates/wpd` runs them, publishes the answers through its
//! `links` key, and this crate turns them back into `cfg`s so its own aarch64
//! dispatch can name the entry points that exist.

use std::env;

/// A value `crates/wpd`'s build script published through its `links` key.
fn dep(name: &str) -> Option<String> {
    env::var(format!("DEP_WPDASM_{name}")).ok()
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(wpd_asm_dotprod, wpd_asm_i8mm)");

    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    if !cfg!(feature = "asm") || arch != "aarch64" {
        return;
    }
    for name in ["dotprod", "i8mm"] {
        if dep(&format!("HAVE_{}", name.to_uppercase())).as_deref() == Some("1") {
            println!("cargo:rustc-cfg=wpd_asm_{name}");
        }
    }
}
