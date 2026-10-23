//! Portrait world map: the laws in force always shown at the top, and the
//! whole law list (the game's L screen) under them on request.
//!
//! The game's law list (L on the world map, USA measurements) is a window of
//! BG2/BG3 over the map: a title bar ("FORBIDDEN", "RECOMMEND") on native rows
//! 20-31, then rows 16 high from row 32, one law each, the forbidden law in a
//! pill; the selected row also shows its recommended law beside it. The first
//! row is the law in force (its pills dark blue and red); Down walks the
//! coming ones and wraps back to it. The pointer is a 16x8 sprite (tile 0,
//! priority 0) four rows into the selected row.
//!
//! The rows are read in a silent run from a snapshot (as the shop reads its
//! rows): L opens the list, each row is taken once it is still, Down moves on,
//! until the first row is back; then the snapshot's machine is dropped. Only
//! BG2/BG3 are read, so the rows come without the map. The run is made again
//! when the date shown changes.
//!
//! The game's ▶ marks the row its law rotation is at; it moves on one row a
//! day, and the law in force follows it (measured: ▶ on the 20th row, Items,
//! on day 1 with Shell in force, the first row; Shell on day 2, Protect on
//! day 4 with ▶ on it). So while travelling, each day that passes (EWRAM
//! 0x2fbb, the day of the month) moves the shown laws on at once, scrolling
//! while the party moves; the law in force stays until ▶ reaches it. Once the
//! map waits for input again the list is read anew and the shown laws only
//! correct themselves if the guess was wrong.
//!
//! Shown: the title bar, the law in force and the next two always; the rest
//! under them while the list is open. The law in force is flush left, the
//! coming ones indented as the game indents its marked row (16 columns; the
//! game's own ▶ indent is taken out first). Forbidden laws are red and
//! recommended ones blue: the pills' palettes (BG2/BG3 banks 4 dark blue,
//! 5 cyan, 9 tan, 10 red; text in entries 1-3, the pill in 10-15) trade
//! places, the cyan forbidden pills becoming a lighter red.
use crate::engine::Engine;
use gba_core::{mem, savestate, world_hud::TRANSPARENT};

const L: u16 = 0x1ff;
const DOWN: u16 = 0x37f;
const RELEASED: u16 = 0x3ff;
/// The list's title bar rows and its first row (native).
const HEADER: (usize, usize) = (20, 32);
const ROW: usize = 16;
/// Columns kept (the pills and the title bar), drawn where the game draws them.
const COLUMNS: (usize, usize) = (16, 232);
/// Rows under the top of the frame the banner starts at.
const BANNER_TOP: i32 = 4;
/// Laws always shown (the one in force and the next ones).
const SHOWN: usize = 3;
/// Indent of the coming laws, and the columns the game's marked row is indented by.
const INDENT: usize = 16;
/// Native rows the shown laws take under the top of the frame (and a gap).
pub(crate) const BANNER_ROWS: i32 = BANNER_TOP + (HEADER.1 - HEADER.0) as i32 + (SHOWN * ROW) as i32 + 4;
/// Silent frames run per frame shown; frames for the list to appear; rows
/// kept at most; frames before trying again after a failed run.
const FRAMES_PER_TICK: u32 = 4;
const OPEN_FRAMES: u32 = 90;
/// Native rows a frame the shown laws scroll by when the laws in force move on.
const MAX_ROWS: usize = 48;
const RETRY: u64 = 120;
/// Frames after Down without a new row before Down is pressed again.
const RETAKE: u32 = 40;
/// Frames the date must stay unchanged before it is trusted (fades, travel).
const STEADY: u32 = 8;
/// The day of the month (EWRAM, measured USA: 1-20; day 20 is followed by
/// day 1 of the next month).
const DAY: usize = 0x2fbb;
const MONTH_DAYS: i32 = 20;
/// Frames after the day changed before the list is read (a day of travel
/// takes about 60; the game ignores L meanwhile).
const SETTLED: u64 = 90;

