//! Touch-only portrait shop lists that scroll like phone lists, and swipe
//! between categories.
//!
//! The game draws only the rows it shows (four) and scrolls a row per Up or
//! Down. To follow the finger every row is kept as the game draws it: a silent
//! look-ahead run (machine snapshot, hidden frames stepping through the list
//! with item Info closed, restore) collects them, a few frames per frame. The
//! list is then drawn from those rows at any offset: dragged, flung, settled on
//! a whole row. Settled, the game is moved to the same first row and
//! selection with its own Up/Down (a step per 4 frames, cursor sound
//! skipped), its other panels held as they were until it is there. With Info
//! open (drawn over the list's map) the rows also come from what was kept.
//!
//! A sideways swipe changes category with the game's own category change: the
//! rows slide out and the new list slides in.
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, ffta_dialogue::{UiPixel, LAYER_INSET, LAYER_WIDTH}, ffta_shop_mouse as shop, savestate};

const ROW: i32 = 16;
/// A row's glyphs start a line above the list's row origin (their outline).
const ROW_TOP: i32 = -1;
const UP: u16 = 0x3bf;
const DOWN: u16 = 0x37f;
const R: u16 = 0x2ff;
const RELEASED: u16 = 0x3ff;
/// Look-ahead frames per frame: one list step (a few milliseconds on a phone).
const LOOKAHEAD_FRAMES: u32 = 4;
/// Fling friction per frame, settling speed, slide speed (native px per frame).
const FRICTION: f32 = 0.93;
const SETTLE: f32 = 0.35;
const SLIDE_SPEED: f32 = 28.;

#[derive(Clone, Copy, Debug, PartialEq)]
struct ListKey { sell: bool, category: u8, list: usize, count: u16 }
impl ListKey {
    fn of(l: &shop::ShopList) -> Self { Self { sell: l.sell, category: l.category, list: l.list, count: l.count } }
}

/// A row as captured: 16 canvas lines.
type Lines = Vec<UiPixel>;

