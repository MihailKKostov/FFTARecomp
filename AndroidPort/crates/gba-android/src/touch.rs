//! Shared semantic click targets, with Android screen-space UI composition.
use gba_core::{ffta_battle_mouse as battle, ffta_dialogue::{self as dialogue,PanelKind}, mem::{self,MemMap}};
use crate::{engine::Engine,ui_layout::{Band,Dock,Layout}};

/// Up to this many clipped pieces are exported per frame (GameSurface slots).
pub const UI_SLOTS:usize=8;

/// Visible blocks of a native UI canvas: row clusters split at empty rows,
/// then each split at empty columns. Native coordinates (x relative to the
/// 240-wide screen). Returns the blocks and how many belong to the top cluster.
/// Row ranges of a native UI canvas holding visible pixels, split at 4+ empty rows.
fn row_clusters(pixels:&[dialogue::UiPixel])->Vec<(usize,usize)>{
    let (width,height)=(dialogue::LAYER_WIDTH,pixels.len()/dialogue::LAYER_WIDTH);
    let row=|y:usize|pixels[y*width..(y+1)*width].iter().any(|p|p.visible());
    let mut clusters=Vec::new();let mut y=0;
    while y<height {
        if !row(y){y+=1;continue;}
        let start=y;let mut end=y+1;let mut gap=0;y+=1;
        while y<height&&gap<4 {if row(y){end=y+1;gap=0}else{gap+=1}y+=1;}
        clusters.push((start,end));
    }
    clusters
}

/// Visible blocks of a native UI canvas: row clusters split at empty columns.
/// Native coordinates (x relative to the 240-wide screen), with each block's
/// row cluster.
pub fn canvas_blocks(pixels:&[dialogue::UiPixel])->Vec<([i32;4],(usize,usize))>{
    let width=dialogue::LAYER_WIDTH;
    let mut blocks=Vec::new();
    for (y0,y1) in row_clusters(pixels) {
        let column=|x:usize|(y0..y1).any(|y|pixels[y*width+x].visible());
        let mut x=0;
        while x<width {
            if !column(x){x+=1;continue;}
            let start=x;let mut end=x+1;let mut gap=0;x+=1;
            while x<width&&gap<8 {if column(x){end=x+1;gap=0}else{gap+=1}x+=1;}
            blocks.push(([start as i32-dialogue::LAYER_INSET,y0 as i32,end as i32-dialogue::LAYER_INSET,y1 as i32],(y0,y1)));
        }
    }
    blocks
}

/// Where a block of the main canvas goes in a portrait docking frame. Decided
/// only by its native position, never by scene classification or its current
/// size, so opening animations and late classification cannot make it jump.
/// `lists`: a Buy or Sell list is up (the shop menu cannot be sliding then).
fn dock_for(state:&dialogue::DialogueState,menu:Option<[i32;4]>,lists:bool,block:[i32;4],(y0,y1):(usize,usize))->Dock{
    // A speech bubble is anchored to a screen edge in the upper rows, also while
    // it opens or closes (shop lists open from the top rows but never touch an
    // edge). Its continuation arrow bobs below it (measured: its bottom row 67
    // to 73), so one starting in the top rows is one whatever its bottom.
    let bubble=(block[0]<=0 || block[2]>=240) && (y1<=72 || y0<=16);
    if state.shop_layout && lists {
        // Inside the lists every window keeps its place (a panel closing at
        // the left edge is not the shop menu sliding).
        return if bubble {Dock::Top} else {Dock::Bottom};
    }
    if state.shop_layout {
        // The town menu sliding out of (or into) the left edge moves with its thumb
        // block; before one was seen, with the measured root window (shop, pub:
        // origin 8,92 = native x 0..96), which slides before the game reports it.
        let menu=menu.filter(|m|(block[1]-m[1]).abs()<=8).or_else(||
            (block[0]<=0 && (64..=100).contains(&block[1])).then_some([0,block[1],96,block[3]]));
        if let Some(m)=menu.filter(|m|block[0]<=m[0] && block[2]<=m[2]) {return Dock::Follow(m);}
        // Shop lists sit at the bottom; the trader's text stays on top.
        return if bubble {Dock::Top} else {Dock::Bottom};
    }
    if y0<8 || bubble {Dock::Top}
    else if block[2]<=120 && block[2]-block[0]<=128 {thumb(state,block)}
    else {Dock::Bottom}
}

