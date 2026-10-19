//! Headless host runner for the Android engine: boots a ROM (and optional
//! save), plays a scripted input file and writes PNG screenshots composed the
//! way GameSurface draws them (world texture plus the two native UI layers).
//!
//!   cargo run --release -p gba-android --example headless -- \
//!       --rom ffta.gba [--sav ffta.sav] --script input.txt --out shots \
//!       [--screen 1080x2400] [--zoom 0] [--touch] [--frames N] [--save-out out.sav]
//!       [--speed N] [--no-skip] [--hashes hashes.txt] [--dock TOP:BOTTOM]
//!
//! --speed runs N frames per engine call like fast forward (events and shots
//! land on batch boundaries); --no-skip draws hidden frames too; --hashes
//! writes one presented-frame hash per batch for comparing runs.
//!
//! Script lines (frame numbers are absolute, '#' starts a comment):
//!   press <frame> <BUTTON[+BUTTON...]> [frames=6]   BUTTON: A B SELECT START RIGHT LEFT UP DOWN R L
//!   tap <frame> <x> <y> [back]                       x/y as 0..1 fractions of the screen
//!   shot <frame> <name>
//!   speed <frame> <N>                                frames per engine call from then on (1-8)
//!   dump <frame> <name>                              writes <out>/<name>.ewram/.iwram/.oam/.io
//!   end <frame>
use gba_android::engine::{Engine, KEYS};
use std::{path::PathBuf, sync::atomic::Ordering};

const BUTTONS: [&str; 10] = ["A", "B", "SELECT", "START", "RIGHT", "LEFT", "UP", "DOWN", "R", "L"];

enum Event { Press(u16, u64), Tap(f32, f32, bool), Shot(String), Speed(u64), Dump(String) }

struct View { extra_x: usize, extra_y: usize, base_width: f32 }

/// Mirrors ViewConfig.create for AspectMode.DEVICE (NativeBridge.kt).
fn device_view(screen: (u32, u32), zoom: i32) -> View {
    let ratio = screen.0.max(1) as f64 / screen.1.max(1) as f64;
    let (width, height) = if ratio >= 1.5 { ((160. * ratio).max(242.).min(1536.), 160.) }
        else if ratio < 1. { (240., (240. / ratio).min(800.)) }
        else { (256., (256. / ratio).min(800.)) };
    let limit = (1536. / width).min(800. / height);
    let max_zoom = ((limit.ln() / 2f64.powf(1. / 6.).ln()).floor() as i32).clamp(0, 6);
    let scale = 2f64.powf(zoom.clamp(0, max_zoom) as f64 / 6.);
    View {
        extra_x: (((width * scale - 240.) / 2.).ceil() as i64).clamp(0, 648) as usize,
        extra_y: (((height * scale - 160.) / 2.).ceil() as i64).clamp(0, 320) as usize,
        base_width: (width * 160. / height) as f32,
    }
}

fn parse_script(text: &str) -> Result<(Vec<(u64, Event)>, Option<u64>), String> {
    let mut events = Vec::new();
    let mut end = None;
    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() { continue; }
        let parts: Vec<_> = line.split_whitespace().collect();
        let error = || format!("script line {}: {line}", number + 1);
        let frame: u64 = parts.get(1).and_then(|f| f.parse().ok()).ok_or_else(error)?;
        match parts[0] {
            "press" => {
                let mut mask = 0u16;
                for name in parts.get(2).ok_or_else(error)?.split('+') {
                    let bit = BUTTONS.iter().position(|b| b.eq_ignore_ascii_case(name)).ok_or_else(error)?;
                    mask |= 1 << bit;
                }
                let duration = parts.get(3).map_or(Ok(6), |d| d.parse()).map_err(|_| error())?;
                events.push((frame, Event::Press(mask, duration)));
            }
            "tap" => {
                let x = parts.get(2).and_then(|v| v.parse().ok()).ok_or_else(error)?;
                let y = parts.get(3).and_then(|v| v.parse().ok()).ok_or_else(error)?;
                events.push((frame, Event::Tap(x, y, parts.get(4) == Some(&"back"))));
            }
            "shot" => events.push((frame, Event::Shot(parts.get(2).ok_or_else(error)?.to_string()))),
            "dump" => events.push((frame, Event::Dump(parts.get(2).ok_or_else(error)?.to_string()))),
            "speed" => events.push((frame, Event::Speed(parts.get(2).and_then(|v| v.parse::<u64>().ok()).ok_or_else(error)?.clamp(1, 8)))),
            "end" => end = Some(frame),
            _ => return Err(error()),
        }
    }
    events.sort_by_key(|e| e.0);
    Ok((events, end))
}

