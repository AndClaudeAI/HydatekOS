//! Hand-written UEFI bindings.
//!
//! HydatekOS uses the platform firmware only as a hardware abstraction for the
//! first milestone: it asks for a framebuffer, input devices, a clock and the
//! boot volume, and draws everything else itself. No third-party crates.

#![allow(dead_code)]

use core::ffi::c_void;
use core::ptr::{null, null_mut};

pub type Handle = *mut c_void;
pub type Event = *mut c_void;
pub type Status = usize;

pub const SUCCESS: Status = 0;
const ERR: usize = 1 << 63;
pub const BUFFER_TOO_SMALL: Status = ERR | 5;
pub const NOT_READY: Status = ERR | 6;
pub const NOT_FOUND: Status = ERR | 14;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid(pub u32, pub u16, pub u16, pub [u8; 8]);

pub const GOP_GUID: Guid = Guid(0x9042a9de, 0x23dc, 0x4a38, [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a]);
pub const SIMPLE_POINTER_GUID: Guid = Guid(0x31878c87, 0x0b75, 0x11d5, [0x9a, 0x4f, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);
pub const ABSOLUTE_POINTER_GUID: Guid = Guid(0x8d59d32b, 0xc655, 0x4ae9, [0x9b, 0x15, 0xf2, 0x59, 0x04, 0x99, 0x2a, 0x43]);
pub const LOADED_IMAGE_GUID: Guid = Guid(0x5b1b31a1, 0x9562, 0x11d2, [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
pub const SIMPLE_FS_GUID: Guid = Guid(0x964e5b22, 0x6459, 0x11d2, [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b]);
pub const TEXT_INPUT_EX_GUID: Guid = Guid(0xdd9e7534, 0x7762, 0x4698, [0x8c, 0x14, 0xf5, 0x85, 0x17, 0xa6, 0x25, 0xaa]);

#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct SystemTable {
    pub hdr: TableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: Handle,
    pub con_in: *mut SimpleTextInput,
    pub console_out_handle: Handle,
    pub con_out: *mut c_void,
    pub std_err_handle: Handle,
    pub std_err: *mut c_void,
    pub runtime: *mut RuntimeServices,
    pub boot: *mut BootServices,
    pub n_tables: usize,
    pub tables: *mut c_void,
}

type Fp = usize; // placeholder for services HydatekOS does not call

#[repr(C)]
pub struct BootServices {
    pub hdr: TableHeader,
    pub raise_tpl: Fp,
    pub restore_tpl: Fp,
    pub allocate_pages: extern "efiapi" fn(u32, u32, usize, *mut u64) -> Status,
    pub free_pages: extern "efiapi" fn(u64, usize) -> Status,
    pub get_memory_map: extern "efiapi" fn(*mut usize, *mut u8, *mut usize, *mut usize, *mut u32) -> Status,
    pub allocate_pool: extern "efiapi" fn(u32, usize, *mut *mut u8) -> Status,
    pub free_pool: extern "efiapi" fn(*mut u8) -> Status,
    pub create_event: extern "efiapi" fn(u32, usize, usize, *mut c_void, *mut Event) -> Status,
    pub set_timer: extern "efiapi" fn(Event, u32, u64) -> Status,
    pub wait_for_event: extern "efiapi" fn(usize, *const Event, *mut usize) -> Status,
    pub signal_event: Fp,
    pub close_event: Fp,
    pub check_event: extern "efiapi" fn(Event) -> Status,
    pub install_protocol_interface: Fp,
    pub reinstall_protocol_interface: Fp,
    pub uninstall_protocol_interface: Fp,
    pub handle_protocol: extern "efiapi" fn(Handle, *const Guid, *mut *mut c_void) -> Status,
    pub reserved: Fp,
    pub register_protocol_notify: Fp,
    pub locate_handle: Fp,
    pub locate_device_path: Fp,
    pub install_configuration_table: Fp,
    pub load_image: Fp,
    pub start_image: Fp,
    pub exit: Fp,
    pub unload_image: Fp,
    pub exit_boot_services: Fp,
    pub get_next_monotonic_count: Fp,
    pub stall: extern "efiapi" fn(usize) -> Status,
    pub set_watchdog_timer: extern "efiapi" fn(usize, u64, usize, *const u16) -> Status,
    pub connect_controller: extern "efiapi" fn(Handle, *const Handle, *const c_void, bool) -> Status,
    pub disconnect_controller: Fp,
    pub open_protocol: Fp,
    pub close_protocol: Fp,
    pub open_protocol_information: Fp,
    pub protocols_per_handle: Fp,
    pub locate_handle_buffer: extern "efiapi" fn(u32, *const Guid, *const c_void, *mut usize, *mut *mut Handle) -> Status,
    pub locate_protocol: extern "efiapi" fn(*const Guid, *const c_void, *mut *mut c_void) -> Status,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Time {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub pad1: u8,
    pub nanosecond: u32,
    pub time_zone: i16,
    pub daylight: u8,
    pub pad2: u8,
}

#[repr(C)]
pub struct RuntimeServices {
    pub hdr: TableHeader,
    pub get_time: extern "efiapi" fn(*mut Time, *mut c_void) -> Status,
    pub set_time: Fp,
    pub get_wakeup_time: Fp,
    pub set_wakeup_time: Fp,
    pub set_virtual_address_map: Fp,
    pub convert_pointer: Fp,
    pub get_variable: Fp,
    pub get_next_variable_name: Fp,
    pub set_variable: Fp,
    pub get_next_high_monotonic_count: Fp,
    pub reset_system: extern "efiapi" fn(u32, Status, usize, *const c_void) -> !,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputKey {
    pub scan_code: u16,
    pub unicode_char: u16,
}

#[repr(C)]
pub struct SimpleTextInput {
    pub reset: extern "efiapi" fn(*mut SimpleTextInput, bool) -> Status,
    pub read_key_stroke: extern "efiapi" fn(*mut SimpleTextInput, *mut InputKey) -> Status,
    pub wait_for_key: Event,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct KeyState {
    pub shift_state: u32,
    pub toggle_state: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct KeyData {
    pub key: InputKey,
    pub state: KeyState,
}

#[repr(C)]
pub struct SimpleTextInputEx {
    pub reset: extern "efiapi" fn(*mut SimpleTextInputEx, bool) -> Status,
    pub read_key_stroke_ex: extern "efiapi" fn(*mut SimpleTextInputEx, *mut KeyData) -> Status,
    pub wait_for_key_ex: Event,
    pub set_state: Fp,
    pub register_key_notify: Fp,
    pub unregister_key_notify: Fp,
}

pub const SHIFT_STATE_VALID: u32 = 0x8000_0000;
pub const RIGHT_SHIFT: u32 = 0x1;
pub const LEFT_SHIFT: u32 = 0x2;
pub const RIGHT_CONTROL: u32 = 0x4;
pub const LEFT_CONTROL: u32 = 0x8;

#[repr(C)]
pub struct GopModeInfo {
    pub version: u32,
    pub hres: u32,
    pub vres: u32,
    pub pixel_format: u32,
    pub pixel_mask: [u32; 4],
    pub pixels_per_scanline: u32,
}

#[repr(C)]
pub struct GopMode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *const GopModeInfo,
    pub size_of_info: usize,
    pub fb_base: u64,
    pub fb_size: usize,
}

#[repr(C)]
pub struct Gop {
    pub query_mode: extern "efiapi" fn(*mut Gop, u32, *mut usize, *mut *const GopModeInfo) -> Status,
    pub set_mode: extern "efiapi" fn(*mut Gop, u32) -> Status,
    pub blt: extern "efiapi" fn(*mut Gop, *const u32, u32, usize, usize, usize, usize, usize, usize, usize) -> Status,
    pub mode: *const GopMode,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct PointerState {
    pub rel_x: i32,
    pub rel_y: i32,
    pub rel_z: i32,
    pub left: u8,
    pub right: u8,
}

#[repr(C)]
pub struct PointerMode {
    pub res_x: u64,
    pub res_y: u64,
    pub res_z: u64,
    pub left: u8,
    pub right: u8,
}

#[repr(C)]
pub struct SimplePointer {
    pub reset: extern "efiapi" fn(*mut SimplePointer, bool) -> Status,
    pub get_state: extern "efiapi" fn(*mut SimplePointer, *mut PointerState) -> Status,
    pub wait_for_input: Event,
    pub mode: *const PointerMode,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct AbsState {
    pub x: u64,
    pub y: u64,
    pub z: u64,
    pub buttons: u32,
}

#[repr(C)]
pub struct AbsMode {
    pub min_x: u64,
    pub min_y: u64,
    pub min_z: u64,
    pub max_x: u64,
    pub max_y: u64,
    pub max_z: u64,
    pub attributes: u32,
}

#[repr(C)]
pub struct AbsolutePointer {
    pub reset: extern "efiapi" fn(*mut AbsolutePointer, bool) -> Status,
    pub get_state: extern "efiapi" fn(*mut AbsolutePointer, *mut AbsState) -> Status,
    pub wait_for_input: Event,
    pub mode: *const AbsMode,
}

#[repr(C)]
pub struct LoadedImage {
    pub revision: u32,
    pub parent: Handle,
    pub system_table: *mut SystemTable,
    pub device_handle: Handle,
    pub file_path: *mut c_void,
    pub reserved: *mut c_void,
    pub load_options_size: u32,
    pub load_options: *mut c_void,
    pub image_base: *mut c_void,
    pub image_size: u64,
    pub code_type: u32,
    pub data_type: u32,
    pub unload: Fp,
}

#[repr(C)]
pub struct SimpleFs {
    pub revision: u64,
    pub open_volume: extern "efiapi" fn(*mut SimpleFs, *mut *mut File) -> Status,
}

#[repr(C)]
pub struct File {
    pub revision: u64,
    pub open: extern "efiapi" fn(*mut File, *mut *mut File, *const u16, u64, u64) -> Status,
    pub close: extern "efiapi" fn(*mut File) -> Status,
    pub delete: extern "efiapi" fn(*mut File) -> Status,
    pub read: extern "efiapi" fn(*mut File, *mut usize, *mut u8) -> Status,
    pub write: extern "efiapi" fn(*mut File, *mut usize, *const u8) -> Status,
    pub get_position: Fp,
    pub set_position: extern "efiapi" fn(*mut File, u64) -> Status,
    pub get_info: Fp,
    pub set_info: Fp,
    pub flush: extern "efiapi" fn(*mut File) -> Status,
}

pub const FILE_READ: u64 = 1;
pub const FILE_WRITE: u64 = 2;
pub const FILE_CREATE: u64 = 0x8000_0000_0000_0000;
pub const ATTR_DIRECTORY: u64 = 0x10;

// ---------------------------------------------------------------------------
// Global access. HydatekOS is single-threaded; the firmware tables live for the
// entire run because we never exit boot services in milestone 1.

static mut ST: *mut SystemTable = null_mut();
static mut IMAGE: Handle = null_mut();

pub unsafe fn init(image: Handle, st: *mut SystemTable) {
    IMAGE = image;
    ST = st;
}

pub fn st() -> &'static SystemTable {
    unsafe { &*ST }
}
pub fn bs() -> &'static BootServices {
    unsafe { &*st().boot }
}
pub fn rt() -> &'static RuntimeServices {
    unsafe { &*st().runtime }
}
pub fn image() -> Handle {
    unsafe { IMAGE }
}

pub fn locate<T>(guid: &Guid) -> Option<*mut T> {
    let mut p: *mut c_void = null_mut();
    let s = (bs().locate_protocol)(guid, null(), &mut p);
    if s == SUCCESS && !p.is_null() { Some(p as *mut T) } else { None }
}

pub fn handle_protocol<T>(h: Handle, guid: &Guid) -> Option<*mut T> {
    let mut p: *mut c_void = null_mut();
    let s = (bs().handle_protocol)(h, guid, &mut p);
    if s == SUCCESS && !p.is_null() { Some(p as *mut T) } else { None }
}

/// All handles supporting `guid`.
pub fn handles(guid: &Guid) -> alloc::vec::Vec<Handle> {
    let mut n = 0usize;
    let mut buf: *mut Handle = null_mut();
    let mut v = alloc::vec::Vec::new();
    if (bs().locate_handle_buffer)(2, guid, null(), &mut n, &mut buf) == SUCCESS {
        for i in 0..n {
            v.push(unsafe { *buf.add(i) });
        }
        (bs().free_pool)(buf as *mut u8);
    }
    v
}

pub fn stall_us(us: usize) {
    (bs().stall)(us);
}

pub fn now() -> Time {
    let mut t = Time::default();
    (rt().get_time)(&mut t, null_mut());
    t
}

pub fn reset(kind: u32) -> ! {
    (rt().reset_system)(kind, SUCCESS, 0, null())
}

pub fn firmware_vendor() -> alloc::string::String {
    let mut s = alloc::string::String::new();
    let p = st().firmware_vendor;
    if p.is_null() {
        return s;
    }
    let mut i = 0;
    loop {
        let c = unsafe { *p.add(i) };
        if c == 0 || i > 64 {
            break;
        }
        s.push(char::from_u32(c as u32).unwrap_or('?'));
        i += 1;
    }
    s
}

/// Total conventional memory reported by the firmware, in bytes.
pub fn total_memory() -> u64 {
    let mut size = 0usize;
    let mut key = 0usize;
    let mut dsize = 0usize;
    let mut dver = 0u32;
    (bs().get_memory_map)(&mut size, null_mut(), &mut key, &mut dsize, &mut dver);
    size += 4096;
    let mut buf = alloc::vec![0u8; size];
    if (bs().get_memory_map)(&mut size, buf.as_mut_ptr(), &mut key, &mut dsize, &mut dver) != SUCCESS || dsize == 0 {
        return 0;
    }
    let mut total = 0u64;
    let mut off = 0;
    while off + dsize <= size {
        let ty = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        let pages = u64::from_le_bytes(buf[off + 24..off + 32].try_into().unwrap());
        // Loader/boot code+data and conventional memory are usable RAM.
        if matches!(ty, 1 | 2 | 3 | 4 | 7) {
            total += pages * 4096;
        }
        off += dsize;
    }
    total
}

/// Ask the firmware to bind drivers to every device (like the shell's
/// `connect -r`), so USB mice, touchpads and keyboards become available.
pub fn connect_all() {
    let mut n = 0usize;
    let mut buf: *mut Handle = null_mut();
    // SearchType AllHandles = 0
    if (bs().locate_handle_buffer)(0, null(), null(), &mut n, &mut buf) != SUCCESS {
        return;
    }
    for i in 0..n {
        (bs().connect_controller)(unsafe { *buf.add(i) }, null(), null(), true);
    }
    (bs().free_pool)(buf as *mut u8);
}
