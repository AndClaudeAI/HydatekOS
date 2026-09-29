//! I2C, and HID over I2C: how most laptop touchpads (and the haptic ones),
//! touch screens and pens are attached, on Intel, AMD and ARM machines alike.
//!
//! - `DesignWare`: the Synopsys DesignWare I2C controller, the one in Intel
//!   (LPSS) and AMD laptops and in many ARM SoCs; polled, register level;
//! - `I2cHid`: Microsoft's HID over I2C protocol on any `Bus`: the device's
//!   HID descriptor, its report descriptor, input reports, commands (reset,
//!   power, get/set report) and output reports. Its reports go through the
//!   same HID code as USB devices (hid.rs, touchpad.rs).
//!
//! What's not here yet: finding the devices. Their controller's address,
//! the device's I2C address and its HID descriptor register come from ACPI
//! (a PNP0C50 device's _CRS and _DSM), which needs an AML interpreter; hw.rs
//! only counts them for Settings for now. Host-tested against a simulated
//! controller and touchpad.

// not called yet on real machines: see "What's not here yet" above
#![allow(dead_code)]

use alloc::vec::Vec;

/// Something that moves bytes to and from I2C devices.
pub trait Bus {
    /// Write `w` to the device at `addr`, then (with a repeated start) read
    /// `r.len()` bytes. Either may be empty. False if the device didn't answer.
    fn xfer(&mut self, addr: u16, w: &[u8], r: &mut [u8]) -> bool;
}

// ---- the DesignWare controller -------------------------------------------------------

/// A controller's registers.
pub trait Regs {
    fn rd(&mut self, off: usize) -> u32;
    fn wr(&mut self, off: usize, v: u32);
}

/// Registers at a physical address (the firmware maps memory 1:1).
#[allow(dead_code)]
pub struct Mmio(pub usize);

impl Regs for Mmio {
    fn rd(&mut self, off: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.0 + off) as *const u32) }
    }
    fn wr(&mut self, off: usize, v: u32) {
        unsafe { core::ptr::write_volatile((self.0 + off) as *mut u32, v) }
    }
}

pub const IC_CON: usize = 0x00;
pub const IC_TAR: usize = 0x04;
pub const IC_DATA_CMD: usize = 0x10;
pub const IC_FS_SCL_HCNT: usize = 0x1C;
pub const IC_FS_SCL_LCNT: usize = 0x20;
pub const IC_INTR_MASK: usize = 0x30;
pub const IC_RAW_INTR_STAT: usize = 0x34;
pub const IC_CLR_TX_ABRT: usize = 0x54;
pub const IC_ENABLE: usize = 0x6C;
pub const IC_STATUS: usize = 0x70;
pub const IC_ENABLE_STATUS: usize = 0x9C;
pub const IC_COMP_TYPE: usize = 0xFC;

/// IC_COMP_TYPE of a DesignWare controller ("DW" 0x0140)
pub const DW_COMP_TYPE: u32 = 0x4457_0140;

pub const CMD_READ: u32 = 1 << 8;
pub const CMD_STOP: u32 = 1 << 9;
pub const CMD_RESTART: u32 = 1 << 10;
const ST_TFNF: u32 = 1 << 1;
const ST_RFNE: u32 = 1 << 3;
const INTR_TX_ABRT: u32 = 1 << 6;
/// the transmit FIFO: reads in flight at once
const FIFO: usize = 8;

pub struct DesignWare<R: Regs> {
    pub regs: R,
    /// polls before giving up on a transfer
    pub patience: u32,
}

impl<R: Regs> DesignWare<R> {
    /// Set the controller up as a 400 kHz master. None if it isn't one.
    /// `clock_mhz`: its input clock (from ACPI; 100-133 MHz is typical).
    pub fn new(mut regs: R, clock_mhz: u32) -> Option<DesignWare<R>> {
        if regs.rd(IC_COMP_TYPE) != DW_COMP_TYPE {
            return None;
        }
        regs.wr(IC_ENABLE, 0);
        // fast mode, master, restarts allowed, not a slave
        regs.wr(IC_CON, 0x01 | 2 << 1 | 1 << 5 | 1 << 6);
        // 400 kHz: 1.3 us low, 0.6 us high (plus rise and fall)
        regs.wr(IC_FS_SCL_HCNT, clock_mhz * 6 / 10);
        regs.wr(IC_FS_SCL_LCNT, clock_mhz * 13 / 10);
        regs.wr(IC_INTR_MASK, 0);
        Some(DesignWare { regs, patience: 200_000 })
    }

