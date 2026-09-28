//! CSS: parsing stylesheets (with @media, @supports and @layer blocks),
//! selectors (type, class, id, attributes, structural pseudo-classes,
//! :not/:is/:where, and all four combinators), media queries evaluated
//! against the window, the cascade's ordering (!important, specificity,
//! source order), custom properties and var().
//!
//! Also the helpers the browser uses for external stylesheets: finding
//! @import rules and rewriting url()s against the sheet's own address.

use super::html::{Dom, Kind, NodeId};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------- text scanning

/// Remove /* comments */ (not inside strings).
pub fn strip_comments(css: &str) -> String {
    let b = css.as_bytes();
    let mut out = String::with_capacity(css.len());
    let mut i = 0;
    let mut start = 0;
    let mut quote = 0u8;
    while i < b.len() {
        let c = b[i];
        if quote != 0 {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            out.push_str(&css[start..i]);
            let end = css[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 2);
            i = end;
            start = end;
            continue;
        }
        i += 1;
    }
    out.push_str(&css[start.min(css.len())..]);
    out
}

/// Index of the first `target` byte at nesting depth 0 (outside strings,
/// parentheses and brackets), from `from`.
fn find_top(s: &str, from: usize, targets: &[u8]) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote = 0u8;
    let mut i = from;
    while i < b.len() {
        let c = b[i];
        if quote != 0 {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == quote {
                quote = 0;
            }
        } else {
            if depth <= 0 && targets.contains(&c) {
                return Some(i);
            }
            match c {
                b'"' | b'\'' => quote = c,
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// The index of the `}` closing the block opened at `open`.
fn block_end(s: &str, open: usize) -> usize {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote = 0u8;
    let mut i = open;
    while i < b.len() {
        let c = b[i];
        if quote != 0 {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == quote {
                quote = 0;
            }
        } else {
            match c {
                b'"' | b'\'' => quote = c,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    b.len()
}

/// Split at a separator outside parentheses and strings.
pub fn split_top_pub(s: &str, sep: u8) -> Vec<&str> {
    split_top(s, sep)
}

/// Split at top-level commas.
fn split_top(s: &str, sep: u8) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(i) = find_top(s, start, &[sep]) {
        out.push(&s[start..i]);
        start = i + 1;
    }
    out.push(&s[start..]);
    out
}

// ---------------------------------------------------------------- declarations

#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub name: String,
    pub value: String,
    pub important: bool,
}

/// Declarations of a block or a style="" attribute (nested rules skipped).
pub fn parse_decls(body: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    let mut start = 0;
    let b = body.as_bytes();
    loop {
        let end = find_top(body, start, b";{").unwrap_or(b.len());
        if end < b.len() && b[end] == b'{' {
            // a nested rule (CSS nesting): skip it
            let close = block_end(body, end);
            start = close + 1;
            if start >= b.len() {
                break;
            }
            continue;
        }
        let d = &body[start..end];
        if let Some(colon) = d.find(':') {
            let name = d[..colon].trim();
            let mut value = d[colon + 1..].trim();
            let mut important = false;
            if let Some(i) = value.to_ascii_lowercase().rfind("!important") {
                important = true;
                value = value[..i].trim();
            }
            if !name.is_empty() && !value.is_empty() {
                let name = if name.starts_with("--") { name.to_string() } else { name.to_ascii_lowercase() };
                out.push(Decl { name, value: value.to_string(), important });
            }
        }
        if end >= b.len() {
            break;
        }
        start = end + 1;
    }
    out
}

// ---------------------------------------------------------------- selectors

#[derive(Clone, Debug)]
enum AttrOp {
    Exists,
    Eq,
    Word,
    Dash,
    Prefix,
    Suffix,
    Contains,
}

#[derive(Clone, Debug)]
struct AttrSel {
    name: String,
    op: AttrOp,
    value: String,
    nocase: bool,
}

#[derive(Clone, Debug)]
enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    NthChild(i32, i32, bool),
    NthOfType(i32, i32, bool),
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Not(Vec<Complex>),
    Is(Vec<Complex>),
    Root,
    Link,
    Empty,
    Checked,
    Disabled,
    Enabled,
}

#[derive(Clone, Debug, Default)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<AttrSel>,
    pseudos: Vec<Pseudo>,
    /// can't match a static page (:hover, ::before, ...)
    never: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Comb {
    Descendant,
    Child,
    Next,
    Later,
}

/// A complex selector, right to left: parts[0] is the element itself, and
/// parts[i].1 says how parts[i + 1] relates to it.
#[derive(Clone, Debug)]
struct Complex {
    parts: Vec<(Compound, Comb)>,
    spec: u32,
}

fn ident_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'_' || b[i] >= 0x80 || b[i] == b'\\') {
        if b[i] == b'\\' {
            i += 1;
        }
        i += 1;
    }
    i.min(b.len())
}

fn unescape_ident(s: &str) -> String {
    s.replace('\\', "")
}

/// an+b notation: (a, b).
fn nth(arg: &str) -> Option<(i32, i32)> {
    let a = arg.trim().to_ascii_lowercase();
    let a = a.split(" of ").next().unwrap_or("").trim().to_string();
    match a.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Some(k) = a.find('n') {
        let coef = a[..k].trim();
        let a_val = match coef {
            "" | "+" => 1,
            "-" => -1,
            c => c.parse().ok()?,
        };
        let rest: String = a[k + 1..].chars().filter(|c| !c.is_whitespace()).collect();
        let b_val = if rest.is_empty() { 0 } else { rest.trim_start_matches('+').parse().ok()? };
        Some((a_val, b_val))
    } else {
        a.parse().ok().map(|b| (0, b))
    }
}

fn parse_compound(s: &str) -> Option<(Compound, u32)> {
    let mut c = Compound::default();
    let mut spec = 0u32;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'*' => i += 1,
            b'#' => {
                let e = ident_end(b, i + 1);
                c.id = Some(unescape_ident(&s[i + 1..e]));
                spec += 1 << 16;
                i = e;
            }
            b'.' => {
                let e = ident_end(b, i + 1);
                if e == i + 1 {
                    return None;
                }
                c.classes.push(unescape_ident(&s[i + 1..e]));
                spec += 1 << 8;
                i = e;
            }
            b'[' => {
                let close = find_top(s, i + 1, b"]").unwrap_or(b.len());
                let inner = &s[i + 1..close.min(s.len())];
                let ops = [("~=", AttrOp::Word), ("|=", AttrOp::Dash), ("^=", AttrOp::Prefix), ("$=", AttrOp::Suffix), ("*=", AttrOp::Contains), ("=", AttrOp::Eq)];
                let mut sel = AttrSel { name: inner.trim().to_ascii_lowercase(), op: AttrOp::Exists, value: String::new(), nocase: false };
                for (tok, op) in ops {
                    if let Some(k) = inner.find(tok) {
                        let mut v = inner[k + tok.len()..].trim();
                        if v.ends_with(" i") || v.ends_with(" I") {
                            sel.nocase = true;
                            v = v[..v.len() - 2].trim();
                        } else if v.ends_with(" s") {
                            v = v[..v.len() - 2].trim();
                        }
                        sel.name = inner[..k].trim().to_ascii_lowercase();
                        sel.value = v.trim_matches(|q| q == '"' || q == '\'').to_string();
                        sel.op = op;
                        break;
                    }
                }
                c.attrs.push(sel);
                spec += 1 << 8;
                i = close + 1;
            }
            b':' => {
                let element = b.get(i + 1) == Some(&b':');
                let st = if element { i + 2 } else { i + 1 };
                let e = ident_end(b, st);
                let name = s[st..e].to_ascii_lowercase();
                let mut arg = "";
                i = e;
                if i < b.len() && b[i] == b'(' {
                    let close = find_top(s, i + 1, b")").unwrap_or(b.len());
                    arg = &s[i + 1..close.min(s.len())];
                    i = close + 1;
                }
                if element || matches!(name.as_str(), "before" | "after" | "first-line" | "first-letter" | "placeholder" | "selection" | "marker") {
                    c.never = true;
                    spec += 1;
                    continue;
                }
                let list = |a: &str| -> Vec<Complex> { split_top(a, b',').into_iter().filter_map(parse_complex).collect() };
                let p = match name.as_str() {
                    "first-child" => Pseudo::FirstChild,
                    "last-child" => Pseudo::LastChild,
                    "only-child" => Pseudo::OnlyChild,
                    "first-of-type" => Pseudo::FirstOfType,
                    "last-of-type" => Pseudo::LastOfType,
                    "only-of-type" => Pseudo::OnlyOfType,
                    "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
                        let Some((a, bb)) = nth(arg) else {
                            c.never = true;
                            continue;
                        };
                        let last = name.contains("last");
                        if name.contains("of-type") {
                            Pseudo::NthOfType(a, bb, last)
                        } else {
                            Pseudo::NthChild(a, bb, last)
                        }
                    }
                    "not" => {
                        let l = list(arg);
                        spec += l.iter().map(|x| x.spec).max().unwrap_or(0);
                        c.pseudos.push(Pseudo::Not(l));
                        continue;
                    }
                    "is" | "matches" | "-webkit-any" | "any" | "where" => {
                        let l = list(arg);
                        if name != "where" {
                            spec += l.iter().map(|x| x.spec).max().unwrap_or(0);
                        }
                        c.pseudos.push(Pseudo::Is(l));
                        continue;
                    }
                    "root" => Pseudo::Root,
                    "link" | "any-link" => Pseudo::Link,
                    "empty" => Pseudo::Empty,
                    "checked" => Pseudo::Checked,
                    "disabled" => Pseudo::Disabled,
                    "enabled" => Pseudo::Enabled,
                    "visited" | "hover" | "focus" | "active" | "focus-within" | "focus-visible" | "target" | "has" | "invalid" | "fullscreen" | "modal" | "popover-open" | "placeholder-shown" | "autofill" => {
                        c.never = true;
                        spec += 1 << 8;
                        continue;
                    }
                    _ => {
                        c.never = true;
                        continue;
                    }
                };
                spec += 1 << 8;
                c.pseudos.push(p);
            }
            x if x.is_ascii_alphabetic() || x == b'-' || x == b'_' || x >= 0x80 => {
                let e = ident_end(b, i);
                c.tag = Some(s[i..e].to_ascii_lowercase());
                spec += 1;
                i = e;
            }
            b'|' => i += 1, // namespace prefixes
            _ => return None,
        }
    }
    Some((c, spec))
}

