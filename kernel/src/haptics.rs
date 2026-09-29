//! Haptic feedback: short vibration patterns for touches, confirmations and
//! mistakes, the way phones and haptic touchpads answer a tap.
//!
//! The UI asks for a *kind* of feedback (`Haptic::Tap`, `Haptic::Error`…);
//! this file turns it into a pattern of pulses at the chosen strength, and
//! the kernel sends patterns to whatever can play them:
//! - **a paired Android phone** (Phone Link, when the phone has a vibration
//!   motor and "Vibrate my phone" is on): the pattern goes as a `haptic`
//!   message and the phone plays it with its motor;
//! - a haptic touchpad or a device's own vibration motor, once HydatekOS has
//!   drivers for them (the firmware gives no access to either).
//!
//! Settings › Sound & haptics draws each pattern as it plays, so what's
//! asked for is visible on any computer.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Haptic {
    /// a key or button pressed on a touch screen
    Tap,
    /// a switch turned on or off
    Click,
    /// a window snapping into place, a detent
    Tick,
    /// something went through (unlocked, saved)
    Success,
    /// something needs attention
    Warning,
    /// something was refused (a wrong PIN)
    Error,
    /// a touch held long enough to do something else
    LongPress,
}

impl Haptic {
    pub const ALL: [Haptic; 7] = [Haptic::Tap, Haptic::Click, Haptic::Tick, Haptic::Success, Haptic::Warning, Haptic::Error, Haptic::LongPress];

    pub fn name(self) -> &'static str {
        match self {
            Haptic::Tap => "Tap",
            Haptic::Click => "Click",
            Haptic::Tick => "Tick",
            Haptic::Success => "Success",
            Haptic::Warning => "Warning",
            Haptic::Error => "Error",
            Haptic::LongPress => "Long press",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strength {
    Light,
    Medium,
    Strong,
}

impl Strength {
    pub const ALL: [Strength; 3] = [Strength::Light, Strength::Medium, Strength::Strong];

    pub fn name(self) -> &'static str {
        match self {
            Strength::Light => "Light",
            Strength::Medium => "Medium",
            Strength::Strong => "Strong",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Strength::Light => "light",
            Strength::Medium => "medium",
            Strength::Strong => "strong",
        }
    }

    pub fn from_id(s: &str) -> Strength {
        match s {
            "light" => Strength::Light,
            "strong" => Strength::Strong,
            _ => Strength::Medium,
        }
    }

    /// How strong, in percent.
    pub fn percent(self) -> u32 {
        match self {
            Strength::Light => 45,
            Strength::Medium => 75,
            Strength::Strong => 100,
        }
    }

    /// Scale an amplitude (1-255) for this strength.
    fn scale(self, amp: u8) -> u8 {
        (amp as u32 * self.percent() / 100).clamp(1, 255) as u8
    }
}

/// One pulse: the motor on for `ms` at `amp` (1-255), then off for `gap` ms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pulse {
    pub ms: u16,
    pub amp: u8,
    pub gap: u16,
}

const fn p(ms: u16, amp: u8, gap: u16) -> Pulse {
    Pulse { ms, amp, gap }
}

/// The pulses for a kind of feedback at a strength.
pub fn pattern(h: Haptic, s: Strength) -> Vec<Pulse> {
    let base: &[Pulse] = match h {
        // crisp and short: many of these in a row while typing
        Haptic::Tap => &[p(10, 180, 0)],
        Haptic::Click => &[p(14, 230, 0)],
        Haptic::Tick => &[p(6, 140, 0)],
        // two rising taps
        Haptic::Success => &[p(12, 150, 70), p(18, 255, 0)],
        // two even, longer pulses
        Haptic::Warning => &[p(30, 200, 90), p(30, 200, 0)],
        // three quick strong buzzes
        Haptic::Error => &[p(40, 255, 50), p(40, 255, 50), p(40, 255, 0)],
        // one long swell
        Haptic::LongPress => &[p(60, 210, 0)],
    };
    base.iter().map(|q| Pulse { amp: s.scale(q.amp), ..*q }).collect()
}

