//! Portrait world map presentation (USA measurements; see gba-core world_hud.rs).
//!
//! The 256x512 map (content in rows 0..352) is rendered beyond the native
//! 160 rows. The game scrolls its camera (BG0VOFS) and only draws towns near
//! it, so the presented texture is shifted to keep the whole map still, towns
//! the game culled are drawn from pixels remembered while they were visible,
//! and the HUD lifted out by the PPU is pinned to the docking frame's edges.
use gba_core::{mem::{self, MemMap}, world_hud::TRANSPARENT};

const MAP_CENTER: i32 = 176;
const STABLE_FRAMES: u16 = 30;
const MISSING_FRAMES: u16 = 20;
/// Map margins: a dark tone instead of the flat blue backdrop.
const MARGIN: i32 = 0xff14231cu32 as i32;

struct Town { x: i32, y: i32, key: u16, pixels: Vec<i32>, stable: u16, missing: u16, seen: bool }

#[derive(Default)]
pub struct WorldMap { towns: Vec<Town> }

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

/// A 32x32 priority-2 object: placed towns and similar static map icons.
fn town_object(a0: u16, a1: u16, a2: u16) -> bool {
    a0 & 0x300 == 0 && a0 >> 14 == 0 && a1 >> 14 == 2 && (a2 >> 10) & 3 == 2
}

/// Decoded 32x32 object pixels (ARGB, 0 = transparent) from VRAM and OBJ palette.
fn decode(bus: &MemMap, a0: u16, a1: u16, a2: u16) -> Vec<i32> {
    let colors = crate::engine::rgba_colors();
    let one_d = half(&bus.io, 0) & 0x40 != 0;
    let bpp8 = a0 & 0x2000 != 0;
    let (tile, palette) = ((a2 & 1023) as usize, (a2 >> 12) as usize);
    let (hflip, vflip) = (a1 & 0x1000 != 0, a1 & 0x2000 != 0);
    let mut out = vec![0; 32 * 32];
    for y in 0..32usize {
        for x in 0..32usize {
            let (sx, sy) = (if hflip { 31 - x } else { x }, if vflip { 31 - y } else { y });
            let (tx, ty) = (sx / 8, sy / 8);
            let step = if bpp8 { 2 } else { 1 };
            let index = if one_d { tile + (ty * 4 + tx) * step } else { tile + ty * 32 + tx * step };
            let base = 0x10000 + (index & 1023) * 32;
            let color = if bpp8 {
                let i = bus.vram.get(base + (sy % 8) * 8 + sx % 8).copied().unwrap_or(0) as usize;
                (i != 0).then(|| half(&bus.palette, 0x200 + i * 2))
            } else {
                let byte = bus.vram.get(base + (sy % 8) * 4 + (sx % 8) / 2).copied().unwrap_or(0);
                let i = ((byte >> (4 * (sx % 2))) & 15) as usize;
                (i != 0).then(|| half(&bus.palette, 0x200 + palette * 32 + i * 2))
            };
            if let Some(c) = color { out[y * 32 + x] = colors[(c & 0x7fff) as usize]; }
        }
    }
    out
}

impl WorldMap {
    /// Rows the texture is shifted by (displayed row r shows rendered row r + shift).
    pub fn shift(bus: &MemMap) -> i32 {
        let vofs = (half(&bus.io, 0x12) & 0x1ff) as i32;
        MAP_CENTER - mem::render_height() as i32 / 2 + mem::extra_y() as i32 - vofs
    }

    fn remember_towns(&mut self, bus: &MemMap, vofs: i32) {
        for town in &mut self.towns { town.seen = false; }
        for entry in bus.oam.chunks_exact(8) {
            let (a0, a1, a2) = (half(entry, 0), half(entry, 2), half(entry, 4));
            if !town_object(a0, a1, a2) { continue; }
            let mut y = (a0 & 255) as i32;
            if y >= 160 { y -= 256; }
            let mut x = (a1 & 511) as i32;
            if x >= 240 { x -= 512; }
            let (x, y) = (x, y + vofs);
            if let Some(town) = self.towns.iter_mut().find(|t| t.x == x && t.y == y && t.key == a2) {
                town.seen = true; town.missing = 0;
                town.stable = town.stable.saturating_add(1);
                if town.stable == STABLE_FRAMES { town.pixels = decode(bus, a0, a1, a2); }
            } else if self.towns.len() < 64 {
                self.towns.push(Town { x, y, key: a2, pixels: Vec::new(), stable: 1, missing: 0, seen: true });
            }
        }
        // A remembered object that is missing while fully inside the native view is gone.
        self.towns.retain_mut(|t| {
            if t.seen { return true; }
            if t.stable < STABLE_FRAMES { return false; }
            if t.y >= vofs && t.y + 32 <= vofs + 156 { t.missing += 1; } else { t.missing = 0; }
            t.missing < MISSING_FRAMES
        });
    }

