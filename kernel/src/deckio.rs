//! Hyda Slides files: the native .hydp format, and PowerPoint (.pptx)
//! presentations to import and export.
//!
//! .hydp is UTF-8 text, one record per line, like .hyds and .hydg:
//!
//!   HYDP 1                            magic and format version
//!   app Hyda Slides                   written by (informational)
//!   size 1280 720                     slide size in units (1/96 inch)
//!   theme dune                        a built-in theme, or
//!   theme custom F7F1E8 1E1B2C 3A3548 C0622B   background, title, text, accent
//!   slides 2                          slide count
//!   slide content                     a slide and its layout
//!   bg 1C1A27                         its own background (optional)
//!   trans fade                        transition (optional)
//!   notes Say hello\nThen …          speaker notes (\n, \t, \\ escaped)
//!   shape body 80 172 1120 480 size=24 anchor=t fill=C0622B line=- color=FFFFFF pic=0
//!   p bullet left 1                   a paragraph: style, alignment, level
//!   t Budget 2026                     its text
//!   f 0:6:b                           formatting runs start:length:flags
//!   pic png iVBORw0KGgo…              a picture (base64), numbered from 0
//!   end 1a2b3c4d                      CRC-32 of every byte before this line
//!
//! Readers skip records they don't know, so later versions can add some.

use crate::deck::*;
use crate::doc::{attr, esc, unesc, Doc, Para, Style, Tok, Tokens, BOLD, ITALIC, STRIKE, UNDERLINE};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// EMU per slide unit.
const EMU: i64 = 9525;

fn hex6(c: u32) -> String {
    format!("{:06X}", c & 0xFFFFFF)
}

fn parse_hex(s: &str) -> Option<u32> {
    if s.len() == 6 {
        u32::from_str_radix(s, 16).ok()
    } else {
        None
    }
}

fn esc_line(s: &str, newlines: bool) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\t' => o.push_str("\\t"),
            '\n' if newlines => o.push_str("\\n"),
            c if c.is_control() => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

fn unesc_line(s: &str) -> String {
    let mut o = String::new();
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        if c == '\\' {
            match cs.next() {
                Some('t') => o.push('\t'),
                Some('n') => o.push('\n'),
                Some(x) => o.push(x),
                None => break,
            }
        } else {
            o.push(c);
        }
    }
    o
}

fn style_id(s: Style) -> &'static str {
    match s {
        Style::Bullet => "bullet",
        Style::Number => "number",
        _ => "body",
    }
}

fn align_id(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

fn fmt_flags(f: u8) -> String {
    let mut fl = String::new();
    for (bit, ch) in [(BOLD, 'b'), (ITALIC, 'i'), (UNDERLINE, 'u'), (STRIKE, 's')] {
        if f & bit != 0 {
            fl.push(ch);
        }
    }
    fl
}

fn write_paras(out: &mut String, doc: &Doc) {
    for p in &doc.paras {
        out.push_str(&format!("p {} {} {}\nt {}\n", style_id(p.style), align_id(p.align), p.level, esc_line(&p.string(), false)));
        let mut runs = Vec::new();
        let mut i = 0;
        while i < p.len() {
            let f = p.fmt[i] & (BOLD | ITALIC | UNDERLINE | STRIKE);
            let mut j = i;
            while j < p.len() && p.fmt[j] & (BOLD | ITALIC | UNDERLINE | STRIKE) == f {
                j += 1;
            }
            if f != 0 {
                runs.push(format!("{}:{}:{}", i, j - i, fmt_flags(f)));
            }
            i = j;
        }
        if !runs.is_empty() {
            out.push_str(&format!("f {}\n", runs.join(" ")));
        }
    }
}

pub fn to_hydp(d: &Deck) -> String {
    let mut out = String::from("HYDP 1\napp Hyda Slides\n");
    out.push_str(&format!("name {}\n", esc_line(&d.name, true)));
    out.push_str(&format!("size {} {}\n", d.w, d.h));
    if d.theme.id == "custom" {
        let t = &d.theme;
        out.push_str(&format!("theme custom {} {} {} {}\n", hex6(t.bg), hex6(t.title), hex6(t.text), hex6(t.accent)));
    } else {
        out.push_str(&format!("theme {}\n", d.theme.id));
    }
    if !d.footer.is_empty() {
        out.push_str(&format!("footer {}\n", esc_line(&d.footer, true)));
    }
    if !d.author.is_empty() {
        out.push_str(&format!("author {}\n", esc_line(&crate::doc::one_line(&d.author), false)));
    }
    if d.numbers {
        out.push_str("numbers 1\n");
    }
    out.push_str(&format!("slides {}\n", d.slides.len()));
    for s in &d.slides {
        out.push_str(&format!("slide {}\n", s.layout.id()));
        if let Some(bg) = s.bg {
            out.push_str(&format!("bg {}\n", hex6(bg)));
        }
        if let Some((c, a)) = s.bg_grad {
            out.push_str(&format!("bgrad {} {}\n", hex6(c), a));
        }
        if s.trans != Trans::None {
            out.push_str(&format!("trans {}\n", s.trans.id()));
        }
        if !s.notes.is_empty() {
            out.push_str(&format!("notes {}\n", esc_line(&s.notes, true)));
        }
        for sh in &s.shapes {
            let col = |c: Option<u32>| c.map(hex6).unwrap_or_else(|| String::from("-"));
            let anchor = match sh.anchor {
                Anchor::Top => "t",
                Anchor::Middle => "m",
                Anchor::Bottom => "b",
            };
            out.push_str(&format!("shape {} {} {} {} {} size={} anchor={} fill={} line={} color={}", sh.kind.id(), sh.x, sh.y, sh.w, sh.h, sh.size, anchor, col(sh.fill), col(sh.line), col(sh.color)));
            if let Some(p) = sh.pic {
                out.push_str(&format!(" pic={}", p));
            }
            if sh.geom != Geom::Rect {
                out.push_str(&format!(" geom={}", sh.geom.id()));
            }
            if sh.rot != 0 {
                out.push_str(&format!(" rot={}", sh.rot));
            }
            if sh.flip_h || sh.flip_v {
                out.push_str(&format!(" flip={}{}", if sh.flip_h { "h" } else { "" }, if sh.flip_v { "v" } else { "" }));
            }
            if let Some((c, a)) = sh.grad {
                out.push_str(&format!(" grad={},{}", hex6(c), a));
            }
            if sh.line_w != 3 {
                out.push_str(&format!(" lw={}", sh.line_w));
            }
            if sh.kind == Kind::Line {
                out.push_str(&format!(" arrows={}{}", if sh.head { "h" } else { "-" }, if sh.tail { "t" } else { "-" }));
            }
            if sh.anim != Anim::None {
                out.push_str(&format!(" anim={},{}", sh.anim.id(), sh.anim_order));
            }
            out.push('\n');
            if sh.kind.has_text() {
                write_paras(&mut out, &sh.text);
            }
            if let Some(t) = &sh.table {
                out.push_str(&format!("table {} {} {} {}\n", t.nrows(), t.ncols(), t.header as u8, t.banded as u8));
                out.push_str(&format!("cols {}\n", t.cols.iter().map(|c| format!("{}", c)).collect::<Vec<_>>().join(" ")));
                out.push_str(&format!("rows {}\n", t.rows.iter().map(|c| format!("{}", c)).collect::<Vec<_>>().join(" ")));
                for r in 0..t.nrows() {
                    for c in 0..t.ncols() {
                        out.push_str(&format!("cell {} {}\n", r, c));
                        write_paras(&mut out, t.cell(r, c));
                    }
                }
            }
            if let Some(c) = &sh.chart {
                out.push_str(&format!("chart {} {}\n", c.kind.id(), c.legend as u8));
                if !c.title.is_empty() {
                    out.push_str(&format!("ctitle {}\n", esc_line(&c.title, true)));
                }
                for cat in &c.cats {
                    out.push_str(&format!("cat {}\n", esc_line(cat, true)));
                }
                for se in &c.series {
                    out.push_str(&format!("series {}\n", esc_line(&se.name, true)));
                    out.push_str(&format!("vals {}\n", se.vals.iter().map(|v| format!("{}", v)).collect::<Vec<_>>().join(" ")));
                }
            }
        }
    }
    for p in &d.pics {
        let kind = if p.data.starts_with(b"\x89PNG") { "png" } else { "jpeg" };
        out.push_str(&format!("pic {} {}\n", kind, crate::crypto::base64(&p.data)));
    }
    let crc = crate::zip::crc32(out.as_bytes());
    out.push_str(&format!("end {:08x}\n", crc));
    out
}

/// Where "p", "t" and "f" records go.
#[derive(Clone, Copy, PartialEq)]
enum Target {
    Shape,
    Cell(usize, usize),
}

fn target_doc<'a>(d: &'a mut Deck, t: Target) -> Option<&'a mut Doc> {
    let sh = d.slides.last_mut()?.shapes.last_mut()?;
    match t {
        Target::Shape => Some(&mut sh.text),
        Target::Cell(r, c) => {
            let tb = sh.table.as_mut()?;
            if r < tb.nrows() && c < tb.ncols() {
                Some(tb.cell_mut(r, c))
            } else {
                None
            }
        }
    }
}

