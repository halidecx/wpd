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

- **A tail is not a detail.** A kernel whose caller's natural batch is smaller
  than one vector spends its whole life in the tail. The cross-colour transform
  is called per tile, a tile can be four pixels, and the AVX2 kernel's one-pixel
  tail was costing more than the vector loop saved. Check what `n` actually is
  at the call sites before deciding the tail can be scalar.

- **A pointer into your own struct is a borrow you cannot express — name the
  field instead.** The C decoder kept two `WebPImage *` that pointed at its own
  members. One was always the same member, so it became a `bool`; the other was
  one of three, so it became a three-variant enum. Every use of a self-pointer
  has to argue that nothing wrote that field in between; a name has nothing to
  argue about, and the match arm costs nothing.

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

**The transformed animation paths had no oracle, which is how the alias survived
one round of fixing.** `tests/parity.c` returns early on any animated file, so
cropping, scaling and flipping are only ever checked against libwebp for stills.
The first attempt at the fix below moved the relabelled image onto the same
variable as the flip and reintroduced the aliasing exactly, and every gate
stayed green. `test_flip_reverses_rows` in `tests/api.c` closes that: a flip is
the last pass over finished rows, so a flipped decode must be the row-reversal
of an unflipped one, which is a property no second decoder has to agree with and
which holds for an animation as much as a still. It runs on `anim_yuva.webp` in
`ARGB_PRE` — the one packed output that is relabelled rather than converted, and
so the only way to reach that branch twice — and it was checked against a
deliberately broken `flip_image` before being kept.

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

**What `sanitize.sh` covers changed, and the README now says so.** ASan
instruments compiler-generated code, and there is no longer any decoder C for it
to instrument — only the harnesses in `tests/`. The decoder still benefits
through the intercepted allocator, so a heap overrun that crosses a redzone is
caught, but not through instrumented loads and stores the way it was. Nothing
regressed; what changed is what a clean run proves. Instrumenting the Rust
proper needs a nightly toolchain and is Phase 8 work.

Gates: `meson test` 186/186, `clicheck.sh` 794/794, `testdata.sh` 183/183 across
five configurations, `animcheck.sh` (42 files x 8 formats, 27 animations),
`rac32.sh` 186/186, `md5check.sh` bit-exact against the C baseline `d241ef8`,
`sanitize.sh` 186 and 185 with no ASan or UBSan report, `fuzz.sh` 300 trials per
file, `cargo test -p wpd` 63/63.

### Phase 6 — the animation compositor

`src/anim.c` is down from 368 lines to 217. What left it is the compositor:
`crates/wpd/src/anim.rs` decides where a frame lands and how it divides into
regions, and `crates/wpd-capi/src/anim.rs` walks those regions over the canvas.
`decode_anmf` stays, and moves with `wpd_decoder.c`.

**The split is the same one the export used, and for the same reason.** What the
compositor needs to know — where the frame goes, what the frame before it left
behind — is all scalars, so `Placement` mirrors as a struct whose size is its
own checksum. What it writes through is a canvas and two DSP tables, so
`CompositeTargets` is three pointers. `anim_is_key_frame` is its own entry point
because the decision has to be made before the placement is complete: it is what
fills in the one field the decoder does not already know.

**The geometry was worth lifting out on its own.** Compositing an animation
frame is two questions — is this a key frame, and which parts of it blend — and
neither touches a pixel. `regions()` answers the second by returning up to five
rectangles: libwebp overwrites the frame rectangle and alpha-blends only where
the previous canvas can be non-transparent, so when the frame before disposed
its own rectangle, the overlap is copied and the four strips around it are
blended. Written as five `composite_region` calls in the C, that is five chances
to get an off-by-one wrong and no way to test it below a whole decode. Written
as a function returning rectangles, the test is that the five tile the frame
exactly: every pixel covered once, none twice. Nine tests cover it, and they run
in microseconds.

**One thing that looked like a bug and was not.** The planar path rounds the
overlap's _extent_ down to even samples but leaves its _corner_ alone, so a copy
region can start inside a 2x2 chroma block and the blitters truncate it. That
reads like an oversight; it is what libwebp does, and `animcheck.sh` is
bit-exact against libwebp across 27 animations, so it stays. The port says so
where it would otherwise look like a transcription slip.