struct Table {
    header: Vec<u16>,
    rows: Vec<Vec<u16>>,
    colors: std::collections::HashMap<u16, u16>,
    /// The row the game marked (▶, its rotation), and the day it was read on.
    marked: Option<usize>,
    day: u8,
    /// The law in force is drawn in the game's dark pills wherever it is in
    /// the rotation: all rows are kept in the light ones (the first one read
    /// turned light), and these turn light pill colours dark.
    dark: std::collections::HashMap<u16, u16>,
}

struct Run {
    state: Vec<u8>,
    date: u64,
    frame: u32,
    /// Key held for `hold` more frames, then `after` frames to react.
    key: u16,
    hold: u32,
    after: u32,
    last: Option<(i32, Vec<u16>)>,
    still: u32,
    /// The row taken last (pointer row and pixels) and frames since Down.
    taken: Option<(i32, Vec<u16>)>,
    waited: u32,
    header: Vec<u16>,
    rows: Vec<Vec<u16>>,
    colors: std::collections::HashMap<u16, u16>,
    dark: std::collections::HashMap<u16, u16>,
    day: u8,
}

#[derive(Default)]
pub struct Laws {
    table: Option<Table>,
    /// The date the table was read on, the date shown now and for how many frames.
    date: Option<u64>,
    shown: (u64, u32),
    run: Option<Run>,
    retry: u64,
    /// Rows of the table the shown laws start at (scrolling between whole
    /// rows), the day of the month now and the frame it last changed.
    pos: f32,
    day: Option<u8>,
    day_changed: u64,
    /// The whole list is shown, scrolled by `scroll` native rows.
    pub open: bool,
    pub scroll: f32,
    /// Texture rows of the shown laws (title bar to the last shown law); a tap
    /// there opens or closes the list.
    pub banner: Option<(i32, i32)>,
    pub screen_banner: Option<(f32, f32)>,
    /// Texture rows of the list under the banner (while open), and its most scroll.
    pub area: Option<(i32, i32)>,
    pub max_scroll: f32,
    /// The same rows on the screen (pixels), and screen pixels per row.
    pub screen_area: Option<(f32, f32)>,
    pub scale: f32,
}

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

/// The list's pointer: its native row (y), if shown.
fn pointer(bus: &mem::MemMap) -> Option<i32> {
    bus.oam.chunks_exact(8).find_map(|o| {
        let (a0, a1, a2) = (half(o, 0), half(o, 2), half(o, 4));
        let y = (a0 & 255) as i32;
        (a0 & 0x300 != 0x200 && a0 >> 14 == 1 && a1 >> 14 == 0 && a2 & 1023 == 0 && (a2 >> 10) & 3 == 0
            && (a1 & 511) < 16 && (36..148).contains(&y)).then_some(y)
    })
}

/// BG2/BG3 of native rows `rows` (240 wide).
fn layers(bus: &mem::MemMap, rows: std::ops::Range<usize>) -> Vec<u16> {
    rows.flat_map(|y| gba_core::ppu::native_layers(bus, 0b1100, y)).collect()
}

/// Forbidden red, recommended blue: the pills' palette banks (measured USA:
/// 4 dark blue and 10 red for the law in force, 5 cyan and 9 tan for the
/// others; text entries 1-3, pill entries 10-15, outline 11) trade places.
/// The cyan forbidden pills become the red one lightened, with the tan
/// pill's text.
fn swapped_colors(palette: &[u8]) -> std::collections::HashMap<u16, u16> {
    let c = |bank: usize, i: usize| half(palette, (bank * 16 + i) * 2) & 0x7fff;
    let light = |v: u16| [0, 5, 10].iter().fold(0, |acc, &s| { let ch = (v >> s) & 31; acc | ((ch + (31 - ch) / 2) << s) });
    let mut map = std::collections::HashMap::new();
    for i in [1, 2, 3, 10, 11, 12, 13, 14, 15] {
        map.insert(c(4, i), c(10, i));
        map.insert(c(10, i), c(4, i));
        map.insert(c(9, i), c(5, i));
        map.insert(c(5, i), match i { 1..=3 => c(9, i), 11 => c(10, i), _ => light(c(10, i)) });
    }
    map
}

