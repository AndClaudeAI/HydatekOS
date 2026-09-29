//! Keyboard and pointer input.
//!
//! Keyboards and the pointers the firmware drives come through its input
//! protocols. HydatekOS drives the rest itself: a PS/2 mouse (ps2.rs) and
//! every USB HID device the firmware leaves alone (usb.rs): mice and tablets
//! on ARM machines, multitouch touchpads with gestures, touch screens, media
//! keys and haptic touchpads.

use crate::efi::{self, AbsolutePointer, SimplePointer, SimpleTextInput, SimpleTextInputEx};
use crate::ui::{Key, Media};
use alloc::vec::Vec;

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Modifiers held for the most recent key (only firmware with the extended
/// text input protocol reports them).
static MODS: AtomicU32 = AtomicU32::new(0);
const M_SHIFT: u32 = 1;
const M_CTRL: u32 = 2;
/// Alt, or ⌥ Option on an Apple keyboard: HydatekOS's Aux key
const M_AUX: u32 = 4;
/// the Hydatek key (where Windows keyboards print ⊞): HydatekOS's system key
const M_LOGO: u32 = 8;
/// Caps, Num and Scroll Lock as the firmware last reported them (efi.rs
/// TOGGLE_* bits; 0 until a key says)
static TOGGLES: AtomicU32 = AtomicU32::new(0);

fn toggle(bit: u8) -> Option<bool> {
    let t = TOGGLES.load(Ordering::Relaxed) as u8;
    (t & efi::TOGGLE_STATE_VALID != 0).then_some(t & bit != 0)
}

/// Caps Lock is on (None: the firmware hasn't said).
pub fn caps_lock() -> Option<bool> {
    toggle(efi::CAPS_LOCK)
}

/// Num Lock is on (None: the firmware hasn't said).
pub fn num_lock() -> Option<bool> {
    toggle(efi::NUM_LOCK)
}

/// Scroll Lock is on (None: the firmware hasn't said).
pub fn scroll_lock() -> Option<bool> {
    toggle(efi::SCROLL_LOCK)
}

/// Every key stroke as the firmware reported it, for the keyboard tester:
/// a ring of the last `RAW_LEN`, each packed as scan code (16 bits), UTF-16
/// character (16), modifiers (8) and lock states (8).
const RAW_LEN: usize = 32;
static RAW: [AtomicU64; RAW_LEN] = [const { AtomicU64::new(0) }; RAW_LEN];
static RAW_COUNT: AtomicU32 = AtomicU32::new(0);

/// A key stroke as the firmware reported it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Raw {
    pub scan: u16,
    pub unicode: u16,
    pub shift: bool,
    pub ctrl: bool,
    pub aux: bool,
    pub logo: bool,
    /// efi.rs TOGGLE_* bits (TOGGLE_STATE_VALID when the firmware said)
    pub toggles: u8,
}

fn record(scan: u16, unicode: u16, mods: u32, toggles: u8) {
    let v = scan as u64 | (unicode as u64) << 16 | ((mods & 0xff) as u64) << 32 | (toggles as u64) << 40;
    let n = RAW_COUNT.load(Ordering::Relaxed);
    RAW[n as usize % RAW_LEN].store(v, Ordering::Relaxed);
    RAW_COUNT.store(n.wrapping_add(1), Ordering::Relaxed);
}

/// How many key strokes have arrived since HydatekOS started.
pub fn raw_count() -> u32 {
    RAW_COUNT.load(Ordering::Relaxed)
}

/// The key strokes after `since` (a `raw_count()`), oldest first; at most the
/// last `RAW_LEN`.
pub fn raw_since(since: u32) -> Vec<Raw> {
    let n = raw_count();
    let from = if n.wrapping_sub(since) as usize > RAW_LEN { n.wrapping_sub(RAW_LEN as u32) } else { since };
    let mut out = Vec::new();
    let mut i = from;
    while i != n {
        let v = RAW[i as usize % RAW_LEN].load(Ordering::Relaxed);
        let m = (v >> 32) as u32 & 0xff;
        out.push(Raw { scan: v as u16, unicode: (v >> 16) as u16, shift: m & M_SHIFT != 0, ctrl: m & M_CTRL != 0, aux: m & M_AUX != 0, logo: m & M_LOGO != 0, toggles: (v >> 40) as u8 });
        i = i.wrapping_add(1);
    }
    out
}

