//! HydatekOS's audio driver: Intel High Definition Audio, the sound hardware
//! in nearly every PC (Realtek, Conexant, Cirrus, IDT codecs behind an
//! Intel or AMD controller), and in virtual machines.
//!
//! - the controller: reset, the command rings (CORB/RIRB) codecs are
//!   talked to through;
//! - each codec's audio function group: its widgets (converters, mixers,
//!   selectors, pins), and a path from every output pin (speakers,
//!   headphones, line out) back to a converter; everything on the way is
//!   powered, unmuted and selected, speaker amplifiers (EAPD) switched on;
//! - one output stream: 48 kHz, 16-bit stereo, from a ring of four buffers
//!   the driver keeps filled a little ahead of the hardware from
//!   sound::Mixer.

use crate::pci;
use crate::sound::{Mixer, Sound};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};

fn r8(a: usize) -> u8 {
    unsafe { read_volatile(a as *const u8) }
}
fn w8(a: usize, v: u8) {
    unsafe { write_volatile(a as *mut u8, v) }
}
fn r16(a: usize) -> u16 {
    unsafe { read_volatile(a as *const u16) }
}
fn w16(a: usize, v: u16) {
    unsafe { write_volatile(a as *mut u16, v) }
}
fn r32(a: usize) -> u32 {
    unsafe { read_volatile(a as *const u32) }
}
fn w32(a: usize, v: u32) {
    unsafe { write_volatile(a as *mut u32, v) }
}

const GCTL: usize = 0x08;
const STATESTS: usize = 0x0E;
const CORBLBASE: usize = 0x40;
const CORBWP: usize = 0x48;
const CORBRP: usize = 0x4A;
const CORBCTL: usize = 0x4C;
const CORBSIZE: usize = 0x4E;
const RIRBLBASE: usize = 0x50;
const RIRBWP: usize = 0x58;
const RINTCNT: usize = 0x5A;
const RIRBCTL: usize = 0x5C;
const RIRBSTS: usize = 0x5D;
const RIRBSIZE: usize = 0x5E;

/// The ring the stream plays from: four buffers of 16 KiB (~340 ms).
const BUF: usize = 4 * 16384;
/// How far ahead of the hardware the driver writes (bytes, ~60 ms).
const LEAD: usize = 48_000 * 4 * 60 / 1000;

pub struct Hda {
    base: usize,
    corb: usize,
    rirb: usize,
    corb_wp: u16,
    rirb_rp: u16,
    sd: usize,
    ring: usize,
    /// where the driver has written up to (bytes into the ring)
    written: usize,
    pub mixer: Mixer,
    pub name: String,
    pub outputs: Vec<&'static str>,
}

fn codec_name(vendor: u16) -> &'static str {
    match vendor {
        0x10EC => "Realtek",
        0x14F1 => "Conexant",
        0x111D | 0x8384 => "IDT",
        0x1013 => "Cirrus Logic",
        0x11D4 => "Analog Devices",
        0x1106 => "VIA",
        0x8086 => "Intel",
        0x1002 => "AMD",
        0x10DE => "NVIDIA",
        0x1AF4 => "QEMU",
        _ => "",
    }
}