    fn target(&mut self, addr: u16) {
        self.regs.wr(IC_ENABLE, 0);
        let mut n = 0;
        while self.regs.rd(IC_ENABLE_STATUS) & 1 != 0 && n < self.patience {
            n += 1;
        }
        self.regs.wr(IC_TAR, addr as u32 & 0x3FF);
        self.regs.wr(IC_ENABLE, 1);
    }

    fn aborted(&mut self) -> bool {
        if self.regs.rd(IC_RAW_INTR_STAT) & INTR_TX_ABRT != 0 {
            self.regs.rd(IC_CLR_TX_ABRT);
            return true;
        }
        false
    }
}

impl<R: Regs> Bus for DesignWare<R> {
    fn xfer(&mut self, addr: u16, w: &[u8], r: &mut [u8]) -> bool {
        if w.is_empty() && r.is_empty() {
            return true;
        }
        self.target(addr);
        let mut n = 0;
        for (i, &b) in w.iter().enumerate() {
            while self.regs.rd(IC_STATUS) & ST_TFNF == 0 {
                n += 1;
                if n > self.patience || self.aborted() {
                    return false;
                }
            }
            let stop = if i + 1 == w.len() && r.is_empty() { CMD_STOP } else { 0 };
            self.regs.wr(IC_DATA_CMD, b as u32 | stop);
        }
        let (mut issued, mut got) = (0, 0);
        while got < r.len() {
            if self.aborted() {
                return false;
            }
            if issued < r.len() && issued - got < FIFO && self.regs.rd(IC_STATUS) & ST_TFNF != 0 {
                let mut c = CMD_READ;
                if issued == 0 && !w.is_empty() {
                    c |= CMD_RESTART;
                }
                if issued + 1 == r.len() {
                    c |= CMD_STOP;
                }
                self.regs.wr(IC_DATA_CMD, c);
                issued += 1;
            }
            if self.regs.rd(IC_STATUS) & ST_RFNE != 0 {
                r[got] = self.regs.rd(IC_DATA_CMD) as u8;
                got += 1;
                n = 0;
            } else {
                n += 1;
                if n > self.patience {
                    return false;
                }
            }
        }
        !self.aborted()
    }
}

// ---- HID over I2C --------------------------------------------------------------------

/// A HID over I2C device's HID descriptor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HidDesc {
    pub report_desc_len: u16,
    pub report_desc_reg: u16,
    pub input_reg: u16,
    pub max_input: u16,
    pub output_reg: u16,
    pub max_output: u16,
    pub command_reg: u16,
    pub data_reg: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

impl HidDesc {
    pub fn parse(b: &[u8]) -> Option<HidDesc> {
        let w = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        // 30 bytes, version 1.00
        if b.len() < 30 || w(0) != 30 || w(2) != 0x0100 {
            return None;
        }
        Some(HidDesc {
            report_desc_len: w(4),
            report_desc_reg: w(6),
            input_reg: w(8),
            max_input: w(10),
            output_reg: w(12),
            max_output: w(14),
            command_reg: w(16),
            data_reg: w(18),
            vendor: w(20),
            product: w(22),
            version: w(24),
        })
    }
}

const OP_RESET: u8 = 1;
const OP_GET_REPORT: u8 = 2;
const OP_SET_REPORT: u8 = 3;
const OP_SET_POWER: u8 = 8;

/// Report types in commands.
pub const INPUT: u8 = 1;
pub const OUTPUT: u8 = 2;
pub const FEATURE: u8 = 3;

pub struct I2cHid {
    pub addr: u16,
    pub desc: HidDesc,
}

fn le(v: u16) -> [u8; 2] {
    v.to_le_bytes()
}