fn parse_complex(s: &str) -> Option<Complex> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // split into compounds and combinators
    let mut items: Vec<(String, Comb)> = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    let mut cur = String::new();
    let mut pending: Option<Comb> = None;
    while i < b.len() {
        let c = b[i];
        match c {
            b'(' | b'[' => {
                let close = find_top(s, i + 1, if c == b'(' { b")" } else { b"]" }).unwrap_or(b.len() - 1);
                cur.push_str(&s[i..=close.min(b.len() - 1)]);
                i = close + 1;
                continue;
            }
            b'>' | b'+' | b'~' if !(c == b'+' && cur.ends_with(|ch: char| ch == '(')) => {
                if !cur.is_empty() {
                    items.push((core::mem::take(&mut cur), pending.take().unwrap_or(Comb::Descendant)));
                }
                pending = Some(match c {
                    b'>' => Comb::Child,
                    b'+' => Comb::Next,
                    _ => Comb::Later,
                });
            }
            c if c.is_ascii_whitespace() => {
                if !cur.is_empty() {
                    items.push((core::mem::take(&mut cur), pending.take().unwrap_or(Comb::Descendant)));
                    pending = None;
                }
            }
            _ => cur.push(c as char),
        }
        i += 1;
    }
    if !cur.is_empty() {
        items.push((cur, pending.take().unwrap_or(Comb::Descendant)));
    }
    // items[k].1 is the combinator before items[k]; rebuild right to left
    let mut parts = Vec::new();
    let mut spec = 0;
    for k in (0..items.len()).rev() {
        let (c, sp) = parse_compound(&items[k].0)?;
        spec += sp;
        let comb = items[k].1;
        parts.push((c, comb));
    }
    // (the combinator stored with each part is the one to its left)
    Some(Complex { parts, spec })
}

