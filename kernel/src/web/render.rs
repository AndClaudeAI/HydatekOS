//! CSS and layout: style rules (type, class, id and descendant selectors with
//! specificity), inherited and box properties, and a block / inline flow layout
//! with text wrapping, lists, simple tables and form controls. The result is a
//! display list the browser draws.

use super::css::{self, Cascade, Media};
use super::html::{Dom, Kind, NodeId};
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use crate::font::{self, Face};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---- colours -----------------------------------------------------------------------

/// Some(Some(rgb)), Some(None) for transparent, None if not a colour.
fn color(v: &str) -> Option<Option<u32>> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("none") {
        return Some(None);
    }
    let (c, a) = crate::image::color::parse(v)?;
    // boxes aren't blended yet: mostly transparent counts as none
    Some(if a < 77 { None } else { Some(c) })
}

// ---- computed style -----------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Disp {
    Block,
    Inline,
    ListItem,
    None,
    Table,
    Row,
    Cell,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
struct Style {
    disp: Disp,
    size: i32,
    bold: bool,
    italic: bool,
    mono: bool,
    color: u32,
    bg: Option<u32>,
    align: Align,
    underline: bool,
    strike: bool,
    pre: bool,
    margin: [i32; 4],
    padding: [i32; 4],
    width: Option<i32>,
    height: Option<i32>,
    max_width: Option<i32>,
    center_box: bool,
    border: Option<u32>,
    link: Option<usize>,
    bg_img: Option<Background>,
    /// custom properties (--name), inherited
    vars: Rc<BTreeMap<String, String>>,
    /// text-transform: 0 none, 1 upper, 2 lower, 3 capitalise
    case: u8,
    /// positioned out of the flow (absolute / fixed)
    out_of_flow: bool,
    /// clipped to nothing
    clipped: bool,
    /// moved far off-screen
    offscreen: bool,
    /// max-height: 0 (a closed menu)
    collapsed: bool,
    overflow_hidden: bool,
    /// text-indent far to the left: the text is hidden (the box isn't)
    no_text: bool,
    /// the background, border and padding of the inline element around text
    ibox: Option<IBox>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct IBox {
    bg: Option<u32>,
    border: Option<u32>,
    pad: [i32; 4],
}

/// A length that may be a percentage (of the box) or automatic.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dim {
    Auto,
    Px(i32),
    Pct(i32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BgSize {
    Auto,
    Cover,
    Contain,
    Set(Dim, Dim),
}

/// A CSS background image.
#[derive(Clone, Debug, PartialEq)]
pub struct Background {
    pub url: String,
    pub size: BgSize,
    pub pos: (Dim, Dim),
    /// repeat across, repeat down
    pub repeat: (bool, bool),
}

fn dim(v: &str, em: i32) -> Option<Dim> {
    let v = v.trim();
    if v == "auto" {
        return Some(Dim::Auto);
    }
    if let Some(p) = v.strip_suffix('%') {
        return p.trim().parse::<f64>().ok().map(|x| Dim::Pct(x as i32));
    }
    length(v, em, 0).map(Dim::Px)
}

/// The url(...) in a background value.
fn css_url(v: &str) -> Option<String> {
    let i = v.find("url(")?;
    let rest = &v[i + 4..];
    let end = rest.find(')')?;
    let u = rest[..end].trim().trim_matches(|c| c == '"' || c == '\'');
    (!u.is_empty()).then(|| u.to_string())
}

fn bg_position(s: &mut Background, words: &[&str], em: i32) {
    let mut pos = (Dim::Pct(0), Dim::Pct(0));
    let mut k = 0;
    for w in words {
        match *w {
            "left" => pos.0 = Dim::Pct(0),
            "right" => pos.0 = Dim::Pct(100),
            "top" => pos.1 = Dim::Pct(0),
            "bottom" => pos.1 = Dim::Pct(100),
            "center" => {
                if k == 0 && words.len() == 1 {
                    pos = (Dim::Pct(50), Dim::Pct(50));
                } else if k == 0 {
                    pos.0 = Dim::Pct(50);
                } else {
                    pos.1 = Dim::Pct(50);
                }
            }
            w => {
                if let Some(d) = dim(w, em) {
                    if k == 0 {
                        pos.0 = d;
                        pos.1 = Dim::Pct(50);
                    } else {
                        pos.1 = d;
                    }
                }
            }
        }
        k += 1;
    }
    s.pos = pos;
}

fn bg_size(v: &str, em: i32) -> BgSize {
    let words: Vec<&str> = v.split_whitespace().collect();
    match words.as_slice() {
        ["cover"] => BgSize::Cover,
        ["contain"] => BgSize::Contain,
        [a] => BgSize::Set(dim(a, em).unwrap_or(Dim::Auto), Dim::Auto),
        [a, b, ..] => BgSize::Set(dim(a, em).unwrap_or(Dim::Auto), dim(b, em).unwrap_or(Dim::Auto)),
        _ => BgSize::Auto,
    }
}

fn bg_repeat(v: &str) -> (bool, bool) {
    match v.trim() {
        "no-repeat" => (false, false),
        "repeat-x" => (true, false),
        "repeat-y" => (false, true),
        _ => (true, true),
    }
}

pub const LINK_COLOR: u32 = 0x1a5fb4;

/// A CSS length in px: units, and calc() / min() / max() / clamp().
fn length(v: &str, em: i32, pct_of: i32) -> Option<i32> {
    let mut p = Calc { s: v.trim().as_bytes(), i: 0, em: em as f64, pct: pct_of as f64 };
    let x = p.expr()?;
    p.ws();
    (p.i == p.s.len() && x.is_finite()).then_some(x as i32)
}

/// A tiny evaluator for CSS maths (viewport units assume a 1000 × 800 window).
struct Calc<'a> {
    s: &'a [u8],
    i: usize,
    em: f64,
    pct: f64,
}

impl Calc<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            self.ws();
            match self.s.get(self.i) {
                Some(b'+') => {
                    self.i += 1;
                    v += self.term()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.term()?;
                }
                _ => return Some(v),
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.factor()?;
        loop {
            self.ws();
            match self.s.get(self.i) {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.factor()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    let d = self.factor()?;
                    if d == 0.0 {
                        return None;
                    }
                    v /= d;
                }
                _ => return Some(v),
            }
        }
    }

    fn args(&mut self) -> Option<Vec<f64>> {
        let mut out = Vec::new();
        loop {
            out.push(self.expr()?);
            self.ws();
            match self.s.get(self.i) {
                Some(b',') => self.i += 1,
                Some(b')') => {
                    self.i += 1;
                    return Some(out);
                }
                _ => return None,
            }
        }
    }

    fn factor(&mut self) -> Option<f64> {
        self.ws();
        let st = self.i;
        if self.s.get(self.i) == Some(&b'(') {
            self.i += 1;
            let v = self.expr()?;
            self.ws();
            if self.s.get(self.i) != Some(&b')') {
                return None;
            }
            self.i += 1;
            return Some(v);
        }
        if self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
            while self.i < self.s.len() && (self.s[self.i].is_ascii_alphabetic() || self.s[self.i] == b'-') {
                self.i += 1;
            }
            let name = core::str::from_utf8(&self.s[st..self.i]).ok()?.to_ascii_lowercase();
            if self.s.get(self.i) != Some(&b'(') {
                return None;
            }
            self.i += 1;
            let a = self.args()?;
            return match (name.as_str(), a.len()) {
                ("calc", 1) => Some(a[0]),
                ("min", n) if n > 0 => a.into_iter().reduce(f64::min),
                ("max", n) if n > 0 => a.into_iter().reduce(f64::max),
                ("clamp", 3) => Some(a[1].max(a[0]).min(a[2])),
                _ => None,
            };
        }
        // a number and its unit
        if matches!(self.s.get(self.i), Some(b'-') | Some(b'+')) {
            self.i += 1;
        }
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            self.i += 1;
        }
        if matches!(self.s.get(self.i), Some(b'e') | Some(b'E')) && self.s.get(self.i + 1).is_some_and(|c| c.is_ascii_digit() || *c == b'-') {
            self.i += 2;
            while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                self.i += 1;
            }
        }
        let n: f64 = core::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok()?;
        let us = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_alphabetic() || self.s[self.i] == b'%') {
            self.i += 1;
        }
        let unit = core::str::from_utf8(&self.s[us..self.i]).ok()?.to_ascii_lowercase();
        let k = match unit.as_str() {
            "" | "px" => 1.0,
            "rem" => 16.0,
            "em" => self.em,
            "ex" | "ch" => self.em / 2.0,
            "pt" => 4.0 / 3.0,
            "pc" => 16.0,
            "in" => 96.0,
            "cm" => 37.8,
            "mm" => 3.78,
            "%" => self.pct / 100.0,
            "vw" | "vi" | "svw" | "lvw" | "dvw" => 10.0,
            "vh" | "vb" | "svh" | "lvh" | "dvh" => 8.0,
            "vmin" => 8.0,
            "vmax" => 10.0,
            _ => return None,
        };
        Some(n * k)
    }
}

