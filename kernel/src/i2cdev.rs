//! Starting the I2C touchpads, touch screens, pens and sensors ACPI lists.
//!
//! For each HID over I2C device (acpi::find_i2c_hid): find its controller's
//! registers (from ACPI, or the PCI device's first BAR for Intel's LPSS
//! controllers, which also have to be taken out of reset), set the bus up
//! with the firmware's timing (FMCN), start the device (i2c::I2cHid) and
//! read what it reports through the same HID code as USB (hidin.rs).
//!
//! HydatekOS polls the devices each frame instead of waiting for their GPIO
//! interrupt (there's no GPIO driver yet).

use crate::acpi::{self, I2cCtl, I2cHidDev, KernelHost};
use crate::aml::Aml;
use crate::hidin::{Event, HidInput};
use crate::i2c::{self, Bus, DesignWare, I2cHid, Mmio};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// An I2C device ACPI describes, and how starting it went.
#[derive(Clone, Debug)]
pub struct Found {
    pub name: String,
    pub kind: String,
    pub status: &'static str,
}

struct Dev {
    ctl: usize,
    hid: I2cHid,
    input: HidInput,
}

pub struct I2cInput {
    ctls: Vec<DesignWare<Mmio>>,
    devs: Vec<Dev>,
    pub found: Vec<Found>,
}

/// PCI configuration space through ECAM (bus 0: Intel's LPSS controllers).
fn cfg(ecam: u64, dev: u8, func: u8, off: u64) -> *mut u32 {
    (ecam + ((dev as u64) << 15 | (func as u64) << 12 | off)) as usize as *mut u32
}

/// The controller's registers: from ACPI, or a PCI device's BAR 0 (switched
/// on, powered up and out of reset).
fn registers(ctl: &I2cCtl, ecam: u64) -> Option<u64> {
    if let Some(m) = ctl.mem {
        return Some(m);
    }
    let (dev, func) = ctl.pci?;
    if ecam == 0 {
        return None;
    }
    unsafe {
        let id = cfg(ecam, dev, func, 0).read_volatile();
        if id == 0xFFFF_FFFF || id == 0 {
            return None;
        }
        // power state D0 (the PCI power management capability)
        let mut cap = (cfg(ecam, dev, func, 0x34).read_volatile() & 0xFC) as u64;
        for _ in 0..16 {
            if cap == 0 {
                break;
            }
            let hdr = cfg(ecam, dev, func, cap).read_volatile();
            if hdr & 0xFF == 1 {
                let pm = cfg(ecam, dev, func, cap + 4);
                pm.write_volatile(pm.read_volatile() & !3);
            }
            cap = ((hdr >> 8) & 0xFC) as u64;
        }
        let bar = cfg(ecam, dev, func, 0x10).read_volatile();
        let mut base = (bar & !0xF) as u64;
        if bar & 6 == 4 {
            base |= (cfg(ecam, dev, func, 0x14).read_volatile() as u64) << 32;
        }
        if base == 0 {
            return None;
        }
        // memory decoding and bus mastering on
        let cmd = cfg(ecam, dev, func, 4);
        cmd.write_volatile(cmd.read_volatile() | 6);
        // Intel LPSS: the private registers take the controller out of reset
        let resets = (base + 0x204) as usize as *mut u32;
        resets.write_volatile(7);
        Some(base)
    }
}

impl I2cInput {
    pub fn start(a: &mut Aml<KernelHost>) -> I2cInput {
        let mut me = I2cInput { ctls: Vec::new(), devs: Vec::new(), found: Vec::new() };
        let mut ctl_paths: Vec<String> = Vec::new();
        for d in acpi::find_i2c_hid(a) {
            log!("i2c: {}", acpi::describe(&d));
            let status = me.start_one(a, &d, &mut ctl_paths);
            log!("i2c: {} -> {}", d.hid, status);
            me.found.push(Found { name: format!("{} at {:#04x}", d.hid, d.addr), kind: String::from(me.devs.last().filter(|_| status == "HydatekOS I2C HID").map_or("HID over I2C device", |x| x.input.what())), status });
        }
        me
    }

    fn start_one(&mut self, a: &mut Aml<KernelHost>, d: &I2cHidDev, paths: &mut Vec<String>) -> &'static str {
        let ctl = match paths.iter().position(|p| *p == d.bus) {
            Some(i) => i,
            None => {
                let c = acpi::i2c_controller(a, &d.bus);
                if !c.designware {
                    return "No driver for its I2C controller";
                }
                let Some(base) = registers(&c, a.host.ecam) else { return "Controller not found" };
                let Some(mut dw) = DesignWare::new(Mmio(base as usize), 133) else { return "Controller not responding" };
                if let Some((h, l, hold)) = c.fmcn {
                    dw.set_counts(h, l, hold);
                }
                log!("i2c: DesignWare controller {} at {:#x}", d.bus, base);
                self.ctls.push(dw);
                paths.push(d.bus.clone());
                self.ctls.len() - 1
            }
        };
        let bus: &mut dyn Bus = &mut self.ctls[ctl];
        let Some(hid) = I2cHid::start(bus, d.addr, d.desc_reg) else { return "Device not answering" };
        let Some(rd) = hid.report_descriptor(bus) else { return "Device not answering" };
        let mut input = HidInput::new(&rd, false);
        if !input.useful() {
            return "Not a device HydatekOS uses";
        }
        for (rid, r) in input.start_reports() {
            hid.set_report(bus, i2c::FEATURE, rid, &r);
        }
        if let Some((rid, len)) = input.waveform_request() {
            if let Some(r) = hid.get_report(bus, i2c::FEATURE, rid, len) {
                input.set_waveforms(&r);
            }
        }
        log!("i2c: driving {:04x}:{:04x} as {}", hid.desc.vendor, hid.desc.product, input.what());
        self.devs.push(Dev { ctl, hid, input });
        "HydatekOS I2C HID"
    }

    pub fn driven(&self) -> usize {
        self.devs.len()
    }

    pub fn haptic_pads(&self) -> usize {
        self.devs.iter().filter(|d| d.input.has_haptics()).count()
    }

    /// Read every device's waiting reports. `now` in ms.
    pub fn poll(&mut self, now: u64, out: &mut Vec<Event>) {
        for d in self.devs.iter_mut() {
            let bus: &mut dyn Bus = &mut self.ctls[d.ctl];
            for _ in 0..8 {
                match d.hid.read_input(bus) {
                    Some(r) => d.input.report(&r, now, out),
                    None => break,
                }
            }
        }
    }

    /// A waveform on every haptic touchpad.
    pub fn play(&mut self, wave: u16, strength: u32, repeat: u32, period: u32) {
        for d in self.devs.iter_mut() {
            if let Some(r) = d.input.haptic_report(wave, strength, repeat, period) {
                let bus: &mut dyn Bus = &mut self.ctls[d.ctl];
                d.hid.output(bus, &r);
            }
        }
    }
}