/// The pen, as last reported: pressure (0..=1000), eraser, when (ms).
static PEN: AtomicU32 = AtomicU32::new(0);
static PEN_AT: AtomicU64 = AtomicU64::new(0);

/// A pen in range right now: (pressure 0..=1000, eraser end).
pub fn pen() -> Option<(i32, bool)> {
    let at = PEN_AT.load(Ordering::Relaxed);
    if at == 0 || crate::arch::ms().saturating_sub(at) > 250 {
        return None;
    }
    let v = PEN.load(Ordering::Relaxed);
    Some(((v & 0xFFFF) as i32, v & 0x10000 != 0))
}

fn held(m: u32) -> bool {
    MODS.load(Ordering::Relaxed) & m != 0
}

/// Shift was held for the most recent key (for Shift+arrow selection).
pub fn shift() -> bool {
    held(M_SHIFT)
}

/// Aux (Alt, or ⌥ Option) was held for the most recent key.
pub fn aux() -> bool {
    held(M_AUX)
}

pub enum Ev {
    Move,
    Down,
    Up,
    RightDown,
    Scroll(i32),
    /// a key and whether Gen (Ctrl) is held
    Key(Key, bool),
    /// a key pressed with the Hydatek key held (lower case): a system shortcut
    Hydatek(Key),
    /// the Hydatek key pressed and let go on its own: the start menu
    HydatekTap,
    /// ambient light from a sensor, lux
    Light(u32),
}

enum Ptr {
    Rel(*mut SimplePointer),
    Abs(*mut AbsolutePointer),
    Ps2(crate::ps2::Ps2Mouse),
}

pub struct Input {
    ptrs: Vec<Ptr>,
    kbd_ex: Option<*mut SimpleTextInputEx>,
    kbd: *mut SimpleTextInput,
    pub x: i32,
    pub y: i32,
    w: i32,
    h: i32,
    left: bool,
    right: bool,
    acc: (i32, i32),
    scroll_acc: i32,
    /// the Hydatek key is down: Some(true) once another key was pressed with it
    hydatek: Option<bool>,
    /// the firmware reports which modifiers are held between key strokes
    held_known: bool,
    /// polls since starting, and the last poll a lone Hydatek key stroke came
    /// in (firmware that only reports strokes)
    polls: u64,
    lone: Option<u64>,
    /// HydatekOS's own USB HID driver
    pub usb: crate::usb::UsbHid,
    /// I2C touchpads, touch screens, pens and sensors (i2cdev.rs)
    pub i2c: Option<crate::i2cdev::I2cInput>,
    /// USB controllers HydatekOS drives itself (xhci.rs)
    pub xhci: Vec<crate::xhci::Xhci>,
    usb_evs: Vec<crate::usb::Event>,
    /// keyboards HydatekOS drives itself: Caps Lock, the key repeating
    /// (key, modifiers, when next), the Hydatek key down (Some(true) once
    /// another key was pressed with it)
    caps: bool,
    rep: Option<(Key, u32, u64)>,
    usb_logo: Option<bool>,
}

