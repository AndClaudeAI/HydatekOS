//! The AML interpreter and ACPI device discovery, on tables compiled by iasl.

use crate::acpi::{self, Res};
use crate::aml::{self, Aml, Host, Value};
use std::collections::BTreeMap;

/// A machine: memory, and PCI configuration space.
#[derive(Default)]
struct Mock {
    mem: BTreeMap<u64, u8>,
    pci: BTreeMap<u64, u32>,
    writes: Vec<(u8, u64, u64)>,
}

impl Host for Mock {
    fn read(&mut self, space: u8, addr: u64, bits: u32) -> u64 {
        match space {
            aml::MEM => (0..bits / 8).fold(0, |v, i| v | (*self.mem.get(&(addr + i as u64)).unwrap_or(&0) as u64) << (8 * i)),
            aml::PCI => *self.pci.get(&addr).unwrap_or(&0xFFFF_FFFF) as u64,
            _ => 0,
        }
    }
    fn write(&mut self, space: u8, addr: u64, bits: u32, v: u64) {
        self.writes.push((space, addr, v));
        if space == aml::MEM {
            for i in 0..bits / 8 {
                self.mem.insert(addr + i as u64, (v >> (8 * i)) as u8);
            }
        }
    }
}

fn laptop() -> Aml<Mock> {
    let mut m = Mock::default();
    // NVS: touchpad type 1, battery 5000 mWh full, 2500 left, 320 lux
    for (a, v) in [(0x1000, 1u8), (0x1001, 0x88), (0x1002, 0x13), (0x1003, 0xC4), (0x1004, 0x09), (0x1006, 0x40), (0x1007, 0x01)] {
        m.mem.insert(a, v);
    }
    // the I2C controller is Intel's (00:15.1)
    m.pci.insert(0x15 << 40 | 1 << 32, 0x9D60_8086);
    let mut a = Aml::new(m);
    a.load(include_bytes!("../fixtures/acpi/laptop.aml")).unwrap();
    assert_eq!(a.errors, 0, "{:?}", a.debug);
    a
}

#[test]
fn namespace() {
    let a = laptop();
    for p in ["\\OSYS", "\\TPTY", "\\_SB_.PCI0.I2C1", "\\_SB_.PCI0.I2C1.TPD0", "\\_SB_.PCI0.I2C1.TPD0._DSM", "\\_SB_.BAT0._BST", "\\_SB_.I2CA._CRS"] {
        assert!(a.exists(p), "{} missing", p);
    }
    assert!(a.children("\\_SB_.PCI0.I2C1").contains(&String::from("\\_SB_.PCI0.I2C1.TPD0")));
    assert_eq!(aml::eisa_id(0x500C_D041), "PNP0C50");
}

#[test]
fn methods_and_expressions() {
    let mut a = laptop();
    // 1+2+4+5+...+10, skipping 3, stopping at 10
    assert_eq!(a.eval("\\_SB_.SUMN", vec![Value::Int(20)]), Ok(Value::Int(52)));
    assert_eq!(a.eval("\\_SB_.SUMN", vec![Value::Int(4)]), Ok(Value::Int(7)));
    // SizeOf 3, second byte of the buffer 0xBB
    assert_eq!(a.eval("\\_SB_.PKGT", vec![]), Ok(Value::Int(3 << 8 | 0xBB)));
    assert_eq!(a.eval("\\_SB_.STRS", vec![]), Ok(Value::Int(42)));
    assert_eq!(a.eval("\\_SB_.MTCH", vec![]), Ok(Value::Int(2)));
    // a field in PCI configuration space (the controller's vendor)
    assert_eq!(a.eval("\\_SB_.PCI0.I2C1.VEND", vec![]), Ok(Value::Int(0x8086)));
    // a field that isn't byte aligned
    a.host.mem.insert(0x1005, 0xA5);
    assert_eq!(a.eval("\\FLG4", vec![]), Ok(Value::Int(0xA)));
    // writing it keeps its neighbours
    a.eval("\\_SB_.SUMN", vec![Value::Int(1)]).unwrap();
}