// ---------------------------------------------------------------- matching

fn is_element(dom: &Dom, n: NodeId) -> bool {
    matches!(dom.nodes[n].kind, Kind::Element { .. })
}

/// Element siblings of n: (position, count), counting all or the same type.
fn position(dom: &Dom, n: NodeId, same_type: bool) -> (usize, usize) {
    let Some(p) = dom.nodes[n].parent else { return (0, 1) };
    let tag = dom.tag(n);
    let mut pos = 0;
    let mut count = 0;
    for &c in &dom.nodes[p].children {
        if !is_element(dom, c) || (same_type && dom.tag(c) != tag) {
            continue;
        }
        if c == n {
            pos = count;
        }
        count += 1;
    }
    (pos, count)
}

fn nth_ok(a: i32, b: i32, idx: i32) -> bool {
    // idx is 1-based; some n >= 0 with a*n + b = idx
    if a == 0 {
        return idx == b;
    }
    let d = idx - b;
    d % a == 0 && d / a >= 0
}

fn prev_element(dom: &Dom, n: NodeId) -> Option<NodeId> {
    let p = dom.nodes[n].parent?;
    let kids = &dom.nodes[p].children;
    let i = kids.iter().position(|&c| c == n)?;
    kids[..i].iter().rev().copied().find(|&c| is_element(dom, c))
}

