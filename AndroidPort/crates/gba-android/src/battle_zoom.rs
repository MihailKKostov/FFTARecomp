//! Zooming out of a battle's field (touch-only portrait): the whole field
//! seen at once, narrow as the portrait screen is (the native 240 columns
//! show part of most fields). The game draws the field beyond its screen
//! when given margins (mem::set_extra: sprites and BG layers both), so while
//! zoomed out the frame is drawn with wider and taller margins, then made
//! smaller into the usual texture about the native screen's centre, before
//! the touch layer draws its windows and buttons (which keep their size).
//! Taps on the field are mapped back through the same scale.
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, mem};

/// The zoomed-out scale: whole source pixels, two by two.
pub(crate) const OVERVIEW: f32 = 0.5;
/// The button's key (`press_button`).
pub(crate) const ZOOM: u16 = 9;
/// Frames without a field (the battle over) before the zoom is forgotten.
const FORGET: u32 = 300;

pub(crate) struct Zoom {
    /// The scale drawn last (1: none), the one moved toward, and now.
    pub shown: f32,
    target: f32,
    current: f32,
    away: u32,
    /// The camera last frame, frames it stayed, and what was being chosen
    /// when the field was last centred.
    camera: (i16, i16),
    still: u32,
    centred: Option<u32>,
    /// The scale a pinch began at.
    pinch_from: f32,
}
impl Default for Zoom { fn default() -> Self { Self { shown: 1., target: 1., current: 1., away: 0, camera: (0, 0), still: 0, centred: None, pinch_from: 1. } } }

/// The margins to draw a texture of `width` x `height` at scale `z`.
fn margins(width: usize, height: usize, z: f32) -> (usize, usize) {
    let side = |size: usize, native: usize, max: usize| ((((size as f32 / z) - native as f32) / 2.).ceil().max(0.) as usize).min(max);
    (side(width, 240, mem::MAX_EXTRA), side(height, 160, mem::MAX_EXTRA_Y))
}

