//! Hyda Scripts, the Hyda Workspace word processor: A4 pages, paragraph
//! styles, bold / italic / underline / strikethrough, alignment, lists, undo,
//! cut / copy / paste, and Word (.docx) files (also .txt and .md).

use super::{App, AppKind, LineEdit, HEADER};
use crate::doc::{Align, Doc, Para, Pos, Style, BOLD, ITALIC, STRIKE, STYLES, UNDERLINE};
use crate::font::{self, Face};
use crate::fs::{basename, join, parent};
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// A4 in points; the page is drawn at `zoom` percent where 100% = 96 dpi
const PAGE_W: i32 = 595;
const PAGE_H: i32 = 842;
const MARGIN: i32 = 72;
const GAP: i32 = 24;
const TOOLBAR: i32 = 46;
const STATUS: i32 = 28;
const PAPER: Color = Color::rgb(0xFFFFFF);
const INK: Color = Color::rgb(0x1E1B2C);
const INK2: Color = Color::rgb(0x5E5866);

const C_PAGE: u32 = 1;
const C_STYLE_MENU: u32 = 2;
const C_BOLD: u32 = 3;
const C_ITALIC: u32 = 4;
const C_UNDERLINE: u32 = 5;
const C_STRIKE: u32 = 6;
const C_LEFT: u32 = 7;
const C_CENTER: u32 = 8;
const C_RIGHT: u32 = 9;
const C_BULLET: u32 = 10;
const C_NUMBER: u32 = 11;
const C_UNDO: u32 = 12;
const C_REDO: u32 = 13;
const C_ZOOM_IN: u32 = 14;
const C_ZOOM_OUT: u32 = 15;
const C_NEW: u32 = 16;
const C_OPEN: u32 = 17;
const C_SAVE: u32 = 18;
const C_RENAME: u32 = 19;
const C_CUT: u32 = 20;
const C_COPY: u32 = 21;
const C_PASTE: u32 = 22;
const C_ALL: u32 = 23;
const C_EXPORT_TXT: u32 = 24;
const C_EXPORT_MD: u32 = 25;
const C_SAVE_DOCX: u32 = 26;
const C_DISMISS: u32 = 27;
const C_FIT: u32 = 28;
const C_STYLE: u32 = 100;
const C_FILE: u32 = 200;

/// One laid-out line of a paragraph.
struct Line {
    p: usize,
    s: usize,
    e: usize,
    /// left edge relative to the content box, device px * 64
    x64: i32,
    /// top, logical, from the top of the first page
    y: i32,
    h: i32,
    /// baseline offset from `y`
    base: i32,
    page: usize,
}

enum Overlay {
    None,
    Styles,
    Open(Vec<String>),
    Rename(LineEdit),
}

pub struct Scripts {
    doc: Doc,
    path: String,
    dirty: bool,
    cur: Pos,
    anchor: Option<Pos>,
    /// formatting for the next typed character (after B/I/U with no selection)
    pending: Option<u8>,
    undo: Vec<(Doc, Pos)>,
    redo: Vec<(Doc, Pos)>,
    typing: bool,
    clip: Vec<Para>,
    /// percent; 0 = fit the page to the window width
    zoom: i32,
    eff_zoom: i32,
    scroll: i32,
    lines: Vec<Line>,
    pages: usize,
    layout_key: (u64, i32, i32),
    ver: u64,
    area: Rect,
    page_x: i32,
    scale: i32,
    mouse: (i32, i32),
    press: Option<Pos>,
    want_visible: bool,
    goal_x: Option<i32>,
    overlay: Overlay,
    /// opened from a Word file another program made: the first save goes to a
    /// copy, so what Hyda Scripts can't show yet (images, tables...) survives
    foreign: bool,
}

/// Font size (points), base weight, space before/after (points), line height %.
fn metrics(s: Style) -> (i32, bool, i32, i32, i32) {
    match s {
        Style::Body => (11, false, 0, 8, 140),
        Style::Title => (26, true, 0, 10, 115),
        Style::H1 => (16, true, 14, 4, 125),
        Style::H2 => (13, true, 10, 3, 125),
        Style::Quote => (11, false, 4, 8, 140),
        Style::Bullet | Style::Number => (11, false, 0, 3, 140),
    }
}

fn face_for(style: Style, f: u8) -> Face {
    let bold = style.heading() || f & BOLD != 0;
    let italic = style == Style::Quote || f & ITALIC != 0;
    match (bold, italic) {
        (false, false) => Face::Regular,
        (true, false) => Face::Semibold,
        (false, true) => Face::Italic,
        (true, true) => Face::SemiboldItalic,
    }
}

/// Paragraph indent (points) for lists and quotes.
fn indent_pt(s: Style) -> i32 {
    match s {
        Style::Bullet | Style::Number => 22,
        Style::Quote => 18,
        _ => 0,
    }
}

impl Scripts {
    pub fn new() -> Scripts {
        Scripts {
            doc: Doc::new(),
            path: String::new(),
            dirty: false,
            cur: Pos::default(),
            anchor: None,
            pending: None,
            undo: vec![],
            redo: vec![],
            typing: false,
            clip: vec![],
            zoom: 0,
            eff_zoom: 100,
            scroll: 0,
            lines: vec![],
            pages: 1,
            layout_key: (u64::MAX, 0, 0),
            ver: 0,
            area: Rect::default(),
            page_x: 0,
            scale: 1,
            mouse: (0, 0),
            press: None,
            want_visible: false,
            goal_x: None,
            overlay: Overlay::None,
            foreign: false,
        }
    }

    // ---- geometry ----------------------------------------------------------

    fn l(&self, pt: i32) -> i32 {
        (pt * self.eff_zoom + 37) / 75
    }

    /// Text size in logical px for a style at the current zoom.
    fn size(&self, s: Style) -> i32 {
        self.l(metrics(s).0).max(6)
    }

    fn adv64(&self, face: Face, size: i32, ch: char) -> i32 {
        if ch == '\t' {
            return 4 * font::advance64(face, size * self.scale, ' ');
        }
        font::advance64(face, size * self.scale, ch)
    }

    fn char_w(&self, p: &Para, i: usize) -> i32 {
        let size = self.size(p.style);
        self.adv64(face_for(p.style, p.fmt[i]), size, p.text[i])
    }

