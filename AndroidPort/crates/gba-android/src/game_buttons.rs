//! Touch-only buttons drawn into the game picture in the style of the game's
//! own menu windows (USA): the window's bar above and below, its translucent
//! body and the item gem, with the label in the game's menu font and colours
//! (white, or the highlighted item's orange while its menu is open).
//!
//! The font and the menu palettes are read from the ROM (where the game
//! copies them from). The window's bar, gem and body tiles are stored in a
//! compressed archive; the five 8x8 tiles below are byte-for-byte copies of
//! them as the game loads them into BG VRAM (measured on the world map).
//!
//! Buttons share one row: the bottom HUD row (native rows 133-159), where
//! the world map shows Clan Funds and the date and town scenes Clan Funds;
//! settings sit in a row under the top of the frame.
use gba_core::mem::MemMap;

/// Menu font: 16x16 cells of four 4-bpp tiles (left column top, bottom, then
/// right column), digits, A-Z, a-z.
const FONT: usize = 0x438630;
const FONT_CELLS: usize = 62;
const FONT_FNV: u64 = 0x76c1_3cdb_afb1_66fd;
/// Menu palettes 11 (highlighted item), 12 (window, items), 13 (item gem).
const PALETTES: usize = 0x41a0c0;
const PALETTES_FNV: u64 = 0xabe7_498d_10b6_faf1;
const TEXT_ACTIVE: usize = 0;
const FRAME: usize = 1;
const GEM: usize = 2;
/// The window's bottom bar (cap, middle), the item gem (two halves), the body.
const BAR_CAP: [u8; 32] = hex("99999999d9ddddddc9ccccccc9cccccc99999999000000000000000000000000");
const BAR: [u8; 32] = hex("99999999ddddddddcccccccccccccccc99999999000000000000000000000000");
const GEM_TOP: [u8; 32] = hex("0000000000000000000000000000000000000000000000000000000000110100");
const GEM_BOTTOM: [u8; 32] = hex("10ba1a0010bb1b0010ba1a000011010000000000000000000000000000000000");
const BODY_INDEX: u8 = 7;
/// The world map's menu blending (BLDALPHA 0x080d), so buttons look the same in every scene.
const EVA: u32 = 13;
const EVB: u32 = 8;

/// The shared button row (native rows, bottom-anchored like the HUD).
pub const ROW: (i32, i32) = (133, 159);
/// Rows a layout keeps free at the bottom for the row.
pub const ROW_HEIGHT: i32 = 28;

const fn digit(c: u8) -> u8 { if c >= b'a' { c - b'a' + 10 } else { c - b'0' } }
const fn hex(text: &str) -> [u8; 32] {
    let bytes = text.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = digit(bytes[2 * i]) << 4 | digit(bytes[2 * i + 1]);
        i += 1;
    }
    out
}

fn fnv(data: &[u8]) -> u64 { data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) }

/// The game's font and menu palettes are present (USA ROM).
pub fn available(rom: &[u8]) -> bool {
    rom.get(FONT..FONT + FONT_CELLS * 128).is_some_and(|d| fnv(d) == FONT_FNV)
        && rom.get(PALETTES..PALETTES + 96).is_some_and(|d| fnv(d) == PALETTES_FNV)
}

fn tile_pixel(tile: &[u8; 32], x: usize, y: usize) -> u8 { (tile[y * 4 + x / 2] >> (4 * (x & 1))) & 15 }

/// A glyph's 16x16 palette indices and its advance (width of its pixels).
fn glyph(rom: &[u8], ch: char) -> Option<([[u8; 16]; 16], usize)> {
    let cell = match ch {
        '0'..='9' => ch as usize - '0' as usize,
        'A'..='Z' => 10 + ch as usize - 'A' as usize,
        'a'..='z' => 36 + ch as usize - 'a' as usize,
        '<' => 92,
        '>' => 93,
        '%' => return percent(rom),
        _ => return None,
    };
    let base = FONT + cell * 128;
    let mut pixels = [[0u8; 16]; 16];
    for (y, row) in pixels.iter_mut().enumerate() {
        for (x, pixel) in row.iter_mut().enumerate() {
            let tile = base + ((x / 8) * 2 + y / 8) * 32;
            *pixel = (rom[tile + (y % 8) * 4 + (x % 8) / 2] >> (4 * (x & 1))) & 15;
        }
    }
    let width = (0..16).rev().find(|&x| pixels.iter().any(|row| row[x] != 0)).map_or(4, |x| x + 1);
    Some((pixels, width))
}

