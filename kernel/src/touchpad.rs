//! Touchpad gestures: what fingers on a multitouch touchpad mean.
//!
//! - one finger moving: the pointer moves;
//! - one finger tapped: a click; two fingers tapped: a right click; three: a
//!   middle click;
//! - two fingers moving up or down (or sideways): scrolling;
//! - the pad's own button: a click, or a right click with two fingers down;
//! - palms (contacts the pad isn't confident are fingers) are ignored.
//!
//! Fed with hid::TouchReport frames; plain logic, host-tested.

use crate::hid::{Contact, TouchReport};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// move the pointer (pixels)
    Move(i32, i32),
    /// scroll by notches: positive is down / right
    Scroll(i32),
    ScrollX(i32),
    /// a button pressed or let go (0 left, 1 right, 2 middle)
    Press(u8),
    Release(u8),
    /// a tap: press and release
    Click(u8),
}

/// A tap is shorter than this (ms) and moves less than `TAP_SLOP` pixels.
const TAP_MS: u64 = 220;
const TAP_SLOP: i32 = 12;
/// Pixels of two-finger movement per scroll notch.
const NOTCH: i32 = 40;

#[derive(Default)]
pub struct Gestures {
    last: Vec<Contact>,
    down_at: u64,
    /// most fingers down during this touch
    most: usize,
    /// how far the fingers have travelled (pixels) this touch
    travel: i32,
    /// fractions of a pixel / notch carried over
    rem: (i32, i32),
    scroll: (i32, i32),
    button: Option<u8>,
    /// the button was pressed during this touch (so lifting isn't a tap)
    pressed: bool,
    /// pointer speed: pixels for the pad's full width
    pub width_px: i32,
}

impl Gestures {
    pub fn new() -> Gestures {
        Gestures { width_px: 1400, ..Default::default() }
    }

    /// Read one frame. `max_x` is the pad's X range; `now` in ms.
    pub fn feed(&mut self, r: &TouchReport, max_x: i32, now: u64) -> Vec<Gesture> {
        let mut out = Vec::new();
        let fingers: Vec<Contact> = r.contacts.iter().filter(|c| c.tip && c.confident).copied().collect();
        let n = fingers.len();
        // device units to pixels (fixed point, 1/256)
        let k = (self.width_px as i64 * 256 / max_x.max(1) as i64) as i32;
        let before = self.last.len();
        if n > 0 && before == 0 {
            // a new touch
            self.down_at = now;
            self.most = 0;
            self.travel = 0;
            self.rem = (0, 0);
            self.scroll = (0, 0);
            self.pressed = false;
        }
        self.most = self.most.max(n);
        // movement of the fingers that were down before too
        let mut moved = (0i32, 0i32, 0i32);
        for f in &fingers {
            if let Some(p) = self.last.iter().find(|p| p.id == f.id) {
                moved.0 += f.x - p.x;
                moved.1 += f.y - p.y;
                moved.2 += 1;
            }
        }
        if moved.2 > 0 && n == before {
            let (dx, dy) = ((moved.0 / moved.2) * k + self.rem.0, (moved.1 / moved.2) * k + self.rem.1);
            let (px, py) = (dx / 256, dy / 256);
            self.rem = (dx - px * 256, dy - py * 256);
            self.travel += px.abs() + py.abs();
            if n == 1 {
                if px != 0 || py != 0 {
                    out.push(Gesture::Move(px, py));
                }
            } else if n == 2 {
                self.scroll.0 += px;
                self.scroll.1 += py;
                // natural scrolling: content follows the fingers
                while self.scroll.1 <= -NOTCH {
                    self.scroll.1 += NOTCH;
                    out.push(Gesture::Scroll(1));
                }
                while self.scroll.1 >= NOTCH {
                    self.scroll.1 -= NOTCH;
                    out.push(Gesture::Scroll(-1));
                }
                while self.scroll.0 <= -NOTCH {
                    self.scroll.0 += NOTCH;
                    out.push(Gesture::ScrollX(1));
                }
                while self.scroll.0 >= NOTCH {
                    self.scroll.0 -= NOTCH;
                    out.push(Gesture::ScrollX(-1));
                }
            }
        }
        // the pad's own button: two fingers down makes it a right click
        match (r.button, self.button) {
            (true, None) => {
                let b = if n >= 2 { 1 } else { 0 };
                self.button = Some(b);
                self.pressed = true;
                out.push(Gesture::Press(b));
            }
            (false, Some(b)) => {
                self.button = None;
                out.push(Gesture::Release(b));
            }
            _ => {}
        }
        // every finger lifted: a tap if quick and still
        if n == 0 && before > 0 && !self.pressed && now.saturating_sub(self.down_at) < TAP_MS && self.travel < TAP_SLOP && !r.button {
            let b = match self.most {
                1 => Some(0),
                2 => Some(1),
                3 => Some(2),
                _ => None,
            };
            if let Some(b) = b {
                out.push(Gesture::Click(b));
            }
        }
        self.last = fingers;
        out
    }
}