Gates: `meson test` 186/186, `clicheck.sh` 794/794, `testdata.sh` 183/183 across
five configurations, `animcheck.sh` (42 files x 8 formats, 27 animations),
`md5check.sh` bit-exact against the C baseline `d241ef8`, `sanitize.sh` 186 and
185, `fuzz.sh` 300 trials per file, `cargo test -p wpd` 72/72. Perf is unchanged
against Phase 5.

### Phase 7a — the streaming input buffer

The six fields that described what had arrived — the pointer, the allocation,
the size, how much of the front had been dropped, the capacity, and whether the
memory was the caller's — are now `InputBuffer`, allocated and grown in
`crates/wpd-capi/src/input.rs` with the arithmetic in `crates/wpd/src/input.rs`.

**This is the piece of Phase 7 that is genuinely its own object.** The rest of
`wpd_decoder.c` is the public API and cannot move until `WPDDecoder` does, but
what has arrived is a thing with three ways in and one invariant, and naming
them made the invariant visible: a whole file copied in, a whole file borrowed
from the caller, or a stream appended to a chunk at a time. Only the third owns
a growing allocation and only it ever drops bytes off the front, which is why
every position the decoder remembers is a stream offset and not a pointer.
`input_compact` now says a borrowed buffer keeps every byte, rather than relying
on its one caller happening to be the append path.

**Two properties worth a test rather than an argument.** That growth doubles is
not decoration: a caller appending a stream byte at a time is the shape a
network feed has, and a buffer that grew by a constant would make that
quadratic. The test appends a byte a million times and asserts the buffer grew
fewer than eight times. The other is that a `keep` behind what has already been
dropped is declined rather than wrapped — the decoder asks to keep the chunk it
is working on, and an earlier compaction may already have stopped short of it,
so the subtraction that looks safe is the one that would underflow.

`MAX_BUFFERED` is stated as what it is: the decoder hands chunk sizes to the
codecs as `int`, so a stream that grew past `INT_MAX` could not be described to
them. The C had the same two bounds checks without saying why.

Gates: `meson test` 186/186, `clicheck.sh` 794/794, `testdata.sh` 183/183 across
five configurations, `animcheck.sh` (42 files x 8 formats, 27 animations),
`rac32.sh` 186/186, `md5check.sh` bit-exact against the C baseline `d241ef8`,
`sanitize.sh` 186 and 185, `fuzz.sh` 300 trials per file, `cargo test -p wpd`
79/79.

Four C files are left, and they move together: `wpd_decoder.c` holds the public
API, `lossy.c` and `anim.c`'s `decode_anmf` both take a `WPDDecoder *`, and
`wpd_compat.c` cannot go until they do, because `wpd_log` is variadic and stable
Rust cannot define a C-variadic function.

### Phase 7b — the decoder itself, and the last of the C

`wpd_decoder.c`, `lossy.c`, `anim.c` and `wpd_compat.c` are gone. `src/` now
holds the DSP headers `tests/checkasm` includes and one translation unit of x86
SIMD constants; there is no C decoder left to compile.

**This is the one step the refactor-then-port rhythm did not fit, and it is
worth saying why.** Every earlier module was narrowed in C first — made opaque,
or given a scalars-and-pointers pair of structs — and proved green on its own
before the implementation moved. That works when the thing being narrowed is
smaller than the interface around it. Here it was the other way round:
`vp8_lossy_step` and `decode_anmf` each read or write about fifteen `WPDDecoder`
fields, including the ones the canvas negotiation mutates from both sides, so
the "narrow struct" would have been the decoder with a different name. The
refactor commit would have been pure ceremony and would still have left the
whole port in the commit after it. So `WPDDecoder` became a Rust struct and the
two drivers came with it, in one step.

**Two self-referential pointers became names.** `lossless_frame` was a
`WebPImage *` that was assigned `&decoder->argb` at all four of its call sites,
so it is a `bool`. `subframe_out` pointed at whichever of `subframe`, `argb` and
`converted` the frame finished in, so it is an `enum Subframe` with three
variants. Neither change was made for tidiness: a raw pointer into your own
struct is a borrow the language cannot check, and every use of it has to argue
that nothing else touched that field in between. Naming the field instead costs
one match arm and removes the argument.

