//! Files: browse, open, create, rename and bin files on the HydatekOS disk.

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::fs::{basename, join};
use crate::gfx::{Color, Rect};
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
pub const C_SET_WALL: u32 = 19;
const C_PLACE: u32 = 100;
/// Recent, in the sidebar (after the places and devices)
const C_RECENT_PLACE: u32 = C_PLACE + 8;
/// + a file in the Home folder's Recent list
const C_RECENT: u32 = 200;
/// the virtual folder of files opened lately
const RECENT: &str = "recent:";
const C_ITEM: u32 = 1000;

enum Focus {
    None,
    Search,
    Rename(LineEdit),
}

pub struct Files {
    /// icons per row, as last drawn (for ↑ and ↓)
    cols: usize,
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
        Files { cols: 1, path: "/home".to_string(), back: vec![], fwd: vec![], sel: None, search: LineEdit::default(), focus: Focus::None, scroll: 0, items: vec![] }
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
        if self.path == RECENT {
            return "Recent".to_string();
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
        } else if self.path == RECENT {
            // full paths, of files that are still there
            sys.recent.iter().filter(|r| sys.fs.exists(&r.0)).map(|r| (r.0.clone(), false, 0)).collect()
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
        if self.path == RECENT {
            return Some(it.0.clone());
        }
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
        let p = if self.path == RECENT { it.0.clone() } else { join(&self.path, &it.0) };
        if it.1 {
            self.go(&p);
        } else {
            sys.reqs.push(Req::OpenPath(p));
        }
    }

    fn delete(&mut self, sys: &mut Sys) {
        if self.path == RECENT {
            return;
        }
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
        let side_w = if compact { 0 } else { 170 };
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
            // the caret where it is; long text scrolls to keep it in view
            let cx = ui.tw(Face::Regular, 13, self.search.before_caret());
            let shift = (cx - (tr.w - 4)).max(0);
            let old = ui.clip_in(tr);
            let tw = ui.tw(Face::Regular, 13, &self.search.text);
            ui.text_in(Rect::new(tr.x - shift, tr.y, tw + 4, tr.h), Face::Regular, 13, &self.search.text, t.text, 0);
            if focused && (ui.ticks / 50) % 2 == 0 {
                ui.rect(Rect::new(tr.x - shift + cx, sr.y + 7, 1, 14), t.text);
            }
            ui.set_clip(old);
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
            let mut y = side.y + 10;
            let iw = side_w - 16;
            // Home and Recent, then the places, then devices
            let item = |ui: &mut Ui, y: &mut i32, icon: Icon, n: &str, on: bool, code: u32| {
                side_icon_item(ui, Rect::new(side.x + 8, *y, iw, 30), icon, n, on, Action::App(inst, code));
                *y += 31;
            };
            item(ui, &mut y, Icon::Home, "Home", self.path == PLACES[0].1, C_PLACE);
            item(ui, &mut y, Icon::Clock, "Recent", self.path == RECENT, C_RECENT_PLACE);
            y += 10;
            ui.label(side.x + 16, y + 10, 10, "PLACES", t.text3);
            y += 20;
            for (i, (n, p)) in PLACES.iter().enumerate().skip(1) {
                item(ui, &mut y, place_icon(n), n, self.path == *p, C_PLACE + i as u32);
            }
            y += 10;
            ui.label(side.x + 16, y + 10, 10, "DEVICES", t.text3);
            y += 20;
            for (i, (n, p)) in DEVICES.iter().enumerate() {
                item(ui, &mut y, if i == 0 { Icon::Laptop } else { Icon::Phone }, n, self.path == *p, C_PLACE + 6 + i as u32);
            }

        }

