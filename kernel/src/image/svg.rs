//! SVG: vector pictures drawn by HydatekOS. Shapes and paths (lines, curves,
//! arcs), fills (non-zero and even-odd), strokes (joins, caps, dashes),
//! solid colours and linear / radial gradients, transforms, groups with
//! opacity, <use> and <symbol>, <style> sheets with simple selectors, and
//! nested <svg>. Not drawn: text, filters, masks, clip paths and patterns.

use super::{color, Image, Result};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const BAD: &str = "the SVG picture can't be read";

// ---------------------------------------------------------------- maths

const PI: f64 = 3.141592653589793;

fn floor(x: f64) -> f64 {
    let t = x as i64 as f64;
    if t > x {
        t - 1.0
    } else {
        t
    }
}

fn sqrt(x: f64) -> f64 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let mut g = f64::from_bits((x.to_bits() >> 1) + (1023u64 << 51));
    for _ in 0..6 {
        g = 0.5 * (g + x / g);
    }
    g
}

fn sin(x: f64) -> f64 {
    // reduce to [-pi, pi], then to [-pi/2, pi/2]
    let mut x = x - 2.0 * PI * floor(x / (2.0 * PI) + 0.5);
    if x > PI / 2.0 {
        x = PI - x;
    } else if x < -PI / 2.0 {
        x = -PI - x;
    }
    let x2 = x * x;
    // Taylor series to x^13
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0 * (1.0 - x2 / 156.0))))))
}

fn cos(x: f64) -> f64 {
    sin(x + PI / 2.0)
}

fn atan(z: f64) -> f64 {
    if z < 0.0 {
        return -atan(-z);
    }
    if z > 1.0 {
        return PI / 2.0 - atan(1.0 / z);
    }
    // halve the angle twice, then a short series
    let z = z / (1.0 + sqrt(1.0 + z * z));
    let z = z / (1.0 + sqrt(1.0 + z * z));
    let z2 = z * z;
    4.0 * z * (1.0 - z2 / 3.0 + z2 * z2 / 5.0 - z2 * z2 * z2 / 7.0 + z2 * z2 * z2 * z2 / 9.0)
}

fn atan2(y: f64, x: f64) -> f64 {
    if x > 0.0 {
        atan(y / x)
    } else if x < 0.0 {
        if y >= 0.0 {
            atan(y / x) + PI
        } else {
            atan(y / x) - PI
        }
    } else if y > 0.0 {
        PI / 2.0
    } else if y < 0.0 {
        -PI / 2.0
    } else {
        0.0
    }
}

fn abs(x: f64) -> f64 {
    if x < 0.0 {
        -x
    } else {
        x
    }
}