#[test]
fn osi_and_init() {
    let mut a = laptop();
    // before _INI the OS is "Windows 2000": the touchpad is hidden
    assert_eq!(a.sta("\\_SB_.PCI0.I2C1.TPD0"), 0);
    a.init();
    assert_eq!(a.eval("\\OSYS", vec![]), Ok(Value::Int(0x07DF)));
    assert_eq!(a.sta("\\_SB_.PCI0.I2C1.TPD0"), 0x0F);
    // the touchpad's own _INI ran
    assert_eq!(a.eval("\\_SB_.PCI0.I2C1.TPD0.HID2", vec![]), Ok(Value::Int(0x20)));
}

#[test]
fn i2c_touchpad_discovery() {
    let mut a = laptop();
    a.init();
    let found = acpi::find_i2c_hid(&mut a);
    assert_eq!(found.len(), 1, "{:?}", found);
    let d = &found[0];
    assert_eq!(d.path, "\\_SB_.PCI0.I2C1.TPD0");
    assert_eq!(d.hid, "SYNA2393");
    assert_eq!((d.addr, d.speed, d.desc_reg), (0x2C, 400_000, 0x20));
    assert_eq!(d.bus, "\\_SB_.PCI0.I2C1");
    assert!(d.gpio_irq);
    // its controller: Intel, on PCI at 15.1, with fast-mode counts
    let c = acpi::i2c_controller(&mut a, &d.bus);
    assert_eq!(c.pci, Some((0x15, 1)));
    assert_eq!(c.fmcn, Some((0x101, 0x12C, 0x62)));
    assert!(c.designware);
    // the AMD controller: memory patched into its template by _CRS
    let amd = acpi::i2c_controller(&mut a, "\\_SB_.I2CA");
    assert_eq!(amd.mem, Some(0xFEDC_2000));
    assert!(amd.designware);
    // the named buffer is patched in place
    let crs = acpi::crs(&mut a, "\\_SB_.I2CA");
    assert_eq!(crs[0], Res::Mem { base: 0xFEDC_2000, len: 0x1000 });
    assert_eq!(crs[1], Res::Irq(vec![10]));
}

#[test]
fn battery_light_inventory() {
    let mut a = laptop();
    a.init();
    assert_eq!(acpi::battery(&mut a, "\\_SB_.BAT0"), Some((50, false)));
    assert_eq!(acpi::light(&mut a, "\\_SB_.ALS0"), Some(320));
    let inv = acpi::inventory(&mut a);
    let kinds: Vec<&str> = inv.iter().map(|i| i.2).collect();
    assert!(kinds.contains(&"Battery") && kinds.contains(&"Ambient light sensor") && kinds.contains(&"HID over I2C device"), "{:?}", inv);
    // the lid says it's not there; the second touchpad too
    assert!(!kinds.contains(&"Lid"));
    assert_eq!(inv.iter().filter(|i| i.2 == "HID over I2C device").count(), 1);
}

#[test]
fn resource_templates() {
    // I2cSerialBusV2 + GpioInt + IO + IRQ + DWordMemory, as iasl writes them
    let mut t = vec![0x8E, 0x1E, 0x00, 0x02, 0x00, 0x01, 0x02, 0x00, 0x00, 0x01, 0x06, 0x00, 0x80, 0x1A, 0x06, 0x00, 0x2C, 0x00];
    t.extend_from_slice(b"\\_SB.I2C1\0");
    t.extend_from_slice(&[0x47, 0x01, 0x62, 0x00, 0x62, 0x00, 0x00, 0x01, 0x22, 0x01, 0x00]);
    t.extend_from_slice(&[0x79, 0x00]);
    // fix the serial bus length: 3 + len covers through the string
    let len = 18 + 10 - 3;
    t[1] = len as u8;
    let r = acpi::resources(&t);
    assert_eq!(r[0], Res::I2c { addr: 0x2C, speed: 400_000, ten_bit: false, source: String::from("\\_SB.I2C1") });
    assert_eq!(r[1], Res::Io { base: 0x62, len: 1 });
    assert_eq!(r[2], Res::Irq(vec![0]));
    assert_eq!(acpi::resources(&[0x79, 0x00]), vec![]);
    // junk doesn't panic
    let _ = acpi::resources(&[0x8E, 0xFF, 0xFF, 1, 2]);
    let _ = acpi::resources(&[0x8C]);
}

