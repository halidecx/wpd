#!/usr/bin/env python3
"""Compare WebP acceptance, frame geometry and RGBA pixels with libwebpdec.

Build the pinned reference with `meson compile -C build libwebpdec`.
Directories contribute their .webp files recursively; explicit files may be
extensionless fuzz artifacts. Outputs stay under wpd-test-data. Exit 1 means an
acceptance, geometry, visible-pixel or abnormal-exit difference; RGB beneath
fully transparent pixels is reported separately and does not fail the run.
"""

import argparse
import hashlib
import json
import math
import subprocess
import tempfile
from collections import Counter
from pathlib import Path


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for data in iter(lambda: source.read(65536), b""):
            result.update(data)
    return result.hexdigest()


def run(command, log, timeout):
    with log.open("wb") as stderr:
        try:
            status = subprocess.run(
                command, stdout=subprocess.DEVNULL, stderr=stderr, timeout=timeout
            ).returncode
        except subprocess.TimeoutExpired:
            return "timeout", "wall-clock limit exceeded"
    with log.open("rb") as stderr:
        stderr.seek(max(0, log.stat().st_size - 4096))
        return status, stderr.read().decode("utf-8", "replace").strip()


def header(source):
    magic = source.readline(4)
    if not magic:
        return None
    if magic != b"P7\n":
        raise ValueError("invalid PAM magic")
    fields = {}
    for _ in range(16):
        line = source.readline(256)
        if line == b"ENDHDR\n":
            break
        key, value = line.rstrip(b"\n").split(b" ", 1)
        if key in fields:
            raise ValueError("duplicate PAM field")
        fields[key] = value
    else:
        raise ValueError("invalid PAM header")
    if (
        fields[b"DEPTH"] != b"4"
        or fields[b"MAXVAL"] != b"255"
        or fields[b"TUPLTYPE"] != b"RGB_ALPHA"
    ):
        raise ValueError("expected straight-RGBA PAM")
    width, height = int(fields[b"WIDTH"]), int(fields[b"HEIGHT"])
    if not 0 < width <= 16384 or not 0 < height <= 16384:
        raise ValueError("invalid PAM dimensions")
    return width, height


def compare(a_path, b_path):
    frames = visible = transparent = 0
    with a_path.open("rb") as a, b_path.open("rb") as b:
        while True:
            a_size, b_size = header(a), header(b)
            if a_size != b_size:
                return "geometry", frames, visible, transparent
            if a_size is None:
                if not frames:
                    raise ValueError("success without frames")
                break
            frames += 1
            remaining = a_size[0] * a_size[1] * 4
            while remaining:
                size = min(remaining, 65536)
                aa, bb = a.read(size), b.read(size)
                if len(aa) != size or len(bb) != size:
                    raise ValueError("truncated PAM pixels")
                remaining -= size
                if aa == bb:
                    continue
                for i in range(0, size, 4):
                    if aa[i : i + 4] == bb[i : i + 4]:
                        continue
                    if aa[i + 3] == bb[i + 3] == 0:
                        transparent += 1
                    else:
                        visible += 1
    outcome = "visible" if visible else "transparent" if transparent else "identical"
    return outcome, frames, visible, transparent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", type=Path)
    parser.add_argument("--wpd", type=Path, default=Path("build/wpd"))
    parser.add_argument("--libwebp", type=Path, default=Path("build/libwebpdec"))
    parser.add_argument("--timeout", type=float, default=60)
    parser.add_argument("--work-dir", type=Path, default=Path("wpd-test-data"))
    args = parser.parse_args()
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("timeout must be positive")
    tools = [args.wpd.resolve(), args.libwebp.resolve()]
    if any(not tool.is_file() for tool in tools):
        parser.error("build both wpd and libwebpdec first")
    files = set()
    for path in args.inputs:
        if path.is_file():
            files.add(path.resolve())
        elif path.is_dir():
            files.update(p.resolve() for p in path.rglob("*.webp") if p.is_file())
        else:
            parser.error(f"input does not exist: {path}")
    if not files:
        parser.error("no input files")
    args.work_dir.mkdir(parents=True, exist_ok=True)
    counts = Counter()
    versions = None
    for path in sorted(files):
        record = {"path": str(path), "input_sha256": digest(path)}
        with tempfile.TemporaryDirectory(
            prefix="webpcompare-", dir=args.work_dir
        ) as td:
            work = Path(td)
            outputs = [work / "wpd.pam", work / "libwebp.pam"]
            results = [
                run(
                    [
                        str(tool),
                        "--fmt",
                        "rgba",
                        "--muxer",
                        "pam",
                        str(path),
                        str(output),
                    ],
                    work / f"{i}.log",
                    args.timeout,
                )
                for i, (tool, output) in enumerate(zip(tools, outputs))
            ]
            if versions is None:
                versions = [
                    error.splitlines()[0] if error else "" for _, error in results
                ]
            for name, (status, error) in zip(["wpd", "libwebp"], results):
                record[f"{name}_status"] = status
                if status:
                    record[f"{name}_error"] = error
            statuses = [status for status, _ in results]
            if any(status not in (0, 1) for status in statuses):
                outcome = "abnormal"
            elif statuses == [0, 0]:
                try:
                    outcome, frames, visible, transparent = compare(*outputs)
                    record.update(
                        frames=frames,
                        visible_pixels=visible,
                        transparent_rgb_pixels=transparent,
                        wpd_output_sha256=digest(outputs[0]),
                        libwebp_output_sha256=digest(outputs[1]),
                    )
                except (KeyError, ValueError) as error:
                    outcome = "abnormal"
                    record["output_error"] = str(error)
            else:
                outcome = (
                    "wpd_only"
                    if statuses[0] == 0
                    else "libwebp_only"
                    if statuses[1] == 0
                    else "neither"
                )
            record["outcome"] = outcome
            counts[outcome] += 1
            print(json.dumps(record), flush=True)
    print(
        json.dumps(
            {
                "summary": dict(counts),
                "files": len(files),
                "tools": [str(tool) for tool in tools],
                "versions": versions,
            }
        )
    )
    return int(
        any(
            counts[key]
            for key in ("visible", "geometry", "wpd_only", "libwebp_only", "abnormal")
        )
    )


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, subprocess.SubprocessError) as error:
        raise SystemExit(str(error))