    fn page_h(&self) -> i32 {
        self.l(PAGE_H)
    }

    fn page_top(&self, k: usize) -> i32 {
        k as i32 * (self.page_h() + GAP)
    }

    fn layout(&mut self) {
        let key = (self.ver, self.eff_zoom, self.scale);
        if key == self.layout_key {
            return;
        }
        self.layout_key = key;
        self.lines.clear();
        let content_w64 = self.l(PAGE_W - 2 * MARGIN) * self.scale * 64;
        let (top, bottom) = (self.l(MARGIN), self.page_h() - self.l(MARGIN));
        let mut page = 0usize;
        let mut y = top;
        for pi in 0..self.doc.paras.len() {
            let p = &self.doc.paras[pi];
            let (_, _, before, after, lh_pct) = metrics(p.style);
            let size = self.size(p.style);
            let lh = size * lh_pct / 100;
            if y > top {
                y += self.l(before);
            }
            let ind64 = self.l(indent_pt(p.style)) * self.scale * 64;
            let avail = content_w64 - ind64;
            // wrap at spaces
            let mut breaks: Vec<(usize, usize, i32)> = Vec::new();
            let (mut s, mut w, mut last_space) = (0usize, 0i32, None::<(usize, i32)>);
            let mut i = 0;
            while i < p.len() {
                let a = self.char_w(p, i);
                if w + a > avail && i > s {
                    let (brk, bw) = match last_space {
                        Some((sp, spw)) if sp > s => (sp, spw),
                        _ => (i, w),
                    };
                    breaks.push((s, brk, bw));
                    s = brk;
                    w = (s..i).map(|k| self.char_w(p, k)).sum();
                    last_space = None;
                }
                w += a;
                i += 1;
                if p.text[i - 1] == ' ' {
                    last_space = Some((i, w));
                }
            }
            breaks.push((s, p.len(), w));
            // keep a heading with the line after it (no heading alone at a page bottom)
            if p.style.heading() && pi + 1 < self.doc.paras.len() && y > top {
                let next = self.doc.paras[pi + 1].style;
                let need = breaks.len() as i32 * lh + self.l(after) + self.l(metrics(next).2) + self.size(next) * metrics(next).4 / 100;
                if y + need > bottom {
                    page += 1;
                    y = top;
                }
            }
            for (s, e, mut w) in breaks {
                // trailing spaces don't count for alignment
                let mut k = e;
                while k > s && p.text[k - 1] == ' ' {
                    k -= 1;
                    w -= self.char_w(p, k);
                }
                if y + lh > bottom && y > top {
                    page += 1;
                    y = top;
                }
                let x64 = ind64
                    + match p.align {
                        Align::Left => 0,
                        Align::Center => (avail - w).max(0) / 2,
                        Align::Right => (avail - w).max(0),
                    };
                let y_abs = page as i32 * (self.page_h() + GAP) + y;
                self.lines.push(Line { p: pi, s, e, x64, y: y_abs, h: lh, base: (lh + size * 7 / 10) / 2, page });
                y += lh;
            }
            y += self.l(after);
        }
        self.pages = page + 1;
    }

    /// Line index holding `pos` (at a wrap boundary, the following line).
    fn line_of(&self, pos: Pos) -> usize {
        let mut found = 0;
        for (li, l) in self.lines.iter().enumerate() {
            if l.p == pos.p && pos.i >= l.s && pos.i <= l.e {
                found = li;
                let next_same = self.lines.get(li + 1).map_or(false, |n| n.p == pos.p && n.s == pos.i);
                if !next_same {
                    break;
                }
            } else if l.p > pos.p {
                break;
            }
        }
        found
    }

    /// x of `pos` within its line, device px * 64 from the content box.
    fn x64_of(&self, li: usize, i: usize) -> i32 {
        let l = &self.lines[li];
        let p = &self.doc.paras[l.p];
        l.x64 + (l.s..i.min(l.e)).map(|k| self.char_w(p, k)).sum::<i32>()
    }

    fn pos_in_line(&self, li: usize, x64: i32) -> Pos {
        let l = &self.lines[li];
        let p = &self.doc.paras[l.p];
        let mut w = l.x64;
        for k in l.s..l.e {
            let a = self.char_w(p, k);
            if w + a / 2 > x64 {
                return Pos::new(l.p, k);
            }
            w += a;
        }
        // don't land after a wrapped line's trailing space
        let end = if l.e > l.s && l.e < p.len() && p.text[l.e - 1] == ' ' { l.e - 1 } else { l.e };
        Pos::new(l.p, end)
    }

    fn content_x(&self) -> i32 {
        self.page_x + self.l(MARGIN)
    }

    fn hit(&self, mx: i32, my: i32) -> Pos {
        if self.lines.is_empty() {
            return Pos::default();
        }
        let gy = my - self.area.y - GAP + self.scroll;
        let mut li = self.lines.len() - 1;
        for (k, l) in self.lines.iter().enumerate() {
            if gy < l.y + l.h {
                li = k;
                break;
            }
        }
        self.pos_in_line(li, (mx - self.content_x()) * self.scale * 64)
    }

    fn doc_height(&self) -> i32 {
        self.page_top(self.pages) - GAP + 2 * GAP
    }

    // ---- editing --------------------------------------------------------------

    fn sel(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cur {
            return None;
        }
        Some(if a < self.cur { (a, self.cur) } else { (self.cur, a) })
    }

