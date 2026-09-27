//! Hyda Grids, the Hyda Workspace spreadsheet: a grid of cells with formulas,
//! number formats, bold / italic, alignment, copy and paste that moves
//! references, AutoSum, column widths and undo. Sheets are saved in Hyda
//! Grids' own format (.hydg); Excel (.xlsx) and CSV files open for viewing and
//! editing, and are exported to, but never saved over.

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::fs::{basename, join, parent};
use crate::gfx::{Color, Rect};
use crate::grid::*;
use crate::gridio;
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const TOOLBAR: i32 = 46;
const FBAR: i32 = 38;
const STATUS: i32 = 28;
const COL_HDR: i32 = 24;
const ROW_HDR: i32 = 46;
const ROW_H: i32 = 26;
const TEXT: i32 = 13;
const PAPER: Color = Color::rgb(0xFFFFFF);
const INK: Color = Color::rgb(0x1E1B2C);
const GRIDLINE: Color = Color::rgb(0xE4DED3);

const C_GRID: u32 = 1;
const C_COLHDR: u32 = 2;
const C_ROWHDR: u32 = 3;
const C_CORNER: u32 = 4;
const C_BOLD: u32 = 5;
const C_ITALIC: u32 = 6;
const C_LEFT: u32 = 7;
const C_CENTER: u32 = 8;
const C_RIGHT: u32 = 9;
const C_GENERAL: u32 = 10;
const C_NUMBER: u32 = 11;
const C_CURRENCY: u32 = 12;
const C_PERCENT: u32 = 13;
const C_AUTOSUM: u32 = 14;
const C_UNDO: u32 = 15;
const C_REDO: u32 = 16;
const C_BAR: u32 = 17;
const C_NEW: u32 = 18;
const C_OPEN: u32 = 19;
const C_SAVE: u32 = 20;
const C_RENAME: u32 = 21;
const C_EXPORT_XLSX: u32 = 22;
const C_EXPORT_CSV: u32 = 23;
const C_CUT: u32 = 24;
const C_COPY: u32 = 25;
const C_PASTE: u32 = 26;
const C_ALL: u32 = 27;
const C_CLEAR: u32 = 28;
const C_DISMISS: u32 = 29;
const C_FILE: u32 = 200;

struct Edit {
    text: String,
    /// caret, in characters
    caret: usize,
    /// typing into the formula bar (arrows move the caret there)
    in_bar: bool,
}

enum Overlay {
    None,
    Open(Vec<String>),
    Rename(LineEdit),
}

pub struct Grids {
    sheet: Sheet,
    path: String,
    dirty: bool,
    /// opened from .xlsx / .csv: saving writes a new .hydg next to it
    imported: bool,
    cur: (u32, u32),
    anchor: Option<(u32, u32)>,
    edit: Option<Edit>,
    /// scroll offsets in logical px
    sx: i32,
    sy: i32,
    undo: Vec<(Sheet, (u32, u32))>,
    redo: Vec<(Sheet, (u32, u32))>,
    /// copied block: top-left and cells (for pasting with moved references)
    clip: Option<((u32, u32), Vec<Vec<Cell>>)>,
    clip_text: String,
    area: Rect,
    mouse: (i32, i32),
    press: Option<(u32, u32)>,
    /// column being resized: (column, pointer x at start, width at start)
    resize: Option<(u32, i32, i32)>,
    want_visible: bool,
    overlay: Overlay,
}

impl Grids {
    pub fn new() -> Grids {
        Grids {
            sheet: Sheet::new(),
            path: String::new(),
            dirty: false,
            imported: false,
            cur: (0, 0),
            anchor: None,
            edit: None,
            sx: 0,
            sy: 0,
            undo: vec![],
            redo: vec![],
            clip: None,
            clip_text: String::new(),
            area: Rect::default(),
            mouse: (0, 0),
            press: None,
            resize: None,
            want_visible: false,
            overlay: Overlay::None,
        }
    }

    // ---- geometry --------------------------------------------------------------

    fn col_x(&self, c: u32) -> i32 {
        (0..c).map(|k| self.sheet.width(k)).sum()
    }

    fn cells_origin(&self) -> (i32, i32) {
        (self.area.x + ROW_HDR - self.sx, self.area.y + COL_HDR - self.sy)
    }

    fn cell_rect(&self, r: u32, c: u32) -> Rect {
        let (ox, oy) = self.cells_origin();
        Rect::new(ox + self.col_x(c), oy + r as i32 * ROW_H, self.sheet.width(c), ROW_H)
    }

    fn col_at(&self, x: i32) -> u32 {
        let (ox, _) = self.cells_origin();
        let mut left = ox;
        for c in 0..MAX_COLS {
            let w = self.sheet.width(c);
            if x < left + w {
                return c;
            }
            left += w;
        }
        MAX_COLS - 1
    }

    fn row_at(&self, y: i32) -> u32 {
        let (_, oy) = self.cells_origin();
        (((y - oy).max(0)) / ROW_H).min(MAX_ROWS as i32 - 1) as u32
    }