**Ownership became `Drop`.** The C's `wpd_decoder_free` was a fifteen-line
sequence, and a field added without a line added to it leaked silently. The
scanner, both frame decoders and the input buffer are now owned values, the
metadata copies and the alpha plane are `Vec`s, and the four images and the
rescaler scratch are the only things `Drop` still has to release by hand —
because they are C-shaped views that double as owners, which is the same reason
`WebPImage` could not be made opaque in Phase 5.

**`wpd_decoder_error` had to stay a fixed array.** The obvious Rust is a
`CString` field, and it is worse: the C's `char[128]` only changed its
_contents_ on the next failure, where a replaced `CString` frees the buffer the
caller is still holding a pointer to. Safer-looking Rust that is less safe at
the ABI, and the only reason to notice is asking what the returned pointer's
lifetime actually is.

**`wpd_log` did not need porting, it needed deleting.** It was the last variadic
in the tree and the stated reason `wpd_compat.c` had to go last, since stable
Rust cannot define a C-variadic function. But the only remaining caller was the
Rust log sink, which was formatting a message and passing it through `"%s"`. Cut
the C out and the variadic goes with it. Two behaviours that were invisible in
its signature had to come across, though, because `clicheck.sh` compares the
tool's stderr byte for byte: it stripped trailing newlines and truncated at 511
bytes, and several call sites still pass a message ending in `\n`.

**Non-nullable function pointers cannot be zeroed.** `WPDYUVDSP` and
`WPDLosslessDSP` are tables of bare `extern "C" fn`, so a `calloc`-shaped
construction of the decoder — `mem::zeroed` and then fix the fields up — is
instant undefined behaviour, not merely ugly. Both tables gained a `new()`
returning a value, and `wpd_decoder_create` is a plain struct literal.

Gates: `meson test` 186/186, `clicheck.sh` 794/794, `testdata.sh` 183/183 across
five configurations, `animcheck.sh` (42 files x 8 formats, 27 animations),
`rac32.sh` 186/186, `md5check.sh` bit-exact against the C baseline `d241ef8`,
`sanitize.sh` 186 and 185 with no ASan or UBSan report, `fuzz.sh` 300 trials per
file, `cargo test -p wpd` 79/79, clippy clean.

Perf, Phase 7a against Phase 7b in one build directory: every file within 1-2%,
which is the noise floor at these sizes. Against the C baseline on the three
files where decoding actually dominates the process — `lossy`, `lossless`,
`anim_yuva` — 1.01, 1.01 and 1.00. The small files show the Rust binary's larger
start-up cost, which is 70 us on a 400 us process and has been there since Phase
1; that is why these comparisons are phase against phase.

### Phase 8a — the tuning pass

Four changes, three of them in safe Rust. Every file in the corpus that does a
measurable amount of decoding is now at or ahead of the preserved C baseline
`d241ef8`.

**The cross-colour transform got assembly, and it was the biggest single win.**
`color_row` in SSSE3 and AVX2, 4.7x and 10.6x the scalar in checkasm, worth 6%
of `lossless.webp` end to end — it was 7.8% of that decode and is now 1.2%. The
trick is the pixel layout. A pixel is `[A, R, G, B]`, so a 16-bit lane pair is
`(R:A, B:G)`: the two channels the transform writes sit in a lane's _high_ byte
and the green it predicts them from in the low byte of the other. One `pshufb`
puts green into both lanes' high halves, and `pmulhw`'s `>> 16` then absorbs the
transform's `>> 5` for nothing, because a byte in a lane's high half is already
multiplied by 256. That is eight instructions per vector against libwebp's ten,
and no final mask, because each delta lands only on the byte it belongs to
rather than having to be cleaned off the two it does not.