/// A town menu still sliding in from the left edge moves with the thumb
/// placement of the complete window, as it slides on the original screen.
fn thumb(state:&dialogue::DialogueState,block:[i32;4])->Dock{
    match state.town_menu_x {
        Some((left,right)) if block[0]<=left && block[2]<right => Dock::Follow([left,block[1],right,block[3]]),
        _ => Dock::Thumb,
    }
}

/// Screen pieces of UI layer `index`. In portrait battle-style scenes the main
/// canvas is cut into blocks placed by `dock_for`; town menus drawn on the
/// extra layer use the same thumb placement. Everything else is one piece.
/// The town menu block of a town frame's main canvas (placed in the thumb zone).
pub fn town_menu(state:&dialogue::DialogueState)->Option<[i32;4]>{
    if !state.town_layout || state.layer().is_none() {return None;}
    canvas_blocks(&state.pixels).into_iter().filter(|(b,_)|b[2]>0 && b[0]<240).find_map(|(b,c)|match dock_for(state,None,false,b,c) {
        Dock::Thumb=>Some(b),Dock::Follow(m)=>Some(m),_=>None})
}

/// Rows of the Info window (`bounds`, its 8-row edges included) down to its
/// last text, in whole tiles with a margin, without its bottom edge.
/// Each row is read between its own edges: while the window slides, rows
/// below can be a step ahead of rows above, and it gets an edge at the
/// screen's right.
fn info_rows(pixels:&[dialogue::UiPixel],[l,t,r,b]:[i32;4])->i32{
    let at=|x:i32,y:i32|pixels.get((y*dialogue::LAYER_WIDTH as i32+x+dialogue::LAYER_INSET) as usize).copied();
    let shown=|x:i32,y:i32|at(x,y).is_some_and(|p|p.visible());
    let text=(t+8..b-8).rev().find(|&y|{
        let Some(left)=(l.min(128)..r.max(240)).find(|&x|shown(x,y)) else {return false};
        let right=(left..240).rev().find(|&x|shown(x,y)).unwrap_or(left)+1;
        let body=at(left+10,y).map(|p|p.color);
        (left+8..(left+96).min(right-8)).any(|x|at(x,y).is_some_and(|p|p.visible() && Some(p.color)!=body))
    });
    let last=text.map_or(t+24,|y|y+1).max(t+24);
    ((last+4-t+7)/8*8).min(b-8-t)
}

/// `menu`: the last town menu block (`town_menu`) while its scene lasts.
pub fn pieces(bus:&MemMap,band:Band,index:usize,menu:Option<[i32;4]>)->Vec<Layout>{
    let state=&bus.ffta_dialogue;
    let docked=band.h>160.*band.s+0.5;
    if docked && index==0 && !state.action_layout && state.layer().is_some() {
        // Blocks entirely beside the native screen are invisible on the original
        // (the shop parks stat digits at x -24) and must stay so.
        let blocks:Vec<_>=canvas_blocks(&state.pixels).into_iter().filter(|(b,_)|b[2]>0 && b[0]<240).collect();
        let lists=matches!(gba_core::ffta_shop_mouse::handler_step(bus),Some((1|2,step)) if step>=23);
        let placed:Vec<_>=blocks.iter().map(|&(b,c)|(b,dock_for(state,menu,lists,b,c))).collect();
        return Layout::docked(band,state.style.scale,&placed);
    }
    if docked && index==1 && state.shop_layout && state.extra.kind==PanelKind::Info {
        // The shop's item Info window at the top: its rows down to the last
        // text, then its bottom edge (the rest is empty window).
        if let Some([l,t,r,b])=state.extra.bounds {
            let rows=info_rows(&state.extra.pixels,[l,t,r,b]);
            return Layout::docked(band,state.style.scale,&[([l,t,r,t+rows],Dock::TopAt(0)),([l,b-8,r,b],Dock::TopAt(rows))]);
        }
    }
    if docked && index==1 && state.town_layout && state.extra.kind==PanelKind::Actions && state.extra.active() {
        if let Some(b)=dialogue::pixel_bounds(&state.extra.pixels) {
            return Layout::docked(band,state.style.scale,&[(b,thumb(state,b))]);
        }
    }
    layout(bus,band,index).into_iter().collect()
}

