//! The game's cursor graphs (USA): menus whose entries name their Left, Right,
//! Up and Down neighbours, moved by one routine (0x08088fec, called every
//! frame with the menu and the pressed keys). Party screens use them for the
//! unit's MENU, the EQUIP slots and more; the title menu too.
//!
//! Menu layout (EWRAM): flags at +0 (bit 1: Left/Right wrap, bit 2: Up/Down
//! wrap), entry i at +4+8i (+1 cursor x, +2 cursor y, +4..+7 Left, Right, Up,
//! Down neighbour, bit 7 marking a wrap), entry count at +0x104, the selected
//! entry at +0x105.
//!
//! A tap jumps straight to an entry: one direction press wakes the routine,
//! and where it stores the neighbour it moved to, the tapped entry is stored
//! instead. Everything after that (sound, highlight, cursor, stats) is the
//! game's own handling of that entry.
use crate::{ffta_rom::Version, mem::MemMap, Machine};

const ROUTINE: u32 = 0x08088fec;
const END: u32 = 0x080891b4;
/// Where the routine stores the new selection (Down, Right, Up/Left): r0 the
/// entry, r5 the neighbour byte it is read from, r6 the selection's address.
const STORES: [u32; 3] = [0x08089074, 0x08089104, 0x08089168];
const COUNT: usize = 0x104;
const SELECTED: usize = 0x105;
const MAX_ENTRIES: u8 = 32;

#[derive(Clone, Copy)]
struct Request { base: usize, target: u8, deadline: u64, committed: Option<u64> }

#[derive(Default)]
pub(crate) struct State {
    usable: bool,
    /// The menu the routine last ran for, and the frame.
    last: Option<(usize, u64)>,
    request: Option<Request>,
}