    fn snapshot(&mut self) {
        self.undo.push((self.doc.clone(), self.cur));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn changed(&mut self) {
        self.ver += 1;
        self.dirty = true;
        self.want_visible = true;
    }

    /// Start an edit: snapshot for undo (typing coalesces into one step).
    fn edit(&mut self, typing: bool) {
        if !(typing && self.typing) {
            self.snapshot();
        }
        self.typing = typing;
    }

    fn delete_sel(&mut self) -> bool {
        if let Some((a, b)) = self.sel() {
            self.doc.delete(a, b);
            self.cur = a;
            self.anchor = None;
            return true;
        }
        self.anchor = None;
        false
    }

    fn type_text(&mut self, s: &str) {
        self.edit(true);
        let f = match self.sel() {
            Some((a, _)) => self.doc.fmt_at(Pos::new(a.p, a.i + 1)),
            None => self.pending.unwrap_or_else(|| self.doc.fmt_at(self.cur)),
        };
        self.delete_sel();
        self.cur = self.doc.insert(self.cur, s, f);
        self.pending = Some(f).filter(|_| self.pending.is_some());
        self.changed();
    }

    fn enter(&mut self) {
        self.edit(false);
        self.delete_sel();
        let p = &mut self.doc.paras[self.cur.p];
        if p.text.is_empty() && p.style.list() {
            // Enter on an empty list item ends the list
            p.style = Style::Body;
        } else {
            self.cur = self.doc.split(self.cur);
        }
        self.changed();
    }

    fn backspace(&mut self) {
        self.edit(false);
        if !self.delete_sel() {
            let c = self.cur;
            if c.i > 0 {
                self.doc.delete(Pos::new(c.p, c.i - 1), c);
                self.cur.i -= 1;
            } else if self.doc.paras[c.p].style.list() || self.doc.paras[c.p].style == Style::Quote {
                // first press at the start of a list item / quote: back to body text
                self.doc.paras[c.p].style = Style::Body;
            } else if c.p > 0 {
                let prev = Pos::new(c.p - 1, self.doc.paras[c.p - 1].len());
                self.doc.delete(prev, c);
                self.cur = prev;
            }
        }
        self.changed();
    }

    fn delete_fwd(&mut self) {
        self.edit(false);
        if !self.delete_sel() {
            let c = self.cur;
            if c.i < self.doc.paras[c.p].len() {
                self.doc.delete(c, Pos::new(c.p, c.i + 1));
            } else if c.p + 1 < self.doc.paras.len() {
                self.doc.delete(c, Pos::new(c.p + 1, 0));
            }
        }
        self.changed();
    }

    fn toggle(&mut self, bit: u8) {
        match self.sel() {
            Some((a, b)) => {
                self.edit(false);
                let on = !self.doc.all_have(a, b, bit);
                self.doc.set_fmt(a, b, bit, on);
                self.changed();
            }
            None => {
                let f = self.pending.unwrap_or_else(|| self.doc.fmt_at(self.cur));
                self.pending = Some(f ^ bit);
            }
        }
    }

    /// Current formatting (for the toolbar's pressed states).
    fn fmt_now(&mut self) -> u8 {
        match self.sel() {
            Some((a, b)) => [BOLD, ITALIC, UNDERLINE, STRIKE].iter().filter(|&&bit| self.doc.all_have(a, b, bit)).fold(0, |acc, &b| acc | b),
            None => self.pending.unwrap_or_else(|| self.doc.fmt_at(self.cur)),
        }
    }

    fn para_range(&self) -> (usize, usize) {
        match self.sel() {
            Some((a, b)) => (a.p, if b.i == 0 && b.p > a.p { b.p - 1 } else { b.p }),
            None => (self.cur.p, self.cur.p),
        }
    }

    fn set_style(&mut self, s: Style) {
        self.edit(false);
        let (a, b) = self.para_range();
        // list buttons toggle
        let all = (a..=b).all(|k| self.doc.paras[k].style == s);
        let s = if all && s.list() { Style::Body } else { s };
        for k in a..=b {
            self.doc.paras[k].style = s;
        }
        self.changed();
    }

    fn set_align(&mut self, al: Align) {
        self.edit(false);
        let (a, b) = self.para_range();
        for k in a..=b {
            self.doc.paras[k].align = al;
        }
        self.changed();
    }

    fn undo_redo(&mut self, undo: bool) {
        let (from, to) = if undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        if let Some((d, c)) = from.pop() {
            to.push((core::mem::replace(&mut self.doc, d), self.cur));
            self.cur = self.doc.clamp(c);
            self.anchor = None;
            self.typing = false;
            self.changed();
        }
    }

    fn copy(&mut self, sys: &mut Sys) {
        if let Some((a, b)) = self.sel() {
            self.clip = self.doc.slice(a, b);
            sys.clipboard = Doc::plain(&self.clip);
        }
    }

    fn paste(&mut self, sys: &mut Sys) {
        // our own copy keeps its formatting; anything else arrives as text
        let frag = if !self.clip.is_empty() && Doc::plain(&self.clip) == sys.clipboard {
            self.clip.clone()
        } else if !sys.clipboard.is_empty() {
            Doc::from_text(&sys.clipboard).paras
        } else {
            return;
        };
        self.edit(false);
        self.delete_sel();
        self.cur = self.doc.paste(self.cur, &frag);
        self.changed();
    }

    // ---- movement ----------------------------------------------------------

    fn move_to(&mut self, pos: Pos, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.cur = self.doc.clamp(pos);
        self.pending = None;
        self.typing = false;
        self.want_visible = true;
    }

    fn step(&self, pos: Pos, fwd: bool, word: bool) -> Pos {
        let d = &self.doc;
        let mut p = pos;
        if fwd {
            if p.i >= d.paras[p.p].len() {
                return if p.p + 1 < d.paras.len() { Pos::new(p.p + 1, 0) } else { p };
            }
            p.i += 1;
            if word {
                let t = &d.paras[p.p].text;
                while p.i < t.len() && !(t[p.i - 1].is_alphanumeric() && !t[p.i].is_alphanumeric()) {
                    p.i += 1;
                }
            }
        } else {
            if p.i == 0 {
                return if p.p > 0 { Pos::new(p.p - 1, d.paras[p.p - 1].len()) } else { p };
            }
            p.i -= 1;
            if word {
                let t = &d.paras[p.p].text;
                while p.i > 0 && !(t[p.i].is_alphanumeric() && !t[p.i - 1].is_alphanumeric()) {
                    p.i -= 1;
                }
            }
        }
        p
    }

    fn vertical(&mut self, lines: i32, extend: bool) {
        if self.lines.is_empty() {
            return;
        }
        let li = self.line_of(self.cur);
        let x = self.goal_x.unwrap_or_else(|| self.x64_of(li, self.cur.i));
        let nl = (li as i32 + lines).clamp(0, self.lines.len() as i32 - 1) as usize;
        let pos = self.pos_in_line(nl, x);
        self.move_to(pos, extend);
        self.goal_x = Some(x);
    }

    // ---- files ------------------------------------------------------------

    fn title(&self) -> String {
        if self.path.is_empty() {
            return String::from("Untitled document");
        }
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[..k]).unwrap_or(b).to_string()
    }

