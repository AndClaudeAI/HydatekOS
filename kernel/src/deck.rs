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
    /// a straight line, optionally with arrowheads
    Line,
    Table,
    Chart,
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
            Kind::Line => "line",
            Kind::Table => "table",
            Kind::Chart => "chart",
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
            "line" => Kind::Line,
            "table" => Kind::Table,
            "chart" => Kind::Chart,
            _ => return None,
        })
    }
    /// A placeholder from the slide's layout (shows a prompt while empty).
    pub fn placeholder(self) -> bool {
        matches!(self, Kind::Title | Kind::Subtitle | Kind::Body)
    }
    /// Holds text of its own (tables keep theirs in their cells).
    pub fn has_text(self) -> bool {
        !matches!(self, Kind::Picture | Kind::Line | Kind::Table | Kind::Chart)
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


/// The outline of a rectangle-kind shape (named as in PowerPoint).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Geom {
    Rect,
    RoundRect,
    Triangle,
    RtTriangle,
    Diamond,
    Pentagon,
    Hexagon,
    Octagon,
    Star5,
    RightArrow,
    LeftArrow,
    UpArrow,
    DownArrow,
    Chevron,
    Parallelogram,
    Trapezoid,
}

pub const GEOMS: [Geom; 16] = [
    Geom::Rect,
    Geom::RoundRect,
    Geom::Triangle,
    Geom::RtTriangle,
    Geom::Diamond,
    Geom::Pentagon,
    Geom::Hexagon,
    Geom::Octagon,
    Geom::Star5,
    Geom::RightArrow,
    Geom::LeftArrow,
    Geom::UpArrow,
    Geom::DownArrow,
    Geom::Chevron,
    Geom::Parallelogram,
    Geom::Trapezoid,
];

impl Geom {
    /// PowerPoint's preset name.
    pub fn id(self) -> &'static str {
        match self {
            Geom::Rect => "rect",
            Geom::RoundRect => "roundRect",
            Geom::Triangle => "triangle",
            Geom::RtTriangle => "rtTriangle",
            Geom::Diamond => "diamond",
            Geom::Pentagon => "pentagon",
            Geom::Hexagon => "hexagon",
            Geom::Octagon => "octagon",
            Geom::Star5 => "star5",
            Geom::RightArrow => "rightArrow",
            Geom::LeftArrow => "leftArrow",
            Geom::UpArrow => "upArrow",
            Geom::DownArrow => "downArrow",
            Geom::Chevron => "chevron",
            Geom::Parallelogram => "parallelogram",
            Geom::Trapezoid => "trapezoid",
        }
    }
    pub fn from_id(s: &str) -> Option<Geom> {
        GEOMS.iter().copied().find(|g| g.id() == s)
    }
    pub fn name(self) -> &'static str {
        match self {
            Geom::Rect => "Rectangle",
            Geom::RoundRect => "Rounded rectangle",
            Geom::Triangle => "Triangle",
            Geom::RtTriangle => "Right triangle",
            Geom::Diamond => "Diamond",
            Geom::Pentagon => "Pentagon",
            Geom::Hexagon => "Hexagon",
            Geom::Octagon => "Octagon",
            Geom::Star5 => "Star",
            Geom::RightArrow => "Right arrow",
            Geom::LeftArrow => "Left arrow",
            Geom::UpArrow => "Up arrow",
            Geom::DownArrow => "Down arrow",
            Geom::Chevron => "Chevron",
            Geom::Parallelogram => "Parallelogram",
            Geom::Trapezoid => "Trapezoid",
        }
    }
}

/// An entrance animation, played by clicks in the slideshow.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anim {
    None,
    Appear,
    Fade,
    /// flies in from the bottom
    Fly,
}