**A four-pixel step in the AVX2 tail was worth as much as the kernel.** A
colour-transform tile can be four pixels wide, so on `predict_topright` every
call fell straight into the one-pixel tail and spent eight vector ops on each
pixel — 6.5% of the file, with the main loop not even appearing in the profile.
Adding an xmm step ahead of the scalar tail took that file from 3% behind the C
baseline to level. Worth remembering that a kernel's tail is not a detail when
the caller's natural batch is smaller than a vector.

**Two bounds checks around three instructions of work.** The packed-palette
expansion's inner loop was twelve instructions, six of them the two range checks
on `row[off + b * PPB..][..PPB]`. LLVM cannot drop them: the relation between
`off`, `full` and the row length that makes the write in bounds is not one it
can see, and no rearrangement of the indexing persuades it. Lifting each block's
indices into a small stack scratch first removes the aliasing between source and
destination, which lets the write walk forwards over a slice taken once per
block. 12% on `palette4bpp_rgb`, from 1.11x behind the baseline to level — **and
no assembly**, on the file that had looked most like the one wanting a gather.

**The alpha palette path was never specialised.** `expand_palette_rows` got the
const-generic group size in Phase 3; `color_indexing_alpha`, which does the same
job into a byte plane rather than an ARGB one, did not. It ran a variable-count
shift per pixel and a `chunks_exact_mut` whose stride the compiler could not
fold. The same fix — a per-group expansion table and a const `PPB` — took
`a_lossy` from 56.5ms to 47.2ms, from 6% behind the C baseline to 13% ahead.
This had been on the register since Phase 3 as "a_lossy 1.05x slower" and the
reason turned out to be a transform that simply never got the treatment its twin
did.

**One measurement that went the other way, kept because it will look obvious
again.** Widening the sparse zero-run skip in `analyze` from eight bytes to
thirty-two looks free: the alphabets run to a couple of thousand entries. It is
3% better on `huffman_long_codes` and 3% worse on `transparent_over`, whose
lists are short enough that the wide stride rarely fires, and eight wins on
both. Reverted, with the numbers in the comment. Writing the same test as
`run.iter().all(|&b| b == 0)` is worse than either stride: `objdump` showed zero
vector instructions in the function, because that compiles to a byte loop with
an early exit rather than the word compare the explicit `u64` gives.

**Where it lands**, pinned to one core against `d241ef8`, at a repeat high
enough that decoding dominates:

| file                      | vs C baseline |
| ------------------------- | ------------- |
| huffman_simple_forms      | 2.19x faster  |
| huffman_long_codes        | 1.50x faster  |
| dispose_bg_fullframe      | 1.20x faster  |
| a_lossy                   | 1.13x faster  |
| transforms_before_palette | 1.08x faster  |
| anim_yuv                  | 1.08x faster  |
| lossless                  | 1.07x faster  |
| palette2bpp_rgb           | 1.06x faster  |
| anim_yuva                 | 1.05x faster  |
| palette4bpp_rgb           | level         |
| predict_topright          | level         |
| lossy                     | 1.01x faster  |
| durations                 | 1.01x slower  |
| anim_rgb                  | 1.01x slower  |
| overlap_inside            | 1.02x slower  |
| odd_frames                | 1.03x slower  |
| transparent_over          | 1.04x slower  |

What is left is 1-4% on animations of very small frames, where building a prefix
code per frame is a third of the decode and the picture itself is a few thousand
pixels. Against libwebp the whole corpus is between 1.02x and 2.54x faster.

**Two things to be careful of when reading numbers on this corpus.** The Rust
binary starts about 70us slower than the C one, which is invisible on
`lossless.webp` and is the whole difference on `transparent_over` at a low
repeat: at `--repeat 60` that file reads as 1.09x slower and at `--repeat 3000`
as 1.04x. And `--repeat` interacts with which build allocates when, so a file
that inverts as the repeat grows is measuring start-up, not decoding.

### Phase 8b — the tooling the port had been deferring

Three gates that had been listed as Phase 8 work since Phase 1, all of which
need a nightly toolchain, and all of which now exist as scripts.

