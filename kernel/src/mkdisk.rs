//! Laying out a disk for HydatekOS: what the installer writes.
//!
//! One GPT partition, an EFI System Partition as large as the disk allows
//! (FAT32 tops out at 2 TiB), formatted FAT32, holding the boot files and
//! \HYDATEK\ (your files and settings). The firmware finds
//! \EFI\BOOT\BOOTX64.EFI (or BOOTAA64.EFI) on it and starts HydatekOS.
//!
//! Everything is in the disk's own sector size, `bs`: 512 bytes, or 4096
//! for "4K native" disks (many NVMe drives), where the partition table,
//! the file system and the firmware's start-up entry all count in 4 KiB
//! sectors.
//!
//! Plain logic, no hardware: it says which sectors get which bytes, and the
//! installer (install.rs) writes them. Host-tested: the result is checked
//! with sgdisk, fsck.fat and mtools.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// The drivers' unit (disks.rs reads and writes 512-byte sectors, whatever
/// the disk's own size).
pub const SECTOR: usize = 512;
/// The smallest disk HydatekOS installs on.
pub const MIN_BYTES: u64 = 256 << 20;
/// Where the partition starts: 1 MiB in, aligned for any disk.
const PART_START_BYTES: u64 = 1 << 20;
/// The GPT's 128 entries of 128 bytes.
const ENTRIES_BYTES: usize = 128 * 128;
/// The EFI System Partition's type.
const ESP_TYPE: [u8; 16] = [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B];

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

fn put16(b: &mut [u8], i: usize, v: u16) {
    b[i..i + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}
fn put64(b: &mut [u8], i: usize, v: u64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}

// ---- is the disk empty? -------------------------------------------------------------

/// Is there nothing on this disk? `first` holds its first 3 sectors, `last`
/// its last one (sectors of `bs` bytes). It's empty only with no partition table (MBR or GPT, at
/// either end) and no file system straight on the disk. Anything else and
/// the installer won't touch it.
pub fn empty(first: &[u8], last: &[u8], bs: usize) -> Result<(), &'static str> {
    if first.len() < 3 * bs.max(SECTOR) || last.len() < bs.max(SECTOR) {
        return Err("couldn't read the disk");
    }
    let s0 = &first[..SECTOR];
    // a GPT header in sector 1 (of either size: a disk's history may differ)
    let gpt_at = |o: usize| first.get(o..o + 8) == Some(&b"EFI PART"[..]);
    if gpt_at(SECTOR) || gpt_at(4096) || gpt_at(bs) || &last[..8] == b"EFI PART" || last.get(last.len() - bs.max(SECTOR)..).map_or(false, |l| &l[..8] == b"EFI PART") {
        return Err("it has a GPT partition table");
    }
    if s0[510] == 0x55 && s0[511] == 0xAA {
        let used = (0..4).any(|i| s0[446 + i * 16 + 4] != 0);
        return Err(if used { "it has an MBR partition table" } else { "it has a boot sector" });
    }
    for sig in [&b"FAT"[..], b"NTFS", b"EXFAT", b"MSDOS", b"mkfs"] {
        if s0[3..11].windows(sig.len()).any(|w| w == sig) || s0[54..62].starts_with(sig) || s0[82..90].starts_with(sig) {
            return Err("it has a file system");
        }
    }
    // ext2/3/4's superblock magic, 1080 bytes in
    if first[1080] == 0x53 && first[1081] == 0xEF {
        return Err("it has a Linux file system");
    }
    Ok(())
}

// ---- the partition table ------------------------------------------------------------

fn entries_sectors(bs: usize) -> u64 {
    ((ENTRIES_BYTES + bs - 1) / bs) as u64
}

