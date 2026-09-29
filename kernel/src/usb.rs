//! HydatekOS's USB HID driver.
//!
//! The firmware runs the USB host controller (xHCI/EHCI) and enumerates
//! devices; it gives each interface an EFI USB I/O protocol. HydatekOS takes
//! every HID interface no firmware driver has claimed (mice and tablets on ARM
//! machines, touchpads, touch screens, media keys, haptic touchpads) and
//! drives it itself:
//! - reads its HID report descriptor and works out what it is (hid.rs);
//! - switches boot-protocol devices to report protocol and turns off idle
//!   repeats;
//! - listens on its interrupt endpoint with asynchronous transfers, which the
//!   firmware completes in the background into a ring per device;
//! - turns reports into pointer movement, clicks, scrolling, touchpad
//!   gestures (touchpad.rs) and media keys;
//! - drives game controllers: HID gamepads, and Xbox 360 / Xbox One pads
//!   (Microsoft's own protocols, which no firmware driver takes); the D-pad,
//!   stick and buttons navigate the desktop (gamepad.rs);
//! - plays haptic feedback directly on the hardware: waveforms on haptic
//!   touchpads (SET_REPORT output reports), and the rumble motors of Xbox,
//!   DualShock 4 and DualSense controllers (interrupt OUT packets).
//!
//! Keyboards stay with the firmware's keyboard driver, which HydatekOS
//! already reads through the text input protocol.

use crate::efi::{self, Guid, Handle, Status};
use crate::gamepad::{self, Motor, Rumble};
use crate::haptics::{Haptic, Pulse};
use crate::hid;
use crate::hidin::HidInput;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicUsize, Ordering};

