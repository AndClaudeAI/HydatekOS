//! Rich-text documents for Hyda Scripts: paragraphs with a style and
//! alignment, characters with bold / italic / underline / strikethrough, the
//! editing operations, and conversion to and from Word (.docx), plain text and
//! Markdown.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const BOLD: u8 = 1;
pub const ITALIC: u8 = 2;
pub const UNDERLINE: u8 = 4;
pub const STRIKE: u8 = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Body,
    Title,
    H1,
    H2,
    Quote,
    Bullet,
    Number,
}

pub const STYLES: [Style; 7] = [Style::Body, Style::Title, Style::H1, Style::H2, Style::Quote, Style::Bullet, Style::Number];

impl Style {
    pub fn name(self) -> &'static str {
        match self {
            Style::Body => "Body",
            Style::Title => "Title",
            Style::H1 => "Heading 1",
            Style::H2 => "Heading 2",
            Style::Quote => "Quote",
            Style::Bullet => "Bulleted list",
            Style::Number => "Numbered list",
        }
    }
    pub fn heading(self) -> bool {
        matches!(self, Style::Title | Style::H1 | Style::H2)
    }
    pub fn list(self) -> bool {
        matches!(self, Style::Bullet | Style::Number)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Para {
    pub text: Vec<char>,
    /// formatting bits per character
    pub fmt: Vec<u8>,
    pub style: Style,
    pub align: Align,
}

impl Para {
    pub fn new(style: Style) -> Para {
        Para { text: Vec::new(), fmt: Vec::new(), style, align: Align::Left }
    }
    pub fn plain(s: &str, style: Style) -> Para {
        let text: Vec<char> = s.chars().collect();
        Para { fmt: vec![0; text.len()], text, style, align: Align::Left }
    }
    fn push(&mut self, s: &str, f: u8) {
        for c in s.chars() {
            self.text.push(c);
            self.fmt.push(f);
        }
    }
    pub fn len(&self) -> usize {
        self.text.len()
    }
    pub fn string(&self) -> String {
        self.text.iter().collect()
    }
}

/// A position: paragraph index and character index within it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub p: usize,
    pub i: usize,
}

impl Pos {
    pub fn new(p: usize, i: usize) -> Pos {
        Pos { p, i }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub paras: Vec<Para>,
}

impl Default for Doc {
    fn default() -> Doc {
        Doc::new()
    }
}

impl Doc {
    pub fn new() -> Doc {
        Doc { paras: vec![Para::new(Style::Body)] }
    }

    pub fn end(&self) -> Pos {
        let p = self.paras.len() - 1;
        Pos::new(p, self.paras[p].len())
    }

    pub fn clamp(&self, pos: Pos) -> Pos {
        let p = pos.p.min(self.paras.len() - 1);
        Pos::new(p, pos.i.min(self.paras[p].len()))
    }

    pub fn words(&self) -> usize {
        self.paras.iter().map(|p| p.string().split_whitespace().count()).sum()
    }

    /// Formatting a character typed at `pos` would get (from the character before).
    pub fn fmt_at(&self, pos: Pos) -> u8 {
        let p = &self.paras[pos.p];
        if pos.i > 0 {
            p.fmt[pos.i - 1]
        } else {
            p.fmt.first().copied().unwrap_or(0)
        }
    }

    /// Insert text (a `\n` starts a new paragraph); returns the position after it.
    pub fn insert(&mut self, mut pos: Pos, s: &str, f: u8) -> Pos {
        for c in s.chars() {
            if c == '\n' {
                pos = self.split(pos);
            } else if c != '\r' {
                let p = &mut self.paras[pos.p];
                p.text.insert(pos.i, c);
                p.fmt.insert(pos.i, f);
                pos.i += 1;
            }
        }
        pos
    }

    /// Split the paragraph at `pos` (Enter). Ending a heading starts body text.
    pub fn split(&mut self, pos: Pos) -> Pos {
        let p = &mut self.paras[pos.p];
        let text = p.text.split_off(pos.i);
        let fmt = p.fmt.split_off(pos.i);
        let style = if p.style.heading() && text.is_empty() { Style::Body } else { p.style };
        let np = Para { text, fmt, style, align: if style == p.style { p.align } else { Align::Left } };
        self.paras.insert(pos.p + 1, np);
        Pos::new(pos.p + 1, 0)
    }

