//! Drawing Hyda Slides slides: the theme, shapes (any outline, rotated,
//! gradient-filled), lines and arrows, text, pictures, tables, charts,
//! footers and slide numbers, and slides part-way through their animations.
//! Everything is drawn in software into a canvas at any size.

use crate::deck::*;
use crate::deckio;
use crate::doc::{Doc, BOLD, ITALIC, STRIKE, UNDERLINE};
use crate::font::{self, Face};
use crate::gfx::{lerp, Canvas, Color, Rect};
use crate::image::Image;
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Decoded pictures and their scaled copies.
#[derive(Default)]
pub struct Pics {
    decoded: BTreeMap<usize, Option<Image>>,
    scaled: BTreeMap<(usize, i32, i32), Rc<Vec<u32>>>,
}

impl Pics {
    pub fn clear(&mut self) {
        self.decoded.clear();
        self.scaled.clear();
    }
    pub fn get(&mut self, deck: &Deck, i: usize, w: i32, h: i32) -> Option<Rc<Vec<u32>>> {
        if w <= 0 || h <= 0 || w > 8000 || h > 8000 {
            return None;
        }
        if let Some(p) = self.scaled.get(&(i, w, h)) {
            return Some(p.clone());
        }
        let img = self.decoded.entry(i).or_insert_with(|| deck.pics.get(i).and_then(|p| crate::image::decode(&p.data).ok()));
        let img = img.as_ref()?;
        let px = Rc::new(crate::gfx::scale_argb(&img.px, img.w as i32, img.h as i32, w, h));
        if self.scaled.len() > 24 {
            self.scaled.clear();
        }
        self.scaled.insert((i, w, h), px.clone());
        Some(px)
    }
}

/// How to draw a slide.
#[derive(Clone, Copy, Default)]
pub struct Opts {
    /// in the editor: empty placeholders show their prompts
    pub prompts: bool,
    /// the shape being typed in: no prompt, and drawn unrotated
    pub editing: Option<usize>,
    /// the slide's number (for the slide number)
    pub number: usize,
    /// slideshow: animation steps shown so far (None: everything)
    pub shown: Option<usize>,
    /// a shape flying in: (shape, offset down in units)
    pub flying: Option<(usize, i32)>,
}

/// Shapes with entrance animations, in the order they play.
pub fn anim_steps(s: &Slide) -> Vec<usize> {
    let mut v: Vec<usize> = (0..s.shapes.len()).filter(|&i| s.shapes[i].anim != Anim::None).collect();
    v.sort_by_key(|&i| (s.shapes[i].anim_order, i));
    v
}

// ---- rasterising --------------------------------------------------------------

pub enum Paint {
    Solid(u32),
    /// from `a` at (x0, y0) to `b` at (x0 + dx, y0 + dy), in pixels
    Grad { a: u32, b: u32, x0: i32, y0: i32, dx: i32, dy: i32 },
}

impl Paint {
    fn at(&self, x: i32, y: i32) -> u32 {
        match *self {
            Paint::Solid(c) => c,
            Paint::Grad { a, b, x0, y0, dx, dy } => {
                let len2 = (dx as i64 * dx as i64 + dy as i64 * dy as i64).max(1);
                let t = (((x - x0) as i64 * dx as i64 + (y - y0) as i64 * dy as i64) * 256 / len2).clamp(0, 256);
                mix(a, b, t as u32)
            }
        }
    }
}