fn attr_ok(dom: &Dom, n: NodeId, a: &AttrSel) -> bool {
    let Some(v) = dom.attr(n, &a.name) else { return false };
    let (v, want) = if a.nocase { (v.to_ascii_lowercase(), a.value.to_ascii_lowercase()) } else { (v.to_string(), a.value.clone()) };
    match a.op {
        AttrOp::Exists => true,
        AttrOp::Eq => v == want,
        AttrOp::Word => v.split_whitespace().any(|w| w == want),
        AttrOp::Dash => v == want || v.starts_with(&(want.clone() + "-")),
        AttrOp::Prefix => !want.is_empty() && v.starts_with(&want),
        AttrOp::Suffix => !want.is_empty() && v.ends_with(&want),
        AttrOp::Contains => !want.is_empty() && v.contains(&want),
    }
}

fn compound_ok(dom: &Dom, n: NodeId, c: &Compound) -> bool {
    if c.never || !is_element(dom, n) {
        return false;
    }
    if let Some(t) = &c.tag {
        if dom.tag(n) != t {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if dom.attr(n, "id") != Some(id.as_str()) {
            return false;
        }
    }
    if !c.classes.is_empty() {
        let cls = dom.attr(n, "class").unwrap_or("");
        if !c.classes.iter().all(|x| cls.split_whitespace().any(|y| y == x)) {
            return false;
        }
    }
    if !c.attrs.iter().all(|a| attr_ok(dom, n, a)) {
        return false;
    }
    for p in &c.pseudos {
        let ok = match p {
            Pseudo::FirstChild => position(dom, n, false).0 == 0,
            Pseudo::LastChild => {
                let (i, k) = position(dom, n, false);
                i + 1 == k
            }
            Pseudo::OnlyChild => position(dom, n, false).1 == 1,
            Pseudo::FirstOfType => position(dom, n, true).0 == 0,
            Pseudo::LastOfType => {
                let (i, k) = position(dom, n, true);
                i + 1 == k
            }
            Pseudo::OnlyOfType => position(dom, n, true).1 == 1,
            Pseudo::NthChild(a, b, last) | Pseudo::NthOfType(a, b, last) => {
                let (i, k) = position(dom, n, matches!(p, Pseudo::NthOfType(..)));
                let idx = if *last { k - i } else { i + 1 };
                nth_ok(*a, *b, idx as i32)
            }
            Pseudo::Not(list) => !list.iter().any(|s| complex_ok(dom, n, s, 0)),
            Pseudo::Is(list) => list.iter().any(|s| complex_ok(dom, n, s, 0)),
            Pseudo::Root => dom.tag(n) == "html" || dom.nodes[n].parent.is_some_and(|p| matches!(dom.nodes[p].kind, Kind::Document)),
            Pseudo::Link => matches!(dom.tag(n), "a" | "area") && dom.attr(n, "href").is_some(),
            Pseudo::Empty => dom.nodes[n].children.iter().all(|&k| match &dom.nodes[k].kind {
                Kind::Text(t) => t.is_empty(),
                _ => false,
            }),
            Pseudo::Checked => dom.attr(n, "checked").is_some() || dom.attr(n, "selected").is_some(),
            Pseudo::Disabled => dom.attr(n, "disabled").is_some(),
            Pseudo::Enabled => matches!(dom.tag(n), "input" | "button" | "select" | "textarea") && dom.attr(n, "disabled").is_none(),
        };
        if !ok {
            return false;
        }
    }
    true
}

/// Does `sel` (from part `k` on) match element `n`?
fn complex_ok(dom: &Dom, n: NodeId, sel: &Complex, k: usize) -> bool {
    if !compound_ok(dom, n, &sel.parts[k].0) {
        return false;
    }
    if k + 1 == sel.parts.len() {
        return true;
    }
    match sel.parts[k].1 {
        Comb::Child => dom.nodes[n].parent.is_some_and(|p| complex_ok(dom, p, sel, k + 1)),
        Comb::Descendant => {
            let mut p = dom.nodes[n].parent;
            while let Some(pn) = p {
                if complex_ok(dom, pn, sel, k + 1) {
                    return true;
                }
                p = dom.nodes[pn].parent;
            }
            false
        }
        Comb::Next => prev_element(dom, n).is_some_and(|s| complex_ok(dom, s, sel, k + 1)),
        Comb::Later => {
            let mut s = prev_element(dom, n);
            while let Some(sn) = s {
                if complex_ok(dom, sn, sel, k + 1) {
                    return true;
                }
                s = prev_element(dom, sn);
            }
            false
        }
    }
}

// ---------------------------------------------------------------- media queries

/// The window, for media queries.
#[derive(Clone, Copy)]
pub struct Media {
    pub width: i32,
    pub height: i32,
}

fn px(v: &str) -> Option<f64> {
    let v = v.trim();
    let num = |s: &str| s.trim().parse::<f64>().ok();
    if let Some(n) = v.strip_suffix("px") {
        return num(n);
    }
    if let Some(n) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
        return num(n).map(|x| x * 16.0);
    }
    num(v)
}

