//! Touch-only portrait battles: the game's battle lists drawn again from its
//! own tiles, docked whole at the bottom, and the unit's commands flattened.
//!
//! A battle list window (USA, measured on Herb Picking) is on BG2: a bar row
//! (tile 0x005, then 0x006), the title row (0x007, the title's glyph tiles,
//! then 0x008), two tile rows per entry (the entry's own tiles, the selected
//! one in palette 11, the others in 12; a bullet column of palette 13 on the
//! left), and a bottom bar (0x009, then 0x00a). Its body is BG3 tile 1 under
//! the entries, blended by BLDALPHA. Windows are drawn again from these
//! tiles: whole (the game's are cut at the screen edge), entries in their
//! unselected colours (touch has no cursor to show), docked by the unit's
//! panel.
//!
//! The unit's commands (Move, Action, Wait, Status) put Action's entries
//! (Fight, its ability sets, Item) in place of Action: one tap for each. They
//! are read in a silent run from a snapshot when the commands are shown (as
//! the laws are, laws.rs): the cursor to Action, A, the Action window's
//! entries taken once it is still. A tap on one of them chooses Action, then
//! it. Where Action cannot be chosen (greyed after acting) the commands stay
//! as they are.
use crate::battle_ui::{window, Window};
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, mem::MemMap, savestate};

const A: u16 = 0x3fe;
const UP: u16 = 0x3bf;
const DOWN: u16 = 0x37f;
const RELEASED: u16 = 0x3ff;
/// Silent frames run per frame shown, and at most in all.
const FRAMES_PER_TICK: u32 = 6;
const RUN_FRAMES: u32 = 120;
/// Native rows the unit's panel takes at the bottom (with a gap): lists too
/// wide to stand beside it go above it.
pub(crate) const PANEL_ROWS: i32 = 72;
const PANEL_WIDTH: i32 = 136;

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

/// A window's pixels as palette codes (bank << 4 | index; 0 transparent),
/// `width` wide: its bars and title (16 rows), its entries (16 rows each), its
/// bottom bar (8 rows). `body` is the body's colour code (under the entries).
#[derive(Clone, Debug, PartialEq)]
pub struct Image { pub width: usize, pub header: Vec<u8>, pub entries: Vec<Vec<u8>>, pub footer: Vec<u8>, pub body: u8 }

/// One 8x8 tile cell of a BG (4-bpp text BG `map`/`chars`), as codes.
fn cell(bus: &MemMap, chars: usize, e: u16, normalize: bool) -> [u8; 64] {
    let mut out = [0u8; 64];
    let tile = (e & 0x3ff) as usize;
    let bank = (e >> 12) as u8;
    let bank = if normalize && bank == 11 { 12 } else { bank };
    for y in 0..8 {
        for x in 0..8 {
            let (sx, sy) = (if e & 0x400 != 0 { 7 - x } else { x }, if e & 0x800 != 0 { 7 - y } else { y });
            let b = bus.vram.get(chars + tile * 32 + sy * 4 + sx / 2).copied().unwrap_or(0);
            let index = (b >> (4 * (sx & 1))) & 15;
            if index != 0 { out[y * 8 + x] = bank << 4 | index; }
        }
    }
    out
}

/// Tile rows `rows` of BG2 from column `left`, `columns` wide, as codes.
fn band(bus: &MemMap, rows: std::ops::Range<usize>, left: usize, columns: usize, normalize: bool) -> Vec<u8> {
    let cnt = half(&bus.io, 0x0c);
    let (map, chars) = (((cnt >> 8) & 31) as usize * 0x800, ((cnt >> 2) & 3) as usize * 0x4000);
    let width = columns * 8;
    let mut out = vec![0u8; rows.len() * 8 * width];
    for (r, row) in rows.enumerate() {
        for c in 0..columns {
            let e = half(&bus.vram, map + ((row % 32) * 32 + (left + c) % 32) * 2);
            if e == 0 { continue; }
            let px = cell(bus, chars, e, normalize);
            for y in 0..8 { out[(r * 8 + y) * width + c * 8..][..8].copy_from_slice(&px[y * 8..y * 8 + 8]); }
        }
    }
    out
}

