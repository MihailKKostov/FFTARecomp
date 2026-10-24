//! Choosing a unit's facing by touch (portrait). The game shows four small
//! arrows over the unit's head, the one it faces lit. Touch draws four large
//! arrows on the ground around its feet instead, along the field's diagonals
//! (the sides a tap chooses, from the feet: ffta_battle_mouse::aim), the
//! side it faces lit, and asks for a direction. A tap toward a side turns the
//! unit there and ends its turn; "Keep facing" ends it as it is.
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, mem};

/// The arrows on the ground, in field units (a cell's side is 16; a unit
/// along x is screen (+1, +1/2), along y (-1, +1/2)): from the feet out.
const SHAFT: (f32, f32) = (13., 23.);
const HEAD_END: f32 = 35.;
const SHAFT_HALF: f32 = 3.5;
const HEAD_HALF: f32 = 9.;

/// The ground direction (along x, along y) of each side's key.
const SIDES: [(u16, (f32, f32)); 4] = [(0x3ef, (1., 0.)), (0x3df, (-1., 0.)), (0x37f, (0., 1.)), (0x3bf, (0., -1.))];

/// Inside the arrow pointing along the ground's +along axis, grown by `grow`.
fn inside(along: f32, across: f32, grow: f32) -> bool {
    let across = across.abs();
    (along >= SHAFT.0 - grow && along <= SHAFT.1 && across <= SHAFT_HALF + grow)
        || (along >= SHAFT.1 - grow && along <= HEAD_END + grow * 1.6 && across <= (HEAD_END - along) * HEAD_HALF / (HEAD_END - SHAFT.1) + grow && across <= HEAD_HALF + grow)
}

#[derive(Default)]
pub(crate) struct Facing {
    /// The side seen lit while the game asks (its arrow animates).
    pub lit: Option<u16>,
}

impl Engine {
    /// Draws the arrows and the prompt over the field while the game asks
    /// for the facing (touch-only portrait).
    pub(crate) fn facing_present(&mut self, dock: (i32, i32)) {
        let bus = &self.machine.bus;
        if !(self.touch_enabled && bus.ffta_dialogue.docked && self.usa) { self.facing.lit = None; return; }
        let Some(feet) = battle::facing_anchor(bus) else { self.facing.lit = None; return };
        if let Some(lit) = battle::facing_lit(bus) { self.facing.lit = Some(lit); }
        let (rw, rh) = (mem::render_width(), mem::render_height());
        // Zoomed out, the arrows keep their size (they are to be tapped).
        let (ox, oy) = self.native_texture(feet[0], feet[1]);
        let blend = |under: i32, (r, g, b): (u32, u32, u32), alpha: u32| -> i32 {
            let u = under as u32;
            let mix = |c: u32, shift: u32| (c * alpha + ((u >> shift) & 255) * (256 - alpha)) / 256;
            (0xff00_0000 | mix(r, 16) << 16 | mix(g, 8) << 8 | mix(b, 0)) as i32
        };
        // A box around the arrows (screen x = along x - along y, y = half their sum).
        let reach = (HEAD_END + 3.) as i32;
        for sy in -reach / 2 - 2..=reach / 2 + 2 {
            for sx in -reach - 2..=reach + 2 {
                let (x, y) = (ox as i32 + sx, oy as i32 + sy);
                if !(0..rw as i32).contains(&x) || !(0..rh as i32).contains(&y) { continue; }
                let (fx, fy) = (sx as f32 + 0.5, sy as f32 + 0.5);
                let (gx, gy) = (fy + fx / 2., fy - fx / 2.);
                for &(key, (dx, dy)) in &SIDES {
                    let along = gx * dx + gy * dy;
                    let across = gx * dy - gy * dx;
                    let lit = self.facing.lit == Some(key);
                    let target = &mut self.pixels[y as usize * rw + x as usize];
                    if inside(along, across, 0.) {
                        *target = if lit { blend(*target, (255, 214, 72), 240) } else { blend(*target, (240, 244, 255), 170) };
                    } else if inside(along, across, 1.3) {
                        *target = blend(*target, (16, 24, 56), 220);
                    }
                }
            }
        }
        // The prompt, over the unit's panel.
        let text = "Choose a direction";
        let w = crate::game_buttons::text_width_scaled(&bus.rom, text, 1) + 24;
        let (x0, y0) = (mem::extra() as i32 + (240 - w) / 2, dock.1 - crate::battle_windows::PANEL_ROWS - 32);
        crate::game_buttons::draw_card(bus, &mut self.pixels, rw, rh, x0, y0, w, 16, &[(12, 0, text, false, 1)]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_arrow_points_out_from_the_feet() {
        // Clear of the feet, a shaft, then a head narrowing to its tip.
        assert!(!inside(5., 0., 0.));
        assert!(inside(18., 3., 0.) && !inside(18., 5., 0.));
        assert!(inside(24., 8., 0.) && !inside(30., 8., 0.) && inside(34., 0., 0.) && !inside(34., 3., 0.));
        assert!(!inside(36., 0., 0.) && inside(36., 0., 1.3));
    }
}