pub fn from_hydp(data: &[u8]) -> Result<Deck, &'static str> {
    let s = core::str::from_utf8(data).map_err(|_| "not a Hyda Slides presentation")?;
    let rest = s.strip_prefix("HYDP ").ok_or("not a Hyda Slides presentation")?;
    let ver: u32 = rest.split('\n').next().unwrap_or("").trim().parse().map_err(|_| "not a Hyda Slides presentation")?;
    if ver != 1 {
        return Err("made by a newer Hyda Slides");
    }
    let end_at = s.rfind("\nend ").ok_or("the file is incomplete")? + 1;
    let want = u32::from_str_radix(s[end_at + 4..].trim(), 16).map_err(|_| "the file is damaged")?;
    if crate::zip::crc32(&data[..end_at]) != want {
        return Err("the file is damaged");
    }
    let mut d = Deck { name: String::new(), w: SLIDE_W, h: SLIDE_H, theme: theme("dune"), slides: vec![], pics: vec![], footer: String::new(), author: String::new(), numbers: false };
    let mut count = None;
    let mut target = Target::Shape;
    const DAMAGED: &str = "the file is damaged";
    for line in s[..end_at].lines().skip(1) {
        let (tag, val) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "name" => d.name = unesc_line(val),
            "size" => {
                let mut it = val.split(' ').filter_map(|v| v.parse::<i32>().ok());
                if let (Some(w), Some(h)) = (it.next(), it.next()) {
                    d.w = w.clamp(100, 10000);
                    d.h = h.clamp(100, 10000);
                }
            }
            "theme" => {
                let mut it = val.split(' ');
                let id = it.next().unwrap_or("");
                if id == "custom" {
                    let c: Vec<u32> = it.filter_map(parse_hex).collect();
                    if c.len() >= 4 {
                        d.theme = Theme { id: "custom".into(), name: "Imported".into(), bg: c[0], title: c[1], text: c[2], accent: c[3], deco: vec![] };
                    }
                } else {
                    d.theme = theme(id);
                }
            }
            "footer" => d.footer = unesc_line(val),
            "author" => d.author = unesc_line(val),
            "numbers" => d.numbers = val.trim() == "1",
            "slides" => count = val.parse::<usize>().ok(),
            "slide" => d.slides.push(Slide { layout: Layout::from_id(val.trim()).unwrap_or(Layout::Blank), shapes: vec![], notes: String::new(), bg: None, bg_grad: None, trans: Trans::None }),
            "bg" => d.slides.last_mut().ok_or(DAMAGED)?.bg = parse_hex(val.trim()),
            "bgrad" => {
                let sl = d.slides.last_mut().ok_or(DAMAGED)?;
                let mut it = val.split(' ');
                sl.bg_grad = it.next().and_then(parse_hex).map(|c| (c, it.next().and_then(|a| a.parse().ok()).unwrap_or(90)));
            }
            "trans" => d.slides.last_mut().ok_or(DAMAGED)?.trans = Trans::from_id(val.trim()),
            "notes" => d.slides.last_mut().ok_or(DAMAGED)?.notes = unesc_line(val),
            "shape" => {
                let sl = d.slides.last_mut().ok_or(DAMAGED)?;
                let mut it = val.split(' ');
                let kind = Kind::from_id(it.next().unwrap_or("")).unwrap_or(Kind::Text);
                let mut n = [0i32; 4];
                for v in n.iter_mut() {
                    *v = it.next().and_then(|x| x.parse().ok()).ok_or(DAMAGED)?;
                }
                let mut sh = Shape::new(kind, n[0], n[1], n[2].max(if kind == Kind::Line { 0 } else { 1 }), n[3].max(if kind == Kind::Line { 0 } else { 1 }));
                sh.text = Doc { paras: vec![], author: String::new() };
                sh.tail = false;
                if let Some(t) = sh.table.as_mut() {
                    t.cells.clear();
                }
                sh.chart = None;
                for kv in it {
                    let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                    match k {
                        "size" => sh.size = v.parse::<u16>().unwrap_or(sh.size).clamp(4, 400),
                        "anchor" => {
                            sh.anchor = match v {
                                "m" => Anchor::Middle,
                                "b" => Anchor::Bottom,
                                _ => Anchor::Top,
                            }
                        }
                        "fill" => sh.fill = parse_hex(v),
                        "line" => sh.line = parse_hex(v),
                        "color" => sh.color = parse_hex(v),
                        "pic" => sh.pic = v.parse().ok(),
                        "geom" => sh.geom = Geom::from_id(v).unwrap_or(Geom::Rect),
                        "rot" => sh.rot = v.parse::<i32>().unwrap_or(0).rem_euclid(360),
                        "flip" => {
                            sh.flip_h = v.contains('h');
                            sh.flip_v = v.contains('v');
                        }
                        "grad" => {
                            let (c, a) = v.split_once(',').unwrap_or((v, "90"));
                            sh.grad = parse_hex(c).map(|c| (c, a.parse().unwrap_or(90)));
                        }
                        "lw" => sh.line_w = v.parse::<i32>().unwrap_or(3).clamp(1, 100),
                        "arrows" => {
                            sh.head = v.contains('h');
                            sh.tail = v.contains('t');
                        }
                        "anim" => {
                            let (a, o) = v.split_once(',').unwrap_or((v, "0"));
                            sh.anim = Anim::from_id(a);
                            sh.anim_order = o.parse().unwrap_or(0);
                        }
                        _ => {}
                    }
                }
                sl.shapes.push(sh);
                target = Target::Shape;
            }
            "table" => {
                let sh = d.slides.last_mut().and_then(|s| s.shapes.last_mut()).ok_or(DAMAGED)?;
                let n: Vec<usize> = val.split(' ').filter_map(|v| v.parse().ok()).collect();
                if n.len() < 2 || n[0] == 0 || n[1] == 0 || n[0] * n[1] > 10_000 {
                    return Err(DAMAGED);
                }
                let mut t = Table::new(n[0], n[1], sh.w, sh.h);
                t.cells = vec![Doc { paras: vec![], author: String::new() }; n[0] * n[1]];
                t.header = n.get(2) == Some(&1);
                t.banded = n.get(3) == Some(&1);
                sh.table = Some(t);
            }
            "cols" | "rows" => {
                let t = d.slides.last_mut().and_then(|s| s.shapes.last_mut()).and_then(|s| s.table.as_mut()).ok_or(DAMAGED)?;
                let v: Vec<i32> = val.split(' ').filter_map(|x| x.parse().ok()).collect();
                let dst = if tag == "cols" { &mut t.cols } else { &mut t.rows };
                if v.len() != dst.len() {
                    return Err(DAMAGED);
                }
                *dst = v.into_iter().map(|x| x.max(1)).collect();
            }
            "cell" => {
                let mut it = val.split(' ').filter_map(|v| v.parse::<usize>().ok());
                let (Some(r), Some(c)) = (it.next(), it.next()) else { return Err(DAMAGED) };
                target = Target::Cell(r, c);
                target_doc(&mut d, target).ok_or(DAMAGED)?;
            }
            "chart" => {
                let sh = d.slides.last_mut().and_then(|s| s.shapes.last_mut()).ok_or(DAMAGED)?;
                let mut it = val.split(' ');
                let kind = ChartKind::from_id(it.next().unwrap_or(""));
                sh.chart = Some(Chart { kind, title: String::new(), cats: vec![], series: vec![], legend: it.next() == Some("1") });
            }
            "ctitle" | "cat" | "series" | "vals" => {
                let c = d.slides.last_mut().and_then(|s| s.shapes.last_mut()).and_then(|s| s.chart.as_mut()).ok_or(DAMAGED)?;
                match tag {
                    "ctitle" => c.title = unesc_line(val),
                    "cat" => c.cats.push(unesc_line(val)),
                    "series" => c.series.push(Series { name: unesc_line(val), vals: vec![] }),
                    _ => {
                        let se = c.series.last_mut().ok_or(DAMAGED)?;
                        se.vals = val.split(' ').filter(|v| !v.is_empty()).map(|v| v.parse::<f64>().unwrap_or(0.0)).collect();
                    }
                }
            }
            "p" => {
                let doc = target_doc(&mut d, target).ok_or(DAMAGED)?;
                let mut it = val.split(' ');
                let style = match it.next().unwrap_or("") {
                    "bullet" => Style::Bullet,
                    "number" => Style::Number,
                    _ => Style::Body,
                };
                let mut p = Para::new(style);
                p.align = match it.next().unwrap_or("") {
                    "center" => Align::Center,
                    "right" => Align::Right,
                    _ => Align::Left,
                };
                p.level = it.next().and_then(|v| v.parse::<u8>().ok()).unwrap_or(0).min(8);
                doc.paras.push(p);
            }
            "t" => {
                let p = target_doc(&mut d, target).and_then(|doc| doc.paras.last_mut()).ok_or(DAMAGED)?;
                let t = unesc_line(val);
                p.push(&t, 0);
            }
            "f" => {
                let p = target_doc(&mut d, target).and_then(|doc| doc.paras.last_mut()).ok_or(DAMAGED)?;
                for run in val.split(' ').filter(|r| !r.is_empty()) {
                    let mut it = run.split(':');
                    let (Some(a), Some(n), Some(fl)) = (it.next(), it.next(), it.next()) else { continue };
                    let (Ok(a), Ok(n)) = (a.parse::<usize>(), n.parse::<usize>()) else { continue };
                    let mut f = 0u8;
                    for ch in fl.chars() {
                        f |= match ch {
                            'b' => BOLD,
                            'i' => ITALIC,
                            'u' => UNDERLINE,
                            's' => STRIKE,
                            _ => 0,
                        };
                    }
                    let e = a.saturating_add(n).min(p.len());
                    for x in p.fmt[a.min(e)..e].iter_mut() {
                        *x = f;
                    }
                }
            }
            "pic" => {
                let (_, b64) = val.split_once(' ').unwrap_or(("", val));
                let data = crate::crypto::base64_decode(b64.trim()).ok_or(DAMAGED)?;
                d.pics.push(Pic { data: alloc::rc::Rc::new(data) });
            }
            _ => {}
        }
    }
    if count.map_or(false, |c| c != d.slides.len()) {
        return Err(DAMAGED);
    }
    for s in d.slides.iter_mut() {
        for sh in s.shapes.iter_mut() {
            if sh.text.paras.is_empty() {
                sh.text = Doc::new();
            }
            if let Some(t) = sh.table.as_mut() {
                for c in t.cells.iter_mut() {
                    if c.paras.is_empty() {
                        *c = Doc::new();
                    }
                }
            }
            if sh.kind == Kind::Table && sh.table.is_none() {
                sh.table = Some(Table::new(2, 2, sh.w, sh.h));
            }
            if sh.kind == Kind::Chart && sh.chart.is_none() {
                sh.chart = Some(Chart::sample(ChartKind::Column));
            }
            if let Some(c) = sh.chart.as_mut() {
                let n = c.cats.len();
                for se in c.series.iter_mut() {
                    se.vals.resize(n, 0.0);
                }
            }
        }
    }
    if d.slides.is_empty() {
        let s = d.new_slide(Layout::Title);
        d.slides.push(s);
    }
    let n = d.pics.len();
    for s in d.slides.iter_mut() {
        for sh in s.shapes.iter_mut() {
            if sh.pic.map_or(false, |p| p >= n) {
                sh.pic = None;
            }
        }
    }
    Ok(d)
}

// ---- pictures ------------------------------------------------------------------

/// Encode ARGB pixels (straight alpha) as a PNG: each row filtered the way
/// that looks smallest, then deflated.
pub fn png_encode(w: u32, h: u32, px: &[u32]) -> Vec<u8> {
    let stride = w as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * h as usize);
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut cand = vec![0u8; stride];
    for y in 0..h as usize {
        for (x, &p) in px[y * w as usize..(y + 1) * w as usize].iter().enumerate() {
            cur[x * 4..x * 4 + 4].copy_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
        }
        let mut best = (u64::MAX, 0u8, Vec::new());
        for f in 0..5u8 {
            for i in 0..stride {
                let a = if i >= 4 { cur[i - 4] } else { 0 };
                let b = prev[i];
                let c = if i >= 4 { prev[i - 4] } else { 0 };
                let pred = match f {
                    0 => 0,
                    1 => a,
                    2 => b,
                    3 => ((a as u16 + b as u16) / 2) as u8,
                    _ => {
                        let p = a as i16 + b as i16 - c as i16;
                        let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
                        if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            b
                        } else {
                            c
                        }
                    }
                };
                cand[i] = cur[i].wrapping_sub(pred);
            }
            let score: u64 = cand.iter().map(|&v| (v as i8).unsigned_abs() as u64).sum();
            if score < best.0 {
                best = (score, f, cand.clone());
            }
        }
        raw.push(best.1);
        raw.extend_from_slice(&best.2);
        core::mem::swap(&mut prev, &mut cur);
    }
    let z = crate::zip::zlib(&raw);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crate::zip::crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// Picture bytes as a PNG or JPEG a presentation can keep: PNG and JPEG are
/// kept as they are, other formats are decoded and stored as PNG.
pub fn picture_bytes(data: &[u8]) -> Option<Vec<u8>> {
    if data.starts_with(b"\x89PNG") || data.starts_with(&[0xFF, 0xD8]) {
        return crate::image::decode(data).ok().map(|_| data.to_vec());
    }
    let img = crate::image::decode(data).ok()?;
    Some(png_encode(img.w, img.h, &img.px))
}

// ---- PowerPoint export ------------------------------------------------------

const XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\"";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT: &str = "application/vnd.openxmlformats-officedocument.presentationml";
const GRP: &str = "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>";
const FONT: &str = "Figtree";

fn rels(items: &[(String, &str, String)]) -> String {
    let mut s = format!("{}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">", XML);
    for (id, ty, target) in items {
        s.push_str(&format!("<Relationship Id=\"{}\" Type=\"{}/{}\" Target=\"{}\"/>", id, REL, ty, target));
    }
    s.push_str("</Relationships>");
    s
}

fn xfrm(x: i32, y: i32, w: i32, h: i32) -> String {
    format!("<a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>", x as i64 * EMU, y as i64 * EMU, w.max(1) as i64 * EMU, h.max(1) as i64 * EMU)
}

fn solid(c: u32) -> String {
    format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", hex6(c))
}

/// The text colour a shape shows (its own, or the theme's).
pub fn text_color(sh: &Shape, t: &Theme) -> u32 {
    if let Some(c) = sh.color {
        return c;
    }
    match sh.kind {
        Kind::Title => t.title,
        Kind::Rect | Kind::Ellipse => {
            if light(sh.fill.unwrap_or(t.accent)) {
                0x1E1B2C
            } else {
                0xFFFFFF
            }
        }
        _ => t.text,
    }
}

fn theme_xml(t: &Theme) -> String {
    let c = |name: &str, v: u32| format!("<a:{0}><a:srgbClr val=\"{1}\"/></a:{0}>", name, hex6(v));
    let accents = [t.accent, 0x4F7CAC, 0x5B9B6B, 0xD9A441, 0x8E6CB5, 0xC2504F];
    let mut s = format!("{}<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"{}\"><a:themeElements><a:clrScheme name=\"{}\">", XML, esc(&t.name), esc(&t.name));
    s.push_str(&c("dk1", t.title));
    s.push_str(&c("lt1", t.bg));
    s.push_str(&c("dk2", t.text));
    s.push_str(&c("lt2", if light(t.bg) { 0xE8E2D8 } else { 0x3A3748 }));
    for (i, a) in accents.iter().enumerate() {
        s.push_str(&c(&format!("accent{}", i + 1), *a));
    }
    s.push_str(&c("hlink", t.accent));
    s.push_str(&c("folHlink", t.accent));
    s.push_str("</a:clrScheme>");
    s.push_str(&format!("<a:fontScheme name=\"Hyda\"><a:majorFont><a:latin typeface=\"{0}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"{0}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme>", FONT));
    let ph = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    s.push_str(&format!("<a:fmtScheme name=\"Hyda\"><a:fillStyleLst>{0}{0}{0}</a:fillStyleLst><a:lnStyleLst><a:ln w=\"6350\">{0}</a:ln><a:ln w=\"12700\">{0}</a:ln><a:ln w=\"19050\">{0}</a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{0}{0}{0}</a:bgFillStyleLst></a:fmtScheme>", ph));
    s.push_str("</a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>");
    s
}

fn bg_xml(c: u32) -> String {
    format!("<p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg>", solid(c))
}

fn clr_map() -> &'static str {
    "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>"
}

fn master_xml(d: &Deck) -> String {
    let t = &d.theme;
    let mut s = format!("{}<p:sldMaster {}><p:cSld>{}<p:spTree>{}", XML, NS, bg_xml(t.bg), GRP);
    let mut id = 2;
    for dc in &t.deco {
        let (x, y, w, h) = dc.place(d.w, d.h);
        s.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"Decoration {}\"/><p:cNvSpPr/><p:nvPr userDrawn=\"1\"/></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"{}\"><a:avLst/></a:prstGeom>{}<a:ln><a:noFill/></a:ln></p:spPr></p:sp>", id, id - 1, xfrm(x, y, w, h), if dc.ellipse { "ellipse" } else { "rect" }, solid(dc.color)));
        id += 1;
    }
    let ph = layout_shapes(Layout::TitleContent, d.w, d.h);
    for (sh, (ty, name)) in ph.iter().zip([("type=\"title\"", "Title Placeholder"), ("type=\"body\" idx=\"1\"", "Text Placeholder")]) {
        s.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"{} {}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph {}/></p:nvPr></p:nvSpPr><p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp>", id, name, id - 1, ty, xfrm(sh.x, sh.y, sh.w, sh.h)));
        id += 1;
    }
    s.push_str("</p:spTree></p:cSld>");
    s.push_str(clr_map());
    s.push_str("<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst>");
    let lvl = |n: usize, sz: u32, bullet: bool| {
        let mar = (34 + 44 * (n as i64 - 1)) * EMU;
        let bu = if bullet { format!(" marL=\"{}\" indent=\"{}\"", mar, -34 * EMU) } else { String::new() };
        let buc = if bullet { "<a:buFont typeface=\"Arial\"/><a:buChar char=\"&#8226;\"/>" } else { "<a:buNone/>" };
        format!("<a:lvl{0}pPr{1} algn=\"l\">{2}<a:defRPr sz=\"{3}\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl{0}pPr>", n, bu, buc, sz)
    };
    s.push_str("<p:txStyles><p:titleStyle><a:lvl1pPr algn=\"l\"><a:buNone/><a:defRPr sz=\"4000\" b=\"1\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mj-lt\"/></a:defRPr></a:lvl1pPr></p:titleStyle><p:bodyStyle>");
    for n in 1..=5 {
        s.push_str(&lvl(n, [2400, 2000, 1800, 1600, 1600][n - 1], true));
    }
    s.push_str("</p:bodyStyle><p:otherStyle>");
    s.push_str(&lvl(1, 1800, false));
    s.push_str("</p:otherStyle></p:txStyles></p:sldMaster>");
    s
}

