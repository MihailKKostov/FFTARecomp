//! Touch-only gestures the engine follows from finger down to up (the app
//! sends them instead of taps when `pointer_down` claims the finger):
//! dragging the party grid, holding the shop's quantity arrows, swiping
//! between units on a status screen. A finger that is lifted without moving
//! is a tap.
use crate::engine::Engine;
use crate::touch::pieces;
use crate::ui_layout::Layout;
use gba_core::{ffta_battle_mouse as battle, ffta_shop_mouse as shop};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Gesture {
    /// The party grid follows the finger (native rows per screen pixel).
    Party { start: (f32, f32), last_y: f32, moved: bool },
    /// A quantity arrow is held (+1 or -1): one step at once, then repeated
    /// faster and in bigger steps while held.
    Quantity { up: bool, since: u64, start: (f32, f32) },
    /// A shop list's rows: a tap, a vertical drag (scroll) or a sideways swipe
    /// (category), decided once the finger clearly moves. `native` the finger's
    /// native point at the start, `anchor` the list's native centre (for mapping
    /// later points), `samples` recent (frame, native point) for the speed.
    Shop { start: (f32, f32), native: (f32, f32), anchor: [i32; 2], vertical: Option<bool>, start_pos: f32, samples: [(u64, f32, f32); 2] },
    /// A unit's status screen (or the pub's mission pages): a sideways swipe
    /// shows the next unit or page (to the left) or the one before (to the
    /// right), the game's Right and Left, once per swipe; `moved` once the
    /// finger clearly moved (no tap then).
    Unit { start: (f32, f32), moved: bool, switched: bool },
    /// The world map's law list follows the finger (`scroll` at the start).
    Laws { start: (f32, f32), scroll: f32, moved: bool },
    /// Dialogue: a tap moves it on; held (`since` the frame it touched), it
    /// is hurried (`text_speed`), `arrow` the frame its page finished.
    Dialogue { start: (f32, f32), since: u64, arrow: Option<u64> },
    /// The battlefield follows the finger (the game's camera, `camera` at the
    /// start); `moved` once it clearly moved (no tap then).
    Pan { start: (f32, f32), camera: (i16, i16), moved: bool },
    /// The battle plays on by itself (enemy turns, actions): held (`since`
    /// the frame it touched), the game runs faster (`TURBO`); a short touch
    /// is a tap.
    Turbo { start: (f32, f32), since: u64 },
    /// A finger on an entry of a battle list (`row` at `native`): a tap
    /// chooses it; held, the game's cursor goes to it and its HELP is shown
    /// (SELECT) until the finger lifts (A closes it).
    Help { start: (f32, f32), since: u64, native: (f32, f32), row: u8, shown: bool },
}

/// Frames emulated per frame shown while a finger holds the battle on.
pub(crate) const TURBO: usize = 4;

/// The battle camera's scroll (EWRAM, USA: the projection descriptor at
/// 0x02007f40 subtracts it from the map's origin). The game clamps it to the
/// map and keeps it until its cursor moves out of view.
const CAMERA: usize = 0x7f64;

/// Quantity arrows beside the game's quantity window (native, from WIN1).
pub(crate) struct QuantityArrows { pub window: [i32; 4] }
impl QuantityArrows {
    /// The arrow cells: '<' left of the window, '>' right of it (16x16 glyph cells).
    pub fn cells(&self) -> [[i32; 2]; 2] {
        let [l, t, r, _] = self.window;
        [[l - 13, t], [r + 3, t]]
    }
    /// The arrow under native point (x, y): a generous area around each glyph.
    pub fn hit(&self, x: f32, y: f32) -> Option<bool> {
        let [l, t, r, b] = self.window;
        if y < (t - 8) as f32 || y >= (b + 8) as f32 { return None; }
        if x >= (l - 28) as f32 && x < l as f32 { return Some(false); }
        if x >= r as f32 && x < (r + 28) as f32 { return Some(true); }
        None
    }
}

/// Frames before a held arrow repeats, and the step for how long it is held.
const REPEAT_AFTER: u64 = 18;
fn step_for(held: u64) -> u8 { if held < 75 { 1 } else if held < 150 { 5 } else { 10 } }

