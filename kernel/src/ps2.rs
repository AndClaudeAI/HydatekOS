//! HydatekOS PS/2 mouse driver (i8042 auxiliary port), polled.
//!
//! Used when the firmware exposes no pointer device of its own. Handles the
//! IntelliMouse extension for the scroll wheel. The firmware's keyboard driver
//! keeps owning the keyboard port; we only consume bytes flagged as AUX.

use core::arch::asm;

const DATA: u16 = 0x60;
const CMD: u16 = 0x64;
const ST_OBF: u8 = 0x01;
const ST_IBF: u8 = 0x02;
const ST_AUX: u8 = 0x20;

unsafe fn inb(p: u16) -> u8 {
    let v: u8;
    asm!("in al, dx", out("al") v, in("dx") p, options(nomem, nostack, preserves_flags));
    v
}
unsafe fn outb(p: u16, v: u8) {
    asm!("out dx, al", in("dx") p, in("al") v, options(nomem, nostack, preserves_flags));
}

fn wait_write() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(CMD) } & ST_IBF == 0 {
            return true;
        }
    }
    false
}

fn read_aux(timeout: u32) -> Option<u8> {
    for _ in 0..timeout {
        let st = unsafe { inb(CMD) };
        if st & ST_OBF != 0 {
            let v = unsafe { inb(DATA) };
            if st & ST_AUX != 0 {
                return Some(v);
            }
        }
        crate::efi::stall_us(10);
    }
    None
}

fn ctl(cmd: u8) {
    if wait_write() {
        unsafe { outb(CMD, cmd) };
    }
}

fn mouse_cmd(b: u8) -> bool {
    ctl(0xD4);
    if wait_write() {
        unsafe { outb(DATA, b) };
    }
    read_aux(5000) == Some(0xFA)
}

pub struct Ps2Mouse {
    pkt: [u8; 4],
    n: usize,
    size: usize,
}

pub struct Packet {
    pub dx: i32,
    pub dy: i32,
    pub dz: i32,
    pub left: bool,
    pub right: bool,
}

impl Ps2Mouse {
    pub fn init() -> Option<Ps2Mouse> {
        if unsafe { inb(CMD) } == 0xFF {
            return None; // no i8042 controller
        }
        ctl(0xA8); // enable aux port
        // enable aux clock in the controller configuration byte
        ctl(0x20);
        let mut cfg = 0u8;
        for _ in 0..10000 {
            let st = unsafe { inb(CMD) };
            if st & ST_OBF != 0 && st & ST_AUX == 0 {
                cfg = unsafe { inb(DATA) };
                break;
            }
        }
        cfg &= !0x20;
        ctl(0x60);
        if wait_write() {
            unsafe { outb(DATA, cfg) };
        }
        if !mouse_cmd(0xF6) {
            return None;
        }
        // IntelliMouse wheel: sample rates 200, 100, 80 then read the device ID.
        for r in [200u8, 100, 80] {
            mouse_cmd(0xF3);
            mouse_cmd(r);
        }
        let mut size = 3;
        if mouse_cmd(0xF2) && read_aux(5000) == Some(3) {
            size = 4;
        }
        mouse_cmd(0xF3);
        mouse_cmd(100);
        if !mouse_cmd(0xF4) {
            return None;
        }
        Some(Ps2Mouse { pkt: [0; 4], n: 0, size })
    }

    /// Drain pending bytes; returns the accumulated movement if any packet completed.
    pub fn poll(&mut self) -> Option<Packet> {
        let mut out: Option<Packet> = None;
        for _ in 0..64 {
            let st = unsafe { inb(CMD) };
            if st & ST_OBF == 0 || st & ST_AUX == 0 {
                break;
            }
            let b = unsafe { inb(DATA) };
            if self.n == 0 && b & 0x08 == 0 {
                continue; // resync: first byte always has bit 3 set
            }
            self.pkt[self.n] = b;
            self.n += 1;
            if self.n == self.size {
                self.n = 0;
                let b0 = self.pkt[0];
                if b0 & 0xC0 != 0 {
                    continue; // overflow
                }
                let dx = self.pkt[1] as i32 - (((b0 as i32) << 4) & 0x100);
                let dy = self.pkt[2] as i32 - (((b0 as i32) << 3) & 0x100);
                let dz = if self.size == 4 { ((self.pkt[3] << 4) as i8 >> 4) as i32 } else { 0 };
                let p = out.get_or_insert(Packet { dx: 0, dy: 0, dz: 0, left: false, right: false });
                p.dx += dx;
                p.dy -= dy;
                p.dz += dz;
                p.left = b0 & 1 != 0;
                p.right = b0 & 2 != 0;
            }
        }
        out
    }
}