fn layout_xml() -> String {
    format!("{}<p:sldLayout {} type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>", XML, NS, GRP)
}

fn notes_master_xml() -> String {
    format!("{}<p:notesMaster {}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg><p:spTree>{}<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\" idx=\"2\"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"685800\" y=\"1143000\"/><a:ext cx=\"5486400\" cy=\"3086100\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" sz=\"quarter\" idx=\"3\"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"685800\" y=\"4400550\"/><a:ext cx=\"5486400\" cy=\"3600450\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p></p:txBody></p:sp></p:spTree></p:cSld>{}<p:notesStyle><a:lvl1pPr marL=\"0\" algn=\"l\"><a:defRPr sz=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill><a:latin typeface=\"+mn-lt\"/></a:defRPr></a:lvl1pPr></p:notesStyle></p:notesMaster>", XML, NS, GRP, clr_map())
}

fn notes_xml(text: &str) -> String {
    let mut paras = String::new();
    for line in text.split('\n') {
        if line.is_empty() {
            paras.push_str("<a:p><a:endParaRPr lang=\"en-US\" dirty=\"0\"/></a:p>");
        } else {
            paras.push_str(&format!("<a:p><a:r><a:rPr lang=\"en-US\" dirty=\"0\"/><a:t>{}</a:t></a:r></a:p>", esc(line)));
        }
    }
    format!("{}<p:notes {}><p:cSld><p:spTree>{}<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr><a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp><p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"body\" idx=\"1\"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>", XML, NS, GRP, paras)
}

/// `<a:p>` elements for a text body.
fn paras_xml(doc: &Doc, kind: Kind, size: u16, color: u32, force_bold: bool) -> String {
    let mut s = String::new();
    for p in &doc.paras {
        let pt = (size as i32 - 4 * p.level as i32).max(size as i32 * 3 / 5).max(6);
        let algn = match p.align {
            Align::Left => "l",
            Align::Center => "ctr",
            Align::Right => "r",
        };
        let base = p.level as i64 * 44;
        let (mar, ind, bu) = match p.style {
            Style::Bullet => ((base + 34) * EMU, -34 * EMU, format!("<a:buFont typeface=\"Arial\"/><a:buChar char=\"{}\"/>", if p.level % 2 == 0 { "&#8226;" } else { "&#8211;" })),
            Style::Number => ((base + 34) * EMU, -34 * EMU, String::from("<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"arabicPeriod\"/>")),
            _ => (base * EMU, 0, String::from("<a:buNone/>")),
        };
        let spc = if kind == Kind::Body { format!("<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>", pt * 30) } else { String::from("<a:spcBef><a:spcPts val=\"0\"/></a:spcBef>") };
        s.push_str(&format!("<a:p><a:pPr marL=\"{}\" lvl=\"{}\" indent=\"{}\" algn=\"{}\"><a:lnSpc><a:spcPct val=\"100000\"/></a:lnSpc>{}{}</a:pPr>", mar, p.level.min(8), ind, algn, spc, bu));
        let rpr = |f: u8| {
            let bold = f & BOLD != 0 || kind == Kind::Title || force_bold;
            format!(
                "<a:rPr lang=\"en-US\" sz=\"{}\"{}{}{}{} dirty=\"0\">{}<a:latin typeface=\"{}\"/></a:rPr>",
                pt * 100,
                if bold { " b=\"1\"" } else { " b=\"0\"" },
                if f & ITALIC != 0 { " i=\"1\"" } else { "" },
                if f & UNDERLINE != 0 { " u=\"sng\"" } else { "" },
                if f & STRIKE != 0 { " strike=\"sngStrike\"" } else { "" },
                solid(color),
                FONT
            )
        };
        let mut i = 0;
        while i < p.len() {
            let f = p.fmt[i];
            let mut j = i;
            while j < p.len() && p.fmt[j] == f {
                j += 1;
            }
            let run: String = p.text[i..j].iter().collect();
            s.push_str(&format!("<a:r>{}<a:t>{}</a:t></a:r>", rpr(f), esc(&run)));
            i = j;
        }
        s.push_str(&format!("<a:endParaRPr lang=\"en-US\" sz=\"{}\" dirty=\"0\">{}</a:endParaRPr></a:p>", pt * 100, solid(color)));
    }
    s
}

fn text_xml(sh: &Shape, t: &Theme) -> String {
    let tb = layout(sh);
    let anchor = match sh.anchor {
        Anchor::Top => "t",
        Anchor::Middle => "ctr",
        Anchor::Bottom => "b",
    };
    let fit = if tb.scale < 100 { format!("<a:normAutofit fontScale=\"{}\"/>", tb.scale * 1000) } else if sh.kind.placeholder() { String::from("<a:normAutofit/>") } else { String::new() };
    format!(
        "<p:txBody><a:bodyPr wrap=\"square\" lIns=\"{0}\" tIns=\"{1}\" rIns=\"{0}\" bIns=\"{1}\" anchor=\"{2}\" rtlCol=\"0\">{3}</a:bodyPr><a:lstStyle/>{4}</p:txBody>",
        INSET_X as i64 * EMU,
        INSET_Y as i64 * EMU,
        anchor,
        fit,
        paras_xml(&sh.text, sh.kind, sh.size, text_color(sh, t), false)
    )
}

/// `<a:xfrm>` with rotation and flips.
fn xfrm_full(sh: &Shape) -> String {
    let mut a = String::new();
    if sh.rot != 0 {
        a.push_str(&format!(" rot=\"{}\"", sh.rot as i64 * 60000));
    }
    if sh.flip_h {
        a.push_str(" flipH=\"1\"");
    }
    if sh.flip_v {
        a.push_str(" flipV=\"1\"");
    }
    format!("<a:xfrm{}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></a:xfrm>", a, sh.x as i64 * EMU, sh.y as i64 * EMU, sh.w.max(0) as i64 * EMU, sh.h.max(0) as i64 * EMU)
}

fn grad_xml(from: u32, to: u32, angle: i32) -> String {
    format!("<a:gradFill rotWithShape=\"1\"><a:gsLst><a:gs pos=\"0\">{}</a:gs><a:gs pos=\"100000\">{}</a:gs></a:gsLst><a:lin ang=\"{}\" scaled=\"0\"/></a:gradFill>", clr(from), clr(to), angle.rem_euclid(360) as i64 * 60000)
}

fn clr(c: u32) -> String {
    format!("<a:srgbClr val=\"{}\"/>", hex6(c))
}

/// The table as DrawingML.
fn table_xml(sh: &Shape, t: &Theme) -> String {
    let Some(tb) = &sh.table else { return String::new() };
    let mut s = format!("<a:tbl><a:tblPr firstRow=\"{}\" bandRow=\"{}\"/><a:tblGrid>", tb.header as u8, tb.banded as u8);
    for w in &tb.cols {
        s.push_str(&format!("<a:gridCol w=\"{}\"/>", *w as i64 * EMU));
    }
    s.push_str("</a:tblGrid>");
    let line = table_line(t);
    let ln = |side: &str| format!("<a:{0} w=\"9525\"><a:solidFill>{1}</a:solidFill></a:{0}>", side, clr(line));
    for r in 0..tb.nrows() {
        s.push_str(&format!("<a:tr h=\"{}\">", tb.rows[r] as i64 * EMU));
        let (fill, text, bold) = cell_style(t, tb, r);
        let text = sh.color.filter(|_| !(tb.header && r == 0)).unwrap_or(text);
        for c in 0..tb.ncols() {
            s.push_str(&format!(
                "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr marL=\"{3}\" marR=\"{3}\" marT=\"{4}\" marB=\"{4}\" anchor=\"ctr\">{1}<a:solidFill>{2}</a:solidFill></a:tcPr></a:tc>",
                paras_xml(tb.cell(r, c), Kind::Text, sh.size, text, bold),
                [ln("lnL"), ln("lnR"), ln("lnT"), ln("lnB")].concat(),
                clr(fill),
                INSET_X as i64 * EMU,
                INSET_Y as i64 * EMU
            ));
        }
        s.push_str("</a:tr>");
    }
    s.push_str("</a:tbl>");
    s
}

const C_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";

fn col_letter(c: usize) -> String {
    crate::grid::col_name(c as u32)
}

/// A chart part (c:chartSpace) that reads its data from an embedded workbook.
fn chart_xml(ch: &Chart, t: &Theme) -> String {
    let n = ch.cats.len();
    let mut ser = String::new();
    let cat_ref = format!("Sheet1!$A$2:$A${}", n + 1);
    let mut cat_cache = format!("<c:ptCount val=\"{}\"/>", n);
    for (i, c) in ch.cats.iter().enumerate() {
        cat_cache.push_str(&format!("<c:pt idx=\"{}\"><c:v>{}</c:v></c:pt>", i, esc(c)));
    }
    for (k, se) in ch.series.iter().enumerate() {
        let col = col_letter(k + 1);
        let color = series_color(t, k);
        let sp = match ch.kind {
            ChartKind::Line => format!("<c:spPr><a:ln w=\"38100\" cap=\"rnd\"><a:solidFill>{}</a:solidFill><a:round/></a:ln></c:spPr><c:marker><c:symbol val=\"none\"/></c:marker>", clr(color)),
            _ => format!("<c:spPr><a:solidFill>{}</a:solidFill></c:spPr>", clr(color)),
        };
        let mut extra = String::new();
        if matches!(ch.kind, ChartKind::Column | ChartKind::Bar) {
            extra.push_str("<c:invertIfNegative val=\"0\"/>");
        }
        if ch.kind == ChartKind::Pie {
            for i in 0..n {
                extra.push_str(&format!("<c:dPt><c:idx val=\"{}\"/><c:bubble3D val=\"0\"/><c:spPr><a:solidFill>{}</a:solidFill><a:ln w=\"19050\"><a:solidFill>{}</a:solidFill></a:ln></c:spPr></c:dPt>", i, clr(series_color(t, i)), clr(t.bg)));
            }
        }
        let mut vals = format!("<c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>", n);
        for (i, v) in se.vals.iter().enumerate().take(n) {
            vals.push_str(&format!("<c:pt idx=\"{}\"><c:v>{}</c:v></c:pt>", i, v));
        }
        ser.push_str(&format!(
            "<c:ser><c:idx val=\"{0}\"/><c:order val=\"{0}\"/><c:tx><c:strRef><c:f>Sheet1!${1}$1</c:f><c:strCache><c:ptCount val=\"1\"/><c:pt idx=\"0\"><c:v>{2}</c:v></c:pt></c:strCache></c:strRef></c:tx>{3}{4}<c:cat><c:strRef><c:f>{5}</c:f><c:strCache>{6}</c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>Sheet1!${1}$2:${1}${7}</c:f><c:numCache>{8}</c:numCache></c:numRef></c:val>{9}</c:ser>",
            k,
            col,
            esc(&se.name),
            sp,
            extra,
            cat_ref,
            cat_cache,
            n + 1,
            vals,
            if ch.kind == ChartKind::Line { "<c:smooth val=\"0\"/>" } else { "" }
        ));
    }
    let axes_ids = "<c:axId val=\"50010\"/><c:axId val=\"50020\"/>";
    let plot = match ch.kind {
        ChartKind::Column | ChartKind::Bar => format!("<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"clustered\"/><c:varyColors val=\"0\"/>{}<c:gapWidth val=\"80\"/>{}</c:barChart>", if ch.kind == ChartKind::Bar { "bar" } else { "col" }, ser, axes_ids),
        ChartKind::Line => format!("<c:lineChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{}<c:marker val=\"1\"/>{}</c:lineChart>", ser, axes_ids),
        ChartKind::Area => format!("<c:areaChart><c:grouping val=\"standard\"/><c:varyColors val=\"0\"/>{}{}</c:areaChart>", ser, axes_ids),
        ChartKind::Pie => format!("<c:pieChart><c:varyColors val=\"1\"/>{}<c:firstSliceAng val=\"0\"/></c:pieChart>", ser),
    };
    let grid = format!("<c:spPr><a:ln w=\"9525\"><a:solidFill>{}</a:solidFill></a:ln></c:spPr>", clr(mix(t.bg, t.text, 50)));
    let axes = if ch.kind == ChartKind::Pie {
        String::new()
    } else {
        let (cpos, vpos) = if ch.kind == ChartKind::Bar { ("l", "b") } else { ("b", "l") };
        format!(
            "<c:catAx><c:axId val=\"50010\"/><c:scaling><c:orientation val=\"{}\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{}\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:majorTickMark val=\"none\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/>{}<c:crossAx val=\"50020\"/><c:crosses val=\"autoZero\"/><c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx><c:valAx><c:axId val=\"50020\"/><c:scaling><c:orientation val=\"minMax\"/></c:scaling><c:delete val=\"0\"/><c:axPos val=\"{}\"/><c:majorGridlines>{}</c:majorGridlines><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:majorTickMark val=\"none\"/><c:minorTickMark val=\"none\"/><c:tickLblPos val=\"nextTo\"/><c:spPr><a:ln><a:noFill/></a:ln></c:spPr><c:crossAx val=\"50010\"/><c:crosses val=\"autoZero\"/><c:crossBetween val=\"between\"/></c:valAx>",
            if ch.kind == ChartKind::Bar { "maxMin" } else { "minMax" },
            cpos,
            grid,
            vpos,
            grid
        )
    };
    let title = if ch.title.is_empty() {
        String::from("<c:autoTitleDeleted val=\"1\"/>")
    } else {
        format!("<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"1800\" b=\"1\">{}</a:defRPr></a:pPr><a:r><a:rPr lang=\"en-US\" sz=\"1800\" b=\"1\">{}</a:rPr><a:t>{}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title><c:autoTitleDeleted val=\"0\"/>", solid(t.title), solid(t.title), esc(&ch.title))
    };
    let legend = if ch.legend { "<c:legend><c:legendPos val=\"b\"/><c:overlay val=\"0\"/></c:legend>" } else { "" };
    format!(
        "{}<c:chartSpace xmlns:c=\"{}\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"{}\"><c:roundedCorners val=\"0\"/><c:chart>{}<c:plotArea><c:layout/>{}{}</c:plotArea>{}<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart><c:spPr><a:noFill/><a:ln><a:noFill/></a:ln></c:spPr><c:txPr><a:bodyPr/><a:lstStyle/><a:p><a:pPr><a:defRPr sz=\"1400\">{}<a:latin typeface=\"{}\"/></a:defRPr></a:pPr><a:endParaRPr lang=\"en-US\"/></a:p></c:txPr><c:externalData r:id=\"rId1\"><c:autoUpdate val=\"0\"/></c:externalData></c:chartSpace>",
        XML,
        C_NS,
        REL,
        title,
        plot,
        axes,
        legend,
        solid(t.text),
        FONT
    )
}