    fn ensure_visible(&mut self) {
        let (r, c) = self.cur;
        let (vw, vh) = (self.area.w - ROW_HDR, self.area.h - COL_HDR);
        let (x, w) = (self.col_x(c), self.sheet.width(c));
        let y = r as i32 * ROW_H;
        if x < self.sx {
            self.sx = x;
        } else if x + w > self.sx + vw {
            self.sx = x + w - vw;
        }
        if y < self.sy {
            self.sy = y;
        } else if y + ROW_H > self.sy + vh {
            self.sy = y + ROW_H - vh;
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        let max_x = (self.col_x(MAX_COLS) - (self.area.w - ROW_HDR)).max(0);
        let max_y = (MAX_ROWS as i32 * ROW_H - (self.area.h - COL_HDR)).max(0);
        self.sx = self.sx.clamp(0, max_x);
        self.sy = self.sy.clamp(0, max_y);
    }

    // ---- selection -------------------------------------------------------------

    fn sel(&self) -> (u32, u32, u32, u32) {
        let a = self.anchor.unwrap_or(self.cur);
        (a.0.min(self.cur.0), a.1.min(self.cur.1), a.0.max(self.cur.0), a.1.max(self.cur.1))
    }

    fn select(&mut self, to: (u32, u32), extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.cur = (to.0.min(MAX_ROWS - 1), to.1.min(MAX_COLS - 1));
        self.want_visible = true;
    }

    fn step(&mut self, dr: i32, dc: i32, extend: bool) {
        let r = (self.cur.0 as i32 + dr).clamp(0, MAX_ROWS as i32 - 1) as u32;
        let c = (self.cur.1 as i32 + dc).clamp(0, MAX_COLS as i32 - 1) as u32;
        self.select((r, c), extend);
    }

    // ---- editing ----------------------------------------------------------------

    fn snapshot(&mut self) {
        self.undo.push((self.sheet.clone(), self.cur));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    fn start_edit(&mut self, first: Option<char>, in_bar: bool) {
        let text = match first {
            Some(c) => c.to_string(),
            None => self.sheet.input(self.cur.0, self.cur.1).to_string(),
        };
        let caret = text.chars().count();
        self.edit = Some(Edit { text, caret, in_bar });
        self.want_visible = true;
    }

    fn commit(&mut self) {
        let Some(e) = self.edit.take() else { return };
        let (r, c) = self.cur;
        if e.text == self.sheet.input(r, c) {
            return;
        }
        self.snapshot();
        let (input, hint) = typed(&e.text);
        self.sheet.set_input(r, c, &input);
        if let Some((num, sym)) = hint {
            let mut f = self.sheet.fmt(r, c);
            if f.num == Num::General {
                f.num = num;
                f.sym = sym;
                self.sheet.set_fmt(r, c, f);
            }
        }
    }

    fn each_selected(&mut self, mut f: impl FnMut(&mut Fmt)) {
        self.snapshot();
        let (r1, c1, r2, c2) = self.sel();
        // whole rows/columns: only touch the used part
        let (ur, uc) = self.sheet.used();
        for r in r1..=r2.min(r1.max(ur)) {
            for c in c1..=c2.min(c1.max(uc)) {
                let mut x = self.sheet.fmt(r, c);
                f(&mut x);
                self.sheet.set_fmt(r, c, x);
            }
        }
    }

    fn toggle_bold(&mut self) {
        let on = !self.sheet.fmt(self.cur.0, self.cur.1).bold;
        self.each_selected(|f| f.bold = on);
    }

    fn toggle_italic(&mut self) {
        let on = !self.sheet.fmt(self.cur.0, self.cur.1).italic;
        self.each_selected(|f| f.italic = on);
    }

    fn clear(&mut self) {
        self.snapshot();
        let (r1, c1, r2, c2) = self.sel();
        self.sheet.cells.retain(|&(r, c), _| !(r >= r1 && r <= r2 && c >= c1 && c <= c2));
    }

    fn undo_redo(&mut self, undo: bool) {
        self.edit = None;
        let (from, to) = if undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        if let Some((s, cur)) = from.pop() {
            to.push((core::mem::replace(&mut self.sheet, s), self.cur));
            self.cur = cur;
            self.anchor = None;
            self.dirty = true;
            self.want_visible = true;
        }
    }

    fn copy(&mut self, sys: &mut Sys) {
        let (r1, c1, r2, c2) = self.sel();
        let (ur, uc) = self.sheet.used();
        let (r2, c2) = (r2.min(ur.max(r1 + 1) - 1).max(r1), c2.min(uc.max(c1 + 1) - 1).max(c1));
        let mut calc = Calc::new(&self.sheet);
        let mut block = Vec::new();
        let mut text = String::new();
        for r in r1..=r2 {
            let mut row = Vec::new();
            for c in c1..=c2 {
                row.push(self.sheet.cells.get(&(r, c)).cloned().unwrap_or_default());
                if c > c1 {
                    text.push('\t');
                }
                text.push_str(&display(&calc.value(r, c), &self.sheet.fmt(r, c)));
            }
            if r < r2 {
                text.push('\n');
            }
            block.push(row);
        }
        self.clip = Some(((r1, c1), block));
        self.clip_text = text.clone();
        sys.clipboard = text;
    }

    fn paste(&mut self, sys: &mut Sys) {
        let (r0, c0) = (self.sel().0, self.sel().1);
        self.snapshot();
        match &self.clip {
            // our own copy: formulas keep pointing at the same relative cells
            Some(((or, oc), block)) if sys.clipboard == self.clip_text => {
                let (dr, dc) = (r0 as i64 - *or as i64, c0 as i64 - *oc as i64);
                let block = block.clone();
                for (i, row) in block.iter().enumerate() {
                    for (j, cell) in row.iter().enumerate() {
                        let (r, c) = (r0 + i as u32, c0 + j as u32);
                        if r < MAX_ROWS && c < MAX_COLS {
                            self.sheet.cells.remove(&(r, c));
                            if !(cell.input.is_empty() && cell.fmt == Fmt::default()) {
                                self.sheet.cells.insert((r, c), Cell { input: shift(&cell.input, dr, dc), fmt: cell.fmt });
                            }
                        }
                    }
                }
            }
            // text from elsewhere: tabs split columns, lines split rows
            _ => {
                let text = sys.clipboard.clone();
                for (i, line) in text.split('\n').enumerate() {
                    for (j, field) in line.trim_end_matches('\r').split('\t').enumerate() {
                        let (r, c) = (r0 + i as u32, c0 + j as u32);
                        if r < MAX_ROWS && c < MAX_COLS {
                            let (input, _) = typed(field);
                            self.sheet.set_input(r, c, &input);
                        }
                    }
                }
            }
        }
    }

    /// Σ: sum the numbers above (or to the left of) the selected cell; with a
    /// range selected, add a total under each column.
    fn autosum(&mut self) {
        let (r1, c1, r2, c2) = self.sel();
        let mut calc = Calc::new(&self.sheet);
        let is_num = |calc: &mut Calc, r: u32, c: u32| matches!(calc.value(r, c), Val::Num(_));
        if r1 == r2 && c1 == c2 {
            let (r, c) = (r1, c1);
            let mut top = r;
            while top > 0 && is_num(&mut calc, top - 1, c) {
                top -= 1;
            }
            let f = if top < r {
                format!("=SUM({}:{})", cell_name(top, c), cell_name(r - 1, c))
            } else {
                let mut left = c;
                while left > 0 && is_num(&mut calc, r, left - 1) {
                    left -= 1;
                }
                if left < c {
                    format!("=SUM({}:{})", cell_name(r, left), cell_name(r, c - 1))
                } else {
                    String::from("=SUM()")
                }
            };
            // the total takes the number format of what it adds up
            if let Some(src) = (top < r).then(|| (r - 1, c)) {
                let (mut f0, sf) = (self.sheet.fmt(r, c), self.sheet.fmt(src.0, src.1));
                if f0.num == Num::General && sf.num != Num::General {
                    f0.num = sf.num;
                    f0.sym = sf.sym;
                    self.snapshot();
                    self.sheet.set_fmt(r, c, f0);
                }
            }
            let caret = if f.ends_with("()") { f.len() - 1 } else { f.len() };
            self.edit = Some(Edit { text: f, caret, in_bar: false });
            return;
        }
        drop(calc);
        self.snapshot();
        let row = (r2 + 1).min(MAX_ROWS - 1);
        for c in c1..=c2 {
            self.sheet.set_input(row, c, &format!("=SUM({}:{})", cell_name(r1, c), cell_name(r2, c)));
            let (mut f0, sf) = (self.sheet.fmt(row, c), self.sheet.fmt(r2, c));
            if f0.num == Num::General && sf.num != Num::General {
                f0.num = sf.num;
                f0.sym = sf.sym;
                self.sheet.set_fmt(row, c, f0);
            }
        }
        self.anchor = None;
        self.cur = (row, c1);
    }

    // ---- files --------------------------------------------------------------

    fn title(&self) -> String {
        if self.path.is_empty() {
            return String::from("Untitled sheet");
        }
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[..k]).unwrap_or(b).to_string()
    }

    fn ext(&self) -> &str {
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[k..]).unwrap_or("")
    }