    /// Delete the range [a, b).
    pub fn delete(&mut self, a: Pos, b: Pos) {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a == b {
            return;
        }
        if a.p == b.p {
            let p = &mut self.paras[a.p];
            p.text.drain(a.i..b.i);
            p.fmt.drain(a.i..b.i);
            return;
        }
        let tail_t: Vec<char> = self.paras[b.p].text[b.i..].to_vec();
        let tail_f: Vec<u8> = self.paras[b.p].fmt[b.i..].to_vec();
        let first = &mut self.paras[a.p];
        first.text.truncate(a.i);
        first.fmt.truncate(a.i);
        first.text.extend(tail_t);
        first.fmt.extend(tail_f);
        self.paras.drain(a.p + 1..=b.p);
    }

    /// Call `f` on the formatting of every character in [a, b).
    fn each_fmt(&mut self, a: Pos, b: Pos, mut f: impl FnMut(&mut u8)) {
        for pi in a.p..=b.p.min(self.paras.len() - 1) {
            let p = &mut self.paras[pi];
            let s = if pi == a.p { a.i } else { 0 };
            let e = if pi == b.p { b.i } else { p.len() };
            for x in p.fmt[s.min(e)..e].iter_mut() {
                f(x);
            }
        }
    }

    pub fn set_fmt(&mut self, a: Pos, b: Pos, bit: u8, on: bool) {
        self.each_fmt(a, b, |x| if on { *x |= bit } else { *x &= !bit });
    }

    /// Every character in [a, b) has `bit` set (and there is at least one).
    pub fn all_have(&mut self, a: Pos, b: Pos, bit: u8) -> bool {
        let (mut all, mut any) = (true, false);
        self.each_fmt(a, b, |x| {
            any = true;
            all &= *x & bit != 0;
        });
        all && any
    }

    /// The paragraphs of [a, b) (the first and last cut to the range).
    pub fn slice(&self, a: Pos, b: Pos) -> Vec<Para> {
        let mut out = Vec::new();
        for pi in a.p..=b.p {
            let p = &self.paras[pi];
            let s = if pi == a.p { a.i } else { 0 };
            let e = if pi == b.p { b.i } else { p.len() };
            out.push(Para { text: p.text[s..e].to_vec(), fmt: p.fmt[s..e].to_vec(), style: p.style, align: p.align });
        }
        out
    }

    pub fn plain(paras: &[Para]) -> String {
        let mut s = String::new();
        for (i, p) in paras.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.extend(p.text.iter());
        }
        s
    }

    /// Paste paragraphs at `pos`; returns the position after them.
    pub fn paste(&mut self, pos: Pos, frag: &[Para]) -> Pos {
        if frag.is_empty() {
            return pos;
        }
        if frag.len() == 1 {
            let p = &mut self.paras[pos.p];
            for (k, (&c, &f)) in frag[0].text.iter().zip(frag[0].fmt.iter()).enumerate() {
                p.text.insert(pos.i + k, c);
                p.fmt.insert(pos.i + k, f);
            }
            return Pos::new(pos.p, pos.i + frag[0].len());
        }
        let right = self.split(pos);
        {
            let first = &mut self.paras[pos.p];
            first.text.extend(frag[0].text.iter());
            first.fmt.extend(frag[0].fmt.iter());
            if pos.i == 0 {
                first.style = frag[0].style;
                first.align = frag[0].align;
            }
        }
        let mid = &frag[1..frag.len() - 1];
        for (k, p) in mid.iter().enumerate() {
            self.paras.insert(right.p + k, p.clone());
        }
        let lp = right.p + mid.len();
        let last = &frag[frag.len() - 1];
        let tail = &mut self.paras[lp];
        for (k, (&c, &f)) in last.text.iter().zip(last.fmt.iter()).enumerate() {
            tail.text.insert(k, c);
            tail.fmt.insert(k, f);
        }
        if tail.text.len() == last.text.len() {
            tail.style = last.style;
            tail.align = last.align;
        }
        Pos::new(lp, last.len())
    }

