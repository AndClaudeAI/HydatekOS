//! Notes: a word-wrapping plain-text editor that saves to the HydatekOS disk.

use super::{side_item, App, AppKind, HEADER};
use crate::font::{self, Face};
use crate::fs::{basename, join};
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const SIZE: i32 = 14;
const LINE: i32 = 22;

/// Multi-line text editor with soft wrapping.
pub struct TextArea {
    pub text: String,
    pub cur: usize,
    pub scroll: i32,
    lines: Vec<(usize, usize)>,
    scale: i32,
    area: Rect,
    want_visible: bool,
}

impl TextArea {
    pub fn new(text: String) -> TextArea {
        TextArea { text, cur: 0, scroll: 0, lines: vec![], scale: 1, area: Rect::default(), want_visible: false }
    }

    fn adv(&self, ch: char) -> i32 {
        font::advance64(Face::Regular, SIZE * self.scale, ch)
    }

    fn layout(&mut self, width: i32) {
        let maxw = (width * self.scale) << 6;
        self.lines.clear();
        let bytes = self.text.as_str();
        let mut para_start = 0;
        loop {
            let para_end = bytes[para_start..].find('\n').map(|i| para_start + i).unwrap_or(bytes.len());
            let mut start = para_start;
            let mut w = 0;
            let mut last_space: Option<usize> = None;
            let mut i = para_start;
            for ch in bytes[para_start..para_end].chars() {
                let a = self.adv(ch);
                if w + a > maxw && i > start {
                    let brk = match last_space {
                        Some(sp) if sp > start => sp,
                        _ => i,
                    };
                    self.lines.push((start, brk));
                    start = brk;
                    w = bytes[start..i].chars().map(|c| self.adv(c)).sum();
                    last_space = None;
                }
                w += a;
                i += ch.len_utf8();
                if ch == ' ' {
                    last_space = Some(i);
                }
            }
            self.lines.push((start, para_end));
            if para_end >= bytes.len() {
                break;
            }
            para_start = para_end + 1;
        }
    }

    fn cursor_line(&self) -> usize {
        let mut li = 0;
        for (i, &(s, e)) in self.lines.iter().enumerate() {
            if self.cur >= s && self.cur <= e {
                li = i;
                // at a wrap boundary, prefer the following line
                if self.cur == e && i + 1 < self.lines.len() && self.lines[i + 1].0 == e {
                    continue;
                }
                break;
            }
        }
        li
    }

    fn x_of(&self, line: usize, pos: usize) -> i32 {
        let (s, _) = self.lines[line];
        self.text[s..pos].chars().map(|c| self.adv(c)).sum::<i32>() / 64 / self.scale
    }

    fn pos_at(&self, line: usize, x: i32) -> usize {
        let (s, e) = self.lines[line];
        let mut w = 0;
        let target = x * self.scale * 64;
        for (i, ch) in self.text[s..e].char_indices() {
            let a = self.adv(ch);
            if w + a / 2 > target {
                return s + i;
            }
            w += a;
        }
        e
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, focused: bool, a: Action) {
        let t = ui.t;
        self.scale = ui.s;
        self.area = r;
        self.layout(r.w);
        let cl = self.cursor_line() as i32;
        if self.want_visible {
            if cl * LINE < self.scroll {
                self.scroll = cl * LINE;
            }
            if (cl + 1) * LINE > self.scroll + r.h {
                self.scroll = (cl + 1) * LINE - r.h;
            }
            self.want_visible = false;
        }
        let max = (self.lines.len() as i32 * LINE - r.h + 8).max(0);
        self.scroll = self.scroll.clamp(0, max);
        let old = ui.clip_in(r);
        let first = (self.scroll / LINE) as usize;
        for li in first..self.lines.len() {
            let y = r.y + li as i32 * LINE - self.scroll;
            if y > r.b() {
                break;
            }
            let (s, e) = self.lines[li];
            ui.text(r.x, y + 16, Face::Regular, SIZE, &self.text[s..e], t.text);
        }
        if focused && (ui.ticks / 50) % 2 == 0 {
            let x = r.x + self.x_of(cl as usize, self.cur);
            let y = r.y + cl * LINE - self.scroll;
            ui.rect(Rect::new(x, y + 3, 2, 17), t.accent);
        }
        ui.set_clip(old);
        ui.zone(r, a);
    }

    pub fn click(&mut self, mx: i32, my: i32) {
        if self.lines.is_empty() {
            return;
        }
        let li = ((my - self.area.y + self.scroll) / LINE).clamp(0, self.lines.len() as i32 - 1) as usize;
        self.cur = self.pos_at(li, mx - self.area.x);
    }

