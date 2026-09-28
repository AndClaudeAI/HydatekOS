//! The on-screen keyboard for touch (portrait) screens, shared by the lock
//! screen and the setup assistant: letters, two symbol pages that cover
//! printable ASCII, and special characters (currencies first, the Naira
//! leading). Its keys report codes; the owner maps them to actions.

use crate::font::Face;
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::ui::{Action, Ui};
use alloc::vec::Vec;

pub const ENTER: u8 = 12;
pub const KB_SHIFT: u8 = 31;
pub const KB_PAGE: u8 = 32; // letters <-> symbols ("123" / "ABC")
pub const KB_MORE: u8 = 33; // 123 <-> #+=
pub const KB_SPACE: u8 = 34;
pub const KB_BACK: u8 = 35;
pub const KB_SPECIAL: u8 = 36; // special characters (currencies, ©, ±, …) on and off
pub const KB_CHAR: u8 = 100; // + row * 16 + column

/// Keyboard pages.
const PAGES: [[&str; 3]; 4] = [
    ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
    ["1234567890", "-/:;()$&@\"", ".,?!'"],
    ["[]{}#%^*+=", "_\\|~<>`", ".,?!'"],
    ["₦€£¥¢₹₵§¶©", "®™°±×÷¿¡«»", "•…µ¬¦¤"],
];

pub enum Cap {
    Char(char),
    Text(&'static str),
    Icon(Icon),
}

/// What a key press means for the text being typed.
pub enum Typed {
    Char(char),
    Back,
    Enter,
    /// a page or shift change: nothing typed
    None,
}

#[derive(Default)]
pub struct Osk {
    page: usize,
    /// 0 off, 1 next letter, 2 caps lock
    shift: u8,
    shift_at: u64,
}

impl Osk {
    pub fn reset(&mut self) {
        self.page = 0;
        self.shift = 0;
    }

    pub fn press(&mut self, code: u8, now: u64) -> Typed {
        match code {
            KB_SHIFT => {
                // tap: next letter upper case; double tap: caps lock; tap again: off
                self.shift = match self.shift {
                    0 if now < self.shift_at + 50 => 2,
                    0 => 1,
                    1 if now < self.shift_at + 50 => 2,
                    _ => 0,
                };
                self.shift_at = now;
            }
            KB_PAGE => self.page = if self.page == 0 { 1 } else { 0 },
            KB_SPECIAL => self.page = if self.page == 3 { 1 } else { 3 },
            KB_MORE => self.page = if self.page == 1 { 2 } else { 1 },
            KB_SPACE => return Typed::Char(' '),
            KB_BACK => return Typed::Back,
            ENTER => return Typed::Enter,
            c if c >= KB_CHAR => {
                let (row, col) = (((c - KB_CHAR) / 16) as usize, ((c - KB_CHAR) % 16) as usize);
                if let Some(ch) = PAGES[self.page].get(row).and_then(|r| r.chars().nth(col)) {
                    let ch = if self.page == 0 && self.shift > 0 { ch.to_ascii_uppercase() } else { ch };
                    if self.shift == 1 {
                        self.shift = 0;
                    }
                    return Typed::Char(ch);
                }
            }
            _ => {}
        }
        Typed::None
    }

    /// A key was typed on a real keyboard: a one-letter shift is used up.
    pub fn typed(&mut self) {
        if self.shift == 1 {
            self.shift = 0;
        }
    }

    /// Start the next letter in upper case (for names).
    pub fn capitalise_next(&mut self) {
        if self.page == 0 && self.shift == 0 {
            self.shift = 1;
        }
    }

