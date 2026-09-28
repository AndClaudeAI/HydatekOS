//! Hyda Slides, the Hyda Workspace presentation program: slides with titles,
//! bullets, text boxes, shapes and pictures on a theme, speaker notes, and a
//! full-screen slideshow with transitions. Presentations are saved in Hyda
//! Slides' own format (.hydp); PowerPoint (.pptx) files open for viewing and
//! editing and are exported to, but never saved over.

use super::{App, AppKind, LineEdit, HEADER};
use crate::deck::*;
use crate::deckio;
use crate::doc::{Doc, Pos, Style, BOLD, ITALIC, STRIKE, UNDERLINE};
use crate::font::{self, Face};
use crate::fs::{basename, join, parent};
use crate::gfx::{Canvas, Color, Rect};
use crate::icons::Icon;
use crate::image::Image;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const TOOLBAR: i32 = 46;
const STRIP_W: i32 = 188;
const NOTES_H: i32 = 86;
const STATUS: i32 = 28;
/// Transition length in ticks (100 per second).
const TRANS_TICKS: u64 = 40;

const C_CANVAS: u32 = 1;
const C_NOTES: u32 = 2;
const C_NEW_SLIDE: u32 = 4;
const C_LAYOUT: u32 = 5;
const C_BOLD: u32 = 6;
const C_ITALIC: u32 = 7;
const C_UNDERLINE: u32 = 8;
const C_LEFT: u32 = 9;
const C_CENTER: u32 = 10;
const C_RIGHT: u32 = 11;
const C_BULLETS: u32 = 12;
const C_NUMBERS: u32 = 13;
const C_SMALLER: u32 = 14;
const C_BIGGER: u32 = 15;
const C_TEXTBOX: u32 = 16;
const C_RECT: u32 = 17;
const C_ELLIPSE: u32 = 18;
const C_PICTURE: u32 = 19;
const C_COLOR: u32 = 20;
const C_THEME: u32 = 21;
const C_TRANS: u32 = 22;
const C_UNDO: u32 = 23;
const C_REDO: u32 = 24;
const C_PLAY: u32 = 25;
const C_PLAY_START: u32 = 26;
const C_NEW: u32 = 27;
const C_OPEN: u32 = 28;
const C_SAVE: u32 = 29;
const C_RENAME: u32 = 30;
const C_EXPORT: u32 = 31;
const C_CUT: u32 = 32;
const C_COPY: u32 = 33;
const C_PASTE: u32 = 34;
const C_DUP: u32 = 35;
const C_DELETE: u32 = 36;
const C_ALL: u32 = 37;
const C_DISMISS: u32 = 38;
const C_SHOW: u32 = 39;
const C_PREV: u32 = 40;
const C_NEXT: u32 = 41;
const C_DEL_SLIDE: u32 = 42;
const C_DUP_SLIDE: u32 = 43;
const C_SLIDE_UP: u32 = 44;
const C_SLIDE_DOWN: u32 = 45;
const C_FRONT: u32 = 46;
const C_BACK: u32 = 47;
const C_STRIP: u32 = 48;
const C_ADD_SLIDE: u32 = 49;
const C_THUMB: u32 = 100;
const C_MENU: u32 = 1000;
const C_FILE: u32 = 2000;

/// Colours offered for shapes and text.
const PALETTE: [u32; 12] = [0x1E1B2C, 0x5A5566, 0xFFFFFF, 0xF7F1E8, 0xC0622B, 0xE8573A, 0xF2B544, 0x0E5A43, 0x4FC3B8, 0x2F6FEB, 0x8E6CB5, 0xC2504F];

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    NewSlide,
    Layout,
    Theme,
    Color,
    Trans,
}

enum Overlay {
    None,
    Open(Vec<String>),
    Pictures(Vec<String>),
    Rename(LineEdit),
    Menu(MenuKind, Rect),
}

#[derive(Clone, Copy)]
struct TextEd {
    shape: usize,
    caret: Pos,
    anchor: Option<Pos>,
    /// typing since the last undo snapshot
    typing: bool,
}

#[derive(Clone, Copy)]
enum Press {
    Move { start: (i32, i32), orig: (i32, i32), moved: bool },
    Resize { handle: u8, start: (i32, i32), orig: (i32, i32, i32, i32), moved: bool },
    Text,
    /// dragging a thumbnail to reorder (moved yet?)
    Thumb(bool),
}

struct Show {
    idx: usize,
    /// the slide shown before, and when the change started
    from: Option<(Canvas, u64)>,
    /// the black "end of slideshow" screen
    end: bool,
    black: bool,
    frame: Option<(usize, i32, i32, u64, Canvas)>,
}

/// Decoded pictures and their scaled copies.
#[derive(Default)]
struct Pics {
    decoded: BTreeMap<usize, Option<Image>>,
    scaled: BTreeMap<(usize, i32, i32), Rc<Vec<u32>>>,
}

