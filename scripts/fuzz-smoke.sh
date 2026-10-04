#!/bin/bash -eu

SECONDS_PER_TARGET="${1:-15}"
CORPUS="${2:-wpd-test-data}"

case "$SECONDS_PER_TARGET" in
    ''|*[!0-9]*|0*) echo "fuzz-smoke.sh: seconds must be positive" >&2; exit 1 ;;
esac

# Keep generated seeds separate from saved developer corpora and artifacts.
mkdir -p fuzz/corpus/ci/{container,e2e,vp8,vp8l} fuzz/artifacts
python3 - "$CORPUS" <<'PY'
from pathlib import Path
import sys

corpus = Path(sys.argv[1])
out = Path("fuzz/corpus/ci")
files = sorted(corpus.glob("*.webp"))
if not files:
    sys.exit(f"no WebP files found in {corpus}")

def chunks(data):
    offset = 0
    while offset + 8 <= len(data):
        tag = data[offset:offset + 4]
        size = int.from_bytes(data[offset + 4:offset + 8], "little")
        end = offset + 8 + size
        if end > len(data):
            break
        payload = data[offset + 8:end]
        if tag == b"ANMF":
            yield from chunks(payload[16:])
        else:
            yield tag, payload
        offset = end + (size & 1)

for path in files:
    data = path.read_bytes()
    # Large seeds make a smoke run spend its budget replaying images.
    if len(data) > 65536:
        continue
    for target in ("container", "e2e"):
        (out / target / path.stem).write_bytes(data)
    for index, (tag, payload) in enumerate(chunks(data[12:])):
        name = f"{path.stem}-{index}"
        if tag == b"VP8 ":
            (out / "vp8" / name).write_bytes(payload)
        elif tag == b"VP8L":
            # The target uses two leading bytes to select ARGB and threading.
            for threads in (0, 1):
                (out / "vp8l" / f"{name}-{threads}").write_bytes(
                    bytes((0, threads)) + payload)

# Exercise the damaged transform regressions through raw and driver targets.
for path in sorted(Path("tests/data/vp8-compat").glob("*.vp8")):
    payload = path.read_bytes()
    name = f"compat-{path.stem}"
    (out / "vp8" / name).write_bytes(payload)
    chunk = b"VP8 " + len(payload).to_bytes(4, "little") + payload
    chunk += bytes(len(payload) & 1)
    data = b"RIFF" + (len(chunk) + 4).to_bytes(4, "little") + b"WEBP" + chunk
    for target in ("container", "e2e"):
        (out / target / name).write_bytes(data)

for target in ("container", "e2e", "vp8", "vp8l"):
    if not any((out / target).iterdir()):
        sys.exit(f"no seeds prepared for {target}")
PY

# With no target argument cargo-fuzz builds every declared target.
cargo +nightly fuzz build -O --debug-assertions
for target in container vp8 vp8l e2e; do
    cargo +nightly fuzz run -O --debug-assertions "$target" \
        "fuzz/corpus/ci/$target" -- -max_total_time="$SECONDS_PER_TARGET" \
        -max_len=65536 -timeout=5 -rss_limit_mb=1024 -seed=1
done
