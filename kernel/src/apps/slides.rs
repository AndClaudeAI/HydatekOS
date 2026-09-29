//! Hyda Slides, the Hyda Workspace presentation program: slides with titles,
//! bullets, text boxes, shapes and pictures on a theme, speaker notes, and a
//! full-screen slideshow with transitions. Presentations are saved in Hyda
//! Slides' own format (.hydp); PowerPoint (.pptx) files open for viewing and
//! editing and are exported to, but never saved over.

use super::{App, AppKind, LineEdit, HEADER};
use crate::deck::*;
use crate::deckio;
use crate::doc::{Doc, Pos, Style, BOLD, ITALIC, UNDERLINE};
use crate::font::Face;
use crate::fs::{basename, join, parent};
use crate::gfx::{Canvas, Color, Rect};
use crate::icons::Icon;
use super::slidedraw::{self, anim_steps, blit, draw_slide, ellipse, mixp, Opts, Pics};
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const TOOLBAR: i32 = 46;
const STRIP_W: i32 = 188;
const NOTES_H: i32 = 86;
const STATUS: i32 = 28;
/// Transition length in ticks (100 per second).
const TRANS_TICKS: u64 = 40;
/// An entrance animation's length in ticks.
const ANIM_TICKS: u64 = 45;

const C_CANVAS: u32 = 1;
const C_NOTES: u32 = 2;
const C_NEW_SLIDE: u32 = 4;
const C_LAYOUT: u32 = 5;
const C_BOLD: u32 = 6;
const C_ITALIC: u32 = 7;
const C_UNDERLINE: u32 = 8;
const C_LEFT: u32 = 9;
const C_CENTER: u32 = 10;
const C_RIGHT: u32 = 11;
const C_BULLETS: u32 = 12;
const C_NUMBERS: u32 = 13;
const C_SMALLER: u32 = 14;
const C_BIGGER: u32 = 15;
const C_TEXTBOX: u32 = 16;
const C_RECT: u32 = 17;
const C_ELLIPSE: u32 = 18;
const C_PICTURE: u32 = 19;
const C_COLOR: u32 = 20;
const C_THEME: u32 = 21;
const C_TRANS: u32 = 22;
const C_UNDO: u32 = 23;
const C_REDO: u32 = 24;
const C_PLAY: u32 = 25;
const C_PLAY_START: u32 = 26;
const C_NEW: u32 = 27;
const C_OPEN: u32 = 28;
const C_SAVE: u32 = 29;
const C_RENAME: u32 = 30;
const C_EXPORT: u32 = 31;
const C_CUT: u32 = 32;
const C_COPY: u32 = 33;
const C_PASTE: u32 = 34;
const C_DUP: u32 = 35;
const C_DELETE: u32 = 36;
const C_ALL: u32 = 37;
const C_DISMISS: u32 = 38;
const C_SHOW: u32 = 39;
const C_PREV: u32 = 40;
const C_NEXT: u32 = 41;
const C_DEL_SLIDE: u32 = 42;
const C_DUP_SLIDE: u32 = 43;
const C_SLIDE_UP: u32 = 44;
const C_SLIDE_DOWN: u32 = 45;
const C_FRONT: u32 = 46;
const C_BACK: u32 = 47;
const C_STRIP: u32 = 48;
const C_ADD_SLIDE: u32 = 49;
const C_TABLE: u32 = 50;
const C_CHART: u32 = 51;
const C_ANIM: u32 = 52;
const C_FOOTER: u32 = 53;
const C_PDF: u32 = 54;
const C_PDF_NOTES: u32 = 55;
const C_PRESENTER: u32 = 56;
const C_ROT_R: u32 = 57;
const C_ROT_L: u32 = 58;
const C_FLIP_H: u32 = 59;
const C_FLIP_V: u32 = 60;
const C_OBJECT: u32 = 61;
const C_DLG_TEXT: u32 = 63;
const C_DLG_NUMBERS: u32 = 64;
const C_DLG_OK: u32 = 65;
const C_DATA_DONE: u32 = 66;
const C_DATA_ADD_ROW: u32 = 67;
const C_DATA_DEL_ROW: u32 = 68;
const C_DATA_ADD_SER: u32 = 69;
const C_DATA_DEL_SER: u32 = 70;
const C_DATA_LEGEND: u32 = 71;
const C_DATA_TITLE: u32 = 72;
const C_THUMB: u32 = 100;
const C_DATA_CELL: u32 = 3000;
const C_DATA_KIND: u32 = 2900;
/// how many files an Open or Insert Picture list can show
const FILES_MAX: u32 = 100;
const C_MENU: u32 = 1000;
const C_FILE: u32 = 2000;

/// Colours offered for shapes and text.
const PALETTE: [u32; 12] = [0x1E1B2C, 0x5A5566, 0xFFFFFF, 0xF7F1E8, 0xC0622B, 0xE8573A, 0xF2B544, 0x0E5A43, 0x4FC3B8, 0x2F6FEB, 0x8E6CB5, 0xC2504F];

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    NewSlide,
    Layout,
    Theme,
    Color,
    Trans,
    /// insert a shape, line or arrow
    Shapes,
    /// insert a table of a chosen size
    Table,
    /// insert a chart of a kind
    Chart,
    /// rows, columns and style of the selected table
    TableEdit,
    /// the selected chart
    ChartEdit,
    Anim,
}

/// The chart data sheet: cell (row, column) in a grid of categories (rows)
/// by series (columns); row 0 holds series names, column 0 category names.
struct DataEd {
    shape: usize,
    cur: (usize, usize),
    /// typing into the current cell (or the title when `title`)
    text: Option<String>,
    title: bool,
}

struct FooterDlg {
    text: LineEdit,
    numbers: bool,
}

enum Overlay {
    None,
    Open(Vec<String>),
    Pictures(Vec<String>),
    Rename(LineEdit),
    Menu(MenuKind, Rect),
    ChartData(DataEd),
    Footer(FooterDlg),
}

#[derive(Clone, Copy)]
struct TextEd {
    shape: usize,
    /// a table cell (row, column) of the shape
    cell: Option<(usize, usize)>,
    caret: Pos,
    anchor: Option<Pos>,
    /// typing since the last undo snapshot
    typing: bool,
}

#[derive(Clone, Copy)]
enum Press {
    Move { start: (i32, i32), orig: (i32, i32), moved: bool },
    Resize { handle: u8, start: (i32, i32), orig: (i32, i32, i32, i32), moved: bool },
    Text,
    /// dragging a thumbnail to reorder (moved yet?)
    Thumb(bool),
    /// turning a shape: its centre (screen) and rotation at the start
    Rotate { centre: (i32, i32), orig: i32, moved: bool },
    /// dragging one end of a line (0 start, 1 end)
    LineEnd { end: u8, moved: bool },
}

struct Show {
    idx: usize,
    /// the slide shown before, and when the change started
    from: Option<(Canvas, u64)>,
    /// the black "end of slideshow" screen
    end: bool,
    black: bool,
    /// (slide, width, height, deck generation, animation steps shown) and the picture
    frame: Option<(usize, i32, i32, u64, usize, Canvas)>,
    /// animation steps shown on this slide; one playing since a tick
    step: usize,
    playing: Option<u64>,
    /// the frame before the playing step (for fading)
    before: Option<Canvas>,
    presenter: bool,
    started: u64,
    next: Option<(usize, i32, u64, Canvas)>,
}

// ---- the app ----------------------------------------------------------------

pub struct Slides {
    deck: Deck,
    path: String,
    dirty: bool,
    imported: bool,
    cur: usize,
    sel: Option<usize>,
    ed: Option<TextEd>,
    /// caret in the speaker notes while typing there
    notes: Option<usize>,
    notes_typing: bool,
    undo: Vec<(Deck, usize)>,
    redo: Vec<(Deck, usize)>,
    clip_shape: Option<(Shape, Option<Pic>)>,
    clip_text: String,
    overlay: Overlay,
    show: Option<Show>,
    // layout of the last frame (logical)
    canvas: Rect,
    strip: Rect,
    notes_r: Rect,
    strip_scroll: i32,
    mouse: (i32, i32),
    press: Option<Press>,
    /// bumped by every change (for the caches)
    gen: u64,
    view: Option<((u64, usize, i32, Option<usize>), Canvas)>,
    thumbs: Vec<Option<(u64, Canvas)>>,
    thumb_gen: Vec<u64>,
    pics: Pics,
    ticks: u64,
    /// the table cell being typed in, as a shape of its own
    cell_sh: Option<Shape>,
}

impl Slides {
    pub fn new() -> Slides {
        let mut s = Slides {
            deck: Deck::new(),
            path: String::new(),
            dirty: false,
            imported: false,
            cur: 0,
            sel: None,
            ed: None,
            notes: None,
            notes_typing: false,
            undo: vec![],
            redo: vec![],
            clip_shape: None,
            clip_text: String::new(),
            overlay: Overlay::None,
            show: None,
            canvas: Rect::default(),
            strip: Rect::default(),
            notes_r: Rect::default(),
            strip_scroll: 0,
            mouse: (0, 0),
            press: None,
            gen: 1,
            view: None,
            thumbs: vec![],
            thumb_gen: vec![],
            pics: Pics::default(),
            ticks: 0,
            cell_sh: None,
        };
        s.touch_all();
        s
    }

    // ---- state helpers --------------------------------------------------------

    fn slide(&self) -> &Slide {
        &self.deck.slides[self.cur]
    }

    fn slide_mut(&mut self) -> &mut Slide {
        let c = self.cur;
        &mut self.deck.slides[c]
    }

    fn shape(&self) -> Option<&Shape> {
        self.sel.and_then(|i| self.slide().shapes.get(i))
    }

    /// The current slide changed.
    fn touch(&mut self) {
        // typing in a table cell: the cell takes the text, rows grow to fit it
        if let (Some(ed), Some(cs)) = (self.ed, self.cell_sh.as_mut()) {
            if let Some((r, c)) = ed.cell {
                let cur = self.cur;
                if let Some(sh) = self.deck.slides[cur].shapes.get_mut(ed.shape) {
                    if let Some(t) = sh.table.as_mut() {
                        if r < t.nrows() && c < t.ncols() {
                            *t.cell_mut(r, c) = cs.text.clone();
                        }
                    }
                    sh.fit_table();
                    if let Some(n) = sh.cell_shape(r, c) {
                        cs.x = n.x;
                        cs.y = n.y;
                        cs.w = n.w;
                        cs.h = n.h;
                    }
                }
            }
        }
        self.gen += 1;
        self.dirty = true;
        if self.thumb_gen.len() != self.deck.slides.len() {
            self.touch_all();
            return;
        }
        self.thumb_gen[self.cur] = self.gen;
    }

    /// Slides were added, removed, moved or restyled.
    fn touch_all(&mut self) {
        self.gen += 1;
        self.thumb_gen = vec![self.gen; self.deck.slides.len()];
        self.thumbs.clear();
        self.thumbs.resize_with(self.deck.slides.len(), || None);
    }