    fn save(&mut self, sys: &mut Sys, quiet: bool) {
        self.commit();
        if self.path.is_empty() {
            let name = match self.sheet.name.as_str() {
                "Sheet1" | "" => "Untitled sheet",
                n => n,
            };
            self.path = sys.fs.unique("/home/Documents", name, ".hydg");
        }
        let mut from = None;
        if self.imported {
            from = Some(self.ext().to_string());
            self.path = sys.fs.unique(&parent(&self.path), &self.title(), ".hydg");
            self.imported = false;
        }
        let ok = sys.fs.write(&self.path, gridio::to_hydg(&self.sheet).as_bytes());
        if ok {
            self.dirty = false;
        }
        if !quiet {
            let msg = if !ok {
                String::from("Couldn't save: the disk is read-only")
            } else if let Some(ext) = from {
                format!("Saved as {} (the {} file is unchanged)", basename(&self.path), ext)
            } else {
                format!("Saved {}", basename(&self.path))
            };
            sys.toast("Hyda Grids", &msg);
        }
    }

    fn export(&mut self, sys: &mut Sys, ext: &str) {
        self.commit();
        let dir = if self.path.is_empty() { String::from("/home/Documents") } else { parent(&self.path) };
        let name = if self.path.is_empty() { String::from("Untitled sheet") } else { self.title() };
        let target = join(&dir, &format!("{}{}", name, ext));
        let path = if sys.fs.exists(&target) { sys.fs.unique(&dir, &name, ext) } else { target };
        let data = if ext == ".csv" { gridio::to_csv(&self.sheet).into_bytes() } else { gridio::to_xlsx(&self.sheet) };
        sys.fs.write(&path, &data);
        sys.toast("Hyda Grids", &format!("Exported {}", basename(&path)));
    }

    fn park(&mut self, sys: &mut Sys) {
        self.commit();
        if self.dirty && (!self.path.is_empty() || !self.sheet.cells.is_empty()) {
            self.save(sys, true);
        }
    }

    fn reset(&mut self, s: Sheet) {
        self.sheet = s;
        self.path.clear();
        self.dirty = false;
        self.imported = false;
        self.cur = (0, 0);
        self.anchor = None;
        self.edit = None;
        self.sx = 0;
        self.sy = 0;
        self.undo.clear();
        self.redo.clear();
    }

    fn load(&mut self, path: &str, sys: &mut Sys) {
        let Some(data) = sys.fs.read(path) else {
            sys.toast("Hyda Grids", &format!("Couldn't open {}", basename(path)));
            return;
        };
        let lower = path.to_ascii_lowercase();
        let parsed = if lower.ends_with(".hydg") {
            gridio::from_hydg(&data)
        } else if lower.ends_with(".xlsx") {
            gridio::from_xlsx(&data)
        } else {
            Ok(gridio::from_csv(&String::from_utf8_lossy(&data)))
        };
        match parsed {
            Ok(s) => {
                self.park(sys);
                self.reset(s);
                self.path = path.to_string();
                self.imported = !lower.ends_with(".hydg");
            }
            Err(why) => sys.toast("Hyda Grids", &format!("Can't open {}: {}", basename(path), why)),
        }
    }

    fn sheets(sys: &Sys) -> Vec<String> {
        let (mut own, mut other) = (Vec::new(), Vec::new());
        for dir in ["/home/Documents", "/home", "/home/Downloads", "/home/Shared"] {
            for (name, d, _) in sys.fs.list(dir) {
                let l = name.to_ascii_lowercase();
                if d {
                    continue;
                }
                if l.ends_with(".hydg") {
                    own.push(join(dir, &name));
                } else if l.ends_with(".xlsx") || l.ends_with(".csv") {
                    other.push(join(dir, &name));
                }
            }
        }
        own.extend(other);
        own
    }