/// The law in force's pills (banks 4 dark blue, 10 red) against the others'
/// (5 cyan, 9 tan), text and pill entries: light to dark.
fn dark_colors(palette: &[u8]) -> std::collections::HashMap<u16, u16> {
    let c = |bank: usize, i: usize| half(palette, (bank * 16 + i) * 2) & 0x7fff;
    [1, 2, 3, 10, 11, 12, 13, 14, 15].iter().flat_map(|&i| [(c(5, i), c(4, i)), (c(9, i), c(10, i))]).collect()
}

/// A row as the game draws it unmarked: its ▶ mark (left of the pill, which
/// starts `INDENT` columns further right) taken out; true if it had one.
fn unmarked(mut row: Vec<u16>) -> (Vec<u16>, bool) {
    // Unmarked rows start their pill at column 17 or 18; a marked row has
    // nothing there (its mark is further right).
    let mid = &row[8 * 240..9 * 240];
    let marked = mid[COLUMNS.0 - 2..COLUMNS.0 + 4].iter().all(|&p| p == TRANSPARENT) && mid.iter().any(|&p| p != TRANSPARENT);
    if marked {
        for line in row.chunks_exact_mut(240) {
            line[..COLUMNS.0 + INDENT].fill(TRANSPARENT);
            line.copy_within(INDENT.., 0);
            line[240 - INDENT..].fill(TRANSPARENT);
        }
    }
    (row, marked)
}

/// The date shown on the map (the HUD's date box), as a hash.
fn date(bus: &mem::MemMap) -> Option<u64> {
    let hud = &bus.world_hud;
    if !hud.map_frame || hud.pixels.len() < 240 * 160 { return None; }
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut any = false;
    for y in 140..160 { for x in 144..240 {
        let p = hud.pixels[y * 240 + x];
        any |= p != TRANSPARENT;
        h = (h ^ p as u64).wrapping_mul(0x100_0000_01b3);
    } }
    any.then_some(h)
}