/// Where the partition goes on a disk of `disk` sectors of `bs` bytes:
/// (first sector, sectors). A whole number of 4 KiB blocks, and no more
/// than FAT32 can count (2^32 sectors).
pub fn partition(disk: u64, bs: usize) -> Result<(u64, u64), &'static str> {
    if disk * (bs as u64) < MIN_BYTES {
        return Err("the disk is smaller than 256 MB");
    }
    let start = PART_START_BYTES / bs as u64;
    let last_usable = disk - 2 - entries_sectors(bs);
    let per4k = (4096 / bs).max(1) as u64;
    let n = (last_usable + 1 - start).min(0xFFFF_FFF8) / per4k * per4k;
    Ok((start, n))
}

/// The GPT: (sector, bytes) to write. A protective MBR, the header and the
/// entries at the start, and their backup copies at the end.
pub fn gpt(disk: u64, bs: usize, start: u64, sectors: u64, disk_guid: [u8; 16], part_guid: [u8; 16]) -> Vec<(u64, Vec<u8>)> {
    let es = entries_sectors(bs);
    let mut mbr = vec![0u8; bs];
    let e = 446;
    mbr[e + 1] = 0x00;
    mbr[e + 2] = 0x02; // CHS 0/0/2
    mbr[e + 4] = 0xEE;
    mbr[e + 5..e + 8].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
    put32(&mut mbr, e + 8, 1);
    put32(&mut mbr, e + 12, (disk - 1).min(0xFFFF_FFFF) as u32);
    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    // 128 entries of 128 bytes, padded to whole sectors
    let mut entries = vec![0u8; es as usize * bs];
    entries[0..16].copy_from_slice(&ESP_TYPE);
    entries[16..32].copy_from_slice(&part_guid);
    put64(&mut entries, 32, start);
    put64(&mut entries, 40, start + sectors - 1);
    for (i, c) in "HydatekOS".encode_utf16().enumerate() {
        put16(&mut entries, 56 + i * 2, c);
    }
    let entries_crc = crc32(&entries[..ENTRIES_BYTES]);
    let header = |me: u64, other: u64, table: u64| {
        let mut h = vec![0u8; bs];
        h[0..8].copy_from_slice(b"EFI PART");
        put32(&mut h, 8, 0x0001_0000);
        put32(&mut h, 12, 92);
        put64(&mut h, 24, me);
        put64(&mut h, 32, other);
        put64(&mut h, 40, 2 + es);
        put64(&mut h, 48, disk - 2 - es);
        h[56..72].copy_from_slice(&disk_guid);
        put64(&mut h, 72, table);
        put32(&mut h, 80, 128);
        put32(&mut h, 84, 128);
        put32(&mut h, 88, entries_crc);
        let crc = crc32(&h[..92]);
        put32(&mut h, 16, crc);
        h
    };
    vec![
        (0, mbr),
        (1, header(1, disk - 1, 2)),
        (2, entries.clone()),
        (disk - 1 - es, entries),
        (disk - 1, header(disk - 1, 1, disk - 1 - es)),
    ]
}

// ---- FAT32 --------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fat32 {
    /// bytes in a sector
    pub bs: u32,
    /// the partition's size
    pub sectors: u32,
    pub per_cluster: u32,
    pub reserved: u32,
    /// each of the two FATs
    pub fat_sectors: u32,
    pub clusters: u32,
}

