//! HTML parsing: a tokenizer and a forgiving tree builder (the parts of the
//! WHATWG algorithm real pages lean on: void elements, raw-text script/style,
//! implied end tags for p/li/td/..., character references).

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub type NodeId = usize;

#[derive(Clone, Debug)]
pub enum Kind {
    Document,
    Element { tag: String, attrs: Vec<(String, String)> },
    Text(String),
}

#[derive(Clone, Debug)]
pub struct Node {
    pub kind: Kind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}

pub struct Dom {
    pub nodes: Vec<Node>,
}

const VOID: [&str; 14] = ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
// <noscript> is ordinary markup: HydatekOS doesn't run scripts, so it shows.
const RAW: [&str; 4] = ["script", "style", "textarea", "title"];

impl Dom {
    pub fn tag(&self, n: NodeId) -> &str {
        match &self.nodes[n].kind {
            Kind::Element { tag, .. } => tag,
            _ => "",
        }
    }

    pub fn attr(&self, n: NodeId, name: &str) -> Option<&str> {
        match &self.nodes[n].kind {
            Kind::Element { attrs, .. } => attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    /// First element with this tag, depth first.
    pub fn find(&self, tag: &str) -> Option<NodeId> {
        (0..self.nodes.len()).find(|&n| self.tag(n) == tag)
    }

    /// All text under `n`, whitespace collapsed.
    pub fn text(&self, n: NodeId) -> String {
        let mut out = String::new();
        self.collect_text(n, &mut out);
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn collect_text(&self, n: NodeId, out: &mut String) {
        match &self.nodes[n].kind {
            Kind::Text(t) => {
                out.push_str(t);
                out.push(' ');
            }
            Kind::Element { tag, .. } if matches!(tag.as_str(), "script" | "style" | "noscript" | "template" | "head" | "title") => {}
            _ => {
                for &c in &self.nodes[n].children {
                    self.collect_text(c, out);
                }
            }
        }
    }

    pub fn title(&self) -> String {
        match self.find("title") {
            Some(t) => {
                let mut s = String::new();
                for &c in &self.nodes[t].children {
                    if let Kind::Text(x) = &self.nodes[c].kind {
                        s.push_str(x);
                    }
                }
                s.split_whitespace().collect::<Vec<_>>().join(" ")
            }
            None => String::new(),
        }
    }

    /// Every link target (href of <a>), in order.
    pub fn links(&self) -> Vec<String> {
        (0..self.nodes.len()).filter(|&n| self.tag(n) == "a").filter_map(|n| self.attr(n, "href")).map(|s| s.to_string()).collect()
    }

    /// <meta name=robots content=noindex/nofollow>
    pub fn robots(&self) -> (bool, bool) {
        let mut index = true;
        let mut follow = true;
        for n in 0..self.nodes.len() {
            if self.tag(n) == "meta" && self.attr(n, "name").map_or(false, |v| v.eq_ignore_ascii_case("robots")) {
                let c = self.attr(n, "content").unwrap_or("").to_ascii_lowercase();
                index &= !c.contains("noindex");
                follow &= !c.contains("nofollow");
            }
        }
        (index, follow)
    }
}

// ---- character references ------------------------------------------------------------

const ENTITIES: [(&str, char); 36] = [
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("nbsp", '\u{a0}'),
    ("copy", '©'),
    ("reg", '®'),
    ("trade", '™'),
    ("mdash", '—'),
    ("ndash", '–'),
    ("hellip", '…'),
    ("laquo", '«'),
    ("raquo", '»'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("bull", '•'),
    ("middot", '·'),
    ("deg", '°'),
    ("plusmn", '±'),
    ("times", '×'),
    ("divide", '÷'),
    ("euro", '€'),
    ("pound", '£'),
    ("yen", '¥'),
    ("cent", '¢'),
    ("sect", '§'),
    ("para", '¶'),
    ("larr", '←'),
    ("rarr", '→'),
    ("check", '✓'),
    ("iexcl", '¡'),
    ("iquest", '¿'),
    ("shy", '\u{ad}'),
];

pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(k) = rest.find('&') {
        out.push_str(&rest[..k]);
        let r = &rest[k + 1..];
        let end = r.find(|c: char| !(c.is_ascii_alphanumeric() || c == '#')).unwrap_or(r.len());
        let name = &r[..end];
        let semi = r[end..].starts_with(';');
        let ch = if let Some(num) = name.strip_prefix('#') {
            let v = if let Some(h) = num.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok() } else { num.parse().ok() };
            v.and_then(char::from_u32).map(|c| if c == '\0' { '\u{fffd}' } else { c })
        } else {
            ENTITIES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
        };
        match ch {
            Some(c) if c == '\u{ad}' => rest = &r[end + semi as usize..],
            Some(c) => {
                out.push(c);
                rest = &r[end + semi as usize..];
            }
            None => {
                out.push('&');
                rest = r;
            }
        }
    }
    out.push_str(rest);
    out
}

// ---- tokenizer + tree builder ----------------------------------------------------------

fn parse_attrs(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        if ns == i {
            i += 1;
            continue;
        }
        let name = s[ns..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = decode_entities(&s[vs..i]);
            }
        }
        if !out.iter().any(|(k, _)| *k == name) {
            out.push((name, value));
        }
    }
    out
}

