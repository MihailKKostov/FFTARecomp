//! Title screen taps (USA ROM). Measured with the headless runner:
//! - "Push START" and the New Game / Saved Game / Color Mode menu share
//!   DISPCNT f740 and BG0-3CNT 3d83 3e0a 3f08 0b01.
//! - EWRAM 02017d3e holds the menu row (0..=2, wrapping).
//! - The feather cursor is the shadow OAM entry at IWRAM 03000070: tile 15e,
//!   Y 70/86/110 for rows 0/1/2, X 94-98; it is parked at Y 168 while "Push
//!   START" shows. In the Color Mode submenu (LCD A / LCD B / TV) it sits
//!   near X 170, so X tells the two menus apart.
//! - The file slots ("Pick slot to load.", FILE NO.1 / FILE NO.2; also
//!   reached from Saved Game's Load) use BG0-3CNT ad0b ac09 2b0a 2a08; the
//!   pointer is an 8x8 sprite (tiles 0x125-0x127 as it shines, priority 0) at Y 69, X 16
//!   for slot 1 and X 128 for slot 2; the panels span X 16-120 and 128-232,
//!   rows 48-150. Left/Right move it, A loads the slot at once. Resume
//!   Battle shows one slot the same way (its mission beside it).
use gba_core::mem::MemMap;

const CURSOR: usize = 0x17d3e;
const FEATHER: usize = 0x70;
const ROW_Y: [u16; 3] = [70, 86, 110];
/// Each row's tap band (native rows): New Game only on its own line, so a tap
/// near Saved Game never starts a new game; Saved Game takes the gap below it.
const ROW_BANDS: [(f32, f32); 3] = [(66., 85.), (85., 106.), (106., 126.)];
const LEFT: u16 = RELEASED & !0x20;
const RIGHT: u16 = RELEASED & !0x10;
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
    // The text sits left of the feather.
    let row = ROW_BANDS.iter().position(|&(top, bottom)| y >= top && y < bottom);
    let Some(target) = row.filter(|_| x < 150.) else { return Some(Vec::new()); };
    let step = if target > current { DOWN } else { UP };
    let mut keys = vec![step; target.abs_diff(current)];
    keys.push(A);
    Some(keys)
}

/// Key presses for a tap on the file slots, or None when they are not shown.
/// A tap on the other slot selects it; a tap on the selected one loads it.
pub fn slot_keys(bus: &MemMap, x: f32, y: f32) -> Option<Vec<u16>> {
    let video = [(0, 0x1f40), (8, 0xad0b), (10, 0xac09), (12, 0x2b0a), (14, 0x2a08)]
        .iter().all(|&(offset, value)| half(&bus.io, offset) == Some(value));
    if !video { return None; }
    let selected = bus.oam.chunks_exact(8).find_map(|o| {
        let (a0, a1, a2) = (half(o, 0)?, half(o, 2)?, half(o, 4)?);
        (a0 & 0x300 != 0x200 && a0 >> 14 == 0 && a1 >> 14 == 0 && (a0 & 255) == 69
            && (0x120..=0x127).contains(&(a2 & 0x3ff)) && (a2 >> 10) & 3 == 0).then_some(a1 & 511)
    }).and_then(|x| match x { 16 => Some(0), 128 => Some(1), _ => None })?;
    if !(48. ..150.).contains(&y) || !(12. ..236.).contains(&x) { return Some(Vec::new()); }
    let target = if x < 124. { 0 } else { 1 };
    Some(if target == selected { vec![A] } else { vec![if target > selected { RIGHT } else { LEFT }] })
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
    fn a_tap_near_saved_game_never_starts_a_new_game() {
        // Saved Game selected (feather at 86): its line and the gap above Color Mode load.
        let bus = title(1, 86, 0x15e);
        for y in [85., 88., 95., 104.] { assert_eq!(keys(&bus, 60., y), Some(vec![A]), "y {y}"); }
        assert_eq!(keys(&bus, 60., 75.), Some(vec![UP, A]));
        assert_eq!(keys(&bus, 60., 112.), Some(vec![DOWN, A]));
    }
    #[test]
    fn file_slots_select_then_load() {
        let mut bus = MemMap::new(vec![]);
        for (offset, value) in [(0usize, 0x1f40u16), (8, 0xad0b), (10, 0xac09), (12, 0x2b0a), (14, 0x2a08)] {
            bus.io[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for slot in 0..128 { bus.oam[slot * 8..slot * 8 + 2].copy_from_slice(&0x0200u16.to_le_bytes()); }
        assert_eq!(slot_keys(&bus, 60., 90.), None);
        for (k, v) in [0x0045u16, 0x0010, 0x0125].iter().enumerate() { bus.oam[2 * k..2 * k + 2].copy_from_slice(&v.to_le_bytes()); }
        assert_eq!(slot_keys(&bus, 60., 90.), Some(vec![A]));
        assert_eq!(slot_keys(&bus, 180., 90.), Some(vec![RIGHT]));
        assert_eq!(slot_keys(&bus, 60., 20.), Some(vec![]));
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
