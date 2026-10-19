//! Portrait world map Menu button, drawn with the game's own resources (USA):
//! a one-line panel in the style of the game's menu windows (their bar, the
//! translucent body and the item gem, all resident in BG VRAM on the world
//! map with their palettes) holding "Menu" in the game's menu font (ROM). Its
//! text uses the menu item colours: white, or the highlighted item's orange
//! while a menu is open. Every resource is verified before it is used.
use gba_core::mem::MemMap;

/// Native screen rectangle (x0, y0, x1, y1) in the bottom HUD row, between
/// Clan Funds (x 3-77) and the date (152-240).
pub const NATIVE: [i32; 4] = [86, 133, 134, 159];

/// Menu font: 16x16 cells of four 4-bpp tiles (left column top, bottom, then
/// right column), digits, A-Z, a-z. The game copies these tiles as they are.
const FONT: usize = 0x438630;
const FONT_CELLS: usize = 62;
const FONT_FNV: u64 = 0x76c1_3cdb_afb1_66fd;
/// World map BG character base and the menu window tiles there: the bottom
/// bar (cap, middle), the item gem (two halves) and the window body.
const CHAR_BASE: usize = 0x8000;
const TILES: [(usize, [u8; 32]); 5] = [
    (0x9, hex("99999999d9ddddddc9ccccccc9cccccc99999999000000000000000000000000")),
    (0xa, hex("99999999ddddddddcccccccccccccccc99999999000000000000000000000000")),
    (0xb, hex("0000000000000000000000000000000000000000000000000000000000110100")),
    (0xc, hex("10ba1a0010bb1b0010ba1a000011010000000000000000000000000000000000")),
    (0x1, hex("7777777777777777777777777777777777777777777777777777777777777777")),
];
/// Palettes: window frame and body, the item gem, normal and highlighted items.
const FRAME: usize = 12;
const GEM: usize = 13;
const TEXT: usize = 12;
const TEXT_ACTIVE: usize = 11;

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

/// The menu font is present (USA ROM).
pub fn font_ok(rom: &[u8]) -> bool {
    rom.get(FONT..FONT + FONT_CELLS * 128).is_some_and(|cells| {
        cells.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) == FONT_FNV
    })
}

fn tiles_ok(bus: &MemMap) -> bool {
    TILES.iter().all(|(t, data)| bus.vram.get(CHAR_BASE + t * 32..CHAR_BASE + t * 32 + 32) == Some(&data[..]))
}

/// 4-bpp pixel of BG tile `t` in the world map character base.
fn tile_pixel(bus: &MemMap, t: usize, x: usize, y: usize) -> u8 {
    (bus.vram[CHAR_BASE + t * 32 + y * 4 + x / 2] >> (4 * (x & 1))) & 15
}

/// A glyph's 16x16 palette indices and its advance (width of its pixels).
fn glyph(rom: &[u8], ch: char) -> Option<([[u8; 16]; 16], usize)> {
    let cell = match ch {
        '0'..='9' => ch as usize - '0' as usize,
        'A'..='Z' => 10 + ch as usize - 'A' as usize,
        'a'..='z' => 36 + ch as usize - 'a' as usize,
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

/// Draws the button into `out` (render_width x render_height texture) with
/// native row r at texture row r + `row_offset`. Returns false (nothing drawn)
/// when a resource does not match.
pub fn draw(bus: &MemMap, out: &mut [i32], rw: usize, rh: usize, ex: usize, row_offset: i32, active: bool) -> bool {
    if !font_ok(&bus.rom) || !tiles_ok(bus) { return false; }
    let colors = crate::engine::rgba_colors();
    let color = |palette: usize, index: u8| u16::from_le_bytes([bus.palette[(palette * 16 + index as usize) * 2],
        bus.palette[(palette * 16 + index as usize) * 2 + 1]]) & 0x7fff;
    let io = |o: usize| u16::from_le_bytes([bus.io[o], bus.io[o + 1]]);
    let (eva, evb) = ((io(0x52) & 31).min(16) as u32, ((io(0x52) >> 8) & 31).min(16) as u32);
    let [x0, y0, x1, y1] = NATIVE;
    let width = (x1 - x0) as usize;
    let mut put = |x: i32, y: i32, pixel: Option<u16>, blend: bool| {
        let (tx, ty) = (ex as i32 + x, y + row_offset);
        if !(0..rw as i32).contains(&tx) || !(0..rh as i32).contains(&ty) { return; }
        let target = &mut out[ty as usize * rw + tx as usize];
        let Some(mut c) = pixel else { return };
        if blend {
            // The window body is translucent (BLDALPHA), as the game's menus.
            let under = *target as u32;
            let world = ((under >> 19) & 31) | ((under >> 11) & 31) << 5 | ((under >> 3) & 31) << 10;
            c = [0, 5, 10].iter().fold(0, |acc, &s| {
                acc | ((((c as u32 >> s) & 31) * eva + ((world >> s) & 31) * evb) / 16).min(31) << s
            }) as u16;
            *target = colors[c as usize];
        } else {
            *target = colors[crate::world_map::faded(bus, c, 0x08) as usize];
        }
    };
    // Bars: the window's bottom bar, mirrored for the top edge.
    let bar = |y: i32, row: usize, put: &mut dyn FnMut(i32, i32, Option<u16>, bool)| {
        for x in 0..width {
            let (tile, tx) = if x < 8 { (0x9, x) } else if x >= width - 8 { (0x9, width - 1 - x) } else { (0xa, x % 8) };
            let index = tile_pixel(bus, tile, tx, row);
            if index != 0 { put(x0 + x as i32, y, Some(color(FRAME, index)), false); }
        }
    };
    for (i, row) in [4, 3, 2, 1, 0].into_iter().enumerate() { bar(y0 + i as i32, row, &mut put); }
    let body = y0 + 5;
    for y in 0..16 {
        for x in 0..width { put(x0 + x as i32, body + y, Some(color(FRAME, tile_pixel(bus, 0x1, x % 8, 0))), true); }
    }
    for (i, row) in (0..5).enumerate() { bar(body + 16 + i as i32, row, &mut put); }
    // The item gem left of the text, as in the game's menus.
    for y in 0..5usize {
        let (tile, row) = if y == 0 { (0xb, 7) } else { (0xc, y - 1) };
        for x in 0..8 {
            let index = tile_pixel(bus, tile, x, row);
            if index != 0 { put(x0 + 2 + x as i32, body + 5 + y as i32, Some(color(GEM, index)), false); }
        }
    }
    let mut cx = x0 + 10;
    for ch in "Menu".chars() {
        let Some((pixels, advance)) = glyph(&bus.rom, ch) else { continue };
        for (y, row) in pixels.iter().enumerate() {
            for (x, &index) in row.iter().enumerate() {
                if index != 0 { put(cx + x as i32, body + y as i32, Some(color(if active { TEXT_ACTIVE } else { TEXT }, index)), false); }
            }
        }
        cx += advance as i32 - 1;
    }
    debug_assert!(cx <= x1 && y1 == body + 21);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_tiles_decode_as_the_window_bar() {
        // Bar rows: outline (9), highlight (d), fill (c), fill, outline.
        assert_eq!(TILES[1].1[..4], [0x99; 4]);
        assert_eq!(TILES[1].1[4..8], [0xdd; 4]);
        assert!(!font_ok(&[0; 16]));
    }
}
