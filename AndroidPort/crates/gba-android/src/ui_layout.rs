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
    fn short_or_invalid_frames_fall_back_to_the_letterbox() {
        let window = (2400, 1080);
        let band = Band::letterbox(window, 355.);
        assert_eq!(Band::docked(window, 355., 0., 1080.), band);
        assert_eq!(Band::docked(window, 355., f32::NAN, 900.), band);
        let portrait = Band::letterbox((1080, 2400), 240.);
        assert_eq!(Band::docked((1080, 2400), 240., 1000., 1200.), portrait);
    }
}