    fn rename_to(&mut self, name: &str, sys: &mut Sys) {
        let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        self.sheet.name = name.to_string();
        let dir = if self.path.is_empty() { String::from("/home/Documents") } else { parent(&self.path) };
        if self.imported || self.path.is_empty() {
            self.path = sys.fs.unique(&dir, name, ".hydg");
            self.imported = false;
            self.save(sys, true);
            return;
        }
        let target = join(&dir, &format!("{}.hydg", name));
        if target != self.path {
            let target = if sys.fs.exists(&target) { sys.fs.unique(&dir, name, ".hydg") } else { target };
            if sys.fs.exists(&self.path) {
                sys.fs.rename(&self.path, &target);
            }
            self.path = target;
        }
        self.dirty = true;
        self.save(sys, true);
    }

    // ---- drawing ------------------------------------------------------------

    fn tool(&self, ui: &mut Ui, r: Rect, label: &str, icon: Option<Icon>, face: Face, code: u32, inst: u32, on: bool) {
        let t = ui.t;
        let a = Action::App(inst, code);
        if on {
            ui.rrect(r, 8, t.accent.with_alpha(45));
        } else if ui.hot(a) {
            ui.rrect(r, 8, t.hover);
        }
        let col = if on { t.accent } else { t.text };
        match icon {
            Some(i) => ui.icon_in(i, r, 16, col),
            None => {
                ui.text_in(r, face, 14, label, col, 1);
            }
        }
        ui.zone(r, a);
    }

    fn render_toolbar(&self, ui: &mut Ui, r: Rect, inst: u32, compact: bool) {
        let t = ui.t;
        let y = r.y + HEADER;
        ui.rect(Rect::new(r.x, y, r.w, TOOLBAR), t.surface);
        let f = self.sheet.fmt(self.cur.0, self.cur.1);
        let b = |x: i32, w: i32| Rect::new(x, y + 7, w, 32);
        let sep = |ui: &mut Ui, x: i32| ui.rect(Rect::new(x, y + 12, 1, 20), t.line);
        let mut x = r.x + 12;
        self.tool(ui, b(x, 32), "B", None, Face::Semibold, C_BOLD, inst, f.bold);
        x += 34;
        self.tool(ui, b(x, 32), "I", None, Face::Italic, C_ITALIC, inst, f.italic);
        x += 40;
        sep(ui, x - 5);
        if !compact {
            for (icon, code, al) in [(Icon::AlignLeft, C_LEFT, HAlign::Left), (Icon::AlignCenter, C_CENTER, HAlign::Center), (Icon::AlignRight, C_RIGHT, HAlign::Right)] {
                self.tool(ui, b(x, 32), "", Some(icon), Face::Regular, code, inst, f.align == al);
                x += 34;
            }
            x += 6;
            sep(ui, x - 5);
        }
        for (label, code, num, w) in [("123", C_GENERAL, Num::General, 40), ("1,234", C_NUMBER, Num::Number, 50), ("₦", C_CURRENCY, Num::Currency, 32), ("%", C_PERCENT, Num::Percent, 32)] {
            if compact && num == Num::General {
                continue;
            }
            self.tool(ui, b(x, w), label, None, Face::Medium, code, inst, f.num == num);
            x += w + 2;
        }
        x += 6;
        sep(ui, x - 5);
        self.tool(ui, b(x, 36), "Σ", None, Face::Semibold, C_AUTOSUM, inst, false);
        x += 40;
        sep(ui, x - 5);
        self.tool(ui, b(x, 32), "", Some(Icon::Undo), Face::Regular, C_UNDO, inst, false);
        x += 34;
        if !compact {
            self.tool(ui, b(x, 32), "", Some(Icon::Redo), Face::Regular, C_REDO, inst, false);
        }
        // formula bar
        let fy = y + TOOLBAR;
        ui.rect(Rect::new(r.x, fy - 1, r.w, 1), t.line);
        ui.rect(Rect::new(r.x, fy, r.w, FBAR), t.surface);
        let (r1, c1, r2, c2) = self.sel();
        let name = range_text(r1, c1, r2.min(MAX_ROWS - 1), c2);
        let nb = Rect::new(r.x + 12, fy + 5, 84, 28);
        ui.rrect(nb, 8, t.chip);
        let name = ui.fit(Face::Medium, 13, &name, nb.w - 12);
        ui.text_in(nb, Face::Medium, 13, &name, t.text, 1);
        ui.text(nb.r() + 10, fy + 25, Face::Italic, 15, "fx", t.text3);
        let fr = Rect::new(nb.r() + 34, fy + 5, r.r() - nb.r() - 46, 28);
        let (text, focused) = match &self.edit {
            Some(e) => (e.text.clone(), e.in_bar),
            None => (self.sheet.input(self.cur.0, self.cur.1).to_string(), false),
        };
        ui.rrect(fr, 8, PAPER);
        ui.stroke(fr, 8, 1, if focused { t.accent } else { t.line });
        let inner = Rect::new(fr.x + 10, fr.y, fr.w - 20, fr.h);
        let old = ui.clip_in(inner);
        let w = ui.text_in(inner, Face::Regular, 13, &text, INK, 0);
        if let Some(e) = &self.edit {
            if e.in_bar && (ui.ticks / 50) % 2 == 0 {
                let pre: String = e.text.chars().take(e.caret).collect();
                let cx = inner.x + ui.tw(Face::Regular, 13, &pre);
                ui.rect(Rect::new(cx, fr.y + 6, 1, 16), INK);
            }
        }
        let _ = w;
        ui.set_clip(old);
        ui.zone(fr, Action::App(inst, C_BAR));
        ui.rect(Rect::new(r.x, fy + FBAR - 1, r.w, 1), t.line);
    }

