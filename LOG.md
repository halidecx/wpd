# wpd Rust rewrite — log

A running record of decisions, milestones, measurements and techniques for
porting wpd from C to Rust. Newest entries go at the bottom of each section.

## Goals

1. **Memory safety in the core.** `crates/wpd` compiles under
   `#![forbid(unsafe_code)]` when built without assembly, so a consumer can
   produce a decoder that is provably free of memory-unsafety by construction.
   With assembly on, `unsafe` exists in exactly one module (`wpd::asm`) and in
   the C ABI shim crate.
2. **No perf regression.** The hot paths are assembly, so the safe scalar
   fallbacks do not have to beat C — assembly closes the gap. Where a safe
   formulation costs measurable speed on a path assembly does not cover, that is
   recorded here along with what was done about it.
3. **No feature or correctness regression.** The public C ABI in `include/wpd.h`
   stays byte-for-byte compatible, and every existing test keeps passing.
4. **Runtime asm dispatch preserved**, including the `wpd_set_cpu_flags_mask`
   override checkasm depends on.
5. **A safe idiomatic Rust API** alongside the C ABI.

## Decisions

| Decision                                                           | Rationale                                                                                                                                                                             |
| ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Cargo + `build.rs` drives the library; Meson keeps the C harnesses | rav1d-proven. Gives clippy, miri, `cargo fuzz` and a normal Rust dependency story. Meson still owns `tools/wpd.c`, `tests/api.c`, `tests/parity.c`, checkasm and the testdata matrix. |
| Two crates: `wpd` (core rlib) and `wpd-capi` (staticlib)           | The "provably safe" claim needs the raw-pointer C ABI out of the core. A Rust consumer taking the `wpd` rlib with `--no-default-features` gets a decoder with zero `unsafe` anywhere. |
| Zero `unsafe` in the core; assembly makes up any perf gap          | The security argument is the point of the rewrite. Scalar fallbacks that lose a few percent to C are acceptable because assembly covers the hot paths on every supported target.      |
| `build.rs` for the assembly lives in `wpd`, not `wpd-capi`         | Otherwise a pure-Rust consumer of the `wpd` rlib with `asm` enabled would fail to link.                                                                                               |
| Split `wpd_decoder.c` into C modules before porting                | Porting a 5332-line file wholesale is where this kind of project goes sideways. Six ~900-line modules with the parity suite green between each is a much more forgiving shape.        |
| Two-tier DSP tables (see below)                                    | Lets the core stay safe while `tests/checkasm/*.c` compiles untouched and still benchmarks the raw assembly symbols.                                                                  |

## Two-tier DSP tables

The design that reconciles "safe core" with "honest checkasm".

**Tier A — safe table** (`crates/wpd/src/dsp/`). Fields are safe Rust `fn`
pointers over slices — though not always a slice per C pointer: see "Aliasing at
the DSP boundary" below for why `pred_add` takes one buffer plus offsets rather
than separate input and output slices. Fallbacks are ordinary safe functions.
Assembly entries are safe `fn` items whose body is a single `unsafe` block in
`wpd::asm`, guarded by length assertions covering the reads the assembly makes
past the nominal row (`upper[-1]`, `upper[num_pixels]`). The decoder only ever
sees Tier A, so it contains no `unsafe` at all.

**Tier B — C ABI table** (`crates/wpd-capi`, `#[cfg(feature = "checkasm")]`).
`#[repr(C)]`, field-for-field identical to `src/vp8l_dsp.h`, `src/vp8dsp.h` and
`src/yuvdsp.h`, so the checkasm tests compile unchanged. Assembly entries are
the **raw assembly symbols** — no wrapper, so `--bench` numbers stay honest.
Fallback entries are thin `extern "C"` trampolines around Tier A; that cost
lands on the reference side, where it affects neither correctness nor the
assembly benchmark.

Both tiers are emitted from one `dsp_table!` macro, so the CPU-flag dispatch
conditions have a single source of truth.

## Safe-and-fast playbook

Techniques for keeping the core free of `unsafe` without paying for it. Grown as
the port proceeds.

- **Pick the element type from what the code actually touches.** The lossless
  decoder never looks at less than a whole pixel, so its pictures are
  `Vec<u32>`, not byte planes plus a `linesize`. That deletes the `4 * x`
  arithmetic and every bounds check that came with it, and it costs nothing at
  the C ABI: a `u32` in native byte order is the same memory the assembly
  already reads.
- **Flat buffers addressed by offset, never per-row slices.** A plane is one
  allocation (`Vec<u8>`) plus stride, width, height and the existing 64-byte
  trailing padding. Because rows are contiguous, the deliberate out-of-row
  accesses in the C — `out[-1]` and `upper[-1]` in `pred_add_1`, and the
  `upper[width] = row[0]` write in `predictor_transform_rows` — become ordinary
  in-bounds indices into the backing buffer. No `unsafe`, no behaviour change.
- **Caller-owned memory never enters the core.** `wpd_decoder_update`,
  `wpd_decoder_open_borrowed` and `wpd_decoder_set_output_buffer` hand over
  pointers whose lifetime the C ABI cannot express. `wpd-capi` stores
  `(ptr, len, stride)` and rebuilds the slice per call inside `unsafe`; the core
  takes the slice as an argument and stores nothing borrowed.
- **Reslice before a masked index.** `let t = &table[..=mask]; t[idx & mask]`
  lets LLVM prove the bound away. Used for the Huffman root/secondary tables.
- **Fixed-width loads.** `u64::from_be_bytes(s[i..i + 8].try_into().unwrap())`
  compiles to a single load; this is how the range-coder refill and the bit
  reader's prefetch avoid both `unsafe` and a byte loop.
- **`chunks_exact(4)` / `chunks_exact_mut(4)`** for pixel loops,
  `copy_from_slice` for memcpy, `split_at_mut` for provably disjoint copies.
- **`vec![0u8; n]`** for the `wpd_mallocz` equivalent: it routes to
  `alloc_zeroed`, i.e. calloc, which earlier benchmarking already found beats
  `aligned_alloc` + `memset` here.
- **Grow, do not grow-then-clear.** `try_reserve` + `resize` + `fill` memsets a
  buffer twice. Clearing it first so `resize` covers the whole thing halves
  that; there is no safe way to get `calloc`'s free zeroing back with a fallible
  allocation, so the one remaining memset is the standing price.
- **Clear between frames without dropping.** An animation reruns the whole
  per-image setup per frame. Transcribing `..._free()` as
  `*self = Default::default()` hands every buffer back so the next frame can ask
  for the same sizes again; clearing the vectors and keeping the capacity is
  what the C's arena reuse amounted to.
- **`const N: usize` where the C had an always-inline literal.** A copy whose
  length is a value goes to `memcpy`; the same copy under a const parameter is
  an inline fixed-width move. Dispatch it with the same `match` the C wrote as a
  `switch`.
- **Index a lookup table by a whole byte.** `[[u32; N]; 256]` indexed by a `u8`
  is provably in bounds; the same table flattened to `[u32; 256 * N]` and
  indexed by `i * N` is not, and costs a check per lookup.