    fn snapshot(&mut self) {
        self.undo.push((self.deck.clone(), self.cur));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    fn undo_redo(&mut self, undo: bool) {
        self.ed = None;
        self.notes = None;
        let (from, to) = if undo { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        if let Some((d, cur)) = from.pop() {
            to.push((core::mem::replace(&mut self.deck, d), self.cur));
            self.cur = cur.min(self.deck.slides.len() - 1);
            self.sel = None;
            self.dirty = true;
            self.pics.clear();
            self.touch_all();
        }
    }

    fn go(&mut self, i: usize) {
        self.stop_edit();
        self.notes = None;
        self.cur = i.min(self.deck.slides.len() - 1);
        self.sel = None;
        self.scroll_to_cur();
    }

    fn scroll_to_cur(&mut self) {
        let (top, h) = (self.cur as i32 * self.thumb_step(), self.thumb_step());
        let vh = (self.strip.h - 50).max(h);
        if top < self.strip_scroll {
            self.strip_scroll = top;
        } else if top + h > self.strip_scroll + vh {
            self.strip_scroll = top + h - vh;
        }
    }

    fn thumb_step(&self) -> i32 {
        (STRIP_W - 44) * self.deck.h / self.deck.w + 18
    }

    fn stop_edit(&mut self) {
        if self.ed.is_some() {
            self.touch();
            self.ed = None;
            self.cell_sh = None;
            self.touch();
        }
    }

    /// Start typing in cell (r, c) of table `shape`.
    fn start_cell_edit(&mut self, shape: usize, r: usize, c: usize, caret: Option<Pos>) {
        self.notes = None;
        let Some(cs) = self.slide().shapes.get(shape).and_then(|s| s.cell_shape(r, c)) else { return };
        let caret = caret.map(|p| cs.text.clamp(p)).unwrap_or(cs.text.end());
        if self.ed.is_some() {
            self.touch();
        }
        self.sel = Some(shape);
        self.cell_sh = Some(cs);
        self.ed = Some(TextEd { shape, cell: Some((r, c)), caret, anchor: None, typing: false });
        self.touch();
    }

    /// The text being typed in, as a shape (a table cell's box for a cell).
    fn text_target(&self) -> Option<Shape> {
        let ed = self.ed?;
        if ed.cell.is_some() {
            return self.cell_sh.clone();
        }
        self.slide().shapes.get(ed.shape).cloned()
    }

    /// The text that formatting commands change.
    fn target_doc(&mut self) -> Option<&mut Doc> {
        if let Some(ed) = self.ed {
            if ed.cell.is_some() {
                return self.cell_sh.as_mut().map(|s| &mut s.text);
            }
        }
        let i = self.sel?;
        let c = self.cur;
        let sh = self.deck.slides[c].shapes.get_mut(i)?;
        if !sh.kind.has_text() {
            return None;
        }
        Some(&mut sh.text)
    }

    fn start_edit(&mut self, shape: usize, caret: Option<Pos>) {
        self.notes = None;
        let Some(sh) = self.slide().shapes.get(shape) else { return };
        if !sh.kind.has_text() {
            return;
        }
        let caret = caret.map(|c| sh.text.clamp(c)).unwrap_or(sh.text.end());
        self.sel = Some(shape);
        self.cell_sh = None;
        self.ed = Some(TextEd { shape, cell: None, caret, anchor: None, typing: false });
        self.touch();
    }

    // ---- slides --------------------------------------------------------------

    fn add_slide(&mut self, l: Layout) {
        self.stop_edit();
        self.snapshot();
        let s = self.deck.new_slide(l);
        let at = (self.cur + 1).min(self.deck.slides.len());
        self.deck.slides.insert(at, s);
        self.touch_all();
        self.go(at);
        // start typing the title
        if let Some(t) = self.slide().shapes.iter().position(|s| s.kind == Kind::Title) {
            self.start_edit(t, None);
        }
    }

    fn duplicate_slide(&mut self) {
        self.stop_edit();
        self.snapshot();
        let s = self.slide().clone();
        self.deck.slides.insert(self.cur + 1, s);
        self.touch_all();
        let n = self.cur + 1;
        self.go(n);
    }

    fn delete_slide(&mut self) {
        self.stop_edit();
        self.snapshot();
        if self.deck.slides.len() == 1 {
            let s = self.deck.new_slide(Layout::Title);
            self.deck.slides[0] = s;
        } else {
            self.deck.slides.remove(self.cur);
        }
        self.deck.prune_pics();
        self.pics.clear();
        self.touch_all();
        let c = self.cur.min(self.deck.slides.len() - 1);
        self.go(c);
    }

    fn move_slide(&mut self, to: usize) {
        let to = to.min(self.deck.slides.len() - 1);
        if to == self.cur {
            return;
        }
        let s = self.deck.slides.remove(self.cur);
        self.deck.slides.insert(to, s);
        self.cur = to;
        self.dirty = true;
        self.touch_all();
        self.scroll_to_cur();
    }

    // ---- shapes ------------------------------------------------------------------

    fn add_shape(&mut self, kind: Kind) {
        self.stop_edit();
        self.snapshot();
        let (w, h) = (self.deck.w, self.deck.h);
        let n = self.slide().shapes.len() as i32 % 6;
        let sh = match kind {
            Kind::Text => {
                let mut s = Shape::new(Kind::Text, w / 2 - 260 + n * 24, h / 2 - 40 + n * 24, 520, 80);
                s.size = 24;
                s
            }
            k => Shape::new(k, w / 2 - 150 + n * 24, h / 2 - 100 + n * 24, 300, 200),
        };
        self.slide_mut().shapes.push(sh);
        let i = self.slide().shapes.len() - 1;
        self.sel = Some(i);
        self.touch();
        if kind == Kind::Text {
            self.start_edit(i, None);
        }
    }

    fn insert_picture(&mut self, path: &str, sys: &mut Sys) {
        let Some(data) = sys.fs.read(path) else { return };
        let Some(bytes) = deckio::picture_bytes(&data) else {
            sys.toast("Hyda Slides", &format!("Can't show {} (not a picture Hyda Slides reads)", basename(path)));
            return;
        };
        let Ok(img) = crate::image::decode(&bytes) else { return };
        self.stop_edit();
        self.snapshot();
        // fit in 60% of the slide, keeping its shape
        let (mw, mh) = (self.deck.w * 3 / 5, self.deck.h * 3 / 5);
        let (iw, ih) = (img.w.max(1) as i64, img.h.max(1) as i64);
        let (w, h) = if iw * mh as i64 > ih * mw as i64 { (mw, (mw as i64 * ih / iw) as i32) } else { ((mh as i64 * iw / ih) as i32, mh) };
        self.deck.pics.push(Pic { data: Rc::new(bytes) });
        let mut sh = Shape::new(Kind::Picture, (self.deck.w - w) / 2, (self.deck.h - h) / 2, w.max(8), h.max(8));
        sh.pic = Some(self.deck.pics.len() - 1);
        self.slide_mut().shapes.push(sh);
        self.sel = Some(self.slide().shapes.len() - 1);
        self.touch();
    }

    fn delete_shape(&mut self) {
        let Some(i) = self.sel else { return };
        self.stop_edit();
        self.snapshot();
        let sh = self.slide_mut().shapes.remove(i);
        // a layout placeholder comes back empty, like in other presentation programs
        if sh.kind.placeholder() && !sh.is_empty() {
            let mut e = sh.clone();
            let (st, al) = (e.text.paras[0].style, e.text.paras[0].align);
            e.text = Doc::new();
            e.text.paras[0].style = st;
            e.text.paras[0].align = al;
            self.slide_mut().shapes.insert(i, e);
        }
        self.sel = None;
        self.deck.prune_pics();
        self.pics.clear();
        self.touch();
    }

    fn restack(&mut self, front: bool) {
        let Some(i) = self.sel else { return };
        self.snapshot();
        let sh = self.slide_mut().shapes.remove(i);
        if front {
            self.slide_mut().shapes.push(sh);
            self.sel = Some(self.slide().shapes.len() - 1);
        } else {
            self.slide_mut().shapes.insert(0, sh);
            self.sel = Some(0);
        }
        if let Some(ed) = self.ed.as_mut() {
            ed.shape = self.sel.unwrap();
        }
        self.touch();
    }

    // ---- text editing ----------------------------------------------------------------

    fn ed_shape(&mut self) -> Option<(&mut Shape, &mut TextEd)> {
        let ed = self.ed.as_mut()?;
        if ed.cell.is_some() {
            return Some((self.cell_sh.as_mut()?, ed));
        }
        let c = self.cur;
        let sh = self.deck.slides[c].shapes.get_mut(ed.shape)?;
        Some((sh, ed))
    }

    fn sel_range(ed: &TextEd) -> Option<(Pos, Pos)> {
        let a = ed.anchor?;
        if a == ed.caret {
            return None;
        }
        Some(if a < ed.caret { (a, ed.caret) } else { (ed.caret, a) })
    }

    /// Before an edit: a snapshot (once per typing run) and the selection deleted.
    fn begin_text_change(&mut self, typing: bool) {
        let need = self.ed.map_or(false, |e| !(typing && e.typing));
        if need {
            self.snapshot();
        }
        if let Some((sh, ed)) = self.ed_shape() {
            ed.typing = typing;
            if let Some((a, b)) = Self::sel_range(ed) {
                sh.text.delete(a, b);
                ed.caret = a;
            }
            ed.anchor = None;
        }
    }

    fn type_text(&mut self, s: &str) {
        self.begin_text_change(true);
        if let Some((sh, ed)) = self.ed_shape() {
            let f = sh.text.fmt_at(ed.caret);
            ed.caret = sh.text.insert(ed.caret, s, f);
        }
        self.touch();
    }

    fn enter(&mut self) {
        self.begin_text_change(false);
        if let Some((sh, ed)) = self.ed_shape() {
            ed.caret = sh.text.split(ed.caret);
        }
        self.touch();
    }

    fn backspace(&mut self, forward: bool) {
        let has_sel = self.ed.map_or(false, |e| Self::sel_range(&e).is_some());
        if has_sel {
            self.begin_text_change(false);
            self.touch();
            return;
        }
        // Backspace at the start of an indented or bulleted paragraph outdents first
        if !forward {
            if let Some((sh, ed)) = self.ed_shape() {
                if ed.caret.i == 0 && sh.kind != Kind::Body && sh.text.paras[ed.caret.p].style != Style::Body && ed.caret.p > 0 {
                    // fall through: join paragraphs
                } else if ed.caret.i == 0 && sh.text.paras[ed.caret.p].level > 0 {
                    let p = ed.caret.p;
                    self.snapshot();
                    if let Some((sh, _)) = self.ed_shape() {
                        sh.text.paras[p].level -= 1;
                    }
                    self.touch();
                    return;
                }
            }
        }
        self.begin_text_change(true);
        if let Some((sh, ed)) = self.ed_shape() {
            let c = ed.caret;
            if forward {
                let end = if c.i < sh.text.paras[c.p].len() { Pos::new(c.p, c.i + 1) } else if c.p + 1 < sh.text.paras.len() { Pos::new(c.p + 1, 0) } else { c };
                sh.text.delete(c, end);
            } else {
                let start = if c.i > 0 { Pos::new(c.p, c.i - 1) } else if c.p > 0 { Pos::new(c.p - 1, sh.text.paras[c.p - 1].len()) } else { c };
                sh.text.delete(start, c);
                ed.caret = start;
            }
        }
        self.touch();
    }

    fn move_caret(&mut self, k: Key, shift: bool, word: bool) {
        let Some((sh, ed)) = self.ed_shape() else { return };
        let tb = layout(sh);
        let c = ed.caret;
        let t = &sh.text;
        let is_w = |ch: char| ch.is_alphanumeric();
        let new = match k {
            Key::Left if word => {
                let mut i = c.i;
                let tx = &t.paras[c.p].text;
                while i > 0 && !is_w(tx[i - 1]) {
                    i -= 1;
                }
                while i > 0 && is_w(tx[i - 1]) {
                    i -= 1;
                }
                if c.i == 0 && c.p > 0 {
                    Pos::new(c.p - 1, t.paras[c.p - 1].len())
                } else {
                    Pos::new(c.p, i)
                }
            }
            Key::Right if word => {
                let tx = &t.paras[c.p].text;
                let mut i = c.i;
                while i < tx.len() && is_w(tx[i]) {
                    i += 1;
                }
                while i < tx.len() && !is_w(tx[i]) {
                    i += 1;
                }
                if c.i == tx.len() && c.p + 1 < t.paras.len() {
                    Pos::new(c.p + 1, 0)
                } else {
                    Pos::new(c.p, i)
                }
            }
            Key::Left => {
                if c.i > 0 {
                    Pos::new(c.p, c.i - 1)
                } else if c.p > 0 {
                    Pos::new(c.p - 1, t.paras[c.p - 1].len())
                } else {
                    c
                }
            }
            Key::Right => {
                if c.i < t.paras[c.p].len() {
                    Pos::new(c.p, c.i + 1)
                } else if c.p + 1 < t.paras.len() {
                    Pos::new(c.p + 1, 0)
                } else {
                    c
                }
            }
            Key::Up | Key::Down => {
                let (li, x) = caret_xy(sh, &tb, c);
                let target = if k == Key::Up { li.checked_sub(1) } else { Some(li + 1).filter(|&l| l < tb.lines.len()) };
                match target {
                    Some(l) => hit(sh, &tb, x, tb.lines[l].top + 1),
                    None if k == Key::Up => Pos::new(0, 0),
                    None => t.end(),
                }
            }
            Key::Home => {
                let (li, _) = caret_xy(sh, &tb, c);
                tb.lines.get(li).map(|l| Pos::new(l.p, l.start)).unwrap_or(c)
            }
            Key::End => {
                let (li, _) = caret_xy(sh, &tb, c);
                match tb.lines.get(li) {
                    Some(l) => {
                        let mut e = l.end;
                        let wrapped = tb.lines.get(li + 1).map_or(false, |n| n.p == l.p);
                        if wrapped && e > l.start && t.paras[l.p].text[e - 1] == ' ' {
                            e -= 1;
                        }
                        Pos::new(l.p, e)
                    }
                    None => c,
                }
            }
            _ => c,
        };
        if shift {
            if ed.anchor.is_none() {
                ed.anchor = Some(c);
            }
        } else {
            ed.anchor = None;
        }
        ed.caret = new;
        ed.typing = false;
    }

    /// Bold / italic / underline: on the selection, or the whole shape when it's
    /// only selected.
    fn toggle_fmt(&mut self, bit: u8) {
        let ed = self.ed;
        let range = ed.and_then(|e| Self::sel_range(&e));
        if self.target_doc().is_none() {
            return;
        }
        self.snapshot();
        let Some(doc) = self.target_doc() else { return };
        let (a, b) = range.unwrap_or((Pos::new(0, 0), doc.end()));
        if a == b {
            // nothing selected: the word at the caret
            if let Some(ed) = ed {
                let (ws, we) = doc.word_at(ed.caret);
                if ws != we {
                    let on = !doc.all_have(ws, we, bit);
                    doc.set_fmt(ws, we, bit, on);
                }
            }
        } else {
            let on = !doc.all_have(a, b, bit);
            doc.set_fmt(a, b, bit, on);
        }
        self.touch();
    }

    /// Paragraphs the caret / selection touches (all, when the shape is only selected).
    fn para_range(&self) -> Option<(usize, usize)> {
        if let Some(ed) = self.ed {
            let a = ed.anchor.unwrap_or(ed.caret);
            return Some((a.p.min(ed.caret.p), a.p.max(ed.caret.p)));
        }
        let sh = self.shape()?;
        if !sh.kind.has_text() {
            return None;
        }
        Some((0, sh.text.paras.len() - 1))
    }

    /// The paragraph the toolbar shows the state of.
    fn cur_para(&self) -> Option<crate::doc::Para> {
        let (a, _) = self.para_range()?;
        self.text_target().or_else(|| self.shape().cloned()).and_then(|s| s.text.paras.get(a).cloned())
    }

    fn each_para(&mut self, f: impl Fn(&mut crate::doc::Para)) {
        let Some((a, b)) = self.para_range() else { return };
        if self.target_doc().is_none() {
            return;
        }
        self.snapshot();
        if let Some(doc) = self.target_doc() {
            let b = b.min(doc.paras.len() - 1);
            for p in doc.paras[a.min(b)..=b].iter_mut() {
                f(p);
            }
        }
        self.touch();
    }

    fn set_list(&mut self, style: Style) {
        let Some(p) = self.cur_para() else { return };
        let on = p.style != style;
        self.each_para(move |p| p.style = if on { style } else { Style::Body });
    }

    fn indent(&mut self, out: bool) {
        self.each_para(move |p| {
            if out {
                p.level = p.level.saturating_sub(1);
            } else {
                p.level = (p.level + 1).min(4);
            }
        });
    }

    fn resize_text(&mut self, bigger: bool) {
        let Some(i) = self.sel else { return };
        self.snapshot();
        let steps = [8u16, 10, 12, 14, 16, 18, 20, 24, 28, 32, 36, 40, 44, 48, 54, 60, 72, 88, 96, 120];
        let c = self.cur;
        let sh = &mut self.deck.slides[c].shapes[i];
        sh.size = if bigger { steps.iter().copied().find(|&s| s > sh.size).unwrap_or(sh.size) } else { steps.iter().rev().copied().find(|&s| s < sh.size).unwrap_or(sh.size) };
        self.touch();
    }

    fn copy(&mut self, sys: &mut Sys) {
        if let Some(ed) = self.ed {
            if let Some((a, b)) = Self::sel_range(&ed) {
                let Some(sh) = self.text_target() else { return };
                sys.clipboard = Doc::plain(&sh.text.slice(a, b));
                self.clip_shape = None;
            }
            return;
        }
        if let Some(sh) = self.shape() {
            let pic = sh.pic.and_then(|p| self.deck.pics.get(p).cloned());
            let text = if sh.is_empty() { String::from("(shape)") } else { sh.plain() };
            self.clip_shape = Some((sh.clone(), pic));
            self.clip_text = text.clone();
            sys.clipboard = text;
        }
    }

    fn paste(&mut self, sys: &mut Sys) {
        if self.ed.is_some() {
            let t = sys.clipboard.clone();
            if !t.is_empty() {
                self.type_text(&t);
            }
            return;
        }
        match &self.clip_shape {
            Some((sh, pic)) if sys.clipboard == self.clip_text => {
                let (mut sh, pic) = (sh.clone(), pic.clone());
                self.snapshot();
                if let Some(p) = pic {
                    self.deck.pics.push(p);
                    sh.pic = Some(self.deck.pics.len() - 1);
                }
                // pasted copies step down and right
                sh.x += 24;
                sh.y += 24;
                if sh.kind.placeholder() {
                    sh.kind = Kind::Text;
                }
                self.clip_shape.as_mut().unwrap().0 = sh.clone();
                self.slide_mut().shapes.push(sh);
                self.sel = Some(self.slide().shapes.len() - 1);
                self.touch();
            }
            _ => {
                let t = sys.clipboard.clone();
                if t.is_empty() {
                    return;
                }
                self.add_shape(Kind::Text);
                self.type_text(&t);
            }
        }
    }

    // ---- files ----------------------------------------------------------------------

    fn title(&self) -> String {
        if self.path.is_empty() {
            return String::from("Untitled presentation");
        }
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[..k]).unwrap_or(b).to_string()
    }

    fn ext(&self) -> &str {
        let b = basename(&self.path);
        b.rfind('.').map(|k| &b[k..]).unwrap_or("")
    }

    fn save(&mut self, sys: &mut Sys, quiet: bool) {
        self.stop_edit();
        if self.path.is_empty() {
            let name = self.deck.slides.iter().map(|s| s.title()).find(|t| !t.trim().is_empty()).unwrap_or_else(|| String::from("Untitled presentation"));
            let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(60).collect();
            self.path = sys.fs.unique("/home/Documents/Presentations", name.trim(), ".hydp");
        }
        let mut from = None;
        if self.imported {
            from = Some(self.ext().to_string());
            self.path = sys.fs.unique(&parent(&self.path), &self.title(), ".hydp");
            self.imported = false;
        }
        self.deck.name = self.title();
        if self.deck.author.is_empty() {
            self.deck.author = sys.profile.name.clone();
        }
        let ok = sys.fs.write(&self.path, deckio::to_hydp(&self.deck).as_bytes());
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
            sys.toast("Hyda Slides", &msg);
        }
    }

    fn export(&mut self, sys: &mut Sys) {
        self.stop_edit();
        let dir = if self.path.is_empty() { String::from("/home/Documents/Presentations") } else { parent(&self.path) };
        let name = self.title();
        let target = join(&dir, &format!("{}.pptx", name));
        let path = if sys.fs.exists(&target) { sys.fs.unique(&dir, &name, ".pptx") } else { target };
        if self.deck.author.is_empty() {
            self.deck.author = sys.profile.name.clone();
        }
        let mut d = self.deck.clone();
        d.name = name;
        let ok = sys.fs.write(&path, &deckio::to_pptx(&d));
        sys.toast("Hyda Slides", &if ok { format!("Exported {}", basename(&path)) } else { String::from("Couldn't export: the disk is read-only") });
    }

    fn park(&mut self, sys: &mut Sys) {
        self.stop_edit();
        let blank = self.deck.slides.len() == 1 && self.deck.slides[0].shapes.iter().all(|s| s.is_empty() && s.kind.placeholder());
        if self.dirty && (!self.path.is_empty() || !blank) {
            self.save(sys, true);
        }
    }

    fn reset(&mut self, d: Deck) {
        self.deck = d;
        self.path.clear();
        self.dirty = false;
        self.imported = false;
        self.cur = 0;
        self.sel = None;
        self.ed = None;
        self.notes = None;
        self.undo.clear();
        self.redo.clear();
        self.strip_scroll = 0;
        self.pics.clear();
        self.view = None;
        self.touch_all();
    }

    fn load(&mut self, path: &str, sys: &mut Sys) {
        let Some(data) = sys.fs.read(path) else {
            sys.toast("Hyda Slides", &format!("Couldn't open {}", basename(path)));
            return;
        };
        let lower = path.to_ascii_lowercase();
        let parsed = if lower.ends_with(".pptx") { deckio::from_pptx(&data) } else { deckio::from_hydp(&data) };
        match parsed {
            Ok(d) => {
                self.park(sys);
                self.reset(d);
                self.path = path.to_string();
                self.imported = lower.ends_with(".pptx");
            }
            Err(why) => sys.toast("Hyda Slides", &format!("Can't open {}: {}", basename(path), why)),
        }
    }

    fn list_files(sys: &Sys, exts: &[&str], own: &str) -> Vec<String> {
        let (mut mine, mut other) = (Vec::new(), Vec::new());
        for dir in ["/home/Documents/Presentations", "/home/Documents", "/home/Pictures", "/home", "/home/Downloads", "/home/Shared"] {
            for (name, d, _) in sys.fs.list(dir) {
                let l = name.to_ascii_lowercase();
                if d {
                    continue;
                }
                if l.ends_with(own) {
                    mine.push(join(dir, &name));
                } else if exts.iter().any(|e| l.ends_with(e)) {
                    other.push(join(dir, &name));
                }
            }
        }
        mine.extend(other);
        mine
    }

    fn rename_to(&mut self, name: &str, sys: &mut Sys) {
        let name: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let dir = if self.path.is_empty() { String::from("/home/Documents/Presentations") } else { parent(&self.path) };
        if self.imported || self.path.is_empty() {
            self.path = sys.fs.unique(&dir, name, ".hydp");
            self.imported = false;
            self.dirty = true;
            self.save(sys, true);
            return;
        }
        let target = join(&dir, &format!("{}.hydp", name));
        if target != self.path {
            let target = if sys.fs.exists(&target) { sys.fs.unique(&dir, name, ".hydp") } else { target };
            if sys.fs.exists(&self.path) {
                sys.fs.rename(&self.path, &target);
            }
            self.path = target;
        }
        self.dirty = true;
        self.save(sys, true);
    }

    // ---- slideshow ------------------------------------------------------------------

    fn play(&mut self, from_start: bool) {
        self.stop_edit();
        self.notes = None;
        self.overlay = Overlay::None;
        let idx = if from_start { 0 } else { self.cur };
        self.show = Some(Show { idx, from: None, end: false, black: false, frame: None, step: 0, playing: None, before: None, presenter: false, started: self.ticks, next: None });
    }

    fn show_step(&mut self, forward: bool) {
        let n = self.deck.slides.len();
        let ticks = self.ticks;
        let Some(sh) = self.show.as_mut() else { return };
        sh.black = false;
        let steps = anim_steps(&self.deck.slides[sh.idx.min(n - 1)]).len();
        if forward {
            if sh.end {
                let idx = sh.idx;
                self.show = None;
                self.go(idx.min(n - 1));
                return;
            }
            if sh.playing.is_some() {
                // a click during an animation finishes it
                sh.playing = None;
                sh.before = None;
                return;
            }
            if sh.step < steps {
                sh.before = sh.frame.as_ref().map(|f| clone_canvas(&f.5));
                sh.step += 1;
                sh.playing = Some(ticks);
                return;
            }
            if sh.idx + 1 >= n {
                sh.end = true;
                sh.from = None;
                return;
            }
            let prev = sh.frame.take().map(|f| f.5);
            sh.idx += 1;
            sh.step = 0;
            sh.from = if self.deck.slides[sh.idx].trans != Trans::None { prev.map(|c| (c, ticks)) } else { None };
        } else {
            if sh.end {
                sh.end = false;
                return;
            }
            sh.playing = None;
            sh.before = None;
            if sh.step > 0 {
                sh.step -= 1;
                return;
            }
            if sh.idx > 0 {
                sh.idx -= 1;
                sh.step = anim_steps(&self.deck.slides[sh.idx]).len();
                sh.from = None;
                sh.frame = None;
            }
        }
    }

    fn render_show(&mut self, ui: &mut Ui, r: Rect, inst: u32) {
        let s = ui.s;
        let full = r.scale(s);
        ui.c.fill_rect(full, Color::rgb(0));
        ui.zone(r, Action::App(inst, C_SHOW));
        let Some(show) = self.show.as_ref() else { return };
        if show.presenter {
            self.render_presenter(ui, r);
            return;
        }
        if show.end || show.black {
            if show.end {
                ui.text_in(Rect::new(r.x, r.y + r.h / 2 - 20, r.w, 40), Face::Regular, 15, "End of slideshow. Click or press Esc to leave.", Color::rgb(0xB8B4C4), 1);
            }
            return;
        }
        // fit the slide to the screen
        let (dw, dh) = (self.deck.w as i64, self.deck.h as i64);
        let (mut w, mut h) = (full.w as i64, full.w as i64 * dh / dw);
        if h > full.h as i64 {
            h = full.h as i64;
            w = h * dw / dh;
        }
        let (w, h) = (w as i32, h as i32);
        self.draw_current(ui.c, full.x + (full.w - w) / 2, full.y + (full.h - h) / 2, w, h);
    }

