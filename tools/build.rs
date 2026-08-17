//! Stamps the binary with the source revision, as meson's `vcs_tag` did for
//! the C tool.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .canonicalize()
        .unwrap();

    for f in ["HEAD", "packed-refs"] {
        let path = root.join(".git").join(f);
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    if let Ok(head) = fs::read_to_string(root.join(".git/HEAD")) {
        if let Some(r) = head.strip_prefix("ref: ") {
            let path = root.join(".git").join(r.trim());
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    let version = Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "rev-parse",
            "--short=7",
            "HEAD",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());

    println!("cargo:rustc-env=WPD_VCS_VERSION={version}");
}