/// Split at spaces outside parentheses (so calc(1rem + 2px) stays whole).
fn words_top(v: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut start = None;
    for (i, c) in v.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c.is_whitespace() && depth <= 0 {
            if let Some(st) = start.take() {
                out.push(&v[st..i]);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push(&v[st..]);
    }
    out
}

fn four(v: &str, em: i32, w: i32) -> Option<[Option<i32>; 4]> {
    let parts: Vec<Option<i32>> = words_top(v).into_iter().map(|p| if p == "auto" { None } else { Some(length(p, em, w).unwrap_or(0)) }).collect();
    Some(match parts.len() {
        1 => [parts[0]; 4],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        4 => [parts[0], parts[1], parts[2], parts[3]],
        _ => return None,
    })
}

fn default_style(tag: &str, parent: &Style) -> Style {
    let mut s = parent.clone();
    s.disp = Disp::Inline;
    s.bg = None;
    s.bg_img = None;
    s.out_of_flow = false;
    s.clipped = false;
    s.offscreen = false;
    s.collapsed = false;
    s.overflow_hidden = false;
    s.margin = [0; 4];
    s.padding = [0; 4];
    s.width = None;
    s.height = None;
    s.max_width = None;
    s.center_box = false;
    s.border = None;
    let em = parent.size;
    match tag {
        "html" | "body" | "div" | "p" | "section" | "article" | "header" | "footer" | "nav" | "main" | "aside" | "form" | "address" | "figure" | "figcaption" | "fieldset" | "details" | "summary" | "dl" | "dt" | "dd" | "center" | "blockquote" | "pre" | "hr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "menu" | "legend" | "caption" | "noscript" => s.disp = Disp::Block,
        "li" => s.disp = Disp::ListItem,
        "table" => s.disp = Disp::Table,
        "tr" => s.disp = Disp::Row,
        "td" | "th" => s.disp = Disp::Cell,
        "thead" | "tbody" | "tfoot" => s.disp = Disp::Block,
        "head" | "script" | "style" | "title" | "meta" | "link" | "template" | "svg" | "iframe" | "object" | "canvas" | "video" | "audio" | "map" | "datalist" => s.disp = Disp::None,
        _ => {}
    }
    match tag {
        "body" => s.margin = [8; 4],
        "p" | "dl" | "figure" => s.margin = [em, 0, em, 0],
        "blockquote" => s.margin = [em, 40, em, 40],
        "h1" => {
            s.size = em * 2;
            s.bold = true;
            s.margin = [em * 2 / 3, 0, em * 2 / 3, 0];
        }
        "h2" => {
            s.size = em * 3 / 2;
            s.bold = true;
            s.margin = [em * 5 / 6, 0, em * 5 / 6, 0];
        }
        "h3" => {
            s.size = em * 117 / 100;
            s.bold = true;
            s.margin = [em, 0, em, 0];
        }
        "h4" | "h5" | "h6" | "b" | "strong" | "dt" | "legend" | "summary" => s.bold = true,
        "i" | "em" | "cite" | "var" | "dfn" | "address" => s.italic = true,
        "u" | "ins" => s.underline = true,
        "s" | "strike" | "del" => s.strike = true,
        "code" | "kbd" | "samp" | "tt" => s.mono = true,
        "pre" => {
            s.mono = true;
            s.pre = true;
            s.margin = [em, 0, em, 0];
        }
        "small" | "sub" | "sup" => s.size = em * 5 / 6,
        "big" => s.size = em * 6 / 5,
        "ul" | "ol" | "menu" => {
            s.margin = [em, 0, em, 0];
            s.padding = [0, 0, 0, 36];
        }
        "dd" => s.margin = [0, 0, 0, 36],
        "center" => s.align = Align::Center,
        "td" => s.padding = [3; 4],
        "th" => {
            s.padding = [3; 4];
            s.bold = true;
        }
        "hr" => s.margin = [em / 2, 0, em / 2, 0],
        _ => {}
    }
    s
}

fn apply(s: &mut Style, parent: &Style, prop: &str, v: &str, containing_w: i32) {
    let vl = v.to_ascii_lowercase();
    let em = parent.size;
    match prop {
        "display" => {
            s.disp = match vl.as_str() {
                "none" => Disp::None,
                "block" | "flex" | "grid" | "flow-root" => Disp::Block,
                "list-item" => Disp::ListItem,
                "table" => Disp::Table,
                "table-row" => Disp::Row,
                "table-cell" => Disp::Cell,
                "inline" | "inline-block" | "inline-flex" | "contents" => Disp::Inline,
                _ => s.disp,
            }
        }
        "visibility" if vl == "hidden" || vl == "collapse" => s.disp = Disp::None,
        "text-transform" => {
            s.case = match vl.as_str() {
                "uppercase" => 1,
                "lowercase" => 2,
                "capitalize" => 3,
                _ => 0,
            }
        }
        "position" => s.out_of_flow = vl == "absolute" || vl == "fixed",
        "overflow" | "overflow-y" | "overflow-x" => s.overflow_hidden = vl.contains("hidden") || vl.contains("clip"),
        // the ways pages hide things visually: clipping to nothing, moving
        // far off-screen, collapsing
        "clip" if vl.replace(' ', "").starts_with("rect(0") || vl.contains("rect(1px") => s.clipped = true,
        "clip-path" if vl.contains("inset(50%") || vl.contains("inset(100%") || vl == "circle(0)" => s.clipped = true,
        "transform" if vl.contains("scale(0)") => s.clipped = true,
        "left" | "top" | "right" if vl.starts_with('-') && length(&vl, em, containing_w).is_some_and(|x| x <= -999) => s.offscreen = true,
        "text-indent" => s.no_text = vl.starts_with('-') && length(&vl, em, containing_w).is_some_and(|x| x <= -999),
        "max-height" if length(&vl, em, 0) == Some(0) => s.collapsed = true,
        "color" => {
            if let Some(Some(c)) = color(&vl) {
                s.color = c;
            }
        }
        "background-color" => {
            if let Some(c) = color(&vl) {
                s.bg = c;
            }
        }
        "background" | "background-image" => {
            let url = css_url(v);
            // gradients: their first colour stands in
            let gradient = vl.contains("gradient(");
            let rest = match (vl.find("url("), vl.find(')')) {
                (Some(a), Some(b)) if b > a => alloc::format!("{} {}", &vl[..a], &vl[b + 1..]),
                _ => vl.clone(),
            };
            if prop == "background" {
                // the shorthand resets what it doesn't set
                s.bg = None;
                s.bg_img = None;
                let (before, size) = match rest.split_once('/') {
                    Some((a, b)) => (a.to_string(), Some(b.to_string())),
                    None => (rest.clone(), None),
                };
                let mut b = Background { url: String::new(), size: BgSize::Auto, pos: (Dim::Pct(0), Dim::Pct(0)), repeat: (true, true) };
                let mut pos_words: Vec<&str> = Vec::new();
                let words: Vec<&str> = before.split_whitespace().collect();
                let size_words: Vec<&str> = size.as_deref().map(|z| z.split_whitespace().collect()).unwrap_or_default();
                for w in &words {
                    if matches!(*w, "repeat" | "no-repeat" | "repeat-x" | "repeat-y") {
                        b.repeat = bg_repeat(w);
                    } else if matches!(*w, "left" | "right" | "top" | "bottom" | "center") || dim(w, em).is_some() {
                        pos_words.push(w);
                    } else if let Some(c) = color(w) {
                        s.bg = c;
                    }
                }
                if !pos_words.is_empty() {
                    bg_position(&mut b, &pos_words, em);
                }
                if let Some(z) = size_words.first() {
                    // the size comes before any later words (colour, repeat)
                    let n = size_words.iter().take_while(|w| matches!(**w, "cover" | "contain" | "auto") || dim(w, em).is_some()).count().max(1);
                    b.size = bg_size(&size_words[..n].join(" "), em);
                    let _ = z;
                    for w in &size_words[n..] {
                        if matches!(*w, "repeat" | "no-repeat" | "repeat-x" | "repeat-y") {
                            b.repeat = bg_repeat(w);
                        } else if let Some(c) = color(w) {
                            s.bg = c;
                        }
                    }
                }
                if let Some(u) = url {
                    b.url = u;
                    s.bg_img = Some(b);
                }
            } else {
                s.bg_img = url.map(|u| Background { url: u, size: BgSize::Auto, pos: (Dim::Pct(0), Dim::Pct(0)), repeat: (true, true) });
            }
            if gradient && s.bg.is_none() {
                if let Some(start) = vl.find("gradient(") {
                    for w in vl[start + 9..].split([',', ' ', ')']) {
                        if let Some(Some(c)) = color(w) {
                            s.bg = Some(c);
                            break;
                        }
                    }
                }
            }
        }
        "background-size" => {
            if let Some(b) = s.bg_img.as_mut() {
                b.size = bg_size(&vl, em);
            }
        }
        "background-position" => {
            if let Some(b) = s.bg_img.as_mut() {
                let words: Vec<&str> = vl.split_whitespace().collect();
                bg_position(b, &words, em);
            }
        }
        "background-repeat" => {
            if let Some(b) = s.bg_img.as_mut() {
                b.repeat = bg_repeat(&vl);
            }
        }
        "font-size" => {
            s.size = match vl.as_str() {
                "xx-small" => 9,
                "x-small" => 10,
                "small" | "smaller" => 13,
                "medium" => 16,
                "large" | "larger" => 18,
                "x-large" => 24,
                "xx-large" => 32,
                _ => length(&vl, em, em).unwrap_or(s.size),
            }
            .clamp(8, 48)
        }
        "font-weight" => s.bold = matches!(vl.as_str(), "bold" | "bolder" | "600" | "700" | "800" | "900"),
        "font-style" => s.italic = vl == "italic" || vl == "oblique",
        "font-family" => s.mono = vl.contains("mono") || vl.contains("courier") || vl.contains("consolas"),
        "font" => {
            if vl.contains("bold") {
                s.bold = true;
            }
            if vl.contains("italic") {
                s.italic = true;
            }
            for w in vl.split_whitespace() {
                if let Some(px) = length(w.split('/').next().unwrap_or(""), em, em) {
                    s.size = px.clamp(8, 48);
                }
            }
        }
        "text-align" => {
            s.align = match vl.as_str() {
                "center" | "-webkit-center" => Align::Center,
                "right" | "end" => Align::Right,
                _ => Align::Left,
            }
        }
        "text-decoration" | "text-decoration-line" => {
            s.underline = vl.contains("underline");
            s.strike = vl.contains("line-through");
        }
        "white-space" => s.pre = vl.starts_with("pre"),
        "margin" | "padding" => {
            if let Some(f) = four(&vl, s.size, containing_w) {
                let vals = [f[0].unwrap_or(0), f[1].unwrap_or(0), f[2].unwrap_or(0), f[3].unwrap_or(0)];
                if prop == "margin" {
                    s.margin = vals;
                    s.center_box = f[1].is_none() && f[3].is_none();
                } else {
                    s.padding = vals;
                }
            }
        }
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" | "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            let k = match &prop[prop.find('-').unwrap() + 1..] {
                "top" => 0,
                "right" => 1,
                "bottom" => 2,
                _ => 3,
            };
            let val = length(&vl, s.size, containing_w).unwrap_or(0);
            if prop.starts_with("margin") {
                s.margin[k] = val;
            } else {
                s.padding[k] = val.max(0);
            }
        }
        "width" => s.width = length(&vl, s.size, containing_w),
        "height" => s.height = if vl.ends_with('%') { None } else { length(&vl, s.size, 0) },
        "max-width" => s.max_width = length(&vl, s.size, containing_w),
        "border-color" => {
            if s.border.is_some() {
                if let Some(Some(c)) = color(vl.split_whitespace().next().unwrap_or("")) {
                    s.border = Some(c);
                }
            }
        }
        "border-width" if length(vl.split_whitespace().next().unwrap_or(""), em, 0) == Some(0) => s.border = None,
        "border-style" if vl.starts_with("none") || vl.starts_with("hidden") => s.border = None,
        "border" | "border-bottom" | "border-top" => {
            if vl.contains("none") || vl.starts_with('0') {
                s.border = None;
            } else {
                for w in vl.split_whitespace() {
                    if let Some(Some(c)) = color(w) {
                        s.border = Some(c);
                    }
                }
                if s.border.is_none() && vl.contains("solid") {
                    s.border = Some(0xcccccc);
                }
            }
        }
        _ => {}
    }
}

