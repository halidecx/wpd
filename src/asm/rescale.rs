use std::ffi::c_int;

use crate::cpu::CpuFlags;
use crate::dsp::rescale::{self as scalar, frac, Export, Import, ImportFn, RescaleDsp};

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

fn import_row_expand<T: Raw<Sig = ImportExpandRaw>>(
    frow: &mut [u32],
    src: &[u8],
    p: Import,
) {
    let n = p.dst_width * p.num_channels;

    /* pmaddwd blends with signed 16-bit weights, and the sliding windows
     * want a run of at least eight source pixels. */
    if p.src_width < 8
        || p.x_add >= 1 << 15
        || !(p.num_channels == 1 || p.num_channels == 4)
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

    /* sum * x_sub must stay inside sixteen unsigned bits: a 1/128 ratio. */
    if p.num_channels != 4 || p.x_add > p.x_sub << 7 {
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

fn export_row_expand<D, B>(dst: &mut [u8], irow: &[u32], frow: &[u32], p: Export)
where
    D: Raw<Sig = ExportDirectRaw>,
    B: Raw<Sig = ExportBlendRaw>,
{
    let n = dst.len();

    assert!(frow.len() >= n, "short rescaler row");
    if p.y_accum == 0 {
        unsafe { (D::F)(dst.as_mut_ptr(), frow.as_ptr(), n as c_int, p.fy_scale) }
    } else {
        let b = frac((-p.y_accum) as u32, p.y_sub);
        let a = 0u32.wrapping_sub(b);

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
    }
}

fn export_row_shrink<S, Z>(dst: &mut [u8], irow: &mut [u32], frow: &[u32], p: Export)
where
    S: Raw<Sig = ExportShrinkRaw>,
    Z: Raw<Sig = ExportShrink0Raw>,
{
    let n = dst.len();
    let yscale = p.fy_scale.wrapping_mul((-p.y_accum) as u32);

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

#[derive(Default)]
pub struct RawTable {
    pub import_row_expand: Option<ImportFn>,
    pub import_row_shrink: Option<ImportFn>,
    pub export_row_expand: Option<crate::dsp::rescale::ExportExpandFn>,
    pub export_row_shrink: Option<crate::dsp::rescale::ExportShrinkFn>,
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
        import_row_expand::<sse2::ImportExpand>(frow, src, p)
    }

    pub fn import_row_shrink_sse2(frow: &mut [u32], src: &[u8], p: Import) {
        import_row_shrink::<sse2::ImportShrink>(frow, src, p)
    }

    pub fn export_row_expand_sse2(
        dst: &mut [u8],
        irow: &[u32],
        frow: &[u32],
        p: Export,
    ) {
        export_row_expand::<sse2::ExportDirect, sse2::ExportBlend>(dst, irow, frow, p)
    }

    pub fn export_row_shrink_sse2(
        dst: &mut [u8],
        irow: &mut [u32],
        frow: &[u32],
        p: Export,
    ) {
        export_row_shrink::<sse2::ExportShrink, sse2::ExportShrink0>(dst, irow, frow, p)
    }

    pub fn export_row_expand_avx2(
        dst: &mut [u8],
        irow: &[u32],
        frow: &[u32],
        p: Export,
    ) {
        export_row_expand::<avx2::ExportDirect, avx2::ExportBlend>(dst, irow, frow, p)
    }

    pub fn export_row_shrink_avx2(
        dst: &mut [u8],
        irow: &mut [u32],
        frow: &[u32],
        p: Export,
    ) {
        export_row_shrink::<avx2::ExportShrink, avx2::ExportShrink0>(dst, irow, frow, p)
    }

    pub fn init(dsp: &mut RescaleDsp, flags: CpuFlags) {
        if flags.contains(CpuFlags::SSE2) {
            dsp.import_row_expand = import_row_expand_sse2;
            dsp.import_row_shrink = import_row_shrink_sse2;
            dsp.export_row_expand = export_row_expand_sse2;
            dsp.export_row_shrink = export_row_shrink_sse2;
        }
        if flags.contains(CpuFlags::AVX2) {
            dsp.export_row_expand = export_row_expand_avx2;
            dsp.export_row_shrink = export_row_shrink_avx2;
        }
    }

    pub fn raw_table(flags: CpuFlags) -> RawTable {
        let mut t = RawTable::default();

        if flags.contains(CpuFlags::SSE2) {
            t.import_row_expand = Some(import_row_expand_sse2);
            t.import_row_shrink = Some(import_row_shrink_sse2);
            t.export_row_expand = Some(export_row_expand_sse2);
            t.export_row_shrink = Some(export_row_shrink_sse2);
        }
        if flags.contains(CpuFlags::AVX2) {
            t.export_row_expand = Some(export_row_expand_avx2);
            t.export_row_shrink = Some(export_row_shrink_avx2);
        }
        t
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod arch {
    use super::*;

    pub fn init(_dsp: &mut RescaleDsp, _flags: CpuFlags) {}

    pub fn raw_table(_flags: CpuFlags) -> RawTable {
        RawTable::default()
    }
}

pub use arch::*;