impl Pics {
    fn clear(&mut self) {
        self.decoded.clear();
        self.scaled.clear();
    }
    fn get(&mut self, deck: &Deck, i: usize, w: i32, h: i32) -> Option<Rc<Vec<u32>>> {
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

// ---- drawing slides ------------------------------------------------------------

/// A filled ellipse (or a ring `t` pixels wide), anti-aliased.
fn ellipse(c: &mut Canvas, r: Rect, col: Color, t: i32) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let span = |a8: i64, b8: i64, dy: i64| -> Option<i64> {
        if a8 <= 0 || b8 <= 0 || dy.abs() >= b8 {
            return None;
        }
        Some(a8 * isqrt(b8 * b8 - dy * dy) / b8)
    };
    let cx8 = (2 * r.x as i64 + r.w as i64) * 4;
    let cy8 = (2 * r.y as i64 + r.h as i64) * 4;
    let (a8, b8) = (r.w as i64 * 4, r.h as i64 * 4);
    let t8 = t as i64 * 8;
    let y0 = r.y.max(c.clip.y);
    let y1 = r.b().min(c.clip.b());
    let mut row = vec![0u8; r.w as usize];
    let mut cov = vec![0i32; r.w as usize];
    let add = |cov: &mut Vec<i32>, l: i64, rr: i64, sign: i32| {
        let lo = ((l >> 3) - r.x as i64).max(0);
        let hi = (((rr + 7) >> 3) - r.x as i64).min(r.w as i64);
        for px in lo..hi {
            let a = (r.x as i64 + px) * 8;
            let o = rr.min(a + 8) - l.max(a);
            if o > 0 {
                cov[px as usize] += sign * o as i32;
            }
        }
    };
    for py in y0..y1 {
        for v in cov.iter_mut() {
            *v = 0;
        }
        for sub in 0..4 {
            let dy = py as i64 * 8 + sub * 2 + 1 - cy8;
            let Some(hw) = span(a8, b8, dy) else { continue };
            add(&mut cov, cx8 - hw, cx8 + hw, 1);
            if t > 0 {
                if let Some(iw) = span(a8 - t8, b8 - t8, dy) {
                    add(&mut cov, cx8 - iw, cx8 + iw, -1);
                }
            }
        }
        for (m, &v) in row.iter_mut().zip(cov.iter()) {
            *m = (v.clamp(0, 32) * 255 / 32) as u8;
        }
        c.mask(r.x, py, r.w, 1, &row, col);
    }
}

fn isqrt(n: i64) -> i64 {
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

fn frame(c: &mut Canvas, r: Rect, t: i32, col: Color) {
    c.fill_rect(Rect::new(r.x, r.y, r.w, t), col);
    c.fill_rect(Rect::new(r.x, r.b() - t, r.w, t), col);
    c.fill_rect(Rect::new(r.x, r.y, t, r.h), col);
    c.fill_rect(Rect::new(r.r() - t, r.y, t, r.h), col);
}

/// Draw a slide `wpx` pixels wide at (x0, y0). In the editor (`prompts`),
/// empty placeholders show their prompt, except the one being typed in.
fn draw_slide(c: &mut Canvas, x0: i32, y0: i32, wpx: i32, deck: &Deck, slide: &Slide, pics: &mut Pics, prompts: Option<Option<usize>>) {
    let k = |u: i32| (u as i64 * wpx as i64 / deck.w.max(1) as i64) as i32;
    let hpx = k(deck.h);
    let old = c.set_clip(c.clip.intersect(&Rect::new(x0, y0, wpx, hpx)));
    let t = &deck.theme;
    c.fill_rect(Rect::new(x0, y0, wpx, hpx), Color::rgb(slide.bg.unwrap_or(t.bg)));
    for dc in &t.deco {
        let (x, y, w, h) = dc.place(deck.w, deck.h);
        let r = Rect::new(x0 + k(x), y0 + k(y), k(x + w) - k(x), k(y + h) - k(y));
        if dc.ellipse {
            ellipse(c, r, Color::rgb(dc.color), 0);
        } else {
            c.fill_rect(r, Color::rgb(dc.color));
        }
    }
    for (si, sh) in slide.shapes.iter().enumerate() {
        let r = Rect::new(x0 + k(sh.x), y0 + k(sh.y), k(sh.x + sh.w) - k(sh.x), k(sh.y + sh.h) - k(sh.y));
        match sh.kind {
            Kind::Picture => {
                if let Some(px) = sh.pic.and_then(|p| pics.get(deck, p, r.w, r.h)) {
                    c.blend_argb(&px, r.w, r.h, r.x, r.y);
                } else {
                    c.fill_rect(r, Color::rgb(0xD8D2C8));
                }
                continue;
            }
            Kind::Rect | Kind::Ellipse => {
                let fill = sh.fill.unwrap_or(t.accent);
                if sh.kind == Kind::Ellipse {
                    ellipse(c, r, Color::rgb(fill), 0);
                    if let Some(l) = sh.line {
                        ellipse(c, r, Color::rgb(l), (k(3)).max(1));
                    }
                } else {
                    c.fill_rect(r, Color::rgb(fill));
                    if let Some(l) = sh.line {
                        frame(c, r, k(3).max(1), Color::rgb(l));
                    }
                }
            }
            _ => {
                if let Some(f) = sh.fill {
                    c.fill_rect(r, Color::rgb(f));
                }
                if let Some(l) = sh.line {
                    frame(c, r, k(3).max(1), Color::rgb(l));
                }
            }
        }
        if sh.is_empty() {
            if let Some(ed) = prompts {
                if sh.kind.placeholder() && ed != Some(si) {
                    // the placeholder's outline and prompt
                    let dim = if light(slide.bg.unwrap_or(t.bg)) { Color::rgb(0x9A938A) } else { Color::rgb(0x8C88A0) };
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
                    let mut p = sh.clone();
                    p.text = Doc::new();
                    p.text.paras[0].align = sh.text.paras[0].align;
                    p.text.paras[0].push(sh.kind.prompt(), 0);
                    draw_text(c, x0, y0, wpx, deck, &p, dim);
                }
            }
            continue;
        }
        draw_text(c, x0, y0, wpx, deck, sh, Color::rgb(deckio::text_color(sh, t)));
    }
    c.set_clip(old);
}

/// Draw a shape's text: laid out in slide units, placed glyph by glyph.
fn draw_text(c: &mut Canvas, x0: i32, y0: i32, wpx: i32, deck: &Deck, sh: &Shape, col: Color) {
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
        let mut run_start: Option<(i32, u8)> = None;
        for i in l.start..l.end {
            let ch = p.text[i];
            let f = p.fmt[i];
            let face = face_for(sh.kind, f);
            let gx = x0 + ((left + pen) * num / den / 64) as i32;
            if run_start.map_or(true, |(_, rf)| rf != f & (UNDERLINE | STRIKE)) {
                run_start = Some((gx, f & (UNDERLINE | STRIKE)));
            }
            if ch != ' ' && ch != '\t' {
                font::draw(c, gx, base, face, px, ch.encode_utf8(&mut buf), col);
            }
            pen += font::advance64(face, l.px, if ch == '\t' { ' ' } else { ch }) as i64;
            // underline / strikethrough under this character
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

/// Copy `src` into `dst` at (x, y), inside `dst`'s clip.
fn blit(dst: &mut Canvas, src: &Canvas, x: i32, y: i32) {
    let r = Rect::new(x, y, src.w, src.h).intersect(&dst.clip);
    for yy in r.y..r.b() {
        let a = (yy * dst.w + r.x) as usize;
        let b = ((yy - y) * src.w + (r.x - x)) as usize;
        dst.px[a..a + r.w as usize].copy_from_slice(&src.px[b..b + r.w as usize]);
    }
}

// ---- the app ----------------------------------------------------------------

pub struct Slides {
    deck: Deck,
    path: String,
    dirty: bool,
    imported: bool,
    cur: usize,
    sel: Option<usize>,
    ed: Option<TextEd>,
    /// caret in the speaker notes while typing there
    notes: Option<usize>,
    notes_typing: bool,
    undo: Vec<(Deck, usize)>,
    redo: Vec<(Deck, usize)>,
    clip_shape: Option<(Shape, Option<Pic>)>,
    clip_text: String,
    overlay: Overlay,
    show: Option<Show>,
    // layout of the last frame (logical)
    canvas: Rect,
    strip: Rect,
    notes_r: Rect,
    strip_scroll: i32,
    mouse: (i32, i32),
    press: Option<Press>,
    /// bumped by every change (for the caches)
    gen: u64,
    view: Option<((u64, usize, i32, Option<usize>), Canvas)>,
    thumbs: Vec<Option<(u64, Canvas)>>,
    thumb_gen: Vec<u64>,
    pics: Pics,
    ticks: u64,
}

impl Slides {
    pub fn new() -> Slides {
        let mut s = Slides {
            deck: Deck::new(),
            path: String::new(),
            dirty: false,
            imported: false,
            cur: 0,
            sel: None,
            ed: None,
            notes: None,
            notes_typing: false,
            undo: vec![],
            redo: vec![],
            clip_shape: None,
            clip_text: String::new(),
            overlay: Overlay::None,
            show: None,
            canvas: Rect::default(),
            strip: Rect::default(),
            notes_r: Rect::default(),
            strip_scroll: 0,
            mouse: (0, 0),
            press: None,
            gen: 1,
            view: None,
            thumbs: vec![],
            thumb_gen: vec![],
            pics: Pics::default(),
            ticks: 0,
        };
        s.touch_all();
        s
    }

    // ---- state helpers --------------------------------------------------------

    fn slide(&self) -> &Slide {
        &self.deck.slides[self.cur]
    }

    fn slide_mut(&mut self) -> &mut Slide {
        let c = self.cur;
        &mut self.deck.slides[c]
    }

    fn shape(&self) -> Option<&Shape> {
        self.sel.and_then(|i| self.slide().shapes.get(i))
    }

    /// The current slide changed.
    fn touch(&mut self) {
        self.gen += 1;
        self.dirty = true;
        if self.thumb_gen.len() != self.deck.slides.len() {
            self.touch_all();
            return;
        }
        self.thumb_gen[self.cur] = self.gen;
    }

    /// Slides were added, removed, moved or restyled.
    fn touch_all(&mut self) {
        self.gen += 1;
        self.thumb_gen = vec![self.gen; self.deck.slides.len()];
        self.thumbs.clear();
        self.thumbs.resize_with(self.deck.slides.len(), || None);
    }

    fn snapshot(&mut self) {
        self.undo.push((self.deck.clone(), self.cur));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    fn undo_redo(&mut self, undo: bool) {
        self.ed = None;
        self.notes = None;
        let (from, to) = if undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        if let Some((d, cur)) = from.pop() {
            to.push((core::mem::replace(&mut self.deck, d), self.cur));
            self.cur = cur.min(self.deck.slides.len() - 1);
            self.sel = None;
            self.dirty = true;
            self.pics.clear();
            self.touch_all();
        }
    }

    fn go(&mut self, i: usize) {
        self.stop_edit();
        self.notes = None;
        self.cur = i.min(self.deck.slides.len() - 1);
        self.sel = None;
        self.scroll_to_cur();
    }

    fn scroll_to_cur(&mut self) {
        let (top, h) = (self.cur as i32 * self.thumb_step(), self.thumb_step());
        let vh = (self.strip.h - 50).max(h);
        if top < self.strip_scroll {
            self.strip_scroll = top;
        } else if top + h > self.strip_scroll + vh {
            self.strip_scroll = top + h - vh;
        }
    }

    fn thumb_step(&self) -> i32 {
        (STRIP_W - 44) * self.deck.h / self.deck.w + 18
    }

    fn stop_edit(&mut self) {
        if let Some(ed) = self.ed.take() {
            let _ = ed;
            self.touch();
        }
    }

    fn start_edit(&mut self, shape: usize, caret: Option<Pos>) {
        self.notes = None;
        let Some(sh) = self.slide().shapes.get(shape) else { return };
        if !sh.kind.has_text() {
            return;
        }
        let caret = caret.map(|c| sh.text.clamp(c)).unwrap_or(sh.text.end());
        self.sel = Some(shape);
        self.ed = Some(TextEd { shape, caret, anchor: None, typing: false });
        self.touch();
    }

    // ---- slides --------------------------------------------------------------

    fn add_slide(&mut self, l: Layout) {
        self.stop_edit();
        self.snapshot();
        let s = self.deck.new_slide(l);
        let at = (self.cur + 1).min(self.deck.slides.len());
        self.deck.slides.insert(at, s);
        self.touch_all();
        self.go(at);
        // start typing the title
        if let Some(t) = self.slide().shapes.iter().position(|s| s.kind == Kind::Title) {
            self.start_edit(t, None);
        }
    }

    fn duplicate_slide(&mut self) {
        self.stop_edit();
        self.snapshot();
        let s = self.slide().clone();
        self.deck.slides.insert(self.cur + 1, s);
        self.touch_all();
        let n = self.cur + 1;
        self.go(n);
    }

    fn delete_slide(&mut self) {
        self.stop_edit();
        self.snapshot();
        if self.deck.slides.len() == 1 {
            let s = self.deck.new_slide(Layout::Title);
            self.deck.slides[0] = s;
        } else {
            self.deck.slides.remove(self.cur);
        }
        self.deck.prune_pics();
        self.pics.clear();
        self.touch_all();
        let c = self.cur.min(self.deck.slides.len() - 1);
        self.go(c);
    }

    fn move_slide(&mut self, to: usize) {
        let to = to.min(self.deck.slides.len() - 1);
        if to == self.cur {
            return;
        }
        let s = self.deck.slides.remove(self.cur);
        self.deck.slides.insert(to, s);
        self.cur = to;
        self.dirty = true;
        self.touch_all();
        self.scroll_to_cur();
    }

    // ---- shapes ------------------------------------------------------------------

    fn add_shape(&mut self, kind: Kind) {
        self.stop_edit();
        self.snapshot();
        let (w, h) = (self.deck.w, self.deck.h);
        let n = self.slide().shapes.len() as i32 % 6;
        let sh = match kind {
            Kind::Text => {
                let mut s = Shape::new(Kind::Text, w / 2 - 260 + n * 24, h / 2 - 40 + n * 24, 520, 80);
                s.size = 24;
                s
            }
            k => Shape::new(k, w / 2 - 150 + n * 24, h / 2 - 100 + n * 24, 300, 200),
        };
        self.slide_mut().shapes.push(sh);
        let i = self.slide().shapes.len() - 1;
        self.sel = Some(i);
        self.touch();
        if kind == Kind::Text {
            self.start_edit(i, None);
        }
    }

    fn insert_picture(&mut self, path: &str, sys: &mut Sys) {
        let Some(data) = sys.fs.read(path) else { return };
        let Some(bytes) = deckio::picture_bytes(&data) else {
            sys.toast("Hyda Slides", &format!("Can't show {} (not a picture Hyda Slides reads)", basename(path)));
            return;
        };
        let Ok(img) = crate::image::decode(&bytes) else { return };
        self.stop_edit();
        self.snapshot();
        // fit in 60% of the slide, keeping its shape
        let (mw, mh) = (self.deck.w * 3 / 5, self.deck.h * 3 / 5);
        let (iw, ih) = (img.w.max(1) as i64, img.h.max(1) as i64);
        let (w, h) = if iw * mh as i64 > ih * mw as i64 { (mw, (mw as i64 * ih / iw) as i32) } else { ((mh as i64 * iw / ih) as i32, mh) };
        self.deck.pics.push(Pic { data: Rc::new(bytes) });
        let mut sh = Shape::new(Kind::Picture, (self.deck.w - w) / 2, (self.deck.h - h) / 2, w.max(8), h.max(8));
        sh.pic = Some(self.deck.pics.len() - 1);
        self.slide_mut().shapes.push(sh);
        self.sel = Some(self.slide().shapes.len() - 1);
        self.touch();
    }

    fn delete_shape(&mut self) {
        let Some(i) = self.sel else { return };
        self.stop_edit();
        self.snapshot();
        let sh = self.slide_mut().shapes.remove(i);
        // a layout placeholder comes back empty, like in other presentation programs
        if sh.kind.placeholder() && !sh.is_empty() {
            let mut e = sh.clone();
            let (st, al) = (e.text.paras[0].style, e.text.paras[0].align);
            e.text = Doc::new();
            e.text.paras[0].style = st;
            e.text.paras[0].align = al;
            self.slide_mut().shapes.insert(i, e);
        }
        self.sel = None;
        self.deck.prune_pics();
        self.pics.clear();
        self.touch();
    }

    fn restack(&mut self, front: bool) {
        let Some(i) = self.sel else { return };
        self.snapshot();
        let sh = self.slide_mut().shapes.remove(i);
        if front {
            self.slide_mut().shapes.push(sh);
            self.sel = Some(self.slide().shapes.len() - 1);
        } else {
            self.slide_mut().shapes.insert(0, sh);
            self.sel = Some(0);
        }
        if let Some(ed) = self.ed.as_mut() {
            ed.shape = self.sel.unwrap();
        }
        self.touch();
    }

    // ---- text editing ----------------------------------------------------------------

    fn ed_shape(&mut self) -> Option<(&mut Shape, &mut TextEd)> {
        let ed = self.ed.as_mut()?;
        let c = self.cur;
        let sh = self.deck.slides[c].shapes.get_mut(ed.shape)?;
        Some((sh, ed))
    }

    fn sel_range(ed: &TextEd) -> Option<(Pos, Pos)> {
        let a = ed.anchor?;
        if a == ed.caret {
            return None;
        }
        Some(if a < ed.caret { (a, ed.caret) } else { (ed.caret, a) })
    }

    /// Before an edit: a snapshot (once per typing run) and the selection deleted.
    fn begin_text_change(&mut self, typing: bool) {
        let need = self.ed.map_or(false, |e| !(typing && e.typing));
        if need {
            self.snapshot();
        }
        if let Some((sh, ed)) = self.ed_shape() {
            ed.typing = typing;
            if let Some((a, b)) = Self::sel_range(ed) {
                sh.text.delete(a, b);
                ed.caret = a;
            }
            ed.anchor = None;
        }
    }

    fn type_text(&mut self, s: &str) {
        self.begin_text_change(true);
        if let Some((sh, ed)) = self.ed_shape() {
            let f = sh.text.fmt_at(ed.caret);
            ed.caret = sh.text.insert(ed.caret, s, f);
        }
        self.touch();
    }

    fn enter(&mut self) {
        self.begin_text_change(false);
        if let Some((sh, ed)) = self.ed_shape() {
            ed.caret = sh.text.split(ed.caret);
        }
        self.touch();
    }

    fn backspace(&mut self, forward: bool) {
        let has_sel = self.ed.map_or(false, |e| Self::sel_range(&e).is_some());
        if has_sel {
            self.begin_text_change(false);
            self.touch();
            return;
        }
        // Backspace at the start of an indented or bulleted paragraph outdents first
        if !forward {
            if let Some((sh, ed)) = self.ed_shape() {
                if ed.caret.i == 0 && sh.kind != Kind::Body && sh.text.paras[ed.caret.p].style != Style::Body && ed.caret.p > 0 {
                    // fall through: join paragraphs
                } else if ed.caret.i == 0 && sh.text.paras[ed.caret.p].level > 0 {
                    let p = ed.caret.p;
                    self.snapshot();
                    if let Some((sh, _)) = self.ed_shape() {
                        sh.text.paras[p].level -= 1;
                    }
                    self.touch();
                    return;
                }
            }
        }
        self.begin_text_change(true);
        if let Some((sh, ed)) = self.ed_shape() {
            let c = ed.caret;
            if forward {
                let end = if c.i < sh.text.paras[c.p].len() { Pos::new(c.p, c.i + 1) } else if c.p + 1 < sh.text.paras.len() { Pos::new(c.p + 1, 0) } else { c };
                sh.text.delete(c, end);
            } else {
                let start = if c.i > 0 { Pos::new(c.p, c.i - 1) } else if c.p > 0 { Pos::new(c.p - 1, sh.text.paras[c.p - 1].len()) } else { c };
                sh.text.delete(start, c);
                ed.caret = start;
            }
        }
        self.touch();
    }

    fn move_caret(&mut self, k: Key, shift: bool, word: bool) {
        let Some((sh, ed)) = self.ed_shape() else { return };
        let tb = layout(sh);
        let c = ed.caret;
        let t = &sh.text;
        let is_w = |ch: char| ch.is_alphanumeric();
        let new = match k {
            Key::Left if word => {
                let mut i = c.i;
                let tx = &t.paras[c.p].text;
                while i > 0 && !is_w(tx[i - 1]) {
                    i -= 1;
                }
                while i > 0 && is_w(tx[i - 1]) {
                    i -= 1;
                }
                if c.i == 0 && c.p > 0 {
                    Pos::new(c.p - 1, t.paras[c.p - 1].len())
                } else {
                    Pos::new(c.p, i)
                }
            }
            Key::Right if word => {
                let tx = &t.paras[c.p].text;
                let mut i = c.i;
                while i < tx.len() && is_w(tx[i]) {
                    i += 1;
                }
                while i < tx.len() && !is_w(tx[i]) {
                    i += 1;
                }
                if c.i == tx.len() && c.p + 1 < t.paras.len() {
                    Pos::new(c.p + 1, 0)
                } else {
                    Pos::new(c.p, i)
                }
            }
            Key::Left => {
                if c.i > 0 {
                    Pos::new(c.p, c.i - 1)
                } else if c.p > 0 {
                    Pos::new(c.p - 1, t.paras[c.p - 1].len())
                } else {
                    c
                }
            }
            Key::Right => {
                if c.i < t.paras[c.p].len() {
                    Pos::new(c.p, c.i + 1)
                } else if c.p + 1 < t.paras.len() {
                    Pos::new(c.p + 1, 0)
                } else {
                    c
                }
            }
            Key::Up | Key::Down => {
                let (li, x) = caret_xy(sh, &tb, c);
                let target = if k == Key::Up { li.checked_sub(1) } else { Some(li + 1).filter(|&l| l < tb.lines.len()) };
                match target {
                    Some(l) => hit(sh, &tb, x, tb.lines[l].top + 1),
                    None if k == Key::Up => Pos::new(0, 0),
                    None => t.end(),
                }
            }
            Key::Home => {
                let (li, _) = caret_xy(sh, &tb, c);
                tb.lines.get(li).map(|l| Pos::new(l.p, l.start)).unwrap_or(c)
            }
            Key::End => {
                let (li, _) = caret_xy(sh, &tb, c);
                match tb.lines.get(li) {
                    Some(l) => {
                        let mut e = l.end;
                        let wrapped = tb.lines.get(li + 1).map_or(false, |n| n.p == l.p);
                        if wrapped && e > l.start && t.paras[l.p].text[e - 1] == ' ' {
                            e -= 1;
                        }
                        Pos::new(l.p, e)
                    }
                    None => c,
                }
            }
            _ => c,
        };
        if shift {
            if ed.anchor.is_none() {
                ed.anchor = Some(c);
            }
        } else {
            ed.anchor = None;
        }
        ed.caret = new;
        ed.typing = false;
    }

    /// Bold / italic / underline: on the selection, or the whole shape when it's
    /// only selected.
    fn toggle_fmt(&mut self, bit: u8) {
        let Some(i) = self.sel else { return };
        if !self.slide().shapes[i].kind.has_text() {
            return;
        }
        self.snapshot();
        let range = match self.ed {
            Some(ed) => Self::sel_range(&ed),
            None => None,
        };
        let c = self.cur;
        let sh = &mut self.deck.slides[c].shapes[i];
        let (a, b) = range.unwrap_or((Pos::new(0, 0), sh.text.end()));
        if a == b {
            // nothing selected: the next characters typed (the word at the caret)
            if let Some(ed) = self.ed {
                let (ws, we) = sh.text.word_at(ed.caret);
                if ws != we {
                    let on = !sh.text.all_have(ws, we, bit);
                    sh.text.set_fmt(ws, we, bit, on);
                }
            }
        } else {
            let on = !sh.text.all_have(a, b, bit);
            sh.text.set_fmt(a, b, bit, on);
        }
        self.touch();
    }

    /// Paragraphs the caret / selection touches (all, when the shape is only selected).
    fn para_range(&self) -> Option<(usize, usize, usize)> {
        let i = self.sel?;
        let sh = self.slide().shapes.get(i)?;
        if !sh.kind.has_text() {
            return None;
        }
        Some(match self.ed {
            Some(ed) => {
                let a = ed.anchor.unwrap_or(ed.caret);
                (i, a.p.min(ed.caret.p), a.p.max(ed.caret.p))
            }
            None => (i, 0, sh.text.paras.len() - 1),
        })
    }

    fn each_para(&mut self, f: impl Fn(&mut crate::doc::Para)) {
        let Some((i, a, b)) = self.para_range() else { return };
        self.snapshot();
        let c = self.cur;
        for p in self.deck.slides[c].shapes[i].text.paras[a..=b].iter_mut() {
            f(p);
        }
        self.touch();
    }

    fn set_list(&mut self, style: Style) {
        let Some((i, a, _)) = self.para_range() else { return };
        let on = self.slide().shapes[i].text.paras[a].style != style;
        self.each_para(move |p| p.style = if on { style } else { Style::Body });
    }

    fn indent(&mut self, out: bool) {
        self.each_para(move |p| {
            if out {
                p.level = p.level.saturating_sub(1);
            } else {
                p.level = (p.level + 1).min(4);
            }
        });
    }

    fn resize_text(&mut self, bigger: bool) {
        let Some(i) = self.sel else { return };
        self.snapshot();
        let steps = [8u16, 10, 12, 14, 16, 18, 20, 24, 28, 32, 36, 40, 44, 48, 54, 60, 72, 88, 96, 120];
        let c = self.cur;
        let sh = &mut self.deck.slides[c].shapes[i];
        sh.size = if bigger { steps.iter().copied().find(|&s| s > sh.size).unwrap_or(sh.size) } else { steps.iter().rev().copied().find(|&s| s < sh.size).unwrap_or(sh.size) };
        self.touch();
    }

    fn set_color(&mut self, col: Option<u32>) {
        let Some(i) = self.sel else {
            // no shape: the slide's background
            self.snapshot();
            self.slide_mut().bg = col;
            self.touch();
            return;
        };
        self.snapshot();
        let c = self.cur;
        let sh = &mut self.deck.slides[c].shapes[i];
        match sh.kind {
            Kind::Rect | Kind::Ellipse => sh.fill = col,
            Kind::Picture => sh.line = col,
            _ => sh.color = col,
        }
        self.touch();
    }

    fn copy(&mut self, sys: &mut Sys) {
        if let Some(ed) = self.ed {
            if let Some((a, b)) = Self::sel_range(&ed) {
                let sh = &self.slide().shapes[ed.shape];
                sys.clipboard = Doc::plain(&sh.text.slice(a, b));
                self.clip_shape = None;
            }
            return;
        }
        if let Some(sh) = self.shape() {
            let pic = sh.pic.and_then(|p| self.deck.pics.get(p).cloned());
            let text = if sh.is_empty() { String::from("(shape)") } else { sh.plain() };
            self.clip_shape = Some((sh.clone(), pic));
            self.clip_text = text.clone();
            sys.clipboard = text;
        }
    }

    fn paste(&mut self, sys: &mut Sys) {
        if self.ed.is_some() {
            let t = sys.clipboard.clone();
            if !t.is_empty() {
                self.type_text(&t);
            }
            return;
        }
        match &self.clip_shape {
            Some((sh, pic)) if sys.clipboard == self.clip_text => {
                let (mut sh, pic) = (sh.clone(), pic.clone());
                self.snapshot();
                if let Some(p) = pic {
                    self.deck.pics.push(p);
                    sh.pic = Some(self.deck.pics.len() - 1);
                }
                // pasted copies step down and right
                sh.x += 24;
                sh.y += 24;
                if sh.kind.placeholder() {
                    sh.kind = Kind::Text;
                }
                self.clip_shape.as_mut().unwrap().0 = sh.clone();
                self.slide_mut().shapes.push(sh);
                self.sel = Some(self.slide().shapes.len() - 1);
                self.touch();
            }
            _ => {
                let t = sys.clipboard.clone();
                if t.is_empty() {
                    return;
                }
                self.add_shape(Kind::Text);
                self.type_text(&t);
            }
        }
    }

    // ---- files ----------------------------------------------------------------------

    fn title(&self) -> String {
        if self.path.is_empty() {
            return String::from("Untitled presentation");
        }
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[..k]).unwrap_or(b).to_string()
    }

    fn ext(&self) -> &str {
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[k..]).unwrap_or("")
    }

    fn save(&mut self, sys: &mut Sys, quiet: bool) {
        self.stop_edit();
        if self.path.is_empty() {
            let name = self.deck.slides.iter().map(|s| s.title()).find(|t| !t.trim().is_empty()).unwrap_or_else(|| String::from("Untitled presentation"));
            let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(60).collect();
            self.path = sys.fs.unique("/home/Documents/Presentations", name.trim(), ".hydp");
        }
        let mut from = None;
        if self.imported {
            from = Some(self.ext().to_string());
            self.path = sys.fs.unique(&parent(&self.path), &self.title(), ".hydp");
            self.imported = false;
        }
        self.deck.name = self.title();
        let ok = sys.fs.write(&self.path, deckio::to_hydp(&self.deck).as_bytes());
        if ok {
            self.dirty = false;
        }
        if !quiet {
            let msg = if !ok {
                String::from("Couldn't save: the disk is read-only")
            } else if let Some(ext) = from {
                format!("Saved as {} (the {} file is unchanged)", basename(&self.path), ext)
            } else {
                format!("Saved {}", basename(&self.path))
            };
            sys.toast("Hyda Slides", &msg);
        }
    }

    fn export(&mut self, sys: &mut Sys) {
        self.stop_edit();
        let dir = if self.path.is_empty() { String::from("/home/Documents/Presentations") } else { parent(&self.path) };
        let name = self.title();
        let target = join(&dir, &format!("{}.pptx", name));
        let path = if sys.fs.exists(&target) { sys.fs.unique(&dir, &name, ".pptx") } else { target };
        let mut d = self.deck.clone();
        d.name = name;
        let ok = sys.fs.write(&path, &deckio::to_pptx(&d));
        sys.toast("Hyda Slides", &if ok { format!("Exported {}", basename(&path)) } else { String::from("Couldn't export: the disk is read-only") });
    }

    fn park(&mut self, sys: &mut Sys) {
        self.stop_edit();
        let blank = self.deck.slides.len() == 1 && self.deck.slides[0].shapes.iter().all(|s| s.is_empty() && s.kind.placeholder());
        if self.dirty && (!self.path.is_empty() || !blank) {
            self.save(sys, true);
        }
    }

    fn reset(&mut self, d: Deck) {
        self.deck = d;
        self.path.clear();
        self.dirty = false;
        self.imported = false;
        self.cur = 0;
        self.sel = None;
        self.ed = None;
        self.notes = None;
        self.undo.clear();
        self.redo.clear();
        self.strip_scroll = 0;
        self.pics.clear();
        self.view = None;
        self.touch_all();
    }

    fn load(&mut self, path: &str, sys: &mut Sys) {
        let Some(data) = sys.fs.read(path) else {
            sys.toast("Hyda Slides", &format!("Couldn't open {}", basename(path)));
            return;
        };
        let lower = path.to_ascii_lowercase();
        let parsed = if lower.ends_with(".pptx") { deckio::from_pptx(&data) } else { deckio::from_hydp(&data) };
        match parsed {
            Ok(d) => {
                self.park(sys);
                self.reset(d);
                self.path = path.to_string();
                self.imported = lower.ends_with(".pptx");
            }
            Err(why) => sys.toast("Hyda Slides", &format!("Can't open {}: {}", basename(path), why)),
        }
    }

    fn list_files(sys: &Sys, exts: &[&str], own: &str) -> Vec<String> {
        let (mut mine, mut other) = (Vec::new(), Vec::new());
        for dir in ["/home/Documents/Presentations", "/home/Documents", "/home/Pictures", "/home", "/home/Downloads", "/home/Shared"] {
            for (name, d, _) in sys.fs.list(dir) {
                let l = name.to_ascii_lowercase();
                if d {
                    continue;
                }
                if l.ends_with(own) {
                    mine.push(join(dir, &name));
                } else if exts.iter().any(|e| l.ends_with(e)) {
                    other.push(join(dir, &name));
                }
            }
        }
        mine.extend(other);
        mine
    }

    fn rename_to(&mut self, name: &str, sys: &mut Sys) {
        let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let dir = if self.path.is_empty() { String::from("/home/Documents/Presentations") } else { parent(&self.path) };
        if self.imported || self.path.is_empty() {
            self.path = sys.fs.unique(&dir, name, ".hydp");
            self.imported = false;
            self.dirty = true;
            self.save(sys, true);
            return;
        }
        let target = join(&dir, &format!("{}.hydp", name));
        if target != self.path {
            let target = if sys.fs.exists(&target) { sys.fs.unique(&dir, name, ".hydp") } else { target };
            if sys.fs.exists(&self.path) {
                sys.fs.rename(&self.path, &target);
            }
            self.path = target;
        }
        self.dirty = true;
        self.save(sys, true);
    }

    // ---- slideshow ------------------------------------------------------------------

    fn play(&mut self, from_start: bool) {
        self.stop_edit();
        self.notes = None;
        self.overlay = Overlay::None;
        let idx = if from_start { 0 } else { self.cur };
        self.show = Some(Show { idx, from: None, end: false, black: false, frame: None });
    }

    fn show_step(&mut self, forward: bool) {
        let n = self.deck.slides.len();
        let Some(sh) = self.show.as_mut() else { return };
        sh.black = false;
        if forward {
            if sh.end {
                self.show = None;
                return;
            }
            if sh.idx + 1 >= n {
                sh.end = true;
                sh.from = None;
                return;
            }
            let prev = sh.frame.take().map(|f| f.4);
            sh.idx += 1;
            if self.deck.slides[sh.idx].trans != Trans::None {
                sh.from = prev.map(|c| (c, self.ticks));
            } else {
                sh.from = None;
            }
        } else {
            if sh.end {
                sh.end = false;
                return;
            }
            if sh.idx > 0 {
                sh.idx -= 1;
                sh.from = None;
                sh.frame = None;
            }
        }
    }

    fn render_show(&mut self, ui: &mut Ui, r: Rect, inst: u32) {
        let s = ui.s;
        let full = r.scale(s);
        ui.c.fill_rect(full, Color::rgb(0));
        ui.zone(r, Action::App(inst, C_SHOW));
        let ticks = self.ticks;
        let Some(show) = self.show.as_mut() else { return };
        if show.end || show.black {
            if show.end {
                ui.text_in(Rect::new(r.x, r.y + r.h / 2 - 20, r.w, 40), Face::Regular, 15, "End of slideshow. Click or press Esc to leave.", Color::rgb(0xB8B4C4), 1);
            }
            return;
        }
        // fit the slide to the screen
        let (dw, dh) = (self.deck.w as i64, self.deck.h as i64);
        let (mut w, mut h) = (full.w as i64, full.w as i64 * dh / dw);
        if h > full.h as i64 {
            h = full.h as i64;
            w = h * dw / dh;
        }
        let (w, h) = (w as i32, h as i32);
        let (x, y) = (full.x + (full.w - w) / 2, full.y + (full.h - h) / 2);
        let idx = show.idx.min(self.deck.slides.len() - 1);
        let fresh = !matches!(&show.frame, Some((i, fw, fh, g, _)) if *i == idx && *fw == w && *fh == h && *g == self.gen);
        if fresh {
            let mut c = Canvas::new(w, h);
            draw_slide(&mut c, 0, 0, w, &self.deck, &self.deck.slides[idx], &mut self.pics, None);
            show.frame = Some((idx, w, h, self.gen, c));
        }
        let cur = &show.frame.as_ref().unwrap().4;
        match &show.from {
            Some((prev, t0)) if prev.w == w && prev.h == h && ticks < t0 + TRANS_TICKS => {
                let p = ((ticks - t0) * 256 / TRANS_TICKS) as u32;
                // ease in-out
                let e = if p < 128 { p * p / 64 } else { 256 - (256 - p) * (256 - p) / 64 };
                match self.deck.slides[idx].trans {
                    Trans::Push => {
                        let off = (h as u32 * e / 256) as i32;
                        let old = ui.c.set_clip(ui.c.clip.intersect(&Rect::new(x, y, w, h)));
                        blit(ui.c, prev, x, y - off);
                        blit(ui.c, cur, x, y + h - off);
                        ui.c.set_clip(old);
                    }
                    _ => {
                        let a = e.min(256);
                        let clip = ui.c.clip;
                        for yy in 0..h {
                            let py = y + yy;
                            if py < clip.y || py >= clip.b() {
                                continue;
                            }
                            let row = (py * ui.c.w + x) as usize;
                            let src = (yy * w) as usize;
                            for xx in 0..w as usize {
                                let (pa, pb) = (prev.px[src + xx], cur.px[src + xx]);
                                ui.c.px[row + xx] = mixp(pa, pb, a);
                            }
                        }
                    }
                }
            }
            _ => {
                show.from = None;
                blit(ui.c, cur, x, y);
            }
        }
    }

    // ---- drawing the editor ---------------------------------------------------------

    fn to_screen(&self, u: i32) -> i32 {
        (u as i64 * self.canvas.w as i64 / self.deck.w.max(1) as i64) as i32
    }

    fn to_units(&self, x: i32, y: i32) -> (i32, i32) {
        let k = |v: i32, o: i32| ((v - o) as i64 * self.deck.w as i64 / self.canvas.w.max(1) as i64) as i32;
        (k(x, self.canvas.x), k(y, self.canvas.y))
    }

    fn shape_rect(&self, sh: &Shape) -> Rect {
        let x = self.canvas.x + self.to_screen(sh.x);
        let y = self.canvas.y + self.to_screen(sh.y);
        Rect::new(x, y, self.canvas.x + self.to_screen(sh.x + sh.w) - x, self.canvas.y + self.to_screen(sh.y + sh.h) - y)
    }

    fn handles(r: Rect) -> [(i32, i32); 8] {
        let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
        [(r.x, r.y), (cx, r.y), (r.r(), r.y), (r.r(), cy), (r.r(), r.b()), (cx, r.b()), (r.x, r.b()), (r.x, cy)]
    }

    fn tool(&self, ui: &mut Ui, r: Rect, label: &str, icon: Option<Icon>, face: Face, code: u32, inst: u32, on: bool, enabled: bool) {
        let t = ui.t;
        let a = Action::App(inst, code);
        if on {
            ui.rrect(r, 8, t.accent.with_alpha(45));
        } else if ui.hot(a) && enabled {
            ui.rrect(r, 8, t.hover);
        }
        let col = if !enabled { t.text3 } else if on { t.accent } else { t.text };
        match icon {
            Some(i) => ui.icon_in(i, r, 16, col),
            None => {
                ui.text_in(r, face, 14, label, col, 1);
            }
        }
        if code == C_UNDERLINE {
            let w = ui.tw(face, 14, label);
            ui.rect(Rect::new(r.x + (r.w - w) / 2, r.y + 23, w, 1), col);
        }
        ui.zone(r, a);
    }

    fn cur_fmt(&self) -> u8 {
        let Some(sh) = self.shape() else { return 0 };
        match self.ed {
            Some(ed) => {
                let (a, b) = Self::sel_range(&ed).unwrap_or((ed.caret, ed.caret));
                if a == b {
                    sh.text.fmt_at(a)
                } else {
                    sh.text.paras[a.p].fmt.get(a.i).copied().unwrap_or(0)
                }
            }
            None => sh.text.paras.first().and_then(|p| p.fmt.first().copied()).unwrap_or(0),
        }
    }

    fn render_toolbar(&self, ui: &mut Ui, r: Rect, inst: u32, compact: bool) {
        let t = ui.t;
        let y = r.y + HEADER;
        ui.rect(Rect::new(r.x, y, r.w, TOOLBAR), t.surface);
        ui.rect(Rect::new(r.x, y + TOOLBAR - 1, r.w, 1), t.line);
        let b = |x: i32, w: i32| Rect::new(x, y + 7, w, 32);
        let sep = |ui: &mut Ui, x: i32| ui.rect(Rect::new(x, y + 12, 1, 20), t.line);
        let text_on = self.shape().map_or(false, |s| s.kind.has_text());
        let fmt = self.cur_fmt();
        let para = self.para_range().map(|(i, a, _)| self.slide().shapes[i].text.paras[a].clone());
        let mut x = r.x + 10;
        // new slide
        let nr = b(x, if compact { 34 } else { 92 });
        let na = Action::App(inst, C_NEW_SLIDE);
        ui.rrect(nr, 8, if ui.hot(na) { t.accent.mix(t.text, 20) } else { t.accent });
        if compact {
            ui.icon_in(Icon::Plus, nr, 16, t.on_accent);
        } else {
            ui.icon(Icon::Plus, nr.x + 9, nr.y + 8, 16, t.on_accent);
            ui.text(nr.x + 30, nr.y + 21, Face::Semibold, 13, "Slide", t.on_accent);
        }
        ui.zone(nr, na);
        x = nr.r() + 6;
        if !compact {
            self.tool(ui, b(x, 64), "Layout", None, Face::Medium, C_LAYOUT, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Layout, _)), true);
            x += 70;
        }
        sep(ui, x - 3);
        x += 4;
        self.tool(ui, b(x, 30), "B", None, Face::Semibold, C_BOLD, inst, text_on && fmt & BOLD != 0, text_on);
        x += 32;
        self.tool(ui, b(x, 30), "I", None, Face::Italic, C_ITALIC, inst, text_on && fmt & ITALIC != 0, text_on);
        x += 32;
        self.tool(ui, b(x, 30), "U", None, Face::Regular, C_UNDERLINE, inst, text_on && fmt & UNDERLINE != 0, text_on);
        x += 36;
        if !compact {
            sep(ui, x - 3);
            for (icon, code, al) in [(Icon::AlignLeft, C_LEFT, Align::Left), (Icon::AlignCenter, C_CENTER, Align::Center), (Icon::AlignRight, C_RIGHT, Align::Right)] {
                self.tool(ui, b(x, 30), "", Some(icon), Face::Regular, code, inst, para.as_ref().map_or(false, |p| p.align == al), text_on);
                x += 32;
            }
            x += 4;
            sep(ui, x - 3);
            self.tool(ui, b(x, 30), "", Some(Icon::ListBullet), Face::Regular, C_BULLETS, inst, para.as_ref().map_or(false, |p| p.style == Style::Bullet), text_on);
            x += 32;
            self.tool(ui, b(x, 30), "", Some(Icon::ListNumber), Face::Regular, C_NUMBERS, inst, para.as_ref().map_or(false, |p| p.style == Style::Number), text_on);
            x += 36;
            sep(ui, x - 3);
            self.tool(ui, b(x, 30), "A-", None, Face::Medium, C_SMALLER, inst, false, text_on);
            x += 32;
            self.tool(ui, b(x, 30), "A+", None, Face::Medium, C_BIGGER, inst, false, text_on);
            x += 36;
            sep(ui, x - 3);
        }
        self.tool(ui, b(x, 30), "", Some(Icon::TextBox), Face::Regular, C_TEXTBOX, inst, false, true);
        x += 32;
        self.tool(ui, b(x, 30), "", Some(Icon::Shapes), Face::Regular, C_RECT, inst, false, true);
        x += 32;
        if !compact {
            let er = b(x, 30);
            let a = Action::App(inst, C_ELLIPSE);
            if ui.hot(a) {
                ui.rrect(er, 8, t.hover);
            }
            ui.c.stroke_rrect(Rect::new(er.x + 7, er.y + 8, 16, 16).scale(ui.s), 8 * ui.s, 2 * ui.s, t.text);
            ui.zone(er, a);
            x += 32;
        }
        self.tool(ui, b(x, 30), "", Some(Icon::Image), Face::Regular, C_PICTURE, inst, false, true);
        x += 36;
        sep(ui, x - 3);
        // colour: shows the selected shape's colour
        let cr = b(x, 34);
        let ca = Action::App(inst, C_COLOR);
        if ui.hot(ca) {
            ui.rrect(cr, 8, t.hover);
        }
        let swatch = match self.shape() {
            Some(sh) if matches!(sh.kind, Kind::Rect | Kind::Ellipse) => sh.fill.unwrap_or(self.deck.theme.accent),
            Some(sh) if sh.kind == Kind::Picture => sh.line.unwrap_or(0xFFFFFF),
            Some(sh) => deckio::text_color(sh, &self.deck.theme),
            None => self.slide().bg.unwrap_or(self.deck.theme.bg),
        };
        ui.text_in(Rect::new(cr.x, cr.y - 3, cr.w, cr.h), Face::Semibold, 14, "A", t.text, 1);
        ui.rect(Rect::new(cr.x + 8, cr.y + 23, cr.w - 16, 5), Color::rgb(swatch));
        ui.stroke(Rect::new(cr.x + 8, cr.y + 23, cr.w - 16, 5), 0, 1, t.line);
        ui.zone(cr, ca);
        x += 38;
        if !compact {
            self.tool(ui, b(x, 62), "Theme", None, Face::Medium, C_THEME, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Theme, _)), true);
            x += 64;
            if x + 150 < r.r() {
                self.tool(ui, b(x, 84), "Transition", None, Face::Medium, C_TRANS, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Trans, _)), true);
                x += 88;
            }
            sep(ui, x - 3);
            if x + 70 < r.r() {
                self.tool(ui, b(x, 30), "", Some(Icon::Undo), Face::Regular, C_UNDO, inst, false, !self.undo.is_empty());
                x += 32;
                self.tool(ui, b(x, 30), "", Some(Icon::Redo), Face::Regular, C_REDO, inst, false, !self.redo.is_empty());
            }
        }
    }

    fn render_strip(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let t = ui.t;
        self.strip = area;
        ui.rect(area, t.sidebar);
        ui.rect(Rect::new(area.r() - 1, area.y, 1, area.h), t.line);
        let tw = STRIP_W - 44;
        let th = tw * self.deck.h / self.deck.w;
        let step = self.thumb_step();
        let body = Rect::new(area.x, area.y, area.w, area.h - 44);
        let max = (self.deck.slides.len() as i32 * step + 12 - body.h).max(0);
        self.strip_scroll = self.strip_scroll.clamp(0, max);
        ui.zone(body, Action::App(inst, C_STRIP));
        let old = ui.clip_in(body);
        let s = ui.s;
        let n = self.deck.slides.len();
        if self.thumbs.len() != n {
            self.touch_all();
        }
        for i in 0..n {
            let y = body.y + 12 + i as i32 * step - self.strip_scroll;
            if y + step < body.y || y > body.b() {
                continue;
            }
            let r = Rect::new(area.x + 32, y, tw, th);
            let on = i == self.cur;
            // the thumbnail: drawn big, then shrunk
            let pw = tw * s;
            let key = self.thumb_gen[i];
            let stale = !matches!(&self.thumbs[i], Some((g, c)) if *g == key && c.w == pw);
            if stale {
                let big = (pw * 3).max(360);
                let mut c = Canvas::new(big, big * self.deck.h / self.deck.w);
                draw_slide(&mut c, 0, 0, big, &self.deck, &self.deck.slides[i], &mut self.pics, None);
                let mut small = Canvas::new(pw, th * s);
                let b = small.bounds();
                small.blit_scaled(&c, b, 0);
                self.thumbs[i] = Some((key, small));
            }
            if let Some((_, c)) = &self.thumbs[i] {
                blit(ui.c, c, r.x * s, r.y * s);
            }
            ui.stroke(r.inset(-2), 4, if on { 2 } else { 1 }, if on { t.accent } else { t.line });
            ui.text_in(Rect::new(area.x + 4, y, 24, 18), if on { Face::Semibold } else { Face::Regular }, 12, &format!("{}", i + 1), if on { t.accent } else { t.text2 }, 2);
            if self.deck.slides[i].trans != Trans::None {
                ui.icon(Icon::Play, area.x + 12, y + 24, 9, t.text3);
            }
            ui.zone(r.inset(-2), Action::App(inst, C_THUMB + i as u32));
        }
        ui.set_clip(old);
        // add a slide
        let ar = Rect::new(area.x + 14, area.b() - 40, area.w - 28, 32);
        let aa = Action::App(inst, C_ADD_SLIDE);
        ui.rrect(ar, 8, if ui.hot(aa) { t.hover } else { t.chip });
        ui.icon(Icon::Plus, ar.x + 12, ar.y + 9, 14, t.text);
        ui.text(ar.x + 32, ar.y + 21, Face::Medium, 13, "New slide", t.text);
        ui.zone(ar, aa);
    }

    fn render_canvas(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let t = ui.t;
        let bg = if t.dark { Color::rgb(0x15141C) } else { Color::rgb(0xE9E4DC) };
        ui.rect(area, bg);
        ui.zone(area, Action::App(inst, C_CANVAS));
        // fit the slide
        let (mw, mh) = (area.w - 48, area.h - 40);
        let (mut w, mut h) = (mw, mw * self.deck.h / self.deck.w);
        if h > mh {
            h = mh;
            w = h * self.deck.w / self.deck.h;
        }
        let r = Rect::new(area.x + (area.w - w) / 2, area.y + (area.h - h) / 2, w.max(40), h.max(20));
        self.canvas = r;
        ui.shadow(r, 2, 10, 3, 40);
        let s = ui.s;
        let editing = self.ed.map(|e| e.shape);
        let key = (self.gen, self.cur, r.w * s, editing);
        if self.view.as_ref().map_or(true, |(k, _)| *k != key) {
            let mut c = Canvas::new(r.w * s, r.h * s);
            draw_slide(&mut c, 0, 0, r.w * s, &self.deck, &self.deck.slides[self.cur], &mut self.pics, Some(editing));
            self.view = Some((key, c));
        }
        if let Some((_, c)) = &self.view {
            let old = ui.clip_in(area);
            blit(ui.c, c, r.x * s, r.y * s);
            ui.set_clip(old);
        }
        let old = ui.clip_in(area);
        // selection, caret
        if let Some(i) = self.sel {
            if let Some(sh) = self.slide().shapes.get(i).cloned() {
                let sr = self.shape_rect(&sh);
                if let Some(ed) = self.ed.filter(|e| e.shape == i) {
                    ui.stroke(sr.inset(-2), 0, 1, t.accent);
                    let tb = layout(&sh);
                    // selection highlight
                    if let Some((a, b)) = Self::sel_range(&ed) {
                        for l in &tb.lines {
                            let (ls, le) = (Pos::new(l.p, l.start), Pos::new(l.p, l.end));
                            if le < a || ls > b {
                                continue;
                            }
                            let p = &sh.text.paras[l.p];
                            let s0 = if a > ls { a.i } else { l.start };
                            let e0 = if b < le { b.i } else { l.end };
                            let x0 = l.x + span_w(sh.kind, p, l.start, s0, l.px);
                            let x1 = l.x + span_w(sh.kind, p, l.start, e0, l.px).max(x0 - l.x + l.x + if b > le { 6 } else { 0 });
                            let hr = Rect::new(sr.x + self.to_screen(x0), sr.y + self.to_screen(l.top), self.to_screen(x1 - x0).max(2), self.to_screen(l.h).max(2));
                            ui.rect(hr, t.accent.with_alpha(70));
                        }
                    }
                    if (ui.ticks / 50) % 2 == 0 || self.ticks < 10 {
                        let (li, cx) = caret_xy(&sh, &tb, ed.caret);
                        let (top, h) = tb.lines.get(li).map(|l| (l.top, l.h)).unwrap_or((INSET_Y, 30));
                        let col = Color::rgb(deckio::text_color(&sh, &self.deck.theme));
                        ui.rect(Rect::new(sr.x + self.to_screen(cx), sr.y + self.to_screen(top), 2, self.to_screen(h).max(8)), col);
                    }
                } else {
                    ui.stroke(sr.inset(-1), 0, 2, t.accent);
                    for (hx, hy) in Self::handles(sr) {
                        let hr = Rect::new(hx - 4, hy - 4, 9, 9);
                        ui.rect(hr, Color::rgb(0xFFFFFF));
                        ui.stroke(hr, 0, 1, t.accent);
                    }
                }
            }
        }
        ui.set_clip(old);
    }

    fn render_notes(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let t = ui.t;
        self.notes_r = area;
        ui.rect(area, t.surface);
        ui.rect(Rect::new(area.x, area.y, area.w, 1), t.line);
        let focused = self.notes.is_some();
        let text = self.slide().notes.clone();
        let inner = Rect::new(area.x + 16, area.y + 8, area.w - 32, area.h - 14);
        let old = ui.clip_in(inner);
        if text.is_empty() && !focused {
            ui.text(inner.x, inner.y + 18, Face::Regular, 13, "Click to add speaker notes", t.text3);
        } else {
            // wrap each line
            let mut y = inner.y + 18;
            let caret = self.notes.unwrap_or(usize::MAX);
            let mut idx = 0usize;
            let mut caret_at: Option<(i32, i32)> = None;
            for line in text.split('\n') {
                let chars: Vec<char> = line.chars().collect();
                let mut start = 0;
                loop {
                    // how many characters fit
                    let mut end = chars.len();
                    while end > start && ui.tw(Face::Regular, 13, &chars[start..end].iter().collect::<String>()) > inner.w {
                        let sp = chars[start..end - 1].iter().rposition(|&c| c == ' ').map(|p| start + p + 1);
                        end = match sp {
                            Some(e) if e > start => e,
                            _ => end - 1,
                        };
                    }
                    let seg: String = chars[start..end].iter().collect();
                    ui.text(inner.x, y, Face::Regular, 13, &seg, t.text);
                    if caret >= idx + start && caret <= idx + end && caret_at.is_none() && (end == chars.len() || caret < idx + end) {
                        let pre: String = chars[start..caret - idx].iter().collect();
                        caret_at = Some((inner.x + ui.tw(Face::Regular, 13, &pre), y));
                    }
                    y += 18;
                    if end >= chars.len() {
                        break;
                    }
                    start = end;
                }
                idx += chars.len() + 1;
            }
            if let Some((cx, cy)) = caret_at {
                if (ui.ticks / 50) % 2 == 0 {
                    ui.rect(Rect::new(cx, cy - 13, 1, 16), t.text);
                }
            }
        }
        ui.set_clip(old);
        if focused {
            ui.stroke(area.inset(3), 6, 1, t.accent);
        }
        ui.zone(area, Action::App(inst, C_NOTES));
    }

    fn render_overlay(&self, ui: &mut Ui, r: Rect, inst: u32) {
        let t = ui.t;
        match &self.overlay {
            Overlay::Open(files) | Overlay::Pictures(files) => {
                let pics = matches!(self.overlay, Overlay::Pictures(_));
                ui.zone(r, Action::App(inst, C_DISMISS));
                let w = (r.w - 40).min(440);
                let n = files.len().min(9) as i32;
                let m = Rect::new(r.x + (r.w - w) / 2, r.y + HEADER + 20, w, if pics { 60 } else { 96 } + n.max(1) * 40);
                ui.shadow(m, 14, 18, 6, 70);
                ui.rrect(m, 14, t.surface);
                ui.text(m.x + 20, m.y + 32, Face::Semibold, 16, if pics { "Insert a picture" } else { "Open a presentation" }, t.text);
                let top = if pics { m.y + 48 } else { m.y + 88 };
                if !pics {
                    let nr = Rect::new(m.x + 12, m.y + 46, m.w - 24, 38);
                    let na = Action::App(inst, C_NEW);
                    if ui.hot(na) {
                        ui.rrect(nr, 8, t.hover);
                    }
                    ui.icon(Icon::Plus, nr.x + 10, nr.y + 11, 16, t.accent);
                    ui.text_in(Rect::new(nr.x + 36, nr.y, nr.w - 40, nr.h), Face::Medium, 13, "New blank presentation", t.accent, 0);
                    ui.zone(nr, na);
                }
                if files.is_empty() {
                    let msg = if pics { "No pictures in Pictures, Documents, Downloads or Shared." } else { "No presentations yet in Documents, Downloads or Shared." };
                    ui.text(m.x + 20, top + 22, Face::Regular, 13, msg, t.text2);
                }
                for (k, f) in files.iter().take(9).enumerate() {
                    let row = Rect::new(m.x + 12, top + k as i32 * 40, m.w - 24, 38);
                    let a = Action::App(inst, C_FILE + k as u32);
                    if ui.hot(a) {
                        ui.rrect(row, 8, t.hover);
                    }
                    ui.icon(if pics { Icon::Image } else { Icon::Slides }, row.x + 10, row.y + 11, 16, t.text2);
                    let name = ui.fit(Face::Medium, 13, basename(f), row.w - 150);
                    ui.text(row.x + 36, row.y + 24, Face::Medium, 13, &name, t.text);
                    let d = parent(f);
                    let d = d.rsplit('/').next().unwrap_or("");
                    let dw = ui.tw(Face::Regular, 12, d);
                    ui.text(row.r() - dw - 10, row.y + 24, Face::Regular, 12, d, t.text3);
                    ui.zone(row, a);
                }
            }
            Overlay::Menu(kind, at) => {
                ui.zone(r, Action::App(inst, C_DISMISS));
                self.render_menu(ui, r, *kind, *at, inst);
            }
            _ => {}
        }
    }

    fn render_menu(&self, ui: &mut Ui, win: Rect, kind: MenuKind, at: Rect, inst: u32) {
        let t = ui.t;
        let item = |k: usize| Action::App(inst, C_MENU + k as u32);
        let place = |w: i32, h: i32| {
            let x = at.x.min(win.r() - w - 8).max(win.x + 8);
            Rect::new(x, at.b() + 4, w, h)
        };
        match kind {
            MenuKind::NewSlide | MenuKind::Layout => {
                let m = place(250, 24 + LAYOUTS.len() as i32 * 52);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let cur_layout = self.slide().layout;
                for (k, l) in LAYOUTS.iter().enumerate() {
                    let row = Rect::new(m.x + 8, m.y + 12 + k as i32 * 52, m.w - 16, 50);
                    let on = kind == MenuKind::Layout && *l == cur_layout;
                    if ui.hot(item(k)) || on {
                        ui.rrect(row, 8, if on { t.accent.with_alpha(40) } else { t.hover });
                    }
                    // a little diagram of the layout
                    let d = Rect::new(row.x + 8, row.y + 7, 64, 36);
                    ui.rect(d, Color::rgb(self.deck.theme.bg));
                    ui.stroke(d, 0, 1, t.line);
                    for ph in layout_shapes(*l, 64, 36) {
                        let pr = Rect::new(d.x + ph.x, d.y + ph.y, ph.w.max(2), ph.h.max(2));
                        let c = if ph.kind == Kind::Title { Color::rgb(self.deck.theme.title) } else { Color::rgb(self.deck.theme.text).with_alpha(120) };
                        match ph.kind {
                            Kind::Title => ui.rect(Rect::new(pr.x + 2, pr.y + pr.h / 2 - 2, pr.w - 4, 4), c),
                            _ => {
                                for j in 0..3 {
                                    let ly = pr.y + 2 + j * 5;
                                    if ly + 2 < pr.b() {
                                        ui.rect(Rect::new(pr.x + 2, ly, (pr.w - 4) * (5 - j) / 5, 2), c);
                                    }
                                }
                            }
                        }
                    }
                    ui.text(row.x + 84, row.y + 30, Face::Medium, 13, l.name(), t.text);
                    ui.zone(row, item(k));
                }
            }
            MenuKind::Theme => {
                let (cols, tw, th) = (2, 150, 84);
                let m = place(cols * (tw + 10) + 14, 3 * (th + 32) + 16);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                for (k, id) in THEME_IDS.iter().enumerate() {
                    let th_ = theme(id);
                    let cell = Rect::new(m.x + 12 + (k as i32 % cols) * (tw + 10), m.y + 12 + (k as i32 / cols) * (th + 32), tw, th);
                    let on = self.deck.theme.id == *id;
                    if ui.hot(item(k)) || on {
                        ui.rrect(Rect::new(cell.x - 4, cell.y - 4, cell.w + 8, cell.h + 30), 8, if on { t.accent.with_alpha(40) } else { t.hover });
                    }
                    ui.rect(cell, Color::rgb(th_.bg));
                    let old = ui.clip_in(cell);
                    for dc in &th_.deco {
                        let (x, y, w, h) = dc.place(SLIDE_W, SLIDE_H);
                        let s = |v: i32| v * tw / SLIDE_W;
                        let dr = Rect::new(cell.x + s(x), cell.y + s(y), s(x + w) - s(x), (s(y + h) - s(y)).max(1));
                        if dc.ellipse {
                            let sc = ui.s;
                            ellipse(ui.c, dr.scale(sc), Color::rgb(dc.color), 0);
                        } else {
                            ui.rect(dr, Color::rgb(dc.color));
                        }
                    }
                    ui.set_clip(old);
                    ui.text(cell.x + 12, cell.y + 44, Face::Semibold, 22, "Aa", Color::rgb(th_.title));
                    ui.rect(Rect::new(cell.x + 12, cell.y + 54, 50, 3), Color::rgb(th_.accent));
                    ui.text(cell.x + 12, cell.y + 70, Face::Regular, 11, "Text on slides", Color::rgb(th_.text));
                    ui.stroke(cell, 0, 1, t.line);
                    ui.text(cell.x, cell.b() + 18, Face::Medium, 13, &th_.name, t.text);
                    ui.zone(Rect::new(cell.x - 4, cell.y - 4, cell.w + 8, cell.h + 30), item(k));
                }
            }
            MenuKind::Color => {
                let m = place(6 * 34 + 20, 2 * 34 + 70);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let what = match self.shape().map(|s| s.kind) {
                    Some(Kind::Rect | Kind::Ellipse) => "Fill colour",
                    Some(Kind::Picture) => "Border colour",
                    Some(_) => "Text colour",
                    None => "Slide background",
                };
                ui.text(m.x + 12, m.y + 22, Face::Semibold, 13, what, t.text);
                for (k, c) in PALETTE.iter().enumerate() {
                    let cell = Rect::new(m.x + 12 + (k as i32 % 6) * 34, m.y + 32 + (k as i32 / 6) * 34, 28, 28);
                    ui.rrect(cell, 6, Color::rgb(*c));
                    ui.stroke(cell, 6, if ui.hot(item(k)) { 2 } else { 1 }, if ui.hot(item(k)) { t.accent } else { t.line });
                    ui.zone(cell, item(k));
                }
                let ar = Rect::new(m.x + 8, m.b() - 36, m.w - 16, 30);
                let a = item(PALETTE.len());
                if ui.hot(a) {
                    ui.rrect(ar, 8, t.hover);
                }
                let label = if self.shape().map_or(false, |s| s.kind == Kind::Picture) { "No border" } else { "Automatic (the theme's)" };
                ui.text(ar.x + 8, ar.y + 20, Face::Medium, 13, label, t.text);
                ui.zone(ar, a);
            }
            MenuKind::Trans => {
                let items = ["None", "Fade", "Push up", "Apply to all slides"];
                let m = place(210, 20 + items.len() as i32 * 36);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let cur = self.slide().trans;
                for (k, name) in items.iter().enumerate() {
                    let row = Rect::new(m.x + 8, m.y + 10 + k as i32 * 36, m.w - 16, 34);
                    let on = matches!((k, cur), (0, Trans::None) | (1, Trans::Fade) | (2, Trans::Push));
                    if ui.hot(item(k)) {
                        ui.rrect(row, 8, t.hover);
                    }
                    if on {
                        ui.icon(Icon::Check, row.x + 8, row.y + 9, 14, t.accent);
                    }
                    if k == 3 {
                        ui.rect(Rect::new(row.x, row.y - 1, row.w, 1), t.line);
                    }
                    ui.text(row.x + 30, row.y + 22, Face::Medium, 13, name, t.text);
                    ui.zone(row, item(k));
                }
            }
        }
    }

    fn menu_pick(&mut self, kind: MenuKind, k: usize) {
        match kind {
            MenuKind::NewSlide => {
                if let Some(l) = LAYOUTS.get(k) {
                    self.add_slide(*l);
                }
            }
            MenuKind::Layout => {
                if let Some(l) = LAYOUTS.get(k) {
                    self.stop_edit();
                    self.snapshot();
                    let mut s = self.slide().clone();
                    self.deck.relayout(&mut s, *l);
                    *self.slide_mut() = s;
                    self.sel = None;
                    self.touch();
                }
            }
            MenuKind::Theme => {
                if let Some(id) = THEME_IDS.get(k) {
                    self.snapshot();
                    self.deck.theme = theme(id);
                    self.touch_all();
                }
            }
            MenuKind::Color => self.set_color(PALETTE.get(k).copied()),
            MenuKind::Trans => {
                self.snapshot();
                if k == 3 {
                    let tr = self.slide().trans;
                    for s in self.deck.slides.iter_mut() {
                        s.trans = tr;
                    }
                    self.touch_all();
                } else {
                    self.slide_mut().trans = [Trans::None, Trans::Fade, Trans::Push][k.min(2)];
                    self.touch();
                }
            }
        }
    }

    fn open_menu(&mut self, kind: MenuKind, code: u32) {
        if matches!(self.overlay, Overlay::Menu(k, _) if k == kind) {
            self.overlay = Overlay::None;
            return;
        }
        // anchor under the toolbar button (found from the pointer)
        let (mx, _) = self.mouse;
        let _ = code;
        let at = Rect::new(mx - 20, self.canvas.y.min(self.mouse.1 + 16) - 6, 40, 1);
        self.overlay = Overlay::Menu(kind, at);
    }

    // ---- canvas clicks -------------------------------------------------------------

    fn handle_at(&self, x: i32, y: i32) -> Option<u8> {
        let sh = self.shape()?;
        if self.ed.is_some() {
            return None;
        }
        let r = self.shape_rect(sh);
        Self::handles(r).iter().position(|&(hx, hy)| (x - hx).abs() <= 6 && (y - hy).abs() <= 6).map(|i| i as u8)
    }

    fn shape_at(&self, ux: i32, uy: i32) -> Option<usize> {
        let slide = self.slide();
        // pictures and shapes: their box; text: only where there's text or a prompt
        slide.shapes.iter().enumerate().rev().find(|(_, s)| s.contains(ux, uy)).map(|(i, _)| i)
    }

    fn canvas_click(&mut self, double: bool) {
        let (mx, my) = self.mouse;
        if let Some(h) = self.handle_at(mx, my) {
            let sh = self.shape().unwrap();
            self.press = Some(Press::Resize { handle: h, start: (mx, my), orig: (sh.x, sh.y, sh.w, sh.h), moved: false });
            return;
        }
        let (ux, uy) = self.to_units(mx, my);
        // in the text being edited: place the caret
        if let Some(ed) = self.ed {
            let sh = self.slide().shapes[ed.shape].clone();
            if sh.contains(ux, uy) {
                let tb = layout(&sh);
                let p = hit(&sh, &tb, ux - sh.x, uy - sh.y);
                let e = self.ed.as_mut().unwrap();
                if double {
                    let (a, b) = sh.text.word_at(p);
                    e.anchor = Some(a);
                    e.caret = b;
                } else if crate::input::shift() {
                    if e.anchor.is_none() {
                        e.anchor = Some(e.caret);
                    }
                    e.caret = p;
                } else {
                    e.anchor = None;
                    e.caret = p;
                }
                e.typing = false;
                self.press = Some(Press::Text);
                return;
            }
            self.stop_edit();
        }
        self.notes = None;
        match self.shape_at(ux, uy) {
            Some(i) => {
                let sh = self.slide().shapes[i].clone();
                let enter_text = sh.kind.has_text() && (double || (self.sel == Some(i) && sh.kind != Kind::Rect && sh.kind != Kind::Ellipse) || (sh.kind.placeholder() && sh.is_empty()) || (self.sel == Some(i) && double));
                if enter_text && (self.sel == Some(i) || double || sh.is_empty()) {
                    let tb = layout(&sh);
                    let p = hit(&sh, &tb, ux - sh.x, uy - sh.y);
                    self.start_edit(i, Some(p));
                    self.press = Some(Press::Text);
                    return;
                }
                self.sel = Some(i);
                self.press = Some(Press::Move { start: (mx, my), orig: (sh.x, sh.y), moved: false });
            }
            None => {
                self.sel = None;
            }
        }
    }

    fn notes_key(&mut self, k: Key, ctrl: bool, sys: &mut Sys) {
        let Some(mut c) = self.notes else { return };
        let mut text: Vec<char> = self.slide().notes.chars().collect();
        c = c.min(text.len());
        let mut changed = false;
        match k {
            Key::Char(ch) if ctrl && ch.to_ascii_lowercase() == 'v' => {
                let p: Vec<char> = sys.clipboard.chars().filter(|c| *c != '\r').collect();
                for (j, ch) in p.iter().enumerate() {
                    text.insert(c + j, *ch);
                }
                c += p.len();
                changed = true;
            }
            Key::Char(_) if ctrl => {}
            Key::Char(ch) if !ch.is_control() => {
                text.insert(c, ch);
                c += 1;
                changed = true;
            }
            Key::Enter => {
                text.insert(c, '\n');
                c += 1;
                changed = true;
            }
            Key::Backspace if c > 0 => {
                text.remove(c - 1);
                c -= 1;
                changed = true;
            }
            Key::Delete if c < text.len() => {
                text.remove(c);
                changed = true;
            }
            Key::Left => c = c.saturating_sub(1),
            Key::Right => c = (c + 1).min(text.len()),
            Key::Home => c = text[..c].iter().rposition(|&x| x == '\n').map(|p| p + 1).unwrap_or(0),
            Key::End => c = text[c..].iter().position(|&x| x == '\n').map(|p| c + p).unwrap_or(text.len()),
            Key::Esc => {
                self.notes = None;
                return;
            }
            _ => {}
        }
        if changed {
            // one undo step per run of typing
            if !self.notes_typing {
                self.snapshot();
                self.notes_typing = true;
            }
            self.slide_mut().notes = text.into_iter().collect();
            self.dirty = true;
        } else {
            self.notes_typing = false;
        }
        self.notes = Some(c);
    }
}

