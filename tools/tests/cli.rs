#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

/* A synthetic two-pixel lossless red/green image, encoded with cwebp 1.6.0
 * from P6\n2 1\n255\n followed by ff0000 00ff00. */
const STILL: &[u8] = &[
    0x52, 0x49, 0x46, 0x46, 0x1e, 0, 0, 0, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38,
    0x4c, 0x11, 0, 0, 0, 0x2f, 1, 0, 0, 0, 0x0f, 0xb0, 0xff, 0xf3, 0x1f, 0xf3, 0x1f,
    0x15, 0x32, 0xa2, 0xff, 1, 0,
];

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wpd-test-data");

        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "cli-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        /* This directory contains only the synthetic inputs and outputs of
         * this test, never the corpus's reference files. */
        fs::remove_dir_all(&self.0).unwrap();
    }
}

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

fn extended() -> Vec<u8> {
    let mut out = chunk(b"VP8X", &[0x2c, 0, 0, 0, 1, 0, 0, 0, 0, 0]);

    out.extend(chunk(b"ICCP", &[0, 255, b'"', b'\\']));
    out.extend_from_slice(&STILL[12..]);
    out.extend(chunk(b"EXIF", b"Exif\0binary"));
    out.extend(chunk(b"XMP ", b"<x>\n</x>"));
    riff(&out)
}

fn animation() -> Vec<u8> {
    let mut out = chunk(b"VP8X", &[2, 0, 0, 0, 1, 0, 0, 0, 0, 0]);

    out.extend(chunk(b"ANIM", &[0, 0, 0, 0, 3, 0]));
    for duration in [0u32, 1, 0xff_ffff] {
        let mut frame = [0u8; 16].to_vec();

        frame[6] = 1;
        frame[12..15].copy_from_slice(&duration.to_le_bytes()[..3]);
        frame[15] = 2;
        frame.extend_from_slice(&STILL[12..]);
        out.extend(chunk(b"ANMF", &frame));
    }
    riff(&out)
}

fn success(output: &Output) -> &str {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::str::from_utf8(&output.stdout).unwrap()
}

#[test]
fn still_json_is_one_complete_object_without_text_info() {
    let output = run(&["--info=json", "-"], STILL);

    assert_eq!(success(&output), concat!(
        "{\"width\":2,\"height\":1,\"frame_count\":1,\"loop_count\":0,",
        "\"has_alpha\":false,\"has_icc\":false,\"durations_ms\":[0],",
        "\"chunks\":[{\"fourcc\":\"VP8L\",\"offset\":12,\"size\":17,\"complete\":true}]}\n"
    ));
    assert!(success(&run(&["--info", "-"], STILL)).starts_with("canvas: 2x1\n"));
}

#[test]
fn streamed_json_keeps_raw_duration_extremes_and_the_loop_count() {
    let data = animation();
    let whole = run(&["--info=json", "-"], &data);
    let text = success(&whole);

    assert!(text.contains("\"frame_count\":3,\"loop_count\":3"));
    assert!(text.contains("\"durations_ms\":[0,1,16777215]"));
    assert_eq!(text.matches("\"fourcc\":\"ANMF\"").count(), 3);
    for size in ["1", "7", "13", "997"] {
        let stream = run(
            &["--info=json", "--stream", size, "--loops", "2", "-"],
            &data,
        );

        assert_eq!(success(&stream), text);
    }
}

#[test]
fn metadata_is_written_as_original_bytes_even_after_streaming() {
    let dir = Scratch::new();
    let icc = dir.path("profile.icc");
    let exif = dir.path("exif.bin");
    let xmp = dir.path("xmp.bin");
    let args = [
        "--info=json",
        "--stream",
        "1",
        "--icc-out",
        &icc,
        "--exif-out",
        &exif,
        "--xmp-out",
        &xmp,
        "-",
    ];
    let output = run(&args, &extended());

    assert!(success(&output).contains("\"has_icc\":true"));
    assert_eq!(fs::read(&icc).unwrap(), [0, 255, b'"', b'\\']);
    assert_eq!(fs::read(&exif).unwrap(), b"Exif\0binary");
    assert_eq!(fs::read(&xmp).unwrap(), b"<x>\n</x>");
    assert!(run(&["--icc-out", &icc, "-"], STILL).status.success());
    assert!(fs::read(&icc).unwrap().is_empty());
}