struct LookAhead { state: Vec<u8>, frame: u32, key: u16, down: Option<bool>, wait: u32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Slide {
    Idle,
    /// Following the finger sideways (native px).
    Drag(f32),
    /// Leaving towards `dir` (+1 right), then waiting for the new list.
    Out { dir: f32, offset: f32, from: (u8, bool) },
    /// The new list arriving from the other side.
    In { offset: f32 },
    /// Back to rest (no category that way).
    Back(f32),
}

pub(crate) struct ShopScroll {
    key: Option<ListKey>,
    /// First row's native top, rows shown, the list's right edge.
    top: i32,
    visible: u8,
    right: i32,
    rows: Vec<Option<Lines>>,
    selected_rows: Vec<Option<Lines>>,
    look: Option<LookAhead>,
    /// Scroll offset (native px of the first row shown), its speed.
    pub(crate) pos: f32,
    velocity: f32,
    pub(crate) dragging: bool,
    settle: Option<f32>,
    /// The first row and selected row the game is being moved to.
    target: Option<(u16, u8)>,
    /// The canvases held while the game catches up (main, Info).
    frozen: Option<(Lines, Lines)>,
    freeze_next: bool,
    pub(crate) slide: Slide,
    /// The shown rows as they were before sliding (they leave with a swipe).
    leaving: Option<Lines>,
    /// The list window's native x extent and fill on each row line (learnt from
    /// a kept row): what shows where no row is.
    window: Option<((i32, i32), Vec<UiPixel>)>,
    /// A button pressed while the game catches up.
    pub(crate) deferred: Option<u16>,
}

impl Default for ShopScroll {
    fn default() -> Self {
        Self { key: None, top: 0, visible: 0, right: 0, rows: Vec::new(), selected_rows: Vec::new(), look: None, pos: 0.,
            velocity: 0., dragging: false, settle: None, target: None, frozen: None, freeze_next: false, slide: Slide::Idle, leaving: None, window: None, deferred: None }
    }
}

impl ShopScroll {
    fn complete(&self) -> bool { !self.rows.is_empty() && self.rows.iter().all(|r| r.is_some()) }
    fn max_pos(&self) -> f32 { (self.rows.len() as i32 - self.visible as i32).max(0) as f32 * ROW as f32 }
    /// The list moves on its own or the game is catching up with it.
    pub(crate) fn busy(&self) -> bool {
        self.dragging || self.velocity != 0. || self.settle.is_some() || self.target.is_some() || self.slide != Slide::Idle
    }
    /// The game is being moved to the shown rows (buttons wait for it).
    pub(crate) fn catching_up(&self) -> bool { self.target.is_some() || self.settle.is_some() || self.velocity != 0. }
    /// Dragged to `pos` (rubber band past the ends).
    pub(crate) fn drag_to(&mut self, raw: f32) {
        let max = self.max_pos();
        self.pos = if raw < 0. { raw * 0.35 } else if raw > max { max + (raw - max) * 0.35 } else { raw };
    }
    /// Settled at `pos`: the game follows, keeping its selection when shown;
    /// otherwise the selection is the row its cursor reaches first (the
    /// bottom one when the list moved down, the top one when it moved up).
    fn follow(&mut self, l: &shop::ShopList, pos: f32) {
        let first = (pos / ROW as f32) as u16;
        let selected = match self.target { Some((f, r)) => f + r as u16, None => l.first + l.row as u16 };
        let last = (first + l.visible as u16 - 1).min(l.count - 1);
        let row = (if selected < first { last } else if selected > last { first } else { selected } - first) as u8;
        if (first, row) != (l.first, l.row) {
            self.target = Some((first, row));
            self.freeze_next = self.frozen.is_none();
        }
    }
    pub(crate) fn release(&mut self, velocity: f32) {
        self.dragging = false;
        self.velocity = if self.complete() { velocity.clamp(-48., 48.) } else { 0. };
    }
    fn capture(&mut self, l: &shop::ShopList, canvas: &[UiPixel]) {
        for r in 0..l.visible as u16 {
            let item = (l.first + r) as usize;
            if item >= self.rows.len() { break; }
            let y = l.origin[1] + ROW_TOP + r as i32 * ROW;
            let Some(lines) = canvas.get(y.max(0) as usize * LAYER_WIDTH..(y + ROW).max(0) as usize * LAYER_WIDTH) else { continue };
            if y < 0 { continue; }
            if self.window.is_none() { self.learn_window(lines); }
            if r as u8 == l.row { self.selected_rows[item] = Some(lines.to_vec()); } else { self.rows[item] = Some(lines.to_vec()); }
        }
    }
    /// The list window's fill at native x on row line `line` (nothing outside it).
    fn background(&self, line: usize, x: i32) -> UiPixel {
        match &self.window {
            Some(((left, right), fill)) if x >= *left && x < *right => fill[line],
            _ => UiPixel::default(),
        }
    }
    /// Learns the window's extent and fill from a kept row: its visible span,
    /// and on each line the most common pixel (the window's own fill).
    fn learn_window(&mut self, row: &[UiPixel]) {
        let mut span = (i32::MAX, i32::MIN);
        let mut fill = Vec::with_capacity(ROW as usize);
        for line in 0..ROW as usize {
            let pixels = &row[line * LAYER_WIDTH + LAYER_INSET as usize..][..240];
            let mut counts: Vec<([u8; 4], usize, UiPixel)> = Vec::new();
            for (x, p) in pixels.iter().enumerate() {
                if !p.visible() { continue; }
                span = (span.0.min(x as i32), span.1.max(x as i32 + 1));
                match counts.iter_mut().find(|c| c.0 == p.packed()) { Some(c) => c.1 += 1, None => counts.push((p.packed(), 1, *p)) }
            }
            fill.push(counts.iter().max_by_key(|c| c.1).map_or(UiPixel::default(), |c| c.2));
        }
        if span.0 < span.1 { self.window = Some(((span.0, span.1), fill)); }
    }
    /// Draws rows from `pos` into the canvas (selected item `selected`), shifted
    /// sideways by `dx`.
    fn draw(&self, canvas: &mut [UiPixel], selected: Option<usize>, dx: i32) {
        let lines = self.visible as i32 * ROW;
        let pos = self.pos.round() as i32;
        for ly in 0..lines {
            let src = pos + ly;
            let (item, line) = (src.div_euclid(ROW), src.rem_euclid(ROW) as usize);
            let row = (item >= 0).then(|| item as usize).and_then(|i| {
                if Some(i) == selected { self.selected_rows.get(i)?.as_ref().or(self.rows.get(i)?.as_ref()) } else { self.rows.get(i)?.as_ref() }
            });
            let y = self.top + ly;
            if !(0..160).contains(&y) { continue; }
            for x in 0..self.right {
                let sx = x - dx;
                let p = match row {
                    Some(r) if (0..self.right).contains(&sx) && (dx == 0 || self.background(line, sx).visible()) =>
                        r[line * LAYER_WIDTH + (sx + LAYER_INSET) as usize],
                    _ => self.background(line, x),
                };
                canvas[y as usize * LAYER_WIDTH + (x + LAYER_INSET) as usize] = p;
            }
        }
    }
    /// The canvas's shown rows (native x 0..right of each line).
    fn rows_region(&self, canvas: &[UiPixel]) -> Lines {
        (0..self.visible as i32 * ROW).flat_map(|ly| {
            let y = (self.top + ly).clamp(0, 159) as usize;
            let start = y * LAYER_WIDTH + LAYER_INSET as usize;
            canvas[start..start + self.right as usize].to_vec()
        }).collect()
    }
    /// Paints shown rows `rows` (from `rows_region`) shifted sideways by `dx`.
    fn paint_shifted(&self, canvas: &mut [UiPixel], rows: &[UiPixel], dx: i32) {
        let width = self.right as usize;
        for ly in 0..self.visible as i32 * ROW {
            let y = self.top + ly;
            if !(0..160).contains(&y) || rows.len() < (ly as usize + 1) * width { continue; }
            let start = y as usize * LAYER_WIDTH + LAYER_INSET as usize;
            let line = ly.rem_euclid(ROW) as usize;
            for x in 0..self.right {
                let sx = x - dx;
                // Rows move inside the window: what leaves it is not drawn, the window's fill shows behind.
                canvas[start + x as usize] = if (0..self.right).contains(&sx) && (dx == 0 || self.background(line, sx).visible()) && self.background(line, x).visible() {
                    rows[ly as usize * width + sx as usize]
                } else if self.window.is_some() { self.background(line, x) } else { canvas[start + x as usize] };
            }
        }
    }
}

impl Engine {
    fn portrait_shop_list(&self) -> Option<shop::ShopList> {
        if !self.town_buttons() { return None; }
        shop::shop_list(&self.machine.bus)
    }
    /// Once per frame, before the machine runs: look-ahead, motion, catching up.
    pub(crate) fn shop_scroll_tick(&mut self) -> Result<(), String> {
        let list = self.portrait_shop_list();
        let phase = shop::phase(&self.machine.bus);
        // Leaving the list (buying, selling) may change any row: start again on return.
        if list.is_none() && !matches!(phase, Some(shop::ShopPhase::List { .. }) | Some(shop::ShopPhase::Busy)) || !self.town_buttons() {
            if self.shop_scroll.key.is_some() || self.shop_scroll.slide != Slide::Idle { self.shop_scroll = Default::default(); }
            self.machine.bus.ffta_battle_mouse.silence_cursor = false;
            return Ok(());
        }
        self.slide_tick(list);
        let Some(l) = list else { return Ok(()) };
        let s = &mut self.shop_scroll;
        if s.key != Some(ListKey::of(&l)) {
            let (slide, leaving, window) = (s.slide, s.leaving.take(), s.window.take());
            *s = ShopScroll { key: Some(ListKey::of(&l)), top: l.origin[1] + ROW_TOP, visible: l.visible, right: l.right,
                rows: vec![None; l.count as usize], selected_rows: vec![None; l.count as usize], pos: l.first as f32 * ROW as f32,
                slide, leaving, window, ..Default::default() };
        }
        // Motion.
        if !s.dragging {
            let max = s.max_pos();
            if s.velocity != 0. {
                s.pos += s.velocity;
                s.velocity *= FRICTION;
                if s.pos < 0. || s.pos > max { s.pos = s.pos.clamp(0., max); s.velocity = 0.; }
                if s.velocity.abs() < 0.4 { s.velocity = 0.; }
            }
            if s.velocity == 0. && s.settle.is_none() {
                let nearest = ((s.pos / ROW as f32).round() * ROW as f32).clamp(0., max);
                if (nearest - s.pos).abs() > 0.01 { s.settle = Some(nearest); }
                else if s.target.is_none() && (nearest / ROW as f32) as u16 != l.first { s.follow(&l, nearest); }
            }
            if let Some(t) = s.settle {
                s.pos += (t - s.pos) * SETTLE;
                if (t - s.pos).abs() < 0.5 {
                    s.pos = t;
                    s.settle = None;
                    s.follow(&l, t);
                }
            }
        }
        // Catching up.
        if let Some((first, row)) = s.target {
            if (l.first, l.row) == (first, row) {
                s.target = None;
                s.frozen = None;
                self.machine.bus.ffta_battle_mouse.silence_cursor = false;
                if let Some(key) = self.shop_scroll.deferred.take() { self.press_button(key); }
            } else if !s.dragging && !battle::busy(&self.machine.bus) {
                // Inside the shown rows the cursor goes straight there; past them
                // it scrolls from the edge row, a short Up/Down a row.
                let bus = &mut self.machine.bus;
                bus.ffta_battle_mouse.silence_cursor = true;
                let edge = if l.first < first { l.visible - 1 } else { 0 };
                let jump = if l.first == first { Some(row) } else if l.row != edge { Some(edge) } else { None };
                if !jump.is_some_and(|r| shop::select_row(bus, r)) {
                    let key = if l.first < first { DOWN } else if l.first > first { UP } else if l.row < row { DOWN } else { UP };
                    battle::press_quick(bus, key);
                }
            }
        } else if !s.busy() {
            s.pos = l.first as f32 * ROW as f32;
            self.machine.bus.ffta_battle_mouse.silence_cursor = false;
        }
        self.look_ahead(l)
    }
    /// Collects the rows not yet kept, in a silent run from a snapshot.
    fn look_ahead(&mut self, l: shop::ShopList) -> Result<(), String> {
        let s = &self.shop_scroll;
        if s.complete() || l.count <= l.visible as u16 || s.busy() {
            if s.complete() { self.shop_scroll.look = None; }
            return Ok(());
        }
        if s.look.is_none() {
            if battle::busy(&self.machine.bus) { return Ok(()); }
            self.shop_scroll.look = Some(LookAhead { state: savestate::snapshot(&self.machine), frame: 0, key: RELEASED, down: None, wait: 0 });
        }
        let real = savestate::snapshot(&self.machine);
        let maps = self.machine.bus.ffta_dialogue.shop_maps.clone();
        let skip = self.machine.bus.skip_render;
        let mut look = self.shop_scroll.look.take().unwrap();
        savestate::restore(&mut self.machine, &look.state)?;
        let mut done = false;
        for _ in 0..LOOKAHEAD_FRAMES {
            let bus = &self.machine.bus;
            if look.frame == 0 {
                // Each step: a key for 2 frames, none for 2, the rows read on the 4th.
                look.key = RELEASED;
                if look.wait > 0 {
                    look.wait = look.wait.saturating_sub(4);
                } else if gba_core::ppu::shop_info_open(bus) {
                    // Closing item Info takes the game a few frames.
                    if matches!(shop::phase(bus), Some(shop::ShopPhase::List { .. })) { look.key = R; look.wait = 16; }
                } else if let Some(at) = shop::shop_list(bus) {
                    let last = at.first + at.row as u16 + 1 >= at.count;
                    look.down = match look.down {
                        None | Some(true) if !last => Some(true),
                        None | Some(true) => Some(false),
                        down => down,
                    };
                    if look.down == Some(false) && at.first == 0 { done = true; break; }
                    look.key = if look.down == Some(true) { DOWN } else { UP };
                }
            }
            let bus = &mut self.machine.bus;
            bus.keys = if look.frame < 2 { look.key } else { RELEASED };
            bus.skip_render = look.frame != 3;
            self.step_frame()?;
            let bus = &mut self.machine.bus;
            bus.audio_buf.clear();
            if look.frame == 3 && !gba_core::ppu::shop_info_open(bus) {
                if let Some(at) = shop::shop_list(bus).filter(|at| ListKey::of(at) == ListKey::of(&l)) {
                    let canvas = std::mem::take(&mut bus.ffta_dialogue.pixels);
                    self.shop_scroll.capture(&at, &canvas);
                    self.machine.bus.ffta_dialogue.pixels = canvas;
                }
                if self.shop_scroll.complete() { done = true; }
            }
            look.frame = (look.frame + 1) % 4;
            if done { break; }
        }
        if !done { look.state = savestate::snapshot(&self.machine); }
        savestate::restore(&mut self.machine, &real)?;
        let bus = &mut self.machine.bus;
        bus.ffta_dialogue.shop_maps = maps;
        bus.skip_render = skip;
        if !done { self.shop_scroll.look = Some(look); }
        Ok(())
    }
    /// After the frame: keeps live rows, draws the list where it is moved.
    pub(crate) fn shop_scroll_present(&mut self) {
        let list = self.portrait_shop_list();
        // Sliding between categories (the game changes the list meanwhile).
        if self.shop_scroll.slide != Slide::Idle && self.town_buttons() { self.present_slide(list); return; }
        let Some(l) = list else { return };
        let bus = &mut self.machine.bus;
        let info = gba_core::ppu::shop_info_open(bus);
        let s = &mut self.shop_scroll;
        if s.key != Some(ListKey::of(&l)) { return; }
        let live = !info && !s.busy() && s.frozen.is_none();
        if live { s.capture(&l, &bus.ffta_dialogue.pixels); }
        if s.freeze_next {
            s.freeze_next = false;
            s.frozen = Some((bus.ffta_dialogue.pixels.clone(), bus.ffta_dialogue.extra.pixels.clone()));
        }
        if let Some((main, extra)) = &s.frozen {
            bus.ffta_dialogue.pixels.clone_from(main);
            if bus.ffta_dialogue.extra.pixels.len() == extra.len() { bus.ffta_dialogue.extra.pixels.clone_from(extra); }
        }
        if !s.complete() || (live && s.slide == Slide::Idle) { return; }
        let selected = Some(match s.target { Some((first, row)) => (first + row as u16) as usize, None => (l.first + l.row as u16) as usize });
        let canvas = &mut bus.ffta_dialogue.pixels;
        s.draw(canvas, selected, 0);
    }
    fn present_slide(&mut self, list: Option<shop::ShopList>) {
        let s = &mut self.shop_scroll;
        let canvas = &mut self.machine.bus.ffta_dialogue.pixels;
        if s.key.is_none() || s.visible == 0 { return; }
        match s.slide {
            Slide::Drag(d) | Slide::Back(d) => {
                // The rows as shown (kept rows where the list was moved), then shifted.
                if s.complete() && list.is_some_and(|l| Some(ListKey::of(&l)) == s.key) {
                    let selected = list.map(|l| (l.first + l.row as u16) as usize);
                    s.draw(canvas, selected, 0);
                }
                let rows = s.rows_region(canvas);
                s.paint_shifted(canvas, &rows, d.round() as i32);
                s.leaving = Some(rows);
            }
            Slide::Out { offset, .. } => {
                let rows = s.leaving.clone().unwrap_or_default();
                s.paint_shifted(canvas, &rows, offset.round() as i32);
            }
            Slide::In { offset } => {
                let rows = s.rows_region(canvas);
                s.paint_shifted(canvas, &rows, offset.round() as i32);
            }
            Slide::Idle => {}
        }
    }
    fn slide_tick(&mut self, list: Option<shop::ShopList>) {
        let s = &mut self.shop_scroll;
        let width = s.right.max(1) as f32;
        s.slide = match s.slide {
            Slide::Back(d) => { let d = d * 0.6; if d.abs() < 0.5 { Slide::Idle } else { Slide::Back(d) } }
            Slide::Out { dir, offset, from } => {
                let offset = (offset + dir * SLIDE_SPEED).clamp(-width, width);
                // The new list is there once the game shows another category, settled.
                match list {
                    Some(l) if (l.category, l.sell) != from && offset.abs() >= width => Slide::In { offset: -dir * width },
                    _ => Slide::Out { dir, offset, from },
                }
            }
            Slide::In { offset } => {
                let next = offset - offset.signum() * SLIDE_SPEED;
                if next.signum() != offset.signum() || next.abs() < 0.5 { Slide::Idle } else { Slide::In { offset: next } }
            }
            other => other,
        };
    }
    /// The neighbouring category towards a swipe (`dx` < 0: the next one).
    pub(crate) fn swipe_category(&self, dx: f32) -> Option<u8> {
        let (current, count) = shop::categories(&self.machine.bus)?;
        if dx < 0. { (current + 1 < count).then_some(current + 1) } else { current.checked_sub(1) }
    }
    /// A sideways swipe ends: change category when it went far or fast enough.
    pub(crate) fn swipe_end(&mut self, dx: f32, speed: f32) {
        let width = self.shop_scroll.right.max(1) as f32;
        let far = dx.abs() > width * 0.25 || speed.abs() > 6.;
        match (far, self.swipe_category(if far { dx + speed } else { dx })) {
            (true, Some(slot)) => {
                let Some(l) = self.portrait_shop_list() else { self.shop_scroll.slide = Slide::Back(dx); return };
                self.mouse.cancel();
                if shop::select_category(&mut self.machine.bus, slot) {
                    self.shop_scroll.slide = Slide::Out { dir: dx.signum(), offset: dx, from: (l.category, l.sell) };
                } else { self.shop_scroll.slide = Slide::Back(dx); }
            }
            _ => self.shop_scroll.slide = Slide::Back(dx),
        }
    }
    /// A tap on the list's rows (native point): the shown row's item. The game
    /// selects it (straight away when it shows that row); a tap on the selected
    /// item chooses it.
    pub(crate) fn shop_rows_tap(&mut self, x: f32, y: f32, nx: f32, ny: f32) {
        let Some(l) = self.portrait_shop_list() else { self.tap(x, y, false); return };
        let s = &mut self.shop_scroll;
        let item = ((s.pos + ny - s.top as f32) / ROW as f32).floor();
        if item < 0. || item >= l.count as f32 || nx >= s.right as f32 { return; }
        let item = item as u16;
        let first = (s.pos / ROW as f32).round() as u16;
        if s.target.is_none() && first == l.first {
            // The game shows these rows: the selected item is chosen, another
            // selected straight away (the game's own Up/Down, entered with its row).
            let row = (item - l.first) as u8;
            // Choosing takes a tap well inside the selected row (not its edges,
            // where a tap for the category tabs or a neighbour can land).
            let within = ny - (s.top + row as i32 * ROW) as f32;
            if row == l.row && !(4. ..13.).contains(&within) { return; }
            self.mouse.cancel();
            let bus = &mut self.machine.bus;
            if row == l.row { battle::press(bus, 0x3fe); }
            else if !shop::select_row(bus, row) { battle::menu_select(bus, row, false); }
            return;
        }
        let row = item.saturating_sub(first).min(l.visible as u16 - 1) as u8;
        s.target = Some((first, row));
        s.freeze_next = s.frozen.is_none();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rows_are_drawn_at_any_offset_and_settle_on_whole_rows() {
        let mut s = ShopScroll { key: Some(ListKey { sell: false, category: 0, list: 0, count: 6 }), top: 20, visible: 4, right: 200, ..Default::default() };
        s.rows = (0..6).map(|i| Some(vec![UiPixel::opaque(i as u16 + 1); ROW as usize * LAYER_WIDTH])).collect();
        s.selected_rows = vec![None; 6];
        assert!(s.complete());
        assert_eq!(s.max_pos(), 32.);
        s.pos = 8.;
        let mut canvas = vec![UiPixel::default(); LAYER_WIDTH * 160];
        s.draw(&mut canvas, None, 0);
        let at = |c: &[UiPixel], x: i32, y: i32| c[y as usize * LAYER_WIDTH + (x + LAYER_INSET) as usize].color;
        // 8 px down: the first line shows row 0's 9th line, the 9th line row 1.
        assert_eq!((at(&canvas, 10, 20), at(&canvas, 10, 28)), (1, 2));
        assert_eq!(at(&canvas, 10, 20 + 63), 5);
        // Past the right edge nothing is drawn.
        assert_eq!(canvas[20 * LAYER_WIDTH + (200 + LAYER_INSET) as usize], UiPixel::default());
        // Rubber band past the ends.
        s.drag_to(-20.);
        assert!(s.pos > -20. && s.pos < 0.);
        s.drag_to(50.);
        assert!(s.pos > 32. && s.pos < 50.);
        // Sideways: shifted rows inside the window, its fill where they left.
        s.pos = 0.;
        s.window = Some(((0, 200), vec![UiPixel::opaque(99); ROW as usize]));
        let mut canvas = vec![UiPixel::default(); LAYER_WIDTH * 160];
        s.draw(&mut canvas, None, 30);
        assert_eq!((at(&canvas, 40, 20), at(&canvas, 10, 20)), (1, 99));
    }
}
