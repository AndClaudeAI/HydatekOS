//! Intel gigabit Ethernet, driven by HydatekOS itself: the 8254x ("e1000")
//! family, the 82574, and the LAN built into Intel chipsets since 2009
//! (82577/82578 with the 5 Series, 82579, I217, I218, I219), such as the
//! 82577LM in an HP EliteBook 8440w.
//!
//! For computers whose firmware has no network driver of its own (or none
//! it will lend). Where the firmware does have one, HydatekOS still prefers
//! this driver for the cards it knows, and the firmware's for the rest
//! (snp.rs).
//!
//! The card is programmed through its registers (BAR 0) with the classic
//! "legacy" descriptors every member of the family understands: 32 receive
//! and 32 transmit descriptors in rings below 4 GiB, 2 KiB buffers,
//! polled (no interrupts). The PHY is left to negotiate the link itself.
//!
//! Chipset LAN (82577 on) shares its PHY with the Management Engine (vPro,
//! AMT), so it isn't reset; its receive and transmit units are stopped,
//! reprogrammed and started again, which is what the ME expects of a host
//! driver.

use crate::pci;
use alloc::string::String;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

// registers
const CTRL: usize = 0x0000;
const STATUS: usize = 0x0008;
const EERD: usize = 0x0014;
const IMC: usize = 0x00D8;
const RCTL: usize = 0x0100;
const TCTL: usize = 0x0400;
const TIPG: usize = 0x0410;
const RDBAL: usize = 0x2800;
const RDBAH: usize = 0x2804;
const RDLEN: usize = 0x2808;
const RDH: usize = 0x2810;
const RDT: usize = 0x2818;
const TDBAL: usize = 0x3800;
const TDBAH: usize = 0x3804;
const TDLEN: usize = 0x3808;
const TDH: usize = 0x3810;
const TDT: usize = 0x3818;
const MTA: usize = 0x5200;
const RAL0: usize = 0x5400;
const RAH0: usize = 0x5404;

const CTRL_ASDE: u32 = 1 << 5;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_RST: u32 = 1 << 26;
const CTRL_LRST: u32 = 1 << 3;
const CTRL_ILOS: u32 = 1 << 7;
const CTRL_VME: u32 = 1 << 30;
const CTRL_PHY_RST: u32 = 1 << 31;
const STATUS_LU: u32 = 1 << 1;

const RCTL_EN: u32 = 1 << 1;
const RCTL_MPE: u32 = 1 << 4;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;
const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;

const RING: usize = 32;
const BUF: usize = 2048;

/// Which family a card is, which decides how it's started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// 8254x: the original e1000 (EEPROM read with the done bit at 4)
    E1000,
    /// 82571-82574, 82583: e1000e (EERD done bit at 1)
    E1000e,
    /// LAN in the chipset: 82577/82578 (5 Series), 82579, I217, I218, I219
    Chipset,
}

/// The Intel cards this driver knows, by PCI device ID.
pub fn family(vendor: u16, device: u16) -> Option<(Family, &'static str)> {
    if vendor != 0x8086 {
        return None;
    }
    Some(match device {
        0x1000 | 0x1001 | 0x1004 | 0x1008 | 0x1009 | 0x100C | 0x100D | 0x100E | 0x100F | 0x1010 | 0x1011 | 0x1012 | 0x1013 | 0x1015 | 0x1016 | 0x1017 | 0x1018 | 0x1019 | 0x101A | 0x101D | 0x101E | 0x1026 | 0x1027 | 0x1028 | 0x1075 | 0x1076 | 0x1077 | 0x1078 | 0x1079 | 0x107A | 0x107B | 0x107C | 0x108A | 0x1099 | 0x10B5 => (Family::E1000, "Intel PRO/1000 (8254x)"),
        0x105E | 0x105F | 0x1060 | 0x107D | 0x107E | 0x107F | 0x108B | 0x108C | 0x109A | 0x10A4 | 0x10A5 | 0x10BC | 0x10B9 => (Family::E1000e, "Intel PRO/1000 (82571-82573)"),
        0x10D3 | 0x10F6 | 0x150C | 0x1501 => (Family::E1000e, "Intel 82574 / 82583"),
        0x10EA => (Family::Chipset, "Intel 82577LM"),
        0x10EB => (Family::Chipset, "Intel 82577LC"),
        0x10EF => (Family::Chipset, "Intel 82578DM"),
        0x10F0 => (Family::Chipset, "Intel 82578DC"),
        0x1502 => (Family::Chipset, "Intel 82579LM"),
        0x1503 => (Family::Chipset, "Intel 82579V"),
        0x153A => (Family::Chipset, "Intel I217-LM"),
        0x153B => (Family::Chipset, "Intel I217-V"),
        0x155A | 0x1559 | 0x15A0 | 0x15A1 | 0x15A2 | 0x15A3 => (Family::Chipset, "Intel I218"),
        0x156F | 0x1570 | 0x15B7 | 0x15B8 | 0x15B9 | 0x15BB | 0x15BC | 0x15BD | 0x15BE | 0x15D6 | 0x15D7 | 0x15D8 | 0x15E3 | 0x0D4E | 0x0D4F | 0x0D4C | 0x0D4D | 0x15FB | 0x15FC | 0x15F9 | 0x15FA => (Family::Chipset, "Intel I219"),
        _ => return None,
    })
}

