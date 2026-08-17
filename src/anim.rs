//! Where an animation's sub-frame lands on the canvas, and which parts of it
//! are blended rather than copied.
//!
//! This is the compositor's geometry, which is all of it that does not touch a
//! pixel: whether a frame stands on its own, and how libwebp's rule about
//! blending only where the canvas can be non-transparent divides the frame
//! into regions.

use crate::container::{ANMF_FLAG_DISPOSE, ANMF_FLAG_NO_BLEND};

/// A rectangle in sub-frame coordinates, and whether it blends onto what is
/// under it or overwrites it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub blend: bool,
}

/// Where the frame goes and what the frame before it left behind.
#[derive(Clone, Copy, Default, Debug)]
pub struct Placement {
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub pos_x: i32,
    pub pos_y: i32,
    pub anmf_flags: u8,
    pub frame_index: i32,
    pub frame_has_alpha: bool,
    pub key_frame: bool,
    pub prev_anmf_flags: u8,
    pub prev_width: i32,
    pub prev_height: i32,
    pub prev_pos_x: i32,
    pub prev_pos_y: i32,
    pub prev_key_frame: bool,
}

impl Placement {
    fn is_full_frame(&self, width: i32, height: i32) -> bool {
        width == self.canvas_width && height == self.canvas_height
    }

    /// Whether this frame stands on its own, so whatever the canvas holds can
    /// be discarded rather than blended with.
    ///
    /// Three ways to qualify: it is the first frame; it covers the canvas and
    /// cannot show through; or the frame before it disposed everything this
    /// one could have seen.
    pub fn is_key_frame(&self, width: i32, height: i32) -> bool {
        if self.frame_index == 0 {
            return true;
        }
        if (!self.frame_has_alpha || self.anmf_flags & ANMF_FLAG_NO_BLEND != 0)
            && self.pos_x == 0
            && self.pos_y == 0
            && self.is_full_frame(width, height)
        {
            return true;
        }
        self.prev_anmf_flags & ANMF_FLAG_DISPOSE != 0
            && (self.is_full_frame(self.prev_width, self.prev_height)
                || self.prev_key_frame)
    }
}