impl Hda {
    pub fn start(d: &pci::Dev) -> Result<Hda, &'static str> {
        let base = d.bar(0) as usize;
        if base == 0 {
            return Err("no registers");
        }
        d.take();
        d.enable();
        // reset the controller and bring it back
        w32(base + GCTL, r32(base + GCTL) & !1);
        if !crate::efi::wait_until(100, || r32(base + GCTL) & 1 == 0) {
            return Err("wouldn't reset");
        }
        crate::efi::stall_ms(1);
        w32(base + GCTL, r32(base + GCTL) | 1);
        if !crate::efi::wait_until(100, || r32(base + GCTL) & 1 == 1) {
            return Err("wouldn't come out of reset");
        }
        // codecs announce themselves within a millisecond
        crate::efi::stall_ms(2);
        let codecs = r16(base + STATESTS);
        if codecs == 0 {
            return Err("no codecs");
        }
        let (corb, rirb) = (crate::efi::dma(1).ok_or("no memory")?, crate::efi::dma(1).ok_or("no memory")?);
        // the command ring (256 entries), then the response ring
        w8(base + CORBCTL, 0);
        crate::efi::wait_until(50, || r8(base + CORBCTL) & 2 == 0);
        if r8(base + CORBSIZE) & 0x40 != 0 {
            w8(base + CORBSIZE, 2);
        }
        w32(base + CORBLBASE, corb as u32);
        w32(base + CORBLBASE + 4, (corb as u64 >> 32) as u32);
        w16(base + CORBRP, 0x8000);
        crate::efi::wait_until(50, || r16(base + CORBRP) & 0x8000 != 0);
        w16(base + CORBRP, 0);
        crate::efi::wait_until(50, || r16(base + CORBRP) & 0x8000 == 0);
        w16(base + CORBWP, 0);
        w8(base + CORBCTL, 2);
        w8(base + RIRBCTL, 0);
        crate::efi::wait_until(50, || r8(base + RIRBCTL) & 2 == 0);
        if r8(base + RIRBSIZE) & 0x40 != 0 {
            w8(base + RIRBSIZE, 2);
        }
        w32(base + RIRBLBASE, rirb as u32);
        w32(base + RIRBLBASE + 4, (rirb as u64 >> 32) as u32);
        w16(base + RIRBWP, 0x8000);
        w16(base + RINTCNT, 0xFF);
        w8(base + RIRBCTL, 2);
        // the first output stream
        let gcap = r16(base);
        let iss = ((gcap >> 8) & 0xF) as usize;
        if (gcap >> 12) & 0xF == 0 {
            return Err("no output streams");
        }
        let sd = base + 0x80 + 0x20 * iss;
        let ring = crate::efi::dma(BUF / 4096).ok_or("no memory")?;
        let mut h = Hda { base, corb, rirb, corb_wp: 0, rirb_rp: 0, sd, ring, written: 0, mixer: Mixer::default(), name: String::new(), outputs: Vec::new() };
        for c in 0..15u32 {
            if codecs & 1 << c != 0 && h.setup_codec(c) {
                break;
            }
        }
        if h.outputs.is_empty() {
            return Err("no speakers, headphones or line out");
        }
        h.start_stream()?;
        let (b, dv, f) = d.loc;
        log!("hda: {:02x}:{:02x}.{} {}: playing to {}", b, dv, f, h.name, h.outputs.join(", "));
        Ok(h)
    }

    /// Send a verb to a codec; its answer.
    fn verb(&mut self, codec: u32, nid: u32, verb: u32, payload: u32) -> Option<u32> {
        let v = if verb < 0x10 { codec << 28 | nid << 20 | verb << 16 | (payload & 0xFFFF) } else { codec << 28 | nid << 20 | verb << 8 | (payload & 0xFF) };
        self.corb_wp = (self.corb_wp + 1) % 256;
        unsafe { write_volatile((self.corb + 4 * self.corb_wp as usize) as *mut u32, v) };
        w16(self.base + CORBWP, self.corb_wp);
        let base = self.base;
        let want = (self.rirb_rp + 1) % 256;
        if !crate::efi::wait_until(20, || r16(base + RIRBWP) & 0xFF == want) {
            return None;
        }
        self.rirb_rp = want;
        // clear "response arrived" (some controllers pause the rings until it is)
        w8(self.base + RIRBSTS, 0x05);
        Some(unsafe { read_volatile((self.rirb + 8 * want as usize) as *const u32) })
    }

    fn param(&mut self, codec: u32, nid: u32, id: u32) -> u32 {
        self.verb(codec, nid, 0xF00, id).unwrap_or(0)
    }

    fn connections(&mut self, codec: u32, nid: u32) -> Vec<u32> {
        let len = self.param(codec, nid, 0x0E);
        let (n, long) = ((len & 0x7F) as usize, len & 0x80 != 0);
        let mut out = Vec::new();
        let per = if long { 2 } else { 4 };
        let mut i = 0;
        while i < n.min(32) {
            let r = self.verb(codec, nid, 0xF02, i as u32).unwrap_or(0);
            for k in 0..per {
                if i + k < n {
                    out.push(if long { (r >> (16 * k)) & 0xFFFF } else { (r >> (8 * k)) & 0xFF });
                }
            }
            i += per;
        }
        out
    }

    /// Walk the audio function group and switch on every output path.
    fn setup_codec(&mut self, c: u32) -> bool {
        let vid = self.param(c, 0, 0x00);
        let groups = self.param(c, 0, 0x04);
        let (gstart, gn) = ((groups >> 16) & 0xFF, groups & 0xFF);
        for g in gstart..gstart + gn {
            if self.param(c, g, 0x05) & 0xFF != 1 {
                continue;
            }
            self.verb(c, g, 0x705, 0);
            let w = self.param(c, g, 0x04);
            let (ws, wn) = ((w >> 16) & 0xFF, w & 0xFF);
            let mut kind = Vec::new();
            for nid in ws..ws + wn {
                kind.push((nid, (self.param(c, nid, 0x09) >> 20) & 0xF));
            }
            let type_of = |nid: u32, kind: &Vec<(u32, u32)>| kind.iter().find(|k| k.0 == nid).map(|k| k.1);
            let mut dacs = Vec::new();
            for &(pin, t) in kind.clone().iter() {
                if t != 4 {
                    continue;
                }
                let caps = self.param(c, pin, 0x0C);
                let cfg = self.verb(c, pin, 0xF1C, 0).unwrap_or(0);
                if caps & 0x10 == 0 || (cfg >> 30) & 3 == 1 {
                    continue;
                }
                let dev = (cfg >> 20) & 0xF;
                let what = match dev {
                    0 => "line out",
                    1 => "speakers",
                    2 => "headphones",
                    5 => "S/PDIF",
                    _ => continue,
                };
                // a path back to a converter (through up to three mixers/selectors)
                let mut path: Vec<(u32, usize)> = Vec::new();
                if !self.find_dac(c, pin, &kind, &type_of, &mut path, 0) {
                    continue;
                }
                // power, select, unmute along the way
                for &(nid, idx) in &path {
                    self.verb(c, nid, 0x705, 0);
                    let t = type_of(nid, &kind).unwrap_or(0);
                    if t == 3 || t == 4 {
                        self.verb(c, nid, 0x701, idx as u32);
                    }
                    // output amp, both sides, at 0 dB (volume is done in software)
                    let zero = self.zero_db(c, nid, g);
                    self.verb(c, nid, 0x3, 0xB000 | zero);
                    if t == 2 {
                        self.verb(c, nid, 0x3, 0x7000 | (idx as u32) << 8 | zero);
                    }
                }
                let dac = path.last().map(|p| p.0).unwrap_or(0);
                // the pin: output on (headphone drive for headphones), speaker amp on
                self.verb(c, pin, 0x707, if dev == 2 { 0xC0 } else { 0x40 });
                if caps & 1 << 16 != 0 {
                    self.verb(c, pin, 0x70C, 2);
                }
                if !dacs.contains(&dac) {
                    dacs.push(dac);
                }
                if !self.outputs.contains(&what) {
                    self.outputs.push(what);
                }
            }
            // every converter used plays stream 1, 48 kHz 16-bit stereo
            for dac in dacs {
                self.verb(c, dac, 0x705, 0);
                self.verb(c, dac, 0x706, 1 << 4);
                self.verb(c, dac, 0x2, 0x0011);
                let zero = self.zero_db(c, dac, g);
                self.verb(c, dac, 0x3, 0xB000 | zero);
            }
            let v = (vid >> 16) as u16;
            let maker = codec_name(v);
            self.name = if maker.is_empty() { format!("HD Audio codec {:08x}", vid) } else { format!("{} HD Audio ({:04x})", maker, vid & 0xFFFF) };
            return !self.outputs.is_empty();
        }
        false
    }

    /// The gain step that is 0 dB on a widget's output amplifier (the
    /// function group's defaults when it has none of its own).
    fn zero_db(&mut self, c: u32, nid: u32, group: u32) -> u32 {
        let caps = match self.param(c, nid, 0x12) {
            0 => self.param(c, group, 0x12),
            v => v,
        };
        caps & 0x7F
    }

    fn find_dac(&mut self, c: u32, nid: u32, kind: &Vec<(u32, u32)>, type_of: &dyn Fn(u32, &Vec<(u32, u32)>) -> Option<u32>, path: &mut Vec<(u32, usize)>, depth: usize) -> bool {
        if depth > 4 {
            return false;
        }
        let conns = self.connections(c, nid);
        for (i, &n) in conns.iter().enumerate() {
            match type_of(n, kind) {
                Some(0) => {
                    path.push((nid, i));
                    path.push((n, 0));
                    return true;
                }
                Some(2) | Some(3) => {
                    path.push((nid, i));
                    if self.find_dac(c, n, kind, type_of, path, depth + 1) {
                        return true;
                    }
                    path.pop();
                }
                _ => {}
            }
        }
        false
    }

    fn start_stream(&mut self) -> Result<(), &'static str> {
        let sd = self.sd;
        // reset the stream
        w8(sd, r8(sd) | 1);
        crate::efi::wait_until(50, || r8(sd) & 1 != 0);
        w8(sd, r8(sd) & !1);
        crate::efi::wait_until(50, || r8(sd) & 1 == 0);
        // four entries in the buffer descriptor list
        let bdl = crate::efi::dma(1).ok_or("no memory")?;
        for i in 0..4 {
            unsafe {
                write_volatile((bdl + 16 * i) as *mut u64, (self.ring + i * BUF / 4) as u64);
                write_volatile((bdl + 16 * i + 8) as *mut u32, (BUF / 4) as u32);
                write_volatile((bdl + 16 * i + 12) as *mut u32, 0);
            }
        }
        w32(sd + 0x18, bdl as u32);
        w32(sd + 0x1C, (bdl as u64 >> 32) as u32);
        w32(sd + 0x08, BUF as u32);
        w16(sd + 0x0C, 3);
        w16(sd + 0x12, 0x0011);
        // stream 1, then run
        w8(sd + 2, (r8(sd + 2) & 0x0F) | 1 << 4);
        w8(sd + 3, 0x1C);
        w8(sd, r8(sd) | 2);
        Ok(())
    }

    /// Keep the ring filled ahead of the hardware. Called every frame.
    pub fn pump(&mut self, gain: i32) {
        let pos = r32(self.sd + 4) as usize % BUF;
        let target = (pos + LEAD) % BUF;
        let mut todo = (target + BUF - self.written) % BUF;
        // fell behind (a long frame): start again just ahead
        if todo > BUF / 2 {
            self.written = pos;
            todo = LEAD;
        }
        let mut tmp = [0i16; 1024];
        while todo >= 4 {
            let n = todo.min(tmp.len() * 2).min(BUF - self.written) / 4 * 4;
            if n == 0 {
                break;
            }
            let frames = &mut tmp[..n / 2];
            self.mixer.fill(frames, gain);
            unsafe { core::ptr::copy_nonoverlapping(frames.as_ptr() as *const u8, (self.ring + self.written) as *mut u8, n) };
            self.written = (self.written + n) % BUF;
            todo -= n;
        }
    }

    pub fn play(&mut self, s: Sound) {
        self.mixer.play(s);
    }
}

/// The first HD Audio controller that has somewhere to play to.
pub fn start() -> Option<Hda> {
    for d in pci::devices() {
        if d.class.0 != 0x04 || d.class.1 != 0x03 {
            continue;
        }
        match Hda::start(&d) {
            Ok(h) => return Some(h),
            Err(e) => log!("hda: {:02x}:{:02x}.{}: {}", d.loc.0, d.loc.1, d.loc.2, e),
        }
    }
    None
}
