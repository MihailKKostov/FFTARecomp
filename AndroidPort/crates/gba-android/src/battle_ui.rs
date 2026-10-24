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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window { Commands, Action, Abilities, Items, System, Other }

/// Title hashes (FNV-1a over the title tiles' 4-bpp pixels).
const TITLES: [(u64, Window); 5] = [
    (0x5952_a0a2_7e52_8322, Window::Commands),
    (0x763c_d545_cd71_d754, Window::Action),
    (0xc31e_3209_61ea_b9ca, Window::Abilities),
    (0x14f7_b225_fa64_6b85, Window::Items),
    (0xba3d_5307_69f2_1ebf, Window::System),
];

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

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
    pub(crate) fn drive_battle_ui(&mut self) {
        if let Some(at) = self.free_return_after {
            let bus = &self.machine.bus;
            if bus.frames > at + 120 { self.free_return_after = None; }
            else if bus.frames >= at && self.free_cursor() && battle::idle(bus) { self.free_return_after = None; self.free_return(); }
        }
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
        if let Some((kind, _)) = window(bus) {
            return Some(match kind {
                // The unit's commands: nothing to go back to (units are
                // looked at by tapping them); the System window.
                Window::Commands => top(&[("System", OPEN_SYSTEM, false)]),
                Window::System => top(&[("Return", CLOSE_SYSTEM, false)]),
                _ => top(&[("Return", B, false)]),
            });
        }
        let facing = battle::facing_center(bus).is_some();
        if battle::Scene::read(bus).is_some() || facing {
            let back = if self.free_cursor() { crate::engine::FREE_HOME } else { B };
            let mut buttons = if self.free_cursor() { top(&[("System", OPEN_SYSTEM, false), ("Return", back, false)]) }
                else { top(&[(if facing { "Back" } else { "Return" }, back, false)]) };
            // Facing: a tap toward a side turns the unit there and ends its
            // turn; the button ends it facing as it is.
            if facing { buttons.extend(row(rom, 236, &[("Keep facing", A, false)])); }
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