    fn ext(&self) -> &str {
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[k..]).unwrap_or("")
    }

    fn encode(&self, ext: &str) -> Vec<u8> {
        match ext {
            ".md" => self.doc.to_markdown().into_bytes(),
            ".txt" => self.doc.to_text().into_bytes(),
            _ => self.doc.to_docx(),
        }
    }

    /// A file name from the first line of text.
    fn suggested_name(&self) -> String {
        let first = self.doc.paras.iter().map(|p| p.string()).find(|s| !s.trim().is_empty()).unwrap_or_default();
        let clean: String = first.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()).take(40).collect();
        let clean = clean.trim().to_string();
        if clean.is_empty() {
            String::from("Untitled document")
        } else {
            clean
        }
    }

    fn save(&mut self, sys: &mut Sys, quiet: bool) {
        if self.path.is_empty() {
            self.path = sys.fs.unique("/home/Documents", &self.suggested_name(), ".docx");
        }
        let mut copied = false;
        if self.foreign {
            let dir = parent(&self.path);
            self.path = sys.fs.unique(&dir, &format!("{} (edited)", self.title()), ".docx");
            self.foreign = false;
            copied = true;
        }
        let ext = self.ext().to_ascii_lowercase();
        let ok = sys.fs.write(&self.path, &self.encode(&ext));
        if ok {
            self.dirty = false;
        }
        if !quiet {
            let msg = if !ok {
                String::from("Couldn't save: the disk is read-only")
            } else if copied {
                format!("Saved as {} (the original is unchanged)", basename(&self.path))
            } else {
                format!("Saved {}", basename(&self.path))
            };
            sys.toast("Hyda Scripts", &msg);
        }
    }

    fn export(&mut self, sys: &mut Sys, ext: &str) {
        let dir = if self.path.is_empty() { String::from("/home/Documents") } else { parent(&self.path) };
        let name = if self.path.is_empty() { self.suggested_name() } else { self.title() };
        let path = sys.fs.unique(&dir, &name, ext);
        sys.fs.write(&path, &self.encode(ext));
        sys.toast("Hyda Scripts", &format!("Exported {}", basename(&path)));
    }

    /// Keep work safe before replacing the document.
    fn park(&mut self, sys: &mut Sys) {
        if self.dirty && (!self.path.is_empty() || self.doc.words() > 0) {
            self.save(sys, true);
        }
    }

    fn load(&mut self, path: &str, sys: &mut Sys) {
        let Some(data) = sys.fs.read(path) else {
            sys.toast("Hyda Scripts", &format!("Couldn't open {}", basename(path)));
            return;
        };
        let lower = path.to_ascii_lowercase();
        let doc = if lower.ends_with(".docx") {
            match Doc::from_docx(&data) {
                Some(d) => d,
                None => {
                    sys.toast("Hyda Scripts", &format!("{} isn't a Word document it can read", basename(path)));
                    return;
                }
            }
        } else if lower.ends_with(".md") {
            Doc::from_markdown(&String::from_utf8_lossy(&data))
        } else {
            Doc::from_text(&String::from_utf8_lossy(&data))
        };
        // made by another program? (Hyda Scripts names itself in docProps/app.xml)
        let foreign = lower.ends_with(".docx")
            && !crate::zip::read(&data, "docProps/app.xml").map_or(false, |a| String::from_utf8_lossy(&a).contains("<Application>Hyda Scripts</Application>"));
        self.park(sys);
        self.reset(doc);
        self.path = path.to_string();
        self.foreign = foreign;
    }

    fn reset(&mut self, doc: Doc) {
        self.doc = doc;
        self.path.clear();
        self.dirty = false;
        self.cur = Pos::default();
        self.anchor = None;
        self.pending = None;
        self.undo.clear();
        self.redo.clear();
        self.scroll = 0;
        self.ver += 1;
        self.foreign = false;
    }

    fn documents(sys: &Sys) -> Vec<String> {
        let mut out = Vec::new();
        for dir in ["/home/Documents", "/home", "/home/Downloads", "/home/Shared"] {
            for (name, d, _) in sys.fs.list(dir) {
                let l = name.to_ascii_lowercase();
                if !d && (l.ends_with(".docx") || l.ends_with(".txt") || l.ends_with(".md")) {
                    out.push(join(dir, &name));
                }
            }
        }
        out
    }

    fn rename_to(&mut self, name: &str, sys: &mut Sys) {
        let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let ext = if self.path.is_empty() { String::from(".docx") } else { self.ext().to_string() };
        let dir = if self.path.is_empty() { String::from("/home/Documents") } else { parent(&self.path) };
        let target = join(&dir, &format!("{}{}", name, ext));
        if target == self.path {
            return;
        }
        let target = if sys.fs.exists(&target) { sys.fs.unique(&dir, name, &ext) } else { target };
        if !self.path.is_empty() && sys.fs.exists(&self.path) {
            sys.fs.rename(&self.path, &target);
            self.path = target;
        } else {
            self.path = target;
            self.save(sys, true);
        }
    }

    // ---- drawing ------------------------------------------------------------

    fn tool(&self, ui: &mut Ui, r: Rect, icon: Icon, code: u32, inst: u32, on: bool) {
        let t = ui.t;
        let a = Action::App(inst, code);
        if on {
            ui.rrect(r, 8, t.accent.with_alpha(45));
        } else if ui.hot(a) {
            ui.rrect(r, 8, t.hover);
        }
        ui.icon_in(icon, r, 16, if on { t.accent } else { t.text });
        ui.zone(r, a);
    }

    fn text_tool(&self, ui: &mut Ui, r: Rect, label: &str, face: Face, code: u32, inst: u32, on: bool, deco: u8) {
        let t = ui.t;
        let a = Action::App(inst, code);
        if on {
            ui.rrect(r, 8, t.accent.with_alpha(45));
        } else if ui.hot(a) {
            ui.rrect(r, 8, t.hover);
        }
        let col = if on { t.accent } else { t.text };
        let w = ui.text_in(r, face, 15, label, col, 1);
        let x = r.x + (r.w - w) / 2;
        if deco == UNDERLINE {
            ui.rect(Rect::new(x, r.y + r.h / 2 + 7, w, 1), col);
        } else if deco == STRIKE {
            ui.rect(Rect::new(x - 1, r.y + r.h / 2 - 1, w + 2, 1), col);
        }
        ui.zone(r, a);
    }

    fn sep(ui: &mut Ui, x: i32, y: i32) {
        let t = ui.t;
        ui.rect(Rect::new(x, y + 12, 1, 20), t.line);
    }

    fn render_toolbar(&mut self, ui: &mut Ui, r: Rect, inst: u32, compact: bool) {
        let t = ui.t;
        let y = r.y + HEADER;
        ui.rect(Rect::new(r.x, y, r.w, TOOLBAR), t.surface);
        ui.rect(Rect::new(r.x, y + TOOLBAR - 1, r.w, 1), t.line);
        let fmt = self.fmt_now();
        let para = &self.doc.paras[self.cur.p];
        let (style, align) = (para.style, para.align);
        let mut x = r.x + 12;
        let b = |x: i32, w: i32| Rect::new(x, y + 7, w, 32);
        // paragraph style
        let sw = if compact { 108 } else { 138 };
        let sr = b(x, sw);
        let sa = Action::App(inst, C_STYLE_MENU);
        ui.rrect(sr, 8, if ui.hot(sa) || matches!(self.overlay, Overlay::Styles) { t.hover } else { t.chip });
        ui.text_in(Rect::new(sr.x + 10, sr.y, sr.w - 30, sr.h), Face::Medium, 13, style.name(), t.text, 0);
        ui.icon(Icon::ChevronRight, sr.r() - 22, sr.y + 9, 14, t.text2);
        ui.zone(sr, sa);
        x += sw + 10;
        Self::sep(ui, x - 5, y);
        for (label, face, code, bit) in [("B", Face::Semibold, C_BOLD, BOLD), ("I", Face::Italic, C_ITALIC, ITALIC), ("U", Face::Regular, C_UNDERLINE, UNDERLINE), ("S", Face::Regular, C_STRIKE, STRIKE)] {
            if compact && bit == STRIKE {
                continue;
            }
            self.text_tool(ui, b(x, 32), label, face, code, inst, fmt & bit != 0, bit & (UNDERLINE | STRIKE));
            x += 34;
        }
        x += 6;
        Self::sep(ui, x - 5, y);
        if !compact {
            for (icon, code, al) in [(Icon::AlignLeft, C_LEFT, Align::Left), (Icon::AlignCenter, C_CENTER, Align::Center), (Icon::AlignRight, C_RIGHT, Align::Right)] {
                self.tool(ui, b(x, 32), icon, code, inst, align == al);
                x += 34;
            }
            x += 6;
            Self::sep(ui, x - 5, y);
        }
        self.tool(ui, b(x, 32), Icon::ListBullet, C_BULLET, inst, style == Style::Bullet);
        x += 34;
        self.tool(ui, b(x, 32), Icon::ListNumber, C_NUMBER, inst, style == Style::Number);
        x += 40;
        Self::sep(ui, x - 5, y);
        self.tool(ui, b(x, 32), Icon::Undo, C_UNDO, inst, false);
        x += 34;
        if !compact {
            self.tool(ui, b(x, 32), Icon::Redo, C_REDO, inst, false);
            x += 34;
        }
        // zoom, right-aligned
        if !compact && x + 150 < r.r() {
            let zx = r.r() - 128;
            let za = Action::App(inst, C_ZOOM_OUT);
            ui.icon_button(Rect::new(zx, y + 9, 28, 28), Icon::Minimize, za, 14);
            let label = format!("{}%", self.eff_zoom);
            let fa = Action::App(inst, C_FIT);
            let fr = Rect::new(zx + 30, y + 9, 56, 28);
            if ui.hot(fa) {
                ui.rrect(fr, 8, t.hover);
            }
            ui.text_in(fr, Face::Medium, 13, &label, if self.zoom == 0 { t.text2 } else { t.text }, 1);
            ui.zone(fr, fa);
            ui.icon_button(Rect::new(zx + 88, y + 9, 28, 28), Icon::Plus, Action::App(inst, C_ZOOM_IN), 14);
        }
    }

    fn render_doc(&mut self, ui: &mut Ui, area: Rect, inst: u32, focused: bool) {
        let t = ui.t;
        self.area = area;
        self.scale = ui.s;
        let fit = ((area.w - 2 * GAP).max(200) * 75 / PAGE_W).clamp(40, 100);
        self.eff_zoom = if self.zoom == 0 { fit } else { self.zoom };
        self.layout();
        let page_w = self.l(PAGE_W);
        self.page_x = area.x + ((area.w - page_w) / 2).max(GAP.min(area.w / 20));
        // keep the caret in view
        if self.want_visible && !self.lines.is_empty() {
            let (ly, lh) = {
                let l = &self.lines[self.line_of(self.cur)];
                (l.y, l.h)
            };
            let (top, bot) = (ly + GAP, ly + GAP + lh);
            if top < self.scroll + 8 {
                self.scroll = top - 8;
            }
            if bot > self.scroll + area.h - 8 {
                self.scroll = bot - area.h + 8;
            }
            self.want_visible = false;
        }
        self.scroll = self.scroll.clamp(0, (self.doc_height() - area.h).max(0));
        ui.rect(area, t.sidebar.mix(t.text, 12));
        let old = ui.clip_in(area);
        let ph = self.page_h();
        for k in 0..self.pages {
            let py = area.y + GAP + self.page_top(k) - self.scroll;
            if py > area.b() || py + ph < area.y {
                continue;
            }
            let pr = Rect::new(self.page_x, py, page_w, ph);
            ui.shadow(pr, 2, 10, 2, 40);
            ui.rect(pr, PAPER);
        }
        let sel = self.sel();
        let cx0 = self.content_x();
        let s = ui.s;
        let caret_li = self.line_of(self.cur);
        let mut numbers: Vec<usize> = vec![0; self.doc.paras.len()];
        let mut n = 0;
        for (k, p) in self.doc.paras.iter().enumerate() {
            n = if p.style == Style::Number { n + 1 } else { 0 };
            numbers[k] = n;
        }
        for (li, l) in self.lines.iter().enumerate() {
            let y = area.y + GAP + l.y - self.scroll;
            if y > area.b() {
                break;
            }
            if y + l.h < area.y {
                continue;
            }
            let p = &self.doc.paras[l.p];
            let size = self.size(p.style);
            let base = y + l.base;
            // selection
            if let Some((a, b)) = sel {
                let lp = Pos::new(l.p, l.s);
                let le = Pos::new(l.p, l.e);
                if a <= le && b >= lp && !(b == lp && l.s != l.e) {
                    let s0 = if a.p == l.p { a.i.max(l.s) } else { l.s };
                    let s1 = if b.p == l.p { b.i.min(l.e) } else { l.e };
                    let x0 = self.x64_of(li, s0) / 64 / s;
                    let mut x1 = self.x64_of(li, s1) / 64 / s;
                    if b.p > l.p && l.e == p.len() {
                        x1 += size / 3; // the paragraph mark
                    }
                    ui.rect(Rect::new(cx0 + x0, y, (x1 - x0).max(2), l.h), t.accent.with_alpha(55));
                }
            }
            // list marker / quote bar on the first line
            if l.s == 0 {
                let ind = self.l(indent_pt(p.style));
                match p.style {
                    Style::Bullet => {
                        ui.circle(cx0 + ind - self.l(12), base - size * 3 / 10, (size / 7).max(2), INK);
                    }
                    Style::Number => {
                        let m = format!("{}.", numbers[l.p]);
                        let mw = ui.tw(Face::Regular, size, &m);
                        ui.text(cx0 + ind - self.l(6) - mw, base, Face::Regular, size, &m, INK);
                    }
                    _ => {}
                }
            }
            if p.style == Style::Quote {
                ui.rect(Rect::new(cx0 + self.l(4), y, (self.l(3)).max(2), l.h), t.accent);
            }
            // text, in runs of equal formatting
            let col = if p.style == Style::Quote { INK2 } else { INK };
            let mut pen64 = (cx0 * s) * 64 + l.x64;
            let mut k = l.s;
            while k < l.e {
                let f = p.fmt[k];
                let mut j = k;
                while j < l.e && p.fmt[j] == f && p.text[j] != '\t' {
                    j += 1;
                }
                if j == k {
                    // a tab
                    pen64 += self.char_w(p, k);
                    k += 1;
                    continue;
                }
                let face = face_for(p.style, f);
                let run: String = p.text[k..j].iter().collect();
                let w64: i32 = (k..j).map(|q| self.char_w(p, q)).sum();
                let dx = (pen64 + 32) >> 6;
                font::draw(ui.c, dx, base * s, face, size * s, &run, col);
                let (lx, lw) = (dx / s, (w64 >> 6) / s);
                if f & UNDERLINE != 0 {
                    ui.rect(Rect::new(lx, base + (size / 9).max(1), lw, (size / 16).max(1)), col);
                }
                if f & STRIKE != 0 {
                    ui.rect(Rect::new(lx, base - size * 3 / 10, lw, (size / 16).max(1)), col);
                }
                pen64 += w64;
                k = j;
            }
            if focused && li == caret_li && sel.is_none() && (ui.ticks / 50) % 2 == 0 {
                let x = cx0 + self.x64_of(li, self.cur.i) / 64 / s;
                ui.rect(Rect::new(x, y + (l.h - size * 5 / 4).max(0) / 2, 2.max(size / 12), size * 5 / 4), INK);
            }
        }
        ui.set_clip(old);
        ui.zone(area, Action::App(inst, C_PAGE));
    }

    fn render_overlay(&mut self, ui: &mut Ui, r: Rect, inst: u32) {
        let t = ui.t;
        match &self.overlay {
            Overlay::None => {}
            Overlay::Styles => {
                ui.zone(r, Action::App(inst, C_DISMISS));
                let m = Rect::new(r.x + 12, r.y + HEADER + TOOLBAR - 4, 230, STYLES.len() as i32 * 40 + 12);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let cur = self.doc.paras[self.cur.p].style;
                for (k, s) in STYLES.iter().enumerate() {
                    let row = Rect::new(m.x + 6, m.y + 6 + k as i32 * 40, m.w - 12, 38);
                    let a = Action::App(inst, C_STYLE + k as u32);
                    if *s == cur {
                        ui.rrect(row, 8, t.accent.with_alpha(40));
                    } else if ui.hot(a) {
                        ui.rrect(row, 8, t.hover);
                    }
                    let (pt, bold, ..) = metrics(*s);
                    let size = (pt * 4 / 3).clamp(13, 24);
                    let face = if bold { Face::Semibold } else if *s == Style::Quote { Face::Italic } else { Face::Regular };
                    let label = match s {
                        Style::Bullet => "•  Bulleted list",
                        Style::Number => "1.  Numbered list",
                        _ => s.name(),
                    };
                    ui.text_in(Rect::new(row.x + 12, row.y, row.w - 20, row.h), face, size, label, t.text, 0);
                    ui.zone(row, a);
                }
            }
            Overlay::Open(files) => {
                ui.zone(r, Action::App(inst, C_DISMISS));
                let w = (r.w - 40).min(420);
                let n = files.len().min(9) as i32;
                let m = Rect::new(r.x + (r.w - w) / 2, r.y + HEADER + 20, w, 96 + (n.max(1)) * 40);
                ui.shadow(m, 14, 18, 6, 70);
                ui.rrect(m, 14, t.surface);
                ui.text(m.x + 20, m.y + 32, Face::Semibold, 16, "Open a document", t.text);
                let nr = Rect::new(m.x + 12, m.y + 46, m.w - 24, 38);
                let na = Action::App(inst, C_NEW);
                if ui.hot(na) {
                    ui.rrect(nr, 8, t.hover);
                }
                ui.icon(Icon::Plus, nr.x + 10, nr.y + 11, 16, t.accent);
                ui.text_in(Rect::new(nr.x + 36, nr.y, nr.w - 40, nr.h), Face::Medium, 13, "New blank document", t.accent, 0);
                ui.zone(nr, na);
                if files.is_empty() {
                    ui.text(m.x + 20, m.y + 110, Face::Regular, 13, "No documents yet in Documents, Downloads or Shared.", t.text2);
                }
                for (k, f) in files.iter().take(9).enumerate() {
                    let row = Rect::new(m.x + 12, m.y + 88 + k as i32 * 40, m.w - 24, 38);
                    let a = Action::App(inst, C_FILE + k as u32);
                    if ui.hot(a) {
                        ui.rrect(row, 8, t.hover);
                    }
                    ui.icon(Icon::Scripts, row.x + 10, row.y + 11, 16, t.text2);
                    let name = ui.fit(Face::Medium, 13, basename(f), row.w - 150);
                    ui.text(row.x + 36, row.y + 24, Face::Medium, 13, &name, t.text);
                    let d = parent(f);
                    let d = d.rsplit('/').next().unwrap_or("");
                    let dw = ui.tw(Face::Regular, 12, d);
                    ui.text(row.r() - dw - 10, row.y + 24, Face::Regular, 12, d, t.text3);
                    ui.zone(row, a);
                }
            }
            Overlay::Rename(e) => {
                let _ = e;
            }
        }
    }
}