fn feature(f: &str, m: Media) -> bool {
    let f = f.trim();
    // range syntax: (width >= 600px), (400px <= width < 900px)
    if f.contains('<') || f.contains('>') || (f.contains('=') && !f.contains(':')) {
        let toks: Vec<&str> = f.split(|c| c == '<' || c == '>' || c == '=').map(str::trim).filter(|t| !t.is_empty()).collect();
        let ops: Vec<String> = {
            let mut v = Vec::new();
            let mut cur = String::new();
            for ch in f.chars() {
                if ch == '<' || ch == '>' || ch == '=' {
                    cur.push(ch);
                } else if !cur.is_empty() {
                    v.push(core::mem::take(&mut cur));
                }
            }
            v
        };
        let value = |name: &str| -> Option<f64> {
            match name {
                "width" => Some(m.width as f64),
                "height" => Some(m.height as f64),
                _ => None,
            }
        };
        let cmp = |a: f64, op: &str, b: f64| match op {
            "<" => a < b,
            "<=" => a <= b,
            ">" => a > b,
            ">=" => a >= b,
            "=" => a == b,
            _ => false,
        };
        return match (toks.as_slice(), ops.as_slice()) {
            ([a, b], [op]) => match (value(a), px(b), px(a), value(b)) {
                (Some(x), Some(y), _, _) => cmp(x, op, y),
                (_, _, Some(x), Some(y)) => cmp(x, op, y),
                _ => false,
            },
            ([a, name, b], [o1, o2]) => match (px(a), value(name), px(b)) {
                (Some(x), Some(v), Some(y)) => cmp(x, o1, v) && cmp(v, o2, y),
                _ => false,
            },
            _ => false,
        };
    }
    let (name, val) = match f.split_once(':') {
        Some((n, v)) => (n.trim(), v.trim()),
        None => (f, ""),
    };
    let w = m.width as f64;
    let h = m.height as f64;
    match name {
        "min-width" => px(val).is_some_and(|v| w >= v),
        "max-width" => px(val).is_some_and(|v| w <= v),
        "width" => px(val).is_some_and(|v| w == v),
        "min-height" => px(val).is_some_and(|v| h >= v),
        "max-height" => px(val).is_some_and(|v| h <= v),
        "orientation" => val == if w >= h { "landscape" } else { "portrait" },
        "prefers-color-scheme" => val == "light",
        "prefers-reduced-motion" | "prefers-reduced-transparency" | "prefers-reduced-data" => val == "no-preference",
        "prefers-contrast" => val == "no-preference",
        "hover" | "any-hover" => val == "hover" || val.is_empty(),
        "pointer" | "any-pointer" => val == "fine" || val.is_empty(),
        "color" | "grid" if val.is_empty() => name == "color",
        "min-resolution" => val.trim_end_matches("dppx").trim_end_matches('x').trim().parse::<f64>().is_ok_and(|v| v <= 1.0) || val.ends_with("dpi") && px(val.trim_end_matches("dpi")).is_some_and(|v| v <= 96.0),
        "max-resolution" => true,
        "-webkit-min-device-pixel-ratio" | "min--moz-device-pixel-ratio" => px(val).is_some_and(|v| v <= 1.0),
        "-webkit-max-device-pixel-ratio" => true,
        "min-aspect-ratio" | "max-aspect-ratio" | "aspect-ratio" => {
            let r: Vec<f64> = val.split('/').filter_map(|x| x.trim().parse().ok()).collect();
            let want = if r.len() == 2 && r[1] > 0.0 { r[0] / r[1] } else { return false };
            let have = w / h.max(1.0);
            match name {
                "min-aspect-ratio" => have >= want,
                "max-aspect-ratio" => have <= want,
                _ => (have - want) < 0.01 && (want - have) < 0.01,
            }
        }
        "scripting" => val == "none",
        "update" => val == "fast",
        _ => false,
    }
}