pub fn layout(bus:&MemMap,band:Band,index:usize)->Option<Layout>{
    let state=&bus.ffta_dialogue;
    let size=state.style.scale;
    if index==1 {
        if !state.separate || !state.extra.active(){return None;}
        if state.extra.kind==PanelKind::Notice {
            return Some(Layout::centered_panel(band,size,state.extra.bounds?));
        }
        let b=if state.extra.kind==PanelKind::Actions {dialogue::panel_bounds(&state.extra.pixels,&state.extra.backing)?}
            else{dialogue::pixel_bounds(&state.extra.pixels)?};
        return Some(Layout::at_corner(band,state.town_layout||b[0]+b[2]>=240,b[1]+b[3]>=160,size,state.style.length,b));
    }
    let (_,right)=state.layer()?;
    if state.action_layout {
        Some(Layout::at_corner(band,false,true,size,state.style.length,dialogue::panel_bounds(&state.pixels,&state.backing)?))
    }else if state.shop_layout||state.site_layout {
        Some(Layout::centered(band,size))
    }else{
        Some(Layout::new_with_anchor(band,if state.town_layout{true}else{right},size,
            [state.style.offset_x,state.style.offset_y],if state.town_layout{0.}else{state.vertical_anchor()}))
    }
}

#[cfg(test)]
mod tests {
    use crate::ui_layout::{Band,Layout};
    #[test]
    fn native_panel_keeps_uniform_screen_scale_at_every_world_zoom(){
        for screen in [(720,720),(1920,1080),(2560,720)] {
            for base in [240.,284.,568.] {
                for size in [0.5,1.,1.5,2.] {
                    let panel=Layout::new_with_anchor(Band::letterbox(screen,base),true,size,[0.,0.],160.);
                    assert!((panel.rect[2]/480.-panel.rect[3]/160.).abs()<0.0001);
                    for _zoom in 0..=6 {
                        // World texture dimensions are deliberately absent from UI layout.
                        let at_zoom=Layout::new_with_anchor(Band::letterbox(screen,base),true,size,[0.,0.],160.);
                        assert_eq!(panel.rect,at_zoom.rect);
                    }
                    let bottom=panel.rect[1]+panel.rect[3];
                    assert!(bottom<=screen.1 as f32+0.51);
                }
            }
        }
    }
    #[test]
    fn disabled_touch_never_changes_guest_buttons(){
        let mut engine=crate::engine::Engine::new(vec![0;192]);
        let keys=engine.machine.bus.keys;
        engine.tap(0.5,0.5,false);
        engine.tap(f32::NAN,0.5,false);
        engine.tap(0.5,0.5,true);
        assert_eq!(engine.machine.bus.keys,keys);
        assert!(!engine.machine.bus.ffta_battle_mouse.enabled);
    }
}
impl Engine {
    /// Portrait battles dock panels to the frame Kotlin reserves; every other
    /// scene keeps the letterboxed 160-row band that matches the game picture.
    /// Where the native 240x160 picture is drawn inside the world texture.
    pub fn world_band(&self)->Band{
        let s=self.world_rect[2]/mem::render_width() as f32;
        Band{s,x:self.world_rect[0]+mem::extra() as f32*s,y:self.world_rect[1]+mem::extra_y() as f32*s,w:240.*s,h:160.*s}
    }
    pub fn ui_band(&self)->Band{
        match self.ui_dock {
            Some((top,bottom)) if self.expanded => {
                // Town scenes keep the bottom HUD row free for the game-style
                // buttons, and the top row for setting buttons when shown.
                let row=crate::game_buttons::ROW_HEIGHT as f32*self.world_rect[2]/mem::render_width() as f32;
                let town=self.town_buttons();
                Band::docked(self.ui_window,self.ui_base,top+if town && self.top_row {row} else {0.},bottom-if town {row} else {0.})
            }
            _ => Band::letterbox(self.ui_window,self.ui_base),
        }
    }
    pub fn configure_touch(&mut self,enabled:bool,base:f32,size:f32,length:f32){
        if enabled!=self.touch_enabled {
            self.touch_enabled=enabled;
            self.mouse=gba_core::ffta_mouse::LanguageMouse::new(&self.machine.bus.rom,enabled);
            self.machine.bus.ffta_battle_mouse.set_enabled(enabled);
        }
        // Square/portrait devices still need the complete 240-pixel native UI.
        self.ui_base=if base.is_finite(){base.clamp(240.,1536.)}else{240.};
        self.machine.bus.ffta_dialogue.separate=true;
        self.machine.bus.ffta_dialogue.style.scale=if size.is_finite(){size.clamp(0.5,2.)}else{1.};
        self.machine.bus.ffta_dialogue.style.length=if length.is_finite(){length.clamp(1.,2.)}else{1.};
    }
    pub fn tap(&mut self,x:f32,y:f32,back:bool){
        if !self.touch_enabled || !x.is_finite() || !y.is_finite() || !(0. ..1.).contains(&x) || !(0. ..1.).contains(&y){return;}
        if back {self.mouse.cancel();battle::back(&mut self.machine.bus);return;}
        let px=x*self.ui_window.0 as f32;let py=y*self.ui_window.1 as f32;
        // Buttons drawn into the game picture (game_buttons.rs).
        if let Some(key)=self.buttons.iter().find(|(_,[bx,by,bw,bh])|px>=*bx && px<bx+bw && py>=*by && py<by+bh).map(|(b,_)|b.key) {
            self.mouse.cancel();
            self.party_goal=None;
            if key==0 {self.toggle_shop_info();}
            // The game is still moving to the shop list's shown rows: the button waits for it.
            else if self.shop_scroll.catching_up() {self.shop_scroll.deferred=Some(key);}
            else {self.press_button(key);}
            return;
        }
        // The shown laws open and close the whole list; with it open, any tap closes it.
        if self.laws.screen_banner.is_some_and(|(a,b)|py>=a && py<b) {self.mouse.cancel();self.laws.toggle();return;}
        if self.laws.open {self.laws.open=false;return;}
        if self.quantity_tap(x,y) {return;}
        // Dialogue waiting for a button moves on with a tap anywhere, also with a menu shown.
        if self.touch_enabled && self.machine.bus.ffta_dialogue.docked && crate::pub_touch::continue_arrow(&self.machine.bus) {
            self.mouse.cancel();battle::press(&mut self.machine.bus,0x3fe);return;
        }
        let band=self.ui_band();
        let state=&self.machine.bus.ffta_dialogue;
        let index=if matches!(state.extra.kind,PanelKind::Actions|PanelKind::Notice)&&state.extra.active(){Some(1)}
            else if state.action_layout||state.town_layout||state.shop_layout {Some(0)}else{None};
        let mut handled=false;
        if (battle::MenuScene::read(&self.machine.bus).is_some()||state.shop_layout)&&index.is_some(){
            let i=index.unwrap();
            if let Some(source)=pieces(&self.machine.bus,band,i,self.town_menu).iter().find_map(|l|l.source(px,py)){
                let mut nx=(source%480) as f32-120.;let ny=(source/480) as f32;
                if !(state.town_layout||state.shop_layout||state.site_layout||state.extra.kind==PanelKind::Notice){
                    let bounds=if i==0{dialogue::panel_bounds(&state.pixels,&state.backing)}else{dialogue::panel_bounds(&state.extra.pixels,&state.extra.backing)};
                    if let Some(b)=bounds{
                        let d=((b[2]-b[0]) as f32*(state.style.length.clamp(1.,2.)-1.)).round();
                        nx+=if b[0]+b[2]>=240{d}else{-d};
                    }
                }
                if self.touch_enabled && self.machine.bus.ffta_dialogue.docked && self.notice_tap(nx,ny) {return;}
                if self.town_buttons() && self.pub_tap(nx,ny) {return;}
                if self.shop_list_tap(nx,ny) {return;}
                handled=battle::aim(&mut self.machine.bus,nx,ny,true);
                if !handled && self.town_buttons() && self.pointer_menu_tap(nx,ny) {return;}
            }
        }else{
            let nx=(px-self.world_rect[0])/self.world_rect[2]*mem::render_width() as f32-mem::extra() as f32;
            let ny=(py-self.world_rect[1])/self.world_rect[3]*mem::render_height() as f32-mem::extra_y() as f32;
            // Portrait Party screens: the party above, the unit's screen below; taps between do nothing.
            let ny=match self.menu_layout {
                Some(layout)=>match layout.hit(nx,ny) {
                    crate::menu_layout::Hit::Unit(i)=>{self.party_tap(&layout,i);return}
                    crate::menu_layout::Hit::Bar(key)=>{
                        // The roster's own hint bar: L Change, Start Item List, R Sort.
                        self.party_bar(key,layout.roster_live);
                        return
                    }
                    crate::menu_layout::Hit::Lower(y)=>{
                        if layout.template!=gba_core::menu_screen::Template::Roster && self.party_menu_tap(nx,y) {return}
                        y
                    }
                    crate::menu_layout::Hit::None=>return,
                },
                None=>ny,
            };
            if self.touch_enabled && self.machine.bus.ffta_dialogue.docked && self.notice_tap(nx,ny) {return;}
            if let Some(keys)=self.usa.then(||crate::title_touch::keys(&self.machine.bus,nx,ny).or_else(||crate::title_touch::slot_keys(&self.machine.bus,nx,ny))).flatten(){
                self.mouse.cancel();
                if !keys.is_empty(){battle::press_sequence(&mut self.machine.bus,&keys);}
                #[cfg(target_os="android")]
                eprintln!("FFTA_TOUCH title x={nx:.1} y={ny:.1} keys={keys:x?}");
                return;
            }
            handled=if self.mouse.scene(&self.machine.bus).is_some(){self.mouse.aim(&self.machine.bus,nx,ny,true)}
                else{battle::aim(&mut self.machine.bus,nx,ny,true)};
        }
        // A tap on nothing confirms (A) only where there is nothing to choose
        // (dialogue, messages): see `blind_confirm`.
        if !handled && self.blind_confirm() {
            self.mouse.cancel();battle::press(&mut self.machine.bus,0x3fe);
        }
        #[cfg(target_os="android")]
        eprintln!("FFTA_TOUCH click x={x:.3} y={y:.3} handled={handled}");
    }
    /// A tap that hits nothing presses A (moves dialogue and messages on).
    /// Touch-only portrait: not where the player chooses by tapping, which a
    /// blind A would choose for them: any game menu (its pointer, OAM tiles
    /// 0x120-0x127, or a menu the touch adapters know), the Party screens,
    /// shops (but a waiting message) and the world map (but a message over
    /// it with no menu). Landscape, with no A button, keeps it everywhere.
    pub(crate) fn blind_confirm(&self)->bool{
        let bus=&self.machine.bus;
        if !(self.touch_enabled && bus.ffta_dialogue.docked) {return true;}
        if let Some(phase)=gba_core::ffta_shop_mouse::phase(bus).filter(|_|self.town_buttons()) {
            return phase==gba_core::ffta_shop_mouse::ShopPhase::Message;
        }
        let menu=self.menu_layout.is_some() || crate::engine::party_cursor(bus).is_some()
            || battle::MenuScene::read(bus).is_some() || gba_core::ffta_graph_menu::GraphMenu::read(bus).is_some();
        if menu {return false;}
        if bus.world_hud.map_frame {return bus.world_hud.menu;}
        true
    }
    /// A game-style button's key. B only closes the shop's item Info window:
    /// close it, then go back.
    pub(crate) fn press_button(&mut self,key:u16){
        if key==crate::engine::PARTY_CLOSE {self.leave_party();}
        else if key==crate::pub_touch::CLOSE_RUMOR {self.close_rumor();}
        else if key==crate::pub_touch::YES {self.notice_yes();}
        else if key==0x3fd && gba_core::ppu::shop_info_open(&self.machine.bus) {battle::press_sequence(&mut self.machine.bus,&[0x2ff,0x3fd]);}
        else {battle::press(&mut self.machine.bus,key);}
    }
    /// Party menus and lists (MENU, EQUIP, ITEMS, ABILITIES): the game's own
    /// cursor (OAM tiles 0x120-0x127, priority 0; with two lists shown the
    /// later one is active) sits beside the selected row, rows 16 apart. A tap
    /// on a row with text in the cursor's column moves it with Up/Down and
    /// chooses with A, like the other touch menus. Returns false (the caller
    /// falls back) when there is no cursor.
    fn party_menu_tap(&mut self,x:f32,y:f32)->bool{
        // Menus moved by the game's cursor graph (MENU, EQUIP): straight to the entry, then A.
        if let (Some(menu),Some((cx,cy)))=(gba_core::ffta_graph_menu::GraphMenu::read(&self.machine.bus),crate::engine::party_cursor(&self.machine.bus)) {
            // Entry positions are the menu's own; the visible cursor places them on screen.
            let at=menu.entry(&self.machine.bus,menu.selected);
            let Some(target)=menu.hit(&self.machine.bus,x-(cx-at.x) as f32,y-(cy-at.y) as f32) else {return true};
            self.mouse.cancel();
            if gba_core::ffta_graph_menu::select(&mut self.machine.bus,target) {
                self.party_goal=Some(crate::engine::PartyGoal{target:0,confirm:true,deadline:self.machine.bus.frames+120,
                    pressed:None,cursor:[0;4],still:0,row:None,graph:Some(target),list:None});
                return true;
            }
        }
        // The game's text lists (items, abilities; some show no pointer): straight to the row, then A.
        if let Some((list,row))=gba_core::ffta_row_list::RowList::at(&self.machine.bus,x,y) {
            self.mouse.cancel();
            gba_core::ffta_row_list::select(&mut self.machine.bus,row as i32-list.row as i32);
            self.party_goal=Some(crate::engine::PartyGoal{target:0,confirm:true,deadline:self.machine.bus.frames+300,
                pressed:None,cursor:[list.row,0,0,0],still:0,row:None,graph:None,list:Some((list.list,row))});
            return true;
        }
        let bus=&self.machine.bus;
        let Some((cx,cy))=crate::engine::party_cursor(bus) else {return false};
        if x<(cx-8) as f32 || x>(cx+120) as f32 {return true;}
        let steps=((y-(cy-4) as f32)/16.).floor() as i32;
        // The tapped row must hold text right of the cursor (both groups: the
        // unit screen is drawn in one piece).
        let row=cy-4+steps*16;
        let groups=&bus.menu_screen.groups;
        let text=(row.max(0)..(row+16).min(160)).any(|r|(cx+8..(cx+100).min(240)).any(|c|{
            (0..2).any(|g|groups.get(g*240*160+(r*240+c) as usize).is_some_and(|&p|p!=gba_core::menu_screen::TRANSPARENT))
        }));
        if !text {return true;}
        self.mouse.cancel();
        self.party_goal=Some(crate::engine::PartyGoal{target:0,confirm:true,deadline:self.machine.bus.frames+600,
            pressed:None,cursor:[0;4],still:0,row:Some(row+4),graph:None,list:None});
        true
    }
    /// Shop Buy and Sell lists: a tap on an item selects it (the game's cursor
    /// routine jumps straight there, so its Info and stats follow); a tap on
    /// the selected item chooses it (A), like the Buy/Sell button. Returns
    /// false off the list's rows (tabs, other windows).
    fn shop_list_tap(&mut self,x:f32,y:f32)->bool{
        // The list (not its blinking pointer) decides.
        if gba_core::ffta_shop_mouse::shop_list(&self.machine.bus).is_none() && crate::engine::shop_list_cursor(&self.machine.bus).is_none() {return false;}
        let Some(scene)=battle::MenuScene::read(&self.machine.bus) else {return false};
        let Some(target)=scene.hit(x,y) else {return false};
        // Choosing takes a tap well inside the selected row, not on its edges.
        let within=y-(scene.origin[1]+target as i32*16) as f32;
        if target==scene.selected && !(3. ..12.).contains(&within) {return true;}
        self.mouse.cancel();
        if target==scene.selected {battle::press(&mut self.machine.bus,0x3fe);}
        // Straight to the item (the game's own Up/Down, entered with that row).
        else if !gba_core::ffta_shop_mouse::select_row(&mut self.machine.bus,target) {battle::menu_select(&mut self.machine.bus,target,false);}
        true
    }
    /// A tap on a unit in the party picture: on the roster the game's cursor
    /// moves there and A opens the unit; on unit screens the game's own
    /// Left/Right unit cycling switches to it.
    fn party_tap(&mut self,layout:&crate::menu_layout::MenuLayout,target:usize){
        use gba_core::menu_screen::Template;
        if !self.party_cache.as_ref().is_some_and(|cache|cache.has_unit(target)) {return;}
        let confirm=match layout.template {
            Template::Roster if layout.roster_live => true,
            Template::Unit => false,
            _ => return,
        };
        self.mouse.cancel();
        self.party_goal=Some(crate::engine::PartyGoal{target,confirm,deadline:self.machine.bus.frames+600,pressed:None,cursor:[0;4],still:0,row:None,graph:None,list:None});
    }
    pub fn export_ui(&self,packed:&mut [u8],params:&mut [f32],background:&mut [i32]){
        // Slots of 9 floats: rect, clip, layer + 2 when it blends over the stats background.
        params.fill(0.);
        if let Some(flag)=params.get_mut(UI_SLOTS*9){*flag=if self.padded{1.}else{0.};}
        let band=self.ui_band();
        let mut slot=0;
        for i in 0..2 {
            let layouts=pieces(&self.machine.bus,band,i,self.town_menu);
            if layouts.is_empty(){continue;}
            let state=&self.machine.bus.ffta_dialogue;
            let pixels=if i==0{&state.pixels}else{&state.extra.pixels};
            let stats=i==1&&state.extra.kind==PanelKind::Stats;
            for layout in layouts.iter().take(UI_SLOTS-slot) {
                params[slot*9..slot*9+4].copy_from_slice(&layout.rect);
                params[slot*9+4..slot*9+8].copy_from_slice(&layout.clip);
                params[slot*9+8]=i as f32+if stats{2.}else{0.};
                slot+=1;
            }
            let offset=i*480*160*4;
            for (target,pixel) in packed[offset..offset+480*160*4].chunks_exact_mut(4).zip(pixels){target.copy_from_slice(&pixel.packed());}
            if stats {
                for(out,&pixel)in background.iter_mut().zip(&state.extra.background){*out=super::engine::rgba_colors()[(pixel&0x7fff) as usize];}
            }
        }
    }
}

#[cfg(test)]
mod dock_tests {
    use super::*;
    #[test]
    fn a_top_bubble_stays_on_top_while_its_arrow_bobs_below_it() {
        let mut state=dialogue::DialogueState::default();
        state.shop_layout=true;
        // The barkeep's bubble (right edge, rows 1-67), its arrow down to row 73.
        for bottom in [67,72,73] {
            assert_eq!(dock_for(&state,None,false,[3,1,240,bottom],(1,bottom as usize)),Dock::Top);
        }
        // Speakers' bubbles low on the screen and the shop's lists stay below.
        assert_eq!(dock_for(&state,None,false,[0,104,180,152],(104,152)),Dock::Bottom);
        assert_eq!(dock_for(&state,None,false,[8,10,232,120],(10,120)),Dock::Bottom);
    }
}