**`scripts/rustsan.sh` is the one that mattered.** When the last C went, so did
what ASan could see: `sanitize.sh` instruments the test harnesses, and the
decoder only benefits through the intercepted allocator. `-Zsanitizer=address`
plus `-Zbuild-std` gets an instrumented standard library and an instrumented
decoder, so every load and store is checked again. The script decodes the whole
corpus in nine output formats in both feature configurations and compares each
result against the reference build — 378 decodes each way, and it found nothing.
That is the answer to the honest caveat Phase 7b had to put in the README.

**miri runs the core crate's tests**, necessarily with `--no-default-features`,
because it cannot execute hand-written assembly. What it proves is that the safe
fallbacks — the ones checkasm compares the assembly against — are free of
undefined behaviour, and with them the slice arithmetic every kernel shares.
79/79, in about a minute. Its value will grow considerably once the driver moves
into the core crate and a whole decode can run under it.

**`cargo fuzz` drives the three parsers** — the RIFF walk, the lossless decoder
and the lossy one — through both their one-shot and their resumable entry
points. This asks a different question from `scripts/fuzz.sh`, which mutates
real files and looks for memory errors: here the failure mode being hunted is a
panic on damaged input, which is a denial of service the C did not have and the
saturating-read discipline of Phase 4 exists to prevent. Twenty seconds a target
found nothing; the corpora are kept out of the tree.

**A gate that was only half on.** `stylecheck.sh` ran clippy with
`--all-features`, and the no-asm build compiles different code: the dispatch
tables and the mode indices that give them their layout have no user without the
assembly, so seventeen warnings lived there unseen. It now runs both
configurations, and the no-asm one is the configuration the safety claim rests
on.

### Phase 8c — the safe picture type, and what the driver port turns on

The remaining Phase 8 goal is a safe Rust API, and the honest form of it is
moving the driver into `crates/wpd` rather than wrapping the C ABI: the point of
the rewrite is that a decode is provably free of memory-unsafety, and today that
holds for the two codec cores and not for the plumbing around them.
`crates/wpd/src/picture.rs` is the type the rest of that port stands on.

**Why `WebPImage` could not just be moved.** It is a single struct that is
sometimes an owner — it holds `alloc[p]` and frees it — and sometimes a view
into memory the VP8 or VP8L decoder owns, and nothing in the type says which. It
also expresses a crop by adding to `data[p]` and a flip by pointing at the last
row and negating `linesize[p]`. None of those three survive contact with an
owning Rust type.

The split is `Buffer`, which owns plane memory and is reused frame to frame,
against `Frame` and `FrameMut`, which are borrowed views. A crop becomes an
`origin` offset and a flip becomes a `flip` flag applied by `Frame::row`, so the
reading order is a property of the view rather than a rewrite of the pointers. A
negative stride still has to exist, but only at the C ABI boundary, where the
shim builds one on the way out — which is exactly where the C ABI demands it and
nowhere else.

**One thing the compiler would not take, and the fix was not a cast.**
`FrameMut::row_pair` hands out rows of two planes at once, which every chroma
kernel needs. The four planes are four separate allocations, so they can never
overlap, but the borrow checker cannot see that through an index. The first
draft reached for a raw pointer and `forbid(unsafe_code)` rejected it, which is
the lint doing its job: `split_at_mut` on the plane array proves the same thing
and costs nothing, at the price of requiring the two planes in order.

What remains is converting `convert`, `export`, `anim`, `lossy` and `decoder` —
about 5,000 lines and some 500 uses of `unsafe` — from pointer walking to this
type. When that lands, `scripts/miri.sh` covers a whole decode rather than the
kernels alone, which is the real reason to want it.

### Phase 8d — no C at all, and the compositor's kernels move

**The library compiles no C.** `src/x86/wpd_simd_constants.c` held seven
constant vectors the assembly read through `cextern_naked`; they are now
ordinary `SECTION_RODATA` labels in the three `.asm` files that use them, which
is how dav1d has always done it and removes a cross-language symbol as well as a
translation unit. `crates/wpd-capi`'s build script no longer invokes a C
compiler at all and has dropped the `cc` build-dependency — what is left of it
is thirty lines that pass the aarch64 extension probes through from
`crates/wpd`. The one remaining use of `cc` in the tree assembles `.S` files and
compiles probe snippets, which is exactly what rav1d's build script uses it for.