    fn face(f: &Fmt) -> Face {
        match (f.bold, f.italic) {
            (false, false) => Face::Regular,
            (true, false) => Face::Semibold,
            (false, true) => Face::Italic,
            (true, true) => Face::SemiboldItalic,
        }
    }

    fn render_grid(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let t = ui.t;
        self.area = area;
        if self.want_visible {
            self.ensure_visible();
            self.want_visible = false;
        }
        self.clamp_scroll();
        ui.rect(area, PAPER);
        let (ox, oy) = self.cells_origin();
        let body = Rect::new(area.x + ROW_HDR, area.y + COL_HDR, area.w - ROW_HDR, area.h - COL_HDR);
        // visible rows and columns
        let r0 = (self.sy / ROW_H) as u32;
        let r_end = ((self.sy + body.h) / ROW_H + 1).min(MAX_ROWS as i32 - 1) as u32;
        let mut c0 = 0;
        let mut x = ox;
        while c0 < MAX_COLS - 1 && x + self.sheet.width(c0) <= body.x {
            x += self.sheet.width(c0);
            c0 += 1;
        }
        let mut cols: Vec<(u32, i32)> = Vec::new();
        let mut cx = x;
        let mut c = c0;
        while c < MAX_COLS && cx < body.r() {
            cols.push((c, cx));
            cx += self.sheet.width(c);
            c += 1;
        }
        let (s_r1, s_c1, s_r2, s_c2) = self.sel();
        let multi = (s_r1, s_c1) != (s_r2, s_c2);
        let mut calc = Calc::new(&self.sheet);
        let old = ui.clip_in(body);
        // selection fill
        if multi {
            let a = self.cell_rect(s_r1, s_c1);
            let b = self.cell_rect(s_r2.min(r_end + 1), s_c2.min(c));
            ui.rect(Rect::new(a.x, a.y, b.r() - a.x, b.b() - a.y), t.accent.with_alpha(28));
        }
        // grid lines
        for &(_, x) in &cols {
            ui.rect(Rect::new(x - 1, body.y, 1, body.h), GRIDLINE);
        }
        if let Some(&(lc, lx)) = cols.last() {
            ui.rect(Rect::new(lx + self.sheet.width(lc) - 1, body.y, 1, body.h), GRIDLINE);
        }
        for r in r0..=r_end {
            let y = oy + (r as i32 + 1) * ROW_H - 1;
            ui.rect(Rect::new(body.x, y, body.w, 1), GRIDLINE);
        }
        // cell contents
        let s = ui.s;
        for r in r0..=r_end {
            let y = oy + r as i32 * ROW_H;
            let mut k = 0;
            while k < cols.len() {
                let (c, x) = cols[k];
                k += 1;
                let input = self.sheet.input(r, c);
                if input.is_empty() {
                    continue;
                }
                if self.edit.as_ref().map_or(false, |e| !e.in_bar) && (r, c) == self.cur {
                    continue;
                }
                let f = self.sheet.fmt(r, c);
                let v = calc.value(r, c);
                let text = display(&v, &f);
                let face = Self::face(&f);
                let w = self.sheet.width(c);
                let tw = ui.tw(face, TEXT, &text);
                let align = match (f.align, &v) {
                    (HAlign::Auto, Val::Num(_)) => HAlign::Right,
                    (HAlign::Auto, Val::Bool(_) | Val::Err(_)) => HAlign::Center,
                    (HAlign::Auto, _) => HAlign::Left,
                    (a, _) => a,
                };
                // text runs on into empty cells to the right
                let mut room = w;
                if align == HAlign::Left && matches!(v, Val::Text(_)) && tw > w - 8 {
                    let mut n = c + 1;
                    while n < MAX_COLS && room < tw + 12 && self.sheet.input(r, n).is_empty() && n < c + 12 {
                        room += self.sheet.width(n);
                        n += 1;
                    }
                }
                let text = if matches!(v, Val::Num(_)) && tw > w - 8 { "#".repeat(((w - 8) / ui.tw(face, TEXT, "#").max(1)).max(1) as usize) } else { text };
                let tw = ui.tw(face, TEXT, &text);
                let tx = match align {
                    HAlign::Right => x + w - 6 - tw,
                    HAlign::Center => x + (w - tw) / 2,
                    _ => x + 6,
                };
                let cell_clip = ui.clip_in(Rect::new(x, y, room - 1, ROW_H - 1));
                let col = if matches!(v, Val::Err(_)) { t.danger } else { INK };
                crate::font::draw(ui.c, tx * s, (y + 17) * s, face, TEXT * s, &text, col);
                ui.set_clip(cell_clip);
            }
        }
        // active cell
        let ar = self.cell_rect(self.cur.0, self.cur.1);
        ui.stroke(Rect::new(ar.x - 1, ar.y - 1, ar.w + 1, ar.h + 1), 0, 2, t.accent);
        if multi {
            let a = self.cell_rect(s_r1, s_c1);
            let b = self.cell_rect(s_r2.min(r_end + 1), s_c2.min(c));
            ui.stroke(Rect::new(a.x - 1, a.y - 1, b.r() - a.x + 1, b.b() - a.y + 1), 0, 1, t.accent);
        }
        // in-cell editor
        if let Some(e) = &self.edit {
            if !e.in_bar {
                let w = ui.tw(Face::Regular, TEXT, &e.text).max(ar.w - 12) + 14;
                let er = Rect::new(ar.x, ar.y, w.min(body.r() - ar.x), ar.h - 1);
                ui.rect(er, PAPER);
                ui.stroke(Rect::new(er.x - 1, er.y - 1, er.w + 1, er.h + 2), 0, 2, t.accent);
                crate::font::draw(ui.c, (er.x + 6) * s, (er.y + 17) * s, Face::Regular, TEXT * s, &e.text, INK);
                if (ui.ticks / 50) % 2 == 0 {
                    let pre: String = e.text.chars().take(e.caret).collect();
                    let cx = er.x + 6 + ui.tw(Face::Regular, TEXT, &pre);
                    ui.rect(Rect::new(cx, er.y + 5, 1, 16), INK);
                }
            }
        }
        ui.set_clip(old);
        ui.zone(body, Action::App(inst, C_GRID));
        // column headers
        let hdr = Rect::new(body.x, area.y, body.w, COL_HDR);
        ui.rect(hdr, t.surface);
        let old = ui.clip_in(hdr);
        for &(c, x) in &cols {
            let w = self.sheet.width(c);
            let on = c >= s_c1 && c <= s_c2;
            if on {
                ui.rect(Rect::new(x, hdr.y, w, COL_HDR), t.accent.with_alpha(40));
            }
            ui.text_in(Rect::new(x, hdr.y, w, COL_HDR), if on { Face::Semibold } else { Face::Medium }, 12, &col_name(c), if on { t.accent } else { t.text2 }, 1);
            ui.rect(Rect::new(x + w - 1, hdr.y + 4, 1, COL_HDR - 8), t.line);
        }
        ui.set_clip(old);
        ui.zone(hdr, Action::App(inst, C_COLHDR));
        // row headers
        let rh = Rect::new(area.x, body.y, ROW_HDR, body.h);
        ui.rect(rh, t.surface);
        let old = ui.clip_in(rh);
        for r in r0..=r_end {
            let y = oy + r as i32 * ROW_H;
            let on = r >= s_r1 && r <= s_r2;
            if on {
                ui.rect(Rect::new(rh.x, y, ROW_HDR, ROW_H), t.accent.with_alpha(40));
            }
            ui.text_in(Rect::new(rh.x, y, ROW_HDR, ROW_H), if on { Face::Semibold } else { Face::Medium }, 12, &format!("{}", r + 1), if on { t.accent } else { t.text2 }, 1);
            ui.rect(Rect::new(rh.x + 6, y + ROW_H - 1, ROW_HDR - 12, 1), t.line);
        }
        ui.set_clip(old);
        ui.zone(rh, Action::App(inst, C_ROWHDR));
        let corner = Rect::new(area.x, area.y, ROW_HDR, COL_HDR);
        ui.rect(corner, t.surface);
        ui.zone(corner, Action::App(inst, C_CORNER));
        ui.rect(Rect::new(area.x, area.y + COL_HDR - 1, area.w, 1), t.line);
        ui.rect(Rect::new(area.x + ROW_HDR - 1, area.y, 1, area.h), t.line);
    }