    /// The slide being shown, drawn at (x, y) w × h pixels: its animations
    /// and the transition into it.
    fn draw_current(&mut self, c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        let ticks = self.ticks;
        let gen = self.gen;
        let Some(show) = self.show.as_mut() else { return };
        let idx = show.idx.min(self.deck.slides.len() - 1);
        let slide = &self.deck.slides[idx];
        let steps = anim_steps(slide);
        let step = show.step.min(steps.len());
        let fresh = !matches!(&show.frame, Some((i, fw, fh, g, st, _)) if *i == idx && *fw == w && *fh == h && *g == gen && *st == step);
        if fresh {
            let mut cv = Canvas::new(w, h);
            let o = Opts { number: idx + 1, shown: Some(step), ..Opts::default() };
            draw_slide(&mut cv, 0, 0, w, &self.deck, slide, &mut self.pics, &o);
            show.frame = Some((idx, w, h, gen, step, cv));
        }
        // an animation playing
        if let Some(t0) = show.playing {
            let p = ((ticks.saturating_sub(t0)) * 256 / ANIM_TICKS).min(256) as u32;
            let e = if p < 128 { p * p / 128 } else { 256 - (256 - p) * (256 - p) / 128 };
            let who = steps.get(step.wrapping_sub(1)).copied();
            if p >= 256 || who.is_none() {
                show.playing = None;
                show.before = None;
            } else {
                let who = who.unwrap();
                match slide.shapes[who].anim {
                    Anim::Fly => {
                        let dy = ((self.deck.h - slide.shapes[who].y) as i64 * (256 - e) as i64 / 256) as i32;
                        let mut cv = Canvas::new(w, h);
                        let o = Opts { number: idx + 1, shown: Some(step - 1), flying: Some((who, dy)), ..Opts::default() };
                        draw_slide(&mut cv, 0, 0, w, &self.deck, slide, &mut self.pics, &o);
                        blit(c, &cv, x, y);
                        return;
                    }
                    Anim::Fade => {
                        if let (Some(before), Some(f)) = (&show.before, &show.frame) {
                            if before.w == w && before.h == h {
                                blend(c, before, &f.5, x, y, e);
                                return;
                            }
                        }
                    }
                    _ => {
                        show.playing = None;
                    }
                }
            }
        }
        let cur = &show.frame.as_ref().unwrap().5;
        match &show.from {
            Some((prev, t0)) if prev.w == w && prev.h == h && ticks < t0 + TRANS_TICKS => {
                let p = ((ticks - t0) * 256 / TRANS_TICKS) as u32;
                let e = if p < 128 { p * p / 128 } else { 256 - (256 - p) * (256 - p) / 128 };
                if slide.trans == Trans::Push {
                    let off = (h as u32 * e / 256) as i32;
                    let old = c.set_clip(c.clip.intersect(&Rect::new(x, y, w, h)));
                    blit(c, prev, x, y - off);
                    blit(c, cur, x, y + h - off);
                    c.set_clip(old);
                } else {
                    blend(c, prev, cur, x, y, e);
                }
            }
            _ => {
                show.from = None;
                blit(c, cur, x, y);
            }
        }
    }

    /// Presenter view: this slide, the next one, the notes and a clock.
    fn render_presenter(&mut self, ui: &mut Ui, r: Rect) {
        let s = ui.s;
        let (bg, text, dim) = (Color::rgb(0x16151D), Color::rgb(0xECE8F4), Color::rgb(0x9C98AC));
        ui.rect(r, bg);
        let n = self.deck.slides.len();
        let Some(show) = self.show.as_ref() else { return };
        let (idx, end, step, started) = (show.idx.min(n - 1), show.end, show.step, show.started);
        let secs = (self.ticks.saturating_sub(started) / 100) as u32;
        let clock = format!("{:02}:{:02}", secs / 60, secs % 60);
        ui.text(r.x + 24, r.y + 34, Face::Semibold, 15, &format!("Slide {} of {}", idx + 1, n), text);
        let cw = ui.tw(Face::Semibold, 22, &clock);
        ui.text(r.x + (r.w - cw) / 2, r.y + 36, Face::Semibold, 22, &clock, Color::rgb(0xF2B544));
        let hint = "Click or → next  ·  ← back  ·  V audience view  ·  Esc end";
        let hw = ui.tw(Face::Regular, 12, hint);
        ui.text(r.r() - hw - 24, r.y + 33, Face::Regular, 12, hint, dim);
        // this slide
        let mw = r.w * 62 / 100 - 36;
        let mh = (mw * self.deck.h / self.deck.w).min(r.h - 64 - 150);
        let mw = mh * self.deck.w / self.deck.h;
        let main = Rect::new(r.x + 24, r.y + 64, mw, mh);
        if end {
            ui.rect(main, Color::rgb(0));
            ui.text_in(main, Face::Regular, 15, "End of slideshow", dim, 1);
        } else {
            self.draw_current(ui.c, main.x * s, main.y * s, main.w * s, main.h * s);
        }
        // next
        let col = Rect::new(main.r() + 24, main.y, r.r() - main.r() - 48, 0);
        ui.text(col.x, col.y + 14, Face::Semibold, 13, "NEXT", dim);
        let nh = col.w * self.deck.h / self.deck.w;
        let nr = Rect::new(col.x, col.y + 26, col.w, nh);
        let steps = anim_steps(&self.deck.slides[idx]).len();
        if step < steps && !end {
            ui.rect(nr, Color::rgb(0x24222E));
            ui.text_in(nr, Face::Regular, 14, &format!("{} more on this slide", steps - step), dim, 1);
        } else if idx + 1 < n && !end {
            let key = (idx + 1, nr.w * s, self.gen);
            let stale = !matches!(&self.show.as_ref().unwrap().next, Some((i, w, g, _)) if (*i, *w, *g) == key);
            if stale {
                let mut cv = Canvas::new(nr.w * s, nr.h * s);
                let o = Opts { number: idx + 2, ..Opts::default() };
                draw_slide(&mut cv, 0, 0, nr.w * s, &self.deck, &self.deck.slides[idx + 1], &mut self.pics, &o);
                self.show.as_mut().unwrap().next = Some((key.0, key.1, key.2, cv));
            }
            if let Some((_, _, _, cv)) = &self.show.as_ref().unwrap().next {
                blit(ui.c, cv, nr.x * s, nr.y * s);
            }
        } else {
            ui.rect(nr, Color::rgb(0x24222E));
            ui.text_in(nr, Face::Regular, 14, "End of slideshow", dim, 1);
        }
        // notes
        let notes = self.deck.slides[idx].notes.clone();
        let ny = main.b() + 24;
        ui.text(main.x, ny + 14, Face::Semibold, 13, "NOTES", dim);
        let area = Rect::new(main.x, ny + 24, r.r() - main.x - 24, r.b() - ny - 36);
        let old = ui.clip_in(area);
        let mut y = area.y + 20;
        if notes.is_empty() {
            ui.text(area.x, y, Face::Regular, 16, "No notes for this slide.", dim);
        }
        for line in ui.wrap(Face::Regular, 18, &notes, area.w) {
            ui.text(area.x, y, Face::Regular, 18, &line, text);
            y += 26;
        }
        ui.set_clip(old);
    }

    // ---- drawing the editor ---------------------------------------------------------

    fn to_screen(&self, u: i32) -> i32 {
        (u as i64 * self.canvas.w as i64 / self.deck.w.max(1) as i64) as i32
    }

    fn to_units(&self, x: i32, y: i32) -> (i32, i32) {
        let k = |v: i32, o: i32| ((v - o) as i64 * self.deck.w as i64 / self.canvas.w.max(1) as i64) as i32;
        (k(x, self.canvas.x), k(y, self.canvas.y))
    }

    fn shape_rect(&self, sh: &Shape) -> Rect {
        let x = self.canvas.x + self.to_screen(sh.x);
        let y = self.canvas.y + self.to_screen(sh.y);
        Rect::new(x, y, self.canvas.x + self.to_screen(sh.x + sh.w) - x, self.canvas.y + self.to_screen(sh.y + sh.h) - y)
    }


    fn tool(&self, ui: &mut Ui, r: Rect, label: &str, icon: Option<Icon>, face: Face, code: u32, inst: u32, on: bool, enabled: bool) {
        let t = ui.t;
        let a = Action::App(inst, code);
        if on {
            ui.rrect(r, 8, t.accent.with_alpha(45));
        } else if ui.hot(a) && enabled {
            ui.rrect(r, 8, t.hover);
        }
        let col = if !enabled { t.text3 } else if on { t.accent } else { t.text };
        match icon {
            Some(i) => ui.icon_in(i, r, 16, col),
            None => {
                ui.text_in(r, face, 14, label, col, 1);
            }
        }
        if code == C_UNDERLINE {
            let w = ui.tw(face, 14, label);
            ui.rect(Rect::new(r.x + (r.w - w) / 2, r.y + 23, w, 1), col);
        }
        ui.zone(r, a);
    }

    fn cur_fmt(&self) -> u8 {
        let Some(sh) = self.text_target().or_else(|| self.shape().cloned()) else { return 0 };
        let sh = &sh;
        match self.ed {
            Some(ed) => {
                let (a, b) = Self::sel_range(&ed).unwrap_or((ed.caret, ed.caret));
                let a = sh.text.clamp(a);
                if a == b {
                    sh.text.fmt_at(a)
                } else {
                    sh.text.paras[a.p].fmt.get(a.i).copied().unwrap_or(0)
                }
            }
            None => sh.text.paras.first().and_then(|p| p.fmt.first().copied()).unwrap_or(0),
        }
    }