Two things to check when moving constants into assembly, both of which decided
where they went: every use is a sixteen-byte load (`mova` inside `INIT_XMM`, or
an explicit `vbroadcasti128`), so sixteen-byte definitions are enough; and
`vp8_intrapred.asm` has an eight-byte entry in its rodata, so the new ones need
an `alignb 16` rather than trusting the section's alignment.

**The compositor's kernels are the first of the driver to move.** The four
region blitters and the canvas clear are now `crates/wpd/src/blit.rs`, safe code
under `forbid(unsafe_code)`, with `crates/wpd-capi` bridging its `WebPImage`
into a `Frame` or `FrameMut` to call them. `blend_row_argb`,
`blend_row_argb_premult` and `extract_green` gained entries in the core
`Vp8lDsp` table with their assembly dispatch, and `WPDDecoder` now holds that
table rather than the C-ABI one — which is left for `checkasm` alone, its only
remaining caller.

**`FrameMut::planes_mut` earned its keep immediately.** The chroma blend writes
U and V while reading the alpha plane of the same picture. Destructuring the
plane array — `let [_, u, v, alpha] = dst.planes_mut()` — proves those three
disjoint for free. This is the case rav1d needs `DisjointMut` for, and the
reason wpd does not is that wpd is single-threaded: rav1d's problem is two
_threads_ writing disjoint parts of one picture, which no amount of
destructuring expresses. That difference is what makes a fully safe pipeline
reachable here and not there, and it would go away the day wpd wants threads.

### Phase 8e — the YUV DSP, the rescaler and the row drivers

`convert` could not become safe code until the two things underneath it did, so
all three moved together.

`crates/wpd/src/dsp/yuv.rs` gained `YuvDsp`, a table of safe `fn` pointers with
the same eighteen entries the C-ABI one has, and `crates/wpd/src/asm/yuv.rs` the
assembly dispatch behind safe wrappers — one marker type per symbol and one
generic wrapper per shape, laid out as `asm/vp8l.rs` is. One small thing worth
knowing: `#[link_name]` takes a literal, not a `concat!`, so a
`pack_syms!("ssse3")` macro does not work and every symbol is spelled out where
it is bound. That turns out to be better anyway, because it keeps them
greppable.

`crates/wpd/src/rescale.rs` gained `Rescaler`, `Scratch` and the two plane
drivers. The C carried its two accumulator rows as two pointers and swapped them
per imported line; they are one `Scratch` allocation split in half here, with a
flag saying which half is which. **Swapping two borrows of one slice is not
something the borrow checker will hold still for, and it does not need to be** —
the swap was only ever a way to name the older of two rows.

`crates/wpd/src/convert.rs` is the row drivers: the fancy and point upsamplers,
the 4:4:4 point converter and both directions between ARGB and planar.
`wpd-capi`'s copies are gone rather than duplicated. `src/yuvdsp.h` and
`src/rescaler.h` keep only what `tests/` and the tool call, and those entry
points take no table any more — the one the core builds from the CPU flags in
force is the one under test, and `checkasm` sets those flags before it asks.
`checkasm` still reaches every kernel through `WPDYUVDSP`, which is what that
table is for now.

The picture type learned two things it needed. `FrameMut` carries `chroma_full`,
so a picture whose U and V the rescaler has brought up to full size reports
full-width chroma rows; and `PlaneMut` gained `row_pair_mut`, because the fancy
upsampler writes an (odd, even) row pair of one plane at a time and
`split_at_mut` on the plane is what proves the two rows disjoint.

### Phase 8f — the decoder's pictures are `Buffer`s

`WebPImage` was two things at once: an owner that held `alloc[p]` and freed it,
and a description of memory one of the codecs owns. The four owned ones —
`canvas`, `converted`, `output`, `transformed` — are `wpd::picture::Buffer` now,
so their memory is released by `Drop` rather than by a sequence in
`wpd_decoder_free` that a new field can be left out of. What is left of
`WebPImage` is the view half, and its allocator is gone.

That let the picture pipeline change shape, and this is the part worth keeping:

- a crop is `Frame::window` rather than arithmetic on `data[p]`;
- a flip is `Frame::flipped` — a reading order — rather than pointing at the
  last row and negating `linesize[p]`;