    /// Word boundaries around `pos` (for double-click selection).
    pub fn word_at(&self, pos: Pos) -> (Pos, Pos) {
        let t = &self.paras[pos.p].text;
        let is_w = |c: char| c.is_alphanumeric() || c == '\'' || c == '_';
        let (mut s, mut e) = (pos.i.min(t.len()), pos.i.min(t.len()));
        while s > 0 && is_w(t[s - 1]) {
            s -= 1;
        }
        while e < t.len() && is_w(t[e]) {
            e += 1;
        }
        (Pos::new(pos.p, s), Pos::new(pos.p, e))
    }

    // ---- Hyda Scripts document (.hyds) ------------------------------------------
    //
    // The native format: UTF-8 text, one record per line.
    //
    //   HYDS 1                      magic and format version
    //   app Hyda Scripts            written by (informational)
    //   paras 3                     paragraph count
    //   p title left                paragraph: style, alignment
    //   t Hello \\ world\t!          its text (\\ = backslash, \t = tab)
    //   f 0:5:b 6:5:iu              formatting runs start:length:flags (b i u s)
    //   ...
    //   end 1a2b3c4d                CRC-32 (hex) of every byte before this line
    //
    // Readers skip record types they don't know, so later versions can add some.

    pub fn to_hyds(&self) -> String {
        let mut out = String::from("HYDS 1\napp Hyda Scripts\n");
        out.push_str(&format!("paras {}\n", self.paras.len()));
        for p in &self.paras {
            let style = match p.style {
                Style::Body => "body",
                Style::Title => "title",
                Style::H1 => "h1",
                Style::H2 => "h2",
                Style::Quote => "quote",
                Style::Bullet => "bullet",
                Style::Number => "number",
            };
            let align = match p.align {
                Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
            };
            out.push_str(&format!("p {} {}\nt ", style, align));
            for &c in &p.text {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\t' => out.push_str("\\t"),
                    // keeps character positions (and so the "f" runs) aligned
                    c if c.is_control() => out.push(' '),
                    c => out.push(c),
                }
            }
            out.push('\n');
            let mut runs = Vec::new();
            let mut i = 0;
            while i < p.len() {
                let f = p.fmt[i] & (BOLD | ITALIC | UNDERLINE | STRIKE);
                let mut j = i;
                while j < p.len() && p.fmt[j] & (BOLD | ITALIC | UNDERLINE | STRIKE) == f {
                    j += 1;
                }
                if f != 0 {
                    let mut fl = String::new();
                    for (bit, ch) in [(BOLD, 'b'), (ITALIC, 'i'), (UNDERLINE, 'u'), (STRIKE, 's')] {
                        if f & bit != 0 {
                            fl.push(ch);
                        }
                    }
                    runs.push(format!("{}:{}:{}", i, j - i, fl));
                }
                i = j;
            }
            if !runs.is_empty() {
                out.push_str(&format!("f {}\n", runs.join(" ")));
            }
        }
        let crc = crate::zip::crc32(out.as_bytes());
        out.push_str(&format!("end {:08x}\n", crc));
        out
    }

    pub fn from_hyds(data: &[u8]) -> Result<Doc, &'static str> {
        let s = core::str::from_utf8(data).map_err(|_| "not a Hyda Scripts document")?;
        let rest = s.strip_prefix("HYDS ").ok_or("not a Hyda Scripts document")?;
        let ver: u32 = rest.split('\n').next().unwrap_or("").trim().parse().map_err(|_| "not a Hyda Scripts document")?;
        if ver != 1 {
            return Err("made by a newer Hyda Scripts");
        }
        // checksum over everything before the "end" line
        let end_at = s.rfind("\nend ").ok_or("the file is incomplete")? + 1;
        let want = u32::from_str_radix(s[end_at + 4..].trim(), 16).map_err(|_| "the file is damaged")?;
        if crate::zip::crc32(&data[..end_at]) != want {
            return Err("the file is damaged");
        }
        let mut paras: Vec<Para> = Vec::new();
        let mut count = None;
        for line in s[..end_at].lines().skip(1) {
            let (tag, val) = line.split_once(' ').unwrap_or((line, ""));
            match tag {
                "paras" => count = val.parse::<usize>().ok(),
                "p" => {
                    let mut it = val.split(' ');
                    let style = match it.next().unwrap_or("") {
                        "title" => Style::Title,
                        "h1" => Style::H1,
                        "h2" => Style::H2,
                        "quote" => Style::Quote,
                        "bullet" => Style::Bullet,
                        "number" => Style::Number,
                        _ => Style::Body,
                    };
                    let align = match it.next().unwrap_or("") {
                        "center" => Align::Center,
                        "right" => Align::Right,
                        _ => Align::Left,
                    };
                    let mut p = Para::new(style);
                    p.align = align;
                    paras.push(p);
                }
                "t" => {
                    let p = paras.last_mut().ok_or("the file is damaged")?;
                    let mut cs = val.chars();
                    while let Some(c) = cs.next() {
                        let c = if c == '\\' {
                            match cs.next() {
                                Some('t') => '\t',
                                Some(o) => o,
                                None => break,
                            }
                        } else {
                            c
                        };
                        p.text.push(c);
                        p.fmt.push(0);
                    }
                }
                "f" => {
                    let p = paras.last_mut().ok_or("the file is damaged")?;
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
                _ => {} // app, or a record from a later version
            }
        }
        if count.map_or(false, |c| c != paras.len()) {
            return Err("the file is damaged");
        }
        if paras.is_empty() {
            paras.push(Para::new(Style::Body));
        }
        Ok(Doc { paras })
    }

    // ---- plain text and Markdown ---------------------------------------------

    pub fn from_text(s: &str) -> Doc {
        // saving ends every paragraph with a newline; drop the final one
        let s = s.strip_suffix('\n').unwrap_or(s);
        let paras: Vec<Para> = s.split('\n').map(|l| Para::plain(l.trim_end_matches('\r'), Style::Body)).collect();
        Doc { paras }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let mut n = 0;
        for p in &self.paras {
            n = if p.style == Style::Number { n + 1 } else { 0 };
            match p.style {
                Style::Bullet => out.push_str("• "),
                Style::Number => out.push_str(&format!("{}. ", n)),
                _ => {}
            }
            out.extend(p.text.iter());
            out.push('\n');
        }
        out
    }

    pub fn from_markdown(s: &str) -> Doc {
        let s = s.strip_suffix('\n').unwrap_or(s);
        let mut paras = Vec::new();
        for line in s.split('\n') {
            let line = line.trim_end_matches('\r');
            let (style, rest) = if let Some(r) = line.strip_prefix("### ") {
                (Style::H2, r)
            } else if let Some(r) = line.strip_prefix("## ") {
                (Style::H1, r)
            } else if let Some(r) = line.strip_prefix("# ") {
                (Style::Title, r)
            } else if let Some(r) = line.strip_prefix("> ") {
                (Style::Quote, r)
            } else if let Some(r) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).or_else(|| line.strip_prefix("• ")) {
                (Style::Bullet, r)
            } else if let Some(k) = line.find(". ").filter(|&k| k > 0 && k < 4 && line[..k].bytes().all(|b| b.is_ascii_digit())) {
                (Style::Number, &line[k + 2..])
            } else {
                (Style::Body, line)
            };
            let mut p = Para::new(style);
            // inline **bold**, *italic* / _italic_, ~~strike~~
            let mut f = 0u8;
            let cs: Vec<char> = rest.chars().collect();
            let mut i = 0;
            while i < cs.len() {
                let two = |a: char| i + 1 < cs.len() && cs[i] == a && cs[i + 1] == a;
                if two('*') || two('_') {
                    f ^= BOLD;
                    i += 2;
                } else if two('~') {
                    f ^= STRIKE;
                    i += 2;
                } else if (cs[i] == '*' || cs[i] == '_') && (f & ITALIC != 0 || cs.get(i + 1).map_or(false, |c| !c.is_whitespace())) {
                    f ^= ITALIC;
                    i += 1;
                } else if cs[i] == '\\' && i + 1 < cs.len() {
                    p.text.push(cs[i + 1]);
                    p.fmt.push(f);
                    i += 2;
                } else {
                    p.text.push(cs[i]);
                    p.fmt.push(f);
                    i += 1;
                }
            }
            paras.push(p);
        }
        if paras.is_empty() {
            paras.push(Para::new(Style::Body));
        }
        Doc { paras }
    }

    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let mut n = 0;
        for p in &self.paras {
            n = if p.style == Style::Number { n + 1 } else { 0 };
            out.push_str(&match p.style {
                Style::Title => "# ".to_string(),
                Style::H1 => "## ".to_string(),
                Style::H2 => "### ".to_string(),
                Style::Quote => "> ".to_string(),
                Style::Bullet => "- ".to_string(),
                Style::Number => format!("{}. ", n),
                Style::Body => String::new(),
            });
            let mut cur = 0u8;
            let marks = |on: u8, out: &mut String| {
                if on & BOLD != 0 {
                    out.push_str("**");
                }
                if on & ITALIC != 0 {
                    out.push('*');
                }
                if on & STRIKE != 0 {
                    out.push_str("~~");
                }
            };
            let close = |off: u8, out: &mut String| {
                if off & STRIKE != 0 {
                    out.push_str("~~");
                }
                if off & ITALIC != 0 {
                    out.push('*');
                }
                if off & BOLD != 0 {
                    out.push_str("**");
                }
            };
            for (&c, &f) in p.text.iter().zip(p.fmt.iter()) {
                let f = f & (BOLD | ITALIC | STRIKE);
                if f != cur {
                    close(cur & !f, &mut out);
                    marks(f & !cur, &mut out);
                    cur = f;
                }
                if matches!(c, '*' | '_' | '~' | '\\') {
                    out.push('\\');
                }
                out.push(c);
            }
            close(cur, &mut out);
            out.push('\n');
        }
        out
    }

    // ---- Word (.docx) -------------------------------------------------------------

    pub fn to_docx(&self) -> Vec<u8> {
        let mut body = String::new();
        // bullets share list 1; each run of numbered paragraphs gets its own
        // list (2, 3, ...) so numbering restarts like it does on screen
        let mut numbered = 0u32;
        let mut prev_number = false;
        for p in &self.paras {
            body.push_str("<w:p>");
            let mut ppr = String::new();
            let sid = match p.style {
                Style::Body => "",
                Style::Title => "Title",
                Style::H1 => "Heading1",
                Style::H2 => "Heading2",
                Style::Quote => "Quote",
                Style::Bullet | Style::Number => "ListParagraph",
            };
            if !sid.is_empty() {
                ppr.push_str(&format!("<w:pStyle w:val=\"{}\"/>", sid));
            }
            if p.style.list() {
                let id = if p.style == Style::Bullet {
                    1
                } else {
                    if !prev_number {
                        numbered += 1;
                    }
                    1 + numbered
                };
                ppr.push_str(&format!("<w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"{}\"/></w:numPr>", id));
            }
            prev_number = p.style == Style::Number;
            match p.align {
                Align::Left => {}
                Align::Center => ppr.push_str("<w:jc w:val=\"center\"/>"),
                Align::Right => ppr.push_str("<w:jc w:val=\"right\"/>"),
            }
            if !ppr.is_empty() {
                body.push_str(&format!("<w:pPr>{}</w:pPr>", ppr));
            }
            let mut i = 0;
            while i < p.len() {
                let f = p.fmt[i];
                let mut j = i;
                while j < p.len() && p.fmt[j] == f {
                    j += 1;
                }
                body.push_str("<w:r>");
                if f != 0 {
                    body.push_str("<w:rPr>");
                    if f & BOLD != 0 {
                        body.push_str("<w:b/>");
                    }
                    if f & ITALIC != 0 {
                        body.push_str("<w:i/>");
                    }
                    if f & STRIKE != 0 {
                        body.push_str("<w:strike/>");
                    }
                    if f & UNDERLINE != 0 {
                        body.push_str("<w:u w:val=\"single\"/>");
                    }
                    body.push_str("</w:rPr>");
                }
                let text: String = p.text[i..j].iter().collect();
                // tabs are their own element in Word
                for (k, part) in text.split('\t').enumerate() {
                    if k > 0 {
                        body.push_str("<w:tab/>");
                    }
                    if !part.is_empty() {
                        body.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(part)));
                    }
                }
                body.push_str("</w:r>");
                i = j;
            }
            body.push_str("</w:p>");
        }
        let doc = format!(
            "{}<w:document xmlns:w=\"{}\"><w:body>{}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>",
            XML_HEAD, W_NS, body
        );
        let mut nums = String::from("<w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num>");
        for k in 0..numbered {
            nums.push_str(&format!(
                "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"1\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"1\"/></w:lvlOverride></w:num>",
                k + 2
            ));
        }
        let numbering = format!("{}<w:numbering xmlns:w=\"{}\">{}{}</w:numbering>", XML_HEAD, W_NS, ABSTRACT_NUMS, nums);
        let mut z = crate::zip::Writer::new();
        z.add("[Content_Types].xml", CONTENT_TYPES.as_bytes());
        z.add("_rels/.rels", RELS.as_bytes());
        z.add("docProps/app.xml", APP_XML.as_bytes());
        z.add("word/document.xml", doc.as_bytes());
        z.add("word/styles.xml", STYLES_XML.as_bytes());
        z.add("word/numbering.xml", numbering.as_bytes());
        z.add("word/_rels/document.xml.rels", DOC_RELS.as_bytes());
        z.finish()
    }

    pub fn from_docx(data: &[u8]) -> Option<Doc> {
        let xml = crate::zip::read(data, "word/document.xml")?;
        let xml = String::from_utf8_lossy(&xml);
        let bullets = crate::zip::read(data, "word/numbering.xml").map(|n| bullet_lists(&String::from_utf8_lossy(&n))).unwrap_or_default();
        let style_names = crate::zip::read(data, "word/styles.xml").map(|s| style_names(&String::from_utf8_lossy(&s))).unwrap_or_default();
        let mut paras = Vec::new();
        let mut para: Option<Para> = None;
        let (mut in_ppr, mut in_rpr, mut in_t) = (false, false, false);
        let mut run_fmt = 0u8;
        let mut list: Option<bool> = None; // Some(bullet?)
        let mut pstyle = String::new();
        for tok in Tokens::new(&xml) {
            match tok {
                Tok::Open(tag, attrs, empty) => match tag {
                    "w:p" => {
                        para = Some(Para::new(Style::Body));
                        list = None;
                        pstyle.clear();
                        if empty {
                            paras.push(para.take().unwrap());
                        }
                    }
                    "w:pPr" if !empty => in_ppr = true,
                    "w:rPr" if !empty => in_rpr = true,
                    "w:pStyle" if in_ppr => pstyle = attr(attrs, "w:val").to_string(),
                    "w:jc" if in_ppr => {
                        if let Some(p) = para.as_mut() {
                            p.align = match attr(attrs, "w:val") {
                                "center" => Align::Center,
                                "right" | "end" => Align::Right,
                                _ => Align::Left,
                            };
                        }
                    }
                    "w:numId" if in_ppr => {
                        let id = attr(attrs, "w:val");
                        if id != "0" && !id.is_empty() {
                            list = Some(bullets.get(id).copied().unwrap_or(true));
                        }
                    }
                    "w:r" => run_fmt = 0,
                    "w:b" | "w:i" | "w:strike" | "w:u" if in_rpr => {
                        let v = attr(attrs, "w:val");
                        let off = matches!(v, "0" | "false" | "none");
                        let bit = match tag {
                            "w:b" => BOLD,
                            "w:i" => ITALIC,
                            "w:strike" => STRIKE,
                            _ => UNDERLINE,
                        };
                        if !off {
                            run_fmt |= bit;
                        }
                    }
                    "w:t" if !empty => in_t = true,
                    "w:tab" if !in_ppr => {
                        if let Some(p) = para.as_mut() {
                            p.push("\t", run_fmt);
                        }
                    }
                    "w:br" | "w:cr" => {
                        // a line break inside a paragraph: continue as a new paragraph
                        if let Some(p) = para.take() {
                            let (style, align) = (p.style, p.align);
                            paras.push(p);
                            let mut np = Para::new(style);
                            np.align = align;
                            para = Some(np);
                        }
                    }
                    _ => {}
                },
                Tok::Close(tag) => match tag {
                    "w:pPr" => {
                        in_ppr = false;
                        if let Some(p) = para.as_mut() {
                            let name = style_names.get(pstyle.as_str()).cloned().unwrap_or_else(|| pstyle.clone());
                            p.style = map_style(&name, list);
                        }
                    }
                    "w:rPr" => in_rpr = false,
                    "w:t" => in_t = false,
                    "w:p" => {
                        if let Some(mut p) = para.take() {
                            if p.style == Style::Body && list.is_some() {
                                p.style = map_style("", list);
                            }
                            paras.push(p);
                        }
                    }
                    _ => {}
                },
                Tok::Text(t) => {
                    if in_t {
                        if let Some(p) = para.as_mut() {
                            p.push(&unesc(t), run_fmt);
                        }
                    }
                }
            }
        }
        if paras.is_empty() {
            paras.push(Para::new(Style::Body));
        }
        Some(Doc { paras })
    }
}

