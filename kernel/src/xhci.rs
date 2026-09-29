//! HydatekOS's own USB host controller driver: xHCI (USB 3, and USB 2 and
//! 1.1 devices through it), the controller in every PC and ARM laptop of
//! the last decade.
//!
//! It takes a controller from the firmware (pci::may_take: never the one
//! holding the boot disk or the only keyboard), resets it, and runs it with
//! its own rings and contexts:
//! - a command ring, an event ring, the device context array (with the
//!   scratchpad buffers the controller asks for);
//! - root ports: reset, speed, hot-plug (port status change events);
//! - each device: Enable Slot, Address Device, its descriptors (control
//!   transfers on endpoint 0), Set Configuration;
//! - HID interfaces: Configure Endpoint for their interrupt IN endpoint, the
//!   report descriptor, and a ring of transfers that keeps reports coming;
//!   the reports go through hidin.rs like every other HID device.
//!
//! Polled (no interrupts): events are read each frame. Storage, audio and
//! Bluetooth devices on these ports are listed but not driven yet.

use crate::hidin::{Event, HidInput};
use crate::pci;
use crate::usb::{class_name, DeviceInfo};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct Trb {
    p: u64,
    s: u32,
    c: u32,
}

const TRB_NORMAL: u32 = 1;
const TRB_SETUP: u32 = 2;
const TRB_DATA: u32 = 3;
const TRB_STATUS: u32 = 4;
const TRB_LINK: u32 = 6;
const TRB_ENABLE_SLOT: u32 = 9;
const TRB_ADDRESS: u32 = 11;
const TRB_CONFIGURE: u32 = 12;
const TRB_EVALUATE: u32 = 13;
const EV_TRANSFER: u32 = 32;
const EV_COMMAND: u32 = 33;
const EV_PORT: u32 = 34;

const IOC: u32 = 1 << 5;
const IDT: u32 = 1 << 6;
const ISP: u32 = 1 << 2;

// operational registers
const USBCMD: usize = 0x00;
const USBSTS: usize = 0x04;
const CRCR: usize = 0x18;
const DCBAAP: usize = 0x30;
const CONFIG: usize = 0x38;
// PORTSC bits
const CCS: u32 = 1 << 0;
const PED: u32 = 1 << 1;
const PR: u32 = 1 << 4;
const PP: u32 = 1 << 9;
const CHANGES: u32 = 0x7F << 17;
const PRC: u32 = 1 << 21;

const RING: usize = 256;

/// A ring of TRBs the driver fills (commands, transfers).
struct Ring {
    base: usize,
    idx: usize,
    cycle: u32,
}

impl Ring {
    fn new() -> Option<Ring> {
        let base = crate::efi::dma(RING * 16 / 4096)?;
        // the last TRB links back to the first, toggling the cycle bit
        let link = (base + (RING - 1) * 16) as *mut Trb;
        unsafe { write_volatile(link, Trb { p: base as u64, s: 0, c: TRB_LINK << 10 | 1 << 1 }) };
        Some(Ring { base, idx: 0, cycle: 1 })
    }

    /// Queue a TRB; its address.
    fn push(&mut self, mut t: Trb) -> u64 {
        let at = self.base + self.idx * 16;
        t.c = (t.c & !1) | self.cycle;
        unsafe {
            write_volatile(at as *mut Trb, Trb { c: t.c ^ 1, ..t });
            fence(Ordering::SeqCst);
            write_volatile((at + 12) as *mut u32, t.c);
        }
        self.idx += 1;
        if self.idx == RING - 1 {
            let link = (self.base + (RING - 1) * 16) as *mut Trb;
            unsafe {
                let mut l = read_volatile(link);
                l.c = (l.c & !1) | self.cycle;
                write_volatile(link, l);
            }
            self.idx = 0;
            self.cycle ^= 1;
        }
        at as u64
    }
}

