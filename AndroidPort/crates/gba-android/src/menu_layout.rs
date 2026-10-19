//! Portrait Party screens (see gba-core menu_screen.rs): the tall view shows the
//! background pattern; the upper group is moved up under the docking frame's
//! top, the lower group down to its bottom (menus and lists in thumb reach).
use gba_core::{mem::{self, MemMap}, menu_screen::{PERIOD, TRANSPARENT}};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuLayout {
    /// Texture rows each group moved by (upper, lower), multiples of PERIOD.
    pub shift: [i32; 2],
    /// Native rows holding each group's pixels: upper ends at `upper_end`,
    /// lower starts at `lower_start`.
    pub upper_end: i32,
    pub lower_start: i32,
}
impl MenuLayout {
    /// Native row shown at native-relative texture row `y`, None in the gap.
    pub fn native_row(&self, y: f32) -> Option<f32> {
        if y >= (self.lower_start + self.shift[1]) as f32 { Some(y - self.shift[1] as f32) }
        else if y < (self.upper_end + self.shift[0]) as f32 { Some(y - self.shift[0] as f32) }
        else { None }
    }
}

/// Shifts placing native rows 0..160 between texture rows `dock` (relative to
/// the native screen's first texture row), by whole pattern periods.
pub fn shifts(dock: (i32, i32)) -> [i32; 2] {
    let up = (dock.0 as f64 / PERIOD as f64).ceil() as i32 * PERIOD;
    let down = ((dock.1 - 160) as f64 / PERIOD as f64).floor() as i32 * PERIOD;
    if up > 0 || down < 0 || up > down { [0, 0] } else { [up, down] }
}

/// Draws both groups into `pixels` (render_width x render_height, the pattern
/// everywhere). `dock` is the (top, bottom) texture rows of the docking frame.
pub fn present(bus: &MemMap, pixels: &mut [i32], dock: (i32, i32)) -> Option<MenuLayout> {
    let screen = &bus.menu_screen;
    if !screen.frame || screen.groups.len() < 2 * 240 * 160 { return None; }
    let (rw, rh) = (mem::render_width(), mem::render_height() as i32);
    let (ex, ey) = (mem::extra(), mem::extra_y() as i32);
    let shift = shifts((dock.0 - ey, dock.1 - ey));
    let colors = crate::engine::rgba_colors();
    let mut rows = [(i32::MAX, i32::MIN); 2];
    for group in 0..2 {
        for y in 0..160 {
            let source = &screen.groups[(group * 160 + y) * 240..][..240];
            if source.iter().all(|&p| p == TRANSPARENT) { continue; }
            rows[group] = (rows[group].0.min(y as i32), rows[group].1.max(y as i32 + 1));
            let target = ey + y as i32 + shift[group];
            if !(0..rh).contains(&target) { continue; }
            let row = &mut pixels[target as usize * rw + ex..][..240];
            for (out, &p) in row.iter_mut().zip(source) {
                if p != TRANSPARENT { *out = colors[(p & 0x7fff) as usize]; }
            }
        }
    }
    let cut = screen.template.cut();
    Some(MenuLayout {
        shift,
        upper_end: if rows[0].1 > i32::MIN { rows[0].1 } else { cut },
        lower_start: if rows[1].0 < i32::MAX { rows[1].0 } else { cut },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_move_to_the_frame_edges_by_whole_pattern_periods() {
        // Frame from 150 rows above the native screen to 160 + 170 below its top.
        let s = shifts((-150, 330));
        assert_eq!(s, [-144, 160]);
        assert!(s.iter().all(|v| v % PERIOD == 0));
        // A frame no taller than the screen keeps the native layout.
        assert_eq!(shifts((0, 160)), [0, 0]);
        assert_eq!(shifts((10, 150)), [0, 0]);
        let layout = MenuLayout { shift: s, upper_end: 61, lower_start: 50 };
        assert_eq!(layout.native_row(-144. + 10.), Some(10.));
        assert_eq!(layout.native_row(160. + 70.), Some(70.));
        assert_eq!(layout.native_row(100.), None);
    }
}
