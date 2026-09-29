//! Game controllers: one state for every kind of pad, navigation with the
//! D-pad and stick, and the packets that drive their rumble motors.
//!
//! - HID gamepads (DualShock 4, DualSense, most third-party pads) come in
//!   through hid::Gamepad;
//! - Xbox 360 and Xbox One / Series pads use Microsoft's own USB protocols
//!   (no HID), decoded here;
//! - rumble: Xbox 360, Xbox One (GIP) and DualShock 4 / DualSense output
//!   reports, played from haptics.rs's pulse patterns by `Motor`.
//!
//! Plain logic, host-tested; usb.rs moves the bytes.

use crate::haptics::Pulse;
use crate::hid::PadReport;
use alloc::vec::Vec;

// buttons, in the Xbox layout every pad is mapped to
pub const A: u32 = 1 << 0;
pub const B: u32 = 1 << 1;
pub const X: u32 = 1 << 2;
pub const Y: u32 = 1 << 3;
pub const LB: u32 = 1 << 4;
pub const RB: u32 = 1 << 5;
pub const BACK: u32 = 1 << 6;
pub const START: u32 = 1 << 7;
pub const LS: u32 = 1 << 8;
pub const RS: u32 = 1 << 9;
pub const GUIDE: u32 = 1 << 10;
pub const UP: u32 = 1 << 11;
pub const DOWN: u32 = 1 << 12;
pub const LEFT: u32 = 1 << 13;
pub const RIGHT: u32 = 1 << 14;

/// A controller's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub buttons: u32,
    /// left stick, -1000..=1000 (right and down positive)
    pub lx: i32,
    pub ly: i32,
    /// triggers, 0..=1000
    pub lt: i32,
    pub rt: i32,
}

fn stick(v: i16) -> i32 {
    (v as i32 * 1000 / 32767).clamp(-1000, 1000)
}

fn le16(r: &[u8], i: usize) -> i16 {
    i16::from_le_bytes([r[i], r[i + 1]])
}

fn bits(b: u8, map: &[(u8, u32)]) -> u32 {
    map.iter().filter(|(m, _)| b & m != 0).fold(0, |a, (_, v)| a | v)
}

/// An Xbox 360 pad's input report (type 0, 20 bytes).
pub fn xbox360(r: &[u8]) -> Option<Pad> {
    if r.len() < 14 || r[0] != 0 || r[1] < 14 {
        return None;
    }
    let buttons = bits(r[2], &[(0x01, UP), (0x02, DOWN), (0x04, LEFT), (0x08, RIGHT), (0x10, START), (0x20, BACK), (0x40, LS), (0x80, RS)])
        | bits(r[3], &[(0x01, LB), (0x02, RB), (0x04, GUIDE), (0x10, A), (0x20, B), (0x40, X), (0x80, Y)]);
    // the Y axis counts up
    Some(Pad { buttons, lx: stick(le16(r, 6)), ly: -stick(le16(r, 8)), lt: r[4] as i32 * 1000 / 255, rt: r[5] as i32 * 1000 / 255 })
}

/// An Xbox One / Series pad's input message (GIP type 0x20).
pub fn xbox_one(r: &[u8]) -> Option<Pad> {
    if r.len() < 18 || r[0] != 0x20 {
        return None;
    }
    let buttons = bits(r[4], &[(0x04, START), (0x08, BACK), (0x10, A), (0x20, B), (0x40, X), (0x80, Y)])
        | bits(r[5], &[(0x01, UP), (0x02, DOWN), (0x04, LEFT), (0x08, RIGHT), (0x10, LB), (0x20, RB), (0x40, LS), (0x80, RS)]);
    let trig = |i: usize| (u16::from_le_bytes([r[i], r[i + 1]]) as i32).min(1023) * 1000 / 1023;
    Some(Pad { buttons, lx: stick(le16(r, 10)), ly: -stick(le16(r, 12)), lt: trig(6), rt: trig(8) })
}

/// The Xbox One guide button comes as its own message (GIP type 0x07).
pub fn xbox_one_guide(r: &[u8]) -> Option<bool> {
    (r.len() >= 5 && r[0] == 0x07).then(|| r[4] & 1 != 0)
}

/// A HID gamepad's report, in the Xbox layout. HID pads number their
/// buttons; the common order (DualShock, most generic pads) is
/// X/□ A/✕ B/○ Y/△ LB RB LT RT Back Start LS RS Guide, but many cheap pads
/// begin A B X Y: `sony` picks the first.
pub fn from_hid(r: &PadReport, sony: bool) -> Pad {
    let order: &[u32] = if sony { &[X, A, B, Y, LB, RB, 0, 0, BACK, START, LS, RS, GUIDE] } else { &[A, B, X, Y, LB, RB, BACK, START, LS, RS, GUIDE] };
    let mut buttons = 0;
    for (i, b) in order.iter().enumerate() {
        if r.buttons & (1 << i) != 0 {
            buttons |= b;
        }
    }
    if let Some(h) = r.hat {
        buttons |= match h {
            0 => UP,
            1 => UP | RIGHT,
            2 => RIGHT,
            3 => DOWN | RIGHT,
            4 => DOWN,
            5 => DOWN | LEFT,
            6 => LEFT,
            7 => UP | LEFT,
            _ => 0,
        };
    }
    Pad { buttons, lx: r.x, ly: r.y, lt: 0, rt: 0 }
}

// ---- navigating with a pad -------------------------------------------------------