    fn render_overlay(&self, ui: &mut Ui, r: Rect, inst: u32) {
        let t = ui.t;
        if let Overlay::Open(files) = &self.overlay {
            ui.zone(r, Action::App(inst, C_DISMISS));
            let w = (r.w - 40).min(420);
            let n = files.len().min(9) as i32;
            let m = Rect::new(r.x + (r.w - w) / 2, r.y + HEADER + 20, w, 96 + n.max(1) * 40);
            ui.shadow(m, 14, 18, 6, 70);
            ui.rrect(m, 14, t.surface);
            ui.text(m.x + 20, m.y + 32, Face::Semibold, 16, "Open a sheet", t.text);
            let nr = Rect::new(m.x + 12, m.y + 46, m.w - 24, 38);
            let na = Action::App(inst, C_NEW);
            if ui.hot(na) {
                ui.rrect(nr, 8, t.hover);
            }
            ui.icon(Icon::Plus, nr.x + 10, nr.y + 11, 16, t.accent);
            ui.text_in(Rect::new(nr.x + 36, nr.y, nr.w - 40, nr.h), Face::Medium, 13, "New blank sheet", t.accent, 0);
            ui.zone(nr, na);
            if files.is_empty() {
                ui.text(m.x + 20, m.y + 110, Face::Regular, 13, "No sheets yet in Documents, Downloads or Shared.", t.text2);
            }
            for (k, f) in files.iter().take(9).enumerate() {
                let row = Rect::new(m.x + 12, m.y + 88 + k as i32 * 40, m.w - 24, 38);
                let a = Action::App(inst, C_FILE + k as u32);
                if ui.hot(a) {
                    ui.rrect(row, 8, t.hover);
                }
                ui.icon(Icon::Sheet, row.x + 10, row.y + 11, 16, t.text2);
                let name = ui.fit(Face::Medium, 13, basename(f), row.w - 150);
                ui.text(row.x + 36, row.y + 24, Face::Medium, 13, &name, t.text);
                let d = parent(f);
                let d = d.rsplit('/').next().unwrap_or("");
                let dw = ui.tw(Face::Regular, 12, d);
                ui.text(row.r() - dw - 10, row.y + 24, Face::Regular, 12, d, t.text3);
                ui.zone(row, a);
            }
        }
    }

    /// Selection summary for the status bar.
    fn summary(&self) -> String {
        let (r1, c1, r2, c2) = self.sel();
        if (r1, c1) == (r2, c2) {
            return String::new();
        }
        let (ur, uc) = self.sheet.used();
        let mut calc = Calc::new(&self.sheet);
        let (mut sum, mut n, mut count) = (0.0, 0, 0);
        for r in r1..=r2.min(ur) {
            for c in c1..=c2.min(uc) {
                match calc.value(r, c) {
                    Val::Num(x) => {
                        sum += x;
                        n += 1;
                        count += 1;
                    }
                    Val::Empty => {}
                    _ => count += 1,
                }
            }
        }
        if n == 0 {
            return format!("Count: {}", count);
        }
        format!("Sum: {}   ·   Average: {}   ·   Count: {}", general(sum), general(sum / n as f64), count)
    }

    // ---- keys while editing a cell -----------------------------------------------

