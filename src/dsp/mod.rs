pub mod filters;
pub mod rescale;
pub mod vp8;
pub mod vp8l;
pub mod vp8pred;
pub mod yuv;

#[inline(always)]
pub(crate) const fn clip_uint8(v: i32) -> u8 {
    let lo = if v < 0 { 0 } else { v };

    (if lo > 255 { 255 } else { lo }) as u8
}

const fn reciprocal_table(numerator: u32) -> [u32; 256] {
    let mut table = [0; 256];
    let mut a = 1;
    while a < table.len() {
        table[a] = numerator / a as u32;
        a += 1;
    }
    table
}
