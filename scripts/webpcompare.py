#!/usr/bin/env python3
"""Compare stills and composited animations with libwebp 1.6.0.

Emit one JSON record per input and a summary. RGB differences under zero alpha
are counted separately. Input files, including extensionless fuzz seeds, are
never changed. Decoded files live in a temporary directory under wpd-test-data.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def run(command, timeout):
    try:
        p = subprocess.run(command, stdout=subprocess.DEVNULL,
                           stderr=subprocess.PIPE, timeout=timeout)
        return p.returncode, p.stderr[-4096:].decode("utf-8", "replace")
    except subprocess.TimeoutExpired:
        return "timeout", "wall-clock limit exceeded"


def check_version(tool, pattern):
    p = subprocess.run([tool, "-version"], capture_output=True, text=True,
                       check=True, timeout=10)
    version = (p.stdout + p.stderr).strip()
    if not re.search(pattern, version):
        raise ValueError(f"{tool}: expected libwebp 1.6.0, got {version!r}")
    return version


def animated(data):
    return (len(data) >= 30 and data[:4] == b"RIFF" and
            data[8:16] == b"WEBPVP8X" and data[20] & 2 != 0)


def pams(path):
    """Read concatenated PAM frames and normalize RGB to straight RGBA."""
    frames = []
    with path.open("rb") as source:
        while True:
            magic = source.readline(4)
            if not magic:
                return frames
            if magic != b"P7\n":
                raise ValueError("invalid PAM magic")
            fields = {}
            for _ in range(32):
                line = source.readline(256)
                if line == b"ENDHDR\n":
                    break
                pair = line.split()
                if len(pair) == 2:
                    fields[pair[0]] = pair[1]
            else:
                raise ValueError("invalid PAM header")
            width, height, depth = [int(fields[key]) for key in
                                    (b"WIDTH", b"HEIGHT", b"DEPTH")]
            if width <= 0 or height <= 0 or depth not in (3, 4):
                raise ValueError("invalid PAM dimensions or depth")
            size = width * height * depth
            if size > 2 ** 30 or fields.get(b"MAXVAL") != b"255":
                raise ValueError("PAM exceeds comparison limit")
            pixels = source.read(size)
            if len(pixels) != size:
                raise ValueError("short PAM frame")
            if depth == 3:
                rgba = bytearray(width * height * 4)
                for channel in range(3):
                    rgba[channel::4] = pixels[channel::3]
                rgba[3::4] = b"\xff" * (width * height)
                pixels = bytes(rgba)
            frames.append((width, height, pixels))


def digest(frames):
    h = hashlib.sha256()
    for width, height, pixels in frames:
        h.update(width.to_bytes(4, "little"))
        h.update(height.to_bytes(4, "little"))
        h.update(pixels)
    return h.hexdigest()


def difference(wpd, reference):
    if [(w, h) for w, h, _ in wpd] != [(w, h) for w, h, _ in reference]:
        return "geometry", 0, 0
    visible = transparent = 0
    for (_, _, a), (_, _, b) in zip(wpd, reference):
        if a == b:
            continue
        for i in range(0, len(a), 4):
            if a[i:i + 4] == b[i:i + 4]:
                continue
            if a[i + 3] == b[i + 3] == 0:
                transparent += 1
            else:
                visible += 1
    return ("visible" if visible else "transparent" if transparent else "identical",
            visible, transparent)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--wpd", default="./build/wpd")
    parser.add_argument("--dwebp", default="dwebp")
    parser.add_argument("--anim-dump", default="anim_dump")
    parser.add_argument("--libwebp-compat", action="store_true")
    parser.add_argument("--noasm", action="store_true",
                        help="use portable C for dwebp; anim_dump remains native")
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--work-dir", type=Path, default=Path("wpd-test-data"))
    args = parser.parse_args()
    if not args.directory.is_dir() or args.timeout <= 0:
        parser.error("a directory and a positive timeout are required")
    args.wpd = os.path.abspath(args.wpd)
    args.work_dir.mkdir(parents=True, exist_ok=True)
    versions = {
        "dwebp": check_version(args.dwebp, r"^1\.6\.0$"),
        "anim_dump": check_version(args.anim_dump, r"Decoder version: 1\.6\.0\b"),
    }
    counts = {key: 0 for key in ("identical", "transparent", "visible", "geometry",
                                 "wpd_only", "libwebp_only", "neither", "abnormal")}
    files = sorted(p for p in args.directory.rglob("*") if p.is_file() and
                   ".git" not in p.relative_to(args.directory).parts)
    for path in files:
        data = path.read_bytes()
        record = {"path": str(path), "sha256": hashlib.sha256(data).hexdigest(),
                  "animation": animated(data)}
        with tempfile.TemporaryDirectory(prefix="webpcompare-", dir=args.work_dir) as td:
            work = Path(td)
            wpath = work / "wpd.pam"
            wcmd = [args.wpd, "--fmt", "rgba", "--muxer", "pam", "--threads", "1"]
            if args.libwebp_compat:
                wcmd.append("--libwebp-compat")
            wstatus, werror = run(wcmd + [str(path), str(wpath)], args.timeout)
            if record["animation"]:
                lcmd = [args.anim_dump, "-pam", "-folder", str(work), "-prefix", "ref"]
            else:
                lcmd = [args.dwebp, "-quiet", "-pam", "-o", str(work / "ref0.pam")]
            if args.noasm and not record["animation"]:
                lcmd.append("-noasm")
            lstatus, lerror = run(lcmd + [str(path)], args.timeout)
            record.update(wpd_status=wstatus, libwebp_status=lstatus)
            if (not isinstance(wstatus, int) or not isinstance(lstatus, int) or
                    wstatus < 0 or lstatus < 0):
                outcome = "abnormal"
            elif wstatus == lstatus == 0:
                try:
                    a = pams(wpath)
                    references = sorted(work.glob("ref*.pam"), key=lambda p:
                                        int(re.search(r"(\d+)\.pam$", p.name)[1]))
                    b = [frame for p in references for frame in pams(p)]
                    if not a or not b:
                        raise ValueError("success without decoded frames")
                    outcome, visible, transparent = difference(a, b)
                    record.update(wpd_pixel_sha256=digest(a),
                                  libwebp_pixel_sha256=digest(b),
                                  visible_pixels=visible, transparent_pixels=transparent,
                                  frames=len(a))
                except (ValueError, KeyError) as error:
                    outcome = "abnormal"
                    record["output_error"] = str(error)
            else:
                outcome = ("wpd_only" if wstatus == 0 else
                           "libwebp_only" if lstatus == 0 else "neither")
            if wstatus != 0:
                record["wpd_error"] = werror.strip()
            if lstatus != 0:
                record["libwebp_error"] = lerror.strip()
            record["outcome"] = outcome
            counts[outcome] += 1
            print(json.dumps(record), flush=True)
    print(json.dumps({"summary": counts, "files": len(files), "versions": versions,
                      "libwebp_compat": args.libwebp_compat, "noasm": args.noasm}))
    return int(not files or any(counts[key] for key in
                               ("visible", "geometry", "wpd_only", "libwebp_only", "abnormal")))


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error))
