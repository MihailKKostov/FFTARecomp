//! Touch-only portrait battles: the game's battle windows told apart, and the
//! functions only the GBA's buttons reached made reachable by touch.
//!
//! The battle's windows share one list instance (`MenuScene`, one base), so
//! they are told apart by their titles: the glyphs the game draws into the
//! title tiles (BG2, the row over the first entry, from the window's left
//! edge tile 0xc007 + 1). Their pixels are hashed (USA, measured on Herb
//! Picking): the hashes are of the font's glyphs, not of where they are.
//!
//! Functions only buttons reached (measured):
//! - START in the free cursor: the SYSTEM window (Turn Order, Mission, Laws,
//!   Options, Flee, Save Now). Not from the command menu, where START does
//!   nothing: B there leaves for the free cursor first.
//! - SELECT on a highlighted entry: its HELP window; A closes it (Down does
//!   not move under it). A long press on an entry shows it (gestures.rs).
//! - R on a unit's Status: its abilities being learned (AP), R again back;
//!   Left/Right turn its pages.
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, mem::MemMap};

const A: u16 = 0x3fe;
const B: u16 = 0x3fd;
const START: u16 = 0x3f7;
const LEFT: u16 = 0x3df;
const RIGHT: u16 = 0x3ef;
/// Buttons the battle adds (`press_button` keys over the KEYINPUT masks'
/// range of single buttons: none of these is a mask the game reads).
pub(crate) const OPEN_SYSTEM: u16 = 6;
pub(crate) const CLOSE_SYSTEM: u16 = 7;
pub(crate) const RECENTER: u16 = 8;
/// The battle camera's scroll (EWRAM, two i16: x, y; gestures.rs).
const CAMERA: usize = 0x7f64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window { Commands, Action, Abilities, Items, System, Order, Other }

/// Title hashes (FNV-1a over the title tiles' 4-bpp pixels).
const TITLES: [(u64, Window); 6] = [
    (0x5952_a0a2_7e52_8322, Window::Commands),
    (0x763c_d545_cd71_d754, Window::Action),
    (0xc31e_3209_61ea_b9ca, Window::Abilities),
    (0x14f7_b225_fa64_6b85, Window::Items),
    (0xba3d_5307_69f2_1ebf, Window::System),
    // System's Turn Order ("LIST").
    (0x5891_5da2_49c7_ce41, Window::Order),
];

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

pub(crate) fn camera(bus: &MemMap) -> (i16, i16) {
    let e = &bus.ewram;
    (i16::from_le_bytes([e[CAMERA], e[CAMERA + 1]]), i16::from_le_bytes([e[CAMERA + 2], e[CAMERA + 3]]))
}
/// The game's copies of BG0-3CNT and their scrolls (IWRAM), copied to the
/// registers each frame (USA, measured).
const BG_SHADOW: usize = 0x942;
/// Where the camera puts the field's origin (EWRAM, two i16).
const FIELD_ORIGIN: usize = 0x7f68;
/// Moves the battle camera: the units follow it each frame, but the field's
/// layers (BG control 0x4c03/0x4e02, which may be BG0-1 or BG2-3) only while
/// the field's cursor runs. With a list open the game leaves their scroll as
/// it was, so it is set here too: the camera less the field's origin
/// (measured: camera -55,274, origin -160,264, scroll 105,10).
pub(crate) fn set_camera(bus: &mut MemMap, (x, y): (i16, i16)) {
    bus.ewram[CAMERA..CAMERA + 2].copy_from_slice(&x.to_le_bytes());
    bus.ewram[CAMERA + 2..CAMERA + 4].copy_from_slice(&y.to_le_bytes());
    let e = &bus.ewram;
    let origin = (i16::from_le_bytes([e[FIELD_ORIGIN], e[FIELD_ORIGIN + 1]]), i16::from_le_bytes([e[FIELD_ORIGIN + 2], e[FIELD_ORIGIN + 3]]));
    let scroll = ((x.wrapping_sub(origin.0) as u16 & 511).to_le_bytes(), (y.wrapping_sub(origin.1) as u16 & 511).to_le_bytes());
    for bg in 0..4 {
        let cnt = half(&bus.io, 8 + bg * 2);
        if !matches!(cnt, 0x4c03 | 0x4e02) || half(&bus.iwram, BG_SHADOW + bg * 2) != cnt { continue; }
        for (at, data) in [(BG_SHADOW + 8 + bg * 4, &mut bus.iwram), (0x10 + bg * 4, &mut bus.io)] {
            data[at..at + 2].copy_from_slice(&scroll.0);
            data[at + 2..at + 4].copy_from_slice(&scroll.1);
        }
    }
}
/// What the player is choosing (a list, a cell): the game places the camera
/// anew when it changes.
pub(crate) fn choosing(bus: &MemMap) -> Option<u32> {
    if let Some(m) = battle::MenuScene::read(bus) { return Some(0x1_0000 | (title_hash(bus, m.origin).unwrap_or(0) as u32 & 0xff00) | m.count as u32); }
    battle::Scene::read(bus).map(|s| 0x2_0000 | (s.cells.iter().any(|c| c.range > 0) as u32) << 8 | s.selected.0 as u32 * 16 + s.selected.1 as u32)
}