/// The list window whose first entry is at `scene.origin`, drawn again from its tiles.
pub fn capture(bus: &MemMap, scene: &battle::MenuScene) -> Option<Image> {
    let scroll = half(&bus.io, 0x18) as i32;
    let title = (scene.origin[1] / 8 - 1).rem_euclid(32) as usize;
    let left = (((scene.origin[0] + scroll) / 8).rem_euclid(32) + 2) as usize;
    let cnt = half(&bus.io, 0x0c);
    let map = ((cnt >> 8) & 31) as usize * 0x800;
    let entry = |row: usize, col: usize| half(&bus.vram, map + ((row % 32) * 32 + col % 32) * 2);
    if title == 0 || entry(title, left) & 0x3ff != 0x007 || entry(title - 1, left) & 0x3ff != 0x005 { return None; }
    let columns = (1..32 - left).take_while(|&c| entry(title - 1, left + c) & 0x3ff == 0x006).count() + 1;
    let count = scene.count as usize;
    let bottom = title + 1 + 2 * count;
    if entry(bottom, left) & 0x3ff != 0x009 { return None; }
    let header = band(bus, title - 1..title + 1, left, columns, false);
    let entries = (0..count).map(|i| band(bus, title + 1 + 2 * i..title + 3 + 2 * i, left, columns, true)).collect();
    let footer = band(bus, bottom..bottom + 1, left, columns, false);
    // The body: BG3 tile 1 (its first pixel), palette 12.
    let b3 = half(&bus.io, 0x0e);
    let chars3 = ((b3 >> 2) & 3) as usize * 0x4000;
    let body = 12 << 4 | (bus.vram.get(chars3 + 32).copied().unwrap_or(0) & 15);
    Some(Image { width: columns * 8, header, entries, footer, body })
}

/// Rows widened to `width`: bars by their last column, entries by nothing.
fn widen(rows: &[u8], from: usize, to: usize, repeat: bool) -> Vec<u8> {
    if to <= from { return rows.to_vec(); }
    rows.chunks_exact(from).flat_map(|line| {
        let mut l = line.to_vec();
        let last = line[from - 1];
        l.resize(to, if repeat { last } else { 0 });
        if repeat { for x in from..to { l[x] = line[from - 8 + (x - from) % 8]; } }
        l
    }).collect()
}

impl Image {
    pub fn height(&self) -> usize { 16 + 16 * self.entries.len() + 8 }
    fn widened(&self, width: usize) -> Image {
        Image { width: width.max(self.width), header: widen(&self.header, self.width, width, true),
            entries: self.entries.iter().map(|e| widen(e, self.width, width, false)).collect(),
            footer: widen(&self.footer, self.width, width, true), body: self.body }
    }
}

/// What a shown entry chooses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick { Row(u8), Action(u8), DoIt, Cancel }

/// A window drawn: its entries' texture rects (x, y, w, h) and what each picks.
#[derive(Clone, Debug, Default)]
pub struct Shown { pub rows: Vec<([i32; 4], Pick)>, pub rect: [i32; 4] }

struct Run { state: Vec<u8>, frame: u32, keys: Vec<u16>, hold: u32, last: Option<(battle::MenuScene, Image)>, still: u32 }

#[derive(Default)]
pub struct Flat {
    /// The commands it was read for, and Action's entries (None: Action cannot be chosen).
    key: Option<Vec<u8>>,
    action: Option<Image>,
    run: Option<Run>,
    /// A tap on one of Action's entries: Action chosen, then entry `.0`.
    goal: Option<(u8, u64, bool)>,
    /// The window drawn last and where (shown on while Action passes by).
    last: Option<(Image, i32, i32)>,
    pub shown: Shown,
    /// A target was chosen (the frame): its preview shows as a card.
    pub card: Option<u64>,
    /// Do it pressed: A on the preview, then Do it in the game's window.
    card_goal: Option<u64>,
    card_seen: bool,
    card_away: u32,
    /// The game's own list is drawn again (export_ui leaves its panel out).
    pub hide_game: bool,
}