// ---- display list -------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Item {
    Rect { x: i32, y: i32, w: i32, h: i32, color: u32 },
    Text { x: i32, y: i32, size: i32, face: Face, color: u32, text: String, underline: bool, strike: bool },
    Frame { x: i32, y: i32, w: i32, h: i32, color: u32 },
    /// an image: index into `Page::images`
    Image { x: i32, y: i32, w: i32, h: i32, img: usize },
    /// a box's background image
    Background { x: i32, y: i32, w: i32, h: i32, img: usize, size: BgSize, pos: (Dim, Dim), repeat: (bool, bool) },
}

/// What the browser knows about an image while laying out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImgStatus {
    Loading,
    Ready(u32, u32),
    Broken,
}

/// The address an <img> shows: `src`, unless that's a placeholder for a
/// lazily loaded image (data-src) or missing (srcset).
pub fn img_src(dom: &Dom, n: NodeId) -> Option<String> {
    let get = |a: &str| dom.attr(n, a).map(str::trim).filter(|v| !v.is_empty());
    let lazy = get("data-src").or_else(|| get("data-lazy-src")).or_else(|| get("data-original"));
    let src = get("src");
    let placeholder = src.map_or(true, |v| v.starts_with("data:") && v.len() < 300);
    if let (false, Some(v)) = (placeholder, src) {
        return Some(v.to_string());
    }
    if let Some(v) = lazy {
        return Some(v.to_string());
    }
    // srcset: the widest candidate up to 1200px, or the 1x one
    if let Some(set) = get("srcset").or_else(|| get("data-srcset")) {
        let mut best: Option<(i32, &str)> = None;
        for cand in set.split(", ") {
            let mut parts = cand.split_whitespace();
            let Some(url) = parts.next() else { continue };
            let d = parts.next().unwrap_or("1x");
            let score = if let Some(w) = d.strip_suffix('w') {
                let w: i32 = w.parse().unwrap_or(0);
                if w <= 1200 { w } else { -w }
            } else if d == "1x" {
                1000
            } else {
                0
            };
            if best.map_or(true, |(b, _)| score > b) {
                best = Some((score, url));
            }
        }
        if let Some((_, u)) = best {
            return Some(u.to_string());
        }
    }
    src.map(|v| v.to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    Text { name: String, value: String, password: bool },
    Submit { name: String, value: String },
    Hidden { name: String, value: String },
    Check { name: String, value: String, on: bool },
}

#[derive(Clone, Debug)]
pub struct Field {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub form: usize,
    pub ctl: Control,
    /// the page's colours for a button: background, text, border
    pub paint: Option<(Option<u32>, u32, Option<u32>)>,
}

#[derive(Clone, Debug)]
pub struct Form {
    pub action: String,
    pub post: bool,
}

#[derive(Default)]
pub struct Page {
    pub items: Vec<Item>,
    /// (x, y, w, h, href)
    pub links: Vec<(i32, i32, i32, i32, usize)>,
    pub hrefs: Vec<String>,
    pub fields: Vec<Field>,
    pub forms: Vec<Form>,
    pub height: i32,
    pub bg: u32,
    /// y of elements with an id (for #fragment links)
    pub anchors: Vec<(String, i32)>,
    /// image addresses, as written in the page
    pub images: Vec<String>,
    /// every image the page uses, drawn yet or not
    pub wanted: Vec<String>,
}

struct Frag {
    x: i32,
    w: i32,
    text: String,
    face: Face,
    size: i32,
    color: u32,
    underline: bool,
    strike: bool,
    link: Option<usize>,
    /// a form control drawn inline: index into fields
    field: Option<usize>,
    /// an image drawn inline: index into Page::images
    image: Option<usize>,
    h: i32,
    ibox: Option<IBox>,
}

struct Lines {
    x0: i32,
    width: i32,
    y: i32,
    cur: Vec<Frag>,
    cur_w: i32,
    align: Align,
    pending_space: bool,
}

fn face(s: &Style) -> Face {
    if s.mono {
        return Face::Mono;
    }
    match (s.bold, s.italic) {
        (false, false) => Face::Regular,
        (true, false) => Face::Semibold,
        (false, true) => Face::Italic,
        (true, true) => Face::SemiboldItalic,
    }
}

fn measure(f: Face, size: i32, s: &str) -> i32 {
    font::measure(f, size, s)
}

struct Engine<'a> {
    dom: &'a Dom,
    images: &'a dyn Fn(&str) -> ImgStatus,
    cascade: &'a Cascade,
    out: Page,
}