    fn render_toolbar(&self, ui: &mut Ui, r: Rect, inst: u32, compact: bool) {
        let t = ui.t;
        let y = r.y + HEADER;
        ui.rect(Rect::new(r.x, y, r.w, TOOLBAR), t.surface);
        ui.rect(Rect::new(r.x, y + TOOLBAR - 1, r.w, 1), t.line);
        let b = |x: i32, w: i32| Rect::new(x, y + 7, w, 32);
        let sep = |ui: &mut Ui, x: i32| ui.rect(Rect::new(x, y + 12, 1, 20), t.line);
        let text_on = self.shape().map_or(false, |s| s.kind.has_text());
        let fmt = self.cur_fmt();
        let para = self.cur_para();
        let mut x = r.x + 10;
        // new slide
        let nr = b(x, if compact { 34 } else { 92 });
        let na = Action::App(inst, C_NEW_SLIDE);
        ui.rrect(nr, 8, if ui.hot(na) { t.accent.mix(t.text, 20) } else { t.accent });
        if compact {
            ui.icon_in(Icon::Plus, nr, 16, t.on_accent);
        } else {
            ui.icon(Icon::Plus, nr.x + 9, nr.y + 8, 16, t.on_accent);
            ui.text(nr.x + 30, nr.y + 21, Face::Semibold, 13, "Slide", t.on_accent);
        }
        ui.zone(nr, na);
        x = nr.r() + 6;
        if !compact {
            match self.shape().map(|s| s.kind) {
                Some(Kind::Table) => self.tool(ui, b(x, 64), "Table ›", None, Face::Medium, C_OBJECT, inst, matches!(self.overlay, Overlay::Menu(MenuKind::TableEdit, _)), true),
                Some(Kind::Chart) => self.tool(ui, b(x, 64), "Chart ›", None, Face::Medium, C_OBJECT, inst, matches!(self.overlay, Overlay::Menu(MenuKind::ChartEdit, _)), true),
                _ => self.tool(ui, b(x, 64), "Layout", None, Face::Medium, C_LAYOUT, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Layout, _)), true),
            }
            x += 70;
        }
        sep(ui, x - 3);
        x += 4;
        self.tool(ui, b(x, 30), "B", None, Face::Semibold, C_BOLD, inst, text_on && fmt & BOLD != 0, text_on);
        x += 32;
        self.tool(ui, b(x, 30), "I", None, Face::Italic, C_ITALIC, inst, text_on && fmt & ITALIC != 0, text_on);
        x += 32;
        self.tool(ui, b(x, 30), "U", None, Face::Regular, C_UNDERLINE, inst, text_on && fmt & UNDERLINE != 0, text_on);
        x += 36;
        if !compact {
            sep(ui, x - 3);
            for (icon, code, al) in [(Icon::AlignLeft, C_LEFT, Align::Left), (Icon::AlignCenter, C_CENTER, Align::Center), (Icon::AlignRight, C_RIGHT, Align::Right)] {
                self.tool(ui, b(x, 30), "", Some(icon), Face::Regular, code, inst, para.as_ref().map_or(false, |p| p.align == al), text_on);
                x += 32;
            }
            x += 4;
            sep(ui, x - 3);
            self.tool(ui, b(x, 30), "", Some(Icon::ListBullet), Face::Regular, C_BULLETS, inst, para.as_ref().map_or(false, |p| p.style == Style::Bullet), text_on);
            x += 32;
            self.tool(ui, b(x, 30), "", Some(Icon::ListNumber), Face::Regular, C_NUMBERS, inst, para.as_ref().map_or(false, |p| p.style == Style::Number), text_on);
            x += 36;
            sep(ui, x - 3);
            self.tool(ui, b(x, 30), "A-", None, Face::Medium, C_SMALLER, inst, false, text_on);
            x += 32;
            self.tool(ui, b(x, 30), "A+", None, Face::Medium, C_BIGGER, inst, false, text_on);
            x += 36;
            sep(ui, x - 3);
        }
        self.tool(ui, b(x, 30), "", Some(Icon::TextBox), Face::Regular, C_TEXTBOX, inst, false, true);
        x += 32;
        self.tool(ui, b(x, 30), "", Some(Icon::Shapes), Face::Regular, C_RECT, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Shapes, _)), true);
        x += 32;
        if !compact {
            self.tool(ui, b(x, 30), "", Some(Icon::Table), Face::Regular, C_TABLE, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Table, _)), true);
            x += 32;
            self.tool(ui, b(x, 30), "", Some(Icon::Chart), Face::Regular, C_CHART, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Chart, _)), true);
            x += 32;
        }
        self.tool(ui, b(x, 30), "", Some(Icon::Image), Face::Regular, C_PICTURE, inst, false, true);
        x += 36;
        sep(ui, x - 3);
        // colour: shows the selected shape's colour
        let cr = b(x, 34);
        let ca = Action::App(inst, C_COLOR);
        if ui.hot(ca) {
            ui.rrect(cr, 8, t.hover);
        }
        let swatch = match self.shape() {
            Some(sh) if matches!(sh.kind, Kind::Rect | Kind::Ellipse) => sh.fill.unwrap_or(self.deck.theme.accent),
            Some(sh) if sh.kind == Kind::Picture => sh.line.unwrap_or(0xFFFFFF),
            Some(sh) if sh.kind == Kind::Line => sh.line.unwrap_or(self.deck.theme.text),
            Some(sh) if sh.kind == Kind::Chart => self.deck.theme.accent,
            Some(sh) => deckio::text_color(sh, &self.deck.theme),
            None => self.slide().bg.unwrap_or(self.deck.theme.bg),
        };
        ui.text_in(Rect::new(cr.x, cr.y - 3, cr.w, cr.h), Face::Semibold, 14, "A", t.text, 1);
        ui.rect(Rect::new(cr.x + 8, cr.y + 23, cr.w - 16, 5), Color::rgb(swatch));
        ui.stroke(Rect::new(cr.x + 8, cr.y + 23, cr.w - 16, 5), 0, 1, t.line);
        ui.zone(cr, ca);
        x += 38;
        if !compact {
            self.tool(ui, b(x, 62), "Theme", None, Face::Medium, C_THEME, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Theme, _)), true);
            x += 64;
            if x + 150 < r.r() {
                self.tool(ui, b(x, 84), "Transition", None, Face::Medium, C_TRANS, inst, matches!(self.overlay, Overlay::Menu(MenuKind::Trans, _)), true);
                x += 88;
            }
            if x + 140 < r.r() {
                let animated = self.shape().map_or(false, |s| s.anim != Anim::None);
                self.tool(ui, b(x, 70), "Animate", None, Face::Medium, C_ANIM, inst, animated || matches!(self.overlay, Overlay::Menu(MenuKind::Anim, _)), self.sel.is_some());
                x += 74;
            }
            sep(ui, x - 3);
            if x + 70 < r.r() {
                self.tool(ui, b(x, 30), "", Some(Icon::Undo), Face::Regular, C_UNDO, inst, false, !self.undo.is_empty());
                x += 32;
                self.tool(ui, b(x, 30), "", Some(Icon::Redo), Face::Regular, C_REDO, inst, false, !self.redo.is_empty());
            }
        }
    }

    fn render_strip(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let t = ui.t;
        self.strip = area;
        ui.rect(area, t.sidebar);
        ui.rect(Rect::new(area.r() - 1, area.y, 1, area.h), t.line);
        let tw = STRIP_W - 44;
        let th = tw * self.deck.h / self.deck.w;
        let step = self.thumb_step();
        let body = Rect::new(area.x, area.y, area.w, area.h - 44);
        let max = (self.deck.slides.len() as i32 * step + 12 - body.h).max(0);
        self.strip_scroll = self.strip_scroll.clamp(0, max);
        ui.zone(body, Action::App(inst, C_STRIP));
        let old = ui.clip_in(body);
        let s = ui.s;
        let n = self.deck.slides.len();
        if self.thumbs.len() != n {
            self.touch_all();
        }
        for i in 0..n {
            let y = body.y + 12 + i as i32 * step - self.strip_scroll;
            if y + step < body.y || y > body.b() {
                continue;
            }
            let r = Rect::new(area.x + 32, y, tw, th);
            let on = i == self.cur;
            // the thumbnail: drawn big, then shrunk
            let pw = tw * s;
            let key = self.thumb_gen[i];
            let stale = !matches!(&self.thumbs[i], Some((g, c)) if *g == key && c.w == pw);
            if stale {
                let big = (pw * 3).max(360);
                let mut c = Canvas::new(big, big * self.deck.h / self.deck.w);
                draw_slide(&mut c, 0, 0, big, &self.deck, &self.deck.slides[i], &mut self.pics, &Opts { number: i + 1, ..Opts::default() });
                let mut small = Canvas::new(pw, th * s);
                let b = small.bounds();
                small.blit_scaled(&c, b, 0);
                self.thumbs[i] = Some((key, small));
            }
            if let Some((_, c)) = &self.thumbs[i] {
                blit(ui.c, c, r.x * s, r.y * s);
            }
            ui.stroke(r.inset(-2), 4, if on { 2 } else { 1 }, if on { t.accent } else { t.line });
            ui.text_in(Rect::new(area.x + 4, y, 24, 18), if on { Face::Semibold } else { Face::Regular }, 12, &format!("{}", i + 1), if on { t.accent } else { t.text2 }, 2);
            if self.deck.slides[i].trans != Trans::None {
                ui.icon(Icon::Play, area.x + 12, y + 24, 9, t.text3);
            }
            ui.zone(r.inset(-2), Action::App(inst, C_THUMB + i as u32));
        }
        ui.set_clip(old);
        // add a slide
        let ar = Rect::new(area.x + 14, area.b() - 40, area.w - 28, 32);
        let aa = Action::App(inst, C_ADD_SLIDE);
        ui.rrect(ar, 8, if ui.hot(aa) { t.hover } else { t.chip });
        ui.icon(Icon::Plus, ar.x + 12, ar.y + 9, 14, t.text);
        ui.text(ar.x + 32, ar.y + 21, Face::Medium, 13, "New slide", t.text);
        ui.zone(ar, aa);
    }

    fn render_canvas(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let t = ui.t;
        let bg = if t.dark { Color::rgb(0x15141C) } else { Color::rgb(0xE9E4DC) };
        ui.rect(area, bg);
        ui.zone(area, Action::App(inst, C_CANVAS));
        // fit the slide
        let (mw, mh) = (area.w - 48, area.h - 40);
        let (mut w, mut h) = (mw, mw * self.deck.h / self.deck.w);
        if h > mh {
            h = mh;
            w = h * self.deck.w / self.deck.h;
        }
        let r = Rect::new(area.x + (area.w - w) / 2, area.y + (area.h - h) / 2, w.max(40), h.max(20));
        self.canvas = r;
        ui.shadow(r, 2, 10, 3, 40);
        let s = ui.s;
        let editing = self.ed.map(|e| e.shape);
        let key = (self.gen, self.cur, r.w * s, editing);
        if self.view.as_ref().map_or(true, |(k, _)| *k != key) {
            let mut c = Canvas::new(r.w * s, r.h * s);
            let o = Opts { prompts: true, editing, number: self.cur + 1, ..Opts::default() };
            draw_slide(&mut c, 0, 0, r.w * s, &self.deck, &self.deck.slides[self.cur], &mut self.pics, &o);
            self.view = Some((key, c));
        }
        if let Some((_, c)) = &self.view {
            let old = ui.clip_in(area);
            blit(ui.c, c, r.x * s, r.y * s);
            ui.set_clip(old);
        }
        let old = ui.clip_in(area);
        // animation order badges
        let steps = anim_steps(self.slide());
        for (k, &i) in steps.iter().enumerate() {
            let sh = &self.slide().shapes[i];
            let (bx, by) = (self.canvas.x + self.to_screen(sh.x) - 6, self.canvas.y + self.to_screen(sh.y) - 6);
            ui.circle(bx, by, 9, t.accent);
            ui.text_in(Rect::new(bx - 9, by - 9, 18, 18), Face::Semibold, 11, &format!("{}", k + 1), t.on_accent, 1);
        }
        // selection, caret
        if let (Some(ed), Some(sh)) = (self.ed, self.text_target()) {
            let sr = self.shape_rect(&sh);
            ui.stroke(sr.inset(-2), 0, 1, t.accent);
            let tb = layout(&sh);
            if let Some((a, b)) = Self::sel_range(&ed) {
                for l in &tb.lines {
                    let (ls, le) = (Pos::new(l.p, l.start), Pos::new(l.p, l.end));
                    if le < a || ls > b {
                        continue;
                    }
                    let p = &sh.text.paras[l.p];
                    let s0 = if a > ls { a.i } else { l.start };
                    let e0 = if b < le { b.i } else { l.end };
                    let x0 = l.x + span_w(sh.kind, p, l.start, s0, l.px);
                    let x1 = (l.x + span_w(sh.kind, p, l.start, e0, l.px)).max(x0 + if b > le { 6 } else { 0 });
                    let hr = Rect::new(sr.x + self.to_screen(x0), sr.y + self.to_screen(l.top), self.to_screen(x1 - x0).max(2), self.to_screen(l.h).max(2));
                    ui.rect(hr, t.accent.with_alpha(70));
                }
            }
            if (ui.ticks / 50) % 2 == 0 || self.ticks < 10 {
                let (li, cx) = caret_xy(&sh, &tb, ed.caret);
                let (top, h) = tb.lines.get(li).map(|l| (l.top, l.h)).unwrap_or((INSET_Y, 30));
                let col = match ed.cell {
                    Some((r, _)) => self.slide().shapes.get(ed.shape).and_then(|t| t.table.as_ref()).map(|tb| cell_style(&self.deck.theme, tb, r).1).unwrap_or(0),
                    None => deckio::text_color(&sh, &self.deck.theme),
                };
                ui.rect(Rect::new(sr.x + self.to_screen(cx), sr.y + self.to_screen(top), 2, self.to_screen(h).max(8)), Color::rgb(col));
            }
        } else if let Some(sh) = self.shape().cloned() {
            let s = ui.s;
            let acc = t.accent.0 & 0xFFFFFF;
            if sh.kind == Kind::Line {
                for (hx, hy) in self.line_handles(&sh) {
                    ui.circle(hx, hy, 6, Color::rgb(0xFFFFFF));
                    ui.stroke(Rect::new(hx - 6, hy - 6, 12, 12), 6, 2, t.accent);
                }
            } else {
                let (hs, rh) = self.handle_points(&sh);
                // the outline, turned with the shape
                let corners: Vec<(i32, i32)> = [hs[0], hs[2], hs[4], hs[6]].iter().map(|&(x, y)| (x * s * 16, y * s * 16)).collect();
                slidedraw::stroke_poly(ui.c, &corners, 2 * s * 16, acc);
                // the turning handle
                let top = hs[1];
                ui.c.fill_rect(Rect::new(top.0 * s, rh.1.min(top.1) * s, s, (rh.1 - top.1).abs() * s), Color::rgb(acc));
                let _ = top;
                ui.circle(rh.0, rh.1, 7, Color::rgb(0xFFFFFF));
                ui.stroke(Rect::new(rh.0 - 7, rh.1 - 7, 14, 14), 7, 2, t.accent);
                for (hx, hy) in hs {
                    let hr = Rect::new(hx - 4, hy - 4, 9, 9);
                    ui.rect(hr, Color::rgb(0xFFFFFF));
                    ui.stroke(hr, 0, 1, t.accent);
                }
            }
        }
        ui.set_clip(old);
    }

    /// Screen positions of a line's two ends.
    fn line_handles(&self, sh: &Shape) -> [(i32, i32); 2] {
        let (a, b) = line_ends(sh);
        [(self.canvas.x + self.to_screen(a.0), self.canvas.y + self.to_screen(a.1)), (self.canvas.x + self.to_screen(b.0), self.canvas.y + self.to_screen(b.1))]
    }

    /// Screen positions of a shape's eight sizing handles (turned with it),
    /// and of its turning handle.
    fn handle_points(&self, sh: &Shape) -> ([(i32, i32); 8], (i32, i32)) {
        let (cx, cy) = (sh.x + sh.w / 2, sh.y + sh.h / 2);
        let (l, t, r, b) = (sh.x, sh.y, sh.x + sh.w, sh.y + sh.h);
        let local = [(l, t), (cx, t), (r, t), (r, cy), (r, b), (cx, b), (l, b), (l, cy)];
        let scr = |(x, y): (i32, i32)| {
            let (x, y) = rotate(x, y, cx, cy, sh.rot);
            (self.canvas.x + self.to_screen(x), self.canvas.y + self.to_screen(y))
        };
        let hs = local.map(scr);
        // the turning handle sits 26 px above the top edge
        let up = (26i64 * self.deck.w as i64 / self.canvas.w.max(1) as i64) as i32;
        (hs, scr((cx, t - up)))
    }

    fn render_notes(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let t = ui.t;
        self.notes_r = area;
        ui.rect(area, t.surface);
        ui.rect(Rect::new(area.x, area.y, area.w, 1), t.line);
        let focused = self.notes.is_some();
        let text = self.slide().notes.clone();
        let inner = Rect::new(area.x + 16, area.y + 8, area.w - 32, area.h - 14);
        let old = ui.clip_in(inner);
        if text.is_empty() && !focused {
            ui.text(inner.x, inner.y + 18, Face::Regular, 13, "Click to add speaker notes", t.text3);
        } else {
            // wrap each line
            let mut y = inner.y + 18;
            let caret = self.notes.unwrap_or(usize::MAX);
            let mut idx = 0usize;
            let mut caret_at: Option<(i32, i32)> = None;
            for line in text.split('\n') {
                let chars: Vec<char> = line.chars().collect();
                let mut start = 0;
                loop {
                    // how many characters fit
                    let mut end = chars.len();
                    while end > start && ui.tw(Face::Regular, 13, &chars[start..end].iter().collect::<String>()) > inner.w {
                        let sp = chars[start..end - 1].iter().rposition(|&c| c == ' ').map(|p| start + p + 1);
                        end = match sp {
                            Some(e) if e > start => e,
                            _ => end - 1,
                        };
                    }
                    let seg: String = chars[start..end].iter().collect();
                    ui.text(inner.x, y, Face::Regular, 13, &seg, t.text);
                    if caret >= idx + start && caret <= idx + end && caret_at.is_none() && (end == chars.len() || caret < idx + end) {
                        let pre: String = chars[start..caret - idx].iter().collect();
                        caret_at = Some((inner.x + ui.tw(Face::Regular, 13, &pre), y));
                    }
                    y += 18;
                    if end >= chars.len() {
                        break;
                    }
                    start = end;
                }
                idx += chars.len() + 1;
            }
            if let Some((cx, cy)) = caret_at {
                if (ui.ticks / 50) % 2 == 0 {
                    ui.rect(Rect::new(cx, cy - 13, 1, 16), t.text);
                }
            }
        }
        ui.set_clip(old);
        if focused {
            ui.stroke(area.inset(3), 6, 1, t.accent);
        }
        ui.zone(area, Action::App(inst, C_NOTES));
    }

    fn render_overlay(&self, ui: &mut Ui, r: Rect, inst: u32) {
        let t = ui.t;
        match &self.overlay {
            Overlay::Open(files) | Overlay::Pictures(files) => {
                let pics = matches!(self.overlay, Overlay::Pictures(_));
                ui.zone(r, Action::App(inst, C_DISMISS));
                let w = (r.w - 40).min(440);
                let n = files.len().min(9) as i32;
                let m = Rect::new(r.x + (r.w - w) / 2, r.y + HEADER + 20, w, if pics { 60 } else { 96 } + n.max(1) * 40);
                ui.shadow(m, 14, 18, 6, 70);
                ui.rrect(m, 14, t.surface);
                ui.text(m.x + 20, m.y + 32, Face::Semibold, 16, if pics { "Insert a picture" } else { "Open a presentation" }, t.text);
                let top = if pics { m.y + 48 } else { m.y + 88 };
                if !pics {
                    let nr = Rect::new(m.x + 12, m.y + 46, m.w - 24, 38);
                    let na = Action::App(inst, C_NEW);
                    if ui.hot(na) {
                        ui.rrect(nr, 8, t.hover);
                    }
                    ui.icon(Icon::Plus, nr.x + 10, nr.y + 11, 16, t.accent);
                    ui.text_in(Rect::new(nr.x + 36, nr.y, nr.w - 40, nr.h), Face::Medium, 13, "New blank presentation", t.accent, 0);
                    ui.zone(nr, na);
                }
                if files.is_empty() {
                    let msg = if pics { "No pictures in Pictures, Documents, Downloads or Shared." } else { "No presentations yet in Documents, Downloads or Shared." };
                    ui.text(m.x + 20, top + 22, Face::Regular, 13, msg, t.text2);
                }
                for (k, f) in files.iter().take(9).enumerate() {
                    let row = Rect::new(m.x + 12, top + k as i32 * 40, m.w - 24, 38);
                    let a = Action::App(inst, C_FILE + k as u32);
                    if ui.hot(a) {
                        ui.rrect(row, 8, t.hover);
                    }
                    ui.icon(if pics { Icon::Image } else { Icon::Slides }, row.x + 10, row.y + 11, 16, t.text2);
                    let name = ui.fit(Face::Medium, 13, basename(f), row.w - 150);
                    ui.text(row.x + 36, row.y + 24, Face::Medium, 13, &name, t.text);
                    let d = parent(f);
                    let d = d.rsplit('/').next().unwrap_or("");
                    let dw = ui.tw(Face::Regular, 12, d);
                    ui.text(row.r() - dw - 10, row.y + 24, Face::Regular, 12, d, t.text3);
                    ui.zone(row, a);
                }
            }
            Overlay::Menu(kind, at) => {
                ui.zone(r, Action::App(inst, C_DISMISS));
                self.render_menu(ui, r, *kind, *at, inst);
            }
            Overlay::ChartData(de) => self.render_data(ui, r, de, inst),
            Overlay::Footer(dlg) => {
                ui.zone(r, Action::App(inst, C_DISMISS));
                let m = Rect::new(r.x + (r.w - 420) / 2, r.y + HEADER + 60, 420, 236);
                ui.shadow(m, 14, 18, 6, 70);
                ui.rrect(m, 14, t.surface);
                ui.zone(m, Action::App(inst, C_DLG_TEXT));
                ui.text(m.x + 20, m.y + 34, Face::Semibold, 16, "Header & footer", t.text);
                ui.text(m.x + 20, m.y + 66, Face::Medium, 13, "Footer text", t.text2);
                ui.line(Rect::new(m.x + 20, m.y + 76, m.w - 40, 34), &dlg.text, "e.g. Hydatek · Lagos", true, Action::App(inst, C_DLG_TEXT));
                ui.text(m.x + 20, m.y + 142, Face::Medium, 13, "Slide numbers", t.text);
                ui.switch(m.r() - 70, m.y + 126, dlg.numbers, Action::App(inst, C_DLG_NUMBERS));
                ui.text(m.x + 20, m.y + 172, Face::Regular, 12, "Shown on every slide except title slides.", t.text3);
                ui.button(Rect::new(m.r() - 120, m.b() - 50, 100, 34), "Apply", Action::App(inst, C_DLG_OK), true);
            }
            _ => {}
        }
    }

    fn render_menu(&self, ui: &mut Ui, win: Rect, kind: MenuKind, at: Rect, inst: u32) {
        let t = ui.t;
        let item = |k: usize| Action::App(inst, C_MENU + k as u32);
        let place = |w: i32, h: i32| {
            let x = at.x.min(win.r() - w - 8).max(win.x + 8);
            Rect::new(x, at.b() + 4, w, h)
        };
        match kind {
            MenuKind::NewSlide | MenuKind::Layout => {
                let m = place(250, 24 + LAYOUTS.len() as i32 * 52);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let cur_layout = self.slide().layout;
                for (k, l) in LAYOUTS.iter().enumerate() {
                    let row = Rect::new(m.x + 8, m.y + 12 + k as i32 * 52, m.w - 16, 50);
                    let on = kind == MenuKind::Layout && *l == cur_layout;
                    if ui.hot(item(k)) || on {
                        ui.rrect(row, 8, if on { t.accent.with_alpha(40) } else { t.hover });
                    }
                    // a little diagram of the layout
                    let d = Rect::new(row.x + 8, row.y + 7, 64, 36);
                    ui.rect(d, Color::rgb(self.deck.theme.bg));
                    ui.stroke(d, 0, 1, t.line);
                    for ph in layout_shapes(*l, 64, 36) {
                        let pr = Rect::new(d.x + ph.x, d.y + ph.y, ph.w.max(2), ph.h.max(2));
                        let c = if ph.kind == Kind::Title { Color::rgb(self.deck.theme.title) } else { Color::rgb(self.deck.theme.text).with_alpha(120) };
                        match ph.kind {
                            Kind::Title => ui.rect(Rect::new(pr.x + 2, pr.y + pr.h / 2 - 2, pr.w - 4, 4), c),
                            _ => {
                                for j in 0..3 {
                                    let ly = pr.y + 2 + j * 5;
                                    if ly + 2 < pr.b() {
                                        ui.rect(Rect::new(pr.x + 2, ly, (pr.w - 4) * (5 - j) / 5, 2), c);
                                    }
                                }
                            }
                        }
                    }
                    ui.text(row.x + 84, row.y + 30, Face::Medium, 13, l.name(), t.text);
                    ui.zone(row, item(k));
                }
            }
            MenuKind::Theme => {
                let (cols, tw, th) = (2, 150, 84);
                let m = place(cols * (tw + 10) + 14, 3 * (th + 32) + 16);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                for (k, id) in THEME_IDS.iter().enumerate() {
                    let th_ = theme(id);
                    let cell = Rect::new(m.x + 12 + (k as i32 % cols) * (tw + 10), m.y + 12 + (k as i32 / cols) * (th + 32), tw, th);
                    let on = self.deck.theme.id == *id;
                    if ui.hot(item(k)) || on {
                        ui.rrect(Rect::new(cell.x - 4, cell.y - 4, cell.w + 8, cell.h + 30), 8, if on { t.accent.with_alpha(40) } else { t.hover });
                    }
                    ui.rect(cell, Color::rgb(th_.bg));
                    let old = ui.clip_in(cell);
                    for dc in &th_.deco {
                        let (x, y, w, h) = dc.place(SLIDE_W, SLIDE_H);
                        let s = |v: i32| v * tw / SLIDE_W;
                        let dr = Rect::new(cell.x + s(x), cell.y + s(y), s(x + w) - s(x), (s(y + h) - s(y)).max(1));
                        if dc.ellipse {
                            let sc = ui.s;
                            ellipse(ui.c, dr.scale(sc), Color::rgb(dc.color));
                        } else {
                            ui.rect(dr, Color::rgb(dc.color));
                        }
                    }
                    ui.set_clip(old);
                    ui.text(cell.x + 12, cell.y + 44, Face::Semibold, 22, "Aa", Color::rgb(th_.title));
                    ui.rect(Rect::new(cell.x + 12, cell.y + 54, 50, 3), Color::rgb(th_.accent));
                    ui.text(cell.x + 12, cell.y + 70, Face::Regular, 11, "Text on slides", Color::rgb(th_.text));
                    ui.stroke(cell, 0, 1, t.line);
                    ui.text(cell.x, cell.b() + 18, Face::Medium, 13, &th_.name, t.text);
                    ui.zone(Rect::new(cell.x - 4, cell.y - 4, cell.w + 8, cell.h + 30), item(k));
                }
            }
            MenuKind::Color => {
                let kind = self.shape().map(|s| s.kind);
                // sections: (title, section number, with a "none" choice)
                let mut secs: Vec<(&str, usize, &str)> = Vec::new();
                match kind {
                    Some(Kind::Rect | Kind::Ellipse) => {
                        secs.push(("Fill", 0, "Theme accent"));
                        secs.push(("Gradient to", 1, "No gradient"));
                        secs.push(("Outline", 2, "No outline"));
                    }
                    Some(Kind::Line) => secs.push(("Line colour", 0, "Theme text colour")),
                    Some(Kind::Picture) => secs.push(("Border", 0, "No border")),
                    Some(Kind::Chart) => {}
                    Some(Kind::Table) => secs.push(("Text colour", 0, "Theme colours")),
                    Some(_) => {
                        secs.push(("Text colour", 0, "Theme text colour"));
                        secs.push(("Fill", 3, "No fill"));
                    }
                    None => {
                        secs.push(("Slide background", 0, "Theme background"));
                        secs.push(("Gradient to", 1, "No gradient"));
                    }
                }
                let widths = matches!(kind, Some(Kind::Line | Kind::Rect | Kind::Ellipse | Kind::Picture));
                let h = secs.len() as i32 * 116 + if widths { 58 } else { 0 } + if kind == Some(Kind::Line) { 58 } else { 0 } + if kind == Some(Kind::Chart) { 40 } else { 0 } + 8;
                let m = place(6 * 34 + 20, h);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let mut y = m.y + 8;
                if kind == Some(Kind::Chart) {
                    ui.text(m.x + 12, y + 26, Face::Regular, 12, "Charts take the theme's colours.", t.text2);
                    y += 40;
                }
                for (title, sec, none) in secs {
                    ui.text(m.x + 12, y + 14, Face::Semibold, 13, title, t.text);
                    for (k, c) in PALETTE.iter().enumerate() {
                        let cell = Rect::new(m.x + 12 + (k as i32 % 6) * 34, y + 24 + (k as i32 / 6) * 34, 28, 28);
                        let a = item(sec * 100 + k);
                        ui.rrect(cell, 6, Color::rgb(*c));
                        ui.stroke(cell, 6, if ui.hot(a) { 2 } else { 1 }, if ui.hot(a) { t.accent } else { t.line });
                        ui.zone(cell, a);
                    }
                    let ar = Rect::new(m.x + 8, y + 92, m.w - 16, 22);
                    let a = item(sec * 100 + 12);
                    if ui.hot(a) {
                        ui.rrect(ar, 6, t.hover);
                    }
                    ui.text(ar.x + 6, ar.y + 16, Face::Medium, 12, none, t.text2);
                    ui.zone(ar, a);
                    y += 116;
                }
                if widths {
                    ui.text(m.x + 12, y + 14, Face::Semibold, 13, "Width", t.text);
                    for (k, w) in [2, 3, 6, 10].iter().enumerate() {
                        let cell = Rect::new(m.x + 12 + k as i32 * 50, y + 24, 44, 26);
                        let a = item(400 + k);
                        let on = self.shape().map_or(false, |s| s.line_w == *w);
                        ui.rrect(cell, 6, if on { t.accent.with_alpha(40) } else if ui.hot(a) { t.hover } else { t.chip });
                        ui.rect(Rect::new(cell.x + 8, cell.y + 13 - w / 4, cell.w - 16, (w / 2).max(1)), t.text);
                        ui.zone(cell, a);
                    }
                    y += 58;
                }
                if kind == Some(Kind::Line) {
                    ui.text(m.x + 12, y + 14, Face::Semibold, 13, "Arrows", t.text);
                    let sh = self.shape().unwrap();
                    for (k, label) in ["—", "→", "←", "←→"].iter().enumerate() {
                        let cell = Rect::new(m.x + 12 + k as i32 * 50, y + 24, 44, 26);
                        let a = item(500 + k);
                        let on = (sh.tail, sh.head) == [(false, false), (true, false), (false, true), (true, true)][k];
                        ui.rrect(cell, 6, if on { t.accent.with_alpha(40) } else if ui.hot(a) { t.hover } else { t.chip });
                        ui.text_in(cell, Face::Semibold, 14, label, t.text, 1);
                        ui.zone(cell, a);
                    }
                }
            }
            MenuKind::Shapes => {
                let cols = 5;
                let n = GEOMS.len() + 3;
                let rows = (n as i32 + cols - 1) / cols;
                let m = place(cols * 46 + 20, rows * 46 + 44);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let hot = (0..n).find(|&k| ui.hot(item(k)));
                let label = match hot {
                    Some(k) if k < GEOMS.len() => GEOMS[k].name(),
                    Some(k) if k == GEOMS.len() => "Ellipse",
                    Some(k) if k == GEOMS.len() + 1 => "Line",
                    Some(_) => "Arrow",
                    None => "Shapes, lines and arrows",
                };
                ui.text(m.x + 12, m.y + 24, Face::Semibold, 13, label, t.text);
                let sc = ui.s;
                for k in 0..n {
                    let cell = Rect::new(m.x + 10 + (k as i32 % cols) * 46, m.y + 34 + (k as i32 / cols) * 46, 42, 42);
                    let a = item(k);
                    if ui.hot(a) {
                        ui.rrect(cell, 8, t.hover);
                    }
                    let ink = t.text.0 & 0xFFFFFF;
                    let inner = Rect::new(cell.x + 9, cell.y + 11, 24, 20).scale(sc);
                    if k < GEOMS.len() + 1 {
                        let (kind, geom) = if k < GEOMS.len() { (Kind::Rect, GEOMS[k]) } else { (Kind::Ellipse, Geom::Rect) };
                        let pts: Vec<(i32, i32)> = outline(kind, geom, inner.w, inner.h).into_iter().map(|(x, y)| (inner.x * 16 + x, inner.y * 16 + y)).collect();
                        slidedraw::fill_poly(ui.c, &pts, &slidedraw::Paint::Solid(ink), 255);
                    } else {
                        let (a0, b0) = ((inner.x * 16, inner.b() * 16), (inner.r() * 16, inner.y * 16));
                        slidedraw::fill_poly(ui.c, &slidedraw::segment(a0, b0, 2 * sc * 16), &slidedraw::Paint::Solid(ink), 255);
                        if k == GEOMS.len() + 2 {
                            let h = 7 * sc * 16;
                            slidedraw::fill_poly(ui.c, &[b0, (b0.0 - h, b0.1 + h / 4), (b0.0 - h / 4, b0.1 + h)], &slidedraw::Paint::Solid(ink), 255);
                        }
                    }
                    ui.zone(cell, a);
                }
            }
            MenuKind::Table => {
                let (cols, rows) = (8, 6);
                let m = place(cols * 26 + 24, rows * 26 + 50);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let hot = (0..cols * rows).find(|&k| ui.hot(item(k as usize)));
                let label = match hot {
                    Some(k) => format!("{} × {} table", k / cols + 1, k % cols + 1),
                    None => String::from("Insert a table"),
                };
                ui.text(m.x + 12, m.y + 24, Face::Semibold, 13, &label, t.text);
                for k in 0..cols * rows {
                    let (r, c) = (k / cols, k % cols);
                    let cell = Rect::new(m.x + 12 + c * 26, m.y + 36 + r * 26, 22, 22);
                    let lit = hot.map_or(false, |h| r <= h / cols && c <= h % cols);
                    ui.rrect(cell, 4, if lit { t.accent.with_alpha(90) } else { t.chip });
                    ui.zone(cell, item(k as usize));
                }
            }
            MenuKind::Chart => {
                let m = place(200, 20 + CHART_KINDS.len() as i32 * 36);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                for (k, ck) in CHART_KINDS.iter().enumerate() {
                    let row = Rect::new(m.x + 8, m.y + 10 + k as i32 * 36, m.w - 16, 34);
                    if ui.hot(item(k)) {
                        ui.rrect(row, 8, t.hover);
                    }
                    ui.icon(Icon::Chart, row.x + 8, row.y + 9, 16, t.accent);
                    ui.text(row.x + 34, row.y + 22, Face::Medium, 13, &format!("{} chart", ck.name()), t.text);
                    ui.zone(row, item(k));
                }
            }
            MenuKind::TableEdit | MenuKind::ChartEdit | MenuKind::Anim => {
                let items = self.list_menu(kind);
                let m = place(230, 20 + items.len() as i32 * 34);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                for (k, (name, on)) in items.iter().enumerate() {
                    let row = Rect::new(m.x + 8, m.y + 10 + k as i32 * 34, m.w - 16, 32);
                    if name.is_empty() {
                        ui.rect(Rect::new(row.x, row.y + 16, row.w, 1), t.line);
                        continue;
                    }
                    if ui.hot(item(k)) {
                        ui.rrect(row, 8, t.hover);
                    }
                    if *on {
                        ui.icon(Icon::Check, row.x + 8, row.y + 9, 14, t.accent);
                    }
                    ui.text(row.x + 30, row.y + 21, Face::Medium, 13, name, t.text);
                    ui.zone(row, item(k));
                }
            }
            MenuKind::Trans => {
                let items = ["None", "Fade", "Push up", "Apply to all slides"];
                let m = place(210, 20 + items.len() as i32 * 36);
                ui.shadow(m, 12, 16, 6, 60);
                ui.rrect(m, 12, t.surface);
                let cur = self.slide().trans;
                for (k, name) in items.iter().enumerate() {
                    let row = Rect::new(m.x + 8, m.y + 10 + k as i32 * 36, m.w - 16, 34);
                    let on = matches!((k, cur), (0, Trans::None) | (1, Trans::Fade) | (2, Trans::Push));
                    if ui.hot(item(k)) {
                        ui.rrect(row, 8, t.hover);
                    }
                    if on {
                        ui.icon(Icon::Check, row.x + 8, row.y + 9, 14, t.accent);
                    }
                    if k == 3 {
                        ui.rect(Rect::new(row.x, row.y - 1, row.w, 1), t.line);
                    }
                    ui.text(row.x + 30, row.y + 22, Face::Medium, 13, name, t.text);
                    ui.zone(row, item(k));
                }
            }
        }
    }

    fn menu_pick(&mut self, kind: MenuKind, k: usize) {
        match kind {
            MenuKind::NewSlide => {
                if let Some(l) = LAYOUTS.get(k) {
                    self.add_slide(*l);
                }
            }
            MenuKind::Layout => {
                if let Some(l) = LAYOUTS.get(k) {
                    self.stop_edit();
                    self.snapshot();
                    let mut s = self.slide().clone();
                    self.deck.relayout(&mut s, *l);
                    *self.slide_mut() = s;
                    self.sel = None;
                    self.touch();
                }
            }
            MenuKind::Theme => {
                if let Some(id) = THEME_IDS.get(k) {
                    self.snapshot();
                    self.deck.theme = theme(id);
                    self.touch_all();
                }
            }
            MenuKind::Color => self.pick_color(k / 100, k % 100),
            MenuKind::Shapes => {
                if k < GEOMS.len() {
                    self.add_shape(Kind::Rect);
                    let g = GEOMS[k];
                    if let Some(i) = self.sel {
                        let sh = &mut self.slide_mut().shapes[i];
                        sh.geom = g;
                        if matches!(g, Geom::RightArrow | Geom::LeftArrow | Geom::Chevron | Geom::Parallelogram) {
                            sh.h = 140;
                        }
                    }
                } else if k == GEOMS.len() {
                    self.add_shape(Kind::Ellipse);
                } else {
                    self.add_shape(Kind::Line);
                    let arrow = k == GEOMS.len() + 2;
                    if let Some(i) = self.sel {
                        self.slide_mut().shapes[i].tail = arrow;
                    }
                }
                self.touch();
            }
            MenuKind::Table => self.insert_table(k / 8 + 1, k % 8 + 1),
            MenuKind::Chart => {
                if let Some(ck) = CHART_KINDS.get(k) {
                    self.add_shape(Kind::Chart);
                    if let Some(i) = self.sel {
                        self.slide_mut().shapes[i].chart = Some(Chart::sample(*ck));
                        self.touch();
                        self.open_data(i);
                    }
                }
            }
            MenuKind::TableEdit => self.table_op(k),
            MenuKind::ChartEdit => self.chart_op(k),
            MenuKind::Anim => self.anim_op(k),
            MenuKind::Trans => {
                self.snapshot();
                if k == 3 {
                    let tr = self.slide().trans;
                    for s in self.deck.slides.iter_mut() {
                        s.trans = tr;
                    }
                    self.touch_all();
                } else {
                    self.slide_mut().trans = [Trans::None, Trans::Fade, Trans::Push][k.min(2)];
                    self.touch();
                }
            }
        }
    }

    fn open_menu(&mut self, kind: MenuKind, code: u32) {
        if matches!(self.overlay, Overlay::Menu(k, _) if k == kind) {
            self.overlay = Overlay::None;
            return;
        }
        // anchor under the toolbar button (found from the pointer)
        let (mx, _) = self.mouse;
        let _ = code;
        let at = Rect::new(mx - 20, self.canvas.y.min(self.mouse.1 + 16) - 6, 40, 1);
        self.overlay = Overlay::Menu(kind, at);
    }

    // ---- canvas clicks -------------------------------------------------------------

    /// The handle under (x, y): 0-7 sizing, 8 turning, 10 / 11 a line's ends.
    fn handle_at(&self, x: i32, y: i32) -> Option<u8> {
        let sh = self.shape()?;
        if self.ed.is_some() {
            return None;
        }
        let near = |(hx, hy): (i32, i32)| (x - hx).abs() <= 7 && (y - hy).abs() <= 7;
        if sh.kind == Kind::Line {
            return self.line_handles(sh).iter().position(|&p| near(p)).map(|i| 10 + i as u8);
        }
        let (hs, rh) = self.handle_points(sh);
        if near(rh) {
            return Some(8);
        }
        hs.iter().position(|&p| near(p)).map(|i| i as u8)
    }

    fn shape_at(&self, ux: i32, uy: i32) -> Option<usize> {
        let slide = self.slide();
        slide.shapes.iter().enumerate().rev().find(|(_, s)| s.contains(ux, uy)).map(|(i, _)| i)
    }

    /// The table cell at a point (slide units) of table shape `i`.
    fn cell_at(&self, i: usize, ux: i32, uy: i32) -> Option<(usize, usize)> {
        let sh = self.slide().shapes.get(i)?;
        let t = sh.table.as_ref()?;
        let (x, y) = sh.to_local(ux, uy);
        let (mut cx, mut cy) = (sh.x, sh.y);
        let c = t.cols.iter().position(|w| {
            cx += w;
            x < cx
        })?;
        let r = t.rows.iter().position(|h| {
            cy += h;
            y < cy
        })?;
        Some((r, c))
    }

    fn canvas_click(&mut self, double: bool) {
        let (mx, my) = self.mouse;
        if let Some(h) = self.handle_at(mx, my) {
            let sh = self.shape().unwrap().clone();
            self.press = Some(match h {
                8 => Press::Rotate { centre: (self.canvas.x + self.to_screen(sh.x + sh.w / 2), self.canvas.y + self.to_screen(sh.y + sh.h / 2)), orig: sh.rot, moved: false },
                10 | 11 => Press::LineEnd { end: h - 10, moved: false },
                _ => Press::Resize { handle: h, start: (mx, my), orig: (sh.x, sh.y, sh.w, sh.h), moved: false },
            });
            return;
        }
        let (ux, uy) = self.to_units(mx, my);
        // in the text being edited: place the caret
        if let (Some(ed), Some(mut sh)) = (self.ed, self.text_target()) {
            sh.rot = 0;
            if sh.contains(ux, uy) {
                let tb = layout(&sh);
                let p = hit(&sh, &tb, ux - sh.x, uy - sh.y);
                let e = self.ed.as_mut().unwrap();
                if double {
                    let (a, b) = sh.text.word_at(p);
                    e.anchor = Some(a);
                    e.caret = b;
                } else if crate::input::shift() {
                    if e.anchor.is_none() {
                        e.anchor = Some(e.caret);
                    }
                    e.caret = p;
                } else {
                    e.anchor = None;
                    e.caret = p;
                }
                e.typing = false;
                self.press = Some(Press::Text);
                return;
            }
            // another cell of the same table
            if ed.cell.is_some() {
                if let Some((r, c)) = self.cell_at(ed.shape, ux, uy) {
                    let cs = self.slide().shapes[ed.shape].cell_shape(r, c).unwrap();
                    let p = hit(&cs, &layout(&cs), ux - cs.x, uy - cs.y);
                    self.start_cell_edit(ed.shape, r, c, Some(p));
                    self.press = Some(Press::Text);
                    return;
                }
            }
            self.stop_edit();
        }
        self.notes = None;
        match self.shape_at(ux, uy) {
            Some(i) => {
                let sh = self.slide().shapes[i].clone();
                if sh.kind == Kind::Table && (self.sel == Some(i) || double) {
                    if let Some((r, c)) = self.cell_at(i, ux, uy) {
                        let cs = sh.cell_shape(r, c).unwrap();
                        let p = hit(&cs, &layout(&cs), ux - cs.x, uy - cs.y);
                        self.start_cell_edit(i, r, c, Some(p));
                        self.press = Some(Press::Text);
                        return;
                    }
                }
                if sh.kind == Kind::Chart && double {
                    self.sel = Some(i);
                    self.open_data(i);
                    return;
                }
                let enter_text = sh.kind.has_text() && (double || (self.sel == Some(i) && sh.kind != Kind::Rect && sh.kind != Kind::Ellipse) || (sh.kind.placeholder() && sh.is_empty()));
                if enter_text && (self.sel == Some(i) || double || sh.is_empty()) {
                    let mut st = sh.clone();
                    st.rot = 0;
                    let (lx, ly) = sh.to_local(ux, uy);
                    let tb = layout(&st);
                    let p = hit(&st, &tb, lx - sh.x, ly - sh.y);
                    self.start_edit(i, Some(p));
                    self.press = Some(Press::Text);
                    return;
                }
                self.sel = Some(i);
                self.press = Some(Press::Move { start: (mx, my), orig: (sh.x, sh.y), moved: false });
            }
            None => {
                self.sel = None;
            }
        }
    }

    fn notes_key(&mut self, k: Key, gen: bool, sys: &mut Sys) {
        let Some(mut c) = self.notes else { return };
        let mut text: Vec<char> = self.slide().notes.chars().collect();
        c = c.min(text.len());
        let mut changed = false;
        match k {
            Key::Char(ch) if gen && ch.to_ascii_lowercase() == 'v' => {
                let p: Vec<char> = sys.clipboard.chars().filter(|c| *c != '\r').collect();
                for (j, ch) in p.iter().enumerate() {
                    text.insert(c + j, *ch);
                }
                c += p.len();
                changed = true;
            }
            Key::Char(_) if gen => {}
            Key::Char(ch) if !ch.is_control() => {
                text.insert(c, ch);
                c += 1;
                changed = true;
            }
            Key::Enter => {
                text.insert(c, '\n');
                c += 1;
                changed = true;
            }
            Key::Backspace if c > 0 => {
                text.remove(c - 1);
                c -= 1;
                changed = true;
            }
            Key::Delete if c < text.len() => {
                text.remove(c);
                changed = true;
            }
            Key::Left => c = c.saturating_sub(1),
            Key::Right => c = (c + 1).min(text.len()),
            Key::Home => c = text[..c].iter().rposition(|&x| x == '\n').map(|p| p + 1).unwrap_or(0),
            Key::End => c = text[c..].iter().position(|&x| x == '\n').map(|p| c + p).unwrap_or(text.len()),
            Key::Esc => {
                self.notes = None;
                return;
            }
            _ => {}
        }
        if changed {
            // one undo step per run of typing
            if !self.notes_typing {
                self.snapshot();
                self.notes_typing = true;
            }
            self.slide_mut().notes = text.into_iter().collect();
            self.dirty = true;
        } else {
            self.notes_typing = false;
        }
        self.notes = Some(c);
    }
}

