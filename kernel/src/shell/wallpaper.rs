//! Wallpapers: HydatekOS's scenes, colours, gradients and your pictures.
//!
//! The scenes are drawn, not stored: a sky, a sun, and land in layers, in
//! integer arithmetic. Each can follow the time of day (dawn, day, dusk,
//! night skies from personal::sky), or keep the light or dark theme's look.
//!
//! - Dune: sky, sun and three rolling dunes (HydatekOS's own);
//! - Lagoon: a still sea with the sun's reflection and an island;
//! - Aurora: northern lights over mountains, with stars;
//! - Hills: green hills folding into the distance;
//! - Mesa: terracotta cliffs with flat tops;
//! - Bloom: soft light, abstract;
//! - Harmattan: the dusty West African dry-season sky over savanna and
//!   acacias.
//!
//! Drawn wallpapers are kept (`cached`), so a frame only copies them.

use crate::gfx::{lerp, sin_q14, Canvas, Color, Rect};
use crate::personal::{self, mix, Fit, Photo, Scene, Wall};
use crate::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicI32, Ordering};

/// The desktop's width in pixels, as last drawn (previews scale to it).
static SCREEN_W: AtomicI32 = AtomicI32::new(1920);

/// Draw the classic Dune scene into `r` (the setup assistant uses it).
pub fn draw(c: &mut Canvas, r: Rect, t: &Theme, tall: bool) {
    scene(c, r, Scene::Dune, t, None, tall);
}

/// The light a scene is drawn in.
#[derive(Clone, Copy)]
struct Light {
    top: u32,
    horizon: u32,
    sun: u32,
    /// the sun's height, -1000 (well below the horizon) to 1000 (overhead)
    height: i32,
    /// how lit the land is, 0-256
    light: i32,
    night: bool,
}

fn light_for(t: &Theme, minutes: Option<u32>) -> Light {
    match minutes {
        Some(m) => {
            let (top, horizon, sun, height, light) = personal::sky(m);
            Light { top, horizon, sun, height, light, night: light < 120 }
        }
        None if t.dark => Light { top: 0x14172B, horizon: 0x2A2440, sun: 0xE9C98F, height: 250, light: 110, night: true },
        None => Light { top: 0xE9EEF2, horizon: 0xF3E6D6, sun: 0xE4B783, height: 450, light: 256, night: false },
    }
}

/// Land colour in this light.
fn lit(c: u32, l: &Light) -> u32 {
    mix(mix(0x0B1026, l.horizon, 60), c, l.light)
}

fn px_blend(c: &mut Canvas, x: i32, y: i32, col: u32, a: u32) {
    if c.clip.contains(x, y) {
        let i = (y * c.w + x) as usize;
        c.px[i] = lerp(c.px[i], col, a.min(256));
    }
}

fn gradient(c: &mut Canvas, r: Rect, top: u32, bottom: u32, from: i32, to: i32) {
    for y in from.max(r.y)..to.min(r.b()) {
        let k = (y - from) * 256 / (to - from).max(1);
        c.fill_rect(Rect::new(r.x, y, r.w, 1), Color::rgb(mix(top, bottom, k)));
    }
}

/// A soft disc of light: full at the middle, fading out by `radius`.
fn glow(c: &mut Canvas, cx: i32, cy: i32, radius: i32, col: u32, strength: u32) {
    let r2 = (radius * radius).max(1) as i64;
    for y in (cy - radius).max(c.clip.y)..(cy + radius).min(c.clip.b()) {
        for x in (cx - radius).max(c.clip.x)..(cx + radius).min(c.clip.r()) {
            let d = ((x - cx) as i64).pow(2) + ((y - cy) as i64).pow(2);
            if d < r2 {
                let f = 256 - (d * 256 / r2) as u32;
                px_blend(c, x, y, col, f * f / 256 * strength / 256);
            }
        }
    }
}