fn mixp(a: u32, b: u32, t: u32) -> u32 {
    let f = |s: u32| {
        let (x, y) = ((a >> s) & 255, (b >> s) & 255);
        ((x * (256 - t) + y * t) >> 8) << s
    };
    f(16) | f(8) | f(0)
}

impl App for Slides {
    fn kind(&self) -> AppKind {
        AppKind::Slides
    }

    fn fullscreen(&self) -> bool {
        self.show.is_some()
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        self.ticks = ui.ticks;
        if self.show.is_some() {
            self.render_show(ui, r, inst);
            return;
        }
        let t = ui.t;
        let compact = super::compact(r);
        let title_x = r.x + 18;
        let bx = r.r() - 118 - if compact { 96 } else { 136 };
        ui.icon(Icon::Slides, title_x, r.y + 13, 18, t.accent);
        let tr = Rect::new(title_x + 26, r.y + 7, bx - title_x - 34, 30);
        match &self.overlay {
            Overlay::Rename(e) => {
                let text = e.text.clone();
                ui.field(tr, &text, "Presentation name", true, Action::App(inst, C_RENAME));
            }
            _ => {
                let mut title = self.title();
                if self.dirty {
                    title.push_str("  •");
                }
                let ta = Action::App(inst, C_RENAME);
                let label = ui.fit(Face::Semibold, 15, &title, tr.w - 12);
                let w = ui.tw(Face::Semibold, 15, &label);
                if ui.hot(ta) {
                    ui.rrect(Rect::new(tr.x - 6, tr.y, w + 12, tr.h), 8, t.hover);
                }
                ui.text_in(tr, Face::Semibold, 15, &label, t.text, 0);
                ui.zone(Rect::new(tr.x - 6, tr.y, w + 12, tr.h), ta);
            }
        }
        let mut x = bx;
        if !compact {
            ui.icon_button(Rect::new(x, r.y + 8, 28, 28), Icon::Plus, Action::App(inst, C_NEW), 16);
            x += 34;
        }
        ui.icon_button(Rect::new(x, r.y + 8, 28, 28), Icon::Folder, Action::App(inst, C_OPEN), 16);
        x += 34;
        ui.icon_button(Rect::new(x, r.y + 8, 28, 28), Icon::Save, Action::App(inst, C_SAVE), 16);
        x += 36;
        // play
        let pr = Rect::new(x, r.y + 8, 30, 28);
        let pa = Action::App(inst, C_PLAY);
        ui.rrect(pr, 8, if ui.hot(pa) { t.accent.mix(t.text, 20) } else { t.accent });
        ui.icon_in(Icon::Play, pr, 13, t.on_accent);
        ui.zone(pr, pa);
        self.render_toolbar(ui, r, inst, compact);
        let top = r.y + HEADER + TOOLBAR;
        let bottom = r.b() - STATUS;
        let strip_w = if compact { 0 } else { STRIP_W };
        if !compact {
            self.render_strip(ui, Rect::new(r.x, top, strip_w, bottom - top), inst);
        }
        let notes_h = if r.h > 480 { NOTES_H } else { 0 };
        self.render_canvas(ui, Rect::new(r.x + strip_w, top, r.w - strip_w, bottom - top - notes_h), inst);
        if notes_h > 0 {
            self.render_notes(ui, Rect::new(r.x + strip_w, bottom - notes_h, r.w - strip_w, notes_h), inst);
        }
        // status bar
        let sy = r.b() - STATUS;
        ui.rect(Rect::new(r.x, sy, r.w, STATUS), t.surface);
        ui.rect(Rect::new(r.x, sy, r.w, 1), t.line);
        let n = self.deck.slides.len();
        let mut sx = r.x + 12;
        if compact {
            ui.icon_button(Rect::new(sx, sy + 3, 22, 22), Icon::ChevronLeft, Action::App(inst, C_PREV), 12);
            sx += 24;
            ui.icon_button(Rect::new(sx, sy + 3, 22, 22), Icon::ChevronRight, Action::App(inst, C_NEXT), 12);
            sx += 30;
        }
        let left = format!("Slide {} of {}   ·   {}   ·   {}", self.cur + 1, n, self.slide().layout.name(), self.deck.theme.name);
        ui.text(sx, sy + 19, Face::Regular, 12, &left, t.text2);
        let right = if self.imported {
            String::from("Viewing a PowerPoint file  ·  saving creates a .hydp copy")
        } else {
            format!("Hyda Slides presentation  ·  {}", if self.dirty { "Edited" } else if self.path.is_empty() { "Not saved yet" } else { "Saved" })
        };
        if !compact {
            let rw = ui.tw(Face::Regular, 12, &right);
            if r.r() - rw - 16 > sx + ui.tw(Face::Regular, 12, &left) + 20 {
                ui.text(r.r() - rw - 16, sy + 19, Face::Regular, 12, &right, t.text3);
            }
        }
        self.render_overlay(ui, r, inst);
    }

