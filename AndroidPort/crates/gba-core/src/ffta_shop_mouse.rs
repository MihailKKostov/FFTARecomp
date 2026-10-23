//! Mouse targets from the original shop category widget, not translated labels.
use crate::{ffta_rom::Version,mem::MemMap,Machine};
#[derive(Clone,Copy)]
struct Profile { start:u32,commit:u32,end:u32,inventory:u32,handlers:[u32;2],
    /// The shop menu's handler, where its steps were measured (USA).
    menu:Option<u32> }
#[derive(Clone,Copy)]
struct Request { base:usize,slot:u8,source:u8,ids:[u8;6],count:u8,deadline:u64,committed:Option<u64> }
#[derive(Default)]
pub(crate) struct State { profile:Option<Profile>,request:Option<Request>,deferred:Option<(f32,f32,bool)>,
    /// A quantity being set (`set_quantity`); kept through the menu cancels, ended by physical input.
    pub(crate) quantity:Option<QuantityRequest>,
    /// A shop list row being selected (`select_row`), likewise.
    pub(crate) row:Option<RowRequest> }
#[derive(Clone,Copy)]
pub(crate) struct RowRequest { cursor:usize, target:u8, deadline:u64, committed:bool }
#[derive(Clone,Copy)]
pub(crate) struct QuantityRequest { target:u8, deadline:u64, committed:bool }
impl State {
    pub(crate) fn new(rom:&[u8],version:Version)->Self {
        let start=match version {Version::Europe=>0x71490,Version::Usa=>0x6efa8};
        let inventory=match version {Version::Europe=>0x71030,Version::Usa=>0x6eb48};
        if rom.get(start..start+12)!=Some(&[0x00,0xb5,0x02,0x1c,0x09,0x04,0x09,0x0c,0x10,0x29,0x09,0xd0])
            || rom.get(start+0x30..start+0x34)!=Some(&[0x10,0x70,0x10,0x1c])
            || rom.get(inventory..inventory+12)!=Some(&[0xf0,0xb5,0x87,0xb0,6,0x1c,0x11,0x4f,0x39,0x68,0x11,0x4a]) {return Self::default();}
        Self {profile:Some(Profile{start:0x08000000+start as u32,commit:0x08000030+start as u32,
            end:0x0800003c+start as u32,inventory:0x08000001+inventory as u32,
            handlers:match version {Version::Usa=>[0x08069659,0x0806b6d1],Version::Europe=>[0x0806bb0d,0x0806db7d]},
            menu:(version==Version::Usa && rom.get(0x68c10..0x68c18)==Some(&[0xf0,0xb5,0x4f,0x46,0x46,0x46,0xc0,0xb4])).then_some(0x08068c11)}),
            request:None,deferred:None,quantity:None,row:None}
    }
    pub(crate) fn cancel(&mut self) {self.request=None;self.deferred=None;}
    pub(crate) fn pending(&self)->bool {self.request.is_some()}
    pub(crate) fn interprets(&self,pc:u32)->bool {
        (self.request.is_some() && self.profile.is_some_and(|p|(p.start..p.end).contains(&pc)))
            || (self.quantity.is_some_and(|q|!q.committed) && (QUANTITY_CODE.0..QUANTITY_CODE.1).contains(&pc))
            || (self.row.is_some_and(|r|!r.committed) && (ROW_CODE.0..ROW_CODE.1).contains(&pc))
    }
}
fn half(b:&[u8],o:usize)->Option<u16>{Some(u16::from_le_bytes(b.get(o..o+2)?.try_into().ok()?))}
fn word(b:&[u8],o:usize)->Option<u32>{Some(u32::from_le_bytes(b.get(o..o+4)?.try_into().ok()?))}
#[derive(Clone,Copy)]
struct Context {base:usize,count:u8,current:u8,ids:[u8;6],x:i32,y:i32,overlay:bool,cursor:usize}
fn context(mem:&MemMap)->Option<Context> {
    let p=mem.ffta_battle_mouse.shop.profile?;
    if half(&mem.io,0)?&0x0f07!=0x0f00 || [8,10,12,14].map(|o|half(&mem.io,o))
        !=[Some(0x4c03),Some(0x4e02),Some(0x0b08),Some(0x0a08)] {return None;}
    let manager=word(&mem.ewram,0xf428)?.checked_sub(0x02000000)? as usize;
    let cursor=manager.checked_add(0x14e0)?;
    if !p.handlers.contains(&word(&mem.ewram,cursor.checked_sub(0x54)?)?) || word(&mem.ewram,cursor)?!=p.inventory
        || word(&mem.ewram,cursor+4)?!=0x02000000+cursor as u32-0x20 {return None;}
    let base=manager.checked_add(0x4470)?;
    let count=*mem.ewram.get(base+7)?;let current=*mem.ewram.get(base)?;
    if count==0 || count>6 || current>=count {return None;}
    let mut ids=[255;6];ids[..count as usize].copy_from_slice(mem.ewram.get(base+1..base+1+count as usize)?);
    for i in 0..count as usize {if ids[i]>5 || ids[..i].contains(&ids[i]){return None;}}
    let x=*mem.ewram.get(base+0x78)? as i32*8;
    let y=*mem.ewram.get(base+0x79)? as i32*8;
    if x+count as i32*16>240 || y+16>160 {return None;}
    // WIN1 frames the quantity window; the item Info window uses it too, and
    // leaves it on, empty (240-240), after it closes.
    let win1=half(&mem.io,0x42)?;
    let overlay=half(&mem.io,0)?&0x4000!=0 && win1>>8<win1&255 && !crate::ppu::shop_info_open(mem);
    Some(Context{base,count,current,ids,x,y,overlay,cursor})
}