/// Draws the codes `pixels` (`width` wide) at texture (x0, y0), unblended.
/// `palette`: the BG palettes the codes are of (the game's, or kept).
pub(crate) fn paste(bus: &MemMap, palette: &[u8], out: &mut [i32], rw: usize, rh: usize, x0: i32, y0: i32, width: usize, pixels: &[u8]) {
    let colors = crate::engine::rgba_colors();
    for (i, &code) in pixels.iter().enumerate() {
        let (tx, ty) = (x0 + (i % width) as i32, y0 + (i / width) as i32);
        if code == 0 || !(0..rw as i32).contains(&tx) || !(0..rh as i32).contains(&ty) { continue; }
        let c = half(palette, ((code >> 4) as usize * 16 + (code & 15) as usize) * 2) & 0x7fff;
        out[ty as usize * rw + tx as usize] = colors[crate::world_map::faded(bus, c, 0x04) as usize];
    }
}

impl Engine {
    /// No silent run or choice of Action's entries under way.
    pub(crate) fn flat_idle(&self) -> bool { self.flat.run.is_none() && self.flat.goal.is_none() }
}

/// Draws `image` into the texture with its top left at texture (x0, y0).
fn draw(bus: &MemMap, out: &mut [i32], rw: usize, rh: usize, x0: i32, y0: i32, image: &Image) {
    let colors = crate::engine::rgba_colors();
    let alpha = half(&bus.io, 0x52);
    let (eva, evb) = ((alpha & 31).min(16) as u32, ((alpha >> 8) & 31).min(16) as u32);
    let color = |code: u8| half(&bus.palette, ((code >> 4) as usize * 16 + (code & 15) as usize) * 2) & 0x7fff;
    let mut put = |x: i32, y: i32, code: u8, blend: bool| {
        let (tx, ty) = (x0 + x, y0 + y);
        if code == 0 || !(0..rw as i32).contains(&tx) || !(0..rh as i32).contains(&ty) { return; }
        let target = &mut out[ty as usize * rw + tx as usize];
        let c = color(code);
        if blend {
            let under = *target as u32;
            let world = ((under >> 19) & 31) | ((under >> 11) & 31) << 5 | ((under >> 3) & 31) << 10;
            let mixed = [0, 5, 10].iter().fold(0, |acc, &s| acc | ((((c as u32 >> s) & 31) * eva + ((world >> s) & 31) * evb) / 16).min(31) << s);
            *target = colors[mixed as usize];
        } else {
            *target = colors[crate::world_map::faded(bus, c, 0x04) as usize];
        }
    };
    let w = image.width as i32;
    for (i, &p) in image.header.iter().enumerate() { put(i as i32 % w, i as i32 / w, p, false); }
    for (k, entry) in image.entries.iter().enumerate() {
        let top = 16 + 16 * k as i32;
        for y in 0..16 { for x in 0..w { put(x, top + y, image.body, true); } }
        for (i, &p) in entry.iter().enumerate() { put(i as i32 % w, top + i as i32 / w, p, false); }
    }
    let top = 16 + 16 * image.entries.len() as i32;
    for (i, &p) in image.footer.iter().enumerate() { put(i as i32 % w, top + i as i32 / w, p, false); }
}

/// The preview of the action being chosen (USA, measured): its record is
/// pointed at by EWRAM 0x0200f588; +0x2d the hit chance (%), +0x2e the HP it
/// takes (negative: restores). Shown while the game waits to be told Do it.
pub fn preview(bus: &MemMap) -> Option<(u8, i16)> {
    let p = u32::from_le_bytes(bus.ewram.get(0xf588..0xf58c)?.try_into().ok()?);
    let o = p.checked_sub(0x0200_0000)? as usize;
    let rec = bus.ewram.get(o..o + 0x30)?;
    Some((rec[0x2d], i16::from_le_bytes([rec[0x2e], rec[0x2f]])))
}
/// The preview is up: the targets' panels (the extra panel's stats) and no
/// list, cell or facing to choose.
pub fn previewing(bus: &MemMap) -> bool {
    bus.ffta_dialogue.extra.kind == gba_core::ffta_dialogue::PanelKind::Stats && battle::MenuScene::read(bus).is_none()
        && battle::Scene::read(bus).is_none() && battle::facing_center(bus).is_none()
}

