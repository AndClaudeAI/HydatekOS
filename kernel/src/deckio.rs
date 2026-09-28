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
    out.push_str(&format!("slides {}\n", d.slides.len()));
    for s in &d.slides {
        out.push_str(&format!("slide {}\n", s.layout.id()));
        if let Some(bg) = s.bg {
            out.push_str(&format!("bg {}\n", hex6(bg)));
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
            out.push('\n');
            if !sh.kind.has_text() {
                continue;
            }
            for p in &sh.text.paras {
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
    }
    for p in &d.pics {
        let kind = if p.data.starts_with(b"\x89PNG") { "png" } else { "jpeg" };
        out.push_str(&format!("pic {} {}\n", kind, crate::crypto::base64(&p.data)));
    }
    let crc = crate::zip::crc32(out.as_bytes());
    out.push_str(&format!("end {:08x}\n", crc));
    out
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
    let mut d = Deck { name: String::new(), w: SLIDE_W, h: SLIDE_H, theme: theme("dune"), slides: vec![], pics: vec![] };
    let mut count = None;
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
            "slides" => count = val.parse::<usize>().ok(),
            "slide" => d.slides.push(Slide { layout: Layout::from_id(val.trim()).unwrap_or(Layout::Blank), shapes: vec![], notes: String::new(), bg: None, trans: Trans::None }),
            "bg" => {
                let sl = d.slides.last_mut().ok_or("the file is damaged")?;
                sl.bg = parse_hex(val.trim());
            }
            "trans" => {
                let sl = d.slides.last_mut().ok_or("the file is damaged")?;
                sl.trans = Trans::from_id(val.trim());
            }
            "notes" => {
                let sl = d.slides.last_mut().ok_or("the file is damaged")?;
                sl.notes = unesc_line(val);
            }
            "shape" => {
                let sl = d.slides.last_mut().ok_or("the file is damaged")?;
                let mut it = val.split(' ');
                let kind = Kind::from_id(it.next().unwrap_or("")).unwrap_or(Kind::Text);
                let mut n = [0i32; 4];
                for v in n.iter_mut() {
                    *v = it.next().and_then(|x| x.parse().ok()).ok_or("the file is damaged")?;
                }
                let mut sh = Shape::new(kind, n[0], n[1], n[2].max(1), n[3].max(1));
                sh.text = Doc { paras: vec![] };
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
                        _ => {}
                    }
                }
                sl.shapes.push(sh);
            }
            "p" => {
                let sh = d.slides.last_mut().and_then(|s| s.shapes.last_mut()).ok_or("the file is damaged")?;
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
                sh.text.paras.push(p);
            }
            "t" => {
                let p = d.slides.last_mut().and_then(|s| s.shapes.last_mut()).and_then(|s| s.text.paras.last_mut()).ok_or("the file is damaged")?;
                let t = unesc_line(val);
                p.push(&t, 0);
            }
            "f" => {
                let p = d.slides.last_mut().and_then(|s| s.shapes.last_mut()).and_then(|s| s.text.paras.last_mut()).ok_or("the file is damaged")?;
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
                let data = crate::crypto::base64_decode(b64.trim()).ok_or("the file is damaged")?;
                d.pics.push(Pic { data: alloc::rc::Rc::new(data) });
            }
            _ => {}
        }
    }
    if count.map_or(false, |c| c != d.slides.len()) {
        return Err("the file is damaged");
    }
    for s in d.slides.iter_mut() {
        for sh in s.shapes.iter_mut() {
            if sh.text.paras.is_empty() {
                sh.text = Doc::new();
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

fn text_xml(sh: &Shape, t: &Theme) -> String {
    let tb = layout(sh);
    let anchor = match sh.anchor {
        Anchor::Top => "t",
        Anchor::Middle => "ctr",
        Anchor::Bottom => "b",
    };
    let fit = if tb.scale < 100 { format!("<a:normAutofit fontScale=\"{}\"/>", tb.scale * 1000) } else if sh.kind.placeholder() { String::from("<a:normAutofit/>") } else { String::new() };
    let mut s = format!("<p:txBody><a:bodyPr wrap=\"square\" lIns=\"{0}\" tIns=\"{1}\" rIns=\"{0}\" bIns=\"{1}\" anchor=\"{2}\" rtlCol=\"0\">{3}</a:bodyPr><a:lstStyle/>", INSET_X as i64 * EMU, INSET_Y as i64 * EMU, anchor, fit);
    let color = text_color(sh, t);
    let mut numbered = false;
    for p in &sh.text.paras {
        let pt = (sh.size as i32 - 4 * p.level as i32).max(sh.size as i32 * 3 / 5).max(8);
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
        numbered |= p.style == Style::Number;
        let spc = if sh.kind == Kind::Body { format!("<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>", pt * 30) } else { String::from("<a:spcBef><a:spcPts val=\"0\"/></a:spcBef>") };
        s.push_str(&format!("<a:p><a:pPr marL=\"{}\" lvl=\"{}\" indent=\"{}\" algn=\"{}\"><a:lnSpc><a:spcPct val=\"100000\"/></a:lnSpc>{}{}</a:pPr>", mar, p.level.min(8), ind, algn, spc, bu));
        let rpr = |f: u8| {
            let bold = f & BOLD != 0 || sh.kind == Kind::Title;
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
    let _ = numbered;
    s.push_str("</p:txBody>");
    s
}

fn slide_xml(d: &Deck, s: &Slide, pic_rel: &BTreeMap<usize, String>) -> String {
    let mut x = format!("{}<p:sld {}><p:cSld>", XML, NS);
    if let Some(bg) = s.bg {
        x.push_str(&bg_xml(bg));
    }
    x.push_str("<p:spTree>");
    x.push_str(GRP);
    let mut id = 2;
    let mut body_idx = 1;
    for sh in &s.shapes {
        if sh.kind.placeholder() && sh.is_empty() {
            continue;
        }
        let name = match sh.kind {
            Kind::Title => "Title",
            Kind::Subtitle => "Subtitle",
            Kind::Body => "Content",
            Kind::Text => "TextBox",
            Kind::Rect => "Rectangle",
            Kind::Ellipse => "Oval",
            Kind::Picture => "Picture",
        };
        if sh.kind == Kind::Picture {
            if let Some(rid) = sh.pic.and_then(|p| pic_rel.get(&p)) {
                x.push_str(&format!("<p:pic><p:nvPicPr><p:cNvPr id=\"{0}\" name=\"{1} {0}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{2}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{3}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>", id, name, rid, xfrm(sh.x, sh.y, sh.w, sh.h)));
                id += 1;
            }
            continue;
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
        let geom = if sh.kind == Kind::Ellipse { "ellipse" } else { "rect" };
        let fill = match (sh.kind, sh.fill) {
            (_, Some(c)) => solid(c),
            (Kind::Rect | Kind::Ellipse, None) => solid(d.theme.accent),
            _ => String::from("<a:noFill/>"),
        };
        let line = match sh.line {
            Some(c) => format!("<a:ln w=\"19050\">{}</a:ln>", solid(c)),
            None => String::from("<a:ln><a:noFill/></a:ln>"),
        };
        x.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{0}\" name=\"{1} {0}\"/>{2}<p:nvPr>{3}</p:nvPr></p:nvSpPr><p:spPr>{4}<a:prstGeom prst=\"{5}\"><a:avLst/></a:prstGeom>{6}{7}</p:spPr>{8}</p:sp>", id, name, locks, ph, xfrm(sh.x, sh.y, sh.w, sh.h), geom, fill, line, text_xml(sh, &d.theme)));
        id += 1;
    }
    x.push_str("</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>");
    match s.trans {
        Trans::Fade => x.push_str("<p:transition spd=\"med\"><p:fade/></p:transition>"),
        Trans::Push => x.push_str("<p:transition spd=\"med\"><p:push dir=\"u\"/></p:transition>"),
        Trans::None => {}
    }
    x.push_str("</p:sld>");
    x
}

pub fn to_pptx(d: &Deck) -> Vec<u8> {
    let mut z = crate::zip::Writer::new();
    let has_notes = d.slides.iter().any(|s| !s.notes.is_empty());
    // content types
    let mut ct = format!("{}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>", XML);
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
    ct.push_str("</Types>");
    z.add("[Content_Types].xml", ct.as_bytes());
    z.add("_rels/.rels", rels(&[("rId1".into(), "officeDocument", "ppt/presentation.xml".into()), ("rId2".into(), "extended-properties", "docProps/app.xml".into()), ("rId3".into(), "metadata/core-properties", "docProps/core.xml".into())]).replace(&format!("{}/metadata/core-properties", REL), "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties").as_bytes());
    z.add("docProps/app.xml", format!("{}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Hyda Slides</Application><Slides>{}</Slides></Properties>", XML, d.slides.len()).as_bytes());
    z.add("docProps/core.xml", format!("{}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title></cp:coreProperties>", XML, esc(&d.name)).as_bytes());
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
    for (i, s) in d.slides.iter().enumerate() {
        let mut r: Vec<(String, &str, String)> = vec![("rId1".into(), "slideLayout", "../slideLayouts/slideLayout1.xml".into())];
        if !s.notes.is_empty() {
            r.push(("rId2".into(), "notesSlide", format!("../notesSlides/notesSlide{}.xml", i + 1)));
        }
        let mut pic_rel = BTreeMap::new();
        for sh in &s.shapes {
            if let Some(p) = sh.pic.filter(|&p| p < d.pics.len() && sh.kind == Kind::Picture) {
                if !pic_rel.contains_key(&p) {
                    let rid = format!("rId{}", 3 + pic_rel.len());
                    let ext = if d.pics[p].data.starts_with(b"\x89PNG") { "png" } else { "jpeg" };
                    r.push((rid.clone(), "image", format!("../media/image{}.{}", p + 1, ext)));
                    pic_rel.insert(p, rid);
                }
            }
        }
        z.add(&format!("ppt/slides/slide{}.xml", i + 1), slide_xml(d, s, &pic_rel).as_bytes());
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
    /// A size in hundredths of a point, in points on the slide.
    fn pt(&self, sz: i64) -> u16 {
        ((sz * self.font_pct + 5_000_000) / 10_000_000).clamp(1, 1000) as u16
    }
    fn unit(&self, emu: i64) -> i32 {
        (emu / self.emu.max(1)) as i32
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
    let bg = root.kids.first()?.path("cSld/bg")?;
    if let Some(pr) = bg.kid("bgPr") {
        return ctx.fill(pr).flatten();
    }
    if let Some(r) = bg.kid("bgRef") {
        return ctx.color(r);
    }
    None
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
    sh.text = Doc { paras };
    if color.is_some() {
        sh.color = color;
    }
}

fn read_shapes(sc: &SlideCtx, tree: &El, xf: Xf, z: &[u8], deck: &mut Deck, out: &mut Vec<Shape>, pic_cache: &mut BTreeMap<String, Option<usize>>) {
    let ctx = sc.ctx;
    for e in &tree.kids {
        match e.local() {
            "sp" => {
                let ph = ph_of(e);
                if let Some((ty, _)) = &ph {
                    if matches!(ty.as_str(), "dt" | "ftr" | "sldNum" | "hdr" | "sldImg") {
                        continue;
                    }
                }
                let inherited = ph.as_ref().map(|(t, i)| find_ph(&sc.phs, t, i)).unwrap_or_default();
                let geo = geo_of(ctx, e).map(|(x, y, w, h)| {
                    let (x, y, w, h) = xf.map(x as i64 * ctx.emu, y as i64 * ctx.emu, w as i64 * ctx.emu, h as i64 * ctx.emu);
                    (ctx.unit(x), ctx.unit(y), ctx.unit(w), ctx.unit(h))
                });
                let Some((x, y, w, h)) = geo.or_else(|| inherited.iter().find_map(|p| p.geo)) else { continue };
                let prst = e.path("spPr/prstGeom").map(|g| g.attr("prst").to_string()).unwrap_or_default();
                let tx_box = e.path("nvSpPr/cNvSpPr").map_or(false, |c| c.attr("txBox") == "1");
                let spfill = e.kid("spPr").and_then(|s| ctx.fill(s));
                let style_fill = e.path("style/fillRef").filter(|f| f.attr("idx") != "0").and_then(|f| ctx.color(f));
                let fill = match spfill {
                    Some(f) => f,
                    None if !tx_box && ph.is_none() => style_fill,
                    None => None,
                };
                let line = e.path("spPr/ln").and_then(|l| ctx.fill(l)).flatten();
                let kind = match ph.as_ref().map(|p| ph_family(&p.0).to_string()) {
                    Some(t) if t == "title" => Kind::Title,
                    Some(t) if t == "subTitle" => Kind::Subtitle,
                    Some(_) => Kind::Body,
                    None if prst == "ellipse" && fill.is_some() => Kind::Ellipse,
                    None if fill.is_some() && !tx_box => Kind::Rect,
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
                sh.line = line;
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
                out.push(sh);
            }
            "pic" => {
                let Some(embed) = e.path("blipFill/blip").map(|b| b.attr("r:embed").to_string()) else { continue };
                let Some((x, y, w, h)) = geo_of(ctx, e) else { continue };
                let (x, y, w, h) = xf.map(x as i64 * ctx.emu, y as i64 * ctx.emu, w as i64 * ctx.emu, h as i64 * ctx.emu);
                let Some((_, part)) = sc.rels.get(&embed) else { continue };
                let idx = *pic_cache.entry(part.clone()).or_insert_with(|| {
                    let data = crate::zip::read(z, part)?;
                    let bytes = picture_bytes(&data)?;
                    deck.pics.push(Pic { data: alloc::rc::Rc::new(bytes) });
                    Some(deck.pics.len() - 1)
                });
                let Some(idx) = idx else { continue };
                let mut sh = Shape::new(Kind::Picture, ctx.unit(x), ctx.unit(y), ctx.unit(w).max(1), ctx.unit(h).max(1));
                sh.pic = Some(idx);
                out.push(sh);
            }
            "grpSp" => {
                let Some(gx) = e.path("grpSpPr/xfrm") else { continue };
                let n = |k: &str, a: &str| gx.kid(k).and_then(|v| v.num(a)).unwrap_or(0);
                let (ox, oy, cx, cy) = xf.map(n("off", "x"), n("off", "y"), n("ext", "cx"), n("ext", "cy"));
                let (chx, chy, chw, chh) = (n("chOff", "x"), n("chOff", "y"), n("chExt", "cx"), n("chExt", "cy"));
                let inner = Xf { ox, oy, cx: chx, cy: chy, sx: (cx, if chw == 0 { cx.max(1) } else { chw }), sy: (cy, if chh == 0 { cy.max(1) } else { chh }) };
                read_shapes(sc, e, inner, z, deck, out, pic_cache);
            }
            "AlternateContent" => {
                // take the fallback (plain DrawingML) branch
                if let Some(fb) = e.kid("Fallback") {
                    read_shapes(sc, fb, xf, z, deck, out, pic_cache);
                }
            }
            _ => {}
        }
    }
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
    let mut deck = Deck { name: String::new(), w: SLIDE_W, h: (cy / emu.max(1)) as i32, theme: theme("dune"), slides: vec![], pics: vec![] };
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
            let mut shapes = Vec::new();
            if let Some(tree) = sroot.path("cSld/spTree") {
                read_shapes(&sc, tree, Xf::id(), z, &mut deck, &mut shapes, &mut pic_cache);
            }
            let bg = bg_of(&ctx, &sx).or_else(|| bg_of(&ctx, &layout)).filter(|&c| c != deck.theme.bg);
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
            deck.slides.push(Slide { layout: lay, shapes, notes, bg, trans });
        }
    }
    if let Some(core) = read_xml(z, "docProps/core.xml") {
        if let Some(t) = core.kids.first().and_then(|c| c.kid("title")) {
            deck.name = t.text.trim().to_string();
        }
    }
    if deck.slides.is_empty() {
        let s = deck.new_slide(Layout::Title);
        deck.slides.push(s);
    }
    Ok(deck)
}
