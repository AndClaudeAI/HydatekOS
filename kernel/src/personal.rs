//! Personalisation: the choices, and the arithmetic behind them.
//!
//! - what's on the desktop and the lock screen: one of HydatekOS's scenes
//!   (drawn, not pictures: wallpaper.rs), a colour, a gradient, or your own
//!   picture, fitted to the screen (fill, fit, stretch, centre, tile);
//! - scenes that follow the time of day: dawn, day, dusk and night skies;
//! - light, dark, or automatic (dark from 19:00 to 07:00);
//! - an accent colour: one of the presets, or one taken from the wallpaper
//!   (its most characteristic colour, made readable in light and dark);
//! - the desktop widgets.
//!
//! Choices are saved as short text (`Look::save` / `Look::load`). Plain
//! logic, integer only, host-tested.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Dune,
    Lagoon,
    Aurora,
    Hills,
    Mesa,
    Bloom,
    Harmattan,
}

impl Scene {
    pub const ALL: [Scene; 7] = [Scene::Dune, Scene::Lagoon, Scene::Aurora, Scene::Hills, Scene::Mesa, Scene::Bloom, Scene::Harmattan];

    pub fn name(self) -> &'static str {
        match self {
            Scene::Dune => "Dune",
            Scene::Lagoon => "Lagoon",
            Scene::Aurora => "Aurora",
            Scene::Hills => "Hills",
            Scene::Mesa => "Mesa",
            Scene::Bloom => "Bloom",
            Scene::Harmattan => "Harmattan",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Scene::Dune => "dune",
            Scene::Lagoon => "lagoon",
            Scene::Aurora => "aurora",
            Scene::Hills => "hills",
            Scene::Mesa => "mesa",
            Scene::Bloom => "bloom",
            Scene::Harmattan => "harmattan",
        }
    }
}

/// What a wallpaper is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wall {
    Scene(Scene),
    Solid(u32),
    Gradient(u32, u32),
    /// a picture, by path
    Picture(String),
}

/// Solid colours and gradients offered in Settings.
pub const SOLIDS: [u32; 6] = [0x2B2A48, 0x1F3B4D, 0x3E5641, 0x7A3B2E, 0xD8CBB8, 0x111111];
pub const GRADIENTS: [(u32, u32); 4] = [(0xF3B391, 0x6A4C93), (0x9AD0EC, 0x1E3A5F), (0xFDE2A7, 0xC4895E), (0x243B55, 0x141E30)];

fn hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim_start_matches('#'), 16).ok().filter(|v| *v <= 0xFF_FFFF)
}

impl Wall {
    pub fn save(&self) -> String {
        match self {
            Wall::Scene(s) => s.id().to_string(),
            Wall::Solid(c) => format!("colour:#{:06x}", c),
            Wall::Gradient(a, b) => format!("gradient:#{:06x},#{:06x}", a, b),
            Wall::Picture(p) => format!("picture:{}", p),
        }
    }

    pub fn load(s: &str) -> Option<Wall> {
        if let Some(p) = s.strip_prefix("picture:") {
            return (!p.is_empty()).then(|| Wall::Picture(p.to_string()));
        }
        if let Some(c) = s.strip_prefix("colour:") {
            return hex(c).map(Wall::Solid);
        }
        if let Some(g) = s.strip_prefix("gradient:") {
            let (a, b) = g.split_once(',')?;
            return Some(Wall::Gradient(hex(a)?, hex(b)?));
        }
        Scene::ALL.iter().find(|x| x.id() == s).map(|x| Wall::Scene(*x))
    }

    pub fn name(&self) -> String {
        match self {
            Wall::Scene(s) => s.name().to_string(),
            Wall::Solid(_) => String::from("Colour"),
            Wall::Gradient(..) => String::from("Gradient"),
            Wall::Picture(p) => p.rsplit('/').next().unwrap_or(p).to_string(),
        }
    }
}

/// How a picture fills the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// cover the screen, cropping what's over
    Fill,
    /// all of it, on a matching colour
    Fit,
    Stretch,
    Centre,
    Tile,
}

impl Fit {
    pub const ALL: [Fit; 5] = [Fit::Fill, Fit::Fit, Fit::Stretch, Fit::Centre, Fit::Tile];