/// Composes one presented frame like FrameRenderer.onDrawFrame.
fn compose(engine: &Engine, view: &View, screen: (u32, u32)) -> Vec<u8> {
    let (sw, sh) = (screen.0 as usize, screen.1 as usize);
    let mut out = vec![0u8; sw * sh * 3];
    for pixel in out.chunks_exact_mut(3) { pixel.copy_from_slice(&[6, 9, 8]); }
    let (ww, wh) = (240 + 2 * view.extra_x, 160 + 2 * view.extra_y);
    let rect = engine.texture_rect();
    // Padded scenes: blurred, dimmed band behind; only the band itself is drawn on top.
    let padded = engine.padded && view.extra_y > 0;
    let crop_x = view.extra_x <= 8;
    let (band_x, band_w) = if crop_x { (view.extra_x, 240) } else { (0, ww) };
    if padded {
        let aspect = sw as f32 / sh as f32;
        let (bw, bh) = if aspect < 1.5 { (240. * aspect / 1.5, 160.) } else { (240., 240. / aspect) };
        // GameSurface samples the engine's 30x20 average grid at texel (1,1) with linear filtering.
        let (gx0, gy0) = (1. + (30. - 30. * bw / 240.) / 2., 1. + (20. - 20. * bh / 160.) / 2.);
        let texel = |x: usize, y: usize| engine.pixels[y.min(21) * ww + x.min(31)] as u32;
        for y in 0..sh {
            for x in 0..sw {
                let gx = gx0 + (x as f32 + 0.5) / sw as f32 * 30. * bw / 240. - 0.5;
                let gy = gy0 + (y as f32 + 0.5) / sh as f32 * 20. * bh / 160. - 0.5;
                let (ix, iy) = (gx.floor().max(0.) as usize, gy.floor().max(0.) as usize);
                let (fx, fy) = (gx - gx.floor(), gy - gy.floor());
                let o = &mut out[(y * sw + x) * 3..][..3];
                for (i, shift) in [16u32, 8, 0].into_iter().enumerate() {
                    let c = |px: usize, py: usize| ((texel(px, py) >> shift) & 255) as f32;
                    let v = c(ix, iy) * (1. - fx) * (1. - fy) + c(ix + 1, iy) * fx * (1. - fy)
                        + c(ix, iy + 1) * (1. - fx) * fy + c(ix + 1, iy + 1) * fx * fy;
                    o[i] = (v * 0.34 + [3., 4.6, 4.][i]) as u8;
                }
            }
        }
    }
    let world_rgb = |x: f32, y: f32, source: &[i32]| -> Option<[u8; 3]> {
        let u = (x - rect[0]) / rect[2];
        let v = (y - rect[1]) / rect[3];
        if !(0. ..1.).contains(&u) || !(0. ..1.).contains(&v) { return None; }
        let p = source[(v * wh as f32) as usize * ww + (u * ww as f32) as usize] as u32;
        Some([(p >> 16) as u8, (p >> 8) as u8, p as u8])
    };
    for y in 0..sh {
        for x in 0..sw {
            if padded {
                let u = ((x as f32 + 0.5 - rect[0]) / rect[2] * ww as f32) as isize;
                let v = ((y as f32 + 0.5 - rect[1]) / rect[3] * wh as f32) as isize;
                if u < band_x as isize || u >= (band_x + band_w) as isize
                    || v < view.extra_y as isize || v >= (view.extra_y + 160) as isize { continue; }
            }
            if let Some(rgb) = world_rgb(x as f32 + 0.5, y as f32 + 0.5, &engine.pixels) {
                out[(y * sw + x) * 3..][..3].copy_from_slice(&rgb);
            }
        }
    }
    let mut packed = vec![0u8; 480 * 160 * 4 * 2];
    let mut params = [0f32; 8 * 9 + 4];
    let mut background = vec![0i32; ww * wh];
    engine.export_ui(&mut packed, &mut params, &mut background);
    if let Some(prefix) = std::env::var_os("FFTA_UI_DUMP") {
        // Both native UI layers as 480x160 images (magenta where transparent).
        for layer in 0..2 {
            let rgb: Vec<u8> = packed[layer * 480 * 160 * 4..(layer + 1) * 480 * 160 * 4].chunks_exact(4).flat_map(|d| {
                if d[2] == 0 && d[3] == 16 { return [255, 0, 255]; }
                let code = d[0] as u32 | (d[1] as u32) << 8;
                [(code & 31) as u8 * 8, ((code >> 5) & 31) as u8 * 8, ((code >> 10) & 31) as u8 * 8]
            }).collect();
            let _ = write_png(&PathBuf::from(format!("{}-layer{layer}.png", prefix.to_string_lossy())), 480, 160, &rgb);
        }
    }
    if std::env::var_os("FFTA_UI_DEBUG").is_some() {
        let state = &engine.machine.bus.ffta_dialogue;
        eprintln!("ui params {params:?}\n  main {:?} extra {:?} kind {:?} action {} town {} shop {} site {} expanded {} padded {}",
            gba_core::ffta_dialogue::pixel_bounds(&state.pixels), gba_core::ffta_dialogue::pixel_bounds(&state.extra.pixels),
            state.extra.kind, state.action_layout, state.town_layout, state.shop_layout, state.site_layout, engine.expanded, engine.padded);
    }
    for slot in 0..8 {
        let p = &params[slot * 9..slot * 9 + 9];
        let layer = (p[8] as usize) % 2;
        if p[2] <= 0. { continue; }
        let stats = p[8] >= 2.;
        let (x0, y0) = (p[0].max(p[4]).max(0.) as usize, p[1].max(p[5]).max(0.) as usize);
        let x1 = ((p[0] + p[2]).min(p[4] + p[6]).min(sw as f32)) as usize;
        let y1 = ((p[1] + p[3]).min(p[5] + p[7]).min(sh as f32)) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let u = ((fx - p[0]) / p[2] * 480.) as usize;
                let v = ((fy - p[1]) / p[3] * 160.) as usize;
                let d = &packed[layer * 480 * 160 * 4 + (v.min(159) * 480 + u.min(479)) * 4..][..4];
                if d[2] == 0 && d[3] == 16 { continue; }
                let code = d[0] as u32 | (d[1] as u32) << 8;
                let fg = [code & 31, (code >> 5) & 31, (code >> 10) & 31];
                let bg = world_rgb(fx, fy, if stats { &background } else { &engine.pixels }).unwrap_or([0; 3]);
                let pixel = &mut out[(y * sw + x) * 3..][..3];
                for c in 0..3 {
                    let lane = if c == 1 { 2 } else { 1 };
                    let b = (bg[c] as u32 * 31 + 127) / 255;
                    let value = ((fg[c] * lane * d[2] as u32 + b * lane * d[3] as u32 + 8) / 16).min(31 * lane) / lane;
                    pixel[c] = (value * 255 / 31) as u8;
                }
            }
        }
    }
    out
}