    fn action(&mut self, code: u32, double: bool, sys: &mut Sys) {
        if self.show.is_some() {
            if code == C_SHOW {
                self.show_step(true);
            }
            return;
        }
        if let Overlay::Rename(e) = &self.overlay {
            if code != C_RENAME {
                let name = e.text.clone();
                self.overlay = Overlay::None;
                self.rename_to(&name, sys);
            }
        }
        // a menu or chooser is open: pick from it, or close it
        match &self.overlay {
            Overlay::Menu(kind, _) => {
                let kind = *kind;
                if (C_MENU..C_FILE).contains(&code) {
                    self.overlay = Overlay::None;
                    self.menu_pick(kind, (code - C_MENU) as usize);
                    return;
                }
                let same = matches!((kind, code), (MenuKind::NewSlide, C_NEW_SLIDE) | (MenuKind::Layout, C_LAYOUT) | (MenuKind::Theme, C_THEME) | (MenuKind::Color, C_COLOR) | (MenuKind::Trans, C_TRANS));
                self.overlay = Overlay::None;
                if same || code == C_DISMISS {
                    return;
                }
            }
            Overlay::Open(_) | Overlay::Pictures(_) if code < C_FILE && code != C_NEW => {
                self.overlay = Overlay::None;
                if matches!(code, C_DISMISS | C_OPEN | C_PICTURE) {
                    return;
                }
            }
            _ => {}
        }
        if code != C_CANVAS && code != C_NOTES && code >= C_NEW_SLIDE && code != C_BOLD && code != C_ITALIC && code != C_UNDERLINE && !(C_LEFT..=C_BIGGER).contains(&code) && code != C_COLOR {
            // toolbar and menu commands leave text editing (formatting keeps it)
            if !(C_MENU..C_FILE).contains(&code) && code != C_THEME && code != C_TRANS && code != C_UNDO && code != C_REDO && code != C_COPY && code != C_CUT && code != C_PASTE && code != C_ALL {
                self.stop_edit();
            }
        }
        self.press = None;
        match code {
            C_CANVAS => self.canvas_click(double),
            C_NOTES => {
                self.stop_edit();
                self.notes_typing = false;
                self.sel = None;
                let n = self.slide().notes.chars().count();
                self.notes = Some(self.notes.unwrap_or(n).min(n));
                if self.notes.is_some() && double {
                    self.notes = Some(n);
                }
            }
            C_STRIP => {
                self.stop_edit();
                self.notes = None;
                self.sel = None;
            }
            c if (C_THUMB..C_MENU).contains(&c) => {
                let i = (c - C_THUMB) as usize;
                if i < self.deck.slides.len() {
                    self.go(i);
                    if double {
                        self.play(false);
                        return;
                    }
                    self.press = Some(Press::Thumb(false));
                }
            }
            C_ADD_SLIDE => {
                let l = if self.slide().layout == Layout::Title { Layout::TitleContent } else { self.slide().layout };
                let l = if l == Layout::Blank { Layout::TitleContent } else { l };
                self.add_slide(l);
            }
            C_NEW_SLIDE => self.open_menu(MenuKind::NewSlide, code),
            C_LAYOUT => self.open_menu(MenuKind::Layout, code),
            C_THEME => self.open_menu(MenuKind::Theme, code),
            C_COLOR => self.open_menu(MenuKind::Color, code),
            C_TRANS => self.open_menu(MenuKind::Trans, code),
            C_BOLD => self.toggle_fmt(BOLD),
            C_ITALIC => self.toggle_fmt(ITALIC),
            C_UNDERLINE => self.toggle_fmt(UNDERLINE),
            C_LEFT => self.each_para(|p| p.align = Align::Left),
            C_CENTER => self.each_para(|p| p.align = Align::Center),
            C_RIGHT => self.each_para(|p| p.align = Align::Right),
            C_BULLETS => self.set_list(Style::Bullet),
            C_NUMBERS => self.set_list(Style::Number),
            C_SMALLER => self.resize_text(false),
            C_BIGGER => self.resize_text(true),
            C_TEXTBOX => self.add_shape(Kind::Text),
            C_RECT => self.add_shape(Kind::Rect),
            C_ELLIPSE => self.add_shape(Kind::Ellipse),
            C_PICTURE => {
                self.overlay = Overlay::Pictures(Self::list_files(sys, &[".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"], "\u{0}"));
            }
            C_UNDO => self.undo_redo(true),
            C_REDO => self.undo_redo(false),
            C_PLAY => self.play(false),
            C_PLAY_START => self.play(true),
            C_NEW => {
                self.overlay = Overlay::None;
                self.park(sys);
                self.reset(Deck::new());
            }
            C_OPEN => {
                self.stop_edit();
                self.overlay = Overlay::Open(Self::list_files(sys, &[".pptx"], ".hydp"));
            }
            C_SAVE => self.save(sys, false),
            C_EXPORT => self.export(sys),
            C_RENAME => {
                if !matches!(self.overlay, Overlay::Rename(_)) {
                    self.stop_edit();
                    self.overlay = Overlay::Rename(LineEdit { text: self.title() });
                }
            }
            C_CUT => {
                self.copy(sys);
                if self.ed.is_some() {
                    self.begin_text_change(false);
                    self.touch();
                } else {
                    self.delete_shape();
                }
            }
            C_COPY => self.copy(sys),
            C_PASTE => self.paste(sys),
            C_DUP => {
                if self.sel.is_some() && self.ed.is_none() {
                    self.copy(sys);
                    self.paste(sys);
                } else {
                    self.duplicate_slide();
                }
            }
            C_DELETE => {
                if self.sel.is_some() {
                    self.delete_shape();
                } else {
                    self.delete_slide();
                }
            }
            C_ALL => {
                if let Some((sh, ed)) = self.ed_shape() {
                    ed.anchor = Some(Pos::new(0, 0));
                    ed.caret = sh.text.end();
                }
            }
            C_PREV => {
                if self.cur > 0 {
                    let c = self.cur - 1;
                    self.go(c);
                }
            }
            C_NEXT => {
                let c = self.cur + 1;
                if c < self.deck.slides.len() {
                    self.go(c);
                }
            }
            C_DEL_SLIDE => self.delete_slide(),
            C_DUP_SLIDE => self.duplicate_slide(),
            C_SLIDE_UP => {
                if self.cur > 0 {
                    self.snapshot();
                    let c = self.cur - 1;
                    self.move_slide(c);
                }
            }
            C_SLIDE_DOWN => {
                self.snapshot();
                let c = self.cur + 1;
                self.move_slide(c);
            }
            C_FRONT => self.restack(true),
            C_BACK => self.restack(false),
            C_DISMISS => {}
            c if c >= C_FILE => {
                let (path, pic) = match &self.overlay {
                    Overlay::Open(files) => (files.get((c - C_FILE) as usize).cloned(), false),
                    Overlay::Pictures(files) => (files.get((c - C_FILE) as usize).cloned(), true),
                    _ => (None, false),
                };
                self.overlay = Overlay::None;
                if let Some(p) = path {
                    if pic {
                        self.insert_picture(&p, sys);
                    } else {
                        self.load(&p, sys);
                    }
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, ctrl: bool, sys: &mut Sys) {
        let shift = crate::input::shift();
        if self.show.is_some() {
            match k {
                Key::Esc => {
                    let idx = self.show.as_ref().map(|s| s.idx).unwrap_or(0);
                    self.show = None;
                    self.go(idx.min(self.deck.slides.len() - 1));
                }
                Key::Right | Key::Down | Key::PageDown | Key::Enter | Key::Char(' ') | Key::Char('n') => self.show_step(true),
                Key::Left | Key::Up | Key::PageUp | Key::Backspace | Key::Char('p') => self.show_step(false),
                Key::Home => {
                    if let Some(s) = self.show.as_mut() {
                        *s = Show { idx: 0, from: None, end: false, black: false, frame: None };
                    }
                }
                Key::End => {
                    let n = self.deck.slides.len() - 1;
                    if let Some(s) = self.show.as_mut() {
                        *s = Show { idx: n, from: None, end: false, black: false, frame: None };
                    }
                }
                Key::Char('b') | Key::Char('.') => {
                    if let Some(s) = self.show.as_mut() {
                        s.black = !s.black;
                    }
                }
                _ => {}
            }
            return;
        }
        if let Overlay::Rename(e) = &mut self.overlay {
            match k {
                Key::Enter => {
                    let name = e.text.clone();
                    self.overlay = Overlay::None;
                    self.rename_to(&name, sys);
                }
                Key::Esc => self.overlay = Overlay::None,
                _ => {
                    e.key(k);
                }
            }
            return;
        }
        if !matches!(self.overlay, Overlay::None) {
            if k == Key::Esc {
                self.overlay = Overlay::None;
            }
            return;
        }
        if k == Key::F(5) {
            self.play(!shift);
            return;
        }
        if self.notes.is_some() {
            self.notes_key(k, ctrl, sys);
            return;
        }
        if ctrl {
            match k {
                Key::Char(c) => match c.to_ascii_lowercase() {
                    'b' => self.toggle_fmt(BOLD),
                    'i' => self.toggle_fmt(ITALIC),
                    'u' => self.toggle_fmt(UNDERLINE),
                    'e' => self.each_para(|p| p.align = Align::Center),
                    'l' => self.each_para(|p| p.align = Align::Left),
                    'r' => self.each_para(|p| p.align = Align::Right),
                    'z' => self.undo_redo(true),
                    'y' => self.undo_redo(false),
                    'x' => self.action(C_CUT, false, sys),
                    'c' => self.copy(sys),
                    'v' => self.paste(sys),
                    'd' => self.action(C_DUP, false, sys),
                    'a' => self.action(C_ALL, false, sys),
                    'm' => self.action(C_ADD_SLIDE, false, sys),
                    's' => self.save(sys, false),
                    'o' => self.action(C_OPEN, false, sys),
                    'n' => self.action(C_NEW, false, sys),
                    ']' => self.resize_text(true),
                    '[' => self.resize_text(false),
                    _ => {}
                },
                Key::Left | Key::Right if self.ed.is_some() => self.move_caret(k, shift, true),
                Key::Up if self.ed.is_none() && self.sel.is_none() => self.action(C_SLIDE_UP, false, sys),
                Key::Down if self.ed.is_none() && self.sel.is_none() => self.action(C_SLIDE_DOWN, false, sys),
                Key::Home if self.ed.is_some() => {
                    if let Some((_, ed)) = self.ed_shape() {
                        if shift && ed.anchor.is_none() {
                            ed.anchor = Some(ed.caret);
                        }
                        if !shift {
                            ed.anchor = None;
                        }
                        ed.caret = Pos::new(0, 0);
                    }
                }
                Key::End if self.ed.is_some() => {
                    if let Some((sh, ed)) = self.ed_shape() {
                        if shift && ed.anchor.is_none() {
                            ed.anchor = Some(ed.caret);
                        }
                        if !shift {
                            ed.anchor = None;
                        }
                        ed.caret = sh.text.end();
                    }
                }
                _ => {}
            }
            return;
        }
        // typing in a shape
        if self.ed.is_some() {
            match k {
                Key::Char(c) if !c.is_control() => self.type_text(c.encode_utf8(&mut [0u8; 4])),
                Key::Enter => self.enter(),
                Key::Backspace => self.backspace(false),
                Key::Delete => self.backspace(true),
                Key::Tab => {
                    let body = self.shape().map_or(false, |s| s.kind == Kind::Body || s.text.paras.iter().any(|p| p.style != Style::Body));
                    if body {
                        self.indent(shift);
                    } else {
                        self.type_text("\t");
                    }
                }
                Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End => self.move_caret(k, shift, false),
                Key::Esc => {
                    self.stop_edit();
                }
                _ => {}
            }
            return;
        }
        // a selected shape
        if let Some(i) = self.sel {
            let step = if shift { 1 } else { 8 };
            let nudge = |d: (i32, i32)| d;
            let d = match k {
                Key::Left => Some(nudge((-step, 0))),
                Key::Right => Some(nudge((step, 0))),
                Key::Up => Some(nudge((0, -step))),
                Key::Down => Some(nudge((0, step))),
                _ => None,
            };
            if let Some((dx, dy)) = d {
                self.snapshot();
                let sh = &mut self.slide_mut().shapes[i];
                sh.x += dx;
                sh.y += dy;
                self.touch();
                return;
            }
            match k {
                Key::Delete | Key::Backspace => self.delete_shape(),
                Key::Esc => self.sel = None,
                Key::Enter | Key::F(2) => self.start_edit(i, None),
                Key::Tab => {
                    let n = self.slide().shapes.len();
                    self.sel = Some(if shift { (i + n - 1) % n } else { (i + 1) % n });
                }
                Key::Char(c) if !c.is_control() => {
                    if self.slide().shapes[i].kind.has_text() {
                        self.start_edit(i, None);
                        self.type_text(c.encode_utf8(&mut [0u8; 4]));
                    }
                }
                _ => {}
            }
            return;
        }
        // nothing selected: move between slides
        match k {
            Key::Up | Key::PageUp | Key::Left => {
                if self.cur > 0 {
                    let c = self.cur - 1;
                    self.go(c);
                }
            }
            Key::Down | Key::PageDown | Key::Right => {
                let c = self.cur + 1;
                if c < self.deck.slides.len() {
                    self.go(c);
                }
            }
            Key::Home => self.go(0),
            Key::End => {
                let n = self.deck.slides.len() - 1;
                self.go(n);
            }
            Key::Tab => {
                if !self.slide().shapes.is_empty() {
                    self.sel = Some(0);
                }
            }
            Key::Delete | Key::Backspace => self.delete_slide(),
            Key::Enter => self.action(C_ADD_SLIDE, false, sys),
            _ => {}
        }
    }

    fn scroll(&mut self, dy: i32) {
        if self.show.is_some() {
            self.show_step(dy > 0);
            return;
        }
        if self.strip.contains(self.mouse.0, self.mouse.1) {
            self.strip_scroll += dy * 60;
        } else if self.ed.is_none() && self.canvas.contains(self.mouse.0, self.mouse.1) {
            let c = (self.cur as i32 + dy.signum()).clamp(0, self.deck.slides.len() as i32 - 1) as usize;
            if c != self.cur {
                self.go(c);
            }
        }
    }

    fn mouse(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    fn drag(&mut self, x: i32, y: i32) {
        let Some(p) = self.press else { return };
        match p {
            Press::Move { start, orig, moved } => {
                let (dx, dy) = (x - start.0, y - start.1);
                if !moved && dx.abs() + dy.abs() < 3 {
                    return;
                }
                if !moved {
                    self.snapshot();
                    self.press = Some(Press::Move { start, orig, moved: true });
                }
                let k = |v: i32| (v as i64 * self.deck.w as i64 / self.canvas.w.max(1) as i64) as i32;
                let (mut nx, mut ny) = (orig.0 + k(dx), orig.1 + k(dy));
                // snap to the slide's centre lines and edges
                if let Some(i) = self.sel {
                    let (w, h) = (self.slide().shapes[i].w, self.slide().shapes[i].h);
                    let snap = |v: i32, size: i32, total: i32| {
                        for target in [0, (total - size) / 2, total - size] {
                            if (v - target).abs() <= 8 {
                                return target;
                            }
                        }
                        v
                    };
                    nx = snap(nx, w, self.deck.w);
                    ny = snap(ny, h, self.deck.h);
                    let sh = &mut self.slide_mut().shapes[i];
                    sh.x = nx;
                    sh.y = ny;
                    self.touch();
                }
            }
            Press::Resize { handle, start, orig, moved } => {
                if !moved {
                    self.snapshot();
                    self.press = Some(Press::Resize { handle, start, orig, moved: true });
                }
                let k = |v: i32| (v as i64 * self.deck.w as i64 / self.canvas.w.max(1) as i64) as i32;
                let (dx, dy) = (k(x - start.0), k(y - start.1));
                let (ox, oy, ow, oh) = orig;
                let (mut l, mut t, mut r, mut b) = (ox, oy, ox + ow, oy + oh);
                match handle {
                    0 => {
                        l += dx;
                        t += dy;
                    }
                    1 => t += dy,
                    2 => {
                        r += dx;
                        t += dy;
                    }
                    3 => r += dx,
                    4 => {
                        r += dx;
                        b += dy;
                    }
                    5 => b += dy,
                    6 => {
                        l += dx;
                        b += dy;
                    }
                    _ => l += dx,
                }
                let Some(i) = self.sel else { return };
                let keep_aspect = self.slide().shapes[i].kind == Kind::Picture && handle % 2 == 0 && !crate::input::shift();
                if keep_aspect && oh > 0 {
                    // width leads; the opposite corner stays put
                    let w = (r - l).max(16);
                    let h = (w as i64 * oh as i64 / ow.max(1) as i64) as i32;
                    if handle == 0 || handle == 2 {
                        t = b - h;
                    } else {
                        b = t + h;
                    }
                }
                let sh = &mut self.slide_mut().shapes[i];
                sh.x = l.min(r - 16);
                sh.y = t.min(b - 16);
                sh.w = (r - l).max(16);
                sh.h = (b - t).max(16);
                self.touch();
            }
            Press::Text => {
                let (ux, uy) = self.to_units(x, y);
                if let Some(ed) = self.ed {
                    let sh = self.slide().shapes[ed.shape].clone();
                    let tb = layout(&sh);
                    let p = hit(&sh, &tb, ux - sh.x, uy - sh.y);
                    let e = self.ed.as_mut().unwrap();
                    if e.anchor.is_none() {
                        e.anchor = Some(e.caret);
                    }
                    e.caret = p;
                }
            }
            Press::Thumb(moved) => {
                if !self.strip.contains(x, self.strip.y + 1) {
                    return;
                }
                let step = self.thumb_step();
                let to = ((y - self.strip.y - 12 + self.strip_scroll).max(0) / step) as usize;
                let to = to.min(self.deck.slides.len() - 1);
                if to != self.cur {
                    if !moved {
                        self.snapshot();
                        self.press = Some(Press::Thumb(true));
                    }
                    self.move_slide(to);
                }
            }
        }
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => vec![("New Presentation", C_NEW), ("Open…", C_OPEN), ("Save", C_SAVE), ("Export as PowerPoint (.pptx)", C_EXPORT), ("Rename…", C_RENAME)],
            1 => vec![("Undo", C_UNDO), ("Redo", C_REDO), ("Cut", C_CUT), ("Copy", C_COPY), ("Paste", C_PASTE), ("Duplicate", C_DUP), ("Delete", C_DELETE), ("Select All", C_ALL), ("Bring to Front", C_FRONT), ("Send to Back", C_BACK)],
            2 => vec![("Play from Start", C_PLAY_START), ("Play from This Slide", C_PLAY), ("Insert Text Box", C_TEXTBOX), ("Insert Rectangle", C_RECT), ("Insert Ellipse", C_ELLIPSE), ("Insert Picture…", C_PICTURE)],
            3 => vec![("New Slide", C_ADD_SLIDE), ("Duplicate Slide", C_DUP_SLIDE), ("Delete Slide", C_DEL_SLIDE), ("Move Slide Up", C_SLIDE_UP), ("Move Slide Down", C_SLIDE_DOWN), ("Previous Slide", C_PREV), ("Next Slide", C_NEXT)],
            _ => vec![],
        }
    }

    fn tick(&mut self, _sys: &mut Sys) {}

    fn close(&mut self, sys: &mut Sys) {
        self.show = None;
        self.park(sys);
    }

    fn animating(&self) -> bool {
        self.ed.is_some() || self.notes.is_some() || self.show.as_ref().map_or(false, |s| s.from.is_some())
    }

    fn open_path(&mut self, path: &str, sys: &mut Sys) {
        self.load(path, sys);
    }
}
