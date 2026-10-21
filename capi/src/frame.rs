use std::cell::Cell;
use std::ffi::{c_int, c_void};
use std::rc::Rc;
use std::{mem, ptr, slice};

use wpd::handout::{Handout, Pixels, RowSink};
use wpd::image::external_plane_fits;
use wpd::picture::Frame;

const WPD_DISPOSE_BACKGROUND: c_int = 1;
const WPD_DISPOSE_NONE: c_int = 0;
const WPD_BLEND_ALPHA: c_int = 0;
const WPD_BLEND_NONE: c_int = 1;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WPDOutputPlane {
    pub data: *mut u8,
    pub size: usize,
    pub stride: isize,
}

impl WPDOutputPlane {
    pub(crate) fn empty() -> Self {
        WPDOutputPlane {
            data: ptr::null_mut(),
            size: 0,
            stride: 0,
        }
    }
}

#[derive(Default)]
pub struct SinkInput {
    pub range: Cell<(usize, usize)>,
    pub overlap: Cell<bool>,
}

/// Caller-owned output planes, written row by row.
pub struct External {
    planes: [WPDOutputPlane; 4],
    input: Rc<SinkInput>,
    /* The row length and count `fits` last accepted for each plane: all
     * `row` will hand out of it. */
    vetted: [Cell<(usize, i32)>; 4],
}

impl External {
    /// # Safety
    ///
    /// Every plane with non-null `data` must meet the `WPDOutputPlane`
    /// contract in `wpd.h`: `size` bytes reachable from `data` in the
    /// direction of `stride`, writable, and not otherwise accessed for as
    /// long as the sink lives.
    pub unsafe fn new(planes: [WPDOutputPlane; 4], input: Rc<SinkInput>) -> Self {
        External {
            planes,
            input,
            vetted: Default::default(),
        }
    }

    fn vet(&self, p: usize, row_len: usize, rows: i32) -> bool {
        let plane = &self.planes[p];

        if plane.data.is_null()
            || plane.stride == 0
            || plane.size > isize::MAX as usize
            || !external_plane_fits(plane.size, plane.stride, row_len, rows)
        {
            return false;
        }
        let Some(offset) = (rows as usize)
            .saturating_sub(1)
            .checked_mul(plane.stride.unsigned_abs())
        else {
            return false;
        };
        let data = plane.data as usize;
        let (start, end) = if plane.stride < 0 {
            (data.checked_sub(offset), data.checked_add(row_len))
        } else {
            (
                Some(data),
                data.checked_add(offset)
                    .and_then(|end| end.checked_add(row_len)),
            )
        };
        let (Some(start), Some(end)) = (start, end) else {
            return false;
        };
        let (input_start, input_end) = self.input.range.get();
        if input_start < input_end && start < input_end && input_start < end {
            self.input.overlap.set(true);
            return false;
        }
        true
    }
}

impl RowSink for External {
    fn fits(&self, p: usize, row_len: usize, rows: i32) -> bool {
        let ok = self.vet(p, row_len, rows);

        self.vetted[p].set(if ok { (row_len, rows) } else { (0, 0) });
        ok
    }

    fn row(&mut self, p: usize, y: i32, len: usize) -> &mut [u8] {
        let plane = &self.planes[p];
        let (row_len, rows) = self.vetted[p].get();

        assert!(
            y >= 0 && y < rows && len <= row_len,
            "row outside the checked plane"
        );
        // `fits` bounded this product by a size that itself fits an isize.
        let offset = y as isize * plane.stride;

        // SAFETY: `new`'s contract makes the plane `size` writable bytes from
        // `data` along `stride`, and `fits` checked that `rows` rows of
        // `row_len` lie inside them and clear of borrowed input. The assert
        // keeps this row within what `fits` accepted.
        unsafe { slice::from_raw_parts_mut(plane.data.offset(offset), len) }
    }
}

#[repr(C)]
pub struct WPDFrame {
    pub struct_size: usize,
    pub data: [*const u8; 4],
    pub stride: [isize; 4],
    pub width: c_int,
    pub height: c_int,
    pub format: c_int,
    pub duration: c_int,
    pub timestamp: i64,
    pub private_data: *mut c_void,
    pub pos_x: c_int,
    pub pos_y: c_int,
    pub dispose: c_int,
    pub blend: c_int,
    pub has_alpha: c_int,
}

fn has_alpha_extent() -> usize {
    mem::offset_of!(WPDFrame, has_alpha) + mem::size_of::<c_int>()
}

pub(crate) fn private_data_extent() -> usize {
    mem::offset_of!(WPDFrame, private_data) + mem::size_of::<*mut c_void>()
}

pub(crate) fn frame_head() -> usize {
    mem::offset_of!(WPDFrame, data)
}

