//! Host-only world map HUD capture for tall (portrait) views.
//!
//! Measured on the USA world map: BG0/BG1 hold the 256x512 map, BG2/BG3 the
//! static HUD and town menus, and the HUD sprites are priority-0 objects with
//! tiles 0x40-0x4b ("L Law", "R Info") and 0x5e-0x71 (the date label). When
//! enabled, those pixels are moved out of the picture into `pixels` so the
//! frontend can pin them to the screen edges while the map stays in place.
//! Town menus and popups share BG2/BG3, so only BG pixels inside the measured
//! HUD boxes are lifted; menus (and their opening animations) stay in the
//! picture and compose as usual while the HUD remains pinned.
use crate::mem::MemMap;

pub const TRANSPARENT: u16 = 0xFFFF;

#[derive(Default)]
pub struct WorldHud {
    /// Requested by the frontend (portrait/square views only).
    pub enabled: bool,
    /// The current frame is a tall world map frame: the HUD is lifted out,
    /// `pixels` is valid and the map presentation applies.
    pub map_frame: bool,
    /// 240x160 RGB555 HUD picture, TRANSPARENT where empty.
    pub pixels: Vec<u16>,
    boxes: Vec<[i32; 4]>,
}

fn half(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}

const OBJ_SIZES: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

pub fn hud_object(a0: u16, a2: u16) -> bool {
    let tile = a2 & 1023;
    a0 & 0x300 == 0 && (a2 >> 10) & 3 == 0 && ((0x40..=0x4b).contains(&tile) || (0x5e..=0x71).contains(&tile))
}

/// Line 0: decide whether this frame is captured and collect HUD object boxes.
pub fn begin_frame(mem: &mut MemMap, world_map: bool) {
    let hud = &mut mem.world_hud;
    hud.map_frame = hud.enabled && world_map && crate::mem::extra_y() > 0;
    hud.boxes.clear();
    if !hud.map_frame { return; }
    hud.pixels.clear();
    hud.pixels.resize(240 * 160, TRANSPARENT);
    for entry in mem.oam.chunks_exact(8) {
        let (a0, a1, a2) = (half(entry, 0), half(entry, 2), half(entry, 4));
        let shape = (a0 >> 14) as usize;
        if shape == 3 || !hud_object(a0, a2) { continue; }
        let (w, h) = OBJ_SIZES[shape][(a1 >> 14) as usize];
        let mut y = (a0 & 255) as i32;
        if y >= 160 { y -= 256; }
        let mut x = (a1 & 511) as i32;
        if x >= 240 { x -= 512; }
        mem.world_hud.boxes.push([x, y, x + w, y + h]);
    }
}

/// Measured HUD blocks (native x0, y0, x1, y1): "L Law", "R Info", Clan Funds
/// (room for more digits) and the date. BG2/BG3 pixels elsewhere are menus.
const HUD_BOXES: [[i32; 4]; 4] = [[0, 0, 40, 26], [200, 0, 240, 26], [0, 132, 100, 160], [144, 140, 240, 160]];

pub fn in_hud_box(x: i32, y: i32) -> bool {
    HUD_BOXES.iter().any(|b| x >= b[0] && x < b[2] && y >= b[1] && y < b[3])
}

pub fn in_hud_object(mem: &MemMap, x: i32, y: i32) -> bool {
    mem.world_hud.boxes.iter().any(|b| x >= b[0] && x < b[2] && y >= b[1] && y < b[3])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_measured_priority_zero_hud_tiles_are_captured() {
        assert!(hud_object(0x4004, 0x0040));
        assert!(hud_object(0x0091, 0x406e));
        // Town icons (priority 2), the mail icon (tile 2c) and labels (priority 1) stay in the map.
        assert!(!hud_object(0x0030, 0x98d2));
        assert!(!hud_object(0x0018, 0x002c));
        assert!(!hud_object(0x4029, 0x044c));
        // Hidden objects are ignored.
        assert!(!hud_object(0x0204, 0x0040));
    }
}
