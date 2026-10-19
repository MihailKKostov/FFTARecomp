//! Shared semantic click targets, with Android screen-space UI composition.
use gba_core::{ffta_battle_mouse as battle, ffta_dialogue::{self as dialogue,PanelKind}, mem::{self,MemMap}};
use crate::{engine::Engine,ui_layout::{Band,Layout}};

/// Up to this many clipped pieces are exported per frame (GameSurface slots).
pub const UI_SLOTS:usize=4;

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

pub fn canvas_regions(pixels:&[dialogue::UiPixel])->(Vec<[i32;4]>,usize){
    let width=dialogue::LAYER_WIDTH;
    let clusters=row_clusters(pixels);
    let mut regions=Vec::new();let mut top=0;
    for (index,&(y0,y1)) in clusters.iter().enumerate() {
        let column=|x:usize|(y0..y1).any(|y|pixels[y*width+x].visible());
        let mut x=0;
        while x<width {
            if !column(x){x+=1;continue;}
            let start=x;let mut end=x+1;let mut gap=0;x+=1;
            while x<width&&gap<8 {if column(x){end=x+1;gap=0}else{gap+=1}x+=1;}
            regions.push([start as i32-dialogue::LAYER_INSET,y0 as i32,end as i32-dialogue::LAYER_INSET,y1 as i32]);
            if index==0&&y0<40 {top+=1;}
        }
    }
    (regions,top)
}

/// Screen pieces of UI layer `index`. Portrait battle-style scenes split town,
/// shop and site canvases so the speaker stays on top and the choices sit in
/// the right thumb's reach; everything else is one piece.
/// `world` is the screen rectangle of the native 240x160 picture inside the world texture.
pub fn pieces(bus:&MemMap,band:Band,world:Band,index:usize)->Vec<Layout>{
    let state=&bus.ffta_dialogue;
    let docked=band.h>160.*band.s+0.5;
    if index==0 && docked && (state.town_layout||state.site_layout) && !state.shop_layout && state.layer().is_some() {
        let (regions,top)=canvas_regions(&state.pixels);
        if regions.len()>1 && regions.len()<=UI_SLOTS {return Layout::split(band,state.style.scale,&regions,top);}
    }
    // Shop lists (their stat digits are captured with the canvas, see ppu.rs
    // shop_numeral) dock to the bottom so the shop artwork above stays visible;
    // a separate dialogue-sized block at the top (the trader's reply) stays on top.
    if index==0 && docked && state.shop_layout {
        if let Some(bounds)=dialogue::pixel_bounds(&state.pixels) {
            let clusters=row_clusters(&state.pixels);
            let header=clusters.first().filter(|&&(y0,y1)|clusters.len()>1 && y0<8 && y1-y0<=72).copied();
            let Some((_,header_end))=header else {
                return vec![Layout::at_corner(band,false,true,state.style.scale,state.style.length,bounds)];
            };
            let rest_top=clusters[1].0 as i32;
            let head=Layout::split(band,state.style.scale,&[[bounds[0],0,bounds[2],header_end as i32]],1);
            let rest=[bounds[0],rest_top,bounds[2],bounds[3]];
            let mut lower=Layout::at_corner(band,false,true,state.style.scale,state.style.length,rest);
            let k=lower.rect[3]/160.;
            lower.clip=[lower.rect[0]+(rest[0]+120) as f32*k,lower.rect[1]+rest[1] as f32*k,
                (rest[2]-rest[0]) as f32*k,(rest[3]-rest[1]) as f32*k];
            return head.into_iter().chain([lower]).collect();
        }
    }
    let _=world;
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
            if let Some(source)=pieces(&self.machine.bus,band,self.world_band(),i).iter().find_map(|l|l.source(px,py)){
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
            let layouts=pieces(&self.machine.bus,band,self.world_band(),i);
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