/// An affine transform: x' = a x + c y + e, y' = b x + d y + f.
#[derive(Clone, Copy, Debug)]
struct M {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl M {
    const ID: M = M { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> M {
        M { a, b, c, d, e, f }
    }

    fn translate(x: f64, y: f64) -> M {
        M::new(1.0, 0.0, 0.0, 1.0, x, y)
    }

    fn scale(x: f64, y: f64) -> M {
        M::new(x, 0.0, 0.0, y, 0.0, 0.0)
    }

    /// self · o: apply `o`, then `self`.
    fn mul(&self, o: &M) -> M {
        M {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }

    fn apply(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    fn invert(&self) -> M {
        let det = self.a * self.d - self.b * self.c;
        if abs(det) < 1e-12 {
            return M::ID;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        M { a, b, c, d, e: -(a * self.e + c * self.f), f: -(b * self.e + d * self.f) }
    }

    /// How much lengths grow (for curve precision and hairlines).
    fn scale_factor(&self) -> f64 {
        sqrt(abs(self.a * self.d - self.b * self.c))
    }
}

// ---------------------------------------------------------------- XML

struct Node {
    tag: String,
    attrs: Vec<(String, String)>,
    kids: Vec<usize>,
    text: String,
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e < 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let c = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse_xml(s: &str) -> Vec<Node> {
    let mut nodes = vec![Node { tag: String::new(), attrs: Vec::new(), kids: Vec::new(), text: String::new() }];
    let mut stack = vec![0usize];
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let end = s[i..].find('<').map_or(s.len(), |k| i + k);
            let top = *stack.last().unwrap_or(&0);
            nodes[top].text.push_str(&unescape(&s[i..end]));
            i = end;
            continue;
        }
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |k| k + 3);
        } else if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").unwrap_or(rest.len());
            let top = *stack.last().unwrap_or(&0);
            nodes[top].text.push_str(&rest[9..end.max(9)]);
            i += (end + 3).min(rest.len());
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            // doctype (maybe with an internal subset) or processing instruction
            let mut depth = 0;
            let mut k = 0;
            for (j, c) in rest.bytes().enumerate() {
                match c {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    b'>' if depth <= 0 => {
                        k = j + 1;
                        break;
                    }
                    _ => {}
                }
            }
            i += if k == 0 { rest.len() } else { k };
        } else if rest.starts_with("</") {
            let end = rest.find('>').map_or(rest.len(), |k| k + 1);
            if stack.len() > 1 {
                stack.pop();
            }
            i += end;
        } else {
            // a start tag
            let mut j = 1;
            let rb = rest.as_bytes();
            while j < rb.len() && !rb[j].is_ascii_whitespace() && rb[j] != b'>' && rb[j] != b'/' {
                j += 1;
            }
            let name = &rest[1..j];
            let tag = name.rsplit(':').next().unwrap_or(name).to_string();
            let mut attrs = Vec::new();
            let mut closed = false;
            loop {
                while j < rb.len() && rb[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j >= rb.len() {
                    break;
                }
                if rb[j] == b'>' {
                    j += 1;
                    break;
                }
                if rb[j] == b'/' {
                    closed = true;
                    j += 1;
                    continue;
                }
                let ks = j;
                while j < rb.len() && !rb[j].is_ascii_whitespace() && rb[j] != b'=' && rb[j] != b'>' && rb[j] != b'/' {
                    j += 1;
                }
                let key = rest[ks..j].to_string();
                while j < rb.len() && rb[j].is_ascii_whitespace() {
                    j += 1;
                }
                let mut val = String::new();
                if j < rb.len() && rb[j] == b'=' {
                    j += 1;
                    while j < rb.len() && rb[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < rb.len() && (rb[j] == b'"' || rb[j] == b'\'') {
                        let q = rb[j];
                        let vs = j + 1;
                        j = vs;
                        while j < rb.len() && rb[j] != q {
                            j += 1;
                        }
                        val = unescape(&rest[vs..j.min(rest.len())]);
                        j += 1;
                    } else {
                        let vs = j;
                        while j < rb.len() && !rb[j].is_ascii_whitespace() && rb[j] != b'>' {
                            j += 1;
                        }
                        val = unescape(&rest[vs..j]);
                    }
                }
                if key.is_empty() {
                    j += 1;
                } else {
                    attrs.push((key, val));
                }
            }
            let id = nodes.len();
            nodes.push(Node { tag, attrs, kids: Vec::new(), text: String::new() });
            let top = *stack.last().unwrap_or(&0);
            nodes[top].kids.push(id);
            if !closed {
                stack.push(id);
            }
            i += j.min(rest.len()).max(1);
        }
        if nodes.len() > 200_000 {
            break;
        }
    }
    nodes
}

// ---------------------------------------------------------------- CSS

struct Rule {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    spec: u32,
    order: usize,
    decls: Vec<(String, String)>,
}

fn decls(s: &str) -> Vec<(String, String)> {
    s.split(';')
        .filter_map(|d| {
            let (k, v) = d.split_once(':')?;
            let v = v.trim().trim_end_matches("!important").trim();
            Some((k.trim().to_ascii_lowercase(), v.to_string()))
        })
        .collect()
}

fn parse_css(css: &str, rules: &mut Vec<Rule>) {
    // drop comments
    let mut s = String::new();
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        s.push_str(&rest[..i]);
        rest = rest[i..].find("*/").map_or("", |e| &rest[i + e + 2..]);
    }
    s.push_str(rest);
    let mut rest = s.as_str();
    while let Some(open) = rest.find('{') {
        let head = rest[..open].trim();
        let Some(close) = rest[open..].find('}') else { break };
        let body = &rest[open + 1..open + close];
        rest = &rest[open + close + 1..];
        if head.starts_with('@') {
            // at-rules: skip a nested block if one opened
            if body.contains('{') {
                if let Some(e) = rest.find('}') {
                    rest = &rest[e + 1..];
                }
            }
            continue;
        }
        let d = decls(body);
        for sel in head.split(',') {
            // the rightmost compound selector only
            let last = sel.split_whitespace().last().unwrap_or("").rsplit('>').next().unwrap_or("");
            if last.contains(':') || last.contains('[') {
                continue;
            }
            let mut r = Rule { tag: None, id: None, classes: Vec::new(), spec: 0, order: rules.len(), decls: d.clone() };
            let mut cur = String::new();
            let mut kind = 't';
            for c in last.chars().chain(core::iter::once('.')) {
                if c == '.' || c == '#' {
                    match kind {
                        't' if !cur.is_empty() && cur != "*" => {
                            r.tag = Some(cur.clone());
                            r.spec += 1;
                        }
                        '.' if !cur.is_empty() => {
                            r.classes.push(cur.clone());
                            r.spec += 100;
                        }
                        '#' if !cur.is_empty() => {
                            r.id = Some(cur.clone());
                            r.spec += 10000;
                        }
                        _ => {}
                    }
                    cur.clear();
                    kind = c;
                } else {
                    cur.push(c);
                }
            }
            rules.push(r);
        }
    }
}

// ---------------------------------------------------------------- style

#[derive(Clone, Debug, PartialEq)]
enum Paint {
    None,
    Color(u32, u8),
    Url(String, Option<(u32, u8)>),
    Current,
}

#[derive(Clone, Debug)]
struct Style {
    fill: Paint,
    stroke: Paint,
    width: f64,
    fill_op: f64,
    stroke_op: f64,
    opacity: f64,
    evenodd: bool,
    cap: u8, // 0 butt, 1 round, 2 square
    join: u8, // 0 miter, 1 round, 2 bevel
    miter: f64,
    dash: Vec<f64>,
    dash_off: f64,
    display: bool,
    visible: bool,
    color: (u32, u8),
}

impl Style {
    fn root() -> Style {
        Style {
            fill: Paint::Color(0, 255),
            stroke: Paint::None,
            width: 1.0,
            fill_op: 1.0,
            stroke_op: 1.0,
            opacity: 1.0,
            evenodd: false,
            cap: 0,
            join: 0,
            miter: 4.0,
            dash: Vec::new(),
            dash_off: 0.0,
            display: true,
            visible: true,
            color: (0, 255),
        }
    }
}

fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

/// A length in px ("%" of `pct`).
fn length(s: &str, pct: f64) -> Option<f64> {
    let s = s.trim();
    let units: [(&str, f64); 9] = [("px", 1.0), ("pt", 4.0 / 3.0), ("pc", 16.0), ("mm", 3.779528), ("cm", 37.79528), ("in", 96.0), ("em", 16.0), ("ex", 8.0), ("%", pct / 100.0)];
    for (u, f) in units {
        if let Some(n) = s.strip_suffix(u) {
            return number(n).map(|v| v * f);
        }
    }
    number(s)
}

fn paint(v: &str) -> Option<Paint> {
    let v = v.trim();
    if v == "none" {
        return Some(Paint::None);
    }
    if v.eq_ignore_ascii_case("currentcolor") {
        return Some(Paint::Current);
    }
    if let Some(r) = v.strip_prefix("url(") {
        let (inside, fallback) = r.split_once(')')?;
        let id = inside.trim().trim_matches(|c| c == '\'' || c == '"').trim_start_matches('#').to_string();
        return Some(Paint::Url(id, color::parse(fallback)));
    }
    color::parse(v).map(|(c, a)| Paint::Color(c, a))
}

fn set(s: &mut Style, k: &str, v: &str) {
    let v = v.trim();
    if v == "inherit" {
        return;
    }
    match k {
        "fill" => {
            if let Some(p) = paint(v) {
                s.fill = p;
            }
        }
        "stroke" => {
            if let Some(p) = paint(v) {
                s.stroke = p;
            }
        }
        "stroke-width" => {
            if let Some(w) = length(v, 100.0) {
                s.width = w.max(0.0);
            }
        }
        "fill-opacity" => s.fill_op = color_frac(v).unwrap_or(s.fill_op),
        "stroke-opacity" => s.stroke_op = color_frac(v).unwrap_or(s.stroke_op),
        "opacity" => s.opacity = color_frac(v).unwrap_or(1.0),
        "fill-rule" => s.evenodd = v == "evenodd",
        "stroke-linecap" => s.cap = match v {
            "round" => 1,
            "square" => 2,
            _ => 0,
        },
        "stroke-linejoin" => s.join = match v {
            "round" => 1,
            "bevel" => 2,
            _ => 0,
        },
        "stroke-miterlimit" => s.miter = number(v).unwrap_or(4.0).max(1.0),
        "stroke-dasharray" => {
            s.dash = if v == "none" { Vec::new() } else { v.split([',', ' ']).filter(|x| !x.is_empty()).filter_map(|x| length(x, 100.0)).collect() };
            if s.dash.len() % 2 == 1 {
                let d = s.dash.clone();
                s.dash.extend(d);
            }
            if s.dash.iter().all(|&x| x <= 0.0) {
                s.dash.clear();
            }
        }
        "stroke-dashoffset" => s.dash_off = length(v, 100.0).unwrap_or(0.0),
        "display" => s.display = v != "none",
        "visibility" => s.visible = v == "visible",
        "color" => {
            if let Some(c) = color::parse(v) {
                s.color = c;
            }
        }
        _ => {}
    }
}

fn color_frac(v: &str) -> Option<f64> {
    let v = v.trim();
    let f = if let Some(p) = v.strip_suffix('%') { number(p)? / 100.0 } else { number(v)? };
    Some(f.clamp(0.0, 1.0))
}

const PROPS: [&str; 17] = [
    "fill", "stroke", "stroke-width", "fill-opacity", "stroke-opacity", "opacity", "fill-rule", "stroke-linecap", "stroke-linejoin", "stroke-miterlimit", "stroke-dasharray",
    "stroke-dashoffset", "display", "visibility", "color", "clip-rule", "stop-color",
];

// ---------------------------------------------------------------- paths

#[derive(Clone, Copy)]
enum Seg {
    Move((f64, f64)),
    Line((f64, f64)),
    Cubic((f64, f64), (f64, f64), (f64, f64)),
    Close,
}

fn nums(s: &str) -> Vec<f64> {
    // numbers like "1.5.5-2e3" : split at signs and second dots
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_digit() || c == b'-' || c == b'+' || c == b'.' {
            let st = i;
            i += 1;
            let mut dot = c == b'.';
            let mut exp = false;
            while i < b.len() {
                let d = b[i];
                if d.is_ascii_digit() {
                    i += 1;
                } else if d == b'.' && !dot && !exp {
                    dot = true;
                    i += 1;
                } else if (d == b'e' || d == b'E') && !exp {
                    exp = true;
                    i += 1;
                    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = s[st..i].parse::<f64>() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Arc segments as lines (SVG spec, appendix F.6).
#[allow(clippy::too_many_arguments)]
fn arc(out: &mut Vec<Seg>, p0: (f64, f64), mut rx: f64, mut ry: f64, phi_deg: f64, large: bool, sweep: bool, p1: (f64, f64), scale: f64) {
    if (p0.0 - p1.0) * (p0.0 - p1.0) + (p0.1 - p1.1) * (p0.1 - p1.1) < 1e-12 {
        return;
    }
    rx = abs(rx);
    ry = abs(ry);
    if rx < 1e-9 || ry < 1e-9 {
        out.push(Seg::Line(p1));
        return;
    }
    let phi = phi_deg * PI / 180.0;
    let (cp, sp) = (cos(phi), sin(phi));
    let (dx, dy) = ((p0.0 - p1.0) / 2.0, (p0.1 - p1.1) / 2.0);
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lam = x1 * x1 / (rx * rx) + y1 * y1 / (ry * ry);
    if lam > 1.0 {
        let s = sqrt(lam);
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut coef = if den > 0.0 { sqrt((num / den).max(0.0)) } else { 0.0 };
    if large == sweep {
        coef = -coef;
    }
    let cx1 = coef * rx * y1 / ry;
    let cy1 = -coef * ry * x1 / rx;
    let cx = cp * cx1 - sp * cy1 + (p0.0 + p1.0) / 2.0;
    let cy = sp * cx1 + cp * cy1 + (p0.1 + p1.1) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| atan2(ux * vy - uy * vx, ux * vx + uy * vy);
    let (ux, uy) = ((x1 - cx1) / rx, (y1 - cy1) / ry);
    let (vx, vy) = ((-x1 - cx1) / rx, (-y1 - cy1) / ry);
    let t1 = ang(1.0, 0.0, ux, uy);
    let mut dt = ang(ux, uy, vx, vy);
    if !sweep && dt > 0.0 {
        dt -= 2.0 * PI;
    } else if sweep && dt < 0.0 {
        dt += 2.0 * PI;
    }
    let n = ((abs(dt) * (rx.max(ry) * scale) / 2.0) as usize).clamp(4, 180);
    for k in 1..=n {
        let t = t1 + dt * k as f64 / n as f64;
        let (ct, st) = (cos(t), sin(t));
        out.push(Seg::Line(if k == n { p1 } else { (cx + rx * ct * cp - ry * st * sp, cy + rx * ct * sp + ry * st * cp) }));
    }
}

fn parse_path(d: &str, scale: f64) -> Vec<Seg> {
    let mut out = Vec::new();
    let (mut cur, mut start) = ((0.0, 0.0), (0.0, 0.0));
    let mut last_c: Option<(f64, f64)> = None; // for S
    let mut last_q: Option<(f64, f64)> = None; // for T
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !c.is_ascii_alphabetic() || c == b'e' || c == b'E' {
            i += 1;
            continue;
        }
        let end = (i + 1..b.len()).find(|&k| b[k].is_ascii_alphabetic() && b[k] != b'e' && b[k] != b'E').unwrap_or(b.len());
        let args = nums(&d[i + 1..end]);
        let rel = c.is_ascii_lowercase();
        let cmd = c.to_ascii_uppercase();
        let at = |p: (f64, f64), cur: (f64, f64)| if rel { (cur.0 + p.0, cur.1 + p.1) } else { p };
        let per = match cmd {
            b'M' | b'L' | b'T' => 2,
            b'H' | b'V' => 1,
            b'C' => 6,
            b'S' | b'Q' => 4,
            b'A' => 7,
            _ => 0,
        };
        if per == 0 {
            if cmd == b'Z' {
                out.push(Seg::Close);
                cur = start;
            }
            last_c = None;
            last_q = None;
            i = end;
            continue;
        }
        let mut k = 0;
        let mut first = true;
        while k + per <= args.len() {
            let a = &args[k..k + per];
            let (mut nc, mut nq) = (None, None);
            match cmd {
                b'M' => {
                    let p = at((a[0], a[1]), cur);
                    if first {
                        out.push(Seg::Move(p));
                        start = p;
                    } else {
                        out.push(Seg::Line(p));
                    }
                    cur = p;
                }
                b'L' => {
                    cur = at((a[0], a[1]), cur);
                    out.push(Seg::Line(cur));
                }
                b'H' => {
                    cur = (if rel { cur.0 + a[0] } else { a[0] }, cur.1);
                    out.push(Seg::Line(cur));
                }
                b'V' => {
                    cur = (cur.0, if rel { cur.1 + a[0] } else { a[0] });
                    out.push(Seg::Line(cur));
                }
                b'C' | b'S' => {
                    let (c1, c2, p) = if cmd == b'C' {
                        (at((a[0], a[1]), cur), at((a[2], a[3]), cur), at((a[4], a[5]), cur))
                    } else {
                        let c1 = last_c.map_or(cur, |lc| (2.0 * cur.0 - lc.0, 2.0 * cur.1 - lc.1));
                        (c1, at((a[0], a[1]), cur), at((a[2], a[3]), cur))
                    };
                    out.push(Seg::Cubic(c1, c2, p));
                    nc = Some(c2);
                    cur = p;
                }
                b'Q' | b'T' => {
                    let (q, p) = if cmd == b'Q' {
                        (at((a[0], a[1]), cur), at((a[2], a[3]), cur))
                    } else {
                        (last_q.map_or(cur, |lq| (2.0 * cur.0 - lq.0, 2.0 * cur.1 - lq.1)), at((a[0], a[1]), cur))
                    };
                    let c1 = (cur.0 + 2.0 / 3.0 * (q.0 - cur.0), cur.1 + 2.0 / 3.0 * (q.1 - cur.1));
                    let c2 = (p.0 + 2.0 / 3.0 * (q.0 - p.0), p.1 + 2.0 / 3.0 * (q.1 - p.1));
                    out.push(Seg::Cubic(c1, c2, p));
                    nq = Some(q);
                    cur = p;
                }
                _ => {
                    let p = at((a[5], a[6]), cur);
                    arc(&mut out, cur, a[0], a[1], a[2], a[3] != 0.0, a[4] != 0.0, p, scale);
                    cur = p;
                }
            }
            last_c = nc;
            last_q = nq;
            first = false;
            k += per;
        }
        i = end;
    }
    out
}

/// Segments to polylines (in the same space), with curve precision `scale`.
fn flatten(segs: &[Seg], scale: f64) -> Vec<(Vec<(f64, f64)>, bool)> {
    let mut out: Vec<(Vec<(f64, f64)>, bool)> = Vec::new();
    let mut cur: Vec<(f64, f64)> = Vec::new();
    let mut start = (0.0, 0.0);
    for s in segs {
        match *s {
            Seg::Move(p) => {
                if cur.len() > 1 || (cur.len() == 1 && !out.is_empty()) {
                    out.push((core::mem::take(&mut cur), false));
                }
                cur.clear();
                cur.push(p);
                start = p;
            }
            Seg::Line(p) => {
                if cur.is_empty() {
                    cur.push(start);
                }
                cur.push(p);
            }
            Seg::Cubic(c1, c2, p) => {
                if cur.is_empty() {
                    cur.push(start);
                }
                let p0 = *cur.last().unwrap_or(&start);
                let len = sqrt((c1.0 - p0.0) * (c1.0 - p0.0) + (c1.1 - p0.1) * (c1.1 - p0.1))
                    + sqrt((c2.0 - c1.0) * (c2.0 - c1.0) + (c2.1 - c1.1) * (c2.1 - c1.1))
                    + sqrt((p.0 - c2.0) * (p.0 - c2.0) + (p.1 - c2.1) * (p.1 - c2.1));
                let n = ((sqrt(len * scale) * 1.5) as usize).clamp(2, 100);
                for k in 1..=n {
                    let t = k as f64 / n as f64;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    cur.push((a * p0.0 + b * c1.0 + c * c2.0 + d * p.0, a * p0.1 + b * c1.1 + c * c2.1 + d * p.1));
                }
            }
            Seg::Close => {
                if !cur.is_empty() {
                    let p = cur[0];
                    out.push((core::mem::take(&mut cur), true));
                    start = p;
                }
            }
        }
    }
    if !cur.is_empty() {
        out.push((cur, false));
    }
    out
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<Seg> {
    const K: f64 = 0.5522847498;
    let (kx, ky) = (rx * K, ry * K);
    vec![
        Seg::Move((cx + rx, cy)),
        Seg::Cubic((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry)),
        Seg::Cubic((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy)),
        Seg::Cubic((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry)),
        Seg::Cubic((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy)),
        Seg::Close,
    ]
}

// ---------------------------------------------------------------- strokes

fn signed_area(p: &[(f64, f64)]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (x0, y0) = p[i];
        let (x1, y1) = p[(i + 1) % p.len()];
        a += x0 * y1 - x1 * y0;
    }
    a
}

fn push_piece(out: &mut Vec<Vec<(f64, f64)>>, mut p: Vec<(f64, f64)>) {
    // every piece wound the same way, so overlaps add up under non-zero
    if signed_area(&p) < 0.0 {
        p.reverse();
    }
    out.push(p);
}

fn disc(c: (f64, f64), r: f64, n: usize) -> Vec<(f64, f64)> {
    (0..n).map(|k| {
        let t = 2.0 * PI * k as f64 / n as f64;
        (c.0 + r * cos(t), c.1 + r * sin(t))
    }).collect()
}

fn dashes(line: &[(f64, f64)], closed: bool, dash: &[f64], offset: f64) -> Vec<Vec<(f64, f64)>> {
    let mut pts = line.to_vec();
    if closed && !pts.is_empty() {
        pts.push(pts[0]);
    }
    let total: f64 = dash.iter().sum();
    if total <= 0.0 {
        return vec![pts];
    }
    let mut pos = offset - total * floor(offset / total);
    let mut idx = 0;
    while pos >= dash[idx] {
        pos -= dash[idx];
        idx = (idx + 1) % dash.len();
    }
    let mut left = dash[idx] - pos;
    let mut on = idx % 2 == 0;
    let mut out = Vec::new();
    let mut cur: Vec<(f64, f64)> = if on { vec![pts[0]] } else { Vec::new() };
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = sqrt((b.0 - a.0) * (b.0 - a.0) + (b.1 - a.1) * (b.1 - a.1));
        let mut done = 0.0;
        while seg - done > left {
            done += left;
            let t = done / seg;
            let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
            if on {
                cur.push(p);
                out.push(core::mem::take(&mut cur));
            } else {
                cur = vec![p];
            }
            on = !on;
            idx = (idx + 1) % dash.len();
            left = dash[idx];
            if out.len() > 10_000 {
                return out;
            }
        }
        left -= seg - done;
        if on {
            cur.push(b);
        }
    }
    if on && cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// Stroke outlines (as polygons to fill with non-zero) for a polyline.
fn stroke(line: &[(f64, f64)], closed: bool, st: &Style, scale: f64, out: &mut Vec<Vec<(f64, f64)>>) {
    let hw = st.width / 2.0;
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(line.len());
    for &p in line {
        if pts.last().map_or(true, |q: &(f64, f64)| abs(q.0 - p.0) + abs(q.1 - p.1) > 1e-9) {
            pts.push(p);
        }
    }
    if closed && pts.len() > 2 && abs(pts[0].0 - pts[pts.len() - 1].0) + abs(pts[0].1 - pts[pts.len() - 1].1) < 1e-9 {
        pts.pop();
    }
    let round_n = ((hw * scale * 1.5) as usize).clamp(8, 64);
    if pts.len() == 1 {
        // a dot: only round and square caps draw it
        let p = pts[0];
        match st.cap {
            1 => push_piece(out, disc(p, hw, round_n)),
            2 => push_piece(out, vec![(p.0 - hw, p.1 - hw), (p.0 + hw, p.1 - hw), (p.0 + hw, p.1 + hw), (p.0 - hw, p.1 + hw)]),
            _ => {}
        }
        return;
    }
    let n = pts.len();
    let segs = if closed { n } else { n - 1 };
    let dir = |i: usize| {
        let (a, b) = (pts[i % n], pts[(i + 1) % n]);
        let l = sqrt((b.0 - a.0) * (b.0 - a.0) + (b.1 - a.1) * (b.1 - a.1)).max(1e-12);
        ((b.0 - a.0) / l, (b.1 - a.1) / l)
    };
    for i in 0..segs {
        let (dx, dy) = dir(i);
        let (nx, ny) = (-dy * hw, dx * hw);
        let (mut a, mut b) = (pts[i], pts[(i + 1) % n]);
        if !closed && st.cap == 2 {
            if i == 0 {
                a = (a.0 - dx * hw, a.1 - dy * hw);
            }
            if i == segs - 1 {
                b = (b.0 + dx * hw, b.1 + dy * hw);
            }
        }
        push_piece(out, vec![(a.0 + nx, a.1 + ny), (b.0 + nx, b.1 + ny), (b.0 - nx, b.1 - ny), (a.0 - nx, a.1 - ny)]);
    }
    // joins
    let joins: Vec<usize> = if closed { (0..n).collect() } else { (1..n - 1).collect() };
    for i in joins {
        let p = pts[i];
        let (d0, d1) = (dir((i + n - 1) % n), dir(i));
        let cross = d0.0 * d1.1 - d0.1 * d1.0;
        if abs(cross) < 1e-9 && d0.0 * d1.0 + d0.1 * d1.1 > 0.0 {
            continue; // straight on
        }
        if st.join == 1 {
            push_piece(out, disc(p, hw, round_n));
            continue;
        }
        // the outer side of the turn
        let s = if cross > 0.0 { -1.0 } else { 1.0 };
        let o0 = (p.0 - d0.1 * hw * s, p.1 + d0.0 * hw * s);
        let o1 = (p.0 - d1.1 * hw * s, p.1 + d1.0 * hw * s);
        let cosang = d0.0 * d1.0 + d0.1 * d1.1;
        let mut done = false;
        if st.join == 0 {
            // miter length / stroke width = 1 / sin(theta / 2)
            let ratio = 1.0 / sqrt(((1.0 + cosang) / 2.0).max(1e-12)).max(1e-6);
            if ratio <= st.miter {
                // the miter tip: where the two offset edges meet
                let (mx, my) = (o0.0 + o1.0 - p.0, o0.1 + o1.1 - p.1);
                let bis = sqrt((mx - p.0) * (mx - p.0) + (my - p.1) * (my - p.1)).max(1e-12);
                let dist = hw * ratio;
                let tip = (p.0 + (mx - p.0) / bis * dist, p.1 + (my - p.1) / bis * dist);
                push_piece(out, vec![p, o0, tip, o1]);
                done = true;
            }
        }
        if !done {
            push_piece(out, vec![p, o0, o1]);
        }
    }
    if !closed && st.cap == 1 {
        push_piece(out, disc(pts[0], hw, round_n));
        push_piece(out, disc(pts[n - 1], hw, round_n));
    }
}

// ---------------------------------------------------------------- raster

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u32>,
}

/// Fill polygons (device space) with anti-aliasing: 4 sub-scanlines per
/// pixel row, exact coverage across each span.
fn fill(cv: &mut Canvas, polys: &[Vec<(f64, f64)>], evenodd: bool, alpha: f64, shade: &dyn Fn(f64, f64) -> u32) {
    let mut edges: Vec<(f64, f64, f64, f64, i32)> = Vec::new();
    let (mut miny, mut maxy, mut minx, mut maxx) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in polys {
        for i in 0..p.len() {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            if a.1 == b.1 || !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) {
                continue;
            }
            let e = if a.1 < b.1 { (a.0, a.1, b.0, b.1, 1) } else { (b.0, b.1, a.0, a.1, -1) };
            miny = miny.min(e.1);
            maxy = maxy.max(e.3);
            minx = minx.min(a.0.min(b.0));
            maxx = maxx.max(a.0.max(b.0));
            edges.push(e);
        }
    }
    if edges.is_empty() || alpha <= 0.0 {
        return;
    }
    let y0 = (floor(miny).max(0.0)) as usize;
    let y1 = ((maxy + 1.0) as usize).min(cv.h);
    let x0 = (floor(minx).max(0.0)) as usize;
    let x1 = ((maxx + 2.0) as usize).min(cv.w);
    if y0 >= y1 || x0 >= x1 {
        return;
    }
    edges.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(core::cmp::Ordering::Equal));
    let mut cov = vec![0f64; x1 - x0 + 1];
    let mut xs: Vec<(f64, i32)> = Vec::new();
    let mut first = 0usize;
    for py in y0..y1 {
        for c in cov.iter_mut() {
            *c = 0.0;
        }
        while first < edges.len() && edges[first].3 <= py as f64 {
            // (edges sorted by top; skip ones entirely above — cheap filter)
            if edges[first].1 > py as f64 {
                break;
            }
            first += 1;
        }
        let mut touched = false;
        for s in 0..4 {
            let sy = py as f64 + (s as f64 + 0.5) / 4.0;
            xs.clear();
            for e in &edges[first..] {
                if e.1 > sy {
                    break;
                }
                if e.3 <= sy {
                    continue;
                }
                xs.push((e.0 + (sy - e.1) * (e.2 - e.0) / (e.3 - e.1), e.4));
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(core::cmp::Ordering::Equal));
            let mut wind = 0;
            for k in 0..xs.len() - 1 {
                wind += xs[k].1;
                let inside = if evenodd { wind % 2 != 0 } else { wind != 0 };
                if !inside {
                    continue;
                }
                let (a, b) = (xs[k].0.max(x0 as f64), xs[k + 1].0.min(x1 as f64));
                if b <= a {
                    continue;
                }
                touched = true;
                let (ia, ib) = (floor(a) as usize, floor(b) as usize);
                if ia == ib {
                    cov[ia - x0] += (b - a) / 4.0;
                } else {
                    cov[ia - x0] += ((ia + 1) as f64 - a) / 4.0;
                    for c in cov.iter_mut().take(ib - x0).skip(ia + 1 - x0) {
                        *c += 0.25;
                    }
                    if ib < x1 {
                        cov[ib - x0] += (b - ib as f64) / 4.0;
                    }
                }
            }
        }
        if !touched {
            continue;
        }
        for x in x0..x1 {
            let c = cov[x - x0].min(1.0);
            if c <= 0.0 {
                continue;
            }
            let src = shade(x as f64 + 0.5, py as f64 + 0.5);
            let a = ((src >> 24) as f64 * c * alpha + 0.5) as u32;
            if a == 0 {
                continue;
            }
            let d = &mut cv.px[py * cv.w + x];
            *d = super::webp::over((src & 0xff_ffff) | a.min(255) << 24, *d);
        }
    }
}

// ---------------------------------------------------------------- the picture

pub struct Svg {
    nodes: Vec<Node>,
    root: usize,
    rules: Vec<Rule>,
    ids: BTreeMap<String, usize>,
    pub width: f64,
    pub height: f64,
    view: Option<[f64; 4]>,
    aspect: String,
}

fn attr<'a>(n: &'a Node, k: &str) -> Option<&'a str> {
    n.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
}

fn href(n: &Node) -> Option<&str> {
    attr(n, "href").or_else(|| attr(n, "xlink:href")).map(|h| h.trim().trim_start_matches('#'))
}

fn viewbox(n: &Node) -> Option<[f64; 4]> {
    let v = nums(attr(n, "viewBox")?);
    (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| [v[0], v[1], v[2], v[3]])
}

/// The transform that fits `view` into a w × h viewport.
fn fit(view: [f64; 4], w: f64, h: f64, aspect: &str) -> M {
    let (sx, sy) = (w / view[2], h / view[3]);
    let mut parts = aspect.split_whitespace();
    let align = parts.next().unwrap_or("xMidYMid");
    if align == "none" {
        return M::scale(sx, sy).mul(&M::translate(-view[0], -view[1]));
    }
    let slice = parts.next() == Some("slice");
    let s = if slice { sx.max(sy) } else { sx.min(sy) };
    let (ew, eh) = (w - view[2] * s, h - view[3] * s);
    let tx = if align.contains("xMin") { 0.0 } else if align.contains("xMax") { ew } else { ew / 2.0 };
    let ty = if align.contains("YMin") { 0.0 } else if align.contains("YMax") { eh } else { eh / 2.0 };
    M::translate(tx, ty).mul(&M::scale(s, s)).mul(&M::translate(-view[0], -view[1]))
}

fn transform(s: &str) -> M {
    let mut m = M::ID;
    let mut rest = s;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim();
        let Some(close) = rest[open..].find(')') else { break };
        let a = nums(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
        let g = |i: usize, d: f64| a.get(i).copied().unwrap_or(d);
        let t = match name {
            "matrix" if a.len() == 6 => M::new(a[0], a[1], a[2], a[3], a[4], a[5]),
            "translate" => M::translate(g(0, 0.0), g(1, 0.0)),
            "scale" => M::scale(g(0, 1.0), g(1, g(0, 1.0))),
            "rotate" => {
                let r = g(0, 0.0) * PI / 180.0;
                let (c, s) = (cos(r), sin(r));
                let rot = M::new(c, s, -s, c, 0.0, 0.0);
                let (cx, cy) = (g(1, 0.0), g(2, 0.0));
                M::translate(cx, cy).mul(&rot).mul(&M::translate(-cx, -cy))
            }
            "skewX" => {
                let r = g(0, 0.0) * PI / 180.0;
                M::new(1.0, 0.0, sin(r) / cos(r), 1.0, 0.0, 0.0)
            }
            "skewY" => {
                let r = g(0, 0.0) * PI / 180.0;
                M::new(1.0, sin(r) / cos(r), 0.0, 1.0, 0.0, 0.0)
            }
            _ => M::ID,
        };
        m = m.mul(&t);
    }
    m
}

pub fn parse(d: &[u8]) -> Result<Svg> {
    let text = core::str::from_utf8(d).map_err(|_| BAD)?;
    let nodes = parse_xml(text);
    let root = (0..nodes.len()).find(|&i| nodes[i].tag == "svg").ok_or(BAD)?;
    let mut rules = Vec::new();
    let mut ids = BTreeMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if n.tag == "style" {
            parse_css(&n.text, &mut rules);
        }
        if let Some(id) = attr(n, "id") {
            ids.entry(id.to_string()).or_insert(i);
        }
    }
    rules.sort_by_key(|r| (r.spec, r.order));
    let r = &nodes[root];
    let view = viewbox(r);
    let w = attr(r, "width").and_then(|v| if v.ends_with('%') { None } else { length(v, 0.0) }).filter(|&v| v > 0.0);
    let h = attr(r, "height").and_then(|v| if v.ends_with('%') { None } else { length(v, 0.0) }).filter(|&v| v > 0.0);
    let (width, height) = match (w, h, view) {
        (Some(w), Some(h), _) => (w, h),
        (Some(w), None, Some(v)) => (w, w * v[3] / v[2]),
        (None, Some(h), Some(v)) => (h * v[2] / v[3], h),
        (None, None, Some(v)) => (v[2], v[3]),
        (Some(w), None, None) => (w, 150.0),
        (None, Some(h), None) => (300.0, h),
        _ => (300.0, 150.0),
    };
    let aspect = attr(r, "preserveAspectRatio").unwrap_or("xMidYMid meet").to_string();
    Ok(Svg { nodes, root, rules, ids, width: width.min(16384.0), height: height.min(16384.0), view, aspect })
}

