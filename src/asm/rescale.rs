use std::ffi::c_int;

use crate::cpu::CpuFlags;
use crate::dsp::rescale::{
    self as scalar, ExportExpand, ExportShrink, Import, RescaleDsp,
};

pub(crate) use super::Raw;

pub type ExportDirectRaw = unsafe extern "C" fn(*mut u8, *const u32, c_int, u32);
pub type ExportBlendRaw =
    unsafe extern "C" fn(*mut u8, *const u32, *const u32, c_int, u32, u32, u32);
pub type ExportShrinkRaw =
    unsafe extern "C" fn(*mut u8, *mut u32, *const u32, c_int, u32, u32);
pub type ExportShrink0Raw = unsafe extern "C" fn(*mut u8, *mut u32, c_int, u32);
pub type ImportExpandRaw =
    unsafe extern "C" fn(*mut u32, *const u8, c_int, c_int, c_int, c_int, c_int);
pub type ImportShrinkRaw =
    unsafe extern "C" fn(*mut u32, *const u8, c_int, c_int, c_int, u32);

/* LUMA marks the kernels that also carry a single-channel path; the rest
 * only pay off where four channels share one accumulator. */
fn import_row_expand<T: Raw<Sig = ImportExpandRaw>, const LUMA: bool>(
    frow: &mut [u32],
    src: &[u8],
    p: Import,
) {
    let n = p.dst_width * p.num_channels;

    /* The blends take unsigned 16-bit weights, and the sliding windows
     * want a run of at least eight source pixels. */
    if p.src_width < 8
        || p.x_add >= 1 << 15
        || !(p.num_channels == 4 || (LUMA && p.num_channels == 1))
    {
        return scalar::import_row_expand(frow, src, p);
    }
    assert!(
        frow.len() >= n && src.len() >= p.src_width * p.num_channels,
        "short rescaler row"
    );
    unsafe {
        (T::F)(
            frow.as_mut_ptr(),
            src.as_ptr(),
            n as c_int,
            (p.src_width * p.num_channels) as c_int,
            p.num_channels as c_int,
            p.x_add as c_int,
            p.x_sub as c_int,
        )
    }
}

fn import_row_shrink<T: Raw<Sig = ImportShrinkRaw>>(
    frow: &mut [u32],
    src: &[u8],
    p: Import,
) {
    let n = p.dst_width * p.num_channels;

    /* x_sub is a 16-bit multiplier lane, and sum * x_sub must stay inside
     * sixteen unsigned bits too: a 1/128 ratio. */
    if p.num_channels != 4 || p.x_sub >= 1 << 16 || p.x_add > p.x_sub << 7 {
        return scalar::import_row_shrink(frow, src, p);
    }
    assert!(
        frow.len() >= n && src.len() >= p.src_width * p.num_channels,
        "short rescaler row"
    );
    unsafe {
        (T::F)(
            frow.as_mut_ptr(),
            src.as_ptr(),
            n as c_int,
            p.x_add as c_int,
            p.x_sub as c_int,
            p.fx_scale,
        )
    }
}

fn export_row_expand<D, B>(dst: &mut [u8], irow: &[u32], frow: &[u32], p: ExportExpand)
where
    D: Raw<Sig = ExportDirectRaw>,
    B: Raw<Sig = ExportBlendRaw>,
{
    let n = dst.len();

    assert!(frow.len() >= n, "short rescaler row");
    if let Some((a, b)) = p.blend() {
        assert!(irow.len() >= n, "short rescaler row");
        unsafe {
            (B::F)(
                dst.as_mut_ptr(),
                irow.as_ptr(),
                frow.as_ptr(),
                n as c_int,
                p.fy_scale,
                a,
                b,
            )
        }
    } else {
        unsafe { (D::F)(dst.as_mut_ptr(), frow.as_ptr(), n as c_int, p.fy_scale) }
    }
}

fn export_row_shrink<S, Z>(
    dst: &mut [u8],
    irow: &mut [u32],
    frow: &[u32],
    p: ExportShrink,
) where
    S: Raw<Sig = ExportShrinkRaw>,
    Z: Raw<Sig = ExportShrink0Raw>,
{
    let n = dst.len();
    let yscale = p.yscale();

    assert!(irow.len() >= n, "short rescaler row");
    if yscale != 0 {
        assert!(frow.len() >= n, "short rescaler row");
        unsafe {
            (S::F)(
                dst.as_mut_ptr(),
                irow.as_mut_ptr(),
                frow.as_ptr(),
                n as c_int,
                yscale,
                p.fxy_scale,
            )
        }
    } else {
        unsafe { (Z::F)(dst.as_mut_ptr(), irow.as_mut_ptr(), n as c_int, p.fxy_scale) }
    }
}