- **Intrinsic replacements.** `__builtin_clz` → `leading_zeros()`,
  `__builtin_bswap64` → `swap_bytes()`. Prefer `31 - x.leading_zeros()` to
  `x.ilog2()`, which carries a panic path for zero that the caller has usually
  already ruled out but LLVM may not see.
- **A row is the unit of a pixel kernel, not a sample.** When a divide, a
  reciprocal or a table lookup is shared across the channels of a pixel, a
  per-sample helper repeats it per channel. Compute it once per pixel and apply
  it, and keep loop bounds that are known at the call site — a 2x2 block, a
  channel count — as const generics, so they unroll the way the C they replace
  did. See Phase 5's chroma blend.

**In a parser, read with saturating helpers rather than proven bounds.** A
bounds check that fires on damaged input is a panic, and a panic on damaged
input is a denial of service the C did not have. Header and chunk walks are
O(chunks), so byte accessors that return zero past the end and a `window()` that
clips cost nothing measurable and leave no residual argument to get wrong. Keep
proven-bound indexing for the pixel loops, where it pays.

- **Check the disassembly, not the theory.** Hot functions are checked for
  surviving `core::panicking` references; a bounds check that LLVM did not
  remove shows up there.

## Perf risk register

Spots where a safe formulation might cost speed and assembly does not cover.
Each gets a measurement at the phase that ports it.

| # | Risk                                                         | Status  |
| - | ------------------------------------------------------------ | ------- |
| 1 | VP8L entropy pixel loop — bounds checks on `base[4 * pos..]` | clear   |
| 2 | `copy_block32` overlapping LZ77 copy                         | clear   |
| 3 | `huff_read_symbol` table indexing                            | clear   |
| 4 | VP56 range coder refill near the buffer end                  | clear   |
| 5 | yuvdsp row glue under `--no-default-features`                | not yet |
| 6 | `Vec` zeroing vs `calloc` on large canvases                  | costs   |
| 7 | Cross-language calls where the C inlined `vp56rac`/bitreader | avoided |
| 8 | DSP wrapper call per kernel invocation, Tier A vs raw symbol | clear   |

## Milestones

### Phase 0a — baseline established (commit `d241ef8`)

The pre-port C build is the reference for everything that follows.

- `meson test -C build` with `-Dtestdata_tests=true`: **187/187 pass**.
- `./build/checkasm`: **151/151 pass** (SSE, SSE2, SSSE3, SSE4.1, AVX2 on this
  host).
- Baseline binary preserved at `build-baseline/wpd`, statically linked against
  `libwpd` so it stays valid as the tree changes. `build-baseline/COMMIT`
  records the commit it came from.

Baseline decode timings on this host (`--repeat 48`, hyperfine, 12 runs, release
build, x86-64 with AVX2):

| File                  | Mean           |
| --------------------- | -------------- |
| `lossless.webp`       | 160.9 ms ± 2.2 |
| `lossy.webp`          | 169.2 ms ± 2.2 |
| `anim_yuva.webp`      | 341.3 ms ± 2.3 |
| `simplelf-lossy.webp` | 160.8 ms ± 2.3 |

Note on A/B methodology: checkasm benchmark comparisons must be made within a
single build directory — comparing binaries linked in different build dirs has
previously produced a spurious ~5% swing. End-to-end `cmpbench.sh` comparisons
use the preserved statically-linked baseline binary, which does not have that
problem.

### Phase 0b — `wpd_decoder.c` split into modules

Pure code motion in C, so that the port can proceed a module at a time rather
than swapping a 5332-line translation unit in one commit. Separable translation
units are load-bearing for the mixed-language build: with the file monolithic
there is no way to port "just the Huffman part".

| Module          | Lines | Contents                                                     |
| --------------- | ----- | ------------------------------------------------------------ |
| `wpd_decoder.c` | 1248  | public API, streaming input buffer                           |
| `vp8l.c`        | 1059  | lossless entropy decode, the four transforms, still driver   |
| `convert.c`     | 612   | format helpers, crop/scale/flip, conversion, region blending |
| `export.c`      | 477   | frame export, caller-owned output buffers                    |
| `container.c`   | 413   | RIFF scan, frame table, `wpd_get_info`                       |
| `anim.c`        | 365   | compositor, canvas, `decode_anmf`                            |
| `huffman.c`     | 367   | prefix-code table build, code-length list readers            |
| `lossy.c`       | 236   | lossy still/alpha driver                                     |
| `image.c`       | 102   | `WebPImage` allocation                                       |

Headers: `wpd_internal.h` (shared helpers), `bitreader.h` (header-only),
`huffman.h`, `image.h`, `container.h`, `vp8l.h`, `wpd_dec.h` (the decoder
struct), plus a header per module for the symbols that cross a boundary.

Two things stayed inline rather than moving into a `.c`: the whole bit reader,
because `br_bits()` sits in the lossless pixel loop and its refill machinery has
to be visible at the call site, and `huff_read_symbol()` for the same reason.
`read_huffman_code_simple`/`_normal` took a `WPDDecoder` only to reach `s->gb`,
so they now take the `LEBitReader` directly and moved with the rest of the
Huffman code.

Verification against the preserved baseline: 187/187 meson tests, checkasm
151/151, `md5check.sh` bit-identical, `animcheck.sh` clean over 42 files × 8
packed formats and 27 animations, `rac32.sh` 187/187, no new compiler warnings.

Timings, same methodology as Phase 0a:

| File                  | Baseline | After split | Delta |
| --------------------- | -------- | ----------- | ----- |
| `lossless.webp`       | 160.6 ms | 160.4 ms    | -0.1% |
| `lossy.webp`          | 169.5 ms | 170.0 ms    | +0.3% |
| `anim_yuva.webp`      | 341.5 ms | 342.9 ms    | +0.4% |
| `simplelf-lossy.webp` | 159.5 ms | 160.2 ms    | +0.4% |

All within run-to-run noise: the extra translation units cost nothing, so no
interim LTO is needed to keep the port's own measurements honest.

### Phase 0c — cargo drives the library, meson keeps the harnesses

The topology is established before any code moves, so its shape is proven while
every line is still C.

- `crates/wpd` — core rlib. Its build script assembles the x86 nasm and the
  aarch64/arm `.S` files, reproducing meson's flags and its `.arch_extension`
  dotprod/i8mm and armv6/armv6t2 probes. The assembly lives here rather than in
  `wpd-capi` so that a pure-Rust consumer of the rlib still links.
- `crates/wpd-capi` — the staticlib C consumers link. Its build script compiles
  whatever C the port has not reached, listed in `C_SOURCES`; entries drop out
  as modules move, and when the list empties the script and its `cc`
  build-dependency go away. Probe results cross from `wpd` via the `links` key,
  so the C and the assembly agree about the target's extensions.
- `meson.build` compiles nothing for the library any more. It runs cargo and
  links `tools/wpd.c`, the api/parity/fuzz/rowbounds tests, checkasm and the
  testdata matrix against the result.

Three things needed care, each of which would have been a silent regression:

1. **checkasm needs `trim_dsp` off.** `trim_dsp` lets the compiler drop every
   fallback the build target cannot reach — which is exactly what checkasm
   compares the assembly against. Linking checkasm against the ordinary release
   library took it from 151 tests to 97 without failing anything. It now gets
   its own cargo configuration, in its own target dir so the two do not
   invalidate each other's incremental state.
2. **A Rust `cdylib` exports only Rust symbols**, which is nothing while the
   entry points are still C — the shared library came out with zero exports. It
   is now linked from the archive with a version script generated from
   `include/wpd.h`, so it exports exactly the 28 `WPD_API` symbols the old build
   did (verified identical) and keeps working unchanged once the entry points
   become `#[no_mangle]` Rust.
3. **Sanitizer flags have to reach the C**, which cargo now compiles. Meson's
   `b_sanitize` is handed across through `CFLAGS`, which the `cc` crate honours;
   confirmed by 209 `__asan_report`/`__ubsan_handle` references in the resulting
   archive. `buildtype=debugoptimized` maps to a new `debugopt` cargo profile
   rather than to `debug`, so sanitizer and fuzz runs stay optimised instead of
   dropping to `-O0`.

One trap worth remembering: `.gitignore` had `build*`, which silently ignored
both `build.rs` files. A clean-clone build is now part of the check.

Verification: 187/187 meson tests, checkasm 151/151, `md5check.sh` bit-identical
against the preserved baseline, `animcheck.sh` clean, `rac32.sh` 187/187,
sanitizers clean for both asm and no-asm builds, no-asm build 186/186, and a
fresh `git clone` configures, builds and passes checkasm. Decode timings 0.0% to
+0.7% against baseline, i.e. noise.

`scripts/stylecheck.sh` now also runs `cargo fmt --all`; `rustfmt.toml` sets the
width to 88.

### Phase 1 — leaf modules

**`cpu` — done.** `src/cpu.c`, `src/x86/cpu.c` and `src/arm/cpu.c` are replaced
by `crates/wpd/src/cpu.rs`, which contains no `unsafe`.

Detection goes through `std`'s `is_x86_feature_detected!` and
`is_aarch64_feature_detected!`. They are safe, and they already do correctly the
parts the C had to spell out: the OSXSAVE and XCR0 checks before believing AVX2,
`getauxval` on Linux, `sysctl` on Apple. On 32-bit arm, where `std` has no
stable detection, `/proc/self/auxv` is parsed instead — the same table
`getauxval` reads, but as a file, so no libc call is needed and the module stays
safe.

The two atomics the C exported are now private to the Rust module; `src/cpu.h`
calls `wpd_get_cpu_flags_raw()` rather than loading them. The `trim_dsp` union
deliberately stays on the C side of that boundary: it has to remain a
compile-time constant at the DSP init call sites or trimming stops trimming.

checkasm's test count is the real check for this module — a detection bug shows
up as CPU tiers silently not being tested, not as a failure. It stayed at 151.
`--cpumask 0` still decodes identically to the default, so the mask reaches
dispatch. Timings -1.0% / +0.9%, noise.

**`vp8l_dsp` — done.** Scalar kernels in `crates/wpd/src/dsp/vp8l.rs`, C ABI
table in `crates/wpd-capi/src/dsp/vp8l.rs`. First use of the two-tier design:
assembly entries are the raw symbols, fallbacks are trampolines that rebuild
slices from the caller's pointers.

**`vp8dsp` — done.** `crates/wpd/src/dsp/vp8.rs` and
`crates/wpd-capi/src/dsp/vp8.rs`. Each instruction set gets a module of
identically named functions bound to its own symbols with `#[link_name]`, so the
`VP8_*_LOOP_FILTER*_MB` compositions are written once instead of once per
variant as `src/vp8dsp.h` did.

**`vp8pred` — done.** `crates/wpd/src/dsp/vp8pred.rs` and its C ABI shim.
checkasm caught a transcription error in `HOR_DOWN_PRED` — one of sixteen taps
averaged the corner with the sample above instead of the one to its left — which
is exactly the failure mode a hand port has and the reason every DSP entry needs
a checkasm test.

**`rescaler` — done.** `crates/wpd/src/rescale.rs` and
`crates/wpd-capi/src/rescale.rs`. No assembly and no checkasm coverage;
`tests/parity.c` is the gate, since the CLI has no scale option and the testdata
matrix never reaches this code. The struct keeps its C layout because
`src/convert.c` still drives it through the inline helpers in `src/rescaler.h`.

**`yuvdsp` — done.** `crates/wpd/src/dsp/yuv.rs` and
`crates/wpd-capi/src/dsp/yuv.rs`. The seam went where "The yuvdsp seam" below
said it would: the table entries and `upsample_row` became Rust, and the four
`wpd_yuv420_to_packed*` drivers came with them rather than staying in C.

The packed layout is a const generic rather than a runtime argument, which is
what the C got from expanding `YUV_TO_OUT` and `UPSAMPLE_PAIRS` once per layout.
Two details are worth keeping:

- The three-byte layouts have no alpha byte, and rather than branch, their
  channel table aliases alpha onto red. The `0xff` store lands first and the red
  store overwrites it, so the result is what `YUV_TO_OUT3` produces and the dead
  store folds away.
- `upsample_pairs` takes the index of the first output pixel it touches instead
  of having the caller bias the pointers. The C block entry point passes
  `top_y - 1` and `top_dst - bpp`; reproducing that literally would mean
  building a slice that starts before the row, and checkasm hands that entry
  point the very first byte of a stack buffer, so it is a real out-of-bounds
  pointer and not a theoretical one.

The gamma tables keep their C names through
`#[cfg_attr(feature = "asm",
export_name = ...)]`, because the assembly gathers
from them directly. They are plain `pub static`s otherwise: `#[no_mangle]` and
`#[export_name]` both trip the `unsafe_code` lint, so an unconditional attribute
would break the `forbid(unsafe_code)` build.

| Module     | with asm       | `-Denable_asm=false` |
| ---------- | -------------- | -------------------- |
| `vp8l_dsp` | 166.7 vs 166.6 | 200.1 vs 198.9       |
| `vp8dsp`   | 210.4 vs 211.2 | 441.9 vs 407.1       |
| `vp8pred`  | 210.9 vs 211.2 | 442.6 vs 406.1       |
| `yuvdsp`   | 210.2 vs 212.6 | 270.8 vs 295.2       |

Lossless figures are 50 iterations of `lossless.webp`, lossy ones 60 of
`lossy.webp`; the `yuvdsp` row is 40 iterations, so its two columns are not
comparable with the rows above, only with each other. The shipping configuration
is neutral throughout. The no-asm build is 8.6% slower on lossy content and
unchanged on lossless; that cost is the bounds checks in the loop filter, and it
is the trade the unsafe-budget decision explicitly accepted.

The upsampler's own row drivers are Rust in _both_ configurations, since they
are never assembly. `anim_yuv.webp`, which is mostly upsampling, came out 3%
faster with assembly enabled and 2% slower without it.

**Phase 1 is complete.** The order it actually ran in was `cpu` → `vp8l_dsp` →
`vp8dsp` → `vp8pred` → `rescaler` → `yuvdsp`.