/// Does a media query list (as in @media or <link media>) match?
pub fn media_matches(list: &str, m: Media) -> bool {
    let list = list.trim().to_ascii_lowercase();
    if list.is_empty() {
        return true;
    }
    split_top(&list, b',').iter().any(|q| {
        let mut q = q.trim();
        let mut negate = false;
        if let Some(r) = q.strip_prefix("not ") {
            negate = true;
            q = r.trim();
        }
        if let Some(r) = q.strip_prefix("only ") {
            q = r.trim();
        }
        // "screen and (min-width: 600px) and (...)"; "or" between features too
        let ok = q.split(" or ").any(|alt| {
            alt.split(" and ").all(|part| {
                let part = part.trim();
                if let Some(inner) = part.strip_prefix('(').and_then(|p| p.strip_suffix(')')) {
                    if let Some(neg) = inner.trim().strip_prefix("not ") {
                        return !feature(neg.trim_matches(|c| c == '(' || c == ')'), m);
                    }
                    feature(inner, m)
                } else if let Some(neg) = part.strip_prefix("not") {
                    !feature(neg.trim().trim_matches(|c| c == '(' || c == ')'), m)
                } else {
                    matches!(part, "all" | "screen" | "")
                }
            })
        });
        ok != negate
    })
}

// ---------------------------------------------------------------- the cascade

struct Rule {
    sel: Complex,
    block: usize,
    order: u32,
}

/// All the author style rules of a page, indexed for fast matching.
#[derive(Default)]
pub struct Cascade {
    rules: Vec<Rule>,
    blocks: Vec<Vec<Decl>>,
    by_id: BTreeMap<String, Vec<usize>>,
    by_class: BTreeMap<String, Vec<usize>>,
    by_tag: BTreeMap<String, Vec<usize>>,
    other: Vec<usize>,
}

/// Stop reading after this many rules (giant framework sheets).
const MAX_RULES: usize = 60_000;

impl Cascade {
    pub fn new(css: &str, m: Media) -> Cascade {
        let mut c = Cascade::default();
        let src = strip_comments(css);
        c.parse_list(&src, m, 0);
        for (i, r) in c.rules.iter().enumerate() {
            let subject = &r.sel.parts[0].0;
            if let Some(id) = &subject.id {
                c.by_id.entry(id.clone()).or_default().push(i);
            } else if let Some(cl) = subject.classes.first() {
                c.by_class.entry(cl.clone()).or_default().push(i);
            } else if let Some(t) = &subject.tag {
                c.by_tag.entry(t.clone()).or_default().push(i);
            } else {
                c.other.push(i);
            }
        }
        c
    }