    pub fn name(self) -> &'static str {
        match self {
            Fit::Fill => "Fill",
            Fit::Fit => "Fit",
            Fit::Stretch => "Stretch",
            Fit::Centre => "Centre",
            Fit::Tile => "Tile",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
    /// dark from 19:00 to 07:00
    Auto,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Light, Mode::Dark, Mode::Auto];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Light => "Light",
            Mode::Dark => "Dark",
            Mode::Auto => "Automatic",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accent {
    Preset(u8),
    /// taken from the wallpaper
    FromWall,
}

/// Everything you chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub wall: Wall,
    pub fit: Fit,
    /// the lock screen's own wallpaper (None: the same as the desktop)
    pub lock: Option<Wall>,
    /// scenes follow the time of day
    pub time_of_day: bool,
    pub mode: Mode,
    pub accent: Accent,
    /// the clock and quick settings on the desktop
    pub widgets: bool,
}

impl Default for Look {
    fn default() -> Look {
        Look { wall: Wall::Scene(Scene::Dune), fit: Fit::Fill, lock: None, time_of_day: false, mode: Mode::Light, accent: Accent::Preset(0), widgets: true }
    }
}

impl Look {
    /// As lines of `key=value`.
    pub fn save(&self) -> String {
        format!(
            "wall={}\nwallfit={}\nlockwall={}\ntimeofday={}\nthememode={}\naccentfrom={}\nwidgets={}\n",
            self.wall.save(),
            self.fit.name().to_ascii_lowercase(),
            self.lock.as_ref().map_or(String::from("same"), |w| w.save()),
            self.time_of_day as u8,
            self.mode.name().to_ascii_lowercase(),
            match self.accent {
                Accent::Preset(i) => format!("{}", i),
                Accent::FromWall => String::from("wallpaper"),
            },
            self.widgets as u8
        )
    }

    /// Read one saved `key=value` (unknown keys are ignored). True if it was ours.
    pub fn load(&mut self, key: &str, v: &str) -> bool {
        match key {
            "wall" => self.wall = Wall::load(v).unwrap_or(Wall::Scene(Scene::Dune)),
            "wallfit" => self.fit = Fit::ALL.iter().copied().find(|f| f.name().eq_ignore_ascii_case(v)).unwrap_or(Fit::Fill),
            "lockwall" => self.lock = if v == "same" { None } else { Wall::load(v) },
            "timeofday" => self.time_of_day = v == "1",
            "thememode" => self.mode = Mode::ALL.iter().copied().find(|m| m.name().eq_ignore_ascii_case(v)).unwrap_or(Mode::Light),
            "accentfrom" => self.accent = if v == "wallpaper" { Accent::FromWall } else { Accent::Preset(v.parse().unwrap_or(0)) },
            "widgets" => self.widgets = v != "0",
            _ => return false,
        }
        true
    }

    /// Dark or not, now (`minutes` since midnight).
    pub fn dark_at(&self, minutes: u32) -> bool {
        match self.mode {
            Mode::Light => false,
            Mode::Dark => true,
            Mode::Auto => !(7 * 60..19 * 60).contains(&(minutes % 1440)),
        }
    }
}

// ---- colour arithmetic -------------------------------------------------------------

pub fn rgb(c: u32) -> (i32, i32, i32) {
    ((c >> 16 & 255) as i32, (c >> 8 & 255) as i32, (c & 255) as i32)
}

pub fn pack(r: i32, g: i32, b: i32) -> u32 {
    (r.clamp(0, 255) as u32) << 16 | (g.clamp(0, 255) as u32) << 8 | b.clamp(0, 255) as u32
}

/// `a` to `b` by `t`/256.
pub fn mix(a: u32, b: u32, t: i32) -> u32 {
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    let t = t.clamp(0, 256);
    pack(ar + (br - ar) * t / 256, ag + (bg - ag) * t / 256, ab + (bb - ab) * t / 256)
}

/// Relative luminance ×1000 (sRGB, approximated with a square).
pub fn luminance(c: u32) -> i32 {
    let (r, g, b) = rgb(c);
    let lin = |v: i32| v * v * 1000 / (255 * 255);
    (2126 * lin(r) + 7152 * lin(g) + 722 * lin(b)) / 10000
}