/// The title of the list window whose first entry is at native `origin`.
pub fn title_hash(bus: &MemMap, origin: [i32; 2]) -> Option<u64> {
    let cnt = half(&bus.io, 0x0c);
    let (map, chars) = (((cnt >> 8) & 31) as usize * 0x800, ((cnt >> 2) & 3) as usize * 0x4000);
    let scroll = half(&bus.io, 0x18) as i32;
    let row = (origin[1] / 8 - 1).rem_euclid(32) as usize;
    let column = ((origin[0] + scroll) / 8).rem_euclid(32) as usize;
    let entry = |c: usize| half(&bus.vram, map + (row * 32 + c % 32) * 2);
    // The window's left edge, then the title's glyph tiles up to its bar.
    if entry(column + 2) & 0x3ff != 0x007 { return None; }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut tiles = 0;
    for c in column + 3..column + 16 {
        let e = entry(c);
        if e == 0 || matches!(e & 0x3ff, 0x007 | 0x008) { break; }
        let at = chars + (e & 0x3ff) as usize * 32;
        for &b in bus.vram.get(at..at + 32)? { hash = (hash ^ b as u64).wrapping_mul(0x100_0000_01b3); }
        tiles += 1;
    }
    (tiles > 0).then_some(hash)
}

/// The battle window shown with its list (None: no battle list).
pub fn window(bus: &MemMap) -> Option<(Window, battle::MenuScene)> {
    if !bus.ffta_dialogue.action_layout { return None; }
    let scene = battle::MenuScene::read(bus)?;
    let hash = title_hash(bus, scene.origin);
    let kind = TITLES.iter().find(|t| Some(t.0) == hash).map_or(Window::Other, |t| t.1);
    Some((kind, scene))
}

/// The law list over a battle (System, Laws): its pointer (measured: tile
/// 0x2dc, 8x16, priority 0, at x 3 beside the first row).
pub fn law_list(bus: &MemMap) -> bool {
    bus.oam.chunks_exact(8).any(|o| half(o, 0) & 0x300 != 0x200 && (half(o, 0) & 255) < 160 && half(o, 4) & 0xfff == 0x2dc)
}
/// A unit's Status in battle (the shared unit screen; no party graph menu).
pub fn status(bus: &MemMap) -> bool {
    bus.menu_screen.frame && bus.menu_screen.template == gba_core::menu_screen::Template::Unit && battle::battle_cursor(bus)
}

