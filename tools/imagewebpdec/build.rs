use std::path::PathBuf;
use std::{env, fs};

/* The banner reports the decoder under test, so take its version from the
 * lock file cargo resolved rather than from the "0.2" we asked for. */
fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let lock =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.lock");

    println!("cargo:rerun-if-changed={}", lock.display());

    let version = fs::read_to_string(&lock)
        .ok()
        .and_then(|text| {
            let mut after = text.split("name = \"image-webp\"").nth(1)?.lines();

            after.find_map(|line| {
                Some(
                    line.trim()
                        .strip_prefix("version = ")?
                        .trim_matches('"')
                        .to_owned(),
                )
            })
        })
        .unwrap_or_else(|| "unknown".to_owned());

    println!("cargo:rustc-env=IMAGE_WEBP_VERSION={version}");
}
