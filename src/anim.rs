use crate::container::{ANMF_FLAG_DISPOSE, ANMF_FLAG_NO_BLEND};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub blend: bool,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct AnimState {
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

#[derive(Clone, Copy, Default, Debug)]
pub struct Placement {
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub frame: AnimState,
}

impl Placement {
    fn is_full_frame(&self, width: i32, height: i32) -> bool {
        width == self.canvas_width && height == self.canvas_height
    }

    pub fn is_key_frame(&self, width: i32, height: i32) -> bool {
        if self.frame.frame_index == 0 {
            return true;
        }
        if (!self.frame.frame_has_alpha
            || self.frame.anmf_flags & ANMF_FLAG_NO_BLEND != 0)
            && self.frame.pos_x == 0
            && self.frame.pos_y == 0
            && self.is_full_frame(width, height)
        {
            return true;
        }
        self.frame.prev_anmf_flags & ANMF_FLAG_DISPOSE != 0
            && (self.is_full_frame(self.frame.prev_width, self.frame.prev_height)
                || self.frame.prev_key_frame)
    }
}

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

    if place.frame.key_frame
        || place.frame.anmf_flags & ANMF_FLAG_NO_BLEND != 0
        || !frame_has_alpha_plane
    {
        out[0] = copy_all;
        return 1;
    }
    if place.frame.prev_anmf_flags & ANMF_FLAG_DISPOSE == 0 {
        out[0] = full;
        return 1;
    }

    let kx = place.frame.pos_x.max(place.frame.prev_pos_x) - place.frame.pos_x;
    let ky = place.frame.pos_y.max(place.frame.prev_pos_y) - place.frame.pos_y;
    let mut kw = (place.frame.pos_x + width)
        .min(place.frame.prev_pos_x + place.frame.prev_width)
        - place.frame.pos_x
        - kx;
    let mut kh = (place.frame.pos_y + height)
        .min(place.frame.prev_pos_y + place.frame.prev_height)
        - place.frame.pos_y
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

    fn on_canvas(frame: AnimState) -> Placement {
        Placement {
            canvas_width: 64,
            canvas_height: 64,
            frame,
        }
    }

    fn place() -> Placement {
        on_canvas(state())
    }

    fn state() -> AnimState {
        AnimState {
            frame_index: 1,
            ..AnimState::default()
        }
    }

    #[test]
    fn the_first_frame_is_always_a_key_frame() {
        let p = on_canvas(AnimState {
            frame_index: 0,
            ..state()
        });

        assert!(p.is_key_frame(1, 1));
    }

    #[test]
    fn a_full_opaque_frame_at_the_origin_is_a_key_frame() {
        let p = on_canvas(AnimState {
            frame_has_alpha: false,
            ..state()
        });

        assert!(p.is_key_frame(64, 64));
        assert!(!p.is_key_frame(63, 64));

        let p = on_canvas(AnimState {
            frame_has_alpha: true,
            ..state()
        });

        assert!(!p.is_key_frame(64, 64));

        let p = on_canvas(AnimState {
            frame_has_alpha: true,
            anmf_flags: ANMF_FLAG_NO_BLEND,
            ..state()
        });

        assert!(p.is_key_frame(64, 64));
    }

    #[test]
    fn a_frame_after_a_full_dispose_is_a_key_frame() {
        let p = on_canvas(AnimState {
            frame_has_alpha: true,
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            prev_width: 64,
            prev_height: 64,
            ..state()
        });

        assert!(p.is_key_frame(10, 10));

        let p = on_canvas(AnimState {
            prev_width: 10,
            prev_height: 10,
            ..p.frame
        });

        assert!(!p.is_key_frame(10, 10));
        assert!(on_canvas(AnimState {
            prev_key_frame: true,
            ..p.frame
        })
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
        let p = on_canvas(AnimState {
            key_frame: true,
            ..state()
        });

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

    #[test]
    fn the_five_regions_tile_the_frame_exactly() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];
        let p = on_canvas(AnimState {
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            pos_x: 4,
            pos_y: 6,
            prev_pos_x: 6,
            prev_pos_y: 8,
            prev_width: 10,
            prev_height: 10,
            ..state()
        });
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

    #[test]
    fn a_planar_overlap_too_thin_to_align_blends_instead() {
        let mut out = [Region {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            blend: false,
        }; 5];
        let p = on_canvas(AnimState {
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            prev_pos_x: 7,
            prev_width: 1,
            prev_height: 64,
            ..state()
        });

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
        let p = on_canvas(AnimState {
            prev_anmf_flags: ANMF_FLAG_DISPOSE,
            prev_pos_x: 40,
            prev_pos_y: 40,
            prev_width: 8,
            prev_height: 8,
            ..state()
        });

        assert_eq!(regions(&p, 8, 8, true, false, &mut out), 1);
        assert!(out[0].blend);
    }
}