impl Fat32 {
    /// The layout for a partition of `sectors` of `bs` bytes: cluster size
    /// by Microsoft's table (by bytes), the FAT just big enough for them.
    pub fn new(sectors: u64, bs: usize) -> Result<Fat32, &'static str> {
        let sectors = sectors.min(0xFFFF_FFFF) as u32;
        let mb = sectors as u64 * bs as u64 >> 20;
        let cluster_bytes: u64 = match mb {
            0..=260 => 512,
            261..=8192 => 4096,
            8193..=16384 => 8192,
            16385..=32768 => 16384,
            _ => 32768,
        };
        let per_cluster = (cluster_bytes / bs as u64).max(1) as u32;
        let reserved = 32u32;
        // the FAT holds a 4-byte entry for each cluster (and two more)
        let mut fat_sectors = 1u32;
        loop {
            let clusters = (sectors - reserved - 2 * fat_sectors) / per_cluster;
            let need = ((clusters as u64 + 2) * 4 + bs as u64 - 1) / bs as u64;
            if need as u32 <= fat_sectors {
                break;
            }
            fat_sectors = need as u32;
        }
        let clusters = ((sectors - reserved - 2 * fat_sectors) / per_cluster).min(0x0FFF_FFF5);
        if clusters < 65525 {
            return Err("too small for FAT32");
        }
        Ok(Fat32 { bs: bs as u32, sectors, per_cluster, reserved, fat_sectors, clusters })
    }

    pub fn data_start(&self) -> u64 {
        self.reserved as u64 + 2 * self.fat_sectors as u64
    }

    pub fn cluster_sector(&self, c: u32) -> u64 {
        self.data_start() + (c as u64 - 2) * self.per_cluster as u64
    }

    fn cluster_bytes(&self) -> usize {
        self.per_cluster as usize * self.bs as usize
    }
}

/// What to write in the partition: `zero` ranges (first sector, count)
/// first, then `writes` (sector, bytes, a whole number of sectors). Sectors
/// count from the partition's start.
pub struct Image {
    pub zero: Vec<(u64, u64)>,
    pub writes: Vec<(u64, Vec<u8>)>,
    pub used_clusters: u32,
}

/// A file or folder to put on the disk: a path with '/' between names
/// ("EFI/BOOT/BOOTX64.EFI"), and the bytes (None for a folder).
pub struct Item {
    pub path: String,
    pub data: Option<Vec<u8>>,
}

struct Node {
    name: String,
    data: Option<Vec<u8>>,
    kids: Vec<usize>,
    parent: usize,
    cluster: u32,
    clusters: u32,
    short: [u8; 11],
    lfn: bool,
}

/// Characters a short name may hold (besides letters and digits).
fn short_ok(c: u8) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || b"!#$%&'()-@^_`{}~".contains(&c)
}

/// The 8.3 name for `name` in a folder that already has `taken`, and
/// whether it needs a long name as well.
fn short_name(name: &str, taken: &[[u8; 11]]) -> ([u8; 11], bool) {
    let (base, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    };
    let exact = !base.is_empty() && base.len() <= 8 && ext.len() <= 3 && base.bytes().all(short_ok) && ext.bytes().all(short_ok);
    let pad = |b: &[u8], e: &[u8]| {
        let mut s = [b' '; 11];
        s[..b.len().min(8)].copy_from_slice(&b[..b.len().min(8)]);
        s[8..8 + e.len().min(3)].copy_from_slice(&e[..e.len().min(3)]);
        s
    };
    if exact {
        let s = pad(base.as_bytes(), ext.as_bytes());
        if !taken.contains(&s) {
            return (s, false);
        }
    }
    let clean = |s: &str| -> Vec<u8> {
        s.bytes()
            .filter(|c| *c != b' ' && *c != b'.')
            .map(|c| {
                let c = c.to_ascii_uppercase();
                if short_ok(c) { c } else { b'_' }
            })
            .collect()
    };
    let (b, e) = (clean(base), clean(ext));
    let b = if b.is_empty() { alloc::vec![b'_'] } else { b };
    for n in 1..1000u32 {
        let tail = alloc::format!("~{}", n);
        let keep = (8 - tail.len()).min(b.len());
        let mut nb = b[..keep].to_vec();
        nb.extend_from_slice(tail.as_bytes());
        let s = pad(&nb, &e[..e.len().min(3)]);
        if !taken.contains(&s) {
            return (s, true);
        }
    }
    (pad(b"HYDATEK", b""), true)
}

fn lfn_checksum(s: &[u8; 11]) -> u8 {
    s.iter().fold(0u8, |sum, &c| (sum >> 1 | sum << 7).wrapping_add(c))
}

