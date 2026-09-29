//! What HydatekOS is running on: the processor (Intel, AMD, Qualcomm
//! Snapdragon, other ARM designs), its cores and features, the graphics
//! hardware and the screen modes the firmware offers. Settings › About and
//! Display show it.
//!
//! Sources, most specific first:
//! - the firmware's SMBIOS tables (the maker's names for the machine and
//!   processor: "Snapdragon(R) X Elite - X1E78100", "Intel(R) Core(TM) i7…");
//! - the processor itself (x86 CPUID, ARM MIDR_EL1 and ID registers: arch.rs);
//! - UEFI's MP Services protocol (how many cores);
//! - PCI configuration space (display controllers: vendor and device);
//! - the Graphics Output Protocol (screen modes).
//!
//! The parsing and the name tables are plain functions, checked by the host
//! tests; `detect` asks the firmware.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// What SMBIOS says (empty strings / 0 when it doesn't).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Smbios {
    pub bios_vendor: String,
    pub bios_version: String,
    pub maker: String,
    pub product: String,
    pub cpu_maker: String,
    pub cpu_name: String,
    pub cores: u32,
    pub threads: u32,
    pub max_mhz: u32,
}

/// Read the SMBIOS structure table (`data`: the structures, one after another).
pub fn parse_smbios(data: &[u8]) -> Smbios {
    let mut out = Smbios::default();
    let mut i = 0;
    while i + 4 <= data.len() {
        let (kind, len) = (data[i], data[i + 1] as usize);
        if len < 4 || i + len > data.len() {
            break;
        }
        let f = &data[i..i + len];
        // the strings follow the formatted part, ending with two NULs
        let mut j = i + len;
        let mut strings: Vec<String> = Vec::new();
        loop {
            let start = j;
            while j < data.len() && data[j] != 0 {
                j += 1;
            }
            if j >= data.len() {
                break;
            }
            if j == start {
                j += 1;
                // an empty set is two NULs as well
                if strings.is_empty() && j < data.len() && data[j] == 0 {
                    j += 1;
                }
                break;
            }
            strings.push(String::from_utf8_lossy(&data[start..j]).trim().to_string());
            j += 1;
        }
        let s = |off: usize| -> String {
            match f.get(off) {
                Some(&n) if n > 0 => strings.get(n as usize - 1).cloned().unwrap_or_default(),
                _ => String::new(),
            }
        };
        let u16_at = |off: usize| -> u32 { if off + 2 <= f.len() { u16::from_le_bytes([f[off], f[off + 1]]) as u32 } else { 0 } };
        match kind {
            0 => {
                out.bios_vendor = s(0x04);
                out.bios_version = s(0x05);
            }
            1 => {
                out.maker = s(0x04);
                out.product = s(0x05);
            }
            // the first processor (a second socket repeats it)
            4 if out.cpu_name.is_empty() => {
                out.cpu_maker = s(0x07);
                out.cpu_name = s(0x10);
                out.max_mhz = u16_at(0x14);
                let byte = |off: usize| f.get(off).copied().unwrap_or(0) as u32;
                out.cores = match byte(0x23) {
                    0xFF => u16_at(0x2A),
                    n => n,
                };
                out.threads = match byte(0x25) {
                    0xFF => u16_at(0x2E),
                    n => n,
                };
            }
            127 => break,
            _ => {}
        }
        i = j;
    }
    out
}

