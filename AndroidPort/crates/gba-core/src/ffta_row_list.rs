//! Party screen item and ability lists (USA): each has one routine that moves
//! its cursor to a visible row (r0 the list, r2 the row), called by its Up and
//! Down handling with the row next to the current one.
//!
//! The lists are the game's text lists: list i at [0x03002818] + 204i +
//! 0x2d00 (+0x16 visible rows, +0x18 entries, +0x1a first shown, -0x48 its
//! window), its cursor at +0x20 (+4 the list, +0xa first row's y in the
//! window, +0x11 the selected visible row). Window w (IWRAM 0x9e0 + 0x4c w;
//! bit 1 shown, +0x34 x and +0x35 y in tiles, +0x36 width, +0x49 title rows)
//! places it on the screen. Some show no pointer, only the highlighted row.
//!
//! A tap jumps straight to a row: one Up or Down press wakes the handling, and
//! the routine is entered with the tapped row instead of the next one. Its
//! redraw, cursor, scroll and stats are the game's own for that row.
use crate::{ffta_rom::Version, Machine, mem::MemMap};

/// Items (EQUIP), abilities (ABILITIES).
const SETTERS: [u32; 2] = [0x08082254, 0x080845b4];

#[derive(Clone, Copy)]
struct Request { step: i32, delta: i32, deadline: u64, committed: bool }

#[derive(Default)]
pub(crate) struct State { usable: bool, request: Option<Request> }

impl State {
    pub(crate) fn new(rom: &[u8], version: Version) -> Self {
        // push {r4-r7,lr}; mov r7,r10; mov r6,r9; mov r5,r8; push {r5-r7}
        let usable = version == Version::Usa && SETTERS.iter().all(|&a| {
            let o = (a - 0x08000000) as usize;
            rom.get(o..o + 10) == Some(&[0xf0, 0xb5, 0x57, 0x46, 0x4e, 0x46, 0x45, 0x46, 0xe0, 0xb4])
        });
        Self { usable, request: None }
    }
    pub(crate) fn interprets(&self, pc: u32) -> bool {
        self.usable && self.request.is_some_and(|r| !r.committed) && SETTERS.contains(&pc)
    }
    pub(crate) fn cancel(&mut self) { self.request = None; }
}

const MANAGER: usize = 0x2818;
const LISTS: usize = 0x2d00;
const LIST_SIZE: usize = 204;

fn half(b: &[u8], o: usize) -> Option<u16> { Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?)) }
fn word(b: &[u8], o: usize) -> Option<u32> { Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?)) }

/// A shown text list on the screen (native coordinates, rows 16 apart).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowList { pub list: usize, pub origin: [i32; 2], pub right: i32, pub rows: u8, pub row: u8 }

impl RowList {
    /// The shown lists of the party screens' list manager.
    pub fn all(mem: &MemMap) -> Vec<Self> {
        let s = &mem.ffta_battle_mouse;
        if !s.rows.usable || !s.enabled { return Vec::new(); }
        let Some(manager) = word(&mem.iwram, MANAGER).and_then(|p| p.checked_sub(0x02000000)) else { return Vec::new() };
        (0..4).filter_map(|i| {
            let list = manager as usize + LISTS + i * LIST_SIZE;
            let cursor = list + 0x20;
            if word(&mem.ewram, cursor + 4)? != 0x02000000 + list as u32 { return None; }
            let window = *mem.ewram.get(list.checked_sub(0x48)?)? as usize;
            let win = 0x9e0 + window * 0x4c;
            if window >= 8 || mem.iwram.get(win)? & 2 == 0 { return None; }
            let visible = *mem.ewram.get(list + 0x16)?;
            let entries = half(&mem.ewram, list + 0x18)?.saturating_sub(half(&mem.ewram, list + 0x1a)?);
            let rows = (visible as u16).min(entries) as u8;
            let row = *mem.ewram.get(cursor + 0x11)?;
            let x = mem.iwram[win + 0x34] as i8 as i32 * 8;
            let y = (mem.iwram[win + 0x35] as i8 as i32 + mem.iwram[win + 0x49] as i8 as i32 - 1) * 8
                + half(&mem.ewram, cursor + 0xa)? as i16 as i32 - 4;
            let right = x + mem.iwram[win + 0x36] as i32 * 8;
            (rows > 0 && row < rows && x >= 0 && right <= 240 && x < right && y >= 0 && y + rows as i32 * 16 <= 160)
                .then_some(Self { list, origin: [x, y], right, rows, row })
        }).collect()
    }
    /// The shown list and its row holding native point (x, y).
    pub fn at(mem: &MemMap, x: f32, y: f32) -> Option<(Self, u8)> {
        Self::all(mem).into_iter().find_map(|l| {
            let row = ((y - l.origin[1] as f32) / 16.).floor();
            (x >= l.origin[0] as f32 && x < l.right as f32 && row >= 0. && row < l.rows as f32).then_some((l, row as u8))
        })
    }
    /// The same list, read again.
    pub fn current(&self, mem: &MemMap) -> Option<Self> { Self::all(mem).into_iter().find(|l| l.list == self.list) }
}

/// Moves the list cursor `delta` visible rows (to a row known to hold an
/// entry) with one press. False when the routines are not this game's.
pub fn select(mem: &mut MemMap, delta: i32) -> bool {
    if !mem.ffta_battle_mouse.rows.usable || delta == 0 { return false; }
    let step = delta.signum();
    if !crate::ffta_battle_mouse::press(mem, if step > 0 { 0x37f } else { 0x3bf }) { return false; }
    mem.ffta_battle_mouse.rows.request = Some(Request { step, delta, deadline: mem.frames + 30, committed: false });
    true
}

/// A jump is waiting for the list's routine.
pub fn pending(mem: &MemMap) -> bool {
    mem.ffta_battle_mouse.rows.request.is_some_and(|r| !r.committed && mem.frames <= r.deadline)
}

impl Machine {
    pub(crate) fn ffta_row_list_entry(&mut self) {
        let pc = self.cpu.regs[15];
        let s = &mut self.bus.ffta_battle_mouse.rows;
        if !s.usable || !SETTERS.contains(&pc) { return; }
        let Some(r) = s.request.filter(|r| !r.committed) else { return };
        if self.bus.frames > r.deadline { s.request = None; return; }
        // r2 is the row next to the current one in the pressed direction.
        let row = (self.cpu.regs[2] & 255) as i32 - r.step + r.delta;
        if !(0..=255).contains(&row) { s.request = None; return; }
        self.cpu.regs[2] = row as u32;
        s.request = Some(Request { committed: true, ..r });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enters_the_row_routine_with_the_tapped_row() {
        let mut m = Machine::new(vec![0; 0x200]);
        assert!(!select(&mut m.bus, 2));
        m.bus.ffta_battle_mouse.rows.usable = true;
        m.bus.ffta_battle_mouse.enabled = true;
        assert!(select(&mut m.bus, 3));
        assert!(pending(&m.bus) && m.bus.ffta_battle_mouse.rows.interprets(SETTERS[0]));
        // Down from row 1: the routine is asked for row 2; row 4 is entered.
        m.cpu.regs[15] = SETTERS[1];
        m.cpu.regs[2] = 2;
        m.ffta_row_list_entry();
        assert_eq!(m.cpu.regs[2], 4);
        assert!(!pending(&m.bus));
        // Up two rows from row 4: asked for 3, entered with 2.
        assert!(select(&mut m.bus, -2));
        m.cpu.regs[2] = 3;
        m.ffta_row_list_entry();
        assert_eq!(m.cpu.regs[2], 2);
    }
}