        // Content
        let area = Rect::new(r.x + side_w, top, r.w - side_w, r.b() - top);
        ui.zone(area, Action::App(inst, C_BG));
        let old = ui.clip_in(Rect::new(area.x, area.y, area.w, area.h - 30));
        let cols = ((area.w - 24) / if compact { 92 } else { 110 }).max(1);
        self.cols = cols as usize;
        let cw = (area.w - 24) / cols;
        let rows = (self.items.len() as i32 + cols - 1) / cols;
        // Home shows its folders, then the files opened lately
        let home = self.path == PLACES[0].1 && self.search.text.is_empty() && !compact;
        let head = if home { 30 } else { 0 };
        let recent: Vec<&(String, u64)> = if home { sys.recent.iter().filter(|r| sys.fs.exists(&r.0)).take(6).collect() } else { vec![] };
        let recent_h = if recent.is_empty() { 0 } else { 56 + recent.len() as i32 * 40 };
        let max_scroll = (head + rows * 118 + 24 + recent_h - (area.h - 30)).max(0);
        self.scroll = self.scroll.clamp(0, max_scroll);
        if home {
            ui.text(area.x + 24, area.y + 30 - self.scroll, Face::Semibold, 14, "Folders", t.text);
        }
        for (i, (name, dir, _)) in self.items.iter().enumerate() {
            let (cx, cy) = (i as i32 % cols, i as i32 / cols);
            let cell = Rect::new(area.x + 12 + cx * cw, area.y + 20 + head + cy * 118 - self.scroll, cw, 112);
            let a = Action::App(inst, C_ITEM + i as u32);
            let tile = Rect::new(cell.x + (cw - 54) / 2, cell.y + 10, 54, 54);
            let sel = self.sel == Some(i);
            if sel || ui.hot(a) {
                ui.rrect(Rect::new(cell.x + 6, cell.y + 2, cw - 12, 108), 12, if sel { t.accent.with_alpha(36) } else { t.hover });
            }
            let shown = if self.path == RECENT { basename(name) } else { name.as_str() };
            if *dir {
                let (glyph, col) = folder_style(&t, shown);
                folder(ui, tile, glyph, col);
            } else {
                ui.rrect(tile, 14, t.tile);
                let (ic, col) = file_style(&t, shown);
                ui.icon_in(ic, tile, 22, col);
            }
            let label = match &self.focus {
                Focus::Rename(s) if sel => s.text.clone(),
                _ => ui.fit(if *dir { Face::Medium } else { Face::Regular }, 13, display_name(shown), cw - 12),
            };
            if *dir && self.path != "phone:" {
                let n = sys.fs.list(&join(&self.path, name)).len();
                let count = format!("{} item{}", n, if n == 1 { "" } else { "s" });
                ui.text_in(Rect::new(cell.x + 4, cell.y + 92, cw - 8, 16), Face::Regular, 12, &count, t.text2, 1);
            }
            let lr = Rect::new(cell.x + 4, cell.y + 70, cw - 8, 22);
            if let (Focus::Rename(e), true) = (&self.focus, sel) {
                ui.rrect(lr, 6, t.surface);
                ui.stroke(lr, 6, 1, t.accent);
                let w = ui.text_in(lr, Face::Regular, 13, &label, t.text, 1);
                if (ui.ticks / 50) % 2 == 0 {
                    let cx = ui.tw(Face::Regular, 13, e.before_caret());
                    ui.rect(Rect::new(lr.x + (lr.w - w) / 2 + cx, lr.y + 4, 1, 14), t.text);
                }
            } else {
                ui.text_in(lr, if *dir { Face::Medium } else { Face::Regular }, 13, &label, t.text, 1);
            }
            ui.zone(cell.inset(4), a);
        }
        if !recent.is_empty() {
            let mut y = area.y + 20 + head + rows * 118 + 10 - self.scroll;
            ui.rect(Rect::new(area.x + 24, y, area.w - 48, 1), t.line);
            y += 30;
            ui.text(area.x + 24, y, Face::Semibold, 14, "Recent files", t.text);
            let a = Action::App(inst, C_RECENT_PLACE);
            let va = "View all";
            let vw = ui.tw(Face::Medium, 12, va);
            let vr = Rect::new(area.r() - 24 - vw - 22, y - 16, vw + 22, 22);
            ui.text(vr.x, y - 1, Face::Medium, 12, va, t.accent);
            ui.icon(Icon::ChevronRight, vr.r() - 14, y - 12, 12, t.accent);
            ui.zone(vr, a);
            y += 14;
            let wide = area.w - 48;
            for (i, (p, when)) in recent.iter().enumerate() {
                let row = Rect::new(area.x + 16, y, area.w - 32, 38);
                let a = Action::App(inst, C_RECENT + i as u32);
                if ui.hot(a) {
                    ui.rrect(row, 10, t.hover);
                }
                let name = basename(p);
                let (ic, col) = file_style(&t, name);
                let chip = Rect::new(row.x + 8, row.y + 7, 24, 24);
                ui.rrect(chip, 7, col);
                ui.icon_in(ic, chip, 14, Color::rgb(0xFFFFFF));
                let nw = wide * 45 / 100;
                let n = ui.fit(Face::Medium, 13, display_name(name), nw - 40);
                ui.text(row.x + 44, row.y + 24, Face::Medium, 13, &n, t.text);
                let parent = p.rsplit_once('/').map_or("", |(d, _)| d);
                let folder_name = if parent == "/home" { "Home" } else { basename(parent) };
                let f = ui.fit(Face::Regular, 12, folder_name, wide * 30 / 100 - 8);
                ui.text(row.x + 8 + nw, row.y + 24, Face::Regular, 12, &f, t.text2);
                let ago = sys.ago(*when);
                ui.text(row.x + 8 + nw + wide * 30 / 100, row.y + 24, Face::Regular, 12, &ago, t.text2);
                ui.zone(row, a);
                y += 40;
            }
        }
        if self.items.is_empty() {
            let msg = if self.path == RECENT {
                "Files you open show up here"
            } else if self.path == "phone:" && !sys.link.paired {
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
                self.focus = Focus::Rename(LineEdit::new(display_name(basename(&p))));
            }
            C_DELETE => self.delete(sys),
            C_RENAME => {
                if let Some(p) = self.selected_path() {
                    self.focus = Focus::Rename(LineEdit::new(display_name(basename(&p))));
                }
            }
            C_OPEN => {
                if let Some(i) = self.sel {
                    self.open(i, sys);
                }
            }
            C_SET_WALL => {
                match self.selected_path() {
                    Some(p) if crate::shell::wallpaper::picture(&sys.fs, &p).is_some() => {
                        sys.look.wall = crate::personal::Wall::Picture(p.clone());
                        sys.save_settings();
                        sys.toast("Wallpaper", &format!("{} is your wallpaper", basename(&p)));
                    }
                    Some(_) => sys.toast("Wallpaper", "That isn't a picture HydatekOS can show"),
                    None => sys.toast("Files", "Select a picture first"),
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
            C_RECENT_PLACE => self.go(RECENT),
            c if (C_RECENT..C_RECENT + 16).contains(&c) => {
                if let Some((p, _)) = sys.recent.get((c - C_RECENT) as usize) {
                    sys.reqs.push(Req::OpenPath(p.clone()));
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

    fn key(&mut self, k: Key, gen: bool, sys: &mut Sys) {
        match &mut self.focus {
            Focus::Search => {
                if k == Key::Esc || k == Key::Enter {
                    self.focus = Focus::None;
                } else if self.search.key_sys(k, gen, sys) {
                    self.sel = None;
                }
                return;
            }
            Focus::Rename(s) => {
                match k {
                    Key::Enter => {
                        let new = s.text.trim().to_string();
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
                    // no slashes in names
                    Key::Char('/') | Key::Char('\\') if !gen => {}
                    Key::Char(_) if !gen && s.text.chars().count() >= 60 => {}
                    _ => {
                        s.key_sys(k, gen, sys);
                        s.text.retain(|c| c != '/' && c != '\\');
                    }
                }
                return;
            }
            Focus::None => {}
        }
        let n = self.items.len();
        let cols = self.cols.max(1);
        let at = |sel: Option<usize>, to: usize| Some(sel.map(|_| to).unwrap_or(0).min(n.saturating_sub(1)));
        match k {
            Key::Right | Key::Tab if n > 0 => self.sel = Some(self.sel.map(|i| (i + 1).min(n - 1)).unwrap_or(0)),
            Key::Left if n > 0 => self.sel = Some(self.sel.map(|i| i.saturating_sub(1)).unwrap_or(0)),
            // a row at a time; Page keys three rows
            Key::Down if n > 0 => self.sel = at(self.sel, self.sel.map_or(0, |i| if i + cols < n { i + cols } else { i })),
            Key::Up if n > 0 => self.sel = at(self.sel, self.sel.map_or(0, |i| if i >= cols { i - cols } else { i })),
            Key::PageDown if n > 0 => self.sel = at(self.sel, self.sel.map_or(0, |i| (i + 3 * cols).min(n - 1))),
            Key::PageUp if n > 0 => self.sel = at(self.sel, self.sel.map_or(0, |i| i.saturating_sub(3 * cols))),
            Key::Home if n > 0 => self.sel = Some(0),
            Key::End if n > 0 => self.sel = Some(n - 1),
            Key::Enter => {
                if let Some(i) = self.sel {
                    self.open(i, sys);
                }
            }
            Key::Delete => self.delete(sys),
            Key::Backspace => self.action(C_BACK, false, sys),
            Key::F(2) => self.action(C_RENAME, false, sys),
            // typing starts a search (not while Gen is held: that's a shortcut)
            Key::Char(c) if !gen && !c.is_control() => {
                self.focus = Focus::Search;
                self.search.key(k);
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
                let mut v = vec![("New Folder", C_NEW_FOLDER), ("New Text File", C_NEW_FILE), ("Open", C_OPEN), ("Rename\tF2", C_RENAME), ("Send to Phone", C_SEND_PHONE), ("Set as Wallpaper", C_SET_WALL), ("Move to Bin\tDelete", C_DELETE)];
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

/// A sidebar entry with its icon.
fn side_icon_item(ui: &mut Ui, r: Rect, icon: Icon, label: &str, selected: bool, a: Action) {
    let t = ui.t;
    let (ink, face) = if selected { (t.on_accent, Face::Semibold) } else { (t.text, Face::Regular) };
    if selected {
        ui.rrect(r, 9, t.accent);
    } else if ui.hot(a) {
        ui.rrect(r, 9, t.hover);
    }
    ui.icon(icon, r.x + 10, r.y + (r.h - 16) / 2, 16, if selected { t.on_accent } else { t.text2 });
    ui.text_in(Rect::new(r.x + 36, r.y, r.w - 40, r.h), face, 13, label, ink, 0);
    ui.zone(r, a);
}

fn place_icon(name: &str) -> Icon {
    match name {
        "Documents" => Icon::Doc,
        "Pictures" => Icon::Image,
        "Downloads" => Icon::Download,
        "Shared" => Icon::Link,
        "Bin" => Icon::Trash,
        _ => Icon::Folder,
    }
}

/// A folder's picture and colour, by its name: warm for the everyday ones,
/// plum and slate for the rest, so a folder looks the same wherever it shows.
fn folder_style(t: &crate::theme::Theme, name: &str) -> (Icon, Color) {
    let lower = name.to_ascii_lowercase();
    let warm = t.accent;
    let light = t.accent.mix(t.sun, 110);
    let plum = Color::rgb(0x6E4462).mix(t.accent, 40);
    let violet = Color::rgb(0x6A5280);
    let navy = t.dune3.mix(Color::rgb(0x2B2A48), 128);
    let slate = Color::rgb(0x5D6079);
    let k = |w: &str| lower.contains(w);
    if k("desktop") {
        (Icon::Monitor, warm)
    } else if k("document") || k("school") || k("invoice") {
        (Icon::Doc, plum)
    } else if k("download") {
        (Icon::Download, warm)
    } else if k("picture") || k("photo") || k("image") {
        (Icon::Image, light)
    } else if k("music") || k("audio") {
        (Icon::Music, violet)
    } else if k("video") || k("movie") {
        (Icon::Video, slate)
    } else if k("project") || k("work") {
        (Icon::Folder, navy)
    } else if k("archive") || k("backup") || k("old") {
        (Icon::Archive, slate)
    } else if k("shared") {
        (Icon::Link, light)
    } else if k("present") || k("slide") {
        (Icon::Slides, warm)
    } else {
        let pick = [warm, plum, navy, light, violet, slate];
        let h = name.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
        (Icon::Folder, pick[h as usize % pick.len()])
    }
}

/// A file's icon and its colour (the Recent list shows them on a chip).
fn file_style(t: &crate::theme::Theme, name: &str) -> (Icon, Color) {
    let ic = file_icon(name, false);
    let lower = name.to_ascii_lowercase();
    let col = match ic {
        Icon::Scripts => Color::rgb(0x2F5DA8),
        Icon::Sheet => Color::rgb(0x2E7D4F),
        Icon::Slides => Color::rgb(0xC0562B),
        _ if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"].iter().any(|e| lower.ends_with(e)) => Color::rgb(0x6B47B8),
        Icon::Image => Color::rgb(0x6B47B8),
        _ => t.accent,
    };
    let ic = if col == Color::rgb(0x6B47B8) { Icon::Image } else { ic };
    (ic, col)
}

/// A folder drawn filled, in `col`, with `glyph` on its front.
fn folder(ui: &mut Ui, r: Rect, glyph: Icon, col: Color) {
    let back = col.mix(Color::rgb(0x000000), 50);
    let (x, y, w, h) = (r.x + 1, r.y + 5, r.w - 2, r.h - 8);
    ui.rrect(Rect::new(x, y, w * 2 / 5, 12), 5, back);
    ui.rrect(Rect::new(x, y + 5, w, h - 5), 8, back);
    let front = Rect::new(x, y + 11, w, h - 11);
    ui.rrect(front, 8, col);
    ui.rect(Rect::new(front.x + 6, front.y, front.w - 12, 1), col.mix(Color::rgb(0xFFFFFF), 70));
    ui.icon_in(glyph, front, 18, Color::rgba(0xFFFFFF, 235));
}