impl Engine {
    /// The quantity window of a touch-only portrait shop, when shown.
    pub(crate) fn quantity_arrows(&self) -> Option<QuantityArrows> {
        let bus = &self.machine.bus;
        if !self.town_buttons() || shop::quantity(bus).is_none() || gba_core::ppu::shop_info_open(bus) { return None; }
        let io = |o: usize| u16::from_le_bytes([bus.io[o], bus.io[o + 1]]);
        if io(0) & 0x4000 == 0 { return None; }
        let (h, v) = (io(0x42), io(0x46));
        let window = [(h >> 8) as i32, (v >> 8) as i32, (h & 255) as i32, (v & 255) as i32];
        (window[0] >= 28 && window[2] <= 212 && window[0] < window[2] && window[1] < window[3]).then_some(QuantityArrows { window })
    }
    /// Screen point (px) to native UI coordinates through the docked piece
    /// showing native point `anchor` (so points beside that piece map too).
    fn native_near(&self, px: f32, py: f32, anchor: [i32; 2]) -> Option<(f32, f32)> {
        let band = self.ui_band();
        let layouts: Vec<Layout> = pieces(&self.machine.bus, band, 0, self.town_menu);
        layouts.iter().find(|l| {
            let sx = l.rect[0] + (anchor[0] + 120) as f32 / 480. * l.rect[2];
            let sy = l.rect[1] + anchor[1] as f32 / 160. * l.rect[3];
            sx >= l.clip[0] && sx < l.clip[0] + l.clip[2] && sy >= l.clip[1] && sy < l.clip[1] + l.clip[3]
        }).map(|l| ((px - l.rect[0]) / l.rect[2] * 480. - 120., (py - l.rect[1]) / l.rect[3] * 160.))
    }
    /// The quantity arrow at screen fraction (x, y), if any.
    fn quantity_arrow_at(&self, x: f32, y: f32) -> Option<bool> {
        let arrows = self.quantity_arrows()?;
        let [l, t, r, b] = arrows.window;
        let (nx, ny) = self.native_near(x * self.ui_window.0 as f32, y * self.ui_window.1 as f32, [(l + r) / 2, (t + b) / 2])?;
        arrows.hit(nx, ny)
    }
    fn quantity_step(&mut self, up: bool, step: u8) {
        let bus = &mut self.machine.bus;
        let Some((value, _)) = shop::quantity(bus) else { return };
        let target = if up { value.saturating_add(step) } else { value.saturating_sub(step).max(1) };
        shop::set_quantity(bus, target);
        self.quantity_flash = (up, bus.frames + 8);
    }
    /// A tap on a quantity arrow (the app's taps, without finger tracking): one step.
    pub(crate) fn quantity_tap(&mut self, x: f32, y: f32) -> bool {
        let Some(up) = self.quantity_arrow_at(x, y) else { return false };
        self.mouse.cancel();
        self.quantity_step(up, 1);
        true
    }