fn ellipse(c: &mut Canvas, cx: i32, cy: i32, rx: i32, ry: i32, col: u32) {
    for dy in -ry..=ry {
        // half-width at this row
        let t = (ry * ry - dy * dy).max(0) as i64 * (rx as i64 * rx as i64) / (ry as i64 * ry as i64).max(1);
        let hw = isqrt(t as u64) as i32;
        c.fill_rect(Rect::new(cx - hw, cy + dy, 2 * hw + 1, 1), Color::rgb(col));
    }
}

fn isqrt(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// A ridge of land: `f` gives its height at each x (0-1024 across) in
/// per mille of the height, times 256 (so edges fall between pixels).
fn ridge(c: &mut Canvas, r: Rect, col: u32, f: impl Fn(i32) -> i32) {
    for x in r.x..r.r() {
        let u = ((x - r.x) as i64 * 1024 / r.w.max(1) as i64) as i32;
        let ypm = f(u);
        let yq8 = r.y * 256 + (ypm as i64 * r.h as i64 / 1000) as i32;
        c.vspan_aa(x, yq8, r.b(), Color::rgb(col));
    }
}

/// A wave of `amp` (per mille), ×256.
fn sine(u: i32, amp: i32, freq: i32, phase: i32) -> i32 {
    amp * sin_q14(u * freq + phase) / 64
}

/// A plain value in ridge units.
const fn q(v: i32) -> i32 {
    v * 256
}

/// A tiny pseudo-random sequence (stars, grass), the same every time.
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 >> 8
    }
}

fn sun(c: &mut Canvas, r: Rect, l: &Light, x_pm: i32, horizon_pm: i32, size_pm: i32) {
    if l.height < -100 {
        return;
    }
    let horizon = r.y + r.h * horizon_pm / 1000;
    let cy = horizon - (r.h * horizon_pm / 1000) * l.height.max(-100) / 1300;
    let cx = r.x + r.w * x_pm / 1000;
    let rad = r.w.min(r.h * 2) * size_pm / 1000;
    glow(c, cx, cy, rad * 3, l.sun, if l.night { 60 } else { 110 });
    let old = c.set_clip(Rect::new(r.x, r.y, r.w, (horizon - r.y).max(0)).intersect(&c.clip));
    c.fill_circle(cx, cy, rad, Color::rgb(l.sun));
    c.set_clip(old);
}

fn stars(c: &mut Canvas, r: Rect, l: &Light, until_pm: i32) {
    if !l.night {
        return;
    }
    let mut g = Lcg(0x4879_7465);
    let n = (r.w * r.h / 2500).clamp(40, 900);
    for _ in 0..n {
        let x = r.x + (g.next() % r.w.max(1) as u32) as i32;
        let y = r.y + (g.next() % (r.h * until_pm / 1000).max(1) as u32) as i32;
        let a = 60 + g.next() % 180;
        px_blend(c, x, y, 0xF4F1FF, a * (256 - l.light.clamp(0, 256) as u32) / 256);
    }
}

