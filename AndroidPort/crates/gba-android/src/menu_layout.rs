//! Portrait Party screens (see gba-core menu_screen.rs) as one layout:
//! the upper half always shows the whole party, the lower half the selected
//! unit (the roster's unit panel, or the unit's status/Equip/Abilities/Jobs
//! screen in one piece, menus in thumb reach).
//!
//! The game draws the roster only on the roster screen. Each unit's cell (its
//! sprite and "No." label) and the roster's hint bar are kept from the settled
//! roster and laid out as a grid filling the upper half, on the roster and on
//! the unit screens alike; the game's own selection ellipse (captured on its
//! own, raw sprite colours) is blended under the unit selected now. Taps use
//! the game's own controls: on the roster its cursor and A (and the bar's L,
//! Start and R), on unit screens its Left/Right unit cycling.
use gba_core::{mem::{self, MemMap}, menu_screen::{Template, PERIOD, TRANSPARENT}, savestate::{Res, R, W}};

/// Measured USA roster state (EWRAM): cursor position (x, y halfwords) and
/// the selected unit's index, which unit screens also show.
const CURSOR: usize = 0xfd30;
const INDEX: usize = 0xfd3e;
/// Native rows of the roster's bar and unit grid; its unit panel is below.
const ROSTER_ROWS: i32 = 100;
/// The hint bar ("L CHANGE", "START: Item List", "R SORT") on rows 0-7.
const BAR_ROWS: i32 = 8;
/// A unit's cell relative to its roster slot: the sprite (rows 4-40) and its
/// "No." label (rows 41-48), 32 columns wide.
const CELL: [i32; 2] = [0, 2];
const CELL_W: i32 = 32;
const CELL_H: i32 = 47;
/// First cell row of the "No." label.
const LABEL_ROW: i32 = 39;
const MAX_UNITS: usize = 24;
/// Rows kept free below the unit screen for the button row.
const BUTTON_ROW: i32 = crate::game_buttons::ROW_HEIGHT;

/// Roster slot of unit `i` (cursor coordinates without scrolling): rows of
/// four 48 apart, rows 40 apart, odd rows starting 24 further left.
pub fn slot(i: usize) -> [i32; 2] {
    let (row, column) = ((i / 4) as i32, (i % 4) as i32);
    [if row % 2 == 0 { 48 } else { 24 } + 48 * column, 8 + 40 * row]
}

pub struct PartyCache {
    bar: Vec<u16>,
    cells: Vec<Option<Vec<u16>>>,
    ellipse: Vec<u16>,
    /// Native position of the cell the ellipse was under when captured.
    ellipse_cell: [i32; 2],
}

fn put_pixels(w: &mut W, pixels: &[u16]) { w.bytes(&pixels.iter().flat_map(|p| p.to_le_bytes()).collect::<Vec<_>>()); }
fn get_pixels(r: &mut R) -> Res<Vec<u16>> { Ok(r.bytes()?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()) }

impl PartyCache {
    /// Kept in the resume state: the party is shown on unit screens too.
    pub fn save(&self, w: &mut W) {
        put_pixels(w, &self.bar);
        w.u32(self.cells.len() as u32);
        for cell in &self.cells { w.bool(cell.is_some()); put_pixels(w, cell.as_deref().unwrap_or(&[])); }
        put_pixels(w, &self.ellipse);
        w.u32(self.ellipse_cell[0] as u32); w.u32(self.ellipse_cell[1] as u32);
    }
    pub fn load(r: &mut R) -> Res<PartyCache> {
        let bar = get_pixels(r)?;
        let count = r.u32()? as usize;
        if count != MAX_UNITS || bar.len() != (BAR_ROWS * 240) as usize { return Err("Bad party cache".into()); }
        let mut cells = Vec::with_capacity(count);
        for _ in 0..count {
            let has = r.bool()?;
            let cell = get_pixels(r)?;
            if has && cell.len() != (CELL_W * CELL_H) as usize { return Err("Bad party cell".into()); }
            cells.push(has.then_some(cell));
        }
        let ellipse = get_pixels(r)?;
        if ellipse.len() != 240 * 160 { return Err("Bad party ellipse".into()); }
        Ok(PartyCache { bar, cells, ellipse, ellipse_cell: [r.u32()? as i32, r.u32()? as i32] })
    }
    pub fn count(&self) -> usize { self.cells.iter().rposition(Option::is_some).map_or(0, |i| i + 1) }
    pub fn has_unit(&self, i: usize) -> bool { self.cells.get(i).is_some_and(Option::is_some) }

