//! Keyboard and pointer input.
//!
//! Milestone 1 reads devices through the firmware's input protocols, which
//! means USB/PS2 keyboards, mice, touchpads and tablets all work on real
//! hardware without HydatekOS drivers. Native drivers come in milestone 2.

use crate::efi::{self, AbsolutePointer, SimplePointer, SimpleTextInput, SimpleTextInputEx};
use crate::ui::{Key, Media};
use alloc::vec::Vec;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Modifiers held for the most recent key (only firmware with the extended
/// text input protocol reports them).
static MODS: AtomicU32 = AtomicU32::new(0);
const M_SHIFT: u32 = 1;
const M_CTRL: u32 = 2;
/// Alt, or ⌥ Option on an Apple keyboard: HydatekOS's Aux key
const M_AUX: u32 = 4;
const M_LOGO: u32 = 8;
/// Ctrl works as the Gen key (a per-account setting; on by default).
static CTRL_IS_GEN: AtomicBool = AtomicBool::new(true);
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

/// Ctrl was held for the most recent key.
pub fn ctrl() -> bool {
    held(M_CTRL)
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

/// The logo key (⊞ Windows, ⌘ Command) was held for the most recent key.
pub fn logo() -> bool {
    held(M_LOGO)
}

pub fn set_ctrl_is_gen(on: bool) {
    CTRL_IS_GEN.store(on, Ordering::Relaxed);
}

pub fn ctrl_is_gen() -> bool {
    CTRL_IS_GEN.load(Ordering::Relaxed)
}

pub enum Ev {
    Move,
    Down,
    Up,
    RightDown,
    Scroll(i32),
    Key(Key, bool),
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
        log!("pointers: {} (firmware abs {}, simple {})", ptrs.len(), abs, simple);
        let kbd_ex = efi::handle_protocol::<SimpleTextInputEx>(st.console_in_handle, &efi::TEXT_INPUT_EX_GUID);
        Input { ptrs, kbd_ex, kbd: st.con_in, x: w / 2, y: h / 2, w, h, left: false, right: false, acc: (0, 0), scroll_acc: 0 }
    }

    pub fn pointer_count(&self) -> usize {
        self.ptrs.len()
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
        for _ in 0..16 {
            let (key, shift) = match self.kbd_ex {
                Some(k) => {
                    let mut d = efi::KeyData::default();
                    if unsafe { ((*k).read_key_stroke_ex)(k, &mut d) } != efi::SUCCESS {
                        break;
                    }
                    if d.state.toggle_state & efi::TOGGLE_STATE_VALID != 0 {
                        TOGGLES.store(d.state.toggle_state as u32, Ordering::Relaxed);
                    }
                    (d.key, d.state.shift_state)
                }
                None => {
                    let mut d = efi::InputKey::default();
                    if unsafe { ((*self.kbd).read_key_stroke)(self.kbd, &mut d) } != efi::SUCCESS {
                        break;
                    }
                    (d, 0)
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
            // Ctrl+letter arrives as a control character even without the
            // extended protocol
            let Some((k, ctrl_char)) = map_key(key, mods & M_CTRL != 0) else { continue };
            if ctrl_char {
                mods |= M_CTRL;
            }
            MODS.store(mods, Ordering::Relaxed);
            out.push(gen_key(k, mods));
        }
    }
}

/// The event for a key and the modifiers held (see keymap.rs): the Gen key
/// is the logo key, or Ctrl while Ctrl works as Gen. Otherwise Ctrl+letter is
/// `Key::Ctrl`, so it never types the letter. Aux+key types a special
/// character, or is `Key::Aux`; Aux+← and Aux+→ move by word, as Gen+← does.
fn gen_key(k: Key, mods: u32) -> Ev {
    let ctrl = mods & M_CTRL != 0;
    let aux = mods & M_AUX != 0;
    let gen = mods & M_LOGO != 0 || (ctrl && ctrl_is_gen());
    match k {
        // Gen+Shift+Z and Gen+z are the same shortcut
        Key::Char(c) if gen => Ev::Key(Key::Char(c.to_ascii_lowercase()), true),
        Key::Char(c) if ctrl && c.is_ascii_alphabetic() => Ev::Key(Key::Ctrl(c.to_ascii_lowercase()), false),
        Key::Char(c) if aux => match crate::keymap::aux_char(c) {
            Some(special) => Ev::Key(Key::Char(special), false),
            None => Ev::Key(Key::Aux(c.to_ascii_lowercase()), false),
        },
        Key::Left | Key::Right if aux => Ev::Key(k, true),
        // the PC's old editing keys: Shift+Insert pastes, Gen+Insert copies,
        // Shift+Delete cuts
        Key::Insert if mods & M_SHIFT != 0 => Ev::Key(Key::Char('v'), true),
        Key::Insert if gen || ctrl => Ev::Key(Key::Char('c'), true),
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