    /// Returns true if the text changed.
    pub fn key(&mut self, k: Key) -> bool {
        self.want_visible = true;
        let prev_char = |s: &str, i: usize| s[..i].chars().next_back().map(|c| i - c.len_utf8()).unwrap_or(0);
        let next_char = |s: &str, i: usize| s[i..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(i);
        match k {
            Key::Char(c) if !c.is_control() => {
                self.text.insert(self.cur, c);
                self.cur += c.len_utf8();
                true
            }
            Key::Tab => {
                self.text.insert_str(self.cur, "    ");
                self.cur += 4;
                true
            }
            Key::Enter => {
                self.text.insert(self.cur, '\n');
                self.cur += 1;
                true
            }
            Key::Backspace if self.cur > 0 => {
                let p = prev_char(&self.text, self.cur);
                self.text.replace_range(p..self.cur, "");
                self.cur = p;
                true
            }
            Key::Delete if self.cur < self.text.len() => {
                let n = next_char(&self.text, self.cur);
                self.text.replace_range(self.cur..n, "");
                true
            }
            Key::Left => {
                self.cur = prev_char(&self.text, self.cur);
                false
            }
            Key::Right => {
                self.cur = next_char(&self.text, self.cur);
                false
            }
            Key::Up | Key::Down | Key::PageUp | Key::PageDown if !self.lines.is_empty() => {
                let cl = self.cursor_line();
                let x = self.x_of(cl, self.cur);
                let step = match k {
                    Key::Up => -1,
                    Key::Down => 1,
                    Key::PageUp => -(self.area.h / LINE).max(1),
                    _ => (self.area.h / LINE).max(1),
                };
                let nl = (cl as i32 + step).clamp(0, self.lines.len() as i32 - 1) as usize;
                self.cur = self.pos_at(nl, x);
                false
            }
            Key::Home if !self.lines.is_empty() => {
                self.cur = self.lines[self.cursor_line()].0;
                false
            }
            Key::End if !self.lines.is_empty() => {
                self.cur = self.lines[self.cursor_line()].1;
                false
            }
            _ => false,
        }
    }
}

const C_EDIT: u32 = 1;
const C_NEW: u32 = 2;
const C_SAVE: u32 = 3;
const C_DELETE: u32 = 4;
const C_LIST: u32 = 5;
const C_NOTE: u32 = 100;

pub struct Notes {
    path: String,
    edit: TextArea,
    dirty: bool,
    list: Vec<String>,
    pub mouse: (i32, i32),
    /// Phone layout: showing the note list instead of the editor.
    list_open: bool,
}

fn is_text(name: &str) -> bool {
    [".txt", ".doc", ".note", ".sheet", ".md"].iter().any(|e| name.ends_with(e))
}

impl Notes {
    pub fn new(sys: &mut Sys) -> Notes {
        let mut n = Notes { path: String::new(), edit: TextArea::new(String::new()), dirty: false, list: vec![], mouse: (0, 0), list_open: false };
        n.rescan(sys);
        let first = n.list.first().cloned().unwrap_or_else(|| "/home/Documents/Untitled.txt".to_string());
        n.load(&first, sys);
        n
    }

    fn rescan(&mut self, sys: &Sys) {
        self.list.clear();
        for dir in ["/home", "/home/Documents", "/home/Downloads", "/home/Shared"] {
            for (name, d, _) in sys.fs.list(dir) {
                if !d && is_text(&name) {
                    self.list.push(join(dir, &name));
                }
            }
        }
    }

    fn load(&mut self, path: &str, sys: &mut Sys) {
        self.save(sys);
        let data = sys.fs.read(path).unwrap_or_default();
        self.path = path.to_string();
        self.edit = TextArea::new(String::from_utf8_lossy(&data).to_string());
        self.dirty = false;
    }