    /// The keyboard panel along the bottom of `r` and its keys (logical
    /// coordinates).
    pub fn layout(&self, r: Rect) -> (Rect, Vec<(Rect, u8, Cap)>) {
        let tall = r.h > r.w;
        let u = |v: i32| if tall { v * r.w / 390 } else { v };
        let pw = if tall { r.w } else { r.w.min(760) };
        let (pad, gap, vgap, kh) = (u(6), u(6), u(10), u(44));
        let kw = (pw - 2 * pad - 9 * gap) / 10;
        let h = 4 * kh + 3 * vgap + 2 * u(10) + if tall { u(14) } else { 0 };
        let panel = Rect::new(r.x + (r.w - pw) / 2, r.b() - h, pw, h);
        let x0 = panel.x + pad;
        let row_y = |i: i32| panel.y + u(10) + i * (kh + vgap);
        let mut keys = Vec::new();
        let page = &PAGES[self.page];
        // rows 1-2: characters, centred
        for (ri, row) in page.iter().take(2).enumerate() {
            let n = row.chars().count() as i32;
            let start = panel.x + (pw - (n * kw + (n - 1) * gap)) / 2;
            for (ci, ch) in row.chars().enumerate() {
                let b = Rect::new(start + ci as i32 * (kw + gap), row_y(ri as i32), kw, kh);
                keys.push((b, KB_CHAR + (ri * 16 + ci) as u8, Cap::Char(ch)));
            }
        }
        // row 3: shift / #+= / 123, characters, backspace
        let wide = kw * 3 / 2;
        let y3 = row_y(2);
        let (left_code, left) = match self.page {
            0 => (KB_SHIFT, Cap::Icon(Icon::Shift)),
            1 => (KB_MORE, Cap::Text("#+=")),
            _ => (KB_MORE, Cap::Text("123")),
        };
        keys.push((Rect::new(x0, y3, wide, kh), left_code, left));
        let row = page[2];
        let n = row.chars().count() as i32;
        let inner = pw - 2 * pad - 2 * wide - 2 * gap;
        let cw = if self.page == 0 { kw } else { (inner - (n - 1) * gap) / n };
        let start = x0 + wide + gap + (inner - (n * cw + (n - 1) * gap)) / 2;
        for (ci, ch) in row.chars().enumerate() {
            keys.push((Rect::new(start + ci as i32 * (cw + gap), y3, cw, kh), KB_CHAR + (32 + ci) as u8, Cap::Char(ch)));
        }
        keys.push((Rect::new(panel.r() - pad - wide, y3, wide, kh), KB_BACK, Cap::Icon(Icon::Backspace)));
        // row 4: page switch, special characters, space, enter
        let y4 = row_y(3);
        let side = kw * 2 + gap;
        let sp = kw * 3 / 2;
        keys.push((Rect::new(x0, y4, side, kh), KB_PAGE, Cap::Text(if self.page == 0 { "123" } else { "ABC" })));
        keys.push((Rect::new(x0 + side + gap, y4, sp, kh), KB_SPECIAL, Cap::Text("₦€£")));
        let sx = x0 + side + sp + 2 * gap;
        keys.push((Rect::new(sx, y4, panel.r() - pad - side - gap - sx, kh), KB_SPACE, Cap::Text("space")));
        keys.push((Rect::new(panel.r() - pad - side, y4, side, kh), ENTER, Cap::Icon(Icon::ChevronRight)));
        (panel, keys)
    }

    /// Draw the panel and keys; `act` turns a key code into the owner's
    /// action, and `swallow` is the action for the panel between the keys.
    pub fn render(&self, ui: &mut Ui, panel: Rect, keys: &[(Rect, u8, Cap)], tall: bool, act: impl Fn(u8) -> Action, swallow: Action) {
        let t = ui.t;
        let u = |v: i32| if tall { v * panel.w / 390 } else { v };
        let bg = Rect::new(panel.x, panel.y, panel.w, panel.h + u(24));
        ui.rrect(bg, u(22), t.chip.with_alpha(245));
        ui.zone(panel, swallow);
        for (b, code, cap) in keys {
            let a = act(*code);
            let special = !matches!(cap, Cap::Char(_)) || *code == KB_SPACE;
            let on = (*code == KB_SHIFT && self.shift > 0) || (*code == KB_SPECIAL && self.page == 3);
            let (mut kbg, fg) = if *code == ENTER {
                (t.accent, t.on_accent)
            } else if on {
                (t.text, t.surface)
            } else if special {
                (t.surface.mix(t.chip, 55), t.text)
            } else {
                (t.surface, t.text)
            };
            if ui.hot(a) {
                kbg = kbg.mix(t.text, 25);
            }
            ui.rrect(*b, u(8), kbg);
            match cap {
                Cap::Char(c) => {
                    let c = if self.page == 0 && self.shift > 0 { c.to_ascii_uppercase() } else { *c };
                    let mut buf = [0u8; 4];
                    ui.text_in(*b, Face::Regular, u(20), c.encode_utf8(&mut buf), fg, 1);
                }
                Cap::Text(s) => {
                    ui.text_in(*b, Face::Medium, u(14), s, fg, 1);
                }
                Cap::Icon(i) => {
                    // caps lock: underline the shift arrow
                    ui.icon_in(*i, *b, u(20), fg);
                    if *code == KB_SHIFT && self.shift == 2 {
                        ui.rect(Rect::new(b.x + b.w / 2 - u(7), b.b() - u(9), u(14), u(2)), fg);
                    }
                }
            }
            ui.zone(*b, a);
        }
    }
}