/// The chart's data as a workbook, so other programs can edit it.
fn chart_workbook(ch: &Chart) -> Vec<u8> {
    let mut sh = crate::grid::Sheet::new();
    sh.name = String::from("Sheet1");
    for (k, se) in ch.series.iter().enumerate() {
        sh.set_input(0, k as u32 + 1, &format!("'{}", se.name));
        for (i, v) in se.vals.iter().enumerate() {
            sh.set_input(i as u32 + 1, k as u32 + 1, &format!("{}", v));
        }
    }
    for (i, c) in ch.cats.iter().enumerate() {
        sh.set_input(i as u32 + 1, 0, &format!("'{}", c));
    }
    crate::gridio::to_xlsx(&sh)
}

/// Entrance animations as PowerPoint's timing tree; `ids` are the shapes'
/// ids on the slide, in the order of `s.shapes`.
fn timing_xml(s: &Slide, ids: &[Option<u32>]) -> String {
    let mut order: Vec<usize> = (0..s.shapes.len()).filter(|&i| s.shapes[i].anim != Anim::None && ids.get(i).copied().flatten().is_some()).collect();
    if order.is_empty() {
        return String::new();
    }
    order.sort_by_key(|&i| (s.shapes[i].anim_order, i));
    let mut ctn = 3;
    let mut clicks = String::new();
    for (grp, &i) in order.iter().enumerate() {
        let spid = ids[i].unwrap();
        let tgt = format!("<p:tgtEl><p:spTgt spid=\"{}\"/></p:tgtEl>", spid);
        let (preset, sub) = match s.shapes[i].anim {
            Anim::Appear => (1, 0),
            Anim::Fly => (2, 4),
            _ => (10, 0),
        };
        let (a, b, c, d) = (ctn, ctn + 1, ctn + 2, ctn + 3);
        let set = format!("<p:set><p:cBhvr><p:cTn id=\"{}\" dur=\"1\" fill=\"hold\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst></p:cTn>{}<p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr><p:to><p:strVal val=\"visible\"/></p:to></p:set>", d, tgt);
        let (effect, used) = match s.shapes[i].anim {
            Anim::Appear => (String::new(), 0),
            Anim::Fly => (
                format!(
                    "<p:anim calcmode=\"lin\" valueType=\"num\"><p:cBhvr additive=\"base\"><p:cTn id=\"{0}\" dur=\"500\" fill=\"hold\"/>{2}<p:attrNameLst><p:attrName>ppt_x</p:attrName></p:attrNameLst></p:cBhvr><p:tavLst><p:tav tm=\"0\"><p:val><p:strVal val=\"#ppt_x\"/></p:val></p:tav><p:tav tm=\"100000\"><p:val><p:strVal val=\"#ppt_x\"/></p:val></p:tav></p:tavLst></p:anim><p:anim calcmode=\"lin\" valueType=\"num\"><p:cBhvr additive=\"base\"><p:cTn id=\"{1}\" dur=\"500\" fill=\"hold\"/>{2}<p:attrNameLst><p:attrName>ppt_y</p:attrName></p:attrNameLst></p:cBhvr><p:tavLst><p:tav tm=\"0\"><p:val><p:strVal val=\"1+#ppt_h/2\"/></p:val></p:tav><p:tav tm=\"100000\"><p:val><p:strVal val=\"#ppt_y\"/></p:val></p:tav></p:tavLst></p:anim>",
                    d + 1,
                    d + 2,
                    tgt
                ),
                2,
            ),
            _ => (format!("<p:animEffect transition=\"in\" filter=\"fade\"><p:cBhvr><p:cTn id=\"{}\" dur=\"500\"/>{}</p:cBhvr></p:animEffect>", d + 1, tgt), 1),
        };
        clicks.push_str(&format!(
            "<p:par><p:cTn id=\"{}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst><p:childTnLst><p:par><p:cTn id=\"{}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst><p:childTnLst><p:par><p:cTn id=\"{}\" presetID=\"{}\" presetClass=\"entr\" presetSubtype=\"{}\" fill=\"hold\" grpId=\"{}\" nodeType=\"clickEffect\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst><p:childTnLst>{}{}</p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>",
            a, b, c, preset, sub, 0, set, effect
        ));
        let _ = grp;
        ctn += 4 + used;
    }
    let mut bld = String::new();
    for &i in &order {
        bld.push_str(&format!("<p:bldP spid=\"{}\" grpId=\"0\"/>", ids[i].unwrap()));
    }
    format!(
        "<p:timing><p:tnLst><p:par><p:cTn id=\"1\" dur=\"indefinite\" restart=\"never\" nodeType=\"tmRoot\"><p:childTnLst><p:seq concurrent=\"1\" nextAc=\"seek\"><p:cTn id=\"2\" dur=\"indefinite\" nodeType=\"mainSeq\"><p:childTnLst>{}</p:childTnLst></p:cTn><p:prevCondLst><p:cond evt=\"onPrev\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst><p:nextCondLst><p:cond evt=\"onNext\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst><p:bldLst>{}</p:bldLst></p:timing>",
        clicks, bld
    )
}

fn footer_xml(d: &Deck, num: usize, id: &mut u32) -> String {
    let t = &d.theme;
    let ((fx, fy, fw, fh), (nx, ny, nw, nh)) = footer_rects(d.w, d.h);
    let col = footer_color(t);
    let rpr = format!("<a:rPr lang=\"en-US\" sz=\"1200\" dirty=\"0\">{}<a:latin typeface=\"{}\"/></a:rPr>", solid(col), FONT);
    let body = "<a:bodyPr lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" anchor=\"ctr\"/><a:lstStyle/>";
    let mut s = String::new();
    if !d.footer.is_empty() {
        s.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"Footer\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"ftr\" sz=\"quarter\" idx=\"11\"/></p:nvPr></p:nvSpPr><p:spPr>{}</p:spPr><p:txBody>{}<a:p><a:pPr algn=\"ctr\"/><a:r>{}<a:t>{}</a:t></a:r></a:p></p:txBody></p:sp>", id, xfrm(fx, fy, fw, fh), body, rpr, esc(&d.footer)));
        *id += 1;
    }
    if d.numbers {
        s.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{}\" name=\"Slide Number\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"sldNum\" sz=\"quarter\" idx=\"12\"/></p:nvPr></p:nvSpPr><p:spPr>{}</p:spPr><p:txBody>{}<a:p><a:pPr algn=\"r\"/><a:fld id=\"{{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}}\" type=\"slidenum\">{}<a:t>{}</a:t></a:fld></a:p></p:txBody></p:sp>", id, xfrm(nx, ny, nw, nh), body, rpr, num));
        *id += 1;
    }
    s
}

fn slide_xml(d: &Deck, s: &Slide, num: usize, pic_rel: &BTreeMap<usize, String>, chart_rel: &BTreeMap<usize, String>) -> String {
    let mut x = format!("{}<p:sld {}><p:cSld>", XML, NS);
    match (s.bg, s.bg_grad) {
        (bg, Some((to, a))) => x.push_str(&format!("<p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg>", grad_xml(bg.unwrap_or(d.theme.bg), to, a))),
        (Some(bg), None) => x.push_str(&bg_xml(bg)),
        _ => {}
    }
    x.push_str("<p:spTree>");
    x.push_str(GRP);
    let mut id: u32 = 2;
    let mut body_idx = 1;
    let mut ids: Vec<Option<u32>> = vec![None; s.shapes.len()];
    for (si, sh) in s.shapes.iter().enumerate() {
        if sh.kind.placeholder() && sh.is_empty() {
            continue;
        }
        let name = match sh.kind {
            Kind::Title => "Title",
            Kind::Subtitle => "Subtitle",
            Kind::Body => "Content",
            Kind::Text => "TextBox",
            Kind::Rect => "Shape",
            Kind::Ellipse => "Oval",
            Kind::Picture => "Picture",
            Kind::Line => "Connector",
            Kind::Table => "Table",
            Kind::Chart => "Chart",
        };
        match sh.kind {
            Kind::Picture => {
                if let Some(rid) = sh.pic.and_then(|p| pic_rel.get(&p)) {
                    let border = match sh.line {
                        Some(c) => format!("<a:ln w=\"{}\">{}</a:ln>", sh.line_w as i64 * EMU, solid(c)),
                        None => String::new(),
                    };
                    x.push_str(&format!("<p:pic><p:nvPicPr><p:cNvPr id=\"{0}\" name=\"{1} {0}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{2}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{3}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>{4}</p:spPr></p:pic>", id, name, rid, xfrm_full(sh), border));
                    ids[si] = Some(id);
                    id += 1;
                }
                continue;
            }
            Kind::Line => {
                let c = sh.line.unwrap_or(d.theme.text);
                x.push_str(&format!(
                    "<p:cxnSp><p:nvCxnSpPr><p:cNvPr id=\"{0}\" name=\"{1} {0}\"/><p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr>{2}<a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom><a:ln w=\"{3}\">{4}{5}{6}</a:ln></p:spPr></p:cxnSp>",
                    id,
                    name,
                    xfrm_full(sh),
                    sh.line_w as i64 * EMU,
                    solid(c),
                    if sh.head { "<a:headEnd type=\"triangle\"/>" } else { "" },
                    if sh.tail { "<a:tailEnd type=\"triangle\"/>" } else { "" }
                ));
                ids[si] = Some(id);
                id += 1;
                continue;
            }
            Kind::Table | Kind::Chart => {
                let data = if sh.kind == Kind::Table {
                    format!("<a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\">{}</a:graphicData>", table_xml(sh, &d.theme))
                } else {
                    match chart_rel.get(&si) {
                        Some(rid) => format!("<a:graphicData uri=\"{0}\"><c:chart xmlns:c=\"{0}\" r:id=\"{1}\"/></a:graphicData>", C_NS, rid),
                        None => continue,
                    }
                };
                x.push_str(&format!(
                    "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{0}\" name=\"{1} {0}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{2}\" y=\"{3}\"/><a:ext cx=\"{4}\" cy=\"{5}\"/></p:xfrm><a:graphic>{6}</a:graphic></p:graphicFrame>",
                    id,
                    name,
                    sh.x as i64 * EMU,
                    sh.y as i64 * EMU,
                    sh.w as i64 * EMU,
                    sh.h as i64 * EMU,
                    data
                ));
                ids[si] = Some(id);
                id += 1;
                continue;
            }
            _ => {}
        }
        let ph = match sh.kind {
            Kind::Title => String::from("<p:ph type=\"title\"/>"),
            Kind::Subtitle => {
                body_idx += 1;
                format!("<p:ph type=\"subTitle\" idx=\"{}\"/>", body_idx - 1)
            }
            Kind::Body => {
                body_idx += 1;
                format!("<p:ph idx=\"{}\"/>", body_idx - 1)
            }
            _ => String::new(),
        };
        let locks = if sh.kind.placeholder() { "<p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>" } else if sh.kind == Kind::Text { "<p:cNvSpPr txBox=\"1\"/>" } else { "<p:cNvSpPr/>" };
        let geom = if sh.kind == Kind::Ellipse { "ellipse" } else if sh.kind == Kind::Rect { sh.geom.id() } else { "rect" };
        let base = match (sh.kind, sh.fill) {
            (_, Some(c)) => Some(c),
            (Kind::Rect | Kind::Ellipse, None) => Some(d.theme.accent),
            _ => None,
        };
        let fill = match (base, sh.grad) {
            (Some(c), Some((to, a))) => grad_xml(c, to, a),
            (Some(c), None) => solid(c),
            (None, _) => String::from("<a:noFill/>"),
        };
        let line = match sh.line {
            Some(c) => format!("<a:ln w=\"{}\">{}</a:ln>", sh.line_w as i64 * EMU, solid(c)),
            None => String::from("<a:ln><a:noFill/></a:ln>"),
        };
        x.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{0}\" name=\"{1} {0}\"/>{2}<p:nvPr>{3}</p:nvPr></p:nvSpPr><p:spPr>{4}<a:prstGeom prst=\"{5}\"><a:avLst/></a:prstGeom>{6}{7}</p:spPr>{8}</p:sp>", id, name, locks, ph, xfrm_full(sh), geom, fill, line, text_xml(sh, &d.theme)));
        ids[si] = Some(id);
        id += 1;
    }
    if shows_footer(d, s) {
        x.push_str(&footer_xml(d, num, &mut id));
    }
    x.push_str("</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>");
    match s.trans {
        Trans::Fade => x.push_str("<p:transition spd=\"med\"><p:fade/></p:transition>"),
        Trans::Push => x.push_str("<p:transition spd=\"med\"><p:push dir=\"u\"/></p:transition>"),
        Trans::None => {}
    }
    x.push_str(&timing_xml(s, &ids));
    x.push_str("</p:sld>");
    x
}