    /// Takes the bar, the visible units' cells and the ellipse from a settled
    /// roster frame; units scrolled out of view keep their earlier cells.
    fn update(&mut self, upper: &[u16], ellipse: &[u16], index: usize, scroll: i32) {
        self.bar = upper[..(BAR_ROWS * 240) as usize].to_vec();
        for i in 0..MAX_UNITS {
            let [sx, sy] = slot(i);
            let (x0, y0) = (sx + CELL[0], sy + CELL[1] + scroll);
            if y0 < BAR_ROWS || y0 + CELL_H > ROSTER_ROWS { continue; }
            // Roster rows overlap: the row above's labels reach into a cell's
            // top. Pixels inside another unit's label box are not this unit's.
            let label = |j: usize, x: i32, y: i32| {
                let [lx, ly] = slot(j);
                x >= lx && x < lx + CELL_W && y >= ly + 41 + scroll && y < ly + 49 + scroll
            };
            let cell: Vec<u16> = (0..CELL_H).flat_map(|y| (0..CELL_W).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let (px, py) = (x0 + x, y0 + y);
                    if (0..MAX_UNITS).any(|j| j != i && label(j, px, py)) { TRANSPARENT } else { upper[(py * 240 + px) as usize] }
                }).collect();
            // A unit is there when its sprite rows hold pixels.
            let sprite = cell[(2 * CELL_W) as usize..(38 * CELL_W) as usize].iter().filter(|&&p| p != TRANSPARENT).count();
            self.cells[i] = (sprite >= 24).then_some(cell);
        }
        self.ellipse = ellipse.to_vec();
        let [sx, sy] = slot(index);
        self.ellipse_cell = [sx + CELL[0], sy + CELL[1] + scroll];
    }
}

/// Upper-half party grid in native-relative rows (`top`..`bottom`): the
/// roster as the game draws it (its hint bar, then four units to a row in its
/// zig-zag: rows 40 apart, every other row 24 further left), from the top of
/// the half (below the touch buttons). A party taller than the half scrolls
/// (`scroll` rows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartyGrid { pub top: i32, pub bottom: i32, pub count: usize, pub scroll: i32 }

const COLUMNS: usize = 4;
const PITCH_Y: i32 = 40;
/// Rows between the bar and the first unit cell, as on the roster (slot 8, cell +2).
const BAR_GAP: i32 = 2;