impl Engine {
    /// A battle's field fills the view (portrait, touch-only), nothing over it
    /// but the touch layer's windows. Not the roster of units to send (its
    /// ring and arrows are the game's sprites, at the screen's edges).
    pub(crate) fn zoomable(&self) -> bool {
        let bus = &self.machine.bus;
        self.usa && self.touch_enabled && bus.ffta_dialogue.docked && mem::extra() <= 8 && self.menu_layout.is_none()
            && battle::field_shown(bus) && crate::deploy_touch::roster(bus).is_none()
    }
    pub(crate) fn zoomed_out(&self) -> bool { self.zoom.target < 1. }
    /// Zoom out, the camera gliding to the field's centre (as a pan, so
    /// Center and zooming in bring it back); or back in, and back (or to
    /// the field's cursor).
    pub(crate) fn toggle_zoom(&mut self) {
        if self.zoomed_out() {
            self.zoom.target = 1.;
            if self.pan_home.is_some() { self.recenter(); return; }
            let bus = &self.machine.bus;
            if let Some(scene) = battle::Scene::read(bus) {
                if let Some(c) = scene.cells.iter().find(|c| (c.x, c.y) == scene.selected) {
                    let camera = crate::battle_ui::camera(bus);
                    self.glide = Some((camera.0 + (c.center[0] - 120.).round() as i16, camera.1 + (c.center[1] - 80.).round() as i16));
                }
            }
            return;
        }
        self.zoom.target = OVERVIEW;
        self.note_pan(crate::battle_ui::camera(&self.machine.bus));
        self.centre_field();
    }
    /// The camera glides to show the field's middle at the screen's, as
    /// near as keeps what is being chosen on the game's own screen: the
    /// game leaves out sprites beyond it (the facing's arrows, read there).
    fn centre_field(&mut self) {
        let bus = &self.machine.bus;
        let Some(b) = battle::field_bounds(bus) else { return };
        let camera = crate::battle_ui::camera(bus);
        // The cells' centres, and a unit's height over the top row.
        let mut shift = ((b[0] + b[2]) / 2. - 120., (b[1] - 24. + b[3]) / 2. - 80.);
        let focus = battle::facing_anchor(bus).or_else(|| battle::Scene::read(bus)
            .and_then(|s| s.cells.iter().find(|c| (c.x, c.y) == s.selected).map(|c| c.center)));
        if let Some([fx, fy]) = focus {
            // Within the screen: the unit (40 tall over its feet) and its arrows.
            shift.0 = shift.0.clamp(fx - 216., fx - 24.);
            shift.1 = shift.1.clamp(fy - 150., fy - 48.);
        }
        let to = (camera.0 + shift.0.round() as i16, camera.1 + shift.1.round() as i16);
        self.zoom.centred = crate::battle_ui::choosing(bus);
        if (to.0 - camera.0).abs() + (to.1 - camera.1).abs() > 2 { self.glide = Some(to); }
    }
    /// Once a frame: zoomed out, the game still moves the camera to its
    /// cursor (a new mode, a cell chosen); once it settles there the field
    /// is centred again. The player's own panning is left as it is.
    pub(crate) fn drive_zoom_camera(&mut self) {
        let camera = crate::battle_ui::camera(&self.machine.bus);
        if camera == self.zoom.camera { self.zoom.still += 1; } else { self.zoom.still = 0; self.zoom.camera = camera; }
        if !self.zoomed_out() || !self.zoomable() || self.glide.is_some() || self.gesture.is_some() || self.zoom.still < 4 { return; }
        if crate::battle_ui::choosing(&self.machine.bus) != self.zoom.centred { self.centre_field(); }
    }
    /// Two fingers spread to `ratio` of their first distance (`start`: the
    /// first move): the field follows, between the overview and 1.
    pub fn pinch(&mut self, ratio: f32, start: bool) -> bool {
        if !ratio.is_finite() || ratio <= 0. || !self.zoomable() { return false; }
        if start { self.zoom.pinch_from = self.zoom.current; }
        self.zoom.target = (self.zoom.pinch_from * ratio).clamp(OVERVIEW, 1.);
        self.zoom.current = self.zoom.target;
        true
    }
    /// The fingers lifted: to the overview or back in, whichever is nearer
    /// (whole pixels: the overview takes the source two by two).
    pub fn pinch_end(&mut self) {
        let out = self.zoom.current < (OVERVIEW + 1.) / 2.;
        // As from the button: toggle_zoom goes the other way from `target`.
        self.zoom.target = if out { 1. } else { OVERVIEW };
        self.toggle_zoom();
    }
    /// Before a frame is run: the margins to draw it with when zoomed out
    /// (the usual ones are `extra_x`, `extra_y`), moving the scale on.
    pub(crate) fn zoom_frame(&mut self, extra_x: usize, extra_y: usize) -> Option<(usize, usize, f32)> {
        let field = self.zoomable();
        let z = &mut self.zoom;
        if field { z.away = 0; } else { z.away += 1; if z.away > FORGET { *z = Zoom::default(); } }
        // A quarter of the way a frame.
        z.current += (z.target - z.current) * 0.25;
        if (z.target - z.current).abs() < 0.01 { z.current = z.target; }
        if !field || z.current >= 0.999 { return None; }
        let (mx, my) = margins(240 + 2 * extra_x, 160 + 2 * extra_y, z.current);
        Some((mx, my, z.current))
    }
    /// Makes the frame drawn `source_w` x `source_h` (BGR555) smaller by `z`
    /// into the texture (render_width x render_height), centres together;
    /// rows beyond the drawn ones are its edge rows (the sky).
    pub(crate) fn zoom_resample(&mut self, source: &[u16], source_w: usize, source_h: usize, z: f32) {
        let (w, h) = (mem::render_width(), mem::render_height());
        let colors = crate::engine::rgba_colors();
        let (cx, cy) = (w as f32 / 2., h as f32 / 2.);
        let (sx0, sy0) = (source_w as f32 / 2., source_h as f32 / 2.);
        // Four samples a pixel, a quarter and three quarters across: at 1/2,
        // exactly the four source pixels under it.
        let offsets = [0.25f32, 0.75];
        let cols: Vec<[usize; 2]> = (0..w).map(|x| offsets.map(|o| (((x as f32 + o - cx) / z + sx0).floor().max(0.) as usize).min(source_w - 1))).collect();
        for y in 0..h {
            let rows = offsets.map(|o| (((y as f32 + o - cy) / z + sy0).floor().max(0.) as usize).min(source_h - 1));
            for x in 0..w {
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for &sy in &rows { for &sx in &cols[x] {
                    let p = source[sy * source_w + sx] as u32;
                    r += p & 31; g += (p >> 5) & 31; b += (p >> 10) & 31;
                } }
                let p = ((r + 2) / 4) | ((g + 2) / 4) << 5 | ((b + 2) / 4) << 10;
                self.pixels[y * w + x] = colors[p as usize];
            }
        }
    }
    /// Native field coordinates of texture (tx, ty), through the zoom shown.
    pub(crate) fn texture_native(&self, tx: f32, ty: f32) -> (f32, f32) {
        let z = self.zoom.shown;
        ((tx - mem::render_width() as f32 / 2.) / z + 120., (ty - mem::render_height() as f32 / 2.) / z + 80.)
    }
    /// Texture coordinates of native field (x, y), through the zoom shown.
    pub(crate) fn native_texture(&self, x: f32, y: f32) -> (f32, f32) {
        let z = self.zoom.shown;
        ((x - 120.) * z + mem::render_width() as f32 / 2., (y - 80.) * z + mem::render_height() as f32 / 2.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zoomed_out_the_frame_is_drawn_twice_as_wide_and_tall() {
        // A portrait texture (256 x 532): 512 columns, and rows up to the limit.
        assert_eq!(margins(256, 532, 0.5), (136, 320));
        assert_eq!(margins(256, 300, 0.5), (136, 220));
        assert_eq!(margins(256, 300, 1.), (8, 70));
    }
}
