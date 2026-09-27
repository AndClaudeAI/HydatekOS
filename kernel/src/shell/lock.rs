//! Lock screen: clock, date, "Up next", phone notifications and an optional
//! PIN. It keeps people out of the session; it does not encrypt files.

use super::wallpaper;
use crate::font::Face;
use crate::gfx::{sin_q14, Rect};
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::String;

pub const TAP: u8 = 0;
pub const DIGIT: u8 = 1; // DIGIT + n for n in 0..=9
pub const BACK: u8 = 11;
pub const ENTER: u8 = 12;

const MAX_TRIES: u32 = 5;
const WAIT_TICKS: u64 = 3000; // 30 s

#[derive(Default)]
pub struct Lock {
    pub entry: String,
    pub error: String,
    fails: u32,
    wait_until: u64,
    shake_from: u64,
}

pub enum Outcome {
    Stay,
    Unlock,
}

impl Lock {
    pub fn reset(&mut self) {
        self.entry.clear();
        self.error.clear();
    }

    fn waiting(&self, now: u64) -> bool {
        now < self.wait_until
    }

    fn submit(&mut self, sys: &Sys, now: u64) -> Outcome {
        if self.waiting(now) || self.entry.is_empty() {
            return Outcome::Stay;
        }
        if sys.check_pin(&self.entry) {
            self.fails = 0;
            self.reset();
            return Outcome::Unlock;
        }
        self.entry.clear();
        self.fails += 1;
        self.shake_from = now;
        if self.fails >= MAX_TRIES {
            self.fails = 0;
            self.wait_until = now + WAIT_TICKS;
            self.error = String::from("Too many attempts");
        } else {
            self.error = String::from("Wrong PIN. Try again.");
        }
        Outcome::Stay
    }

    fn digit(&mut self, d: char, now: u64) {
        if !self.waiting(now) && self.entry.len() < 8 {
            self.entry.push(d);
            self.error.clear();
        }
    }

