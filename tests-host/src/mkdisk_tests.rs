//! The installer's disk layout: checked with the tools everyone else uses
//! (sgdisk for the partition table, fsck.fat and mtools for the file system).

use crate::mkdisk::*;
use std::io::{Seek, SeekFrom, Write};
use std::process::Command;

fn have(tool: &str) -> bool {
    Command::new("sh").arg("-c").arg(format!("command -v {}", tool)).output().map(|o| o.status.success()).unwrap_or(false)
}

fn write_at(f: &mut std::fs::File, sector: u64, bs: usize, bytes: &[u8]) {
    f.seek(SeekFrom::Start(sector * bs as u64)).unwrap();
    f.write_all(bytes).unwrap();
}

/// A sparse file of `sectors` (of `bs` bytes) with `img` applied, as the
/// installer does.
fn apply(path: &str, sectors: u64, bs: usize, zero: &[(u64, u64)], writes: &[(u64, Vec<u8>)], base: u64) {
    let mut f = std::fs::File::create(path).unwrap();
    f.set_len(sectors * bs as u64).unwrap();
    // pretend the disk had old junk where zeros are promised: the zero
    // ranges must really clear it
    for &(s, n) in zero {
        let junk = vec![0xA5u8; (n.min(64) as usize) * bs];
        write_at(&mut f, base + s, bs, &junk);
        let zeros = vec![0u8; bs];
        for k in 0..n {
            write_at(&mut f, base + s + k, bs, &zeros);
        }
    }
    for (s, b) in writes {
        assert_eq!(b.len() % bs, 0, "a write of part of a sector");
        write_at(&mut f, base + s, bs, b);
    }
}

fn u32at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn u64at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}

/// Every field of both GPT copies, as the UEFI spec has them, for a disk of
/// `disk` sectors of `bs` bytes: the check sgdisk can't do for 4K sectors.
fn check_gpt(img: &[u8], disk: u64, bs: usize, start: u64, n: u64) {
    let sec = |l: u64| &img[l as usize * bs..(l as usize + 1) * bs];
    // protective MBR
    let mbr = sec(0);
    assert_eq!((mbr[510], mbr[511]), (0x55, 0xAA));
    assert_eq!(mbr[446 + 4], 0xEE);
    assert_eq!(u32at(mbr, 446 + 8), 1);
    let es = ((128 * 128 + bs - 1) / bs) as u64;
    for (me, other) in [(1u64, disk - 1), (disk - 1, 1u64)] {
        let h = sec(me);
        assert_eq!(&h[0..8], b"EFI PART", "header at {}", me);
        let mut hh = h[..92].to_vec();
        hh[16..20].copy_from_slice(&[0; 4]);
        assert_eq!(crc32(&hh), u32at(h, 16), "header CRC at {}", me);
        assert_eq!(u64at(h, 24), me);
        assert_eq!(u64at(h, 32), other);
        assert_eq!(u64at(h, 40), 2 + es, "first usable");
        assert_eq!(u64at(h, 48), disk - 2 - es, "last usable");
        let table = u64at(h, 72);
        assert_eq!(table, if me == 1 { 2 } else { disk - 1 - es });
        assert_eq!((u32at(h, 80), u32at(h, 84)), (128, 128));
        let entries = &img[table as usize * bs..table as usize * bs + 128 * 128];
        assert_eq!(crc32(entries), u32at(h, 88), "entries CRC for header at {}", me);
        // the one partition: an ESP inside the usable space
        assert_eq!(&entries[0..4], &[0x28, 0x73, 0x2A, 0xC1]);
        assert_eq!((u64at(entries, 32), u64at(entries, 40)), (start, start + n - 1));
        assert!(start >= 2 + es && start + n - 1 <= disk - 2 - es);
        assert!(entries[128..].iter().all(|b| *b == 0));
    }
}

#[test]
fn crc32_matches_the_standard() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
}

#[test]
fn only_empty_disks_count_as_empty() {
    let z = vec![0u8; 3 * SECTOR];
    let last = vec![0u8; SECTOR];
    assert!(empty(&z, &last, 512).is_ok());
    // an MBR with a partition
    let mut m = z.clone();
    m[510] = 0x55;
    m[511] = 0xAA;
    m[446 + 4] = 0x07;
    assert!(empty(&m, &last, 512).unwrap_err().contains("MBR"));
    // a GPT at the start, or only its backup at the end
    let mut g = z.clone();
    g[SECTOR..SECTOR + 8].copy_from_slice(b"EFI PART");
    assert!(empty(&g, &last, 512).is_err());
    let mut gl = last.clone();
    gl[..8].copy_from_slice(b"EFI PART");
    assert!(empty(&z, &gl, 512).is_err());
    // a FAT or NTFS file system straight on the disk
    let mut f = z.clone();
    f[3..11].copy_from_slice(b"NTFS    ");
    assert!(empty(&f, &last, 512).is_err());
    let mut f = z.clone();
    f[82..90].copy_from_slice(b"FAT32   ");
    assert!(empty(&f, &last, 512).is_err());
    // ext4
    let mut e = z.clone();
    e[1080] = 0x53;
    e[1081] = 0xEF;
    assert!(empty(&e, &last, 512).is_err());
    // a 4K-native disk: its GPT header is 4096 bytes in
    let z4 = vec![0u8; 3 * 4096];
    let last4 = vec![0u8; 4096];
    assert!(empty(&z4, &last4, 4096).is_ok());
    let mut g4 = z4.clone();
    g4[4096..4104].copy_from_slice(b"EFI PART");
    assert!(empty(&g4, &last4, 4096).is_err());
    // can't read enough: not empty
    assert!(empty(&z[..SECTOR], &last, 512).is_err());
}