    /// A finger touches the screen at (x, y) (fractions): true when the
    /// engine follows it (the app then sends its moves and lift, not a tap).
    pub fn pointer_down(&mut self, x: f32, y: f32, slop: f32) -> bool {
        self.gesture = None;
        self.gesture_slop = if slop.is_finite() && slop > 0. { slop } else { 48. };
        if !self.touch_enabled || !x.is_finite() || !y.is_finite() { return false; }
        if let Some(up) = self.quantity_arrow_at(x, y) {
            self.mouse.cancel();
            self.quantity_step(up, 1);
            self.gesture = Some(Gesture::Quantity { up, since: self.machine.bus.frames, start: (x, y) });
            return true;
        }
        if let Some(l) = self.town_buttons().then(|| shop::shop_list(&self.machine.bus)).flatten() {
            let anchor = [l.right / 2, l.origin[1] + l.visible as i32 * 8];
            let px = (x * self.ui_window.0 as f32, y * self.ui_window.1 as f32);
            if let Some((nx, ny)) = self.native_near(px.0, px.1, anchor) {
                if nx >= 0. && nx < l.right as f32 && ny >= l.origin[1] as f32 && ny < (l.origin[1] + l.visible as i32 * 16) as f32 {
                    // A finger on a moving list stops it.
                    let s = &mut self.shop_scroll;
                    s.release(0.);
                    let frame = self.machine.bus.frames;
                    self.gesture = Some(Gesture::Shop { start: (x, y), native: (nx, ny), anchor, vertical: None,
                        start_pos: self.shop_scroll.pos, samples: [(frame, nx, ny); 2] });
                    return true;
                }
            }
        }
        let py = y * self.ui_window.1 as f32;
        if self.laws.open && self.laws.screen_area.is_some_and(|(a, b)| py >= a && py < b) {
            self.gesture = Some(Gesture::Laws { start: (x, y), scroll: self.laws.scroll, moved: false });
            return true;
        }
        if self.party_drag_starts(x, y) {
            self.gesture = Some(Gesture::Party { start: (x, y), last_y: y, moved: false });
            return true;
        }
        // Unit status screens, and the pub's mission pages: sideways swipes turn.
        let pages = (self.town_buttons() || gba_core::ffta_battle_mouse::battle_cursor(&self.machine.bus)) && crate::pub_touch::mission_pages(&self.machine.bus) && crate::pub_touch::notice(&self.machine.bus).is_none();
        if (self.menu_layout.is_some() && (crate::engine::party_status(&self.machine.bus) || crate::battle_ui::status(&self.machine.bus))) || pages {
            self.gesture = Some(Gesture::Unit { start: (x, y), moved: false, switched: false });
            return true;
        }
        if let Some(native) = self.docked_native(x, y).filter(|_| crate::battle_ui::window(&self.machine.bus).is_some()) {
            if let Some(row) = battle::MenuScene::read(&self.machine.bus).and_then(|m| m.hit(native.0, native.1)) {
                self.gesture = Some(Gesture::Help { start: (x, y), since: self.machine.bus.frames, native, row, shown: false });
                return true;
            }
        }
        if self.battle_pan() {
            let e = &self.machine.bus.ewram;
            let camera = (i16::from_le_bytes([e[CAMERA], e[CAMERA + 1]]), i16::from_le_bytes([e[CAMERA + 2], e[CAMERA + 3]]));
            self.gesture = Some(Gesture::Pan { start: (x, y), camera, moved: false });
            return true;
        }
        if self.dialogue_hold() {
            self.gesture = Some(Gesture::Dialogue { start: (x, y), since: self.machine.bus.frames, arrow: None });
            return true;
        }
        if self.battle_runs() {
            self.gesture = Some(Gesture::Turbo { start: (x, y), since: self.machine.bus.frames });
            return true;
        }
        false
    }
    pub fn pointer_move(&mut self, x: f32, y: f32) {
        if !x.is_finite() || !y.is_finite() { return; }
        let (w, h) = (self.ui_window.0 as f32, self.ui_window.1 as f32);
        match self.gesture {
            Some(Gesture::Party { start, last_y, moved }) => {
                // Lists follow the finger as soon as it clearly moves.
                let moved = moved || ((y - start.1) * h).abs() >= self.gesture_slop / 3.;
                if moved { self.party_drag(y - last_y); }
                self.gesture = Some(Gesture::Party { start, last_y: if moved { y } else { last_y }, moved });
            }
            Some(Gesture::Unit { start, moved, switched }) => {
                let (dx, dy) = ((x - start.0) * w, (y - start.1) * h);
                let moved = moved || dx.hypot(dy) >= self.gesture_slop;
                // The game's own unit switch: Right shows the next unit, Left the one before.
                let switch = !switched && dx.abs() >= self.gesture_slop && dx.abs() >= dy.abs();
                if switch {
                    self.mouse.cancel();
                    self.party_goal = None;
                    battle::press(&mut self.machine.bus, if dx < 0. { 0x3ef } else { 0x3df });
                }
                self.gesture = Some(Gesture::Unit { start, moved, switched: switched || switch });
            }
            Some(Gesture::Laws { start, scroll, moved }) => {
                let dy = (y - start.1) * h;
                let moved = moved || dy.abs() >= self.gesture_slop / 3.;
                if moved && self.laws.scale > 0. { self.laws.scroll = (scroll - dy / self.laws.scale).clamp(0., self.laws.max_scroll); }
                self.gesture = Some(Gesture::Laws { start, scroll, moved });
            }
            Some(Gesture::Quantity { start, .. }) => {
                // Sliding off the arrow ends the hold.
                if ((x - start.0) * w).hypot((y - start.1) * h) > self.gesture_slop * 2. { self.gesture = None; }
            }
            Some(Gesture::Shop { start, native, anchor, mut vertical, start_pos, mut samples }) => {
                let Some((nx, ny)) = self.native_near(x * w, y * h, anchor) else { return };
                let (dx, dy) = ((x - start.0) * w, (y - start.1) * h);
                if vertical.is_none() && dx.hypot(dy) >= self.gesture_slop / 3. {
                    // Sideways only where there is another category.
                    vertical = Some(dy.abs() >= dx.abs() || self.swipe_category(nx - native.0).is_none() && self.swipe_category(native.0 - nx).is_none());
                }
                let frame = self.machine.bus.frames;
                if frame > samples[1].0 { samples = [samples[1], (frame, nx, ny)]; } else { samples[1] = (frame, nx, ny); }
                match vertical {
                    Some(true) => { self.shop_scroll.dragging = true; self.shop_scroll.drag_to(start_pos - (ny - native.1)); }
                    Some(false) => {
                        let d = nx - native.0;
                        // Resisting where there is no category that way.
                        self.shop_scroll.slide = crate::shop_scroll::Slide::Drag(if self.swipe_category(d).is_some() { d } else { d * 0.3 });
                    }
                    None => {}
                }
                self.gesture = Some(Gesture::Shop { start, native, anchor, vertical, start_pos, samples });
            }
            Some(Gesture::Pan { start, camera, moved }) => {
                let (dx, dy) = ((x - start.0) * w, (y - start.1) * h);
                let moved = moved || dx.hypot(dy) >= self.gesture_slop / 2.;
                if moved && self.battle_pan() {
                    // Screen pixels to native: the field's on-screen scale.
                    let scale = gba_core::mem::render_width() as f32 / self.world_rect[2].max(1.);
                    let to = |c: i16, d: f32| (c as f32 - d * scale).round().clamp(-1024., 1024.) as i16;
                    let (cx, cy) = (to(camera.0, dx), to(camera.1, dy));
                    let e = &mut self.machine.bus.ewram;
                    e[CAMERA..CAMERA + 2].copy_from_slice(&cx.to_le_bytes());
                    e[CAMERA + 2..CAMERA + 4].copy_from_slice(&cy.to_le_bytes());
                }
                self.gesture = Some(Gesture::Pan { start, camera, moved });
            }
            // Held anywhere: the finger may slide.
            Some(Gesture::Dialogue { .. } | Gesture::Turbo { .. } | Gesture::Help { .. }) | None => {}
        }
    }
    /// The finger is lifted (or the gesture cancelled).
    pub fn pointer_up(&mut self, x: f32, y: f32, cancel: bool) {
        let gesture = self.gesture.take();
        if cancel { return; }
        match gesture {
            Some(Gesture::Party { start, moved: false, .. } | Gesture::Unit { start, moved: false, .. } | Gesture::Pan { start, moved: false, .. }) => self.tap(start.0, start.1, false),
            // Lifted before it hurried: a tap.
            Some(Gesture::Dialogue { start, since, .. }) if self.machine.bus.frames < since + crate::text_speed::HOLD => self.tap(start.0, start.1, false),
            Some(Gesture::Help { shown: true, .. }) => { self.mouse.cancel(); battle::press(&mut self.machine.bus, 0x3fe); }
            Some(Gesture::Help { start, .. }) => self.tap(start.0, start.1, false),
            Some(Gesture::Turbo { start, since }) if self.machine.bus.frames < since + crate::text_speed::HOLD => self.tap(start.0, start.1, false),
            Some(Gesture::Shop { start, native, vertical: None, .. }) => self.shop_rows_tap(start.0, start.1, native.0, native.1),
            Some(Gesture::Shop { native, vertical: Some(vertical), samples: [(f0, x0, y0), (f1, x1, y1)], anchor, .. }) => {
                let frames = (f1.saturating_sub(f0)).max(1) as f32;
                let fresh = self.machine.bus.frames <= f1 + 3;
                if vertical {
                    self.shop_scroll.release(if fresh { -(y1 - y0) / frames } else { 0. });
                } else {
                    let (w, h) = (self.ui_window.0 as f32, self.ui_window.1 as f32);
                    let end = self.native_near(x * w, y * h, anchor).map_or(x1, |p| p.0);
                    self.swipe_end(end - native.0, if fresh { (x1 - x0) / frames } else { 0. });
                }
            }
            _ => {}
        }
    }
    /// A drag may pan the battlefield: the player chooses on it (a cell, a
    /// command, the facing, units to send), not while the game moves its camera.
    pub(crate) fn battle_pan(&self) -> bool {
        let bus = &self.machine.bus;
        self.usa && self.touch_enabled && bus.ffta_dialogue.docked
            && (battle::Scene::read(bus).is_some() || battle::facing_center(bus).is_some()
                || crate::deploy_touch::shown(bus)
                || (bus.ffta_dialogue.action_layout && battle::MenuScene::read(bus).is_some()))
    }
    /// The battle plays on without the player: the field shown, nothing to
    /// choose or read (no cursor, command, facing, units to send, notice or
    /// dialogue).
    pub(crate) fn battle_runs(&self) -> bool {
        let bus = &self.machine.bus;
        self.usa && self.touch_enabled && bus.ffta_dialogue.docked && battle::field_shown(bus)
            && !self.battle_pan() && !crate::text_speed::shown(bus)
            && crate::pub_touch::notice(bus).is_none() && !crate::pub_touch::continue_arrow(bus)
            && battle::MenuScene::read(bus).is_none()
    }
    /// Frames to emulate for `count` shown: more while a finger holds the
    /// battle on, until there is something to choose.
    pub(crate) fn turbo_frames(&mut self, count: usize) -> usize {
        let Some(Gesture::Turbo { since, .. }) = self.gesture else { return count };
        if self.machine.bus.frames < since + crate::text_speed::HOLD { return count; }
        if !self.battle_runs() { return count; }
        count.max(TURBO)
    }
    /// Each emulated frame: a finger held on dialogue hurries it.
    pub(crate) fn drive_dialogue_gesture(&mut self) {
        let Some(Gesture::Dialogue { start, since, mut arrow }) = self.gesture else { return };
        self.drive_dialogue_hold(since, &mut arrow);
        if matches!(self.gesture, Some(Gesture::Dialogue { .. })) { self.gesture = Some(Gesture::Dialogue { start, since, arrow }); }
    }
    /// Once per frame: a finger held on a battle list's entry shows its help.
    fn drive_help(&mut self) {
        let Some(Gesture::Help { start, since, native, row, shown: false }) = self.gesture else { return };
        let bus = &mut self.machine.bus;
        if bus.frames < since + crate::text_speed::HOLD { return; }
        let Some(menu) = battle::MenuScene::read(bus) else { self.gesture = None; return };
        if menu.selected != row { battle::aim(bus, native.0, native.1, false); return; }
        if !battle::idle(bus) { return; }
        self.mouse.cancel();
        battle::press(&mut self.machine.bus, 0x3fb);
        self.gesture = Some(Gesture::Help { start, since, native, row, shown: true });
    }
    /// Once per frame: repeats a held quantity arrow.
    pub(crate) fn drive_gestures(&mut self) {
        self.drive_help();
        let Some(Gesture::Quantity { up, since, .. }) = self.gesture else { return };
        let bus = &self.machine.bus;
        if shop::quantity(bus).is_none() { self.gesture = None; return; }
        let held = bus.frames.saturating_sub(since);
        if held < REPEAT_AFTER || shop::quantity_pending(bus) || !battle::idle(bus) { return; }
        self.quantity_step(up, step_for(held));
    }
    /// Draws the quantity arrows into the UI canvas beside the game's window
    /// (the game's menu font '<' and '>', highlighted while used).
    pub(crate) fn draw_quantity_arrows(&mut self) {
        let Some(arrows) = self.quantity_arrows() else { return };
        let frames = self.machine.bus.frames;
        let held = match self.gesture { Some(Gesture::Quantity { up, .. }) => Some(up), _ => None };
        let flash = (frames < self.quantity_flash.1).then_some(self.quantity_flash.0);
        let [left, right] = arrows.cells();
        let bus = &mut self.machine.bus;
        for (cell, up, ch) in [(left, false, '<'), (right, true, '>')] {
            let active = held == Some(up) || flash == Some(up);
            crate::game_buttons::draw_glyph_ui(&bus.rom, &mut bus.ffta_dialogue.pixels, cell, ch, active);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrows_sit_beside_the_quantity_window_and_hold_speeds_up() {
        let arrows = QuantityArrows { window: [152, 88, 192, 104] };
        assert_eq!(arrows.cells(), [[139, 88], [195, 88]]);
        assert_eq!(arrows.hit(140., 96.), Some(false));
        assert_eq!(arrows.hit(200., 90.), Some(true));
        assert_eq!(arrows.hit(170., 96.), None);
        assert_eq!(arrows.hit(140., 130.), None);
        assert_eq!((step_for(10), step_for(100), step_for(200)), (1, 5, 10));
    }
}
