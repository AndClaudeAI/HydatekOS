//! Files: browse, open, create, rename and bin files on the HydatekOS disk.

use super::{side_item, App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::fs::{basename, join};
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::{Req, Sys};
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const PLACES: [(&str, &str); 6] = [
    ("Home", "/home"),
    ("Documents", "/home/Documents"),
    ("Pictures", "/home/Pictures"),
    ("Downloads", "/home/Downloads"),
    ("Shared", "/home/Shared"),
    ("Bin", "/trash"),
];
const DEVICES: [(&str, &str); 2] = [("This laptop", "/"), ("My phone", "phone:")];

const C_BACK: u32 = 1;
const C_FWD: u32 = 2;
const C_SEARCH: u32 = 3;
const C_BG: u32 = 4;
pub const C_NEW_FOLDER: u32 = 10;
pub const C_NEW_FILE: u32 = 11;
pub const C_DELETE: u32 = 12;
pub const C_RENAME: u32 = 13;
pub const C_EMPTY_BIN: u32 = 14;
pub const C_RESTORE: u32 = 15;
pub const C_OPEN: u32 = 16;
pub const C_SEND_PHONE: u32 = 17;
const C_PLACE: u32 = 100;
const C_ITEM: u32 = 1000;

enum Focus {
    None,
    Search,
    Rename(String),
}

pub struct Files {
    path: String,
    back: Vec<String>,
    fwd: Vec<String>,
    sel: Option<usize>,
    search: LineEdit,
    focus: Focus,
    scroll: i32,
    items: Vec<(String, bool, usize)>,
}

pub fn display_name(name: &str) -> &str {
    name.strip_suffix(".txt").or_else(|| name.strip_suffix(".img")).unwrap_or(name)
}

pub fn file_icon(name: &str, dir: bool) -> Icon {
    if dir {
        Icon::Folder
    } else if [".sheet", ".hydg", ".xlsx", ".csv"].iter().any(|e| name.to_ascii_lowercase().ends_with(e)) {
        Icon::Sheet
    } else if name.ends_with(".img") {
        Icon::Image
    } else if [".hyds", ".docx"].iter().any(|e| name.to_ascii_lowercase().ends_with(e)) {
        Icon::Scripts
    } else if [".hydp", ".pptx"].iter().any(|e| name.to_ascii_lowercase().ends_with(e)) {
        Icon::Slides
    } else {
        Icon::Doc
    }
}

impl Files {
    pub fn new() -> Files {
        Files { path: "/home/Documents".to_string(), back: vec![], fwd: vec![], sel: None, search: LineEdit::default(), focus: Focus::None, scroll: 0, items: vec![] }
    }

    fn go(&mut self, p: &str) {
        if p != self.path {
            self.back.push(core::mem::replace(&mut self.path, p.to_string()));
            self.fwd.clear();
        }
        self.sel = None;
        self.scroll = 0;
        self.search.text.clear();
        self.focus = Focus::None;
    }

    fn title(&self) -> String {
        if self.path == "phone:" {
            return "My phone".to_string();
        }
        if self.path == "/" {
            return "This laptop".to_string();
        }
        for (n, p) in PLACES {
            if p == self.path {
                return n.to_string();
            }
        }
        basename(&self.path).to_string()
    }

    fn refresh(&mut self, sys: &Sys) {
        self.items = if self.path == "phone:" {
            if sys.link.paired {
                sys.link.photos.iter().map(|p| (p.name.clone(), false, 0)).collect()
            } else {
                vec![]
            }
        } else {
            sys.fs.list(&self.path)
        };
        let q = self.search.text.to_lowercase();
        if !q.is_empty() {
            self.items.retain(|i| i.0.to_lowercase().contains(&q));
        }
    }

    fn selected_path(&self) -> Option<String> {
        let i = self.sel?;
        let it = self.items.get(i)?;
        Some(join(&self.path, &it.0))
    }

    fn open(&mut self, i: usize, sys: &mut Sys) {
        let Some(it) = self.items.get(i).cloned() else { return };
        if self.path == "phone:" {
            // Copy the photo from the phone to Pictures.
            if sys.link.request_photo(i) {
                sys.toast("Phone Link", "Copying the photo from your phone...");
            } else {
                let dst = sys.fs.unique("/home/Pictures", display_name(&it.0), ".img");
                sys.fs.write(&dst, b"HYDATEK-IMAGE");
                sys.toast("Phone Link", &format!("Saved {} to Pictures", display_name(&it.0)));
            }
            return;
        }
        let p = join(&self.path, &it.0);
        if it.1 {
            self.go(&p);
        } else {
            sys.reqs.push(Req::OpenPath(p));
        }
    }

    fn delete(&mut self, sys: &mut Sys) {
        let Some(p) = self.selected_path() else { return };
        if self.path == "/trash" {
            sys.fs.remove(&p);
        } else {
            let dst = sys.fs.unique("/trash", basename(&p), "");
            sys.fs.rename(&p, &dst);
        }
        self.sel = None;
    }
}

impl App for Files {
    fn kind(&self) -> AppKind {
        AppKind::Files
    }


    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        self.refresh(sys);
        let t = ui.t;
        let compact = super::compact(r);
        let side_w = if compact { 0 } else { 150 };
        // Header
        ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);
        let b = Rect::new(r.x + 14, r.y + 8, 28, 28);
        ui.icon_button(b, Icon::ChevronLeft, Action::App(inst, C_BACK), 16);
        let f = Rect::new(r.x + 46, r.y + 8, 28, 28);
        ui.icon_button(f, Icon::ChevronRight, Action::App(inst, C_FWD), 16);
        if self.back.is_empty() {
            ui.rrect(b, 8, t.surface.with_alpha(150));
        }
        if self.fwd.is_empty() {
            ui.rrect(f, 8, t.surface.with_alpha(150));
        }
        let title = self.title();
        let sx = (r.x + r.w / 2 - 105).max(r.x + 90 + ui.tw(Face::Semibold, 16, &title) + 20);
        let sw = (r.r() - 130 - sx).min(230);
        ui.text_in(Rect::new(r.x + 84, r.y, sx - r.x - 90, HEADER), Face::Semibold, 16, &ui.fit(Face::Semibold, 16, &title, sx - r.x - 100), t.text, 0);
        if sw > 110 {
            let sr = Rect::new(sx, r.y + 8, sw, 28);
            let ph = format!("Search {}", title);
            let focused = matches!(self.focus, Focus::Search);
            ui.rrect(sr, 10, t.chip);
            if focused {
                ui.stroke(sr, 10, 1, t.accent);
            }
            ui.icon(Icon::Search, sr.x + 12, sr.y + 8, 12, t.text2);
            let tr = Rect::new(sr.x + 32, sr.y, sr.w - 40, sr.h);
            if self.search.text.is_empty() {
                let ph = ui.fit(Face::Regular, 13, &ph, tr.w);
                ui.text_in(tr, Face::Regular, 13, &ph, t.text3, 0);
            }
            let w = ui.text_in(tr, Face::Regular, 13, &self.search.text, t.text, 0);
            if focused && (ui.ticks / 50) % 2 == 0 {
                ui.rect(Rect::new(tr.x + w + 1, sr.y + 7, 1, 14), t.text);
            }
            ui.zone(sr, Action::App(inst, C_SEARCH));
        }

        // Places: chips on phones, sidebar on desktops
        let mut top = r.y + HEADER + 1;
        if compact {
            let mut x = r.x + 12;
            for (i, (n, p)) in PLACES.iter().enumerate() {
                let n = if *n == "Documents" { "Docs" } else { n };
                let w = ui.tw(Face::Medium, 12, n) + 20;
                if x + w > r.r() - 8 {
                    break;
                }
                let c = Rect::new(x, top + 8, w, 26);
                let sel = self.path == *p;
                ui.rrect(c, 13, if sel { t.accent } else { t.chip });
                ui.text_in(c, Face::Medium, 12, n, if sel { t.on_accent } else { t.text }, 1);
                ui.zone(c, Action::App(inst, C_PLACE + i as u32));
                x += w + 6;
            }
            top += 40;
        }
        let side = Rect::new(r.x, r.y + HEADER + 1, side_w, r.h - HEADER - 1);
        if !compact {
            super::panel(ui, r, side, t.sidebar);
            let mut y = side.y + 14;
            ui.label(side.x + 16, y + 10, 10, "PLACES", t.text2);
            y += 20;
            for (i, (n, p)) in PLACES.iter().enumerate() {
                side_item(ui, Rect::new(side.x + 8, y, side_w - 16, 26), n, self.path == *p, Action::App(inst, C_PLACE + i as u32));
                y += 27;
            }
            y += 12;
            ui.label(side.x + 16, y + 10, 10, "DEVICES", t.text2);
            y += 20;
            for (i, (n, p)) in DEVICES.iter().enumerate() {
                side_item(ui, Rect::new(side.x + 8, y, side_w - 16, 26), n, self.path == *p, Action::App(inst, C_PLACE + 6 + i as u32));
                y += 27;
            }

        }

        // Content
        let area = Rect::new(r.x + side_w, top, r.w - side_w, r.b() - top);
        ui.zone(area, Action::App(inst, C_BG));
        let old = ui.clip_in(Rect::new(area.x, area.y, area.w, area.h - 30));
        let cols = ((area.w - 24) / if compact { 92 } else { 110 }).max(1);
        let cw = (area.w - 24) / cols;
        let rows = (self.items.len() as i32 + cols - 1) / cols;
        let max_scroll = (rows * 118 + 24 - (area.h - 30)).max(0);
        self.scroll = self.scroll.clamp(0, max_scroll);
        for (i, (name, dir, _)) in self.items.iter().enumerate() {
            let (cx, cy) = (i as i32 % cols, i as i32 / cols);
            let cell = Rect::new(area.x + 12 + cx * cw, area.y + 20 + cy * 118 - self.scroll, cw, 112);
            let a = Action::App(inst, C_ITEM + i as u32);
            let tile = Rect::new(cell.x + (cw - 54) / 2, cell.y + 12, 54, 54);
            let sel = self.sel == Some(i);
            if sel || ui.hot(a) {
                ui.rrect(Rect::new(cell.x + 6, cell.y + 4, cw - 12, 104), 12, if sel { t.accent.with_alpha(36) } else { t.hover });
            }
            ui.rrect(tile, 14, t.tile);
            let ic = file_icon(name, *dir);
            ui.icon_in(ic, tile, 22, if *dir { t.accent } else { t.text });
            let label = match &self.focus {
                Focus::Rename(s) if sel => s.clone(),
                _ => ui.fit(Face::Regular, 13, display_name(name), cw - 12),
            };
            let lr = Rect::new(cell.x + 4, cell.y + 74, cw - 8, 22);
            if matches!(self.focus, Focus::Rename(_)) && sel {
                ui.rrect(lr, 6, t.surface);
                ui.stroke(lr, 6, 1, t.accent);
                let w = ui.text_in(lr, Face::Regular, 13, &label, t.text, 1);
                if (ui.ticks / 50) % 2 == 0 {
                    ui.rect(Rect::new(lr.x + (lr.w + w) / 2 + 1, lr.y + 4, 1, 14), t.text);
                }
            } else {
                ui.text_in(lr, Face::Regular, 13, &label, t.text, 1);
            }
            ui.zone(cell.inset(4), a);
        }
        if self.items.is_empty() {
            let msg = if self.path == "phone:" && !sys.link.paired {
                "Pair your phone in Phone Link to browse its photos"
            } else if !self.search.text.is_empty() {
                "No matching items"
            } else {
                "This folder is empty"
            };
            ui.text_in(Rect::new(area.x, area.y + 60, area.w, 30), Face::Regular, 14, msg, t.text3, 1);
        }
        ui.set_clip(old);
        let n = self.items.len();
        let mut status = format!("{} item{}", n, if n == 1 { "" } else { "s" });
        if self.path == "phone:" && n > 0 {
            status.push_str(" · double-click a photo to copy it to Pictures");
        } else if self.path == "/trash" && n > 0 {
            status.push_str(" · File menu › Empty Bin");
        }
        if !sys.fs.persistent {
            status.push_str("  ·  Live session (changes are not saved to disk)");
        }
        ui.text(area.x + 20, area.b() - 12, Face::Regular, 12, &status, t.text2);
    }

    fn action(&mut self, code: u32, double: bool, sys: &mut Sys) {
        if !matches!(code, C_SEARCH) && matches!(self.focus, Focus::Search) {
            self.focus = Focus::None;
        }
        if let Focus::Rename(_) = self.focus {
            if code != C_ITEM + self.sel.unwrap_or(usize::MAX) as u32 {
                self.key(Key::Enter, false, sys);
            }
        }
        match code {
            C_BACK => {
                if let Some(p) = self.back.pop() {
                    self.fwd.push(core::mem::replace(&mut self.path, p));
                    self.sel = None;
                }
            }
            C_FWD => {
                if let Some(p) = self.fwd.pop() {
                    self.back.push(core::mem::replace(&mut self.path, p));
                    self.sel = None;
                }
            }
            C_SEARCH => self.focus = Focus::Search,
            C_BG => self.sel = None,
            C_NEW_FOLDER | C_NEW_FILE if self.path.starts_with('/') && self.path != "/" => {
                let p = if code == C_NEW_FOLDER {
                    let p = sys.fs.unique(&self.path, "New folder", "");
                    sys.fs.mkdir(&p);
                    p
                } else {
                    let p = sys.fs.unique(&self.path, "Untitled", ".txt");
                    sys.fs.write(&p, b"");
                    p
                };
                self.refresh(sys);
                self.sel = self.items.iter().position(|i| i.0 == basename(&p));
                self.focus = Focus::Rename(display_name(basename(&p)).to_string());
            }
            C_DELETE => self.delete(sys),
            C_RENAME => {
                if let Some(p) = self.selected_path() {
                    self.focus = Focus::Rename(display_name(basename(&p)).to_string());
                }
            }
            C_OPEN => {
                if let Some(i) = self.sel {
                    self.open(i, sys);
                }
            }
            C_SEND_PHONE => {
                match self.selected_path() {
                    Some(p) if !sys.fs.is_dir(&p) => {
                        let data = sys.fs.read(&p).unwrap_or_default();
                        let name = basename(&p).to_string();
                        if sys.link.send_file(&name, data) {
                            sys.toast("Phone Link", &format!("Sending {} to your phone", name));
                        } else {
                            sys.toast("Phone Link", "Connect your phone in Phone Link first");
                        }
                    }
                    _ => sys.toast("Files", "Select a file to send"),
                }
            }
            C_EMPTY_BIN => {
                for (n, _, _) in sys.fs.list("/trash") {
                    sys.fs.remove(&join("/trash", &n));
                }
                sys.toast("Bin", "Bin emptied");
            }
            C_RESTORE => {
                if self.path == "/trash" {
                    if let Some(p) = self.selected_path() {
                        let dst = sys.fs.unique("/home/Documents", basename(&p), "");
                        sys.fs.rename(&p, &dst);
                        sys.toast("Files", "Restored to Documents");
                    }
                }
            }
            c if (C_PLACE..C_PLACE + 8).contains(&c) => {
                let i = (c - C_PLACE) as usize;
                let p = if i < 6 { PLACES[i].1 } else { DEVICES[i - 6].1 };
                self.go(p);
            }
            c if c >= C_ITEM => {
                let i = (c - C_ITEM) as usize;
                if double {
                    self.open(i, sys);
                } else {
                    self.sel = Some(i);
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, _ctrl: bool, sys: &mut Sys) {
        match &mut self.focus {
            Focus::Search => {
                if k == Key::Esc || k == Key::Enter {
                    self.focus = Focus::None;
                } else if self.search.key(k) {
                    self.sel = None;
                }
                return;
            }
            Focus::Rename(s) => {
                match k {
                    Key::Enter => {
                        let new = s.trim().to_string();
                        self.focus = Focus::None;
                        if let Some(p) = self.selected_path() {
                            let old = basename(&p).to_string();
                            let ext = if old.ends_with(".txt") && !new.contains('.') { ".txt" } else if old.ends_with(".img") && !new.contains('.') { ".img" } else { "" };
                            let np = join(&self.path, &format!("{}{}", new, ext));
                            if !new.is_empty() && np != p && !new.contains('/') {
                                sys.fs.rename(&p, &np);
                                self.refresh(sys);
                                self.sel = self.items.iter().position(|i| join(&self.path, &i.0) == np);
                            }
                        }
                    }
                    Key::Esc => self.focus = Focus::None,
                    Key::Backspace => {
                        s.pop();
                    }
                    Key::Char(c) if !c.is_control() && c != '/' && c != '\\' && s.len() < 60 => s.push(c),
                    _ => {}
                }
                return;
            }
            Focus::None => {}
        }
        let n = self.items.len();
        match k {
            Key::Right | Key::Tab if n > 0 => self.sel = Some(self.sel.map(|i| (i + 1).min(n - 1)).unwrap_or(0)),
            Key::Left if n > 0 => self.sel = Some(self.sel.map(|i| i.saturating_sub(1)).unwrap_or(0)),
            Key::Enter => {
                if let Some(i) = self.sel {
                    self.open(i, sys);
                }
            }
            Key::Delete => self.delete(sys),
            Key::Backspace => self.action(C_BACK, false, sys),
            Key::F(2) => self.action(C_RENAME, false, sys),
            Key::Char(c) if !c.is_control() => {
                self.focus = Focus::Search;
                self.search.key(k);
                let _ = c;
            }
            _ => {}
        }
    }

    fn scroll(&mut self, dy: i32) {
        self.scroll += dy * 40;
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => {
                let mut v = vec![("New Folder", C_NEW_FOLDER), ("New Text File", C_NEW_FILE), ("Open", C_OPEN), ("Rename", C_RENAME), ("Send to Phone", C_SEND_PHONE), ("Move to Bin", C_DELETE)];
                if self.path == "/trash" {
                    v = vec![("Restore", C_RESTORE), ("Delete Permanently", C_DELETE), ("Empty Bin", C_EMPTY_BIN)];
                }
                v
            }
            3 => vec![("Home", C_PLACE), ("Documents", C_PLACE + 1), ("Pictures", C_PLACE + 2), ("Downloads", C_PLACE + 3), ("Bin", C_PLACE + 5), ("Back", C_BACK)],
            _ => vec![],
        }
    }

    fn animating(&self) -> bool {
        !matches!(self.focus, Focus::None)
    }

    fn open_path(&mut self, path: &str, _sys: &mut Sys) {
        self.go(path);
    }
}
