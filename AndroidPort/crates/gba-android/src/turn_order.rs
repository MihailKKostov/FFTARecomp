//! The turn order over the field (touch-only portrait): who acts next, in a
//! card in the sky while the player chooses. The game shows it only in
//! System, Turn Order (a list, "LIST": a number and a name a row, the unit
//! whose turn it is first, enemies' names red). It is read in a silent run
//! from a snapshot (as Action's entries are, battle_windows.rs) when a unit's
//! commands are shown: B for the free cursor, START for System, its first
//! entry (Turn Order), then the list once it is still. The rows after the
//! first are drawn again from its tiles, in the list's own colours (kept
//! with it: enemies' red is loaded only while the list is shown).
use crate::battle_ui::{window, Window};
use crate::battle_windows::{capture, paste, Image};
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, savestate};

const A: u16 = 0x3fe;
const B: u16 = 0x3fd;
const START: u16 = 0x3f7;
const UP: u16 = 0x3bf;
const RELEASED: u16 = 0x3ff;
const FRAMES_PER_TICK: u32 = 8;
const RUN_FRAMES: u32 = 240;
/// Units shown after the one whose turn it is.
const SHOWN: usize = 4;
/// Columns of a row left out: its bullet.
const BULLET: usize = 8;

struct Run { state: Vec<u8>, frame: u32, keys: Vec<u16>, hold: u32, stage: u8, last: Option<(Image, Vec<u8>)>, still: u32 }

#[derive(Default)]
pub(crate) struct Order {
    /// The unit's cell it was read for, the list's rows, and a run under way.
    key: Option<Vec<u8>>,
    rows: Option<(Image, Vec<u8>)>,
    run: Option<Run>,
}

impl Engine {
    /// Once a frame (after Action's entries are read): reads the turn order
    /// for the unit whose commands are shown.
    pub(crate) fn order_tick(&mut self) -> Result<(), String> {
        // Read anew each turn: forgotten while the battle runs between turns.
        if self.battle_runs() || !battle::field_shown(&self.machine.bus) { self.order = Order::default(); }
        let bus = &self.machine.bus;
        let wanted = self.touch_enabled && bus.ffta_dialogue.docked && self.usa;
        if !wanted || !window(bus).is_some_and(|w| w.0 == Window::Commands) { self.order.run = None; return Ok(()); }
        let key = bus.ewram[0xf3b8..0xf3c4].to_vec();
        if self.order.run.is_none() {
            if self.order.key.as_ref() == Some(&key) || !self.flat_idle() || !battle::idle(bus) || self.gesture.is_some() { return Ok(()); }
            self.order.key = Some(key);
            self.order.run = Some(Run { state: savestate::snapshot(&self.machine), frame: 0, keys: vec![B], hold: 0, stage: 0, last: None, still: 0 });
        }
        let real = savestate::snapshot(&self.machine);
        let skip = self.machine.bus.skip_render;
        let mut run = self.order.run.take().unwrap();
        savestate::restore(&mut self.machine, &run.state)?;
        let mut end = None;
        for _ in 0..FRAMES_PER_TICK {
            let bus = &mut self.machine.bus;
            bus.keys = match run.keys.first() { Some(&k) if run.hold < 2 => k, _ => RELEASED };
            if !run.keys.is_empty() { run.hold += 1; if run.hold >= 5 { run.keys.remove(0); run.hold = 0; } }
            bus.skip_render = true;
            self.step_frame()?;
            let bus = &mut self.machine.bus;
            bus.audio_buf.clear();
            run.frame += 1;
            if run.frame > RUN_FRAMES { end = Some(None); break; }
            if !run.keys.is_empty() { continue; }
            match run.stage {
                // The free cursor: START.
                0 => if battle::Scene::read(bus).is_some_and(|s| s.cells.iter().all(|c| c.range == 0)) { run.keys = vec![START]; run.stage = 1; },
                // System: its first entry.
                1 => if let Some((Window::System, scene)) = window(bus) {
                    run.keys = vec![UP; scene.selected as usize];
                    run.keys.push(A);
                    run.stage = 2;
                },
                // The list, once still.
                _ => {
                    let Some((Window::Order, scene)) = window(bus) else { run.still = 0; continue };
                    let Some(image) = capture(bus, &scene) else { continue };
                    if run.last.as_ref().is_some_and(|l| l.0 == image) { run.still += 1; } else { run.still = 0; run.last = Some((image, bus.palette[..0x200].to_vec())); }
                    if run.still >= 3 { end = Some(run.last.take()); break; }
                }
            }
        }
        if end.is_none() { run.state = savestate::snapshot(&self.machine); }
        savestate::restore(&mut self.machine, &real)?;
        self.machine.bus.skip_render = skip;
        match end {
            Some(rows) => self.order.rows = rows,
            None => self.order.run = Some(run),
        }
        Ok(())
    }
    /// The card, top left under the laws, while the player chooses (the
    /// commands, the field's cursor, the facing).
    pub(crate) fn order_present(&mut self, dock: (i32, i32)) {
        let bus = &self.machine.bus;
        if !(self.touch_enabled && bus.ffta_dialogue.docked && self.usa) { return; }
        let choosing = window(bus).is_some_and(|w| w.0 == Window::Commands) || battle::Scene::read(bus).is_some() || battle::facing_center(bus).is_some();
        if !choosing || crate::deploy_touch::shown(bus) { return; }
        let Some((rows, palette)) = self.order.rows.as_ref() else { return };
        let next: Vec<&Vec<u8>> = rows.entries.iter().skip(1).take(SHOWN).collect();
        if next.is_empty() { return; }
        let (rw, rh) = (gba_core::mem::render_width(), gba_core::mem::render_height());
        let width = rows.width - BULLET;
        // Each row's pixels without the bullet, and the right part with nothing drawn.
        let cut: Vec<Vec<u8>> = next.iter().map(|e| e.chunks_exact(rows.width).flat_map(|l| l[BULLET..].iter().copied()).collect()).collect();
        let used = cut.iter().flat_map(|e| e.chunks_exact(width).map(|l| l.iter().rposition(|&c| c != 0).map_or(0, |p| p + 1))).max().unwrap_or(0);
        let title = "Next";
        let w = (used as i32 + 12).max(crate::game_buttons::text_width_scaled(&bus.rom, title, 1) + 16);
        let body = 16 + 16 * cut.len() as i32;
        let top = self.laws.banner.map_or(dock.0 + 8, |b| b.1 + 6);
        let x0 = gba_core::mem::extra() as i32 + 4;
        crate::game_buttons::draw_card(bus, &mut self.pixels, rw, rh, x0, top, w, body, &[(8, 0, title, false, 1)]);
        for (k, row) in cut.iter().enumerate() {
            let line: Vec<u8> = row.chunks_exact(width).flat_map(|l| l[..used].iter().copied()).collect();
            paste(bus, palette, &mut self.pixels, rw, rh, x0 + 6, top + 5 + 16 + 16 * k as i32, used, &line);
        }
    }
}
