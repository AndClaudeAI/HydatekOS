//! Installing HydatekOS on a disk in this computer.
//!
//! Run from the USB stick, HydatekOS drives the computer's own NVMe and SATA
//! disks itself (disks.rs), so it writes the new disk itself too: the
//! partition table and file system laid out by mkdisk.rs, holding the boot
//! files and, if asked, everything in \HYDATEK\ (accounts, settings, files).
//!
//! What keeps it safe:
//! - it only writes to a disk HydatekOS drives itself, never the one it
//!   started from (the firmware keeps that one);
//! - the disk must be empty (no partition table, no file system), checked
//!   when the list is made and again just before the first write;
//! - the partition table goes on last, so an install that stops half way
//!   leaves a disk that still reads as empty and can simply be done again;
//! - everything written is read back and compared.
//!
//! The work goes a little at a time (`step`), so the screen stays alive.

use crate::mkdisk::{self, Item, SECTOR};
use crate::storage::Disk;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// A disk that can be written.
pub trait Target {
    fn read_at(&mut self, lba: u64, buf: &mut [u8]) -> bool;
    fn write_at(&mut self, lba: u64, data: &[u8]) -> bool;
    /// in 512-byte sectors
    fn sectors(&self) -> u64;
    /// the disk's own sector size (512, or 4096 for 4K-native disks)
    fn block_size(&self) -> usize;
}

impl Target for crate::nvme::Nvme {
    fn read_at(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        self.read(lba, buf)
    }
    fn write_at(&mut self, lba: u64, data: &[u8]) -> bool {
        self.write(lba, data)
    }
    fn sectors(&self) -> u64 {
        self.size()
    }
    fn block_size(&self) -> usize {
        self.block as usize
    }
}

impl Target for crate::ahci::SataDisk {
    fn read_at(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        self.read(lba, buf)
    }
    fn write_at(&mut self, lba: u64, data: &[u8]) -> bool {
        self.write(lba, data)
    }
    fn sectors(&self) -> u64 {
        self.size()
    }
    fn block_size(&self) -> usize {
        SECTOR
    }
}

/// Which disk: an NVMe or a SATA one, by its place in disks.rs's lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Nvme(usize),
    Sata(usize),
}

/// A disk the installer could use, as Settings shows it.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub which: Which,
    pub name: String,
    pub kind: &'static str,
    pub bytes: u64,
    /// Ok: empty, HydatekOS can go here; Err: why not
    pub ready: Result<(), String>,
}

/// How an install is going.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Running { phase: &'static str, done: u64, total: u64 },
    /// `menu`: the firmware's start-up menu has HydatekOS first
    Done { disk: String, menu: bool },
    Failed(String),
}

/// Reads of up to 4 KiB, a whole number of sectors, never across a 4 KiB
/// block (for disks with 4 KiB blocks).
fn chunk_at(lba: u64, left: usize) -> usize {
    let room = (8 - (lba % 8) as usize) * SECTOR;
    left.min(room)
}

fn read_all(t: &mut dyn Target, lba: u64, out: &mut [u8]) -> bool {
    let mut at = 0usize;
    while at < out.len() {
        let l = lba + (at / SECTOR) as u64;
        let n = chunk_at(l, out.len() - at);
        if !t.read_at(l, &mut out[at..at + n]) {
            return false;
        }
        at += n;
    }
    true
}

/// Is `t` empty? Reads its first sectors and its last one.
pub fn check_empty(t: &mut dyn Target) -> Result<(), String> {
    let n = t.sectors();
    if n * (SECTOR as u64) < mkdisk::MIN_BYTES {
        return Err(String::from("it's smaller than 256 MB"));
    }
    let bs = t.block_size();
    let mut first = vec![0u8; 3 * bs];
    let mut last = vec![0u8; bs];
    if !read_all(t, 0, &mut first) || !read_all(t, n - (bs / SECTOR) as u64, &mut last) {
        return Err(String::from("it couldn't be read"));
    }
    mkdisk::empty(&first, &last, bs).map_err(|e| e.to_string())
}

/// The disks HydatekOS drives, and which could take it.
pub fn candidates(disks: &mut crate::disks::Disks) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (i, d) in disks.nvme.iter_mut().enumerate() {
        let ready = check_empty(d);
        out.push(Candidate { which: Which::Nvme(i), name: d.model.trim().to_string(), kind: "NVMe SSD", bytes: d.size() * SECTOR as u64, ready });
    }
    for (i, d) in disks.sata.iter_mut().enumerate() {
        let ready = check_empty(d);
        out.push(Candidate { which: Which::Sata(i), name: d.model.trim().to_string(), kind: "SATA disk", bytes: d.size() * SECTOR as u64, ready });
    }
    out
}