/* The wrappers above are safe Rust fns, so the C bindings cannot hand them
 * out the way the other modules hand out raw symbols. They get told which
 * kernel the dispatch below picked instead, and wrap that one themselves,
 * which keeps the choice in one place. */
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Kernel {
    #[default]
    Scalar,
    Sse2,
    Avx2,
    Neon,
}

#[derive(Clone, Copy, Default)]
pub struct Selection {
    pub import_row_expand: Kernel,
    pub import_row_shrink: Kernel,
    pub export_row_expand: Kernel,
    pub export_row_shrink: Kernel,
}

/* One description of the ladder, read out as both the dispatch table and
 * the selection: they cannot drift apart. */
macro_rules! ladder {
    ($(
        $flag:ident => $kernel:ident {
            $( $field:ident = $wrap:ident; )*
        }
    )*) => {
        pub fn init(dsp: &mut RescaleDsp, flags: CpuFlags) {
            $(
                if flags.contains(CpuFlags::$flag) {
                    $( dsp.$field = $wrap; )*
                }
            )*
        }

        pub fn selection(flags: CpuFlags) -> Selection {
            let mut s = Selection::default();

            $(
                if flags.contains(CpuFlags::$flag) {
                    $( s.$field = Kernel::$kernel; )*
                }
            )*
            s
        }
    };
}

#[cfg(target_arch = "x86_64")]
mod arch {
    use super::*;

    pub mod sse2 {
        use super::*;

        raw!(
            ExportDirect,
            export_direct,
            ExportDirectRaw,
            "ff_rescale_export_direct_sse2",
            (*mut u8, *const u32, c_int, u32)
        );
        raw!(
            ExportBlend,
            export_blend,
            ExportBlendRaw,
            "ff_rescale_export_blend_sse2",
            (*mut u8, *const u32, *const u32, c_int, u32, u32, u32)
        );
        raw!(
            ExportShrink,
            export_shrink,
            ExportShrinkRaw,
            "ff_rescale_export_shrink_sse2",
            (*mut u8, *mut u32, *const u32, c_int, u32, u32)
        );
        raw!(
            ExportShrink0,
            export_shrink0,
            ExportShrink0Raw,
            "ff_rescale_export_shrink0_sse2",
            (*mut u8, *mut u32, c_int, u32)
        );
        raw!(
            ImportExpand,
            import_expand,
            ImportExpandRaw,
            "ff_rescale_import_expand_sse2",
            (*mut u32, *const u8, c_int, c_int, c_int, c_int, c_int)
        );
        raw!(
            ImportShrink,
            import_shrink,
            ImportShrinkRaw,
            "ff_rescale_import_shrink_sse2",
            (*mut u32, *const u8, c_int, c_int, c_int, u32)
        );
    }

    pub mod avx2 {
        use super::*;

        raw!(
            ExportDirect,
            export_direct,
            ExportDirectRaw,
            "ff_rescale_export_direct_avx2",
            (*mut u8, *const u32, c_int, u32)
        );
        raw!(
            ExportBlend,
            export_blend,
            ExportBlendRaw,
            "ff_rescale_export_blend_avx2",
            (*mut u8, *const u32, *const u32, c_int, u32, u32, u32)
        );
        raw!(
            ExportShrink,
            export_shrink,
            ExportShrinkRaw,
            "ff_rescale_export_shrink_avx2",
            (*mut u8, *mut u32, *const u32, c_int, u32, u32)
        );
        raw!(
            ExportShrink0,
            export_shrink0,
            ExportShrink0Raw,
            "ff_rescale_export_shrink0_avx2",
            (*mut u8, *mut u32, c_int, u32)
        );
    }

    pub fn import_row_expand_sse2(frow: &mut [u32], src: &[u8], p: Import) {
        import_row_expand::<sse2::ImportExpand, true>(frow, src, p)
    }

    pub fn import_row_shrink_sse2(frow: &mut [u32], src: &[u8], p: Import) {
        import_row_shrink::<sse2::ImportShrink>(frow, src, p)
    }