/// Fill a polygon (points in 1/16 pixel), anti-aliased, non-zero winding;
/// `alpha` 0..=255.
pub fn fill_poly(c: &mut Canvas, pts: &[(i32, i32)], paint: &Paint, alpha: u32) {
    if pts.len() < 3 || alpha == 0 {
        return;
    }
    let (mut lx, mut ly, mut hx, mut hy) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for &(x, y) in pts {
        lx = lx.min(x);
        ly = ly.min(y);
        hx = hx.max(x);
        hy = hy.max(y);
    }
    let bx0 = (lx >> 4).max(c.clip.x);
    let bx1 = ((hx + 15) >> 4).min(c.clip.r());
    let by0 = (ly >> 4).max(c.clip.y);
    let by1 = ((hy + 15) >> 4).min(c.clip.b());
    if bx0 >= bx1 || by0 >= by1 {
        return;
    }
    let w = (bx1 - bx0) as usize;
    let mut cov = vec![0i32; w];
    let mut xs: Vec<(i64, i32)> = Vec::new();
    let n = pts.len();
    for py in by0..by1 {
        for v in cov.iter_mut() {
            *v = 0;
        }
        let mut any = false;
        for sub in 0..4 {
            let sy = (py * 16 + sub * 4 + 2) as i64;
            xs.clear();
            for i in 0..n {
                let (x0, y0) = (pts[i].0 as i64, pts[i].1 as i64);
                let (x1, y1) = (pts[(i + 1) % n].0 as i64, pts[(i + 1) % n].1 as i64);
                if (y0 <= sy && y1 > sy) || (y1 <= sy && y0 > sy) {
                    let x = x0 + (sy - y0) * (x1 - x0) / (y1 - y0);
                    xs.push((x, if y1 > y0 { 1 } else { -1 }));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by_key(|p| p.0);
            let mut wind = 0;
            let mut start = 0i64;
            for &(x, d) in xs.iter() {
                let was = wind;
                wind += d;
                if was == 0 && wind != 0 {
                    start = x;
                } else if was != 0 && wind == 0 {
                    // span [start, x) in 1/16 px
                    let (a, b) = (start.max(bx0 as i64 * 16), x.min(bx1 as i64 * 16));
                    if a < b {
                        any = true;
                        let mut p = a >> 4;
                        while p * 16 < b {
                            let o = b.min((p + 1) * 16) - a.max(p * 16);
                            cov[(p - bx0 as i64) as usize] += o as i32;
                            p += 1;
                        }
                    }
                }
            }
        }
        if !any {
            continue;
        }
        let row = (py * c.w) as usize;
        for (i, &v) in cov.iter().enumerate() {
            if v <= 0 {
                continue;
            }
            let a = (v.min(64) as u32 * alpha / 64) as u32;
            let x = bx0 + i as i32;
            let col = paint.at(x, py);
            let d = &mut c.px[row + x as usize];
            *d = if a >= 255 { col } else { lerp(*d, col, a + (a >> 7)) };
        }
    }
}

pub fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// A thick segment (1/16 px coordinates, width in 1/16 px) as a polygon.
pub fn segment(a: (i32, i32), b: (i32, i32), w: i32) -> Vec<(i32, i32)> {
    let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
    let len = isqrt(dx * dx + dy * dy).max(1);
    let (nx, ny) = ((-dy * w as i64 / 2 / len) as i32, (dx * w as i64 / 2 / len) as i32);
    vec![(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)]
}

/// Stroke a closed outline.
pub fn stroke_poly(c: &mut Canvas, pts: &[(i32, i32)], w16: i32, col: u32) {
    let n = pts.len();
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        fill_poly(c, &segment(a, b, w16), &Paint::Solid(col), 255);
        // fill the joint
        let h = w16 / 2;
        fill_poly(c, &[(a.0 - h, a.1 - h), (a.0 + h, a.1 - h), (a.0 + h, a.1 + h), (a.0 - h, a.1 + h)], &Paint::Solid(col), 255);
    }
}

/// A filled ellipse in `r` (pixels), for theme decorations.
pub fn ellipse(c: &mut Canvas, r: Rect, col: Color) {
    let pts: Vec<(i32, i32)> = outline(Kind::Ellipse, Geom::Rect, r.w.max(1), r.h.max(1)).iter().map(|&(x, y)| (r.x * 16 + x, r.y * 16 + y)).collect();
    fill_poly(c, &pts, &Paint::Solid(col.0 & 0xFFFFFF), 255);
}

/// Copy `src` into `dst` at (x, y), inside `dst`'s clip.
pub fn blit(dst: &mut Canvas, src: &Canvas, x: i32, y: i32) {
    let r = Rect::new(x, y, src.w, src.h).intersect(&dst.clip);
    for yy in r.y..r.b() {
        let a = (yy * dst.w + r.x) as usize;
        let b = ((yy - y) * src.w + (r.x - x)) as usize;
        dst.px[a..a + r.w as usize].copy_from_slice(&src.px[b..b + r.w as usize]);
    }
}

/// Blend two whole pixels `t`/256 of the way.
pub fn mixp(a: u32, b: u32, t: u32) -> u32 {
    lerp(a, b, t.min(256))
}

/// Draw something rotated by `deg` about the centre of `r` (pixels): `f`
/// draws it unrotated into a canvas the size of `r`, given the offset of
/// `r`'s corner. The background under it is carried through.
fn draw_rotated(c: &mut Canvas, r: Rect, deg: i32, f: impl FnOnce(&mut Canvas, i32, i32)) {
    if r.w <= 0 || r.h <= 0 || r.w > 6000 || r.h > 6000 {
        return;
    }
    let mut tmp = Canvas::new(r.w, r.h);
    let (cx, cy) = (r.x * 16 + r.w * 8, r.y * 16 + r.h * 8);
    let clip = c.bounds();
    // what's under the shape, as the shape sees it
    for v in 0..r.h {
        for u in 0..r.w {
            let (x, y) = rotate((r.x + u) * 16 + 8, (r.y + v) * 16 + 8, cx, cy, deg);
            let (x, y) = ((x >> 4).clamp(clip.x, clip.r() - 1), (y >> 4).clamp(clip.y, clip.b() - 1));
            tmp.px[(v * r.w + u) as usize] = c.px[(y * c.w + x) as usize];
        }
    }
    f(&mut tmp, -r.x, -r.y);
    // back, rotated: every pixel of the rotated box
    let corners = [(r.x, r.y), (r.r(), r.y), (r.r(), r.b()), (r.x, r.b())].map(|(x, y)| rotate(x * 16, y * 16, cx, cy, deg));
    let (mut lx, mut ly, mut hx, mut hy) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for (x, y) in corners {
        lx = lx.min(x >> 4);
        ly = ly.min(y >> 4);
        hx = hx.max((x >> 4) + 1);
        hy = hy.max((y >> 4) + 1);
    }
    let cl = c.clip;
    for y in ly.max(cl.y)..hy.min(cl.b()) {
        for x in lx.max(cl.x)..hx.min(cl.r()) {
            let (u, v) = rotate(x * 16 + 8, y * 16 + 8, cx, cy, -deg);
            let (u, v) = (u - r.x * 16 - 8, v - r.y * 16 - 8);
            if u < -8 || v < -8 || u >= r.w * 16 - 8 || v >= r.h * 16 - 8 {
                continue;
            }
            // bilinear
            let (u0, v0) = ((u >> 4).clamp(0, r.w - 1), (v >> 4).clamp(0, r.h - 1));
            let (u1, v1) = ((u0 + 1).min(r.w - 1), (v0 + 1).min(r.h - 1));
            let (fu, fv) = ((u & 15) as u32 * 16, (v & 15) as u32 * 16);
            let p = |a: i32, b: i32| tmp.px[(b * r.w + a) as usize];
            let top = lerp(p(u0, v0), p(u1, v0), fu);
            let bot = lerp(p(u0, v1), p(u1, v1), fu);
            c.px[(y * c.w + x) as usize] = lerp(top, bot, fv);
        }
    }
}

// ---- slides ---------------------------------------------------------------------

/// Slide units to pixels.
#[derive(Clone, Copy)]
struct K {
    x0: i32,
    y0: i32,
    num: i64,
    den: i64,
}

impl K {
    fn s(&self, u: i32) -> i32 {
        (u as i64 * self.num / self.den) as i32
    }
    fn x(&self, u: i32) -> i32 {
        self.x0 + self.s(u)
    }
    fn y(&self, u: i32) -> i32 {
        self.y0 + self.s(u)
    }
    /// a point in slide units to 1/16 px
    fn p16(&self, x16: i32, y16: i32) -> (i32, i32) {
        (self.x0 * 16 + (x16 as i64 * self.num / self.den) as i32, self.y0 * 16 + (y16 as i64 * self.num / self.den) as i32)
    }
    fn rect(&self, x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect::new(self.x(x), self.y(y), self.x(x + w) - self.x(x), self.y(y + h) - self.y(y))
    }
    /// a font size in points to pixels
    fn font(&self, pt: i32) -> i32 {
        ((pt * 4 / 3) as i64 * self.num / self.den).max(6) as i32
    }
}

/// Draw a slide `wpx` pixels wide at (x0, y0).
pub fn draw_slide(c: &mut Canvas, x0: i32, y0: i32, wpx: i32, deck: &Deck, slide: &Slide, pics: &mut Pics, o: &Opts) {
    let k = K { x0, y0, num: wpx as i64, den: deck.w.max(1) as i64 };
    let hpx = k.s(deck.h);
    let old = c.set_clip(c.clip.intersect(&Rect::new(x0, y0, wpx, hpx)));
    let t = &deck.theme;
    let bg = slide.bg.unwrap_or(t.bg);
    match slide.bg_grad {
        Some((to, a)) => {
            let full = [(x0 * 16, y0 * 16), ((x0 + wpx) * 16, y0 * 16), ((x0 + wpx) * 16, (y0 + hpx) * 16), (x0 * 16, (y0 + hpx) * 16)];
            let paint = grad_paint(bg, to, a, Rect::new(x0, y0, wpx, hpx));
            fill_poly(c, &full, &paint, 255);
        }
        None => c.fill_rect(Rect::new(x0, y0, wpx, hpx), Color::rgb(bg)),
    }
    if slide.bg.is_none() && slide.bg_grad.is_none() {
        for dc in &t.deco {
            let (x, y, w, h) = dc.place(deck.w, deck.h);
            let r = k.rect(x, y, w, h);
            if dc.ellipse {
                ellipse(c, r, Color::rgb(dc.color));
            } else {
                c.fill_rect(r, Color::rgb(dc.color));
            }
        }
    }
    // animated shapes not yet shown stay hidden
    let steps = anim_steps(slide);
    for (si, sh) in slide.shapes.iter().enumerate() {
        let mut dy = 0;
        if let Some(shown) = o.shown {
            if let Some(pos) = steps.iter().position(|&i| i == si) {
                match o.flying {
                    Some((f, d)) if f == si => dy = d,
                    _ if pos >= shown => continue,
                    _ => {}
                }
            }
        }
        let straight = o.editing == Some(si);
        let prompt = o.prompts && o.editing != Some(si);
        if dy != 0 {
            let mut moved = sh.clone();
            moved.y += dy;
            draw_shape(c, k, deck, &moved, pics, straight, prompt);
        } else {
            draw_shape(c, k, deck, sh, pics, straight, prompt);
        }
    }
    if shows_footer(deck, slide) {
        let ((fx, fy, fw, fh), (nx, ny, nw, nh)) = footer_rects(deck.w, deck.h);
        let col = Color::rgb(footer_color(t));
        let px = k.font(12);
        if !deck.footer.is_empty() {
            let w = font::measure(Face::Regular, px, &deck.footer);
            let r = k.rect(fx, fy, fw, fh);
            font::draw(c, r.x + (r.w - w) / 2, r.y + (r.h + px * 7 / 10) / 2, Face::Regular, px, &deck.footer, col);
        }
        if deck.numbers {
            let s = alloc::format!("{}", o.number);
            let w = font::measure(Face::Regular, px, &s);
            let r = k.rect(nx, ny, nw, nh);
            font::draw(c, r.r() - w, r.y + (r.h + px * 7 / 10) / 2, Face::Regular, px, &s, col);
        }
    }
    c.set_clip(old);
}

fn grad_paint(from: u32, to: u32, deg: i32, r: Rect) -> Paint {
    let (cs, sn) = (cos_deg(deg) as i64, sin_deg(deg) as i64);
    // the box's extent along the direction
    let ext = ((r.w as i64 * cs.abs() + r.h as i64 * sn.abs()) >> 14).max(1);
    let (dx, dy) = ((ext * cs >> 14) as i32, (ext * sn >> 14) as i32);
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    Paint::Grad { a: from, b: to, x0: cx - dx / 2, y0: cy - dy / 2, dx, dy }
}

/// A shape's outline in 1/16 px, flipped and rotated as it's drawn.
fn shape_points(k: K, sh: &Shape, rot: i32) -> Vec<(i32, i32)> {
    let (w16, h16) = (sh.w.max(1) * 16, sh.h.max(1) * 16);
    let (cx, cy) = (sh.x * 16 + w16 / 2, sh.y * 16 + h16 / 2);
    outline(sh.kind, sh.geom, sh.w, sh.h)
        .into_iter()
        .map(|(x, y)| {
            let x = if sh.flip_h { w16 - x } else { x };
            let y = if sh.flip_v { h16 - y } else { y };
            let (x, y) = rotate(sh.x * 16 + x, sh.y * 16 + y, cx, cy, rot);
            k.p16(x, y)
        })
        .collect()
}

fn draw_shape(c: &mut Canvas, k: K, deck: &Deck, sh: &Shape, pics: &mut Pics, straight: bool, prompt: bool) {
    let t = &deck.theme;
    let rot = if straight { 0 } else { sh.rot };
    let r = k.rect(sh.x, sh.y, sh.w, sh.h);
    match sh.kind {
        Kind::Line => {
            draw_line(c, k, deck, sh);
            return;
        }
        Kind::Picture => {
            let draw = |c: &mut Canvas, dx: i32, dy: i32, pics: &mut Pics| {
                let rr = Rect::new(r.x + dx, r.y + dy, r.w, r.h);
                if let Some(px) = sh.pic.and_then(|p| pics.get(deck, p, rr.w, rr.h)) {
                    c.blend_argb(&px, rr.w, rr.h, rr.x, rr.y);
                } else {
                    c.fill_rect(rr, Color::rgb(0xD8D2C8));
                }
            };
            if rot == 0 {
                draw(c, 0, 0, pics);
            } else {
                draw_rotated(c, r, rot, |tmp, dx, dy| draw(tmp, dx, dy, pics));
            }
            if let Some(l) = sh.line {
                stroke_poly(c, &shape_points(k, sh, rot), (k.s(sh.line_w) * 16).max(16), l);
            }
            return;
        }
        Kind::Table => {
            let kk = k;
            if rot == 0 {
                draw_table(c, kk, deck, sh);
            } else {
                draw_rotated(c, r, rot, |tmp, dx, dy| draw_table(tmp, K { x0: kk.x0 + dx, y0: kk.y0 + dy, ..kk }, deck, sh));
            }
            return;
        }
        Kind::Chart => {
            if let Some(ch) = &sh.chart {
                if rot == 0 {
                    draw_chart(c, r, k, ch, t);
                } else {
                    draw_rotated(c, r, rot, |tmp, _, _| draw_chart(tmp, Rect::new(0, 0, r.w, r.h), k, ch, t));
                }
            }
            return;
        }
        _ => {}
    }
    // the shape's fill and outline
    let base = match sh.kind {
        Kind::Rect | Kind::Ellipse => Some(sh.fill.unwrap_or(t.accent)),
        _ => sh.fill,
    };
    if base.is_some() || sh.line.is_some() {
        let pts = shape_points(k, sh, rot);
        if let Some(f) = base {
            let paint = match sh.grad {
                Some((to, a)) => grad_paint(f, to, a + rot, r),
                None => Paint::Solid(f),
            };
            fill_poly(c, &pts, &paint, 255);
        }
        if let Some(l) = sh.line {
            stroke_poly(c, &pts, (k.s(sh.line_w) * 16).max(16), l);
        }
    }
    // text
    let show_prompt = sh.is_empty() && prompt && sh.kind.placeholder();
    if sh.is_empty() && !show_prompt {
        return;
    }
    let dim_bg = if light(t.bg) { 0x9A938A } else { 0x8C88A0 };
    if show_prompt && rot == 0 {
        // the placeholder's outline
        let dim = Color::rgb(dim_bg);
        let mut x = r.x;
        while x < r.r() {
            c.fill_rect(Rect::new(x, r.y, 6.min(r.r() - x), 1), dim);
            c.fill_rect(Rect::new(x, r.b() - 1, 6.min(r.r() - x), 1), dim);
            x += 10;
        }
        let mut y = r.y;
        while y < r.b() {
            c.fill_rect(Rect::new(r.x, y, 1, 6.min(r.b() - y)), dim);
            c.fill_rect(Rect::new(r.r() - 1, y, 1, 6.min(r.b() - y)), dim);
            y += 10;
        }
    }
    let (body, col) = if show_prompt {
        let mut p = sh.clone();
        p.text = Doc::new();
        p.text.paras[0].align = sh.text.paras[0].align;
        p.text.paras[0].push(sh.kind.prompt(), 0);
        (p, dim_bg)
    } else {
        (sh.clone(), deckio::text_color(sh, t))
    };
    if rot == 0 {
        draw_text(c, k.x0, k.y0, k.num as i32, deck, &body, Color::rgb(col));
    } else {
        draw_rotated(c, r, rot, |tmp, dx, dy| draw_text(tmp, k.x0 + dx, k.y0 + dy, k.num as i32, deck, &body, Color::rgb(col)));
    }
}

fn draw_line(c: &mut Canvas, k: K, deck: &Deck, sh: &Shape) {
    let col = sh.line.unwrap_or(deck.theme.text);
    let ((ax, ay), (bx, by)) = line_ends(sh);
    let a = k.p16(ax * 16, ay * 16);
    let b = k.p16(bx * 16, by * 16);
    let w = (k.s(sh.line_w) * 16).max(16);
    let (dx, dy) = ((b.0 - a.0) as i64, (b.1 - a.1) as i64);
    let len = isqrt(dx * dx + dy * dy).max(1);
    let head = (w as i64 * 3).max(10 * 16).min(len / 2);
    let (ux, uy) = (dx * 1024 / len, dy * 1024 / len);
    let mut s = a;
    let mut e = b;
    let tri = |tip: (i32, i32), dir: i64| -> Vec<(i32, i32)> {
        // dir: +1 pointing along the line (end), -1 back (start)
        let (bx, by) = (tip.0 as i64 - dir * ux * head / 1024, tip.1 as i64 - dir * uy * head / 1024);
        let (nx, ny) = (-uy * head * 6 / 10 / 1024, ux * head * 6 / 10 / 1024);
        vec![tip, ((bx + nx) as i32, (by + ny) as i32), ((bx - nx) as i32, (by - ny) as i32)]
    };
    if sh.tail {
        fill_poly(c, &tri(b, 1), &Paint::Solid(col), 255);
        e = ((b.0 as i64 - ux * head * 8 / 10 / 1024) as i32, (b.1 as i64 - uy * head * 8 / 10 / 1024) as i32);
    }
    if sh.head {
        fill_poly(c, &tri(a, -1), &Paint::Solid(col), 255);
        s = ((a.0 as i64 + ux * head * 8 / 10 / 1024) as i32, (a.1 as i64 + uy * head * 8 / 10 / 1024) as i32);
    }
    fill_poly(c, &segment(s, e, w), &Paint::Solid(col), 255);
}

/// Draw a shape's text: laid out in slide units, placed glyph by glyph.
pub fn draw_text(c: &mut Canvas, x0: i32, y0: i32, wpx: i32, deck: &Deck, sh: &Shape, col: Color) {
    let tb = layout(sh);
    let (num, den) = (wpx as i64, deck.w.max(1) as i64);
    let k = |u: i64| (u * num / den) as i32;
    let mut buf = [0u8; 4];
    for l in &tb.lines {
        let p = &sh.text.paras[l.p];
        let px = k(l.px as i64).max(1);
        let base = y0 + k((sh.y + l.base) as i64);
        if let Some((bx, mark)) = &l.bullet {
            let f = face_for(sh.kind, p.fmt.first().copied().unwrap_or(0) & !(BOLD | ITALIC));
            font::draw(c, x0 + k((sh.x + bx) as i64), base, f, px, mark, col);
        }
        // pen in 1/64 unit
        let mut pen: i64 = 0;
        let left = (sh.x + l.x) as i64 * 64;
        for i in l.start..l.end {
            let ch = p.text[i];
            let f = p.fmt[i];
            let face = face_for(sh.kind, f);
            let gx = x0 + ((left + pen) * num / den / 64) as i32;
            if ch != ' ' && ch != '\t' {
                font::draw(c, gx, base, face, px, ch.encode_utf8(&mut buf), col);
            }
            pen += font::advance64(face, l.px, if ch == '\t' { ' ' } else { ch }) as i64;
            if f & (UNDERLINE | STRIKE) != 0 {
                let gx2 = x0 + ((left + pen) * num / den / 64) as i32;
                let th = (px / 14).max(1);
                if f & UNDERLINE != 0 {
                    c.fill_rect(Rect::new(gx, base + px / 9, gx2 - gx, th), col);
                }
                if f & STRIKE != 0 {
                    c.fill_rect(Rect::new(gx, base - px * 3 / 10, gx2 - gx, th), col);
                }
            }
        }
    }
}

fn draw_table(c: &mut Canvas, k: K, deck: &Deck, sh: &Shape) {
    let Some(tb) = &sh.table else { return };
    let t = &deck.theme;
    let line = Color::rgb(table_line(t));
    let mut y = sh.y;
    for r in 0..tb.nrows() {
        let (fill, text, bold) = cell_style(t, tb, r);
        let text = sh.color.filter(|_| !(tb.header && r == 0)).unwrap_or(text);
        let mut x = sh.x;
        for col in 0..tb.ncols() {
            let cr = k.rect(x, y, tb.cols[col], tb.rows[r]);
            c.fill_rect(cr, Color::rgb(fill));
            if let Some(mut cs) = sh.cell_shape(r, col) {
                if !cs.is_empty() {
                    if bold {
                        for p in cs.text.paras.iter_mut() {
                            for f in p.fmt.iter_mut() {
                                *f |= BOLD;
                            }
                        }
                    }
                    draw_text(c, k.x0, k.y0, k.num as i32, deck, &cs, Color::rgb(text));
                }
                let _ = &mut cs;
            }
            x += tb.cols[col];
        }
        y += tb.rows[r];
    }
    // the grid
    let r = k.rect(sh.x, sh.y, tb.cols.iter().sum(), tb.rows.iter().sum());
    let mut x = sh.x;
    for col in 0..=tb.ncols() {
        c.fill_rect(Rect::new(k.x(x).min(r.r() - 1), r.y, 1, r.h), line);
        if col < tb.ncols() {
            x += tb.cols[col];
        }
    }
    let mut y = sh.y;
    for row in 0..=tb.nrows() {
        c.fill_rect(Rect::new(r.x, k.y(y).min(r.b() - 1), r.w, 1), line);
        if row < tb.nrows() {
            y += tb.rows[row];
        }
    }
}

fn text_at(c: &mut Canvas, x: i32, base: i32, face: Face, px: i32, s: &str, col: u32, align: u8, maxw: i32) {
    let mut s = String::from(s);
    while !s.is_empty() && font::measure(face, px, &s) > maxw.max(8) {
        s.pop();
    }
    let w = font::measure(face, px, &s);
    let x = match align {
        1 => x - w / 2,
        2 => x - w,
        _ => x,
    };
    font::draw(c, x, base, face, px, &s, Color::rgb(col));
}

/// Draw a chart in `r` (pixels); `k` gives the text sizes.
fn draw_chart(c: &mut Canvas, r: Rect, k: K, ch: &Chart, t: &Theme) {
    let old = c.set_clip(c.clip.intersect(&r));
    let lbl = k.font(13);
    let text = t.text;
    let grid = mix(t.bg, t.text, 50);
    let mut top = r.y + 4;
    let mut bottom = r.b() - 4;
    if !ch.title.is_empty() {
        let px = k.font(18);
        text_at(c, r.x + r.w / 2, top + px, Face::Semibold, px, &ch.title, t.title, 1, r.w);
        top += px * 3 / 2 + 4;
    }
    let n = ch.cats.len();
    if ch.legend {
        let items: Vec<(String, u32)> = if ch.kind == ChartKind::Pie { ch.cats.iter().enumerate().map(|(i, c)| (c.clone(), series_color(t, i))).collect() } else { ch.series.iter().enumerate().map(|(i, s)| (s.name.clone(), series_color(t, i))).collect() };
        let sw = lbl * 7 / 10;
        let widths: Vec<i32> = items.iter().map(|(s, _)| sw + 6 + font::measure(Face::Regular, lbl, s) + lbl).collect();
        let total: i32 = widths.iter().sum();
        let mut x = r.x + (r.w - total).max(0) / 2;
        let base = bottom - lbl / 4;
        for ((s, col), w) in items.iter().zip(widths.iter()) {
            c.fill_rect(Rect::new(x, base - sw, sw, sw), Color::rgb(*col));
            font::draw(c, x + sw + 6, base, Face::Regular, lbl, s, Color::rgb(text));
            x += w;
        }
        bottom -= lbl * 2;
    }
    if n == 0 || ch.series.is_empty() {
        c.set_clip(old);
        return;
    }
    if ch.kind == ChartKind::Pie {
        let vals: Vec<f64> = ch.series[0].vals.iter().map(|v| if *v > 0.0 { *v } else { 0.0 }).collect();
        let total: f64 = vals.iter().sum();
        let rad = ((r.w).min(bottom - top) / 2 - 6).max(4);
        let (cx, cy) = (r.x + r.w / 2, top + (bottom - top) / 2);
        if total <= 0.0 {
            c.set_clip(old);
            return;
        }
        let mut a0 = -90.0f64;
        for (i, v) in vals.iter().enumerate() {
            let span = v / total * 360.0;
            if span <= 0.0 {
                continue;
            }
            let mut pts = vec![(cx * 16, cy * 16)];
            let steps = ((span / 3.0) as i32).max(2);
            for s in 0..=steps {
                let a = (a0 + span * s as f64 / steps as f64) as i32;
                pts.push((cx * 16 + (rad as i64 * 16 * cos_deg(a) as i64 >> 14) as i32, cy * 16 + (rad as i64 * 16 * sin_deg(a) as i64 >> 14) as i32));
            }
            let col = series_color(t, i);
            fill_poly(c, &pts, &Paint::Solid(col), 255);
            // a gap between slices
            let edge = |a: f64| (cx * 16 + (rad as i64 * 16 * cos_deg(a as i32) as i64 >> 14) as i32, cy * 16 + (rad as i64 * 16 * sin_deg(a as i32) as i64 >> 14) as i32);
            fill_poly(c, &segment((cx * 16, cy * 16), edge(a0), 32), &Paint::Solid(t.bg), 255);
            if span >= 14.0 {
                let mid = (a0 + span / 2.0) as i32;
                let (lx, ly) = (cx + (rad as i64 * 62 / 100 * cos_deg(mid) as i64 >> 14) as i32, cy + (rad as i64 * 62 / 100 * sin_deg(mid) as i64 >> 14) as i32);
                let pct = alloc::format!("{}%", (v / total * 100.0 + 0.5) as i64);
                text_at(c, lx, ly + lbl / 3, Face::Semibold, lbl, &pct, on(col), 1, rad);
            }
            a0 += span;
        }
        fill_poly(c, &segment((cx * 16, cy * 16), (cx * 16, (cy - rad) * 16), 32), &Paint::Solid(t.bg), 255);
        c.set_clip(old);
        return;
    }
    let (lo, hi) = ch.range();
    let (first, step, count) = nice_ticks(lo, hi);
    let last = first + step * count as f64;
    let labels: Vec<String> = (0..=count).map(|i| fmt_num(first + step * i as f64)).collect();
    let horiz = ch.kind == ChartKind::Bar;
    let val_w = labels.iter().map(|s| font::measure(Face::Regular, lbl, s)).max().unwrap_or(0);
    let cat_w = ch.cats.iter().map(|s| font::measure(Face::Regular, lbl, s)).max().unwrap_or(0).min(r.w / 3);
    let plot = if horiz { Rect::new(r.x + cat_w + 10, top + 4, r.w - cat_w - 18, bottom - top - lbl * 2 - 4) } else { Rect::new(r.x + val_w + 10, top + 4, r.w - val_w - 18, bottom - top - lbl * 2 - 8) };
    if plot.w < 10 || plot.h < 10 {
        c.set_clip(old);
        return;
    }
    let span = (last - first).max(1e-9);
    // value -> pixel along the value axis
    let vpos = |v: f64| -> i32 {
        let f = ((v - first) / span).max(-0.05).min(1.05);
        if horiz {
            plot.x + (f * plot.w as f64) as i32
        } else {
            plot.b() - (f * plot.h as f64) as i32
        }
    };
    // grid and value labels
    for (i, s) in labels.iter().enumerate() {
        let v = first + step * i as f64;
        let p = vpos(v);
        if horiz {
            c.fill_rect(Rect::new(p, plot.y, 1, plot.h), Color::rgb(grid));
            text_at(c, p, plot.b() + lbl + 4, Face::Regular, lbl, s, text, 1, plot.w);
        } else {
            c.fill_rect(Rect::new(plot.x, p, plot.w, 1), Color::rgb(grid));
            text_at(c, plot.x - 8, p + lbl / 3, Face::Regular, lbl, s, text, 2, val_w + 4);
        }
    }
    let zero = vpos(0.0f64.max(first).min(last));
    let ns = ch.series.len();
    let group = if horiz { plot.h as f64 / n as f64 } else { plot.w as f64 / n as f64 };
    // category labels
    for (i, s) in ch.cats.iter().enumerate() {
        let mid = (group * (i as f64 + 0.5)) as i32;
        if horiz {
            text_at(c, plot.x - 8, plot.y + mid + lbl / 3, Face::Regular, lbl, s, text, 2, cat_w + 4);
        } else {
            text_at(c, plot.x + mid, plot.b() + lbl + 6, Face::Regular, lbl, s, text, 1, group as i32);
        }
    }
    match ch.kind {
        ChartKind::Column | ChartKind::Bar => {
            let bw = group * 0.72 / ns as f64;
            for (si, se) in ch.series.iter().enumerate() {
                let col = series_color(t, si);
                for (i, v) in se.vals.iter().enumerate().take(n) {
                    let a = (group * i as f64 + group * 0.14 + bw * si as f64) as i32;
                    let b = (group * i as f64 + group * 0.14 + bw * (si + 1) as f64) as i32 - 1;
                    let p = vpos(*v);
                    let rr = if horiz { Rect::new(zero.min(p), plot.y + a, (p - zero).abs().max(1), (b - a).max(1)) } else { Rect::new(plot.x + a, zero.min(p), (b - a).max(1), (p - zero).abs().max(1)) };
                    c.fill_rect(rr, Color::rgb(col));
                }
            }
        }
        ChartKind::Line | ChartKind::Area => {
            let w16 = (k.s(4) * 16).max(40);
            for (si, se) in ch.series.iter().enumerate().rev() {
                let col = series_color(t, si);
                let pts: Vec<(i32, i32)> = se.vals.iter().take(n).enumerate().map(|(i, v)| ((plot.x * 16 + (group * (i as f64 + 0.5) * 16.0) as i32), vpos(*v) * 16)).collect();
                if ch.kind == ChartKind::Area && pts.len() > 1 {
                    let mut poly = pts.clone();
                    poly.push((pts[pts.len() - 1].0, zero * 16));
                    poly.push((pts[0].0, zero * 16));
                    fill_poly(c, &poly, &Paint::Solid(col), 150);
                }
                for w in pts.windows(2) {
                    fill_poly(c, &segment(w[0], w[1], w16), &Paint::Solid(col), 255);
                }
                if ch.kind == ChartKind::Line {
                    for &(x, y) in &pts {
                        let d = w16 * 3 / 4;
                        let dot: Vec<(i32, i32)> = (0..12).map(|a| (x + (d as i64 * cos_deg(a * 30) as i64 >> 14) as i32, y + (d as i64 * sin_deg(a * 30) as i64 >> 14) as i32)).collect();
                        fill_poly(c, &dot, &Paint::Solid(col), 255);
                    }
                }
            }
        }
        ChartKind::Pie => {}
    }
    // the base line
    if horiz {
        c.fill_rect(Rect::new(zero, plot.y, 1, plot.h), Color::rgb(mix(t.bg, t.text, 140)));
    } else {
        c.fill_rect(Rect::new(plot.x, zero, plot.w, 1), Color::rgb(mix(t.bg, t.text, 140)));
    }
    c.set_clip(old);
}
