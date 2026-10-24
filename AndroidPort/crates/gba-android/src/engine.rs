use crate::compiler::{self, NativeCode};
use gba_core::{Machine, cpu::FLAG_I, ffta_battle_mouse as battle, mem};
use sha1::{Digest, Sha1};
use std::{collections::BTreeSet, path::PathBuf, sync::{Mutex, atomic::{AtomicU32, Ordering}}, time::Instant};
pub static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);
pub static STATUS: Mutex<String> = Mutex::new(String::new());
pub static KEYS: AtomicU32 = AtomicU32::new(0x3ff);
pub const MAX_STEPS: u64 = 5_000_000;
pub fn status(message: impl Into<String>) { *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = message.into(); }
/// Measured USA Party screen selection (EWRAM): column and row bytes, and the
/// unit index the game derives from them.
const PARTY_COLUMN:usize=0xfd29;
const PARTY_INDEX:usize=0xfd3e;
/// The Return button on a unit's status screen: leaves the Party screens
/// (B to the roster, then B again).
/// Return in the battle's free cursor (`free_return`).
pub(crate) const FREE_HOME: u16 = 5;
pub(crate) const PARTY_CLOSE:u16=1;
/// Frames the roster is shown settled before its hint bar keys are taken
/// (measured: Start ignored 30 frames after B from a status screen, taken at 45).
const ROSTER_SETTLE:u32=24;

/// A unit's status screen (the top unit screen): its MENU (Equip Items, Pick
/// Abilities, Change Jobs, Leave Clan) is the game's cursor graph of four
/// entries at x 143 (measured USA; Equip and Abilities have theirs at x 0).
pub(crate) fn party_status(bus:&gba_core::mem::MemMap)->bool{
    bus.menu_screen.frame && bus.menu_screen.template==gba_core::menu_screen::Template::Unit
        && gba_core::ffta_graph_menu::GraphMenu::read(bus).is_some_and(|m|m.count==4 && m.entry(bus,0).x>=128)
}

#[derive(Clone,Copy)]
pub(crate) struct PartyGoal {
    pub target:usize,
    pub confirm:bool,
    pub deadline:u64,
    /// The selection was set (index then, frame).
    pub pressed:Option<(usize,u64)>,
    /// Roster cursor position (EWRAM 0xfd30) and for how many frames it has
    /// not moved: the cursor glides to a unit and A is only taken once it rests.
    pub cursor:[u8;4],
    pub still:u32,
    /// Party menus: the native row the menu cursor (`party_cursor`) must
    /// reach instead of a unit (it moves with Up/Down; A once it rests).
    pub row:Option<i32>,
    /// Graph menus (gba-core ffta_graph_menu): the entry jumped to; A once it is selected.
    pub graph:Option<u8>,
    /// Text lists (gba-core ffta_row_list): the list and the visible row jumped to.
    pub list:Option<(usize,u8)>,
}

/// The shop's Buy/Sell list cursor: the menu pointer at the list's left edge.
pub(crate) fn shop_list_cursor(bus:&gba_core::mem::MemMap)->Option<(i32,i32)>{
    let state=&bus.ffta_dialogue;
    if !state.shop_layout || state.town_layout {return None;}
    party_cursor(bus).filter(|&(x,_)|x<16)
}

/// The game's menu cursor on Party screens (OAM tiles 0x120-0x127, priority 0;
/// with two lists shown the later one is the active list's). Not dialogue's
/// continuation arrow (8x16, palette 2, tile 0x122), which shares the tiles.
pub(crate) fn party_cursor(bus:&gba_core::mem::MemMap)->Option<(i32,i32)>{
    let half=|o:&[u8],i:usize|u16::from_le_bytes([o[i],o[i+1]]);
    bus.oam.chunks_exact(8).filter(|o|{
        let (a0,a2)=(half(o,0),half(o,4));
        let arrow=a0&0xff00==0x8000 && a2==0x2122;
        // 4-bpp: a portrait's 8-bpp tiles can share the numbers (measured: the
        // results' portrait, 32x64 at 0,0, tile 0x126).
        !arrow && a0&0x300!=0x200 && a0&0x2000==0 && (0x120..=0x127).contains(&(a2&1023)) && (a2>>10)&3==0 && (a0&255)<160
    }).map(|o|((half(o,2)&511) as i32,(half(o,0)&255) as i32)).last()
}

