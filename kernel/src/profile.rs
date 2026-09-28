//! The profile of the person who uses this computer: their name and picture,
//! kept in /system/profile.txt (and /system/profile.png for a photo).
//!
//! ```text
//! name=Ada Obi
//! avatar=initials 3            initials on colour 3, or
//!                              avatar=motif 2 / avatar=picture
//! since=2026-09-28             the day it was set up
//! ```

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Longest name kept, in characters.
pub const NAME_MAX: usize = 40;
/// A photo is kept as a square this many pixels wide.
pub const PIC_SIZE: u32 = 256;

/// Backgrounds for initials: (name, colour).
pub const COLOURS: [(&str, u32); 8] = [
    ("Ember", 0xC0612B),
    ("Saffron", 0xD99A2B),
    ("Moss", 0x4F7942),
    ("Lagoon", 0x2A8C8C),
    ("Ocean", 0x2F6690),
    ("Indigo", 0x4B4E9E),
    ("Plum", 0x7B3F7A),
    ("Rose", 0xB84A62),
];

/// Drawn pictures to choose from (see `avatar.rs`).
pub const MOTIFS: [&str; 6] = ["Sunrise", "Night", "Waves", "Hills", "Peaks", "Bloom"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Avatar {
    /// initials on `COLOURS[i]`
    Initials(u8),
    /// `MOTIFS[i]`
    Motif(u8),
    /// the photo in /system/profile.png
    Picture,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Profile {
    pub name: String,
    pub avatar: Avatar,
    /// (year, month, day) it was set up
    pub since: (u16, u8, u8),
}

impl Default for Profile {
    fn default() -> Profile {
        Profile { name: String::new(), avatar: Avatar::Initials(0), since: (0, 0, 0) }
    }
}

impl Profile {
    /// Set up yet? (A profile always has a name.)
    pub fn ready(&self) -> bool {
        !self.name.is_empty()
    }

    pub fn to_text(&self) -> String {
        let avatar = match self.avatar {
            Avatar::Initials(i) => format!("initials {}", i),
            Avatar::Motif(i) => format!("motif {}", i),
            Avatar::Picture => String::from("picture"),
        };
        let (y, m, d) = self.since;
        format!("name={}\navatar={}\nsince={:04}-{:02}-{:02}\n", self.name, avatar, y, m, d)
    }

    /// Read a profile; unknown lines are skipped and bad values fall back to
    /// the defaults, so a damaged file never stops the computer starting.
    pub fn parse(text: &str) -> Profile {
        let mut p = Profile::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "name" => p.name = clean_name(v).unwrap_or_default(),
                "avatar" => {
                    let mut it = v.split_whitespace();
                    let kind = it.next().unwrap_or("");
                    let i: u8 = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
                    p.avatar = match kind {
                        "motif" if (i as usize) < MOTIFS.len() => Avatar::Motif(i),
                        "picture" => Avatar::Picture,
                        "initials" => Avatar::Initials(i % COLOURS.len() as u8),
                        // an unknown or damaged choice: the first colour
                        _ => Avatar::Initials(0),
                    };
                }
                "since" => {
                    let n: Vec<u32> = v.split('-').filter_map(|x| x.parse().ok()).collect();
                    if n.len() == 3 && (1..=12).contains(&n[1]) && (1..=31).contains(&n[2]) {
                        p.since = (n[0] as u16, n[1] as u8, n[2] as u8);
                    }
                }
                _ => {}
            }
        }
        p
    }

    pub fn first_name(&self) -> &str {
        first_name(&self.name)
    }
}

/// A name as typed, tidied: spaces trimmed and collapsed, no control
/// characters, at most `NAME_MAX` characters. `None` if nothing is left.
pub fn clean_name(s: &str) -> Option<String> {
    let mut out = String::new();
    for word in s.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    let out: String = out.chars().take(NAME_MAX).collect();
    let out = String::from(out.trim_end());
    (!out.is_empty()).then_some(out)
}

/// Up to two letters: the first letters of the first and last words
/// ("Ada Obi" → "AO", "chinwe" → "C", "Ngozi Okonjo-Iweala" → "NO").
pub fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).collect();
    let first = |w: &str| w.chars().find(|c| c.is_alphanumeric());
    let mut s = String::new();
    if let Some(c) = words.first().and_then(|w| first(w)) {
        s.extend(c.to_uppercase());
    }
    if words.len() > 1 {
        if let Some(c) = words.last().and_then(|w| first(w)) {
            s.extend(c.to_uppercase());
        }
    }
    s
}

pub fn first_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or("")
}

/// "Good morning" from 05:00, "Good afternoon" from 12:00, "Good evening"
/// from 17:00 until 05:00.
pub fn greeting(hour: u8) -> &'static str {
    match hour {
        5..=11 => "Good morning",
        12..=16 => "Good afternoon",
        _ => "Good evening",
    }
}

/// A colour for a new profile's initials, picked from the name so the same
/// name always starts with the same colour.
pub fn colour_for(name: &str) -> u8 {
    let h = name.to_lowercase().bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    (h % COLOURS.len() as u32) as u8
}

/// The centre square of a picture (ARGB pixels), scaled to `size` × `size`
/// with an area average. Transparent pixels are laid on white.
pub fn square(px: &[u32], w: u32, h: u32, size: u32) -> Vec<u32> {
    let side = w.min(h).max(1) as u64;
    let (ox, oy) = ((w as u64 - side.min(w as u64)) / 2, (h as u64 - side.min(h as u64)) / 2);
    let n = size.max(1) as u64;
    let mut out = Vec::with_capacity((n * n) as usize);
    for y in 0..n {
        let (y0, y1) = (oy + y * side / n, (oy + (y + 1) * side / n).max(oy + y * side / n + 1).min(h as u64));
        for x in 0..n {
            let (x0, x1) = (ox + x * side / n, (ox + (x + 1) * side / n).max(ox + x * side / n + 1).min(w as u64));
            let (mut r, mut g, mut b, mut k) = (0u64, 0u64, 0u64, 0u64);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let p = px[(yy * w as u64 + xx) as usize];
                    let a = (p >> 24) as u64;
                    // over white
                    let mix = |c: u32| (c as u64 * a + 255 * (255 - a)) / 255;
                    r += mix((p >> 16) & 255);
                    g += mix((p >> 8) & 255);
                    b += mix(p & 255);
                    k += 1;
                }
            }
            let k = k.max(1);
            out.push(0xFF00_0000 | ((r / k) as u32) << 16 | ((g / k) as u32) << 8 | (b / k) as u32);
        }
    }
    out
}
