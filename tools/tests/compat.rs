#![forbid(unsafe_code)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

/* A synthetic two-pixel lossless red/green image, encoded with cwebp 1.6.0
 * from P6\n2 1\n255\n followed by ff0000 00ff00. */
const STILL: &[u8] = &[
    0x52, 0x49, 0x46, 0x46, 0x1e, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38,
    0x4c, 0x11, 0, 0, 0, 0x2f, 1, 0, 0, 0, 0x0f, 0xb0, 0xff, 0xf3, 0x1f, 0xf3, 0x1f,
    0x15, 0x32, 0xa2, 0xff, 1, 0,
];

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wpd"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();

    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if payload.len() & 1 != 0 {
        out.push(0);
    }
    out
}

fn riff(payload: &[u8]) -> Vec<u8> {
    let mut out = b"RIFF".to_vec();

    out.extend_from_slice(&(payload.len() as u32 + 4).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(payload);
    out
}

#[test]
fn compatibility_applies_to_info_output_muxers_and_streaming() {
    let mut payload = chunk(b"VP8X", &[0, 0, 0, 0, 1, 0, 0, 0, 0, 0]);

    payload.extend(chunk(b"VP8X", &[0, 0, 0, 0, 1, 0, 0, 0, 0, 0]));
    payload.extend_from_slice(&STILL[12..]);
    payload.extend_from_slice(b"JUNK\xff\xff\xff\xfftail");
    let data = riff(&payload);
    let reference = run(&["--muxer", "pam", "-", "-"], STILL);

    assert!(reference.status.success());
    assert!(!run(&["--info", "-"], &data).status.success());
    assert!(run(&["--libwebp-compat", "--info", "-"], &data)
        .status
        .success());
    for stream in ["1", "997"] {
        let output = run(
            &[
                "--libwebp-compat",
                "--stream",
                stream,
                "--muxer",
                "pam",
                "-",
                "-",
            ],
            &data,
        );

        assert!(output.status.success());
        assert_eq!(output.stdout, reference.stdout);
    }
}
