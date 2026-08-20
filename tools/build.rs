use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("..")
        .canonicalize()
        .unwrap();

    let git_path = |f: &str| -> Option<PathBuf> {
        let out = Command::new("git")
            .args(["-C", root.to_str()?, "rev-parse", "--git-path", f])
            .output()
            .ok()
            .filter(|o| o.status.success())?;
        let path = root.join(String::from_utf8(out.stdout).ok()?.trim());

        path.exists().then_some(path)
    };

    for f in ["HEAD", "packed-refs"] {
        if let Some(path) = git_path(f) {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    if let Some(head) = git_path("HEAD").and_then(|p| fs::read_to_string(p).ok()) {
        if let Some(r) = head.strip_prefix("ref: ") {
            if let Some(path) = git_path(r.trim()) {
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