### Phase 1 addendum — `tools/` in Rust

`tools/wpd.c`, `tools/md5.c` and the `tools/compat/` getopt shim are replaced by
`crates/wpd-tool`. The binary links the same archive the library target stages,
so it still reaches the decoder through the C ABI exactly as an outside consumer
would; the bindings in `src/sys.rs` are hand-written for that reason rather than
generated.

The constraint here is that this binary _is_ the test harness —
`scripts/md5check.sh`, `scripts/testdata.sh` and `scripts/animcheck.sh` all
drive it — so its behaviour had to stay byte for byte identical. What was
checked against the C tool: `--help`, `--info` (still and animated, streaming
and whole-file), the exit codes for each malformed-argument path, and every
muxer crossed with every pixel format on seven test files.

That last check earned its keep. `WPDFrameInfo` declares `pos_x, pos_y` before
`width, height`, the opposite of `WPDFrame`; transcribing it in the other order
compiled, linked and ran, and only showed up as `--info` printing
`0x0 at
400,400`. A hand-written binding to a C ABI needs a differential test
against something that already agrees with the header.

It is now a script rather than a session's worth of ad-hoc comparisons:
`scripts/clicheck.sh OLD NEW` runs 794 invocations through both binaries and
diffs the exit status, stdout, stderr and the bytes of any file written. The
banner names the revision and usage echoes `argv[0]`, so both are folded to a
fixed string before comparing; nothing else is normalised.

Running it caught two more differences that no pixel comparison could see.
`getopt_long` reports a long option whose value is missing as an unknown option,
not as a bad value, so `--fmt` at the end of the argument list has to say
`unknown option or missing option value` — the port was reporting
`invalid output pixel format`. And Rust's `io::Error` renders as
`No such file or directory (os error 2)` where `strerror` gave just the message,
so the tool now strips the suffix.

One apparent failure was a real difference between the two _builds_ rather than
the two binaries: `build` had `trim_dsp=false` and the baseline `if-release`, so
only the baseline warned that `--cpumask` could not go below the compile-time
target.

**And then fixing that broke the baseline.** Reconfiguring the baseline's build
directory and recompiling it staged a binary built with `enable_asm=false`,
because that directory had been set up as the no-asm C reference. Nothing
noticed: assembly and fallback produce identical output, so `md5check.sh`,
`clicheck.sh` and the whole testdata matrix stayed green. Only the benchmarks
changed, and they changed by 1.9x, which is exactly the kind of number that
should have been disbelieved on sight rather than explained.

The preserved baseline binary is the reference for every measurement in this
file. Before trusting a number against it, check that it still has assembly in
it — `nm build-baseline/wpd | grep ff_vp8_idct_add_sse2` — and never rebuild it
from a directory whose options have not been read.

### Phase 2 — `vp8.c` and `vp56rac` in Rust

1720 lines of lossy frame decoder plus 350 of range coder leave the tree. What
replaces them is `crates/wpd/src/vp8/`, which contains no `unsafe`.

**Making the state opaque came first.** `struct WPDDecoder` embedded
`VP8Context` by value, and only two lines in the tree named it. Replacing it
with a pointer was a prerequisite for the port — otherwise Rust would have had
to mirror `WPDDecoder`'s layout as well. It is not a speed-up: measured against
`8248221`, the commit before it, lossy and lossless are both unchanged. An
earlier draft of this entry claimed 1.9x for it, which was the clobbered
baseline described above, not the change.

**Two things are expressed differently from the C.**

Each plane is one flat `Vec<u8>` addressed by offset rather than an interior
pointer. The decoder deliberately reads and writes outside the visible frame:
the left border column at `dst[-1]`, the row above the first macroblock, the
four samples above and to the right of a subblock. Against a flat allocation
those are ordinary indices. This is playbook item 1, and it is what lets the
entire macroblock loop be safe code rather than a wall of `unsafe`.

The range coders hold offsets instead of pointers, which deletes
`wpd_vp56_save_offsets` and `wpd_vp56_restore_offsets` outright. The C had to
re-point three raw pointers into the chunk after every streaming append that
reallocated it; an offset cannot be invalidated that way. The chunk arrives as a
`&[u8]` argument per call, so a coder that outlives the buffer it was reading is
not expressible.

**The DSP tables grew their safe tier.** A Tier A entry takes the plane and the
offset of the position it acts on; the wrapper in `wpd::asm` turns those two
numbers into the pointer the assembly wants, after checking the plane reaches
that far. Because a table field is a plain `fn` pointer with nowhere to keep a
symbol, each assembly entry point is a marker type carrying the symbol as an
associated constant, and one generic wrapper body monomorphises into a distinct
function per instruction set. That also let every symbol move to a single
declaration site: `wpd-capi` now builds its C ABI table from the same items, so
there is no second list of `#[link_name]`s to drift, and `checkasm --bench`
still sees bare symbols.

### The two chroma planes cannot share one offset

The C passed `dst[1]` and `dst[2]` to the chroma loop filters as two pointers.
The obvious Rust translation gives the entry the two plane slices and one shared
offset, since U and V have identical geometry.

They do not. Each plane is its own allocation, and the padding each needs to put
its rows on a 64-byte boundary depends on where the allocator happened to put
it, so their origins differ. Filtering V at U's offset is wrong by whatever that
difference is.

It survived every gate that only looks at whole-frame output for content whose
dimensions are a multiple of 16, and `md5check.sh` caught it on the five inputs
that are not — sixteen bytes differed in the V plane, in adjacent pairs at
multiples of four, which is the signature of a subblock edge filtered in the
wrong place. The rule to carry forward: an offset belongs to an allocation, and
two allocations do not share one, however alike their shapes.

### Closing the 3% the safe version started with

The port arrived 2-3% slower and three changes closed it. Each was found by
profile, not by guessing, and the first guess was wrong every time.

The innermost coefficient loop passed the eleven-byte probability array **by
value** where the C kept a pointer, copying it per token. Taking a reference
(with a lifetime tying it to `probs`) recovered lossy on its own.

`decode_mb_coeffs`, `xchg_mb_border`, `intra_predict`, `idct_mb` and the two
filter helpers are `wpd_always_inline` in the C and were not being inlined here.
Marking them `#[inline(always)]` made the profile match the C's shape exactly —
`decode_rows_tmpl` plus `decode_coeffs_inner` came to 48.1% against the C's
47.9% — but did not move the wall clock, which was the clue that the remaining
cost was instruction count rather than call overhead. Pinned to one core: 1400M
instructions in 469M cycles for the C, 1586M in 480M for Rust. 13% more
instructions absorbed by better IPC into 2.3% more cycles.

Those instructions were bounds checks. `block[ZIGZAG_SCAN[i] as usize]` indexes
a sixteen-entry array with a table value the compiler cannot bound, so `& 15` —
a no-op on every entry the table holds — removes the check. And
`decode_mb_coeffs` reached through `self.top_nnz[mb_x]` thirteen times; taking a
copy of the row, working on it and writing it back at the end is what the C did
by holding a pointer to it, and costs nothing.