- `transform_image` returns the `Frame` to read from instead of writing a
  pointer through an out-parameter;
- and the negative stride the C ABI promises is built in exactly one place,
  `export::handout`, on the way out.

**One behaviour had to be re-derived rather than transcribed.** The C
premultiplied in place through whatever pointer it was holding, which could in
principle have been the source picture. It never is: `set.premultiply` is only
set for a premultiplied output format, no picture the decoder holds is ever
premultiplied, so a planar source has been through `convert_to_packed` and an
ARGB one through the packer or through the copy the relabelling path makes for a
still. Every path reaching there has written into `output`, and naming that
buffer is what let the pass take a `FrameMut`. **Where the C mutated through a
pointer whose provenance the reader has to reconstruct, work out which object it
actually is and name it** — the answer is usually one object, and if it is not,
the code was relying on something.

### Phase 8g — the safe Rust API

`crates/wpd-capi/src/api.rs` is the safe API, modelled on rav1d's
`src/rust_api.rs`. Every entry point `include/wpd.h` declares is reachable
without writing `unsafe`, and two things the C ABI can only ask for in prose are
types here:

- **A picture borrows the decoder that produced it.** `wpd_decoder_next_frame`
  hands out pointers into memory the next call reuses; `Picture<'a>` holds the
  borrow, so asking for the next frame while the previous one is alive does not
  compile.
- **Opening without a copy borrows the input.** `wpd_decoder_open_borrowed`
  promises the caller keeps the bytes alive for the decoder's whole life;
  `Decoder<'a>::open_borrowed` makes that a lifetime.

`Picture::row(plane, y)` hands out a slice rather than a pointer and a stride,
and it applies the flip, so a caller never sees the negative stride at all.

It lives in `wpd-capi` rather than the core crate because the driver it wraps
does, and it moves up with the driver. `crates/wpd-capi/tests/api.rs` drives it
over the whole corpus: every packed format, planar chroma, sub-frame mode, a
stream fed in 97-byte pieces against a whole-file decode, and a flip against its
unflipped twin.

**`crates/wpd-tool` went through it and dropped `sys.rs`.** The tool is now free
of `unsafe` in all three of its files, and `src/yuvdsp.h` lost
`wpd_argb_to_yuv444`, which existed for the tool's `y4m` writer and has no
caller left. `clicheck.sh` compares 794 invocations of the tool against the C
baseline byte for byte, which is what makes a migration like this checkable at
all: the `--info` output prints C `int`s, so the API's `bool`s have to be turned
back into `0` and `1` at the print, and `blend` inverts — `WPD_BLEND_ALPHA` is
zero. Both would have been silent without that gate.

The C ABI is not left untested by the move: `tests/api.c` is 3,558 lines and 521
calls covering every entry point `include/wpd.h` declares.

### Phase 8h — the driver stops pointing at itself

Four things went at once, because they were one thing.

**The internal C ABI had no C left on the other side of it.** `crates/wpd-capi`
carried `extern "C"` shims for the input buffer, the container scanner and both
codecs — 122 uses of `unsafe` whose whole job was to turn a pointer back into
the slice the core wanted. Grepping `src/` and `tests/` for the symbols they
export finds nothing: the only callers were Rust. They are gone, and the driver
calls the core directly. A shim you keep for a caller that no longer exists is
not a boundary, it is a copy of one.

**The input became a type.** `wpd::input::Input<'a>` owns a growing `Vec` for a
stream and borrows a slice for a whole file the caller lends, and which of the
two it is holding is now in the type rather than in a `borrowed` flag beside a
pointer. `Input::at(offset)` hands out the stream from an offset on, so
`WPDDecoder::file_at` returns `&[u8]` and the chunk walk in `next_frame` and
`decode_anmf` indexes slices instead of doing pointer arithmetic against an
`end` pointer. `WPDDecoder<'a>` carries the input's lifetime, and
`Decoder<'a>::open_borrowed` in the safe API is now that lifetime rather than a
`PhantomData` standing next to one.