    fn edit_key(&mut self, k: Key) -> bool {
        let Some(e) = self.edit.as_mut() else { return false };
        let len = e.text.chars().count();
        let byte = |t: &str, i: usize| t.char_indices().nth(i).map(|x| x.0).unwrap_or(t.len());
        match k {
            Key::Char(ch) if !ch.is_control() => {
                let at = byte(&e.text, e.caret);
                e.text.insert(at, ch);
                e.caret += 1;
            }
            Key::Backspace if e.caret > 0 => {
                let at = byte(&e.text, e.caret - 1);
                e.text.remove(at);
                e.caret -= 1;
            }
            Key::Delete if e.caret < len => {
                let at = byte(&e.text, e.caret);
                e.text.remove(at);
            }
            Key::Left if e.in_bar || e.text.starts_with('=') => e.caret = e.caret.saturating_sub(1),
            Key::Right if e.in_bar || e.text.starts_with('=') => e.caret = (e.caret + 1).min(len),
            Key::Home => e.caret = 0,
            Key::End => e.caret = len,
            Key::Esc => self.edit = None,
            Key::Enter | Key::Tab | Key::Up | Key::Down | Key::Left | Key::Right => {
                self.commit();
                let shift = crate::input::shift();
                match k {
                    Key::Enter => self.step(if shift { -1 } else { 1 }, 0, false),
                    Key::Tab => self.step(0, if shift { -1 } else { 1 }, false),
                    Key::Up => self.step(-1, 0, false),
                    Key::Down => self.step(1, 0, false),
                    Key::Left => self.step(0, -1, false),
                    _ => self.step(0, 1, false),
                }
            }
            _ => {}
        }
        true
    }
}

