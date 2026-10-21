//! Machine save states (resume after the app is closed).
//!
//! A state is taken between frames and holds everything the guest can
//! observe: CPU registers (all banks), every memory region and I/O register,
//! timers, DMA channels, interrupt and video timing, sound (PSG channels,
//! direct-sound FIFOs), the cartridge backup chip and RTC, plus the host
//! helpers whose captured values persist across frames (battle map origins).
//! Running on from a restored state produces the same frames as running on
//! from the moment it was taken (`tests::restored_state_runs_identically`).
//! Host caches rebuilt every frame (UI capture, sprite publication, HLE audio
//! mixers) are not stored.
use crate::machine::Machine;

const MAGIC: &[u8; 8] = b"FFTARES1";

pub struct W(pub Vec<u8>);
impl W {
    pub fn u8(&mut self, v: u8) { self.0.push(v); }
    pub fn bool(&mut self, v: bool) { self.0.push(v as u8); }
    pub fn u16(&mut self, v: u16) { self.0.extend_from_slice(&v.to_le_bytes()); }
    pub fn u32(&mut self, v: u32) { self.0.extend_from_slice(&v.to_le_bytes()); }
    pub fn u64(&mut self, v: u64) { self.0.extend_from_slice(&v.to_le_bytes()); }
    pub fn i64(&mut self, v: i64) { self.u64(v as u64); }
    pub fn bytes(&mut self, v: &[u8]) { self.u32(v.len() as u32); self.0.extend_from_slice(v); }
}

pub type Res<T> = Result<T, String>;

pub struct R<'a> { data: &'a [u8], pos: usize }
impl<'a> R<'a> {
    pub fn new(data: &'a [u8]) -> Self { R { data, pos: 0 } }
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        let out = self.data.get(self.pos..self.pos + n).ok_or("Resume state is truncated")?;
        self.pos += n;
        Ok(out)
    }
    pub fn u8(&mut self) -> Res<u8> { Ok(self.take(1)?[0]) }
    pub fn bool(&mut self) -> Res<bool> { Ok(self.u8()? != 0) }
    pub fn u16(&mut self) -> Res<u16> { Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap())) }
    pub fn u32(&mut self) -> Res<u32> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
    pub fn u64(&mut self) -> Res<u64> { Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    pub fn i64(&mut self) -> Res<i64> { Ok(self.u64()? as i64) }
    pub fn bytes(&mut self) -> Res<&'a [u8]> { let n = self.u32()? as usize; self.take(n) }
    /// Fills `target` from a stored region of exactly its length.
    pub fn fill(&mut self, target: &mut [u8]) -> Res<()> {
        let data = self.bytes()?;
        if data.len() != target.len() { return Err("Resume state region size differs".into()); }
        target.copy_from_slice(data);
        Ok(())
    }
}

fn digest(rom: &[u8]) -> u64 {
    rom.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

pub fn save(m: &Machine) -> Vec<u8> { save_with(m, digest(&m.bus.rom)) }
fn save_with(m: &Machine, rom: u64) -> Vec<u8> {
    let mut w = W(Vec::with_capacity(512 * 1024));
    w.0.extend_from_slice(MAGIC);
    w.u64(rom);
    m.cpu.save_state(&mut w);
    m.bus.save_state(&mut w);
    w.0
}
/// An in-memory state for the host's short look-ahead runs (restored with
/// `restore` into the same machine): no ROM digest, which costs milliseconds.
pub fn snapshot(m: &Machine) -> Vec<u8> { save_with(m, 0) }
pub fn restore(m: &mut Machine, data: &[u8]) -> Res<()> { load_with(m, data, None) }

/// Restores a state taken with `save` for the same ROM. On error the machine
/// is left as it was (the state is decoded into a copy first).
pub fn load(m: &mut Machine, data: &[u8]) -> Res<()> { let d = digest(&m.bus.rom); load_with(m, data, Some(d)) }
fn load_with(m: &mut Machine, data: &[u8], rom: Option<u64>) -> Res<()> {
    if data.get(..8) != Some(&MAGIC[..]) { return Err("Not a resume state".into()); }
    let mut r = R { data, pos: 8 };
    let stored = r.u64()?;
    if rom.is_some_and(|d| d != stored) { return Err("Resume state belongs to another ROM".into()); }
    let cpu = crate::cpu::Cpu::load_state(&mut r)?;
    // Everything is decoded before anything is assigned.
    let bus = crate::mem::BusState::load(&mut r)?;
    if r.pos != data.len() { return Err("Resume state has trailing data".into()); }
    bus.apply(&mut m.bus)?;
    m.cpu = cpu;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codec_round_trips_and_rejects_bad_input() {
        let mut w = W(Vec::new());
        w.u8(7); w.bool(true); w.u16(0xbeef); w.u32(0xdead_beef); w.u64(u64::MAX - 3); w.i64(-5); w.bytes(&[1, 2, 3]);
        let mut r = R { data: &w.0, pos: 0 };
        assert_eq!((r.u8().unwrap(), r.bool().unwrap(), r.u16().unwrap(), r.u32().unwrap()), (7, true, 0xbeef, 0xdead_beef));
        assert_eq!((r.u64().unwrap(), r.i64().unwrap(), r.bytes().unwrap()), (u64::MAX - 3, -5, &[1u8, 2, 3][..]));
        assert!(r.u8().is_err());
        let mut m = Machine::new(vec![0; 0x200]);
        assert!(load(&mut m, b"garbage").is_err());
        let state = save(&m);
        assert!(load(&mut m, &state[..state.len() - 1]).is_err());
        assert!(load(&mut m, &state).is_ok());
        let mut other = Machine::new(vec![1; 0x200]);
        assert!(load(&mut other, &state).is_err());
    }
    #[test]
    fn restored_state_matches_guest_memory_and_registers() {
        let mut m = Machine::new(vec![0; 0x200]);
        m.bus.ewram[123] = 9;
        m.bus.io[0x50] = 0x44;
        m.cpu.regs[3] = 0x1234;
        m.bus.frames = 77;
        let state = save(&m);
        let mut n = Machine::new(vec![0; 0x200]);
        load(&mut n, &state).unwrap();
        assert_eq!((n.bus.ewram[123], n.bus.io[0x50], n.cpu.regs[3], n.bus.frames), (9, 0x44, 0x1234, 77));
        assert_eq!(save(&n), state);
    }
}
