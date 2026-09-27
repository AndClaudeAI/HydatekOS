//! Software rasteriser. Everything on screen is drawn by this module into a
//! 32-bit back buffer (0x00RRGGBB). Anti-aliasing uses integer supersampling
//! and cached coverage masks because the UEFI target is soft-float only.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::UnsafeCell;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn r(&self) -> i32 {
        self.x + self.w
    }
    pub fn b(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.r() && y < self.b()
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.r().min(o.r());
        let b = self.b().min(o.b());
        Rect::new(x, y, (r - x).max(0), (b - y).max(0))
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(x, y, self.r().max(o.r()) - x, self.b().max(o.b()) - y)
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    pub fn inset(&self, d: i32) -> Rect {
        Rect::new(self.x + d, self.y + d, self.w - 2 * d, self.h - 2 * d)
    }
    pub fn scale(&self, s: i32) -> Rect {
        Rect::new(self.x * s, self.y * s, self.w * s, self.h * s)
    }
}

/// ARGB colour; alpha 255 is opaque.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgb(v: u32) -> Color {
        Color(0xff00_0000 | v)
    }
    pub const fn rgba(v: u32, a: u8) -> Color {
        Color(((a as u32) << 24) | (v & 0xff_ffff))
    }
    pub fn a(self) -> u32 {
        self.0 >> 24
    }
    pub fn with_alpha(self, a: u8) -> Color {
        Color::rgba(self.0, a)
    }
    pub fn mix(self, o: Color, t: u32) -> Color {
        Color::rgb(lerp(self.0, o.0, t))
    }
}

/// Blend two 0x00RRGGBB pixels: `a` from 0 (all `dst`) to 256 (all `src`).
#[inline(always)]
pub fn lerp(dst: u32, src: u32, a: u32) -> u32 {
    // a in 0..=256
    let rb = ((src & 0xff00ff).wrapping_sub(dst & 0xff00ff)).wrapping_mul(a) >> 8;
    let g = ((src & 0x00ff00).wrapping_sub(dst & 0x00ff00)).wrapping_mul(a) >> 8;
    (((dst & 0xff00ff).wrapping_add(rb)) & 0xff00ff) | (((dst & 0x00ff00).wrapping_add(g)) & 0x00ff00)
}