/// The long-name entries for `name`, in the order they're stored.
fn lfn_entries(name: &str, short: &[u8; 11]) -> Vec<[u8; 32]> {
    let mut units: Vec<u16> = name.encode_utf16().collect();
    // ended by a 0 unless it fills its last entry exactly
    if units.len() % 13 != 0 {
        units.push(0);
    }
    while units.len() % 13 != 0 {
        units.push(0xFFFF);
    }
    let n = units.len() / 13;
    let sum = lfn_checksum(short);
    let mut out = Vec::new();
    for k in (0..n).rev() {
        let mut e = [0u8; 32];
        e[0] = (k + 1) as u8 | if k == n - 1 { 0x40 } else { 0 };
        e[11] = 0x0F;
        e[13] = sum;
        let part = &units[k * 13..k * 13 + 13];
        let slots = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        for (i, u) in part.iter().enumerate() {
            put16(&mut e, slots[i], *u);
        }
        out.push(e);
    }
    out
}

/// The FAT32 volume holding `items`: every folder and file placed one
/// after another from cluster 2 (the root folder), their FAT chains, and
/// the boot sectors. `hidden` is the partition's first sector on the disk;
/// `stamp` is (date, time) in FAT's format.
pub fn fat32(fs: &Fat32, items: &[Item], label: &str, volume_id: u32, stamp: (u16, u16), hidden: u64) -> Result<Image, &'static str> {
    // the tree
    let mut nodes = vec![Node { name: String::new(), data: None, kids: Vec::new(), parent: 0, cluster: 2, clusters: 0, short: [b' '; 11], lfn: false }];
    for it in items {
        let mut at = 0usize;
        let parts: Vec<&str> = it.path.split('/').filter(|p| !p.is_empty()).collect();
        for (i, p) in parts.iter().enumerate() {
            let leaf = i + 1 == parts.len();
            let found = nodes[at].kids.iter().copied().find(|k| nodes[*k].name.eq_ignore_ascii_case(p));
            at = match found {
                Some(k) => {
                    if leaf && it.data.is_some() {
                        nodes[k].data = it.data.clone();
                    }
                    k
                }
                None => {
                    let data = if leaf { it.data.clone() } else { None };
                    nodes.push(Node { name: String::from(*p), data, kids: Vec::new(), parent: at, cluster: 0, clusters: 0, short: [b' '; 11], lfn: false });
                    let k = nodes.len() - 1;
                    nodes[at].kids.push(k);
                    k
                }
            };
        }
    }
    // short names, folder by folder
    for d in 0..nodes.len() {
        let mut taken: Vec<[u8; 11]> = Vec::new();
        for k in nodes[d].kids.clone() {
            let (s, lfn) = short_name(&nodes[k].name, &taken);
            taken.push(s);
            nodes[k].short = s;
            nodes[k].lfn = lfn;
        }
    }
    // sizes in clusters
    let cb = fs.cluster_bytes();
    let entries_of = |nodes: &Vec<Node>, d: usize| -> usize {
        let own = if d == 0 { 1 } else { 2 }; // the label, or "." and ".."
        own + nodes[d].kids.iter().map(|k| 1 + if nodes[*k].lfn { (nodes[*k].name.encode_utf16().count() + 12) / 13 } else { 0 }).sum::<usize>()
    };
    for i in 0..nodes.len() {
        let bytes = match &nodes[i].data {
            Some(d) => d.len(),
            None => entries_of(&nodes, i) * 32,
        };
        nodes[i].clusters = ((bytes + cb - 1) / cb) as u32;
        if nodes[i].data.is_none() {
            nodes[i].clusters = nodes[i].clusters.max(1);
        }
    }
    // placed one after another, root first
    let mut next = 2u32;
    for i in 0..nodes.len() {
        if nodes[i].clusters > 0 {
            nodes[i].cluster = next;
            next += nodes[i].clusters;
        }
    }
    let used = next - 2;
    if used > fs.clusters {
        return Err("the files don't fit");
    }
    let dirent = |n: &Node, name: &[u8; 11], cluster: u32, attr: u8, size: u32| {
        let mut e = [0u8; 32];
        e[..11].copy_from_slice(name);
        e[11] = attr;
        put16(&mut e, 14, stamp.1);
        put16(&mut e, 16, stamp.0);
        put16(&mut e, 18, stamp.0);
        put16(&mut e, 20, (cluster >> 16) as u16);
        put16(&mut e, 22, stamp.1);
        put16(&mut e, 24, stamp.0);
        put16(&mut e, 26, cluster as u16);
        put32(&mut e, 28, size);
        let _ = n;
        e
    };

    let mut img = Image { zero: Vec::new(), writes: Vec::new(), used_clusters: used };
    // the reserved sectors and both FATs start as zeros
    img.zero.push((0, fs.reserved as u64));
    let bsz = fs.bs as usize;
    let fat_used = ((next as u64 * 4 + bsz as u64 - 1) / bsz as u64).min(fs.fat_sectors as u64);
    for f in 0..2u64 {
        let base = fs.reserved as u64 + f * fs.fat_sectors as u64;
        img.zero.push((base + fat_used, fs.fat_sectors as u64 - fat_used));
    }
    // boot sector and FSInfo, and their copies at 6 and 7
    let mut bs = vec![0u8; bsz];
    bs[0..3].copy_from_slice(&[0xEB, 0x58, 0x90]);
    bs[3..11].copy_from_slice(b"HYDATEK ");
    put16(&mut bs, 11, bsz as u16);
    bs[13] = fs.per_cluster as u8;
    put16(&mut bs, 14, fs.reserved as u16);
    bs[16] = 2;
    bs[21] = 0xF8;
    put16(&mut bs, 24, 63);
    put16(&mut bs, 26, 255);
    put32(&mut bs, 28, hidden.min(0xFFFF_FFFF) as u32);
    put32(&mut bs, 32, fs.sectors);
    put32(&mut bs, 36, fs.fat_sectors);
    put32(&mut bs, 44, 2);
    put16(&mut bs, 48, 1);
    put16(&mut bs, 50, 6);
    bs[64] = 0x80;
    bs[66] = 0x29;
    put32(&mut bs, 67, volume_id);
    let mut lab = [b' '; 11];
    for (i, c) in label.bytes().take(11).enumerate() {
        lab[i] = c.to_ascii_uppercase();
    }
    bs[71..82].copy_from_slice(&lab);
    bs[82..90].copy_from_slice(b"FAT32   ");
    // not a bootable volume by itself: a tiny loop, then the signature
    bs[90..92].copy_from_slice(&[0xEB, 0xFE]);
    bs[510] = 0x55;
    bs[511] = 0xAA;
    let mut fsinfo = vec![0u8; bsz];
    put32(&mut fsinfo, 0, 0x4161_5252);
    put32(&mut fsinfo, 484, 0x6141_7272);
    put32(&mut fsinfo, 488, fs.clusters - used);
    put32(&mut fsinfo, 492, next);
    put32(&mut fsinfo, 508, 0xAA55_0000);
    img.writes.push((0, bs.clone()));
    img.writes.push((1, fsinfo.clone()));
    img.writes.push((6, bs));
    img.writes.push((7, fsinfo));
    // the FAT: media and end marks, then each chain
    let mut fat = vec![0u8; fat_used as usize * bsz];
    put32(&mut fat, 0, 0x0FFF_FFF8);
    put32(&mut fat, 4, 0x0FFF_FFFF);
    for n in &nodes {
        for k in 0..n.clusters {
            let c = n.cluster + k;
            let v = if k + 1 == n.clusters { 0x0FFF_FFFF } else { c + 1 };
            put32(&mut fat, c as usize * 4, v);
        }
    }
    for f in 0..2u64 {
        img.writes.push((fs.reserved as u64 + f * fs.fat_sectors as u64, fat.clone()));
    }
    // folders and files
    for i in 0..nodes.len() {
        let n = &nodes[i];
        let mut bytes = match &n.data {
            Some(d) => d.clone(),
            None => {
                let mut out: Vec<u8> = Vec::new();
                if i == 0 {
                    out.extend_from_slice(&dirent(n, &lab, 0, 0x08, 0));
                } else {
                    let up = if n.parent == 0 { 0 } else { nodes[n.parent].cluster };
                    out.extend_from_slice(&dirent(n, b".          ", n.cluster, 0x10, 0));
                    out.extend_from_slice(&dirent(n, b"..         ", up, 0x10, 0));
                }
                for &k in &n.kids {
                    let c = &nodes[k];
                    if c.lfn {
                        for e in lfn_entries(&c.name, &c.short) {
                            out.extend_from_slice(&e);
                        }
                    }
                    let (attr, size) = match &c.data {
                        Some(d) => (0x20, d.len() as u32),
                        None => (0x10, 0),
                    };
                    out.extend_from_slice(&dirent(c, &c.short, if c.clusters > 0 { c.cluster } else { 0 }, attr, size));
                }
                out
            }
        };
        if n.clusters == 0 {
            continue;
        }
        bytes.resize(n.clusters as usize * cb, 0);
        img.writes.push((fs.cluster_sector(n.cluster), bytes));
    }
    Ok(img)
}