    /// A click or tap on the lock screen.
    pub fn action(&mut self, a: u8, sys: &Sys, now: u64) -> Outcome {
        if !sys.has_pin() {
            return Outcome::Unlock;
        }
        match a {
            BACK => {
                self.entry.pop();
            }
            ENTER => return self.submit(sys, now),
            d if (DIGIT..DIGIT + 10).contains(&d) => {
                self.digit((b'0' + d - DIGIT) as char, now);
                // unlock as soon as a correct PIN is complete (tap-friendly)
                if self.entry.len() >= 4 && sys.check_pin(&self.entry) {
                    return self.submit(sys, now);
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn key(&mut self, k: Key, sys: &Sys, now: u64) -> Outcome {
        if !sys.has_pin() {
            return Outcome::Unlock;
        }
        match k {
            Key::Char(c) if c.is_ascii_digit() => {
                self.digit(c, now);
                if self.entry.len() >= 4 && sys.check_pin(&self.entry) {
                    return self.submit(sys, now);
                }
            }
            Key::Backspace => {
                self.entry.pop();
            }
            Key::Esc => self.entry.clear(),
            Key::Enter => return self.submit(sys, now),
            _ => {}
        }
        Outcome::Stay
    }

    /// Needs redrawing every tick (shake animation, lockout countdown)?
    pub fn animating(&self, now: u64) -> bool {
        now < self.shake_from + 40 || self.waiting(now)
    }

    pub fn render(&self, ui: &mut Ui, r: Rect, sys: &Sys, now: u64) {
        let t = ui.t;
        let tall = r.h > r.w;
        wallpaper::draw(ui.c, r.scale(ui.s), &t, tall);
        ui.zone(r, Action::Lock(TAP));
        let u = |v: i32| if tall { v * r.w / 390 } else { v };
        // status icons
        ui.icon(Icon::Battery, r.r() - u(40), r.y + u(14), u(18), t.text);
        ui.icon(Icon::Wifi, r.r() - u(66), r.y + u(14), u(16), if sys.net.ip.is_some() { t.text } else { t.text3 });
        // clock
        let (x, top) = if tall { (r.x + u(24), r.y + u(90)) } else { (r.x + r.w / 10, r.y + r.h * 22 / 100) };
        ui.text(x + 4, top, Face::Regular, u(20), &sys.date_long(), t.text2);
        ui.text(x, top + u(112), Face::Display, u(128), &sys.clock(), t.text);
        // up next
        let card_w = if tall { r.w - u(48) } else { 380 };
        let card = Rect::new(x, top + u(146), card_w, u(86));
        ui.rrect(card, u(24), t.surface.with_alpha(235));
        let ic = Rect::new(card.x + u(18), card.y + u(21), u(44), u(44));
        ui.rrect(ic, u(12), t.accent);
        ui.icon_in(Icon::Calendar, ic, u(20), t.on_accent);
        ui.label(card.x + u(76), card.y + u(28), u(11), "UP NEXT", t.accent);
        let (title, sub) = match sys.next_event() {
            Some(e) => (e.title.clone(), format!("{}{}{}", sys.event_when(e), if e.place.is_empty() { "" } else { " · " }, e.place)),
            None => (String::from("Nothing scheduled"), String::from("Enjoy your day")),
        };
        let tw = card.w - u(96);
        let title = ui.fit(Face::Semibold, u(16), &title, tw);
        let sub = ui.fit(Face::Regular, u(13), &sub, tw);
        ui.text(card.x + u(76), card.y + u(52), Face::Semibold, u(16), &title, t.text);
        ui.text(card.x + u(76), card.y + u(71), Face::Regular, u(13), &sub, t.text2);

        // phone notifications (titles only: bodies stay private while locked)
        let mut ny = card.b() + u(14);
        let nx = x;
        let nw = card_w;
        let unread = sys.link.unread();
        let mut rows: alloc::vec::Vec<(Icon, String, String)> = alloc::vec::Vec::new();
        if unread > 0 {
            rows.push((Icon::Chat, String::from("Messages"), format!("{} unread conversation{}", unread, if unread == 1 { "" } else { "s" })));
        }
        for n in sys.link.notifs.iter().take(3 - rows.len().min(3)) {
            rows.push((Icon::Bell, n.app.clone(), n.title.clone()));
        }
        let max_rows = ((r.b() - ny - if tall { u(360) } else { 40 }) / u(64)).clamp(0, 3) as usize;
        for (ic, app, line) in rows.iter().take(max_rows) {
            let row = Rect::new(nx, ny, nw, u(56));
            ui.rrect(row, u(18), t.surface.with_alpha(215));
            ui.icon(*ic, row.x + u(16), row.y + u(18), u(20), t.accent);
            let a = ui.fit(Face::Semibold, u(12), app, row.w - u(70));
            ui.text(row.x + u(50), row.y + u(24), Face::Semibold, u(12), &a, t.text);
            let l = ui.fit(Face::Regular, u(13), line, row.w - u(70));
            ui.text(row.x + u(50), row.y + u(42), Face::Regular, u(13), &l, t.text2);
            ny += u(64);
        }

        // unlock area
        let cx = r.x + r.w / 2;
        if !sys.has_pin() {
            let hint = if tall { "Tap to unlock" } else { "Click or press any key to unlock" };
            let fs = u(14);
            let hw = ui.tw(Face::Medium, fs, hint);
            let hy = r.b() - if tall { u(150) } else { 140 };
            let pill = Rect::new(cx - hw / 2 - u(20), hy - u(24), hw + u(40), u(36));
            ui.rrect(pill, u(18), t.surface.with_alpha(220));
            ui.text(cx - hw / 2, hy, Face::Medium, fs, hint, t.text);
            return;
        }
        // PIN entry: dots, message, keypad
        let shake = if now < self.shake_from + 40 {
            let k = (now - self.shake_from) as i32;
            sin_q14(k * 90) * (40 - k) / 40 * 10 / 16384
        } else {
            0
        };
        let keypad = tall || r.h >= 600;
        let key = if tall { u(64) } else { 52 };
        let gap = if tall { u(18) } else { 14 };
        let pad_h = if keypad { 4 * key + 3 * gap } else { 0 };
        // phones: bottom centre; desktops: right-hand column, vertically centred
        let cx = if tall { cx } else { r.r() - r.w / 10 - 150 };
        let base = if tall { r.b() - pad_h - u(120) } else { r.y + (r.h - pad_h) / 2 + 40 };
        let panel = Rect::new(cx - 150, base - 70, 300, pad_h + 110);
        if !tall {
            ui.rrect(panel, 26, t.surface.with_alpha(200));
        }
        let n = self.entry.len() as i32;
        let slots = n.max(4);
        let dx = cx - (slots * 22 - 8) / 2 + shake;
        for i in 0..slots {
            let (px, py) = (dx + i * 22 + 7, base - 34);
            if i < n {
                ui.circle(px, py, 7, t.text);
            } else {
                ui.circle(px, py, 7, t.text.with_alpha(60));
                ui.circle(px, py, 5, t.surface);
            }
        }
        let msg = if self.waiting(now) {
            format!("Too many attempts. Try again in {} s", (self.wait_until - now + 99) / 100)
        } else if !self.error.is_empty() {
            self.error.clone()
        } else {
            String::from("Enter your PIN")
        };
        let col = if self.error.is_empty() && !self.waiting(now) { t.text2 } else { t.danger };
        let mw = ui.tw(Face::Medium, 13, &msg);
        ui.text(cx - mw / 2, base - 4, Face::Medium, 13, &msg, col);
        if !keypad {
            return;
        }
        let labels = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", ""];
        let x0 = cx - (3 * key + 2 * gap) / 2;
        for (i, l) in labels.iter().enumerate() {
            let (col, row) = (i as i32 % 3, i as i32 / 3);
            let b = Rect::new(x0 + col * (key + gap), base + 14 + row * (key + gap), key, key);
            let a = match i {
                9 => Action::Lock(BACK),
                11 => Action::Lock(ENTER),
                _ => Action::Lock(DIGIT + l.as_bytes()[0] - b'0'),
            };
            let bg = if ui.hot(a) { t.surface } else { t.surface.with_alpha(170) };
            ui.circle(b.x + key / 2, b.y + key / 2, key / 2, bg);
            match i {
                9 => ui.icon_in(Icon::ChevronLeft, b, key * 3 / 8, t.text),
                11 => ui.icon_in(Icon::Check, b, key * 3 / 8, t.accent),
                _ => {
                    ui.text_in(b, Face::Semibold, if tall { u(24) } else { 20 }, l, t.text, 1);
                }
            }
            ui.zone(b, a);
        }
    }
}