Do not measure on a hybrid CPU without `taskset`. An unpinned three-way
comparison put one binary on an efficiency core and reported a 19% difference
between two builds whose code for that input is byte-identical.

**Results**, pinned to one core, 15 runs, `--repeat 60`, against the preserved C
baseline `d241ef8` and against `8248221`, the last commit before this phase:

| file           | vs C baseline | vs pre-phase-2 |
| -------------- | ------------- | -------------- |
| lossy          | 1.00x         | 1.01x          |
| a_lossy        | 0.98x         | 0.99x          |
| odd_lossy      | 1.04x         | 1.00x          |
| simplelf-lossy | 1.00x         | 1.01x          |
| anim_yuv       | 1.04x         | 1.01x          |
| anim_yuva      | 1.01x         | 1.02x          |
| lossless       | 0.99x         | 1.00x          |

Parity, which is the goal. Nothing here is a speed-up and nothing is a
regression beyond the ±4% this host resolves.

The safe fallbacks are the part that does cost something. With `--cpumask none`
on both binaries, so each runs its own scalar code:

| file    | C scalar | Rust scalar |            |
| ------- | -------- | ----------- | ---------- |
| lossy   | 412.8 ms | 463.8 ms    | 12% slower |
| a_lossy | 102.4 ms | 117.3 ms    | 15% slower |

That is the cost of the bounds checks the assembly path does not pay, and it is
the trade the unsafe-budget decision accepted: a consumer who wants the
provably-safe build gives up about an eighth of the lossy decode rate.

Gates: `checkasm` 151/151, `meson test` 186/186, `clicheck.sh` 794/794,
`testdata.sh` across five configurations, `animcheck.sh`, `rac32.sh` 186/186
(which is what exercises the 32-bit range coder — its own module, not a
compile-time variant of the 64-bit one), `md5check.sh`, `sanitize.sh` 186 and
185, `fuzz.sh` 300 trials per file.

### Phase 3 — VP8L, the whole lossless decoder

`src/vp8l.c`, `src/huffman.c`, `src/bitreader.h` and `src/huffman.h` are gone.
`crates/wpd/src/vp8l/` replaces them: the bit reader, the prefix codes, the
pixel loop and the four transforms. `crates/wpd-capi/src/vp8l.rs` implements
`src/vp8l.h` unchanged, so the container, the animation compositor and the lossy
decoder's alpha path go on calling the same sixteen functions.

**A picture is a `Vec<u32>`, not a byte plane.** The C addressed every image
through `data[0]` and a `linesize`, and every pixel access was `4 * x` bytes off
a row pointer. Nothing in the lossless decoder ever looks at less than a whole
pixel: the entropy loop walks the picture linearly, the predictors already took
`uint32_t *`, and the transforms read and write whole `[A, R, G, B]` groups. So
the picture became one `u32` per pixel, in native byte order over the same
memory the assembly and the C ABI still see. That is what closed risk 1 — there
is no `base[4 * pos..]` left to bounds-check, only `pixels[pos]` against a
length the loop condition already tests.

**Risks closed.**

- **1, the pixel loop.** Closed by the `u32` picture. The loop is 28.2% of
  `durations` against the C's 27.0%, normalised — parity.
- **2, `copy_block32`.** Three cases, all disjoint slices, no overlap to reason
  about: `dist >= length` is one `copy_from_slice` after a `split_at_mut`;
  `dist == 1` is a `fill`; anything else advances in `dist`-sized steps, each
  reading only what earlier steps finished writing. Under 2% of any profile.
- **3, `huff_read_symbol`.** The root table is resliced to exactly `mask + 1`
  entries when the reader is resolved, so `prefetch() & (len - 1)` is in bounds
  by construction. The five trees of a meta-block are resolved once, when the
  block changes, rather than per symbol.
- **6, `Vec` zeroing vs `calloc`.** Measured, and it does cost. See below.

**What the profile charged for, and what fixed it.** Every one of these was
found by `perf`, and the first three were worth 1.4x, 1.45x and 2x on the
functions they touched.

`expand_palette_rows` — the packed-palette expansion — was 2.9x slower than the
C on `palette4bpp_rgb`. The C specialises the group size through a `switch` over
an `always_inline` helper, so its copy is a fixed 8, 16 or 32 bytes; the Rust
passed the group size as a value and every group went through `memcpy`. A
`const PPB` parameter with the same three-way dispatch, an expansion table built
per group rather than per index and indexed by a whole byte so the lookup needs
no check, and the table sized `[[u32; PPB]; 256]` rather than a
zeroed-then-partly-filled `[u32; 256 * 8]`.

`huff_analyze`'s zero-run skip tested eight bytes with `.iter().all()` where the
C read a `u64` and compared it to zero. `u64::from_ne_bytes(s.try_into()?) == 0`
is the playbook entry, and it took `analyze` from 45% slower to parity.

`read_huffman_code_normal` builds a table for the code-length code that the C
kept in a 128-entry stack array. The Rust allocated a `Vec` for it — one malloc
per prefix code, five per meta-block. A `[u32; 1 << 7]` local, as in the C.

Per-frame allocation churn. An animation runs `image_ctx_free` between frames,
and transcribing that as `*self = Default::default()` handed every buffer back
to the allocator so the next frame could ask for the same sizes again. Clearing
the vectors without dropping them, plus reusing the sorted-symbol and
code-length scratch across prefix codes, and pre-reserving the arena the C grew
in 4096-entry chunks.

**Risk 6, measured.** `wpd_mallocz` is `calloc`, and for a picture-sized
allocation that is nearly free: the pages arrive zeroed. `try_reserve` then
`resize` then `fill` — which is what a first draft writes — memsets the buffer
twice, and on `palette4bpp_rgb`, where the tool builds a fresh decoder per
repeat, that was 17.8% of the decode. Dropping the buffer before growing it, so
`resize` zeroes it exactly once, halved that. The remaining single memset is the
standing cost of not calling `alloc_zeroed`, which is `unsafe`; it only shows up
on a decoder that is used once, because a reused one takes the `fill` path the C
takes too.

**Results**, pinned to one core, 15 runs, `--repeat 400`, against the preserved
C baseline `d241ef8`:

| file                      | vs C baseline |
| ------------------------- | ------------- |
| huffman_simple_forms      | 1.53x faster  |
| huffman_long_codes        | 1.32x faster  |
| palette_rgb               | 1.06x faster  |
| lossless                  | 1.01x faster  |
| palette2bpp_rgb           | 1.01x faster  |
| anim_rgb                  | 1.02x slower  |
| predict_topright          | 1.03x slower  |
| a_lossy                   | 1.05x slower  |
| transforms_before_palette | 1.05x slower  |
| overlap_exact             | 1.08x slower  |
| kitchen_sink              | 1.08x slower  |
| durations                 | 1.10x slower  |
| palette4bpp_rgb           | 1.11x slower  |