/// An ARM core from its Main ID Register: (implementer, core design).
#[cfg_attr(target_arch = "x86_64", allow(dead_code))]
pub fn arm_core(midr: u64) -> (&'static str, String) {
    let imp = (midr >> 24) & 0xFF;
    let part = (midr >> 4) & 0xFFF;
    let vendor = match imp {
        0x41 => "Arm",
        0x42 => "Broadcom",
        0x46 => "Fujitsu",
        0x48 => "HiSilicon",
        0x4E => "NVIDIA",
        0x50 => "Applied Micro",
        0x51 => "Qualcomm",
        0x61 => "Apple",
        0x6D => "Microsoft",
        0xC0 => "Ampere",
        // "reserved for software use": emulators (QEMU's "max" processor)
        0x00 => "Emulated",
        _ => "",
    };
    let design = match (imp, part) {
        (0x41, 0xD03) => "Cortex-A53",
        (0x41, 0xD04) => "Cortex-A35",
        (0x41, 0xD05) => "Cortex-A55",
        (0x41, 0xD07) => "Cortex-A57",
        (0x41, 0xD08) => "Cortex-A72",
        (0x41, 0xD09) => "Cortex-A73",
        (0x41, 0xD0A) => "Cortex-A75",
        (0x41, 0xD0B) => "Cortex-A76",
        (0x41, 0xD0C) => "Neoverse N1",
        (0x41, 0xD0D) => "Cortex-A77",
        (0x41, 0xD40) => "Neoverse V1",
        (0x41, 0xD41) => "Cortex-A78",
        (0x41, 0xD44) => "Cortex-X1",
        (0x41, 0xD46) => "Cortex-A510",
        (0x41, 0xD47) => "Cortex-A710",
        (0x41, 0xD48) => "Cortex-X2",
        (0x41, 0xD49) => "Neoverse N2",
        (0x41, 0xD4B) => "Cortex-A78C",
        (0x41, 0xD4D) => "Cortex-A715",
        (0x41, 0xD4E) => "Cortex-X3",
        (0x41, 0xD4F) => "Neoverse V2",
        (0x41, 0xD80) => "Cortex-A520",
        (0x41, 0xD81) => "Cortex-A720",
        (0x41, 0xD82) => "Cortex-X4",
        // Snapdragon X Elite / Plus
        (0x51, 0x001) => "Oryon",
        (0x51, 0x800) | (0x51, 0x802) | (0x51, 0x804) => "Kryo (performance core)",
        (0x51, 0x801) | (0x51, 0x803) | (0x51, 0x805) => "Kryo (efficiency core)",
        (0x51, 0xC00) => "Falkor",
        _ => "",
    };
    let model = match (vendor, design) {
        ("Emulated", _) => String::from("Emulated ARM64 processor (all features)"),
        ("", _) => format!("ARM core (implementer {:#04x}, part {:#05x})", imp, part),
        (v, "") => format!("{} core (part {:#05x})", v, part),
        (v, d) => format!("{} {}", v, d),
    };
    (if vendor.is_empty() { "ARM" } else { vendor }, model)
}

/// A PCI vendor's name.
pub fn pci_vendor(id: u16) -> &'static str {
    match id {
        0x8086 => "Intel",
        0x1002 | 0x1022 => "AMD",
        0x10DE => "NVIDIA",
        0x17CB | 0x5143 => "Qualcomm",
        0x13B5 => "Arm",
        0x1234 => "QEMU",
        0x1AF4 | 0x1B36 => "Red Hat",
        0x15AD => "VMware",
        0x80EE => "VirtualBox",
        0x1414 => "Microsoft",
        0x1A03 => "ASPEED",
        0x102B => "Matrox",
        _ => "",
    }
}

/// A display controller's name from its PCI vendor and device.
pub fn gpu_name(vendor: u16, device: u16) -> String {
    let known = match (vendor, device) {
        (0x1234, 0x1111) => "QEMU standard VGA",
        (0x1AF4, 0x1050) => "Virtio GPU",
        (0x1B36, 0x0100) => "QXL display",
        (0x15AD, 0x0405) => "VMware SVGA II",
        (0x80EE, 0xBEEF) => "VirtualBox graphics",
        (0x1414, 0x5353) => "Hyper-V video",
        (0x1A03, 0x2000) => "ASPEED server graphics",
        _ => "",
    };
    if !known.is_empty() {
        return known.to_string();
    }
    match pci_vendor(vendor) {
        "Intel" => "Intel graphics".to_string(),
        "AMD" => "AMD Radeon graphics".to_string(),
        "NVIDIA" => "NVIDIA GeForce / RTX graphics".to_string(),
        "Qualcomm" => "Qualcomm Adreno graphics".to_string(),
        "" => format!("Display controller {:04x}:{:04x}", vendor, device),
        v => format!("{} graphics", v),
    }
}