fn clone_canvas(c: &Canvas) -> Canvas {
    let mut n = Canvas::new(c.w, c.h);
    n.px.copy_from_slice(&c.px);
    n
}

/// Draw `a` blended into `b` by `t`/256 at (x, y).
fn blend(dst: &mut Canvas, a: &Canvas, b: &Canvas, x: i32, y: i32, t: u32) {
    let clip = dst.clip;
    for yy in 0..a.h.min(b.h) {
        let py = y + yy;
        if py < clip.y || py >= clip.b() {
            continue;
        }
        for xx in 0..a.w.min(b.w) {
            let px = x + xx;
            if px < clip.x || px >= clip.r() {
                continue;
            }
            let i = (yy * a.w + xx) as usize;
            dst.px[(py * dst.w + px) as usize] = mixp(a.px[i], b.px[i], t);
        }
    }
}

impl Slides {
    // ---- colours, tables, charts, animations ---------------------------------------

    /// Tab / Shift+Tab between table cells; Tab in the last cell adds a row.
    fn next_cell(&mut self, forward: bool) {
        let Some(TextEd { shape, cell: Some((r, c)), .. }) = self.ed else { return };
        let Some(t) = self.slide().shapes.get(shape).and_then(|s| s.table.clone()) else { return };
        let (nr, nc) = (t.nrows(), t.ncols());
        let idx = r * nc + c;
        let next = if forward {
            if idx + 1 >= nr * nc {
                self.touch();
                self.snapshot();
                let cur = self.cur;
                let sh = &mut self.deck.slides[cur].shapes[shape];
                if let Some(t) = sh.table.as_mut() {
                    t.insert_row(nr);
                }
                sh.fit_table();
            }
            idx + 1
        } else {
            match idx.checked_sub(1) {
                Some(i) => i,
                None => return,
            }
        };
        self.start_cell_edit(shape, next / nc, next % nc, None);
        if let Some(ed) = self.ed.as_mut() {
            // select the cell's text, as other programs do
            let end = self.cell_sh.as_ref().map(|c| c.text.end()).unwrap_or_default();
            ed.anchor = Some(Pos::new(0, 0));
            ed.caret = end;
        }
    }