#[test]
fn failed_decodes_do_not_publish_json_or_metadata() {
    let dir = Scratch::new();
    let icc = dir.path("profile.icc");
    let data = extended();

    for size in ["1", "997"] {
        let output = run(
            &["--info=json", "--stream", size, "--icc-out", &icc, "-"],
            &data[..data.len() - 1],
        );

        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!dir.0.join("profile.icc").exists());
    }
    for cut in 0..STILL.len() - 1 {
        let output = run(&["--info=json", "-"], &STILL[..cut]);

        assert!(!output.status.success(), "accepted truncation at {cut}");
        assert!(output.stdout.is_empty());
    }
    let output = run(&["--info=json", "--max-input", "1", "-"], STILL);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn metadata_io_errors_and_stdout_collisions_fail_cleanly() {
    let dir = Scratch::new();
    let missing = dir.path("missing/profile.icc");
    let output = run(&["--info=json", "--icc-out", &missing, "-"], STILL);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    for args in [vec!["--info=json", "-", "-"], vec!["--icc-out", "-", "-"]] {
        let output = run(&args, &[]);

        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn a_frame_sequence_keeps_durations_and_matches_the_pam_stream() {
    let dir = Scratch::new();
    let data = animation();
    let reference = run(&["--muxer", "pam", "-", "-"], &data);

    assert!(
        reference.status.success(),
        "{}",
        String::from_utf8_lossy(&reference.stderr)
    );
    let mut expected_manifest = None;

    for (name, stream) in [("whole", "997"), ("stream", "1")] {
        let path = dir.path(name);
        let output = run(
            &[
                "--muxer", "frames", "--stream", stream, "--loops", "2", "--repeat",
                "2", "-", &path,
            ],
            &data,
        );

        assert!(success(&output).is_empty());
        let manifest =
            fs::read_to_string(dir.0.join(name).join("manifest.json")).unwrap();

        assert!(manifest.contains("\"loop_count\":3,\"composited\":true"));
        assert!(manifest.contains("\"duration_ms\":0,"));
        assert!(manifest.contains("\"duration_ms\":1,"));
        assert!(manifest.contains("\"duration_ms\":16777215,"));
        assert!(manifest.ends_with("],\"frame_count\":3}\n"));
        assert!(!dir.0.join(name).join("manifest.json.part").exists());
        if let Some(expected) = &expected_manifest {
            assert_eq!(&manifest, expected);
        } else {
            expected_manifest = Some(manifest);
        }
        let mut pixels = Vec::new();

        for i in 0..3 {
            pixels.extend(
                fs::read(dir.0.join(name).join(format!("frame-{i:06}.pam"))).unwrap(),
            );
        }
        assert_eq!(pixels, reference.stdout);
        assert_eq!(fs::read_dir(dir.0.join(name)).unwrap().count(), 4);
    }
}

#[test]
fn sequence_failure_has_no_final_manifest_and_never_overwrites_a_directory() {
    let dir = Scratch::new();
    let path = dir.path("frames");
    let output = run(
        &["--muxer", "frames", "--max-output", "1", "-", &path],
        &animation(),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(!dir.0.join("frames/manifest.json").exists());
    let sentinel = dir.0.join("frames/owner-file");

    fs::write(&sentinel, b"preserve this").unwrap();
    let output = run(&["--muxer", "frames", "-", &path], STILL);

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(fs::read(sentinel).unwrap(), b"preserve this");
    let data = animation();
    let truncated = dir.path("truncated");
    let output = run(
        &["--muxer", "frames", "--stream", "1", "-", &truncated],
        &data[..data.len() - 2],
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(!dir.0.join("truncated/manifest.json").exists());
}

#[test]
fn sequence_byte_limit_covers_the_manifest_and_every_pam_file() {
    let dir = Scratch::new();
    let path = dir.path("reference");

    success(&run(&["--muxer", "frames", "-", &path], STILL));
    let size: u64 = fs::read_dir(&path)
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    let exact = dir.path("exact");
    let exact_limit = size.to_string();

    success(&run(
        &[
            "--muxer",
            "frames",
            "--max-output",
            &exact_limit,
            "-",
            &exact,
        ],
        STILL,
    ));
    let short = dir.path("short");
    let short_limit = (size - 1).to_string();
    let output = run(
        &[
            "--muxer",
            "frames",
            "--max-output",
            &short_limit,
            "-",
            &short,
        ],
        STILL,
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(!dir.0.join("short/manifest.json").exists());
    for args in [
        vec!["--muxer", "frames", "-", "-"],
        vec!["--muxer", "frames", "--info", "-"],
    ] {
        assert_eq!(run(&args, &[]).status.code(), Some(2));
    }
    for alias in ["-", "/dev/stdout", "/dev/fd/1", "/proc/self/fd/1"] {
        for args in [
            vec!["--info=json", "--muxer", "pam", "-", alias],
            vec!["--info=json", "--icc-out", alias, "-"],
            vec!["--muxer", "frames", "-", alias],
        ] {
            let output = run(&args, &[]);

            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
        }
    }
}

#[test]
fn sequence_subframes_and_scaling_describe_the_actual_output_geometry() {
    let dir = Scratch::new();

    for (name, options, geometry, composited) in [
        (
            "scaled",
            vec!["--scale", "4x2"],
            "\"width\":4,\"height\":2",
            true,
        ),
        (
            "subframes",
            vec!["--subframe"],
            "\"width\":2,\"height\":1",
            false,
        ),
    ] {
        let path = dir.path(name);
        let mut args = vec!["--muxer", "frames"];

        args.extend(options);
        args.extend(["-", &path]);
        success(&run(&args, &animation()));
        let manifest =
            fs::read_to_string(dir.0.join(name).join("manifest.json")).unwrap();

        assert!(manifest.contains("\"canvas_width\":2,\"canvas_height\":1"));
        assert!(manifest.contains(&format!("\"composited\":{composited}")));
        assert_eq!(manifest.matches(geometry).count(), 3);
    }
}