fn target<'a>(disks: &'a mut crate::disks::Disks, w: Which) -> Option<&'a mut dyn Target> {
    match w {
        Which::Nvme(i) => disks.nvme.get_mut(i).map(|d| d as &mut dyn Target),
        Which::Sata(i) => disks.sata.get_mut(i).map(|d| d as &mut dyn Target),
    }
}

/// One piece of the work.
enum Op {
    Zero(u64, u64),
    Write(u64, Vec<u8>),
}

pub struct Job {
    which: Which,
    disk_name: String,
    ops: Vec<Op>,
    /// the op in hand, and how far into it (sectors)
    at: usize,
    off: u64,
    /// read back and compare once everything's written
    verifying: bool,
    done: u64,
    total: u64,
    /// the new partition: where, and its GUID (for the start-up entry)
    part: (u64, u64, [u8; 16]),
}

/// Put HydatekOS in the firmware's start-up menu, first: a new Boot####
/// entry for the partition (found by its GUID wherever the disk is), at
/// the front of BootOrder. False if the firmware wouldn't have it (then it
/// still finds \EFI\BOOT\ on the disk by itself, or it's picked in its
/// boot menu).
fn add_boot_entry(part: (u64, u64, [u8; 16])) -> bool {
    use crate::efi::{get_var, set_var, GLOBAL_VARIABLE};
    let file = alloc::format!("\\EFI\\BOOT\\{}", crate::arch::BOOT_FILE);
    let opt = mkdisk::load_option("HydatekOS", 1, part.0, part.1, part.2, &file);
    // the number: ours from an earlier install if there is one, else the first free
    let desc = |v: &[u8]| -> String {
        let units: Vec<u16> = v.get(6..).unwrap_or(&[]).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
        String::from_utf16_lossy(&units)
    };
    let (mut free, mut ours) = (None, None);
    for i in 0..0x100u16 {
        match get_var(&alloc::format!("Boot{:04X}", i), &GLOBAL_VARIABLE) {
            None => free = free.or(Some(i)),
            Some(v) if desc(&v) == "HydatekOS" => ours = ours.or(Some(i)),
            Some(_) => {}
        }
    }
    let n = ours.or(free);
    let Some(n) = n else { return false };
    if !set_var(&alloc::format!("Boot{:04X}", n), &GLOBAL_VARIABLE, &opt) {
        log!("install: the firmware wouldn't keep a start-up entry");
        return false;
    }
    let order = get_var("BootOrder", &GLOBAL_VARIABLE).unwrap_or_default();
    let ok = set_var("BootOrder", &GLOBAL_VARIABLE, &mkdisk::boot_order_first(&order, n));
    log!("install: start-up entry Boot{:04X} \"HydatekOS\"{}", n, if ok { ", first in BootOrder" } else { " (BootOrder unchanged)" });
    ok
}

/// What goes on the new disk: the boot files and, with `bring`, everything
/// in \HYDATEK\ on the stick.
pub fn files(fs: &crate::fs::Vfs, bring: bool) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    for name in ["BOOTX64.EFI", "BOOTAA64.EFI"] {
        if let Some(d) = fs.read_volume(&alloc::format!("\\EFI\\BOOT\\{}", name), 64 << 20) {
            if !d.is_empty() {
                items.push(Item { path: alloc::format!("EFI/BOOT/{}", name), data: Some(d) });
            }
        }
    }
    if !items.iter().any(|i| i.path.ends_with(crate::arch::BOOT_FILE)) {
        return Err(String::from("HydatekOS's own boot file couldn't be read from the stick"));
    }
    if bring {
        let tree = fs.read_volume_tree("\\HYDATEK", 512 << 20).ok_or_else(|| String::from("your files couldn't all be read (or come to more than 512 MB)"))?;
        items.push(Item { path: String::from("HYDATEK"), data: None });
        for (p, d) in tree {
            items.push(Item { path: alloc::format!("HYDATEK/{}", p), data: d });
        }
    }
    Ok(items)
}