impl<'a> Engine<'a> {
    fn style_of(&self, n: NodeId, parent: &Style, containing_w: i32) -> Style {
        let tag = self.dom.tag(n);
        let mut s = default_style(tag, parent);
        // the browser's link style comes first, so pages can restyle links
        if tag == "a" && self.dom.attr(n, "href").is_some() {
            s.color = LINK_COLOR;
            s.underline = true;
        }
        // presentational attributes (below every style rule)
        if let Some(a) = self.dom.attr(n, "align") {
            s.align = match a.to_ascii_lowercase().as_str() {
                "center" | "middle" => Align::Center,
                "right" => Align::Right,
                _ => Align::Left,
            };
        }
        if let Some(c) = self.dom.attr(n, "bgcolor").and_then(color) {
            s.bg = c;
        }
        if tag == "font" {
            if let Some(Some(c)) = self.dom.attr(n, "color").and_then(color) {
                s.color = c;
            }
        }
        // the cascade: author rules, then style="" (each !important last)
        let inline = self.dom.attr(n, "style").map(css::parse_decls).unwrap_or_default();
        let mut decls: Vec<(bool, u32, u32, &css::Decl)> = self.cascade.matching(self.dom, n);
        for d in &inline {
            decls.push((d.important, u32::MAX, u32::MAX, d));
        }
        decls.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        // custom properties first, then everything that may use them
        if decls.iter().any(|d| d.3.name.starts_with("--")) {
            let vars = Rc::make_mut(&mut s.vars);
            for (_, _, _, d) in &decls {
                if d.name.starts_with("--") {
                    vars.insert(d.name.clone(), d.value.clone());
                }
            }
        }
        for (_, _, _, d) in &decls {
            if d.name.starts_with("--") {
                continue;
            }
            let lower = d.value.to_ascii_lowercase();
            if matches!(lower.as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer") {
                continue;
            }
            if d.value.contains("var(") {
                let vars = &s.vars;
                if let Some(v) = css::resolve_vars(&d.value, &|k: &str| vars.get(k).cloned(), 0) {
                    apply(&mut s, parent, &d.name, &v, containing_w);
                }
            } else {
                apply(&mut s, parent, &d.name, &d.value, containing_w);
            }
        }
        // an inline element with a background or border (a button-like link)
        if s.disp == Disp::Inline {
            if s.bg.is_some() || s.border.is_some() {
                s.ibox = Some(IBox { bg: s.bg, border: s.border, pad: s.padding });
            }
        } else {
            s.ibox = None;
        }
        // hidden by the ways pages hide things visually
        let tiny = s.width.is_some_and(|w| w <= 1) || s.height.is_some_and(|h| h <= 1);
        if s.clipped || (s.out_of_flow && (s.offscreen || (s.overflow_hidden && tiny))) || (s.overflow_hidden && (s.collapsed || s.height == Some(0))) {
            s.disp = Disp::None;
        }
        if tag == "a" && self.dom.attr(n, "href").is_some() {
            s.link = Some(usize::MAX); // assigned by the caller
        }
        if tag == "a" && self.dom.attr(n, "href").is_none() {
            s.link = parent.link;
        }
        if s.disp != Disp::None && self.dom.attr(n, "hidden").is_some() {
            s.disp = Disp::None;
        }
        if tag == "input" && self.dom.attr(n, "type").map_or(false, |t| t.eq_ignore_ascii_case("hidden")) {
            s.disp = Disp::Inline;
        }
        s
    }

