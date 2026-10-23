//! Touch for the pub's mission screens and the game's Yes/No notices (USA,
//! measured in Sprohm's pub), which show no menu pointer to tap along:
//!
//! - Mission details: a panel of pages (description, reward, requirements,
//!   fee) with the game's page arrows in its top corners (OAM tile 0x12b,
//!   palette 1, priority 0, 16x16, rows 0-15; they bob between x 14 and 16,
//!   208 and 210). Left/Right turn pages, A goes on (takes the mission, or
//!   its fee page asks to accept), B goes back to the list.
//! - Notices ("Accept these conditions?"): a window titled NOTICE (BG3 tiles
//!   0xb04a-0xb04d in a row) with "Yes / No" three tile rows under the title
//!   and the game's ring left of Yes (measured: title at tile column 3, row
//!   2; ring, a blinking sprite, at x 27, y 44). The ring never moves: A is
//!   Yes, B is No.
//! - A rumor's text: a window titled RUMORS (BG2: its title six tiles from
//!   0xc03e after the window's 0xc007 edge, "MISSION" five, "LIST" three)
//!   with no pointer. Its last page shows no arrow; A closes it.
//! - Dialogue waiting for a button shows the game's arrow (8x16, palette 2,
//!   tile 0x122): a tap anywhere moves it on, also over a menu that is shown
//!   meanwhile (rumors, the barkeep's lines).
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, mem::MemMap};

const A: u16 = 0x3fe;
const B: u16 = 0x3fd;
const LEFT: u16 = 0x3df;
const RIGHT: u16 = 0x3ef;
const UP: u16 = 0x3bf;
const DOWN: u16 = 0x37f;

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

/// Shown sprites: (x, y, attribute 0, attribute 2).
fn sprites(bus: &MemMap) -> impl Iterator<Item = (i32, i32, u16, u16)> + '_ {
    bus.oam.chunks_exact(8).filter_map(|o| {
        let (a0, a1, a2) = (half(o, 0), half(o, 2), half(o, 4));
        let (x, y) = ((a1 & 511) as i32, (a0 & 255) as i32);
        (a0 & 0x300 != 0x200 && y < 160 && x < 240).then_some((x, y, a0, a2))
    })
}

/// Dialogue waits for a button (its arrow is shown).
pub fn continue_arrow(bus: &MemMap) -> bool {
    sprites(bus).any(|(_, _, a0, a2)| a0 & 0xff00 == 0x8000 && a2 == 0x2122)
}

/// The mission details' page arrows are shown.
pub fn mission_pages(bus: &MemMap) -> bool {
    let arrow = |left: bool| sprites(bus).any(|(x, y, a0, a2)| a2 & 0xf3ff == 0x112b && a0 >> 14 == 0 && y < 16 && if left { x < 40 } else { x >= 192 });
    arrow(true) && arrow(false)
}

/// The Yes/No line of a notice: the game's ring left of Yes (native x, y).
pub fn notice(bus: &MemMap) -> Option<(i32, i32)> {
    let cnt = half(&bus.io, 0x0e);
    if cnt & 0x8000 != 0 || half(&bus.io, 0x1c) & 511 != 0 || half(&bus.io, 0x1e) & 511 != 0 { return None; }
    let base = ((cnt >> 8) & 31) as usize * 0x800;
    let entry = |col: usize, row: usize| half(&bus.vram, base + (row * 32 + col) * 2);
    (0..18).find_map(|row| (0..26).find(|&col| (0..4).all(|i| entry(col + i, row) == 0xb04a + i as u16))
        .map(|col| (col as i32 * 8 + 3, row as i32 * 8 + 28)))
}

/// A rumor's text is shown.
pub fn rumor_text(bus: &MemMap) -> bool {
    let cnt = half(&bus.io, 0x0c);
    if cnt & 0x8000 != 0 || half(&bus.io, 0x18) & 511 != 0 || half(&bus.io, 0x1a) & 511 != 0 || crate::engine::party_cursor(bus).is_some() { return false; }
    let base = ((cnt >> 8) & 31) as usize * 0x800;
    let entry = |col: usize, row: usize| half(&bus.vram, base + (row * 32 + col) * 2);
    (0..18).any(|row| (0..22).any(|col| entry(col, row) == 0xc007
        && (0..6).all(|i| entry(col + 1 + i, row) == 0xc03e + i as u16) && entry(col + 7, row) == 0xc008))
}