impl I2cHid {
    /// Start a device at `addr` whose HID descriptor is at register
    /// `desc_reg` (both from ACPI): read its descriptor, power it on, reset it.
    pub fn start(bus: &mut dyn Bus, addr: u16, desc_reg: u16) -> Option<I2cHid> {
        let mut b = [0u8; 30];
        if !bus.xfer(addr, &le(desc_reg), &mut b) {
            return None;
        }
        let desc = HidDesc::parse(&b)?;
        let d = I2cHid { addr, desc };
        d.power(bus, true);
        d.command(bus, OP_RESET, 0, 0, &[]);
        // the reset's answer: an empty input report
        let mut r = alloc::vec![0u8; desc.max_input.max(2) as usize];
        for _ in 0..50 {
            if bus.xfer(addr, &[], &mut r) && r[0] == 0 && r[1] == 0 {
                break;
            }
        }
        Some(d)
    }

    fn command(&self, bus: &mut dyn Bus, op: u8, kind: u8, id: u8, tail: &[u8]) -> bool {
        let mut w = Vec::new();
        w.extend_from_slice(&le(self.desc.command_reg));
        // report ids of 15 and up go in a byte of their own
        let low = kind << 4 | id.min(15);
        w.push(low);
        w.push(op);
        if id >= 15 {
            w.push(id);
        }
        w.extend_from_slice(tail);
        bus.xfer(self.addr, &w, &mut [])
    }

    /// Turn the device on or put it to sleep.
    pub fn power(&self, bus: &mut dyn Bus, on: bool) -> bool {
        self.command(bus, OP_SET_POWER, 0, if on { 0 } else { 1 }, &[])
    }

    pub fn report_descriptor(&self, bus: &mut dyn Bus) -> Option<Vec<u8>> {
        let mut r = alloc::vec![0u8; self.desc.report_desc_len as usize];
        bus.xfer(self.addr, &le(self.desc.report_desc_reg), &mut r).then_some(r)
    }

    /// The next input report (with its report id, without the length), or
    /// None if there's nothing new.
    pub fn read_input(&self, bus: &mut dyn Bus) -> Option<Vec<u8>> {
        let mut r = alloc::vec![0u8; self.desc.max_input.max(2) as usize];
        if !bus.xfer(self.addr, &[], &mut r) {
            return None;
        }
        let n = u16::from_le_bytes([r[0], r[1]]) as usize;
        if n <= 2 || n > r.len() {
            return None;
        }
        Some(r[2..n].to_vec())
    }

    /// Ask for a report (a feature report such as a haptic touchpad's
    /// waveform list); the answer starts with the report id if it has one.
    pub fn get_report(&self, bus: &mut dyn Bus, kind: u8, id: u8, len: usize) -> Option<Vec<u8>> {
        let mut w = Vec::new();
        w.extend_from_slice(&le(self.desc.command_reg));
        w.push(kind << 4 | id.min(15));
        w.push(OP_GET_REPORT);
        if id >= 15 {
            w.push(id);
        }
        w.extend_from_slice(&le(self.desc.data_reg));
        let mut r = alloc::vec![0u8; len + 2];
        if !bus.xfer(self.addr, &w, &mut r) {
            return None;
        }
        let n = (u16::from_le_bytes([r[0], r[1]]) as usize).clamp(2, r.len());
        Some(r[2..n].to_vec())
    }

    /// Send a report through the command register (feature reports, the
    /// touchpad's input mode). `report` starts with its id if it has one.
    pub fn set_report(&self, bus: &mut dyn Bus, kind: u8, id: u8, report: &[u8]) -> bool {
        let mut tail = Vec::new();
        tail.extend_from_slice(&le(self.desc.data_reg));
        tail.extend_from_slice(&le(report.len() as u16 + 2));
        tail.extend_from_slice(report);
        self.command(bus, OP_SET_REPORT, kind, id, &tail)
    }

    /// Send an output report (a haptic waveform) on the output register.
    pub fn output(&self, bus: &mut dyn Bus, report: &[u8]) -> bool {
        let mut w = Vec::new();
        w.extend_from_slice(&le(self.desc.output_reg));
        w.extend_from_slice(&le(report.len() as u16 + 2));
        w.extend_from_slice(report);
        bus.xfer(self.addr, &w, &mut [])
    }
}