const USB_IO_GUID: Guid = Guid(0x2B2F68D6, 0x0CD2, 0x44CF, [0x8E, 0x8B, 0xBB, 0xA2, 0x0B, 0x1B, 0x5B, 0x75]);
const TEXT_INPUT_GUID: Guid = Guid(0x387477C1, 0x69C7, 0x11D2, [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B]);

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct DeviceRequest {
    request_type: u8,
    request: u8,
    value: u16,
    index: u16,
    length: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
pub struct DeviceDescriptor {
    pub length: u8,
    pub kind: u8,
    pub usb: u16,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
    pub max_packet0: u8,
    pub vendor: u16,
    pub product: u16,
    pub release: u16,
    pub i_maker: u8,
    pub i_product: u8,
    pub i_serial: u8,
    pub configs: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct InterfaceDescriptor {
    length: u8,
    kind: u8,
    number: u8,
    alternate: u8,
    endpoints: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
    i_name: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct EndpointDescriptor {
    length: u8,
    kind: u8,
    address: u8,
    attributes: u8,
    max_packet: u16,
    interval: u8,
}

type AsyncCallback = extern "efiapi" fn(*mut c_void, usize, *mut c_void, u32) -> Status;

#[repr(C)]
struct UsbIo {
    control: extern "efiapi" fn(*mut UsbIo, *mut DeviceRequest, u32, u32, *mut c_void, usize, *mut u32) -> Status,
    bulk: usize,
    async_interrupt: extern "efiapi" fn(*mut UsbIo, u8, bool, usize, usize, Option<AsyncCallback>, *mut c_void) -> Status,
    sync_interrupt: extern "efiapi" fn(*mut UsbIo, u8, *mut c_void, *mut usize, usize, *mut u32) -> Status,
    isochronous: usize,
    async_isochronous: usize,
    get_device_descriptor: extern "efiapi" fn(*mut UsbIo, *mut DeviceDescriptor) -> Status,
    get_config_descriptor: usize,
    get_interface_descriptor: extern "efiapi" fn(*mut UsbIo, *mut InterfaceDescriptor) -> Status,
    get_endpoint_descriptor: extern "efiapi" fn(*mut UsbIo, u8, *mut EndpointDescriptor) -> Status,
    get_string_descriptor: extern "efiapi" fn(*mut UsbIo, u16, u8, *mut *mut u16) -> Status,
    get_supported_languages: usize,
    port_reset: usize,
}

const DATA_IN: u32 = 0;
const DATA_OUT: u32 = 1;
const NO_DATA: u32 = 2;

// ---- the report ring the firmware fills -----------------------------------------------

const SLOTS: usize = 32;
const REPORT: usize = 64;

/// Reports from one endpoint: the firmware's completion callback writes,
/// the input loop reads (single producer, single consumer).
struct Pipe {
    buf: UnsafeCell<[[u8; REPORT]; SLOTS]>,
    len: UnsafeCell<[u8; SLOTS]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl Sync for Pipe {}

extern "efiapi" fn on_report(data: *mut c_void, len: usize, ctx: *mut c_void, status: u32) -> Status {
    if status == 0 && len > 0 && !data.is_null() && !ctx.is_null() {
        let p = unsafe { &*(ctx as *const Pipe) };
        let (h, t) = (p.head.load(Ordering::Acquire), p.tail.load(Ordering::Acquire));
        if h.wrapping_sub(t) < SLOTS {
            let n = len.min(REPORT);
            unsafe {
                let slot = &mut (*p.buf.get())[h % SLOTS];
                core::ptr::copy_nonoverlapping(data as *const u8, slot.as_mut_ptr(), n);
                (*p.len.get())[h % SLOTS] = n as u8;
            }
            p.head.store(h.wrapping_add(1), Ordering::Release);
        }
    }
    efi::SUCCESS
}

impl Pipe {
    fn take(&self) -> Option<Vec<u8>> {
        let (h, t) = (self.head.load(Ordering::Acquire), self.tail.load(Ordering::Acquire));
        if h == t {
            return None;
        }
        let r = unsafe {
            let n = (&*self.len.get())[t % SLOTS] as usize;
            (&*self.buf.get())[t % SLOTS][..n].to_vec()
        };
        self.tail.store(t.wrapping_add(1), Ordering::Release);
        Some(r)
    }
}

// ---- devices ------------------------------------------------------------------------

/// A USB device as Settings › Devices lists it.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    /// what it is: "Mouse", "Touchpad", "Keyboard", "Storage"…
    pub kind: String,
    pub ids: String,
    /// who drives it
    pub driver: &'static str,
}

struct Dev {
    handle: Handle,
    io: *mut UsbIo,
    iface: u16,
    /// what its reports mean
    hid: HidInput,
    /// its rumble motors, and the OUT endpoint that reaches them
    rumble: Option<Rumble>,
    motor: Motor,
    out_ep: Option<u8>,
    seq: u8,
    pipe: &'static Pipe,
}

pub use crate::hidin::Event;

/// A Bluetooth adapter: HCI commands go out as control transfers, events
/// come in on the interrupt endpoint (split across USB packets).
pub struct BtDev {
    handle: Handle,
    io: *mut UsbIo,
    iface: u16,
    pipe: &'static Pipe,
    buf: Vec<u8>,
    pub adapter: crate::bt::Adapter,
    scanned: bool,
}

pub struct UsbHid {
    devs: Vec<Dev>,
    /// Bluetooth adapters HydatekOS drives
    pub bts: Vec<BtDev>,
    /// every USB interface seen, for Settings
    pub info: Vec<DeviceInfo>,
    /// counts up whenever `info` changes
    pub generation: u32,
    seen: Vec<Handle>,
    polls: u64,
}

fn string(io: *mut UsbIo, idx: u8) -> String {
    if idx == 0 {
        return String::new();
    }
    let mut p: *mut u16 = core::ptr::null_mut();
    if unsafe { ((*io).get_string_descriptor)(io, 0x0409, idx, &mut p) } != efi::SUCCESS || p.is_null() {
        return String::new();
    }
    let mut v = Vec::new();
    unsafe {
        let mut k = 0;
        while *p.add(k) != 0 && k < 128 {
            v.push(*p.add(k));
            k += 1;
        }
        (efi::bs().free_pool)(p as *mut u8);
    }
    String::from_utf16_lossy(&v).trim().into()
}

fn control(io: *mut UsbIo, rt: u8, req: u8, value: u16, index: u16, dir: u32, data: &mut [u8]) -> bool {
    let mut r = DeviceRequest { request_type: rt, request: req, value, index, length: data.len() as u16 };
    let mut status = 0u32;
    let ptr = if data.is_empty() { core::ptr::null_mut() } else { data.as_mut_ptr() as *mut c_void };
    unsafe { ((*io).control)(io, &mut r, dir, 500, ptr, data.len(), &mut status) == efi::SUCCESS }
}

/// The HID report descriptor's length, from the configuration descriptor.
fn report_descriptor_len(io: *mut UsbIo, iface: u8) -> usize {
    let mut head = [0u8; 9];
    if !control(io, 0x80, 6, 0x0200, 0, DATA_IN, &mut head) {
        return 0;
    }
    let total = (u16::from_le_bytes([head[2], head[3]]) as usize).clamp(9, 4096);
    let mut all = alloc::vec![0u8; total];
    if !control(io, 0x80, 6, 0x0200, 0, DATA_IN, &mut all) {
        return 0;
    }
    // walk the descriptors: our interface, then its HID descriptor (0x21)
    let (mut i, mut ours) = (0, false);
    while i + 2 <= all.len() {
        let (len, kind) = (all[i] as usize, all[i + 1]);
        if len < 2 {
            break;
        }
        if kind == 4 && i + 3 <= all.len() {
            ours = all[i + 2] == iface;
        } else if kind == 0x21 && ours && i + 9 <= all.len() {
            return u16::from_le_bytes([all[i + 7], all[i + 8]]) as usize;
        }
        i += len;
    }
    0
}

/// Start a Bluetooth adapter (class E0, subclass 1, protocol 1).
fn attach_bt(h: Handle, io: *mut UsbIo) -> Option<BtDev> {
    let mut id = InterfaceDescriptor::default();
    if unsafe { ((*io).get_interface_descriptor)(io, &mut id) } != efi::SUCCESS || (id.class, id.subclass, id.protocol) != (0xE0, 1, 1) || id.number != 0 {
        return None;
    }
    let mut ep = None;
    for i in 0..id.endpoints {
        let mut e = EndpointDescriptor::default();
        if unsafe { ((*io).get_endpoint_descriptor)(io, i, &mut e) } == efi::SUCCESS && e.attributes & 3 == 3 && e.address & 0x80 != 0 {
            ep = Some(e);
        }
    }
    let ep = ep?;
    let pipe: &'static Pipe = Box::leak(Box::new(Pipe { buf: UnsafeCell::new([[0; REPORT]; SLOTS]), len: UnsafeCell::new([0; SLOTS]), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }));
    let size = (ep.max_packet & 0x7FF).clamp(1, REPORT as u16) as usize;
    let st = unsafe { ((*io).async_interrupt)(io, ep.address, true, ep.interval.clamp(1, 16) as usize, size, Some(on_report), pipe as *const Pipe as *mut c_void) };
    if st != efi::SUCCESS {
        return None;
    }
    log!("bt: driving a Bluetooth adapter (interface {})", id.number);
    Some(BtDev { handle: h, io, iface: id.number as u16, pipe, buf: Vec::new(), adapter: crate::bt::Adapter::new(), scanned: false })
}

impl BtDev {
    fn poll(&mut self) {
        // events arrive in pieces the size of the endpoint's packets
        while let Some(r) = self.pipe.take() {
            self.buf.extend_from_slice(&r);
            while self.buf.len() >= 2 && self.buf.len() >= 2 + self.buf[1] as usize {
                let n = 2 + self.buf[1] as usize;
                let ev: Vec<u8> = self.buf.drain(..n).collect();
                self.adapter.event(&ev);
            }
            if self.buf.len() > 1024 {
                self.buf.clear();
            }
        }
        if self.adapter.ready && !self.scanned {
            self.scanned = true;
            log!("bt: adapter {} ({}, {}) ready; scanning", self.adapter.address(), self.adapter.name, crate::bt::version_name(self.adapter.version));
            self.adapter.scan();
        }
        if let Some(mut c) = self.adapter.next() {
            control(self.io, 0x20, 0, 0, self.iface, DATA_OUT, &mut c);
        }
    }

    pub fn rescan(&mut self) {
        self.adapter.scan();
    }
}

/// The USB class of an interface, in words.
pub fn class_name(class: u8, subclass: u8, protocol: u8) -> &'static str {
    match (class, subclass, protocol) {
        (1, _, _) => "Audio",
        (2, _, _) | (0x0A, _, _) => "Modem / network",
        (3, 1, 1) => "Keyboard",
        (3, 1, 2) => "Mouse",
        (3, _, _) => "HID device",
        (6, _, _) => "Camera / scanner (still image)",
        (7, _, _) => "Printer",
        (8, _, _) => "Storage",
        (9, _, _) => "Hub",
        (0x0B, _, _) => "Smart card reader",
        (0x0E, _, _) => "Camera",
        (0xE0, 1, 1) => "Bluetooth adapter",
        (0xE0, _, _) => "Wireless adapter",
        (0xEF, 2, 1) => "Composite device",
        (0xFF, _, _) => "Vendor-specific device",
        _ => "USB device",
    }
}

impl UsbHid {
    pub fn new() -> UsbHid {
        let mut u = UsbHid { devs: Vec::new(), bts: Vec::new(), info: Vec::new(), generation: 1, seen: Vec::new(), polls: 0 };
        u.scan();
        u
    }

    /// Take any new USB interfaces (called at start-up and every few seconds:
    /// devices come and go).
    fn scan(&mut self) {
        let handles = efi::handles(&USB_IO_GUID);
        // forget devices that went away
        self.devs.retain(|d| handles.contains(&d.handle));
        self.bts.retain(|d| handles.contains(&d.handle));
        let gone: Vec<Handle> = self.seen.iter().filter(|h| !handles.contains(h)).copied().collect();
        if !gone.is_empty() {
            self.seen.retain(|h| handles.contains(h));
        }
        let mut changed = !gone.is_empty();
        for h in handles {
            if self.seen.contains(&h) {
                continue;
            }
            self.seen.push(h);
            changed = true;
            let Some(io) = efi::handle_protocol::<UsbIo>(h, &USB_IO_GUID) else { continue };
            if let Some(b) = attach_bt(h, io) {
                self.bts.push(b);
            } else if let Some(d) = self.attach(h, io) {
                self.devs.push(d);
            }
        }
        if changed {
            self.inventory();
        }
    }

    /// Everything plugged in, for Settings › Devices.
    fn inventory(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.info.clear();
        for &h in &self.seen {
            let Some(io) = efi::handle_protocol::<UsbIo>(h, &USB_IO_GUID) else { continue };
            let (mut dd, mut id) = (DeviceDescriptor::default(), InterfaceDescriptor::default());
            unsafe {
                ((*io).get_device_descriptor)(io, &mut dd);
                ((*io).get_interface_descriptor)(io, &mut id);
            }
            let product = string(io, dd.i_product);
            let maker = string(io, dd.i_maker);
            let ours = self.devs.iter().find(|d| d.handle == h);
            let kind = match ours {
                Some(d) => String::from(d.hid.what()),
                None => String::from(class_name(id.class, id.subclass, id.protocol)),
            };
            if self.bts.iter().any(|b| b.handle == h) {
                let (v, p) = (dd.vendor, dd.product);
                self.info.push(DeviceInfo { name: if product.is_empty() { String::from("Bluetooth adapter") } else { product }, kind: String::from("Bluetooth adapter"), ids: format!("{:04x}:{:04x}", v, p), driver: "HydatekOS Bluetooth (HCI)" });
                continue;
            }
            let driver = if ours.is_some() {
                "HydatekOS USB HID"
            } else if efi::handle_protocol::<c_void>(h, &TEXT_INPUT_GUID).is_some()
                || efi::handle_protocol::<c_void>(h, &efi::SIMPLE_POINTER_GUID).is_some()
                || efi::handle_protocol::<c_void>(h, &efi::ABSOLUTE_POINTER_GUID).is_some()
                || efi::handle_protocol::<c_void>(h, &efi::SIMPLE_FS_GUID).is_some()
                || id.class == 9
            {
                "Firmware"
            } else {
                "No driver yet"
            };
            let name = match (maker.is_empty(), product.is_empty()) {
                (_, false) if !maker.is_empty() && !product.starts_with(&maker) => format!("{} {}", maker, product),
                (_, false) => product,
                (false, true) => maker,
                _ => kind.clone(),
            };
            let (v, p) = (dd.vendor, dd.product);
            self.info.push(DeviceInfo { name, kind, ids: format!("{:04x}:{:04x}", v, p), driver });
        }
    }

    /// Start driving a HID interface, if it's one HydatekOS takes.
    fn attach(&self, h: Handle, io: *mut UsbIo) -> Option<Dev> {
        let mut id = InterfaceDescriptor::default();
        if unsafe { ((*io).get_interface_descriptor)(io, &mut id) } != efi::SUCCESS {
            return None;
        }
        let mut dd = DeviceDescriptor::default();
        unsafe { ((*io).get_device_descriptor)(io, &mut dd) };
        let (vendor, product) = (dd.vendor, dd.product);
        let iface = id.number as u16;
        // Xbox controllers: vendor class, Microsoft's protocols
        let xbox = match (id.class, id.subclass, id.protocol) {
            (0xFF, 0x5D, 0x01) => Some(false),
            (0xFF, 0x47, 0xD0) => Some(true),
            _ => None,
        };
        if id.class != 3 && xbox.is_none() {
            return None;
        }
        // the firmware's keyboard and pointer drivers keep what they claimed
        if efi::handle_protocol::<c_void>(h, &TEXT_INPUT_GUID).is_some()
            || efi::handle_protocol::<c_void>(h, &efi::SIMPLE_POINTER_GUID).is_some()
            || efi::handle_protocol::<c_void>(h, &efi::ABSOLUTE_POINTER_GUID).is_some()
        {
            return None;
        }
        // boot keyboards too, if the firmware hasn't bound them yet
        if id.class == 3 && id.subclass == 1 && id.protocol == 1 {
            return None;
        }
        // the endpoints: interrupt IN for reports, interrupt OUT for motors
        let (mut ep_in, mut ep_out) = (None, None);
        for i in 0..id.endpoints {
            let mut e = EndpointDescriptor::default();
            if unsafe { ((*io).get_endpoint_descriptor)(io, i, &mut e) } == efi::SUCCESS && e.attributes & 3 == 3 {
                if e.address & 0x80 != 0 {
                    ep_in = ep_in.or(Some(e));
                } else {
                    ep_out = ep_out.or(Some(e.address));
                }
            }
        }
        let ep = ep_in?;
        let mut rumble = None;
        let hid = if let Some(one) = xbox {
            rumble = Some(if one { Rumble::XboxOne } else { Rumble::Xbox360 });
            HidInput::xbox(one)
        } else {
            let len = match report_descriptor_len(io, id.number) {
                0 => 512,
                n => n.min(4096),
            };
            let mut rd = alloc::vec![0u8; len];
            if !control(io, 0x81, 6, 0x2200, iface, DATA_IN, &mut rd) {
                return None;
            }
            let sony = vendor == 0x054C;
            let mut hid = HidInput::new(&rd, sony);
            if sony {
                rumble = match product {
                    0x05C4 | 0x09CC | 0x0BA0 => Some(Rumble::DualShock4),
                    0x0CE6 | 0x0DF2 => Some(Rumble::DualSense),
                    _ => None,
                };
            }
            if !hid.useful() {
                log!("usb: HID interface {} ({}) not used", iface, hid.what());
                return None;
            }
            // report protocol (not the boot protocol), and no idle repeats
            if id.subclass == 1 {
                control(io, 0x21, 0x0B, 1, iface, NO_DATA, &mut []);
            }
            control(io, 0x21, 0x0A, 0, iface, NO_DATA, &mut []);
            // touchpads report fingers, sensors report, when told to
            for (rid, mut r) in hid.start_reports() {
                control(io, 0x21, 0x09, 0x0300 | rid as u16, iface, DATA_OUT, &mut r);
            }
            // a haptic touchpad's waveforms
            if let Some((rid, len)) = hid.waveform_request() {
                let mut r = alloc::vec![0u8; len];
                if control(io, 0xA1, 0x01, 0x0300 | rid as u16, iface, DATA_IN, &mut r) {
                    hid.set_waveforms(&r);
                }
            }
            hid
        };
        let mut dev = Dev {
            handle: h,
            io,
            iface,
            hid,
            rumble,
            motor: Motor::default(),
            out_ep: ep_out,
            seq: 0,
            pipe: Box::leak(Box::new(Pipe { buf: UnsafeCell::new([[0; REPORT]; SLOTS]), len: UnsafeCell::new([0; SLOTS]), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) })),
        };
        let size = (ep.max_packet & 0x7FF).clamp(1, REPORT as u16) as usize;
        let st = unsafe { ((*io).async_interrupt)(io, ep.address, true, ep.interval.clamp(1, 32) as usize, size, Some(on_report), dev.pipe as *const Pipe as *mut c_void) };
        if st != efi::SUCCESS {
            log!("usb: interface {} wouldn't start ({:#x})", iface, st);
            return None;
        }
        if dev.rumble == Some(Rumble::XboxOne) {
            // it stays quiet until told to start
            let mut m = gamepad::xbox_one_start(0);
            dev.seq = 1;
            dev.send(&mut m);
        }
        let extra = if dev.hid.has_haptics() {
            " with haptics"
        } else if dev.rumble.is_some() {
            " with rumble motors"
        } else {
            ""
        };
        log!("usb: driving {:04x}:{:04x} interface {} as {}{}", vendor, product, iface, dev.hid.what(), extra);
        Some(dev)
    }

    /// Read what the devices sent since last time, and run the motors.
    /// `now` in ms.
    pub fn poll(&mut self, now: u64, out: &mut Vec<Event>) {
        self.polls += 1;
        if self.polls % 200 == 0 {
            self.scan();
        }
        for b in self.bts.iter_mut() {
            b.poll();
        }
        for d in self.devs.iter_mut() {
            while let Some(r) = d.pipe.take() {
                d.hid.report(&r, now, out);
            }
            if let Some(a) = d.motor.due(now) {
                d.set_motors(a);
            }
        }
    }

    /// How many interfaces HydatekOS drives.
    pub fn driven(&self) -> usize {
        self.devs.len()
    }

    /// Haptic touchpads HydatekOS can play waveforms on.
    pub fn haptic_pads(&self) -> usize {
        self.devs.iter().filter(|d| d.hid.has_haptics()).count()
    }

    /// Controllers with rumble motors.
    pub fn motors(&self) -> usize {
        self.devs.iter().filter(|d| d.rumble.is_some()).count()
    }

    /// Play feedback on every haptic touchpad (as a waveform) and every
    /// controller's motors (as pulses). `strength` 0-100.
    pub fn feel(&mut self, h: Haptic, pulses: &[Pulse], strength: u32, now: u64) {
        let (wave, repeat, period) = crate::haptics::waveform(h);
        for d in self.devs.iter_mut() {
            if let Some(mut r) = d.hid.haptic_report(wave, strength, repeat, period) {
                let rid = hid::report_id(&r, d.hid.desc.ids) as u16;
                control(d.io, 0x21, 0x09, 0x0200 | rid, d.iface, DATA_OUT, &mut r);
            }
            if d.rumble.is_some() {
                d.motor.play(pulses, now);
                if let Some(a) = d.motor.due(now) {
                    d.set_motors(a);
                }
            }
        }
    }
}

impl Dev {
    /// Send a packet to the device: on its interrupt OUT endpoint, or as a
    /// HID output report.
    fn send(&mut self, data: &mut [u8]) -> bool {
        if let Some(ep) = self.out_ep {
            let (mut len, mut status) = (data.len(), 0u32);
            let st = unsafe { ((*self.io).sync_interrupt)(self.io, ep, data.as_mut_ptr() as *mut c_void, &mut len, 100, &mut status) };
            return st == efi::SUCCESS;
        }
        let rid = data.first().copied().unwrap_or(0) as u16;
        control(self.io, 0x21, 0x09, 0x0200 | rid, self.iface, DATA_OUT, data)
    }

    fn set_motors(&mut self, amp: u8) {
        let Some(kind) = self.rumble else { return };
        let (strong, weak) = gamepad::split(amp);
        let mut p = gamepad::rumble(kind, strong, weak, self.seq);
        self.seq = self.seq.wrapping_add(1);
        self.send(&mut p);
    }
}
