//! Disks: what's on them.
//!
//! The NVMe and SATA drivers (nvme.rs, ahci.rs) read sectors; this reads
//! the partition table (GPT, or the old MBR) and recognises each
//! partition's file system (FAT12/16/32, exFAT, NTFS, ext2/3/4, Btrfs,
//! APFS, HFS+, ISO 9660) and its label, for Settings › Devices. Plain logic,
//! host-tested with images made by mtools and mkfs.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Something that reads 512-byte sectors.
pub trait Disk {
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool;
    /// sectors
    fn size(&self) -> u64;
}

#[derive(Clone, Debug, PartialEq)]
pub struct Partition {
    pub start: u64,
    pub sectors: u64,
    /// what the partition table says it's for
    pub kind: String,
    /// the file system found in it, and its label
    pub fs: String,
    pub label: String,
}

fn le16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn le32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
fn le64(b: &[u8], i: usize) -> u64 {
    le32(b, i) as u64 | (le32(b, i + 4) as u64) << 32
}

/// A GUID as GPT stores it, in its usual text form.
pub fn guid(b: &[u8]) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        le32(b, 0),
        le16(b, 4),
        le16(b, 6),
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    )
}

/// What a GPT partition type is for.
pub fn gpt_kind(t: &str) -> &'static str {
    match t {
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B" => "EFI system",
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" => "Basic data",
        "E3C9E316-0B5C-4DB8-817D-F92DF00215AE" => "Microsoft reserved",
        "DE94BBA4-06D1-4D40-A16A-BFD50179D6AC" => "Windows recovery",
        "0FC63DAF-8483-4772-8E79-3D69D8477DE4" => "Linux data",
        "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F" => "Linux swap",
        "4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709" => "Linux root (x86-64)",
        "B921B045-1DF0-41C3-AF44-4C6F280D3FAE" => "Linux root (ARM64)",
        "7C3457EF-0000-11AA-AA11-00306543ECAC" => "APFS",
        "48465300-0000-11AA-AA11-00306543ECAC" => "HFS+",
        "21686148-6449-6E6F-744E-656564454649" => "BIOS boot",
        _ => "Partition",
    }
}

/// What an MBR partition type is for.
pub fn mbr_kind(t: u8) -> &'static str {
    match t {
        0x01 | 0x04 | 0x06 | 0x0E => "FAT",
        0x0B | 0x0C => "FAT32",
        0x07 => "NTFS / exFAT",
        0x83 => "Linux",
        0x82 => "Linux swap",
        0xEF => "EFI system",
        0xEE => "GPT protective",
        _ => "Partition",
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).trim_matches(|c: char| c == ' ' || c == '\0').to_string()
}

/// The file system in a partition (its first sectors), and its label.
pub fn filesystem(d: &mut dyn Disk, start: u64) -> (String, String) {
    let mut s = [0u8; 4096];
    if !d.read(start, &mut s[..512]) {
        return (String::new(), String::new());
    }
    for k in 1..8 {
        if !d.read(start + k, &mut s[512 * k as usize..512 * (k as usize + 1)]) {
            break;
        }
    }
    let b = &s;
    if &b[3..11] == b"NTFS    " {
        return (String::from("NTFS"), String::new());
    }
    if &b[3..11] == b"EXFAT   " {
        return (String::from("exFAT"), String::new());
    }
    if b[510] == 0x55 && b[511] == 0xAA {
        if &b[82..87] == b"FAT32" {
            return (String::from("FAT32"), text(&b[71..82]));
        }
        if &b[54..59] == b"FAT12" || &b[54..59] == b"FAT16" {
            return (text(&b[54..59]), text(&b[43..54]));
        }
    }
    // ext2/3/4: the superblock at byte 1024
    if le16(b, 1024 + 56) == 0xEF53 {
        let compat = le32(b, 1024 + 92);
        let incompat = le32(b, 1024 + 96);
        let v = if incompat & 0x40 != 0 { "ext4" } else if compat & 4 != 0 { "ext3" } else { "ext2" };
        return (String::from(v), text(&b[1024 + 120..1024 + 136]));
    }
    // APFS container / HFS+
    if &b[32..36] == b"NXSB" {
        return (String::from("APFS"), String::new());
    }
    if &b[1024..1026] == b"H+" || &b[1024..1026] == b"HX" {
        return (String::from("HFS+"), String::new());
    }
    // Btrfs: superblock at 64 KiB
    let mut sb = [0u8; 512];
    if d.read(start + 128, &mut sb) && &sb[64..72] == b"_BHRfS_M" {
        return (String::from("Btrfs"), text(&sb[299..299 + 64]));
    }
    // ISO 9660: sector 16 of 2 KiB (64 of 512)
    if d.read(start + 64, &mut sb) && &sb[1..6] == b"CD001" {
        return (String::from("ISO 9660"), text(&sb[40..72]));
    }
    (String::new(), String::new())
}