/// An interrupt IN endpoint bringing HID reports.
struct Pipe {
    dci: u8,
    ring: Ring,
    /// the buffers queued: (TRB address, buffer)
    bufs: Vec<(u64, usize)>,
    len: usize,
    hid: HidInput,
}

struct UsbDev {
    slot: u8,
    port: u8,
    ep0: Ring,
    input: usize,
    pipes: Vec<Pipe>,
}

pub struct Xhci {
    base: usize,
    op: usize,
    rt: usize,
    db: usize,
    ctx: usize,
    ports: u8,
    dcbaa: usize,
    cmd: Ring,
    ev: usize,
    ev_idx: usize,
    ev_cycle: u32,
    devs: Vec<UsbDev>,
    /// events that arrived while waiting for something else
    backlog: Vec<Trb>,
    /// ports whose state changed
    changed: Vec<u8>,
    /// a scratch page for control transfers
    scratch: usize,
    pub name: String,
    pub info: Vec<DeviceInfo>,
    pub generation: u32,
}

fn rd(a: usize) -> u32 {
    unsafe { read_volatile(a as *const u32) }
}
fn wr(a: usize, v: u32) {
    unsafe { write_volatile(a as *mut u32, v) }
}
fn wr64(a: usize, v: u64) {
    wr(a, v as u32);
    wr(a + 4, (v >> 32) as u32);
}

/// Wait up to `ms` for `f`.
fn wait(ms: u64, mut f: impl FnMut() -> bool) -> bool {
    let end = crate::arch::ms() + ms;
    loop {
        if f() {
            return true;
        }
        if crate::arch::ms() > end {
            return false;
        }
        core::hint::spin_loop();
    }
}