The main lossless benchmark is at parity and the two files that are all prefix
codes are much faster. What is left is 5-10% on files made of many small frames,
and it is spread thin: the prefix-code build path is still about a third more
expensive than the C's, and `color_rows` — the cross-colour transform — is about
15% more. LLVM vectorises `color_rows` on 32-bit lanes with a lot of `pshufd`
and `punpckldq` where the C's byte pointers let GCC stay on 16-bit lanes. Saying
the product of two `i8`s fits an `i16`, and sign-extending the three multipliers
once per tile instead of once per pixel, did not move it. This is the one place
where the flat-`u32` picture costs something rather than paying, and it is on
the list for the Phase 8 tuning pass.

**The fallbacks, this time, are not the expensive part.** Both sides built with
`-Denable_asm=false`, per the two-no-asm-builds note below:

| file         | Rust scalar vs C scalar |
| ------------ | ----------------------- |
| lossless     | 1.01x faster            |
| palette_rgb  | 1.02x slower            |
| kitchen_sink | 1.07x slower            |

Against the eighth the lossy fallbacks gave up. The lossless decoder is mostly
slice walks and table lookups, which is what safe Rust is good at; the lossy one
is block arithmetic on small fixed-size arrays, which is where the checks land.

**A resumable batch needs one scratch row, not a staging picture.** The C keeps
`c->top`, a single row, because the predictor for the first row of a batch needs
the row above it _as the predictor left it_ — the transforms that run after the
predictor overwrite it. The Tier A predictor signature is `(plane, out, up, n)`:
one allocation, two offsets, because the top-right neighbour of a row's last
pixel is the first pixel of that same row. A separate `top` buffer cannot be
expressed in it.

The answer is not a staging picture — a batch is not bounded, so that could
double the memory. It is a two-row scratch: the saved row goes in the first
half, the batch's first row is copied into the second so the two are adjacent,
that one row is predicted there and copied back, and every row after it already
has the row it needs immediately above it in the picture. Two row copies per
batch, and only on the progressive path, which is the one a caller opts into by
asking for rows early.

**Caller memory stayed out of the core.** `vp8l_set_alpha_dst` hands the decoder
a pointer into a picture the lossy path owns, and the C kept it in `VP8LContext`
across the decode. Here it is an argument to `decode_frame`, so the core never
holds a borrow it did not receive; the shim keeps the `(pointer, stride)` pair
and rebuilds the slice per call, exactly as it does for the chunk.

Gates: `checkasm` 151/151, `meson test` 186/186, `clicheck.sh` 794/794,
`testdata.sh` 183/183, `animcheck.sh` (42 files x 8 formats, 27 animations),
`rac32.sh` 186/186, `md5check.sh` against the C baseline, `sanitize.sh` 186 and
185, `fuzz.sh` 300 trials per file, `cargo test -p wpd` 43/43.

### Phase 4 — the RIFF container (`82ed367` and the port that follows it)

`src/container.c` and `src/container.h` — the chunk-list walk, `wpd_get_info`,
the metadata offsets and the ANMF frame table — are now
`crates/wpd/src/container.rs`, behind `crates/wpd-capi/src/container.rs`. Seven
C files are left: `wpd_decoder.c`, `anim.c`, `convert.c`, `export.c`, `image.c`,
`lossy.c` and `wpd_compat.c`.

**The refactor came first, as it did for VP8 and VP8L.** `HeaderScan` was a
struct the decoder embedded by value and read field by field, which a Rust
implementation cannot offer without mirroring its layout. Commit `82ed367` puts
it behind a pointer and hands the container back a `ScanInfo`: the part it
actually reads, separated from where the walk stopped, the frame table and how
far into an ANMF the alpha walk has gone. That commit is green on its own, so
the API shape is proven behaviour-preserving before any Rust enters.

`collect_frames` stopped being a field the caller pokes and became an argument.
It was only ever set to 1 by one function and cleared by teardown; as a
parameter nothing has to remember to clear it, and the promise that reading a
file's information allocates nothing is visible at the call site.

**A parser reads with saturating helpers, not proven-in-range indexing.** This
is the first module whose whole job is to walk attacker-controlled length
fields, and it is the first place where the usual advice — prove the bound, then
index — is the wrong trade. The C could argue every read in range by
construction; a Rust bounds check that fires on damaged input is a panic, and a
panic on damaged input is a denial of service the C did not have. So every read
in `container.rs` goes through `byte`/`rl16`/`rl24`/`rl32`, which return zero
past the end of the window, and every sub-slice goes through `window`, which
clips. Nothing here is hot — it is O(chunks), once per file plus once per
streaming append — so the cost is not measurable, and there is no residual
argument to get wrong.

The two new tests are shaped around that: one walks every prefix of a synthetic
file, and one corrupts every byte of it to five values in turn and scans the
result. Neither asserts anything about the output; the assertion is that the
scan returns.

**Two error variants, not a second error type.** `Error` gained `Truncated` and
`NotWebp`, which the container needs and no codec raises. `status_from_internal`
on the C side passes a `WPDStatus` through unchanged, so the shim maps them to
`WPD_ERR_TRUNCATED` and `WPD_ERR_NOT_WEBP` and nothing else has to know.

**`WPDImageInfo` is written a field at a time.** The struct is versioned by
`struct_size` and a caller's copy may be a longer revision than this build knows
about. Assigning the whole struct writes its tail padding too, which in a longer
revision is a field; the C guarded that with `WPD_FIELD_END`, and the Rust
guards it by never writing the struct whole.

**No perf change, which is the expected result.** The scan is per file, not per
pixel. Re-measured against the C baseline `d241ef8`, pinned to one core, 20
runs, `--repeat 400`, the table is Phase 3's within noise.

One correction to that table: `palette4bpp_rgb` measures 1.20x slower, not the
1.11x recorded in Phase 3. Building the Phase 3 tree fresh and timing it against
this one put them within 1% of each other, so the gap was there before this
phase and the earlier figure was optimistic — a reminder that a single
measurement session is not a baseline.

Gates: `meson test` 186/186, `clicheck.sh` 794/794, `testdata.sh` 183/183,
`animcheck.sh` (42 files x 8 formats, 27 animations), `rac32.sh` 186/186,
`md5check.sh` against the C baseline, `sanitize.sh` 186 and 185, `fuzz.sh` 300
trials per file, `cargo test -p wpd` 51/51.

### Phase 5 — plane allocation, the image ops and the export

`src/image.c`, `src/convert.c` and `src/export.c` are gone. The decision-making
— which format packs how, what a crop resolves to, what a scale rounds to, and
the YUVA blend arithmetic — is `crates/wpd/src/image.rs`, safe and unit-tested;
the plane walking is `crates/wpd-capi/src/{image,convert,export}.rs`. Four C
files are left: `wpd_decoder.c`, `anim.c`, `lossy.c` and `wpd_compat.c`.

**`export.c` came last, and needed a shape the earlier phases did not.** It was
the one module whose state is genuinely the decoder's: `export_packed` and the
two `export_still_*` functions read and write about twenty `WPDDecoder` fields,
including four scratch `WebPImage` slots, the caller's output planes, and the
`converted_rows`/`converted_format` pair that makes a partial export resumable.
Neither of the two moves that worked before applies — there is no struct to make
opaque, and twenty parameters is not an interface.

