//! HydatekOS's SATA driver: AHCI, for SATA SSDs and hard disks (and the
//! SATA controllers in older laptops, desktops and virtual machines).
//!
//! It takes an AHCI controller from the firmware (never the one holding the
//! boot disk), claims it from the BIOS, and for each port with a disk sets
//! up a command list and received-FIS area, asks the disk who it is
//! (IDENTIFY DEVICE) and reads and writes with READ/WRITE DMA EXT, polling.

use crate::pci;
use crate::storage::{ata_string, Disk};
use alloc::string::String;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

fn rd(a: usize) -> u32 {
    unsafe { read_volatile(a as *const u32) }
}
fn wr(a: usize, v: u32) {
    unsafe { write_volatile(a as *mut u32, v) }
}

const P_CLB: usize = 0x00;
const P_FB: usize = 0x08;
const P_IS: usize = 0x10;
const P_CMD: usize = 0x18;
const P_TFD: usize = 0x20;
const P_SIG: usize = 0x24;
const P_SSTS: usize = 0x28;
const P_SERR: usize = 0x30;
const P_CI: usize = 0x38;

/// One SATA disk.
pub struct SataDisk {
    port: usize,
    list: usize,
    table: usize,
    buf: usize,
    pub model: String,
    pub serial: String,
    pub sectors: u64,
}

impl SataDisk {
    /// Run one command (slot 0) moving `bytes` through the bounce page.
    fn command(&mut self, cmd: u8, lba: u64, count: u16, write: bool, bytes: usize) -> bool {
        let p = self.port;
        // the command header: a 5-dword FIS, one PRD entry
        unsafe {
            write_volatile(self.list as *mut u32, 5 | (write as u32) << 6 | 1 << 16);
            write_volatile((self.list + 4) as *mut u32, 0);
            write_volatile((self.list + 8) as *mut u64, self.table as u64);
            core::ptr::write_bytes(self.table as *mut u8, 0, 0x90);
            let f = self.table as *mut u8;
            *f = 0x27;
            *f.add(1) = 0x80;
            *f.add(2) = cmd;
            for k in 0..3 {
                *f.add(4 + k) = (lba >> (8 * k)) as u8;
                *f.add(8 + k) = (lba >> (24 + 8 * k)) as u8;
            }
            *f.add(7) = 0x40;
            *f.add(12) = count as u8;
            *f.add(13) = (count >> 8) as u8;
            let prd = self.table + 0x80;
            write_volatile(prd as *mut u64, self.buf as u64);
            write_volatile((prd + 12) as *mut u32, (bytes.max(2) - 1) as u32);
        }
        if !crate::efi::wait_until(1000, || rd(p + P_TFD) & 0x88 == 0) {
            return false;
        }
        wr(p + P_IS, 0xFFFF_FFFF);
        fence(Ordering::SeqCst);
        wr(p + P_CI, 1);
        let ok = crate::efi::wait_until(5000, || rd(p + P_CI) & 1 == 0 || rd(p + P_IS) & 1 << 30 != 0);
        ok && rd(p + P_IS) & 1 << 30 == 0 && rd(p + P_TFD) & 1 == 0
    }

    /// Write sectors (at most 4 KiB at a time).
    pub fn write(&mut self, lba: u64, data: &[u8]) -> bool {
        let n = data.len().min(4096) / 512 * 512;
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), self.buf as *mut u8, n) };
        self.command(0x35, lba, (n / 512) as u16, true, n)
    }
}

impl Disk for SataDisk {
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        let n = (buf.len().min(4096) / 512).max(1) * 512;
        if !self.command(0x25, lba, (n / 512) as u16, false, n) {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping(self.buf as *const u8, buf.as_mut_ptr(), n.min(buf.len())) };
        true
    }

    fn size(&self) -> u64 {
        self.sectors
    }
}

/// Take an AHCI controller and start its disks.
pub fn start(d: &pci::Dev) -> Result<Vec<SataDisk>, &'static str> {
    let abar = d.bar(5) as usize;
    if abar == 0 {
        return Err("no registers");
    }
    if !d.take() {
        return Err("the firmware wouldn't let go");
    }
    d.enable();
    // claim it from the BIOS
    if rd(abar + 0x24) & 1 != 0 {
        wr(abar + 0x28, rd(abar + 0x28) | 2);
        crate::efi::wait_until(100, || rd(abar + 0x28) & 1 == 0);
    }
    // AHCI mode
    wr(abar + 0x04, rd(abar + 0x04) | 1 << 31);
    let pi = rd(abar + 0x0C);
    let mut disks = Vec::new();
    for i in 0..32 {
        if pi & 1 << i == 0 {
            continue;
        }
        let p = abar + 0x100 + 0x80 * i;
        // a device, link up, and an ATA disk (not a CD drive)
        if rd(p + P_SSTS) & 0xF != 3 || rd(p + P_SIG) != 0x0000_0101 {
            continue;
        }
        // stop the port, give it our command list and FIS area, start it
        wr(p + P_CMD, rd(p + P_CMD) & !1);
        crate::efi::wait_until(500, || rd(p + P_CMD) & 1 << 15 == 0);
        wr(p + P_CMD, rd(p + P_CMD) & !(1 << 4));
        crate::efi::wait_until(500, || rd(p + P_CMD) & 1 << 14 == 0);
        let (Some(list), Some(table), Some(buf)) = (crate::efi::dma(1), crate::efi::dma(1), crate::efi::dma(1)) else { break };
        wr(p + P_CLB, list as u32);
        wr(p + P_CLB + 4, (list as u64 >> 32) as u32);
        let fis = list + 0x400;
        wr(p + P_FB, fis as u32);
        wr(p + P_FB + 4, (fis as u64 >> 32) as u32);
        wr(p + P_SERR, 0xFFFF_FFFF);
        wr(p + P_IS, 0xFFFF_FFFF);
        wr(p + P_CMD, rd(p + P_CMD) | 1 << 4);
        wr(p + P_CMD, rd(p + P_CMD) | 1);
        let mut disk = SataDisk { port: p, list, table, buf, model: String::new(), serial: String::new(), sectors: 0 };
        if !disk.command(0xEC, 0, 0, false, 512) {
            log!("ahci: port {}: IDENTIFY failed", i);
            continue;
        }
        let id = unsafe { core::slice::from_raw_parts(buf as *const u8, 512) };
        disk.model = ata_string(id, 27, 20);
        disk.serial = ata_string(id, 10, 10);
        disk.sectors = u64::from_le_bytes(id[200..208].try_into().unwrap());
        if disk.sectors == 0 {
            disk.sectors = u32::from_le_bytes(id[120..124].try_into().unwrap()) as u64;
        }
        log!("ahci: port {}: {} ({}), {} sectors", i, disk.model, disk.serial, disk.sectors);
        disks.push(disk);
    }
    Ok(disks)
}