/// Where the shop is, from its running handler and that handler's step
/// (manager [0x0200f428] +0x148c handler, +0x1490 step; measured USA):
/// the menu rests at 5; Buy and Sell lists at 23 (60-63: the game toggling
/// item Info), quantity at 43, the price confirmation at 102, and wait for a
/// button after a message at 103 (Thanks) and 123 (cannot sell equipped).
/// Every other step is a transition.
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum ShopPhase { Menu, List{sell:bool}, Quantity{sell:bool}, Confirm{sell:bool}, Message, Busy }
/// The running shop handler (0 the menu, 1 Buy, 2 Sell) and its step.
pub fn handler_step(mem:&MemMap)->Option<(u8,u8)> {
    let p=mem.ffta_battle_mouse.shop.profile?;
    let menu=p.menu?;
    let manager=word(&mem.ewram,0xf428)?.checked_sub(0x02000000)? as usize;
    let handler=word(&mem.ewram,manager.checked_add(0x148c)?)?;
    let step=*mem.ewram.get(manager+0x1490)?;
    if handler==menu {return Some((0,step));}
    Some((1+p.handlers.iter().position(|&h|h==handler)? as u8,step))
}
pub fn phase(mem:&MemMap)->Option<ShopPhase> {
    let p=mem.ffta_battle_mouse.shop.profile?;
    let menu=p.menu?;
    let manager=word(&mem.ewram,0xf428)?.checked_sub(0x02000000)? as usize;
    let handler=word(&mem.ewram,manager.checked_add(0x148c)?)?;
    let step=*mem.ewram.get(manager+0x1490)?;
    if handler==menu {return Some(if step==5 {ShopPhase::Menu}else{ShopPhase::Busy});}
    let sell=match p.handlers.iter().position(|&h|h==handler)? {0=>false,_=>true};
    Some(match step {
        23|60..=63=>ShopPhase::List{sell},
        43=>ShopPhase::Quantity{sell},
        102=>ShopPhase::Confirm{sell},
        103|123=>ShopPhase::Message,
        _=>ShopPhase::Busy,
    })
}
pub(crate) fn active(mem:&MemMap)->bool {context(mem).is_some()}
pub fn select_category(mem:&mut MemMap,slot:u8)->bool {
    if !mem.ffta_battle_mouse.enabled {return false;}
    let Some(c)=context(mem)else{return false;};
    if c.overlay || slot>=c.count {return false;}
    if mem.ffta_battle_mouse.shop.request.is_some_and(|r|r.base==c.base&&r.slot==slot&&r.ids==c.ids){return true;}
    mem.ffta_battle_mouse.cancel();
    if slot!=c.current {
        mem.ffta_battle_mouse.shop.request=Some(Request{base:c.base,slot,source:c.current,ids:c.ids,count:c.count,
            deadline:mem.frames+120,committed:None});
        mem.ffta_battle_mouse.record_shop_event(1024+slot as u16);
    }
    true
}
pub(crate) fn aim(mem:&mut MemMap,x:f32,y:f32,confirm:bool)->bool {
    if !mem.ffta_battle_mouse.enabled {return false;}
    let Some(c)=context(mem)else{return false;};
    if c.overlay {
        // Quantity uses an original digit cursor below the numeric field.
        // Click tens/units to choose a digit; wheel sends its original Up/Down.
        if confirm && half(&mem.io,0).is_some_and(|v|v&0x6000==0x4000) {
            let h=half(&mem.io,0x42).unwrap_or(0);let v=half(&mem.io,0x46).unwrap_or(0);
            let right=(h&255)as i32;let top=(v>>8)as i32;let bottom=(v&255)as i32;
            let column=(x/8.).floor()as i32*8;
            if y>=top as f32 && y<bottom as f32 && (column==right-16 || column==right-24) {
                let current=mem.oam.chunks_exact(8).find_map(|o| {
                    (half(o,4)==Some(0x112f)).then(||half(o,2).unwrap_or(0)&511)
                });
                if let Some(current)=current {
                    if current as i32!=column {mem.ffta_battle_mouse.queue_shop_key(if column==right-24 {0x3df}else{0x3ef});}
                    return true;
                }
            }
        }
        return false;
    }
    if !x.is_finite() || !y.is_finite(){return false;}
    if y>=c.y as f32 && y<(c.y+16)as f32 && x>=c.x as f32 && x<(c.x+6*16).min(240)as f32 {
        // Measured BG3 tile rows: Sell packs its live IDs contiguously,
        // e.g. IDs [1,2,3,5] are drawn at X=16,32,48,64, not 32,48,64,96.
        let slot=((x-c.x as f32)/16.)as u8;
        if slot<c.count {select_category(mem,slot);}else{mem.ffta_battle_mouse.cancel();}
        return true;
    }
    if mem.ffta_battle_mouse.shop.pending() {
        // Preserve the latest pointer/click until the new category has really
        // published its list. Never select an item from the previous category.
        mem.ffta_battle_mouse.shop.deferred=Some((x,y,confirm));return true;
    }
    let list=c.cursor-0x20;
    let top=half(&mem.ewram,c.cursor+10).unwrap_or(160)as f32;
    let capacity=mem.ewram[list+0x16].min(8)as f32;
    if x>=0. && x<240. && y>=top && y<top+capacity*16.
        && crate::ffta_battle_mouse::MenuScene::shop_inventory(mem).and_then(|s|s.hit(x,y)).is_none() {
        mem.ffta_battle_mouse.cancel();return true;
    }
    false
}
pub(crate) fn keys(mem:&mut MemMap)->Option<u16> {
    let r=mem.ffta_battle_mouse.shop.request?;
    let valid=context(mem).is_some_and(|c|!c.overlay&&c.base==r.base&&c.count==r.count&&c.ids==r.ids);
    if !valid || mem.frames>r.deadline {mem.ffta_battle_mouse.shop.cancel();return None;}
    if let Some(frame)=r.committed {
        if mem.frames>=frame+2 && context(mem).is_some_and(|c|c.current==r.slot) {
            let deferred=mem.ffta_battle_mouse.shop.deferred.take();
            mem.ffta_battle_mouse.shop.cancel();
            if let Some((x,y,confirm))=deferred {crate::ffta_battle_mouse::aim(mem,x,y,confirm);}
        }
        return Some(0x3ff);
    }
    Some(0x3ef) // wake the original Right branch once; its store receives the target
}
/// A shop list as shown: the list (EWRAM), its entries, the first shown, rows
/// shown, the selected row; its first row's native top-left and right edge;
/// its category (id) and whether it is the Sell list.
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct ShopList { pub list:usize, pub count:u16, pub first:u16, pub visible:u8, pub row:u8,
    pub origin:[i32;2], pub right:i32, pub category:u8, pub sell:bool }
