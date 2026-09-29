//! Wi-Fi through the firmware, where the laptop's firmware has a Wi-Fi
//! driver (the UEFI Wireless MAC Connection II protocol, which Intel's
//! Wi-Fi firmware drivers provide on many laptops): scanning for networks,
//! without blocking (the firmware signals an event when it's done).

use crate::efi::{self, Guid};
use crate::wifi::Security;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;

const WMC2_GUID: Guid = Guid(0x1b0fb9bf, 0x699d, 0x4fdd, [0xa7, 0xc3, 0x25, 0x46, 0x68, 0x1b, 0xf6, 0x3b]);

#[repr(C)]
struct Ssid {
    len: u8,
    ssid: [u8; 32],
}

#[repr(C)]
struct GetNetworksData {
    count: u32,
    list: [Ssid; 1],
}

#[repr(C)]
struct Network {
    bss_type: u32,
    ssid: Ssid,
    akm: *const u8,
    cipher: *const u8,
}

#[repr(C)]
struct NetworkDesc {
    network: Network,
    quality: u8,
}

#[repr(C)]
struct GetNetworksResult {
    count: u8,
    desc: [NetworkDesc; 1],
}

#[repr(C)]
struct GetNetworksToken {
    event: efi::Event,
    status: efi::Status,
    data: *mut GetNetworksData,
    result: *mut GetNetworksResult,
}

#[repr(C)]
struct Wmc2 {
    get_networks: extern "efiapi" fn(*mut Wmc2, *mut GetNetworksToken) -> efi::Status,
    connect_network: usize,
    disconnect_network: usize,
}

/// A network the firmware found.
#[derive(Clone, Debug)]
pub struct Found {
    pub ssid: String,
    pub security: Security,
    /// 0-100
    pub quality: u8,
}

pub struct FirmwareWifi {
    wmc: *mut Wmc2,
    token: Option<&'static mut GetNetworksToken>,
    pub networks: Vec<Found>,
    pub scans: u32,
}

/// The security an AKM suite list says (00-0F-AC:2 PSK, :8 SAE, :1 802.1X).
fn security(akm: *const u8) -> Security {
    if akm.is_null() {
        return Security::Open;
    }
    unsafe {
        let n = u16::from_le_bytes([*akm, *akm.add(1)]) as usize;
        if n == 0 {
            return Security::Open;
        }
        let mut akms = Vec::new();
        for k in 0..n.min(8) {
            let s = akm.add(2 + 4 * k);
            if *s == 0x00 && *s.add(1) == 0x0F && *s.add(2) == 0xAC {
                akms.push(*s.add(3));
            }
        }
        match (akms.iter().any(|a| *a == 2 || *a == 6), akms.contains(&8)) {
            (true, true) => Security::Wpa2Wpa3,
            (false, true) => Security::Wpa3Personal,
            (true, false) => Security::Wpa2Personal,
            _ if akms.contains(&1) => Security::Enterprise,
            _ => Security::Wpa2Personal,
        }
    }
}

impl FirmwareWifi {
    pub fn find() -> Option<FirmwareWifi> {
        let h = efi::handles(&WMC2_GUID).into_iter().next()?;
        let wmc = efi::handle_protocol::<Wmc2>(h, &WMC2_GUID)?;
        log!("wifi: the firmware has a Wi-Fi driver");
        Some(FirmwareWifi { wmc, token: None, networks: Vec::new(), scans: 0 })
    }

    /// Start a scan (all networks) unless one is running.
    pub fn scan(&mut self) {
        if self.token.is_some() {
            return;
        }
        let mut ev: efi::Event = core::ptr::null_mut();
        if (efi::bs().create_event)(0, 0, 0, core::ptr::null_mut::<c_void>(), &mut ev) != efi::SUCCESS {
            return;
        }
        let data = alloc::boxed::Box::leak(alloc::boxed::Box::new(GetNetworksData { count: 0, list: [Ssid { len: 0, ssid: [0; 32] }] }));
        let token = alloc::boxed::Box::leak(alloc::boxed::Box::new(GetNetworksToken { event: ev, status: 0, data, result: core::ptr::null_mut() }));
        if unsafe { ((*self.wmc).get_networks)(self.wmc, token) } == efi::SUCCESS {
            self.token = Some(token);
        }
    }

    /// Collect a finished scan. True when the list changed.
    pub fn poll(&mut self) -> bool {
        let Some(t) = self.token.as_ref() else { return false };
        if (efi::bs().check_event)(t.event) != efi::SUCCESS {
            return false;
        }
        let t = self.token.take().unwrap();
        self.scans += 1;
        if t.status != efi::SUCCESS || t.result.is_null() {
            return false;
        }
        let mut out = Vec::new();
        unsafe {
            let r = &*t.result;
            let first = r.desc.as_ptr();
            for k in 0..r.count as usize {
                let d = &*first.add(k);
                let n = (d.network.ssid.len as usize).min(32);
                let ssid = String::from_utf8_lossy(&d.network.ssid.ssid[..n]).into();
                out.push(Found { ssid, security: security(d.network.akm), quality: d.quality.min(100) });
            }
            (efi::bs().free_pool)(t.result as *mut u8);
        }
        out.sort_by(|a, b| b.quality.cmp(&a.quality));
        out.dedup_by(|a, b| a.ssid == b.ssid);
        self.networks = out;
        true
    }
}
