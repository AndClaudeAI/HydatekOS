//! ACPI: the firmware's tables, the devices in them, and what they use.
//!
//! - `resources`: resource templates (_CRS): memory, I/O ports, interrupts,
//!   GPIOs and I2C / SPI / UART connections;
//! - `find_i2c_hid`: every HID over I2C device (touchpads, touch screens,
//!   pens, sensor hubs) with its bus address, speed, HID descriptor register
//!   (from _DSM) and the controller it hangs off;
//! - `i2c_controller`: where that controller's registers are and its timing;
//! - `inventory`: the devices worth naming in Settings (batteries, the lid,
//!   buttons, light sensors…).
//!
//! These run on aml.rs and are host-tested; `tables` and `KernelHost`
//! (UEFI only) find the tables and reach the hardware.

use crate::aml::{self, Aml, Host, Value};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub enum Res {
    Mem { base: u64, len: u64 },
    Io { base: u16, len: u16 },
    Irq(Vec<u32>),
    Gpio { int: bool, pins: Vec<u16>, source: String },
    I2c { addr: u16, speed: u32, ten_bit: bool, source: String },
    Spi { speed: u32, cs: u16, source: String },
    Uart { baud: u32, source: String },
}

fn u16le(b: &[u8], i: usize) -> u16 {
    b.get(i..i + 2).map_or(0, |x| u16::from_le_bytes([x[0], x[1]]))
}
fn u32le(b: &[u8], i: usize) -> u32 {
    b.get(i..i + 4).map_or(0, |x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}
fn u64le(b: &[u8], i: usize) -> u64 {
    u32le(b, i) as u64 | (u32le(b, i + 4) as u64) << 32
}
fn cstr(b: &[u8], i: usize) -> String {
    b.get(i..).unwrap_or(&[]).iter().take_while(|c| **c != 0).map(|c| *c as char).collect()
}

/// Read a resource template.
pub fn resources(b: &[u8]) -> Vec<Res> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let t = b[i];
        if t & 0x80 != 0 {
            let len = u16le(b, i + 1) as usize;
            let end = (i + 3 + len).min(b.len());
            let d = &b[i..end];
            match t {
                0x86 => out.push(Res::Mem { base: u32le(d, 4) as u64, len: u32le(d, 8) as u64 }),
                0x85 => out.push(Res::Mem { base: u32le(d, 4) as u64, len: u32le(d, 16) as u64 }),
                0x87 | 0x88 | 0x8A => {
                    let kind = d.get(3).copied().unwrap_or(0);
                    let (min, len) = match t {
                        0x87 => (u32le(d, 10) as u64, u32le(d, 22) as u64),
                        0x88 => (u16le(d, 8) as u64, u16le(d, 14) as u64),
                        _ => (u64le(d, 14), u64le(d, 38)),
                    };
                    match kind {
                        0 if len > 0 => out.push(Res::Mem { base: min, len }),
                        1 if len > 0 => out.push(Res::Io { base: min as u16, len: len as u16 }),
                        _ => {}
                    }
                }
                0x89 => {
                    let n = d.get(4).copied().unwrap_or(0) as usize;
                    out.push(Res::Irq((0..n).map(|k| u32le(d, 5 + 4 * k)).collect()));
                }
                0x8C => {
                    let int = d.get(4) == Some(&0);
                    let (pt, rs) = (u16le(d, 14) as usize, u16le(d, 17) as usize);
                    let pins = (pt..rs.max(pt)).step_by(2).map(|k| u16le(d, k)).collect();
                    out.push(Res::Gpio { int, pins, source: cstr(d, rs) });
                }
                0x8E => {
                    let kind = d.get(5).copied().unwrap_or(0);
                    let tdlen = u16le(d, 10) as usize;
                    let source = cstr(d, 12 + tdlen);
                    match kind {
                        1 => out.push(Res::I2c { addr: u16le(d, 16), speed: u32le(d, 12), ten_bit: u16le(d, 7) & 1 != 0, source }),
                        2 => out.push(Res::Spi { speed: u32le(d, 12), cs: u16le(d, 19), source }),
                        3 => out.push(Res::Uart { baud: u32le(d, 12), source }),
                        _ => {}
                    }
                }
                _ => {}
            }
            i = end;
        } else {
            let len = (t & 7) as usize;
            match t >> 3 {
                0x04 => {
                    let mask = u16le(b, i + 1);
                    out.push(Res::Irq((0..16).filter(|k| mask >> k & 1 != 0).collect()));
                }
                0x08 => out.push(Res::Io { base: u16le(b, i + 2), len: b.get(i + 7).copied().unwrap_or(0) as u16 }),
                0x09 => out.push(Res::Io { base: u16le(b, i + 1) & 0x3FF, len: b.get(i + 3).copied().unwrap_or(0) as u16 }),
                0x0F => break,
                _ => {}
            }
            i += 1 + len;
        }
    }
    out
}