#[inline(always)]
fn blend(dst: &mut u32, src: u32, a: u32) {
    if a >= 255 {
        *dst = src & 0xff_ffff;
    } else if a > 0 {
        *dst = lerp(*dst, src, a + (a >> 7));
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
    pub clip: Rect,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        Canvas { w, h, px: vec![0; (w * h) as usize], clip: Rect::new(0, 0, w, h) }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    pub fn set_clip(&mut self, r: Rect) -> Rect {
        let old = self.clip;
        self.clip = r.intersect(&self.bounds());
        old
    }

    pub fn fill_rect(&mut self, r: Rect, c: Color) {
        let r = r.intersect(&self.clip);
        if r.is_empty() || c.a() == 0 {
            return;
        }
        let a = c.a();
        for y in r.y..r.b() {
            let row = &mut self.px[(y * self.w + r.x) as usize..(y * self.w + r.r()) as usize];
            if a >= 255 {
                row.fill(c.0 & 0xff_ffff);
            } else {
                for p in row {
                    blend(p, c.0, a);
                }
            }
        }
    }

    /// Blend an 8-bit coverage mask at (x, y) in colour `c`.
    pub fn mask(&mut self, x: i32, y: i32, w: i32, h: i32, m: &[u8], c: Color) {
        let dst = Rect::new(x, y, w, h).intersect(&self.clip);
        if dst.is_empty() {
            return;
        }
        let ca = c.a();
        for yy in dst.y..dst.b() {
            let mrow = ((yy - y) * w) as usize;
            let prow = (yy * self.w) as usize;
            for xx in dst.x..dst.r() {
                let cov = m[mrow + (xx - x) as usize] as u32;
                if cov != 0 {
                    blend(&mut self.px[prow + xx as usize], c.0, (cov * ca) / 255);
                }
            }
        }
    }

    /// Anti-aliased rounded rectangle.
    pub fn fill_rrect(&mut self, r: Rect, radius: i32, c: Color) {
        let rad = radius.min(r.w / 2).min(r.h / 2).max(0);
        if rad == 0 {
            return self.fill_rect(r, c);
        }
        let m = corner_mask(rad);
        // centre band and side bands
        self.fill_rect(Rect::new(r.x, r.y + rad, r.w, r.h - 2 * rad), c);
        self.fill_rect(Rect::new(r.x + rad, r.y, r.w - 2 * rad, rad), c);
        self.fill_rect(Rect::new(r.x + rad, r.b() - rad, r.w - 2 * rad, rad), c);
        let clip = self.clip;
        let ca = c.a();
        let corners = [(r.x, r.y, false, false), (r.r() - rad, r.y, true, false), (r.x, r.b() - rad, false, true), (r.r() - rad, r.b() - rad, true, true)];
        for (cx, cy, fx, fy) in corners {
            let cr = Rect::new(cx, cy, rad, rad).intersect(&clip);
            for yy in cr.y..cr.b() {
                let my = if fy { rad - 1 - (yy - cy) } else { yy - cy };
                for xx in cr.x..cr.r() {
                    let mx = if fx { rad - 1 - (xx - cx) } else { xx - cx };
                    let cov = m[(my * rad + mx) as usize] as u32;
                    if cov != 0 {
                        blend(&mut self.px[(yy * self.w + xx) as usize], c.0, cov * ca / 255);
                    }
                }
            }
        }
    }

    /// Rounded-rectangle outline of thickness `t` (drawn by ring masks).
    pub fn stroke_rrect(&mut self, r: Rect, radius: i32, t: i32, c: Color) {
        let rad = radius.min(r.w / 2).min(r.h / 2).max(t);
        let m = ring_mask(rad, t);
        self.fill_rect(Rect::new(r.x + rad, r.y, r.w - 2 * rad, t), c);
        self.fill_rect(Rect::new(r.x + rad, r.b() - t, r.w - 2 * rad, t), c);
        self.fill_rect(Rect::new(r.x, r.y + rad, t, r.h - 2 * rad), c);
        self.fill_rect(Rect::new(r.r() - t, r.y + rad, t, r.h - 2 * rad), c);
        let clip = self.clip;
        let ca = c.a();
        let corners = [(r.x, r.y, false, false), (r.r() - rad, r.y, true, false), (r.x, r.b() - rad, false, true), (r.r() - rad, r.b() - rad, true, true)];
        for (cx, cy, fx, fy) in corners {
            let cr = Rect::new(cx, cy, rad, rad).intersect(&clip);
            for yy in cr.y..cr.b() {
                let my = if fy { rad - 1 - (yy - cy) } else { yy - cy };
                for xx in cr.x..cr.r() {
                    let mx = if fx { rad - 1 - (xx - cx) } else { xx - cx };
                    let cov = m[(my * rad + mx) as usize] as u32;
                    if cov != 0 {
                        blend(&mut self.px[(yy * self.w + xx) as usize], c.0, cov * ca / 255);
                    }
                }
            }
        }
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, r: i32, c: Color) {
        self.fill_rrect(Rect::new(cx - r, cy - r, 2 * r, 2 * r), r, c);
    }

    /// Soft drop shadow around `r` (9-slice of a cached blurred mask).
    pub fn shadow(&mut self, r: Rect, radius: i32, blur: i32, dy: i32, alpha: u8) {
        let (m, n) = shadow_mask(radius, blur);
        let c = Color::rgba(0x1a1410, alpha);
        let half = n / 2;
        let x0 = r.x - 2 * blur;
        let y0 = r.y - 2 * blur + dy;
        let x1 = r.r() + 2 * blur;
        let y1 = r.b() + 2 * blur + dy;
        let area = Rect::new(x0, y0, x1 - x0, y1 - y0).intersect(&self.clip);
        let ca = c.a();
        for yy in area.y..area.b() {
            let my = if yy - y0 < half { yy - y0 } else if y1 - 1 - yy < half { n - 1 - (y1 - 1 - yy) } else { half };
            let row = (yy * self.w) as usize;
            for xx in area.x..area.r() {
                let mx = if xx - x0 < half { xx - x0 } else if x1 - 1 - xx < half { n - 1 - (x1 - 1 - xx) } else { half };
                let cov = m[(my * n + mx) as usize] as u32;
                if cov != 0 {
                    blend(&mut self.px[row + xx as usize], c.0, cov * ca / 255);
                }
            }
        }
    }

    /// Copy `src` (same size) into self inside `r`.
    pub fn copy_from(&mut self, src: &Canvas, r: Rect) {
        let r = r.intersect(&self.bounds()).intersect(&src.bounds());
        for y in r.y..r.b() {
            let a = (y * self.w + r.x) as usize;
            let b = (y * src.w + r.x) as usize;
            self.px[a..a + r.w as usize].copy_from_slice(&src.px[b..b + r.w as usize]);
        }
    }

    /// Draw `src` scaled into `dst` (box-filtered; intended for shrinking),
    /// clipped to a rounded rectangle of `radius`.
    pub fn blit_scaled(&mut self, src: &Canvas, dst: Rect, radius: i32) {
        let rad = radius.min(dst.w / 2).min(dst.h / 2).max(0);
        let m: &[u8] = if rad > 0 { corner_mask(rad) } else { &[] };
        let d = dst.intersect(&self.clip);
        if d.is_empty() || dst.w <= 0 || dst.h <= 0 {
            return;
        }
        // 16.16 fixed-point source step
        let sx = ((src.w as i64) << 16) / dst.w as i64;
        let sy = ((src.h as i64) << 16) / dst.h as i64;
        for y in d.y..d.b() {
            let y0 = (((y - dst.y) as i64 * sy) >> 16) as i32;
            let y1 = ((((y - dst.y + 1) as i64 * sy) >> 16) as i32).clamp(y0 + 1, src.h);
            for x in d.x..d.r() {
                let x0 = (((x - dst.x) as i64 * sx) >> 16) as i32;
                let x1 = ((((x - dst.x + 1) as i64 * sx) >> 16) as i32).clamp(x0 + 1, src.w);
                let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
                for yy in y0..y1 {
                    let row = (yy * src.w) as usize;
                    for xx in x0..x1 {
                        let p = src.px[row + xx as usize];
                        r += (p >> 16) & 255;
                        g += (p >> 8) & 255;
                        b += p & 255;
                        n += 1;
                    }
                }
                let c = ((r / n) << 16) | ((g / n) << 8) | (b / n);
                // coverage inside the rounded outline
                let (lx, ly) = (x - dst.x, y - dst.y);
                let mx = if lx < rad { lx } else if lx >= dst.w - rad { dst.w - 1 - lx } else { rad };
                let my = if ly < rad { ly } else if ly >= dst.h - rad { dst.h - 1 - ly } else { rad };
                let cov = if mx < rad && my < rad { m[(my * rad + mx) as usize] as u32 } else { 255 };
                blend(&mut self.px[(y * self.w + x) as usize], c, cov);
            }
        }
    }

    /// Horizontal span with fractional coverage at both ends (for wallpaper curves).
    pub fn vspan_aa(&mut self, x: i32, y_top_q8: i32, y_bot: i32, c: Color) {
        if x < self.clip.x || x >= self.clip.r() {
            return;
        }
        let yt = y_top_q8 >> 8;
        let frac = 256 - (y_top_q8 & 255);
        let top = yt.max(self.clip.y);
        let bot = y_bot.min(self.clip.b());
        for y in top..bot {
            let i = (y * self.w + x) as usize;
            if y == yt {
                blend(&mut self.px[i], c.0, (frac as u32 * c.a()) >> 8);
            } else {
                blend(&mut self.px[i], c.0, c.a());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mask caches

struct Caches {
    corners: BTreeMap<i32, Vec<u8>>,
    rings: BTreeMap<(i32, i32), Vec<u8>>,
    shadows: BTreeMap<(i32, i32), (Vec<u8>, i32)>,
}

struct CacheCell(UnsafeCell<Option<Caches>>);
unsafe impl Sync for CacheCell {}
static CACHE: CacheCell = CacheCell(UnsafeCell::new(None));

fn caches() -> &'static mut Caches {
    unsafe {
        let c = &mut *CACHE.0.get();
        if c.is_none() {
            *c = Some(Caches { corners: BTreeMap::new(), rings: BTreeMap::new(), shadows: BTreeMap::new() });
        }
        c.as_mut().unwrap()
    }
}

/// Coverage of a quarter circle (top-left corner, centre at (r, r)).
fn corner_mask(r: i32) -> &'static [u8] {
    let c = caches();
    if !c.corners.contains_key(&r) {
        let mut m = vec![0u8; (r * r) as usize];
        let rr = (8 * r as i64) * (8 * r as i64);
        for y in 0..r {
            for x in 0..r {
                let mut n = 0;
                for sy in 0..4 {
                    let dy = (8 * r - (8 * y + 2 * sy + 1)) as i64;
                    for sx in 0..4 {
                        let dx = (8 * r - (8 * x + 2 * sx + 1)) as i64;
                        if dx * dx + dy * dy <= rr {
                            n += 1;
                        }
                    }
                }
                m[(y * r + x) as usize] = (n * 255 / 16) as u8;
            }
        }
        c.corners.insert(r, m);
    }
    c.corners.get(&r).unwrap()
}

fn ring_mask(r: i32, t: i32) -> &'static [u8] {
    let c = caches();
    if !c.rings.contains_key(&(r, t)) {
        let mut m = vec![0u8; (r * r) as usize];
        let ro = (8 * r as i64).pow(2);
        let ri = (8 * (r - t) as i64).pow(2);
        for y in 0..r {
            for x in 0..r {
                let mut n = 0;
                for sy in 0..4 {
                    let dy = (8 * r - (8 * y + 2 * sy + 1)) as i64;
                    for sx in 0..4 {
                        let dx = (8 * r - (8 * x + 2 * sx + 1)) as i64;
                        let d = dx * dx + dy * dy;
                        if d <= ro && d > ri {
                            n += 1;
                        }
                    }
                }
                m[(y * r + x) as usize] = (n * 255 / 16) as u8;
            }
        }
        c.rings.insert((r, t), m);
    }
    c.rings.get(&(r, t)).unwrap()
}

/// Blurred rounded-square mask used for 9-slice shadows. Returns (mask, size).
fn shadow_mask(radius: i32, blur: i32) -> (&'static [u8], i32) {
    let c = caches();
    if !c.shadows.contains_key(&(radius, blur)) {
        let inner = 2 * radius + 2;
        let n = inner + 4 * blur;
        let mut cv = Canvas::new(n, n);
        cv.fill_rrect(Rect::new(2 * blur, 2 * blur, inner, inner), radius, Color::rgb(0xff));
        let mut a: Vec<i32> = cv.px.iter().map(|p| (p & 0xff) as i32).collect();
        // three box-blur passes approximate a gaussian
        for _ in 0..3 {
            a = box_blur(&a, n, blur.max(1));
        }
        let m: Vec<u8> = a.iter().map(|v| (*v).clamp(0, 255) as u8).collect();
        c.shadows.insert((radius, blur), (m, n));
    }
    let (m, n) = c.shadows.get(&(radius, blur)).unwrap();
    (m, *n)
}

fn box_blur(src: &[i32], n: i32, r: i32) -> Vec<i32> {
    let d = 2 * r + 1;
    let mut tmp = vec![0i32; src.len()];
    let mut out = vec![0i32; src.len()];
    for y in 0..n {
        for x in 0..n {
            let mut s = 0;
            for k in -r..=r {
                let xx = (x + k).clamp(0, n - 1);
                s += src[(y * n + xx) as usize];
            }
            tmp[(y * n + x) as usize] = s / d;
        }
    }
    for y in 0..n {
        for x in 0..n {
            let mut s = 0;
            for k in -r..=r {
                let yy = (y + k).clamp(0, n - 1);
                s += tmp[(yy * n + x) as usize];
            }
            out[(y * n + x) as usize] = s / d;
        }
    }
    out
}

/// sin(2π·i/1024) in Q14.
pub fn sin_q14(i: i32) -> i32 {
    // quarter-wave table computed with a 5th order polynomial at first use
    let i = i.rem_euclid(1024);
    let (q, j) = (i / 256, i % 256);
    let v = |k: i32| -> i32 {
        // x in Q14 radians over [0, π/2]
        let x = (k as i64 * 25736) / 256; // π/2 = 1.5708 * 16384 = 25736
        let x2 = x * x >> 14;
        let x3 = x2 * x >> 14;
        let x5 = x3 * x2 >> 14;
        let x7 = x5 * x2 >> 14;
        (x - x3 / 6 + x5 / 120 - x7 / 5040) as i32
    };
    match q {
        0 => v(j),
        1 => v(256 - j),
        2 => -v(j),
        _ => -v(256 - j),
    }
}