#[test]
fn partitions_and_fat_sizes() {
    assert!(partition(200 << 11, 512).is_err()); // 200 MB
    for bs in [512usize, 4096] {
        for (gb, cluster_kib) in [(1u64, 4u32), (8, 4), (12, 8), (30, 16), (500, 32), (1800, 32)] {
            let disk = (gb << 30) / bs as u64;
            let (start, n) = partition(disk, bs).unwrap();
            assert_eq!(start * bs as u64, 1 << 20);
            let es = (16384 / bs) as u64;
            assert!(start + n <= disk - 1 - es, "{} GB", gb);
            assert_eq!(n * bs as u64 % 4096, 0);
            let fs = Fat32::new(n, bs).unwrap();
            assert_eq!(fs.per_cluster * bs as u32 / 1024, cluster_kib, "{} GB, {}-byte sectors", gb, bs);
            assert!(fs.clusters >= 65525);
            assert!((fs.clusters as u64 + 2) * 4 <= fs.fat_sectors as u64 * bs as u64);
            assert!(fs.data_start() + fs.clusters as u64 * fs.per_cluster as u64 <= fs.sectors as u64);
        }
    }
    // a 4 TB disk: the partition stops where FAT32 can't count further
    let (_, n) = partition(4000u64 << 21, 512).unwrap();
    assert!(n <= 0xFFFF_FFFF);
    assert!(Fat32::new(n, 512).is_ok());
    // with 4K sectors FAT32 reaches 16 TiB: an 8 TB disk fits whole
    let disk = (8000u64 << 30) / 4096;
    let (_, n) = partition(disk, 4096).unwrap();
    assert!(n > disk - 1000);
    assert!(Fat32::new(n, 4096).is_ok());
}

fn items() -> Vec<Item> {
    let mut v = vec![
        Item { path: "EFI/BOOT/BOOTX64.EFI".into(), data: Some((0..3_000_000u32).map(|i| (i * 7 + i / 13) as u8).collect()) },
        Item { path: "EFI/BOOT/BOOTAA64.EFI".into(), data: Some(vec![0x4D; 12345]) },
        Item { path: "HYDATEK/system/users.txt".into(), data: Some(b"ada:admin\n".to_vec()) },
        Item { path: "HYDATEK/home/Pictures/Dunes at dusk.png".into(), data: Some(vec![0x89; 70_000]) },
        Item { path: "HYDATEK/home/Documents/Photos 2026".into(), data: None },
        Item { path: "HYDATEK/home/Documents/Caf\u{e9} r\u{e9}sum\u{e9}.txt".into(), data: Some("Ol\u{e1}".as_bytes().to_vec()) },
        Item { path: "HYDATEK/home/Documents/Long file name one.txt".into(), data: Some(b"one".to_vec()) },
        Item { path: "HYDATEK/home/Documents/Long file name two.txt".into(), data: Some(b"two".to_vec()) },
        Item { path: "HYDATEK/home/Documents/Thirteen char".into(), data: Some(b"13".to_vec()) },
        Item { path: "HYDATEK/home/Documents/empty.txt".into(), data: Some(Vec::new()) },
    ];
    // enough files that a folder needs several clusters
    for i in 0..300 {
        v.push(Item { path: format!("HYDATEK/home/Notes/Note number {:03} about things.txt", i), data: Some(format!("note {}", i).into_bytes()) });
    }
    v
}

#[test]
fn a_whole_disk_checks_out() {
    whole_disk(512);
}

#[test]
fn a_whole_4k_disk_checks_out() {
    whole_disk(4096);
}