impl Anim {
    pub fn id(self) -> &'static str {
        match self {
            Anim::None => "none",
            Anim::Appear => "appear",
            Anim::Fade => "fade",
            Anim::Fly => "fly",
        }
    }
    pub fn from_id(s: &str) -> Anim {
        match s {
            "appear" => Anim::Appear,
            "fade" => Anim::Fade,
            "fly" => Anim::Fly,
            _ => Anim::None,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Table {
    /// column widths and row heights, in units (rows grow to fit their text)
    pub cols: Vec<i32>,
    pub rows: Vec<i32>,
    /// the cells, row by row
    pub cells: Vec<Doc>,
    /// the first row is a heading row
    pub header: bool,
    /// rows alternate shades
    pub banded: bool,
}

impl Table {
    pub fn new(rows: usize, cols: usize, w: i32, h: i32) -> Table {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let mut cw = vec![w / cols as i32; cols];
        cw[cols - 1] += w - w / cols as i32 * cols as i32;
        Table { cols: cw, rows: vec![h / rows as i32; rows], cells: vec![Doc::new(); rows * cols], header: true, banded: true }
    }
    pub fn nrows(&self) -> usize {
        self.rows.len()
    }
    pub fn ncols(&self) -> usize {
        self.cols.len()
    }
    pub fn cell(&self, r: usize, c: usize) -> &Doc {
        &self.cells[r * self.ncols() + c]
    }
    pub fn cell_mut(&mut self, r: usize, c: usize) -> &mut Doc {
        let n = self.ncols();
        &mut self.cells[r * n + c]
    }
    pub fn insert_row(&mut self, at: usize) {
        let at = at.min(self.nrows());
        let h = self.rows.get(at.min(self.nrows() - 1)).copied().unwrap_or(40);
        self.rows.insert(at, h);
        let n = self.ncols();
        for k in 0..n {
            self.cells.insert(at * n + k, Doc::new());
        }
    }
    pub fn delete_row(&mut self, r: usize) {
        if self.nrows() <= 1 || r >= self.nrows() {
            return;
        }
        let n = self.ncols();
        self.rows.remove(r);
        self.cells.drain(r * n..r * n + n);
    }
    /// A new column takes half of the column it's put beside.
    pub fn insert_col(&mut self, at: usize) {
        let at = at.min(self.ncols());
        let src = at.min(self.ncols() - 1);
        let half = self.cols[src] / 2;
        self.cols[src] -= half;
        self.cols.insert(at, half.max(20));
        let old = self.ncols() - 1;
        for r in (0..self.nrows()).rev() {
            self.cells.insert(r * old + at, Doc::new());
        }
    }
    pub fn delete_col(&mut self, c: usize) {
        if self.ncols() <= 1 || c >= self.ncols() {
            return;
        }
        let n = self.ncols();
        let w = self.cols.remove(c);
        let give = if c < self.ncols() { c } else { c - 1 };
        self.cols[give] += w;
        for r in (0..self.nrows()).rev() {
            self.cells.remove(r * n + c);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChartKind {
    Column,
    Bar,
    Line,
    Area,
    Pie,
}

pub const CHART_KINDS: [ChartKind; 5] = [ChartKind::Column, ChartKind::Bar, ChartKind::Line, ChartKind::Area, ChartKind::Pie];

impl ChartKind {
    pub fn id(self) -> &'static str {
        match self {
            ChartKind::Column => "column",
            ChartKind::Bar => "bar",
            ChartKind::Line => "line",
            ChartKind::Area => "area",
            ChartKind::Pie => "pie",
        }
    }
    pub fn from_id(s: &str) -> ChartKind {
        CHART_KINDS.iter().copied().find(|k| k.id() == s).unwrap_or(ChartKind::Column)
    }
    pub fn name(self) -> &'static str {
        match self {
            ChartKind::Column => "Column",
            ChartKind::Bar => "Bar",
            ChartKind::Line => "Line",
            ChartKind::Area => "Area",
            ChartKind::Pie => "Pie",
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Series {
    pub name: String,
    pub vals: Vec<f64>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Chart {
    pub kind: ChartKind,
    pub title: String,
    pub cats: Vec<String>,
    pub series: Vec<Series>,
    pub legend: bool,
}

impl Chart {
    pub fn sample(kind: ChartKind) -> Chart {
        let s = |name: &str, v: [f64; 4]| Series { name: name.into(), vals: v.to_vec() };
        let series = if kind == ChartKind::Pie { vec![s("Share", [45.0, 25.0, 18.0, 12.0])] } else { vec![s("2025", [4.3, 2.5, 3.5, 4.5]), s("2026", [5.1, 3.9, 4.2, 6.0])] };
        let cats = if kind == ChartKind::Pie { vec!["Solar", "Fintech", "Software", "Other"] } else { vec!["Q1", "Q2", "Q3", "Q4"] };
        Chart { kind, title: String::new(), cats: cats.iter().map(|c| c.to_string()).collect(), series, legend: true }
    }
    /// Smallest and largest values (0 always included).
    pub fn range(&self) -> (f64, f64) {
        let (mut lo, mut hi) = (0.0f64, 0.0f64);
        for s in &self.series {
            for &v in &s.vals {
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
        }
        if hi == lo {
            hi = lo + 1.0;
        }
        (lo, hi)
    }
}

/// Round axis steps: (first tick, step, count) covering [lo, hi] in about 5 steps.
pub fn nice_ticks(lo: f64, hi: f64) -> (f64, f64, usize) {
    let span = (hi - lo).max(1e-9);
    let raw = span / 5.0;
    let mut mag = 1.0f64;
    while mag * 10.0 <= raw {
        mag *= 10.0;
    }
    while mag > raw {
        mag /= 10.0;
    }
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let first = floor(lo / step) * step;
    let mut n = 0;
    while first + step * (n as f64) < hi - step * 1e-9 {
        n += 1;
    }
    (first, step, n.max(1))
}

fn floor(x: f64) -> f64 {
    let t = x as i64 as f64;
    if t > x {
        t - 1.0
    } else {
        t
    }
}

/// A number for an axis or a data sheet: no needless decimals.
pub fn fmt_num(v: f64) -> String {
    let neg = v < 0.0;
    let a = if neg { -v } else { v };
    let scaled = (a * 100.0 + 0.5) as u64;
    let (int, frac) = (scaled / 100, scaled % 100);
    let mut s = if frac == 0 {
        alloc::format!("{}", int)
    } else if frac % 10 == 0 {
        alloc::format!("{}.{}", int, frac / 10)
    } else {
        alloc::format!("{}.{:02}", int, frac)
    };
    if neg && scaled != 0 {
        s.insert(0, '-');
    }
    s
}

/// Parse a number typed in the chart's data (commas and spaces ignored).
pub fn parse_num(s: &str) -> Option<f64> {
    let t: String = s.chars().filter(|c| !matches!(c, ',' | ' ' | '₦' | '$' | '%')).collect();
    if t.is_empty() {
        return None;
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_string()),
        None => (false, t),
    };
    let (i, f) = t.split_once('.').unwrap_or((&t, ""));
    if !i.chars().all(|c| c.is_ascii_digit()) || !f.chars().all(|c| c.is_ascii_digit()) || (i.is_empty() && f.is_empty()) {
        return None;
    }
    let mut v = 0.0f64;
    for c in i.chars() {
        v = v * 10.0 + (c as u8 - b'0') as f64;
    }
    let mut scale = 0.1;
    for c in f.chars() {
        v += (c as u8 - b'0') as f64 * scale;
        scale /= 10.0;
    }
    Some(if neg { -v } else { v })
}

// ---- angles and outlines ------------------------------------------------------------

const SIN: [i32; 91] = [
    0, 286, 572, 857, 1143, 1428, 1713, 1997, 2280, 2563, 2845, 3126, 3406, 3686, 3964, 4240, 4516, 4790, 5063, 5334, 5604, 5872, 6138, 6402, 6664, 6924, 7182, 7438, 7692, 7943, 8192, 8438, 8682, 8923, 9162, 9397, 9630, 9860, 10087, 10311, 10531, 10749, 10963, 11174, 11381, 11585, 11786, 11982, 12176, 12365, 12551, 12733, 12911, 13085, 13255, 13421, 13583, 13741, 13894, 14044, 14189,
    14330, 14466, 14598, 14726, 14849, 14968, 15082, 15191, 15296, 15396, 15491, 15582, 15668, 15749, 15826, 15897, 15964, 16026, 16083, 16135, 16182, 16225, 16262, 16294, 16322, 16344, 16362, 16374, 16382, 16384,
];

/// sin of whole degrees, times 16384.
pub fn sin_deg(d: i32) -> i32 {
    let d = d.rem_euclid(360);
    match d {
        0..=90 => SIN[d as usize],
        91..=180 => SIN[(180 - d) as usize],
        181..=270 => -SIN[(d - 180) as usize],
        _ => -SIN[(360 - d) as usize],
    }
}

pub fn cos_deg(d: i32) -> i32 {
    sin_deg(d + 90)
}

/// Rotate (x, y) by `deg` (clockwise on screen) around (cx, cy).
pub fn rotate(x: i32, y: i32, cx: i32, cy: i32, deg: i32) -> (i32, i32) {
    if deg % 360 == 0 {
        return (x, y);
    }
    let (s, c) = (sin_deg(deg) as i64, cos_deg(deg) as i64);
    let (dx, dy) = ((x - cx) as i64, (y - cy) as i64);
    (cx + ((dx * c - dy * s) >> 14) as i32, cy + ((dx * s + dy * c) >> 14) as i32)
}

/// The outline of a shape in its own box (0,0)-(w,h), in 1/16 units.
pub fn outline(kind: Kind, geom: Geom, w: i32, h: i32) -> Vec<(i32, i32)> {
    let (w, h) = (w.max(1) * 16, h.max(1) * 16);
    let ss = w.min(h);
    let ell = |n: i32, rx: i32, ry: i32, cx: i32, cy: i32, start: i32| -> Vec<(i32, i32)> {
        (0..n).map(|k| {
            let a = start + k * 360 / n;
            (cx + (rx as i64 * cos_deg(a) as i64 >> 14) as i32, cy + (ry as i64 * sin_deg(a) as i64 >> 14) as i32)
        }).collect()
    };
    if kind == Kind::Ellipse {
        return ell(90, w / 2, h / 2, w / 2, h / 2, 0);
    }
    match geom {
        Geom::Rect => vec![(0, 0), (w, 0), (w, h), (0, h)],
        Geom::RoundRect => {
            let r = ss / 6;
            let mut p = Vec::new();
            for (cx, cy, a0) in [(w - r, r, 270), (w - r, h - r, 0), (r, h - r, 90), (r, r, 180)] {
                for k in 0..=9 {
                    let a = a0 + k * 10;
                    p.push((cx + (r as i64 * cos_deg(a) as i64 >> 14) as i32, cy + (r as i64 * sin_deg(a) as i64 >> 14) as i32));
                }
            }
            p
        }
        Geom::Triangle => vec![(w / 2, 0), (w, h), (0, h)],
        Geom::RtTriangle => vec![(0, 0), (w, h), (0, h)],
        Geom::Diamond => vec![(w / 2, 0), (w, h / 2), (w / 2, h), (0, h / 2)],
        Geom::Pentagon => ell(5, w / 2, h / 2, w / 2, h / 2 + h / 20, 270),
        Geom::Hexagon => {
            let a = ss / 4;
            vec![(a, 0), (w - a, 0), (w, h / 2), (w - a, h), (a, h), (0, h / 2)]
        }
        Geom::Octagon => {
            let a = ss * 29 / 100;
            vec![(a, 0), (w - a, 0), (w, a), (w, h - a), (w - a, h), (a, h), (0, h - a), (0, a)]
        }
        Geom::Star5 => {
            let (cx, cy) = (w / 2, h / 2 + h / 20);
            let mut p = Vec::new();
            for k in 0..10 {
                let a = 270 + k * 36;
                let (rx, ry) = if k % 2 == 0 { (w / 2, h / 2) } else { (w * 19 / 100, h * 19 / 100) };
                p.push((cx + (rx as i64 * cos_deg(a) as i64 >> 14) as i32, cy + (ry as i64 * sin_deg(a) as i64 >> 14) as i32));
            }
            p
        }
        Geom::RightArrow => {
            let xh = w - ss / 2;
            vec![(0, h / 4), (xh, h / 4), (xh, 0), (w, h / 2), (xh, h), (xh, h * 3 / 4), (0, h * 3 / 4)]
        }
        Geom::LeftArrow => {
            let xh = ss / 2;
            vec![(w, h / 4), (xh, h / 4), (xh, 0), (0, h / 2), (xh, h), (xh, h * 3 / 4), (w, h * 3 / 4)]
        }
        Geom::DownArrow => {
            let yh = h - ss / 2;
            vec![(w / 4, 0), (w / 4, yh), (0, yh), (w / 2, h), (w, yh), (w * 3 / 4, yh), (w * 3 / 4, 0)]
        }
        Geom::UpArrow => {
            let yh = ss / 2;
            vec![(w / 4, h), (w / 4, yh), (0, yh), (w / 2, 0), (w, yh), (w * 3 / 4, yh), (w * 3 / 4, h)]
        }
        Geom::Chevron => {
            let a = ss / 2;
            vec![(0, 0), (w - a, 0), (w, h / 2), (w - a, h), (0, h), (a, h / 2)]
        }
        Geom::Parallelogram => {
            let a = ss / 4;
            vec![(a, 0), (w, 0), (w - a, h), (0, h)]
        }
        Geom::Trapezoid => {
            let a = ss / 4;
            vec![(0, h), (a, 0), (w - a, 0), (w, h)]
        }
    }
}

/// A line's two ends (start, end) in slide units.
pub fn line_ends(sh: &Shape) -> ((i32, i32), (i32, i32)) {
    let (x0, x1) = if sh.flip_h { (sh.x + sh.w, sh.x) } else { (sh.x, sh.x + sh.w) };
    let (y0, y1) = if sh.flip_v { (sh.y + sh.h, sh.y) } else { (sh.y, sh.y + sh.h) };
    ((x0, y0), (x1, y1))
}

/// Set a line from its two ends.
pub fn set_line_ends(sh: &mut Shape, a: (i32, i32), b: (i32, i32)) {
    sh.x = a.0.min(b.0);
    sh.y = a.1.min(b.1);
    sh.w = (a.0 - b.0).abs();
    sh.h = (a.1 - b.1).abs();
    sh.flip_h = a.0 > b.0;
    sh.flip_v = a.1 > b.1;
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
    /// outline of a rectangle-kind shape
    pub geom: Geom,
    /// clockwise, in degrees
    pub rot: i32,
    pub flip_h: bool,
    pub flip_v: bool,
    /// a gradient from the fill to this colour, at this angle (0: left to right, 90: top to bottom)
    pub grad: Option<(u32, i32)>,
    /// outline / line width in units
    pub line_w: i32,
    /// arrowheads at a line's start and end
    pub head: bool,
    pub tail: bool,
    pub anim: Anim,
    /// order among the slide's animations
    pub anim_order: u16,
    pub table: Option<Table>,
    pub chart: Option<Chart>,
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
        let mut sh = Shape { kind, x, y, w, h, text, size, fill: None, line: None, color: None, anchor, pic: None, geom: Geom::Rect, rot: 0, flip_h: false, flip_v: false, grad: None, line_w: 3, head: false, tail: false, anim: Anim::None, anim_order: 0, table: None, chart: None };
        match kind {
            Kind::Table => {
                sh.size = 18;
                sh.table = Some(Table::new(3, 3, w, h));
            }
            Kind::Chart => sh.chart = Some(Chart::sample(ChartKind::Column)),
            Kind::Line => sh.tail = true,
            _ => {}
        }
        sh
    }

    /// A point in slide units, in this shape's own (unrotated) frame.
    pub fn to_local(&self, x: i32, y: i32) -> (i32, i32) {
        rotate(x, y, self.x + self.w / 2, self.y + self.h / 2, -self.rot)
    }

    /// The text box of a table's cell, as a shape of its own.
    pub fn cell_shape(&self, r: usize, c: usize) -> Option<Shape> {
        let t = self.table.as_ref()?;
        if r >= t.nrows() || c >= t.ncols() {
            return None;
        }
        let x = self.x + t.cols[..c].iter().sum::<i32>();
        let y = self.y + t.rows[..r].iter().sum::<i32>();
        let mut s = Shape::new(Kind::Text, x, y, t.cols[c], t.rows[r]);
        s.text = t.cell(r, c).clone();
        s.size = self.size;
        s.anchor = Anchor::Middle;
        s.color = self.color;
        Some(s)
    }

    /// Grow a table's rows to fit their text, and the shape around them.
    pub fn fit_table(&mut self) {
        let Some(t) = self.table.clone() else { return };
        let mut t = t;
        let total: i32 = t.cols.iter().sum();
        if total != self.w && total > 0 {
            // columns follow the shape's width
            let mut acc = 0;
            let n = t.ncols();
            for (i, c) in t.cols.iter_mut().enumerate() {
                *c = if i + 1 == n { self.w - acc } else { (*c as i64 * self.w as i64 / total as i64) as i32 };
                acc += *c;
            }
        }
        for r in 0..t.nrows() {
            let mut need = self.size as i32 * 4 / 3 * 6 / 5 + 2 * INSET_Y + 8;
            for c in 0..t.ncols() {
                let x: i32 = t.cols[..c].iter().sum();
                let mut s = Shape::new(Kind::Text, x, 0, t.cols[c], 10_000);
                s.text = t.cell(r, c).clone();
                s.size = self.size;
                let tb = layout_at(&s, 100);
                need = need.max(tb.height + 2 * INSET_Y + 8);
            }
            t.rows[r] = t.rows[r].max(need).max(20);
        }
        self.h = t.rows.iter().sum();
        self.table = Some(t);
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
        if self.kind == Kind::Line {
            // near the line
            let ((x0, y0), (x1, y1)) = line_ends(self);
            let (dx, dy) = ((x1 - x0) as i64, (y1 - y0) as i64);
            let len2 = (dx * dx + dy * dy).max(1);
            let t = (((x - x0) as i64 * dx + (y - y0) as i64 * dy) * 1024 / len2).clamp(0, 1024);
            let (px, py) = (x0 as i64 + dx * t / 1024, y0 as i64 + dy * t / 1024);
            let (ex, ey) = (x as i64 - px, y as i64 - py);
            return ex * ex + ey * ey <= (10 + self.line_w as i64).pow(2);
        }
        let (x, y) = self.to_local(x, y);
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Slide {
    pub layout: Layout,
    pub shapes: Vec<Shape>,
    pub notes: String,
    pub bg: Option<u32>,
    /// a gradient from the background to this colour, at this angle
    pub bg_grad: Option<(u32, i32)>,
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
    /// footer text and slide numbers, on every slide but title slides
    pub footer: String,
    pub numbers: bool,
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
        let mut d = Deck { name: String::from("Untitled presentation"), w: SLIDE_W, h: SLIDE_H, theme: theme("dune"), slides: vec![], pics: vec![], footer: String::new(), numbers: false };
        d.slides.push(d.new_slide(Layout::Title));
        d
    }

    pub fn new_slide(&self, l: Layout) -> Slide {
        Slide { layout: l, shapes: layout_shapes(l, self.w, self.h), notes: String::new(), bg: None, bg_grad: None, trans: Trans::None }
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
                if sh.kind.has_text() && !sh.is_empty() {
                    out.push_str(&sh.plain());
                    out.push('\n');
                }
                if let Some(t) = &sh.table {
                    for c in &t.cells {
                        out.push_str(&Doc::plain(&c.paras));
                        out.push(' ');
                    }
                    out.push('\n');
                }
                if let Some(c) = &sh.chart {
                    out.push_str(&c.title);
                    for x in &c.cats {
                        out.push(' ');
                        out.push_str(x);
                    }
                    for x in &c.series {
                        out.push(' ');
                        out.push_str(&x.name);
                    }
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

pub fn layout_at(sh: &Shape, scale: i32) -> TextBox {
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
    let mut tb = layout_at(sh, scale);
    if sh.kind != Kind::Picture {
        while tb.height > room && scale > 40 {
            scale = (scale - 8).max(40);
            tb = layout_at(sh, scale);
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

// ---- colours and places shared by the renderer and the exporters ----------------------

/// Blend colour `a` towards `b` by `t`/256.
pub fn mix(a: u32, b: u32, t: u32) -> u32 {
    let t = t.min(256);
    let f = |s: u32| ((((a >> s) & 255) * (256 - t) + ((b >> s) & 255) * t) >> 8) << s;
    f(16) | f(8) | f(0)
}

/// Readable text on a fill.
pub fn on(fill: u32) -> u32 {
    if light(fill) {
        0x1E1B2C
    } else {
        0xFFFFFF
    }
}

/// Colour of a chart's series (or a pie's slice) `i`.
pub fn series_color(t: &Theme, i: usize) -> u32 {
    const MORE: [u32; 5] = [0x4F7CAC, 0x5B9B6B, 0xD9A441, 0x8E6CB5, 0xC2504F];
    if i == 0 {
        t.accent
    } else {
        MORE[(i - 1) % MORE.len()]
    }
}

/// A table cell's fill and text colour, and whether its text is bold.
pub fn cell_style(t: &Theme, tb: &Table, r: usize) -> (u32, u32, bool) {
    if tb.header && r == 0 {
        return (t.accent, on(t.accent), true);
    }
    let data_row = if tb.header { r - 1 } else { r };
    let fill = if tb.banded && data_row % 2 == 0 { mix(t.bg, t.accent, 34) } else { mix(t.bg, t.text, 8) };
    (fill, t.text, false)
}

/// The lines between table cells.
pub fn table_line(t: &Theme) -> u32 {
    mix(t.bg, t.text, 70)
}

/// Where the footer text and the slide number go.
pub fn footer_rects(w: i32, h: i32) -> ((i32, i32, i32, i32), (i32, i32, i32, i32)) {
    ((w * 3 / 10, h - 54, w * 4 / 10, 40), (w - 210, h - 54, 150, 40))
}

pub fn footer_color(t: &Theme) -> u32 {
    mix(t.bg, t.text, 170)
}

/// Does this slide show the footer and number? (Not on title slides.)
pub fn shows_footer(d: &Deck, s: &Slide) -> bool {
    (d.numbers || !d.footer.is_empty()) && s.layout != Layout::Title
}