pub fn shop_list(mem:&MemMap)->Option<ShopList> {
    let Some(ShopPhase::List{sell})=phase(mem) else {return None};
    let c=context(mem)?;
    let scene=crate::ffta_battle_mouse::MenuScene::shop_inventory(mem)?;
    let list=c.cursor-0x20;
    if scene.base!=c.cursor {return None;}
    let count=half(&mem.ewram,list+0x18)?;let first=half(&mem.ewram,list+0x1a)?;
    let visible=*mem.ewram.get(list+0x16)?;let row=*mem.ewram.get(c.cursor+0x11)?;
    if visible==0 || first as u32+row as u32>=count as u32 {return None;}
    Some(ShopList{list,count,first,visible,row,origin:scene.origin,right:scene.right,category:c.ids[c.current as usize],sell})
}
/// Category tabs of the shown list: the current tab and how many.
pub fn categories(mem:&MemMap)->Option<(u8,u8)> {let c=context(mem)?;(!c.overlay).then_some((c.current,c.count))}

/// The quantity window (USA, shop manager +0x395c): the number at +0x79, the
/// most allowed (stock, Clan Funds) at +0x7a, at +0x7c whether Up/Down move
/// it by 10. Its Up and Down store the new number at 0x0802a7a4/0x0802a7e6
/// (r0 the number, r6 its address), then keep it within 1 and the most.
const QUANTITY:usize=0x395c;
const QUANTITY_STORES:[u32;2]=[0x0802a7a4,0x0802a7e6];
const QUANTITY_CODE:(u32,u32)=(0x0802a780,0x0802a7f4);
fn quantity_address(mem:&MemMap)->Option<usize> {
    let p=mem.ffta_battle_mouse.shop.profile?;p.menu?;
    if mem.rom.get(0x2a7a0..0x2a7a6)!=Some(&[0x30,0x78,0x01,0x30,0x30,0x70]) || mem.rom.get(0x2a7e2..0x2a7e8)!=Some(&[0x30,0x78,0x01,0x38,0x30,0x70]) {return None;}
    if !matches!(phase(mem),Some(ShopPhase::Quantity{..})) {return None;}
    let manager=word(&mem.ewram,0xf428)?.checked_sub(0x02000000)? as usize;
    Some(manager+QUANTITY+0x79)
}
/// The number being bought or sold and the most allowed, in the quantity window.
pub fn quantity(mem:&MemMap)->Option<(u8,u8)> {
    let a=quantity_address(mem)?;
    let (value,most)=(*mem.ewram.get(a)?,*mem.ewram.get(a+1)?);
    (value>=1 && value<=most && most<=99).then_some((value,most))
}
/// Sets the quantity with one Up or Down of the game's own window (its sound,
/// redraw and limits). False outside the quantity window.
pub fn set_quantity(mem:&mut MemMap,target:u8)->bool {
    if !mem.ffta_battle_mouse.enabled {return false;}
    let Some((value,most))=quantity(mem) else {return false};
    let target=target.clamp(1,most);
    if target==value {return true;}
    if !crate::ffta_battle_mouse::press(mem,if target>value {0x3bf}else{0x37f}) {return false;}
    mem.ffta_battle_mouse.shop.quantity=Some(QuantityRequest{target,deadline:mem.frames+20,committed:false});
    true
}
/// A quantity set is waiting for the game's window.
pub fn quantity_pending(mem:&MemMap)->bool {
    mem.ffta_battle_mouse.shop.quantity.is_some_and(|q|!q.committed && mem.frames<=q.deadline)
}
fn quantity_entry(m:&mut Machine) {
    let pc=m.cpu.regs[15];
    if !QUANTITY_STORES.contains(&pc) {return;}
    let Some(q)=m.bus.ffta_battle_mouse.shop.quantity.filter(|q|!q.committed) else {return};
    if m.bus.frames>q.deadline {m.bus.ffta_battle_mouse.shop.quantity=None;return;}
    let Some(a)=quantity_address(&m.bus) else {return};
    if m.cpu.regs[6]!=0x02000000+a as u32 {return;}
    m.cpu.regs[0]=q.target as u32;
    m.bus.ffta_battle_mouse.shop.quantity=Some(QuantityRequest{committed:true,..q});
}
/// The shop lists' Down and Up (USA, a list owner in r9, its cursor at
/// r9+0x4c, the shown row at r9+0x5d) compute the next row into r6 and then
/// check it, play the cursor sound and set the cursor from it: the tapped row
/// is put there instead, so the cursor moves straight to it. The handler
/// (0x08029f7c) has two ways: scrolling lists (0x08029fe2, 0x0802a06c) and
/// ones that wrap around instead (0x0802a118, 0x0802a172). It is interpreted
/// from its entry: once compiled, its blocks start there.
const ROW_NEXT:[u32;4]=[0x08029fe2,0x0802a06c,0x0802a118,0x0802a172];
const ROW_CODE:(u32,u32)=(0x08029f7c,0x0802a190);
fn row_code(mem:&MemMap)->bool {
    mem.ffta_battle_mouse.shop.profile.is_some_and(|p|p.menu.is_some())
        && mem.rom.get(0x29f7c..0x29f80)==Some(&[0xf0,0xb5,0x57,0x46]) && mem.rom.get(0x29fe0..0x29fe4)==Some(&[0x06,0x0e,0x37,0x06]) && mem.rom.get(0x2a06a..0x2a06e)==Some(&[0x06,0x0e,0x35,0x06])
        && [0x2a118,0x2a172].iter().all(|&o|mem.rom.get(o..o+4)==Some(&[0x34,0x06,0x56,0x45]))
}
/// Moves the shop list's cursor straight to shown row `row` (one Up or Down
/// of the game's own). False where that is not this game's list.
pub fn select_row(mem:&mut MemMap,row:u8)->bool {
    if !mem.ffta_battle_mouse.enabled || !row_code(mem) {return false;}
    let Some(l)=shop_list(mem) else {return false};
    if row>=l.visible || l.first as u32+row as u32>=l.count as u32 {return false;}
    if row==l.row {return true;}
    if !crate::ffta_battle_mouse::press(mem,if row>l.row {0x37f}else{0x3bf}) {return false;}
    mem.ffta_battle_mouse.shop.row=Some(RowRequest{cursor:l.list+0x20,target:row,deadline:mem.frames+20,committed:false});
    true
}
/// A row selection is waiting for the game's list.
pub fn row_pending(mem:&MemMap)->bool {
    mem.ffta_battle_mouse.shop.row.is_some_and(|r|!r.committed && mem.frames<=r.deadline)
}
fn row_entry(m:&mut Machine) {
    let pc=m.cpu.regs[15];
    if !ROW_NEXT.contains(&pc) {return;}
    let Some(r)=m.bus.ffta_battle_mouse.shop.row.filter(|r|!r.committed) else {return};
    if m.bus.frames>r.deadline {m.bus.ffta_battle_mouse.shop.row=None;return;}
    if m.cpu.regs[9].wrapping_add(0x4c)!=0x02000000+r.cursor as u32 {return;}
    m.cpu.regs[6]=r.target as u32;
    m.bus.ffta_battle_mouse.shop.row=Some(RowRequest{committed:true,..r});
}
pub(crate) fn entry(m:&mut Machine) {
    if m.bus.ffta_battle_mouse.enabled {quantity_entry(m);row_entry(m);}
    let s=&m.bus.ffta_battle_mouse;
    let Some(p)=s.shop.profile.filter(|_|s.enabled)else{return;};
    if m.cpu.regs[15]!=p.commit {return;}
    let Some(r)=s.shop.request.filter(|r|r.committed.is_none())else{return;};
    let Some(c)=context(&m.bus)else{return;};
    if c.overlay || c.base!=r.base || c.current!=r.source || c.ids!=r.ids || c.count!=r.count || r.slot>=c.count
        || m.cpu.regs[2]!=0x02000000+c.base as u32 || m.bus.frames>r.deadline {return;}
    m.cpu.regs[0]=r.slot as u32;
    m.bus.ffta_battle_mouse.shop.request.as_mut().unwrap().committed=Some(m.bus.frames);
}
pub fn scroll(mem:&mut MemMap,up:bool)->bool {
    if !mem.ffta_battle_mouse.enabled || context(mem).is_none()
        || half(&mem.io,0).is_some_and(|v|v&0x6000==0x6000){return false;}
    mem.ffta_battle_mouse.queue_shop_key(if up {0x3bf}else{0x37f});true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn list_down_and_up_enter_the_tapped_row() {
        let mut m=Machine::new(vec![]);
        let cursor=0x2443c;
        // Both ways of the list (scrolling, wrapping), Down and Up: r6 holds the next row.
        for pc in ROW_NEXT {
            m.bus.ffta_battle_mouse.shop.row=Some(RowRequest{cursor,target:3,deadline:10,committed:false});
            assert!(row_pending(&m.bus) && m.bus.ffta_battle_mouse.shop.interprets(ROW_CODE.0));
            m.cpu.regs[15]=pc;m.cpu.regs[6]=1;m.cpu.regs[9]=0x02000000+cursor as u32-0x4c;
            row_entry(&mut m);
            assert_eq!(m.cpu.regs[6],3);
            assert!(!row_pending(&m.bus) && !m.bus.ffta_battle_mouse.shop.interprets(ROW_CODE.0));
        }
        // Another list's owner is left alone; so is a request past its deadline.
        m.bus.ffta_battle_mouse.shop.row=Some(RowRequest{cursor,target:3,deadline:10,committed:false});
        m.cpu.regs[6]=1;m.cpu.regs[9]=0x02000000;
        row_entry(&mut m);
        assert_eq!(m.cpu.regs[6],1);
        m.bus.frames=11;m.cpu.regs[9]=0x02000000+cursor as u32-0x4c;
        row_entry(&mut m);
        assert_eq!(m.cpu.regs[6],1);
        assert!(m.bus.ffta_battle_mouse.shop.row.is_none());
    }
    fn fixture(version:Version,sell:bool)->Machine {
        let mut m=Machine::new(vec![]);m.cpu.cpsr=0x3f;
        let start=if version==Version::Europe {0x71490}else{0x6efa8};
        let inventory=if version==Version::Europe {0x71030}else{0x6eb48};
        m.bus.rom.resize(0x72000,0);
        m.bus.rom[start..start+12].copy_from_slice(&[0,0xb5,2,0x1c,9,4,9,0x0c,0x10,0x29,9,0xd0]);
        m.bus.rom[start+0x30..start+0x34].copy_from_slice(&[0x10,0x70,0x10,0x1c]);
        m.bus.rom[inventory..inventory+12].copy_from_slice(&[0xf0,0xb5,0x87,0xb0,6,0x1c,0x11,0x4f,0x39,0x68,0x11,0x4a]);
        m.bus.ffta_battle_mouse.shop=State::new(&m.bus.rom,version);m.bus.ffta_battle_mouse.enabled=true;
        for (o,v)in[(0,0x1f40u16),(8,0x4c03),(10,0x4e02),(12,0x0b08),(14,0x0a08)]{m.bus.io[o..o+2].copy_from_slice(&v.to_le_bytes());}
        m.bus.ewram[0xf428..0xf42c].copy_from_slice(&0x02001000u32.to_le_bytes());
        let cursor=0x24e0;let base=0x5470;
        m.bus.ewram[cursor..cursor+4].copy_from_slice(&(0x08000001+inventory as u32).to_le_bytes());
        let handler=m.bus.ffta_battle_mouse.shop.profile.unwrap().handlers[usize::from(sell)];
        m.bus.ewram[cursor-0x54..cursor-0x50].copy_from_slice(&handler.to_le_bytes());
        m.bus.ewram[cursor+4..cursor+8].copy_from_slice(&0x020024c0u32.to_le_bytes());
        let ids:&[u8]=if sell {&[1,2,3,5]}else{&[0,1,2,3,4,5]};
        m.bus.ewram[base+7]=ids.len()as u8;m.bus.ewram[base+1..base+1+ids.len()].copy_from_slice(ids);
        m.bus.ewram[base+0x78]=2;m
    }
    #[test]
    fn shop_phase_follows_the_handler_and_its_resting_steps() {
        let mut m=fixture(Version::Usa,false);
        m.bus.rom[0x68c10..0x68c18].copy_from_slice(&[0xf0,0xb5,0x4f,0x46,0x46,0x46,0xc0,0xb4]);
        m.bus.ffta_battle_mouse.shop=State::new(&m.bus.rom,Version::Usa);
        let set=|m:&mut Machine,handler:u32,step:u8|{
            m.bus.ewram[0x1000+0x148c..0x1000+0x1490].copy_from_slice(&handler.to_le_bytes());m.bus.ewram[0x1000+0x1490]=step;
        };
        for (handler,step,expected) in [
            (0x08068c11,5,Some(ShopPhase::Menu)),(0x08068c11,21,Some(ShopPhase::Busy)),
            (0x08069659,23,Some(ShopPhase::List{sell:false})),(0x08069659,61,Some(ShopPhase::List{sell:false})),
            (0x0806b6d1,43,Some(ShopPhase::Quantity{sell:true})),(0x08069659,102,Some(ShopPhase::Confirm{sell:false})),
            (0x08069659,103,Some(ShopPhase::Message)),(0x0806b6d1,123,Some(ShopPhase::Message)),
            (0x08069659,200,Some(ShopPhase::Busy)),(0,255,None),
        ] {set(&mut m,handler,step);assert_eq!(phase(&m.bus),expected,"{handler:08x} {step}");}
        // Europe: the steps were not measured there.
        let mut e=fixture(Version::Europe,false);
        e.bus.ewram[0x1000+0x148c..0x1000+0x1490].copy_from_slice(&0x0806bb0du32.to_le_bytes());e.bus.ewram[0x1000+0x1490]=23;
        assert_eq!(phase(&e.bus),None);
    }
    #[test]
    fn categories_use_available_ids_and_original_store_for_both_regions() {
        for version in [Version::Usa,Version::Europe] {for sell in [false,true] {
            let mut m=fixture(version,sell);let c=context(&m.bus).unwrap();
            for slot in 0..6 {
                assert!(aim(&mut m.bus,(c.x+slot as i32*16+8)as f32,8.,false));
                if slot>=c.count {assert!(!m.bus.ffta_battle_mouse.shop.pending());continue;}
                if slot==context(&m.bus).unwrap().current {continue;}
                assert!(m.bus.ffta_battle_mouse.shop.pending());
                assert_eq!(keys(&mut m.bus),Some(0x3ef));
                let p=m.bus.ffta_battle_mouse.shop.profile.unwrap();
                m.cpu.regs[15]=p.commit;m.cpu.regs[2]=0x02000000+c.base as u32;m.cpu.regs[0]=1;
                let before=m.bus.ewram.clone();entry(&mut m);
                assert_eq!(m.cpu.regs[0],slot as u32);assert_eq!(m.bus.ewram,before);
                m.step();assert_eq!(m.bus.ewram[c.base],slot);
                m.bus.frames+=2;assert_eq!(keys(&mut m.bus),Some(0x3ff));
            }
        }}
    }
    #[test]
    fn unavailable_categories_and_context_changes_never_confirm_or_retarget() {
        let mut m=fixture(Version::Europe,true);
        assert!(aim(&mut m.bus,88.,8.,true));assert!(!m.bus.ffta_battle_mouse.shop.pending());
        assert!(!select_category(&mut m.bus,4));
        assert!(select_category(&mut m.bus,2));m.bus.ewram[0x5471]=0;
        assert_eq!(keys(&mut m.bus),None);assert!(!m.bus.ffta_battle_mouse.shop.pending());
        m.bus.ewram[0x5471]=1;assert!(select_category(&mut m.bus,2));
        assert_eq!(crate::ffta_battle_mouse::keys(&mut m.bus,0x3fd),0x3fd);
        assert!(!m.bus.ffta_battle_mouse.shop.pending());
        // WIN1 on and empty (left after item Info closes): no window over the list.
        m.bus.io[..2].copy_from_slice(&0x5f40u16.to_le_bytes());
        for(o,v)in[(0x42,0xf0f0u16),(0x46,0x0898)]{m.bus.io[o..o+2].copy_from_slice(&v.to_le_bytes());}
        assert!(categories(&m.bus).is_some());
        // The quantity window (WIN1 152-192 x 88-104) is over it.
        for(o,v)in[(0x42,0x98c0u16),(0x46,0x5868)]{m.bus.io[o..o+2].copy_from_slice(&v.to_le_bytes());}
        assert!(categories(&m.bus).is_none());
        assert!(!aim(&mut m.bus,56.,8.,true));assert!(scroll(&mut m.bus,false));
        m.bus.io[..2].copy_from_slice(&0x7f40u16.to_le_bytes());assert!(!scroll(&mut m.bus,false));
    }
    #[test]
    fn quick_item_click_waits_until_the_requested_category_is_published() {
        let mut m=fixture(Version::Europe,false);
        assert!(select_category(&mut m.bus,2));
        assert!(aim(&mut m.bus,40.,24.,true));
        assert!(m.bus.ffta_battle_mouse.shop.deferred.is_some());
        m.bus.ffta_battle_mouse.shop.request.as_mut().unwrap().committed=Some(0);
        m.bus.frames=4;assert_eq!(keys(&mut m.bus),Some(0x3ff));
        assert!(m.bus.ffta_battle_mouse.shop.pending()); // old category still in RAM
        m.bus.ewram[0x5470]=2;
        assert_eq!(keys(&mut m.bus),Some(0x3ff));
        assert!(!m.bus.ffta_battle_mouse.shop.pending());
        assert!(m.bus.ffta_battle_mouse.shop.deferred.is_none());
    }
    #[test]
    fn quantity_digits_and_wheel_use_original_controls_but_confirmation_does_not() {
        let mut m=fixture(Version::Europe,false);
        for(o,v)in[(0,0x5f40u16),(0x42,0x98c0),(0x46,0x5868)]{m.bus.io[o..o+2].copy_from_slice(&v.to_le_bytes());}
        for(o,v)in[(0,0x006bu16),(2,0x10b0),(4,0x112f)]{m.bus.oam[o..o+2].copy_from_slice(&v.to_le_bytes());}
        assert!(aim(&mut m.bus,170.,96.,true));
        assert_eq!(crate::ffta_battle_mouse::keys(&mut m.bus,0x3ff),0x3df);
        m.bus.ffta_battle_mouse.cancel();assert!(scroll(&mut m.bus,true));
        assert_eq!(crate::ffta_battle_mouse::keys(&mut m.bus,0x3ff),0x3bf);
        m.bus.io[..2].copy_from_slice(&0x7f40u16.to_le_bytes());
        assert!(!scroll(&mut m.bus,true));assert!(!aim(&mut m.bus,170.,96.,true));
    }
    #[test]
    fn wrong_function_bytes_disable_the_category_hook() {
        let mut m=fixture(Version::Europe,false);m.bus.rom[0x71490]^=1;
        assert!(State::new(&m.bus.rom,Version::Europe).profile.is_none());
    }
}
