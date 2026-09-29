//! Hardware detection: SMBIOS, ARM cores, graphics names.

use crate::hw::*;

/// One SMBIOS structure: type, the formatted part after the 4-byte header,
/// and its strings.
fn structure(kind: u8, body: &[u8], strings: &[&str]) -> Vec<u8> {
    let mut v = vec![kind, (4 + body.len()) as u8, 0, 0];
    v.extend_from_slice(body);
    for s in strings {
        v.extend_from_slice(s.as_bytes());
        v.push(0);
    }
    if strings.is_empty() {
        v.push(0);
    }
    v.push(0);
    v
}

#[test]
fn smbios_snapdragon_laptop() {
    let mut t = Vec::new();
    // type 0: BIOS vendor (1), version (2)
    t.extend(structure(0, &[1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], &["Qualcomm", "UEFI 3.2"]));
    // type 1: manufacturer (1), product (2)
    t.extend(structure(1, &[1, 2, 0, 0], &["Microsoft Corporation", "Surface Laptop 7"]));
    // type 4: socket 1, type, family, maker 2 (0x07), id (8 bytes), version 3
    // (0x10), voltage, clock, max 3400 MHz (0x14), current, status, upgrade,
    // 3 cache handles, serial, asset, part, cores 12 (0x23), enabled, threads 12
    let mut body = vec![0u8; 0x2A - 4];
    body[0x04 - 4] = 1;
    body[0x07 - 4] = 2;
    body[0x10 - 4] = 3;
    body[0x14 - 4..0x16 - 4].copy_from_slice(&3400u16.to_le_bytes());
    body[0x23 - 4] = 12;
    body[0x24 - 4] = 12;
    body[0x25 - 4] = 12;
    t.extend(structure(4, &body, &["SoC", "Qualcomm Technologies Inc", "Snapdragon(R) X Elite - X1E80100 - Qualcomm(R) Oryon(TM) CPU"]));
    // a structure with no strings, then the end
    t.extend(structure(32, &[0; 7], &[]));
    t.extend(structure(127, &[], &[]));
    let s = parse_smbios(&t);
    assert_eq!(s.bios_vendor, "Qualcomm");
    assert_eq!(s.product, "Surface Laptop 7");
    assert_eq!(s.cpu_maker, "Qualcomm Technologies Inc");
    assert_eq!((s.cores, s.threads, s.max_mhz), (12, 12, 3400));
    let name = cpu_name(&s.cpu_name, "Qualcomm Oryon", 0x511F_0011);
    assert_eq!(name, "Snapdragon X Elite - X1E80100 - Qualcomm Oryon CPU");
    assert!(is_snapdragon(&name, 0x511F_0011));
    // truncated or garbage tables stop instead of reading past the end
    for n in 0..t.len() {
        let _ = parse_smbios(&t[..n]);
    }
}

#[test]
fn processor_names() {
    // x86: CPUID's brand string wins over SMBIOS
    assert_eq!(cpu_name("pc-q35-8.2", "Intel(R) Core(TM) i7-1165G7 CPU @ 2.80GHz", 0), "Intel Core i7-1165G7 CPU @ 2.80GHz");
    // ARM in a virtual machine: SMBIOS has the machine type, MIDR the core
    assert_eq!(cpu_name("virt-8.2", "Arm Cortex-A72", 0x410F_D083), "Arm Cortex-A72");
    assert_eq!(arm_core(0x410F_D083), ("Arm", "Arm Cortex-A72".to_string()));
    assert_eq!(arm_core(0x511F_0011), ("Qualcomm", "Qualcomm Oryon".to_string()));
    assert_eq!(arm_core(0x517F_802C).1, "Qualcomm Kryo (performance core)");
    assert_eq!(arm_core(0x000F_0510).0, "Emulated");
    assert!(arm_core(0x7700_1230).1.starts_with("ARM core"));
    assert!(!is_snapdragon("Arm Cortex-A72", 0x410F_D083));
}

#[test]
fn graphics_names() {
    assert_eq!(gpu_name(0x1234, 0x1111), "QEMU standard VGA");
    assert_eq!(gpu_name(0x1AF4, 0x1050), "Virtio GPU");
    assert_eq!(gpu_name(0x8086, 0x9A49), "Intel graphics");
    assert_eq!(gpu_name(0x1002, 0x15BF), "AMD Radeon graphics");
    assert_eq!(gpu_name(0x17CB, 0x0001), "Qualcomm Adreno graphics");
    assert_eq!(gpu_name(0xABCD, 0x0001), "Display controller abcd:0001");
}