    fn pick_color(&mut self, sec: usize, k: usize) {
        let col = PALETTE.get(k).copied();
        self.snapshot();
        let c = self.cur;
        match self.sel {
            None => {
                let s = &mut self.deck.slides[c];
                match sec {
                    0 => s.bg = col,
                    _ => s.bg_grad = col.map(|to| (to, 90)),
                }
            }
            Some(i) => {
                let theme_accent = self.deck.theme.accent;
                let sh = &mut self.deck.slides[c].shapes[i];
                match (sec, sh.kind) {
                    (0, Kind::Rect | Kind::Ellipse) => sh.fill = col,
                    (0, Kind::Line | Kind::Picture) => sh.line = col,
                    (0, _) => sh.color = col,
                    (1, _) => {
                        if sh.fill.is_none() {
                            sh.fill = Some(theme_accent);
                        }
                        sh.grad = col.map(|to| (to, 90));
                    }
                    (2, _) => sh.line = col,
                    (3, _) => sh.fill = col,
                    (4, _) => sh.line_w = [2, 3, 6, 10][k.min(3)],
                    (5, _) => {
                        let (tail, head) = [(false, false), (true, false), (false, true), (true, true)][k.min(3)];
                        sh.tail = tail;
                        sh.head = head;
                    }
                    _ => {}
                }
                if sec == 4 && sh.kind != Kind::Line && sh.line.is_none() {
                    sh.line = Some(0x1E1B2C);
                }
            }
        }
        self.touch();
    }

    fn insert_table(&mut self, rows: usize, cols: usize) {
        self.stop_edit();
        self.snapshot();
        let w = (self.deck.w * 3 / 4).min(cols as i32 * 220);
        let mut sh = Shape::new(Kind::Table, (self.deck.w - w) / 2, 180, w, rows as i32 * 50);
        sh.table = Some(Table::new(rows, cols, w, rows as i32 * 50));
        sh.fit_table();
        self.slide_mut().shapes.push(sh);
        let i = self.slide().shapes.len() - 1;
        self.sel = Some(i);
        self.touch();
        self.start_cell_edit(i, 0, 0, None);
    }

    /// The cell a table command applies to: the one being typed in, else the last.
    fn table_cell(&self, i: usize) -> (usize, usize) {
        match self.ed {
            Some(TextEd { shape, cell: Some(rc), .. }) if shape == i => rc,
            _ => self.slide().shapes[i].table.as_ref().map(|t| (t.nrows() - 1, t.ncols() - 1)).unwrap_or((0, 0)),
        }
    }

    fn list_menu(&self, kind: MenuKind) -> Vec<(&'static str, bool)> {
        let sh = self.shape();
        match kind {
            MenuKind::TableEdit => {
                let t = sh.and_then(|s| s.table.as_ref());
                vec![
                    ("Insert row above", false),
                    ("Insert row below", false),
                    ("Insert column left", false),
                    ("Insert column right", false),
                    ("", false),
                    ("Delete row", false),
                    ("Delete column", false),
                    ("", false),
                    ("Heading row", t.map_or(false, |t| t.header)),
                    ("Banded rows", t.map_or(false, |t| t.banded)),
                ]
            }
            MenuKind::ChartEdit => {
                let c = sh.and_then(|s| s.chart.as_ref());
                let mut v = vec![("Edit data…", false), ("", false)];
                for (k, name) in ["Column", "Bar", "Line", "Area", "Pie"].iter().enumerate() {
                    v.push((*name, c.map_or(false, |c| c.kind == CHART_KINDS[k])));
                }
                v.push(("", false));
                v.push(("Legend", c.map_or(false, |c| c.legend)));
                v
            }
            _ => {
                let a = sh.map(|s| s.anim).unwrap_or(Anim::None);
                vec![("No animation", a == Anim::None), ("Appear", a == Anim::Appear), ("Fade in", a == Anim::Fade), ("Fly in from below", a == Anim::Fly), ("", false), ("Play earlier", false), ("Play later", false)]
            }
        }
    }

    fn table_op(&mut self, k: usize) {
        let Some(i) = self.sel else { return };
        let (r, c) = self.table_cell(i);
        self.stop_edit();
        self.snapshot();
        let cur = self.cur;
        let sh = &mut self.deck.slides[cur].shapes[i];
        let Some(t) = sh.table.as_mut() else { return };
        match k {
            0 => t.insert_row(r),
            1 => t.insert_row(r + 1),
            2 => t.insert_col(c),
            3 => t.insert_col(c + 1),
            5 => t.delete_row(r),
            6 => t.delete_col(c),
            8 => t.header = !t.header,
            9 => t.banded = !t.banded,
            _ => {}
        }
        sh.fit_table();
        self.touch();
    }

    fn chart_op(&mut self, k: usize) {
        let Some(i) = self.sel else { return };
        match k {
            0 => self.open_data(i),
            2..=6 => {
                self.snapshot();
                let cur = self.cur;
                if let Some(c) = self.deck.slides[cur].shapes[i].chart.as_mut() {
                    c.kind = CHART_KINDS[k - 2];
                }
                self.touch();
            }
            8 => {
                self.snapshot();
                let cur = self.cur;
                if let Some(c) = self.deck.slides[cur].shapes[i].chart.as_mut() {
                    c.legend = !c.legend;
                }
                self.touch();
            }
            _ => {}
        }
    }

    fn anim_op(&mut self, k: usize) {
        let Some(i) = self.sel else { return };
        self.snapshot();
        let steps = anim_steps(self.slide());
        let next = self.slide().shapes.iter().map(|s| s.anim_order).max().unwrap_or(0) + 1;
        let cur = self.cur;
        let shapes = &mut self.deck.slides[cur].shapes;
        match k {
            0..=3 => {
                let a = [Anim::None, Anim::Appear, Anim::Fade, Anim::Fly][k];
                if shapes[i].anim == Anim::None && a != Anim::None {
                    shapes[i].anim_order = next;
                }
                shapes[i].anim = a;
            }
            5 | 6 => {
                // swap places with the neighbour in the play order
                if let Some(pos) = steps.iter().position(|&s| s == i) {
                    let other = if k == 5 { pos.checked_sub(1) } else { Some(pos + 1).filter(|&p| p < steps.len()) };
                    if let Some(o) = other {
                        // renumber everything in order, then swap the two
                        for (n, &s) in steps.iter().enumerate() {
                            shapes[s].anim_order = n as u16 + 1;
                        }
                        let j = steps[o];
                        let (a, b) = (shapes[i].anim_order, shapes[j].anim_order);
                        shapes[i].anim_order = b;
                        shapes[j].anim_order = a;
                    }
                }
            }
            _ => {}
        }
        self.touch();
    }

    fn rotate_by(&mut self, deg: i32) {
        let Some(i) = self.sel else { return };
        self.stop_edit();
        self.snapshot();
        let sh = &mut self.slide_mut().shapes[i];
        if sh.kind == Kind::Line {
            let ((x0, y0), (x1, y1)) = line_ends(sh);
            let (cx, cy) = (sh.x + sh.w / 2, sh.y + sh.h / 2);
            set_line_ends(sh, rotate(x0, y0, cx, cy, deg), rotate(x1, y1, cx, cy, deg));
        } else {
            sh.rot = (sh.rot + deg).rem_euclid(360);
        }
        self.touch();
    }

    fn flip(&mut self, horizontal: bool) {
        let Some(i) = self.sel else { return };
        self.snapshot();
        let sh = &mut self.slide_mut().shapes[i];
        if horizontal {
            sh.flip_h = !sh.flip_h;
        } else {
            sh.flip_v = !sh.flip_v;
        }
        self.touch();
    }

    // ---- the chart's data sheet -------------------------------------------------------

    fn open_data(&mut self, i: usize) {
        self.stop_edit();
        if self.slide().shapes.get(i).and_then(|s| s.chart.as_ref()).is_none() {
            return;
        }
        self.snapshot();
        self.overlay = Overlay::ChartData(DataEd { shape: i, cur: (1, 1), text: None, title: false });
    }

    fn chart_mut(&mut self, i: usize) -> Option<&mut Chart> {
        let c = self.cur;
        self.deck.slides[c].shapes.get_mut(i)?.chart.as_mut()
    }

    /// What a data cell shows.
    fn data_cell(ch: &Chart, r: usize, c: usize) -> String {
        match (r, c) {
            (0, 0) => String::new(),
            (0, c) => ch.series.get(c - 1).map(|s| s.name.clone()).unwrap_or_default(),
            (r, 0) => ch.cats.get(r - 1).cloned().unwrap_or_default(),
            (r, c) => ch.series.get(c - 1).and_then(|s| s.vals.get(r - 1)).map(|v| fmt_num(*v)).unwrap_or_default(),
        }
    }

    /// Put the typed text into the chart.
    fn data_commit(&mut self) {
        let Overlay::ChartData(de) = &mut self.overlay else { return };
        let Some(text) = de.text.take() else { return };
        let (i, (r, c), title) = (de.shape, de.cur, de.title);
        de.title = false;
        let Some(ch) = self.chart_mut(i) else { return };
        if title {
            ch.title = text;
        } else {
            match (r, c) {
                (0, 0) => {}
                (0, c) => {
                    if let Some(s) = ch.series.get_mut(c - 1) {
                        s.name = text;
                    }
                }
                (r, 0) => {
                    if let Some(x) = ch.cats.get_mut(r - 1) {
                        *x = text;
                    }
                }
                (r, c) => {
                    if let Some(v) = ch.series.get_mut(c - 1).and_then(|s| s.vals.get_mut(r - 1)) {
                        *v = parse_num(&text).unwrap_or(*v);
                    }
                }
            }
        }
        self.touch();
    }

    fn data_move(&mut self, dr: i32, dc: i32) {
        self.data_commit();
        let Overlay::ChartData(de) = &self.overlay else { return };
        let i = de.shape;
        let Some(ch) = self.slide().shapes.get(i).and_then(|s| s.chart.as_ref()) else { return };
        let (rows, cols) = (ch.cats.len() + 1, ch.series.len() + 1);
        if let Overlay::ChartData(de) = &mut self.overlay {
            de.cur = ((de.cur.0 as i32 + dr).clamp(0, rows as i32 - 1) as usize, (de.cur.1 as i32 + dc).clamp(0, cols as i32 - 1) as usize);
        }
    }

    fn data_action(&mut self, code: u32) {
        let Overlay::ChartData(de) = &self.overlay else { return };
        let i = de.shape;
        if code != C_DATA_TITLE {
            self.data_commit();
        }
        match code {
            C_DATA_DONE => {
                self.overlay = Overlay::None;
                return;
            }
            C_DATA_TITLE => {
                self.data_commit();
                let t = self.slide().shapes.get(i).and_then(|s| s.chart.as_ref()).map(|c| c.title.clone()).unwrap_or_default();
                if let Overlay::ChartData(de) = &mut self.overlay {
                    de.title = true;
                    de.text = Some(t);
                }
                return;
            }
            c if c >= C_DATA_CELL => {
                let (r, col) = (((c - C_DATA_CELL) / 100) as usize, ((c - C_DATA_CELL) % 100) as usize);
                if let Overlay::ChartData(de) = &mut self.overlay {
                    de.cur = (r, col);
                }
                return;
            }
            _ => {}
        }
        let Some(ch) = self.chart_mut(i) else { return };
        match code {
            C_DATA_ADD_ROW => {
                ch.cats.push(format!("Item {}", ch.cats.len() + 1));
                for s in ch.series.iter_mut() {
                    s.vals.push(0.0);
                }
            }
            C_DATA_DEL_ROW => {
                if ch.cats.len() > 1 {
                    ch.cats.pop();
                    for s in ch.series.iter_mut() {
                        s.vals.pop();
                    }
                }
            }
            C_DATA_ADD_SER => {
                let n = ch.cats.len();
                ch.series.push(Series { name: format!("Series {}", ch.series.len() + 1), vals: vec![0.0; n] });
            }
            C_DATA_DEL_SER => {
                if ch.series.len() > 1 {
                    ch.series.pop();
                }
            }
            C_DATA_LEGEND => ch.legend = !ch.legend,
            c if (C_DATA_KIND..C_DATA_KIND + 5).contains(&c) => ch.kind = CHART_KINDS[(c - C_DATA_KIND) as usize],
            _ => {}
        }
        self.data_move(0, 0);
        self.touch();
    }

    fn data_key(&mut self, k: Key, gen: bool, sys: &mut Sys) {
        let shift = crate::input::shift();
        let Overlay::ChartData(de) = &mut self.overlay else { return };
        match k {
            Key::Esc | Key::F(5) => {
                self.data_commit();
                self.overlay = Overlay::None;
            }
            Key::Char(c) if gen && c.to_ascii_lowercase() == 'v' => {
                let t = sys.clipboard.lines().next().unwrap_or("").to_string();
                de.text.get_or_insert_with(String::new).push_str(&t);
            }
            Key::Char(c) if !gen && !c.is_control() => {
                de.text.get_or_insert_with(String::new).push(c);
            }
            Key::Backspace => {
                if de.text.is_none() && !de.title {
                    de.text = Some(String::new());
                } else if let Some(t) = de.text.as_mut() {
                    t.pop();
                }
            }
            Key::Enter => {
                if de.title {
                    self.data_commit();
                } else {
                    self.data_move(if shift { -1 } else { 1 }, 0);
                }
            }
            Key::Tab => self.data_move(0, if shift { -1 } else { 1 }),
            Key::Up => self.data_move(-1, 0),
            Key::Down => self.data_move(1, 0),
            Key::Left if de.text.is_none() => self.data_move(0, -1),
            Key::Right if de.text.is_none() => self.data_move(0, 1),
            Key::Delete => {
                de.text = Some(String::new());
                self.data_commit();
            }
            _ => {}
        }
    }

