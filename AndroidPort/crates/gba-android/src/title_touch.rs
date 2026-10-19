//! Title screen taps (USA ROM). Measured with the headless runner:
//! - "Push START" and the New Game / Saved Game / Color Mode menu share
//!   DISPCNT f740 and BG0-3CNT 3d83 3e0a 3f08 0b01.
//! - EWRAM 02017d3e holds the menu row (0..=2, wrapping).
//! - The feather cursor is the shadow OAM entry at IWRAM 03000070: tile 15e,
//!   Y 70/86/110 for rows 0/1/2, X 94-98; it is parked at Y 168 while "Push
//!   START" shows. In the Color Mode submenu (LCD A / LCD B / TV) it sits
//!   near X 170, so X tells the two menus apart.
use gba_core::mem::MemMap;

const CURSOR: usize = 0x17d3e;
const FEATHER: usize = 0x70;
const ROW_Y: [u16; 3] = [70, 86, 110];
const RELEASED: u16 = 0x3ff;
const A: u16 = RELEASED & !1;
const START: u16 = RELEASED & !8;
const UP: u16 = RELEASED & !0x40;
const DOWN: u16 = RELEASED & !0x80;

fn half(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*data.get(offset)?, *data.get(offset + 1)?]))
}

fn title_video(bus: &MemMap) -> bool {
    [(0, 0xf740), (8, 0x3d83), (10, 0x3e0a), (12, 0x3f08), (14, 0x0b01)]
        .iter().all(|&(offset, value)| half(&bus.io, offset) == Some(value))
}

/// Key presses (active-low masks) for a tap at native coordinates, or None
/// when this is not the title screen. An empty list consumes the tap.
pub fn keys(bus: &MemMap, x: f32, y: f32) -> Option<Vec<u16>> {
    if !title_video(bus) { return None; }
    let feather_y = half(&bus.iwram, FEATHER)? & 0xff;
    let feather_tile = half(&bus.iwram, FEATHER + 4)? & 0x3ff;
    if feather_tile != 0x15e {
        return (feather_y >= 160).then(|| vec![START]);
    }
    let feather_x = half(&bus.iwram, FEATHER + 2)? & 0x1ff;
    let current = *bus.ewram.get(CURSOR)? as usize;
    if ROW_Y.get(current) != Some(&feather_y) || !(80..120).contains(&feather_x) { return None; }
    // Rows are 16 px tall below the feather's top; the text sits left of it.
    let row = ROW_Y.iter().position(|&top| y >= top as f32 - 2. && y < top as f32 + 18.);
    let Some(target) = row.filter(|_| x < 150.) else { return Some(Vec::new()); };
    let step = if target > current { DOWN } else { UP };
    let mut keys = vec![step; target.abs_diff(current)];
    keys.push(A);
    Some(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn title(cursor: u8, feather_y: u16, tile: u16) -> MemMap {
        let mut bus = MemMap::new(vec![]);
        for (offset, value) in [(0usize, 0xf740u16), (8, 0x3d83), (10, 0x3e0a), (12, 0x3f08), (14, 0x0b01)] {
            bus.io[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        bus.ewram[CURSOR] = cursor;
        bus.iwram[FEATHER..FEATHER + 2].copy_from_slice(&(0x4000 | feather_y).to_le_bytes());
        bus.iwram[FEATHER + 2..FEATHER + 4].copy_from_slice(&0x8062u16.to_le_bytes());
        bus.iwram[FEATHER + 4..FEATHER + 6].copy_from_slice(&(0x4000 | tile).to_le_bytes());
        bus
    }
    #[test]
    fn tapped_row_is_reached_with_original_cursor_moves_then_confirmed() {
        let bus = title(0, 70, 0x15e);
        assert_eq!(keys(&bus, 60., 95.), Some(vec![DOWN, A]));
        assert_eq!(keys(&bus, 60., 115.), Some(vec![DOWN, DOWN, A]));
        assert_eq!(keys(&bus, 60., 75.), Some(vec![A]));
        let bus = title(2, 110, 0x15e);
        assert_eq!(keys(&bus, 30., 72.), Some(vec![UP, UP, A]));
    }
    #[test]
    fn taps_between_rows_or_on_the_artwork_do_nothing() {
        let bus = title(0, 70, 0x15e);
        assert_eq!(keys(&bus, 60., 30.), Some(vec![]));
        assert_eq!(keys(&bus, 200., 75.), Some(vec![]));
    }
    #[test]
    fn push_start_screen_presses_start_and_other_scenes_are_ignored() {
        assert_eq!(keys(&title(0, 168, 0), 120., 80.), Some(vec![START]));
        // Inconsistent cursor/feather state is not trusted.
        assert_eq!(keys(&title(1, 70, 0x15e), 60., 95.), None);
        // Color Mode submenu: feather beside LCD A / LCD B / TV.
        let mut submenu = title(2, 110, 0x15e);
        submenu.iwram[FEATHER + 2..FEATHER + 4].copy_from_slice(&0x80aau16.to_le_bytes());
        assert_eq!(keys(&submenu, 60., 115.), None);
        let mut other = title(0, 70, 0x15e);
        other.io[0..2].copy_from_slice(&0x1f40u16.to_le_bytes());
        assert_eq!(keys(&other, 60., 95.), None);
    }
}