#[cfg(any(test, feature = "checkasm"))]
pub(crate) fn plane_extent(
    stride: isize,
    rows: usize,
    row_len: usize,
) -> Option<usize> {
    let stride = usize::try_from(stride).ok()?;

    if rows == 0 || row_len == 0 || stride < row_len {
        return None;
    }
    (rows - 1)
        .checked_mul(stride)?
        .checked_add(row_len)
        .filter(|&size| size <= isize::MAX as usize)
}

pub(crate) unsafe fn frame_valid(frame: *const WPDFrame) -> bool {
    // SAFETY: `frame` is non-null here, and the caller guarantees it points
    // to a `WPDFrame` whose leading `struct_size` is readable.
    !frame.is_null()
        && unsafe { ptr::addr_of!((*frame).struct_size).read() }
            >= private_data_extent()
}

pub(crate) unsafe fn frame_extent(frame: *const WPDFrame) -> usize {
    // SAFETY: the caller guarantees `frame` is non-null and readable at least
    // through `struct_size`.
    if unsafe { ptr::addr_of!((*frame).struct_size).read() } >= has_alpha_extent() {
        has_alpha_extent()
    } else {
        private_data_extent()
    }
}

pub(crate) unsafe fn frame_clear(frame: *mut WPDFrame) {
    let head = frame_head();
    // SAFETY: the caller guarantees `frame` passed `frame_valid`.
    let extent = unsafe { frame_extent(frame) };

    // SAFETY: `frame_extent` never exceeds the caller's `struct_size`, which
    // the caller guarantees is writable, and is at least `head`.
    unsafe { ptr::write_bytes(frame.cast::<u8>().add(head), 0, extent - head) };
}

pub(crate) unsafe fn write_frame(
    handout: &Handout<'_>,
    ext: &[WPDOutputPlane; 4],
    frame: *mut WPDFrame,
) {
    // SAFETY: the caller guarantees `frame` passed `frame_valid` and is
    // writable through its `struct_size`.
    unsafe { frame_clear(frame) };

    let planes = handout.planes();

    match &handout.pixels {
        Pixels::Own(img) => {
            for p in 0..planes {
                let (data, stride) = handout_plane(img, p);

                // SAFETY: `p` < `planes` <= 4 indexes the `data` and `stride`
                // arrays, which lie inside any frame `frame_valid` accepts.
                unsafe {
                    ptr::addr_of_mut!((*frame).data)
                        .cast::<*const u8>()
                        .add(p)
                        .write(data);
                    ptr::addr_of_mut!((*frame).stride)
                        .cast::<isize>()
                        .add(p)
                        .write(stride);
                }
            }
        }
        Pixels::Sink => {
            for (p, plane) in ext.iter().enumerate() {
                // SAFETY: `p` < 4 indexes the `data` and `stride` arrays,
                // which lie inside any frame `frame_valid` accepts.
                unsafe {
                    ptr::addr_of_mut!((*frame).data)
                        .cast::<*const u8>()
                        .add(p)
                        .write(if p < planes { plane.data } else { ptr::null() });
                    ptr::addr_of_mut!((*frame).stride)
                        .cast::<isize>()
                        .add(p)
                        .write(if p < planes { plane.stride } else { 0 });
                }
            }
        }
        Pixels::None => {}
    }
    // SAFETY: these fields all precede `private_data`, so they lie within the
    // extent `frame_valid` guaranteed.
    unsafe {
        ptr::addr_of_mut!((*frame).width).write(handout.width);
        ptr::addr_of_mut!((*frame).height).write(handout.height);
        ptr::addr_of_mut!((*frame).format).write(handout.format as c_int);
        ptr::addr_of_mut!((*frame).duration).write(handout.duration);
        ptr::addr_of_mut!((*frame).timestamp).write(handout.timestamp);
    }
    // SAFETY: `frame` passed `frame_valid`, per the caller.
    if unsafe { frame_extent(frame) } < has_alpha_extent() {
        return;
    }
    // SAFETY: the caller's `struct_size` reaches `has_alpha` (checked just
    // above), so every field written here lies inside its frame.
    unsafe {
        ptr::addr_of_mut!((*frame).pos_x).write(handout.pos_x);
        ptr::addr_of_mut!((*frame).pos_y).write(handout.pos_y);
        ptr::addr_of_mut!((*frame).dispose).write(if handout.dispose_to_background {
            WPD_DISPOSE_BACKGROUND
        } else {
            WPD_DISPOSE_NONE
        });
        ptr::addr_of_mut!((*frame).blend).write(if handout.no_blend {
            WPD_BLEND_NONE
        } else {
            WPD_BLEND_ALPHA
        });
        ptr::addr_of_mut!((*frame).has_alpha).write(c_int::from(handout.has_alpha));
    }
}