/// Parse and draw at the picture's own size.
pub fn decode(d: &[u8]) -> Result<Image> {
    let s = parse(d)?;
    let (w, h) = s.size();
    super::check_size(w, h)?;
    Ok(s.render(w, h))
}

impl Svg {
    pub fn size(&self) -> (u32, u32) {
        ((self.width + 0.5) as u32, (self.height + 0.5) as u32)
    }

    fn style_of(&self, n: usize, parent: &Style) -> Style {
        let node = &self.nodes[n];
        let mut s = parent.clone();
        s.opacity = 1.0;
        s.display = true;
        for p in PROPS {
            if let Some(v) = attr(node, p) {
                set(&mut s, p, v);
            }
        }
        let classes: Vec<&str> = attr(node, "class").map(|c| c.split_whitespace().collect()).unwrap_or_default();
        let id = attr(node, "id");
        for r in &self.rules {
            let ok = r.tag.as_deref().map_or(true, |t| t == node.tag) && r.id.as_deref().map_or(true, |i| Some(i) == id) && r.classes.iter().all(|c| classes.contains(&c.as_str()));
            if ok {
                for (k, v) in &r.decls {
                    set(&mut s, k, v);
                }
            }
        }
        if let Some(st) = attr(node, "style") {
            for (k, v) in decls(st) {
                set(&mut s, &k, &v);
            }
        }
        s
    }