/// A legacy receive descriptor.
#[repr(C)]
struct RxDesc {
    addr: u64,
    len: u16,
    csum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

/// A legacy transmit descriptor.
#[repr(C)]
struct TxDesc {
    addr: u64,
    len: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

pub struct E1000 {
    base: usize,
    rx: usize,
    tx: usize,
    rx_bufs: usize,
    tx_bufs: usize,
    rx_cur: usize,
    tx_cur: usize,
    pub mac: [u8; 6],
    pub name: String,
    pub at: (u8, u8, u8),
}

impl E1000 {
    fn rd(&self, r: usize) -> u32 {
        unsafe { read_volatile((self.base + r) as *const u32) }
    }
    fn wr(&self, r: usize, v: u32) {
        unsafe { write_volatile((self.base + r) as *mut u32, v) }
    }

    /// Find the first Intel card this driver knows, take it from the
    /// firmware and start it.
    pub fn find() -> Option<E1000> {
        for d in pci::devices() {
            let Some((fam, model)) = family(d.vendor, d.device) else { continue };
            log!("e1000: {} ({:04x}:{:04x}) at {:02x}:{:02x}.{}", model, d.vendor, d.device, d.loc.0, d.loc.1, d.loc.2);
            match Self::start(&d, fam, model) {
                Some(n) => return Some(n),
                None => log!("e1000: couldn't start it; the firmware's driver is used if there is one"),
            }
        }
        None
    }

