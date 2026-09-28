//! Hyda Slides presentations: slides made of shapes (titles, text, bullets,
//! rectangles, ellipses and pictures) on a theme, speaker notes, and the text
//! layout the editor, the thumbnails and the slideshow share.
//!
//! Slides are measured in slide units: a widescreen slide is 1280 × 720 (one
//! unit is 1/96 inch, 9,525 EMU in PowerPoint's terms). Text sizes are in
//! points; a point is 4/3 of a unit.

use crate::doc::{Doc, Para, Pos, Style, BOLD, ITALIC};
use crate::font::{self, Face};
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub use crate::doc::Align;

pub const SLIDE_W: i32 = 1280;
pub const SLIDE_H: i32 = 720;
/// Text inset inside a shape (units).
pub const INSET_X: i32 = 12;
pub const INSET_Y: i32 = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Title,
    Subtitle,
    Body,
    Text,
    Rect,
    Ellipse,
    Picture,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Title => "title",
            Kind::Subtitle => "subtitle",
            Kind::Body => "body",
            Kind::Text => "text",
            Kind::Rect => "rect",
            Kind::Ellipse => "ellipse",
            Kind::Picture => "picture",
        }
    }
    pub fn from_id(s: &str) -> Option<Kind> {
        Some(match s {
            "title" => Kind::Title,
            "subtitle" => Kind::Subtitle,
            "body" => Kind::Body,
            "text" => Kind::Text,
            "rect" => Kind::Rect,
            "ellipse" => Kind::Ellipse,
            "picture" => Kind::Picture,
            _ => return None,
        })
    }
    /// A placeholder from the slide's layout (shows a prompt while empty).
    pub fn placeholder(self) -> bool {
        matches!(self, Kind::Title | Kind::Subtitle | Kind::Body)
    }
    pub fn has_text(self) -> bool {
        self != Kind::Picture
    }
    pub fn prompt(self) -> &'static str {
        match self {
            Kind::Title => "Click to add title",
            Kind::Subtitle => "Click to add subtitle",
            _ => "Click to add text",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    Top,
    Middle,
    Bottom,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Title,
    TitleContent,
    Section,
    TwoContent,
    TitleOnly,
    Blank,
}

pub const LAYOUTS: [Layout; 6] = [Layout::Title, Layout::TitleContent, Layout::Section, Layout::TwoContent, Layout::TitleOnly, Layout::Blank];

impl Layout {
    pub fn id(self) -> &'static str {
        match self {
            Layout::Title => "title",
            Layout::TitleContent => "content",
            Layout::Section => "section",
            Layout::TwoContent => "two",
            Layout::TitleOnly => "titleonly",
            Layout::Blank => "blank",
        }
    }
    pub fn from_id(s: &str) -> Option<Layout> {
        LAYOUTS.iter().copied().find(|l| l.id() == s)
    }
    pub fn name(self) -> &'static str {
        match self {
            Layout::Title => "Title slide",
            Layout::TitleContent => "Title and content",
            Layout::Section => "Section header",
            Layout::TwoContent => "Two columns",
            Layout::TitleOnly => "Title only",
            Layout::Blank => "Blank",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trans {
    None,
    Fade,
    Push,
}

impl Trans {
    pub fn id(self) -> &'static str {
        match self {
            Trans::None => "none",
            Trans::Fade => "fade",
            Trans::Push => "push",
        }
    }
    pub fn from_id(s: &str) -> Trans {
        match s {
            "fade" => Trans::Fade,
            "push" => Trans::Push,
            _ => Trans::None,
        }
    }
}

/// A decoration drawn under every slide of a theme. A negative `y` counts
/// from the bottom of the slide, a negative `x` from the right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Deco {
    pub ellipse: bool,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub color: u32,
}