    /// Render at w × h pixels.
    pub fn render(&self, w: u32, h: u32) -> Image {
        let mut cv = Canvas { w: w as usize, h: h as usize, px: vec![0; w as usize * h as usize] };
        let view = self.view.unwrap_or([0.0, 0.0, self.width, self.height]);
        let m = if self.view.is_some() { fit(view, w as f64, h as f64, &self.aspect) } else { M::scale(w as f64 / self.width, h as f64 / self.height) };
        let st = self.style_of(self.root, &Style::root());
        let mut budget = 200_000usize;
        for &k in &self.nodes[self.root].kids {
            self.draw(k, &m, &st, st.opacity, &mut cv, 0, &mut budget);
        }
        Image { w, h, px: cv.px, frames: Vec::new() }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&self, n: usize, ctm: &M, parent: &Style, alpha: f64, cv: &mut Canvas, depth: usize, budget: &mut usize) {
        if depth > 40 || *budget == 0 {
            return;
        }
        *budget -= 1;
        let node = &self.nodes[n];
        let st = self.style_of(n, parent);
        if !st.display {
            return;
        }
        let local = attr(node, "transform").map(transform).unwrap_or(M::ID);
        let m = ctm.mul(&local);
        let alpha = alpha * st.opacity;
        let len = |k: &str, pct: f64| attr(node, k).and_then(|v| length(v, pct)).unwrap_or(0.0);
        let segs = match node.tag.as_str() {
            "g" | "a" | "switch" => {
                let kids = node.kids.clone();
                self.group(cv, alpha, st.opacity, |me, layer, a| {
                    for &k in &kids {
                        me.draw(k, &m, &st, a, layer, depth + 1, budget);
                    }
                });
                return;
            }
            "svg" => {
                // a nested viewport
                let (x, y) = (len("x", 0.0), len("y", 0.0));
                let (vw, vh) = (attr(node, "width").and_then(|v| length(v, self.width)).unwrap_or(self.width), attr(node, "height").and_then(|v| length(v, self.height)).unwrap_or(self.height));
                let inner = match viewbox(node) {
                    Some(v) => M::translate(x, y).mul(&fit(v, vw, vh, attr(node, "preserveAspectRatio").unwrap_or(""))),
                    None => M::translate(x, y),
                };
                let m2 = m.mul(&inner);
                for &k in &node.kids {
                    self.draw(k, &m2, &st, alpha, cv, depth + 1, budget);
                }
                return;
            }
            "use" => {
                let Some(&target) = href(node).and_then(|id| self.ids.get(id)) else { return };
                let (x, y) = (len("x", 0.0), len("y", 0.0));
                let mut m2 = m.mul(&M::translate(x, y));
                let t = &self.nodes[target];
                if t.tag == "symbol" || t.tag == "svg" {
                    if let Some(v) = viewbox(t) {
                        let vw = attr(node, "width").and_then(|v| length(v, 0.0)).unwrap_or(v[2]);
                        let vh = attr(node, "height").and_then(|v| length(v, 0.0)).unwrap_or(v[3]);
                        m2 = m2.mul(&fit(v, vw, vh, attr(t, "preserveAspectRatio").unwrap_or("")));
                    }
                    let st2 = self.style_of(target, &st);
                    for &k in &t.kids {
                        self.draw(k, &m2, &st2, alpha * st2.opacity, cv, depth + 1, budget);
                    }
                } else {
                    self.draw(target, &m2, &st, alpha, cv, depth + 1, budget);
                }
                return;
            }
            "path" => parse_path(attr(node, "d").unwrap_or(""), m.scale_factor()),
            "rect" => {
                let (x, y, w, h) = (len("x", self.width), len("y", self.height), len("width", self.width), len("height", self.height));
                if w <= 0.0 || h <= 0.0 {
                    return;
                }
                let mut rx = attr(node, "rx").and_then(|v| length(v, self.width));
                let mut ry = attr(node, "ry").and_then(|v| length(v, self.height));
                if rx.is_none() {
                    rx = ry;
                }
                if ry.is_none() {
                    ry = rx;
                }
                let (rx, ry) = (rx.unwrap_or(0.0).clamp(0.0, w / 2.0), ry.unwrap_or(0.0).clamp(0.0, h / 2.0));
                if rx > 0.0 && ry > 0.0 {
                    let mut s = vec![Seg::Move((x + rx, y)), Seg::Line((x + w - rx, y))];
                    arc(&mut s, (x + w - rx, y), rx, ry, 0.0, false, true, (x + w, y + ry), m.scale_factor());
                    s.push(Seg::Line((x + w, y + h - ry)));
                    arc(&mut s, (x + w, y + h - ry), rx, ry, 0.0, false, true, (x + w - rx, y + h), m.scale_factor());
                    s.push(Seg::Line((x + rx, y + h)));
                    arc(&mut s, (x + rx, y + h), rx, ry, 0.0, false, true, (x, y + h - ry), m.scale_factor());
                    s.push(Seg::Line((x, y + ry)));
                    arc(&mut s, (x, y + ry), rx, ry, 0.0, false, true, (x + rx, y), m.scale_factor());
                    s.push(Seg::Close);
                    s
                } else {
                    vec![Seg::Move((x, y)), Seg::Line((x + w, y)), Seg::Line((x + w, y + h)), Seg::Line((x, y + h)), Seg::Close]
                }
            }
            "circle" => {
                let r = len("r", self.width);
                if r <= 0.0 {
                    return;
                }
                ellipse(len("cx", self.width), len("cy", self.height), r, r)
            }
            "ellipse" => {
                let (rx, ry) = (len("rx", self.width), len("ry", self.height));
                if rx <= 0.0 || ry <= 0.0 {
                    return;
                }
                ellipse(len("cx", self.width), len("cy", self.height), rx, ry)
            }
            "line" => vec![Seg::Move((len("x1", self.width), len("y1", self.height))), Seg::Line((len("x2", self.width), len("y2", self.height)))],
            "polyline" | "polygon" => {
                let p = nums(attr(node, "points").unwrap_or(""));
                let mut s = Vec::new();
                for (k, c) in p.chunks(2).filter(|c| c.len() == 2).enumerate() {
                    s.push(if k == 0 { Seg::Move((c[0], c[1])) } else { Seg::Line((c[0], c[1])) });
                }
                if node.tag == "polygon" && !s.is_empty() {
                    s.push(Seg::Close);
                }
                s
            }
            _ => return, // defs, symbol, gradients, style, text, ...
        };
        if !st.visible || segs.is_empty() {
            return;
        }
        let scale = m.scale_factor();
        let lines = flatten(&segs, scale.max(0.01));
        // bounding box in user space (for objectBoundingBox gradients)
        let mut bb = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
        for (l, _) in &lines {
            for p in l {
                bb = [bb[0].min(p.0), bb[1].min(p.1), bb[2].max(p.0), bb[3].max(p.1)];
            }
        }
        let fillable = node.tag != "line";
        if fillable && st.fill != Paint::None {
            let polys: Vec<Vec<(f64, f64)>> = lines.iter().filter(|(l, _)| l.len() > 2).map(|(l, _)| l.iter().map(|&p| m.apply(p)).collect()).collect();
            if let Some(shade) = self.shader(&st.fill, &st, bb, &m) {
                fill(cv, &polys, st.evenodd, alpha * st.fill_op, &*shade);
            }
        }
        if st.stroke != Paint::None && st.width > 0.0 {
            let mut pieces = Vec::new();
            for (l, closed) in &lines {
                if st.dash.is_empty() {
                    stroke(l, *closed, &st, scale, &mut pieces);
                } else {
                    for d in dashes(l, *closed, &st.dash, st.dash_off) {
                        stroke(&d, false, &st, scale, &mut pieces);
                    }
                }
            }
            let polys: Vec<Vec<(f64, f64)>> = pieces.iter().map(|p| p.iter().map(|&q| m.apply(q)).collect()).collect();
            if let Some(shade) = self.shader(&st.stroke, &st, bb, &m) {
                fill(cv, &polys, false, alpha * st.stroke_op, &*shade);
            }
        }
    }