    pub fn export_row_expand_sse2(
        dst: &mut [u8],
        irow: &[u32],
        frow: &[u32],
        p: ExportExpand,
    ) {
        export_row_expand::<sse2::ExportDirect, sse2::ExportBlend>(dst, irow, frow, p)
    }

    pub fn export_row_shrink_sse2(
        dst: &mut [u8],
        irow: &mut [u32],
        frow: &[u32],
        p: ExportShrink,
    ) {
        export_row_shrink::<sse2::ExportShrink, sse2::ExportShrink0>(dst, irow, frow, p)
    }

    pub fn export_row_expand_avx2(
        dst: &mut [u8],
        irow: &[u32],
        frow: &[u32],
        p: ExportExpand,
    ) {
        export_row_expand::<avx2::ExportDirect, avx2::ExportBlend>(dst, irow, frow, p)
    }

    pub fn export_row_shrink_avx2(
        dst: &mut [u8],
        irow: &mut [u32],
        frow: &[u32],
        p: ExportShrink,
    ) {
        export_row_shrink::<avx2::ExportShrink, avx2::ExportShrink0>(dst, irow, frow, p)
    }

    ladder! {
        SSE2 => Sse2 {
            import_row_expand = import_row_expand_sse2;
            import_row_shrink = import_row_shrink_sse2;
            export_row_expand = export_row_expand_sse2;
            export_row_shrink = export_row_shrink_sse2;
        }
        AVX2 => Avx2 {
            export_row_expand = export_row_expand_avx2;
            export_row_shrink = export_row_shrink_avx2;
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arch {
    use super::*;

    pub mod neon {
        use super::*;

        raw!(
            ExportDirect,
            export_direct,
            ExportDirectRaw,
            "ff_rescale_export_direct_neon",
            (*mut u8, *const u32, c_int, u32)
        );
        raw!(
            ExportBlend,
            export_blend,
            ExportBlendRaw,
            "ff_rescale_export_blend_neon",
            (*mut u8, *const u32, *const u32, c_int, u32, u32, u32)
        );
        raw!(
            ExportShrink,
            export_shrink,
            ExportShrinkRaw,
            "ff_rescale_export_shrink_neon",
            (*mut u8, *mut u32, *const u32, c_int, u32, u32)
        );
        raw!(
            ExportShrink0,
            export_shrink0,
            ExportShrink0Raw,
            "ff_rescale_export_shrink0_neon",
            (*mut u8, *mut u32, c_int, u32)
        );
        raw!(
            ImportExpand,
            import_expand,
            ImportExpandRaw,
            "ff_rescale_import_expand_neon",
            (*mut u32, *const u8, c_int, c_int, c_int, c_int, c_int)
        );
        raw!(
            ImportShrink,
            import_shrink,
            ImportShrinkRaw,
            "ff_rescale_import_shrink_neon",
            (*mut u32, *const u8, c_int, c_int, c_int, u32)
        );
    }

    pub fn import_row_expand_neon(frow: &mut [u32], src: &[u8], p: Import) {
        import_row_expand::<neon::ImportExpand, false>(frow, src, p)
    }

    pub fn import_row_shrink_neon(frow: &mut [u32], src: &[u8], p: Import) {
        import_row_shrink::<neon::ImportShrink>(frow, src, p)
    }

    pub fn export_row_expand_neon(
        dst: &mut [u8],
        irow: &[u32],
        frow: &[u32],
        p: ExportExpand,
    ) {
        export_row_expand::<neon::ExportDirect, neon::ExportBlend>(dst, irow, frow, p)
    }

    pub fn export_row_shrink_neon(
        dst: &mut [u8],
        irow: &mut [u32],
        frow: &[u32],
        p: ExportShrink,
    ) {
        export_row_shrink::<neon::ExportShrink, neon::ExportShrink0>(dst, irow, frow, p)
    }

    ladder! {
        NEON => Neon {
            import_row_expand = import_row_expand_neon;
            import_row_shrink = import_row_shrink_neon;
            export_row_expand = export_row_expand_neon;
            export_row_shrink = export_row_shrink_neon;
        }
    }
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
mod arch {
    use super::*;

    pub fn init(_dsp: &mut RescaleDsp, _flags: CpuFlags) {}

    pub fn selection(_flags: CpuFlags) -> Selection {
        Selection::default()
    }
}

pub use arch::*;