    // -- inline flow --

    fn flush_line(&mut self, l: &mut Lines, force: bool) {
        if l.cur.is_empty() {
            if force {
                l.y += 16;
            }
            return;
        }
        let lh = l.cur.iter().map(|f| if f.image.is_some() { f.h } else if f.field.is_some() { f.h + 4 } else { f.size * 135 / 100 }).max().unwrap_or(16);
        let asc = l.cur.iter().map(|f| if f.field.is_some() || f.image.is_some() { f.h } else { f.size * 95 / 100 }).max().unwrap_or(12);
        let base = l.y + (lh - asc) / 2 + asc - 2;
        let used: i32 = l.cur_w;
        let dx = match l.align {
            Align::Left => 0,
            Align::Center => ((l.width - used) / 2).max(0),
            Align::Right => (l.width - used).max(0),
        };
        for f in l.cur.drain(..) {
            let x = l.x0 + f.x + dx;
            if let Some(fi) = f.field {
                let fld = &mut self.out.fields[fi];
                fld.x = x;
                fld.y = base - f.h + 4;
                continue;
            }
            if let Some(img) = f.image {
                let y = base - f.h + 2;
                if let Some(li) = f.link {
                    self.out.links.push((x, y, f.w, f.h, li));
                }
                self.out.items.push(Item::Image { x, y, w: f.w, h: f.h, img });
                continue;
            }
            if let Some(li) = f.link {
                self.out.links.push((x, l.y, f.w, lh, li));
            }
            if let Some(b) = f.ibox {
                let (bx, by) = (x - b.pad[3], base - f.size * 95 / 100 - b.pad[0] - 2);
                let (bw, bh) = (f.w + b.pad[3] + b.pad[1], f.size * 125 / 100 + b.pad[0] + b.pad[2]);
                if let Some(c) = b.bg {
                    self.out.items.push(Item::Rect { x: bx, y: by, w: bw, h: bh, color: c });
                }
                if let Some(c) = b.border {
                    self.out.items.push(Item::Frame { x: bx, y: by, w: bw, h: bh, color: c });
                }
            }
            self.out.items.push(Item::Text { x, y: base, size: f.size, face: f.face, color: f.color, text: f.text, underline: f.underline, strike: f.strike });
        }
        l.y += lh;
        l.cur_w = 0;
        l.pending_space = false;
    }