pub(crate) unsafe fn frame_private_data(frame: *const WPDFrame) -> *mut c_void {
    // SAFETY: the caller guarantees `frame` passed `frame_valid`, so its
    // storage reaches past `private_data`.
    unsafe { ptr::addr_of!((*frame).private_data).read() }
}

pub(crate) unsafe fn frame_set_private_data(frame: *mut WPDFrame, owner: *mut c_void) {
    // SAFETY: the caller guarantees `frame` passed `frame_valid`, so its
    // writable storage reaches past `private_data`.
    unsafe { ptr::addr_of_mut!((*frame).private_data).write(owner) };
}

pub(crate) unsafe fn frame_set_plane(
    frame: *mut WPDFrame,
    p: usize,
    data: *const u8,
    stride: isize,
) {
    // SAFETY: the caller guarantees `frame` passed `frame_valid` and `p` < 4,
    // so both writes land inside the `data` and `stride` arrays.
    unsafe {
        ptr::addr_of_mut!((*frame).data)
            .cast::<*const u8>()
            .add(p)
            .write(data);
        ptr::addr_of_mut!((*frame).stride)
            .cast::<isize>()
            .add(p)
            .write(stride);
    }
}

fn handout_plane(img: &Frame<'_>, p: usize) -> (*const u8, isize) {
    if img.plane[p].is_empty() {
        return (ptr::null(), 0);
    }
    let stride = img.plane[p].stride() as isize;

    (
        img.row(p, 0).as_ptr(),
        if img.flip { -stride } else { stride },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_stride_checks_the_lowest_output_row_for_overlap() {
        let mut storage = [0u8; 64];
        let base = storage.as_mut_ptr() as usize;
        let input = Rc::new(SinkInput::default());
        input.range.set((base, base + 8));
        let mut planes = [WPDOutputPlane::empty(); 4];
        planes[0] = WPDOutputPlane {
            data: storage.as_mut_ptr().wrapping_add(32),
            size: 48,
            stride: -16,
        };
        // SAFETY: the plane lies inside `storage`, which outlives the sink.
        let sink = unsafe { External::new(planes, Rc::clone(&input)) };
        assert!(!sink.fits(0, 8, 3));
        assert!(input.overlap.get());
        input.range.set((base + 40, base + 48));
        input.overlap.set(false);
        assert!(sink.fits(0, 8, 3));
        assert!(!input.overlap.get());
    }

    #[repr(C)]
    struct LegacyFrame {
        struct_size: usize,
        data: [*const u8; 4],
        stride: [isize; 4],
        width: c_int,
        height: c_int,
        format: c_int,
        duration: c_int,
        timestamp: i64,
        private_data: *mut c_void,
    }

    #[test]
    fn legacy_frame_storage_is_only_accessed_through_its_extent() {
        assert_eq!(mem::size_of::<LegacyFrame>(), private_data_extent());
        let mut frame = LegacyFrame {
            struct_size: mem::size_of::<LegacyFrame>(),
            data: [std::ptr::NonNull::<u8>::dangling().as_ptr().cast_const(); 4],
            stride: [7; 4],
            width: 11,
            height: 13,
            format: 2,
            duration: 17,
            timestamp: 19,
            private_data: ptr::null_mut(),
        };
        let frame = (&mut frame as *mut LegacyFrame).cast::<WPDFrame>();

        // SAFETY: `frame` points to a live `LegacyFrame`, which begins with
        // `struct_size` like a `WPDFrame`.
        assert!(unsafe { frame_valid(frame) });
        let handout = Handout {
            width: 23,
            height: 29,
            duration: 31,
            timestamp: 37,
            ..Handout::default()
        };
        // SAFETY: `frame` passed `frame_valid`, and its `struct_size` is the
        // full `LegacyFrame`, which `write_frame` must not write past.
        unsafe { write_frame(&handout, &[WPDOutputPlane::empty(); 4], frame) };
        // SAFETY: `width`, `height` and `private_data` share their offsets with
        // `LegacyFrame`'s, which is live and initialised.
        assert_eq!(unsafe { ptr::addr_of!((*frame).width).read() }, 23);
        // SAFETY: as above.
        assert_eq!(unsafe { ptr::addr_of!((*frame).height).read() }, 29);
        // SAFETY: as above.
        assert!(unsafe { frame_private_data(frame) }.is_null());
    }

    #[test]
    fn plane_extents_reject_invalid_or_unaddressable_geometry() {
        assert_eq!(plane_extent(8, 3, 4), Some(20));
        assert_eq!(plane_extent(3, 3, 4), None);
        assert_eq!(plane_extent(-8, 3, 4), None);
        assert_eq!(plane_extent(8, 0, 4), None);
        assert_eq!(plane_extent(8, 3, 0), None);
        assert_eq!(plane_extent(isize::MAX, 2, 1), None);
    }
}