    #[allow(dead_code)] // used by the host tests
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    fn parse_list(&mut self, src: &str, m: Media, depth: u32) {
        if depth > 8 {
            return;
        }
        let b = src.as_bytes();
        let mut i = 0;
        while i < b.len() && self.rules.len() < MAX_RULES {
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b';') {
                i += 1;
            }
            if i >= b.len() {
                break;
            }
            let Some(stop) = find_top(src, i, b"{;}") else { break };
            if b[stop] == b'}' {
                // a stray close brace
                i = stop + 1;
                continue;
            }
            let prelude = src[i..stop].trim();
            if b[stop] == b';' {
                // @import, @charset, @namespace: statements, nothing to apply
                i = stop + 1;
                continue;
            }
            let close = block_end(src, stop);
            let body = &src[stop + 1..close.min(src.len())];
            i = close + 1;
            if let Some(at) = prelude.strip_prefix('@') {
                let (name, cond) = at.split_once(|c: char| c.is_whitespace() || c == '(').map_or((at, ""), |(n, _)| (n, &at[n.len()..]));
                match name.to_ascii_lowercase().as_str() {
                    "media" => {
                        if media_matches(cond, m) {
                            self.parse_list(body, m, depth + 1);
                        }
                    }
                    "supports" => {
                        // assume support, except for the obviously negated
                        if !cond.trim_start().starts_with("not") {
                            self.parse_list(body, m, depth + 1);
                        }
                    }
                    "layer" | "container" | "scope" | "document" | "-moz-document" => self.parse_list(body, m, depth + 1),
                    _ => {} // font-face, keyframes, page, property, ...
                }
                continue;
            }
            let decls = parse_decls(body);
            if decls.is_empty() {
                continue;
            }
            let block = self.blocks.len();
            self.blocks.push(decls);
            for s in split_top(prelude, b',') {
                if let Some(sel) = parse_complex(s) {
                    let order = self.rules.len() as u32;
                    self.rules.push(Rule { sel, block, order });
                }
            }
        }
    }

    /// Declarations that apply to `n`: (important, specificity, order, decl),
    /// sorted into cascade order.
    pub fn matching<'a>(&'a self, dom: &Dom, n: NodeId) -> Vec<(bool, u32, u32, &'a Decl)> {
        let mut cands: Vec<usize> = self.other.clone();
        if let Some(v) = self.by_tag.get(dom.tag(n)) {
            cands.extend(v);
        }
        if let Some(id) = dom.attr(n, "id") {
            if let Some(v) = self.by_id.get(id) {
                cands.extend(v);
            }
        }
        if let Some(cl) = dom.attr(n, "class") {
            for c in cl.split_whitespace() {
                if let Some(v) = self.by_class.get(c) {
                    cands.extend(v);
                }
            }
        }
        cands.sort_unstable();
        cands.dedup();
        let mut out = Vec::new();
        for i in cands {
            let r = &self.rules[i];
            if complex_ok(dom, n, &r.sel, 0) {
                for d in &self.blocks[r.block] {
                    out.push((d.important, r.sel.spec, r.order, d));
                }
            }
        }
        out.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        out
    }
}

// ---------------------------------------------------------------- var()