/// Contrast ratio ×100 between two colours (WCAG).
pub fn contrast(a: u32, b: u32) -> i32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 50) * 100 / (lo + 50)
}

fn saturation(c: u32) -> i32 {
    let (r, g, b) = rgb(c);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    if mx == 0 {
        0
    } else {
        (mx - mn) * 255 / mx
    }
}

/// The wallpaper's characteristic colour: the most common colour among its
/// colourful pixels (its greys and near-blacks don't count).
pub fn dominant(px: &[u32]) -> u32 {
    let mut hist = alloc::vec![0u32; 4096];
    let step = (px.len() / 20_000).max(1);
    let mut any = 0;
    for p in px.iter().step_by(step) {
        let c = *p & 0xFF_FFFF;
        let (s, l) = (saturation(c), luminance(c));
        if s < 60 || !(30..950).contains(&l) {
            continue;
        }
        // weighted towards colourful pixels
        hist[((c >> 12 & 0xF00) | (c >> 8 & 0xF0) | (c >> 4 & 0xF)) as usize] += 1 + s as u32 / 64;
        any += 1;
    }
    if any == 0 {
        return 0x6B6B6B;
    }
    let (i, _) = hist.iter().enumerate().max_by_key(|(_, n)| **n).unwrap();
    // the middle of that bucket, averaged over its pixels
    let (mut r, mut g, mut b, mut n) = (0i64, 0i64, 0i64, 0i64);
    for p in px.iter().step_by(step) {
        let c = *p & 0xFF_FFFF;
        if ((c >> 12 & 0xF00) | (c >> 8 & 0xF0) | (c >> 4 & 0xF)) as usize == i {
            let (cr, cg, cb) = rgb(c);
            r += cr as i64;
            g += cg as i64;
            b += cb as i64;
            n += 1;
        }
    }
    pack((r / n) as i32, (g / n) as i32, (b / n) as i32)
}

/// An accent pair made from a colour: one readable under white text (light
/// theme), one bright enough on the dark theme's surfaces.
pub fn accent_pair(c: u32) -> (u32, u32) {
    let mut light = c;
    // white text on it: at least 3:1
    for _ in 0..16 {
        if contrast(light, 0xFFFFFF) >= 300 {
            break;
        }
        light = mix(light, 0x000000, 32);
    }
    let mut dark = c;
    // on the dark surface (#23202C): at least 3:1
    for _ in 0..16 {
        if contrast(dark, 0x23202C) >= 300 {
            break;
        }
        dark = mix(dark, 0xFFFFFF, 32);
    }
    (light, dark)
}

// ---- the time of day ----------------------------------------------------------------

/// The sky at a time of day: (top, horizon, sun colour, sun height 0-1000,
/// light 0-256 for the land).
pub fn sky(minutes: u32) -> (u32, u32, u32, i32, i32) {
    // keyframes: (minute, top, horizon, sun, height, light)
    const K: [(u32, u32, u32, u32, i32, i32); 7] = [
        (0, 0x0B1026, 0x1B2340, 0xC9D2E8, -300, 70),
        (5 * 60 + 30, 0x1B2346, 0x7A5A7A, 0xF6B38A, -40, 110),
        (7 * 60, 0x6E8CB8, 0xF4C7A1, 0xFFD9A0, 150, 200),
        (13 * 60, 0x8FB8E0, 0xE8EEF2, 0xFFF3D6, 900, 256),
        (18 * 60, 0x7C8FC0, 0xF6B98A, 0xFFC27A, 250, 220),
        (19 * 60 + 30, 0x2E2E5E, 0xC76B5A, 0xE8704A, -20, 130),
        (21 * 60, 0x0B1026, 0x1B2340, 0xC9D2E8, -300, 70),
    ];
    let m = minutes % 1440;
    let mut i = K.len() - 1;
    for (k, key) in K.iter().enumerate() {
        if key.0 <= m {
            i = k;
        }
    }
    let a = K[i];
    let b = if i + 1 < K.len() { K[i + 1] } else { (1440, K[0].1, K[0].2, K[0].3, K[0].4, K[0].5) };
    let span = (b.0 - a.0).max(1) as i32;
    let t = ((m - a.0) as i32 * 256 / span).clamp(0, 256);
    (mix(a.1, b.1, t), mix(a.2, b.2, t), mix(a.3, b.3, t), a.4 + (b.4 - a.4) * t / 256, a.5 + (b.5 - a.5) * t / 256)
}