/// The processor's name for Settings, from the three sources:
/// - x86 (`midr` 0): the brand string CPUID gives is exact ("AMD Ryzen 7
///   7840U w/ Radeon 780M Graphics"), so it comes first;
/// - ARM has no brand string: SMBIOS names the product ("Snapdragon(R) X
///   Elite - X1E78100 - Qualcomm(R) Oryon(TM) CPU") when it's a real one, and
///   otherwise (virtual machines put the machine type there, "virt-8.2") the
///   core design from MIDR.
pub fn cpu_name(smbios: &str, cpuid_model: &str, midr: u64) -> String {
    let smbios = tidy(smbios);
    let cpuid_model = tidy(cpuid_model);
    if midr == 0 {
        return if !cpuid_model.is_empty() { cpuid_model } else if !smbios.is_empty() { smbios } else { String::from("Unknown processor") };
    }
    const PRODUCTS: [&str; 14] = ["snapdragon", "qualcomm", "oryon", "kryo", "cortex", "neoverse", "ampere", "altra", "graviton", "mediatek", "dimensity", "tegra", "rockchip", "apple"];
    let l = smbios.to_ascii_lowercase();
    if PRODUCTS.iter().any(|p| l.contains(p)) {
        smbios
    } else if !cpuid_model.is_empty() {
        cpuid_model
    } else {
        String::from("ARM64 processor")
    }
}

/// The processor is a Qualcomm Snapdragon (by SMBIOS name or ARM implementer).
pub fn is_snapdragon(cpu_name: &str, midr: u64) -> bool {
    cpu_name.to_ascii_lowercase().contains("snapdragon") || (midr >> 24) & 0xFF == 0x51
}

/// Tidy a maker's processor name: "Intel(R) Core(TM) i7-1165G7 CPU @ 2.80GHz"
/// becomes "Intel Core i7-1165G7 CPU @ 2.80GHz".
pub fn tidy(name: &str) -> String {
    let mut s = name.replace("(R)", "").replace("(r)", "").replace("(TM)", "").replace("(tm)", "").replace("®", "").replace("™", "");
    while s.contains("  ") {
        s = s.replace("  ", " ");
    }
    s.trim().to_string()
}

/// A graphics device.
#[derive(Clone, Debug, PartialEq)]
pub struct Gpu {
    pub name: String,
    /// PCI vendor:device, or empty for built-in graphics that aren't on PCI
    pub ids: String,
}

/// Everything Settings shows about the hardware.
#[derive(Clone, Debug, Default)]
pub struct Hardware {
    pub arch: &'static str,
    /// "Intel", "AMD", "Qualcomm" (Settings shows the full name instead)
    #[allow(dead_code)]
    pub cpu_vendor: String,
    pub cpu: String,
    /// the core design (ARM), when SMBIOS names the product
    pub core: String,
    pub cores: u32,
    pub threads: u32,
    pub max_mhz: u32,
    pub features: Vec<&'static str>,
    pub snapdragon: bool,
    pub gpus: Vec<Gpu>,
    /// the firmware's screen modes (width, height), and the one in use
    pub modes: Vec<(u32, u32)>,
    pub mode: (u32, u32),
    pub machine: String,
    pub bios: String,
}

#[cfg(target_os = "uefi")]
pub use detect::detect;

#[cfg(target_os = "uefi")]
mod detect {
    use super::*;
    use crate::efi::{self, Guid};