impl PartyGrid {
    fn rows(&self) -> i32 { self.count.div_ceil(COLUMNS).max(1) as i32 }
    fn content(&self) -> i32 { (self.rows() - 1) * PITCH_Y + CELL_H }
    /// The bar's first row: the top of the half.
    pub fn bar(&self) -> i32 { self.top }
    /// Rows where units are shown (below the bar).
    pub fn area(&self) -> (i32, i32) { (self.bar() + BAR_ROWS, self.bottom) }
    pub fn max_scroll(&self) -> i32 { let (a, b) = self.area(); (BAR_GAP + self.content() - (b - a)).max(0) }
    /// Top-left of unit `i`'s cell (the roster slot, moved into the half).
    pub fn cell(&self, i: usize) -> [i32; 2] {
        let [x, y] = slot(i);
        [x + CELL[0], self.area().0 + BAR_GAP + (y - slot(0)[1]) - self.scroll.clamp(0, self.max_scroll())]
    }
    /// Cells overlap like the roster's (a unit's "No." label beside the heads
    /// of the next row): a label belongs to its unit, otherwise the unit whose
    /// sprite is nearest the tap wins.
    pub fn unit_at(&self, x: f32, y: f32) -> Option<usize> {
        let (a, b) = self.area();
        if y < a as f32 || y >= b as f32 { return None; }
        let inside = |i: usize, rows: (i32, i32)| {
            let [cx, cy] = self.cell(i).map(|v| v as f32);
            x >= cx - 4. && x < cx + CELL_W as f32 + 4. && y >= cy + rows.0 as f32 && y < cy + rows.1 as f32
        };
        (0..self.count).find(|&i| inside(i, (LABEL_ROW, CELL_H))).or_else(|| {
            (0..self.count).filter(|&i| inside(i, (0, CELL_H))).min_by_key(|&i| {
                let [cx, cy] = self.cell(i);
                let (dx, dy) = (x - (cx + CELL_W / 2) as f32, y - (cy + LABEL_ROW / 2) as f32);
                (dx * dx + dy * dy) as i64
            })
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuLayout {
    pub template: Template,
    /// The roster is on screen and settled: taps move its cursor.
    pub roster_live: bool,
    /// The party grid shown in the upper half, if any.
    pub grid: Option<PartyGrid>,
    /// Texture rows the lower block moved by, and its first native row.
    pub lower_shift: i32,
    pub lower_start: i32,
}

pub enum Hit { Unit(usize), Bar(u16), Lower(f32), None }

impl MenuLayout {
    /// What a tap at native-relative texture point (x, y) lands on.
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        if let Some(grid) = self.grid {
            if y >= grid.bar() as f32 && y < (grid.bar() + BAR_ROWS) as f32 {
                // The bar's L, Start and R thirds, as printed on it.
                return Hit::Bar(if x < 80. { 0x1ff } else if x < 160. { 0x3f7 } else { 0x2ff });
            }
            if let Some(i) = grid.unit_at(x, y) { return Hit::Unit(i); }
        }
        let lower = self.lower_shift as f32;
        if y >= lower + self.lower_start as f32 && y < lower + 160. { return Hit::Lower(y - lower); }
        Hit::None
    }
}

fn half(data: &[u8], offset: usize) -> i32 { u16::from_le_bytes([data[offset], data[offset + 1]]) as i32 }

/// The game's rule under a unit's header (BG1 tile 0xa8, palette 12: a
/// yellow-green line in a dark outline, 5 of its 8 rows), loaded on every
/// Party screen. None if the tile is not there.
fn separator(bus: &MemMap) -> Option<Vec<u16>> {
    let char_base = ((half(&bus.io, 0x0a) >> 2) & 3) as usize * 0x4000;
    let tile = bus.vram.get(char_base + 0xa8 * 32..char_base + 0xa9 * 32)?;
    if tile[..4] != [0x11; 4] || tile[16..] != [0x11, 0x11, 0x11, 0x11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] { return None; }
    Some((0..64).map(|i| {
        let index = (tile[i / 2] >> (4 * (i & 1))) & 15;
        if index == 0 { TRANSPARENT } else { half(&bus.palette, (12 * 16 + index as usize) * 2) as u16 }
    }).collect())
}
/// Rows from the separator's top to the unit screen below it.
const SEPARATOR_GAP: i32 = 9;

/// Draws the Party screen into `pixels` (the background pattern everywhere).
pub fn present(bus: &MemMap, pixels: &mut [i32], dock: (i32, i32), cache: &mut Option<PartyCache>, scroll: &mut i32) -> Option<MenuLayout> {
    let screen = &bus.menu_screen;
    if !screen.frame || screen.groups.len() < 3 * 240 * 160 { *cache = None; return None; }
    let (rw, rh) = (mem::render_width(), mem::render_height() as i32);
    let (ex, ey) = (mem::extra() as i32, mem::extra_y() as i32);
    let (top, bottom) = (dock.0 - ey, dock.1 - ey);
    if bottom - top < 2 * 160 { return None; }
    // The bottom HUD row (game_buttons::ROW) stays free for the Return button.
    let lower_shift = ((bottom - 160 - BUTTON_ROW) as f64 / PERIOD as f64).floor() as i32 * PERIOD;
    let colors = crate::engine::rgba_colors();
    let group = |g: usize| &screen.groups[g * 240 * 160..(g + 1) * 240 * 160];
    let template = screen.template;
    // The roster has settled when its bar is drawn (at least six of rows 0-7
    // nearly full width) and its "No." labels, which the game erases first
    // when it leaves the roster (BG text between the bar and the panel).
    let roster_live = template == Template::Roster
        && group(0)[..(BAR_ROWS * 240) as usize].chunks_exact(240).filter(|row| row.iter().filter(|&&p| p != TRANSPARENT).count() >= 180).count() >= 6;
    let index = bus.ewram.get(INDEX).copied().unwrap_or(0) as usize;
    // Captured only while the cursor rests (its position at 0xfd30 equals its
    // target at 0xfd38): the roster's scroll is read from it.
    let resting = bus.ewram.get(CURSOR..CURSOR + 4) == bus.ewram.get(CURSOR + 8..CURSOR + 12);
    if roster_live && resting && screen.label_pixels >= 40 {
        let scroll = half(&bus.ewram, CURSOR + 2) - slot(index)[1];
        cache.get_or_insert_with(|| PartyCache { bar: Vec::new(), cells: vec![None; MAX_UNITS], ellipse: Vec::new(), ellipse_cell: [0, 0] })
            .update(group(0), group(2), index, scroll);
    }
    let io = |o: usize| u16::from_le_bytes([bus.io[o], bus.io[o + 1]]) as u32;
    let (eva, evb) = ((io(0x52) & 31).min(16), ((io(0x52) >> 8) & 31).min(16));
    // Copies `width`-wide `source` pixels to native point (x, y); blended ones
    // are semi-transparent sprite colours over the pattern (BLDALPHA).
    // `rows` clips the native rows written (the scrolling party area).
    let mut blit = |source: &[u16], width: i32, x: i32, y: i32, blend: bool, rows: (i32, i32)| {
        for (i, &p) in source.iter().enumerate() {
            if p == TRANSPARENT { continue; }
            let (tx, ty) = (ex + x + i as i32 % width, ey + y + i as i32 / width);
            if !(rows.0 + ey..rows.1 + ey).contains(&ty) { continue; }
            if !(0..rw as i32).contains(&tx) || !(0..rh).contains(&ty) { continue; }
            let out = &mut pixels[ty as usize * rw + tx as usize];
            *out = if blend {
                let under = *out as u32;
                let world = ((under >> 19) & 31) | ((under >> 11) & 31) << 5 | ((under >> 3) & 31) << 10;
                colors[[0, 5, 10].iter().fold(0u32, |acc, &s| {
                    acc | ((((p as u32 >> s) & 31) * eva + ((world >> s) & 31) * evb) / 16).min(31) << s
                }) as usize]
            } else { colors[(p & 0x7fff) as usize] };
        }
    };
    // Upper half: the party grid, the selection under the unit selected now.
    let all = (i32::MIN / 2, i32::MAX / 2);
    // Between the party and the unit: the game's header rule, across the screen.
    let rule = lower_shift - SEPARATOR_GAP;
    if let Some(tile) = separator(bus) {
        for x in (-ex..rw as i32 - ex).step_by(8) { blit(&tile, 8, x, rule, false, all); }
    }
    let grid = cache.as_ref().filter(|c| c.count() > 0).map(|kept| {
        let mut grid = PartyGrid { top: top + 4, bottom: ((top + bottom) / 2).min(rule - 2), count: kept.count(), scroll: 0 };
        *scroll = (*scroll).clamp(0, grid.max_scroll());
        grid.scroll = *scroll;
        let area = grid.area();
        blit(&kept.bar, 240, 0, grid.bar(), false, all);
        if index < grid.count {
            let [cx, cy] = grid.cell(index);
            blit(&kept.ellipse, 240, cx - kept.ellipse_cell[0], cy - kept.ellipse_cell[1], true, area);
        }
        for i in 0..grid.count {
            if let Some(cell) = &kept.cells[i] {
                let [cx, cy] = grid.cell(i);
                blit(cell, CELL_W, cx, cy, false, area);
            }
        }
        grid
    });
    // Lower half: the roster's unit panel, or the unit screen in one piece.
    let rows = if template == Template::Roster { ROSTER_ROWS..160 } else { 0..160 };
    let groups: &[usize] = if template == Template::Roster { &[1] } else { &[0, 1] };
    let mut lower_start = 160;
    for &g in groups {
        let picture = group(g);
        if let Some(first) = rows.clone().find(|&y| picture[(y * 240) as usize..((y + 1) * 240) as usize].iter().any(|&p| p != TRANSPARENT)) {
            lower_start = lower_start.min(first);
        }
        blit(&picture[(rows.start * 240) as usize..(rows.end * 240) as usize], 240, 0, lower_shift + rows.start, false, all);
    }
    Some(MenuLayout { template, roster_live, grid, lower_shift, lower_start })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roster_slots_follow_the_measured_cursor_positions() {
        // Measured cursor: units 0, 1, 2 at (48|96|144, 8); unit 5 at (72, 48).
        assert_eq!([slot(0), slot(1), slot(2), slot(5)], [[48, 8], [96, 8], [144, 8], [72, 48]]);
    }
    #[test]
    fn the_party_keeps_the_rosters_four_to_a_row_zig_zag_and_scrolls_when_tall() {
        let grid = PartyGrid { top: -140, bottom: 90, count: 6, scroll: 0 };
        let cells: Vec<_> = (0..6).map(|i| grid.cell(i)).collect();
        // The roster's own spacing: 48 across, rows 40 apart, the second row 24 to the left.
        assert_eq!((cells[1][0] - cells[0][0], cells[1][1] - cells[0][1]), (48, 0));
        assert_eq!((cells[4][0] - cells[0][0], cells[4][1] - cells[0][1]), (-24, 40));
        // Bar at the top of the half, the units just below it.
        assert_eq!(grid.bar(), grid.top);
        assert_eq!(cells[0][1], grid.top + BAR_ROWS + BAR_GAP);
        assert_eq!(grid.max_scroll(), 0);
        assert_eq!(grid.unit_at((cells[4][0] + 16) as f32, (cells[4][1] + 20) as f32), Some(4));
        // Where cells overlap (row 2 under row 1), the nearer unit is chosen.
        assert_eq!(grid.unit_at((cells[0][0] + 2) as f32, (cells[0][1] + 44) as f32), Some(0));
        assert_eq!(grid.unit_at((cells[4][0] + 16) as f32, (cells[4][1] + 10) as f32), Some(4));
        // Twenty-four units do not fit: they scroll, tapped where drawn.
        let full = PartyGrid { count: 24, ..grid };
        assert!(full.max_scroll() > 0);
        let scrolled = PartyGrid { scroll: full.max_scroll(), ..full };
        assert_eq!(scrolled.cell(9)[1], full.cell(9)[1] - full.max_scroll());
        let [x, y] = scrolled.cell(9);
        assert_eq!(scrolled.unit_at((x + 16) as f32, (y + 20) as f32), Some(9));
        assert_eq!(PartyGrid { scroll: 10_000, ..full }.cell(23), PartyGrid { scroll: full.max_scroll(), ..full }.cell(23));
    }
    #[test]
    fn taps_land_on_the_bar_units_and_the_lower_block() {
        let grid = PartyGrid { top: -140, bottom: 90, count: 6, scroll: 0 };
        let layout = MenuLayout { template: Template::Unit, roster_live: false, grid: Some(grid), lower_shift: 160, lower_start: 0 };
        assert!(matches!(layout.hit(120., (grid.bar() + 4) as f32), Hit::Bar(0x3f7)));
        let [cx, cy] = grid.cell(2);
        assert!(matches!(layout.hit((cx + 10) as f32, (cy + 10) as f32), Hit::Unit(2)));
        assert!(matches!(layout.hit(100., 230.), Hit::Lower(y) if y == 70.));
        assert!(matches!(layout.hit(100., 120.), Hit::None));
    }
}
