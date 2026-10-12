#!/usr/bin/env python3
"""Compare chroma interpolation using decbench's libwebp Q90 methodology."""

import argparse
import json
import statistics
import subprocess
from pathlib import Path


def run(args, **kwargs):
    result = subprocess.run(
        [str(a) for a in args], check=True, capture_output=True, **kwargs
    )
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("references", type=Path, help="directory of reference PNGs")
    parser.add_argument("--wpd", default="build/wpd")
    parser.add_argument(
        "--output", type=Path, default=Path("wpd-test-data/chromaquality")
    )
    parser.add_argument("--sharp-yuv", action="store_true")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    results = []

    for reference in sorted(args.references.glob("*.png")):
        encoded = args.output / (reference.stem + ".webp")
        if args.sharp_yuv:
            run(
                [
                    "cwebp",
                    "-quiet",
                    "-m",
                    "6",
                    "-q",
                    "90",
                    "-sharp_yuv",
                    reference,
                    "-o",
                    encoded,
                ]
            )
        else:
            probe = json.loads(
                run(
                    [
                        "ffprobe",
                        "-v",
                        "error",
                        "-select_streams",
                        "v:0",
                        "-show_entries",
                        "stream=width,height",
                        "-of",
                        "json",
                        reference,
                    ]
                )
            )["streams"][0]
            raw = run(
                [
                    "ffmpeg",
                    "-v",
                    "error",
                    "-i",
                    reference,
                    "-pix_fmt",
                    "yuv420p",
                    "-f",
                    "rawvideo",
                    "-",
                ]
            )
            run(
                [
                    "cwebp",
                    "-quiet",
                    "-s",
                    probe["width"],
                    probe["height"],
                    "-m",
                    "6",
                    "-q",
                    "90",
                    "-o",
                    encoded,
                    "--",
                    "-",
                ],
                input=raw,
            )

        scores = {}
        for mode in ["fancy", "extra-fancy", "ffmpeg-filtered"]:
            decoded = args.output / (reference.stem + "-" + mode + ".ppm")
            if mode == "ffmpeg-filtered":
                run(
                    [
                        "ffmpeg",
                        "-v",
                        "error",
                        "-y",
                        "-i",
                        encoded,
                        "-vf",
                        "scale=flags=lanczos+accurate_rnd+full_chroma_int:param0=5,format=rgb24",
                        "-frames:v",
                        "1",
                        decoded,
                    ]
                )
            else:
                run(
                    [
                        args.wpd,
                        "-f",
                        "rgb",
                        *(["--extra-fancy"] if mode == "extra-fancy" else []),
                        encoded,
                        decoded,
                    ]
                )
            metric = subprocess.run(
                ["fssimu2", "--json", str(reference), str(decoded)],
                check=True,
                capture_output=True,
            )
            # fssimu2's human-readable scores use stderr; JSON uses stdout.
            score = json.loads(metric.stdout or metric.stderr)
            scores[mode] = score["score"]
        results.append({"image": reference.name, **scores})
        print(reference.name, scores, flush=True)

    if not results:
        parser.error("the reference directory contains no PNGs")
    report = {
        "images": results,
        "means": {m: statistics.mean(r[m] for r in results) for m in scores},
    }
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print("Means:", report["means"])


if __name__ == "__main__":
    main()