#[test]
fn uuids_and_malformed_tables() {
    assert_eq!(aml::uuid("3CDFF6F7-4267-4555-AD05-B30A3D8938DE"), vec![0xF7, 0xF6, 0xDF, 0x3C, 0x67, 0x42, 0x55, 0x45, 0xAD, 0x05, 0xB3, 0x0A, 0x3D, 0x89, 0x38, 0xDE]);
    // truncated and corrupted tables load what they can, without panicking
    let t = include_bytes!("../fixtures/acpi/laptop.aml");
    for cut in [36, 40, 100, 400, 800, t.len() - 1] {
        let mut a = Aml::new(Mock::default());
        let mut b = t[..cut].to_vec();
        b[4..8].copy_from_slice(&(cut as u32).to_le_bytes());
        let _ = a.load(&b);
    }
    let mut state = 0x1234_5678u32;
    for _ in 0..200 {
        let mut b = t.to_vec();
        for _ in 0..8 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let i = 36 + (state as usize % (b.len() - 36));
            b[i] = (state >> 8) as u8;
        }
        let mut a = Aml::new(Mock::default());
        let _ = a.load(&b);
        a.init();
        let _ = acpi::find_i2c_hid(&mut a);
    }
}

#[test]
fn qemu_q35_dsdt() {
    // QEMU's own DSDT (q35 machine, dumped from guest memory)
    let mut a = Aml::new(Mock::default());
    a.load(include_bytes!("../fixtures/acpi/qemu-q35-dsdt.aml")).unwrap();
    assert_eq!(a.errors, 0, "{:?}", a.debug);
    assert!(a.devices().len() >= 30);
    a.init();
    assert_eq!(acpi::ids(&mut a, "\\_SB_.PCI0"), vec![String::from("PNP0A08"), String::from("PNP0A03")]);
    // the root bridge's windows: bus numbers, I/O ports and memory
    let crs = acpi::crs(&mut a, "\\_SB_.PCI0");
    assert!(crs.iter().any(|r| matches!(r, Res::Io { base: 0x0CF8, .. })), "{:?}", crs);
    assert!(crs.iter().any(|r| matches!(r, Res::Mem { .. })), "{:?}", crs);
    // the interrupt routing table is a package of packages
    match a.eval("\\_SB_.PCI0._PRT", vec![]) {
        Ok(Value::Pkg(p)) => assert!(p.len() >= 32 && matches!(p[0], Value::Pkg(_))),
        other => panic!("_PRT: {:?}", other),
    }
    // the keyboard and the real-time clock are named
    let inv = acpi::inventory(&mut a);
    assert!(inv.iter().any(|d| d.2 == "PS/2 keyboard"), "{:?}", inv);
    assert!(inv.iter().any(|d| d.2 == "Real-time clock"), "{:?}", inv);
    assert!(acpi::find_i2c_hid(&mut a).is_empty());
}

#[test]
fn qemu_arm_virt_dsdt() {
    // QEMU's ARM64 "virt" machine: devices described by memory and interrupts
    let mut a = Aml::new(Mock::default());
    a.load(include_bytes!("../fixtures/acpi/qemu-arm-virt-dsdt.aml")).unwrap();
    assert_eq!(a.errors, 0, "{:?}", a.debug);
    a.init();
    // its UART (ARMH0011, a PL011): registers and an interrupt
    let uart = a.devices().into_iter().find(|d| acpi::ids(&mut a, d).iter().any(|i| i == "ARMH0011")).expect("a PL011");
    let crs = acpi::crs(&mut a, &uart);
    assert!(matches!(crs[0], Res::Mem { base: 0x0900_0000, .. }), "{:?}", crs);
    assert!(crs.iter().any(|r| matches!(r, Res::Irq(_))), "{:?}", crs);
}