// ---- pictures on the screen -------------------------------------------------------

/// Resample `src` (sw × sh) to dw × dh: an area average when shrinking,
/// bilinear when enlarging.
pub fn resample(src: &[u32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u32> {
    let mut out = alloc::vec![0u32; dw * dh];
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return out;
    }
    for y in 0..dh {
        let (y0, y1) = (y * sh / dh, ((y + 1) * sh / dh).max(y * sh / dh + 1).min(sh));
        for x in 0..dw {
            let c = if sw >= dw && sh >= dh {
                let (x0, x1) = (x * sw / dw, ((x + 1) * sw / dw).max(x * sw / dw + 1).min(sw));
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        let p = src[yy * sw + xx];
                        r += p >> 16 & 255;
                        g += p >> 8 & 255;
                        b += p & 255;
                        n += 1;
                    }
                }
                (r / n) << 16 | (g / n) << 8 | b / n
            } else {
                // bilinear, in 1/256 steps
                let fx = ((x * 2 + 1) * sw * 128 / dw).saturating_sub(128);
                let fy = ((y * 2 + 1) * sh * 128 / dh).saturating_sub(128);
                let (ix, iy) = ((fx >> 8).min(sw - 1), (fy >> 8).min(sh - 1));
                let (tx, ty) = ((fx & 255) as i32, (fy & 255) as i32);
                let (jx, jy) = ((ix + 1).min(sw - 1), (iy + 1).min(sh - 1));
                let top = mix(src[iy * sw + ix], src[iy * sw + jx], tx);
                let bot = mix(src[jy * sw + ix], src[jy * sw + jx], tx);
                mix(top, bot, ty)
            };
            out[y * dw + x] = 0xFF00_0000 | c;
        }
    }
    out
}

/// A picture made into a wallpaper of `w` × `h`.
pub fn compose(src: &[u32], sw: usize, sh: usize, w: usize, h: usize, fit: Fit) -> Vec<u32> {
    if sw == 0 || sh == 0 || src.len() < sw * sh {
        return alloc::vec![0xFF2B2A48; w * h];
    }
    match fit {
        Fit::Stretch => resample(src, sw, sh, w, h),
        Fit::Fill => {
            // the largest part of the picture with the screen's shape
            let (cw, ch) = if sw * h > sh * w { (sh * w / h, sh) } else { (sw, sw * h / w) };
            let (cx, cy) = ((sw - cw) / 2, (sh - ch) / 2);
            let mut crop = Vec::with_capacity(cw * ch);
            for y in cy..cy + ch {
                crop.extend_from_slice(&src[y * sw + cx..y * sw + cx + cw]);
            }
            resample(&crop, cw, ch, w, h)
        }
        Fit::Fit | Fit::Centre => {
            let (dw, dh) = if fit == Fit::Centre && sw <= w && sh <= h {
                (sw, sh)
            } else if fit == Fit::Centre {
                // too big to centre whole: shown at the size that fits
                if sw * h > sh * w { (w, (sh * w / sw).max(1)) } else { ((sw * h / sh).max(1), h) }
            } else if sw * h > sh * w {
                (w, (sh * w / sw).max(1))
            } else {
                ((sw * h / sh).max(1), h)
            };
            let pic = resample(src, sw, sh, dw, dh);
            // around it: the picture's own colour, darkened
            let bg = 0xFF00_0000 | mix(dominant(src), 0x101010, 140);
            let mut out = alloc::vec![bg; w * h];
            let (ox, oy) = ((w - dw) / 2, (h - dh) / 2);
            for y in 0..dh {
                out[(oy + y) * w + ox..(oy + y) * w + ox + dw].copy_from_slice(&pic[y * dw..(y + 1) * dw]);
            }
            out
        }
        Fit::Tile => {
            let mut out = alloc::vec![0u32; w * h];
            for y in 0..h {
                for x in 0..w {
                    out[y * w + x] = 0xFF00_0000 | src[(y % sh) * sw + x % sw];
                }
            }
            out
        }
    }
}