    fn start(d: &pci::Dev, fam: Family, model: &str) -> Option<E1000> {
        let base = d.bar(0) as usize;
        if base == 0 {
            log!("e1000: no register window");
            return None;
        }
        // the firmware's driver lets go, then memory and bus mastering on
        d.take();
        d.enable();
        let mut n = E1000 { base, rx: 0, tx: 0, rx_bufs: 0, tx_bufs: 0, rx_cur: 0, tx_cur: 0, mac: [0; 6], name: String::new(), at: d.loc };
        // the address the firmware (or the card's NVM) put in receive address 0
        let mut mac = n.ral_mac();
        n.wr(IMC, 0xFFFF_FFFF);
        n.wr(RCTL, 0);
        n.wr(TCTL, TCTL_PSP);
        crate::efi::stall_us(10_000);
        if fam != Family::Chipset {
            // a clean start: the card reloads its settings from its EEPROM
            n.wr(CTRL, n.rd(CTRL) | CTRL_RST);
            crate::efi::stall_us(10_000);
            if !crate::efi::wait_until(100, || n.rd(CTRL) & CTRL_RST == 0) {
                log!("e1000: reset didn't finish");
                return None;
            }
            n.wr(IMC, 0xFFFF_FFFF);
            if mac == [0; 6] {
                mac = n.ral_mac();
            }
        }
        if mac == [0; 6] {
            mac = n.eeprom_mac(fam).unwrap_or([0; 6]);
        }
        if mac == [0; 6] || mac[0] & 1 != 0 {
            log!("e1000: no usable MAC address");
            return None;
        }
        n.mac = mac;
        // receive address 0 = ours, valid
        n.wr(RAL0, u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
        n.wr(RAH0, u16::from_le_bytes([mac[4], mac[5]]) as u32 | 1 << 31);
        for i in 0..128 {
            n.wr(MTA + 4 * i, 0);
        }
        // link: let the PHY negotiate; no loopback, no VLANs
        let ctrl = n.rd(CTRL) & !(CTRL_LRST | CTRL_PHY_RST | CTRL_ILOS | CTRL_VME);
        n.wr(CTRL, ctrl | CTRL_SLU | CTRL_ASDE);
        // rings and buffers, below 4 GiB for the card's DMA
        let pages = |bytes: usize| (bytes + 4095) / 4096;
        n.rx = crate::efi::dma(pages(RING * 16))?;
        n.tx = crate::efi::dma(pages(RING * 16))?;
        n.rx_bufs = crate::efi::dma(pages(RING * BUF))?;
        n.tx_bufs = crate::efi::dma(pages(RING * BUF))?;
        for i in 0..RING {
            let d = n.rxd(i);
            unsafe {
                write_volatile(&mut (*d).addr, (n.rx_bufs + i * BUF) as u64);
                write_volatile(&mut (*d).status, 0);
            }
            let t = n.txd(i);
            unsafe {
                write_volatile(&mut (*t).addr, (n.tx_bufs + i * BUF) as u64);
                write_volatile(&mut (*t).status, 1); // free
                write_volatile(&mut (*t).cmd, 0);
            }
        }
        fence(Ordering::SeqCst);
        n.wr(RDBAL, n.rx as u32);
        n.wr(RDBAH, (n.rx as u64 >> 32) as u32);
        n.wr(RDLEN, (RING * 16) as u32);
        n.wr(RDH, 0);
        n.wr(RDT, (RING - 1) as u32);
        n.wr(TDBAL, n.tx as u32);
        n.wr(TDBAH, (n.tx as u64 >> 32) as u32);
        n.wr(TDLEN, (RING * 16) as u32);
        n.wr(TDH, 0);
        n.wr(TDT, 0);
        // receive: on, broadcasts, every multicast (mDNS), 2 KiB buffers, strip the CRC
        n.wr(RCTL, RCTL_EN | RCTL_BAM | RCTL_MPE | RCTL_SECRC);
        // transmit: on, pad short frames, collision threshold and distance for full duplex
        n.wr(TIPG, 0x0060_200A);
        n.wr(TCTL, TCTL_EN | TCTL_PSP | 0x0F << 4 | 0x3F << 12);
        n.name = alloc::format!("{} ({:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})", model, mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
        // the link can take a few seconds to come up; it's checked as it goes
        crate::efi::wait_until(3000, || n.rd(STATUS) & STATUS_LU != 0);
        log!("e1000: HydatekOS drives {}, link {}", n.name, if n.link_up() { "up" } else { "not up yet" });
        Some(n)
    }

    fn rxd(&self, i: usize) -> *mut RxDesc {
        (self.rx + i * 16) as *mut RxDesc
    }
    fn txd(&self, i: usize) -> *mut TxDesc {
        (self.tx + i * 16) as *mut TxDesc
    }

    fn ral_mac(&self) -> [u8; 6] {
        let (lo, hi) = (self.rd(RAL0), self.rd(RAH0));
        if hi & 1 << 31 == 0 {
            return [0; 6];
        }
        let l = lo.to_le_bytes();
        let h = hi.to_le_bytes();
        [l[0], l[1], l[2], l[3], h[0], h[1]]
    }

    /// The address from the card's EEPROM/NVM, words 0-2.
    fn eeprom_mac(&self, fam: Family) -> Option<[u8; 6]> {
        let (shift, done) = if fam == Family::E1000 { (8, 1 << 4) } else { (2, 1 << 1) };
        let mut mac = [0u8; 6];
        for w in 0..3u32 {
            self.wr(EERD, 1 | w << shift);
            if !crate::efi::wait_until(10, || self.rd(EERD) & done != 0) {
                return None;
            }
            let v = (self.rd(EERD) >> 16) as u16;
            mac[w as usize * 2..w as usize * 2 + 2].copy_from_slice(&v.to_le_bytes());
        }
        Some(mac)
    }

    pub fn link_up(&self) -> bool {
        self.rd(STATUS) & STATUS_LU != 0
    }

    /// Queue a frame (up to 1514 bytes); false if the ring is full.
    pub fn send(&mut self, frame: &[u8]) -> bool {
        if frame.len() > 1514 {
            return false;
        }
        let i = self.tx_cur;
        let d = self.txd(i);
        unsafe {
            if read_volatile(&(*d).status) & 1 == 0 {
                return false; // the card hasn't sent the last one in this slot
            }
            let buf = (self.tx_bufs + i * BUF) as *mut u8;
            core::ptr::copy_nonoverlapping(frame.as_ptr(), buf, frame.len());
            write_volatile(&mut (*d).len, frame.len() as u16);
            write_volatile(&mut (*d).status, 0);
            // end of packet, insert the CRC, report status
            write_volatile(&mut (*d).cmd, 0x01 | 0x02 | 0x08);
        }
        self.tx_cur = (i + 1) % RING;
        fence(Ordering::SeqCst);
        self.wr(TDT, self.tx_cur as u32);
        true
    }

    /// The next received frame, if there is one.
    pub fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let i = self.rx_cur;
        let d = self.rxd(i);
        let (status, errors, len) = unsafe { (read_volatile(&(*d).status), read_volatile(&(*d).errors), read_volatile(&(*d).len) as usize) };
        if status & 1 == 0 {
            return None;
        }
        fence(Ordering::SeqCst);
        // whole frames only (end of packet), without errors
        let ok = status & 2 != 0 && errors == 0 && len <= buf.len();
        if ok {
            unsafe { core::ptr::copy_nonoverlapping((self.rx_bufs + i * BUF) as *const u8, buf.as_mut_ptr(), len) };
        }
        unsafe { write_volatile(&mut (*d).status, 0) };
        fence(Ordering::SeqCst);
        // give the descriptor back
        self.wr(RDT, i as u32);
        self.rx_cur = (i + 1) % RING;
        if ok { Some(len) } else { self.recv(buf) }
    }
}