    /// Rebuilds `pixels` (render_width x render_height, rendered as usual) into
    /// the still-map presentation. `dock` is the (top, bottom) texture rows the
    /// pinned HUD may occupy.
    pub fn present(&mut self, bus: &MemMap, pixels: &mut Vec<i32>, dock: (i32, i32)) {
        let (rw, rh) = (mem::render_width(), mem::render_height());
        let (ex, ey) = (mem::extra() as i32, mem::extra_y() as i32);
        let vofs = (half(&bus.io, 0x12) & 0x1ff) as i32;
        let shift = Self::shift(bus);
        self.remember_towns(bus, vofs);
        let colors = crate::engine::rgba_colors();
        let backdrop = colors[(half(&bus.palette, 0) & 0x7fff) as usize];
        let mut out = vec![MARGIN; rw * rh];
        for r in 0..rh as i32 {
            let src = r + shift;
            if !(0..rh as i32).contains(&src) { continue; }
            let (o, s) = (r as usize * rw, src as usize * rw);
            for (target, &pixel) in out[o..o + rw].iter_mut().zip(&pixels[s..s + rw]) {
                *target = if pixel == backdrop { MARGIN } else { pixel };
            }
        }
        // Absolute map row a is displayed at row a - first.
        let first = vofs - ey + shift;
        // Culled towns are drawn whole; towns the game still shows only get the
        // rows outside its native view, which it cannot draw.
        for town in self.towns.iter().filter(|t| !t.pixels.is_empty()) {
            for (i, &p) in town.pixels.iter().enumerate() {
                let row = town.y + (i / 32) as i32;
                if p == 0 || (town.seen && (vofs..vofs + 160).contains(&row)) { continue; }
                let (x, y) = (town.x + ex + (i % 32) as i32, row - first);
                if (0..rw as i32).contains(&x) && (0..rh as i32).contains(&y) { out[y as usize * rw + x as usize] = p; }
            }
        }
        // HUD: blocks touching the top or bottom of the native screen are pinned
        // to the docking frame; anything in between (town menus) stays with the map.
        let hud = &bus.world_hud.pixels;
        let row_used = |y: usize| hud[y * 240..(y + 1) * 240].iter().any(|&p| p != TRANSPARENT);
        // While a menu is open the HUD stays in the picture (see world_hud.rs).
        let mut y = if bus.world_hud.active { 0 } else { 160 };
        while y < 160 {
            if !row_used(y) { y += 1; continue; }
            let start = y;
            let mut end = y + 1;
            let mut gap = 0;
            y += 1;
            while y < 160 && gap < 4 { if row_used(y) { end = y + 1; gap = 0 } else { gap += 1 } y += 1; }
            let offset = if start < 32 { dock.0 } else if end > 128 { dock.1 - 160 } else { vofs - first };
            for row in start..end {
                let target = row as i32 + offset;
                if !(0..rh as i32).contains(&target) { continue; }
                for x in 0..240 {
                    let p = hud[row * 240 + x];
                    if p != TRANSPARENT { out[target as usize * rw + (x as i32 + ex) as usize] = colors[(p & 0x7fff) as usize]; }
                }
            }
        }
        *pixels = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn town_objects_are_static_32x32_priority_2_icons_only() {
        assert!(town_object(0x0030, 0x8068, 0x98d2));
        assert!(!town_object(0x0030, 0x4068, 0x98d2));
        assert!(!town_object(0x0032, 0x8070, 0x01d6));
        assert!(!town_object(0x0230, 0x8068, 0x98d2));
    }
    #[test]
    fn the_map_stays_still_while_the_game_camera_scrolls() {
        mem::set_extra(8);
        mem::set_extra_y(205);
        let mut bus = MemMap::new(vec![]);
        let mut shifts = Vec::new();
        for vofs in [114u16, 145, 190] {
            bus.io[0x12..0x14].copy_from_slice(&vofs.to_le_bytes());
            shifts.push(WorldMap::shift(&bus) + vofs as i32);
        }
        assert!(shifts.windows(2).all(|w| w[0] == w[1]));
        mem::set_extra(0);
        mem::set_extra_y(0);
    }
}
