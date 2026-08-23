use crate::dsp::yuv::{
    bpp, upsample_row, yuv420_row, yuv444_row, UpsampleDst, UpsampleSrc, YuvDsp,
    LAYOUT_ARGB, LAYOUT_BGR, LAYOUT_BGRA, LAYOUT_RGB, LAYOUT_RGBA,
};
use crate::dsp::yuv::{extract_alpha, RowFn};
use crate::picture::{PlaneMut, PlaneRef};

pub struct YuvPlanes<'a> {
    pub y: PlaneRef<'a>,
    pub u: PlaneRef<'a>,
    pub v: PlaneRef<'a>,
    pub a: Option<PlaneRef<'a>>,
}

const fn first_pair(row_start: usize) -> usize {
    if row_start != 0 {
        row_start.div_ceil(2)
    } else {
        1
    }
}

const fn first_row(row_start: usize) -> usize {
    if row_start != 0 {
        2 * first_pair(row_start) - 1
    } else {
        0
    }
}

fn lone_row<'a, const L: usize>(
    dsp: &YuvDsp,
    dst: &mut PlaneMut<'_>,
    y: &'a [u8],
    u: &'a [u8],
    v: &'a [u8],
    row: i32,
    width: usize,
) {
    let src = UpsampleSrc {
        top_y: y,
        bottom_y: None,
        top_u: u,
        top_v: v,
        cur_u: u,
        cur_v: v,
    };
    let mut out = UpsampleDst {
        top: dst.row_mut(row, 0, bpp(L) * width),
        bottom: None,
    };

    upsample_row::<L>(dsp, &src, &mut out, width);
}

fn upsample_rows<const L: usize>(
    dsp: &YuvDsp,
    dst: &mut PlaneMut<'_>,
    src: &YuvPlanes<'_>,
    width: usize,
    height: usize,
    row_start: usize,
    row_end: usize,
) {
    let chroma = width.div_ceil(2);

    fn row<'a>(p: &PlaneRef<'a>, y: usize, len: usize) -> &'a [u8] {
        p.row(y as i32, 0, len)
    }

    if row_start == 0 {
        lone_row::<L>(
            dsp,
            dst,
            row(&src.y, 0, width),
            row(&src.u, 0, chroma),
            row(&src.v, 0, chroma),
            0,
            width,
        );
    }

    let mut j = first_pair(row_start);

    while 2 * j < row_end {
        let up = UpsampleSrc {
            top_y: row(&src.y, 2 * j - 1, width),
            bottom_y: Some(row(&src.y, 2 * j, width)),
            top_u: row(&src.u, j - 1, chroma),
            top_v: row(&src.v, j - 1, chroma),
            cur_u: row(&src.u, j, chroma),
            cur_v: row(&src.v, j, chroma),
        };
        let (top, bottom) =
            dst.row_pair_mut((2 * j - 1) as i32, 2 * j as i32, 0, bpp(L) * width);
        let mut out = UpsampleDst {
            top,
            bottom: Some(bottom),
        };

        upsample_row::<L>(dsp, &up, &mut out, width);
        j += 1;
    }

    if height % 2 == 0 && row_end == height {
        let last = height.div_ceil(2) - 1;

        lone_row::<L>(
            dsp,
            dst,
            row(&src.y, height - 1, width),
            row(&src.u, last, chroma),
            row(&src.v, last, chroma),
            (height - 1) as i32,
            width,
        );
    }
}

macro_rules! by_layout {
    ($layout:expr, $run:ident) => {
        match $layout {
            LAYOUT_RGBA => $run!(LAYOUT_RGBA),
            LAYOUT_BGRA => $run!(LAYOUT_BGRA),
            LAYOUT_RGB => $run!(LAYOUT_RGB),
            LAYOUT_BGR => $run!(LAYOUT_BGR),
            _ => $run!(LAYOUT_ARGB),
        }
    };
}

fn dispatch_alpha_rows(
    dispatch: RowFn,
    dst: &mut PlaneMut<'_>,
    alpha: &PlaneRef<'_>,
    width: usize,
    from: usize,
    to: usize,
) {
    for y in from..to {
        dispatch(
            dst.row_mut(y as i32, 0, 4 * width),
            alpha.row(y as i32, 0, width),
        );
    }
}

/// The fewest rows worth putting on a thread of their own. Below this the
/// spawn costs more than the conversion it takes away.
const MIN_BAND_ROWS: usize = 64;