fn write_png(path: &std::path::Path, width: u32, height: u32, rgb: &[u8]) -> std::io::Result<()> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data { c ^= b as u32; for _ in 0..8 { c = if c & 1 != 0 { 0xedb88320 ^ (c >> 1) } else { c >> 1 }; } }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let sum = crc(&out[start..]);
        out.extend(sum.to_be_bytes());
    }
    let mut raw = Vec::with_capacity((width as usize * 3 + 1) * height as usize);
    for row in rgb.chunks_exact(width as usize * 3) { raw.push(0); raw.extend(row); }
    // zlib stream of stored (uncompressed) deflate blocks.
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<_> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        z.push((i + 1 == blocks.len()) as u8);
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(*block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw { a = (a + byte as u32) % 65521; b = (b + a) % 65521; }
    z.extend(((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png)
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut rom, mut sav, mut script, mut out, mut save_out) = (None, None, None, PathBuf::from("shots"), None);
    let (mut screen, mut zoom, mut touch, mut frames) = ((2400u32, 1080u32), 0, false, None);
    let (mut speed, mut skip, mut hashes) = (1u64, true, None);
    let mut dock = None;
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--rom" => rom = Some(PathBuf::from(value()?)),
            "--sav" => sav = Some(PathBuf::from(value()?)),
            "--script" => script = Some(PathBuf::from(value()?)),
            "--out" => out = PathBuf::from(value()?),
            "--save-out" => save_out = Some(PathBuf::from(value()?)),
            "--screen" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--screen WIDTHxHEIGHT")?;
                screen = (w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?);
            }
            "--zoom" => zoom = value()?.parse().map_err(|_| "bad zoom")?,
            "--frames" => frames = Some(value()?.parse::<u64>().map_err(|_| "bad frame count")?),
            "--touch" => touch = true,
            "--speed" => speed = value()?.parse::<u64>().map_err(|_| "bad speed")?.clamp(1, 8),
            "--no-skip" => skip = false,
            "--hashes" => hashes = Some(PathBuf::from(value()?)),
            "--dock" => {
                let v = value()?;
                let (t, b) = v.split_once(':').ok_or("--dock TOP:BOTTOM in screen pixels")?;
                dock = Some((t.parse().map_err(|_| "bad dock top")?, b.parse().map_err(|_| "bad dock bottom")?));
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    let rom = std::fs::read(rom.ok_or("--rom is required")?).map_err(|e| format!("read ROM: {e}"))?;
    if !gba_android::engine::verify_rom(&rom) { return Err("ROM is not the exact FFTA USA/Europe image".into()); }
    let (events, end) = match &script {
        Some(path) => parse_script(&std::fs::read_to_string(path).map_err(|e| format!("read script: {e}"))?)?,
        None => (Vec::new(), None),
    };
    let last = frames.or(end).unwrap_or_else(|| events.last().map_or(600, |e| e.0 + 1));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut engine = Engine::new(rom);
    if let Some(path) = &sav { engine.machine.bus.load_save_data(&std::fs::read(path).map_err(|e| format!("read save: {e}"))?); }
    engine.save_path = save_out;
    engine.skip_hidden_frames = skip;
    let mut hash_log = String::new();
    let mut frame_ns = 0u64;
    if std::env::var_os("FFTA_ANDROID_TCC_LIBRARY").is_some() {
        // Like the APK: compile boot code, then missed blocks in the background.
        engine.adaptive = true;
        engine.cache_dir = Some(std::env::temp_dir().join("ffta-headless-cache"));
        engine.prepare()?;
    }
    let view = device_view(screen, zoom);
    engine.configure_touch(touch, view.base_width, 1., 1.);
    engine.ui_dock = dock;
    engine.set_view(screen, view.extra_x, view.extra_y, true);
    let mut held: Vec<(u16, u64)> = Vec::new();
    let mut next = 0;
    let started = std::time::Instant::now();
    let mut frame = 0;
    while frame < last {
        let mut shots = Vec::new();
        let mut dumps = Vec::new();
        engine.set_view(screen, view.extra_x, view.extra_y, true);
        // Events due before this batch ends run now; a speed change applies from
        // the batch that starts at or after its frame, so it ends the scan early.
        let mut pending = next;
        while pending < events.len() && events[pending].0 < frame + speed {
            if let Event::Speed(n) = events[pending].1 {
                if events[pending].0 > frame { break; }
                speed = n;
            }
            pending += 1;
        }
        for index in std::mem::replace(&mut next, pending)..pending {
            match &events[index].1 {
                Event::Press(mask, duration) => held.push((*mask, frame + duration)),
                Event::Tap(x, y, back) => engine.tap(*x, *y, *back),
                Event::Shot(name) => shots.push(name.clone()),
                Event::Speed(_) => {}
                Event::Dump(name) => dumps.push(name.clone()),
            }
        }
        held.retain(|h| h.1 > frame);
        let pressed = held.iter().fold(0u16, |m, h| m | h.0);
        KEYS.store((!pressed & 0x3ff) as u32, Ordering::Relaxed);
        // OAM as it was while the frame was drawn (the game rewrites it at VBlank).
        let oam_before = (!dumps.is_empty()).then(|| engine.machine.bus.oam.clone());
        engine.frame(speed as usize, view.extra_x, view.extra_y, false)?;
        frame_ns += engine.frame_ns;
        if hashes.is_some() {
            let hash = engine.pixels.iter().fold(0xcbf29ce484222325u64, |h, p| (h ^ *p as u32 as u64).wrapping_mul(0x100000001b3));
            hash_log += &format!("{} {hash:016x} {}\n", frame + speed - 1, engine.frame_ns);
        }
        for name in dumps {
            std::fs::write(out.join(format!("{name}.ewram")), &engine.machine.bus.ewram).map_err(|e| e.to_string())?;
            std::fs::write(out.join(format!("{name}.iwram")), &engine.machine.bus.iwram).map_err(|e| e.to_string())?;
            std::fs::write(out.join(format!("{name}.oam")), &engine.machine.bus.oam).map_err(|e| e.to_string())?;
            if let Some(oam) = &oam_before { std::fs::write(out.join(format!("{name}.oam0")), oam).map_err(|e| e.to_string())?; }
            std::fs::write(out.join(format!("{name}.io")), &engine.machine.bus.io).map_err(|e| e.to_string())?;
        }
        if !shots.is_empty() { engine.set_view(screen, view.extra_x, view.extra_y, true); }
        for name in shots {
            let path = out.join(format!("{name}.png"));
            write_png(&path, screen.0, screen.1, &compose(&engine, &view, screen)).map_err(|e| e.to_string())?;
            if std::env::var_os("FFTA_UI_DEBUG").is_some() {
                // Raw world texture and the untouched 240x160 hardware frame, plus BG scroll registers.
                let raw: Vec<u8> = engine.pixels.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
                write_png(&out.join(format!("{name}.raw.png")), (240 + 2 * view.extra_x) as u32,
                    (160 + 2 * view.extra_y) as u32, &raw).map_err(|e| e.to_string())?;
                let native: Vec<u8> = engine.machine.bus.framebuffer.iter().flat_map(|&p| {
                    let c = |v: u16| ((v & 31) as u32 * 255 / 31) as u8;
                    [c(p), c(p >> 5), c(p >> 10)]
                }).collect();
                write_png(&out.join(format!("{name}.native.png")), 240, 160, &native).map_err(|e| e.to_string())?;
                let io = &engine.machine.bus.io;
                let r = |o: usize| u16::from_le_bytes([io[o], io[o + 1]]);
                if let Some(active) = &engine.machine.bus.ffta_sprites.active {
                    for h in active {
                        let [a0, a1, a2] = h.attrs;
                        eprintln!("host x={} y={} a0={a0:04x} a1={a1:04x} a2={a2:04x} tile={:#x} prio={} slot={}",
                            h.x, h.y, a2 & 1023, (a2 >> 10) & 3, h.slot);
                    }
                }
                let hud = &engine.machine.bus.world_hud;
                if hud.map_frame {
                    // HUD blocks: 8x8 cells holding captured pixels, merged by flood fill.
                    let mut cells = [[false; 30]; 20];
                    for (i, &p) in hud.pixels.iter().enumerate() { if p != 0xffff { cells[i / 240 / 8][i % 240 / 8] = true; } }
                    let mut seen = [[false; 30]; 20];
                    for cy in 0..20 { for cx in 0..30 {
                        if !cells[cy][cx] || seen[cy][cx] { continue; }
                        let mut stack = vec![(cy, cx)]; let mut b = [cx, cy, cx, cy]; seen[cy][cx] = true;
                        while let Some((y, x)) = stack.pop() {
                            b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
                            for (dy, dx) in [(0i32, 1i32), (0, -1), (1, 0), (-1, 0)] {
                                let (ny, nx) = (y as i32 + dy, x as i32 + dx);
                                if (0..20).contains(&ny) && (0..30).contains(&nx) && cells[ny as usize][nx as usize] && !seen[ny as usize][nx as usize] {
                                    seen[ny as usize][nx as usize] = true; stack.push((ny as usize, nx as usize));
                                }
                            }
                        }
                        eprintln!("hud block x {}..{} y {}..{}", b[0] * 8, b[2] * 8 + 8, b[1] * 8, b[3] * 8 + 8);
                    }}
                }
                eprintln!("dispcnt {:04x} bgcnt {:04x} {:04x} {:04x} {:04x} hofs {} {} {} {} vofs {} {} {} {}",
                    r(0), r(8), r(10), r(12), r(14), r(16), r(20), r(24), r(28), r(18), r(22), r(26), r(30));
            }
            println!("frame {frame}: {}", path.display());
        }
        frame += speed;
    }
    engine.save()?;
    if let Some(path) = &hashes { std::fs::write(path, hash_log).map_err(|e| e.to_string())?; }
    eprintln!("engine time {:.2} ms per emulated frame (speed {speed}, skip {skip})", frame_ns as f64 / 1e6 / last as f64);
    eprintln!("{last} frames in {:.1}s ({} native dispatches, {} interpreted instructions)",
        started.elapsed().as_secs_f64(), engine.native_blocks, engine.fallback_steps);
    Ok(())
}