fn map_style(name: &str, list: Option<bool>) -> Style {
    let n: String = name.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    if n == "title" {
        Style::Title
    } else if n == "heading1" {
        Style::H1
    } else if n.starts_with("heading") || n == "subtitle" {
        Style::H2
    } else if n.contains("quot") {
        Style::Quote
    } else if n.contains("listbullet") {
        Style::Bullet
    } else if n.contains("listnumber") {
        Style::Number
    } else {
        match list {
            Some(true) => Style::Bullet,
            Some(false) => Style::Number,
            None => Style::Body,
        }
    }
}

/// numbering.xml: which list ids (numId) are bulleted (vs numbered).
fn bullet_lists(xml: &str) -> BTreeMap<String, bool> {
    let mut abs_fmt: BTreeMap<String, bool> = BTreeMap::new();
    let mut num_abs: Vec<(String, String)> = Vec::new();
    let (mut cur_abs, mut lvl0, mut cur_num) = (String::new(), false, String::new());
    for tok in Tokens::new(xml) {
        match tok {
            Tok::Open("w:abstractNum", a, _) => cur_abs = attr(a, "w:abstractNumId").to_string(),
            Tok::Open("w:lvl", a, _) => lvl0 = attr(a, "w:ilvl") == "0",
            Tok::Open("w:numFmt", a, _) if lvl0 && !cur_abs.is_empty() => {
                abs_fmt.entry(cur_abs.clone()).or_insert(attr(a, "w:val") == "bullet");
            }
            Tok::Open("w:num", a, _) => cur_num = attr(a, "w:numId").to_string(),
            Tok::Open("w:abstractNumId", a, _) if !cur_num.is_empty() => num_abs.push((cur_num.clone(), attr(a, "w:val").to_string())),
            Tok::Close("w:abstractNum") => cur_abs.clear(),
            Tok::Close("w:num") => cur_num.clear(),
            _ => {}
        }
    }
    num_abs.into_iter().map(|(n, a)| (n, abs_fmt.get(&a).copied().unwrap_or(true))).collect()
}