/// The conversion reads three planes and writes up to four bytes a pixel, so
/// it is limited by memory rather than by arithmetic and stops getting faster
/// well before it runs out of threads. Past this many bands the spawns cost
/// more than the rows they take away, and a decode asked for ten threads is
/// slower than one asked for three.
const MAX_BANDS: usize = 3;

/// Cuts `[row_start, row_end)` into one band per thread.
///
/// **Every band but the first has to begin on an odd row.** The fancy
/// upsampler emits an (odd, even) row pair at a time, so a band beginning on
/// an even row also rewrites the row before it, which belongs to the band
/// ahead. With odd starts the bands are disjoint, `first_row()` of a band's
/// start is that start, and the alpha pass over each band covers its own rows
/// and no others, so a split conversion writes exactly what one call would.
fn split_bands<'p>(
    dst: &'p mut PlaneMut<'_>,
    row_start: usize,
    row_end: usize,
    n: usize,
) -> Vec<(PlaneMut<'p>, usize, usize)> {
    let total = row_end - row_start;
    let mut cuts = Vec::with_capacity(n + 1);

    cuts.push(row_start);
    for i in 1..n {
        /* Every raw band is at least MIN_BAND_ROWS deep, so rounding a cut up
         * to the next odd row cannot make one overtake the next. */
        cuts.push((row_start + i * total / n) | 1);
    }
    cuts.push(row_end);

    let mut bands = Vec::with_capacity(n);
    let mut rest = dst.reborrow();

    for i in 0..n {
        let (head, tail) = rest.split_rows_at(cuts[i + 1] as i32);

        bands.push((head, cuts[i], cuts[i + 1]));
        rest = tail;
    }
    bands
}

#[allow(clippy::too_many_arguments)]
pub fn yuv420_to_packed_rows(
    dsp: &YuvDsp,
    layout: usize,
    dst: &mut PlaneMut<'_>,
    src: &YuvPlanes<'_>,
    width: usize,
    height: usize,
    row_start: usize,
    row_end: usize,
    threads: usize,
) -> usize {
    if width == 0 || height == 0 || row_start >= row_end {
        return row_start;
    }

    let n =
        crate::task::pieces(row_end - row_start, MIN_BAND_ROWS, threads.min(MAX_BANDS));

    if n < 2 {
        packed_rows_span(dsp, layout, dst, src, width, height, row_start, row_end);
        return first_row(row_start);
    }

    let mut bands = split_bands(dst, row_start, row_end, n);

    crate::task::for_each(n, &mut bands, |(dst, from, to)| {
        packed_rows_span(dsp, layout, dst, src, width, height, *from, *to);
    });
    first_row(row_start)
}