impl Input {
    pub fn new(w: i32, h: i32) -> Input {
        let st = efi::st();
        let mut ptrs = Vec::new();
        for hnd in efi::handles(&efi::ABSOLUTE_POINTER_GUID) {
            if let Some(p) = efi::handle_protocol::<AbsolutePointer>(hnd, &efi::ABSOLUTE_POINTER_GUID) {
                unsafe { ((*p).reset)(p, true) };
                ptrs.push(Ptr::Abs(p));
            }
        }
        // Prefer the console splitter's aggregate pointer; fall back to every device.
        let mut have_rel = false;
        if let Some(p) = efi::handle_protocol::<SimplePointer>(st.console_in_handle, &efi::SIMPLE_POINTER_GUID) {
            unsafe { ((*p).reset)(p, true) };
            ptrs.push(Ptr::Rel(p));
            have_rel = true;
        }
        if !have_rel {
            for hnd in efi::handles(&efi::SIMPLE_POINTER_GUID) {
                if let Some(p) = efi::handle_protocol::<SimplePointer>(hnd, &efi::SIMPLE_POINTER_GUID) {
                    unsafe { ((*p).reset)(p, true) };
                    ptrs.push(Ptr::Rel(p));
                }
            }
        }
        // Only the console splitter's virtual pointers exist: no firmware mouse
        // driver is bound, so drive a PS/2 mouse ourselves.
        let simple = efi::handles(&efi::SIMPLE_POINTER_GUID).len();
        let abs = efi::handles(&efi::ABSOLUTE_POINTER_GUID).len();
        if simple <= 1 && abs <= 1 {
            if let Some(m) = crate::ps2::Ps2Mouse::init() {
                log!("input: HydatekOS PS/2 mouse driver active");
                ptrs.push(Ptr::Ps2(m));
            }
        }
        let usb = crate::usb::UsbHid::new();
        log!("pointers: {} (firmware abs {}, simple {}), USB HID devices driven by HydatekOS: {}", ptrs.len(), abs, simple, usb.driven());
        let kbd_ex = efi::handle_protocol::<SimpleTextInputEx>(st.console_in_handle, &efi::TEXT_INPUT_EX_GUID);
        if let Some(k) = kbd_ex {
            // ask for strokes of modifiers alone (the Hydatek key tapped on its
            // own), keeping the lock keys as they are
            let mut d = efi::KeyData::default();
            unsafe { ((*k).read_key_stroke_ex)(k, &mut d) };
            let locks = if d.state.toggle_state & efi::TOGGLE_STATE_VALID != 0 { d.state.toggle_state & 0x07 } else { 0 };
            let mut t = efi::TOGGLE_STATE_VALID | efi::KEY_STATE_EXPOSED | locks;
            let st = unsafe { ((*k).set_state)(k, &mut t) };
            log!("input: partial keys {}", if st == efi::SUCCESS { "reported" } else { "not supported" });
        }
        Input { ptrs, kbd_ex, kbd: st.con_in, x: w / 2, y: h / 2, w, h, left: false, right: false, acc: (0, 0), scroll_acc: 0, hydatek: None, held_known: false, polls: 0, lone: None, usb, i2c: None, xhci: Vec::new(), usb_evs: Vec::new(), caps: false, rep: None, usb_logo: None }
    }

    pub fn pointer_count(&self) -> usize {
        self.ptrs.len() + self.usb.driven() + self.i2c.as_ref().map_or(0, |i| i.driven()) + self.xhci.iter().map(|x| x.driven()).sum::<usize>()
    }