    fn render_data(&self, ui: &mut Ui, r: Rect, de: &DataEd, inst: u32) {
        let t = ui.t;
        let Some(ch) = self.slide().shapes.get(de.shape).and_then(|s| s.chart.as_ref()) else { return };
        ui.zone(r, Action::App(inst, C_DISMISS));
        let (rows, cols) = (ch.cats.len() + 1, ch.series.len() + 1);
        let (cw0, cw, rh) = (150, 104, 30);
        let gw = cw0 + cw * (cols as i32 - 1);
        let w = (gw + 40).max(600).min(r.w - 20);
        let vis_rows = rows.min(9) as i32;
        let h = 170 + vis_rows * rh;
        let m = Rect::new(r.x + (r.w - w) / 2, r.y + HEADER + 30, w, h);
        ui.shadow(m, 14, 18, 6, 70);
        ui.rrect(m, 14, t.surface);
        ui.zone(m, Action::App(inst, C_DLG_TEXT));
        ui.text(m.x + 20, m.y + 32, Face::Semibold, 16, "Chart data", t.text);
        // chart kinds
        let mut x = m.x + 130;
        for (k, ck) in CHART_KINDS.iter().enumerate() {
            let a = Action::App(inst, C_DATA_KIND + k as u32);
            let bw = ui.tw(Face::Medium, 12, ck.name()) + 18;
            let b = Rect::new(x, m.y + 14, bw, 26);
            ui.rrect(b, 8, if ch.kind == *ck { t.accent.with_alpha(45) } else if ui.hot(a) { t.hover } else { t.chip });
            ui.text_in(b, Face::Medium, 12, ck.name(), if ch.kind == *ck { t.accent } else { t.text }, 1);
            ui.zone(b, a);
            x += bw + 6;
        }
        // title
        let tr = Rect::new(m.x + 20, m.y + 50, m.w - 40, 30);
        let title = if de.title { de.text.clone().unwrap_or_default() } else { ch.title.clone() };
        ui.field(tr, &title, "Chart title (optional)", de.title, Action::App(inst, C_DATA_TITLE));
        // the grid
        let gx = m.x + 20;
        let gy = m.y + 92;
        let old = ui.clip_in(Rect::new(gx, gy, m.w - 40, vis_rows * rh + 1));
        let first = de.cur.0.saturating_sub(vis_rows as usize - 1);
        for (vr, row) in (0..rows).skip(if de.cur.0 >= vis_rows as usize { first } else { 0 }).take(vis_rows as usize).enumerate() {
            let row_ix = if de.cur.0 >= vis_rows as usize { row } else { vr };
            let y = gy + vr as i32 * rh;
            for col in 0..cols {
                let cx = gx + if col == 0 { 0 } else { cw0 + cw * (col as i32 - 1) };
                let wcol = if col == 0 { cw0 } else { cw };
                let cell = Rect::new(cx, y, wcol, rh);
                let header = row_ix == 0 || col == 0;
                ui.rect(cell, if header { t.chip } else { Color::rgb(0xFFFFFF) });
                ui.stroke(cell, 0, 1, t.line);
                let on = de.cur == (row_ix, col) && !de.title;
                let text = if on { de.text.clone().unwrap_or_else(|| Self::data_cell(ch, row_ix, col)) } else { Self::data_cell(ch, row_ix, col) };
                let ink = if header { t.text } else { Color::rgb(0x1E1B2C) };
                let face = if header { Face::Semibold } else { Face::Regular };
                let label = ui.fit(face, 13, &text, wcol - 14);
                if col == 0 || row_ix == 0 {
                    ui.text(cell.x + 8, cell.y + 20, face, 13, &label, ink);
                } else {
                    let tw = ui.tw(face, 13, &label);
                    ui.text(cell.r() - tw - 8, cell.y + 20, face, 13, &label, ink);
                }
                if on {
                    ui.stroke(Rect::new(cell.x, cell.y, cell.w + 1, cell.h + 1), 0, 2, t.accent);
                    if de.text.is_some() && (ui.ticks / 50) % 2 == 0 {
                        let tw = ui.tw(face, 13, &label);
                        let cx = if col == 0 || row_ix == 0 { cell.x + 8 + tw } else { cell.r() - 8 };
                        ui.rect(Rect::new(cx, cell.y + 7, 1, 16), t.accent);
                    }
                }
                if (row_ix, col) != (0, 0) {
                    ui.zone(cell, Action::App(inst, C_DATA_CELL + row_ix as u32 * 100 + col as u32));
                }
            }
        }
        ui.set_clip(old);
        // buttons
        let by = m.b() - 52;
        let mut x = m.x + 20;
        for (label, code) in [("+ Row", C_DATA_ADD_ROW), ("– Row", C_DATA_DEL_ROW), ("+ Series", C_DATA_ADD_SER), ("– Series", C_DATA_DEL_SER)] {
            let bw = ui.tw(Face::Medium, 12, label) + 20;
            let b = Rect::new(x, by + 6, bw, 28);
            let a = Action::App(inst, code);
            ui.rrect(b, 8, if ui.hot(a) { t.hover } else { t.chip });
            ui.text_in(b, Face::Medium, 12, label, t.text, 1);
            ui.zone(b, a);
            x += bw + 6;
        }
        ui.text(x + 8, by + 25, Face::Medium, 12, "Legend", t.text);
        ui.switch(x + 62, by + 8, ch.legend, Action::App(inst, C_DATA_LEGEND));
        ui.button(Rect::new(m.r() - 110, by + 2, 90, 34), "Done", Action::App(inst, C_DATA_DONE), true);
        ui.text(m.x + 20, m.b() - 60, Face::Regular, 11, "Type to change a cell · Enter / Tab move · Esc done", t.text3);
    }

    // ---- PDF -------------------------------------------------------------------------------

    /// Export the slides (or notes pages) as a PDF.
    fn export_pdf(&mut self, sys: &mut Sys, notes: bool) {
        self.stop_edit();
        let dir = if self.path.is_empty() { String::from("/home/Documents/Presentations") } else { parent(&self.path) };
        let name = if notes { format!("{} (notes)", self.title()) } else { self.title() };
        let target = join(&dir, &format!("{}.pdf", name));
        let path = if sys.fs.exists(&target) { sys.fs.unique(&dir, &name, ".pdf") } else { target };
        if self.deck.author.is_empty() {
            self.deck.author = sys.profile.name.clone();
        }
        let data = self.pdf_bytes(notes);
        let ok = sys.fs.write(&path, &data);
        sys.toast("Hyda Slides", &if ok { format!("Exported {}", basename(&path)) } else { String::from("Couldn't export: the disk is read-only") });
    }

    fn pdf_bytes(&mut self, notes: bool) -> Vec<u8> {
        let (dw, dh) = (self.deck.w, self.deck.h);
        // slides are drawn 1600 pixels wide
        let pw = 1600;
        let ph = pw * dh / dw;
        let mut pages = Vec::new();
        for (i, slide) in self.deck.slides.iter().enumerate() {
            let mut cv = Canvas::new(pw, ph);
            let o = Opts { number: i + 1, ..Opts::default() };
            draw_slide(&mut cv, 0, 0, pw, &self.deck, slide, &mut self.pics, &o);
            let mut rgb = Vec::with_capacity((pw * ph * 3) as usize);
            for p in &cv.px {
                rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
            }
            // text in points: slide units are 3/4 point; the page is the slide
            let (page_w, page_h, sx, sy, scale) = if notes { (595, 842, 50, 60, 495 * 1000 / (dw * 3 / 4)) } else { (dw * 3 / 4, dh * 3 / 4, 0, 0, 1000) };
            let tp = |u: i32| u * 3 / 4 * scale / 1000;
            let mut texts = Vec::new();
            for sh in &slide.shapes {
                if sh.rot != 0 {
                    continue;
                }
                let mut boxes: Vec<Shape> = Vec::new();
                if sh.kind.has_text() && !sh.is_empty() {
                    boxes.push(sh.clone());
                }
                if let Some(t) = &sh.table {
                    for r in 0..t.nrows() {
                        for c in 0..t.ncols() {
                            if let Some(cs) = sh.cell_shape(r, c) {
                                if !cs.is_empty() {
                                    boxes.push(cs);
                                }
                            }
                        }
                    }
                }
                for b in boxes {
                    let tb = layout(&b);
                    for l in &tb.lines {
                        let p = &b.text.paras[l.p];
                        let s: String = p.text[l.start..l.end].iter().collect();
                        let s = s.trim_end().to_string();
                        if s.is_empty() {
                            continue;
                        }
                        let w = span_w(b.kind, p, l.start, l.start + s.chars().count(), l.px);
                        texts.push(crate::pdf::Text { x: sx + tp(b.x + l.x), y: sy + tp(b.y + l.base), size: tp(l.px).max(1), s, w: tp(w), visible: false, color: 0 });
                    }
                }
            }
            if notes {
                let (iw, ih) = (495, 495 * dh / dw);
                let mut y = 60 + ih + 40;
                let mut page = crate::pdf::Page { w: page_w, h: page_h, image: Some((50, 60, iw, ih, pw as u32, ph as u32, rgb)), texts };
                page.texts.push(crate::pdf::Text { x: 50, y: 40, size: 10, s: format!("Slide {} of {}", i + 1, self.deck.slides.len()), w: 0, visible: true, color: 0x6B6780 });
                for line in crate::pdf::wrap(&slide.notes, 12, 495) {
                    if y > 800 {
                        pages.push(page);
                        page = crate::pdf::Page { w: page_w, h: page_h, image: None, texts: vec![] };
                        y = 60;
                    }
                    page.texts.push(crate::pdf::Text { x: 50, y, size: 12, s: line, w: 0, visible: true, color: 0x1E1B2C });
                    y += 17;
                }
                pages.push(page);
            } else {
                pages.push(crate::pdf::Page { w: page_w, h: page_h, image: Some((0, 0, page_w, page_h, pw as u32, ph as u32, rgb)), texts });
            }
        }
        crate::pdf::write(&self.title(), &self.deck.author, &pages)
    }
}

impl App for Slides {
    fn kind(&self) -> AppKind {
        AppKind::Slides
    }