They divide in two, though, and the division is the point: what the export needs
to _know_ about the frame is all scalars, and what it reads through or carries
between calls is all pointers. `ExportSettings` and `ExportTargets` are those
halves. Splitting them that way is not tidiness — a struct of only scalars and a
struct of only pointers both have no interior padding, so a field added on one
side of the ABI and not the other changes the size, and a
`const _: () = assert!(size_of::<..>() == ..)` on each catches it at compile
time. The layouts were checked field for field against the C as well.

Two questions moved to the caller because they are about the decode rather than
the output: which of the decoder's three alpha flags applies to this frame, and
what its timestamp is measured from.

**`WebPImage` stays a C struct, deliberately.** Everything else so far became
opaque before it was ported. This one cannot: `crop_image` and `flip_image`
build _views_ by adding to `data[p]` and negating `linesize[p]`, and the rest of
the decoder passes those views around beside owned images in the same type. No
owning Rust type expresses that. What moved instead is the ownership — every
byte an image holds is now allocated and released on the Rust side, so the
`(alloc, alloc_size)` pair is the single description of each block, and the size
arithmetic a damaged header drives is checked rather than argued.

The one C-side change the split needed: `scale_image` used to free a plane by
hand, so `image_drop_plane` had to exist for the allocator to stay the only
owner.

**The refactor commit is the same rhythm, aimed at a different problem.**
`convert.c` did not embed decoder state, it _reached into_ it: `s->options`, the
two DSP tables, the rescaler scratch, `s->pos_x`/`pos_y`. Commit `710e37d` gives
every function what it uses and nothing more, which is what let the port land
without any of `WPDDecoder` crossing the boundary. The DSP tables stay as
parameters because they are chosen per CPU at init, so there is no constant to
select from. `scaled_size` moved out of `lossy.c`, where a pure function of the
options happened to live next to its only caller.

**A cleared plane cleared its stride, and 146 tests failed.**
`image_alloc_plane` in the C only freed the block; the Rust `drop_plane` also
zeroed `data` and `linesize`, because that is what the rest of its callers want.
The allocators set `linesize` _before_ calling it, as the C safely did, so every
freshly grown image came back with a stride of zero. The symptom was unhelpful —
packed output came out looking gamma-shifted, which sent the first hour after a
colour bug — and the cause was visible in one `fprintf` of `img->linesize[0]`.
Setting the geometry after the allocation, never before, is the invariant; it is
now stated where `alloc_plane` is defined.

**The chroma blend was 1.10x slower until it stopped dividing twice.** The C
computed one reciprocal per 2x2 block and applied it to U and V. The first port
expressed the blend as a per-sample function, so it divided once per channel,
and integer division is the whole kernel. Splitting it into a `Mix` — worked out
once from the two alphas, then applied — got most of it back. The rest was the
2x2 alpha average walking two dynamically bounded loops per sample where clang
had unrolled them: both counts are known at the call, so they became const
generics, and the odd-width half block became a tail rather than a branch inside
the loop. `anim_yuv` went from 1.10x slower than Phase 4 to 1.02x faster, and
`blend_yuva_region` from 12.3% of that decode back to under 8%.

Worth keeping: a per-sample helper is the natural way to write a blend and the
wrong way to compile one, whenever a divide or a table lookup is shared across
the channels of a pixel. The row kernel is the unit, not the sample.

**A reference taken before the allocation is a reference to the old image.**
`export_still_packed` bound `&*t->converted` at the top, the way the C bound a
pointer, and then called through to code that grows that image. The C reloaded
the struct on every access; Rust is entitled not to, because a shared reference
promises the value will not change underneath it. The result was a segfault in
`pack_rgb565` writing through the stride the image had before it was allocated.
Every borrow of a `WebPImage` in the ported modules is now taken after the last
call that can reallocate it, and the two helpers that allocate return only the
row they started at.

This is the sharper edge of the same rule the ports keep meeting: a raw pointer
in C is a re-read, and a `&T` in Rust is a promise. Translating one to the other
is only safe where nothing writes through the pointer in between.

**The C reused one view for cropping and flipping; the port does not.**
`export_packed` kept a single stack `WebPImage view` that `transform_image` may
point `img` at, and then flipped by assigning `view = *img` — sometimes with
`img == &view`. In C that is a legal self-assignment. In Rust it is a write to
the referent of a live shared reference, which is exactly the aliasing the
optimiser is allowed to assume away. The flip now gets an image of its own; the
cost is one stack struct and the question stops arising.

**A zero stride divided by zero.** `export_external_rows` guarded its capacity
check with `advance < row` and then divided the caller's buffer size by
`advance`. A zero-width image reaches it with both zero, which passes the first
test and divides by zero in the second. `external_plane_fits` in `wpd::image`
makes the guard explicit — a plane that advances by nothing holds one row — and
covers it with a test.

**One file left 4% slower, and the time is not in this phase's code.**
`palette2bpp_rgb` measures 1.04x slower than Phase 4, reproducibly, while its
two siblings `palette_rgb` and `palette4bpp_rgb` are unchanged. Under a call
graph the extra time is inside `wpd::vp8l::Picture::alloc` — Phase 3 code this
phase did not touch — zeroing a block that used to come back already zero. The
calloc count and byte total are identical between the two builds, so what
changed is which allocation lands where: the image planes now come from the Rust
allocator instead of `wpd_mallocz`, and the lossless picture no longer draws a
fresh mapping from the kernel. It is a heap-layout coincidence at one size, not
a cost in the ported code, and it belongs to the Phase 8 perf pass rather than
to a workaround here. Everything else is within 2% of Phase 4.

The export port itself is perf-neutral: measured against the tree just before
it, every file is within 2% and `palette4bpp_rgb` is 3% faster.

Gates: `meson test` 186/186, `clicheck.sh` 794/794, `testdata.sh` 183/183 across
five configurations, `animcheck.sh` (42 files x 8 formats, 27 animations),
`rac32.sh` 186/186, `md5check.sh` bit-exact against the C baseline `d241ef8`,
`sanitize.sh` 186 and 185 with no ASan or UBSan report, `fuzz.sh` 300 trials per
file, `cargo test -p wpd` 63/63.

## Measuring the fallbacks needs two no-asm builds

The first fallback numbers this port produced were wrong by more than a factor
of two, and the reason is worth writing down.

`--cpumask none` does **not** disable the assembly in a release build. With
`trim_dsp` on — its default is `if-release` — `wpd_get_cpu_flags()` unions the
compile-time baseline back in, and on x86-64 that baseline includes SSE and
SSE2. A release binary run with `--cpumask none` therefore still dispatches to
the SSE2 loop filters. That is deliberate: a binary cannot run on a CPU below
its own target, so trimming the unreachable fallbacks is free.

The consequence for this port is that comparing `--cpumask none` on a preserved
C binary against `--cpumask none` on the Rust build compares SSE2 assembly with
Rust scalar code. To measure the fallbacks, build both sides with
`-Denable_asm=false`:

```sh
git worktree add /tmp/wpd-base <pre-port-commit>
meson setup /tmp/wpd-base/b /tmp/wpd-base -Dbuildtype=release -Denable_asm=false
meson setup build-noasm . -Dbuildtype=release -Denable_asm=false
```

