// Screen-space UI coordinates shared with the desktop layout rules.

/// Screen rectangle that hosts the native UI: horizontally the letterboxed
/// game width, vertically either the 160-row letterbox (`h == 160 * s`) or,
/// in portrait battles, a taller docking frame so panels sit at its edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub s: f32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Band {
    pub fn letterbox(window: (u32, u32), base_width: f32) -> Self {
        let s = (window.0 as f32 / base_width).min(window.1 as f32 / 160.);
        let (x, y) = ((window.0 as f32 - base_width * s) / 2., (window.1 as f32 - 160. * s) / 2.);
        Self { s, x, y, w: base_width * s, h: 160. * s }
    }
    /// Same scale and width as the letterbox, spanning `top..bottom` when that is taller.
    pub fn docked(window: (u32, u32), base_width: f32, top: f32, bottom: f32) -> Self {
        let band = Self::letterbox(window, base_width);
        let top = top.max(0.);
        let bottom = bottom.min(window.1 as f32);
        if !(top.is_finite() && bottom.is_finite()) || bottom - top <= band.h { return band; }
        Self { y: top, h: bottom - top, ..band }
    }
    fn docked_frame(&self) -> bool { self.h > 160. * self.s + 0.5 }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dock { Top, Bottom, Thumb,
    /// Moved like the thumb block with these bounds (a menu sliding in or out).
    Follow([i32; 4]) }

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub rect: [f32; 4],
    pub clip: [f32; 4],
}
impl Layout {
    /// A complete modal stays horizontally centered and grows down from its
    /// original top margin, independently of the world's camera and zoom.
    /// In a docking frame it is centered vertically instead.
    pub fn centered_panel(band: Band, size: f32, bounds: [i32; 4]) -> Self {
        let Band { s, x, y, w, h } = band;
        let k = s * size.clamp(0.5, 2.);
        let top = if band.docked_frame() { y + h * 0.5 - (bounds[1] + bounds[3]) as f32 * 0.5 * k }
            else { y + bounds[1] as f32 * (s - k) };
        Self { rect: [(x + w * 0.5 - (120. + (bounds[0] + bounds[2]) as f32 * 0.5) * k).round(),
            top.round(), 480. * k, 160. * k], clip: [x, y, w, h] }
    }
    /// Center the complete host UI canvas inside the game viewport.
    pub fn centered(band: Band, size: f32) -> Self {
        let Band { s, x, y, w, h } = band;
        let k = s * size.clamp(0.5, 2.);
        let left = x + w * 0.5 - 240. * k;
        Self { rect: [left.round(), (y + (h - 160. * k) * 0.5).round(), 480. * k, 160. * k], clip: [x, y, w, h] }
    }
    /// `anchor_y` (0..160) keeps the panel's native vertical anchor: 0 pins it
    /// to the band's top, 160 to its bottom.
    pub fn new_with_anchor(band: Band, right: bool, size: f32, offset: [f32; 2], anchor_y: f32) -> Self {
        let Band { s, x, y, w, h } = band;
        let k = s * size.clamp(0.5, 2.);
        let left = if right { x + w - 240. * k } else { x };
        Self {
            rect: [
                (left - 120. * k + offset[0] * s).round(),
                (y + anchor_y.clamp(0., 160.) / 160. * (h - 160. * k) + offset[1] * s).round(),
                480. * k,
                160. * k,
            ],
            clip: [x, y, w, h],
        }
    }
    /// A compact native panel grows inward from its own content corner.
    pub fn at_corner(band: Band, right: bool, bottom: bool, size: f32, length: f32, bounds: [i32; 4]) -> Self {
        let Band { s, x, y, w, h } = band;
        let k = s * size.clamp(0.5, 2.);
        // Length changes native backing geometry before upload, never texture scale.
        let _ = length;
        let left = if right { x + w - bounds[2] as f32 * k } else { x - bounds[0] as f32 * k };
        let top = if bottom { y + h - bounds[3] as f32 * k } else { y - bounds[1] as f32 * k };
        Self { rect: [(left - 120. * k).round(), top.round(), 480. * k, 160. * k], clip: [x, y, w, h] }
    }
    /// Portrait docking frame: one native canvas cut into blocks (native
    /// coordinates, x relative to the 240-wide screen), each drawn as its own
    /// clipped copy. Top blocks keep their native offset under the frame's top
    /// edge, bottom blocks above its bottom edge (both at their native x);
    /// thumb blocks sit lower right, inset from the edge where a right thumb
    /// rests.
    pub fn docked(band: Band, size: f32, blocks: &[([i32; 4], Dock)]) -> Vec<Self> {
        let Band { s, x, y, w, h } = band;
        let k = s * size.clamp(0.5, 2.);
        let canvas_left = x + w * 0.5 - 240. * k;
        let thumb = |r: [i32; 4]| {
            let (pw, ph) = ((r[2] - r[0]) as f32 * k, (r[3] - r[1]) as f32 * k);
            ((x + w - 0.08 * w - pw).max(x), (y + 0.80 * h - ph).max(y))
        };
        blocks.iter().map(|&(r, dock)| {
            let (pw, ph) = ((r[2] - r[0]) as f32 * k, (r[3] - r[1]) as f32 * k);
            let (left, top) = match dock {
                Dock::Top => (canvas_left + (r[0] + 120) as f32 * k, y + r[1] as f32 * k),
                Dock::Bottom => (canvas_left + (r[0] + 120) as f32 * k, y + h - (160 - r[1]) as f32 * k),
                Dock::Thumb => thumb(r),
                Dock::Follow(m) => {
                    let (left, top) = thumb(m);
                    (left + (r[0] - m[0]) as f32 * k, top + (r[1] - m[1]) as f32 * k)
                }
            };
            Self { rect: [(left - (r[0] + 120) as f32 * k).round(), (top - r[1] as f32 * k).round(), 480. * k, 160. * k],
                clip: [left.round(), top.round(), pw.round(), ph.round()] }
        }).collect()
    }
    pub fn source(&self, x: f32, y: f32) -> Option<usize> {
        if x < self.clip[0]
            || y < self.clip[1]
            || x >= self.clip[0] + self.clip[2]
            || y >= self.clip[1] + self.clip[3]
        {
            return None;
        }
        let u = (x - self.rect[0]) / self.rect[2];
        let v = (y - self.rect[1]) / self.rect[3];
        ((0. ..1.).contains(&u) && (0. ..1.).contains(&v))
            .then(|| (v * 160.).floor() as usize * 480 + (u * 480.).floor() as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn docking_frame_moves_bottom_panels_to_its_edge_and_keeps_scale() {
        let window = (1080, 2400);
        let band = Band::letterbox(window, 240.);
        let docked = Band::docked(window, 240., 120., 2300.);
        let bounds = [0, 104, 88, 152];
        let a = Layout::at_corner(band, false, true, 1., 1., bounds);
        let b = Layout::at_corner(docked, false, true, 1., 1., bounds);
        assert_eq!(a.rect[2], b.rect[2]);
        assert_eq!(a.rect[1] + 152. * band.s, band.y + band.h);
        assert_eq!(b.rect[1] + 152. * band.s, 2300.);
        let top = Layout::new_with_anchor(docked, false, 1., [0., 0.], 0.);
        assert_eq!(top.rect[1], 120.);
        assert_eq!(b.clip, [0., 120., 1080., 2180.]);
    }
    #[test]
    fn docked_blocks_keep_native_offsets_and_menus_go_to_the_thumb_zone() {
        let band = Band::docked((1080, 2400), 240., 160., 2340.);
        let dialogue = [0, 4, 240, 66];
        let menu = [0, 78, 96, 142];
        let funds = [162, 137, 240, 157];
        let pieces = Layout::docked(band, 1., &[(dialogue, Dock::Top), (menu, Dock::Thumb), (funds, Dock::Bottom)]);
        // Top: canvas row 4 sits 4 native rows under the frame top, full width.
        assert_eq!(pieces[0].clip, [0., (160. + 4. * 4.5f32).round(), 1080., (62. * 4.5f32).round()]);
        // Thumb: inset from the right edge, bottom at 80% of the frame, source maps back.
        let m = pieces[1].clip;
        assert!((1080. - (m[0] + m[2]) - 0.08 * 1080.).abs() <= 1.);
        assert!((m[1] + m[3] - (160. + 0.80 * 2180.)).abs() <= 1.);
        assert_eq!(pieces[1].source(m[0] + 1., m[1] + 1.), Some(78 * 480 + 120));
        // Bottom: native row 160 at the frame bottom, native x kept.
        let f = pieces[2].clip;
        assert!((f[1] + f[3] - (2340. - 3. * 4.5)).abs() <= 1.);
        assert!((f[0] - 162. * 4.5).abs() <= 1.);
        // A block that is still sliding in keeps the same translation (no jump).
        let sliding = Layout::docked(band, 1., &[([150, 137, 228, 157], Dock::Bottom)]);
        assert_eq!(sliding[0].rect, pieces[2].rect);
        // A menu sliding off to the left keeps the thumb block's translation.
        let closing = Layout::docked(band, 1., &[([0, 78, 72, 142], Dock::Follow(menu))]);
        assert_eq!(closing[0].rect, pieces[1].rect);
        assert_eq!(closing[0].clip[0], pieces[1].clip[0]);
    }
    #[test]
    fn short_or_invalid_frames_fall_back_to_the_letterbox() {
        let window = (2400, 1080);
        let band = Band::letterbox(window, 355.);
        assert_eq!(Band::docked(window, 355., 0., 1080.), band);
        assert_eq!(Band::docked(window, 355., f32::NAN, 900.), band);
        let portrait = Band::letterbox((1080, 2400), 240.);
        assert_eq!(Band::docked((1080, 2400), 240., 1000., 1200.), portrait);
    }
}