    fn fullscreen(&self) -> bool {
        self.show.is_some()
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        self.ticks = ui.ticks;
        if self.show.is_some() {
            self.render_show(ui, r, inst);
            return;
        }
        let t = ui.t;
        let compact = super::compact(r);
        let title_x = r.x + 18;
        let bx = r.r() - 118 - if compact { 96 } else { 136 };
        ui.icon(Icon::Slides, title_x, r.y + 13, 18, t.accent);
        let tr = Rect::new(title_x + 26, r.y + 7, bx - title_x - 34, 30);
        match &self.overlay {
            Overlay::Rename(e) => {
                ui.line(tr, e, "Presentation name", true, Action::App(inst, C_RENAME));
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
        let mut x = bx;
        if !compact {
            ui.icon_button(Rect::new(x, r.y + 8, 28, 28), Icon::Plus, Action::App(inst, C_NEW), 16);
            x += 34;
        }
        ui.icon_button(Rect::new(x, r.y + 8, 28, 28), Icon::Folder, Action::App(inst, C_OPEN), 16);
        x += 34;
        ui.icon_button(Rect::new(x, r.y + 8, 28, 28), Icon::Save, Action::App(inst, C_SAVE), 16);
        x += 36;
        // play
        let pr = Rect::new(x, r.y + 8, 30, 28);
        let pa = Action::App(inst, C_PLAY);
        ui.rrect(pr, 8, if ui.hot(pa) { t.accent.mix(t.text, 20) } else { t.accent });
        ui.icon_in(Icon::Play, pr, 13, t.on_accent);
        ui.zone(pr, pa);
        self.render_toolbar(ui, r, inst, compact);
        let top = r.y + HEADER + TOOLBAR;
        let bottom = r.b() - STATUS;
        let strip_w = if compact { 0 } else { STRIP_W };
        if !compact {
            self.render_strip(ui, Rect::new(r.x, top, strip_w, bottom - top), inst);
        }
        let notes_h = if r.h > 480 { NOTES_H } else { 0 };
        self.render_canvas(ui, Rect::new(r.x + strip_w, top, r.w - strip_w, bottom - top - notes_h), inst);
        if notes_h > 0 {
            self.render_notes(ui, Rect::new(r.x + strip_w, bottom - notes_h, r.w - strip_w, notes_h), inst);
        }
        // status bar
        let sy = r.b() - STATUS;
        ui.rect(Rect::new(r.x, sy, r.w, STATUS), t.surface);
        ui.rect(Rect::new(r.x, sy, r.w, 1), t.line);
        let n = self.deck.slides.len();
        let mut sx = r.x + 12;
        if compact {
            ui.icon_button(Rect::new(sx, sy + 3, 22, 22), Icon::ChevronLeft, Action::App(inst, C_PREV), 12);
            sx += 24;
            ui.icon_button(Rect::new(sx, sy + 3, 22, 22), Icon::ChevronRight, Action::App(inst, C_NEXT), 12);
            sx += 30;
        }
        let left = format!("Slide {} of {}   ·   {}   ·   {}", self.cur + 1, n, self.slide().layout.name(), self.deck.theme.name);
        ui.text(sx, sy + 19, Face::Regular, 12, &left, t.text2);
        let right = if self.imported {
            String::from("Viewing a PowerPoint file  ·  saving creates a .hydp copy")
        } else {
            format!("Hyda Slides presentation  ·  {}", if self.dirty { "Edited" } else if self.path.is_empty() { "Not saved yet" } else { "Saved" })
        };
        if !compact {
            let rw = ui.tw(Face::Regular, 12, &right);
            if r.r() - rw - 16 > sx + ui.tw(Face::Regular, 12, &left) + 20 {
                ui.text(r.r() - rw - 16, sy + 19, Face::Regular, 12, &right, t.text3);
            }
        }
        self.render_overlay(ui, r, inst);
    }

    fn action(&mut self, code: u32, double: bool, sys: &mut Sys) {
        if self.show.is_some() {
            if code == C_SHOW {
                self.show_step(true);
            }
            return;
        }
        if let Overlay::Rename(e) = &self.overlay {
            if code != C_RENAME {
                let name = e.text.clone();
                self.overlay = Overlay::None;
                self.rename_to(&name, sys);
            }
        }
        // the chart data sheet and the footer dialog
        match &mut self.overlay {
            Overlay::ChartData(_) => {
                match code {
                    C_DLG_TEXT => {}
                    C_DISMISS => {
                        self.data_commit();
                        self.overlay = Overlay::None;
                    }
                    c if c == C_DATA_TITLE || (C_DATA_DONE..=C_DATA_LEGEND).contains(&c) || c >= C_DATA_KIND => self.data_action(c),
                    _ => {}
                }
                return;
            }
            Overlay::Footer(dlg) => {
                match code {
                    C_DLG_NUMBERS => dlg.numbers = !dlg.numbers,
                    C_DLG_OK => {
                        let (text, numbers) = (dlg.text.text.trim().to_string(), dlg.numbers);
                        self.overlay = Overlay::None;
                        self.snapshot();
                        self.deck.footer = text;
                        self.deck.numbers = numbers;
                        self.touch_all();
                    }
                    C_DISMISS => self.overlay = Overlay::None,
                    _ => {}
                }
                return;
            }
            _ => {}
        }
        // a menu or chooser is open: pick from it, or close it
        match &self.overlay {
            Overlay::Menu(kind, _) => {
                let kind = *kind;
                if (C_MENU..C_FILE).contains(&code) {
                    self.overlay = Overlay::None;
                    self.menu_pick(kind, (code - C_MENU) as usize);
                    return;
                }
                let same = matches!(
                    (kind, code),
                    (MenuKind::NewSlide, C_NEW_SLIDE) | (MenuKind::Layout, C_LAYOUT) | (MenuKind::Theme, C_THEME) | (MenuKind::Color, C_COLOR) | (MenuKind::Trans, C_TRANS) | (MenuKind::Shapes, C_RECT) | (MenuKind::Table, C_TABLE) | (MenuKind::Chart, C_CHART) | (MenuKind::TableEdit | MenuKind::ChartEdit, C_OBJECT) | (MenuKind::Anim, C_ANIM)
                );
                self.overlay = Overlay::None;
                if same || code == C_DISMISS {
                    return;
                }
            }
            Overlay::Open(_) | Overlay::Pictures(_) if code < C_FILE && code != C_NEW => {
                self.overlay = Overlay::None;
                if matches!(code, C_DISMISS | C_OPEN | C_PICTURE) {
                    return;
                }
            }
            _ => {}
        }
        if code != C_CANVAS && code != C_NOTES && code >= C_NEW_SLIDE && code != C_BOLD && code != C_ITALIC && code != C_UNDERLINE && !(C_LEFT..=C_BIGGER).contains(&code) && code != C_COLOR {
            // toolbar and menu commands leave text editing (formatting keeps it)
            if !(C_MENU..C_FILE).contains(&code) && !matches!(code, C_THEME | C_TRANS | C_UNDO | C_REDO | C_COPY | C_CUT | C_PASTE | C_ALL | C_OBJECT) {
                self.stop_edit();
            }
        }
        self.press = None;
        match code {
            C_CANVAS => self.canvas_click(double),
            C_NOTES => {
                self.stop_edit();
                self.notes_typing = false;
                self.sel = None;
                let n = self.slide().notes.chars().count();
                self.notes = Some(self.notes.unwrap_or(n).min(n));
                if self.notes.is_some() && double {
                    self.notes = Some(n);
                }
            }
            C_STRIP => {
                self.stop_edit();
                self.notes = None;
                self.sel = None;
            }
            c if (C_THUMB..C_MENU).contains(&c) => {
                let i = (c - C_THUMB) as usize;
                if i < self.deck.slides.len() {
                    self.go(i);
                    if double {
                        self.play(false);
                        return;
                    }
                    self.press = Some(Press::Thumb(false));
                }
            }
            C_ADD_SLIDE => {
                let l = if self.slide().layout == Layout::Title { Layout::TitleContent } else { self.slide().layout };
                let l = if l == Layout::Blank { Layout::TitleContent } else { l };
                self.add_slide(l);
            }
            C_NEW_SLIDE => self.open_menu(MenuKind::NewSlide, code),
            C_LAYOUT => self.open_menu(MenuKind::Layout, code),
            C_THEME => self.open_menu(MenuKind::Theme, code),
            C_COLOR => self.open_menu(MenuKind::Color, code),
            C_TRANS => self.open_menu(MenuKind::Trans, code),
            C_BOLD => self.toggle_fmt(BOLD),
            C_ITALIC => self.toggle_fmt(ITALIC),
            C_UNDERLINE => self.toggle_fmt(UNDERLINE),
            C_LEFT => self.each_para(|p| p.align = Align::Left),
            C_CENTER => self.each_para(|p| p.align = Align::Center),
            C_RIGHT => self.each_para(|p| p.align = Align::Right),
            C_BULLETS => self.set_list(Style::Bullet),
            C_NUMBERS => self.set_list(Style::Number),
            C_SMALLER => self.resize_text(false),
            C_BIGGER => self.resize_text(true),
            C_TEXTBOX => self.add_shape(Kind::Text),
            C_RECT => self.open_menu(MenuKind::Shapes, code),
            C_TABLE => self.open_menu(MenuKind::Table, code),
            C_CHART => self.open_menu(MenuKind::Chart, code),
            C_OBJECT => match self.shape().map(|s| s.kind) {
                Some(Kind::Table) => self.open_menu(MenuKind::TableEdit, code),
                Some(Kind::Chart) => self.open_menu(MenuKind::ChartEdit, code),
                _ => {}
            },
            C_ANIM => {
                if self.sel.is_some() {
                    self.open_menu(MenuKind::Anim, code);
                }
            }
            C_FOOTER => self.overlay = Overlay::Footer(FooterDlg { text: LineEdit::new(self.deck.footer.clone()), numbers: self.deck.numbers || self.deck.footer.is_empty() }),
            C_PDF => self.export_pdf(sys, false),
            C_PDF_NOTES => self.export_pdf(sys, true),
            C_PRESENTER => {
                self.play(false);
                if let Some(s) = self.show.as_mut() {
                    s.presenter = true;
                }
            }
            C_ROT_R => self.rotate_by(90),
            C_ROT_L => self.rotate_by(-90),
            C_FLIP_H => self.flip(true),
            C_FLIP_V => self.flip(false),
            C_ELLIPSE => self.add_shape(Kind::Ellipse),
            C_PICTURE => {
                self.overlay = Overlay::Pictures(Self::list_files(sys, &[".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"], "\u{0}"));
            }
            C_UNDO => self.undo_redo(true),
            C_REDO => self.undo_redo(false),
            C_PLAY => self.play(false),
            C_PLAY_START => self.play(true),
            C_NEW => {
                self.overlay = Overlay::None;
                self.park(sys);
                self.reset(Deck::new());
            }
            C_OPEN => {
                self.stop_edit();
                self.overlay = Overlay::Open(Self::list_files(sys, &[".pptx"], ".hydp"));
            }
            C_SAVE => self.save(sys, false),
            C_EXPORT => self.export(sys),
            C_RENAME => {
                if !matches!(self.overlay, Overlay::Rename(_)) {
                    self.stop_edit();
                    self.overlay = Overlay::Rename(LineEdit::new(self.title()));
                }
            }
            C_CUT => {
                self.copy(sys);
                if self.ed.is_some() {
                    self.begin_text_change(false);
                    self.touch();
                } else {
                    self.delete_shape();
                }
            }
            C_COPY => self.copy(sys),
            C_PASTE => self.paste(sys),
            C_DUP => {
                if self.sel.is_some() && self.ed.is_none() {
                    self.copy(sys);
                    self.paste(sys);
                } else {
                    self.duplicate_slide();
                }
            }
            C_DELETE => {
                if self.sel.is_some() {
                    self.delete_shape();
                } else {
                    self.delete_slide();
                }
            }
            C_ALL => {
                if let Some((sh, ed)) = self.ed_shape() {
                    ed.anchor = Some(Pos::new(0, 0));
                    ed.caret = sh.text.end();
                }
            }
            C_PREV => {
                if self.cur > 0 {
                    let c = self.cur - 1;
                    self.go(c);
                }
            }
            C_NEXT => {
                let c = self.cur + 1;
                if c < self.deck.slides.len() {
                    self.go(c);
                }
            }
            C_DEL_SLIDE => self.delete_slide(),
            C_DUP_SLIDE => self.duplicate_slide(),
            C_SLIDE_UP => {
                if self.cur > 0 {
                    self.snapshot();
                    let c = self.cur - 1;
                    self.move_slide(c);
                }
            }
            C_SLIDE_DOWN => {
                self.snapshot();
                let c = self.cur + 1;
                self.move_slide(c);
            }
            C_FRONT => self.restack(true),
            C_BACK => self.restack(false),
            C_DISMISS => {}
            c if (C_FILE..C_FILE + FILES_MAX).contains(&c) => {
                let (path, pic) = match &self.overlay {
                    Overlay::Open(files) => (files.get((c - C_FILE) as usize).cloned(), false),
                    Overlay::Pictures(files) => (files.get((c - C_FILE) as usize).cloned(), true),
                    _ => (None, false),
                };
                self.overlay = Overlay::None;
                if let Some(p) = path {
                    if pic {
                        self.insert_picture(&p, sys);
                    } else {
                        self.load(&p, sys);
                    }
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, gen: bool, sys: &mut Sys) {
        let shift = crate::input::shift();
        if self.show.is_some() {
            match k {
                Key::Esc => {
                    let idx = self.show.as_ref().map(|s| s.idx).unwrap_or(0);
                    self.show = None;
                    self.go(idx.min(self.deck.slides.len() - 1));
                }
                Key::Right | Key::Down | Key::PageDown | Key::Enter | Key::Char(' ') | Key::Char('n') => self.show_step(true),
                Key::Left | Key::Up | Key::PageUp | Key::Backspace | Key::Char('p') => self.show_step(false),
                Key::Home | Key::End => {
                    let n = if k == Key::Home { 0 } else { self.deck.slides.len() - 1 };
                    if let Some(s) = self.show.as_mut() {
                        s.idx = n;
                        s.step = 0;
                        s.from = None;
                        s.frame = None;
                        s.playing = None;
                        s.end = false;
                    }
                }
                Key::Char('v') | Key::Char('V') => {
                    if let Some(s) = self.show.as_mut() {
                        s.presenter = !s.presenter;
                    }
                }
                Key::Char('b') | Key::Char('.') => {
                    if let Some(s) = self.show.as_mut() {
                        s.black = !s.black;
                    }
                }
                _ => {}
            }
            return;
        }
        if let Overlay::Rename(e) = &mut self.overlay {
            match k {
                Key::Enter => {
                    let name = e.text.clone();
                    self.overlay = Overlay::None;
                    self.rename_to(&name, sys);
                }
                Key::Esc => self.overlay = Overlay::None,
                _ => {
                    e.key_sys(k, gen, sys);
                }
            }
            return;
        }
        if matches!(self.overlay, Overlay::ChartData(_)) {
            self.data_key(k, gen, sys);
            return;
        }
        if let Overlay::Footer(dlg) = &mut self.overlay {
            match k {
                Key::Enter => self.action(C_DLG_OK, false, sys),
                Key::Esc => self.overlay = Overlay::None,
                _ => {
                    dlg.text.key_sys(k, gen, sys);
                }
            }
            return;
        }
        if !matches!(self.overlay, Overlay::None) {
            if k == Key::Esc {
                self.overlay = Overlay::None;
            }
            return;
        }
        if k == Key::F(5) {
            self.play(!shift);
            return;
        }
        if self.notes.is_some() {
            self.notes_key(k, gen, sys);
            return;
        }
        if gen {
            match k {
                Key::Char(c) => match c.to_ascii_lowercase() {
                    'b' => self.toggle_fmt(BOLD),
                    'i' => self.toggle_fmt(ITALIC),
                    'u' => self.toggle_fmt(UNDERLINE),
                    'e' => self.each_para(|p| p.align = Align::Center),
                    'l' => self.each_para(|p| p.align = Align::Left),
                    'r' => self.each_para(|p| p.align = Align::Right),
                    'z' => self.undo_redo(true),
                    'y' => self.undo_redo(false),
                    'x' => self.action(C_CUT, false, sys),
                    'c' => self.copy(sys),
                    'v' => self.paste(sys),
                    'd' => self.action(C_DUP, false, sys),
                    'a' => self.action(C_ALL, false, sys),
                    'm' => self.action(C_ADD_SLIDE, false, sys),
                    's' => self.save(sys, false),
                    'o' => self.action(C_OPEN, false, sys),
                    'n' => self.action(C_NEW, false, sys),
                    ']' => self.resize_text(true),
                    '[' => self.resize_text(false),
                    _ => {}
                },
                Key::Left | Key::Right if self.ed.is_some() => self.move_caret(k, shift, true),
                Key::Up if self.ed.is_none() && self.sel.is_none() => self.action(C_SLIDE_UP, false, sys),
                Key::Down if self.ed.is_none() && self.sel.is_none() => self.action(C_SLIDE_DOWN, false, sys),
                Key::Home if self.ed.is_some() => {
                    if let Some((_, ed)) = self.ed_shape() {
                        if shift && ed.anchor.is_none() {
                            ed.anchor = Some(ed.caret);
                        }
                        if !shift {
                            ed.anchor = None;
                        }
                        ed.caret = Pos::new(0, 0);
                    }
                }
                Key::End if self.ed.is_some() => {
                    if let Some((sh, ed)) = self.ed_shape() {
                        if shift && ed.anchor.is_none() {
                            ed.anchor = Some(ed.caret);
                        }
                        if !shift {
                            ed.anchor = None;
                        }
                        ed.caret = sh.text.end();
                    }
                }
                _ => {}
            }
            return;
        }
        // typing in a shape
        if self.ed.is_some() {
            match k {
                Key::Char(c) if !c.is_control() => self.type_text(c.encode_utf8(&mut [0u8; 4])),
                Key::Enter => self.enter(),
                Key::Backspace => self.backspace(false),
                Key::Delete => self.backspace(true),
                Key::Tab if self.ed.map_or(false, |e| e.cell.is_some()) => self.next_cell(!shift),
                Key::Tab => {
                    let body = self.shape().map_or(false, |s| s.kind == Kind::Body || s.text.paras.iter().any(|p| p.style != Style::Body));
                    if body {
                        self.indent(shift);
                    } else {
                        self.type_text("\t");
                    }
                }
                Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End => self.move_caret(k, shift, false),
                Key::Esc => {
                    self.stop_edit();
                }
                _ => {}
            }
            return;
        }
        // a selected shape
        if let Some(i) = self.sel {
            let step = if shift { 1 } else { 8 };
            let nudge = |d: (i32, i32)| d;
            let d = match k {
                Key::Left => Some(nudge((-step, 0))),
                Key::Right => Some(nudge((step, 0))),
                Key::Up => Some(nudge((0, -step))),
                Key::Down => Some(nudge((0, step))),
                _ => None,
            };
            if let Some((dx, dy)) = d {
                self.snapshot();
                let sh = &mut self.slide_mut().shapes[i];
                sh.x += dx;
                sh.y += dy;
                self.touch();
                return;
            }
            match k {
                Key::Delete | Key::Backspace => self.delete_shape(),
                Key::Esc => self.sel = None,
                Key::Enter | Key::F(2) => self.start_edit(i, None),
                Key::Tab => {
                    let n = self.slide().shapes.len();
                    self.sel = Some(if shift { (i + n - 1) % n } else { (i + 1) % n });
                }
                Key::Char(c) if !c.is_control() => {
                    if self.slide().shapes[i].kind.has_text() {
                        self.start_edit(i, None);
                        self.type_text(c.encode_utf8(&mut [0u8; 4]));
                    }
                }
                _ => {}
            }
            return;
        }
        // nothing selected: move between slides
        match k {
            Key::Up | Key::PageUp | Key::Left => {
                if self.cur > 0 {
                    let c = self.cur - 1;
                    self.go(c);
                }
            }
            Key::Down | Key::PageDown | Key::Right => {
                let c = self.cur + 1;
                if c < self.deck.slides.len() {
                    self.go(c);
                }
            }
            Key::Home => self.go(0),
            Key::End => {
                let n = self.deck.slides.len() - 1;
                self.go(n);
            }
            Key::Tab => {
                if !self.slide().shapes.is_empty() {
                    self.sel = Some(0);
                }
            }
            Key::Delete | Key::Backspace => self.delete_slide(),
            Key::Enter => self.action(C_ADD_SLIDE, false, sys),
            _ => {}
        }
    }

    fn scroll(&mut self, dy: i32) {
        if self.show.is_some() {
            self.show_step(dy > 0);
            return;
        }
        if self.strip.contains(self.mouse.0, self.mouse.1) {
            self.strip_scroll += dy * 60;
        } else if self.ed.is_none() && self.canvas.contains(self.mouse.0, self.mouse.1) {
            let c = (self.cur as i32 + dy.signum()).clamp(0, self.deck.slides.len() as i32 - 1) as usize;
            if c != self.cur {
                self.go(c);
            }
        }
    }

    fn mouse(&mut self, x: i32, y: i32) {
        self.mouse = (x, y);
    }

    fn drag(&mut self, x: i32, y: i32) {
        let Some(p) = self.press else { return };
        match p {
            Press::Move { start, orig, moved } => {
                let (dx, dy) = (x - start.0, y - start.1);
                if !moved && dx.abs() + dy.abs() < 3 {
                    return;
                }
                if !moved {
                    self.snapshot();
                    self.press = Some(Press::Move { start, orig, moved: true });
                }
                let k = |v: i32| (v as i64 * self.deck.w as i64 / self.canvas.w.max(1) as i64) as i32;
                let (mut nx, mut ny) = (orig.0 + k(dx), orig.1 + k(dy));
                // snap to the slide's centre lines and edges
                if let Some(i) = self.sel {
                    let (w, h) = (self.slide().shapes[i].w, self.slide().shapes[i].h);
                    let snap = |v: i32, size: i32, total: i32| {
                        for target in [0, (total - size) / 2, total - size] {
                            if (v - target).abs() <= 8 {
                                return target;
                            }
                        }
                        v
                    };
                    nx = snap(nx, w, self.deck.w);
                    ny = snap(ny, h, self.deck.h);
                    let sh = &mut self.slide_mut().shapes[i];
                    sh.x = nx;
                    sh.y = ny;
                    self.touch();
                }
            }
            Press::Resize { handle, start, orig, moved } => {
                if !moved {
                    self.snapshot();
                    self.press = Some(Press::Resize { handle, start, orig, moved: true });
                }
                let Some(i) = self.sel else { return };
                let rot = self.slide().shapes[i].rot;
                let k = |v: i32| (v as i64 * self.deck.w as i64 / self.canvas.w.max(1) as i64) as i32;
                // the pointer's movement in the shape's own (turned) frame
                let (dx, dy) = rotate(k(x - start.0), k(y - start.1), 0, 0, -rot);
                let (ox, oy, ow, oh) = orig;
                let (mut l, mut t, mut r, mut b) = (ox, oy, ox + ow, oy + oh);
                match handle {
                    0 => {
                        l += dx;
                        t += dy;
                    }
                    1 => t += dy,
                    2 => {
                        r += dx;
                        t += dy;
                    }
                    3 => r += dx,
                    4 => {
                        r += dx;
                        b += dy;
                    }
                    5 => b += dy,
                    6 => {
                        l += dx;
                        b += dy;
                    }
                    _ => l += dx,
                }
                let keep_aspect = self.slide().shapes[i].kind == Kind::Picture && handle % 2 == 0 && !crate::input::shift();
                if keep_aspect && oh > 0 {
                    // width leads; the opposite corner stays put
                    let w = (r - l).max(16);
                    let h = (w as i64 * oh as i64 / ow.max(1) as i64) as i32;
                    if handle == 0 || handle == 2 {
                        t = b - h;
                    } else {
                        b = t + h;
                    }
                }
                let (l, t) = (l.min(r - 16), t.min(b - 16));
                let (w, h) = ((r - l).max(16), (b - t).max(16));
                // keep the opposite side where it was on screen when turned
                let (ocx, ocy) = (ox + ow / 2, oy + oh / 2);
                let (ncx, ncy) = rotate(l + w / 2, t + h / 2, ocx, ocy, rot);
                let sh = &mut self.slide_mut().shapes[i];
                sh.x = ncx - w / 2;
                sh.y = ncy - h / 2;
                sh.w = w;
                sh.h = h;
                if sh.kind == Kind::Table {
                    // rows share the new height, then grow to fit their text
                    if let Some(tb) = sh.table.as_mut() {
                        let total: i32 = tb.rows.iter().sum::<i32>().max(1);
                        for rr in tb.rows.iter_mut() {
                            *rr = (*rr as i64 * h as i64 / total as i64) as i32;
                        }
                    }
                    sh.fit_table();
                }
                self.touch();
            }
            Press::Rotate { centre, orig, moved } => {
                if !moved {
                    self.snapshot();
                    self.press = Some(Press::Rotate { centre, orig, moved: true });
                }
                let Some(i) = self.sel else { return };
                let (dx, dy) = ((x - centre.0) as i64, (y - centre.1) as i64);
                if dx == 0 && dy == 0 {
                    return;
                }
                // the angle of the pointer from the centre, 0 pointing up
                let mut best = (i64::MAX, 0);
                for a in 0..360 {
                    let (s, c) = (sin_deg(a) as i64, -cos_deg(a) as i64);
                    // the direction (sin a, -cos a) nearest the pointer's
                    let cross = (dx * c - dy * s).abs();
                    let dot = dx * s + dy * c;
                    if dot > 0 && cross < best.0 {
                        best = (cross, a);
                    }
                }
                let mut a = best.1;
                if crate::input::shift() {
                    a = (a + 7) / 15 * 15;
                } else {
                    for snap in [0, 90, 180, 270, 360] {
                        if (a - snap).abs() <= 3 {
                            a = snap;
                        }
                    }
                }
                let _ = orig;
                self.slide_mut().shapes[i].rot = a.rem_euclid(360);
                self.touch();
            }
            Press::LineEnd { end, moved } => {
                if !moved {
                    self.snapshot();
                    self.press = Some(Press::LineEnd { end, moved: true });
                }
                let Some(i) = self.sel else { return };
                let (ux, uy) = self.to_units(x, y);
                let sh = &mut self.slide_mut().shapes[i];
                let (a, b) = line_ends(sh);
                let mut p = (ux, uy);
                // Shift keeps it level, upright or diagonal
                let other = if end == 0 { b } else { a };
                if crate::input::shift() {
                    let (dx, dy) = (p.0 - other.0, p.1 - other.1);
                    if dy.abs() * 2 < dx.abs() {
                        p.1 = other.1;
                    } else if dx.abs() * 2 < dy.abs() {
                        p.0 = other.0;
                    } else {
                        let m = dx.abs().max(dy.abs());
                        p = (other.0 + m * dx.signum(), other.1 + m * dy.signum());
                    }
                }
                if end == 0 {
                    set_line_ends(sh, p, b);
                } else {
                    set_line_ends(sh, a, p);
                }
                self.touch();
            }
            Press::Text => {
                let (ux, uy) = self.to_units(x, y);
                if let (Some(_), Some(mut sh)) = (self.ed, self.text_target()) {
                    sh.rot = 0;
                    let tb = layout(&sh);
                    let p = hit(&sh, &tb, ux - sh.x, uy - sh.y);
                    let e = self.ed.as_mut().unwrap();
                    if e.anchor.is_none() {
                        e.anchor = Some(e.caret);
                    }
                    e.caret = p;
                }
            }
            Press::Thumb(moved) => {
                if !self.strip.contains(x, self.strip.y + 1) {
                    return;
                }
                let step = self.thumb_step();
                let to = ((y - self.strip.y - 12 + self.strip_scroll).max(0) / step) as usize;
                let to = to.min(self.deck.slides.len() - 1);
                if to != self.cur {
                    if !moved {
                        self.snapshot();
                        self.press = Some(Press::Thumb(true));
                    }
                    self.move_slide(to);
                }
            }
        }
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => vec![("New Presentation\tGen+N", C_NEW), ("Open…\tGen+O", C_OPEN), ("Save\tGen+S", C_SAVE), ("Export as PowerPoint (.pptx)", C_EXPORT), ("Export as PDF", C_PDF), ("Export Notes Pages (PDF)", C_PDF_NOTES), ("Print…", C_PDF), ("Rename…", C_RENAME)],
            1 => vec![("Undo\tGen+Z", C_UNDO), ("Redo\tGen+Y", C_REDO), ("Cut\tGen+X", C_CUT), ("Copy\tGen+C", C_COPY), ("Paste\tGen+V", C_PASTE), ("Duplicate\tGen+D", C_DUP), ("Delete\tDelete", C_DELETE), ("Select All\tGen+A", C_ALL), ("Bring to Front", C_FRONT), ("Send to Back", C_BACK), ("Rotate Right 90°", C_ROT_R), ("Rotate Left 90°", C_ROT_L), ("Flip Horizontal", C_FLIP_H), ("Flip Vertical", C_FLIP_V)],
            2 => vec![("Play from Start\tF5", C_PLAY_START), ("Play from This Slide\tShift+F5", C_PLAY), ("Presenter View", C_PRESENTER), ("Insert Text Box", C_TEXTBOX), ("Insert Shape…", C_RECT), ("Insert Table…", C_TABLE), ("Insert Chart…", C_CHART), ("Insert Picture…", C_PICTURE), ("Header & Footer…", C_FOOTER)],
            3 => vec![("New Slide\tGen+M", C_ADD_SLIDE), ("Duplicate Slide", C_DUP_SLIDE), ("Delete Slide", C_DEL_SLIDE), ("Move Slide Up\tGen+↑", C_SLIDE_UP), ("Move Slide Down\tGen+↓", C_SLIDE_DOWN), ("Previous Slide", C_PREV), ("Next Slide", C_NEXT)],
            _ => vec![],
        }
    }

    fn tick(&mut self, _sys: &mut Sys) {}

    fn close(&mut self, sys: &mut Sys) {
        self.show = None;
        self.park(sys);
    }

    fn animating(&self) -> bool {
        self.ed.is_some() || self.notes.is_some() || matches!(self.overlay, Overlay::ChartData(_)) || self.show.as_ref().map_or(false, |s| s.from.is_some() || s.playing.is_some() || s.presenter)
    }

    fn open_path(&mut self, path: &str, sys: &mut Sys) {
        self.load(path, sys);
    }
}