/// Draw a scene into `r` (physical pixels). `minutes` (since midnight)
/// makes it follow the time of day; `tall` is a phone's shape.
pub fn scene(c: &mut Canvas, r: Rect, s: Scene, t: &Theme, minutes: Option<u32>, tall: bool) {
    let old = c.set_clip(r.intersect(&c.clip));
    let l = light_for(t, minutes);
    match s {
        Scene::Dune => {
            let (top, bottom) = if minutes.is_some() { (l.top, l.horizon) } else { (t.sky.0 & 0xFF_FFFF, t.sky2.0 & 0xFF_FFFF) };
            gradient(c, r, top, bottom, r.y, r.b());
            stars(c, r, &l, 500);
            // the sun where it always was, unless it's night
            let (sx, sy, sr) = if tall { (r.x + r.w * 80 / 100, r.y + r.h * 18 / 100, r.w * 18 / 100) } else { (r.x + r.w * 86 / 100, r.y + r.h * 29 / 100, r.w * 10 / 100) };
            let sun_c = if minutes.is_some() { l.sun } else { t.sun.0 & 0xFF_FFFF };
            if minutes.is_none() || l.height > -150 {
                let drop = if minutes.is_some() { (450 - l.height).max(0) * r.h / 2600 } else { 0 };
                c.fill_circle(sx, sy + drop, sr, Color::rgb(sun_c));
            }
            let layers: [(u32, i32, i32, i32, i32, i32, i32, i32); 3] = if tall {
                [(t.dune1.0, 715, 22, 1, 600, 10, 3, 100), (t.dune2.0, 812, 28, 1, 150, 8, 2, 400), (t.dune3.0, 895, 25, 1, 900, 6, 3, 200)]
            } else {
                [(t.dune1.0, 640, 22, 1, 700, 8, 3, 100), (t.dune2.0, 770, 30, 1, 100, 10, 2, 300), (t.dune3.0, 890, 26, 1, 850, 8, 3, 500)]
            };
            for (col, base, a1, f1, p1, a2, f2, p2) in layers {
                let col = if minutes.is_some() { lit(col & 0xFF_FFFF, &l) } else { col & 0xFF_FFFF };
                ridge(c, r, col, |u| q(base) + sine(u, a1, f1, p1) + sine(u, a2, f2, p2));
            }
        }
        Scene::Lagoon => {
            let hz = 580;
            let horizon = r.y + r.h * hz / 1000;
            gradient(c, r, l.top, l.horizon, r.y, horizon);
            stars(c, r, &l, 450);
            sun(c, r, &l, 620, hz, 60);
            // the sea: darker and bluer towards the shore
            let sea_far = mix(l.horizon, lit(0x2F6F8F, &l), 150);
            let sea_near = lit(0x123A52, &l);
            gradient(c, r, sea_far, sea_near, horizon, r.b());
            // the sun's path on the water: short bright strokes
            let mut g = Lcg(7);
            let sx = r.x + r.w * 620 / 1000;
            let strength = if l.height > -100 { 200 } else { 60 };
            for y in horizon + 2..r.b() {
                if g.next() % 3 != 0 {
                    continue;
                }
                let depth = (y - horizon) as u32;
                let spread = 8 + depth as i32 / 3;
                let x = sx + (g.next() % (2 * spread as u32 + 1)) as i32 - spread;
                let len = 6 + (g.next() % 22) as i32;
                for k in 0..len {
                    px_blend(c, x + k, y, if l.height > -100 { l.sun } else { 0xDDE6F5 }, strength * (256 - (depth * 256 / (r.b() - horizon).max(1) as u32).min(255)) / 256);
                }
            }
            // an island on the left
            let island = lit(0x2E4A3A, &l);
            let (ix0, ix1) = (r.x + r.w * 6 / 100, r.x + r.w * 34 / 100);
            for x in ix0..ix1 {
                let u = (x - ix0) * 1024 / (ix1 - ix0).max(1);
                let hgt = sin_q14(u / 2) as i64 * (r.h as i64 * 55 / 1000) / 16384 + (sine(u, r.h * 8 / 1000, 5, 100) / 256) as i64;
                c.vspan_aa(x, (horizon - hgt as i32) * 256, horizon + 1, Color::rgb(island));
            }
        }
        Scene::Aurora => {
            // a northern sky: always dark enough for the lights
            let top = if l.night { 0x050A18 } else { mix(l.top, 0x0B1633, 150) };
            let bottom = if l.night { 0x10304A } else { mix(l.horizon, 0x1D3F5F, 140) };
            gradient(c, r, top, bottom, r.y, r.b());
            let night = Light { night: true, light: 60, ..l };
            stars(c, r, &night, 700);
            // curtains of light
            for (col, base, amp, freq, phase, len) in [(0x3CE8A8u32, 300, 60, 3, 100, 260), (0x5AC8F0, 360, 45, 2, 700, 200), (0xB06CF0, 250, 40, 4, 400, 150)] {
                for x in r.x..r.r() {
                    let u = (x - r.x) * 1024 / r.w.max(1);
                    let top_y = r.y + r.h * (base + (sine(u, amp, freq, phase) + sine(u, amp / 3, freq * 5, phase * 3)) / 256) / 1000;
                    let n = r.h * len / 1000;
                    // brighter in folds
                    let fold = (140 + sine(u, 110, freq * 7, phase) / 256).clamp(20, 256) as u32;
                    for k in 0..n {
                        let a = fold * (n - k) as u32 / n as u32 * if k < 6 { k as u32 + 1 } else { 6 } / 6;
                        px_blend(c, x, top_y + k, col, a * 150 / 256);
                    }
                }
            }
            // mountains, with a little snow light
            ridge(c, r, 0x1A2438, |u| q(720) + sine(u, 70, 3, 200) + sine(u, 25, 11, 50));
            ridge(c, r, 0x0C121E, |u| q(820) + sine(u, 60, 2, 600) + sine(u, 20, 9, 300));
        }
        Scene::Hills => {
            let hz = 620;
            let sky_top = if minutes.is_none() && !l.night { 0x9CC7EA } else { l.top };
            let sky_bot = if minutes.is_none() && !l.night { 0xE6F0F4 } else { l.horizon };
            gradient(c, r, sky_top, sky_bot, r.y, r.y + r.h * hz / 1000);
            gradient(c, r, sky_bot, sky_bot, r.y + r.h * hz / 1000, r.b());
            stars(c, r, &l, 550);
            sun(c, r, &l, 250, hz, 55);
            for (col, base, a1, f1, p1) in [(0xA9C98Au32, 560, 40, 2, 300), (0x7FAF6B, 650, 45, 3, 800), (0x4F7942, 760, 50, 2, 150), (0x2F5530, 870, 40, 3, 600)] {
                ridge(c, r, lit(col, &l), |u| q(base) + sine(u, a1, f1, p1) + sine(u, a1 / 4, f1 * 4, p1 * 2));
            }
        }
        Scene::Mesa => {
            let hz = 640;
            let (top, bot) = if minutes.is_none() && !l.night { (0xF0C9A0, 0xF7E3CC) } else { (l.top, l.horizon) };
            gradient(c, r, top, bot, r.y, r.y + r.h * hz / 1000);
            gradient(c, r, bot, bot, r.y + r.h * hz / 1000, r.b());
            stars(c, r, &l, 500);
            sun(c, r, &l, 760, hz, 70);
            // flat-topped cliffs: plateaus with steep sides
            let mesa = |u: i32, plateaus: &[(i32, i32, i32)], ground: i32| -> i32 {
                let mut y = ground;
                for &(a, b, top) in plateaus {
                    // smooth steep edges 12 units wide
                    let e = 12;
                    let k = if u < a - e || u > b + e { 0 } else if u < a { (u - a + e) * 256 / e } else if u > b { (b + e - u) * 256 / e } else { 256 };
                    y = y.min(ground - (ground - top) * k / 256);
                }
                y
            };
            ridge(c, r, lit(0xE0A57A, &l), |u| q(mesa(u, &[(80, 300, 470), (620, 900, 430)], 700)));
            ridge(c, r, lit(0xB86A45, &l), |u| q(mesa(u, &[(300, 560, 560), (880, 1010, 600)], 760)) + sine(u, 4, 17, 0));
            ridge(c, r, lit(0x7A3B2E, &l), |u| q(820) + sine(u, 15, 3, 400) + sine(u, 5, 13, 100));
            ridge(c, r, lit(0x5A2A22, &l), |u| q(920) + sine(u, 10, 2, 800));
        }
        Scene::Bloom => {
            let (a, b) = if t.dark || l.night { (0x1B1630, 0x2A1F3D) } else { (0xFBF1E6, 0xF3E3F0) };
            gradient(c, r, a, b, r.y, r.b());
            let dim = if t.dark || l.night { 150 } else { 230 };
            for (x, y, rad, col) in [(200, 250, 520, 0xF3A683u32), (800, 200, 460, 0xB39DDB), (620, 780, 560, 0x7FD1C7), (120, 900, 380, 0xF6D365), (950, 700, 420, 0xF78FB3)] {
                glow(c, r.x + r.w * x / 1000, r.y + r.h * y / 1000, r.w.max(r.h) * rad / 1000, col, dim);
            }
        }
        Scene::Harmattan => {
            // dust in the air: a pale, warm, flat sky and a sun you can look at
            let hz = 700;
            let dusty = |col: u32| mix(col, 0xD9B99A, if l.night { 60 } else { 150 });
            let (top, bot) = if minutes.is_none() && !l.night { (0xE8D2B8, 0xF2DCC0) } else { (dusty(l.top), dusty(l.horizon)) };
            gradient(c, r, top, bot, r.y, r.y + r.h * hz / 1000);
            stars(c, r, &l, 400);
            if l.height > -100 {
                let horizon = r.y + r.h * hz / 1000;
                let cy = horizon - r.h * hz / 1000 * l.height.max(-50) / 1600;
                let (cx, rad) = (r.x + r.w * 330 / 1000, r.w.min(r.h * 2) * 60 / 1000);
                glow(c, cx, cy, rad * 4, 0xF6D2A0, 60);
                c.fill_circle(cx, cy, rad, Color::rgb(mix(dusty(l.sun), 0xFFF3E0, 90)));
            }
            // savanna
            ridge(c, r, lit(0xC9A06A, &l), |u| q(690) + sine(u, 6, 3, 200));
            ridge(c, r, lit(0xA87C4A, &l), |u| q(760) + sine(u, 10, 2, 700));
            ridge(c, r, lit(0x7E5A35, &l), |u| q(880) + sine(u, 12, 3, 100));
            // acacias: a leaning trunk, a wide flat crown
            let tree = lit(0x3A2A1E, &l);
            for (x, base, size) in [(620, 745, 100), (820, 770, 70), (180, 780, 55), (930, 720, 35)] {
                let (bx, by) = (r.x + r.w * x / 1000, r.y + r.h * base / 1000);
                let s = r.h * size / 1000;
                for k in 0..s {
                    let lean = k * s / 5 / s.max(1);
                    c.fill_rect(Rect::new(bx + lean - s / 30, by - k, (s / 15).max(2), 1), Color::rgb(tree));
                }
                let (cx, cy) = (bx + s / 5, by - s);
                ellipse(c, cx, cy, s, s / 6, tree);
                ellipse(c, cx - s / 3, cy + s / 12, s * 2 / 3, s / 8, tree);
            }
            // birds
            let bird = lit(0x4A3A30, &l);
            for (x, y, w) in [(450, 300, 10), (480, 320, 8), (510, 290, 9)] {
                let (bx, by, w) = (r.x + r.w * x / 1000, r.y + r.h * y / 1000, r.w * w / 1000);
                for k in 0..w {
                    let d = (w - k) / 3;
                    px_blend(c, bx - k, by - d, bird, 220);
                    px_blend(c, bx + k, by - d, bird, 220);
                }
            }
        }
    }
    c.set_clip(old);
}

