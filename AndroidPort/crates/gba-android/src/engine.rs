use crate::compiler::{self, NativeCode};
use gba_core::{Machine, cpu::FLAG_I, mem};
use sha1::{Digest, Sha1};
use std::{collections::BTreeSet, path::PathBuf, sync::{Mutex, atomic::{AtomicU32, Ordering}}, time::Instant};
pub static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
pub static STATUS: Mutex<String> = Mutex::new(String::new());
pub static KEYS: AtomicU32 = AtomicU32::new(0x3ff);
pub const MAX_STEPS: u64 = 5_000_000;
pub fn status(message: impl Into<String>) { *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = message.into(); }
pub struct Engine {
    pub mouse: gba_core::ffta_mouse::LanguageMouse,
    pub touch_enabled: bool,
    pub ui_base: f32,
    pub ui_export_ns: u64,
    pub ui_window: (u32,u32),
    /// Screen rows (top, bottom) where portrait battles may place UI panels.
    pub ui_dock: Option<(f32,f32)>,
    /// The last presented frame showed extended map rows, or only the
    /// black-padded 240x160 picture.
    pub expanded: bool,
    pub padded: bool,
    world_map: crate::world_map::WorldMap,
    /// Rows the presented texture is shifted by (portrait world map), for taps.
    view_shift: i32,
    pub world_rect: [f32;4],
    pub machine: Box<Machine>,
    code: Option<NativeCode>,
    extensions: Vec<NativeCode>,
    pending: Option<std::sync::mpsc::Receiver<Result<NativeCode,String>>>,
    missed: BTreeSet<u32>,
    pub adaptive: bool,
    pub pixels: Vec<i32>,
    pub audio: Vec<i16>,
    pcm: crate::audio::Audio,
    pub native_blocks: u64,
    pub fallback_steps: u64,
    pub frame_ns: u64,
    pub prepare_ms: u64,
    pub save_path: Option<PathBuf>,
    /// Fast forward draws only the last frame of each batch.
    pub skip_hidden_frames: bool,
    pub cache_dir: Option<PathBuf>,
    pub cache_hit: bool,
    rom_cache_key: String,
    /// USA image: RAM addresses measured on it (title taps) apply.
    pub usa: bool,
}
pub fn verify_rom(bytes: &[u8]) -> bool {
    let sha1=format!("{:x}", Sha1::digest(bytes));
    let sha256=gba_core::ffta_rom::digest(bytes);
    (sha1 == "4ac05441f4de70a4ec3dd932116346c61b8783d9"
        && sha256 == gba_core::ffta_rom::SHA256)
        || (sha1 == "9efaf328cbbcbc830be14940e42e4b92d90dfb58"
        && sha256 == "8fab1e390cb9d5e695bfdb86e81246892e1aa8c0cb4014db8235ff76538ea632")
}
impl Engine {
    /// Screen rectangle of the whole world texture, as GameSurface draws it. A padded
    /// portrait/square frame shows only its 240x160 picture, fitted to the screen width,
    /// so the texture rectangle is placed to line that picture up.
    /// Where GameSurface draws the world texture (world_rect without the tap shift).
    pub fn texture_rect(&self)->[f32;4]{
        let mut rect=self.world_rect;
        rect[1]+=self.view_shift as f32*rect[3]/mem::render_height() as f32;
        rect
    }
    pub fn set_view(&mut self,screen:(u32,u32),extra_x:usize,extra_y:usize,fill:bool){
        let (sw,sh)=(screen.0 as f32,screen.1 as f32);
        let (ww,wh)=((240+extra_x*2) as f32,(160+extra_y*2) as f32);
        self.ui_window=screen;
        if self.padded && extra_y>0 && extra_x<=8 {
            let s=(sw/240.).min(sh/160.);
            let (bx,by)=(((sw-240.*s)/2.).trunc(),((sh-160.*s)/2.).trunc());
            self.world_rect=[bx-extra_x as f32*s,by-extra_y as f32*s,ww*s,wh*s];
            return;
        }
        let scale=if fill{(sw/ww).max(sh/wh)}else{(sw/ww).min(sh/wh)};
        let (width,height)=((ww*scale) as i32,(wh*scale) as i32);
        // A shifted texture row r shows rendered row r + view_shift; map taps to the rendered row.
        let shift=self.view_shift as f32*height as f32/wh;
        self.world_rect=[((screen.0 as i32-width)/2) as f32,((screen.1 as i32-height)/2) as f32-shift,width as f32,height as f32];
    }
    pub fn new(rom: Vec<u8>) -> Self {
        let rom_cache_key=gba_core::ffta_rom::digest(&rom);
        let mouse=gba_core::ffta_mouse::LanguageMouse::new(&rom,false);
        Self { mouse,touch_enabled:false,ui_base:240.,ui_export_ns:0,ui_window:(240,160),ui_dock:None,expanded:false,padded:false,world_map:Default::default(),view_shift:0,world_rect:[0.,0.,240.,160.],machine: Box::new(Machine::new(rom)), code: None, extensions: Vec::new(), pending: None,
            missed: BTreeSet::new(), adaptive: cfg!(target_os="android"), pixels: Vec::new(),
            audio: Vec::with_capacity(4096), pcm: crate::audio::Audio::default(), native_blocks: 0,
            fallback_steps: 0, frame_ns: 0, prepare_ms: 0, save_path: None, skip_hidden_frames: true, cache_dir: None, cache_hit: false,
            usa: rom_cache_key==gba_core::ffta_rom::SHA256, rom_cache_key }
    }
    pub fn prepare(&mut self) -> Result<(), String> {
        let start = Instant::now();
        if let Some(path)=self.cache_path("base") {
            if valid_cache(&path) {
                if let Ok(code)=compiler::load_object(&path) {
                    self.code=Some(code);
                    self.cache_hit=true;
                    for index in 0..16 {
                        if let Some(hot)=self.cache_path(&format!("hot-{index}")) {
                            if valid_cache(&hot) {
                                if let Ok(code)=compiler::load_object(&hot) {
                                    self.code.as_mut().unwrap().blocks.extend(code.blocks.iter().map(|(k,v)|(*k,*v)));
                                    self.extensions.push(code);
                                }
                            }
                        }
                    }
                    self.prepare_ms=start.elapsed().as_millis() as u64;
                    status("Ready (cached game)");
                    return Ok(());
                }
            }
        }
        status("Profiling boot code (90 frames)...");
        mem::set_extra(0);
        mem::set_extra_y(0);
        // Separate machine: preparation never advances the user's game/save.
        let mut profile = Machine::new(self.machine.bus.rom.clone());
        if let Some(save) = self.machine.bus.save_data() { profile.bus.load_save_data(save); }
        let mut seeds = BTreeSet::from([0x08000000]);
        for frame in 0..90 {
            let before = profile.bus.frames;
            let mut previous_end = u32::MAX;
            for _ in 0..MAX_STEPS {
                let pc = profile.cpu.regs[15];
                if matches!(pc >> 24, 2 | 3 | 8) && pc != previous_end {
                    seeds.insert(pc | profile.cpu.thumb() as u32);
                }
                previous_end = pc.wrapping_add(if profile.cpu.thumb() {2} else {4});
                profile.step();
                if profile.bus.frames != before { break; }
            }
            profile.bus.audio_buf.clear();
            if frame % 15 == 0 { status(format!("Profiling boot: {frame}/90 frames")); }
        }
        status("Discovering native ARM/Thumb blocks...");
        let view = recomp_core::analyze::View { rom: &profile.bus.rom, ewram: Some(&profile.bus.ewram), iwram: Some(&profile.bus.iwram), bios: None };
        let analysis = recomp_core::analyze::analyze(&view, &seeds.into_iter().collect::<Vec<_>>());
        let object=self.cache_path("base");
        self.code = Some(compiler::compile(&analysis, &view, object.as_deref(), |done, total| {
            status(format!("Compiling native blocks: {done}/{total}"));
        })?);
        if let Some(path)=&object { seal_cache(path)?; }
        self.prepare_ms = start.elapsed().as_millis() as u64;
        status(format!("Ready: {} native blocks, {} ms", self.code.as_ref().unwrap().blocks.len(), self.prepare_ms));
        Ok(())
    }
    pub fn frame(&mut self, count: usize, extra_x: usize, extra_y: usize, sound: bool) -> Result<(), String> {
        self.frame_audio(count,extra_x,extra_y,if sound{1}else{0})
    }
    pub fn frame_audio(&mut self,count:usize,extra_x:usize,extra_y:usize,mode:u32)->Result<(),String>{
        let start = Instant::now();
        self.collect_compilation();
        mem::set_extra(extra_x);
        mem::set_extra_y(extra_y);
        // Portrait views tall enough for the whole world map (352 rows) keep the map
        // still and pin its HUD to the edges; shorter views keep the game camera.
        self.machine.bus.world_hud.enabled=extra_x<=8 && 160+2*extra_y>=360;
        self.audio.clear();
        self.pcm.begin(count.clamp(1,8),mode,
            self.cache_dir.as_deref().unwrap_or(&std::env::temp_dir()))?;
        let count=count.clamp(1,8);
        for index in 0..count {
            let m = &mut *self.machine;
            m.bus.skip_render=self.skip_hidden_frames && index+1<count;
            let keys=self.mouse.keys(&mut m.bus,(KEYS.load(Ordering::Relaxed) as u16)&0x3ff);
            m.bus.keys = gba_core::ffta_battle_mouse::keys(&mut m.bus,keys);
            m.bus.frame_ready = false;
            let mut previous_end = u32::MAX;
            for _ in 0..MAX_STEPS {
                if m.bus.frame_ready { break; }
                let pc = m.cpu.regs[15];
                let native = if m.bus.halted
                    || (pc == gba_core::machine::IRQ_RETURN_ADDR && m.cpu.mode() == gba_core::Mode::Irq)
                    || (m.bus.irq_pending() && !m.cpu.flag(FLAG_I))
                    || m.bus.ffta_battle_mouse.interprets(pc) || m.ffta_sprite_interpret(pc) { None }
                    else { self.code.as_ref().and_then(|code| code.blocks.get(&(pc | m.cpu.thumb() as u32))).copied() };
                if let Some(function) = native {
                    if let Some(h) = m.bus.mp2k.as_deref() {
                        let key = pc | m.cpu.thumb() as u32;
                        if h.active && h.hook_match(key) { m.bus.mp2k_frame_hook(key); }
                    }
                    function(&gba_core::capi::RT_API, m as *mut _ as *mut _);
                    self.native_blocks += 1;
                    previous_end = u32::MAX;
                } else {
                    if self.adaptive && pc != previous_end && matches!(pc>>24,2|3|8) && self.missed.len()<2048
                        && self.code.as_ref().is_some_and(|code|!code.blocks.contains_key(&(pc|m.cpu.thumb() as u32))) {
                        self.missed.insert(pc | m.cpu.thumb() as u32);
                    }
                    previous_end = pc.wrapping_add(if m.cpu.thumb(){2}else{4});
                    m.step(); self.fallback_steps += 1;
                }
            }
            if !m.bus.frame_ready { return Err(format!("Frame stalled at PC {:08x}", m.cpu.regs[15])); }
            self.pcm.feed(&m.bus.audio_buf);
            m.bus.audio_buf.clear();
        }
        self.pcm.finish(&mut self.audio)?;
        let size = mem::render_width() * mem::render_height();
        self.pixels.resize(size, 0);
        let expanded_rows=mem::extra_y()>0 && (self.machine.bus.ffta_movement.enabled || self.machine.bus.ffta_stage.enabled)
            && (0..4).any(|bg|gba_core::ppu::ffta_terrain(&self.machine.bus,bg)
                || gba_core::ffta_stage::layer(&self.machine.bus,bg).is_some()
                || (self.machine.bus.world_hud.enabled && gba_core::ppu::ffta_world_map(&self.machine.bus,bg)));
        let padded=mem::extra_y()>0 && !expanded_rows;
        self.expanded=expanded_rows;
        self.padded=padded;
        if padded { self.pixels.fill(0xff000000u32 as i32); }
        let source = if mem::extra() == 0 && mem::extra_y() == 0 {
            &self.machine.bus.framebuffer[..]
        } else { &self.machine.bus.wide_framebuffer[..if padded{mem::render_width()*160}else{size}] };
        let offset=if padded{mem::extra_y()*mem::render_width()}else{0};
        let colors=rgba_colors();
        for (out,&pixel) in self.pixels[offset..].iter_mut().zip(source){
            *out=colors[(pixel&0x7fff) as usize];
        }
        if padded { backdrop_grid(&mut self.pixels); }
        // world_rect still carries the shift of the frame it was set for.
        let rect=self.texture_rect();
        self.view_shift=0;
        if expanded_rows && self.machine.bus.world_hud.map_frame {
            let rh=mem::render_height() as f32;
            let s=rect[3]/rh;
            let dock=match self.ui_dock {
                Some((top,bottom)) if s>0. => (((top-rect[1])/s).ceil() as i32,((bottom-rect[1])/s).floor() as i32),
                _ => (0,rh as i32),
            };
            self.world_map.present(&self.machine.bus,&mut self.pixels,dock);
            self.view_shift=crate::world_map::WorldMap::shift(&self.machine.bus);
        }
        self.frame_ns = start.elapsed().as_nanos() as u64;
        if self.adaptive && self.machine.bus.frames % 120 < count as u64 { self.start_compilation(); }
        Ok(())
    }
    fn collect_compilation(&mut self) {
        let result = self.pending.as_ref().and_then(|rx|rx.try_recv().ok());
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(code) => {
                    if let Some(primary) = &mut self.code { primary.blocks.extend(code.blocks.iter().map(|(k,v)|(*k,*v))); }
                    self.extensions.push(code);
                }
                Err(error) => status(format!("Background compiler: {error}; interpreter remains available")),
            }
        }
    }
    fn start_compilation(&mut self) {
        if self.pending.is_some() || self.code.is_none() || self.missed.is_empty() || self.extensions.len()>=16 { return; }
        let seeds=std::mem::take(&mut self.missed);
        let rom=self.machine.bus.rom.clone();
        let ewram=self.machine.bus.ewram.clone();
        let iwram=self.machine.bus.iwram.clone();
        let object=self.cache_path(&format!("hot-{}",self.extensions.len()));
        let (tx,rx)=std::sync::mpsc::channel();
        self.pending=Some(rx);
        std::thread::spawn(move || {
            #[cfg(target_os="android")]
            unsafe { libc::setpriority(libc::PRIO_PROCESS,0,10); }
            let view=recomp_core::analyze::View{rom:&rom,ewram:Some(&ewram),iwram:Some(&iwram),bios:None};
            let mut analysis=recomp_core::analyze::analyze(&view,&seeds.iter().copied().collect::<Vec<_>>());
            // Only observed misses are compiled; generated RAM blocks keep content guards.
            analysis.blocks.retain(|block|seeds.contains(&block.key()));
            let result=compiler::compile(&analysis,&view,object.as_deref(),|_,_|{}).and_then(|code|{
                if let Some(path)=&object{seal_cache(path)?;}
                Ok(code)
            });
            let _=tx.send(result);
        });
    }
    fn cache_path(&self,kind:&str)->Option<PathBuf> {
        let directory=self.cache_dir.as_ref()?;
        if std::fs::create_dir_all(directory).is_err(){return None;}
        Some(directory.join(format!("{kind}-{}-{}.o",env!("FFTA_ANDROID_CACHE_TAG"),self.rom_cache_key)))
    }
    pub fn save(&self) -> Result<(), String> {
        if let (Some(path), Some(data)) = (&self.save_path, self.machine.bus.save_data()) {
            let temp = path.with_extension("sav.tmp");
            std::fs::write(&temp, data).map_err(|e| e.to_string())?;
            std::fs::rename(temp, path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn diagnostics(&self) -> String {
        let state=&self.machine.bus.ffta_dialogue;
        let ui_hash=state.layer().map(|(p,_)|p).unwrap_or(&[]).iter()
            .chain(if state.extra.active(){state.extra.pixels.iter()}else{[].iter()})
            .flat_map(|pixel|pixel.packed()).fold(0xcbf29ce484222325u64,|h,b|(h^b as u64).wrapping_mul(0x100000001b3));
        let hash = self.pixels.iter().fold(0xcbf29ce484222325u64, |h,p| (h ^ *p as u32 as u64).wrapping_mul(0x100000001b3));
        let oam_hash=self.machine.bus.oam.iter().fold(0xcbf29ce484222325u64,|h,&v|(h^v as u64).wrapping_mul(0x100000001b3));
        format!("UI FNV64: {ui_hash:016x}\nOAM FNV64: {oam_hash:016x}\nBackend: {}\nNative blocks: {}\nNative dispatches: {}\nInterpreter fallback instructions: {}\nFrames: {}\nLast frame CPU: {:.2} ms\nPreparation: {} ms\nRender: {}x{}\nFramebuffer FNV64: {hash:016x}\nPC: {:08x}\nGame audio: real GBA PCM 65536 -> 48000 Hz\nCache hit: {}\n", 
            if self.code.is_some() {"libtcc in-process native blocks"} else {"cached interpreter (not recompiled)"},
            self.code.as_ref().map_or(0,|c|c.blocks.len()),self.native_blocks,self.fallback_steps,self.machine.bus.frames,
            self.frame_ns as f64/1e6,self.prepare_ms,mem::render_width(),mem::render_height(),self.machine.cpu.regs[15],self.cache_hit)
    }
}
/// Padded frames: GameSurface fills the bars with a blurred copy of the picture.
/// Stretching 160 native rows across a tall screen shows each row as a stripe,
/// so a 30x20 grid of 8x8 averages (plus a 1-texel clamped border) is written
/// into the unused padding rows at texture (0,0)-(32,22) for it to sample.
pub const BACKDROP_GRID:(usize,usize)=(30,20);
fn backdrop_grid(pixels:&mut [i32]){
    let (rw,ey,ex)=(mem::render_width(),mem::extra_y(),mem::extra());
    let (gw,gh)=BACKDROP_GRID;
    if ey<gh+2 || rw<gw+2 {return;}
    let mut grid=vec![0i32;(gw+2)*(gh+2)];
    for gy in 0..gh {for gx in 0..gw {
        let mut sum=[0u32;3];
        for dy in 0..8 {for dx in 0..8 {
            let p=pixels[(ey+gy*8+dy)*rw+ex+gx*8+dx] as u32;
            sum[0]+=(p>>16)&255;sum[1]+=(p>>8)&255;sum[2]+=p&255;
        }}
        grid[(gy+1)*(gw+2)+gx+1]=(0xff000000|(sum[0]/64)<<16|(sum[1]/64)<<8|sum[2]/64) as i32;
    }}
    for gy in 0..gh+2 {for gx in 0..gw+2 {
        let (sy,sx)=(gy.clamp(1,gh),gx.clamp(1,gw));
        pixels[gy*rw+gx]=grid[sy*(gw+2)+sx];
    }}
}
pub(crate) fn rgba_colors()->&'static[i32]{
    static COLORS:std::sync::OnceLock<Box<[i32]>>=std::sync::OnceLock::new();
    COLORS.get_or_init(||(0u32..32768).map(|pixel|{
        let r=(pixel&31)*255/31;
        let g=((pixel>>5)&31)*255/31;
        let b=((pixel>>10)&31)*255/31;
        (0xff000000|(r<<16)|(g<<8)|b) as i32
    }).collect::<Vec<_>>().into_boxed_slice())
}
fn seal_cache(path:&std::path::Path)->Result<(),String>{
    let data=std::fs::read(path).map_err(|e|e.to_string())?;
    std::fs::write(path.with_extension("sha256"),gba_core::ffta_rom::digest(&data)).map_err(|e|e.to_string())
}
fn valid_cache(path:&std::path::Path)->bool{
    if !std::fs::metadata(path).is_ok_and(|meta|meta.len()>0 && meta.len()<64*1024*1024){return false;}
    match (std::fs::read(path),std::fs::read_to_string(path.with_extension("sha256"))){
        (Ok(data),Ok(hash))=>gba_core::ffta_rom::digest(&data)==hash,
        _=>false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_color_conversion_matches_every_hardware_color(){
        for pixel in 0u32..65536{
            let r=(pixel&31)*255/31;let g=((pixel>>5)&31)*255/31;let b=((pixel>>10)&31)*255/31;
            assert_eq!(rgba_colors()[(pixel&0x7fff) as usize],(0xff000000|(r<<16)|(g<<8)|b) as i32);
        }
    }
    static VIEW_TEST:std::sync::Mutex<()>=std::sync::Mutex::new(());
    #[test]
    fn rejects_large_arbitrary_file() { assert!(!verify_rom(&vec![0; 8*1024*1024])); }
    #[test]
    fn real_bus_keys_and_ppu_match_desktop_for_600_frames() {
        let _lock=VIEW_TEST.lock().unwrap();
        let mut rom = vec![0; 192];
        rom[..4].copy_from_slice(&0xeafffffeu32.to_le_bytes());
        let mut mobile = Engine::new(rom.clone());
        let mut reference = Machine::new(rom);
        for frame in 0..600 {
            let keys = if frame % 2 == 0 {0x3fe} else {0x3ff};
            KEYS.store(keys, Ordering::Relaxed);
            reference.bus.keys = keys as u16;
            reference.run_frame(MAX_STEPS);
            mobile.frame(1,0,0,false).unwrap();
            assert_eq!(mobile.machine.bus.framebuffer, reference.bus.framebuffer);
            assert_eq!(mobile.machine.bus.oam, reference.bus.oam);
            assert_eq!(mobile.machine.bus.keys, reference.bus.keys);
            assert_eq!(mobile.machine.cpu.regs, reference.cpu.regs);
        }
    }
    #[test]
    fn square_view_does_not_show_stale_rows_outside_battle() {
        let _lock=VIEW_TEST.lock().unwrap();
        let mut rom=vec![0;192];
        rom[..4].copy_from_slice(&0xeafffffeu32.to_le_bytes());
        let mut engine=Engine::new(rom);
        engine.machine.bus.io[0]=0x80;
        engine.frame(1,8,48,false).unwrap();
        assert_eq!(engine.pixels.len(),256*256);
        assert_eq!(engine.pixels[0],0xff000000u32 as i32);
        assert_eq!(engine.pixels[48*256],0xffffffffu32 as i32);
        assert_eq!(engine.pixels[255*256],0xff000000u32 as i32);
        mem::set_extra(0);mem::set_extra_y(0);
    }
    #[test]
    #[ignore = "requires a native TinyCC library and the user's exact ROM"]
    fn recompiled_ffta_matches_desktop_for_600_frames() {
        let rom = std::fs::read(std::env::var_os("FFTA_TEST_ROM").expect("FFTA_TEST_ROM")).unwrap();
        assert!(verify_rom(&rom));
        let mut mobile = Engine::new(rom.clone());
        let mut reference = Machine::new(rom);
        mobile.adaptive = true;
        let cache = std::env::var_os("FFTA_ANDROID_CACHE_TEST").map(PathBuf::from);
        mobile.cache_dir = cache.clone();
        println!("FFTA_ANDROID test: preparing native code");
        mobile.prepare().unwrap();
        if cache.is_some() {
            let mut warm=Engine::new(mobile.machine.bus.rom.clone());
            warm.cache_dir=cache;
            warm.adaptive=true;
            warm.prepare().unwrap();
            assert!(warm.cache_hit,"warm start must load objects without profiling/recompiling");
            println!("FFTA_ANDROID warm cache hit: {} ms",warm.prepare_ms);
            mobile=warm;
        }
        println!("FFTA_ANDROID test: prepared {} blocks", mobile.code.as_ref().unwrap().blocks.len());
        mobile.configure_touch(false,240.,1.,1.);
        for _ in 0..600 {
            KEYS.store(0x3ff,Ordering::Relaxed);
            reference.run_frame(MAX_STEPS);
            reference.bus.audio_buf.clear();
            mobile.frame(1,0,0,false).unwrap();
            assert_eq!(mobile.machine.bus.framebuffer,reference.bus.framebuffer,"frame {}",reference.bus.frames);
            assert_eq!(mobile.machine.bus.oam,reference.bus.oam,"OAM frame {}",reference.bus.frames);
            for (&pixel,&color) in mobile.pixels.iter().zip(&reference.bus.framebuffer) {
                assert_eq!(pixel,rgba_colors()[(color&0x7fff) as usize],"presented zero-extra pixels");
            }
        }
        println!("FFTA_ANDROID 600 frames framebuffer/OAM IDENTICAL\n{}",mobile.diagnostics());
    }
}

#[cfg(test)]
mod tutorial_replay_tests {
    use super::*;
    #[test]
    #[ignore = "requires user ROM, replay and backup save; writes metrics only"]
    fn tutorial_characters_replay() {
        use std::io::Write;
        let rom=std::fs::read(std::env::var_os("FFTA_TEST_ROM").unwrap()).unwrap();assert!(verify_rom(&rom));
        let mut engine=Engine::new(rom);engine.adaptive=true;
        engine.machine.bus.load_save_data(&std::fs::read(std::env::var_os("FFTA_TEST_SAVE").unwrap()).unwrap());
        engine.prepare().unwrap();
        engine.configure_touch(false,240.,1.,1.);
        let replay=std::fs::read_to_string(std::env::var_os("FFTA_TEST_INPUT").unwrap()).unwrap();
        let points=replay.lines().skip(1).filter(|s|!s.trim().is_empty()).map(|s|{
            let mut p=s.split_whitespace();(p.next().unwrap().parse::<u64>().unwrap(),u32::from_str_radix(p.next().unwrap(),16).unwrap())
        }).collect::<Vec<_>>();
        let mut file=std::fs::File::create(std::env::var_os("FFTA_METRIC_OUT").unwrap()).unwrap();
        writeln!(file,"frame,roster_valid,present,drawn,ids,drawn_ids,duplicates,missing_positions").unwrap();
        let mut cursor=0;let mut keys=1023;
        for frame in 0..13100u64 {
            while cursor<points.len() && points[cursor].0<=frame{keys=points[cursor].1;cursor+=1;}
            KEYS.store(keys,Ordering::Relaxed);
            gba_core::ffta_sprites::set_character_metrics(frame>=11899);
            engine.frame(1,136,176,false).unwrap();
            if frame>=11899{writeln!(file,"{}",gba_core::ffta_sprites::tutorial_metric(&engine.machine.bus)).unwrap();}
        }
        // Own debug data only: decoded OAM attributes, never ROM code.
        let bus=&engine.machine.bus;
        let path=PathBuf::from(std::env::var_os("FFTA_METRIC_OUT").unwrap()).with_extension("oam.csv");
        let mut out=std::fs::File::create(path).unwrap();
        writeln!(out,"slot,a0,a1,a2,host_replacements").unwrap();
        for(slot,entry)in bus.oam.chunks_exact(8).enumerate(){
            let a=|i|u16::from_le_bytes([entry[i],entry[i+1]]);
            let replacements=bus.ffta_sprites.active.iter().flatten().filter(|o|o.replaces && o.slot==slot).count();
            writeln!(out,"{slot},{:04x},{:04x},{:04x},{replacements}",a(0),a(2),a(4)).unwrap();
        }
        gba_core::ffta_sprites::set_character_metrics(false);mem::set_extra(0);mem::set_extra_y(0);
    }
}
