// Compare DSP dispatch across builds with an explicit CPU mask, e.g.:
// cargo run --release --example dspbench -- 3 idct_dc
// The optional second argument selects a kernel name prefix. Timings are
// medians in nanoseconds per call; DC coefficients are restored each call.
// The -32768 DC case supplies changing signed coefficients.
use std::hint::black_box;
use std::time::Instant;
use wpd::dsp::yuv::{self, UpsampleDst, UpsampleSrc};
use wpd::dsp::{filters::FilterDsp, vp8::Vp8Dsp, vp8l::Vp8lDsp, yuv::YuvDsp};

fn bench(mut f: impl FnMut()) -> f64 {
    for _ in 0..1000 {
        f();
    }
    let start = Instant::now();
    for _ in 0..10000 {
        f();
    }
    let reps = ((0.002 / start.elapsed().as_secs_f64()) * 10000.0) as usize;
    let reps = reps.clamp(1000, 1000000);
    let mut samples = Vec::new();
    for _ in 0..9 {
        let start = Instant::now();
        for _ in 0..reps {
            f();
        }
        samples.push(start.elapsed().as_secs_f64() * 1e9 / reps as f64);
    }
    samples.sort_by(f64::total_cmp);
    samples[4]
}

fn main() {
    let prefix = std::env::args().nth(2).unwrap_or_default();
    let mask: u32 = std::env::args()
        .nth(1)
        .unwrap_or("3".into())
        .parse()
        .unwrap();
    wpd::cpu::init();
    wpd::cpu::set_mask(mask);
    let vp = Vp8Dsp::new();
    let loss = Vp8lDsp::new();
    let filter = FilterDsp::new();
    let yuv = YuvDsp::new();
    let mut plane = vec![100u8; 65536];
    let mut block = [0i16; 16];
    if "idct_dc".starts_with(&prefix) {
        for stride in [4, 16, 32, 128, 1024] {
            for dc in [-2048, -127, 0, 127, 2047, i16::MIN] {
                let f = black_box(vp.idct_dc_add);
                let mut state = 1u32;
                let ns = bench(|| {
                    let dc = if dc == i16::MIN {
                        state = state.wrapping_mul(1103515245).wrapping_add(12345);
                        (state >> 20) as i16 - 2048
                    } else {
                        dc
                    };
                    block[0] = black_box(dc);
                    f(
                        black_box(&mut plane),
                        1,
                        black_box(stride),
                        black_box(&mut block),
                    );
                });
                println!("idct_dc,{stride},{dc},{ns:.4}");
            }
        }
    }
    if "wht_dc".starts_with(&prefix) {
        let mut blocks = [[0i16; 16]; 16];
        let mut dc = [0i16; 16];
        let f = black_box(vp.luma_dc_wht_dc);
        let ns = bench(|| {
            dc[0] = black_box(2047);
            f(black_box(&mut blocks), black_box(&mut dc));
        });
        println!("wht_dc,0,0,{ns:.4}");
    }
    for (name, f) in [
        ("loop_v16", vp.v_loop_filter16y),
        ("loop_h16", vp.h_loop_filter16y),
        ("loop_v16_inner", vp.v_loop_filter16y_inner),
        ("loop_h16_inner", vp.h_loop_filter16y_inner),
    ] {
        if !name.starts_with(&prefix) {
            continue;
        }
        for stride in [32, 128, 1024] {
            for pattern in 0..4 {
                let mut state = 1u32;
                let source: Vec<_> = (0..16 * stride + 32)
                    .map(|i| {
                        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                        match pattern {
                            0 => 128,
                            1 => 120 + ((state >> 24) & 15) as u8,
                            2 => (state >> 24) as u8,
                            _ => {
                                if i / stride < 8 {
                                    120
                                } else {
                                    130
                                }
                            }
                        }
                    })
                    .collect();
                let f = black_box(f);
                let ns = bench(|| {
                    plane[..source.len()].copy_from_slice(&source);
                    f(
                        black_box(&mut plane),
                        4 * stride + 4,
                        black_box(stride),
                        black_box(40),
                        black_box(15),
                        black_box(1),
                    );
                });
                println!("{name},{stride},{pattern},{ns:.4}");
            }
        }
    }
    if "upsample".starts_with(&prefix) {
        bench_upsample::<{ yuv::LAYOUT_ARGB }>(&yuv);
        bench_upsample::<{ yuv::LAYOUT_RGBA }>(&yuv);
        bench_upsample::<{ yuv::LAYOUT_BGRA }>(&yuv);
        bench_upsample::<{ yuv::LAYOUT_RGB }>(&yuv);
        bench_upsample::<{ yuv::LAYOUT_BGR }>(&yuv);
    }
    let mut pixels = vec![0u32; 16384];
    let above = vec![17u8; 8192];
    for n in [
        1, 3, 4, 7, 8, 15, 16, 17, 31, 32, 33, 63, 64, 65, 127, 128, 161, 255, 256,
        509, 512, 1024, 4096,
    ] {
        for k in [0, 1, 2, 3, 4, 8, 9, 11, 12, 13] {
            if !format!("pred_{k}").starts_with(&prefix) {
                continue;
            }
            let f = black_box(loss.pred_add[k]);
            let ns = bench(|| f(black_box(&mut pixels), 8192, 1, black_box(n)));
            println!("pred_{k},{n},0,{ns:.4}");
        }
        for (name, f) in [("pack_rgba", yuv.pack_rgba), ("pack_bgra", yuv.pack_bgra)] {
            if name.starts_with(&prefix) {
                let source: Vec<_> =
                    (0..4 * n).map(|i| i.wrapping_mul(123) as u8).collect();
                let f = black_box(f);
                let ns =
                    bench(|| f(black_box(&mut plane[..4 * n]), black_box(&source)));
                println!("{name},{n},0,{ns:.4}");
            }
        }
        for pattern in 0..4 {
            let mut state = 1u32;
            let alpha: Vec<_> = (0..n)
                .map(|i| {
                    state = state.wrapping_mul(1103515245).wrapping_add(12345);
                    match pattern {
                        1 => 0,
                        2 => 255,
                        3 => {
                            if i % 2 == 0 {
                                0
                            } else {
                                255
                            }
                        }
                        _ => (state >> 24) as u8,
                    }
                })
                .collect();
            if "blend_argb".starts_with(&prefix) {
                let source: Vec<_> =
                    alpha.iter().flat_map(|&a| [a, 87, 123, 200]).collect();
                let f = black_box(loss.blend_row_argb);
                let ns =
                    bench(|| f(black_box(&mut plane[..4 * n]), black_box(&source)));
                println!("blend_argb,{n},{pattern},{ns:.4}");
            }
            for (name, inverse) in [("multiply", false), ("unmultiply", true)] {
                if name.starts_with(&prefix) {
                    let f = black_box(yuv.multiply_row);
                    let ns = bench(|| {
                        f(black_box(&mut plane[..n]), black_box(&alpha), inverse)
                    });
                    println!("{name},{n},{pattern},{ns:.4}");
                }
            }
            for (name, inverse) in
                [("premultiply_argb", false), ("unpremultiply_argb", true)]
            {
                if name.starts_with(&prefix) {
                    let mut argb: Vec<_> =
                        alpha.iter().flat_map(|&a| [a, 87, 123, 200]).collect();
                    let f = black_box(yuv.premultiply_argb_row);
                    let ns = bench(|| f(black_box(&mut argb), inverse));
                    println!("{name},{n},{pattern},{ns:.4}");
                }
            }
        }
        if !"vertical".starts_with(&prefix) {
            continue;
        }
        let f = black_box(filter.vertical_unfilter);
        let ns = bench(|| f(Some(black_box(&above[..n])), black_box(&mut plane[..n])));
        println!("vertical,{n},0,{ns:.4}");
    }
}

fn bench_upsample<const L: usize>(dsp: &YuvDsp) {
    for n in [1usize, 3, 16, 31, 32, 33, 64, 161, 512, 4096] {
        let mut state = 1u32;
        let planes = [(); 6].map(|_| {
            (0..n)
                .map(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    (state >> 24) as u8
                })
                .collect::<Vec<_>>()
        });
        let [ty, by, tu, tv, cu, cv] = &planes;
        let mut top = vec![0u8; yuv::bpp(L) * n];
        let mut bottom = top.clone();
        for both in [false, true] {
            let src = UpsampleSrc {
                top_y: ty,
                bottom_y: both.then_some(by.as_slice()),
                top_u: tu,
                top_v: tv,
                cur_u: cu,
                cur_v: cv,
            };
            let f = black_box(yuv::upsample_row::<L>);
            let ns = bench(|| {
                let mut dst = UpsampleDst {
                    top: &mut top,
                    bottom: if both { Some(&mut bottom) } else { None },
                };
                f(
                    black_box(dsp),
                    black_box(&src),
                    black_box(&mut dst),
                    black_box(n),
                );
            });
            println!("upsample_{L},{n},{},{ns:.4}", u8::from(both));
        }
    }
}