pub fn to_pptx(d: &Deck) -> Vec<u8> {
    let mut z = crate::zip::Writer::new();
    let has_notes = d.slides.iter().any(|s| !s.notes.is_empty());
    // charts, numbered through the deck
    let mut charts: Vec<(usize, usize)> = Vec::new();
    for (i, s) in d.slides.iter().enumerate() {
        for (k, sh) in s.shapes.iter().enumerate() {
            if sh.kind == Kind::Chart && sh.chart.is_some() {
                charts.push((i, k));
            }
        }
    }
    // content types
    let mut ct = format!("{}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/><Default Extension=\"xlsx\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet\"/>", XML);
    ct.push_str(&format!("<Override PartName=\"/ppt/presentation.xml\" ContentType=\"{}.presentation.main+xml\"/>", CT));
    ct.push_str(&format!("<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"{}.slideMaster+xml\"/>", CT));
    ct.push_str(&format!("<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"{}.slideLayout+xml\"/>", CT));
    ct.push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    ct.push_str(&format!("<Override PartName=\"/ppt/presProps.xml\" ContentType=\"{}.presProps+xml\"/><Override PartName=\"/ppt/viewProps.xml\" ContentType=\"{}.viewProps+xml\"/><Override PartName=\"/ppt/tableStyles.xml\" ContentType=\"{}.tableStyles+xml\"/>", CT, CT, CT));
    ct.push_str("<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>");
    if has_notes {
        ct.push_str(&format!("<Override PartName=\"/ppt/notesMasters/notesMaster1.xml\" ContentType=\"{}.notesMaster+xml\"/>", CT));
        ct.push_str("<Override PartName=\"/ppt/theme/theme2.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    }
    for (i, s) in d.slides.iter().enumerate() {
        ct.push_str(&format!("<Override PartName=\"/ppt/slides/slide{}.xml\" ContentType=\"{}.slide+xml\"/>", i + 1, CT));
        if !s.notes.is_empty() {
            ct.push_str(&format!("<Override PartName=\"/ppt/notesSlides/notesSlide{}.xml\" ContentType=\"{}.notesSlide+xml\"/>", i + 1, CT));
        }
    }
    for n in 0..charts.len() {
        ct.push_str(&format!("<Override PartName=\"/ppt/charts/chart{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawingml.chart+xml\"/>", n + 1));
    }
    ct.push_str("</Types>");
    z.add("[Content_Types].xml", ct.as_bytes());
    z.add("_rels/.rels", rels(&[("rId1".into(), "officeDocument", "ppt/presentation.xml".into()), ("rId2".into(), "extended-properties", "docProps/app.xml".into()), ("rId3".into(), "metadata/core-properties", "docProps/core.xml".into())]).replace(&format!("{}/metadata/core-properties", REL), "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties").as_bytes());
    z.add("docProps/app.xml", format!("{}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Hyda Slides</Application><Slides>{}</Slides></Properties>", XML, d.slides.len()).as_bytes());
    z.add("docProps/core.xml", crate::doc::core_xml(&d.name, &d.author).as_bytes());
    // presentation
    let mut pr: Vec<(String, &str, String)> = vec![("rId1".into(), "slideMaster", "slideMasters/slideMaster1.xml".into()), ("rId2".into(), "theme", "theme/theme1.xml".into()), ("rId3".into(), "presProps", "presProps.xml".into()), ("rId4".into(), "viewProps", "viewProps.xml".into()), ("rId5".into(), "tableStyles", "tableStyles.xml".into())];
    if has_notes {
        pr.push(("rId6".into(), "notesMaster", "notesMasters/notesMaster1.xml".into()));
    }
    let mut ids = String::new();
    for i in 0..d.slides.len() {
        pr.push((format!("rId{}", 10 + i), "slide", format!("slides/slide{}.xml", i + 1)));
        ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{}\"/>", 256 + i, 10 + i));
    }
    let nm = if has_notes { "<p:notesMasterIdLst><p:notesMasterId r:id=\"rId6\"/></p:notesMasterIdLst>" } else { "" };
    z.add("ppt/presentation.xml", format!("{}<p:presentation {} saveSubsetFonts=\"1\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>{}<p:sldIdLst>{}</p:sldIdLst><p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>", XML, NS, nm, ids, d.w as i64 * EMU, d.h as i64 * EMU).as_bytes());
    z.add("ppt/_rels/presentation.xml.rels", rels(&pr).as_bytes());
    z.add("ppt/presProps.xml", format!("{}<p:presentationPr {}/>", XML, NS).as_bytes());
    z.add("ppt/viewProps.xml", format!("{}<p:viewPr {}><p:normalViewPr><p:restoredLeft sz=\"15620\"/><p:restoredTop sz=\"94660\"/></p:normalViewPr><p:gridSpacing cx=\"76200\" cy=\"76200\"/></p:viewPr>", XML, NS).as_bytes());
    z.add("ppt/tableStyles.xml", format!("{}<a:tblStyleLst xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" def=\"{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}\"/>", XML).as_bytes());
    z.add("ppt/theme/theme1.xml", theme_xml(&d.theme).as_bytes());
    z.add("ppt/slideMasters/slideMaster1.xml", master_xml(d).as_bytes());
    z.add("ppt/slideMasters/_rels/slideMaster1.xml.rels", rels(&[("rId1".into(), "slideLayout", "../slideLayouts/slideLayout1.xml".into()), ("rId2".into(), "theme", "../theme/theme1.xml".into())]).as_bytes());
    z.add("ppt/slideLayouts/slideLayout1.xml", layout_xml().as_bytes());
    z.add("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels(&[("rId1".into(), "slideMaster", "../slideMasters/slideMaster1.xml".into())]).as_bytes());
    if has_notes {
        z.add("ppt/theme/theme2.xml", theme_xml(&theme("paper")).as_bytes());
        z.add("ppt/notesMasters/notesMaster1.xml", notes_master_xml().as_bytes());
        z.add("ppt/notesMasters/_rels/notesMaster1.xml.rels", rels(&[("rId1".into(), "theme", "../theme/theme2.xml".into())]).as_bytes());
    }
    // pictures
    for (i, p) in d.pics.iter().enumerate() {
        let ext = if p.data.starts_with(b"\x89PNG") { "png" } else { "jpeg" };
        z.add(&format!("ppt/media/image{}.{}", i + 1, ext), &p.data);
    }
    // charts and their workbooks
    for (n, &(si, k)) in charts.iter().enumerate() {
        let ch = d.slides[si].shapes[k].chart.as_ref().unwrap();
        z.add(&format!("ppt/charts/chart{}.xml", n + 1), chart_xml(ch, &d.theme).as_bytes());
        z.add(&format!("ppt/embeddings/Microsoft_Excel_Worksheet{}.xlsx", n + 1), &chart_workbook(ch));
        z.add(&format!("ppt/charts/_rels/chart{}.xml.rels", n + 1), rels(&[("rId1".into(), "package", format!("../embeddings/Microsoft_Excel_Worksheet{}.xlsx", n + 1))]).as_bytes());
    }
    for (i, s) in d.slides.iter().enumerate() {
        let mut r: Vec<(String, &str, String)> = vec![("rId1".into(), "slideLayout", "../slideLayouts/slideLayout1.xml".into())];
        if !s.notes.is_empty() {
            r.push(("rId2".into(), "notesSlide", format!("../notesSlides/notesSlide{}.xml", i + 1)));
        }
        let mut pic_rel = BTreeMap::new();
        let mut chart_rel = BTreeMap::new();
        let mut next = 3;
        for (k, sh) in s.shapes.iter().enumerate() {
            if let Some(p) = sh.pic.filter(|&p| p < d.pics.len() && sh.kind == Kind::Picture) {
                if !pic_rel.contains_key(&p) {
                    let rid = format!("rId{}", next);
                    next += 1;
                    let ext = if d.pics[p].data.starts_with(b"\x89PNG") { "png" } else { "jpeg" };
                    r.push((rid.clone(), "image", format!("../media/image{}.{}", p + 1, ext)));
                    pic_rel.insert(p, rid);
                }
            }
            if let Some(n) = charts.iter().position(|&c| c == (i, k)) {
                let rid = format!("rId{}", next);
                next += 1;
                r.push((rid.clone(), "chart", format!("../charts/chart{}.xml", n + 1)));
                chart_rel.insert(k, rid);
            }
        }
        z.add(&format!("ppt/slides/slide{}.xml", i + 1), slide_xml(d, s, i + 1, &pic_rel, &chart_rel).as_bytes());
        z.add(&format!("ppt/slides/_rels/slide{}.xml.rels", i + 1), rels(&r).as_bytes());
        if !s.notes.is_empty() {
            z.add(&format!("ppt/notesSlides/notesSlide{}.xml", i + 1), notes_xml(&s.notes).as_bytes());
            z.add(&format!("ppt/notesSlides/_rels/notesSlide{}.xml.rels", i + 1), rels(&[("rId1".into(), "notesMaster", "../notesMasters/notesMaster1.xml".into()), ("rId2".into(), "slide", format!("../slides/slide{}.xml", i + 1))]).as_bytes());
        }
    }
    z.finish()
}

// ---- PowerPoint import ------------------------------------------------------------

/// A parsed XML element.
#[derive(Default, Debug)]
struct El {
    name: String,
    attrs: String,
    kids: Vec<El>,
    text: String,
}

impl El {
    fn parse(s: &str) -> El {
        let mut stack: Vec<El> = vec![El::default()];
        for t in Tokens::new(s) {
            match t {
                Tok::Open(name, attrs, empty) => {
                    let e = El { name: name.to_string(), attrs: attrs.to_string(), ..El::default() };
                    if empty {
                        stack.last_mut().unwrap().kids.push(e);
                    } else {
                        stack.push(e);
                    }
                }
                Tok::Close(_) => {
                    if stack.len() > 1 {
                        let e = stack.pop().unwrap();
                        stack.last_mut().unwrap().kids.push(e);
                    }
                }
                Tok::Text(t) => {
                    if let Some(top) = stack.last_mut() {
                        if !t.trim().is_empty() || top.local() == "t" {
                            top.text.push_str(&unesc(t));
                        }
                    }
                }
            }
        }
        while stack.len() > 1 {
            let e = stack.pop().unwrap();
            stack.last_mut().unwrap().kids.push(e);
        }
        stack.pop().unwrap()
    }
    fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }
    fn kid(&self, local: &str) -> Option<&El> {
        self.kids.iter().find(|k| k.local() == local)
    }
    fn kids<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.kids.iter().filter(move |k| k.local() == local)
    }
    fn path(&self, p: &str) -> Option<&El> {
        let mut e = self;
        for part in p.split('/') {
            e = e.kid(part)?;
        }
        Some(e)
    }
    fn attr(&self, n: &str) -> &str {
        attr(&self.attrs, n)
    }
    fn num(&self, n: &str) -> Option<i64> {
        self.attr(n).parse().ok()
    }
}