/// styles.xml: style id -> display name (ids are localised in some writers).
fn style_names(xml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut id = String::new();
    for tok in Tokens::new(xml) {
        match tok {
            Tok::Open("w:style", a, _) => id = attr(a, "w:styleId").to_string(),
            Tok::Open("w:name", a, _) if !id.is_empty() => {
                out.insert(id.clone(), attr(a, "w:val").to_string());
            }
            Tok::Close("w:style") => id.clear(),
            _ => {}
        }
    }
    out
}

// ---- a very small XML tokenizer ----------------------------------------------------

enum Tok<'a> {
    /// name, raw attribute text, self-closing
    Open(&'a str, &'a str, bool),
    Close(&'a str),
    Text(&'a str),
}

struct Tokens<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> Tokens<'a> {
    fn new(s: &'a str) -> Tokens<'a> {
        Tokens { s, i: 0 }
    }
}

impl<'a> Iterator for Tokens<'a> {
    type Item = Tok<'a>;
    fn next(&mut self) -> Option<Tok<'a>> {
        let s = self.s;
        loop {
            if self.i >= s.len() {
                return None;
            }
            if s.as_bytes()[self.i] != b'<' {
                let e = s[self.i..].find('<').map(|k| self.i + k).unwrap_or(s.len());
                let t = &s[self.i..e];
                self.i = e;
                return Some(Tok::Text(t));
            }
            let rest = &s[self.i..];
            if rest.starts_with("<?") || rest.starts_with("<!") {
                let end = if rest.starts_with("<!--") { rest.find("-->").map(|k| k + 3) } else { rest.find('>').map(|k| k + 1) };
                self.i += end.unwrap_or(rest.len());
                continue;
            }
            let end = rest.find('>').unwrap_or(rest.len() - 1);
            let inner = &rest[1..end];
            self.i += end + 1;
            if let Some(name) = inner.strip_prefix('/') {
                return Some(Tok::Close(name.trim()));
            }
            let empty = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let (name, attrs) = match inner.find(|c: char| c.is_whitespace()) {
                Some(k) => (&inner[..k], &inner[k..]),
                None => (inner, ""),
            };
            return Some(Tok::Open(name, attrs, empty));
        }
    }
}