pub struct Engine {
    pub mouse: gba_core::ffta_mouse::LanguageMouse,
    pub touch_enabled: bool,
    pub ui_base: f32,
    pub ui_export_ns: u64,
    pub ui_window: (u32,u32),
    /// Screen rows (top, bottom) where portrait battles may place UI panels.
    pub ui_dock: Option<(f32,f32)>,
    /// Last town menu block (touch::town_menu) while its town scene lasts.
    pub(crate) town_menu: Option<[i32;4]>,
    /// The last presented frame showed extended map rows, or only the
    /// black-padded 240x160 picture.
    pub expanded: bool,
    pub padded: bool,
    world_map: crate::world_map::WorldMap,
    /// Portrait Party screen placement of the current frame (menu_layout.rs).
    pub menu_layout: Option<crate::menu_layout::MenuLayout>,
    /// The roster kept for the unit screens (menu_layout.rs).
    pub(crate) party_cache: Option<crate::menu_layout::PartyCache>,
    /// A unit selected by tap, reached one press at a time (`drive_party_goal`).
    pub(crate) party_goal: Option<PartyGoal>,
    /// Native rows the party grid is scrolled by (menu_layout.rs).
    pub(crate) party_scroll: i32,
    /// The Party screens were shown last frame; they opened on the roster and
    /// its first unit is still to be opened; Return is leaving them (until);
    /// a roster hint bar key waits for the roster to take input (key, until,
    /// frames it has been settled).
    party_shown: bool,
    party_open_first: bool,
    party_leave: Option<u64>,
    party_bar: Option<(u16,u64,u32)>,
    /// Portrait world map laws (laws.rs).
    pub(crate) laws: crate::laws::Laws,
    /// A rumor's text is being closed (until), pub_touch.rs.
    pub(crate) rumor_close: Option<u64>,
    /// Town root menu (item count, selected item) last shown: names the shop list's button.
    shop_choice: (u8,u8),
    /// Last battery save written, and the frame of the last resume state.
    last_battery: Option<Vec<u8>>,
    resume_frame: u64,
    /// Screen rectangle of the app's own world map Menu button (touch-only
    /// portrait, only when the game's resources for `buttons` are missing).
    pub menu_button: Option<[f32;4]>,
    /// Buttons drawn into the game picture this frame (game_buttons.rs) and
    /// their screen rectangles.
    pub buttons: Vec<(crate::game_buttons::Button,[f32;4])>,
    /// Touch-only shops show the item Info window throughout (a saved setting,
    /// `<save>.prefs`), and when the game last had it toggled for that.
    pub shop_info: bool,
    shop_info_pressed: u64,
    /// Frames the shop list has been shown with the Info window closed.
    shop_list_shown: u32,
    /// The finger the engine follows (gestures.rs), the app's touch slop (px),
    /// and the quantity arrow last stepped (until the frame).
    pub(crate) gesture: Option<crate::gestures::Gesture>,
    /// The cell the battle's free cursor started on (the unit whose turn it is).
    pub(crate) free_home: Option<(u8,u8)>,
    /// System was opened from the unit's commands (battle_ui.rs): closing it goes back there.
    pub(crate) system_return: bool,
    pub(crate) free_return_after: Option<u64>,
    /// Touch-only shop lists that scroll and swipe (shop_scroll.rs).
    pub(crate) shop_scroll: crate::shop_scroll::ShopScroll,
    pub(crate) gesture_slop: f32,
    pub(crate) quantity_flash: (bool,u64),
    /// The frame keeps a row under its top for setting buttons.
    pub(crate) top_row: bool,
    /// HUD brightness of the previous frame and how many frames it has been
    /// unchanged: the game fades the world map by rewriting the palette.
    hud_luma: (u64,u32),
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
    /// Portrait world map: a Menu button (Start) in the HUD row pinned to the
    /// frame bottom, in the gap between Clan Funds (native x 3-77) and the date
    /// (152-240), rows 140-156. Shown only while the map waits for input: not
    /// during fades (HUD brightness changed within 8 frames, or a BLDY fade)
    /// and not while a game menu is open (Start does not close it).
    fn map_menu_button(&mut self)->Option<[f32;4]>{
        let bus=&self.machine.bus;
        if !(bus.world_hud.map_frame && bus.ffta_dialogue.docked) {self.hud_luma=(0,0);return None;}
        let luma:u64=bus.world_hud.pixels.iter().filter(|&&p|p!=gba_core::world_hud::TRANSPARENT)
            .map(|&p|(p&31) as u64*2+((p>>5)&31) as u64*4+((p>>10)&31) as u64).sum();
        let steady=if luma>0 && luma==self.hud_luma.0 {self.hud_luma.1.saturating_add(1)}else{0};
        self.hud_luma=(luma,steady);
        if steady<8 {return None;}
        let (_,bottom)=self.ui_dock?;
        let io=|o:usize|u16::from_le_bytes([bus.io[o],bus.io[o+1]]);
        if matches!((io(0x50)>>6)&3,2|3) && io(0x54)&31!=0 {return None;}
        if bus.world_hud.menu {return None;}
        const NATIVE:[f32;4]=[88.,140.,140.,156.];
        let s=self.world_rect[2]/mem::render_width() as f32;
        Some([self.world_rect[0]+(mem::extra() as f32+NATIVE[0])*s,bottom-(160.-NATIVE[1])*s,
            (NATIVE[2]-NATIVE[0])*s,(NATIVE[3]-NATIVE[1])*s])
    }
    /// Resume state of the whole machine (gba-core savestate.rs), taken between frames.
    /// The machine state followed by what the portrait presentation keeps
    /// across frames (remembered towns, the party picture), so a resumed game
    /// looks exactly as it was left.
    pub fn save_state(&self)->Vec<u8>{
        let core=gba_core::savestate::save(&self.machine);
        let mut w=gba_core::savestate::W(Vec::with_capacity(core.len()+64*1024));
        w.bytes(&core);
        self.world_map.save(&mut w);
        w.bool(self.party_cache.is_some());
        if let Some(cache)=&self.party_cache {cache.save(&mut w);}
        self.laws.save(&mut w);
        w.0
    }
    /// Restores a resume state. Input in flight is dropped; a presentation
    /// part that cannot be read starts empty (it is refilled as the game draws).
    pub fn load_state(&mut self,data:&[u8])->Result<(),String>{
        let mut r=gba_core::savestate::R::new(data);
        gba_core::savestate::load(&mut self.machine,r.bytes()?)?;
        self.mouse.cancel();
        self.machine.bus.ffta_battle_mouse.cancel();
        self.party_goal=None;self.town_menu=None;self.menu_layout=None;self.view_shift=0;
        self.world_map=crate::world_map::WorldMap::load(&mut r).unwrap_or_default();
        self.party_cache=match r.bool() {
            Ok(true)=>crate::menu_layout::PartyCache::load(&mut r).ok(),
            _=>None,
        };
        self.laws=crate::laws::Laws::load(&mut r).unwrap_or_default();
        Ok(())
    }
    /// Touch-only portrait buttons of this scene, in the bottom HUD row:
    /// world map Menu (Start; B closes an open menu, then it is highlighted),
    /// Party screens Return (B), town lists Return, shop lists Buy/Sell and,
    /// while the shop offers its "R Info" (OAM tiles 0x2d4-0x2d9), the Info
    /// setting at the top right.
    /// A town scene (shop, pub, prison...) with the game-style button row.
    pub(crate) fn town_buttons(&self)->bool{
        let bus=&self.machine.bus;
        self.touch_enabled && bus.ffta_dialogue.docked && !self.padded && crate::game_buttons::available(&bus.rom)
            && gba_core::ppu::docked_town_scene(bus)
    }
    /// The battle's free cursor (Return from the command menu): the field's
    /// cursor with nothing to choose among (no move or target cells).
    pub(crate) fn free_cursor(&self)->bool{
        let bus=&self.machine.bus;
        !crate::deploy_touch::shown(bus) && battle::Scene::read(bus).is_some_and(|s|s.cells.iter().all(|c|c.range==0))
    }
    /// Remembers where the free cursor starts (on the unit whose turn it is);
    /// forgets it once a turn goes on (moving, a target, the facing).
    fn track_free_home(&mut self){
        let bus=&self.machine.bus;
        if battle::facing_center(bus).is_some() || battle::Scene::read(bus).is_some_and(|s|s.cells.iter().any(|c|c.range>0)) {self.free_home=None;return;}
        if self.free_home.is_none() && self.free_cursor() {self.free_home=battle::Scene::read(bus).map(|s|s.selected);}
    }
    /// Return in the free cursor: back to the unit whose turn it is, its command menu open.
    pub(crate) fn free_return(&mut self){
        self.mouse.cancel();
        match self.free_home {
            Some((x,y)) => {battle::select(&mut self.machine.bus,x,y,true);}
            None => {battle::press(&mut self.machine.bus,0x3fd);}
        }
    }
    fn scene_buttons(&self)->Vec<crate::game_buttons::Button>{
        use crate::game_buttons::row;
        let bus=&self.machine.bus;
        if !self.touch_enabled || !bus.ffta_dialogue.docked || self.padded || !crate::game_buttons::available(&bus.rom) {return Vec::new();}
        let (start,back)=(0x3f7,0x3fd);
        if let Some(at)=crate::pub_touch::notice(bus) {
            return if crate::pub_touch::notice_asks(bus,at) {row(&bus.rom,236,&[("Yes",crate::pub_touch::YES,false),("No",back,false)])}
                else {row(&bus.rom,236,&[("OK",0x3fe,false)])};
        }
        if let Some(buttons)=self.deploy_buttons() {return buttons;}
        if bus.world_hud.map_frame {
            let open=bus.world_hud.menu;
            let w=crate::game_buttons::width(&bus.rom,"Menu");
            return row(&bus.rom,110+w/2,&[("Menu",if open {back} else {start},open)]);
        }
        if self.menu_layout.is_some() {
            let mut buttons=row(&bus.rom,236,&[("Return",if party_status(bus) {PARTY_CLOSE} else {back},false)]);
            if crate::battle_ui::status(bus) {buttons.extend(self.battle_buttons().unwrap_or_default());}
            return buttons;
        }
        if self.town_buttons() {
            if let Some(phase)=gba_core::ffta_shop_mouse::phase(bus) {return self.shop_buttons(phase);}
            if let Some(buttons)=self.pub_buttons() {return buttons;}
            // Menus offering Leave (the town root menus) have no Return; lists
            // and other sub-menus (a menu pointer, no root menu) do.
            if bus.ffta_dialogue.town_layout || party_cursor(bus).is_none() {return Vec::new();}
            let half=|o:&[u8],i:usize|u16::from_le_bytes([o[i],o[i+1]]);
            let hint=bus.oam.chunks_exact(8).any(|o|half(o,0)&0x300!=0x200 && (0x2d4..=0x2d9).contains(&(half(o,4)&1023)) && (half(o,4)>>10)&3==0);
            if shop_list_cursor(bus).is_some() {
                // Shop lists: choose the selected item (and confirm its quantity), back;
                // at the top right the Info setting (key 0) while the game offers its Info.
                let choose=if self.shop_choice==(3,1) {"Sell"} else {"Buy"};
                let mut buttons=row(&bus.rom,236,&[(choose,0x3fe,false),("Return",back,false)]);
                if hint || gba_core::ppu::shop_info_open(bus) {
                    buttons.extend(row(&bus.rom,236,&[("Info",0,self.shop_info)]).into_iter().map(|b|crate::game_buttons::Button{top:true,..b}));
                }
                return buttons;
            }
            return row(&bus.rom,236,&[("Return",back,false)]);
        }
        if let Some(buttons)=self.battle_buttons() {return buttons;}
        Vec::new()
    }
    /// Shop buttons by where the shop is (none in its menu, which has Leave,
    /// or while it changes): lists choose the selected item or go back, with
    /// the Info setting at the top right; quantity confirms or goes back; the
    /// price confirmation is OK or Cancel; a message waits for OK.
    fn shop_buttons(&self,phase:gba_core::ffta_shop_mouse::ShopPhase)->Vec<crate::game_buttons::Button>{
        use gba_core::ffta_shop_mouse::ShopPhase;
        use crate::game_buttons::{row,Button};
        let rom=&self.machine.bus.rom;
        let (ok,back)=(0x3fe,0x3fd);
        let choose=|sell:bool|if sell {"Sell"} else {"Buy"};
        match phase {
            ShopPhase::List{sell}=>{
                let mut buttons=row(rom,236,&[(choose(sell),ok,false),("Return",back,false)]);
                buttons.extend(row(rom,236,&[("Info",0,self.shop_info)]).into_iter().map(|b|Button{top:true,..b}));
                buttons
            }
            ShopPhase::Quantity{sell}=>row(rom,236,&[(choose(sell),ok,false),("Return",back,false)]),
            ShopPhase::Confirm{..}=>row(rom,236,&[("OK",ok,false),("Cancel",back,false)]),
            ShopPhase::Message=>row(rom,236,&[("OK",ok,false)]),
            ShopPhase::Menu|ShopPhase::Busy=>Vec::new(),
        }
    }
    /// Keeps the game's item Info window open or closed, as set, wherever the
    /// shop offers it (its own R toggle, at most every 30 frames). It opens
    /// once the list has settled and been kept to be drawn under it.
    fn sync_shop_info(&mut self){
        let bus=&mut self.machine.bus;
        let offered=self.buttons.iter().any(|(b,_)|b.top && b.key==0);
        let open=gba_core::ppu::shop_info_open(bus);
        self.shop_list_shown=if offered && !open {self.shop_list_shown.saturating_add(1)} else {0};
        if !offered || battle::busy(bus) || self.party_goal.is_some() || self.shop_scroll.busy() {return;}
        if open==self.shop_info || bus.frames<self.shop_info_pressed+30 {return;}
        if !open && (self.shop_list_shown<10 || !gba_core::ppu::shop_list_kept(bus)) {return;}
        battle::press(bus,0x2ff);
        self.shop_info_pressed=bus.frames;
    }
    /// The Info setting button was tapped.
    pub(crate) fn toggle_shop_info(&mut self){
        self.shop_info=!self.shop_info;
        self.save_prefs();
    }
    fn prefs_path(&self)->Option<PathBuf>{self.save_path.as_ref().map(|p|p.with_extension("prefs"))}
    /// Settings kept next to the save: `shop_info=1`.
    pub fn load_prefs(&mut self){
        let text=self.prefs_path().and_then(|p|std::fs::read_to_string(p).ok()).unwrap_or_default();
        self.shop_info=text.lines().any(|l|l.trim()=="shop_info=1");
    }
    fn save_prefs(&self){
        if let Some(path)=self.prefs_path() {
            if let Err(e)=write_atomic(&path,format!("shop_info={}\n",self.shop_info as u8).as_bytes()) {status(format!("Settings not saved: {e}"));}
        }
    }
    /// Party screens opened from the map: the first unit's status opens as
    /// soon as the roster has settled (its cursor moves there, then A). Return
    /// on a status screen leaves: the roster it goes back to is closed too.
    fn drive_party_flow(&mut self){
        let Some(layout)=self.menu_layout else {return};
        let bus=&mut self.machine.bus;
        if let Some(until)=self.party_leave {
            if bus.frames>until {self.party_leave=None;}
            else if layout.roster_live && battle::idle(bus) {battle::press(bus,0x3fd);self.party_leave=None;}
            return;
        }
        if let Some((key,until,settled))=self.party_bar {
            // The roster takes its keys a few frames after it is drawn.
            let settled=if layout.roster_live {settled+1} else {0};
            self.party_bar=Some((key,until,settled));
            if bus.frames>until {self.party_bar=None;}
            else if settled>=ROSTER_SETTLE && battle::idle(bus) {battle::press(bus,key);self.party_bar=None;}
            return;
        }
        if self.party_open_first && party_status(bus) {self.party_open_first=false;}
        if self.party_open_first && layout.roster_live && self.party_goal.is_none() {
            self.party_open_first=false;
            self.party_goal=Some(PartyGoal{target:0,confirm:true,deadline:bus.frames+600,pressed:None,cursor:[0;4],still:0,row:None,graph:None,list:None});
        }
    }
    /// The roster's hint bar (L Change, Start Item List, R Sort): on the roster
    /// its key; on a status screen back to the roster (B), then the key.
    pub(crate) fn party_bar(&mut self,key:u16,roster:bool){
        self.mouse.cancel();
        self.party_goal=None;
        self.party_open_first=false;
        if roster {battle::press(&mut self.machine.bus,key);}
        else if party_status(&self.machine.bus) {
            battle::press(&mut self.machine.bus,0x3fd);
            self.party_bar=Some((key,self.machine.bus.frames+300,0));
        }
    }
    /// Leaves the Party screens from a unit's status screen (`PARTY_CLOSE`).
    pub(crate) fn leave_party(&mut self){
        self.party_goal=None;
        self.party_open_first=false;
        battle::press(&mut self.machine.bus,0x3fd);
        self.party_leave=Some(self.machine.bus.frames+300);
    }
    /// Moves the game's unit selection (EWRAM 0xfd3e) to a tapped unit with
    /// its own Left/Right, one press at a time: the cursor and the unit screen
    /// animate and ignore input meanwhile, so the next press waits until the
    /// index has changed (or 30 frames passed) and the cursor has rested for 6
    /// frames. Then A, if asked, once it has rested for 10.
    fn drive_party_goal(&mut self){
        let Some(mut goal)=self.party_goal else {return};
        let bus=&mut self.machine.bus;
        // Goals belong to Party screens: they end when those do (never step on elsewhere).
        if bus.frames>goal.deadline || !bus.menu_screen.frame {self.party_goal=None;return;}
        if !battle::idle(bus) {return;}
        if let Some(target)=goal.graph {
            // The jump is made by the game's own routine; A once it has settled.
            if gba_core::ffta_graph_menu::pending(bus) {return;}
            let Some(menu)=gba_core::ffta_graph_menu::GraphMenu::read(bus) else {self.party_goal=None;return};
            if menu.selected!=target {self.party_goal=None;return;}
            goal.still+=1;
            self.party_goal=Some(goal);
            if goal.still<4 {return;}
            if goal.confirm {battle::press(bus,0x3fe);}
            self.party_goal=None;
            return;
        }
        if let Some((list,target))=goal.list {
            // The list's own routine jumps to the row; lists it does not move step.
            if gba_core::ffta_row_list::pending(bus) {return;}
            let Some(shown)=gba_core::ffta_row_list::RowList::all(bus).into_iter().find(|l|l.list==list) else {self.party_goal=None;return};
            goal.still=if goal.cursor[0]==shown.row {goal.still+1} else {0};
            goal.cursor[0]=shown.row;
            self.party_goal=Some(goal);
            if goal.still<4 {return;}
            if shown.row==target {if goal.confirm {battle::press(bus,0x3fe);} self.party_goal=None;return;}
            battle::press(bus,if target>shown.row {0x37f} else {0x3bf});
            goal.still=0;
            self.party_goal=Some(goal);
            return;
        }
        if let Some(row)=goal.row {
            // Menu rows: Up/Down until the cursor rests on the row, then A
            // (after a jump made by the list's own routine, usually none).
            let Some((_,y))=party_cursor(bus) else {self.party_goal=None;return};
            goal.still=if goal.cursor[0]==y as u8 {goal.still+1} else {0};
            goal.cursor[0]=y as u8;
            self.party_goal=Some(goal);
            if goal.still<4 {return;}
            if y==row {if goal.confirm {battle::press(bus,0x3fe);} self.party_goal=None;return;}
            battle::press(bus,if row>y {0x37f} else {0x3bf});
            goal.still=0;
            self.party_goal=Some(goal);
            return;
        }
        let index=bus.ewram.get(PARTY_INDEX).copied().unwrap_or(0) as usize;
        let cursor:[u8;4]=bus.ewram[0xfd30..0xfd34].try_into().unwrap_or([0;4]);
        goal.still=if cursor==goal.cursor {goal.still+1} else {0};
        goal.cursor=cursor;
        self.party_goal=Some(goal);
        if goal.still<6 {return;}
        if index==goal.target {
            if goal.confirm && goal.still<10 {return;}
            if goal.confirm {battle::press(bus,0x3fe);}
            self.party_goal=None;
            return;
        }
        if goal.pressed.is_some() {return;}
        // The game's selection is a row and column (index = row * 4 + column,
        // recomputed every frame on the roster). On the roster its cursor glides
        // straight to the unit set there; unit screens only switch on Left/Right,
        // so they start from the unit next to the target and make one switch.
        let set=|bus:&mut gba_core::mem::MemMap,unit:usize|{bus.ewram[PARTY_COLUMN]=(unit%4) as u8;bus.ewram[PARTY_COLUMN+1]=(unit/4) as u8;};
        if goal.confirm {
            set(bus,goal.target);
        } else {
            let from=if goal.target>0 {goal.target-1} else {goal.target+1};
            set(bus,from);
            battle::press(bus,if from<goal.target {0x3ef} else {0x3df});
        }
        goal.pressed=Some((index,bus.frames));
        self.party_goal=Some(goal);
    }
    /// A drag starting at screen point (x, y) (0..1) scrolls the party grid.
    pub fn party_drag_starts(&self,x:f32,y:f32)->bool{
        let Some(grid)=self.menu_layout.and_then(|l|l.grid) else {return false};
        let ny=(y*self.ui_window.1 as f32-self.world_rect[1])/self.world_rect[3]*mem::render_height() as f32-mem::extra_y() as f32;
        let (a,b)=grid.area();
        grid.max_scroll()>0 && x.is_finite() && ny>=a as f32 && ny<b as f32
    }
    /// Scrolls the party grid by a drag of `dy` (fraction of the screen height).
    pub fn party_drag(&mut self,dy:f32){
        if !dy.is_finite() {return;}
        let rows=dy*self.ui_window.1 as f32/self.world_rect[3]*mem::render_height() as f32;
        self.party_scroll=(self.party_scroll as f32-rows).round().max(0.) as i32;
    }
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
        // Portrait shows only the native 240 columns across the full width: the 8
        // extra columns each side hold wrapped scenery, and UI panels then share
        // the picture's scale. GameSurface draws the same rectangle.
        let portrait_crop=fill && sh>sw && extra_x<=8;
        let scale=if portrait_crop{sw/240.}else if fill{(sw/ww).max(sh/wh)}else{(sw/ww).min(sh/wh)};
        let (width,height)=((ww*scale) as i32,(wh*scale) as i32);
        // A shifted texture row r shows rendered row r + view_shift; map taps to the rendered row.
        let shift=self.view_shift as f32*height as f32/wh;
        self.world_rect=[((screen.0 as i32-width)/2) as f32,((screen.1 as i32-height)/2) as f32-shift,width as f32,height as f32];
    }
    pub fn new(rom: Vec<u8>) -> Self {
        let rom_cache_key=gba_core::ffta_rom::digest(&rom);
        let mouse=gba_core::ffta_mouse::LanguageMouse::new(&rom,false);
        Self { mouse,touch_enabled:false,ui_base:240.,ui_export_ns:0,ui_window:(240,160),ui_dock:None,town_menu:None,expanded:false,padded:false,world_map:Default::default(),menu_layout:None,party_cache:None,party_goal:None,party_scroll:0,party_shown:false,party_open_first:false,party_leave:None,party_bar:None,laws:Default::default(),rumor_close:None,shop_choice:(0,0),last_battery:None,resume_frame:0,menu_button:None,buttons:Vec::new(),shop_info:false,shop_info_pressed:0,shop_list_shown:0,gesture:None,free_home:None,system_return:false,free_return_after:None,shop_scroll:Default::default(),gesture_slop:48.,quantity_flash:(false,0),top_row:false,hud_luma:(0,0),view_shift:0,world_rect:[0.,0.,240.,160.],machine: Box::new(Machine::new(rom)), code: None, extensions: Vec::new(), pending: None,
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
        self.machine.bus.ffta_dialogue.docked=self.machine.bus.world_hud.enabled && self.ui_dock.is_some();
        self.machine.bus.menu_screen.enabled=self.machine.bus.ffta_dialogue.docked;
        // The shop's Info button (scene_buttons) stands in for the game's "R Info" hint.
        self.machine.bus.ffta_dialogue.hide_info_hint=self.touch_enabled && crate::game_buttons::available(&self.machine.bus.rom);
        self.audio.clear();
        let count=self.turbo_frames(count);
        self.pcm.begin(count.clamp(1,8),mode,
            self.cache_dir.as_deref().unwrap_or(&std::env::temp_dir()))?;
        let count=count.clamp(1,8);
        self.drive_gestures();
        self.shop_scroll_tick()?;
        self.laws_tick()?;
        self.drive_party_flow();
        self.drive_pub();
        self.track_free_home();
        self.drive_battle_ui();
        for index in 0..count {
            self.drive_party_goal();
            self.drive_dialogue_gesture();
            let m = &mut *self.machine;
            m.bus.skip_render=self.skip_hidden_frames && index+1<count;
            let keys=self.mouse.keys(&mut m.bus,(KEYS.load(Ordering::Relaxed) as u16)&0x3ff);
            m.bus.keys = gba_core::ffta_battle_mouse::keys(&mut m.bus,keys);
            self.step_frame()?;
            let m = &mut *self.machine;
            self.pcm.feed(&m.bus.audio_buf);
            m.bus.audio_buf.clear();
        }
        self.pcm.finish(&mut self.audio)?;
        let size = mem::render_width() * mem::render_height();
        self.pixels.resize(size, 0);
        let expanded_rows=mem::extra_y()>0 && (self.machine.bus.ffta_movement.enabled || self.machine.bus.ffta_stage.enabled)
            && (0..4).any(|bg|gba_core::ppu::ffta_terrain(&self.machine.bus,bg)
                || gba_core::ffta_stage::layer(&self.machine.bus,bg).is_some()
                || (self.machine.bus.world_hud.enabled && gba_core::ppu::ffta_world_map(&self.machine.bus,bg)))
            || (mem::extra_y()>0 && self.machine.bus.menu_screen.frame);
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
        let dock={
            let rh=mem::render_height() as f32;
            let s=rect[3]/rh;
            match self.ui_dock {
                Some((top,bottom)) if s>0. => (((top-rect[1])/s).ceil() as i32,((bottom-rect[1])/s).floor() as i32),
                _ => (0,rh as i32),
            }
        };
        if expanded_rows && self.machine.bus.world_hud.map_frame {
            // Touch-only: no L/R hints (there are no shoulder buttons), and the
            // map lower, under the laws (laws.rs).
            let touch=self.touch_enabled && self.machine.bus.ffta_dialogue.docked && crate::game_buttons::available(&self.machine.bus.rom);
            let drop=if touch {crate::world_map::WorldMap::drop_for(dock,dock.0+crate::laws::BANNER_ROWS)} else {0};
            self.world_map.present(&self.machine.bus,&mut self.pixels,dock,!touch,drop);
            self.view_shift=crate::world_map::WorldMap::shift(&self.machine.bus,drop);
        }
        self.menu_layout=if expanded_rows {crate::menu_layout::present(&self.machine.bus,&mut self.pixels,dock,&mut self.party_cache,&mut self.party_scroll)}else{None};
        let shown=self.menu_layout.is_some();
        // Their first frames can still read as a unit screen: a status screen
        // reached before the roster settles (resuming there) cancels it.
        if shown && !self.party_shown {self.party_open_first=true;}
        if !shown {self.party_open_first=false;self.party_leave=None;self.party_bar=None;}
        self.party_shown=shown;
        self.shop_scroll_present();
        self.draw_quantity_arrows();
        if expanded_rows {self.laws_present(dock);} else {self.laws.area=None;self.laws.banner=None;}
        let buttons=self.scene_buttons();
        let s=rect[3]/mem::render_height() as f32;
        self.laws.screen_area=self.laws.area.map(|(a,b)|(rect[1]+a as f32*s,rect[1]+b as f32*s));
        self.laws.screen_banner=self.laws.banner.map(|(a,b)|(rect[1]+a as f32*s,rect[1]+b as f32*s));
        self.laws.scale=s;
        self.top_row=buttons.iter().any(|b|b.top);
        self.buttons=buttons.into_iter().map(|button|{
            let offset=if button.top {dock.0+1-crate::game_buttons::ROW.0} else {dock.1-160};
            crate::game_buttons::draw(&self.machine.bus,&mut self.pixels,mem::render_width(),mem::render_height(),mem::extra(),offset,&button);
            let (y0,y1)=crate::game_buttons::ROW;
            let screen=[rect[0]+(mem::extra() as i32+button.x.0) as f32*s,rect[1]+(offset+y0) as f32*s,
                (button.x.1-button.x.0) as f32*s,(y1-y0) as f32*s];
            (button,screen)
        }).collect();
        self.sync_shop_info();
        // The app's own Menu button stands in only where the game's resources are missing.
        self.menu_button=if self.buttons.is_empty() {self.map_menu_button()} else {None};
        let state=&self.machine.bus.ffta_dialogue;
        if state.town_layout {
            if let Some(menu)=battle::MenuScene::read(&self.machine.bus) {self.shop_choice=(menu.count,menu.selected);}
        }
        if state.town_layout {self.town_menu=crate::touch::town_menu(state).or(self.town_menu);}
        else if !state.shop_layout {self.town_menu=None;}
        self.frame_ns = start.elapsed().as_nanos() as u64;
        if self.adaptive && self.machine.bus.frames % 120 < count as u64 { self.start_compilation(); }
        Ok(())
    }
    /// Runs the machine to the end of its frame with the keys already set
    /// (compiled blocks where there are, the interpreter elsewhere).
    pub(crate) fn step_frame(&mut self)->Result<(),String>{
        let m = &mut *self.machine;
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
    /// Persists progress. The battery save is written only when it changed,
    /// and the resume state first, so a resume state never holds an older
    /// in-game save than the battery file. Every 600 frames (about 10 s) the
    /// resume state is refreshed as well; `checkpoint` forces it.
    pub fn save(&mut self) -> Result<(), String> {
        let data = self.machine.bus.save_data().map(|d| d.to_vec());
        let changed = data.is_some() && data != self.last_battery;
        if changed || self.machine.bus.frames >= self.resume_frame + RESUME_INTERVAL { self.write_resume()?; }
        if let (true, Some(path), Some(data)) = (changed, &self.save_path, &data) {
            write_atomic(path, data)?;
        }
        if changed { self.last_battery = data; }
        Ok(())
    }
    /// The battery save as loaded from disk is already written.
    pub fn note_battery(&mut self) { self.last_battery = self.machine.bus.save_data().map(|d| d.to_vec()); }
    /// App paused, closing or leaving the game: resume state and battery save now.
    pub fn checkpoint(&mut self) -> Result<(), String> {
        self.write_resume()?;
        self.save()
    }
    fn resume_path(&self) -> Option<PathBuf> { self.save_path.as_ref().map(|p| p.with_extension("resume")) }
    fn write_resume(&mut self) -> Result<(), String> {
        if let Some(path) = self.resume_path() { write_atomic(&path, &self.save_state())?; }
        self.resume_frame = self.machine.bus.frames;
        Ok(())
    }
    /// Continues from the resume state next to the save file, if there is one
    /// for this ROM. A state that cannot be used is kept aside (`.resume.bad`)
    /// and the game starts from its battery save as before.
    pub fn resume(&mut self) -> Result<bool, String> {
        let Some(path) = self.resume_path().filter(|p| p.is_file()) else { return Ok(false) };
        let data = std::fs::read(&path).map_err(|e| format!("Read resume state: {e}"))?;
        if let Err(error) = self.load_state(&data) {
            let _ = std::fs::rename(&path, path.with_extension("resume.bad"));
            return Err(error);
        }
        self.last_battery = self.machine.bus.save_data().map(|d| d.to_vec());
        self.resume_frame = self.machine.bus.frames;
        Ok(true)
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

/// Frames between periodic resume states (about 10 s of play).
const RESUME_INTERVAL: u64 = 600;

/// Writes through a synced temporary file and a rename, so a crash or power
/// loss leaves either the old or the new file, never a torn one.
fn write_atomic(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let temp = path.with_extension(format!("{}.tmp", path.extension().and_then(|e| e.to_str()).unwrap_or("")));
    let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
    file.write_all(data).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&temp, path).map_err(|e| e.to_string())
}
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
        // Padding rows stay black, apart from the backdrop grid GameSurface samples
        // (never displayed) in the top-left 32x22 corner.
        assert_eq!(engine.pixels[40*256+100],0xff000000u32 as i32);
        assert_eq!(engine.pixels[1*256+1],0xffffffffu32 as i32);
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
