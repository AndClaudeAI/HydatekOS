//! HydatekOS's NVMe driver: the SSDs in today's laptops and desktops.
//!
//! It takes an NVMe controller from the firmware (never the one holding the
//! boot disk), resets it, sets up the admin queues, asks who it is
//! (Identify), creates one I/O queue pair and reads and writes 512-byte
//! (or 4 KiB) blocks through it, polling for completions.

use crate::pci;
use crate::storage::Disk;
use alloc::string::String;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

const QSIZE: usize = 64;

fn rd32(a: usize) -> u32 {
    unsafe { read_volatile(a as *const u32) }
}
fn wr32(a: usize, v: u32) {
    unsafe { write_volatile(a as *mut u32, v) }
}
fn rd64(a: usize) -> u64 {
    rd32(a) as u64 | (rd32(a + 4) as u64) << 32
}
fn wr64(a: usize, v: u64) {
    wr32(a, v as u32);
    wr32(a + 4, (v >> 32) as u32);
}

struct Queue {
    sq: usize,
    cq: usize,
    tail: usize,
    head: usize,
    phase: u16,
    id: u16,
    cid: u16,
}

pub struct Nvme {
    base: usize,
    stride: usize,
    admin: Queue,
    io: Queue,
    /// a page for data (and a second for transfers crossing a page)
    buf: usize,
    pub model: String,
    pub serial: String,
    /// namespace 1: blocks and block size
    pub blocks: u64,
    pub block: u32,
    timeout_ms: u64,
}

