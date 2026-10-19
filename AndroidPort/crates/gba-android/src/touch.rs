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
fn dock_for(state:&dialogue::DialogueState,menu:Option<[i32;4]>,block:[i32;4],(y0,y1):(usize,usize))->Dock{
    // A speech bubble is anchored to a screen edge in the upper rows, also while
    // it opens or closes (shop lists open from the top rows but never touch an edge).
    let bubble=(block[0]<=0 || block[2]>=240) && y1<=72;
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
    canvas_blocks(&state.pixels).into_iter().filter(|(b,_)|b[2]>0 && b[0]<240).find_map(|(b,c)|match dock_for(state,None,b,c) {
        Dock::Thumb=>Some(b),Dock::Follow(m)=>Some(m),_=>None})
}

/// `menu`: the last town menu block (`town_menu`) while its scene lasts.
pub fn pieces(bus:&MemMap,band:Band,index:usize,menu:Option<[i32;4]>)->Vec<Layout>{
    let state=&bus.ffta_dialogue;
    let docked=band.h>160.*band.s+0.5;
    if docked && index==0 && !state.action_layout && state.layer().is_some() {
        // Blocks entirely beside the native screen are invisible on the original
        // (the shop parks stat digits at x -24) and must stay so.
        let blocks:Vec<_>=canvas_blocks(&state.pixels).into_iter().filter(|(b,_)|b[2]>0 && b[0]<240).collect();
        let placed:Vec<_>=blocks.iter().map(|&(b,c)|(b,dock_for(state,menu,b,c))).collect();
        return Layout::docked(band,state.style.scale,&placed);
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
            Some((top,bottom)) if self.expanded => Band::docked(self.ui_window,self.ui_base,top,bottom),
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
                handled=battle::aim(&mut self.machine.bus,nx,ny,true);
            }
        }else{
            let nx=(px-self.world_rect[0])/self.world_rect[2]*mem::render_width() as f32-mem::extra() as f32;
            let ny=(py-self.world_rect[1])/self.world_rect[3]*mem::render_height() as f32-mem::extra_y() as f32;
            if let Some(keys)=self.usa.then(||crate::title_touch::keys(&self.machine.bus,nx,ny)).flatten(){
                self.mouse.cancel();
                if !keys.is_empty(){battle::press_sequence(&mut self.machine.bus,&keys);}
                #[cfg(target_os="android")]
                eprintln!("FFTA_TOUCH title x={nx:.1} y={ny:.1} keys={keys:x?}");
                return;
            }
            handled=if self.mouse.scene(&self.machine.bus).is_some(){self.mouse.aim(&self.machine.bus,nx,ny,true)}
                else{battle::aim(&mut self.machine.bus,nx,ny,true)};
        }
        if !handled {self.mouse.cancel();battle::press(&mut self.machine.bus,0x3fe);}
        #[cfg(target_os="android")]
        eprintln!("FFTA_TOUCH click x={x:.3} y={y:.3} handled={handled}");
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