    pub fn poll(&mut self, speed: i32, out: &mut Vec<Ev>) {
        let mut moved = false;
        let mut left = self.left;
        let mut right = self.right;
        for p in self.ptrs.iter_mut() {
            match p {
                Ptr::Ps2(m) => {
                    if let Some(pk) = m.poll() {
                        let k = speed.clamp(1, 9);
                        let dx = pk.dx * k / 3;
                        let dy = pk.dy * k / 3;
                        if dx != 0 || dy != 0 {
                            self.x = (self.x + dx).clamp(0, self.w - 1);
                            self.y = (self.y + dy).clamp(0, self.h - 1);
                            moved = true;
                        }
                        self.scroll_acc += pk.dz;
                        left = pk.left;
                        right = pk.right;
                    }
                }
                &mut Ptr::Rel(p) => {
                    let mut s = efi::PointerState::default();
                    if unsafe { ((*p).get_state)(p, &mut s) } != efi::SUCCESS {
                        continue;
                    }
                    let (rx, ry) = unsafe { ((*(*p).mode).res_x.max(1) as i32, (*(*p).mode).res_y.max(1) as i32) };
                    // counts -> pixels with gentle acceleration
                    let k = speed.clamp(1, 9);
                    let accel = |v: i32| if v.abs() > 8 { v * 2 } else { v };
                    self.acc.0 += accel(s.rel_x) * k * 64 / rx.min(64).max(1);
                    self.acc.1 += accel(s.rel_y) * k * 64 / ry.min(64).max(1);
                    let dx = self.acc.0 / 64;
                    let dy = self.acc.1 / 64;
                    self.acc.0 -= dx * 64;
                    self.acc.1 -= dy * 64;
                    if dx != 0 || dy != 0 {
                        self.x = (self.x + dx).clamp(0, self.w - 1);
                        self.y = (self.y + dy).clamp(0, self.h - 1);
                        moved = true;
                    }
                    if s.rel_z != 0 {
                        self.scroll_acc += s.rel_z;
                    }
                    left = s.left != 0;
                    right = s.right != 0;
                }
                &mut Ptr::Abs(p) => {
                    let mut s = efi::AbsState::default();
                    if unsafe { ((*p).get_state)(p, &mut s) } != efi::SUCCESS {
                        continue;
                    }
                    let m = unsafe { &*(*p).mode };
                    let spanx = (m.max_x - m.min_x).max(1);
                    let spany = (m.max_y - m.min_y).max(1);
                    let nx = ((s.x.saturating_sub(m.min_x)) * self.w as u64 / spanx) as i32;
                    let ny = ((s.y.saturating_sub(m.min_y)) * self.h as u64 / spany) as i32;
                    let (nx, ny) = (nx.clamp(0, self.w - 1), ny.clamp(0, self.h - 1));
                    if nx != self.x || ny != self.y {
                        self.x = nx;
                        self.y = ny;
                        moved = true;
                    }
                    left = s.buttons & 1 != 0;
                    right = s.buttons & 2 != 0;
                }
            }
        }
        // HydatekOS's own USB devices
        let mut evs = core::mem::take(&mut self.usb_evs);
        self.usb.poll(crate::arch::ms(), &mut evs);
        if let Some(i) = self.i2c.as_mut() {
            i.poll(crate::arch::ms(), &mut evs);
        }
        for x in self.xhci.iter_mut() {
            x.poll(crate::arch::ms(), &mut evs);
        }
        let mut media = Vec::new();
        let mut keys = Vec::new();
        for e in evs.drain(..) {
            use crate::usb::Event as U;
            match e {
                U::Move(dx, dy) => {
                    let k = speed.clamp(1, 9);
                    self.acc.0 += dx * k * 64 / 3;
                    self.acc.1 += dy * k * 64 / 3;
                    let (px, py) = (self.acc.0 / 64, self.acc.1 / 64);
                    self.acc.0 -= px * 64;
                    self.acc.1 -= py * 64;
                    if px != 0 || py != 0 {
                        self.x = (self.x + px).clamp(0, self.w - 1);
                        self.y = (self.y + py).clamp(0, self.h - 1);
                        moved = true;
                    }
                }
                U::Place(ax, ay) => {
                    let nx = (ax as i64 * self.w as i64 / 32768) as i32;
                    let ny = (ay as i64 * self.h as i64 / 32768) as i32;
                    let (nx, ny) = (nx.clamp(0, self.w - 1), ny.clamp(0, self.h - 1));
                    if nx != self.x || ny != self.y {
                        self.x = nx;
                        self.y = ny;
                        moved = true;
                    }
                }
                U::Buttons(b) => {
                    // each change goes out as it happened (a tap's press and
                    // release can arrive in the same poll)
                    let (l, r) = (b & 1 != 0, b & 2 != 0);
                    if moved && (l != left || r != right) {
                        out.push(Ev::Move);
                        moved = false;
                    }
                    if l != left {
                        left = l;
                        self.left = l;
                        out.push(if l { Ev::Down } else { Ev::Up });
                    }
                    if r != right {
                        right = r;
                        self.right = r;
                        if r {
                            out.push(Ev::RightDown);
                        }
                    }
                }
                U::Scroll(n) => self.scroll_acc += n,
                // its position and tip come as Place and Buttons too
                U::Pen { pressure, eraser, .. } => {
                    PEN.store(pressure.clamp(0, 1000) as u32 | (eraser as u32) << 16, Ordering::Relaxed);
                    PEN_AT.store(crate::arch::ms().max(1), Ordering::Relaxed);
                }
                U::Light(lux) => keys.push(Ev::Light(lux)),
                U::Key { usage, down, mods } => self.usb_key(usage, down, mods, &mut keys),
                U::Nav(n) => {
                    use crate::gamepad::Nav as N;
                    keys.push(match n {
                        N::Up => Ev::Key(Key::Up, false),
                        N::Down => Ev::Key(Key::Down, false),
                        N::Left => Ev::Key(Key::Left, false),
                        N::Right => Ev::Key(Key::Right, false),
                        N::Accept => Ev::Key(Key::Enter, false),
                        N::Back => Ev::Key(Key::Esc, false),
                        N::Next | N::Prev => Ev::Key(Key::Tab, false),
                        N::Menu => Ev::HydatekTap,
                    });
                }
                U::Media(u) => {
                    let m = match u {
                        0xE2 => Some(Media::Mute),
                        0xE9 => Some(Media::VolumeUp),
                        0xEA => Some(Media::VolumeDown),
                        0x6F => Some(Media::BrightnessUp),
                        0x70 => Some(Media::BrightnessDown),
                        0x32 | 0x34 => Some(Media::Sleep),
                        0xB8 => Some(Media::Eject),
                        _ => None,
                    };
                    if let Some(m) = m {
                        media.push(m);
                    }
                }
            }
        }
        self.usb_evs = evs;
        // a held key repeats: after half a second, 30 times a second
        if let Some((k, m, next)) = self.rep {
            let now = crate::arch::ms();
            if now >= next {
                keys.push(gen_key(k, m));
                self.rep = Some((k, m, now + 33));
            }
        }
        for m in media {
            out.push(Ev::Key(Key::Media(m), false));
        }
        out.extend(keys);
        if moved {
            out.push(Ev::Move);
        }
        if left != self.left {
            self.left = left;
            out.push(if left { Ev::Down } else { Ev::Up });
        }
        if right != self.right {
            self.right = right;
            if right {
                out.push(Ev::RightDown);
            }
        }
        if self.scroll_acc != 0 {
            // wheel "down" (towards the user) reports positive z on most firmware
            let z = self.scroll_acc;
            self.scroll_acc = 0;
            out.push(Ev::Scroll(z.signum()));
        }
        // keyboard
        self.polls += 1;
        // is the Hydatek key held? (when the firmware says between strokes)
        let mut logo_held: Option<bool> = None;
        for _ in 0..16 {
            let (key, shift, toggles) = match self.kbd_ex {
                Some(k) => {
                    let mut d = efi::KeyData::default();
                    if unsafe { ((*k).read_key_stroke_ex)(k, &mut d) } != efi::SUCCESS {
                        // UEFI 2.3.1+: the modifiers held come back with NOT_READY
                        if d.state.shift_state & efi::SHIFT_STATE_VALID != 0 {
                            logo_held = Some(d.state.shift_state & (efi::LEFT_LOGO | efi::RIGHT_LOGO) != 0);
                        }
                        break;
                    }
                    if d.state.toggle_state & efi::TOGGLE_STATE_VALID != 0 {
                        TOGGLES.store(d.state.toggle_state as u32, Ordering::Relaxed);
                    }
                    (d.key, d.state.shift_state, d.state.toggle_state)
                }
                None => {
                    let mut d = efi::InputKey::default();
                    if unsafe { ((*self.kbd).read_key_stroke)(self.kbd, &mut d) } != efi::SUCCESS {
                        break;
                    }
                    (d, 0, 0)
                }
            };
            let valid = shift & efi::SHIFT_STATE_VALID != 0;
            let has = |bits: u32| valid && shift & bits != 0;
            let mut mods = 0;
            for (bits, m) in [(efi::LEFT_SHIFT | efi::RIGHT_SHIFT, M_SHIFT), (efi::LEFT_CONTROL | efi::RIGHT_CONTROL, M_CTRL), (efi::LEFT_ALT | efi::RIGHT_ALT, M_AUX), (efi::LEFT_LOGO | efi::RIGHT_LOGO, M_LOGO)] {
                if has(bits) {
                    mods |= m;
                }
            }
            record(key.scan_code, key.unicode_char, mods, toggles);
            if mods & M_LOGO != 0 {
                logo_held = Some(true);
            }
            if key.scan_code == 0 && key.unicode_char == 0 {
                // a modifier alone: note a lone Hydatek key
                if mods & M_LOGO != 0 {
                    self.hydatek.get_or_insert(false);
                    self.lone = Some(self.polls);
                }
                continue;
            }
            // Ctrl+letter arrives as a control character even without the
            // extended protocol
            let Some((k, ctrl_char)) = map_key(key, mods & M_CTRL != 0) else { continue };
            if ctrl_char {
                mods |= M_CTRL;
            }
            MODS.store(mods, Ordering::Relaxed);
            if mods & M_LOGO != 0 {
                self.hydatek = Some(true);
                self.lone = None;
            }
            out.push(gen_key(k, mods));
        }
        // the Hydatek key tapped on its own opens the start menu
        match logo_held {
            Some(true) => {
                self.hydatek.get_or_insert(false);
            }
            Some(false) => {
                if !self.held_known {
                    self.held_known = true;
                    log!("input: firmware reports held modifiers");
                }
                if self.hydatek.take() == Some(false) {
                    out.push(Ev::HydatekTap);
                }
                self.lone = None;
            }
            None => {}
        }
        // firmware that doesn't say when it's let go: a lone stroke with
        // nothing after it for a third of a second
        if !self.held_known {
            if let Some(at) = self.lone {
                if self.polls > at + 30 {
                    self.lone = None;
                    if self.hydatek.take() == Some(false) {
                        out.push(Ev::HydatekTap);
                    }
                }
            }
        }
    }
}