impl Engine {
    /// System from the command menu (B, then START) or the free cursor (START).
    pub(crate) fn open_system(&mut self) {
        self.mouse.cancel();
        let from_menu = window(&self.machine.bus).is_some_and(|w| w.0 == Window::Commands);
        self.system_return = from_menu;
        if from_menu { battle::press_sequence(&mut self.machine.bus, &[B, START]); } else { battle::press(&mut self.machine.bus, START); }
    }
    /// Leaves System: back to where it was opened (the unit's commands).
    pub(crate) fn close_system(&mut self) {
        self.mouse.cancel();
        battle::press(&mut self.machine.bus, B);
        if std::mem::take(&mut self.system_return) { self.free_return_after = Some(self.machine.bus.frames + 4); }
    }
    /// Once a frame: finishes leaving System for the unit's commands once the
    /// free cursor is back.
    /// A pan began (gestures.rs): where the game had the camera.
    pub(crate) fn note_pan(&mut self, from: (i16, i16)) {
        if self.pan_home.is_none() { self.pan_home = choosing(&self.machine.bus).map(|k| (from, k)); }
    }
    /// Panned away from where the game put the camera (zoomed out, Zoom in
    /// brings it back).
    fn panned(&self) -> bool {
        !self.zoomed_out() && self.pan_home.is_some_and(|(home, _)| { let c = camera(&self.machine.bus); (c.0 - home.0).abs() + (c.1 - home.1).abs() > 8 })
    }
    /// Center: the camera glides back to where the game had it.
    pub(crate) fn recenter(&mut self) { if let Some(home) = self.pan_home { self.glide = Some(home.0); } }
    /// Once a frame: forgets the camera's home once the game moves on, and
    /// glides the camera toward it (a third of the way a frame).
    fn drive_camera(&mut self) {
        let bus = &mut self.machine.bus;
        let key = choosing(bus);
        if self.pan_home.is_some_and(|(_, k)| Some(k) != key) { self.pan_home = None; self.glide = None; }
        let Some(to) = self.glide else { return };
        let c = camera(bus);
        let step = |a: i16, b: i16| { let d = b as i32 - a as i32; (a as i32 + if d.abs() <= 2 { d } else { d / 3 + d.signum() }) as i16 };
        let next = (step(c.0, to.0), step(c.1, to.1));
        set_camera(bus, next);
        // Back home: nothing to come back to (zoomed out, the home is kept).
        if next == to { self.glide = None; if self.pan_home.is_some_and(|h| h.0 == to) { self.pan_home = None; } }
    }
    pub(crate) fn drive_battle_ui(&mut self) {
        self.drive_zoom_camera();
        self.drive_camera();
        if let Some(deadline) = self.inspect_status {
            let bus = &self.machine.bus;
            if bus.frames > deadline { self.inspect_status = None; }
            else if battle::idle(bus) && window(bus).is_some_and(|(w, s)| w == Window::Commands && s.count == 1) {
                self.inspect_status = None;
                battle::press(&mut self.machine.bus, A);
            }
        }
        if let Some((cell, deadline)) = self.inspect {
            let bus = &self.machine.bus;
            if bus.frames > deadline { self.inspect = None; }
            else if self.free_cursor() && battle::idle(bus) {
                self.inspect = None;
                // The free cursor starts on the unit whose turn it is: that
                // one tapped goes straight back to its commands.
                if Some(cell) == self.free_home { self.free_return(); } else { battle::select(&mut self.machine.bus, cell.0, cell.1, false); }
            }
        }
        if let Some(at) = self.free_return_after {
            let bus = &self.machine.bus;
            if bus.frames > at + 120 { self.free_return_after = None; }
            else if bus.frames >= at && self.free_cursor() && battle::idle(bus) { self.free_return_after = None; self.free_return(); }
        }
    }
    /// A tap on a unit on the field while the unit whose turn it is chooses
    /// a command: that unit is looked at (B for the free cursor, which then
    /// goes to it: its panel shows; Return comes back). True when taken.
    pub(crate) fn inspect_tap(&mut self, x: f32, y: f32) -> bool {
        let bus = &self.machine.bus;
        if !window(bus).is_some_and(|w| w.0 == Window::Commands) || !battle::idle(bus) { return false; }
        // Its cell: B moves the camera back to the unit whose turn it is.
        let Some(cell) = battle::unit_cell_at(bus, x, y) else { return false };
        self.mouse.cancel();
        self.free_home = None;
        battle::press(&mut self.machine.bus, B);
        self.inspect = Some((cell, self.machine.bus.frames + 90));
        true
    }
    /// A tap at native (x, y) in the free cursor: whether it confirms (A).
    /// A unit is looked at first (the cursor goes to it, its panel shows);
    /// a tap on the unit it rests on then confirms: the game offers that
    /// unit's one command, Status, which is chosen (`inspect_status`); on the
    /// unit whose turn it is, its commands. An empty cell is only looked at.
    /// None outside the free cursor.
    pub(crate) fn free_cursor_confirm(&mut self, x: f32, y: f32) -> Option<bool> {
        let bus = &self.machine.bus;
        if !self.free_cursor() { return None; }
        let scene = battle::Scene::read(bus)?;
        let cell = scene.pick(bus, x, y).map(|c| (c.x, c.y));
        let on = cell == Some(scene.selected);
        let unit = battle::unit_cell_at(bus, x, y).is_some();
        if on && cell != self.free_home && unit { self.inspect_status = Some(bus.frames + 60); }
        Some(on && (cell == self.free_home || unit))
    }
    /// The mission's pages over a battle (System, Mission): the corner
    /// arrows turn them, as in the pub (pub_touch.rs). True when taken.
    pub(crate) fn battle_pages_tap(&mut self, x: f32, y: f32) -> bool {
        let bus = &self.machine.bus;
        if !battle::battle_cursor(bus) || !crate::pub_touch::mission_pages(bus) { return false; }
        let key = if y < 28. && x < 48. { Some(LEFT) } else if y < 28. && x >= 192. { Some(RIGHT) } else { None };
        if let Some(key) = key { self.mouse.cancel(); battle::press(&mut self.machine.bus, key); }
        true
    }
    /// The battle's buttons, or None outside battles.
    pub(crate) fn battle_buttons(&self) -> Option<Vec<crate::game_buttons::Button>> {
        use crate::game_buttons::{row, Button};
        let bus = &self.machine.bus;
        let rom = &bus.rom;
        let top = |items: &[(&'static str, u16, bool)]| -> Vec<Button> { row(rom, 236, items).into_iter().map(|b| Button { top: true, ..b }).collect() };
        if status(bus) {
            // Its own Return is at the bottom (menu_layout). On the Abilities
            // page (the game's "R Info" hint shown) R shows what is being
            // learned; R again goes back (measured: hint tiles 0xdd0/0xdd4 at
            // y 62; the learning view's arrows tile 0x1c4 at y 60).
            let sprite = |tile: u16, y: u8| bus.oam.chunks_exact(8).any(|o| half(o, 0) & 0x300 != 0x200 && half(o, 4) == tile && o[0] == y);
            // B (or A) leaves it for the Abilities page; Left/Right there turn to
            // the other jobs' abilities (the swipes).
            if sprite(0x1c4, 60) { return Some(top(&[("Learning", B, true)])); }
            if sprite(0xdd0, 62) { return Some(top(&[("Learning", 0x2ff, false)])); }
            return Some(Vec::new());
        }
        // The mission's pages and the law list (System): back to System.
        if battle::battle_cursor(bus) && (crate::pub_touch::mission_pages(bus) || law_list(bus)) { return Some(top(&[("Return", B, false)])); }
        // Zoom, top left: where the field is chosen on.
        let zoom = || -> Vec<Button> {
            if !self.zoomable() { return Vec::new(); }
            let label = if self.zoomed_out() { "Zoom in" } else { "Zoom out" };
            top(&[(label, crate::battle_zoom::ZOOM, false)]).into_iter()
                .map(|b| Button { x: (4, 4 + b.x.1 - b.x.0), ..b }).collect()
        };
        if let Some((kind, _)) = window(bus) {
            if kind == Window::Commands {
                let mut buttons = if self.panned() { top(&[("System", OPEN_SYSTEM, false), ("Center", RECENTER, false)]) } else { top(&[("System", OPEN_SYSTEM, false)]) };
                buttons.extend(zoom());
                return Some(buttons);
            }
            return Some(match kind {
                // The unit's commands: nothing to go back to (units are
                // looked at by tapping them); the System window.
                Window::System => top(&[("Return", CLOSE_SYSTEM, false)]),
                _ => top(&[("Return", B, false)]),
            });
        }
        let facing = battle::facing_center(bus).is_some();
        if battle::Scene::read(bus).is_some() || facing {
            let back = if self.free_cursor() { crate::engine::FREE_HOME } else { B };
            let mut buttons = if self.free_cursor() { top(&[("System", OPEN_SYSTEM, false), ("Return", back, false)]) }
                else if self.panned() { top(&[("Center", RECENTER, false), (if facing { "Back" } else { "Return" }, back, false)]) }
                else { top(&[(if facing { "Back" } else { "Return" }, back, false)]) };
            // Facing: a tap toward a side turns the unit there and ends its
            // turn; the button ends it facing as it is.
            if facing { buttons.extend(row(rom, 236, &[("Keep facing", A, false)])); }
            buttons.extend(zoom());
            return Some(buttons);
        }
        // Other windows over the battle (the unit list, Options, Mission).
        if bus.ffta_dialogue.action_layout && battle::MenuScene::read(bus).is_some() { return Some(top(&[("Return", B, false)])); }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_field_scrolls_with_the_camera_on_its_own_layers() {
        let mut m = gba_core::Machine::new(vec![0; 0x200]);
        let bus = &mut m.bus;
        // Measured with the commands shown: the field on BG0-1, origin -160,264.
        for (bg, cnt) in [0x4c03u16, 0x4e02, 0x5c08, 0x4a09].into_iter().enumerate() {
            bus.io[8 + bg * 2..10 + bg * 2].copy_from_slice(&cnt.to_le_bytes());
            bus.iwram[BG_SHADOW + bg * 2..BG_SHADOW + 2 + bg * 2].copy_from_slice(&cnt.to_le_bytes());
        }
        bus.ewram[FIELD_ORIGIN..FIELD_ORIGIN + 4].copy_from_slice(&[0x60, 0xff, 0x08, 0x01]);
        set_camera(bus, (-55, 274));
        assert_eq!(camera(bus), (-55, 274));
        for bg in 0..2 {
            assert_eq!((half(&bus.io, 0x10 + bg * 4), half(&bus.io, 0x12 + bg * 4)), (105, 10));
            assert_eq!((half(&bus.iwram, BG_SHADOW + 8 + bg * 4), half(&bus.iwram, BG_SHADOW + 10 + bg * 4)), (105, 10));
        }
        // The lists' layers stay.
        assert_eq!(half(&bus.io, 0x18), 0);
        set_camera(bus, (-170, 260));
        assert_eq!((half(&bus.io, 0x10), half(&bus.io, 0x12)), (502, 508));
    }
    #[test]
    fn a_window_is_told_by_its_title_glyphs() {
        let mut m = gba_core::Machine::new(vec![0; 0x200]);
        // BG2 as in battle: map 0xe000, characters 0x8000, scrolled 512 (none).
        m.bus.io[0x0c..0x0e].copy_from_slice(&0x5c08u16.to_le_bytes());
        m.bus.io[0x18..0x1a].copy_from_slice(&512u16.to_le_bytes());
        let put = |m: &mut gba_core::Machine, col: usize, row: usize, e: u16| { let o = 0xe000 + (row * 32 + col) * 2; m.bus.vram[o..o + 2].copy_from_slice(&e.to_le_bytes()); };
        // The command menu's first entry at 160,84: its title row 9, edge at column 22.
        assert_eq!(title_hash(&m.bus, [160, 84]), None);
        put(&mut m, 22, 9, 0xc007);
        assert_eq!(title_hash(&m.bus, [160, 84]), None);
        put(&mut m, 23, 9, 0xc03e);
        put(&mut m, 24, 9, 0xc008);
        let blank = title_hash(&m.bus, [160, 84]);
        assert!(blank.is_some());
        // Other glyph pixels: another title.
        m.bus.vram[0x8000 + 0x3e * 32] = 0x11;
        assert!(title_hash(&m.bus, [160, 84]).is_some() && title_hash(&m.bus, [160, 84]) != blank);
    }
}