/// '%', which the menu font lacks: drawn as its digits are (measured on
/// '0': outline index 1, fill 3, a shadow 2 under and right of the fill),
/// their height.
fn percent(rom: &[u8]) -> Option<([[u8; 16]; 16], usize)> {
    let (zero, _) = glyph(rom, '0')?;
    let (edge, fill, shadow) = (1u8, 3u8, 2u8);
    let top = zero.iter().position(|r| r.iter().any(|&i| i != 0))?;
    let bottom = zero.iter().rposition(|r| r.iter().any(|&i| i != 0))?;
    // Rings over and under a slash, as tall as the digits; outlined like them.
    const SHAPE: [&str; 11] = [
        ".##...#.", "#..#..#.", "#..#.#..", ".##..#..", "....#...", "....#...",
        "...#....", "..#..##.", "..#.#..#", ".#..#..#", ".#...##.",
    ];
    let mut px = [[0u8; 16]; 16];
    let h = bottom - top + 1;
    for y in 0..h {
        let row = SHAPE[y * SHAPE.len() / h.max(1)].as_bytes();
        // Strokes two pixels wide, as the font's.
        for (x, &c) in row.iter().enumerate() { if c == b'#' { px[top + y][x + 2] = fill; px[top + y][x + 3] = fill; } }
    }
    let shape = px;
    for y in 0..16 { for x in 0..16 {
        if shape[y][x] != fill { continue; }
        for (dx, dy) in [(1i32, 1i32), (1, 0), (0, 1), (-1, 0), (0, -1), (-1, -1), (1, -1), (-1, 1)] {
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if (0..16).contains(&nx) && (0..16).contains(&ny) && shape[ny as usize][nx as usize] == 0 && px[ny as usize][nx as usize] == 0 {
                px[ny as usize][nx as usize] = if (dx, dy) == (1, 1) { shadow } else { edge };
            }
        }
    } }
    Some((px, 13))
}

fn text_width(rom: &[u8], label: &str) -> i32 {
    label.chars().map(|c| glyph(rom, c).map_or(4, |g| g.1 as i32 - 1)).sum::<i32>() + 1
}

/// Native width of a button for `label`: gem, text, margins, whole tiles.
pub fn width(rom: &[u8], label: &str) -> i32 { (10 + text_width(rom, label) + 6 + 7) / 8 * 8 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    /// Native x range in the button row.
    pub x: (i32, i32),
    pub label: &'static str,
    /// KEYINPUT mask pressed on a tap.
    pub key: u16,
    /// Drawn highlighted (its menu is open, or the setting it toggles is on).
    pub active: bool,
    /// In the row under the top of the frame instead of the bottom HUD row.
    pub top: bool,
}