fn attr<'a>(attrs: &'a str, name: &str) -> &'a str {
    let mut rest = attrs;
    while let Some(k) = rest.find(name) {
        let before_ok = k == 0 || rest.as_bytes()[k - 1].is_ascii_whitespace();
        let after = &rest[k + name.len()..];
        let after_t = after.trim_start();
        if before_ok && after_t.starts_with('=') {
            let v = after_t[1..].trim_start();
            if let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') {
                let v = &v[1..];
                return &v[..v.find(q).unwrap_or(v.len())];
            }
        }
        rest = after;
    }
    ""
}

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 => {}
            c => o.push(c),
        }
    }
    o
}

fn unesc(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(k) = rest.find('&') {
        o.push_str(&rest[..k]);
        let r = &rest[k..];
        let Some(semi) = r.find(';').filter(|&e| e < 12) else {
            o.push('&');
            rest = &r[1..];
            continue;
        };
        let ent = &r[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => o.push(c),
            None => o.push_str(&r[..=semi]),
        }
        rest = &r[semi + 1..];
    }
    o.push_str(rest);
    o
}

const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>";

const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>";

const DOC_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/></Relationships>";

const APP_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Hyda Scripts</Application></Properties>";

const ABSTRACT_NUMS: &str = "<w:abstractNum w:abstractNumId=\"0\"><w:multiLevelType w:val=\"singleLevel\"/><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"\u{2022}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId=\"1\"><w:multiLevelType w:val=\"singleLevel\"/><w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"720\" w:hanging=\"360\"/></w:pPr></w:lvl></w:abstractNum>";

const STYLES_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Figtree\" w:hAnsi=\"Figtree\" w:eastAsia=\"Figtree\" w:cs=\"Figtree\"/><w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"en-GB\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"300\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:after=\"160\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"52\"/><w:szCs w:val=\"52\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"80\"/><w:outlineLvl w:val=\"0\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"32\"/><w:szCs w:val=\"32\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"200\" w:after=\"60\"/><w:outlineLvl w:val=\"1\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"26\"/><w:szCs w:val=\"26\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"Quote\"><w:name w:val=\"Quote\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:ind w:left=\"567\"/><w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"12\" w:color=\"B5581B\"/></w:pBdr></w:pPr><w:rPr><w:i/><w:color w:val=\"5E5866\"/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"720\"/></w:pPr></w:style>\
</w:styles>";
