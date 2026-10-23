//! Hold to hurry dialogue (touch-only): a finger held where a tap moves the
//! dialogue on prints the text faster, and turns its pages after a short
//! look; nothing else of the game runs faster.
//!
//! The game's text printer (USA, measured in Sprohm's pub): the message being
//! printed is the one at `MESSAGE` (0 when there is none), its text pointer at
//! +0x308 into its own buffer from +0x30c (2 bytes a character). Each frame the
//! printer adds its speed (0x80) to the 8.8 accumulator at +0x18 and prints a
//! character for each 0x100 there: one every two frames. With 0x400 in it, it
//! prints four a frame (with 0x1000 a whole line at once). Its state at +8 is
//! 5 while printing, 6 once the page waits for a button (with or without the
//! game's arrow).
use crate::engine::Engine;
use gba_core::{ffta_battle_mouse as battle, mem::MemMap};

/// The current message's pointer (EWRAM), read by the printer (USA).
const MESSAGE: usize = 0x918c;
/// A literal pool of the printer holding `MESSAGE`'s address (the ROM check).
const PRINTER_POOL: usize = 0x129f4;
/// Accumulator while hurried: four characters a frame (8x).
const HURRIED: u16 = 0x400;
/// Frames a finger stays down before it hurries (shorter: a tap).
pub(crate) const HOLD: u64 = 15;
/// Frames a finished page stays while hurried, then it is turned.
const PAGE_LOOK: u64 = 20;

fn word(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(offset..offset + 4)?.try_into().ok()?))
}

/// The message being printed: its offset in EWRAM.
fn message(bus: &MemMap) -> Option<usize> {
    if word(&bus.rom, PRINTER_POOL)? != 0x0200_0000 + MESSAGE as u32 { return None; }
    let at = word(&bus.ewram, MESSAGE)?.checked_sub(0x0200_0000)? as usize;
    if at + 0x30c > bus.ewram.len() - 0x2000 { return None; }
    let text = word(&bus.ewram, at + 0x308)?.checked_sub(0x0200_0000)? as usize;
    (at + 0x30c..at + 0x30c + 0x2000).contains(&text).then_some(at)
}

/// A message is being printed or waits for a button.
pub(crate) fn shown(bus: &MemMap) -> bool { message(bus).is_some() }

/// The message's page is printed and waits for a button.
fn page_done(bus: &MemMap) -> bool { message(bus).is_some_and(|at| bus.ewram[at + 8] == 6) }

/// Prints the current message faster this frame. False when there is none.
fn hurry(bus: &mut MemMap) -> bool {
    let Some(at) = message(bus) else { return false };
    let acc = u16::from_le_bytes([bus.ewram[at + 0x18], bus.ewram[at + 0x19]]);
    bus.ewram[at + 0x18..at + 0x1a].copy_from_slice(&acc.max(HURRIED).to_le_bytes());
    true
}

impl Engine {
    /// A tap would move the dialogue on: nothing to choose (no menu, notice,
    /// field cursor or facing), or the game's arrow waits.
    fn tap_moves_on(&self) -> bool {
        let bus = &self.machine.bus;
        crate::pub_touch::continue_arrow(bus) || (self.blind_confirm() && crate::pub_touch::notice(bus).is_none()
            && battle::Scene::read(bus).is_none() && battle::facing_center(bus).is_none())
    }
    /// A finger may hurry the dialogue here: a message is shown and a tap
    /// would move it on.
    pub(crate) fn dialogue_hold(&self) -> bool { shown(&self.machine.bus) && self.tap_moves_on() }
    /// Each frame, with a finger held (`since` the frame it touched): hurries
    /// the printing, and turns a finished page after `PAGE_LOOK` frames.
    pub(crate) fn drive_dialogue_hold(&mut self, since: u64, arrow: &mut Option<u64>) {
        let bus = &mut self.machine.bus;
        if bus.frames < since + HOLD || !hurry(bus) { *arrow = None; return; }
        if !page_done(bus) && !crate::pub_touch::continue_arrow(bus) { *arrow = None; return; }
        let seen = *arrow.get_or_insert(bus.frames);
        // Never a choice: a question waits for the player's own tap.
        if bus.frames >= seen + PAGE_LOOK && battle::idle(bus) && self.tap_moves_on() {
            *arrow = None;
            self.mouse.cancel();
            battle::press(&mut self.machine.bus, 0x3fe);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_held_finger_fills_the_printers_accumulator_only_while_a_message_is_printed() {
        let mut rom = vec![0; 0x20000];
        rom[PRINTER_POOL..PRINTER_POOL + 4].copy_from_slice(&0x0200918cu32.to_le_bytes());
        let mut m = gba_core::Machine::new(rom);
        let bus = &mut m.bus;
        // No message.
        assert!(!shown(bus) && !hurry(bus));
        // The pub's: at 0x02017368, its text pointer in its buffer.
        bus.ewram[MESSAGE..MESSAGE + 4].copy_from_slice(&0x02017368u32.to_le_bytes());
        bus.ewram[0x17670..0x17674].copy_from_slice(&0x02017674u32.to_le_bytes());
        bus.ewram[0x17380..0x17382].copy_from_slice(&0x80u16.to_le_bytes());
        assert!(hurry(bus));
        assert_eq!(&bus.ewram[0x17380..0x17382], &HURRIED.to_le_bytes());
        // Printing (5), then waiting for a button (6).
        bus.ewram[0x17370] = 5;
        assert!(!page_done(bus));
        bus.ewram[0x17370] = 6;
        assert!(page_done(bus));
        // Already faster (a panel printed at once): left as it is.
        bus.ewram[0x17380..0x17382].copy_from_slice(&0x3500u16.to_le_bytes());
        assert!(hurry(bus));
        assert_eq!(&bus.ewram[0x17380..0x17382], &0x3500u16.to_le_bytes());
        // A stale pointer (its text pointer elsewhere): not a message.
        bus.ewram[0x17670..0x17674].copy_from_slice(&0x08001234u32.to_le_bytes());
        assert!(!shown(bus));
        // Another ROM: never.
        bus.ewram[0x17670..0x17674].copy_from_slice(&0x02017674u32.to_le_bytes());
        bus.rom[PRINTER_POOL] = 0;
        assert!(!shown(bus));
    }
}