/// Substitute var(--name, fallback) using `lookup`. None if a variable is
/// missing without a fallback (the declaration is then ignored).
pub fn resolve_vars(v: &str, lookup: &dyn Fn(&str) -> Option<String>, depth: u32) -> Option<String> {
    if depth > 16 {
        return None;
    }
    let mut out = String::new();
    let mut rest = v;
    while let Some(i) = rest.find("var(") {
        out.push_str(&rest[..i]);
        let args_start = i + 4;
        let close = find_top(rest, args_start, b")")?;
        let args = &rest[args_start..close];
        let (name, fallback) = match find_top(args, 0, b",") {
            Some(k) => (args[..k].trim(), Some(args[k + 1..].trim())),
            None => (args.trim(), None),
        };
        let val = match lookup(name) {
            Some(x) => resolve_vars(&x, lookup, depth + 1)?,
            None => resolve_vars(fallback?, lookup, depth + 1)?,
        };
        out.push_str(&val);
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

// ---------------------------------------------------------------- external sheets

/// The url() or quoted address in an @import prelude.
fn import_target(prelude: &str) -> Option<(String, String)> {
    let p = prelude.trim();
    let (url, rest) = if let Some(r) = p.strip_prefix("url(") {
        let end = r.find(')')?;
        (r[..end].trim().trim_matches(|c| c == '"' || c == '\''), &r[end + 1..])
    } else {
        let q = p.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let end = p[1..].find(q)? + 1;
        (&p[1..end], &p[end + 1..])
    };
    // skip layer / layer(...) / supports(...) before the media list
    let mut media = rest.trim();
    for word in ["layer", "supports"] {
        if let Some(r) = media.strip_prefix(word) {
            media = match r.strip_prefix('(') {
                Some(inner) => find_top(inner, 0, b")").map_or("", |k| &inner[k + 1..]),
                None => r,
            }
            .trim();
        }
    }
    Some((url.to_string(), media.to_string()))
}

/// The @import rules at the top of a sheet: (address, media).
pub fn imports(css: &str) -> Vec<(String, String)> {
    let src = strip_comments(css);
    let mut out = Vec::new();
    let mut i = 0;
    let b = src.as_bytes();
    while i < b.len() {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let rest = &src[i..];
        let lower = rest.get(..10).unwrap_or(rest).to_ascii_lowercase();
        if lower.starts_with("@charset") || lower.starts_with("@layer") && find_top(rest, 0, b";{").is_some_and(|k| rest.as_bytes()[k] == b';') {
            i += find_top(rest, 0, b";").map_or(rest.len(), |k| k + 1);
            continue;
        }
        if !lower.starts_with("@import") {
            break;
        }
        let end = find_top(rest, 0, b";").unwrap_or(rest.len());
        if let Some(t) = import_target(&rest[7..end]) {
            out.push(t);
        }
        i += end + 1;
    }
    out
}

/// Rewrite every url(...) and @import address with `resolve` (to make a
/// sheet's relative addresses absolute, against the sheet's own address).
pub fn absolutize(css: &str, resolve: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    loop {
        let u = rest.find("url(");
        let imp = rest.find("@import");
        let next = match (u, imp) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        if Some(next) == imp && u != Some(next) {
            // @import "file.css";
            out.push_str(&rest[..next + 7]);
            let after = &rest[next + 7..];
            let t = after.trim_start();
            let skipped = after.len() - t.len();
            if let Some(q) = t.chars().next().filter(|&c| c == '"' || c == '\'') {
                if let Some(end) = t[1..].find(q) {
                    let addr = &t[1..end + 1];
                    out.push_str(&after[..skipped]);
                    out.push(q);
                    out.push_str(&resolve(addr).unwrap_or_else(|| addr.to_string()));
                    out.push(q);
                    rest = &t[end + 2..];
                    continue;
                }
            }
            rest = after;
            continue;
        }
        out.push_str(&rest[..next + 4]);
        let after = &rest[next + 4..];
        let Some(end) = after.find(')') else {
            rest = after;
            break;
        };
        let raw = after[..end].trim();
        let q = raw.chars().next().filter(|&c| c == '"' || c == '\'');
        let addr = raw.trim_matches(|c| c == '"' || c == '\'');
        if addr.starts_with("data:") || addr.starts_with('#') {
            out.push_str(&after[..end]);
        } else {
            let abs = resolve(addr).unwrap_or_else(|| addr.to_string());
            match q {
                Some(q) => {
                    out.push(q);
                    out.push_str(&abs);
                    out.push(q);
                }
                None => out.push_str(&abs),
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// The document's style sources in order: Ok(inline text) for <style>, and
/// Err((href, media)) for <link rel=stylesheet>.
pub fn sources(dom: &Dom) -> Vec<Result<String, (String, String)>> {
    let mut out = Vec::new();
    for n in 0..dom.nodes.len() {
        match dom.tag(n) {
            "style" => {
                let media = dom.attr(n, "media").unwrap_or("").to_string();
                let mut text = String::new();
                for &c in &dom.nodes[n].children {
                    if let Kind::Text(t) = &dom.nodes[c].kind {
                        text.push_str(t);
                    }
                }
                out.push(Ok(if media.is_empty() { text } else { alloc::format!("@media {} {{\n{}\n}}", media, text) }));
            }
            "link" => {
                let rel = dom.attr(n, "rel").unwrap_or("").to_ascii_lowercase();
                let words: Vec<&str> = rel.split_whitespace().collect();
                if words.contains(&"stylesheet") && !words.contains(&"alternate") && dom.attr(n, "disabled").is_none() {
                    if let Some(h) = dom.attr(n, "href").filter(|h| !h.trim().is_empty()) {
                        out.push(Err((h.trim().to_string(), dom.attr(n, "media").unwrap_or("").to_string())));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Wrap a sheet's text in its media condition.
pub fn with_media(css: &str, media: &str) -> String {
    let m = media.trim();
    if m.is_empty() || m.eq_ignore_ascii_case("all") {
        css.to_string()
    } else {
        alloc::format!("@media {} {{\n{}\n}}", m, css)
    }
}
