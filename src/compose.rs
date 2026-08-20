/*! Composing a macroblock-edge loop filter out of single-edge kernels.
 *
 * The assembly table (`crate::asm::vp8`) and the C ABI table
 * (`wpd_capi::dsp::vp8`) both need this over raw pointers, differing only in
 * which single-edge function they hand it, so the composition lives here once
 * rather than in each of them.
 */

#[doc(hidden)]
#[macro_export]
macro_rules! composed_mb_at {
    (vert, $p:expr, $stride:expr, $k:expr) => {
        $p.offset($k * $stride)
    };
    (horiz, $p:expr, $stride:expr, $k:expr) => {
        $p.add($k as usize)
    };
}

/// Emits `unsafe extern "C"` filters that run an edge kernel on the macroblock
/// edge and an inner kernel on the subblock edges 4, 8 and 12 samples in.
#[macro_export]
macro_rules! composed_mb {
    (simple $vis:vis $name:ident, $dir:ident, $edge:expr) => {
        #[allow(clippy::missing_safety_doc)]
        $vis unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_lim: ::std::ffi::c_int,
            bedge_lim: ::std::ffi::c_int,
        ) {
            let f = $edge;

            unsafe {
                f(dst, stride, mbedge_lim);
                f($crate::composed_mb_at!($dir, dst, stride, 4), stride, bedge_lim);
                f($crate::composed_mb_at!($dir, dst, stride, 8), stride, bedge_lim);
                f($crate::composed_mb_at!($dir, dst, stride, 12), stride, bedge_lim);
            }
        }
    };
    (luma $vis:vis $name:ident, $dir:ident, $edge:expr, $inner:expr) => {
        #[allow(clippy::missing_safety_doc)]
        $vis unsafe extern "C" fn $name(
            dst: *mut u8,
            stride: isize,
            mbedge_e: ::std::ffi::c_int,
            bedge_e: ::std::ffi::c_int,
            flim_i: ::std::ffi::c_int,
            hev: ::std::ffi::c_int,
        ) {
            let (edge, inner) = ($edge, $inner);

            unsafe {
                edge(dst, stride, mbedge_e, flim_i, hev);
                for k in [4, 8, 12] {
                    inner(
                        $crate::composed_mb_at!($dir, dst, stride, k),
                        stride,
                        bedge_e,
                        flim_i,
                        hev,
                    );
                }
            }
        }
    };
    (chroma $vis:vis $name:ident, $dir:ident, $edge:expr, $inner:expr) => {
        #[allow(clippy::missing_safety_doc)]
        $vis unsafe extern "C" fn $name(
            dst_u: *mut u8,
            dst_v: *mut u8,
            stride: isize,
            mbedge_e: ::std::ffi::c_int,
            bedge_e: ::std::ffi::c_int,
            flim_i: ::std::ffi::c_int,
            hev: ::std::ffi::c_int,
        ) {
            let (edge, inner) = ($edge, $inner);

            unsafe {
                edge(dst_u, dst_v, stride, mbedge_e, flim_i, hev);
                inner(
                    $crate::composed_mb_at!($dir, dst_u, stride, 4),
                    $crate::composed_mb_at!($dir, dst_v, stride, 4),
                    stride,
                    bedge_e,
                    flim_i,
                    hev,
                );
            }
        }
    };
}