    fn save(&mut self, sys: &mut Sys) {
        if self.dirty && !self.path.is_empty() {
            sys.fs.write(&self.path, self.edit.text.as_bytes());
            self.dirty = false;
        }
    }
}

impl App for Notes {
    fn kind(&self) -> AppKind {
        AppKind::Notes
    }


    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let compact = super::compact(r);
        let side_w = if compact { if self.list_open { r.w } else { 0 } } else { 180 };
        let side = Rect::new(r.x, r.y, side_w, r.h);
        if side_w > 0 {
            super::panel(ui, r, side, t.sidebar);
            ui.text_in(Rect::new(r.x + 18, r.y, 100, HEADER), Face::Semibold, 15, "Notes", t.text, 0);
            ui.icon_button(Rect::new(r.x + side_w - 40, r.y + 8, 28, 28), Icon::Plus, Action::App(inst, C_NEW), 16);
            let mut y = r.y + HEADER + 4;
            for (i, p) in self.list.iter().enumerate() {
                if y > r.b() - 30 {
                    break;
                }
                let name = super::files::display_name(basename(p));
                let label = ui.fit(Face::Regular, 13, name, side_w - 40);
                side_item(ui, Rect::new(r.x + 8, y, side_w - 16, 28), &label, *p == self.path, Action::App(inst, C_NOTE + i as u32));
                y += 30;
            }
        }
        if side_w >= r.w {
            return;
        }
        let main = Rect::new(r.x + side_w, r.y, r.w - side_w, r.h);
        let mut tx = main.x + 22;
        if compact {
            ui.icon_button(Rect::new(main.x + 10, r.y + 8, 28, 28), Icon::ChevronLeft, Action::App(inst, C_LIST), 16);
            tx = main.x + 44;
        }
        ui.rect(Rect::new(main.x, r.y + HEADER, main.w, 1), t.line);
        let mut title = super::files::display_name(basename(&self.path)).to_string();
        if self.dirty {
            title.push_str("  •");
        }
        let bx = if compact { main.r() - 76 } else { main.r() - 184 };
        let tw = bx - tx - 8;
        let title = ui.fit(Face::Semibold, 15, &title, tw);
        ui.text_in(Rect::new(tx, r.y, tw, HEADER), Face::Semibold, 15, &title, t.text, 0);
        ui.icon_button(Rect::new(bx, r.y + 8, 28, 28), Icon::Trash, Action::App(inst, C_DELETE), 16);
        ui.icon_button(Rect::new(bx + 34, r.y + 8, 28, 28), Icon::Save, Action::App(inst, C_SAVE), 16);
        let words = self.edit.text.split_whitespace().count();
        let status = format!("{} words · {} · Ctrl+S to save", words, if self.dirty { "edited" } else { "saved" });
        ui.text(main.x + 22, r.b() - 12, Face::Regular, 12, &status, t.text3);
        let ed = Rect::new(main.x + 22, r.y + HEADER + 16, main.w - 44, r.h - HEADER - 46);
        self.edit.render(ui, ed, true, Action::App(inst, C_EDIT));
        let _ = sys;
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        match code {
            C_EDIT => self.edit.click(self.mouse.0, self.mouse.1),
            C_LIST => {
                self.save(sys);
                self.rescan(sys);
                self.list_open = true;
            }
            C_NEW => {
                self.list_open = false;
                let p = sys.fs.unique("/home/Documents", "Untitled", ".txt");
                self.save(sys);
                sys.fs.write(&p, b"");
                self.rescan(sys);
                self.load(&p, sys);
            }
            C_SAVE => {
                self.dirty = true;
                self.save(sys);
                sys.toast("Notes", &format!("Saved {}", basename(&self.path)));
            }
            C_DELETE => {
                let dst = sys.fs.unique("/trash", basename(&self.path), "");
                sys.fs.rename(&self.path, &dst);
                self.dirty = false;
                self.rescan(sys);
                let next = self.list.first().cloned();
                match next {
                    Some(p) => self.load(&p, sys),
                    None => {
                        self.path = sys.fs.unique("/home/Documents", "Untitled", ".txt");
                        self.edit = TextArea::new(String::new());
                    }
                }
                sys.toast("Notes", "Moved to Bin");
            }
            c if c >= C_NOTE => {
                self.list_open = false;
                if let Some(p) = self.list.get((c - C_NOTE) as usize).cloned() {
                    self.load(&p, sys);
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, ctrl: bool, sys: &mut Sys) {
        if ctrl && matches!(k, Key::Char('s') | Key::Char('S')) {
            return self.action(C_SAVE, false, sys);
        }
        if ctrl {
            return;
        }
        if self.edit.key(k) {
            if !self.dirty {
                self.dirty = true;
                if !sys.fs.exists(&self.path) {
                    sys.fs.write(&self.path, b"");
                    self.rescan(sys);
                }
            }
        }
    }

    fn scroll(&mut self, dy: i32) {
        self.edit.scroll += dy * LINE * 2;
    }

    fn mouse(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => vec![("New Note", C_NEW), ("Save", C_SAVE), ("Move to Bin", C_DELETE)],
            _ => vec![],
        }
    }

    fn close(&mut self, sys: &mut Sys) {
        self.save(sys);
    }

    fn animating(&self) -> bool {
        true
    }

    fn open_path(&mut self, path: &str, sys: &mut Sys) {
        self.rescan(sys);
        if !self.list.iter().any(|p| p == path) {
            self.list.insert(0, path.to_string());
        }
        self.load(path, sys);
    }
}