impl Engine {
    /// Before the frames: reads Action's entries for the commands shown (a
    /// few silent frames at a time), and drives a chosen entry of them.
    pub(crate) fn flat_tick(&mut self) -> Result<(), String> {
        self.drive_flat_goal();
        self.drive_card();
        let bus = &self.machine.bus;
        let wanted = self.touch_enabled && bus.ffta_dialogue.docked && self.usa;
        let commands = window(bus).filter(|w| w.0 == Window::Commands).and_then(|(_, s)| capture(bus, &s).map(|i| (s, i)));
        let Some((scene, image)) = commands.filter(|_| wanted) else { self.flat.run = None; return Ok(()) };
        // The commands as drawn (greyed entries too) and the unit's cell.
        let mut key: Vec<u8> = image.entries.concat();
        key.extend_from_slice(&bus.ewram[0xf3b8..0xf3c4]);
        if self.flat.key.as_ref() == Some(&key) && self.flat.run.is_none() { return Ok(()); }
        if self.flat.run.is_none() {
            if !battle::idle(bus) || self.gesture.is_some() || self.flat.goal.is_some() { return Ok(()); }
            // Up or Down to Action (entry 1), then A.
            let s = scene.selected as i32;
            let mut keys = vec![if s < 1 { DOWN } else { UP }; (s - 1).unsigned_abs() as usize];
            keys.push(A);
            self.flat.key = Some(key);
            self.flat.action = None;
            self.flat.run = Some(Run { state: savestate::snapshot(&self.machine), frame: 0, keys, hold: 0, last: None, still: 0 });
        }
        let real = savestate::snapshot(&self.machine);
        let skip = self.machine.bus.skip_render;
        let mut run = self.flat.run.take().unwrap();
        savestate::restore(&mut self.machine, &run.state)?;
        let mut end = None;
        for _ in 0..FRAMES_PER_TICK {
            let bus = &mut self.machine.bus;
            // Each key held 2 frames, then 3 released.
            bus.keys = match run.keys.first() { Some(&k) if run.hold < 2 => k, _ => RELEASED };
            if !run.keys.is_empty() { run.hold += 1; if run.hold >= 5 { run.keys.remove(0); run.hold = 0; } }
            bus.skip_render = true;
            self.step_frame()?;
            let bus = &mut self.machine.bus;
            bus.audio_buf.clear();
            run.frame += 1;
            if run.frame > RUN_FRAMES { end = Some(None); break; }
            if !run.keys.is_empty() { continue; }
            let Some((Window::Action, scene)) = window(bus) else { run.still = 0; continue };
            let Some(image) = capture(bus, &scene) else { continue };
            if run.last.as_ref().is_some_and(|(s, i)| s.origin == scene.origin && *i == image) { run.still += 1; } else { run.still = 0; run.last = Some((scene, image)); }
            if run.still >= 3 { end = Some(run.last.take().map(|l| l.1)); break; }
        }
        if end.is_none() { run.state = savestate::snapshot(&self.machine); }
        savestate::restore(&mut self.machine, &real)?;
        self.machine.bus.skip_render = skip;
        match end {
            Some(action) => self.flat.action = action,
            None => self.flat.run = Some(run),
        }
        Ok(())
    }

    /// The card: kept from the target's choice until the preview passes;
    /// Do it waits for the game's Do it / Cancel window, then takes Do it.
    fn drive_card(&mut self) {
        let bus = &mut self.machine.bus;
        if let Some(since) = self.flat.card {
            // Until the preview has come (it takes a moment) and gone, or never came.
            // The panels come in a few frames apart: gone after 12 frames without.
            if previewing(bus) { self.flat.card_seen = true; self.flat.card_away = 0; } else { self.flat.card_away += 1; }
            let gone = self.flat.card_seen && self.flat.card_away > 12 && self.flat.card_goal.is_none();
            if gone || (!self.flat.card_seen && bus.frames > since + 240) { self.flat.card = None; self.flat.card_seen = false; }
        }
        let Some(deadline) = self.flat.card_goal else { return };
        if bus.frames > deadline { self.flat.card_goal = None; self.flat.card = None; return; }
        if !battle::idle(bus) { return; }
        if let Some(scene) = battle::MenuScene::read(bus).filter(|m| m.count == 2) {
            self.flat.card_goal = None;
            self.flat.card = None;
            battle::aim(bus, (scene.origin[0] + 16) as f32, (scene.origin[1] + 4) as f32, true);
        }
    }
    /// The player confirmed a target: its preview will show as the card.
    pub(crate) fn target_chosen(&mut self) { self.flat.card = Some(self.machine.bus.frames); self.flat.card_seen = false; }