/// Draw any wallpaper. `picture` is the decoded picture for `Wall::Picture`
/// (pixels, width, height).
pub fn paint(c: &mut Canvas, r: Rect, w: &Wall, fit: Fit, t: &Theme, minutes: Option<u32>, tall: bool, picture: Option<(&[u32], usize, usize)>) {
    match w {
        Wall::Scene(s) => scene(c, r, *s, t, minutes, tall),
        Wall::Solid(col) => c.fill_rect(r, Color::rgb(*col)),
        Wall::Gradient(a, b) => {
            // diagonal: top-left to bottom-right
            for y in r.y..r.b() {
                for x in r.x..r.r() {
                    let k = ((x - r.x) * 256 / r.w.max(1) + (y - r.y) * 256 / r.h.max(1)) / 2;
                    let i = (y * c.w + x) as usize;
                    if i < c.px.len() {
                        c.px[i] = mix(*a, *b, k);
                    }
                }
            }
        }
        Wall::Picture(_) | Wall::Photo(_) => match picture {
            Some((px, pw, ph)) => {
                // photographs always fill, around their own focus
                let img = match w {
                    // (off-centre only on a tall screen, which sees a narrow slice)
                    Wall::Photo(ph_) => {
                        let focus = if tall || r.h > r.w { ph_.focus() } else { 500 };
                        personal::compose_at(px, pw, ph, r.w as usize, r.h as usize, Fit::Fill, focus)
                    }
                    _ => personal::compose(px, pw, ph, r.w as usize, r.h as usize, fit),
                };
                for y in 0..r.h {
                    let row = &img[(y * r.w) as usize..((y + 1) * r.w) as usize];
                    let at = ((r.y + y) * c.w + r.x) as usize;
                    c.px[at..at + r.w as usize].copy_from_slice(row);
                }
            }
            // the picture's gone: the default scene instead
            None => scene(c, r, Scene::Dune, t, minutes, tall),
        },
    }
}

