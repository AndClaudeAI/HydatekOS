//! Firmware network card access through the UEFI Simple Network Protocol.
//!
//! Like the framebuffer, milestone 1 borrows the NIC driver from the firmware
//! (most PC firmware ships one for PXE boot). Everything above raw Ethernet
//! frames — ARP, IP, DHCP, TCP, mDNS, HTTP, the Link protocol — is HydatekOS.

use crate::efi::{self, Handle, Status};
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr::null_mut;

#[repr(C)]
pub struct Snp {
    pub revision: u64,
    pub start: extern "efiapi" fn(*mut Snp) -> Status,
    pub stop: extern "efiapi" fn(*mut Snp) -> Status,
    pub initialize: extern "efiapi" fn(*mut Snp, usize, usize) -> Status,
    pub reset: extern "efiapi" fn(*mut Snp, bool) -> Status,
    pub shutdown: extern "efiapi" fn(*mut Snp) -> Status,
    pub receive_filters: extern "efiapi" fn(*mut Snp, u32, u32, bool, usize, *const [u8; 32]) -> Status,
    pub station_address: usize,
    pub statistics: usize,
    pub mcast_ip_to_mac: usize,
    pub nvdata: usize,
    pub get_status: extern "efiapi" fn(*mut Snp, *mut u32, *mut *mut c_void) -> Status,
    pub transmit: extern "efiapi" fn(*mut Snp, usize, usize, *const u8, *const [u8; 32], *const [u8; 32], *const u16) -> Status,
    pub receive: extern "efiapi" fn(*mut Snp, *mut usize, *mut usize, *mut u8, *mut [u8; 32], *mut [u8; 32], *mut u16) -> Status,
    pub wait_for_packet: efi::Event,
    pub mode: *const u8,
}

// Offsets into EFI_SIMPLE_NETWORK_MODE: ten u32 fields, then
// MCastFilter[16] of 32-byte EFI_MAC_ADDRESS, then the station addresses.
const M_STATE: usize = 0;
const M_FILTER_MASK: usize = 24;
const M_CURRENT_ADDR: usize = 40 + 16 * 32;
const M_MEDIA_PRESENT_SUPPORTED: usize = M_CURRENT_ADDR + 3 * 32 + 3;
const M_MEDIA_PRESENT: usize = M_CURRENT_ADDR + 3 * 32 + 4;

const FILTER_UNICAST: u32 = 0x01;
const FILTER_MULTICAST: u32 = 0x02;
const FILTER_BROADCAST: u32 = 0x04;
const FILTER_PROMISC: u32 = 0x08;
const FILTER_PROMISC_MCAST: u32 = 0x10;

const TX_SLOTS: usize = 64;

pub struct FirmwareNic {
    snp: *mut Snp,
    pub mac: [u8; 6],
    tx: Vec<[u8; 1536]>,
    tx_next: usize,
    pub name: String,
}

impl FirmwareNic {
    /// Find, claim and start the first Ethernet interface the firmware offers.
    pub fn open() -> Option<FirmwareNic> {
        let handles = efi::handles(&efi::SNP_GUID);
        log!("net: {} firmware network interface(s)", handles.len());
        for h in handles {
            if let Some(n) = Self::try_open(h) {
                return Some(n);
            }
        }
        None
    }

    fn try_open(h: Handle) -> Option<FirmwareNic> {
        // Claim the card exclusively so the firmware's own IP stack stops polling it.
        let mut p: *mut c_void = null_mut();
        let st = (efi::bs().open_protocol)(h, &efi::SNP_GUID, &mut p, efi::image(), null_mut(), 0x20);
        if st != efi::SUCCESS || p.is_null() {
            log!("net: exclusive open failed ({:x}); sharing the interface", st);
            p = efi::handle_protocol::<c_void>(h, &efi::SNP_GUID)?;
        }
        let snp = p as *mut Snp;
        unsafe {
            let state = || *((*snp).mode.add(M_STATE) as *const u32);
            if state() == 0 {
                ((*snp).start)(snp);
            }
            if state() == 1 {
                ((*snp).initialize)(snp, 0, 0);
            }
            if state() != 2 {
                log!("net: interface failed to initialise (state {})", state());
                return None;
            }
            let mask = *((*snp).mode.add(M_FILTER_MASK) as *const u32);
            let mut want = FILTER_UNICAST | FILTER_BROADCAST;
            want |= if mask & FILTER_PROMISC_MCAST != 0 { FILTER_PROMISC_MCAST } else if mask & FILTER_MULTICAST != 0 { 0 } else { FILTER_PROMISC };
            ((*snp).receive_filters)(snp, want & mask, 0, false, 0, core::ptr::null());
            if want & mask & FILTER_PROMISC_MCAST == 0 && mask & FILTER_MULTICAST != 0 {
                // mDNS group 224.0.0.251
                let mut m = [0u8; 32];
                m[..6].copy_from_slice(&[0x01, 0x00, 0x5e, 0x00, 0x00, 0xfb]);
                ((*snp).receive_filters)(snp, (want | FILTER_MULTICAST) & mask, 0, false, 1, &m);
            }
            let mut mac = [0u8; 6];
            core::ptr::copy_nonoverlapping((*snp).mode.add(M_CURRENT_ADDR), mac.as_mut_ptr(), 6);
            let name = alloc::format!("Ethernet ({:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
            log!("net: using {}", name);
            Some(FirmwareNic { snp, mac, tx: alloc::vec![[0u8; 1536]; TX_SLOTS], tx_next: 0, name })
        }
    }

    pub fn wait_event(&self) -> efi::Event {
        unsafe { (*self.snp).wait_for_packet }
    }

    pub fn link_up(&self) -> bool {
        unsafe {
            let supported = *(*self.snp).mode.add(M_MEDIA_PRESENT_SUPPORTED) != 0;
            !supported || *(*self.snp).mode.add(M_MEDIA_PRESENT) != 0
        }
    }

    pub fn send(&mut self, frame: &[u8]) -> bool {
        if frame.len() > 1514 {
            return false;
        }
        unsafe {
            // Reclaim finished transmit buffers.
            for _ in 0..TX_SLOTS {
                let mut done: *mut c_void = null_mut();
                if ((*self.snp).get_status)(self.snp, null_mut(), &mut done) != efi::SUCCESS || done.is_null() {
                    break;
                }
            }
            let slot = &mut self.tx[self.tx_next];
            self.tx_next = (self.tx_next + 1) % TX_SLOTS;
            let len = frame.len().max(60);
            slot[..frame.len()].copy_from_slice(frame);
            slot[frame.len()..len].fill(0);
            let st = ((*self.snp).transmit)(self.snp, 0, len, slot.as_ptr(), core::ptr::null(), core::ptr::null(), core::ptr::null());
            st == efi::SUCCESS
        }
    }

    pub fn recv(&mut self, buf: &mut [u8]) -> Option<usize> {
        let mut len = buf.len();
        let st = unsafe { ((*self.snp).receive)(self.snp, null_mut(), &mut len, buf.as_mut_ptr(), null_mut(), null_mut(), null_mut()) };
        if st == efi::SUCCESS { Some(len) } else { None }
    }
}