    /// Chooses Action, then its entry, once its window is up.
    fn drive_flat_goal(&mut self) {
        let Some((entry, deadline, aimed)) = self.flat.goal else { return };
        let bus = &mut self.machine.bus;
        if bus.frames > deadline { self.flat.goal = None; return; }
        if !battle::idle(bus) { return; }
        match window(bus) {
            Some((Window::Action, scene)) => {
                self.flat.goal = None;
                battle::aim(bus, (scene.origin[0] + 16) as f32, (scene.origin[1] + 16 * entry as i32 + 4) as f32, true);
            }
            Some((Window::Commands, scene)) if !aimed => {
                self.flat.goal = Some((entry, deadline, true));
                battle::aim(bus, (scene.origin[0] + 16) as f32, (scene.origin[1] + 20) as f32, true);
            }
            _ => {}
        }
    }

    /// After the frame: the battle's lists drawn again, docked by the unit's
    /// panel at the bottom (`dock` texture rows).
    pub(crate) fn battle_windows_present(&mut self, dock: (i32, i32)) {
        self.flat.shown = Shown::default();
        self.flat.hide_game = false;
        let bus = &self.machine.bus;
        if !(self.touch_enabled && bus.ffta_dialogue.docked && self.usa) { return; }
        if self.flat.goal.is_some() {
            // On the way to one of Action's entries: the commands stay as they were.
            if let Some((image, x0, y0)) = self.flat.last.clone() {
                let (rw, rh) = (gba_core::mem::render_width(), gba_core::mem::render_height());
                draw(bus, &mut self.pixels, rw, rh, x0, y0, &image);
                self.flat.hide_game = true;
            }
            return;
        }
        if self.flat.card.is_some() && self.flat.card_seen && preview(bus).is_some_and(|p| p.0 > 0) {
            if let Some((hit, hp)) = preview(bus) { self.draw_card(dock, hit, hp); }
            return;
        }
        let Some((kind, scene)) = window(bus) else { return };
        if !matches!(kind, Window::Commands | Window::Action | Window::Abilities | Window::Items | Window::System) { return; }
        let Some(image) = capture(bus, &scene) else { return };
        let (image, picks): (Image, Vec<Pick>) = match (&kind, &self.flat.action) {
            (Window::Commands, Some(action)) if image.entries.len() == 4 => {
                let width = image.width.max(action.width);
                let (c, a) = (image.widened(width), action.widened(width));
                let mut entries = vec![c.entries[0].clone()];
                entries.extend(a.entries.iter().cloned());
                entries.extend(c.entries[2..].iter().cloned());
                let mut picks = vec![Pick::Row(0)];
                picks.extend((0..a.entries.len() as u8).map(Pick::Action));
                picks.extend([Pick::Row(2), Pick::Row(3)]);
                (Image { entries, ..c }, picks)
            }
            _ => { let n = image.entries.len() as u8; (image, (0..n).map(Pick::Row).collect()) }
        };
        let (rw, rh) = (gba_core::mem::render_width(), gba_core::mem::render_height());
        let ex = gba_core::mem::extra() as i32;
        let (w, h) = (image.width as i32, image.height() as i32);
        // Beside the unit's panel when it fits there, else above it.
        // The commands beside the unit's panel; lists over it (they are wider).
        let bottom = if kind == Window::Commands && w <= 240 - PANEL_WIDTH { dock.1 - 2 } else { dock.1 - PANEL_ROWS };
        let (x0, y0) = (ex + 240 - w, bottom - h);
        draw(bus, &mut self.pixels, rw, rh, x0, y0, &image);
        self.flat.last = Some((image.clone(), x0, y0));
        self.flat.shown = Shown {
            rows: picks.into_iter().enumerate().map(|(k, p)| ([x0, y0 + 16 + 16 * k as i32, w, 16], p)).collect(),
            rect: [x0, y0, w, h],
        };
        self.flat.hide_game = true;
    }

