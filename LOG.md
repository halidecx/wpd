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
- **Intrinsic replacements.** `__builtin_clz` → `leading_zeros()`,
  `__builtin_bswap64` → `swap_bytes()`.
- **Check the disassembly, not the theory.** Hot functions are checked for
  surviving `core::panicking` references; a bounds check that LLVM did not
  remove shows up there.

## Perf risk register

Spots where a safe formulation might cost speed and assembly does not cover.
Each gets a measurement at the phase that ports it.

| # | Risk                                                         | Status  |
| - | ------------------------------------------------------------ | ------- |
| 1 | VP8L entropy pixel loop — bounds checks on `base[4 * pos..]` | not yet |
| 2 | `copy_block32` overlapping LZ77 copy                         | not yet |
| 3 | `huff_read_symbol` table indexing                            | not yet |
| 4 | VP56 range coder refill near the buffer end                  | clear   |
| 5 | yuvdsp row glue under `--no-default-features`                | not yet |
| 6 | `Vec` zeroing vs `calloc` on large canvases                  | not yet |
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
target. Worth recording because it also means any benchmark taken across that
pair before this point was comparing two different configurations.

Option parsing reproduces `getopt_long` with `opterr = 0` — clustered short
options, `--name value` and `--name=value`, `--` to stop, operands and options
interleaved. Unambiguous long-option abbreviation is the one thing left out;
nothing in the tree uses it.

`wpd-capi`'s lib name moved from `wpd` to `wpd_capi` so the archive can also
ship as an rlib without colliding with the core crate's own `libwpd.rlib`.
`tools/cargo_build.sh` stages it back to `libwpd.a`, so nothing downstream
notices.

The y4m writer is the one place the tool reaches past the public header, as the
C tool did with `#include "yuvdsp.h"`. It now uses the real `WPDYUVDSP` type
from `wpd-capi` instead of redeclaring it.

### Phase 2 — `vp8.c` and `vp56rac` in Rust

1720 lines of lossy frame decoder plus 350 of range coder leave the tree. What
replaces them is `crates/wpd/src/vp8/`, which contains no `unsafe`.

**Making the state opaque first paid for itself twice over.**
`struct
WPDDecoder` embedded `VP8Context` by value, and only two lines in the
tree named it. Replacing it with a pointer was a prerequisite for the port —
otherwise Rust would have had to mirror `WPDDecoder`'s layout as well — but it
also turned out to be the single largest speed-up of the project so far: **1.9x
on lossy, 1.18x on lossless**, against a decoder whose lossless path that commit
does not touch at all. `VP8Context` is several kilobytes of probability tables
and coefficient blocks, and it sat in the middle of the struct holding `ldsp`,
`ydsp` and the VP8L cursor fields that the pixel loops reload constantly. Taking
it out compacted everything the hot loops read into far fewer cache lines. The
same shape as the aliasing-hoist win recorded below, at a different scale.

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

**Results**, pinned, 15 runs, `--repeat 60`, against `3a5e80d` (same tree, VP8
still in C) and against the C baseline `d241ef8`:

| file           | vs pre-port | vs C baseline |
| -------------- | ----------- | ------------- |
| lossy          | 1.01x       | 1.92x         |
| a_lossy        | 1.00x       | 1.83x         |
| simplelf-lossy | 1.01x       | 1.41x         |
| anim_yuv       | 1.01x       | 1.28x         |
| anim_yuva      | 1.01x       | 1.15x         |
| lossless       | 1.00x       | 1.18x         |

Gates: `checkasm` 151/151, `meson test` 186/186, `clicheck.sh` 794/794,
`testdata.sh` across five configurations, `animcheck.sh`, `rac32.sh` 186/186
(which is what exercises the 32-bit range coder — its own module, not a
compile-time variant of the 64-bit one), `md5check.sh`, `sanitize.sh` 186 and
185, `fuzz.sh` 300 trials per file.

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