**The decoder's pictures became borrows.** `WebPImage` — the `(pointer,
stride)`
set the two codecs' output was latched into after every decode — is deleted. The
VP8 planes and the alpha plane beside them are fields of the same struct, so
`lossy_view` builds a `Frame` out of both and it cannot go stale. Which picture
an export reads is a `Source`, a name, not a pointer captured at some earlier
moment.

**The targets became borrows too, and that named a rule.** `ExportTargets` was
nine raw pointers into the decoder. It is nine references now, taken by
destructuring the decoder at the call — which promptly failed to compile,
because a whole-frame export can be _handed_ the conversion buffer as its
source, and the resumable row exports _write_ it. Both were reachable through
the same struct of pointers and nothing said they were never the same buffer at
the same time. Splitting it into `ExportTargets` and `RowTargets` is what says
it. The compiler asked the question the C never had to answer.

`anim.rs` and `lossy.rs` are at zero `unsafe`; `wpd-capi` is at 286, against 579
before this phase and 785 when Phase 8c started. The alpha filter's inverse
prediction, which walked four pointers per pixel, is `PlaneMut::row_pair_mut`
and plain indexing.

**Two things a `Vec` charges for that a `malloc` did not.** Both showed up as a
1.36x regression on the small hand-written lossless files, where a decode is
five microseconds and anything fixed dominates.

`Vec::resize` zeroes what it adds, and `realloc` does not. The input buffer's
capacity doubles from 64 KiB, so sizing the _length_ to the capacity meant a 64
KiB memset for every decoder — for a 300-byte file. The fix is to reserve the
capacity and only lengthen to what is used: growth stays amortised, and the
zeroing is proportional to the bytes that exist.

Holding `wpd::vp8::Decoder` by value put 4 KiB into `WPDDecoder` that a
lossless-only file never touches. The C built it on the first VP8 chunk, and
`Option<Box<_>>` says the same thing. Worth keeping even though the constructor
itself measures 186ns: the cost was the struct, not the work.

Neither is visible against the C baseline alone — `bench.sh` and `cmpbench.sh`
compare against `d241ef8`, which is four commits and a whole phase away, so a 9%
difference there says nothing about the change in hand. Building the previous
commit in a `git worktree` and benchmarking against _that_ is what separated
"this phase regressed the small files" from "the small files have been 9% off
the C since the driver became Rust." The first was true mid-phase and is fixed;
the second is true and predates it.

**Why the driver still cannot move into `crates/wpd`.** The lossless canvas is a
`Vec<u32>` — the pixel loops want words — and everything downstream of it wants
rows of bytes. There is no way to reinterpret one as the other in the standard
library without `unsafe`: `slice::align_to` is itself unsafe, and `to_ne_bytes`
copies. So it needs `unsafe` or a dependency, and the core promises neither —
`#![forbid(unsafe_code)]` without the `asm` feature is a headline property. What
is left of the module is `crates/wpd-capi/src/vp8l.rs`, about fifty lines whose
only job is that cast, holding four of the crate's 286. Everything else in the
driver could move today.

**What rav1d does about the same problem.** It stores picture buffers as `[u8]`
and produces typed views with `zerocopy`: `DisjointMutGuard::cast_slice::<V>`
calls `V::slice_from(bytes)` for `V: AsBytes + FromBytes`, which is safe and
fallible — the `[u8] -> [u16]` direction can fail on alignment, which is why the
allocation is `AlignedVec64`. Not every path goes through it;
`Pixels::as_mut_ptr` does a raw `.cast()` and its comment says the invariant is
verified by the `zerocopy` bound elsewhere. So the honest summary is that rav1d
picked bytes as the canonical storage and used `zerocopy` to get words back out
of them.

wpd picked the other canonical storage, and the direction it needs is the easy
one: `[u32] -> [u8]` only ever weakens alignment and `u32` has no invalid bit
patterns, so `zerocopy`'s `as_bytes()` is infallible — no `Option`, no alignment
check, no `AlignedVec`. That makes the dependency cheaper than the framing above
suggests, and it is the one thing standing between the driver and the core.
Deferred rather than declined: wpd has no runtime dependencies today, and that
is worth spending deliberately rather than in passing.

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
