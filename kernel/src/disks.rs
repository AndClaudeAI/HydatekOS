//! Starting the storage HydatekOS drives itself (NVMe and SATA controllers
//! the boot disk isn't on) and describing what's on each disk.

use crate::pci;
use crate::storage::{self, Disk, DiskInfo};
use alloc::format;
use alloc::vec::Vec;

/// A disk made for testing writes carries this in sector 40; only then does
/// HydatekOS write (to sector 41) and read it back at start-up.
const WRITE_TEST: &[u8] = b"HYDATEK WRITE TEST SECTOR!";

fn describe(d: &mut dyn Disk, name: alloc::string::String, kind: &'static str, driver: &'static str) -> DiskInfo {
    let partitions = storage::partitions(d);
    DiskInfo { name, kind, bytes: d.size() * 512, partitions, driver }
}

fn write_test(d: &mut dyn Disk, write: &mut dyn FnMut(u64, &[u8]) -> bool, who: &str) {
    let mut s = [0u8; 512];
    if !d.read(40, &mut s) || &s[..WRITE_TEST.len()] != WRITE_TEST {
        return;
    }
    let mut out = [0u8; 512];
    let msg = format!("Written by HydatekOS's {} driver", who);
    out[..msg.len()].copy_from_slice(msg.as_bytes());
    let ok = write(41, &out) && d.read(41, &mut s) && s == out;
    log!("{}: write test {}", who, if ok { "passed (sector 41 written and read back)" } else { "FAILED" });
}

pub struct Disks {
    pub nvme: Vec<crate::nvme::Nvme>,
    pub sata: Vec<crate::ahci::SataDisk>,
    pub info: Vec<DiskInfo>,
    /// the controllers taken (bus, device, function)
    pub taken: Vec<(u8, u8, u8)>,
}

pub fn start_all() -> Disks {
    let boot = crate::efi::device_path(crate::efi::boot_device());
    let mut all = Disks { nvme: Vec::new(), sata: Vec::new(), info: Vec::new(), taken: Vec::new() };
    for d in pci::devices() {
        let is_nvme = d.class == (0x01, 0x08, 0x02);
        let is_ahci = d.class == (0x01, 0x06, 0x01);
        if !is_nvme && !is_ahci {
            continue;
        }
        if pci::behind(&boot, &d.path()) {
            log!("disks: {:02x}:{:02x}.{} holds the boot disk: the firmware keeps it", d.loc.0, d.loc.1, d.loc.2);
            continue;
        }
        all.taken.push(d.loc);
        if is_nvme {
            match crate::nvme::Nvme::start(&d) {
                Ok(mut n) => {
                    let name = n.model.clone();
                    let mut info = describe(&mut n, name, "NVMe SSD", "HydatekOS NVMe");
                    let p: *mut crate::nvme::Nvme = &mut n;
                    write_test(&mut n, &mut |l, b| unsafe { (*p).write(l, b) }, "nvme");
                    info.partitions = storage::partitions(&mut n);
                    all.info.push(info);
                    all.nvme.push(n);
                }
                Err(e) => log!("nvme: {}", e),
            }
        } else {
            match crate::ahci::start(&d) {
                Ok(v) => {
                    for mut s in v {
                        let name = s.model.clone();
                        let info = describe(&mut s, name, "SATA disk", "HydatekOS AHCI");
                        let p: *mut crate::ahci::SataDisk = &mut s;
                        write_test(&mut s, &mut |l, b| unsafe { (*p).write(l, b) }, "ahci");
                        all.info.push(info);
                        all.sata.push(s);
                    }
                }
                Err(e) => log!("ahci: {}", e),
            }
        }
    }
    for i in &all.info {
        log!("disks: {} ({}), {}: {}", i.name, i.kind, storage::size_text(i.bytes), storage::summary(&i.partitions));
    }
    all
}