/// Resolve a relative part name against a part's folder.
fn resolve(base: &str, target: &str) -> String {
    if let Some(t) = target.strip_prefix('/') {
        return t.to_string();
    }
    let mut parts: Vec<&str> = base.rsplit_once('/').map(|x| x.0).unwrap_or("").split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// Relationships of a part: id -> (type, part name).
fn read_rels(z: &[u8], part: &str) -> BTreeMap<String, (String, String)> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rp = if dir.is_empty() { format!("_rels/{}.rels", file) } else { format!("{}/_rels/{}.rels", dir, file) };
    let mut m = BTreeMap::new();
    if let Some(x) = crate::zip::read(z, &rp) {
        let e = El::parse(&String::from_utf8_lossy(&x));
        if let Some(root) = e.kid("Relationships") {
            for r in root.kids("Relationship") {
                let ty = r.attr("Type").rsplit('/').next().unwrap_or("").to_string();
                let target = if r.attr("TargetMode") == "External" { String::new() } else { resolve(part, &unesc(r.attr("Target"))) };
                m.insert(r.attr("Id").to_string(), (ty, target));
            }
        }
    }
    m
}

fn read_xml(z: &[u8], part: &str) -> Option<El> {
    let x = crate::zip::read(z, part)?;
    Some(El::parse(&String::from_utf8_lossy(&x)))
}

struct Ctx {
    scheme: BTreeMap<String, u32>,
    /// EMU per unit on this deck
    emu: i64,
    /// point sizes are scaled by this (1/100000) when the deck isn't 13.33" wide
    font_pct: i64,
}

impl Ctx {
    fn color(&self, e: &El) -> Option<u32> {
        if let Some(c) = e.kid("srgbClr") {
            return parse_hex(&c.attr("val").to_ascii_uppercase());
        }
        if let Some(c) = e.kid("schemeClr") {
            let v = c.attr("val");
            let key = match v {
                "tx1" => "dk1",
                "bg1" => "lt1",
                "tx2" => "dk2",
                "bg2" => "lt2",
                o => o,
            };
            return self.scheme.get(key).copied();
        }
        if let Some(c) = e.kid("sysClr") {
            return parse_hex(&c.attr("lastClr").to_ascii_uppercase()).or(Some(if c.attr("val") == "window" { 0xFFFFFF } else { 0 }));
        }
        if let Some(c) = e.kid("prstClr") {
            return Some(match c.attr("val") {
                "white" => 0xFFFFFF,
                "red" => 0xFF0000,
                "green" => 0x008000,
                "blue" => 0x0000FF,
                "yellow" => 0xFFFF00,
                "gray" | "grey" => 0x808080,
                _ => 0,
            });
        }
        None
    }
    fn fill(&self, e: &El) -> Option<Option<u32>> {
        if e.kid("noFill").is_some() {
            return Some(None);
        }
        if let Some(f) = e.kid("solidFill") {
            return Some(self.color(f));
        }
        if let Some(g) = e.kid("gradFill") {
            // a gradient shows its first colour
            if let Some(gs) = g.path("gsLst/gs") {
                return Some(self.color(gs));
            }
        }
        None
    }
    /// A gradient fill: (its last colour, its angle).
    fn grad(&self, e: &El) -> Option<(u32, i32)> {
        let g = e.kid("gradFill")?;
        let stops: Vec<&El> = g.kid("gsLst")?.kids("gs").collect();
        let last = stops.iter().max_by_key(|s| s.num("pos").unwrap_or(0))?;
        let ang = g.kid("lin").and_then(|l| l.num("ang")).map(|a| (a / 60000) as i32).unwrap_or(90);
        Some((self.color(last)?, ang))
    }
    /// A size in hundredths of a point, in points on the slide.
    fn pt(&self, sz: i64) -> u16 {
        ((sz * self.font_pct + 5_000_000) / 10_000_000).clamp(1, 1000) as u16
    }
    /// EMU to slide units, rounded.
    fn unit(&self, emu: i64) -> i32 {
        let e = self.emu.max(1);
        ((emu + if emu >= 0 { e / 2 } else { -e / 2 }) / e) as i32
    }
}

/// A placeholder inherited from a layout or master.
#[derive(Clone, Default)]
struct PhInfo {
    ty: String,
    idx: String,
    geo: Option<(i32, i32, i32, i32)>,
    size: Option<u16>,
    anchor: Option<Anchor>,
    align: Option<Align>,
}

fn ph_of(sp: &El) -> Option<(String, String)> {
    let nv = sp.kid("nvSpPr").or_else(|| sp.kid("nvPicPr"))?;
    let ph = nv.path("nvPr/ph")?;
    let ty = ph.attr("type");
    Some((if ty.is_empty() { String::from("body") } else { ty.to_string() }, ph.attr("idx").to_string()))
}

fn geo_of(ctx: &Ctx, sp: &El) -> Option<(i32, i32, i32, i32)> {
    let x = sp.path("spPr/xfrm").or_else(|| sp.path("grpSpPr/xfrm"))?;
    let off = x.kid("off")?;
    let ext = x.kid("ext")?;
    Some((ctx.unit(off.num("x")?), ctx.unit(off.num("y")?), ctx.unit(ext.num("cx")?), ctx.unit(ext.num("cy")?)))
}

fn anchor_of(sp: &El) -> Option<Anchor> {
    match sp.path("txBody/bodyPr")?.attr("anchor") {
        "t" => Some(Anchor::Top),
        "ctr" => Some(Anchor::Middle),
        "b" => Some(Anchor::Bottom),
        _ => None,
    }
}

fn algn(v: &str) -> Option<Align> {
    match v {
        "l" | "just" | "dist" => Some(Align::Left),
        "ctr" => Some(Align::Center),
        "r" => Some(Align::Right),
        _ => None,
    }
}

fn placeholders(ctx: &Ctx, root: &El) -> Vec<PhInfo> {
    let mut out = Vec::new();
    let Some(tree) = root.kids.first().and_then(|r| r.path("cSld/spTree")) else { return out };
    for sp in tree.kids("sp") {
        let Some((ty, idx)) = ph_of(sp) else { continue };
        let lvl1 = sp.path("txBody/lstStyle/lvl1pPr");
        out.push(PhInfo {
            ty,
            idx,
            geo: geo_of(ctx, sp),
            size: lvl1.and_then(|l| l.kid("defRPr")).and_then(|r| r.num("sz")).map(|v| ctx.pt(v)),
            anchor: anchor_of(sp),
            align: lvl1.and_then(|l| algn(l.attr("algn"))),
        });
    }
    out
}

fn ph_family(t: &str) -> &str {
    match t {
        "ctrTitle" | "title" => "title",
        "subTitle" => "subTitle",
        "body" | "obj" => "body",
        o => o,
    }
}

/// The inherited placeholders for (type, idx), layout first: the same idx,
/// else the same type (a subtitle falls back to a body).
fn find_ph<'a>(lists: &'a [Vec<PhInfo>], ty: &str, idx: &str) -> Vec<&'a PhInfo> {
    let fam = ph_family(ty);
    let mut found = Vec::new();
    for l in lists {
        let by_idx = if idx.is_empty() || fam == "title" { None } else { l.iter().find(|p| p.idx == idx && ph_family(&p.ty) != "title") };
        let hit = by_idx.or_else(|| l.iter().find(|p| ph_family(&p.ty) == fam)).or_else(|| if fam == "subTitle" { l.iter().find(|p| ph_family(&p.ty) == "body") } else { None });
        if let Some(h) = hit {
            found.push(h);
        }
    }
    found
}

struct Styles {
    title_align: Option<Align>,
    title: u16,
    body: [u16; 5],
    other: u16,
    title_color: Option<u32>,
    text_color: Option<u32>,
}

fn master_styles(ctx: &Ctx, master: &El) -> Styles {
    let mut st = Styles { title_align: None, title: 44, body: [28, 24, 20, 20, 20], other: 18, title_color: None, text_color: None };
    let Some(tx) = master.kids.first().and_then(|m| m.kid("txStyles")) else { return st };
    let sz = |e: Option<&El>| e.and_then(|l| l.kid("defRPr")).and_then(|r| r.num("sz")).map(|v| ctx.pt(v));
    if let Some(v) = sz(tx.path("titleStyle/lvl1pPr")) {
        st.title = v;
    }
    st.title_align = tx.path("titleStyle/lvl1pPr").and_then(|l| algn(l.attr("algn")));
    st.title_color = tx.path("titleStyle/lvl1pPr/defRPr").and_then(|r| ctx.fill(r)).flatten();
    if let Some(b) = tx.kid("bodyStyle") {
        for i in 0..5 {
            if let Some(v) = sz(b.kid(&format!("lvl{}pPr", i + 1))) {
                st.body[i] = v;
            }
        }
        st.text_color = b.path("lvl1pPr/defRPr").and_then(|r| ctx.fill(r)).flatten();
    }
    if let Some(v) = sz(tx.path("otherStyle/lvl1pPr")) {
        st.other = v;
    }
    st
}

fn bg_of(ctx: &Ctx, root: &El) -> Option<u32> {
    bg_full(ctx, root).0
}

/// A background: its colour and gradient.
fn bg_full(ctx: &Ctx, root: &El) -> (Option<u32>, Option<(u32, i32)>) {
    let Some(bg) = root.kids.first().and_then(|r| r.path("cSld/bg")) else { return (None, None) };
    if let Some(pr) = bg.kid("bgPr") {
        return (ctx.fill(pr).flatten(), ctx.grad(pr));
    }
    (bg.kid("bgRef").and_then(|r| ctx.color(r)), None)
}

struct SlideCtx<'a> {
    ctx: &'a Ctx,
    styles: &'a Styles,
    phs: Vec<Vec<PhInfo>>,
    rels: BTreeMap<String, (String, String)>,
}

/// Group transform: child EMU -> slide EMU.
#[derive(Clone, Copy)]
struct Xf {
    ox: i64,
    oy: i64,
    cx: i64,
    cy: i64,
    sx: (i64, i64),
    sy: (i64, i64),
}

impl Xf {
    fn id() -> Xf {
        Xf { ox: 0, oy: 0, cx: 0, cy: 0, sx: (1, 1), sy: (1, 1) }
    }
    fn map(&self, x: i64, y: i64, w: i64, h: i64) -> (i64, i64, i64, i64) {
        (self.ox + (x - self.cx) * self.sx.0 / self.sx.1.max(1), self.oy + (y - self.cy) * self.sy.0 / self.sy.1.max(1), w * self.sx.0 / self.sx.1.max(1), h * self.sy.0 / self.sy.1.max(1))
    }
}

fn read_text(sc: &SlideCtx, sp: &El, kind: Kind, default_size: u16, sh: &mut Shape) {
    let Some(body) = sp.kid("txBody") else { return };
    let ctx = sc.ctx;
    let mut paras = Vec::new();
    let mut sizes: BTreeMap<u16, usize> = BTreeMap::new();
    let mut color = None;
    let mut end_color = None;
    let bullets_default = kind == Kind::Body;
    let lst_algn = body.path("lstStyle/lvl1pPr").and_then(|l| algn(l.attr("algn")));
    for p in body.kids("p") {
        let ppr = p.kid("pPr");
        let level = ppr.and_then(|x| x.num("lvl")).unwrap_or(0).clamp(0, 8) as u8;
        let style = match ppr {
            Some(x) if x.kid("buNone").is_some() => Style::Body,
            Some(x) if x.kid("buAutoNum").is_some() => Style::Number,
            Some(x) if x.kid("buChar").is_some() || x.kid("buBlip").is_some() => Style::Bullet,
            _ => {
                if bullets_default {
                    Style::Bullet
                } else {
                    Style::Body
                }
            }
        };
        let mut para = Para::new(style);
        para.level = level;
        para.align = ppr.and_then(|x| algn(x.attr("algn"))).or(lst_algn).unwrap_or(sh.text.paras.first().map(|p| p.align).unwrap_or(Align::Left));
        let mut push_run = |para: &mut Para, r: &El, text: &str| {
            let mut f = 0u8;
            if let Some(rp) = r.kid("rPr") {
                let on = |v: &str| matches!(v, "1" | "true");
                if on(rp.attr("b")) && kind != Kind::Title {
                    f |= BOLD;
                }
                if on(rp.attr("i")) {
                    f |= ITALIC;
                }
                if !matches!(rp.attr("u"), "" | "none") {
                    f |= UNDERLINE;
                }
                if !matches!(rp.attr("strike"), "" | "noStrike") {
                    f |= STRIKE;
                }
                if let Some(sz) = rp.num("sz") {
                    *sizes.entry(ctx.pt(sz).max(1)).or_insert(0) += text.chars().count();
                }
                if color.is_none() {
                    color = ctx.fill(rp).flatten();
                }
            }
            para.push(text, f);
        };
        for r in &p.kids {
            match r.local() {
                "r" | "fld" => {
                    let t = r.kid("t").map(|t| t.text.clone()).unwrap_or_default();
                    push_run(&mut para, r, &t);
                }
                "br" => push_run(&mut para, r, " "),
                "endParaRPr" if end_color.is_none() => end_color = ctx.fill(r).flatten(),
                _ => {}
            }
        }
        paras.push(para);
    }
    if paras.is_empty() {
        return;
    }
    if color.is_none() && paras.iter().all(|p| p.text.is_empty()) {
        color = end_color;
    }
    // drop trailing empty paragraphs beyond the first
    while paras.len() > 1 && paras.last().map_or(false, |p| p.text.is_empty()) {
        paras.pop();
    }
    // the size most of the text has, relative to its level
    let size = sizes.iter().max_by_key(|(_, n)| **n).map(|(s, _)| *s).unwrap_or(default_size);
    let min_level = paras.iter().filter(|p| !p.text.is_empty()).map(|p| p.level).min().unwrap_or(0);
    sh.size = (size as i32 + 4 * min_level as i32).clamp(6, 200) as u16;
    if let Some(sc) = body.path("bodyPr/normAutofit").and_then(|n| n.num("fontScale")) {
        if sizes.is_empty() {
            sh.size = (sh.size as i64 * sc / 100_000).max(6) as u16;
        }
    }
    sh.text = Doc { paras, author: String::new() };
    if color.is_some() {
        sh.color = color;
    }
}