impl Xhci {
    /// Take a controller and bring it up; the devices on its ports are started.
    pub fn start(d: &pci::Dev) -> Result<Xhci, &'static str> {
        let base = d.bar(0) as usize;
        if base == 0 {
            return Err("no registers");
        }
        if !d.take() {
            return Err("the firmware wouldn't let go");
        }
        d.enable();
        let caplen = rd(base) & 0xFF;
        let hcs1 = rd(base + 4);
        let hcs2 = rd(base + 8);
        let hcc1 = rd(base + 0x10);
        let op = base + caplen as usize;
        let rt = base + (rd(base + 0x18) & !0x1F) as usize;
        let db = base + (rd(base + 0x14) & !3) as usize;
        let ctx = if hcc1 & 4 != 0 { 64 } else { 32 };
        let (slots, ports) = ((hcs1 & 0xFF).min(32) as u8, (hcs1 >> 24) as u8);
        // take it from the firmware's SMM code (USB legacy support)
        let mut xecp = ((hcc1 >> 16) << 2) as usize;
        for _ in 0..32 {
            if xecp == 0 {
                break;
            }
            let v = rd(base + xecp);
            if v & 0xFF == 1 {
                wr(base + xecp, v | 1 << 24);
                wait(500, || rd(base + xecp) & 1 << 16 == 0);
                // no SMIs
                wr(base + xecp + 4, rd(base + xecp + 4) & 0xFFFF_0000 | 0xE000_0000);
            }
            let next = ((v >> 8) & 0xFF) as usize;
            xecp = if next == 0 { 0 } else { xecp + next * 4 };
        }
        // stop, reset
        wr(op + USBCMD, rd(op + USBCMD) & !1);
        if !wait(100, || rd(op + USBSTS) & 1 != 0) {
            return Err("wouldn't stop");
        }
        wr(op + USBCMD, 2);
        if !wait(500, || rd(op + USBCMD) & 2 == 0 && rd(op + USBSTS) & 1 << 11 == 0) {
            return Err("wouldn't reset");
        }
        wr(op + CONFIG, slots as u32);
        let dcbaa = crate::efi::dma(1).ok_or("no memory")?;
        // scratchpad buffers
        let sp = (((hcs2 >> 21) & 0x1F) << 5 | (hcs2 >> 27) & 0x1F) as usize;
        if sp > 0 {
            let arr = crate::efi::dma(1).ok_or("no memory")?;
            for i in 0..sp.min(512) {
                let page = crate::efi::dma(1).ok_or("no memory")?;
                unsafe { write_volatile((arr + 8 * i) as *mut u64, page as u64) };
            }
            unsafe { write_volatile(dcbaa as *mut u64, arr as u64) };
        }
        wr64(op + DCBAAP, dcbaa as u64);
        let cmd = Ring::new().ok_or("no memory")?;
        wr64(op + CRCR, cmd.base as u64 | 1);
        // one event ring segment
        let ev = crate::efi::dma(1).ok_or("no memory")?;
        let erst = crate::efi::dma(1).ok_or("no memory")?;
        unsafe {
            write_volatile(erst as *mut u64, ev as u64);
            write_volatile((erst + 8) as *mut u32, RING as u32);
        }
        let ir = rt + 0x20;
        wr(ir + 8, 1);
        wr64(ir + 0x18, ev as u64);
        wr64(ir + 0x10, erst as u64);
        // run
        wr(op + USBCMD, 1);
        if !wait(100, || rd(op + USBSTS) & 1 == 0) {
            return Err("wouldn't run");
        }
        let (b, dv, f) = d.loc;
        let mut x = Xhci {
            base,
            op,
            rt,
            db,
            ctx,
            ports,
            dcbaa,
            cmd,
            ev,
            ev_idx: 0,
            ev_cycle: 1,
            devs: Vec::new(),
            backlog: Vec::new(),
            changed: Vec::new(),
            scratch: crate::efi::dma(1).ok_or("no memory")?,
            name: format!("xHCI {:02x}:{:02x}.{}", b, dv, f),
            info: Vec::new(),
            generation: 1,
        };
        let _ = x.base;
        // power the ports, give devices time to connect
        for p in 1..=ports {
            let a = x.portsc(p);
            let v = rd(a);
            if v & PP == 0 {
                wr(a, PP);
            }
        }
        crate::efi::stall_ms(100);
        for p in 1..=ports {
            if rd(x.portsc(p)) & CCS != 0 {
                x.attach(p);
            }
        }
        log!("xhci: {} running: {} ports, {} slots, {}-byte contexts, {} devices", x.name, ports, slots, ctx, x.devs.len());
        Ok(x)
    }

    fn portsc(&self, p: u8) -> usize {
        self.op + 0x400 + 0x10 * (p as usize - 1)
    }

    fn ring_doorbell(&self, slot: u8, target: u8) {
        fence(Ordering::SeqCst);
        wr(self.db + 4 * slot as usize, target as u32);
    }

    /// The next event, if there is one.
    fn next_event(&mut self) -> Option<Trb> {
        if let Some(t) = (!self.backlog.is_empty()).then(|| self.backlog.remove(0)) {
            return Some(t);
        }
        self.pop_event()
    }

    fn pop_event(&mut self) -> Option<Trb> {
        let at = self.ev + self.ev_idx * 16;
        let t = unsafe { read_volatile(at as *const Trb) };
        if t.c & 1 != self.ev_cycle {
            return None;
        }
        self.ev_idx += 1;
        if self.ev_idx == RING {
            self.ev_idx = 0;
            self.ev_cycle ^= 1;
        }
        // tell the controller how far we've read (and clear the busy flag)
        wr64(self.rt + 0x20 + 0x18, (self.ev + self.ev_idx * 16) as u64 | 8);
        Some(t)
    }

    /// Wait for the event `want` picks out, keeping the others for later.
    fn wait_event(&mut self, ms: u64, want: impl Fn(&Trb) -> bool) -> Option<Trb> {
        let end = crate::arch::ms() + ms;
        loop {
            if let Some(t) = self.pop_event() {
                if want(&t) {
                    return Some(t);
                }
                if self.backlog.len() < 256 {
                    self.backlog.push(t);
                }
                continue;
            }
            if crate::arch::ms() > end {
                return None;
            }
            core::hint::spin_loop();
        }
    }

    /// Run a command; its completion (code, slot).
    fn command(&mut self, t: Trb) -> Option<(u32, u8)> {
        let at = self.cmd.push(t);
        self.ring_doorbell(0, 0);
        let e = self.wait_event(500, |e| e.c >> 10 & 0x3F == EV_COMMAND && e.p == at)?;
        Some((e.s >> 24, (e.c >> 24) as u8))
    }

    /// A control transfer on a device's endpoint 0. Data up to 4 KiB.
    fn control(&mut self, di: usize, req: [u8; 8], data: &mut [u8]) -> bool {
        let len = u16::from_le_bytes([req[6], req[7]]) as usize;
        let n = len.min(data.len()).min(4096);
        let dir_in = req[0] & 0x80 != 0;
        if !dir_in {
            unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.scratch as *mut u8, n) };
        }
        let slot = self.devs[di].slot;
        let setup = u64::from_le_bytes(req);
        let trt = if n == 0 { 0 } else if dir_in { 3 } else { 2 };
        let ring = &mut self.devs[di].ep0;
        ring.push(Trb { p: setup, s: 8, c: TRB_SETUP << 10 | IDT | trt << 16 });
        if n > 0 {
            ring.push(Trb { p: self.scratch as u64, s: n as u32, c: TRB_DATA << 10 | (dir_in as u32) << 16 });
        }
        let status_in = n == 0 || !dir_in;
        let at = ring.push(Trb { p: 0, s: 0, c: TRB_STATUS << 10 | IOC | (status_in as u32) << 16 });
        self.ring_doorbell(slot, 1);
        let Some(e) = self.wait_event(500, |e| e.c >> 10 & 0x3F == EV_TRANSFER && (e.c >> 24) as u8 == slot && e.p == at) else { return false };
        let code = e.s >> 24;
        if code != 1 && code != 13 {
            return false;
        }
        if dir_in && n > 0 {
            unsafe { core::ptr::copy_nonoverlapping(self.scratch as *const u8, data.as_mut_ptr(), n) };
        }
        true
    }

    fn get_descriptor(&mut self, di: usize, kind: u8, index: u8, lang: u16, buf: &mut [u8]) -> bool {
        let l = buf.len() as u16;
        self.control(di, [0x80, 6, index, kind, lang as u8, (lang >> 8) as u8, l as u8, (l >> 8) as u8], buf)
    }

    fn string(&mut self, di: usize, idx: u8) -> String {
        if idx == 0 {
            return String::new();
        }
        let mut b = [0u8; 255];
        if !self.get_descriptor(di, 3, idx, 0x0409, &mut b) {
            return String::new();
        }
        let n = (b[0] as usize).min(255);
        let u: Vec<u16> = (2..n).step_by(2).map(|i| u16::from_le_bytes([b[i], b[i + 1]])).collect();
        String::from_utf16_lossy(&u).trim().into()
    }

    /// Reset a port and start the device on it.
    fn attach(&mut self, port: u8) {
        let a = self.portsc(port);
        let v = rd(a);
        // USB 2 ports need a reset to enable; USB 3 ones enable themselves
        if v & PED == 0 {
            wr(a, (v & !(PED | CHANGES)) | PR);
            if !wait(500, || rd(a) & PRC != 0) {
                return;
            }
            crate::efi::stall_ms(20);
        }
        let v = rd(a);
        wr(a, (v & !PED) | (v & CHANGES));
        if v & PED == 0 {
            return;
        }
        let speed = (v >> 10) & 0xF;
        let Some((1, slot)) = self.command(Trb { p: 0, s: 0, c: TRB_ENABLE_SLOT << 10 }) else { return };
        // contexts: input (control + slot + 31 endpoints) and output
        let (Some(input), Some(out), Some(ep0)) = (crate::efi::dma(1), crate::efi::dma(1), Ring::new()) else { return };
        unsafe { write_volatile((self.dcbaa + 8 * slot as usize) as *mut u64, out as u64) };
        let cs = self.ctx;
        let mps0: u32 = match speed {
            2 | 1 => 8,
            3 => 64,
            _ => 512,
        };
        unsafe {
            // add the slot and endpoint 0
            write_volatile((input + 4) as *mut u32, 0b11);
            let sl = input + cs;
            write_volatile(sl as *mut u32, 1 << 27 | speed << 20);
            write_volatile((sl + 4) as *mut u32, (port as u32) << 16);
            let e0 = input + 2 * cs;
            write_volatile((e0 + 4) as *mut u32, 3 << 1 | 4 << 3 | mps0 << 16);
            write_volatile((e0 + 8) as *mut u64, ep0.base as u64 | 1);
            write_volatile((e0 + 16) as *mut u32, 8);
        }
        let Some((1, _)) = self.command(Trb { p: input as u64, s: 0, c: TRB_ADDRESS << 10 | (slot as u32) << 24 }) else {
            log!("xhci: port {} wouldn't take an address", port);
            return;
        };
        self.devs.push(UsbDev { slot, port, ep0, input, pipes: Vec::new() });
        let di = self.devs.len() - 1;
        // the device descriptor, fixing endpoint 0's packet size first
        let mut dd = [0u8; 18];
        if !self.get_descriptor(di, 1, 0, 0, &mut dd[..8]) {
            return;
        }
        if dd[7] as u32 != mps0 && dd[7] >= 8 {
            unsafe {
                write_volatile((input + 4) as *mut u32, 0b10);
                let e0 = input + 2 * cs;
                write_volatile((e0 + 4) as *mut u32, 3 << 1 | 4 << 3 | (dd[7] as u32) << 16);
            }
            self.command(Trb { p: input as u64, s: 0, c: TRB_EVALUATE << 10 | (slot as u32) << 24 });
        }
        if !self.get_descriptor(di, 1, 0, 0, &mut dd) {
            return;
        }
        let (vendor, product) = (u16::from_le_bytes([dd[8], dd[9]]), u16::from_le_bytes([dd[10], dd[11]]));
        let (maker, name) = (self.string(di, dd[14]), self.string(di, dd[15]));
        // the configuration
        let mut head = [0u8; 9];
        if !self.get_descriptor(di, 2, 0, 0, &mut head) {
            return;
        }
        let total = (u16::from_le_bytes([head[2], head[3]]) as usize).clamp(9, 4096);
        let mut conf = alloc::vec![0u8; total];
        if !self.get_descriptor(di, 2, 0, 0, &mut conf) {
            return;
        }
        self.control(di, [0, 9, conf[5], 0, 0, 0, 0, 0], &mut []);
        let label = match (maker.is_empty(), name.is_empty()) {
            (_, false) if !maker.is_empty() && !name.starts_with(&maker) => format!("{} {}", maker, name),
            (_, false) => name,
            (false, true) => maker,
            _ => String::from("USB device"),
        };
        self.interfaces(di, &conf, speed, vendor, product, &label);
    }

    /// Walk a configuration: start its HID interfaces, list the rest.
    fn interfaces(&mut self, di: usize, conf: &[u8], speed: u32, vendor: u16, product: u16, label: &str) {
        let mut i = 0;
        let mut cur: Option<(u8, u8, u8, u8)> = None;
        let mut hid_len = 0usize;
        let mut started = Vec::new();
        let mut listed = false;
        while i + 2 <= conf.len() {
            let (len, kind) = (conf[i] as usize, conf[i + 1]);
            if len < 2 || i + len > conf.len() {
                break;
            }
            let d = &conf[i..i + len];
            match kind {
                4 if len >= 9 => {
                    cur = Some((d[2], d[5], d[6], d[7]));
                    hid_len = 0;
                    if d[5] != 3 && !listed {
                        self.info.push(DeviceInfo { name: String::from(label), kind: String::from(class_name(d[5], d[6], d[7])), ids: format!("{:04x}:{:04x}", vendor, product), driver: "No driver yet" });
                        listed = true;
                    }
                }
                0x21 if len >= 9 => hid_len = u16::from_le_bytes([d[7], d[8]]) as usize,
                5 if len >= 7 => {
                    let (addr, attr, mps, interval) = (d[2], d[3], u16::from_le_bytes([d[4], d[5]]) & 0x7FF, d[6]);
                    if let Some((num, 3, sub, proto)) = cur {
                        if attr & 3 == 3 && addr & 0x80 != 0 && !started.contains(&num) {
                            started.push(num);
                            let what = self.start_hid(di, num, sub, proto, addr, mps, interval, speed, hid_len, vendor);
                            self.info.push(DeviceInfo { name: String::from(label), kind: String::from(what.unwrap_or("HID device")), ids: format!("{:04x}:{:04x}", vendor, product), driver: if what.is_some() { "HydatekOS xHCI + HID" } else { "No driver yet" } });
                            listed = true;
                        }
                    }
                }
                _ => {}
            }
            i += len;
        }
        self.generation = self.generation.wrapping_add(1);
    }

    #[allow(clippy::too_many_arguments)]
    fn start_hid(&mut self, di: usize, iface: u8, sub: u8, _proto: u8, addr: u8, mps: u16, interval: u8, speed: u32, hid_len: usize, vendor: u16) -> Option<&'static str> {
        let mut rd_ = alloc::vec![0u8; if hid_len == 0 { 512 } else { hid_len.min(4096) }];
        if !self.control(di, [0x81, 6, 0, 0x22, iface, 0, rd_.len() as u8, (rd_.len() >> 8) as u8], &mut rd_) {
            return None;
        }
        let hid = HidInput::new(&rd_, vendor == 0x054C);
        if !hid.useful() {
            return None;
        }
        if sub == 1 {
            self.control(di, [0x21, 0x0B, 1, 0, iface, 0, 0, 0], &mut []);
        }
        self.control(di, [0x21, 0x0A, 0, 0, iface, 0, 0, 0], &mut []);
        for (rid, mut r) in hid.start_reports() {
            let l = r.len() as u16;
            self.control(di, [0x21, 0x09, rid, 3, iface, 0, l as u8, (l >> 8) as u8], &mut r);
        }
        // the endpoint's context
        let dci = (addr & 0xF) * 2 + 1;
        let ring = Ring::new()?;
        let cs = self.ctx;
        let dev = &self.devs[di];
        let (input, slot) = (dev.input, dev.slot);
        let xi = match speed {
            3 | 4 | 5 => (interval.clamp(1, 16) - 1) as u32,
            _ => {
                let frames = (interval.max(1) as u32) * 8;
                (31 - frames.leading_zeros()).clamp(3, 10)
            }
        };
        let max_dci = dev.pipes.iter().map(|p| p.dci).max().unwrap_or(1).max(dci);
        unsafe {
            core::ptr::write_bytes(input as *mut u8, 0, cs * 33);
            write_volatile((input + 4) as *mut u32, 1 | 1 << dci);
            let sl = input + cs;
            write_volatile(sl as *mut u32, (max_dci as u32) << 27 | speed << 20);
            write_volatile((sl + 4) as *mut u32, (dev.port as u32) << 16);
            let ep = input + (dci as usize + 1) * cs;
            write_volatile(ep as *mut u32, xi << 16);
            write_volatile((ep + 4) as *mut u32, 3 << 1 | 7 << 3 | (mps as u32) << 16);
            write_volatile((ep + 8) as *mut u64, ring.base as u64 | 1);
            write_volatile((ep + 16) as *mut u32, (mps as u32) << 16 | mps as u32);
        }
        let Some((1, _)) = self.command(Trb { p: input as u64, s: 0, c: TRB_CONFIGURE << 10 | (slot as u32) << 24 }) else { return None };
        let what = hid.what();
        let mut pipe = Pipe { dci, ring, bufs: Vec::new(), len: (mps as usize).clamp(8, 1024), hid };
        // keep eight transfers waiting
        for _ in 0..8 {
            let Some(b) = crate::efi::dma(1) else { break };
            let at = pipe.ring.push(Trb { p: b as u64, s: pipe.len as u32, c: TRB_NORMAL << 10 | IOC | ISP });
            pipe.bufs.push((at, b));
        }
        self.devs[di].pipes.push(pipe);
        self.ring_doorbell(slot, dci);
        log!("xhci: driving {} on port {} (interface {}, endpoint {:#x})", what, self.devs[di].port, iface, addr);
        Some(what)
    }

    pub fn driven(&self) -> usize {
        self.devs.iter().map(|d| d.pipes.len()).sum()
    }

    /// Read the event ring: reports from HID devices, ports changing.
    pub fn poll(&mut self, now: u64, out: &mut Vec<Event>) {
        for _ in 0..64 {
            let Some(e) = self.next_event() else { break };
            match e.c >> 10 & 0x3F {
                EV_TRANSFER => {
                    let (slot, dci) = ((e.c >> 24) as u8, (e.c >> 16 & 0x1F) as u8);
                    let code = e.s >> 24;
                    let Some(d) = self.devs.iter_mut().find(|d| d.slot == slot) else { continue };
                    let Some(p) = d.pipes.iter_mut().find(|p| p.dci == dci) else { continue };
                    let Some(k) = p.bufs.iter().position(|b| b.0 == e.p) else { continue };
                    let (_, buf) = p.bufs.remove(k);
                    if code == 1 || code == 13 {
                        let n = p.len.saturating_sub((e.s & 0xFF_FFFF) as usize);
                        let r = unsafe { core::slice::from_raw_parts(buf as *const u8, n) }.to_vec();
                        p.hid.report(&r, now, out);
                    }
                    // queue it again
                    if code == 1 || code == 13 {
                        let at = p.ring.push(Trb { p: buf as u64, s: p.len as u32, c: TRB_NORMAL << 10 | IOC | ISP });
                        p.bufs.push((at, buf));
                        fence(Ordering::SeqCst);
                        wr(self.db + 4 * slot as usize, dci as u32);
                    }
                }
                EV_PORT => {
                    let port = (e.p >> 24) as u8;
                    if !self.changed.contains(&port) {
                        self.changed.push(port);
                    }
                }
                _ => {}
            }
        }
        // a device plugged in or pulled out
        for port in core::mem::take(&mut self.changed) {
            if port == 0 || port > self.ports {
                continue;
            }
            let a = self.portsc(port);
            let v = rd(a);
            wr(a, (v & !PED) | (v & CHANGES));
            let had = self.devs.iter().position(|d| d.port == port);
            if v & CCS != 0 && had.is_none() {
                crate::efi::stall_ms(50);
                self.attach(port);
            } else if v & CCS == 0 {
                if let Some(i) = had {
                    let slot = self.devs[i].slot;
                    self.devs.remove(i);
                    // disable the slot (type 10)
                    self.command(Trb { p: 0, s: 0, c: 10 << 10 | (slot as u32) << 24 });
                    self.info.clear();
                    self.generation = self.generation.wrapping_add(1);
                    log!("xhci: device on port {} unplugged", port);
                }
            }
        }
    }
}

/// Take every xHCI controller HydatekOS may, and start them.
pub fn start_all() -> Vec<Xhci> {
    let mut out = Vec::new();
    let mut taken: Vec<Vec<u8>> = Vec::new();
    for d in pci::devices() {
        if d.class != (0x0C, 0x03, 0x30) {
            continue;
        }
        let path = d.path();
        if !pci::may_take(&path, &taken) {
            log!("xhci: {:02x}:{:02x}.{} stays with the firmware (boot disk or keyboard)", d.loc.0, d.loc.1, d.loc.2);
            continue;
        }
        match Xhci::start(&d) {
            Ok(x) => {
                out.push(x);
                taken.push(path);
            }
            Err(e) => log!("xhci: {:02x}:{:02x}.{}: {}", d.loc.0, d.loc.1, d.loc.2, e),
        }
    }
    out
}
