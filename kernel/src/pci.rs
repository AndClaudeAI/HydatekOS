//! PCI devices through the firmware's PCI I/O protocol, and taking one over
//! from the firmware's driver.

use crate::efi::{self, Guid, Handle};
use alloc::vec::Vec;

pub const PCI_IO_GUID: Guid = Guid(0x4CF5B200, 0x68B8, 0x4CA5, [0x9E, 0xEC, 0xB2, 0x3E, 0x3F, 0x50, 0x02, 0x9A]);

#[repr(C)]
pub struct CfgAccess {
    pub read: extern "efiapi" fn(*mut PciIo, u32, u32, usize, *mut u8) -> efi::Status,
    pub write: extern "efiapi" fn(*mut PciIo, u32, u32, usize, *mut u8) -> efi::Status,
}

#[repr(C)]
pub struct PciIo {
    pub poll_mem: usize,
    pub poll_io: usize,
    pub mem: [usize; 2],
    pub io: [usize; 2],
    pub pci: CfgAccess,
    pub copy_mem: usize,
    pub map: usize,
    pub unmap: usize,
    pub allocate_buffer: usize,
    pub free_buffer: usize,
    pub flush: usize,
    pub get_location: extern "efiapi" fn(*mut PciIo, *mut usize, *mut usize, *mut usize, *mut usize) -> efi::Status,
    pub attributes: extern "efiapi" fn(*mut PciIo, u32, u64, *mut u64) -> efi::Status,
}

/// A PCI function.
#[derive(Clone, Copy)]
pub struct Dev {
    pub handle: Handle,
    pub io: *mut PciIo,
    pub vendor: u16,
    pub device: u16,
    /// base class, subclass, programming interface
    pub class: (u8, u8, u8),
    /// bus, device, function
    pub loc: (u8, u8, u8),
}

impl Dev {
    pub fn read32(&self, off: u32) -> u32 {
        let mut v = 0u32;
        unsafe { ((*self.io).pci.read)(self.io, 2, off, 1, &mut v as *mut u32 as *mut u8) };
        v
    }

    pub fn write32(&self, off: u32, mut v: u32) {
        unsafe { ((*self.io).pci.write)(self.io, 2, off, 1, &mut v as *mut u32 as *mut u8) };
    }

    pub fn write16(&self, off: u32, mut v: u16) {
        unsafe { ((*self.io).pci.write)(self.io, 1, off, 1, &mut v as *mut u16 as *mut u8) };
    }

    /// A memory BAR's address (64-bit BARs take two slots).
    pub fn bar(&self, i: u32) -> u64 {
        let lo = self.read32(0x10 + 4 * i);
        if lo & 1 != 0 {
            return 0;
        }
        let mut a = (lo & !0xF) as u64;
        if lo & 6 == 4 {
            a |= (self.read32(0x14 + 4 * i) as u64) << 32;
        }
        a
    }

    /// Memory decoding and bus mastering on (a driver the firmware stopped
    /// turns them off); power state D0.
    pub fn enable(&self) {
        let mut sup = 0u64;
        // EFI_PCI_IO_ATTRIBUTE_MEMORY | IO | BUS_MASTER, operation Enable
        unsafe { ((*self.io).attributes)(self.io, 2, 0x0700, &mut sup) };
        let cmd = self.read32(4) as u16;
        self.write16(4, cmd | 0x0006);
        let mut cap = self.read32(0x34) & 0xFC;
        for _ in 0..16 {
            if cap == 0 {
                break;
            }
            let hdr = self.read32(cap);
            if hdr & 0xFF == 1 {
                let pm = self.read32(cap + 4);
                if pm & 3 != 0 {
                    self.write32(cap + 4, pm & !3);
                }
            }
            cap = (hdr >> 8) & 0xFC;
        }
    }

    pub fn path(&self) -> Vec<u8> {
        efi::device_path(self.handle)
    }

    /// Stop the firmware's drivers for it (and everything behind it).
    pub fn take(&self) -> bool {
        (efi::bs().disconnect_controller)(self.handle, core::ptr::null_mut(), core::ptr::null_mut()) == efi::SUCCESS
    }
}

/// Every PCI function.
pub fn devices() -> Vec<Dev> {
    let mut out = Vec::new();
    for h in efi::handles(&PCI_IO_GUID) {
        let Some(io) = efi::handle_protocol::<PciIo>(h, &PCI_IO_GUID) else { continue };
        let mut d = Dev { handle: h, io, vendor: 0, device: 0, class: (0, 0, 0), loc: (0, 0, 0) };
        let id = d.read32(0);
        if id == 0xFFFF_FFFF {
            continue;
        }
        let class = d.read32(8);
        d.vendor = id as u16;
        d.device = (id >> 16) as u16;
        d.class = ((class >> 24) as u8, (class >> 16) as u8, (class >> 8) as u8);
        let (mut seg, mut bus, mut dev, mut func) = (0usize, 0usize, 0usize, 0usize);
        unsafe { ((*io).get_location)(io, &mut seg, &mut bus, &mut dev, &mut func) };
        d.loc = (bus as u8, dev as u8, func as u8);
        out.push(d);
    }
    out
}

/// A device path begins with another (the device is behind that controller).
pub fn behind(path: &[u8], controller: &[u8]) -> bool {
    !controller.is_empty() && path.len() >= controller.len() && &path[..controller.len()] == controller
}

/// Whether HydatekOS may take a controller from the firmware: not when the
/// boot disk is behind it, and the firmware always keeps a keyboard (one not
/// behind this controller or any already `taken`), so there's one to fall
/// back on.
pub fn may_take(ctl: &[u8], taken: &[Vec<u8>]) -> bool {
    if behind(&efi::device_path(efi::boot_device()), ctl) {
        return false;
    }
    let kbds: Vec<Vec<u8>> = efi::handles(&efi::TEXT_INPUT_EX_GUID).into_iter().map(efi::device_path).filter(|p| !p.is_empty()).collect();
    kbds.is_empty() || kbds.iter().any(|k| !behind(k, ctl) && !taken.iter().any(|t| behind(k, t)))
}