Same lesson as the recorded checkasm A/B pitfall: the two sides of a comparison
have to be configured identically, and "identically" includes the options that
silently change which code runs.

## Monomorphised drivers stop inlining their helpers

The loop filter got slower when its direction, edge size and subblock flag
became const generic parameters — 470 ms to 561 ms — which is the opposite of
what const folding the strides should do. The profile showed why:
`filter_common` and `filter_mbedge` had become their own symbols. The
monomorphised driver was large enough that LLVM stopped inlining them, and every
filtered edge cost a call.

`#[inline(always)]` on the predicates and the two filters took it to 444 ms.
That attribute is not a micro-optimisation here; it is the direct equivalent of
the `wpd_always_inline` the C spells on the same functions, and the same rule
should apply to every DSP kernel ported from a `wpd_always_inline` C helper.

## Size the region by what the kernel reads

Every kernel here takes the exact region it touches as one slice, which turns
the C's negative indices — `p[-4 * stride]`, `src[-stride - 1]` — into ordinary
ones. The extent of that region has to be derived per entry point, not chosen
once per file, because the C reads different neighbourhoods per variant:

- The **simple** loop filter touches `p[-2 * s] .. p[+1 * s]`, not the eight
  samples `LOAD_PIXELS` names. The C reads all eight in the abstract machine and
  relies on the optimiser to drop the unused loads; at the bottom of a
  macroblock the four it does not use are past the end of the buffer, so
  building an eight-sample slice there would be a genuine out-of-bounds read
  where the C had none in practice.
- Each **predictor** reads some of the row above, the column to its left and the
  corner between them. `DC_128_PRED8x8` reads none of them — and it is called
  precisely where there is no neighbour to read.

The failure mode is safe in both directions: too small a region panics on the
first access, which the tests catch; too large is inert, since the kernel never
touches what it does not read. But only an exact extent is a correct
`from_raw_parts`, so each one is spelled out beside the kernel it wraps.

## The yuvdsp seam (resolved)

`src/yuvdsp.c` is the one Phase 1 file that is not purely a DSP table. Its 843
lines are two halves:

- **Table entries** (through line ~507): the five `upsample_block_*`, the alpha
  dispatchers, the eight packers, three premultipliers, `argb_to_y`,
  `argb_to_yuv444`, `argb_to_uv`, and the two gamma tables the assembly gathers
  from directly.
- **Row drivers** (line ~508 on): `wpd_argb_to_yuva`, `wpd_argb_to_yuv444`, the
  five `upsample_row_*`, `wpd_yuv420_to_packed_rows` and its three siblings.
  `src/convert.c` and `src/export.c` call these, not the table.

The drivers cannot simply stay in C: `upsample_row_*` calls
`upsample_pairs_##name` for the row edges and `yuv_to_##name` for the first and
last pixel, both of which live in the kernel half. Leaving them behind would
mean two copies of the same arithmetic.

The seam to cut is therefore **below `upsample_row`, not above it**: port the
table entries plus `upsample_row` (dispatching on layout rather than being macro
expanded per layout), and either port the four `wpd_yuv420_to_packed*` drivers
with them or leave those in C calling a new `wpd_upsample_row(dsp, layout, ...)`
entry point. The per-row edge work is a handful of pixels, so the lost inlining
there is not measurable; `upsample_block` is already reached through a function
pointer.

The two gamma tables should be generated into Rust from the C source rather than
retyped, and exported `#[no_mangle]` so the assembly keeps gathering from them.

## Ordering correction: `vp56rac` moves with `vp8.c`, not before it

The plan had `vp56rac` early in Phase 1 with the other leaves. That is wrong.
`src/vp56rac.h` is almost entirely `wpd_always_inline` and its callers are the
token-decoding loops in `vp8.c`. Porting the coder while `vp8.c` is still C
would turn every `vp56_rac_get_prob` into a cross-language call in the hottest
loop of lossy decoding.

The distinction that matters is **how the caller reaches the module**:

- Reached through a function pointer (`vp8dsp`, `vp8pred`, `vp8l_dsp`) or by an
  ordinary per-row call (`yuvdsp` glue, `rescaler`) — safe to port on its own,
  because the call was never inlined anyway.
- Reached by inlining (`vp56rac`, and the bit reader and `huff_read_symbol`
  headers) — must move in the same commit as its caller.

So Phase 1 is `cpu` → `vp8l_dsp` → `vp8dsp` → `vp8pred` → `rescaler` → `yuvdsp`,
and `vp56rac` joins Phase 2 with `vp8.c`. Likewise `bitreader.h` and
`huffman.h`'s inline half move with `vp8l.c` in Phase 3.

## Aliasing at the DSP boundary

Working out the Tier A signatures means reading each call site, because the C
prototypes permit aliasing the callers do not use — and, in one case, aliasing
the callers _do_ use that a naive `(&[u32], &mut [u32])` pair would make
unsound.

`pred_add(in, upper, num_pixels, out)` is the interesting one:

- `in` and `out` are **always the same pointer** at every call site in
  `predictor_transform_rows`. So the signature cannot be `(&[u32], &mut [u32])`;
  the input has to be read out of the output slice.
- `upper` and `out` **overlap by one element** whenever the rows are physically
  adjacent: the predictors read `upper[num_pixels]` as the top-right neighbour,
  and when `upper + width == row` that element _is_ `row[0]`. This is the
  deliberate trick the `upper[width] = row[0]` write in the caller exists to
  support. A `&[u32]` upper alongside a `&mut [u32]` row would be UB.
- `upper` is `NULL` for the first row (predictors 0 and 1 do not read it).

The flat-buffer form from the playbook — one mutable slice plus offsets —
answers all three, and that is what the C ABI trampolines would have needed.
What the implementation actually settled on is better, and it generalises:

```rust
pub fn pred_add_5(out: &mut [u32], upper: &[u32], left: u32, top_left: u32);
```

Both out-of-row neighbours are passed **by value**. `left` is the running result
the predictor already carries — the C rereads `out[x - 1]` each iteration only
to get back what it just wrote — and `top_left` for `x > 0` is the `top` loaded
on the previous iteration. Carrying them in registers is what the C loop
effectively does anyway, and once they are out of the slices, `out` and `upper`
are disjoint even on physically adjacent rows. Fewer loads and no aliasing
question at the same time.

The same shape recurs: **the neighbours a kernel reads outside its own row
belong in scalars, not in the slice.** It removed the overlap here, and it is
why the loop filter's window is loaded once into `[i32; 8]` rather than reread
per predicate.

The other four are simpler, confirmed against their call sites: `extract_green`
writes an alpha plane from a separate ARGB image (disjoint); `blend_row_argb`
and `blend_row_argb_premult` blend a frame into the canvas, which are always
different `WebPImage` allocations (disjoint); `map_color32` has exactly one
caller and it passes `dst == src`, so its safe form takes a single `&mut [u8]`.

General rule for the rest of the port: **derive each Tier A signature from the
call sites, not from the C prototype.** The prototypes are uniformly more
permissive than the code, and the gap is where both unsoundness and unnecessary
copies hide.