    /// Children of a group with opacity are drawn on their own layer, then
    /// faded as one, so where they overlap doesn't show through.
    fn group(&self, cv: &mut Canvas, alpha: f64, opacity: f64, mut body: impl FnMut(&Self, &mut Canvas, f64)) {
        if opacity >= 1.0 {
            body(self, cv, alpha);
            return;
        }
        let mut layer = Canvas { w: cv.w, h: cv.h, px: vec![0; cv.px.len()] };
        body(self, &mut layer, 1.0);
        for (d, &s) in cv.px.iter_mut().zip(layer.px.iter()) {
            let a = ((s >> 24) as f64 * alpha) as u32;
            if a > 0 {
                *d = super::webp::over((s & 0xff_ffff) | a.min(255) << 24, *d);
            }
        }
    }

    /// A function giving the paint's colour at a device pixel.
    fn shader(&self, p: &Paint, st: &Style, bb: [f64; 4], m: &M) -> Option<alloc::boxed::Box<dyn Fn(f64, f64) -> u32>> {
        let solid = |(c, a): (u32, u8)| -> alloc::boxed::Box<dyn Fn(f64, f64) -> u32> {
            let v = (a as u32) << 24 | c;
            alloc::boxed::Box::new(move |_, _| v)
        };
        match p {
            Paint::None => None,
            Paint::Color(c, a) => Some(solid((*c, *a))),
            Paint::Current => Some(solid(st.color)),
            Paint::Url(id, fallback) => match self.ids.get(id).and_then(|&g| self.gradient(g, bb, m)) {
                Some(g) => Some(g),
                None => fallback.map(solid),
            },
        }
    }