// ---- the cache ----------------------------------------------------------------------

/// What a drawn wallpaper was drawn for.
#[derive(Clone, PartialEq)]
struct Key {
    wall: Wall,
    fit: Fit,
    dark: bool,
    /// time-of-day wallpapers are redrawn every ten minutes
    slot: Option<u32>,
    w: i32,
    h: i32,
    tall: bool,
}

struct Cache {
    /// desktop and lock screen
    entries: Vec<(Key, Canvas)>,
    /// decoded pictures, by path
    pictures: Vec<(String, Vec<u32>, usize, usize)>,
}

struct Cell(UnsafeCell<Option<Cache>>);
unsafe impl Sync for Cell {}
static CACHE: Cell = Cell(UnsafeCell::new(None));

fn cache() -> &'static mut Cache {
    // drawing happens on the main core only
    unsafe { (*CACHE.0.get()).get_or_insert_with(|| Cache { entries: Vec::new(), pictures: Vec::new() }) }
}

/// The photographs that come with HydatekOS.
fn photo_bytes(p: Photo) -> &'static [u8] {
    match p {
        Photo::Summit => include_bytes!("../../assets/wallpapers/summit.jpg"),
        Photo::Peak => include_bytes!("../../assets/wallpapers/peak.jpg"),
        Photo::Wave => include_bytes!("../../assets/wallpapers/wave.jpg"),
        Photo::Jetty => include_bytes!("../../assets/wallpapers/jetty.jpg"),
        Photo::Dew => include_bytes!("../../assets/wallpapers/dew.jpg"),
        Photo::Skyline => include_bytes!("../../assets/wallpapers/skyline.jpg"),
        Photo::Shade => include_bytes!("../../assets/wallpapers/shade.jpg"),
        Photo::Shore => include_bytes!("../../assets/wallpapers/shore.jpg"),
        Photo::GoldVein => include_bytes!("../../assets/wallpapers/goldvein.jpg"),
        Photo::Valley => include_bytes!("../../assets/wallpapers/valley.jpg"),
    }
}

