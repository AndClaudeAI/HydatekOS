//! HydatekOS's sounds: made, not recorded.
//!
//! Each system sound is a few soft tones (sine waves with a quick attack and
//! an exponential fade), mixed at 48 kHz into 16-bit stereo for the audio
//! driver (hda.rs). The volume follows Settings, on a curve that sounds even
//! (loudness is logarithmic). Plain logic, host-tested, and integer only
//! (UEFI has no floating-point maths library).

use alloc::vec::Vec;

pub const RATE: u32 = 48_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// HydatekOS starting: a rising chord
    Startup,
    /// something arrived
    Notify,
    /// refused, wrong PIN
    Error,
    /// a tick for the volume keys
    Volume,
    /// unlocked, saved
    Success,
}

/// One tone: frequency (Hz), when it starts and how long it rings (ms), and
/// how loud (0-1000).
#[derive(Clone, Copy, Debug)]
struct Tone {
    hz: u32,
    at: u32,
    ms: u32,
    level: u32,
}

const fn t(hz: u32, at: u32, ms: u32, level: u32) -> Tone {
    Tone { hz, at, ms, level }
}

// C major ninth, spread: C4 G4 E5 D6
const STARTUP: [Tone; 4] = [t(262, 0, 1400, 700), t(392, 90, 1300, 600), t(659, 180, 1200, 500), t(1175, 300, 1000, 350)];
// two bright notes: E6 then B5
const NOTIFY: [Tone; 2] = [t(1319, 0, 260, 600), t(988, 110, 420, 600)];
// a low falling pair
const ERROR: [Tone; 2] = [t(220, 0, 220, 800), t(185, 150, 320, 800)];
const VOLUME: [Tone; 1] = [t(880, 0, 60, 500)];
// G5 then C6
const SUCCESS: [Tone; 2] = [t(784, 0, 200, 600), t(1047, 90, 380, 600)];

fn tones(s: Sound) -> &'static [Tone] {
    match s {
        Sound::Startup => &STARTUP,
        Sound::Notify => &NOTIFY,
        Sound::Error => &ERROR,
        Sound::Volume => &VOLUME,
        Sound::Success => &SUCCESS,
    }
}

/// sin(2π·phase/65536) × 32767, from a quarter-wave table.
fn sine(phase: u32) -> i32 {
    const N: usize = 64;
    // 32767·sin(k·π/128), k = 0..=64
    const Q: [i32; N + 1] = [
        0, 804, 1608, 2410, 3212, 4011, 4808, 5602, 6393, 7179, 7962, 8739, 9512, 10278, 11039, 11793,
        12539, 13279, 14010, 14732, 15446, 16151, 16846, 17530, 18204, 18868, 19519, 20159, 20787, 21403, 22005, 22594,
        23170, 23731, 24279, 24811, 25329, 25832, 26319, 26790, 27245, 27683, 28105, 28510, 28898, 29268, 29621, 29956,
        30273, 30571, 30852, 31113, 31356, 31580, 31785, 31971, 32137, 32285, 32412, 32521, 32609, 32678, 32728, 32757,
        32767,
    ];
    let p = phase & 0xFFFF;
    let quad = p >> 14;
    let x = p & 0x3FFF;
    // position in the quarter, with interpolation between table entries
    let (i, frac) = ((x >> 8) as usize, (x & 0xFF) as i32);
    let at = |k: usize| Q[k];
    let v = match quad {
        0 => at(i) + ((at(i + 1) - at(i)) * frac >> 8),
        1 => at(N - i) + ((at(N - i - 1) - at(N - i)) * frac >> 8),
        2 => -(at(i) + ((at(i + 1) - at(i)) * frac >> 8)),
        _ => -(at(N - i) + ((at(N - i - 1) - at(N - i)) * frac >> 8)),
    };
    v
}

/// A fade: 1024 at the start, halving every `half` samples (integer).
fn decay(n: u32, half: u32) -> i32 {
    let halves = n / half.max(1);
    if halves >= 16 {
        return 0;
    }
    let base = 1024 >> halves;
    // straight between halvings
    let frac = (n % half.max(1)) as i32 * 1024 / half.max(1) as i32;
    (base - (base / 2) * frac / 1024) as i32
}

/// A sound, rendered: 16-bit mono samples at RATE.
pub fn render(s: Sound) -> Vec<i16> {
    let ts = tones(s);
    let end = ts.iter().map(|t| t.at + t.ms).max().unwrap_or(0);
    let n = (end * RATE / 1000) as usize;
    let mut acc = alloc::vec![0i32; n];
    for tone in ts {
        let start = (tone.at * RATE / 1000) as usize;
        let len = (tone.ms * RATE / 1000) as usize;
        let step = (tone.hz as u64 * 65536 / RATE as u64) as u32;
        let attack = RATE / 200; // 5 ms
        let half = (len as u32 / 5).max(1);
        let mut phase = 0u32;
        for k in 0..len.min(n.saturating_sub(start)) {
            let env = if (k as u32) < attack { k as i32 * 1024 / attack as i32 } else { decay(k as u32 - attack, half) };
            // and a soft octave above for warmth
            let v = sine(phase) * 3 / 4 + sine(phase.wrapping_mul(2)) / 4;
            acc[start + k] += v * env / 1024 * tone.level as i32 / 1000 / 3;
            phase = phase.wrapping_add(step);
        }
    }
    acc.into_iter().map(|v| v.clamp(-32767, 32767) as i16).collect()
}

/// The volume setting (0-100) as a gain in 1/1024: even-sounding steps
/// (-40 dB at 1, 0 dB at 100), silence at 0.
pub fn gain(volume: u8, muted: bool) -> i32 {
    if muted || volume == 0 {
        return 0;
    }
    // 10^((v-100)/50): a table per 10, straight between
    const T: [i32; 11] = [10, 16, 26, 41, 64, 102, 162, 257, 407, 646, 1024];
    let v = volume.min(100) as usize;
    let (i, f) = (v / 10, (v % 10) as i32);
    if i == 10 {
        return 1024;
    }
    T[i] + (T[i + 1] - T[i]) * f / 10
}

/// Mixes sounds into the stream the driver plays.
#[derive(Default)]
pub struct Mixer {
    /// playing: (samples, how far in)
    voices: Vec<(Vec<i16>, usize)>,
}

impl Mixer {
    pub fn play(&mut self, s: Sound) {
        if self.voices.len() < 8 {
            self.voices.push((render(s), 0));
        }
    }

    /// Fill `out` (interleaved stereo) with what's playing, at `gain`/1024.
    pub fn fill(&mut self, out: &mut [i16], gain: i32) {
        for frame in out.chunks_mut(2) {
            let mut v = 0i32;
            for (samples, pos) in self.voices.iter_mut() {
                if let Some(s) = samples.get(*pos) {
                    v += *s as i32;
                    *pos += 1;
                }
            }
            let v = (v * gain / 1024).clamp(-32767, 32767) as i16;
            for c in frame.iter_mut() {
                *c = v;
            }
        }
        self.voices.retain(|(s, p)| *p < s.len());
    }
}