    /// The preview as a card over the targets' panels: the chance and the HP
    /// it takes (or restores) large, then Do it and Cancel.
    fn draw_card(&mut self, dock: (i32, i32), hit: u8, hp: i16) {
        let bus = &self.machine.bus;
        let (rw, rh) = (gba_core::mem::render_width(), gba_core::mem::render_height());
        let ex = gba_core::mem::extra() as i32;
        let (w, body) = (200, 112);
        let (x0, y0) = (ex + (240 - w) / 2, dock.1 - PANEL_ROWS - body - 10);
        let chance = format!("Hit {hit}%");
        let amount = if hp < 0 { format!("Restores {}", -hp) } else { format!("Damage {hp}") };
        let half_w = w / 2;
        let center = |text: &str, scale: i32, from: i32, width: i32| from + (width - crate::game_buttons::text_width_scaled(&bus.rom, text, scale)) / 2;
        let lines = [
            (center(&chance, 2, 0, w), 2, chance.as_str(), false, 2),
            (center(&amount, 2, 0, w), 38, amount.as_str(), false, 2),
            (center("Do it", 2, 0, half_w), 76, "Do it", true, 2),
            (center("Cancel", 2, half_w, half_w), 76, "Cancel", false, 2),
        ];
        crate::game_buttons::draw_card(bus, &mut self.pixels, rw, rh, x0, y0, w, body, &lines);
        let row = y0 + 5 + 70;
        self.flat.shown = Shown { rows: vec![([x0, row, half_w, 44], Pick::DoIt), ([x0 + half_w, row, half_w, 44], Pick::Cancel)], rect: [x0, y0, w, body + 10] };
    }

    /// A tap at screen pixels on a drawn battle list: its entry chosen. True when taken.
    pub(crate) fn battle_windows_tap(&mut self, px: f32, py: f32) -> bool {
        let Some(pick) = self.battle_pick(px, py) else { return self.battle_window_at(px, py) };
        self.mouse.cancel();
        let bus = &mut self.machine.bus;
        match pick {
            Pick::DoIt => { self.flat.card_goal = Some(bus.frames + 240); battle::press(bus, A); return true; }
            Pick::Cancel => { self.flat.card = None; battle::press(bus, 0x3fd); return true; }
            _ => {}
        }
        let Some(scene) = battle::MenuScene::read(bus) else { return true };
        match pick {
            Pick::Row(i) => { battle::aim(bus, (scene.origin[0] + 16) as f32, (scene.origin[1] + 16 * i as i32 + 4) as f32, true); }
            Pick::Action(k) => { self.flat.goal = Some((k, bus.frames + 180, false)); }
            _ => {}
        }
        true
    }
    /// The entry of a drawn battle list at screen pixels.
    pub(crate) fn battle_pick(&self, px: f32, py: f32) -> Option<Pick> {
        let (rect, s) = self.texture_mapping();
        let (tx, ty) = ((px - rect[0]) / s, (py - rect[1]) / s);
        self.flat.shown.rows.iter().find(|(r, _)| tx >= r[0] as f32 && tx < (r[0] + r[2]) as f32 && ty >= r[1] as f32 && ty < (r[1] + r[3]) as f32).map(|(_, p)| *p)
    }
    /// Screen pixels inside a drawn battle list (its bars too).
    pub(crate) fn battle_window_at(&self, px: f32, py: f32) -> bool {
        let (rect, s) = self.texture_mapping();
        let (tx, ty) = ((px - rect[0]) / s, (py - rect[1]) / s);
        let r = self.flat.shown.rect;
        r[2] > 0 && tx >= r[0] as f32 && tx < (r[0] + r[2]) as f32 && ty >= r[1] as f32 && ty < (r[1] + r[3]) as f32
    }
    /// The texture's screen rect and screen pixels per texel.
    fn texture_mapping(&self) -> ([f32; 4], f32) {
        let rect = self.texture_rect();
        (rect, (rect[3] / gba_core::mem::render_height() as f32).max(1e-3))
    }
    /// The native point of the game's own entry a drawn entry stands for (its help).
    pub(crate) fn battle_pick_native(&self, pick: Pick) -> Option<(f32, f32)> {
        let Pick::Row(i) = pick else { return None };
        let scene = battle::MenuScene::read(&self.machine.bus)?;
        Some(((scene.origin[0] + 16) as f32, (scene.origin[1] + 16 * i as i32 + 4) as f32))
    }
}
