#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["rich>=14.3.2"]
# ///
"""Compare wpd filters using upstream decbench's encoding and metric functions."""

import argparse
import importlib.util
import json
import shlex
import statistics
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("references", type=Path)
    parser.add_argument("--decbench", type=Path, required=True, help="path to dec.py")
    parser.add_argument("--wpd", type=Path, default=Path("build/wpd"))
    parser.add_argument("--previous", type=Path, help="previous fast-fancy binary")
    parser.add_argument(
        "--encoder",
        choices=["libwebp", "libwebp_default", "libwebp_sharpyuv"],
        default="libwebp",
    )
    parser.add_argument(
        "--output", type=Path, default=Path("wpd-test-data/chromametrics")
    )
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("decbench", args.decbench)
    dec = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(dec)
    args.output.mkdir(parents=True, exist_ok=True)
    references = sorted(args.references.glob("*.png"))
    if not references:
        parser.error("no reference PNGs found")

    decoders = {
        "wpd_fancy": (args.wpd, ""),
        "wpd_fast": (args.wpd, "--fast-fancy"),
        "wpd_lanczos5": (args.wpd, "--extra-fancy"),
    }
    if args.previous:
        decoders["wpd_previous"] = (args.previous, "--fast-fancy")
    for name, (binary, flags) in decoders.items():
        dec.DECODERS["webp"][name] = {
            "type": "png",
            "cmd": (
                f"{shlex.quote(str(binary.resolve()))} {flags} -f rgb "
                '"{encoded}" "{decoded}.ppm" && '
                'ffmpeg -v error -y -i "{decoded}.ppm" '
                '-frames:v 1 "{decoded}"'
            ),
        }
    names = ["dwebp", "ffmpeg_filtered", *decoders]
    metrics = list(dec.METRICS)
    images = []
    for reference in references:
        scores = dec.process_image(
            reference, args.encoder, names, metrics, "webp", args.output
        )
        if any(v is None for values in scores.values() for v in values.values()):
            raise RuntimeError(f"missing metric for {reference}")
        images.append({"image": reference.name, "scores": scores})
        # Checkpoint complete images without silently accepting partial results.
        (args.output / "images.json").write_text(json.dumps(images, indent=2) + "\n")

    samples = {
        d: {m: [r["scores"][d][m] for r in images] for m in metrics} for d in names
    }
    report = {
        "encoder": args.encoder,
        "images": images,
        "means": {
            d: {m: statistics.mean(values) for m, values in mm.items()}
            for d, mm in samples.items()
        },
    }
    (args.output / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    dec.print_summary(samples, metrics, len(images))


if __name__ == "__main__":
    main()