    const SMBIOS3_GUID: Guid = Guid(0xF2FD1544, 0x9794, 0x4A2C, [0x99, 0x2E, 0xE5, 0xBB, 0xCF, 0x20, 0xE3, 0x94]);
    const SMBIOS_GUID: Guid = Guid(0xEB9D2D31, 0x2D88, 0x11D3, [0x9A, 0x16, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D]);
    const MP_GUID: Guid = Guid(0x3FDDA605, 0xA76E, 0x4F46, [0xAD, 0x29, 0x12, 0xF4, 0x53, 0x1B, 0x3D, 0x08]);
    const PCI_IO_GUID: Guid = Guid(0x4CF5B200, 0x68B8, 0x4CA5, [0x9E, 0xEC, 0xB2, 0x3E, 0x3F, 0x50, 0x02, 0x9A]);

    #[repr(C)]
    struct ConfigTable {
        guid: Guid,
        table: *const u8,
    }

    #[repr(C)]
    struct MpServices {
        get_number_of_processors: extern "efiapi" fn(*mut MpServices, *mut usize, *mut usize) -> efi::Status,
    }

    #[repr(C)]
    struct PciAccess {
        read: extern "efiapi" fn(*mut PciIo, u32, u32, usize, *mut u8) -> efi::Status,
        write: usize,
    }

    #[repr(C)]
    struct PciIo {
        poll_mem: usize,
        poll_io: usize,
        mem: [usize; 2],
        io: [usize; 2],
        pci: PciAccess,
        copy_mem: usize,
        map: usize,
        unmap: usize,
        allocate_buffer: usize,
        free_buffer: usize,
        flush: usize,
        get_location: extern "efiapi" fn(*mut PciIo, *mut usize, *mut usize, *mut usize, *mut usize) -> efi::Status,
    }

    fn table(guid: &Guid) -> Option<*const u8> {
        let st = efi::st();
        let tables = st.tables as *const ConfigTable;
        for i in 0..st.n_tables {
            let t = unsafe { &*tables.add(i) };
            if t.guid.0 == guid.0 && t.guid.1 == guid.1 && t.guid.2 == guid.2 && t.guid.3 == guid.3 {
                return Some(t.table);
            }
        }
        None
    }

    fn smbios() -> Smbios {
        unsafe {
            if let Some(ep) = table(&SMBIOS3_GUID) {
                if core::slice::from_raw_parts(ep, 5) == b"_SM3_" {
                    let len = u32::from_le_bytes(core::slice::from_raw_parts(ep.add(0x0C), 4).try_into().unwrap()) as usize;
                    let addr = u64::from_le_bytes(core::slice::from_raw_parts(ep.add(0x10), 8).try_into().unwrap()) as usize;
                    if addr != 0 && len > 0 && len < 1 << 20 {
                        return parse_smbios(core::slice::from_raw_parts(addr as *const u8, len));
                    }
                }
            }
            if let Some(ep) = table(&SMBIOS_GUID) {
                if core::slice::from_raw_parts(ep, 4) == b"_SM_" {
                    let len = u16::from_le_bytes([*ep.add(0x16), *ep.add(0x17)]) as usize;
                    let addr = u32::from_le_bytes(core::slice::from_raw_parts(ep.add(0x18), 4).try_into().unwrap()) as usize;
                    if addr != 0 && len > 0 {
                        return parse_smbios(core::slice::from_raw_parts(addr as *const u8, len));
                    }
                }
            }
        }
        Smbios::default()
    }

    fn cores() -> u32 {
        if let Some(mp) = efi::locate::<MpServices>(&MP_GUID) {
            let (mut total, mut enabled) = (0usize, 0usize);
            if unsafe { ((*mp).get_number_of_processors)(mp, &mut total, &mut enabled) } == efi::SUCCESS {
                return enabled.max(1) as u32;
            }
        }
        0
    }