/// A device's hardware ids: _HID, then _CID (one or a package of them).
pub fn ids<H: Host>(a: &mut Aml<H>, dev: &str) -> Vec<String> {
    let mut out = Vec::new();
    let add = |v: &Value, out: &mut Vec<String>| match v {
        Value::Str(s) => out.push(s.clone()),
        Value::Int(i) => out.push(aml::eisa_id(*i)),
        _ => {}
    };
    if let Some(Ok(v)) = a.child(dev, "_HID") {
        add(&v, &mut out);
    }
    if let Some(Ok(v)) = a.child(dev, "_CID") {
        match &v {
            Value::Pkg(p) => {
                for e in p {
                    add(e, &mut out);
                }
            }
            _ => add(&v, &mut out),
        }
    }
    out
}

pub fn crs<H: Host>(a: &mut Aml<H>, dev: &str) -> Vec<Res> {
    match a.child(dev, "_CRS") {
        Some(Ok(Value::Buf(b))) => resources(&b),
        _ => vec![],
    }
}

/// HID over I2C's _DSM: which register the HID descriptor is at.
pub const HID_I2C_DSM: &str = "3CDFF6F7-4267-4555-AD05-B30A3D8938DE";

/// A HID over I2C device ACPI describes.
#[derive(Clone, Debug, PartialEq)]
pub struct I2cHidDev {
    pub path: String,
    pub hid: String,
    pub addr: u16,
    pub speed: u32,
    pub desc_reg: u16,
    /// the controller's ACPI path
    pub bus: String,
    /// its interrupt is a GPIO (HydatekOS polls instead)
    pub gpio_irq: bool,
}

pub fn find_i2c_hid<H: Host>(a: &mut Aml<H>) -> Vec<I2cHidDev> {
    let mut out = Vec::new();
    for dev in a.devices() {
        let ids = ids(a, &dev);
        if !ids.iter().any(|i| i == "PNP0C50" || i == "ACPI0C50") {
            continue;
        }
        if a.sta(&dev) & 1 == 0 {
            continue;
        }
        let res = crs(a, &dev);
        let Some(Res::I2c { addr, speed, source, .. }) = res.iter().find(|r| matches!(r, Res::I2c { .. })).cloned() else { continue };
        let dsm = aml::join(&dev, "_DSM");
        let desc_reg = if a.exists(&dsm) {
            let args = vec![Value::Buf(aml::uuid(HID_I2C_DSM)), Value::Int(1), Value::Int(1), Value::Pkg(vec![])];
            a.eval(&dsm, args).ok().and_then(|v| a.to_int(&v).ok()).unwrap_or(1) as u16
        } else {
            1
        };
        let src = aml::normalize(&source);
        let bus = a.resolve(&aml::parent(&dev), &src).unwrap_or(src);
        let gpio_irq = res.iter().any(|r| matches!(r, Res::Gpio { int: true, .. }));
        out.push(I2cHidDev { path: dev, hid: ids[0].clone(), addr, speed, desc_reg, bus, gpio_irq });
    }
    out
}

