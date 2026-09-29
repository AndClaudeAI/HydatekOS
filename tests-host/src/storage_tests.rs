//! Partition tables and file systems, on a disk made by mformat and mkfs.ext4
//! (the fixture keeps only its sectors that aren't zero).

use crate::storage::*;
use std::collections::BTreeMap;

struct Img {
    sectors: BTreeMap<u64, Vec<u8>>,
    size: u64,
}

impl Disk for Img {
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> bool {
        if lba >= self.size {
            return false;
        }
        match self.sectors.get(&lba) {
            Some(s) => buf[..512].copy_from_slice(s),
            None => buf[..512].fill(0),
        }
        true
    }
    fn size(&self) -> u64 {
        self.size
    }
}

fn gpt_disk() -> Img {
    let z = include_bytes!("../fixtures/storage-gpt.bin");
    let raw = crate::zip::inflate(&z[2..], 1 << 16).expect("zlib");
    let size = u64::from_le_bytes(raw[..8].try_into().unwrap());
    let mut sectors = BTreeMap::new();
    for c in raw[8..].chunks(520) {
        sectors.insert(u64::from_le_bytes(c[..8].try_into().unwrap()), c[8..].to_vec());
    }
    Img { sectors, size }
}

#[test]
fn gpt_fat32_and_ext4() {
    let mut d = gpt_disk();
    let p = partitions(&mut d);
    assert_eq!(p.len(), 2, "{:?}", p);
    assert_eq!((p[0].start, p[0].sectors), (2048, 64 * 2048));
    assert_eq!(p[0].kind, "Basic data");
    assert_eq!((p[0].fs.as_str(), p[0].label.as_str()), ("FAT32", "HYDADATA"));
    assert_eq!(p[1].kind, "Linux data");
    assert_eq!((p[1].fs.as_str(), p[1].label.as_str()), ("ext4", "linuxhome"));
    assert_eq!(summary(&p), "2 partitions: FAT32 “HYDADATA”, ext4 “linuxhome”");
}

#[test]
fn mbr_and_bare_volumes() {
    // an MBR with one FAT32 partition: the GPT disk's FAT boot sector moved
    let mut g = gpt_disk();
    let mut fat = vec![0u8; 512];
    g.read(2048, &mut fat);
    let mut mbr = vec![0u8; 512];
    mbr[446 + 4] = 0x0C;
    mbr[446 + 8..446 + 12].copy_from_slice(&63u32.to_le_bytes());
    mbr[446 + 12..446 + 16].copy_from_slice(&1000u32.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    let mut d = Img { sectors: BTreeMap::from([(0, mbr), (63, fat.clone())]), size: 2000 };
    let p = partitions(&mut d);
    assert_eq!(p.len(), 1);
    assert_eq!((p[0].start, p[0].kind.as_str(), p[0].fs.as_str(), p[0].label.as_str()), (63, "FAT32", "FAT32", "HYDADATA"));
    // a stick formatted without a table
    let mut d = Img { sectors: BTreeMap::from([(0, fat)]), size: 2000 };
    let p = partitions(&mut d);
    assert_eq!((p[0].kind.as_str(), p[0].fs.as_str()), ("Whole disk", "FAT32"));
    // a blank disk
    let mut d = Img { sectors: BTreeMap::new(), size: 2000 };
    assert!(partitions(&mut d).is_empty());
    assert_eq!(summary(&[]), "Empty (no partitions)");
}

#[test]
fn names_and_sizes() {
    assert_eq!(size_text(512 * 1000 * 1000 * 1000), "512 GB");
    assert_eq!(size_text(1_500_000_000_000), "1.5 TB");
    assert_eq!(size_text(100_663_296), "100 MB");
    assert_eq!(gpt_kind("C12A7328-F81F-11D2-BA4B-00A0C93EC93B"), "EFI system");
    // ATA strings are byte-swapped words
    let mut id = vec![0u8; 512];
    id[54..62].copy_from_slice(b"EQUMH DD");
    assert_eq!(ata_string(&id, 27, 4), "QEMU HDD");
}