impl Input {
    /// A key from a keyboard HydatekOS drives (USB through its own xHCI
    /// driver, or I2C): HID usages, US layout.
    fn usb_key(&mut self, usage: u8, down: bool, hid_mods: u8, out: &mut Vec<Ev>) {
        static FIRST: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(true);
        if FIRST.swap(false, Ordering::Relaxed) {
            log!("input: typing on a keyboard HydatekOS drives (HID usage {:#04x})", usage);
        }
        let mut mods = 0;
        for (bits, m) in [(0x22u8, M_SHIFT), (0x11, M_CTRL), (0x44, M_AUX), (0x88, M_LOGO)] {
            if hid_mods & bits != 0 {
                mods |= m;
            }
        }
        MODS.store(mods, Ordering::Relaxed);
        // the Hydatek key (GUI): tapped alone, the start menu
        if usage == 0xE3 || usage == 0xE7 {
            if down {
                self.usb_logo = Some(false);
            } else if self.usb_logo.take() == Some(false) {
                out.push(Ev::HydatekTap);
            }
            return;
        }
        if (0xE0..=0xE7).contains(&usage) {
            return;
        }
        if !down {
            if matches!(self.rep, Some(_)) {
                self.rep = None;
            }
            return;
        }
        if usage == 0x39 {
            self.caps = !self.caps;
            TOGGLES.store((efi::TOGGLE_STATE_VALID | if self.caps { efi::CAPS_LOCK } else { 0 }) as u32, Ordering::Relaxed);
            return;
        }
        let shift = mods & M_SHIFT != 0;
        let k = match usage {
            0x28 | 0x58 => Key::Enter,
            0x29 => Key::Esc,
            0x2A => Key::Backspace,
            0x2B => Key::Tab,
            0x3A..=0x45 => Key::F(usage - 0x39),
            0x68..=0x73 => Key::F(usage - 0x68 + 13),
            0x48 => Key::Pause,
            0x49 => Key::Insert,
            0x4A => Key::Home,
            0x4B => Key::PageUp,
            0x4C => Key::Delete,
            0x4D => Key::End,
            0x4E => Key::PageDown,
            0x4F => Key::Right,
            0x50 => Key::Left,
            0x51 => Key::Down,
            0x52 => Key::Up,
            0x7F => Key::Media(Media::Mute),
            0x80 => Key::Media(Media::VolumeUp),
            0x81 => Key::Media(Media::VolumeDown),
            u => match crate::hidin::usage_char(u, shift) {
                Some(c) if c.is_ascii_alphabetic() && self.caps => Key::Char(if shift { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() }),
                Some(c) => Key::Char(c),
                None => return,
            },
        };
        record(0, match k {
            Key::Char(c) => c as u16,
            _ => 0,
        }, mods, TOGGLES.load(Ordering::Relaxed) as u8);
        if mods & M_LOGO != 0 {
            self.usb_logo = Some(true);
        }
        out.push(gen_key(k, mods));
        // letters, digits, arrows and Backspace repeat
        if !matches!(k, Key::Esc | Key::Tab | Key::Enter | Key::Media(_) | Key::F(_)) && mods & (M_LOGO | M_CTRL) == 0 {
            self.rep = Some((k, mods, crate::arch::ms() + 500));
        } else {
            self.rep = None;
        }
    }
}