fn whole_disk(bs: usize) {
    if !(have("sgdisk") && have("fsck.fat") && have("mdir") && have("mtype")) {
        eprintln!("skipped: needs sgdisk, fsck.fat and mtools");
        return;
    }
    let dir = std::env::temp_dir().join(format!("hydatek-mkdisk-{}", bs));
    std::fs::create_dir_all(&dir).unwrap();
    let disk = (320u64 << 20) / bs as u64; // 320 MB
    let (start, n) = partition(disk, bs).unwrap();
    let fs = Fat32::new(n, bs).unwrap();
    let items = items();
    let img = fat32(&fs, &items, "HYDATEK", 0x1234_5678, fat_stamp(2026, 9, 30, 12, 34, 56), start).unwrap();
    assert!(img.used_clusters > 0);

    // the partition table, on the whole disk
    let disk_path = dir.join("disk.img");
    let gp = gpt(disk, bs, start, n, [1; 16], [2; 16]);
    apply(disk_path.to_str().unwrap(), disk, bs, &[], &gp, 0);
    check_gpt(&std::fs::read(&disk_path).unwrap(), disk, bs, start, n);
    if bs == 512 {
    let out = Command::new("sgdisk").arg("-v").arg(&disk_path).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("No problems found"), "sgdisk -v: {}", text);
    let out = Command::new("sgdisk").arg("-i").arg("1").arg(&disk_path).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(text.contains("C12A7328-F81F-11D2-BA4B-00A0C93EC93B"), "{}", text);
    assert!(text.contains("'HydatekOS'"), "{}", text);
    assert!(text.contains(&format!("First sector: {} ", start)), "{}", text);
    }

    // the file system, as its own file
    let part = dir.join("part.img");
    let p = part.to_str().unwrap();
    apply(p, fs.sectors as u64, bs, &img.zero, &img.writes, 0);
    let out = Command::new("fsck.fat").args(["-n", "-v"]).arg(&part).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "fsck.fat: {}", text);
    assert!(!text.contains("differ") && !text.contains("Free cluster summary wrong"), "fsck.fat: {}", text);

    // every file reads back through mtools, byte for byte
    for it in &items {
        let Some(data) = &it.data else { continue };
        let out = Command::new("mtype").env("LC_ALL", "C.UTF-8").args(["-i", p]).arg(format!("::/{}", it.path)).output().unwrap();
        assert!(out.status.success(), "mtype {}: {}", it.path, String::from_utf8_lossy(&out.stderr));
        assert_eq!(&out.stdout, data, "{}", it.path);
    }
    // long names come back as written
    let out = Command::new("mdir").env("LC_ALL", "C.UTF-8").args(["-i", p, "::/HYDATEK/home/Documents"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    for name in ["Photos 2026", "Long file name one.txt", "Long file name two.txt", "Thirteen char", "empty.txt"] {
        assert!(text.contains(name), "{} missing from:\n{}", name, text);
    }
    let out = Command::new("mdir").env("LC_ALL", "C.UTF-8").args(["-i", p, "-b", "::/HYDATEK/home/Notes"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).lines().count(), 300);
    // the label
    let out = Command::new("mdir").env("LC_ALL", "C.UTF-8").args(["-i", p, "::/"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("HYDATEK"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn too_much_doesnt_fit() {
    let fs = Fat32::new(300 << 11, 512).unwrap();
    let big = vec![Item { path: "big.bin".into(), data: Some(vec![0; 400 << 20]) }];
    assert!(fat32(&fs, &big, "X", 1, (0, 0), 2048).is_err());
}

#[test]
fn start_up_entry_layout() {
    let guid = [0xAB; 16];
    let o = load_option("HydatekOS", 1, 2048, 4192216, guid, "\\EFI\\BOOT\\BOOTX64.EFI");
    // attributes: active
    assert_eq!(u32::from_le_bytes(o[0..4].try_into().unwrap()), 1);
    let path_len = u16::from_le_bytes([o[4], o[5]]) as usize;
    // the description, UTF-16 with its 0
    let desc: Vec<u16> = o[6..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    assert_eq!(String::from_utf16(&desc).unwrap(), "HydatekOS");
    let p = &o[6 + (desc.len() + 1) * 2..];
    assert_eq!(p.len(), path_len);
    // Hard Drive node: partition 1 at 2048, its size, the GUID, GPT
    assert_eq!(&p[0..4], &[4, 1, 42, 0]);
    assert_eq!(u32::from_le_bytes(p[4..8].try_into().unwrap()), 1);
    assert_eq!(u64::from_le_bytes(p[8..16].try_into().unwrap()), 2048);
    assert_eq!(u64::from_le_bytes(p[16..24].try_into().unwrap()), 4192216);
    assert_eq!(&p[24..40], &guid);
    assert_eq!((p[40], p[41]), (2, 2));
    // File node with the path
    let f = &p[42..];
    assert_eq!((f[0], f[1]), (4, 4));
    let flen = u16::from_le_bytes([f[2], f[3]]) as usize;
    let name: Vec<u16> = f[4..flen].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    assert_eq!(String::from_utf16(&name).unwrap(), "\\EFI\\BOOT\\BOOTX64.EFI\0");
    // the end node closes it
    assert_eq!(&f[flen..], &[0x7F, 0xFF, 4, 0]);
}

#[test]
fn boot_order_puts_hydatekos_first() {
    let order = [1u8, 0, 3, 0, 2, 0];
    assert_eq!(boot_order_first(&order, 3), vec![3, 0, 1, 0, 2, 0]);
    assert_eq!(boot_order_first(&order, 7), vec![7, 0, 1, 0, 3, 0, 2, 0]);
    assert_eq!(boot_order_first(&[], 0), vec![0, 0]);
}
