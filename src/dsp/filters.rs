/* Alpha-plane unfiltering, libwebp's WebPUnfilters. Each call reconstructs
 * one row in place; a missing previous row marks the top row, which every
 * mode left-predicts. */

pub fn horizontal_unfilter(prev: Option<&[u8]>, row: &mut [u8]) {
    let Some((first, rest)) = row.split_first_mut() else {
        return;
    };

    if let Some(prev) = prev {
        *first = first.wrapping_add(prev[0]);
    }

    let mut left = *first;

    for px in rest {
        left = px.wrapping_add(left);
        *px = left;
    }
}

pub fn vertical_unfilter(prev: Option<&[u8]>, row: &mut [u8]) {
    let Some(prev) = prev else {
        return horizontal_unfilter(None, row);
    };

    for (px, &above) in row.iter_mut().zip(prev) {
        *px = px.wrapping_add(above);
    }
}

pub fn gradient_unfilter(prev: Option<&[u8]>, row: &mut [u8]) {
    let Some(prev) = prev else {
        return horizontal_unfilter(None, row);
    };
    if row.is_empty() {
        return;
    }
    let prev = &prev[..row.len()];

    row[0] = row[0].wrapping_add(prev[0]);

    let mut left = i32::from(row[0]);

    for x in 1..row.len() {
        let sum = left + i32::from(prev[x]) - i32::from(prev[x - 1]);
        let px = row[x].wrapping_add(sum.clamp(0, 255) as u8);

        left = i32::from(px);
        row[x] = px;
    }
}

pub type UnfilterFn = fn(Option<&[u8]>, &mut [u8]);

pub struct FilterDsp {
    pub horizontal_unfilter: UnfilterFn,
    pub vertical_unfilter: UnfilterFn,
    pub gradient_unfilter: UnfilterFn,
}

impl FilterDsp {
    pub const fn scalar() -> Self {
        FilterDsp {
            horizontal_unfilter,
            vertical_unfilter,
            gradient_unfilter,
        }
    }

    pub fn new() -> Self {
        #[allow(unused_mut)]
        let mut table = Self::scalar();

        #[cfg(feature = "asm")]
        crate::asm::filters::init(&mut table, crate::cpu::flags());
        table
    }
}

impl Default for FilterDsp {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_row_is_left_predicted_in_every_mode() {
        let src = [1u8, 2, 3, 250];
        let want = [1u8, 3, 6, 0];

        for f in [horizontal_unfilter, vertical_unfilter, gradient_unfilter] {
            let mut row = src;

            f(None, &mut row);
            assert_eq!(row, want);
        }
    }

    #[test]
    fn vertical_adds_the_row_above() {
        let prev = [10u8, 20, 250];
        let mut row = [1u8, 2, 10];

        vertical_unfilter(Some(&prev), &mut row);
        assert_eq!(row, [11, 22, 4]);
    }

    #[test]
    fn gradient_clamps_its_predictor() {
        let prev = [0u8, 255, 0];
        let mut row = [200u8, 0, 0];

        gradient_unfilter(Some(&prev), &mut row);
        /* x=1: clamp(200 + 255 - 0) = 255; x=2: clamp(255 + 0 - 255) = 0. */
        assert_eq!(row, [200, 255, 0]);
    }
}