/// A haptic touchpad plays waveforms rather than pulses: the HID waveform
/// (hid.rs WAVE_*), how many more times to play it, and how far apart (ms).
pub fn waveform(h: Haptic) -> (u16, u32, u32) {
    match h {
        Haptic::Tap | Haptic::Tick => (crate::hid::WAVE_CLICK, 0, 0),
        Haptic::Click => (crate::hid::WAVE_PRESS, 0, 0),
        Haptic::Success => (crate::hid::WAVE_CLICK, 1, 80),
        Haptic::Warning => (crate::hid::WAVE_BUZZ, 1, 120),
        Haptic::Error => (crate::hid::WAVE_BUZZ, 2, 90),
        Haptic::LongPress => (crate::hid::WAVE_RUMBLE, 0, 0),
    }
}

/// How long a pattern lasts (ms).
pub fn duration(pulses: &[Pulse]) -> u32 {
    pulses.iter().map(|q| q.ms as u32 + q.gap as u32).sum()
}

/// A pattern as the phone's `haptic` message carries it: "ms:amp:gap,…".
pub fn encode(pulses: &[Pulse]) -> String {
    let mut out = String::new();
    for (i, q) in pulses.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&alloc::format!("{}:{}:{}", q.ms, q.amp, q.gap));
    }
    out
}

/// Read an encoded pattern (at most 16 pulses, each at most 1 s): what the
/// phone does with a `haptic` message (and the tests check it).
#[allow(dead_code)]
pub fn decode(s: &str) -> Option<Vec<Pulse>> {
    let mut out = Vec::new();
    for part in s.split(',').filter(|p| !p.is_empty()) {
        let mut it = part.split(':');
        let ms: u16 = it.next()?.trim().parse().ok()?;
        let amp: u8 = it.next()?.trim().parse().ok()?;
        let gap: u16 = it.next()?.trim().parse().ok()?;
        if ms == 0 || ms > 1000 || gap > 1000 || amp == 0 || out.len() == 16 {
            return None;
        }
        out.push(Pulse { ms, amp, gap });
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The feedback engine: settings, the patterns waiting to be played, and
/// the latest one (for Settings to draw).
pub struct Haptics {
    pub on: bool,
    pub strength: Strength,
    /// play them on the paired phone too
    pub phone: bool,
    /// patterns asked for since the outputs last took them
    pub pending: Vec<Vec<Pulse>>,
    /// the same, for HydatekOS's own drivers (haptic touchpads, pad motors)
    pub device: Vec<(Haptic, Vec<Pulse>)>,
    /// the latest feedback and when it was asked for (ms)
    pub last: Option<(Haptic, u64)>,
    /// how many have been asked for (Settings shows it)
    pub count: u32,
}

impl Default for Haptics {
    fn default() -> Haptics {
        Haptics { on: true, strength: Strength::Medium, phone: false, pending: Vec::new(), device: Vec::new(), last: None, count: 0 }
    }
}

/// Taps closer together than this are felt as one.
const MIN_GAP_MS: u64 = 25;

impl Haptics {
    /// Ask for feedback `h` at time `now` (ms).
    pub fn feel(&mut self, h: Haptic, now: u64) {
        if !self.on {
            return;
        }
        if let Some((_, t)) = self.last {
            if now < t + MIN_GAP_MS && matches!(h, Haptic::Tap | Haptic::Tick) {
                return;
            }
        }
        self.last = Some((h, now));
        self.count = self.count.wrapping_add(1);
        if self.pending.len() < 8 {
            self.pending.push(pattern(h, self.strength));
        }
        if self.device.len() < 8 {
            self.device.push((h, pattern(h, self.strength)));
        }
    }
}