    fn gpus() -> Vec<Gpu> {
        let mut out = Vec::new();
        for h in efi::handles(&PCI_IO_GUID) {
            let Some(p) = efi::handle_protocol::<PciIo>(h, &PCI_IO_GUID) else { continue };
            let (mut id, mut class) = (0u32, 0u32);
            unsafe {
                // width 2: 32-bit reads
                if ((*p).pci.read)(p, 2, 0, 1, &mut id as *mut u32 as *mut u8) != efi::SUCCESS {
                    continue;
                }
                ((*p).pci.read)(p, 2, 8, 1, &mut class as *mut u32 as *mut u8);
            }
            // base class 0x03: display controller
            if class >> 24 != 0x03 {
                continue;
            }
            let (vendor, device) = (id as u16, (id >> 16) as u16);
            out.push(Gpu { name: gpu_name(vendor, device), ids: format!("{:04x}:{:04x}", vendor, device) });
        }
        out
    }

    fn modes() -> (Vec<(u32, u32)>, (u32, u32)) {
        let mut list = Vec::new();
        let mut cur = (0, 0);
        if let Some(gop) = efi::locate::<efi::Gop>(&efi::GOP_GUID) {
            unsafe {
                let mode = &*(*gop).mode;
                cur = ((*mode.info).hres, (*mode.info).vres);
                for m in 0..mode.max_mode {
                    let mut size = 0usize;
                    let mut inf: *const efi::GopModeInfo = core::ptr::null();
                    if ((*gop).query_mode)(gop, m, &mut size, &mut inf) == efi::SUCCESS {
                        let wh = ((*inf).hres, (*inf).vres);
                        if !list.contains(&wh) {
                            list.push(wh);
                        }
                    }
                }
            }
        }
        list.sort_by(|a, b| (b.0 * b.1).cmp(&(a.0 * a.1)));
        (list, cur)
    }

    /// Ask the firmware and the processor.
    pub fn detect() -> Hardware {
        let id = crate::arch::cpu();
        let sm = smbios();
        let cpu = cpu_name(&sm.cpu_name, &id.model, id.midr);
        let snapdragon = is_snapdragon(&cpu, id.midr);
        let mut gpus = gpus();
        if gpus.is_empty() {
            // graphics that aren't on PCI: the SoC's own, through the firmware
            let name = if snapdragon { "Qualcomm Adreno (built into the Snapdragon)" } else { "Built-in graphics (firmware framebuffer)" };
            gpus.push(Gpu { name: String::from(name), ids: String::new() });
        }
        let (modes, mode) = modes();
        let cores = match cores() {
            0 => sm.cores,
            n => n,
        };
        let machine = [sm.maker.as_str(), sm.product.as_str()].iter().filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("to be filled by o.e.m.")).cloned().collect::<Vec<_>>().join(" ");
        let bios = [sm.bios_vendor.as_str(), sm.bios_version.as_str()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" ");
        // the core design says something the SMBIOS name may not (ARM)
        let core = if id.midr != 0 && cpu != id.model { id.model.clone() } else { String::new() };
        let hw = Hardware {
            arch: crate::arch::NAME,
            cpu_vendor: if sm.cpu_maker.is_empty() { id.vendor } else { tidy(&sm.cpu_maker) },
            cpu,
            core,
            cores,
            threads: sm.threads.max(cores),
            max_mhz: sm.max_mhz,
            features: id.features,
            snapdragon,
            gpus,
            modes,
            mode,
            machine,
            bios,
        };
        log!("hw: midr {:#x}, SMBIOS processor \"{}\", machine \"{}\"", id.midr, sm.cpu_name, hw.machine);
        log!("hw: {} {} ({} cores) [{}]; graphics: {}; {} screen modes", hw.arch, hw.cpu, hw.cores, hw.features.join(" "), hw.gpus.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().join(", "), hw.modes.len());
        hw
    }
}