impl Nvme {
    pub fn start(d: &pci::Dev) -> Result<Nvme, &'static str> {
        let base = d.bar(0) as usize;
        if base == 0 {
            return Err("no registers");
        }
        if !d.take() {
            return Err("the firmware wouldn't let go");
        }
        d.enable();
        let cap = rd64(base);
        let stride = 4 << ((cap >> 32) & 0xF);
        let timeout_ms = ((cap >> 24) & 0xFF).max(1) * 500;
        // disable, wait until it's not ready
        wr32(base + 0x14, rd32(base + 0x14) & !1);
        if !crate::efi::wait_until(timeout_ms, || rd32(base + 0x1C) & 1 == 0) {
            return Err("wouldn't stop");
        }
        let mk = |id| -> Option<Queue> { Some(Queue { sq: crate::efi::dma(1)?, cq: crate::efi::dma(1)?, tail: 0, head: 0, phase: 1, id, cid: 0 }) };
        let admin = mk(0).ok_or("no memory")?;
        wr32(base + 0x24, ((QSIZE - 1) << 16 | (QSIZE - 1)) as u32);
        wr64(base + 0x28, admin.sq as u64);
        wr64(base + 0x30, admin.cq as u64);
        // enable: 4 KiB pages, 64-byte submissions, 16-byte completions
        wr32(base + 0x14, 6 << 16 | 4 << 20 | 1);
        if !crate::efi::wait_until(timeout_ms, || rd32(base + 0x1C) & 3 == 1) {
            return Err("wouldn't start");
        }
        let io = mk(1).ok_or("no memory")?;
        let buf = crate::efi::dma(2).ok_or("no memory")?;
        let mut n = Nvme { base, stride, admin, io, buf, model: String::new(), serial: String::new(), blocks: 0, block: 512, timeout_ms };
        // who is it
        n.admin_cmd(0x06, 0, n.buf as u64, 1, 0).ok_or("Identify failed")?;
        let id = unsafe { core::slice::from_raw_parts(n.buf as *const u8, 4096) };
        n.serial = String::from_utf8_lossy(&id[4..24]).trim().into();
        n.model = String::from_utf8_lossy(&id[24..64]).trim().into();
        // namespace 1
        n.admin_cmd(0x06, 1, n.buf as u64, 0, 0).ok_or("Identify namespace failed")?;
        let ns = unsafe { core::slice::from_raw_parts(n.buf as *const u8, 4096) };
        n.blocks = u64::from_le_bytes(ns[0..8].try_into().unwrap());
        let fmt = (ns[26] & 0xF) as usize;
        let lbads = ns[128 + 4 * fmt + 2];
        n.block = 1 << lbads.clamp(9, 12);
        // the I/O queues: completion queue 1, then submission queue 1
        let (cq, sq) = (n.io.cq as u64, n.io.sq as u64);
        n.admin_cmd(0x05, 0, cq, ((QSIZE - 1) << 16 | 1) as u32, 1).ok_or("no completion queue")?;
        n.admin_cmd(0x01, 0, sq, ((QSIZE - 1) << 16 | 1) as u32, 1 << 16 | 1).ok_or("no submission queue")?;
        let (b, dv, f) = d.loc;
        log!("nvme: {:02x}:{:02x}.{} {} ({}), {} blocks of {} bytes", b, dv, f, n.model, n.serial, n.blocks, n.block);
        Ok(n)
    }

    /// Submit a command on a queue and wait for it. The completion's status
    /// is 0 on success.
    fn submit(&mut self, admin: bool, cmd: [u32; 16]) -> Option<u32> {
        let (base, stride, tmo) = (self.base, self.stride, self.timeout_ms);
        let q = if admin { &mut self.admin } else { &mut self.io };
        q.cid = q.cid.wrapping_add(1);
        let mut c = cmd;
        c[0] |= (q.cid as u32) << 16;
        let at = q.sq + q.tail * 64;
        for (i, w) in c.iter().enumerate() {
            unsafe { write_volatile((at + 4 * i) as *mut u32, *w) };
        }
        q.tail = (q.tail + 1) % QSIZE;
        fence(Ordering::SeqCst);
        wr32(base + 0x1000 + (2 * q.id as usize) * stride, q.tail as u32);
        let e = q.cq + q.head * 16;
        let phase = q.phase;
        if !crate::efi::wait_until(tmo.min(5000), || unsafe { read_volatile((e + 14) as *const u16) } & 1 == phase) {
            return None;
        }
        let dw0 = unsafe { read_volatile(e as *const u32) };
        let status = unsafe { read_volatile((e + 14) as *const u16) } >> 1;
        q.head = (q.head + 1) % QSIZE;
        if q.head == 0 {
            q.phase ^= 1;
        }
        wr32(base + 0x1000 + (2 * q.id as usize + 1) * stride, q.head as u32);
        (status == 0).then_some(dw0)
    }

    fn admin_cmd(&mut self, op: u32, nsid: u32, prp: u64, cdw10: u32, cdw11: u32) -> Option<u32> {
        let mut c = [0u32; 16];
        c[0] = op;
        c[1] = nsid;
        c[6] = prp as u32;
        c[7] = (prp >> 32) as u32;
        c[10] = cdw10;
        c[11] = cdw11;
        self.submit(true, c)
    }

    /// Read or write `count` blocks at `lba` through the bounce pages (at
    /// most 8 KiB at a time).
    fn rw(&mut self, write: bool, lba: u64, count: u32) -> bool {
        let prp1 = self.buf as u64;
        let bytes = count as u64 * self.block as u64;
        let mut c = [0u32; 16];
        c[0] = if write { 0x01 } else { 0x02 };
        c[1] = 1;
        c[6] = prp1 as u32;
        c[7] = (prp1 >> 32) as u32;
        if bytes > 4096 {
            let prp2 = prp1 + 4096;
            c[8] = prp2 as u32;
            c[9] = (prp2 >> 32) as u32;
        }
        c[10] = lba as u32;
        c[11] = (lba >> 32) as u32;
        c[12] = count - 1;
        self.submit(false, c).is_some()
    }

    /// Write 512-byte sectors (read-modify-write when blocks are bigger).
    pub fn write(&mut self, lba512: u64, data: &[u8]) -> bool {
        let per = (self.block / 512) as u64;
        let (blk, off) = (lba512 / per, (lba512 % per) as usize * 512);
        let n = data.len().min(4096 - off);
        if per > 1 && !self.rw(false, blk, 1) {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), (self.buf + off) as *mut u8, n) };
        let count = if per > 1 { 1 } else { (n / 512).max(1) as u32 };
        self.rw(true, blk, count)
    }
}

impl Disk for Nvme {
    fn read(&mut self, lba512: u64, buf: &mut [u8]) -> bool {
        let per = (self.block / 512) as u64;
        let (blk, off) = (lba512 / per, (lba512 % per) as usize * 512);
        let n = buf.len().min(4096 - off);
        let count = if per > 1 { 1 } else { (n / 512).max(1) as u32 };
        if !self.rw(false, blk, count) {
            return false;
        }
        unsafe { core::ptr::copy_nonoverlapping((self.buf + off) as *const u8, buf.as_mut_ptr(), n) };
        true
    }

    fn size(&self) -> u64 {
        self.blocks * (self.block as u64 / 512)
    }
}