impl Deco {
    pub fn place(&self, sw: i32, sh: i32) -> (i32, i32, i32, i32) {
        let x = if self.x < 0 { sw + self.x } else { self.x };
        let y = if self.y < 0 { sh + self.y } else { self.y };
        let w = if self.w == 0 { sw } else { self.w };
        (x, y, w, self.h)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Theme {
    /// "dune", "night" ... or "custom" (from an imported file)
    pub id: String,
    pub name: String,
    pub bg: u32,
    pub title: u32,
    pub text: u32,
    pub accent: u32,
    pub deco: Vec<Deco>,
}

const fn d(ellipse: bool, x: i32, y: i32, w: i32, h: i32, color: u32) -> Deco {
    Deco { ellipse, x, y, w, h, color }
}

pub const THEME_IDS: [&str; 6] = ["dune", "night", "paper", "lagos", "coral", "slate"];

pub fn theme(id: &str) -> Theme {
    let (name, bg, title, text, accent, deco): (&str, u32, u32, u32, u32, &[Deco]) = match id {
        "night" => ("Night", 0x1C1A27, 0xFFFFFF, 0xD9D4E6, 0xF2B544, &[d(true, -330, -330, 520, 520, 0x252236), d(false, 0, 0, 0, 8, 0xF2B544)]),
        "paper" => ("Paper", 0xFFFFFF, 0x111418, 0x3A3F47, 0x2F6FEB, &[d(false, 0, 0, 14, 720, 0x2F6FEB)]),
        "lagos" => ("Lagos", 0x0E5A43, 0xFFFFFF, 0xE3F1EA, 0xF5C542, &[d(false, 0, -18, 0, 18, 0xF5C542), d(true, -250, -420, 420, 420, 0x11684E)]),
        "coral" => ("Coral", 0xFFF4EF, 0x6E2718, 0x4A3530, 0xE8573A, &[d(true, -300, -170, 440, 440, 0xFBD5C9), d(true, -120, -80, 160, 160, 0xF7B8A6)]),
        "slate" => ("Slate", 0x22303C, 0xFFFFFF, 0xC9D6E2, 0x4FC3B8, &[d(false, 0, -10, 0, 10, 0x4FC3B8), d(false, -60, 0, 60, 720, 0x2A3A48)]),
        _ => ("Dune", 0xF7F1E8, 0x1E1B2C, 0x3A3548, 0xC0622B, &[d(true, -380, -150, 720, 300, 0xEDE0CC), d(false, 0, -12, 0, 12, 0xC0622B)]),
    };
    let id = if THEME_IDS.contains(&id) { id } else { "dune" };
    Theme { id: id.to_string(), name: name.to_string(), bg, title, text, accent, deco: deco.to_vec() }
}

/// Is a colour light (for choosing text on it)?
pub fn light(c: u32) -> bool {
    let (r, g, b) = ((c >> 16) & 255, (c >> 8) & 255, c & 255);
    r * 299 + g * 587 + b * 114 > 150_000
}

#[derive(Clone, PartialEq, Debug)]
pub struct Shape {
    pub kind: Kind,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub text: Doc,
    /// text size in points
    pub size: u16,
    pub fill: Option<u32>,
    pub line: Option<u32>,
    /// text colour (None: the theme's)
    pub color: Option<u32>,
    pub anchor: Anchor,
    /// index into `Deck::pics`
    pub pic: Option<usize>,
}

impl Shape {
    pub fn new(kind: Kind, x: i32, y: i32, w: i32, h: i32) -> Shape {
        let (size, anchor) = match kind {
            Kind::Title => (40, Anchor::Middle),
            Kind::Subtitle => (24, Anchor::Top),
            Kind::Body => (24, Anchor::Top),
            Kind::Rect | Kind::Ellipse => (20, Anchor::Middle),
            _ => (20, Anchor::Top),
        };
        let mut text = Doc::new();
        if kind == Kind::Body {
            text.paras[0].style = Style::Bullet;
        }
        if matches!(kind, Kind::Rect | Kind::Ellipse) {
            text.paras[0].align = Align::Center;
        }
        Shape { kind, x, y, w, h, text, size, fill: None, line: None, color: None, anchor, pic: None }
    }

    pub fn is_empty(&self) -> bool {
        self.text.paras.iter().all(|p| p.text.is_empty())
    }

    pub fn plain(&self) -> String {
        Doc::plain(&self.text.paras)
    }

    pub fn set_plain(&mut self, s: &str) {
        let style = self.text.paras[0].style;
        let align = self.text.paras[0].align;
        let mut d = Doc::new();
        d.paras[0].style = style;
        d.paras[0].align = align;
        let end = d.insert(Pos::new(0, 0), s, 0);
        let _ = end;
        self.text = d;
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Slide {
    pub layout: Layout,
    pub shapes: Vec<Shape>,
    pub notes: String,
    pub bg: Option<u32>,
    pub trans: Trans,
}

impl Slide {
    pub fn title(&self) -> String {
        self.shapes.iter().find(|s| s.kind == Kind::Title).map(|s| s.plain().replace('\n', " ")).unwrap_or_default()
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Pic {
    /// PNG or JPEG bytes (shared by undo snapshots)
    pub data: Rc<Vec<u8>>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Deck {
    pub name: String,
    pub w: i32,
    pub h: i32,
    pub theme: Theme,
    pub slides: Vec<Slide>,
    pub pics: Vec<Pic>,
}

impl Default for Deck {
    fn default() -> Deck {
        Deck::new()
    }
}

/// The placeholders of a layout on a slide `w` × `h` units.
pub fn layout_shapes(l: Layout, w: i32, h: i32) -> Vec<Shape> {
    // positions are designed for 1280 × 720 and stretched to the slide
    let sy = |y: i32| y * h / SLIDE_H;
    let sx = |x: i32| x * w / SLIDE_W;
    let mk = |k: Kind, x: i32, y: i32, ww: i32, hh: i32| Shape::new(k, sx(x), sy(y), sx(ww), sy(hh));
    match l {
        Layout::Title => {
            let mut t = mk(Kind::Title, 110, 190, 1060, 190);
            t.size = 54;
            t.anchor = Anchor::Bottom;
            t.text.paras[0].align = Align::Center;
            let mut s = mk(Kind::Subtitle, 110, 400, 1060, 110);
            s.text.paras[0].align = Align::Center;
            vec![t, s]
        }
        Layout::TitleContent => vec![mk(Kind::Title, 80, 36, 1120, 116), mk(Kind::Body, 80, 172, 1120, 480)],
        Layout::Section => {
            let mut t = mk(Kind::Title, 110, 250, 1060, 150);
            t.size = 48;
            t.anchor = Anchor::Bottom;
            let s = mk(Kind::Subtitle, 110, 414, 1060, 90);
            vec![t, s]
        }
        Layout::TwoContent => vec![mk(Kind::Title, 80, 36, 1120, 116), mk(Kind::Body, 80, 172, 545, 480), mk(Kind::Body, 655, 172, 545, 480)],
        Layout::TitleOnly => vec![mk(Kind::Title, 80, 36, 1120, 116)],
        Layout::Blank => vec![],
    }
}

impl Deck {
    pub fn new() -> Deck {
        let mut d = Deck { name: String::from("Untitled presentation"), w: SLIDE_W, h: SLIDE_H, theme: theme("dune"), slides: vec![], pics: vec![] };
        d.slides.push(d.new_slide(Layout::Title));
        d
    }

    pub fn new_slide(&self, l: Layout) -> Slide {
        Slide { layout: l, shapes: layout_shapes(l, self.w, self.h), notes: String::new(), bg: None, trans: Trans::None }
    }

    /// Change a slide's layout, carrying its text over to the new placeholders.
    pub fn relayout(&self, slide: &mut Slide, l: Layout) {
        let mut fresh = layout_shapes(l, self.w, self.h);
        let mut used = vec![false; slide.shapes.len()];
        for ph in fresh.iter_mut() {
            // the first unused old placeholder of the same kind (subtitles and bodies stand in for each other)
            let same = |k: Kind| k == ph.kind || (matches!(k, Kind::Subtitle | Kind::Body) && matches!(ph.kind, Kind::Subtitle | Kind::Body));
            if let Some(i) = slide.shapes.iter().enumerate().position(|(i, s)| !used[i] && s.kind.placeholder() && same(s.kind)) {
                used[i] = true;
                let old = &slide.shapes[i];
                let mut text = old.text.clone();
                for p in text.paras.iter_mut() {
                    if ph.kind == Kind::Body && old.kind != Kind::Body && p.style == Style::Body {
                        p.style = Style::Bullet;
                    } else if ph.kind != Kind::Body && old.kind == Kind::Body {
                        p.style = Style::Body;
                        p.level = 0;
                    }
                    if ph.kind != old.kind {
                        p.align = ph.text.paras[0].align;
                    }
                }
                ph.text = text;
                ph.color = old.color;
            }
        }
        // leftover placeholders with text become text boxes; other shapes stay
        let mut keep: Vec<Shape> = Vec::new();
        for (i, s) in slide.shapes.drain(..).enumerate() {
            if used[i] {
                continue;
            }
            if s.kind.placeholder() {
                if s.is_empty() {
                    continue;
                }
                let mut s = s;
                s.kind = Kind::Text;
                keep.push(s);
            } else {
                keep.push(s);
            }
        }
        fresh.extend(keep);
        slide.shapes = fresh;
        slide.layout = l;
    }

    /// Everything's text, for search.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for s in &self.slides {
            for sh in &s.shapes {
                if !sh.is_empty() {
                    out.push_str(&sh.plain());
                    out.push('\n');
                }
            }
            if !s.notes.is_empty() {
                out.push_str(&s.notes);
                out.push('\n');
            }
        }
        out
    }

    /// Drop pictures no shape uses and renumber the rest.
    pub fn prune_pics(&mut self) {
        let mut used = vec![false; self.pics.len()];
        for s in &self.slides {
            for sh in &s.shapes {
                if let Some(p) = sh.pic {
                    if p < used.len() {
                        used[p] = true;
                    }
                }
            }
        }
        let mut map = vec![usize::MAX; self.pics.len()];
        let mut k = 0;
        for i in 0..self.pics.len() {
            if used[i] {
                map[i] = k;
                k += 1;
            }
        }
        let mut i = 0;
        self.pics.retain(|_| {
            i += 1;
            used[i - 1]
        });
        for s in self.slides.iter_mut() {
            for sh in s.shapes.iter_mut() {
                sh.pic = sh.pic.and_then(|p| map.get(p).copied().filter(|&m| m != usize::MAX));
            }
        }
    }
}

// ---- text layout ------------------------------------------------------------------

/// One laid-out line of a shape's text, in slide units relative to the shape.
#[derive(Clone, Debug, PartialEq)]
pub struct TLine {
    pub p: usize,
    pub start: usize,
    pub end: usize,
    /// left of the text, and its width
    pub x: i32,
    pub w: i32,
    /// top of the line box and its height; baseline
    pub top: i32,
    pub h: i32,
    pub base: i32,
    /// font size in units (after shrinking to fit)
    pub px: i32,
    /// bullet or number, drawn at `bx` (first line of a paragraph only)
    pub bullet: Option<(i32, String)>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct TextBox {
    pub lines: Vec<TLine>,
    pub height: i32,
    /// text shrunk to this percentage to fit
    pub scale: i32,
}

pub fn face_for(kind: Kind, f: u8) -> Face {
    let bold = f & BOLD != 0 || kind == Kind::Title;
    match (bold, f & ITALIC != 0) {
        (false, false) => Face::Regular,
        (true, false) => Face::Semibold,
        (false, true) => Face::Italic,
        (true, true) => Face::SemiboldItalic,
    }
}

/// Width (units) of characters [s, e) of a paragraph at `px`.
pub fn span_w(kind: Kind, p: &Para, s: usize, e: usize, px: i32) -> i32 {
    let mut w = 0;
    let mut i = s;
    while i < e {
        let f = p.fmt[i];
        let mut j = i;
        let mut run = String::new();
        while j < e && p.fmt[j] == f {
            let c = p.text[j];
            run.push(if c == '\t' { ' ' } else { c });
            j += 1;
        }
        w += font::measure(face_for(kind, f), px, &run);
        i = j;
    }
    w
}

/// Font size (units) of a paragraph at `scale` percent.
pub fn para_px(sh: &Shape, p: &Para, scale: i32) -> i32 {
    let pt = (sh.size as i32 - 4 * p.level as i32).max(sh.size as i32 * 3 / 5).max(8);
    (pt * 4 * scale / 300).max(4)
}

fn indent(p: &Para) -> (i32, i32) {
    let lvl = p.level as i32 * 44;
    match p.style {
        Style::Bullet | Style::Number => (lvl, lvl + 34),
        _ => (lvl, lvl),
    }
}

fn lay_at(sh: &Shape, scale: i32) -> TextBox {
    let inner_w = (sh.w - 2 * INSET_X).max(8);
    let mut lines = Vec::new();
    let mut y = 0;
    let mut numbers = [0u32; 9];
    for (pi, p) in sh.text.paras.iter().enumerate() {
        let px = para_px(sh, p, scale);
        let lh = px * 6 / 5;
        if pi > 0 && sh.kind == Kind::Body {
            y += px * 2 / 5;
        }
        let (bx, tx) = indent(p);
        let lvl = (p.level as usize).min(8);
        let bullet = match p.style {
            Style::Bullet => Some((bx, String::from(if p.level % 2 == 0 { "•" } else { "–" }))),
            Style::Number => {
                numbers[lvl] += 1;
                Some((bx, alloc::format!("{}.", numbers[lvl])))
            }
            _ => None,
        };
        if p.style != Style::Number {
            numbers[lvl] = 0;
        }
        for n in numbers.iter_mut().skip(lvl + 1) {
            *n = 0;
        }
        let avail = (inner_w - tx).max(px);
        // break into lines at spaces
        let n = p.text.len();
        let mut start = 0;
        let mut first = true;
        loop {
            let trim = |mut e: usize| {
                while e > start && p.text[e - 1] == ' ' {
                    e -= 1;
                }
                e
            };
            // take whole words while they fit (the first word always)
            let mut stop = start;
            let mut too_wide = false;
            loop {
                let mut j = stop;
                while j < n && p.text[j] == ' ' {
                    j += 1;
                }
                while j < n && p.text[j] != ' ' {
                    j += 1;
                }
                if j == stop {
                    break;
                }
                if span_w(sh.kind, p, start, trim(j), px) <= avail {
                    stop = j;
                } else {
                    if stop == start {
                        stop = j;
                        too_wide = true;
                    }
                    break;
                }
            }
            // spaces after the last word hang at the end of the line
            while stop < n && p.text[stop] == ' ' {
                stop += 1;
            }
            if too_wide {
                // a single word wider than the box: break it by characters
                let mut k = start + 1;
                while k < n && span_w(sh.kind, p, start, k + 1, px) <= avail {
                    k += 1;
                }
                stop = k.min(n);
            }
            let vis = trim(stop);
            let w = span_w(sh.kind, p, start, vis, px);
            let x = match p.align {
                Align::Left => tx,
                Align::Center => tx + (avail - w) / 2,
                Align::Right => tx + avail - w,
            };
            lines.push(TLine { p: pi, start, end: stop, x: INSET_X + x, w, top: y, h: lh, base: y + px * 23 / 25, px, bullet: if first { bullet.clone().map(|(b, s)| (INSET_X + b, s)) } else { None } });
            y += lh;
            first = false;
            if stop >= n {
                break;
            }
            start = stop;
        }
    }
    TextBox { lines, height: y, scale }
}

/// Lay out a shape's text, shrinking it (down to 40%) when it overflows.
pub fn layout(sh: &Shape) -> TextBox {
    let room = sh.h - 2 * INSET_Y;
    let mut scale = 100;
    let mut tb = lay_at(sh, scale);
    if sh.kind != Kind::Picture {
        while tb.height > room && scale > 40 {
            scale = (scale - 8).max(40);
            tb = lay_at(sh, scale);
        }
    }
    // vertical anchoring
    let off = INSET_Y
        + match sh.anchor {
            Anchor::Top => 0,
            Anchor::Middle => (room - tb.height) / 2,
            Anchor::Bottom => room - tb.height,
        };
    for l in tb.lines.iter_mut() {
        l.top += off;
        l.base += off;
    }
    tb
}

/// The caret position nearest (x, y) (units, relative to the shape).
pub fn hit(sh: &Shape, tb: &TextBox, x: i32, y: i32) -> Pos {
    if tb.lines.is_empty() {
        return Pos::new(0, 0);
    }
    let li = tb.lines.iter().position(|l| y < l.top + l.h).unwrap_or(tb.lines.len() - 1);
    let l = &tb.lines[li];
    let p = &sh.text.paras[l.p];
    let mut best = l.start;
    let mut best_d = i32::MAX;
    // the end of a wrapped line sits before its trailing space
    let mut last = l.end;
    if li + 1 < tb.lines.len() && tb.lines[li + 1].p == l.p && last > l.start && p.text[last - 1] == ' ' {
        last -= 1;
    }
    for i in l.start..=last {
        let cx = l.x + span_w(sh.kind, p, l.start, i, l.px);
        let dd = (cx - x).abs();
        if dd < best_d {
            best_d = dd;
            best = i;
        }
    }
    Pos::new(l.p, best)
}

/// Line index and x (units, relative to the shape) of a caret position.
pub fn caret_xy(sh: &Shape, tb: &TextBox, pos: Pos) -> (usize, i32) {
    let mut found = 0;
    for (li, l) in tb.lines.iter().enumerate() {
        if l.p == pos.p && pos.i >= l.start && (pos.i < l.end || pos.i == l.end && (li + 1 == tb.lines.len() || tb.lines[li + 1].p != l.p)) {
            found = li;
            break;
        }
        if l.p == pos.p && pos.i >= l.start {
            found = li;
        }
    }
    let l = &tb.lines.get(found).cloned().unwrap_or(TLine { p: 0, start: 0, end: 0, x: INSET_X, w: 0, top: INSET_Y, h: 20, base: 16, px: 16, bullet: None });
    let p = &sh.text.paras[l.p.min(sh.text.paras.len() - 1)];
    let x = l.x + span_w(sh.kind, p, l.start, pos.i.min(p.len()).max(l.start), l.px);
    (found, x)
}