impl App for Scripts {
    fn kind(&self) -> AppKind {
        AppKind::Scripts
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        let t = ui.t;
        let compact = super::compact(r);
        // header: title (click to rename), then file buttons
        let title_x = r.x + 18;
        // the header's right 110 units hold the window controls
        let bx = r.r() - 118 - if compact { 62 } else { 96 };
        ui.icon(Icon::Scripts, title_x, r.y + 13, 18, t.accent);
        let tr = Rect::new(title_x + 26, r.y + 7, bx - title_x - 34, 30);
        match &self.overlay {
            Overlay::Rename(e) => {
                let text = e.text.clone();
                ui.field(tr, &text, "Document name", true, Action::App(inst, C_RENAME));
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
        if !compact {
            ui.icon_button(Rect::new(bx, r.y + 8, 28, 28), Icon::Plus, Action::App(inst, C_NEW), 16);
        }
        ui.icon_button(Rect::new(bx + if compact { 0 } else { 34 }, r.y + 8, 28, 28), Icon::Folder, Action::App(inst, C_OPEN), 16);
        ui.icon_button(Rect::new(bx + if compact { 34 } else { 68 }, r.y + 8, 28, 28), Icon::Save, Action::App(inst, C_SAVE), 16);
        self.render_toolbar(ui, r, inst, compact);
        let area = Rect::new(r.x, r.y + HEADER + TOOLBAR, r.w, r.h - HEADER - TOOLBAR - STATUS);
        let focused = !matches!(self.overlay, Overlay::Rename(_));
        self.render_doc(ui, area, inst, focused);
        // status bar
        let sy = r.b() - STATUS;
        ui.rect(Rect::new(r.x, sy, r.w, 1), t.line);
        let page = if self.lines.is_empty() { 1 } else { self.lines[self.line_of(self.cur)].page + 1 };
        let left = format!("Page {} of {}  ·  {} words", page, self.pages, self.doc.words());
        ui.text(r.x + 16, sy + 19, Face::Regular, 12, &left, t.text2);
        let kind = match self.ext().to_ascii_lowercase().as_str() {
            ".md" => "Markdown",
            ".txt" => "Plain text",
            _ => "Word document",
        };
        let right = format!("{}  ·  {}", kind, if self.dirty { "Edited" } else if self.path.is_empty() { "Not saved yet" } else { "Saved" });
        let rw = ui.tw(Face::Regular, 12, &right);
        if !compact {
            ui.text(r.r() - rw - 16, sy + 19, Face::Regular, 12, &right, t.text3);
        }
        self.render_overlay(ui, r, inst);
    }

    fn action(&mut self, code: u32, double: bool, sys: &mut Sys) {
        if let Overlay::Rename(e) = &self.overlay {
            if code != C_RENAME {
                let name = e.text.clone();
                self.overlay = Overlay::None;
                self.rename_to(&name, sys);
            }
        }
        if matches!(self.overlay, Overlay::Styles | Overlay::Open(_)) && code < C_STYLE && code != C_NEW {
            self.overlay = Overlay::None;
            if code == C_DISMISS || code == C_STYLE_MENU || code == C_OPEN {
                return;
            }
        }
        if code != C_PAGE {
            self.goal_x = None;
        }
        match code {
            C_PAGE => {
                let pos = self.hit(self.mouse.0, self.mouse.1);
                if double {
                    let (a, b) = self.doc.word_at(pos);
                    self.anchor = Some(a);
                    self.cur = b;
                    self.press = None;
                } else {
                    let extend = crate::input::shift();
                    self.move_to(pos, extend);
                    self.press = Some(pos);
                }
                self.goal_x = None;
                self.want_visible = false;
            }
            C_STYLE_MENU => self.overlay = Overlay::Styles,
            C_BOLD => self.toggle(BOLD),
            C_ITALIC => self.toggle(ITALIC),
            C_UNDERLINE => self.toggle(UNDERLINE),
            C_STRIKE => self.toggle(STRIKE),
            C_LEFT => self.set_align(Align::Left),
            C_CENTER => self.set_align(Align::Center),
            C_RIGHT => self.set_align(Align::Right),
            C_BULLET => self.set_style(Style::Bullet),
            C_NUMBER => self.set_style(Style::Number),
            C_UNDO => self.undo_redo(true),
            C_REDO => self.undo_redo(false),
            C_ZOOM_IN => self.zoom = (self.eff_zoom / 10 * 10 + 10).min(200),
            C_ZOOM_OUT => self.zoom = ((self.eff_zoom + 9) / 10 * 10 - 10).max(50),
            C_FIT => self.zoom = 0,
            C_NEW => {
                self.overlay = Overlay::None;
                self.park(sys);
                self.reset(Doc::new());
            }
            C_OPEN => self.overlay = Overlay::Open(Self::documents(sys)),
            C_SAVE => self.save(sys, false),
            C_SAVE_DOCX => {
                if self.ext().to_ascii_lowercase() != ".docx" {
                    let dir = if self.path.is_empty() { String::from("/home/Documents") } else { parent(&self.path) };
                    let name = if self.path.is_empty() { self.suggested_name() } else { self.title() };
                    self.path = sys.fs.unique(&dir, &name, ".docx");
                }
                self.save(sys, false);
            }
            C_EXPORT_TXT => self.export(sys, ".txt"),
            C_EXPORT_MD => self.export(sys, ".md"),
            C_RENAME => {
                if !matches!(self.overlay, Overlay::Rename(_)) {
                    let name = if self.path.is_empty() { self.suggested_name() } else { self.title() };
                    self.overlay = Overlay::Rename(LineEdit { text: name });
                }
            }
            C_CUT => {
                self.copy(sys);
                if self.sel().is_some() {
                    self.edit(false);
                    self.delete_sel();
                    self.changed();
                }
            }
            C_COPY => self.copy(sys),
            C_PASTE => self.paste(sys),
            C_ALL => {
                self.anchor = Some(Pos::default());
                self.cur = self.doc.end();
            }
            C_DISMISS => {}
            c if (C_STYLE..C_STYLE + STYLES.len() as u32).contains(&c) => {
                self.overlay = Overlay::None;
                let s = STYLES[(c - C_STYLE) as usize];
                if s.list() {
                    // choosing a list from the menu never toggles it off
                    let (a, b) = self.para_range();
                    if (a..=b).all(|k| self.doc.paras[k].style == s) {
                        return;
                    }
                }
                self.set_style(s);
            }
            c if c >= C_FILE => {
                let path = match &self.overlay {
                    Overlay::Open(files) => files.get((c - C_FILE) as usize).cloned(),
                    _ => None,
                };
                self.overlay = Overlay::None;
                if let Some(p) = path {
                    self.load(&p, sys);
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, ctrl: bool, sys: &mut Sys) {
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
            if matches!(k, Key::Esc) {
                self.overlay = Overlay::None;
            }
            return;
        }
        let shift = crate::input::shift();
        if ctrl {
            match k {
                Key::Char(c) => match c.to_ascii_lowercase() {
                    'b' => self.toggle(BOLD),
                    'i' => self.toggle(ITALIC),
                    'u' => self.toggle(UNDERLINE),
                    'z' => self.undo_redo(true),
                    'y' => self.undo_redo(false),
                    'x' => self.action(C_CUT, false, sys),
                    'c' => self.copy(sys),
                    'v' => self.paste(sys),
                    'a' => self.action(C_ALL, false, sys),
                    's' => self.save(sys, false),
                    'o' => self.action(C_OPEN, false, sys),
                    'n' => self.action(C_NEW, false, sys),
                    'e' => self.set_align(Align::Center),
                    'l' => self.set_align(Align::Left),
                    'r' => self.set_align(Align::Right),
                    '=' | '+' => self.action(C_ZOOM_IN, false, sys),
                    '-' => self.action(C_ZOOM_OUT, false, sys),
                    '0' => self.set_style(Style::Body),
                    '1' => self.set_style(Style::H1),
                    '2' => self.set_style(Style::H2),
                    _ => {}
                },
                // Ctrl+I arrives as Tab (0x09) without the extended keyboard protocol
                Key::Tab => self.toggle(ITALIC),
                Key::Left => self.move_to(self.step(self.cur, false, true), shift),
                Key::Right => self.move_to(self.step(self.cur, true, true), shift),
                Key::Home => self.move_to(Pos::default(), shift),
                Key::End => self.move_to(self.doc.end(), shift),
                _ => {}
            }
            return;
        }
        if !matches!(k, Key::Up | Key::Down | Key::PageUp | Key::PageDown) {
            self.goal_x = None;
        }
        match k {
            Key::Char(c) if !c.is_control() => {
                let mut b = [0u8; 4];
                self.type_text(c.encode_utf8(&mut b));
            }
            Key::Tab => self.type_text("\t"),
            Key::Enter => self.enter(),
            Key::Backspace => self.backspace(),
            Key::Delete => self.delete_fwd(),
            Key::Left => {
                match (self.sel(), shift) {
                    (Some((a, _)), false) => self.move_to(a, false),
                    _ => self.move_to(self.step(self.cur, false, false), shift),
                }
            }
            Key::Right => {
                match (self.sel(), shift) {
                    (Some((_, b)), false) => self.move_to(b, false),
                    _ => self.move_to(self.step(self.cur, true, false), shift),
                }
            }
            Key::Up => self.vertical(-1, shift),
            Key::Down => self.vertical(1, shift),
            Key::PageUp => self.vertical(-(self.area.h / 20).max(1), shift),
            Key::PageDown => self.vertical((self.area.h / 20).max(1), shift),
            Key::Home if !self.lines.is_empty() => {
                let l = &self.lines[self.line_of(self.cur)];
                self.move_to(Pos::new(l.p, l.s), shift);
            }
            Key::End if !self.lines.is_empty() => {
                let li = self.line_of(self.cur);
                let end = self.pos_in_line(li, i32::MAX);
                let l = &self.lines[li];
                self.move_to(Pos::new(l.p, if l.e == self.doc.paras[l.p].len() { l.e } else { end.i }), shift);
            }
            Key::Esc => self.anchor = None,
            _ => {}
        }
    }

    fn scroll(&mut self, dy: i32) {
        self.scroll += dy * 48;
    }

    fn mouse(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    fn drag(&mut self, x: i32, y: i32) {
        let Some(start) = self.press else { return };
        // auto-scroll when dragging past the edges
        if y < self.area.y {
            self.scroll -= 16;
        } else if y > self.area.b() {
            self.scroll += 16;
        }
        let pos = self.hit(x, y);
        self.anchor = Some(start);
        self.cur = pos;
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => vec![
                ("New Document", C_NEW),
                ("Open…", C_OPEN),
                ("Save", C_SAVE),
                ("Save as Word (.docx)", C_SAVE_DOCX),
                ("Export as Text (.txt)", C_EXPORT_TXT),
                ("Export as Markdown (.md)", C_EXPORT_MD),
                ("Rename…", C_RENAME),
            ],
            1 => vec![("Undo", C_UNDO), ("Redo", C_REDO), ("Cut", C_CUT), ("Copy", C_COPY), ("Paste", C_PASTE), ("Select All", C_ALL)],
            2 => vec![("Zoom In", C_ZOOM_IN), ("Zoom Out", C_ZOOM_OUT), ("Fit Page Width", C_FIT)],
            _ => vec![],
        }
    }

    fn close(&mut self, sys: &mut Sys) {
        if self.dirty && (!self.path.is_empty() || self.doc.words() > 0) {
            self.save(sys, false);
        }
    }

    fn animating(&self) -> bool {
        true
    }

    fn open_path(&mut self, path: &str, sys: &mut Sys) {
        self.load(path, sys);
    }
}