impl State {
    pub(crate) fn new(rom: &[u8], version: Version) -> Self {
        let at = |a: u32, bytes: &[u8]| rom.get((a - 0x08000000) as usize..(a - 0x08000000) as usize + bytes.len()) == Some(bytes);
        // push {r4-r6,lr}; adds r4,r0,#0 / movs r0,#127; ands r0,r5; strb r0,[r6]; lsls r1,r5,#3
        let usable = version == Version::Usa && at(ROUTINE, &[0x70, 0xb5, 0x04, 0x1c])
            && STORES.iter().all(|&s| at(s - 4, &[0x7f, 0x20, 0x28, 0x40, 0x30, 0x70, 0xe9, 0x00]));
        Self { usable, ..Self::default() }
    }
    pub(crate) fn interprets(&self, pc: u32) -> bool {
        self.usable && (pc == ROUTINE || (self.request.is_some() && (ROUTINE..END).contains(&pc)))
    }
    pub(crate) fn cancel(&mut self) { self.request = None; }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry { pub x: i32, pub y: i32, neighbours: [u8; 4] }

#[derive(Clone, Copy, Debug)]
pub struct GraphMenu { base: usize, flags: u8, pub count: u8, pub selected: u8 }

impl GraphMenu {
    /// The menu the game is moving this frame (its routine ran this or the previous frame).
    pub fn read(mem: &MemMap) -> Option<Self> {
        let s = &mem.ffta_battle_mouse.graph;
        let (base, frame) = s.last.filter(|_| s.usable && mem.ffta_battle_mouse.enabled)?;
        if frame + 1 < mem.frames { return None; }
        let count = *mem.ewram.get(base + COUNT)?;
        let selected = *mem.ewram.get(base + SELECTED)?;
        if count == 0 || count > MAX_ENTRIES || selected >= count { return None; }
        Some(Self { base, flags: mem.ewram[base], count, selected })
    }
    pub fn entry(&self, mem: &MemMap, i: u8) -> Entry {
        let e = &mem.ewram[self.base + 4 + 8 * i as usize..][..8];
        Entry { x: e[1] as i32, y: e[2] as i32, neighbours: [e[4], e[5], e[6], e[7]] }
    }
    /// The entry whose row (cursor row -4..+12, from 8 left of the cursor)
    /// holds native point (x, y); with several, the one nearest on the left.
    pub fn hit(&self, mem: &MemMap, x: f32, y: f32) -> Option<u8> {
        (0..self.count).filter(|&i| {
            let e = self.entry(mem, i);
            y >= (e.y - 4) as f32 && y < (e.y + 12) as f32 && x >= (e.x - 8) as f32 && x < (e.x + 128) as f32
        }).max_by_key(|&i| self.entry(mem, i).x)
    }
    /// A direction key (KEYINPUT mask) the routine moves on from the selected
    /// entry, preferring the target's direction.
    fn key_towards(&self, mem: &MemMap, target: u8) -> Option<u16> {
        // Left, Right, Up, Down: key masks and the wrap flag each needs.
        const KEYS: [(u16, u8); 4] = [(0x3df, 2), (0x3ef, 2), (0x3bf, 4), (0x37f, 4)];
        let order: [usize; 4] = if target > self.selected { [3, 1, 2, 0] } else { [2, 0, 3, 1] };
        let n = self.entry(mem, self.selected).neighbours;
        order.into_iter().find(|&d| {
            let to = n[d] & 0x7f;
            to < self.count && to != self.selected && (n[d] & 0x80 == 0 || self.flags & KEYS[d].1 != 0)
        }).map(|d| KEYS[d].0)
    }
}

/// Moves the cursor of the graph menu being shown straight to `target` (one
/// press). False when there is no such menu or no way to wake it.
pub fn select(mem: &mut MemMap, target: u8) -> bool {
    let Some(menu) = GraphMenu::read(mem) else { return false };
    if target >= menu.count { return false; }
    if target == menu.selected { return true; }
    let Some(key) = menu.key_towards(mem, target) else { return false };
    if !crate::ffta_battle_mouse::press(mem, key) { return false; }
    mem.ffta_battle_mouse.graph.request = Some(Request { base: menu.base, target, deadline: mem.frames + 30, committed: None });
    true
}

/// A jump is waiting for the routine.
pub fn pending(mem: &MemMap) -> bool {
    let s = &mem.ffta_battle_mouse.graph;
    s.request.is_some_and(|r| r.committed.is_none() && mem.frames <= r.deadline)
}

impl Machine {
    pub(crate) fn ffta_graph_menu_entry(&mut self) {
        let pc = self.cpu.regs[15];
        let s = &mut self.bus.ffta_battle_mouse.graph;
        if !s.usable { return; }
        if pc == ROUTINE {
            let base = self.cpu.regs[0].wrapping_sub(0x02000000) as usize;
            if base + SELECTED < self.bus.ewram.len() { s.last = Some((base, self.bus.frames)); }
            if s.request.is_some_and(|r| self.bus.frames > r.deadline || r.committed.is_some()) { s.request = None; }
            return;
        }
        if !STORES.contains(&pc) { return; }
        let Some(r) = s.request.filter(|r| r.committed.is_none()) else { return };
        if self.cpu.regs[6] != 0x02000000 + (r.base + SELECTED) as u32 { return; }
        self.cpu.regs[0] = r.target as u32;
        self.cpu.regs[5] = r.target as u32;
        s.request = Some(Request { committed: Some(self.bus.frames), ..r });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jumps_by_rewriting_the_stored_neighbour() {
        let mut m = Machine::new(vec![0; 0x200]);
        // No USA routine in this ROM: nothing is interpreted or read.
        assert!(!m.bus.ffta_battle_mouse.graph.interprets(ROUTINE));
        let base = 0xf814;
        // The party unit MENU as measured: four entries, 16 rows apart, Up/Down wrapping.
        m.bus.ewram[base] = 5;
        for i in 0..4u8 {
            let e = &mut m.bus.ewram[base + 4 + 8 * i as usize..][..8];
            e.copy_from_slice(&[i, 0x8f, 0x5d + 16 * i, 0, 0x80 | i, 0x80 | i, if i == 0 { 0x83 } else { i - 1 }, if i == 3 { 0x80 } else { i + 1 }]);
        }
        m.bus.ewram[base + COUNT] = 4;
        m.bus.ffta_battle_mouse.graph.usable = true;
        m.bus.ffta_battle_mouse.enabled = true;
        m.bus.ffta_battle_mouse.graph.last = Some((base, 0));
        let menu = GraphMenu::read(&m.bus).unwrap();
        assert_eq!((menu.count, menu.selected), (4, 0));
        assert_eq!(menu.hit(&m.bus, 160., 0x5d as f32 + 32.), Some(2));
        assert_eq!(menu.hit(&m.bus, 100., 0x5d as f32), None);
        assert_eq!(menu.key_towards(&m.bus, 2), Some(0x37f));
        assert!(select(&mut m.bus, 2));
        assert!(pending(&m.bus));
        // The routine stores Down's neighbour (1): the target is stored instead.
        m.cpu.regs[15] = STORES[0];
        m.cpu.regs[0] = 1; m.cpu.regs[5] = 1; m.cpu.regs[6] = 0x02000000 + (base + SELECTED) as u32;
        m.ffta_graph_menu_entry();
        assert_eq!((m.cpu.regs[0], m.cpu.regs[5]), (2, 2));
        assert!(!pending(&m.bus));
        // Another menu's store is left alone.
        m.bus.ffta_battle_mouse.graph.request = Some(Request { base, target: 3, deadline: 30, committed: None });
        m.cpu.regs[0] = 1; m.cpu.regs[6] = 0x02000000;
        m.ffta_graph_menu_entry();
        assert_eq!(m.cpu.regs[0], 1);
    }
}
