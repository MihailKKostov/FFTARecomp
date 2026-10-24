//! Touch for choosing the units sent into battle (USA, measured on Herb
//! Picking). The game shows two views:
//! - The roster: the unit offered in a ring at the bottom right (sprites from
//!   native 172,104 to 220,152), the game's arrows beside the field (OAM tile
//!   0x2e0, palette 1, at x 6 and 218, y 88) to offer another, and its hints
//!   at the top ("L View", "START: To Battle!" from tile 0x314, "R Info").
//!   Left/Right offer another unit, A sends it (the cursor then carries it
//!   onto the field), START asks "Begin battle?", R shows the unit.
//! - Dispatch: the cursor carries the unit (the "Dispatch MODE" label, tiles
//!   0x300 and 0x308, at the bottom right). A tap on a blue cell places it and
//!   the game asks for its facing (both taken by the battle adapter); B puts
//!   it back.
//! Neither view has a blind A: a stray tap must not send a unit.
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, mem::MemMap};

const A: u16 = 0x3fe;
const B: u16 = 0x3fd;
const START: u16 = 0x3f7;
const R: u16 = 0x2ff;
const LEFT: u16 = 0x3df;
const RIGHT: u16 = 0x3ef;

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

/// Shown sprites: (x, y, attribute 2).
fn sprites(bus: &MemMap) -> impl Iterator<Item = (i32, i32, u16)> + '_ {
    bus.oam.chunks_exact(8).filter_map(|o| {
        let (a0, a1, a2) = (half(o, 0), half(o, 2), half(o, 4));
        (a0 & 0x300 != 0x200).then_some(((a1 & 511) as i32, (a0 & 255) as i32, a2))
    })
}

/// The roster is shown: the arrows' native x (left, right).
pub fn roster(bus: &MemMap) -> Option<(i32, i32)> {
    if !sprites(bus).any(|(_, y, a2)| a2 == 0x314 && y < 32) { return None; }
    let arrows: Vec<i32> = sprites(bus).filter(|&(_, y, a2)| a2 & 0x3ff == 0x2e0 && (80..100).contains(&y)).map(|(x, _, _)| x).collect();
    Some((*arrows.iter().min()?, *arrows.iter().max()?))
}

/// The cursor carries a unit onto the field.
pub fn dispatch(bus: &MemMap) -> bool {
    let label = |tile: u16| sprites(bus).any(|(_, y, a2)| a2 & 0x3ff == tile && y >= 112);
    label(0x300) && label(0x308)
}

/// Either view of choosing units.
pub fn shown(bus: &MemMap) -> bool { roster(bus).is_some() || dispatch(bus) }

impl Engine {
    /// The roster's buttons: Info and Return at the top, Place and Battle at
    /// the bottom right (the unit's panel is at the bottom left).
    pub(crate) fn deploy_buttons(&self) -> Option<Vec<crate::game_buttons::Button>> {
        use crate::game_buttons::{row, Button};
        let rom = &self.machine.bus.rom;
        roster(&self.machine.bus)?;
        let mut buttons = row(rom, 236, &[("Place", A, false), ("Battle", START, false)]);
        buttons.extend(row(rom, 236, &[("Info", R, false), ("Return", B, false)]).into_iter().map(|b| Button { top: true, ..b }));
        Some(buttons)
    }
    /// A tap at native (x, y) while choosing units: the roster's arrows offer
    /// another unit, its ring sends the one offered. True while choosing
    /// units and nothing on the field is meant (no blind A).
    pub(crate) fn deploy_tap(&mut self, x: f32, y: f32) -> bool {
        let bus = &self.machine.bus;
        let Some((left, right)) = roster(bus) else { return false };
        let key = if (76. ..116.).contains(&y) && x >= (left - 10) as f32 && x < (left + 26) as f32 { Some(LEFT) }
            else if (76. ..116.).contains(&y) && x >= (right - 10) as f32 && x < (right + 26) as f32 { Some(RIGHT) }
            else if (x - 196.).hypot(y - 128.) < 30. { Some(A) }
            else { None };
        if let Some(key) = key { self.mouse.cancel(); battle::press(&mut self.machine.bus, key); }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sprite(m: &mut gba_core::Machine, slot: usize, a: [u16; 3]) {
        for (k, v) in a.iter().enumerate() { m.bus.oam[slot * 8 + 2 * k..slot * 8 + 2 * k + 2].copy_from_slice(&v.to_le_bytes()); }
    }
    #[test]
    fn the_roster_and_dispatch_are_told_by_their_hints() {
        let mut m = gba_core::Machine::new(vec![0; 0x200]);
        for slot in 0..128 { sprite(&mut m, slot, [0x0200, 0, 0]); }
        assert!(!shown(&m.bus));
        // Measured: "START" hint, the arrows at x 6 and 218.
        sprite(&mut m, 0, [0x4011, 0x4050, 0x314]);
        assert_eq!(roster(&m.bus), None);
        sprite(&mut m, 1, [0x0058, 0x5006, 0x12e0]);
        sprite(&mut m, 2, [0x0058, 0x40da, 0x12e0]);
        assert_eq!(roster(&m.bus), Some((6, 218)));
        for slot in 0..3 { sprite(&mut m, slot, [0x0200, 0, 0]); }
        // "Dispatch MODE".
        sprite(&mut m, 0, [0x4080, 0x80c3, 0x300]);
        assert!(!dispatch(&m.bus));
        sprite(&mut m, 1, [0x8080, 0x00e3, 0x308]);
        assert!(dispatch(&m.bus) && roster(&m.bus).is_none());
    }
}