/// The partitions on a disk (GPT first, then MBR).
pub fn partitions(d: &mut dyn Disk) -> Vec<Partition> {
    let mut out = Vec::new();
    // a volume without a table (a "superfloppy" USB stick): its boot sector
    // ends like an MBR, but names its file system
    let (fs, label) = filesystem(d, 0);
    if matches!(fs.as_str(), "FAT12" | "FAT16" | "FAT32" | "NTFS" | "exFAT") {
        out.push(Partition { start: 0, sectors: d.size(), kind: String::from("Whole disk"), fs, label });
        return out;
    }
    let mut mbr = [0u8; 512];
    if !d.read(0, &mut mbr) || mbr[510] != 0x55 || mbr[511] != 0xAA {
        // no table: maybe a whole-disk file system
        let (fs, label) = filesystem(d, 0);
        if !fs.is_empty() {
            out.push(Partition { start: 0, sectors: d.size(), kind: String::from("Whole disk"), fs, label });
        }
        return out;
    }
    let mut hdr = [0u8; 512];
    if d.read(1, &mut hdr) && &hdr[0..8] == b"EFI PART" {
        let (lba, n, size) = (le64(&hdr, 72), le32(&hdr, 80).min(256) as u64, le32(&hdr, 84).clamp(128, 512) as u64);
        let mut sec = [0u8; 512];
        let mut cur = u64::MAX;
        for i in 0..n {
            let off = i * size;
            let s = lba + off / 512;
            if s != cur {
                if !d.read(s, &mut sec) {
                    break;
                }
                cur = s;
            }
            let e = &sec[(off % 512) as usize..(off % 512 + size.min(128)) as usize];
            if e[..16].iter().all(|b| *b == 0) {
                continue;
            }
            let (first, last) = (le64(e, 32), le64(e, 40));
            let name: Vec<u16> = (0..36).map(|k| le16(e, 56 + 2 * k)).take_while(|c| *c != 0).collect();
            let (fs, label) = filesystem(d, first);
            let pname = String::from_utf16_lossy(&name);
            out.push(Partition { start: first, sectors: last.saturating_sub(first) + 1, kind: String::from(gpt_kind(&guid(&e[..16]))), fs, label: if label.is_empty() { pname } else { label } });
        }
        return out;
    }
    for i in 0..4 {
        let e = &mbr[446 + 16 * i..462 + 16 * i];
        let (t, start, n) = (e[4], le32(e, 8) as u64, le32(e, 12) as u64);
        if t == 0 || n == 0 {
            continue;
        }
        let (fs, label) = filesystem(d, start);
        out.push(Partition { start, sectors: n, kind: String::from(mbr_kind(t)), fs, label });
    }
    if out.is_empty() {
        // a FAT volume without a table (a "superfloppy" USB stick)
        let (fs, label) = filesystem(d, 0);
        if !fs.is_empty() {
            out.push(Partition { start: 0, sectors: d.size(), kind: String::from("Whole disk"), fs, label });
        }
    }
    out
}

/// A size in words ("512 GB").
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = bytes;
    let mut u = 0;
    let mut frac = 0;
    while v >= 1000 && u < 4 {
        frac = (v % 1000) / 100;
        v /= 1000;
        u += 1;
    }
    if v < 10 && u > 0 && frac > 0 {
        format!("{}.{} {}", v, frac, UNITS[u])
    } else {
        format!("{} {}", v, UNITS[u])
    }
}

/// ATA IDENTIFY strings: words with their bytes swapped.
pub fn ata_string(id: &[u8], from_word: usize, words: usize) -> String {
    let mut s = Vec::new();
    for w in from_word..from_word + words {
        s.push(id[2 * w + 1]);
        s.push(id[2 * w]);
    }
    text(&s)
}

/// A disk HydatekOS drives, for Settings.
#[derive(Clone, Debug, Default)]
pub struct DiskInfo {
    pub name: String,
    pub kind: &'static str,
    pub bytes: u64,
    pub partitions: Vec<Partition>,
    pub driver: &'static str,
}

/// A disk's one-line summary: "2 partitions: FAT32 “HYDADATA”, ext4".
pub fn summary(parts: &[Partition]) -> String {
    if parts.is_empty() {
        return String::from("Empty (no partitions)");
    }
    let each: Vec<String> = parts
        .iter()
        .map(|p| {
            let fs = if p.fs.is_empty() { p.kind.clone() } else { p.fs.clone() };
            if p.label.is_empty() {
                fs
            } else {
                format!("{} “{}”", fs, p.label)
            }
        })
        .collect();
    format!("{} partition{}: {}", parts.len(), if parts.len() == 1 { "" } else { "s" }, each.join(", "))
}