#[allow(clippy::too_many_arguments)]
fn packed_rows_span(
    dsp: &YuvDsp,
    layout: usize,
    dst: &mut PlaneMut<'_>,
    src: &YuvPlanes<'_>,
    width: usize,
    height: usize,
    row_start: usize,
    row_end: usize,
) {
    let first = first_row(row_start);

    macro_rules! run {
        ($l:expr) => {
            upsample_rows::<$l>(dsp, dst, src, width, height, row_start, row_end)
        };
    }

    by_layout!(layout, run);

    if let (Some(alpha), Some(dispatch)) = (&src.a, dsp.alpha_dispatcher(layout)) {
        dispatch_alpha_rows(dispatch, dst, alpha, width, first, row_end);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn yuv420_to_packed_simple(
    dsp: &YuvDsp,
    layout: usize,
    dst: &mut PlaneMut<'_>,
    src: &YuvPlanes<'_>,
    width: usize,
    row_start: usize,
    row_end: usize,
    threads: usize,
) {
    if width == 0 || row_start >= row_end {
        return;
    }

    let n =
        crate::task::pieces(row_end - row_start, MIN_BAND_ROWS, threads.min(MAX_BANDS));

    if n < 2 {
        packed_simple_span(dsp, layout, dst, src, width, row_start, row_end);
        return;
    }

    let mut bands = split_bands(dst, row_start, row_end, n);

    crate::task::for_each(n, &mut bands, |(dst, from, to)| {
        packed_simple_span(dsp, layout, dst, src, width, *from, *to);
    });
}

fn packed_simple_span(
    dsp: &YuvDsp,
    layout: usize,
    dst: &mut PlaneMut<'_>,
    src: &YuvPlanes<'_>,
    width: usize,
    row_start: usize,
    row_end: usize,
) {
    let chroma = width.div_ceil(2);

    for j in row_start..row_end {
        let y = src.y.row(j as i32, 0, width);
        let u = src.u.row((j >> 1) as i32, 0, chroma);
        let v = src.v.row((j >> 1) as i32, 0, chroma);
        let out = dst.row_mut(j as i32, 0, bpp(layout) * width);

        macro_rules! run {
            ($l:expr) => {
                yuv420_row::<$l>(out, y, u, v)
            };
        }

        by_layout!(layout, run);
    }

    if let (Some(alpha), Some(dispatch)) = (&src.a, dsp.alpha_dispatcher(layout)) {
        dispatch_alpha_rows(dispatch, dst, alpha, width, row_start, row_end);
    }
}

pub fn yuv444_to_packed(
    layout: usize,
    dst: &mut PlaneMut<'_>,
    src: &YuvPlanes<'_>,
    width: usize,
    height: usize,
) {
    for j in 0..height {
        let y = src.y.row(j as i32, 0, width);
        let u = src.u.row(j as i32, 0, width);
        let v = src.v.row(j as i32, 0, width);
        let out = dst.row_mut(j as i32, 0, bpp(layout) * width);

        macro_rules! run {
            ($l:expr) => {
                yuv444_row::<$l>(out, y, u, v)
            };
        }

        by_layout!(layout, run);
    }
}

pub fn argb_to_yuva(
    dsp: &YuvDsp,
    dst: &mut [PlaneMut<'_>; 4],
    argb: &PlaneRef<'_>,
    want_alpha: bool,
    width: usize,
    row_start: i32,
    row_end: i32,
) {
    let chroma = width.div_ceil(2);
    let stride = argb.stride();
    let mut row = row_start;

    while row < row_end {
        let pair = row + 1 < row_end;
        let span = if pair { 4 * width + stride } else { 4 * width };
        let src = argb.row(row, 0, span);
        let [y, u, v, _] = dst;

        (dsp.argb_to_y)(y.row_mut(row, 0, width), &src[..4 * width]);
        if pair {
            (dsp.argb_to_y)(y.row_mut(row + 1, 0, width), &src[stride..]);
        }
        (dsp.argb_to_uv)(
            u.row_mut(row >> 1, 0, chroma),
            v.row_mut(row >> 1, 0, chroma),
            src,
            if pair { stride } else { 0 },
            width,
            want_alpha,
        );
        row += 2;
    }

    if !want_alpha {
        return;
    }
    for row in row_start..row_end {
        extract_alpha(dst[3].row_mut(row, 0, width), argb.row(row, 0, 4 * width));
    }
}

pub fn argb_to_yuv444(
    dsp: &YuvDsp,
    dst: &mut [PlaneMut<'_>; 4],
    argb: &PlaneRef<'_>,
    width: usize,
    height: i32,
) {
    for row in 0..height {
        let [y, u, v, _] = dst;

        (dsp.argb_to_yuv444)(
            y.row_mut(row, 0, width),
            u.row_mut(row, 0, width),
            v.row_mut(row, 0, width),
            argb.row(row, 0, 4 * width),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Format;
    use crate::picture::Buffer;

    #[test]
    fn splitting_the_row_range_gives_the_same_picture() {
        let dsp = YuvDsp::new();
        let (w, h) = (37usize, 11usize);
        let mut src = Buffer::default();

        src.alloc_planar(w as i32, h as i32, true).unwrap();
        for y in 0..h as i32 {
            for (x, p) in src.frame_mut().row(0, y).iter_mut().enumerate() {
                *p = (x as u8).wrapping_mul(7).wrapping_add(y as u8);
            }
        }
        for y in 0..h.div_ceil(2) as i32 {
            src.frame_mut().row(1, y).fill(90);
            src.frame_mut().row(2, y).fill(200);
        }

        fn planes(b: &Buffer) -> YuvPlanes<'_> {
            let f = b.frame();

            YuvPlanes {
                y: f.plane[0],
                u: f.plane[1],
                v: f.plane[2],
                a: None,
            }
        }

        let mut whole = Buffer::default();
        let mut split = Buffer::default();

        whole.alloc_argb(w as i32, h as i32).unwrap();
        split.alloc_argb(w as i32, h as i32).unwrap();

        let f = src.frame();

        yuv420_to_packed_rows(
            &dsp,
            LAYOUT_ARGB,
            &mut whole.frame_mut().planes_mut()[0],
            &planes(&src),
            w,
            h,
            0,
            h,
            1,
        );
        let _ = f;

        let mut at = 0;

        while at < h {
            let end = (at + 3).min(h);

            yuv420_to_packed_rows(
                &dsp,
                LAYOUT_ARGB,
                &mut split.frame_mut().planes_mut()[0],
                &planes(&src),
                w,
                h,
                at,
                end,
                1,
            );
            at = end;
        }

        for y in 0..h as i32 {
            assert_eq!(whole.frame().row(0, y), split.frame().row(0, y), "row {y}");
        }
    }

    #[test]
    fn bands_write_exactly_what_one_pass_writes() {
        let dsp = YuvDsp::new();
        let (w, h) = (37usize, 200usize);
        let mut src = Buffer::default();

        src.alloc_planar(w as i32, h as i32, true).unwrap();
        for y in 0..h as i32 {
            for (x, p) in src.frame_mut().row(0, y).iter_mut().enumerate() {
                *p = (x as u8).wrapping_mul(13).wrapping_add(y as u8);
            }
        }
        for y in 0..h.div_ceil(2) as i32 {
            for (x, p) in src.frame_mut().row(1, y).iter_mut().enumerate() {
                *p = (x as u8).wrapping_mul(3).wrapping_add(y as u8);
            }
            src.frame_mut().row(2, y).fill(200);
        }

        fn planes(b: &Buffer) -> YuvPlanes<'_> {
            let f = b.frame();

            YuvPlanes {
                y: f.plane[0],
                u: f.plane[1],
                v: f.plane[2],
                a: None,
            }
        }

        /* Every start parity, so a band that has to move to the next odd row
         * is covered alongside one that does not. */
        for start in [0usize, 1, 2, 63, 64] {
            let mut whole = Buffer::default();

            whole.alloc_argb(w as i32, h as i32).unwrap();
            yuv420_to_packed_rows(
                &dsp,
                LAYOUT_ARGB,
                &mut whole.frame_mut().planes_mut()[0],
                &planes(&src),
                w,
                h,
                start,
                h,
                1,
            );

            for threads in [2usize, 3, 5, 8] {
                let mut banded = Buffer::default();

                banded.alloc_argb(w as i32, h as i32).unwrap();
                yuv420_to_packed_rows(
                    &dsp,
                    LAYOUT_ARGB,
                    &mut banded.frame_mut().planes_mut()[0],
                    &planes(&src),
                    w,
                    h,
                    start,
                    h,
                    threads,
                );
                for y in first_row(start) as i32..h as i32 {
                    assert_eq!(
                        whole.frame().row(0, y),
                        banded.frame().row(0, y),
                        "row {y} from {start} at {threads} threads"
                    );
                }
            }
        }
    }

    #[test]
    fn a_flat_picture_survives_the_round_trip_to_planar() {
        let dsp = YuvDsp::new();
        let mut argb = Buffer::default();

        argb.alloc_argb(8, 4).unwrap();
        for y in 0..4 {
            for px in argb.frame_mut().row(0, y).chunks_exact_mut(4) {
                px.copy_from_slice(&[255, 128, 128, 128]);
            }
        }

        let mut planar = Buffer::default();

        planar.alloc_planar(8, 4, true).unwrap();

        let src = argb.frame().plane[0];

        argb_to_yuva(&dsp, planar.frame_mut().planes_mut(), &src, true, 8, 0, 4);
        assert_eq!(planar.frame().row(3, 0), [255; 8]);
        for &u in planar.frame().row(1, 0) {
            assert!(u.abs_diff(128) <= 1);
        }

        let mut back = Buffer::default();

        back.alloc_packed(8, 4, 4, Format::Argb).unwrap();

        let f = planar.frame();
        let planes = YuvPlanes {
            y: f.plane[0],
            u: f.plane[1],
            v: f.plane[2],
            a: Some(f.plane[3]),
        };

        yuv420_to_packed_rows(
            &dsp,
            LAYOUT_ARGB,
            &mut back.frame_mut().planes_mut()[0],
            &planes,
            8,
            4,
            0,
            4,
            1,
        );
        for px in back.frame().row(0, 0).chunks_exact(4) {
            assert_eq!(px[0], 255);
            for &c in &px[1..] {
                assert!(c.abs_diff(128) <= 2, "{c}");
            }
        }
    }
}