/// What a slide's shape tree gives.
#[derive(Default)]
struct Found {
    shapes: Vec<Shape>,
    /// the ids of each shape and of the groups around it (for animations)
    ids: Vec<Vec<u32>>,
    footer: Option<String>,
    number: bool,
}

fn cnv_id(e: &El) -> Option<u32> {
    for nv in ["nvSpPr", "nvPicPr", "nvCxnSpPr", "nvGraphicFramePr", "nvGrpSpPr"] {
        if let Some(c) = e.path(&format!("{}/cNvPr", nv)) {
            return c.attr("id").parse().ok();
        }
    }
    None
}

/// Rotation and flips of a shape's xfrm.
fn xf_attrs(e: &El) -> (i32, bool, bool) {
    let x = e.path("spPr/xfrm").or_else(|| e.kid("xfrm"));
    match x {
        Some(x) => ((x.num("rot").unwrap_or(0) / 60000) as i32, matches!(x.attr("flipH"), "1" | "true"), matches!(x.attr("flipV"), "1" | "true")),
        None => (0, false, false),
    }
}

fn descendants<'a>(e: &'a El, local: &str, out: &mut Vec<&'a El>) {
    for k in &e.kids {
        if k.local() == local {
            out.push(k);
        }
        descendants(k, local, out);
    }
}

fn all_text(e: &El) -> String {
    let mut v = Vec::new();
    descendants(e, "t", &mut v);
    v.iter().map(|t| t.text.as_str()).collect::<Vec<_>>().join("")
}

/// Points (idx, text) of a chart data reference.
fn points(e: &El) -> Vec<String> {
    let mut pts = Vec::new();
    descendants(e, "pt", &mut pts);
    let mut count = Vec::new();
    descendants(e, "ptCount", &mut count);
    let n = count.first().and_then(|c| c.num("val")).map(|v| v as usize).unwrap_or(0).max(pts.iter().filter_map(|p| p.num("idx")).map(|i| i as usize + 1).max().unwrap_or(0)).min(10_000);
    let mut out = vec![String::new(); n];
    for p in pts {
        if let (Some(i), Some(v)) = (p.num("idx"), p.kid("v")) {
            if (i as usize) < n {
                out[i as usize] = v.text.trim().to_string();
            }
        }
    }
    out
}

fn read_chart(z: &[u8], part: &str) -> Option<Chart> {
    let cs = read_xml(z, part)?;
    let chart = cs.kids.first()?.kid("chart")?;
    let plot = chart.kid("plotArea")?;
    let (el, kind) = plot.kids.iter().find_map(|k| {
        let kind = match k.local() {
            "barChart" | "bar3DChart" => {
                if k.kid("barDir").map_or(false, |d| d.attr("val") == "bar") {
                    ChartKind::Bar
                } else {
                    ChartKind::Column
                }
            }
            "lineChart" | "line3DChart" | "scatterChart" | "radarChart" => ChartKind::Line,
            "areaChart" | "area3DChart" => ChartKind::Area,
            "pieChart" | "pie3DChart" | "doughnutChart" | "ofPieChart" => ChartKind::Pie,
            _ => return None,
        };
        Some((k, kind))
    })?;
    let mut cats: Vec<String> = Vec::new();
    let mut series = Vec::new();
    for (k, ser) in el.kids("ser").enumerate() {
        let name = ser.kid("tx").map(|t| {
            let mut v = Vec::new();
            descendants(t, "v", &mut v);
            v.first().map(|v| v.text.trim().to_string()).unwrap_or_default()
        });
        let name = name.filter(|n| !n.is_empty()).unwrap_or_else(|| format!("Series {}", k + 1));
        if cats.is_empty() {
            if let Some(c) = ser.kid("cat").or_else(|| ser.kid("xVal")) {
                cats = points(c);
            }
        }
        let vals: Vec<f64> = ser.kid("val").or_else(|| ser.kid("yVal")).map(|v| points(v).iter().map(|s| s.parse::<f64>().unwrap_or(0.0)).collect()).unwrap_or_default();
        series.push(Series { name, vals });
    }
    let n = series.iter().map(|s| s.vals.len()).max().unwrap_or(0).max(cats.len());
    cats.resize(n, String::new());
    for (i, c) in cats.iter_mut().enumerate() {
        if c.is_empty() {
            *c = format!("{}", i + 1);
        }
    }
    for s in series.iter_mut() {
        s.vals.resize(n, 0.0);
    }
    let title = chart.kid("title").map(all_text).unwrap_or_default();
    let title = if title.is_empty() && chart.kid("title").is_some() && series.len() == 1 && chart.kid("autoTitleDeleted").map_or(true, |a| a.attr("val") != "1") { series[0].name.clone() } else { title };
    Some(Chart { kind, title, cats, series, legend: chart.kid("legend").is_some() })
}

fn read_table(sc: &SlideCtx, tbl: &El) -> Option<(Table, u16)> {
    let ctx = sc.ctx;
    let cols: Vec<i32> = tbl.path("tblGrid").map(|g| g.kids("gridCol").map(|c| ctx.unit(c.num("w").unwrap_or(0)).max(8)).collect()).unwrap_or_default();
    if cols.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    let mut cells = Vec::new();
    let mut sizes: BTreeMap<u16, usize> = BTreeMap::new();
    for tr in tbl.kids("tr") {
        rows.push(ctx.unit(tr.num("h").unwrap_or(0)).max(12));
        let mut n = 0;
        for tc in tr.kids("tc") {
            if n == cols.len() {
                break;
            }
            let mut tmp = Shape::new(Kind::Text, 0, 0, 100, 100);
            tmp.size = 0;
            read_text(sc, tc, Kind::Text, sc.styles.other, &mut tmp);
            if tmp.size > 0 && !tmp.is_empty() {
                *sizes.entry(tmp.size).or_insert(0) += 1;
            }
            cells.push(tmp.text);
            n += 1;
        }
        while n < cols.len() {
            cells.push(Doc::new());
            n += 1;
        }
    }
    if rows.is_empty() {
        return None;
    }
    let pr = tbl.kid("tblPr");
    let flag = |a: &str| pr.map_or(false, |p| matches!(p.attr(a), "1" | "true"));
    let (mut header, mut banded) = (flag("firstRow"), flag("bandRow"));
    let has_flags = pr.map_or(false, |p| !p.attr("firstRow").is_empty() || !p.attr("bandRow").is_empty());
    if !has_flags {
        // no style flags (as LibreOffice writes tables): read them from the cells' fills
        let fills: Vec<Option<u32>> = tbl.kids("tr").map(|tr| tr.kid("tc").and_then(|tc| tc.kid("tcPr")).and_then(|p| ctx.fill(p)).flatten()).collect();
        let n = fills.len();
        header = n > 1 && fills[0].is_some() && fills[0] != fills[1];
        let data = if header { &fills[1..] } else { &fills[..] };
        banded = data.len() > 2 && data.windows(2).all(|w| w[0] != w[1]) && data.windows(3).all(|w| w[0] == w[2]);
    }
    let size = sizes.iter().max_by_key(|(_, n)| **n).map(|(s, _)| *s).unwrap_or(18);
    Some((Table { cols, rows, cells, header, banded }, size))
}