impl Engine {
    /// Pub screens without a pointer: buttons for the bottom row.
    pub(crate) fn pub_buttons(&self) -> Option<Vec<crate::game_buttons::Button>> {
        use crate::game_buttons::row;
        let bus = &self.machine.bus;
        if notice(bus).is_some() { return Some(row(&bus.rom, 236, &[("Yes", A, false), ("No", B, false)])); }
        if mission_pages(bus) { return Some(row(&bus.rom, 236, &[("OK", A, false), ("Return", B, false)])); }
        None
    }
    /// A tap at native (x, y) on a pub screen without a pointer, or on dialogue
    /// waiting for a button. True when it was taken.
    pub(crate) fn pub_tap(&mut self, x: f32, y: f32) -> bool {
        let bus = &mut self.machine.bus;
        let key = if let Some((rx, ry)) = notice(bus) {
            // "Yes" right of the ring, "No" after it; the rest of the notice does nothing.
            let row = y >= (ry - 6) as f32 && y < (ry + 14) as f32;
            if row && x >= (rx - 4) as f32 && x < (rx + 36) as f32 { Some(A) }
            else if row && x >= (rx + 36) as f32 && x < (rx + 80) as f32 { Some(B) }
            else { None }
        } else if mission_pages(bus) {
            // The page arrows (and the corners around them).
            if y < 28. && x < 48. { Some(LEFT) } else if y < 28. && x >= 192. { Some(RIGHT) } else { None }
        } else if continue_arrow(bus) || rumor_text(bus) {
            Some(A)
        } else {
            return false;
        };
        if let Some(key) = key { self.mouse.cancel(); battle::press(&mut self.machine.bus, key); }
        true
    }
    /// A menu with the game's pointer but no list the other adapters know (the
    /// pub's mission item slots, "<--->", "<--->", "OK"): rows 16 apart with the
    /// pointer at the right of its row's text. A tap on a row moves the pointer
    /// there one Up/Down at a time, then A. True when it was taken.
    pub(crate) fn pointer_menu_tap(&mut self, x: f32, y: f32) -> bool {
        let bus = &self.machine.bus;
        let Some((cx, cy)) = crate::engine::party_cursor(bus) else { return false };
        if cx < 48 || x < (cx - 96) as f32 || x > (cx + 16) as f32 { return false; }
        let steps = ((y - (cy - 4) as f32) / 16.).floor() as i32;
        if !(-6..=6).contains(&steps) { return false; }
        self.mouse.cancel();
        let mut keys = vec![if steps < 0 { UP } else { DOWN }; steps.unsigned_abs() as usize];
        keys.push(A);
        battle::press_sequence(&mut self.machine.bus, &keys);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn machine() -> gba_core::Machine {
        let mut m = gba_core::Machine::new(vec![0; 0x200]);
        for slot in 0..128 { m.bus.oam[slot * 8..slot * 8 + 2].copy_from_slice(&0x0200u16.to_le_bytes()); }
        // BG2 and BG3 as in the pub (screen bases 0x5800, 0x5000).
        m.bus.io[0x0c..0x0e].copy_from_slice(&0x0b08u16.to_le_bytes());
        m.bus.io[0x0e..0x10].copy_from_slice(&0x0a08u16.to_le_bytes());
        m
    }
    fn sprite(m: &mut gba_core::Machine, slot: usize, a: [u16; 3]) {
        for (k, v) in a.iter().enumerate() { m.bus.oam[slot * 8 + 2 * k..slot * 8 + 2 * k + 2].copy_from_slice(&v.to_le_bytes()); }
    }
    fn tiles(m: &mut gba_core::Machine, base: usize, col: usize, row: usize, entries: &[u16]) {
        for (i, e) in entries.iter().enumerate() { let o = base + (row * 32 + col + i) * 2; m.bus.vram[o..o + 2].copy_from_slice(&e.to_le_bytes()); }
    }
    #[test]
    fn pub_screens_are_told_by_what_the_game_draws() {
        let mut m = machine();
        assert!(!mission_pages(&m.bus) && notice(&m.bus).is_none() && !rumor_text(&m.bus) && !continue_arrow(&m.bus));
        // Page arrows in both top corners (measured: palette 1, tile 0x12b, x 14 and 210).
        sprite(&mut m, 0, [0x0006, 0x400e, 0x112b]);
        assert!(!mission_pages(&m.bus));
        sprite(&mut m, 1, [0x0006, 0x40d2, 0x112b]);
        assert!(mission_pages(&m.bus));
        // NOTICE title at tile column 3, row 2: Yes / No three rows under it.
        tiles(&mut m, 0x5000, 3, 2, &[0xb04a, 0xb04b, 0xb04c, 0xb04d]);
        assert_eq!(notice(&m.bus), Some((27, 44)));
        // RUMORS: six title tiles between the window's edges.
        tiles(&mut m, 0x5800, 2, 2, &[0xc007, 0xc03e, 0xc03f, 0xc040, 0xc041, 0xc042, 0xc043, 0xc008]);
        assert!(rumor_text(&m.bus));
        // MISSION is five: not a rumor.
        tiles(&mut m, 0x5800, 2, 2, &[0xc007, 0xc03e, 0xc03f, 0xc040, 0xc041, 0xc042, 0xc008, 0xc008]);
        assert!(!rumor_text(&m.bus));
        // Dialogue's arrow.
        sprite(&mut m, 2, [0x808e, 0x0074, 0x2122]);
        assert!(continue_arrow(&m.bus));
    }
}