impl Job {
    /// Plan the install. Nothing is written yet.
    pub fn new(disks: &mut crate::disks::Disks, which: Which, name: &str, items: &[Item], now: crate::efi::Time) -> Result<Job, String> {
        let t = target(disks, which).ok_or_else(|| String::from("that disk has gone"))?;
        check_empty(t).map_err(|e| alloc::format!("The disk isn't empty: {}.", e))?;
        // laid out in the disk's own sectors; written in 512-byte ones
        let bs = t.block_size();
        let per = (bs / SECTOR) as u64;
        let disk = t.sectors() / per;
        let (start, sectors) = mkdisk::partition(disk, bs).map_err(|e| e.to_string())?;
        let fs = mkdisk::Fat32::new(sectors, bs).map_err(|e| e.to_string())?;
        let mut ids = [0u8; 36];
        crate::rng::fill(&mut ids);
        let stamp = mkdisk::fat_stamp(now.year, now.month, now.day, now.hour, now.minute, now.second);
        let vol_id = u32::from_le_bytes([ids[32], ids[33], ids[34], ids[35]]);
        let img = mkdisk::fat32(&fs, items, "HYDATEK", vol_id, stamp, start).map_err(|e| e.to_string())?;
        log!("install: FAT32 of {} clusters of {} KiB, {} used", fs.clusters, fs.per_cluster * fs.bs / 1024, img.used_clusters);
        // GUIDs: version 4, variant 1
        let guid = |b: &[u8]| {
            let mut g = [0u8; 16];
            g.copy_from_slice(&b[..16]);
            g[7] = g[7] & 0x0F | 0x40;
            g[8] = g[8] & 0x3F | 0x80;
            g
        };
        let gpt = mkdisk::gpt(disk, bs, start, sectors, guid(&ids[0..16]), guid(&ids[16..32]));
        let mut ops = Vec::new();
        for (s, n) in img.zero {
            if n > 0 {
                ops.push(Op::Zero((start + s) * per, n * per));
            }
        }
        for (s, b) in img.writes {
            ops.push(Op::Write((start + s) * per, b));
        }
        // the partition table last: the MBR at the very end
        let mut gpt = gpt;
        gpt.sort_by_key(|(s, _)| if *s == 0 { u64::MAX } else { *s });
        for (s, b) in gpt {
            ops.push(Op::Write(s * per, b));
        }
        let writes: u64 = ops.iter().map(|o| if let Op::Write(_, b) = o { (b.len() / SECTOR) as u64 } else { 0 }).sum();
        let zeros: u64 = ops.iter().map(|o| if let Op::Zero(_, n) = o { *n } else { 0 }).sum();
        log!("install: {} sectors of {} bytes on {} ({} to clear, {} to write, then read back)", disk, bs, name, zeros, writes);
        Ok(Job { which, disk_name: String::from(name), ops, at: 0, off: 0, verifying: false, done: 0, total: zeros + 2 * writes, part: (start, sectors, guid(&ids[16..32])) })
    }

    fn phase(&self) -> &'static str {
        if self.verifying {
            return "Checking what was written";
        }
        match self.ops.get(self.at) {
            Some(Op::Zero(..)) => "Preparing the disk",
            Some(Op::Write(s, _)) if *s < 2048 || self.at + 5 >= self.ops.len() => "Finishing",
            _ => "Copying HydatekOS",
        }
    }

    /// Do some of the work: about `budget` sectors. The new state.
    pub fn step(&mut self, disks: &mut crate::disks::Disks, budget: u64) -> State {
        let Some(t) = target(disks, self.which) else { return State::Failed(String::from("The disk went away.")) };
        let zeros = [0u8; 4096];
        let mut buf = [0u8; 4096];
        let mut left = budget;
        while left > 0 {
            if self.at >= self.ops.len() {
                if self.verifying {
                    log!("install: done on {}", self.disk_name);
                    let menu = add_boot_entry(self.part);
                    return State::Done { disk: self.disk_name.clone(), menu };
                }
                self.verifying = true;
                self.at = 0;
                self.off = 0;
                continue;
            }
            let (lba, count) = match &self.ops[self.at] {
                Op::Zero(s, n) => (*s, *n),
                Op::Write(s, b) => (*s, (b.len() / SECTOR) as u64),
            };
            if self.verifying && matches!(self.ops[self.at], Op::Zero(..)) {
                self.at += 1;
                continue;
            }
            let l = lba + self.off;
            let bytes = chunk_at(l, ((count - self.off) as usize) * SECTOR);
            let n = (bytes / SECTOR) as u64;
            let ok = match &self.ops[self.at] {
                Op::Zero(..) => t.write_at(l, &zeros[..bytes]),
                Op::Write(_, b) => {
                    let part = &b[(self.off as usize) * SECTOR..(self.off as usize) * SECTOR + bytes];
                    if self.verifying {
                        t.read_at(l, &mut buf[..bytes]) && &buf[..bytes] == part
                    } else {
                        t.write_at(l, part)
                    }
                }
            };
            if !ok {
                let what = if self.verifying { "didn't read back as written" } else { "couldn't be written" };
                log!("install: sector {} {}", l, what);
                return State::Failed(alloc::format!("Sector {} {}. The disk may be failing.", l, what));
            }
            self.off += n;
            self.done += n;
            left = left.saturating_sub(n);
            if self.off >= count {
                self.at += 1;
                self.off = 0;
            }
        }
        State::Running { phase: self.phase(), done: self.done, total: self.total.max(1) }
    }
}
