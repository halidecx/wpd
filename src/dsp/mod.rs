pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;

#[inline(always)]
pub(crate) const fn clip_uint8(v: i32) -> u8 {
    let lo = if v < 0 { 0 } else { v };

    (if lo > 255 { 255 } else { lo }) as u8
}