fn read_shapes(sc: &SlideCtx, tree: &El, xf: Xf, groups: &[u32], z: &[u8], deck: &mut Deck, out: &mut Found, pic_cache: &mut BTreeMap<String, Option<usize>>) {
    let ctx = sc.ctx;
    let mapped = |x: i32, y: i32, w: i32, h: i32| {
        let (x, y, w, h) = xf.map(x as i64 * ctx.emu, y as i64 * ctx.emu, w as i64 * ctx.emu, h as i64 * ctx.emu);
        (ctx.unit(x), ctx.unit(y), ctx.unit(w), ctx.unit(h))
    };
    for e in &tree.kids {
        let mut ids: Vec<u32> = cnv_id(e).into_iter().collect();
        ids.extend_from_slice(groups);
        let before = out.shapes.len();
        match e.local() {
            "sp" | "cxnSp" => {
                let ph = ph_of(e);
                if let Some((ty, _)) = &ph {
                    match ty.as_str() {
                        "ftr" => {
                            let t = e.kid("txBody").map(all_text).unwrap_or_default();
                            if !t.trim().is_empty() && out.footer.is_none() {
                                out.footer = Some(t.trim().to_string());
                            }
                            continue;
                        }
                        "sldNum" => {
                            out.number = true;
                            continue;
                        }
                        "dt" | "hdr" | "sldImg" => continue,
                        _ => {}
                    }
                }
                let inherited = ph.as_ref().map(|(t, i)| find_ph(&sc.phs, t, i)).unwrap_or_default();
                let geo = geo_of(ctx, e).map(|(x, y, w, h)| mapped(x, y, w, h));
                let Some((x, y, w, h)) = geo.or_else(|| inherited.iter().find_map(|p| p.geo)) else { continue };
                let (rot, fh, fv) = xf_attrs(e);
                let prst = e.path("spPr/prstGeom").map(|g| g.attr("prst").to_string()).unwrap_or_default();
                let lw = e.path("spPr/ln").and_then(|l| l.num("w")).map(|w| ((w + ctx.emu / 2) / ctx.emu.max(1)) as i32);
                let is_line = e.local() == "cxnSp" || prst == "line" || prst.starts_with("straightConnector") || prst.starts_with("bentConnector") || prst.starts_with("curvedConnector");
                if is_line {
                    let mut sh = Shape::new(Kind::Line, x, y, w.max(0), h.max(0));
                    sh.flip_h = fh;
                    sh.flip_v = fv;
                    sh.line = e.path("spPr/ln").and_then(|l| ctx.fill(l)).flatten().or_else(|| e.path("style/lnRef").and_then(|f| ctx.color(f)));
                    sh.line_w = lw.unwrap_or(1).clamp(1, 60);
                    let end = |k: &str| e.path(&format!("spPr/ln/{}", k)).map_or(false, |x| !matches!(x.attr("type"), "" | "none"));
                    sh.head = end("headEnd");
                    sh.tail = end("tailEnd");
                    // a line is drawn in its box, so turn rotation into its ends
                    if rot != 0 {
                        let ((x0, y0), (x1, y1)) = line_ends(&sh);
                        let (cx, cy) = (x + w / 2, y + h / 2);
                        set_line_ends(&mut sh, rotate(x0, y0, cx, cy, rot), rotate(x1, y1, cx, cy, rot));
                    }
                    out.shapes.push(sh);
                } else {
                    let tx_box = e.path("nvSpPr/cNvSpPr").map_or(false, |c| c.attr("txBox") == "1");
                    let spfill = e.kid("spPr").and_then(|s| ctx.fill(s));
                    let grad = e.kid("spPr").and_then(|s| ctx.grad(s));
                    let style_fill = e.path("style/fillRef").filter(|f| f.attr("idx") != "0").and_then(|f| ctx.color(f));
                    let fill = match spfill {
                        Some(f) => f,
                        None if !tx_box && ph.is_none() => style_fill,
                        None => None,
                    };
                    let line = e.path("spPr/ln").and_then(|l| ctx.fill(l)).flatten();
                    let geom = Geom::from_id(&prst);
                    let drawn = fill.is_some() || line.is_some();
                    let kind = match ph.as_ref().map(|p| ph_family(&p.0).to_string()) {
                        Some(t) if t == "title" => Kind::Title,
                        Some(t) if t == "subTitle" => Kind::Subtitle,
                        Some(_) => Kind::Body,
                        None if prst == "ellipse" && drawn => Kind::Ellipse,
                        None if drawn && !tx_box && (fill.is_some() || geom.map_or(false, |g| g != Geom::Rect)) => Kind::Rect,
                        None => Kind::Text,
                    };
                    let default_size = inherited.iter().find_map(|p| p.size).unwrap_or(match kind {
                        Kind::Title => sc.styles.title,
                        Kind::Body => sc.styles.body[0],
                        Kind::Subtitle => sc.styles.body[0].min(24),
                        _ => sc.styles.other,
                    });
                    let mut sh = Shape::new(kind, x, y, w.max(1), h.max(1));
                    sh.fill = fill;
                    sh.grad = grad;
                    sh.line = line;
                    if let Some(lw) = lw {
                        sh.line_w = lw.clamp(1, 60);
                    }
                    sh.rot = rot.rem_euclid(360);
                    sh.flip_h = fh;
                    sh.flip_v = fv;
                    if kind == Kind::Rect {
                        sh.geom = geom.unwrap_or(Geom::Rect);
                    }
                    sh.size = default_size;
                    sh.anchor = anchor_of(e).or_else(|| inherited.iter().find_map(|p| p.anchor)).unwrap_or(match kind {
                        Kind::Title | Kind::Rect | Kind::Ellipse => Anchor::Middle,
                        _ => Anchor::Top,
                    });
                    if let Some(a) = inherited.iter().find_map(|p| p.align).or(if matches!(kind, Kind::Rect | Kind::Ellipse) { Some(Align::Center) } else if kind == Kind::Title { sc.styles.title_align } else { None }) {
                        sh.text.paras[0].align = a;
                    }
                    if kind == Kind::Title {
                        sh.color = sc.styles.title_color;
                    } else if kind != Kind::Rect && kind != Kind::Ellipse {
                        sh.color = sc.styles.text_color;
                    }
                    read_text(sc, e, kind, default_size, &mut sh);
                    if let Some(c) = e.path("style/fontRef").and_then(|f| ctx.color(f)) {
                        if sh.color.is_none() {
                            sh.color = Some(c);
                        }
                    }
                    if matches!(kind, Kind::Rect | Kind::Ellipse | Kind::Text) && sh.is_empty() && fill.is_none() && line.is_none() {
                        continue;
                    }
                    out.shapes.push(sh);
                }
            }
            "pic" => {
                let Some(embed) = e.path("blipFill/blip").map(|b| b.attr("r:embed").to_string()) else { continue };
                let Some((x, y, w, h)) = geo_of(ctx, e).map(|(x, y, w, h)| mapped(x, y, w, h)) else { continue };
                let Some((_, part)) = sc.rels.get(&embed) else { continue };
                let idx = *pic_cache.entry(part.clone()).or_insert_with(|| {
                    let data = crate::zip::read(z, part)?;
                    let bytes = picture_bytes(&data)?;
                    deck.pics.push(Pic { data: alloc::rc::Rc::new(bytes) });
                    Some(deck.pics.len() - 1)
                });
                let Some(idx) = idx else { continue };
                let mut sh = Shape::new(Kind::Picture, x, y, w.max(1), h.max(1));
                sh.pic = Some(idx);
                sh.rot = xf_attrs(e).0.rem_euclid(360);
                sh.line = e.path("spPr/ln").and_then(|l| ctx.fill(l)).flatten();
                out.shapes.push(sh);
            }
            "graphicFrame" => {
                let Some(x) = e.kid("xfrm") else { continue };
                let (Some(off), Some(ext)) = (x.kid("off"), x.kid("ext")) else { continue };
                let (gx, gy, gw, gh) = mapped(ctx.unit(off.num("x").unwrap_or(0)), ctx.unit(off.num("y").unwrap_or(0)), ctx.unit(ext.num("cx").unwrap_or(0)), ctx.unit(ext.num("cy").unwrap_or(0)));
                let Some(gd) = e.path("graphic/graphicData") else { continue };
                let uri = gd.attr("uri");
                if uri.ends_with("/table") {
                    if let Some((t, size)) = gd.kid("tbl").and_then(|t| read_table(sc, t)) {
                        let mut sh = Shape::new(Kind::Table, gx, gy, t.cols.iter().sum::<i32>().max(1), t.rows.iter().sum::<i32>().max(1));
                        sh.size = size;
                        sh.table = Some(t);
                        let _ = (gw, gh);
                        sh.fit_table();
                        out.shapes.push(sh);
                    }
                } else if uri.ends_with("/chart") {
                    let rid = gd.kid("chart").map(|c| c.attr("r:id").to_string()).unwrap_or_default();
                    if let Some(chart) = sc.rels.get(&rid).and_then(|(_, part)| read_chart(z, part)) {
                        let mut sh = Shape::new(Kind::Chart, gx, gy, gw.max(20), gh.max(20));
                        sh.chart = Some(chart);
                        out.shapes.push(sh);
                    }
                }
            }
            "grpSp" => {
                let Some(gx) = e.path("grpSpPr/xfrm") else { continue };
                let n = |k: &str, a: &str| gx.kid(k).and_then(|v| v.num(a)).unwrap_or(0);
                let (ox, oy, cx, cy) = xf.map(n("off", "x"), n("off", "y"), n("ext", "cx"), n("ext", "cy"));
                let (chx, chy, chw, chh) = (n("chOff", "x"), n("chOff", "y"), n("chExt", "cx"), n("chExt", "cy"));
                let inner = Xf { ox, oy, cx: chx, cy: chy, sx: (cx, if chw == 0 { cx.max(1) } else { chw }), sy: (cy, if chh == 0 { cy.max(1) } else { chh }) };
                read_shapes(sc, e, inner, &ids, z, deck, out, pic_cache);
                continue;
            }
            "AlternateContent" => {
                // take the fallback (plain DrawingML) branch
                if let Some(fb) = e.kid("Fallback") {
                    read_shapes(sc, fb, xf, groups, z, deck, out, pic_cache);
                }
                continue;
            }
            _ => {}
        }
        while out.ids.len() < out.shapes.len() {
            out.ids.push(ids.clone());
        }
        let _ = before;
    }
}

/// Entrance animations in a slide's timing: (shape id, animation) in play order.
fn read_timing(sroot: &El) -> Vec<(u32, Anim)> {
    let Some(t) = sroot.kid("timing") else { return vec![] };
    let mut ctns = Vec::new();
    descendants(t, "cTn", &mut ctns);
    let mut out = Vec::new();
    for c in ctns {
        if c.attr("presetClass") != "entr" {
            continue;
        }
        let anim = match c.attr("presetID") {
            "1" => Anim::Appear,
            "2" | "3" | "12" | "47" | "42" => Anim::Fly,
            _ => Anim::Fade,
        };
        let mut tg = Vec::new();
        descendants(c, "spTgt", &mut tg);
        if let Some(id) = tg.first().and_then(|s| s.attr("spid").parse::<u32>().ok()) {
            if !out.iter().any(|(i, _)| *i == id) {
                out.push((id, anim));
            }
        }
    }
    out
}

fn notes_text(z: &[u8], part: &str) -> String {
    let Some(n) = read_xml(z, part) else { return String::new() };
    let Some(tree) = n.kids.first().and_then(|r| r.path("cSld/spTree")) else { return String::new() };
    for sp in tree.kids("sp") {
        if let Some((ty, _)) = ph_of(sp) {
            if ty == "body" {
                let mut lines = Vec::new();
                if let Some(b) = sp.kid("txBody") {
                    for p in b.kids("p") {
                        let mut s = String::new();
                        for r in &p.kids {
                            match r.local() {
                                "r" | "fld" => s.push_str(&r.kid("t").map(|t| t.text.clone()).unwrap_or_default()),
                                "br" => s.push('\n'),
                                _ => {}
                            }
                        }
                        lines.push(s);
                    }
                }
                while lines.last().map_or(false, |l| l.is_empty()) {
                    lines.pop();
                }
                return lines.join("\n");
            }
        }
    }
    String::new()
}

pub fn from_pptx(z: &[u8]) -> Result<Deck, &'static str> {
    let pres = read_xml(z, "ppt/presentation.xml").ok_or("not a PowerPoint presentation")?;
    let root = pres.kids.first().ok_or("not a PowerPoint presentation")?;
    let (cx, cy) = root.kid("sldSz").map(|s| (s.num("cx").unwrap_or(12192000), s.num("cy").unwrap_or(6858000))).unwrap_or((12192000, 6858000));
    let cx = cx.clamp(914400, 51206400);
    let cy = cy.clamp(914400, 51206400);
    let emu = cx / SLIDE_W as i64;
    let prels = read_rels(z, "ppt/presentation.xml");
    let mut deck = Deck { name: String::new(), w: SLIDE_W, h: (cy / emu.max(1)) as i32, theme: theme("dune"), slides: vec![], pics: vec![], footer: String::new(), author: String::new(), numbers: false };
    // the first master's theme colours
    let master_part = root.path("sldMasterIdLst/sldMasterId").and_then(|m| prels.get(m.attr("r:id"))).map(|x| x.1.clone()).unwrap_or_else(|| String::from("ppt/slideMasters/slideMaster1.xml"));
    let mrels = read_rels(z, &master_part);
    let mut scheme = BTreeMap::new();
    if let Some(tp) = mrels.values().find(|(t, _)| t == "theme").map(|x| x.1.clone()) {
        if let Some(th) = read_xml(z, &tp) {
            if let Some(cs) = th.kids.first().and_then(|t| t.path("themeElements/clrScheme")) {
                let tmp = Ctx { scheme: BTreeMap::new(), emu, font_pct: 100_000 };
                for k in &cs.kids {
                    if let Some(c) = tmp.color(k) {
                        scheme.insert(k.local().to_string(), c);
                    }
                }
            }
        }
    }
    let ctx = Ctx { scheme, emu, font_pct: 12192000 * 100_000 / cx };
    let master = read_xml(z, &master_part).unwrap_or_default();
    let styles = master_styles(&ctx, &master);
    let master_phs = placeholders(&ctx, &master);
    let master_bg = bg_of(&ctx, &master);
    let sch = |k: &str, d: u32| ctx.scheme.get(k).copied().unwrap_or(d);
    deck.theme = Theme {
        id: "custom".into(),
        name: "Imported".into(),
        bg: master_bg.unwrap_or(sch("lt1", 0xFFFFFF)),
        title: styles.title_color.unwrap_or(sch("dk1", 0)),
        text: styles.text_color.unwrap_or(sch("dk1", 0)),
        accent: sch("accent1", 0x4472C4),
        deco: vec![],
    };
    let mut pic_cache = BTreeMap::new();
    if let Some(list) = root.kid("sldIdLst") {
        for sid in list.kids("sldId") {
            let Some((_, part)) = prels.get(sid.attr("r:id")) else { continue };
            let Some(sx) = read_xml(z, part) else { continue };
            let Some(sroot) = sx.kids.first() else { continue };
            if sroot.attr("show") == "0" {
                continue;
            }
            let srels = read_rels(z, part);
            let layout_part = srels.values().find(|(t, _)| t == "slideLayout").map(|x| x.1.clone()).unwrap_or_default();
            let layout = read_xml(z, &layout_part).unwrap_or_default();
            let sc = SlideCtx { ctx: &ctx, styles: &styles, phs: vec![placeholders(&ctx, &layout), master_phs.clone()], rels: srels.clone() };
            let mut found = Found::default();
            if let Some(tree) = sroot.path("cSld/spTree") {
                read_shapes(&sc, tree, Xf::id(), &[], z, &mut deck, &mut found, &mut pic_cache);
            }
            if let Some(f) = found.footer.take() {
                if deck.footer.is_empty() {
                    deck.footer = f;
                }
            }
            deck.numbers |= found.number;
            // entrance animations, in the order they play
            for (k, (id, anim)) in read_timing(sroot).into_iter().enumerate() {
                for (i, ids) in found.ids.iter().enumerate() {
                    if ids.contains(&id) {
                        found.shapes[i].anim = anim;
                        found.shapes[i].anim_order = k as u16 + 1;
                    }
                }
            }
            let shapes = found.shapes;
            let (sbg, sgrad) = bg_full(&ctx, &sx);
            let (lbg, lgrad) = bg_full(&ctx, &layout);
            let bg_grad = sgrad.or(if sbg.is_none() { lgrad } else { None });
            let bg = sbg.or(lbg).filter(|&c| c != deck.theme.bg || bg_grad.is_some());
            let notes = srels.values().find(|(t, _)| t == "notesSlide").map(|x| notes_text(z, &x.1)).unwrap_or_default();
            let trans = match sroot.kid("transition").or_else(|| sroot.path("AlternateContent/Fallback/transition")) {
                Some(t) if t.kid("push").is_some() => Trans::Push,
                Some(t) if !t.kids.is_empty() => Trans::Fade,
                _ => Trans::None,
            };
            let titles = shapes.iter().filter(|s| s.kind == Kind::Title).count();
            let bodies = shapes.iter().filter(|s| s.kind == Kind::Body).count();
            let subs = shapes.iter().filter(|s| s.kind == Kind::Subtitle).count();
            let lay = match (titles, bodies, subs) {
                (_, 0, 1..) => Layout::Title,
                (1.., 2.., _) => Layout::TwoContent,
                (1.., 1, _) => Layout::TitleContent,
                (1.., 0, 0) => Layout::TitleOnly,
                _ => Layout::Blank,
            };
            deck.slides.push(Slide { layout: lay, shapes, notes, bg, bg_grad, trans });
        }
    }
    if let Some(core) = read_xml(z, "docProps/core.xml") {
        if let Some(t) = core.kids.first().and_then(|c| c.kid("title")) {
            deck.name = t.text.trim().to_string();
        }
    }
    deck.author = crate::doc::core_creator(z);
    if deck.slides.is_empty() {
        let s = deck.new_slide(Layout::Title);
        deck.slides.push(s);
    }
    Ok(deck)
}