impl App for Grids {
    fn kind(&self) -> AppKind {
        AppKind::Grids
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        let t = ui.t;
        let compact = super::compact(r);
        let title_x = r.x + 18;
        let bx = r.r() - 118 - if compact { 62 } else { 96 };
        ui.icon(Icon::Sheet, title_x, r.y + 13, 18, t.accent);
        let tr = Rect::new(title_x + 26, r.y + 7, bx - title_x - 34, 30);
        match &self.overlay {
            Overlay::Rename(e) => {
                let text = e.text.clone();
                ui.field(tr, &text, "Sheet name", true, Action::App(inst, C_RENAME));
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
        let top = r.y + HEADER + TOOLBAR + FBAR;
        let area = Rect::new(r.x, top, r.w, r.b() - STATUS - top);
        self.render_grid(ui, area, inst);
        let sy = r.b() - STATUS;
        ui.rect(Rect::new(r.x, sy, r.w, STATUS), t.surface);
        ui.rect(Rect::new(r.x, sy, r.w, 1), t.line);
        let sum = self.summary();
        let left = if sum.is_empty() { format!("{}", self.sheet.name) } else { sum };
        ui.text(r.x + 16, sy + 19, Face::Regular, 12, &left, t.text2);
        let right = if self.imported {
            let kind = if self.ext().eq_ignore_ascii_case(".csv") { "CSV" } else { "Excel" };
            format!("Viewing an {} file  ·  saving creates a .hydg copy", kind)
        } else {
            format!("Hyda Grids sheet  ·  {}", if self.dirty { "Edited" } else if self.path.is_empty() { "Not saved yet" } else { "Saved" })
        };
        if !compact {
            let rw = ui.tw(Face::Regular, 12, &right);
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
        if matches!(self.overlay, Overlay::Open(_)) && code < C_FILE && code != C_NEW {
            self.overlay = Overlay::None;
            if code == C_DISMISS || code == C_OPEN {
                return;
            }
        }
        self.press = None;
        self.resize = None;
        let (mx, my) = self.mouse;
        match code {
            C_GRID => {
                let cell = (self.row_at(my), self.col_at(mx));
                // clicking a cell while typing a formula inserts its reference
                if let Some(e) = self.edit.as_mut() {
                    let before: String = e.text.chars().take(e.caret).collect();
                    if e.text.starts_with('=') && before.ends_with(['=', '+', '-', '*', '/', '^', '(', ',', ':', '&', '<', '>', ';']) {
                        let name = cell_name(cell.0, cell.1);
                        let byte = e.text.char_indices().nth(e.caret).map(|x| x.0).unwrap_or(e.text.len());
                        e.text.insert_str(byte, &name);
                        e.caret += name.chars().count();
                        return;
                    }
                    self.commit();
                }
                if double && cell == self.cur {
                    self.start_edit(None, false);
                    return;
                }
                let shift = crate::input::shift();
                self.select(cell, shift);
                self.want_visible = false;
                if !shift {
                    self.press = Some(cell);
                }
            }
            C_COLHDR => {
                self.commit();
                // near a column's right edge: resize it
                let c = self.col_at(mx);
                let right = self.cell_rect(0, c).r();
                let left = self.cell_rect(0, c).x;
                let edge = if mx >= right - 5 { Some(c) } else if mx <= left + 4 && c > 0 { Some(c - 1) } else { None };
                if let Some(ec) = edge {
                    if double {
                        self.snapshot();
                        self.sheet.widths.remove(&ec);
                    } else {
                        self.resize = Some((ec, mx, self.sheet.width(ec)));
                        self.snapshot();
                    }
                    return;
                }
                self.anchor = Some((0, c));
                self.cur = (MAX_ROWS - 1, c);
                self.press = Some((u32::MAX, c));
            }
            C_ROWHDR => {
                self.commit();
                let r = self.row_at(my);
                self.anchor = Some((r, 0));
                self.cur = (r, MAX_COLS - 1);
                self.press = Some((r, u32::MAX));
            }
            C_CORNER | C_ALL => {
                self.commit();
                self.anchor = Some((0, 0));
                self.cur = (MAX_ROWS - 1, MAX_COLS - 1);
            }
            C_BAR => {
                if self.edit.is_none() {
                    self.start_edit(None, true);
                } else if let Some(e) = self.edit.as_mut() {
                    e.in_bar = true;
                }
            }
            C_BOLD => self.toggle_bold(),
            C_ITALIC => self.toggle_italic(),
            C_LEFT => self.each_selected(|f| f.align = HAlign::Left),
            C_CENTER => self.each_selected(|f| f.align = HAlign::Center),
            C_RIGHT => self.each_selected(|f| f.align = HAlign::Right),
            C_GENERAL => self.each_selected(|f| f.num = Num::General),
            C_NUMBER => self.each_selected(|f| f.num = Num::Number),
            C_CURRENCY => self.each_selected(|f| {
                f.num = Num::Currency;
                f.sym = '₦';
            }),
            C_PERCENT => self.each_selected(|f| f.num = Num::Percent),
            C_AUTOSUM => {
                self.commit();
                self.autosum();
            }
            C_UNDO => self.undo_redo(true),
            C_REDO => self.undo_redo(false),
            C_NEW => {
                self.overlay = Overlay::None;
                self.park(sys);
                self.reset(Sheet::new());
            }
            C_OPEN => {
                self.commit();
                self.overlay = Overlay::Open(Self::sheets(sys));
            }
            C_SAVE => self.save(sys, false),
            C_EXPORT_XLSX => self.export(sys, ".xlsx"),
            C_EXPORT_CSV => self.export(sys, ".csv"),
            C_RENAME => {
                if !matches!(self.overlay, Overlay::Rename(_)) {
                    self.commit();
                    self.overlay = Overlay::Rename(LineEdit { text: self.title() });
                }
            }
            C_CUT => {
                self.copy(sys);
                self.clear();
            }
            C_COPY => self.copy(sys),
            C_PASTE => self.paste(sys),
            C_CLEAR => self.clear(),
            C_DISMISS => {}
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
                    'b' => self.toggle_bold(),
                    'i' => self.toggle_italic(),
                    'z' => self.undo_redo(true),
                    'y' => self.undo_redo(false),
                    'x' => self.action(C_CUT, false, sys),
                    'c' => self.copy(sys),
                    'v' => {
                        self.commit();
                        self.paste(sys);
                    }
                    'a' => self.action(C_ALL, false, sys),
                    's' => self.save(sys, false),
                    'o' => self.action(C_OPEN, false, sys),
                    'n' => self.action(C_NEW, false, sys),
                    _ => {}
                },
                Key::Tab => self.toggle_italic(),
                Key::Home => {
                    self.commit();
                    self.select((0, 0), shift);
                }
                Key::End => {
                    self.commit();
                    let (r, c) = self.sheet.used();
                    self.select((r - 1, c - 1), shift);
                }
                _ => {}
            }
            return;
        }
        if self.edit_key(k) {
            return;
        }
        match k {
            Key::Char(c) if !c.is_control() => self.start_edit(Some(c), false),
            Key::F(2) => self.start_edit(None, false),
            Key::Enter => self.step(if shift { -1 } else { 1 }, 0, false),
            Key::Tab => self.step(0, if shift { -1 } else { 1 }, false),
            Key::Up => self.step(-1, 0, shift),
            Key::Down => self.step(1, 0, shift),
            Key::Left => self.step(0, -1, shift),
            Key::Right => self.step(0, 1, shift),
            Key::PageUp => self.step(-20, 0, shift),
            Key::PageDown => self.step(20, 0, shift),
            Key::Home => self.select((self.cur.0, 0), shift),
            Key::Delete => self.clear(),
            Key::Backspace => {
                self.clear();
                self.start_edit(None, false);
            }
            Key::Esc => self.anchor = None,
            _ => {}
        }
    }

    fn scroll(&mut self, dy: i32) {
        // over the column headers the wheel scrolls sideways
        if self.mouse.1 < self.area.y + COL_HDR {
            self.sx += dy * 60;
        } else {
            self.sy += dy * ROW_H * 3;
        }
        self.clamp_scroll();
    }

    fn mouse(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    fn drag(&mut self, x: i32, y: i32) {
        if let Some((c, x0, w0)) = self.resize {
            self.sheet.widths.insert(c, (w0 + x - x0).clamp(24, 600));
            if self.sheet.width(c) == DEFAULT_WIDTH {
                self.sheet.widths.remove(&c);
            }
            return;
        }
        let Some(start) = self.press else { return };
        if y > self.area.b() - 4 {
            self.sy += ROW_H;
        } else if y < self.area.y + COL_HDR && start.0 != u32::MAX {
            self.sy -= ROW_H;
        }
        if x > self.area.r() - 4 {
            self.sx += 40;
        } else if x < self.area.x + ROW_HDR && start.1 != u32::MAX {
            self.sx -= 40;
        }
        self.clamp_scroll();
        let (r, c) = (self.row_at(y), self.col_at(x));
        match start {
            (u32::MAX, c0) => {
                self.anchor = Some((0, c0));
                self.cur = (MAX_ROWS - 1, c);
            }
            (r0, u32::MAX) => {
                self.anchor = Some((r0, 0));
                self.cur = (r, MAX_COLS - 1);
            }
            s => {
                self.anchor = Some(s);
                self.cur = (r, c);
            }
        }
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => vec![
                ("New Sheet", C_NEW),
                ("Open…", C_OPEN),
                ("Save", C_SAVE),
                ("Export as Excel (.xlsx)", C_EXPORT_XLSX),
                ("Export as CSV (.csv)", C_EXPORT_CSV),
                ("Rename…", C_RENAME),
            ],
            1 => vec![("Undo", C_UNDO), ("Redo", C_REDO), ("Cut", C_CUT), ("Copy", C_COPY), ("Paste", C_PASTE), ("Clear", C_CLEAR), ("Select All", C_ALL)],
            _ => vec![],
        }
    }

    fn close(&mut self, sys: &mut Sys) {
        self.commit();
        if self.dirty && (!self.path.is_empty() || !self.sheet.cells.is_empty()) {
            self.save(sys, false);
        }
    }

    fn animating(&self) -> bool {
        self.edit.is_some()
    }

    fn open_path(&mut self, path: &str, sys: &mut Sys) {
        self.load(path, sys);
    }
}