    fn word(&mut self, l: &mut Lines, s: &Style, word: &str) {
        let f = face(s);
        let size = s.size.clamp(8, 48);
        let space = if l.pending_space && !l.cur.is_empty() { measure(f, size, " ") } else { 0 };
        let w = measure(f, size, word);
        if l.cur_w + space + w > l.width && !l.cur.is_empty() {
            self.flush_line(l, false);
        }
        let space = if l.pending_space && !l.cur.is_empty() { measure(f, size, " ") } else { 0 };
        // extend the previous fragment when the style continues
        if let Some(prev) = l.cur.last_mut() {
            if prev.field.is_none() && prev.image.is_none() && prev.ibox == s.ibox && prev.face == f && prev.size == size && prev.color == s.color && prev.link == s.link && prev.underline == s.underline && prev.strike == s.strike {
                if space > 0 {
                    prev.text.push(' ');
                }
                prev.text.push_str(word);
                // measure the run as a whole: per-word rounding adds up
                prev.w = measure(f, size, &prev.text);
                l.cur_w = prev.x + prev.w;
                l.pending_space = false;
                return;
            }
        }
        l.cur_w += space;
        l.cur.push(Frag { x: l.cur_w, w, text: word.to_string(), face: f, size, color: s.color, underline: s.underline, strike: s.strike, link: s.link, field: None, image: None, h: 0, ibox: s.ibox });
        l.cur_w += w;
        l.pending_space = false;
    }