/// The pixels behind a picture or photograph wallpaper, decoded and kept.
pub fn source(fs: &crate::fs::Vfs, wall: &Wall) -> Option<(&'static [u32], usize, usize)> {
    match wall {
        Wall::Picture(p) => picture(fs, p),
        Wall::Photo(p) => decoded(&alloc::format!("photo:{}", p.id()), || Some(photo_bytes(*p).to_vec())),
        _ => None,
    }
}

/// A picture file, decoded (kept; the largest side at most 3840).
pub fn picture(fs: &crate::fs::Vfs, path: &str) -> Option<(&'static [u32], usize, usize)> {
    decoded(path, || fs.read(path))
}

fn decoded(key: &str, load: impl FnOnce() -> Option<Vec<u8>>) -> Option<(&'static [u32], usize, usize)> {
    let path = key;
    let c = cache();
    if !c.pictures.iter().any(|p| p.0 == path) {
        let data = load()?;
        let img = crate::image::decode(&data).ok()?;
        let (mut w, mut h, mut px) = (img.w as usize, img.h as usize, img.px);
        if w.max(h) > 3840 {
            let (nw, nh) = if w > h { (3840, h * 3840 / w) } else { (w * 3840 / h, 3840) };
            px = personal::resample(&px, w, h, nw, nh);
            w = nw;
            h = nh;
        }
        if c.pictures.len() >= 3 {
            c.pictures.remove(0);
        }
        c.pictures.push((String::from(path), px, w, h));
    }
    c.pictures.iter().find(|p| p.0 == path).map(|p| (&p.1[..], p.2, p.3))
}