/// Elements whose start closes an open <p>.
const CLOSES_P: [&str; 26] = [
    "address", "article", "aside", "blockquote", "div", "dl", "fieldset", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hr", "main", "nav", "ol", "p", "pre", "section", "table", "ul", "figure",
];

pub fn parse(src: &str) -> Dom {
    let mut dom = Dom { nodes: vec![Node { kind: Kind::Document, parent: None, children: vec![] }] };
    let mut stack: Vec<NodeId> = vec![0];
    let add = |dom: &mut Dom, stack: &Vec<NodeId>, kind: Kind| -> NodeId {
        let parent = *stack.last().unwrap();
        let id = dom.nodes.len();
        dom.nodes.push(Node { kind, parent: Some(parent), children: vec![] });
        dom.nodes[parent].children.push(id);
        id
    };
    let text = |dom: &mut Dom, stack: &Vec<NodeId>, t: &str| {
        if t.is_empty() {
            return;
        }
        let parent = *stack.last().unwrap();
        // merge with a preceding text node
        if let Some(&last) = dom.nodes[parent].children.last() {
            if let Kind::Text(prev) = &mut dom.nodes[last].kind {
                prev.push_str(t);
                return;
            }
        }
        let id = dom.nodes.len();
        dom.nodes.push(Node { kind: Kind::Text(t.to_string()), parent: Some(parent), children: vec![] });
        dom.nodes[parent].children.push(id);
    };
    let open_tag = |dom: &Dom, stack: &Vec<NodeId>| -> String { dom.tag(*stack.last().unwrap()).to_string() };
    let close_to = |dom: &Dom, stack: &mut Vec<NodeId>, tag: &str, stop: &[&str]| -> bool {
        // pop up to and including the nearest open `tag`, unless a `stop` element comes first
        for k in (1..stack.len()).rev() {
            let t = dom.tag(stack[k]);
            if t == tag {
                stack.truncate(k);
                return true;
            }
            if stop.contains(&t) {
                return false;
            }
        }
        false
    };
    let mut i = 0;
    let b = src.as_bytes();
    let mut nodes_budget = 200_000usize;
    while i < b.len() && nodes_budget > 0 {
        if b[i] != b'<' {
            let e = src[i..].find('<').map(|k| i + k).unwrap_or(src.len());
            let t = decode_entities(&src[i..e]);
            text(&mut dom, &stack, &t);
            i = e;
            continue;
        }
        let rest = &src[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map(|k| k + 3).unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            i += rest.find('>').map(|k| k + 1).unwrap_or(rest.len());
            continue;
        }
        let closing = rest.starts_with("</");
        let name_start = if closing { 2 } else { 1 };
        let name_end = rest[name_start..].find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/').map(|k| k + name_start).unwrap_or(rest.len());
        let name = rest[name_start..name_end].to_ascii_lowercase();
        if name.is_empty() || !name.as_bytes()[0].is_ascii_alphabetic() {
            text(&mut dom, &stack, "<");
            i += 1;
            continue;
        }
        // find the end of the tag, respecting quotes
        let mut j = name_end;
        let rb = rest.as_bytes();
        let mut q = 0u8;
        while j < rb.len() {
            let c = rb[j];
            if q != 0 {
                if c == q {
                    q = 0;
                }
            } else if c == b'"' || c == b'\'' {
                q = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        let inner = &rest[name_end..j.min(rest.len())];
        i += (j + 1).min(rest.len());
        nodes_budget -= 1;
        if closing {
            match name.as_str() {
                "p" => {
                    if !close_to(&dom, &mut stack, "p", &["div", "td", "li", "body", "html"]) {
                        // </p> without <p>: an empty paragraph
                        let p = add(&mut dom, &stack, Kind::Element { tag: "p".into(), attrs: vec![] });
                        let _ = p;
                    }
                }
                "li" => {
                    close_to(&dom, &mut stack, "li", &["ul", "ol"]);
                }
                "td" | "th" => {
                    close_to(&dom, &mut stack, &name, &["tr", "table"]);
                }
                "tr" => {
                    close_to(&dom, &mut stack, "tr", &["table"]);
                }
                "body" | "html" => {}
                _ => {
                    close_to(&dom, &mut stack, &name, &[]);
                }
            }
            continue;
        }
        // implied end tags
        let cur = open_tag(&dom, &stack);
        if CLOSES_P.contains(&name.as_str()) || name == "li" && cur == "p" {
            close_to(&dom, &mut stack, "p", &["div", "td", "li", "body", "html", "button"]);
        }
        match name.as_str() {
            "li" => {
                close_to(&dom, &mut stack, "li", &["ul", "ol", "div", "table"]);
            }
            "dt" | "dd" => {
                if !close_to(&dom, &mut stack, "dt", &["dl"]) {
                    close_to(&dom, &mut stack, "dd", &["dl"]);
                }
            }
            "tr" => {
                close_to(&dom, &mut stack, "tr", &["table", "tbody", "thead", "tfoot"]);
            }
            "td" | "th" => {
                if !close_to(&dom, &mut stack, "td", &["tr", "table"]) {
                    close_to(&dom, &mut stack, "th", &["tr", "table"]);
                }
            }
            "option" => {
                close_to(&dom, &mut stack, "option", &["select"]);
            }
            _ => {}
        }
        let attrs = parse_attrs(inner);
        let id = add(&mut dom, &stack, Kind::Element { tag: name.clone(), attrs });
        let self_closing = inner.trim_end().ends_with('/');
        if VOID.contains(&name.as_str()) || (self_closing && !RAW.contains(&name.as_str())) {
            continue;
        }
        if RAW.contains(&name.as_str()) {
            // raw text up to the matching end tag
            let lower = src[i..].to_ascii_lowercase();
            let end = lower.find(&alloc::format!("</{}", name)).map(|k| i + k).unwrap_or(src.len());
            let raw = &src[i..end];
            let t = if name == "title" || name == "textarea" { decode_entities(raw) } else { raw.to_string() };
            if !t.is_empty() {
                let tid = dom.nodes.len();
                dom.nodes.push(Node { kind: Kind::Text(t), parent: Some(id), children: vec![] });
                dom.nodes[id].children.push(tid);
            }
            i = end;
            if let Some(k) = src[i..].find('>') {
                i += k + 1;
            }
            continue;
        }
        stack.push(id);
    }
    dom
}