/// Buttons laid out right-aligned ending at native x `right`, `gap` apart.
pub fn row(rom: &[u8], right: i32, items: &[(&'static str, u16, bool)]) -> Vec<Button> {
    let mut x1 = right;
    let mut out: Vec<Button> = items.iter().rev().map(|&(label, key, active)| {
        let x0 = x1 - width(rom, label);
        let button = Button { x: (x0, x1), label, key, active, top: false };
        x1 = x0 - 4;
        button
    }).collect();
    out.reverse();
    out
}

/// Draws `button` into `out` (render_width x render_height) with native row r
/// at texture row r + `row_offset`. It fades with the scene (BLDCNT
/// brightness on BG3, like the game's windows).
pub fn draw(bus: &MemMap, out: &mut [i32], rw: usize, rh: usize, ex: usize, row_offset: i32, button: &Button) {
    let rom = &bus.rom;
    let colors = crate::engine::rgba_colors();
    let color = |palette: usize, index: u8| {
        let o = PALETTES + (palette * 16 + index as usize) * 2;
        u16::from_le_bytes([rom[o], rom[o + 1]]) & 0x7fff
    };
    let (x0, x1) = button.x;
    let width = (x1 - x0) as usize;
    let mut put = |x: i32, y: i32, c: u16, blend: bool| {
        let (tx, ty) = (ex as i32 + x, y + row_offset);
        if !(0..rw as i32).contains(&tx) || !(0..rh as i32).contains(&ty) { return; }
        let target = &mut out[ty as usize * rw + tx as usize];
        let mut c = c;
        if blend {
            let under = *target as u32;
            let world = ((under >> 19) & 31) | ((under >> 11) & 31) << 5 | ((under >> 3) & 31) << 10;
            c = [0, 5, 10].iter().fold(0, |acc, &s| {
                acc | ((((c as u32 >> s) & 31) * EVA + ((world >> s) & 31) * EVB) / 16).min(31) << s
            }) as u16;
            *target = colors[c as usize];
        } else {
            *target = colors[crate::world_map::faded(bus, c, 0x08) as usize];
        }
    };
    let bar = |y: i32, row: usize, put: &mut dyn FnMut(i32, i32, u16, bool)| {
        for x in 0..width {
            let (tile, tx) = if x < 8 { (&BAR_CAP, x) } else if x >= width - 8 { (&BAR_CAP, width - 1 - x) } else { (&BAR, x % 8) };
            let index = tile_pixel(tile, tx, row);
            if index != 0 { put(x0 + x as i32, y, color(FRAME, index), false); }
        }
    };
    let y0 = ROW.0;
    // Top bar (the bottom bar mirrored), body, bottom bar.
    for (i, row) in [4, 3, 2, 1, 0].into_iter().enumerate() { bar(y0 + i as i32, row, &mut put); }
    let body = y0 + 5;
    for y in 0..16 {
        for x in 0..width { put(x0 + x as i32, body + y, color(FRAME, BODY_INDEX), true); }
    }
    for row in 0..5 { bar(body + 16 + row as i32, row, &mut put); }
    for y in 0..5usize {
        let (tile, row) = if y == 0 { (&GEM_TOP, 7) } else { (&GEM_BOTTOM, y - 1) };
        for x in 0..8 {
            let index = tile_pixel(tile, x, row);
            if index != 0 { put(x0 + 2 + x as i32, body + 5 + y as i32, color(GEM, index), false); }
        }
    }
    let mut cx = x0 + 10;
    for ch in button.label.chars() {
        let Some((pixels, advance)) = glyph(rom, ch) else { cx += 4; continue };
        for (y, row) in pixels.iter().enumerate() {
            for (x, &index) in row.iter().enumerate() {
                if index != 0 { put(cx + x as i32, body + y as i32, color(if button.active { TEXT_ACTIVE } else { FRAME }, index), false); }
            }
        }
        cx += advance as i32 - 1;
    }
}

/// Draws a window in the buttons' style into the texture: its bars over and
/// under a translucent body `body_h` rows high, `w` wide, top left at texture
/// (x0, y0); each line of text (in the menu font, `scale` times its size,
/// highlighted while `active`) at its row in the body, from `left` columns in.
pub fn draw_card(bus: &MemMap, out: &mut [i32], rw: usize, rh: usize, x0: i32, y0: i32, w: i32, body_h: i32, lines: &[(i32, i32, &str, bool, i32)]) {
    let rom = &bus.rom;
    if !available(rom) { return; }
    let colors = crate::engine::rgba_colors();
    let color = |palette: usize, index: u8| {
        let o = PALETTES + (palette * 16 + index as usize) * 2;
        u16::from_le_bytes([rom[o], rom[o + 1]]) & 0x7fff
    };
    let mut put = |x: i32, y: i32, c: u16, blend: bool| {
        if !(0..rw as i32).contains(&x) || !(0..rh as i32).contains(&y) { return; }
        let target = &mut out[y as usize * rw + x as usize];
        let mut c = c;
        if blend {
            let under = *target as u32;
            let world = ((under >> 19) & 31) | ((under >> 11) & 31) << 5 | ((under >> 3) & 31) << 10;
            c = [0, 5, 10].iter().fold(0, |acc, &s| acc | ((((c as u32 >> s) & 31) * EVA + ((world >> s) & 31) * EVB) / 16).min(31) << s) as u16;
            *target = colors[c as usize];
        } else {
            *target = colors[crate::world_map::faded(bus, c, 0x08) as usize];
        }
    };
    let width = w as usize;
    let bar = |y: i32, row: usize, put: &mut dyn FnMut(i32, i32, u16, bool)| {
        for x in 0..width {
            let (tile, tx) = if x < 8 { (&BAR_CAP, x) } else if x >= width - 8 { (&BAR_CAP, width - 1 - x) } else { (&BAR, x % 8) };
            let index = tile_pixel(tile, tx, row);
            if index != 0 { put(x0 + x as i32, y, color(FRAME, index), false); }
        }
    };
    for (i, row) in [4, 3, 2, 1, 0].into_iter().enumerate() { bar(y0 + i as i32, row, &mut put); }
    let body = y0 + 5;
    for y in 0..body_h { for x in 0..w { put(x0 + x, body + y, color(FRAME, BODY_INDEX), true); } }
    for row in 0..5 { bar(body + body_h + row as i32, row, &mut put); }
    for &(left, top, text, active, scale) in lines {
        let mut cx = x0 + left;
        for ch in text.chars() {
            let Some((pixels, advance)) = glyph(rom, ch) else { cx += 4 * scale; continue };
            for (y, row) in pixels.iter().enumerate() {
                for (x, &index) in row.iter().enumerate() {
                    if index == 0 { continue; }
                    for dy in 0..scale { for dx in 0..scale {
                        put(cx + x as i32 * scale + dx, body + top + y as i32 * scale + dy, color(if active { TEXT_ACTIVE } else { FRAME }, index), false);
                    } }
                }
            }
            cx += (advance as i32 - 1) * scale;
        }
    }
}
/// Native width of `text` in the menu font at `scale`.
pub fn text_width_scaled(rom: &[u8], text: &str, scale: i32) -> i32 { text_width(rom, text) * scale }

/// Draws one menu-font glyph into a native UI canvas (`ffta_dialogue`
/// pixels) with its 16x16 cell at native `cell`: the window text colours, or
/// the highlighted item's while `active`.
pub fn draw_glyph_ui(rom: &[u8], canvas: &mut [gba_core::ffta_dialogue::UiPixel], cell: [i32; 2], ch: char, active: bool) {
    use gba_core::ffta_dialogue::{UiPixel, LAYER_INSET, LAYER_WIDTH};
    if !available(rom) { return; }
    let Some((pixels, _)) = glyph(rom, ch) else { return };
    let color = |index: u8| {
        let o = PALETTES + ((if active { TEXT_ACTIVE } else { FRAME }) * 16 + index as usize) * 2;
        u16::from_le_bytes([rom[o], rom[o + 1]]) & 0x7fff
    };
    for (y, row) in pixels.iter().enumerate() {
        for (x, &index) in row.iter().enumerate() {
            let (cx, cy) = (cell[0] + x as i32 + LAYER_INSET, cell[1] + y as i32);
            if index == 0 || !(0..LAYER_WIDTH as i32).contains(&cx) || !(0..160).contains(&cy) { continue; }
            if let Some(p) = canvas.get_mut(cy as usize * LAYER_WIDTH + cx as usize) { *p = UiPixel::opaque(color(index)); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_tiles_decode_as_the_bar_and_buttons_never_overlap() {
        // Bar rows: outline (9), highlight (d), fill (c), fill, outline.
        assert_eq!((0..8).map(|x| tile_pixel(&BAR, x, 1)).collect::<Vec<_>>(), vec![0xd; 8]);
        assert_eq!(tile_pixel(&GEM_BOTTOM, 3, 1), 0xb);
        assert!(!available(&[0; 16]));
        // Without a ROM font every letter is 4 wide; the row stays ordered and gapped.
        let rom = vec![0u8; FONT + FONT_CELLS * 128];
        let buttons = row(&rom, 156, &[("Info", 0x2ff, false), ("Return", 0x3fd, false)]);
        assert_eq!(buttons[1].x.1, 156);
        assert_eq!(buttons[0].x.1 + 4, buttons[1].x.0);
        assert!(buttons.iter().all(|b| (b.x.1 - b.x.0) % 8 == 0));
    }
}