    /// Follow a gradient's href chain for an attribute.
    fn grad_attr(&self, mut g: usize, k: &str) -> Option<String> {
        for _ in 0..10 {
            let n = &self.nodes[g];
            if let Some(v) = attr(n, k) {
                return Some(v.to_string());
            }
            g = *href(n).and_then(|h| self.ids.get(h))?;
        }
        None
    }

    fn gradient(&self, g: usize, bb: [f64; 4], m: &M) -> Option<alloc::boxed::Box<dyn Fn(f64, f64) -> u32>> {
        let tag = self.nodes[g].tag.as_str();
        if tag != "linearGradient" && tag != "radialGradient" {
            return None;
        }
        // stops: from the first gradient in the chain that has any
        let mut src = g;
        for _ in 0..10 {
            if self.nodes[src].kids.iter().any(|&k| self.nodes[k].tag == "stop") {
                break;
            }
            match href(&self.nodes[src]).and_then(|h| self.ids.get(h)) {
                Some(&n) => src = n,
                None => break,
            }
        }
        let mut stops: Vec<(f64, u32)> = Vec::new();
        for &k in &self.nodes[src].kids {
            let s = &self.nodes[k];
            if s.tag != "stop" {
                continue;
            }
            let mut off = attr(s, "offset").and_then(color_frac).unwrap_or(0.0);
            if let Some(&(last, _)) = stops.last() {
                off = off.max(last);
            }
            let mut c = attr(s, "stop-color").and_then(color::parse).unwrap_or((0, 255));
            let mut op = attr(s, "stop-opacity").and_then(color_frac).unwrap_or(1.0);
            if let Some(style) = attr(s, "style") {
                for (k, v) in decls(style) {
                    match k.as_str() {
                        "stop-color" => c = color::parse(&v).unwrap_or(c),
                        "stop-opacity" => op = color_frac(&v).unwrap_or(op),
                        _ => {}
                    }
                }
            }
            for r in &self.rules {
                let classes: Vec<&str> = attr(s, "class").map(|c| c.split_whitespace().collect()).unwrap_or_default();
                if r.tag.as_deref().map_or(true, |t| t == "stop") && r.id.is_none() && !r.classes.is_empty() && r.classes.iter().all(|x| classes.contains(&x.as_str())) {
                    for (k, v) in &r.decls {
                        match k.as_str() {
                            "stop-color" => c = color::parse(v).unwrap_or(c),
                            "stop-opacity" => op = color_frac(v).unwrap_or(op),
                            _ => {}
                        }
                    }
                }
            }
            let a = (c.1 as f64 * op + 0.5) as u32;
            stops.push((off, a << 24 | c.0));
        }
        if stops.is_empty() {
            return None;
        }
        if stops.len() == 1 {
            let v = stops[0].1;
            return Some(alloc::boxed::Box::new(move |_, _| v));
        }
        let user = self.grad_attr(g, "gradientUnits").as_deref() == Some("userSpaceOnUse");
        let gt = self.grad_attr(g, "gradientTransform").map(|t| transform(&t)).unwrap_or(M::ID);
        let spread = self.grad_attr(g, "spreadMethod").unwrap_or_default();
        let unit = if user { M::ID } else { M::new(bb[2] - bb[0], 0.0, 0.0, bb[3] - bb[1], bb[0], bb[1]) };
        // device -> gradient space
        let inv = m.mul(&unit).mul(&gt).invert();
        let (pw, ph) = (self.width, self.height);
        let get = |k: &str, d: f64, pct: f64| -> f64 {
            self.grad_attr(g, k)
                .and_then(|v| if !user && v.ends_with('%') { number(v.trim_end_matches('%')).map(|x| x / 100.0) } else { length(&v, pct) })
                .unwrap_or(d)
        };
        let color_at = move |t: f64| -> u32 {
            let t = match spread.as_str() {
                "repeat" => t - floor(t),
                "reflect" => {
                    let r = t - 2.0 * floor(t / 2.0);
                    if r > 1.0 {
                        2.0 - r
                    } else {
                        r
                    }
                }
                _ => t.clamp(0.0, 1.0),
            };
            if t <= stops[0].0 {
                return stops[0].1;
            }
            for w in stops.windows(2) {
                if t <= w[1].0 {
                    let span = w[1].0 - w[0].0;
                    let f = if span > 1e-9 { (t - w[0].0) / span } else { 1.0 };
                    let mix = |sh: u32| {
                        let (a, b) = (((w[0].1 >> sh) & 255) as f64, ((w[1].1 >> sh) & 255) as f64);
                        ((a + (b - a) * f + 0.5) as u32) << sh
                    };
                    return mix(24) | mix(16) | mix(8) | mix(0);
                }
            }
            stops[stops.len() - 1].1
        };
        if tag == "linearGradient" {
            let (x1, y1) = (get("x1", 0.0, pw), get("y1", 0.0, ph));
            let (x2, y2) = (get("x2", if user { pw } else { 1.0 }, pw), get("y2", 0.0, ph));
            let (dx, dy) = (x2 - x1, y2 - y1);
            let dd = (dx * dx + dy * dy).max(1e-12);
            Some(alloc::boxed::Box::new(move |x, y| {
                let (gx, gy) = inv.apply((x, y));
                color_at(((gx - x1) * dx + (gy - y1) * dy) / dd)
            }))
        } else {
            let half = if user { 0.0 } else { 0.5 };
            let (cx, cy) = (get("cx", half, pw), get("cy", half, ph));
            let r = get("r", half, pw).max(1e-9);
            let (fx, fy) = (get("fx", cx, pw), get("fy", cy, ph));
            Some(alloc::boxed::Box::new(move |x, y| {
                let (gx, gy) = inv.apply((x, y));
                // t where the point lies on the circle interpolated from the focus
                let (dx, dy) = (gx - fx, gy - fy);
                let (ex, ey) = (cx - fx, cy - fy);
                let a = ex * ex + ey * ey - r * r;
                let b = dx * ex + dy * ey;
                let c = dx * dx + dy * dy;
                let t = if abs(a) < 1e-12 {
                    if abs(b) < 1e-12 { 0.0 } else { c / (2.0 * b) }
                } else {
                    let disc = b * b - a * c;
                    let s = sqrt(disc.max(0.0));
                    ((b - s) / a).max((b + s) / a).max(0.0)
                };
                color_at(t)
            }))
        }
    }
}