/// How compositing this frame divides into regions, in the order they are
/// applied.
///
/// libwebp overwrites the frame rectangle and alpha-blends only where the
/// previous canvas can be non-transparent; blending elsewhere would round the
/// colour down against pixels that are not there. When the frame before this
/// one disposed its rectangle, the part of this frame outside that rectangle
/// is the part with something under it, so the overlap is copied and the four
/// strips around it are blended.
///
/// `chroma_aligned` is set for a planar canvas, whose 2x2 chroma blocks cannot
/// be split down the middle: an overlap that does not land on even samples is
/// given up on and the whole frame is blended instead.
pub fn regions(
    place: &Placement,
    width: i32,
    height: i32,
    frame_has_alpha_plane: bool,
    chroma_aligned: bool,
    out: &mut [Region; 5],
) -> usize {
    let full = Region {
        x: 0,
        y: 0,
        w: width,
        h: height,
        blend: true,
    };
    let copy_all = Region {
        blend: false,
        ..full
    };

    if place.key_frame
        || place.anmf_flags & ANMF_FLAG_NO_BLEND != 0
        || !frame_has_alpha_plane
    {
        out[0] = copy_all;
        return 1;
    }
    if place.prev_anmf_flags & ANMF_FLAG_DISPOSE == 0 {
        out[0] = full;
        return 1;
    }

    let kx = place.pos_x.max(place.prev_pos_x) - place.pos_x;
    let ky = place.pos_y.max(place.prev_pos_y) - place.pos_y;
    let mut kw = (place.pos_x + width).min(place.prev_pos_x + place.prev_width)
        - place.pos_x
        - kx;
    let mut kh = (place.pos_y + height).min(place.prev_pos_y + place.prev_height)
        - place.pos_y
        - ky;

    if kw <= 0 || kh <= 0 {
        out[0] = full;
        return 1;
    }
    if chroma_aligned {
        kw &= !1;
        kh &= !1;
        if kw == 0 || kh == 0 {
            out[0] = full;
            return 1;
        }
        /* Only the extent is rounded, not the corner: that is what the C did,
        and the region blitters truncate an odd corner to its chroma block. */
    }

    out[0] = Region {
        x: 0,
        y: 0,
        w: width,
        h: ky,
        blend: true,
    };
    out[1] = Region {
        x: 0,
        y: ky + kh,
        w: width,
        h: height - ky - kh,
        blend: true,
    };
    out[2] = Region {
        x: 0,
        y: ky,
        w: kx,
        h: kh,
        blend: true,
    };
    out[3] = Region {
        x: kx + kw,
        y: ky,
        w: width - kx - kw,
        h: kh,
        blend: true,
    };
    out[4] = Region {
        x: kx,
        y: ky,
        w: kw,
        h: kh,
        blend: false,
    };
    5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> Placement {
        Placement {
            canvas_width: 64,
            canvas_height: 64,
            frame_index: 1,
            ..Placement::default()
        }
    }

    #[test]
    fn the_first_frame_is_always_a_key_frame() {
        let p = Placement {
            frame_index: 0,
            ..place()
        };

        assert!(p.is_key_frame(1, 1));
    }

    #[test]
    fn a_full_opaque_frame_at_the_origin_is_a_key_frame() {
        let p = Placement {
            frame_has_alpha: false,
            ..place()
        };

        assert!(p.is_key_frame(64, 64));
        assert!(!p.is_key_frame(63, 64));

        let p = Placement {
            frame_has_alpha: true,
            ..place()
        };

        assert!(!p.is_key_frame(64, 64));

        let p = Placement {
            frame_has_alpha: true,
            anmf_flags: ANMF_FLAG_NO_BLEND,
            ..place()
        };

        assert!(p.is_key_frame(64, 64));
    }

    #[test]
    fn a_frame_after_a_full_dispose_is_a_key_frame() {
        let p = Placement {
            frame_has_alpha: true,
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            prev_width: 64,
            prev_height: 64,
            ..place()
        };

        assert!(p.is_key_frame(10, 10));

        let p = Placement {
            prev_width: 10,
            prev_height: 10,
            ..p
        };

        assert!(!p.is_key_frame(10, 10));
        assert!(Placement {
            prev_key_frame: true,
            ..p
        }
        .is_key_frame(10, 10));
    }

    #[test]
    fn a_key_frame_is_copied_whole() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];
        let p = Placement {
            key_frame: true,
            ..place()
        };

        assert_eq!(regions(&p, 8, 8, true, false, &mut out), 1);
        assert!(!out[0].blend);
        assert_eq!((out[0].w, out[0].h), (8, 8));
    }

    #[test]
    fn a_frame_with_no_alpha_plane_cannot_blend() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];

        assert_eq!(regions(&place(), 8, 8, false, false, &mut out), 1);
        assert!(!out[0].blend);
    }

    #[test]
    fn without_a_dispose_behind_it_the_whole_frame_blends() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];

        assert_eq!(regions(&place(), 8, 8, true, false, &mut out), 1);
        assert!(out[0].blend);
    }

    /// The four blended strips and the copied overlap have to tile the frame
    /// exactly: every pixel once, none twice.
    #[test]
    fn the_five_regions_tile_the_frame_exactly() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];
        let p = Placement {
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            pos_x: 4,
            pos_y: 6,
            prev_pos_x: 6,
            prev_pos_y: 8,
            prev_width: 10,
            prev_height: 10,
            ..place()
        };
        let (w, h) = (12, 14);

        assert_eq!(regions(&p, w, h, true, false, &mut out), 5);

        let mut hits = vec![0u8; (w * h) as usize];

        for r in &out {
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    assert!((0..w).contains(&x) && (0..h).contains(&y));
                    hits[(y * w + x) as usize] += 1;
                }
            }
        }
        assert!(hits.iter().all(|&n| n == 1));
        assert!(!out[4].blend);
        assert!(out[..4].iter().all(|r| r.blend));
    }

    /// A planar canvas cannot split a 2x2 chroma block, so an overlap that
    /// rounds away to nothing gives up and blends the frame whole.
    #[test]
    fn a_planar_overlap_too_thin_to_align_blends_instead() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];
        let p = Placement {
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            prev_pos_x: 7,
            prev_width: 1,
            prev_height: 64,
            ..place()
        };

        assert_eq!(regions(&p, 32, 32, true, true, &mut out), 1);
        assert!(out[0].blend);
        assert_eq!(regions(&p, 32, 32, true, false, &mut out), 5);
    }

    #[test]
    fn a_disjoint_previous_frame_leaves_nothing_to_keep() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];
        let p = Placement {
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            prev_pos_x: 40,
            prev_pos_y: 40,
            prev_width: 8,
            prev_height: 8,
            ..place()
        };

        assert_eq!(regions(&p, 8, 8, true, false, &mut out), 1);
        assert!(out[0].blend);
    }
}