/// DesignWare I2C controllers, by ACPI id (Intel LPSS, AMD, Ampere, HiSilicon…).
pub const DESIGNWARE: [&str; 13] = ["INT33C2", "INT33C3", "INT3432", "INT3433", "80860F41", "808622C1", "AMDI0010", "AMDI0019", "AMDI0510", "AMD0010", "APMC0D0F", "HISI02A1", "HYGO0010"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct I2cCtl {
    pub path: String,
    pub hid: String,
    /// its registers, when ACPI gives them (not a PCI device)
    pub mem: Option<u64>,
    /// its PCI device and function (Intel LPSS controllers are on PCI)
    pub pci: Option<(u8, u8)>,
    /// fast-mode clock counts from FMCN: high, low, SDA hold
    pub fmcn: Option<(u32, u32, u32)>,
    pub designware: bool,
}

pub fn i2c_controller<H: Host>(a: &mut Aml<H>, path: &str) -> I2cCtl {
    let ids = ids(a, path);
    let mem = crs(a, path).iter().find_map(|r| match r {
        Res::Mem { base, .. } => Some(*base),
        _ => None,
    });
    let pci = if mem.is_none() { a.int(path, "_ADR").map(|v| ((v >> 16) as u8, v as u8)) } else { None };
    let fmcn = match a.child(path, "FMCN") {
        Some(Ok(Value::Pkg(p))) if p.len() >= 3 => {
            let g = |i: usize| a.to_int(&p[i]).unwrap_or(0) as u32;
            Some((g(0), g(1), g(2)))
        }
        _ => None,
    };
    // Intel's PCI controllers have no _HID; their name says what they are
    let designware = ids.iter().any(|i| DESIGNWARE.contains(&i.as_str())) || (pci.is_some() && aml::leaf(path).starts_with("I2C"));
    I2cCtl { path: path.to_string(), hid: ids.first().cloned().unwrap_or_default(), mem, pci, fmcn, designware }
}

/// A device worth naming, by its id.
pub fn device_kind(id: &str) -> Option<&'static str> {
    Some(match id {
        "PNP0C50" | "ACPI0C50" => "HID over I2C device",
        "PNP0C0A" => "Battery",
        "ACPI0003" => "AC adapter",
        "PNP0C0D" => "Lid",
        "PNP0C0C" => "Power button",
        "PNP0C0E" => "Sleep button",
        "ACPI0008" => "Ambient light sensor",
        "PNP0C09" => "Embedded controller",
        "PNP0303" | "PNP030B" => "PS/2 keyboard",
        "PNP0F13" | "PNP0F03" => "PS/2 mouse",
        "ACPI0011" => "Generic buttons",
        "INT33D5" | "INTC1051" | "INTC1070" => "Intel HID event filter",
        "PNP0C14" => "WMI",
        "PNP0A03" | "PNP0A08" => "PCI root bridge",
        "PNP0B00" => "Real-time clock",
        "PNP0501" => "Serial port",
        "ACPI0007" => "Processor",
        "PNP0C80" => "Memory",
        "ACPI000E" => "Wake alarm",
        "INT0002" => "Intel GPIO wake",
        "MSFT0101" => "TPM",
        "QCOM0C10" | "QCOM0220" | "QCOM0411" => "Qualcomm serial engine",
        id if DESIGNWARE.contains(&id) => "I2C controller (DesignWare)",
        _ => return None,
    })
}

/// The devices ACPI describes that Settings names, present ones only:
/// (path, id, kind).
pub fn inventory<H: Host>(a: &mut Aml<H>) -> Vec<(String, String, &'static str)> {
    let mut out = Vec::new();
    for dev in a.devices() {
        let ids = ids(a, &dev);
        let Some((id, kind)) = ids.iter().find_map(|i| device_kind(i).map(|k| (i.clone(), k))) else { continue };
        if kind == "Processor" || kind == "PCI root bridge" || kind == "Memory" {
            continue;
        }
        if a.sta(&dev) & 1 == 0 {
            continue;
        }
        out.push((dev, id, kind));
    }
    out
}