/// The event for a key and the modifiers held (see keymap.rs): Gen is Ctrl.
/// With the Hydatek key held it's a system shortcut (`Ev::Hydatek`). Aux+key
/// types a special character, or is `Key::Aux`; Aux+← and Aux+→ move by
/// word, as Gen+← does.
fn gen_key(k: Key, mods: u32) -> Ev {
    let gen = mods & M_CTRL != 0;
    let aux = mods & M_AUX != 0;
    if mods & M_LOGO != 0 {
        return Ev::Hydatek(match k {
            Key::Char(c) => Key::Char(c.to_ascii_lowercase()),
            k => k,
        });
    }
    match k {
        // Gen+Shift+Z and Gen+z are the same shortcut
        Key::Char(c) if gen => Ev::Key(Key::Char(c.to_ascii_lowercase()), true),
        Key::Char(c) if aux => match crate::keymap::aux_char(c) {
            Some(special) => Ev::Key(Key::Char(special), false),
            None => Ev::Key(Key::Aux(c.to_ascii_lowercase()), false),
        },
        Key::Left | Key::Right if aux => Ev::Key(k, true),
        // the PC's old editing keys: Shift+Insert pastes, Gen+Insert copies,
        // Shift+Delete cuts
        Key::Insert if mods & M_SHIFT != 0 => Ev::Key(Key::Char('v'), true),
        Key::Insert if gen => Ev::Key(Key::Char('c'), true),
        Key::Delete if mods & M_SHIFT != 0 && !gen => Ev::Key(Key::Char('x'), true),
        k => Ev::Key(k, gen),
    }
}