/// A date and time in FAT's format.
pub fn fat_stamp(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> (u16, u16) {
    let date = (year.saturating_sub(1980) << 9) | (month as u16) << 5 | day as u16;
    let time = (hour as u16) << 11 | (minute as u16) << 5 | (second as u16 / 2);
    (date, time)
}

// ---- the start-up menu entry ----------------------------------------------------------

/// A UEFI load option (the contents of a Boot#### variable) starting
/// `file` on the partition with GUID `part_guid`: a short-form device path
/// (the partition, then the file), which the firmware matches against
/// every disk it can see.
pub fn load_option(description: &str, part_number: u32, start: u64, sectors: u64, part_guid: [u8; 16], file: &str) -> Vec<u8> {
    let mut path = Vec::new();
    // Hard Drive media node: type 4, subtype 1, 42 bytes
    let mut hd = vec![0u8; 42];
    hd[0] = 4;
    hd[1] = 1;
    put16(&mut hd, 2, 42);
    put32(&mut hd, 4, part_number);
    put64(&mut hd, 8, start);
    put64(&mut hd, 16, sectors);
    hd[24..40].copy_from_slice(&part_guid);
    hd[40] = 2; // GPT
    hd[41] = 2; // signature is a GUID
    path.extend_from_slice(&hd);
    // File path node: type 4, subtype 4, the path in UTF-16 with its 0
    let name: Vec<u16> = file.encode_utf16().chain(core::iter::once(0)).collect();
    let len = 4 + name.len() * 2;
    let mut fp = vec![0u8; len];
    fp[0] = 4;
    fp[1] = 4;
    put16(&mut fp, 2, len as u16);
    for (i, u) in name.iter().enumerate() {
        put16(&mut fp, 4 + i * 2, *u);
    }
    path.extend_from_slice(&fp);
    // the end
    path.extend_from_slice(&[0x7F, 0xFF, 4, 0]);

    let mut opt = Vec::new();
    opt.extend_from_slice(&1u32.to_le_bytes()); // LOAD_OPTION_ACTIVE
    opt.extend_from_slice(&(path.len() as u16).to_le_bytes());
    for u in description.encode_utf16().chain(core::iter::once(0)) {
        opt.extend_from_slice(&u.to_le_bytes());
    }
    opt.extend_from_slice(&path);
    opt
}

/// BootOrder with `n` first (and not twice).
pub fn boot_order_first(order: &[u8], n: u16) -> Vec<u8> {
    let mut out = n.to_le_bytes().to_vec();
    for c in order.chunks_exact(2) {
        if u16::from_le_bytes([c[0], c[1]]) != n {
            out.extend_from_slice(c);
        }
    }
    out
}