    fn text(&mut self, l: &mut Lines, s: &Style, t: &str) {
        if s.no_text {
            return;
        }
        let cased;
        let t = match s.case {
            1 => {
                cased = t.to_uppercase();
                cased.as_str()
            }
            2 => {
                cased = t.to_lowercase();
                cased.as_str()
            }
            3 => {
                let mut out = String::with_capacity(t.len());
                let mut start = true;
                for ch in t.chars() {
                    if start && ch.is_alphabetic() {
                        out.extend(ch.to_uppercase());
                    } else {
                        out.push(ch);
                    }
                    start = ch.is_whitespace();
                }
                cased = out;
                cased.as_str()
            }
            _ => t,
        };
        if s.pre {
            for (k, line) in t.split('\n').enumerate() {
                if k > 0 {
                    self.flush_line(l, true);
                }
                let line = line.replace('\t', "    ");
                if !line.is_empty() {
                    l.pending_space = false;
                    let f = face(s);
                    let w = measure(f, s.size, &line);
                    l.cur.push(Frag { x: l.cur_w, w, text: line, face: f, size: s.size, color: s.color, underline: s.underline, strike: s.strike, link: s.link, field: None, image: None, h: 0, ibox: s.ibox });
                    l.cur_w += w;
                }
            }
            return;
        }
        if t.starts_with(|c: char| c.is_whitespace()) {
            l.pending_space = true;
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        let n = words.len();
        for (k, w) in words.into_iter().enumerate() {
            let w = w.replace('\u{a0}', " ");
            self.word(l, s, &w);
            if k + 1 < n {
                l.pending_space = true;
            }
        }
        if n > 0 && t.ends_with(|c: char| c.is_whitespace()) {
            l.pending_space = true;
        }
    }

    fn field(&mut self, l: &mut Lines, s: &Style, form: usize, ctl: Control, w: i32, h: i32) {
        if l.cur_w + w > l.width && !l.cur.is_empty() {
            self.flush_line(l, false);
        }
        if l.pending_space && !l.cur.is_empty() {
            l.cur_w += 6;
        }
        let fi = self.out.fields.len();
        self.out.fields.push(Field { x: 0, y: 0, w, h, form, ctl, paint: (s.bg.is_some() || s.border.is_some()).then_some((s.bg, s.color, s.border)) });
        l.cur.push(Frag { x: l.cur_w, w, text: String::new(), face: face(s), size: s.size, color: s.color, underline: false, strike: false, link: None, field: Some(fi), image: None, h, ibox: None });
        l.cur_w += w;
        l.pending_space = true;
    }

    fn inline(&mut self, n: NodeId, s: &Style, l: &mut Lines, form: usize) {
        match &self.dom.nodes[n].kind {
            Kind::Text(t) => self.text(l, s, t),
            Kind::Element { tag, .. } => {
                let tag = tag.clone();
                let mut cs = self.style_of(n, s, l.width);
                if cs.disp == Disp::None {
                    return;
                }
                if cs.link == Some(usize::MAX) {
                    self.out.hrefs.push(self.dom.attr(n, "href").unwrap_or("").to_string());
                    cs.link = Some(self.out.hrefs.len() - 1);
                }
                if let Some(id) = self.dom.attr(n, "id").or_else(|| if tag == "a" { self.dom.attr(n, "name") } else { None }) {
                    self.out.anchors.push((id.to_string(), l.y));
                }
                match tag.as_str() {
                    "br" => {
                        self.flush_line(l, true);
                        return;
                    }
                    "img" => {
                        self.image(n, &cs, l);
                        return;
                    }
                    "input" | "button" | "textarea" | "select" => {
                        self.control(n, &tag, &cs, l, form);
                        return;
                    }
                    _ => {}
                }
                // horizontal margin and padding of inline elements
                let lead = cs.margin[3].max(0) + cs.padding[3];
                if lead > 0 && !l.cur.is_empty() {
                    l.cur_w += lead;
                }
                for &c in &self.dom.nodes[n].children.clone() {
                    if self.is_block(c, &cs, l.width) {
                        // a block inside an inline: lay it out as its own block
                        self.flush_line(l, false);
                        let x0 = l.x0;
                        let w = l.width;
                        let y = l.y;
                        l.y = self.block(c, &cs, x0, y, w, form);
                    } else {
                        self.inline(c, &cs, l, form);
                    }
                }
                let trail = cs.margin[1].max(0) + cs.padding[1];
                if trail > 0 && !l.cur.is_empty() {
                    l.cur_w += trail;
                }
            }
            Kind::Document => {}
        }
    }

    fn alt_text(&mut self, n: NodeId, s: &Style, l: &mut Lines) {
        let alt = self.dom.attr(n, "alt").unwrap_or("").trim().to_string();
        if !alt.is_empty() {
            let mut st = s.clone();
            st.italic = true;
            if st.link.is_none() {
                st.color = 0x77716a;
            }
            self.text(l, &st, &alloc::format!("[{}]", alt));
        }
    }

    /// An <img> as an inline box: its size from CSS, its width/height
    /// attributes and the picture itself, no wider than the line.
    fn image(&mut self, n: NodeId, s: &Style, l: &mut Lines) {
        let Some(src) = img_src(self.dom, n) else {
            self.alt_text(n, s, l);
            return;
        };
        self.out.wanted.push(src.clone());
        let status = (self.images)(&src);
        let attr = |a: &str| self.dom.attr(n, a).and_then(|v| v.trim().trim_end_matches("px").parse::<i32>().ok()).filter(|&v| v > 0);
        let cw = s.width.filter(|&v| v > 0).or_else(|| attr("width"));
        let ch = s.height.filter(|&v| v > 0).or_else(|| attr("height"));
        let natural = match status {
            ImgStatus::Ready(w, h) if w > 0 && h > 0 => Some((w as i32, h as i32)),
            _ => None,
        };
        let size = match (cw, ch, natural) {
            (Some(w), Some(h), _) => Some((w, h)),
            (Some(w), None, Some((nw, nh))) => Some((w, (w as i64 * nh as i64 / nw as i64) as i32)),
            (None, Some(h), Some((nw, nh))) => Some(((h as i64 * nw as i64 / nh as i64) as i32, h)),
            (None, None, Some(nat)) => Some(nat),
            _ => None,
        };
        let size = if status == ImgStatus::Broken { None } else { size };
        let Some((mut w, mut h)) = size else {
            if status == ImgStatus::Broken {
                self.alt_text(n, s, l);
            }
            return;
        };
        let max = s.max_width.map_or(l.width, |m| m.min(l.width)).max(1);
        if w > max {
            h = (h as i64 * max as i64 / w as i64) as i32;
            w = max;
        }
        if w <= 0 || h <= 0 {
            return;
        }
        if l.cur_w + w > l.width && !l.cur.is_empty() {
            self.flush_line(l, false);
        }
        if l.pending_space && !l.cur.is_empty() {
            l.cur_w += measure(face(s), s.size, " ");
        }
        let img = self.out.images.len();
        self.out.images.push(src);
        l.cur.push(Frag { x: l.cur_w, w, text: String::new(), face: face(s), size: s.size, color: s.color, underline: false, strike: false, link: s.link, field: None, image: Some(img), h, ibox: None });
        l.cur_w += w;
        l.pending_space = false;
    }

    fn control(&mut self, n: NodeId, tag: &str, s: &Style, l: &mut Lines, form: usize) {
        let name = self.dom.attr(n, "name").unwrap_or("").to_string();
        let value = self.dom.attr(n, "value").unwrap_or("").to_string();
        let ty = self.dom.attr(n, "type").unwrap_or(if tag == "button" { "submit" } else { "text" }).to_ascii_lowercase();
        let h = (s.size * 2).max(26);
        match (tag, ty.as_str()) {
            ("input", "hidden") => {
                self.out.fields.push(Field { x: 0, y: 0, w: 0, h: 0, form, ctl: Control::Hidden { name, value }, paint: None });
            }
            ("input", "submit") | ("input", "button") | ("button", _) => {
                let label = if tag == "button" { self.dom.text(n) } else if value.is_empty() { String::from("Submit") } else { value.clone() };
                let w = measure(face(s), s.size, &label) + 24;
                let ctl = if ty == "submit" || tag == "button" && ty != "button" { Control::Submit { name, value: label } } else { Control::Submit { name: String::new(), value: label } };
                self.field(l, s, form, ctl, w, h);
            }
            ("input", "checkbox") | ("input", "radio") => {
                let on = self.dom.attr(n, "checked").is_some();
                self.field(l, s, form, Control::Check { name, value: if value.is_empty() { String::from("on") } else { value }, on }, 16, 16);
            }
            ("input", "image") | ("input", "file") | ("input", "reset") | ("input", "color") | ("input", "range") => {}
            ("select", _) => {
                // the selected (or first) option
                let mut val = String::new();
                let mut first = None;
                for m in 0..self.dom.nodes.len() {
                    if self.dom.tag(m) == "option" && is_inside(self.dom, m, n) {
                        let v = self.dom.attr(m, "value").map(|x| x.to_string()).unwrap_or_else(|| self.dom.text(m));
                        if first.is_none() {
                            first = Some(v.clone());
                        }
                        if self.dom.attr(m, "selected").is_some() {
                            val = v;
                        }
                    }
                }
                if val.is_empty() {
                    val = first.unwrap_or_default();
                }
                let w = measure(face(s), s.size, &val) + 36;
                self.out.fields.push(Field { x: 0, y: 0, w: 0, h: 0, form, ctl: Control::Hidden { name, value: val.clone() }, paint: None });
                self.field(l, s, form, Control::Submit { name: String::new(), value: val }, w.min(l.width), h);
            }
            _ => {
                let chars: i32 = self.dom.attr(n, "size").and_then(|v| v.parse().ok()).unwrap_or(24);
                let w = if tag == "textarea" { l.width.min(480) } else { (chars * s.size * 55 / 100 + 16).clamp(80, l.width.max(80)) };
                let value = if tag == "textarea" { self.dom.text(n) } else { value };
                let hh = if tag == "textarea" { h * 3 } else { h };
                self.field(l, s, form, Control::Text { name, value, password: ty == "password" }, w, hh);
            }
        }
    }

    fn is_block(&self, n: NodeId, parent: &Style, w: i32) -> bool {
        match &self.dom.nodes[n].kind {
            Kind::Element { .. } => !matches!(self.style_of(n, parent, w).disp, Disp::Inline | Disp::None),
            _ => false,
        }
    }

    /// Lay out a block at (x, y) with width w; returns the y after it.
    fn block(&mut self, n: NodeId, parent: &Style, x: i32, y: i32, w: i32, form: usize) -> i32 {
        let tag = self.dom.tag(n).to_string();
        let mut s = self.style_of(n, parent, w);
        if s.disp == Disp::None {
            return y;
        }
        if s.link == Some(usize::MAX) {
            self.out.hrefs.push(self.dom.attr(n, "href").unwrap_or("").to_string());
            s.link = Some(self.out.hrefs.len() - 1);
        }
        let form = if tag == "form" {
            self.out.forms.push(Form { action: self.dom.attr(n, "action").unwrap_or("").to_string(), post: self.dom.attr(n, "method").map_or(false, |m| m.eq_ignore_ascii_case("post")) });
            self.out.forms.len() - 1
        } else {
            form
        };
        if let Some(id) = self.dom.attr(n, "id") {
            self.out.anchors.push((id.to_string(), y));
        }
        let [mt, mr, mb, ml] = s.margin;
        let [pt, pr, pb, pl] = s.padding;
        let mut bw = w - ml.max(0) - mr.max(0);
        if let Some(width) = s.width {
            bw = bw.min(width.max(40));
        }
        if let Some(mw) = s.max_width {
            bw = bw.min(mw.max(40));
        }
        let bx = if s.center_box && bw < w { x + (w - bw) / 2 } else { x + ml.max(0) };
        let top = y + mt.max(0);
        let bg_at = self.out.items.len();
        let cx = bx + pl;
        let cw = (bw - pl - pr).max(20);
        let mut cy = top + pt;
        if tag == "hr" {
            self.out.items.push(Item::Rect { x: bx, y: cy, w: bw, h: 1, color: 0xcfc8bd });
            return cy + 1 + mb.max(0);
        }
        if tag == "img" {
            // display: block
            let mut lines = Lines { x0: x + ml.max(0), width: w - ml.max(0) - mr.max(0), y: cy, cur: vec![], cur_w: 0, align: if s.center_box { Align::Center } else { Align::Left }, pending_space: false };
            self.image(n, &s, &mut lines);
            self.flush_line(&mut lines, false);
            return lines.y + pb + mb.max(0);
        }
        if s.disp == Disp::Row {
            // cells side by side, equal widths
            let cells: Vec<NodeId> = self.dom.nodes[n].children.iter().copied().filter(|&c| matches!(self.dom.tag(c), "td" | "th")).collect();
            if !cells.is_empty() {
                let k = cells.len() as i32;
                let colw = cw / k;
                let mut bottom = cy;
                for (i, c) in cells.into_iter().enumerate() {
                    let b = self.block(c, &s, cx + i as i32 * colw, cy, colw, form);
                    bottom = bottom.max(b);
                }
                cy = bottom;
            }
        } else {
            let mut lines = Lines { x0: cx, width: cw, y: cy, cur: vec![], cur_w: 0, align: s.align, pending_space: false };
            let marker_y = cy;
            for &c in &self.dom.nodes[n].children.clone() {
                if self.is_block(c, &s, cw) {
                    self.flush_line(&mut lines, false);
                    let y2 = lines.y;
                    lines.y = self.block(c, &s, cx, y2, cw, form);
                } else {
                    self.inline(c, &s, &mut lines, form);
                }
            }
            self.flush_line(&mut lines, false);
            if s.disp == Disp::ListItem {
                let par = self.dom.nodes[n].parent.unwrap_or(0);
                let ordered = self.dom.tag(par) == "ol";
                let marker = if ordered {
                    let idx = self.dom.nodes[par].children.iter().filter(|&&c| self.dom.tag(c) == "li").position(|&c| c == n).unwrap_or(0);
                    alloc::format!("{}.", idx + 1)
                } else {
                    String::from("•")
                };
                let f = face(&s);
                let mw = measure(f, s.size, &marker);
                let base = marker_y + s.size * 135 / 100 * 3 / 4;
                self.out.items.push(Item::Text { x: cx - mw - 8, y: base, size: s.size, face: f, color: s.color, text: marker, underline: false, strike: false });
            }
            cy = lines.y;
        }
        let bottom = cy + pb;
        if let Some(b) = &s.bg_img {
            self.out.wanted.push(b.url.clone());
            let img = self.out.images.len();
            self.out.images.push(b.url.clone());
            self.out.items.insert(bg_at, Item::Background { x: bx, y: top, w: bw, h: bottom - top, img, size: b.size, pos: b.pos, repeat: b.repeat });
        }
        if let Some(bg) = s.bg {
            if tag != "body" && tag != "html" {
                self.out.items.insert(bg_at, Item::Rect { x: bx, y: top, w: bw, h: bottom - top, color: bg });
            } else {
                self.out.bg = bg;
            }
        }
        if let Some(bc) = s.border {
            self.out.items.push(Item::Frame { x: bx, y: top, w: bw, h: bottom - top, color: bc });
        }
        bottom + mb.max(0)
    }
}

fn is_inside(dom: &Dom, mut n: NodeId, anc: NodeId) -> bool {
    while let Some(p) = dom.nodes[n].parent {
        if p == anc {
            return true;
        }
        n = p;
    }
    false
}

/// Style and lay out a parsed page for a viewport `width` px wide, without
/// images (their alt text shows). Used by the host tests.
#[allow(dead_code)]
pub fn layout(dom: &Dom, extra_css: &str, width: i32) -> Page {
    layout_with(dom, extra_css, width, &|_| ImgStatus::Broken)
}

/// The window a page of content width `width` sits in (for media queries).
pub fn media_for(width: i32) -> Media {
    Media { width: width + 48, height: 800 }
}

/// Lay out a page with its own <style> blocks (and `extra_css` first);
/// `images` says what's known about each image address.
pub fn layout_with(dom: &Dom, extra_css: &str, width: i32, images: &dyn Fn(&str) -> ImgStatus) -> Page {
    let mut text = String::from(extra_css);
    for s in css::sources(dom).into_iter().flatten() {
        text.push('\n');
        text.push_str(&s);
    }
    let cascade = Cascade::new(&text, media_for(width));
    layout_styled(dom, &cascade, width, images)
}

/// Lay out a page with a prepared cascade (its stylesheets, fetched).
pub fn layout_styled(dom: &Dom, cascade: &Cascade, width: i32, images: &dyn Fn(&str) -> ImgStatus) -> Page {
    let root_style = Style {
        disp: Disp::Block,
        size: 16,
        bold: false,
        italic: false,
        mono: false,
        color: 0x1e1b2c,
        bg: None,
        align: Align::Left,
        underline: false,
        strike: false,
        pre: false,
        margin: [0; 4],
        padding: [0; 4],
        width: None,
        height: None,
        max_width: None,
        center_box: false,
        border: None,
        link: None,
        bg_img: None,
        vars: Rc::new(BTreeMap::new()),
        case: 0,
        out_of_flow: false,
        clipped: false,
        offscreen: false,
        collapsed: false,
        overflow_hidden: false,
        no_text: false,
        ibox: None,
    };
    let mut e = Engine { dom, images, cascade, out: Page { items: vec![], links: vec![], hrefs: vec![], fields: vec![], forms: vec![Form { action: String::new(), post: false }], height: 0, bg: 0xffffff, anchors: vec![], images: vec![], wanted: vec![] } };
    let mut y = 0;
    let top: Vec<NodeId> = dom.nodes[0].children.clone();
    let mut lines = Lines { x0: 0, width, y: 0, cur: vec![], cur_w: 0, align: Align::Left, pending_space: false };
    for c in top {
        if e.is_block(c, &root_style, width) {
            e.flush_line(&mut lines, false);
            y = e.block(c, &root_style, 0, lines.y.max(y), width, 0);
            lines.y = y;
        } else {
            e.inline(c, &root_style, &mut lines, 0);
        }
    }
    e.flush_line(&mut lines, false);
    e.out.height = lines.y.max(y) + 16;
    e.out
}
