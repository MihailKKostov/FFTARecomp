//! Host-only portrait layout of the Party screens (USA measurements).
//!
//! The roster and the unit screens (status, Equip, Pick Abilities, Change
//! Jobs) draw their panels over BG0, a 256x512 checkered pattern repeating
//! every 32 columns and 16 rows. In portrait the PPU fills the whole tall view
//! with that pattern and composes the rest of the screen as two groups over
//! it: the upper group (unit header, or roster bar and unit grid) and the
//! lower group (stats, menus and lists, or the roster's unit panel). Each group
//! is the difference between its composition and the background alone, so
//! translucent panels keep their blending; the frontend moves the groups
//! apart by whole pattern periods, where the background underneath is the same.
use crate::mem::MemMap;

pub const TRANSPARENT: u16 = 0xFFFF;
/// Vertical period of the BG0 pattern: groups only move by multiples of it.
pub const PERIOD: i32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Template {
    /// Unit grid and bar above row 100, the selected unit's panel below.
    #[default]
    Roster,
    /// Header (portrait, name, HP) above the separator ending at row 61.
    Unit,
    /// No header (the roster's Item List): one group, kept together.
    Whole,
}
impl Template {
    pub fn cut(self) -> i32 { match self { Template::Roster => 100, Template::Unit => 61, Template::Whole => -1000 } }
}

#[derive(Default)]
pub struct MenuScreen {
    /// Requested by the frontend (portrait docking frame only).
    pub enabled: bool,
    /// The current frame is a Party screen captured in two groups.
    pub frame: bool,
    frame_before: bool,
    pub template: Template,
    /// Three 240x160 RGB555 pictures (upper group, lower group, roster
    /// selection ellipse), TRANSPARENT where empty. The ellipse is semi-
    /// transparent: its picture holds the raw sprite colours (no background),
    /// so the frontend can blend it under any unit (BLDALPHA).
    pub groups: Vec<u16>,
    /// BG1-3 pixels drawn in native rows 8-99 this frame (on the roster: its
    /// "No." labels, which the game erases first when it leaves the roster).
    pub label_pixels: u32,
    /// Group of each guest OAM slot (0 upper, 1 lower, 2 ellipse) for this frame.
    pub(crate) slot_group: Vec<u8>,
    /// Sprite group being rendered (None: all sprites).
    pub(crate) rendering: Option<u8>,
}

fn half(data: &[u8], offset: usize) -> u16 { u16::from_le_bytes([data[offset], data[offset + 1]]) }

/// Measured Party screen BG control (roster: BG2 priority 1, unit screens: 2).
pub fn party_screen(mem: &MemMap) -> bool {
    half(&mem.io, 0) & 0x0f07 == 0x0f00
        && half(&mem.io, 8) == 0xae0b && half(&mem.io, 10) == 0xac0a
        && matches!(half(&mem.io, 12), 0x2b09 | 0x2b0a) && half(&mem.io, 14) == 0x2a08
}

/// Unit screens give BG2 priority 2 from their first frame (the roster 1);
/// their header separator is only redrawn into place a few frames later.
fn unit_screen(mem: &MemMap) -> bool { half(&mem.io, 12) == 0x2b0a }

/// The Item List shares the unit screens' BG control but has no header: its
/// list title bar is on BG3 tile rows 0-1, which unit screens leave empty.
fn headerless(mem: &MemMap) -> bool {
    let base = ((half(&mem.io, 14) >> 8) & 31) as usize * 0x800;
    (0..2 * 32).any(|i| mem.vram.get(base + i * 2..base + i * 2 + 2).is_some_and(|e| e != [0, 0]))
}

/// A unit header's separator (30 tiles 0xc0a8 on one BG1 row) is on screen;
/// while it slides in it is redrawn on tile rows 2, 5 and 7.
fn unit_header(mem: &MemMap) -> bool {
    let base = ((half(&mem.io, 10) >> 8) & 31) as usize * 0x800;
    (0..8).any(|row| (0..30).all(|x| {
        let o = base + (row * 32 + x) * 2;
        mem.vram.get(o..o + 2) == Some(&[0xa8, 0xc0][..])
    }))
}

const OBJ_SIZES: [[(i32, i32); 4]; 3] = [
    [(8, 8), (16, 16), (32, 32), (64, 64)],
    [(16, 8), (32, 8), (32, 16), (64, 32)],
    [(8, 16), (8, 32), (16, 32), (32, 64)],
];