/// What a pad means to the desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// A: Enter
    Accept,
    /// B: Escape
    Back,
    /// Start / Menu / Guide: the start menu
    Menu,
    /// RB, LB: the next / previous field (Tab, Shift+Tab)
    Next,
    Prev,
}

/// The stick must lean this far (of 1000) to count as a direction.
const DEAD: i32 = 550;
/// A held direction repeats after this long, then this often (ms).
const REPEAT_AFTER: u64 = 400;
const REPEAT_EVERY: u64 = 110;

#[derive(Default)]
pub struct Navigator {
    held: u32,
    since: u64,
    last_repeat: u64,
}

impl Navigator {
    pub fn new() -> Navigator {
        Navigator::default()
    }

    /// The pad's new state at `now` (ms): what the desktop should do.
    pub fn feed(&mut self, p: &Pad, now: u64) -> Vec<Nav> {
        let mut dirs = p.buttons & (UP | DOWN | LEFT | RIGHT);
        if p.ly <= -DEAD {
            dirs |= UP;
        } else if p.ly >= DEAD {
            dirs |= DOWN;
        }
        if p.lx <= -DEAD {
            dirs |= LEFT;
        } else if p.lx >= DEAD {
            dirs |= RIGHT;
        }
        let now_held = (p.buttons & !(UP | DOWN | LEFT | RIGHT)) | dirs;
        let pressed = now_held & !self.held;
        let mut out = Vec::new();
        let emit = |b: u32, out: &mut Vec<Nav>| {
            for (m, n) in [(UP, Nav::Up), (DOWN, Nav::Down), (LEFT, Nav::Left), (RIGHT, Nav::Right), (A, Nav::Accept), (B, Nav::Back), (START, Nav::Menu), (GUIDE, Nav::Menu), (RB, Nav::Next), (LB, Nav::Prev)] {
                if b & m != 0 {
                    out.push(n);
                }
            }
        };
        emit(pressed, &mut out);
        if pressed & (UP | DOWN | LEFT | RIGHT) != 0 {
            self.since = now;
            self.last_repeat = now;
        } else if dirs != 0 && now >= self.since + REPEAT_AFTER && now >= self.last_repeat + REPEAT_EVERY {
            // held: repeat the directions
            self.last_repeat = now;
            emit(dirs, &mut out);
        }
        self.held = now_held;
        out
    }
}

// ---- rumble motors ---------------------------------------------------------------

/// Which rumble protocol a pad speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rumble {
    Xbox360,
    XboxOne,
    DualShock4,
    DualSense,
}

/// The Xbox One pad's "power on" message: it sends nothing until it gets it.
pub fn xbox_one_start(seq: u8) -> Vec<u8> {
    alloc::vec![0x05, 0x20, seq, 0x01, 0x00]
}

/// The packet that sets a pad's motors: `strong` (the big, low motor) and
/// `weak` (the small, high one), 0-255.
pub fn rumble(kind: Rumble, strong: u8, weak: u8, seq: u8) -> Vec<u8> {
    match kind {
        Rumble::Xbox360 => alloc::vec![0x00, 0x08, 0x00, strong, weak, 0x00, 0x00, 0x00],
        Rumble::XboxOne => {
            let pct = |v: u8| (v as u32 * 100 / 255) as u8;
            // all motors, triggers off, played once for up to 2.55 s
            alloc::vec![0x09, 0x00, seq, 0x09, 0x00, 0x0F, 0x00, 0x00, pct(strong), pct(weak), 0xFF, 0x00, 0x00]
        }
        Rumble::DualShock4 => {
            let mut r = alloc::vec![0u8; 32];
            r[0] = 0x05;
            r[1] = 0x07; // motors, light bar, flash
            r[4] = weak;
            r[5] = strong;
            // HydatekOS blue on the light bar
            r[6] = 0x10;
            r[7] = 0x60;
            r[8] = 0xFF;
            r
        }
        Rumble::DualSense => {
            let mut r = alloc::vec![0u8; 48];
            r[0] = 0x02;
            r[1] = 0x03; // compatible rumble, and it replaces the haptics
            r[3] = weak;
            r[4] = strong;
            r
        }
    }
}

/// Plays haptic patterns on a pad's motors: pulses become "motors on at
/// this strength" and "motors off" at the right moments.
#[derive(Default)]
pub struct Motor {
    /// (when, strength): what to set next
    queue: Vec<(u64, u8)>,
}

impl Motor {
    /// Queue a pattern starting at `now` (ms); it replaces what's playing.
    pub fn play(&mut self, pulses: &[Pulse], now: u64) {
        self.queue.clear();
        let mut t = now;
        for q in pulses {
            self.queue.push((t, q.amp));
            // motors spin up slowly: short pulses are stretched so they're felt
            t += (q.ms as u64).max(40);
            self.queue.push((t, 0));
            t += q.gap as u64;
        }
    }

    /// The strength to set now, if it changes.
    pub fn due(&mut self, now: u64) -> Option<u8> {
        let mut last = None;
        while let Some(&(t, a)) = self.queue.first() {
            if t > now {
                break;
            }
            last = Some(a);
            self.queue.remove(0);
        }
        last
    }
}

/// A pulse's strength on each motor: strong pulses use the big motor, light
/// ones only the small one (it feels crisper).
pub fn split(amp: u8) -> (u8, u8) {
    if amp == 0 {
        (0, 0)
    } else if amp < 160 {
        (0, amp)
    } else {
        (amp, amp)
    }
}