/// The drawn wallpaper for `wall` at w × h (physical pixels), drawing it
/// if it isn't kept already.
#[allow(clippy::too_many_arguments)]
pub fn cached(fs: &crate::fs::Vfs, wall: &Wall, fit: Fit, t: &Theme, minutes: Option<u32>, w: i32, h: i32, tall: bool) -> &'static Canvas {
    let key = Key { wall: wall.clone(), fit, dark: t.dark, slot: minutes.map(|m| m / 10), w, h, tall };
    if !tall {
        SCREEN_W.store(w, Ordering::Relaxed);
    }
    let c = cache();
    if let Some(i) = c.entries.iter().position(|e| e.0 == key) {
        return &c.entries[i].1;
    }
    let pic = source(fs, wall).map(|(px, pw, ph)| (px.to_vec(), pw, ph));
    let mut canvas = Canvas::new(w, h);
    let b = canvas.bounds();
    paint(&mut canvas, b, wall, fit, t, minutes, tall, pic.as_ref().map(|p| (&p.0[..], p.1, p.2)));
    let c = cache();
    // the desktop's and the lock screen's (and one being previewed)
    if c.entries.len() >= 3 {
        c.entries.remove(0);
    }
    c.entries.push((key, canvas));
    &c.entries.last().unwrap().1
}

/// Is a drawn wallpaper dark where things sit on it (its upper two
/// thirds)? Then what's drawn straight on it wants light ink.
pub fn is_dark(c: &Canvas) -> bool {
    let (w, h) = (c.w.max(1) as usize, (c.h as usize * 2 / 3).max(1));
    let (mut sum, mut n) = (0i64, 0i64);
    for y in (0..h).step_by((h / 48).max(1)) {
        for x in (0..w).step_by((w / 64).max(1)) {
            if let Some(p) = c.px.get(y * w + x) {
                sum += personal::luminance(*p & 0xFF_FFFF) as i64;
                n += 1;
            }
        }
    }
    // mid-grey is about 250 on this scale
    n > 0 && sum / n < 280
}

/// A small drawing of a wallpaper for Settings.
pub fn thumbnail(fs: &crate::fs::Vfs, wall: &Wall, fit: Fit, t: &Theme, minutes: Option<u32>, w: i32, h: i32) -> Canvas {
    let mut c = Canvas::new(w, h);
    let b = c.bounds();
    let pic = match wall {
        Wall::Photo(_) => source(fs, wall).map(|(px, pw, ph)| (px.to_vec(), pw, ph)),
        Wall::Picture(p) => picture(fs, p).map(|(px, pw, ph)| {
            // centred and tiled pictures keep their size on screen, so a
            // preview shrinks them as much as it shrinks the screen
            let screen = SCREEN_W.load(Ordering::Relaxed).max(w) as usize;
            if matches!(fit, Fit::Centre | Fit::Tile) && screen > w as usize {
                let (sw, sh) = ((pw * w as usize / screen).max(1), (ph * w as usize / screen).max(1));
                (personal::resample(px, pw, ph, sw, sh), sw, sh)
            } else {
                (px.to_vec(), pw, ph)
            }
        }),
        _ => None,
    };
    paint(&mut c, b, wall, fit, t, minutes, false, pic.as_ref().map(|p| (&p.0[..], p.1, p.2)));
    c
}