/// Native box of an OAM entry, None when hidden.
pub(crate) fn object_box(a0: u16, a1: u16) -> Option<[i32; 4]> {
    let affine = a0 & 0x100 != 0;
    if !affine && a0 & 0x200 != 0 { return None; }
    let shape = (a0 >> 14) as usize;
    if shape == 3 { return None; }
    let (mut w, mut h) = OBJ_SIZES[shape][(a1 >> 14) as usize];
    if affine && a0 & 0x200 != 0 { w *= 2; h *= 2; }
    let mut y = (a0 & 255) as i32;
    if y >= 160 { y -= 256; }
    let mut x = (a1 & 511) as i32;
    if x >= 240 { x -= 512; }
    (x + w > 0 && y + h > 0 && x < 240 && y < 160).then_some([x, y, x + w, y + h])
}

/// Objects of the same priority whose boxes overlap or touch form one picture (a
/// portrait, a unit with its shadow); each picture goes to the group of its
/// centre row, so no picture is ever split. On the roster the unit panel's
/// portrait (priority 1, parts not always touching) is always in the lower
/// group and the semi-transparent selection ellipse is a group of its own.
pub fn sprite_groups(oam: &[u8], template: Template) -> Vec<u8> {
    let cut = template.cut();
    let roster = template == Template::Roster;
    let entries: Vec<Option<([i32; 4], u16)>> = (0..128).map(|i| {
        let (a0, a1, a2) = (half(oam, i * 8), half(oam, i * 8 + 2), half(oam, i * 8 + 4));
        object_box(a0, a1).map(|b| (b, (a2 >> 10) & 3))
    }).collect();
    let mut parent: Vec<usize> = (0..128).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i { parent[i] = parent[parent[i]]; i = parent[i]; }
        i
    }
    for i in 0..128 {
        let Some((a, pa)) = entries[i] else { continue };
        for j in i + 1..128 {
            let Some((b, pb)) = entries[j] else { continue };
            if pa == pb && a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3] {
                let (ri, rj) = (root(&mut parent, i), root(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut span = vec![(i32::MAX, i32::MIN); 128];
    for i in 0..128 {
        if let Some((b, _)) = entries[i] {
            let r = root(&mut parent, i);
            span[r] = (span[r].0.min(b[1]), span[r].1.max(b[3]));
        }
    }
    (0..128).map(|i| {
        let a0 = half(oam, i * 8);
        if roster && entries[i].is_some() {
            if (a0 >> 10) & 3 == 1 { return 2; }
            if entries[i].is_some_and(|(_, p)| p == 1) { return 1; }
        }
        let (top, bottom) = span[root(&mut parent, i)];
        u8::from(entries[i].is_some() && top + bottom >= 2 * cut)
    }).collect()
}

/// Line 0: decide whether this frame is captured and assign sprites to groups.
pub fn begin_frame(mem: &mut MemMap) {
    let frame = mem.menu_screen.enabled && crate::mem::extra_y() > 0 && party_screen(mem);
    mem.menu_screen.frame_before = mem.menu_screen.frame;
    mem.menu_screen.frame = frame;
    mem.menu_screen.rendering = None;
    mem.menu_screen.label_pixels = 0;
    if !frame { return; }
    let previous = if mem.menu_screen.frame_before { mem.menu_screen.template } else { Template::Roster };
    let template = match (unit_screen(mem), headerless(mem)) {
        (true, true) => Template::Whole,
        // The Item List erases its title bar first when it closes: stay whole
        // until a unit header appears, so its icons do not fly to the top.
        (true, false) if previous == Template::Whole && !unit_header(mem) => Template::Whole,
        (true, false) => Template::Unit,
        _ => Template::Roster,
    };
    mem.menu_screen.template = template;
    mem.menu_screen.slot_group = sprite_groups(&mem.oam, template);
    mem.menu_screen.groups.clear();
    mem.menu_screen.groups.resize(3 * 240 * 160, TRANSPARENT);
}

/// Whether BG `bg` belongs to group `group` on native row `line`: BG1 (and
/// on the roster every panel plane) splits at the cut; on unit screens the
/// BG2/BG3 menu planes always belong to the lower group, so Equip and
/// ability panels overlapping the header separator move with their lists.
pub(crate) fn bg_in_group(template: Template, bg: usize, line: usize, group: u8) -> bool {
    if group == 2 { return false; }
    let lower = match (template, bg) {
        (_, 0) => return true,
        (Template::Whole, _) | (Template::Unit, 2 | 3) => true,
        _ => line as i32 >= template.cut(),
    };
    u8::from(lower) == group
}

/// Group of a sprite while a group is being rendered: guest slots use the
/// frame's assignment, other (host) copies the same rules by themselves.
pub(crate) fn sprite_visible(mem: &MemMap, slot: Option<usize>, a0: u16, a1: u16, a2: u16) -> bool {
    let Some(group) = mem.menu_screen.rendering else { return true };
    let template = mem.menu_screen.template;
    let assigned = match slot {
        Some(i) => mem.menu_screen.slot_group.get(i).copied().unwrap_or(1),
        None if template == Template::Roster && (a0 >> 10) & 3 == 1 => 2,
        None if template == Template::Roster && (a2 >> 10) & 3 == 1 => 1,
        None => object_box(a0, a1).map_or(1, |b| u8::from(b[1] + b[3] >= 2 * template.cut())),
    };
    assigned == group
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put(oam: &mut [u8], i: usize, a0: u16, a1: u16, a2: u16) {
        oam[i * 8..i * 8 + 6].copy_from_slice(&[a0.to_le_bytes(), a1.to_le_bytes(), a2.to_le_bytes()].concat());
    }
    #[test]
    fn overlapping_pictures_stay_together_in_the_group_of_their_centre() {
        let mut oam = vec![0u8; 1024];
        for i in 0..128 { put(&mut oam, i, 0x0200, 0, 0); }
        // Measured unit screen: portrait (-8..56), unit + shadow, cursor (69).
        put(&mut oam, 0, 0xa0f8, 0xd000, 0x0e18);
        put(&mut oam, 1, 0xa018, 0x9020, 0x0e68);
        put(&mut oam, 2, 0x0045, 0x0077, 0x0120);
        // Measured roster: the unit panel portrait (tiles from row 64 to 152,
        // priority 1, edge to edge) and unit No. 5 at priority 2 over it.
        for (i, [a0, a1, a2]) in [[0xa058, 0xd000, 0x0618], [0xa058, 0x9020, 0x0658], [0xa078, 0x9020, 0x0668],
            [0xa058, 0x5030, 0x0678], [0x2048, 0x5030, 0x0680], [0x6048, 0x1020, 0x0688], [0x2040, 0x1030, 0x068c],
            [0x0034, 0x8018, 0x48d0]].into_iter().enumerate() { put(&mut oam, 3 + i, a0, a1, a2); }
        let unit = sprite_groups(&oam, Template::Unit);
        assert_eq!(&unit[..3], &[0, 0, 1]);
        assert!(sprite_groups(&oam, Template::Whole).iter().take(11).all(|&g| g == 1));
        let roster = sprite_groups(&oam, Template::Roster);
        assert_eq!(&roster[3..11], &[1, 1, 1, 1, 1, 1, 1, 0]);
        // Roster: the selection ellipse (semi-transparent) is its own group; a
        // stray priority-1 portrait part above the cut stays with the panel.
        put(&mut oam, 20, 0x441c, 0x8040, 0xcdb0);
        put(&mut oam, 21, 0x2010, 0x1000, 0x068c);
        let roster = sprite_groups(&oam, Template::Roster);
        assert_eq!((roster[20], roster[21]), (2, 1));
        assert_eq!(sprite_groups(&oam, Template::Unit)[20], 0);
        assert_eq!(roster[100], 0);
    }
    #[test]
    fn menu_planes_follow_their_lists_and_bg1_splits_at_the_separator() {
        assert!(bg_in_group(Template::Unit, 3, 50, 1));
        assert!(bg_in_group(Template::Unit, 1, 58, 0));
        assert!(bg_in_group(Template::Unit, 1, 61, 1));
        assert!(bg_in_group(Template::Roster, 3, 2, 0));
        assert!(bg_in_group(Template::Roster, 2, 120, 1));
        assert!(bg_in_group(Template::Roster, 0, 120, 0) && bg_in_group(Template::Roster, 0, 120, 1));
        assert!(bg_in_group(Template::Whole, 1, 2, 1) && bg_in_group(Template::Whole, 3, 0, 1));
        assert!((0..4).all(|bg| !bg_in_group(Template::Roster, bg, 40, 2)));
    }
}