/// The present devices with this hardware id.
pub fn devices_with<H: Host>(a: &mut Aml<H>, id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for d in a.devices() {
        if ids(a, &d).iter().any(|i| i == id) && a.sta(&d) & 1 != 0 {
            out.push(d);
        }
    }
    out
}

/// A battery's charge (percent) and whether it's charging, from _BIX/_BIF and _BST.
pub fn battery<H: Host>(a: &mut Aml<H>, dev: &str) -> Option<(u32, bool)> {
    let full = match a.child(dev, "_BIX").or_else(|| a.child(dev, "_BIF")) {
        Some(Ok(Value::Pkg(p))) => {
            // _BIX has a revision first; last full charge capacity
            let i = if p.len() > 13 { 3 } else { 2 };
            a.to_int(p.get(i)?).ok()?
        }
        _ => return None,
    };
    let Some(Ok(Value::Pkg(st))) = a.child(dev, "_BST") else { return None };
    let state = a.to_int(st.first()?).ok()?;
    let left = a.to_int(st.get(2)?).ok()?;
    if full == 0 || full == 0xFFFF_FFFF || left == 0xFFFF_FFFF {
        return None;
    }
    Some(((left * 100 / full).min(100) as u32, state & 2 != 0))
}

/// Ambient light in lux from an ACPI light sensor (_ALI).
pub fn light<H: Host>(a: &mut Aml<H>, dev: &str) -> Option<u32> {
    a.int(dev, "_ALI").filter(|v| *v != 0xFFFF_FFFF).map(|v| v as u32)
}

/// Names for the log.
pub fn describe(d: &I2cHidDev) -> String {
    format!("{} ({}) at {:#04x} on {}, {} kHz, HID descriptor at {:#06x}{}", d.path, d.hid, d.addr, d.bus, d.speed / 1000, d.desc_reg, if d.gpio_irq { ", GPIO interrupt" } else { "" })
}

// ---- the tables and the hardware (UEFI) ------------------------------------------------

#[cfg(target_os = "uefi")]
pub use firmware::*;

#[cfg(target_os = "uefi")]
mod firmware {
    use super::*;
    use crate::efi::{self, Guid};

    const ACPI2_GUID: Guid = Guid(0x8868E871, 0xE4F1, 0x11D3, [0xBC, 0x22, 0x00, 0x80, 0xC7, 0x3C, 0x88, 0x81]);

    #[repr(C)]
    struct ConfigTable {
        guid: Guid,
        table: *const u8,
    }