fn put_pixels(w: &mut savestate::W, pixels: &[u16]) { w.bytes(&pixels.iter().flat_map(|p| p.to_le_bytes()).collect::<Vec<_>>()); }
fn get_pixels(r: &mut savestate::R, len: usize) -> savestate::Res<Vec<u16>> {
    let bytes = r.bytes()?;
    if bytes.len() != len * 2 { return Err("Bad law pixels".into()); }
    Ok(bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
}

impl Laws {
    pub fn toggle(&mut self) { self.open = !self.open; self.scroll = 0.; }
    /// Kept in the resume state, so the laws are shown at once after resuming.
    pub fn save(&self, w: &mut savestate::W) {
        w.bool(self.table.is_some());
        let Some(t) = &self.table else { return };
        w.u64(self.date.unwrap_or(0));
        put_pixels(w, &t.header);
        w.u32(t.rows.len() as u32);
        for row in &t.rows { put_pixels(w, row); }
        w.u32(t.colors.len() as u32);
        for (&from, &to) in &t.colors { w.u16(from); w.u16(to); }
        w.u32(t.marked.map_or(u32::MAX, |m| m as u32));
        w.u8(t.day);
        w.u32(t.dark.len() as u32);
        for (&from, &to) in &t.dark { w.u16(from); w.u16(to); }
    }
    pub fn load(r: &mut savestate::R) -> savestate::Res<Laws> {
        if !r.bool()? { return Ok(Laws::default()); }
        let date = r.u64()?;
        let header = get_pixels(r, 240 * (HEADER.1 - HEADER.0))?;
        let count = r.u32()? as usize;
        if count == 0 || count > MAX_ROWS { return Err("Bad law count".into()); }
        let rows = (0..count).map(|_| get_pixels(r, 240 * ROW)).collect::<savestate::Res<Vec<_>>>()?;
        let colors = (0..r.u32()?.min(256)).map(|_| Ok((r.u16()?, r.u16()?))).collect::<savestate::Res<_>>()?;
        let marked = r.u32()?;
        let marked = (marked != u32::MAX).then_some(marked as usize).filter(|&m| m < count);
        let day = r.u8()?;
        let dark = (0..r.u32()?.min(256)).map(|_| Ok((r.u16()?, r.u16()?))).collect::<savestate::Res<_>>()?;
        Ok(Laws { table: Some(Table { header, rows, colors, marked, day, dark }), date: Some(date), ..Laws::default() })
    }
    fn current(&self) -> Option<&Table> { self.table.as_ref() }
    /// Rows the laws in force have moved on since the table was read: one a
    /// day once ▶ is on the law in force (until then the law in force stays).
    fn target(&self) -> f32 {
        let (Some(t), Some(day)) = (&self.table, self.day) else { return 0. };
        let elapsed = (day as i32 - t.day as i32).rem_euclid(MONTH_DAYS) as usize;
        let n = t.rows.len();
        let ahead = match t.marked { Some(m) if m > 0 => elapsed.saturating_sub(n - m), _ => elapsed };
        ahead as f32
    }
    /// Takes the list read anew. When it is the old one turned on by some laws
    /// (most rows in the same order), the shown laws keep their place and move
    /// on to it from there (no jump where the guess was right).
    fn take(&mut self, new: Table) {
        let turned = self.table.as_ref().and_then(|old| {
            let n = old.rows.len();
            if n != new.rows.len() || n < 2 { return None; }
            // Rows 1.. of the new list against the old one turned on by k.
            let turned = |k: usize| (1..n).filter(|&i| new.rows[i] == old.rows[(i + k) % n]).count();
            let (k, same) = (0..n).map(|k| (k, turned(k))).max_by_key(|&(_, c)| c)?;
            (same * 2 >= n).then_some(k)
        });
        self.pos = match turned {
            // Seen from the new list, rows back (or ahead) of its first row.
            Some(k) => {
                let n = new.rows.len() as f32;
                let p = (self.pos - k as f32).rem_euclid(n);
                if p > n / 2. { p - n } else { p }
            }
            None => 0.,
        };
        self.table = Some(new);
    }
    /// Once a frame shown: the shown laws scroll toward the laws in force,
    /// a row in 16 frames (faster when further behind).
    fn follow(&mut self) {
        let target = self.target();
        let d = target - self.pos;
        let step = (1. / ROW as f32).max(d.abs() / 8.);
        self.pos = if d.abs() <= step { target } else { self.pos + step * d.signum() };
    }
}

impl Engine {
    /// Before the frames: follows the date, and reads the law list in a silent
    /// run when it is not known for it (a few frames at a time).
    pub(crate) fn laws_tick(&mut self) -> Result<(), String> {
        let bus = &self.machine.bus;
        let wanted = self.touch_enabled && bus.ffta_dialogue.docked && crate::game_buttons::available(&bus.rom);
        if !wanted || !bus.world_hud.map_frame { self.laws.run = None; self.laws.open = false; return Ok(()); }
        let io = |o: usize| half(&bus.io, o);
        let fading = matches!((io(0x50) >> 6) & 3, 2 | 3) && io(0x54) & 31 != 0;
        if let Some(d) = date(bus).filter(|_| !bus.world_hud.menu && !fading) {
            let n = if d == self.laws.shown.0 { self.laws.shown.1.saturating_add(1) } else { 0 };
            self.laws.shown = (d, n);
        }
        if bus.world_hud.menu { self.laws.open = false; }
        // The day passing (travelling) moves the shown laws on.
        let day = bus.ewram.get(DAY).copied().filter(|d| (1..=MONTH_DAYS as u8).contains(d));
        if day != self.laws.day { self.laws.day = day; self.laws.day_changed = bus.frames; }
        let steady = self.laws.shown.1 >= STEADY && bus.frames >= self.laws.day_changed + SETTLED;
        if self.laws.run.is_none() {
            if !steady || self.laws.date == Some(self.laws.shown.0) || bus.frames < self.laws.retry
                || bus.world_hud.menu || !gba_core::ffta_battle_mouse::idle(bus) || self.gesture.is_some() { return Ok(()); }
            self.laws.run = Some(Run { state: savestate::snapshot(&self.machine), date: self.laws.shown.0, frame: 0, key: L, hold: 2, after: 2,
                last: None, still: 0, taken: None, waited: 0, header: Vec::new(), rows: Vec::new(), colors: Default::default(), dark: Default::default(), day: 0 });
        }
        let real = savestate::snapshot(&self.machine);
        let skip = self.machine.bus.skip_render;
        let mut run = self.laws.run.take().unwrap();
        savestate::restore(&mut self.machine, &run.state)?;
        let mut end = None;
        for _ in 0..FRAMES_PER_TICK {
            let bus = &mut self.machine.bus;
            bus.keys = if run.hold > 0 { run.key } else { RELEASED };
            bus.skip_render = true;
            self.step_frame()?;
            let bus = &mut self.machine.bus;
            bus.audio_buf.clear();
            run.frame += 1;
            if run.hold > 0 { run.hold -= 1; continue; }
            if run.after > 0 { run.after -= 1; continue; }
            let Some(y) = pointer(bus) else {
                if run.frame > OPEN_FRAMES { end = Some(false); break; }
                continue;
            };
            let band = layers(bus, (y - 4) as usize..(y + 12) as usize);
            if run.last.as_ref().is_some_and(|(ly, lb)| *ly == y && *lb == band) { run.still += 1; } else { run.still = 0; run.last = Some((y, band.clone())); }
            run.waited += 1;
            if run.frame > OPEN_FRAMES * 4 { end = Some(false); break; }
            // Still, and moved on from the row taken last (the list can scroll
            // a few frames after the pointer reached its last row).
            if run.still < 3 { continue; }
            if run.taken.as_ref().is_some_and(|(ty, tb)| *ty == y && *tb == band) {
                // Down was not taken: once more.
                if run.waited > RETAKE { (run.key, run.hold, run.after, run.waited) = (DOWN, 2, 2, 0); }
                continue;
            }
            if run.rows.is_empty() {
                run.header = layers(bus, HEADER.0..HEADER.1);
                run.colors = swapped_colors(&bus.palette);
                run.dark = dark_colors(&bus.palette);
                run.day = bus.ewram.get(DAY).copied().unwrap_or(0);
            }
            if run.rows.first() == Some(&band) || run.rows.len() >= MAX_ROWS { end = Some(true); break; }
            run.rows.push(band.clone());
            run.taken = Some((y, band));
            (run.key, run.hold, run.after, run.still, run.last, run.waited) = (DOWN, 2, 2, 0, None, 0);
        }
        if end.is_none() { run.state = savestate::snapshot(&self.machine); }
        savestate::restore(&mut self.machine, &real)?;
        self.machine.bus.skip_render = skip;
        match end {
            Some(true) => {
                let (rows, marks): (Vec<_>, Vec<_>) = run.rows.into_iter().map(unmarked).unzip();
                let marked = marks.iter().position(|&m| m);
                let mut rows: Vec<Vec<u16>> = rows;
                // The law in force, read in its dark pills, kept light like the rest.
                let light: std::collections::HashMap<u16, u16> = run.dark.iter().map(|(&l, &d)| (d, l)).collect();
                if let Some(first) = rows.first_mut() { for p in first.iter_mut() { if let Some(&l) = light.get(&(*p & 0x7fff)) { *p = l; } } }
                self.laws.take(Table { header: run.header, rows, colors: run.colors, marked, day: run.day, dark: run.dark });
                self.laws.date = Some(run.date);
            }
            Some(false) => self.laws.retry = self.machine.bus.frames + RETRY,
            None => self.laws.run = Some(run),
        }
        Ok(())
    }

    /// After the frame, on the portrait world map: under the top of the frame
    /// the list's title bar, the law in force and the next ones (the coming
    /// ones indented), the rest under them while open (a tap on the shown
    /// laws opens and closes it).
    pub(crate) fn laws_present(&mut self, dock: (i32, i32)) {
        self.laws.area = None;
        self.laws.banner = None;
        self.laws.follow();
        let bus = &self.machine.bus;
        if !bus.world_hud.map_frame || bus.world_hud.menu || self.laws.current().is_none() { return; }
        let Some(table) = self.laws.table.take() else { return };
        let (rw, rh) = (mem::render_width(), mem::render_height() as i32);
        let ex = mem::extra() as i32;
        let colors = crate::engine::rgba_colors();
        let pixels = &mut self.pixels;
        // Native pixels of `source` (240 wide) to texture row `top`, `indent`
        // columns to the right, clipped to rows `clip`.
        let mut put = |source: &[u16], top: i32, indent: i32, clip: (i32, i32), in_force: bool| {
            for (i, &p) in source.iter().enumerate() {
                let (x, y) = ((i % 240) as i32 + indent, top + (i / 240) as i32);
                if p == TRANSPARENT || !(COLUMNS.0 as i32..COLUMNS.1 as i32).contains(&x) || y < clip.0 || y >= clip.1 || !(0..rh).contains(&y) { continue; }
                let tx = ex + x;
                if !(0..rw as i32).contains(&tx) { continue; }
                let raw = p & 0x7fff;
                let raw = if in_force { table.dark.get(&raw).copied().unwrap_or(raw) } else { raw };
                let color = table.colors.get(&raw).copied().unwrap_or(raw);
                pixels[y as usize * rw + tx as usize] = colors[crate::world_map::faded(bus, color, 0x0c) as usize];
            }
        };
        let all = (i32::MIN, i32::MAX);
        let top = dock.0 + BANNER_TOP;
        put(&table.header, top, 0, all, false);
        let first = top + (HEADER.1 - HEADER.0) as i32;
        let n = table.rows.len();
        let row_at = |i: i64| &table.rows[i.rem_euclid(n as i64) as usize];
        let shown = n.min(SHOWN);
        let mut end = first + (shown * ROW) as i32;
        self.laws.banner = Some((top, end));
        // From the row the laws have scrolled to: whole rows and the part between.
        let base = self.laws.pos.floor();
        let part = ((self.laws.pos - base) * ROW as f32).round() as i32;
        let base = base as i64;
        // A row's indent follows its place: none in the first, 16 from the second.
        for j in 0..=shown as i64 {
            let y = first + j as i32 * ROW as i32 - part;
            // The row in the first place (more than half in it) is the law in force.
            let in_force = (y - first).abs() * 2 < ROW as i32 || (j == 0 && part * 2 == ROW as i32);
            put(row_at(base + j), y, ((y - first) * INDENT as i32 / ROW as i32).clamp(0, INDENT as i32), (first, end), in_force);
        }
        if self.laws.open && n > shown {
            // Down to the bottom HUD row.
            let (a, b) = (end, dock.1 - 160 + crate::game_buttons::ROW.0 - 2);
            let content = ((table.rows.len() - shown) * ROW) as i32;
            self.laws.max_scroll = (content - (b - a)).max(0) as f32;
            self.laws.scroll = self.laws.scroll.clamp(0., self.laws.max_scroll);
            let scroll = self.laws.scroll.round() as i32;
            for i in shown..n {
                put(row_at(base + i as i64), a + ((i - shown) * ROW) as i32 - scroll, INDENT as i32, (a, b), false);
            }
            end = b.min(a + content);
            self.laws.area = Some((a, end));
        }
        self.laws.table = Some(table);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table(names: &[u16], marked: Option<usize>, day: u8) -> Table {
        Table { header: vec![7; 240 * (HEADER.1 - HEADER.0)], rows: names.iter().map(|&n| vec![n; 240 * ROW]).collect(), colors: [(1, 2)].into(), marked, day, dark: [(3, 4)].into() }
    }
    fn settle(laws: &mut Laws) { for _ in 0..200 { laws.follow(); } }
    #[test]
    fn the_laws_move_on_a_row_a_day_once_the_mark_is_on_the_law_in_force() {
        // Day 1: Shell (10) in force, ▶ on the last row: it reaches Shell on day 2.
        let mut laws = Laws::default();
        laws.take(table(&[10, 11, 12, 13, 14, 15], Some(5), 1));
        laws.day = Some(1);
        settle(&mut laws);
        assert_eq!(laws.pos, 0.);
        for (day, pos) in [(2, 0.), (3, 1.), (4, 2.)] {
            laws.day = Some(day);
            laws.follow();
            // Scrolling meanwhile, not there at once.
            assert!(pos == 0. || laws.pos < pos);
            settle(&mut laws);
            assert_eq!(laws.pos, pos);
        }
        // Read anew on day 4 (Protect, 12, in force and marked): no jump.
        laws.take(table(&[12, 13, 14, 15, 99, 11], Some(0), 4));
        assert_eq!(laws.pos, 0.);
        laws.day = Some(5);
        settle(&mut laws);
        assert_eq!(laws.pos, 1.);
        // A guess that was wrong (read anew a row further): the shown laws move on to it.
        laws.take(table(&[14, 15, 99, 11, 12, 13], Some(0), 5));
        assert_eq!(laws.pos, -1.);
        settle(&mut laws);
        assert_eq!(laws.pos, 0.);
        // A different list: shown from its first row.
        laws.pos = 0.5;
        laws.take(table(&[1, 2, 3, 4, 5, 6], Some(0), 5));
        assert_eq!(laws.pos, 0.);
    }
    #[test]
    fn the_laws_keep_moving_on_into_the_next_month() {
        // Read on day 18 (measured: day 20 is followed by day 1).
        let mut laws = Laws::default();
        laws.take(table(&(10..30).collect::<Vec<_>>(), Some(0), 18));
        for (day, pos) in [(19, 1.), (20, 2.), (1, 3.), (2, 4.), (5, 7.)] {
            laws.day = Some(day);
            settle(&mut laws);
            assert_eq!(laws.pos, pos, "day {day}");
        }
    }
    #[test]
    fn the_laws_survive_the_resume_state() {
        let mut laws = Laws::default();
        laws.take(table(&[10, 11, 12], Some(2), 7));
        laws.date = Some(42);
        let mut w = savestate::W(Vec::new());
        laws.save(&mut w);
        let back = Laws::load(&mut savestate::R::new(&w.0)).unwrap();
        let t = back.table.as_ref().unwrap();
        assert_eq!((back.date, t.rows.len(), t.rows[2][0], t.colors.get(&1), t.marked, t.day, t.dark.get(&3)), (Some(42), 3, 12, Some(&2), Some(2), 7, Some(&4)));
        // Nothing kept: nothing shown.
        let mut w = savestate::W(Vec::new());
        Laws::default().save(&mut w);
        assert!(Laws::load(&mut savestate::R::new(&w.0)).unwrap().table.is_none());
    }
    #[test]
    fn the_pointer_is_the_lists_small_sprite_only() {
        let mut m = gba_core::Machine::new(vec![0; 0x200]);
        let set = |m: &mut gba_core::Machine, slot: usize, a: [u16; 3]| {
            for (k, v) in a.iter().enumerate() { m.bus.oam[slot * 8 + 2 * k..slot * 8 + 2 * k + 2].copy_from_slice(&v.to_le_bytes()); }
        };
        for slot in 0..128 { set(&mut m, slot, [0x0200, 0, 0]); }
        // "L Law" (tile 0x40) is not it; the pointer at (8, 36) is, on row 32.
        set(&mut m, 1, [0x0004, 0x4004, 0x0040]);
        assert_eq!(pointer(&m.bus), None);
        set(&mut m, 0, [0x4024, 0x0008, 0x0000]);
        assert_eq!(pointer(&m.bus), Some(36));
        // Hidden, it is not shown.
        set(&mut m, 0, [0x4224, 0x0008, 0x0000]);
        assert_eq!(pointer(&m.bus), None);
    }
}