fn map_key(k: efi::InputKey, ctrl: bool) -> Option<(Key, bool)> {
    let key = match k.scan_code {
        0x01 => Key::Up,
        0x02 => Key::Down,
        0x03 => Key::Right,
        0x04 => Key::Left,
        0x05 => Key::Home,
        0x06 => Key::End,
        0x07 => Key::Insert,
        0x08 => Key::Delete,
        0x09 => Key::PageUp,
        0x0a => Key::PageDown,
        0x0b..=0x14 => Key::F((k.scan_code - 0x0a) as u8),
        0x15 => Key::F(11),
        0x16 => Key::F(12),
        0x17 => Key::Esc,
        0x48 => Key::Pause,
        0x68..=0x73 => Key::F((k.scan_code - 0x68 + 13) as u8),
        0x7f => Key::Media(Media::Mute),
        0x80 => Key::Media(Media::VolumeUp),
        0x81 => Key::Media(Media::VolumeDown),
        0x100 => Key::Media(Media::BrightnessUp),
        0x101 => Key::Media(Media::BrightnessDown),
        0x102 => Key::Media(Media::Sleep),
        0x103 => Key::Media(Media::Hibernate),
        0x104 => Key::Media(Media::Display),
        0x105 => Key::Media(Media::Recovery),
        0x106 => Key::Media(Media::Eject),
        0 => match k.unicode_char {
            0 => return None,
            0x08 => Key::Backspace,
            0x09 => Key::Tab,
            0x0a | 0x0d => Key::Enter,
            0x1b => Key::Esc,
            c @ 1..=26 => return Some((Key::Char((b'a' + c as u8 - 1) as char), true)),
            c => Key::Char(char::from_u32(c as u32)?),
        },
        // shown by the keyboard tester; apps leave it alone
        code => Key::Other(code),
    };
    Some((key, ctrl))
}