    unsafe fn slice(p: *const u8, n: usize) -> &'static [u8] {
        unsafe { core::slice::from_raw_parts(p, n) }
    }

    unsafe fn table_at(p: *const u8) -> Option<&'static [u8]> {
        if p.is_null() {
            return None;
        }
        let len = u32::from_le_bytes(unsafe { slice(p.add(4), 4) }.try_into().ok()?) as usize;
        (36..16 << 20).contains(&len).then(|| unsafe { slice(p, len) })
    }

    /// Every ACPI table: the XSDT's, plus the DSDT the FADT points to.
    pub fn tables() -> Vec<&'static [u8]> {
        let st = efi::st();
        let cts = st.tables as *const ConfigTable;
        let mut rsdp = core::ptr::null();
        for i in 0..st.n_tables {
            let t = unsafe { &*cts.add(i) };
            if t.guid.0 == ACPI2_GUID.0 && t.guid.1 == ACPI2_GUID.1 && t.guid.2 == ACPI2_GUID.2 && t.guid.3 == ACPI2_GUID.3 {
                rsdp = t.table;
            }
        }
        let mut out = Vec::new();
        unsafe {
            if rsdp.is_null() || slice(rsdp, 8) != b"RSD PTR " || *rsdp.add(15) < 2 {
                return out;
            }
            let xsdt = u64::from_le_bytes(slice(rsdp.add(24), 8).try_into().unwrap()) as usize as *const u8;
            let Some(x) = table_at(xsdt) else { return out };
            for k in 0..(x.len() - 36) / 8 {
                let p = u64::from_le_bytes(x[36 + 8 * k..44 + 8 * k].try_into().unwrap()) as usize as *const u8;
                let Some(t) = table_at(p) else { continue };
                out.push(t);
                if &t[0..4] == b"FACP" {
                    let mut d = if t.len() >= 148 { u64::from_le_bytes(t[140..148].try_into().unwrap()) as usize } else { 0 };
                    if d == 0 && t.len() >= 44 {
                        d = u32::from_le_bytes(t[40..44].try_into().unwrap()) as usize;
                    }
                    if let Some(dsdt) = table_at(d as *const u8) {
                        out.push(dsdt);
                    }
                }
            }
        }
        out
    }

    /// Memory, I/O ports and PCI configuration space (through ECAM, from the MCFG table).
    pub struct KernelHost {
        pub ecam: u64,
        pub ec: Option<(u16, u16)>,
    }

    impl Host for KernelHost {
        fn read(&mut self, space: u8, addr: u64, bits: u32) -> u64 {
            unsafe {
                match (space, bits) {
                    (aml::MEM, 8) => core::ptr::read_volatile(addr as usize as *const u8) as u64,
                    (aml::MEM, 16) => core::ptr::read_volatile(addr as usize as *const u16) as u64,
                    (aml::MEM, 32) => core::ptr::read_volatile(addr as usize as *const u32) as u64,
                    (aml::MEM, _) => core::ptr::read_volatile(addr as usize as *const u64),
                    (aml::IO, 8) => crate::arch::inb(addr as u16) as u64,
                    (aml::IO, 16) => crate::arch::inw(addr as u16) as u64,
                    (aml::IO, _) => crate::arch::inl(addr as u16) as u64,
                    (aml::PCI, _) if self.ecam != 0 => {
                        let a = self.ecam_addr(addr);
                        match bits {
                            8 => core::ptr::read_volatile(a as *const u8) as u64,
                            16 => core::ptr::read_volatile(a as *const u16) as u64,
                            _ => core::ptr::read_volatile(a as *const u32) as u64,
                        }
                    }
                    (aml::EC, _) => self.ec_read(addr as u8) as u64,
                    _ => u64::MAX >> (64 - bits.min(64)),
                }
            }
        }

        fn write(&mut self, space: u8, addr: u64, bits: u32, v: u64) {
            unsafe {
                match (space, bits) {
                    (aml::MEM, 8) => core::ptr::write_volatile(addr as usize as *mut u8, v as u8),
                    (aml::MEM, 16) => core::ptr::write_volatile(addr as usize as *mut u16, v as u16),
                    (aml::MEM, 32) => core::ptr::write_volatile(addr as usize as *mut u32, v as u32),
                    (aml::MEM, _) => core::ptr::write_volatile(addr as usize as *mut u64, v),
                    (aml::IO, 8) => crate::arch::outb(addr as u16, v as u8),
                    (aml::IO, 16) => crate::arch::outw(addr as u16, v as u16),
                    (aml::IO, _) => crate::arch::outl(addr as u16, v as u32),
                    (aml::PCI, _) if self.ecam != 0 => {
                        let a = self.ecam_addr(addr);
                        match bits {
                            8 => core::ptr::write_volatile(a as *mut u8, v as u8),
                            16 => core::ptr::write_volatile(a as *mut u16, v as u16),
                            _ => core::ptr::write_volatile(a as *mut u32, v as u32),
                        }
                    }
                    (aml::EC, 8) => self.ec_write(addr as u8, v as u8),
                    _ => {}
                }
            }
        }

        fn sleep_ms(&mut self, ms: u64) {
            (efi::bs().stall)(ms as usize * 1000);
        }

        fn timer(&mut self) -> u64 {
            crate::arch::ms() * 10_000
        }
    }

    impl KernelHost {
        fn ecam_addr(&self, a: u64) -> usize {
            let (bus, dev, func, off) = (a >> 48 & 0xFF, a >> 40 & 0x1F, a >> 32 & 7, a & 0xFFF);
            (self.ecam + (bus << 20 | dev << 15 | func << 12 | off)) as usize
        }

        /// The embedded controller: wait until it can take a byte / has one.
        fn ec_wait(&self, cmd: u16, want_obf: bool) -> bool {
            for _ in 0..20_000 {
                let s = unsafe { crate::arch::inb(cmd) };
                if want_obf && s & 1 != 0 || !want_obf && s & 2 == 0 {
                    return true;
                }
            }
            false
        }

        fn ec_read(&mut self, reg: u8) -> u8 {
            let Some((data, cmd)) = self.ec else { return 0 };
            if !self.ec_wait(cmd, false) {
                return 0;
            }
            unsafe { crate::arch::outb(cmd, 0x80) };
            if !self.ec_wait(cmd, false) {
                return 0;
            }
            unsafe { crate::arch::outb(data, reg) };
            if !self.ec_wait(cmd, true) {
                return 0;
            }
            unsafe { crate::arch::inb(data) }
        }

        fn ec_write(&mut self, reg: u8, v: u8) {
            let Some((data, cmd)) = self.ec else { return };
            if !self.ec_wait(cmd, false) {
                return;
            }
            unsafe { crate::arch::outb(cmd, 0x81) };
            if !self.ec_wait(cmd, false) {
                return;
            }
            unsafe { crate::arch::outb(data, reg) };
            if !self.ec_wait(cmd, false) {
                return;
            }
            unsafe { crate::arch::outb(data, v) };
        }
    }

    /// Load every DSDT and SSDT and run _INI.
    pub fn load() -> Aml<KernelHost> {
        let tabs = tables();
        let mut ecam = 0;
        for t in &tabs {
            if &t[0..4] == b"MCFG" && t.len() >= 60 {
                ecam = u64::from_le_bytes(t[44..52].try_into().unwrap());
            }
        }
        // the embedded controller's ports, from ECDT (so EC regions work while loading)
        let mut ec = None;
        for t in &tabs {
            if &t[0..4] == b"ECDT" && t.len() >= 65 {
                let (c, d) = (u64::from_le_bytes(t[40..48].try_into().unwrap()), u64::from_le_bytes(t[52..60].try_into().unwrap()));
                if c != 0 && d != 0 && c < 0x10000 && d < 0x10000 {
                    ec = Some((d as u16, c as u16));
                }
            }
        }
        // no embedded controller ports on ARM
        #[cfg(not(target_arch = "x86_64"))]
        let ec: Option<(u16, u16)> = {
            let _ = ec;
            None
        };
        let mut a = Aml::new(KernelHost { ecam, ec });
        for t in tabs.iter().filter(|t| &t[0..4] == b"DSDT") {
            let _ = a.load(t);
            log!("acpi: DSDT at {:#x}, {} bytes", t.as_ptr() as usize, t.len());
        }
        for t in tabs.iter().filter(|t| &t[0..4] == b"SSDT") {
            let _ = a.load(t);
        }
        // no ECDT: the EC device's _CRS names its ports
        #[cfg(target_arch = "x86_64")]
        if a.host.ec.is_none() {
            for d in a.devices() {
                if ids(&mut a, &d).iter().any(|i| i == "PNP0C09") {
                    let io: Vec<u16> = crs(&mut a, &d).iter().filter_map(|r| if let Res::Io { base, .. } = r { Some(*base) } else { None }).collect();
                    if io.len() >= 2 {
                        a.host.ec = Some((io[0], io[1]));
                    }
                }
            }
        }
        a.init();
        log!("acpi: {} objects, {} devices, {} load errors{}", a.ns.len(), a.devices().len(), a.errors, if a.host.ec.is_some() { ", embedded controller" } else { "" });
        a
    }
}
