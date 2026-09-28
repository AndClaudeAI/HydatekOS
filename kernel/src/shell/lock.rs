//! Lock screen: clock, date, "Up next", phone notifications and sign-in with a
//! PIN, a password or the paired phone's fingerprint sensor. It keeps people
//! out of the session; it does not encrypt files.

use super::osk::{Osk, Typed};
use super::wallpaper;
use crate::font::Face;
use crate::gfx::{sin_q14, Rect};
use crate::icons::Icon;
use crate::sys::{Person, Sys};
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const TAP: u8 = 0;
pub const DIGIT: u8 = 1; // DIGIT + n for n in 0..=9
pub const BACK: u8 = 11;
pub use super::osk::{ENTER, KB_CHAR, KB_SHIFT, KB_SPECIAL};
pub const MODE_PIN: u8 = 20;
pub const MODE_PASSWORD: u8 = 21;
pub const MODE_FINGER: u8 = 22;
pub const RETRY: u8 = 23;
pub const FIELD: u8 = 24;
/// PERSON + n: choose account n (with several accounts)
pub const PERSON: u8 = 40;
// on-screen keyboard
pub const KB_TOGGLE: u8 = 30;

const MAX_TRIES: u32 = 5;
const WAIT_TICKS: u64 = 3000; // 30 s
const FINGER_TICKS: u64 = 6000; // the phone has 60 s to answer

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Pin,
    Password,
    Finger,
}

#[derive(Default)]
pub struct Lock {
    /// with several accounts: the one being signed in to (index in sys.people)
    pub who: usize,
    pub mode: Mode,
    pub entry: String,
    pub password: String,
    pub error: String,
    fails: u32,
    wait_until: u64,
    shake_from: u64,
    /// pending fingerprint request: (id, tick sent)
    finger: Option<(String, u64)>,
    /// on-screen keyboard: shown, and its state
    osk: bool,
    kb: Osk,
}

pub enum Outcome {
    Stay,
    Unlock,
}

// ---- whose sign-in: with several accounts, the one chosen on the lock screen

/// The chosen account, when there are several (otherwise the signed-in one's
/// own settings are used).
fn person(sys: &Sys, who: usize) -> Option<&Person> {
    if sys.people.len() > 1 { sys.people.get(who) } else { None }
}

fn has_pin(sys: &Sys, who: usize) -> bool {
    person(sys, who).map_or(sys.has_pin(), |p| p.has_pin())
}

fn has_password(sys: &Sys, who: usize) -> bool {
    person(sys, who).map_or(sys.has_password(), |p| p.has_password())
}

fn secured(sys: &Sys, who: usize) -> bool {
    person(sys, who).map_or(sys.secured(), |p| p.secured())
}

fn finger(sys: &Sys, who: usize) -> bool {
    person(sys, who).map_or(sys.lock_finger, |p| p.finger)
}

fn check_pin(sys: &Sys, who: usize, pin: &str) -> bool {
    person(sys, who).map_or_else(|| sys.check_pin(pin), |p| p.check_pin(pin))
}

fn check_password(sys: &Sys, who: usize, pw: &str) -> bool {
    person(sys, who).map_or_else(|| sys.check_password(pw), |p| p.check_password(pw))
}

fn touch(sys: &Sys) -> bool {
    sys.screen.1 > sys.screen.0
}

fn methods(sys: &Sys, who: usize) -> Vec<Mode> {
    let mut m = Vec::new();
    if has_pin(sys, who) {
        m.push(Mode::Pin);
    }
    if has_password(sys, who) {
        m.push(Mode::Password);
    }
    if finger(sys, who) && secured(sys, who) {
        m.push(Mode::Finger);
    }
    m
}

impl Lock {
    /// Start on the signed-in account (called on every lock).
    pub fn select_current(&mut self, sys: &Sys) {
        self.who = sys.people.iter().position(|p| p.id == sys.user).unwrap_or(0);
    }

    /// The account chosen to sign in to, when there are several.
    pub fn chosen<'a>(&self, sys: &'a Sys) -> Option<&'a str> {
        person(sys, self.who).map(|p| p.id.as_str())
    }

    /// Choose another account: whatever was typed is forgotten.
    fn choose(&mut self, i: usize, sys: &mut Sys) {
        if i < sys.people.len() && i != self.who {
            self.cancel_finger(sys);
            self.who = i;
            self.reset(sys);
        }
    }

    /// Clear what was typed; also called on every lock. Starts on the PIN if
    /// there is one, otherwise on the password.
    pub fn reset(&mut self, sys: &Sys) {
        self.entry.clear();
        self.password.clear();
        self.error.clear();
        self.finger = None;
        self.mode = if has_pin(sys, self.who) { Mode::Pin } else { Mode::Password };
        self.kb.reset();
        // touch-first (portrait) screens open the keyboard with the password
        self.osk = self.mode == Mode::Password && touch(sys);
    }

    fn waiting(&self, now: u64) -> bool {
        now < self.wait_until
    }

    fn failed(&mut self, now: u64, what: &str) {
        self.fails += 1;
        self.shake_from = now;
        if self.fails >= MAX_TRIES {
            self.fails = 0;
            self.wait_until = now + WAIT_TICKS;
            self.error = String::from("Too many attempts");
        } else {
            self.error = format!("Wrong {}. Try again.", what);
        }
    }

    fn submit(&mut self, sys: &Sys, now: u64) -> Outcome {
        if self.waiting(now) {
            return Outcome::Stay;
        }
        match self.mode {
            Mode::Pin if !self.entry.is_empty() => {
                if check_pin(sys, self.who, &self.entry) {
                    return self.unlocked(sys);
                }
                self.entry.clear();
                self.failed(now, "PIN");
            }
            Mode::Password if !self.password.is_empty() => {
                if check_password(sys, self.who, &self.password) {
                    return self.unlocked(sys);
                }
                self.password.clear();
                self.failed(now, "password");
            }
            _ => {}
        }
        Outcome::Stay
    }

    fn unlocked(&mut self, sys: &Sys) -> Outcome {
        self.fails = 0;
        self.reset(sys);
        Outcome::Unlock
    }

    fn digit(&mut self, d: char, now: u64) {
        if !self.waiting(now) && self.entry.len() < 8 {
            self.entry.push(d);
            self.error.clear();
        }
    }

    fn type_char(&mut self, c: char, now: u64) {
        if !self.waiting(now) && self.password.chars().count() < 64 {
            self.password.push(c);
            self.error.clear();
        }
    }

    /// A key on the on-screen keyboard.
    fn osk_key(&mut self, a: u8, now: u64) -> Outcome {
        match self.kb.press(a, now) {
            Typed::Char(c) => self.type_char(c, now),
            Typed::Back => {
                self.password.pop();
            }
            Typed::Enter | Typed::None => {}
        }
        Outcome::Stay
    }

    fn set_mode(&mut self, m: Mode, sys: &mut Sys, now: u64) {
        if !methods(sys, self.who).contains(&m) {
            return;
        }
        if self.mode == Mode::Finger && m != Mode::Finger {
            self.cancel_finger(sys);
        }
        if m == Mode::Password && self.mode != Mode::Password {
            self.osk = touch(sys);
        }
        self.mode = m;
        self.error.clear();
        if m == Mode::Finger && self.finger.is_none() {
            self.ask_phone(sys, now);
        }
    }

    /// Send a fingerprint request to the paired phone.
    fn ask_phone(&mut self, sys: &mut Sys, now: u64) {
        self.error.clear();
        if !(finger(sys, self.who) && sys.phone_can_confirm()) {
            return;
        }
        let mut id = [0u8; 8];
        crate::rng::fill(&mut id);
        let id = crate::crypto::hex(&id);
        if sys.link.request_unlock(&id) {
            self.finger = Some((id, now));
        }
    }

    /// Withdraw a pending fingerprint request (the phone closes its prompt).
    pub fn cancel_finger(&mut self, sys: &mut Sys) {
        if let Some((id, _)) = self.finger.take() {
            sys.link.cancel_unlock(&id);
        }
    }

    /// The phone answered a fingerprint request.
    pub fn phone_answer(&mut self, id: &str, ok: bool, sys: &Sys, now: u64) -> Outcome {
        let Some((want, sent)) = &self.finger else { return Outcome::Stay };
        if want != id || now > sent + FINGER_TICKS || !finger(sys, self.who) {
            return Outcome::Stay;
        }
        self.finger = None;
        if ok {
            return self.unlocked(sys);
        }
        self.error = String::from("Not confirmed on your phone");
        Outcome::Stay
    }

    /// Per-tick housekeeping: give up on a phone that doesn't answer.
    pub fn tick(&mut self, sys: &mut Sys, now: u64) -> bool {
        if let Some((_, sent)) = &self.finger {
            if now > sent + FINGER_TICKS || !sys.link.online {
                self.cancel_finger(sys);
                self.error = String::from("Your phone didn't answer");
                return true;
            }
        }
        false
    }

    /// A click or tap on the lock screen.
    pub fn action(&mut self, a: u8, sys: &mut Sys, now: u64) -> Outcome {
        if (PERSON..PERSON + crate::accounts::MAX as u8).contains(&a) {
            self.choose((a - PERSON) as usize, sys);
            return Outcome::Stay;
        }
        if !secured(sys, self.who) {
            return Outcome::Unlock;
        }
        match a {
            MODE_PIN => self.set_mode(Mode::Pin, sys, now),
            MODE_PASSWORD => self.set_mode(Mode::Password, sys, now),
            MODE_FINGER => self.set_mode(Mode::Finger, sys, now),
            RETRY => self.ask_phone(sys, now),
            KB_TOGGLE | FIELD if self.mode == Mode::Password => {
                // the field opens the keyboard; the keyboard button toggles it
                self.osk = if a == FIELD { true } else { !self.osk };
            }
            a if (KB_SHIFT..=KB_SPECIAL).contains(&a) || a >= KB_CHAR => {
                if self.mode == Mode::Password && self.osk {
                    return self.osk_key(a, now);
                }
            }
            BACK => {
                self.entry.pop();
            }
            ENTER => return self.submit(sys, now),
            d if (DIGIT..DIGIT + 10).contains(&d) && self.mode == Mode::Pin => {
                self.digit((b'0' + d - DIGIT) as char, now);
                // unlock as soon as a correct PIN is complete (tap-friendly)
                if self.entry.len() >= 4 && check_pin(sys, self.who, &self.entry) {
                    return self.submit(sys, now);
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn key(&mut self, k: Key, sys: &mut Sys, now: u64) -> Outcome {
        // with several accounts, ← and → choose (except while typing a password)
        let n = sys.people.len();
        if n > 1 && (self.mode != Mode::Password || self.password.is_empty() || !secured(sys, self.who)) {
            match k {
                Key::Left => return self.choose_step(n - 1, sys),
                Key::Right => return self.choose_step(1, sys),
                _ => {}
            }
        }
        if !secured(sys, self.who) {
            return Outcome::Unlock;
        }
        // typing picks a method: digits the PIN, anything else the password
        // (once in the password, digits stay there)
        if let Key::Char(c) = k {
            if self.mode != Mode::Password {
                let want = if c.is_ascii_digit() && has_pin(sys, self.who) { Mode::Pin } else { Mode::Password };
                if self.mode != want && methods(sys, self.who).contains(&want) {
                    self.set_mode(want, sys, now);
                }
            }
        }
        match self.mode {
            Mode::Pin => match k {
                Key::Char(c) if c.is_ascii_digit() => {
                    self.digit(c, now);
                    if self.entry.len() >= 4 && check_pin(sys, self.who, &self.entry) {
                        return self.submit(sys, now);
                    }
                }
                Key::Backspace => {
                    self.entry.pop();
                }
                Key::Esc => self.entry.clear(),
                Key::Enter => return self.submit(sys, now),
                _ => {}
            },
            Mode::Password => match k {
                Key::Char(c) if !c.is_control() => self.type_char(c, now),
                Key::Backspace => {
                    self.password.pop();
                }
                Key::Esc => self.password.clear(),
                Key::Enter => return self.submit(sys, now),
                _ => {}
            },
            Mode::Finger => {
                if let Key::Enter = k {
                    if self.finger.is_none() {
                        self.ask_phone(sys, now);
                    }
                }
            }
        }
        Outcome::Stay
    }

    fn choose_step(&mut self, by: usize, sys: &mut Sys) -> Outcome {
        let n = sys.people.len();
        self.choose((self.who + by) % n, sys);
        Outcome::Stay
    }

    /// Needs redrawing every tick (shake, lockout countdown, caret, waiting)?
    pub fn animating(&self, now: u64) -> bool {
        now < self.shake_from + 40 || self.waiting(now) || self.mode == Mode::Password || self.finger.is_some()
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
        let secured = secured(sys, self.who);
        let methods = methods(sys, self.who);
        let mode = if methods.contains(&self.mode) { self.mode } else { *methods.first().unwrap_or(&Mode::Pin) };
        let keypad = mode == Mode::Pin && (tall || r.h >= 600);
        let key = if tall { u(54) } else { 52 };
        let gap = if tall { u(12) } else { 14 };
        let pad_h = if keypad { 4 * key + 3 * gap } else { 0 };
        // the clock shrinks a little when the keypad needs the room
        let osk = mode == Mode::Password && self.osk;
        let kb = if osk { Some(self.kb.layout(r)) } else { None };
        let kb_h = kb.as_ref().map(|k| k.0.h).unwrap_or(0);
        let cs = match (keypad || osk, tall) {
            (true, true) => u(80),
            (true, false) if r.h - kb_h < 760 => 100,
            _ => u(128),
        };
        // one centred column: date, clock, Up next, who's signing in, sign-in,
        // sign-in options
        let many = sys.people.len() > 1;
        let pd = u(if tall { 44 } else { 52 });
        let user_h = if many {
            pd + u(46)
        } else if sys.profile.ready() {
            u(if tall { 58 } else { 64 })
        } else {
            0
        };
        let head_h = u(20) + u(16) + cs * 3 / 4 + u(34) + u(86) + user_h;
        let body_h = match mode {
            _ if !secured => 0,
            Mode::Pin => u(36) + u(28) + u(20) + pad_h,
            Mode::Password => u(28) + u(48) + u(44),
            Mode::Finger => u(28) + u(84) + u(84),
        };
        let opts_h = if secured && methods.len() > 1 { u(22) + u(44) } else { 0 };
        let cx = r.x + r.w / 2;
        let top = if secured {
            r.y + ((r.h - kb_h - head_h - body_h - opts_h) / 2).max(u(40)) + u(20)
        } else {
            r.y + ((r.h - head_h) / 2 - r.h / 10).max(u(40)) + u(20)
        };
        let date = sys.date_long();
        let dw = ui.tw(Face::Regular, u(20), &date);
        ui.text(cx - dw / 2, top, Face::Regular, u(20), &date, t.text2);
        let clock = sys.clock();
        let clock_base = top + u(16) + cs * 3 / 4;
        let cw = ui.tw(Face::Display, cs, &clock);
        ui.text(cx - cw / 2, clock_base, Face::Display, cs, &clock, t.text);
        // up next
        let card_w = if tall { r.w - u(48) } else { 420 };
        let card = Rect::new(cx - card_w / 2, clock_base + u(34), card_w, u(86));
        ui.rrect(card, u(24), t.surface.with_alpha(235));
        let ic = Rect::new(card.x + u(18), card.y + u(21), u(44), u(44));
        ui.rrect(ic, u(12), t.accent);
        // another account chosen: nothing of the signed-in person's shows
        let other = person(sys, self.who).map_or(false, |p| p.id != sys.user);
        ui.icon_in(if other { Icon::User } else { Icon::Calendar }, ic, u(20), t.on_accent);
        ui.label(card.x + u(76), card.y + u(28), u(11), if other { "SIGN IN" } else { "UP NEXT" }, t.accent);
        let (title, sub) = match sys.next_event() {
            _ if other => (String::from("Choose your account"), String::from(if tall { "Tap your picture below" } else { "Click your picture, or use ← and →" })),
            Some(e) => (e.title.clone(), format!("{}{}{}", sys.event_when(e), if e.place.is_empty() { "" } else { " · " }, e.place)),
            None => (String::from("Nothing scheduled"), String::from("Enjoy your day")),
        };
        let tw = card.w - u(96);
        let title = ui.fit(Face::Semibold, u(16), &title, tw);
        let sub = ui.fit(Face::Regular, u(13), &sub, tw);
        ui.text(card.x + u(76), card.y + u(52), Face::Semibold, u(16), &title, t.text);
        ui.text(card.x + u(76), card.y + u(71), Face::Regular, u(13), &sub, t.text2);

        if many {
            // everyone who uses this computer; the chosen one is ringed
            let n = sys.people.len() as i32;
            let iw = u(88).min((r.w - u(24)) / n);
            let x0 = cx - iw * n / 2;
            let y = card.b() + u(16);
            for (i, p) in sys.people.iter().enumerate() {
                let c = x0 + i as i32 * iw + iw / 2;
                let a = Action::Lock(PERSON + i as u8);
                let on = i == self.who;
                if on || ui.hot(a) {
                    ui.circle(c, y + pd / 2, pd / 2 + u(4), if on { t.accent } else { t.surface });
                }
                ui.circle(c, y + pd / 2, pd / 2 + u(2), t.surface.with_alpha(235));
                ui.avatar(Rect::new(c - pd / 2, y, pd, pd), &p.avatar);
                let face = if on { Face::Semibold } else { Face::Medium };
                let name = ui.fit(face, u(13), crate::profile::first_name(&p.name), iw - u(8));
                let nw = ui.tw(face, u(13), &name);
                ui.text(c - nw / 2, y + pd + u(20), face, u(13), &name, if on { t.text } else { t.text2 });
                ui.zone(Rect::new(c - iw / 2, y - u(4), iw, pd + u(30)), a);
            }
        } else if user_h > 0 {
            // the profile picture and name
            let d = u(if tall { 38 } else { 44 });
            let name = ui.fit(Face::Semibold, u(17), &sys.profile.name, card.w - d - u(20));
            let nw = ui.tw(Face::Semibold, u(17), &name);
            let row_w = d + u(12) + nw;
            let x = cx - row_w / 2;
            let y = card.b() + (user_h - d) / 2 + u(4);
            ui.circle(x + d / 2, y + d / 2, d / 2 + u(2), t.surface.with_alpha(235));
            ui.avatar(Rect::new(x, y, d, d), &sys.avatar);
            ui.text(x + d + u(12), y + d / 2 + u(6), Face::Semibold, u(17), &name, t.text);
        }
        let card = Rect::new(card.x, card.y, card.w, card.h + user_h);
        if !secured {
            self.render_open(ui, r, sys, card, tall);
            return;
        }
        let body = card.b();
        match mode {
            Mode::Pin => self.render_pin(ui, r, cx, body, (keypad, key, gap), now),
            Mode::Password => self.render_password(ui, r, cx, body, now),
            Mode::Finger => self.render_finger(ui, r, cx, body, sys, now),
        }
        if opts_h > 0 {
            // sign-in options: one round button per method
            let size = u(44);
            let sp = u(16);
            let n = methods.len() as i32;
            let y = body + body_h + u(22);
            let x0 = cx - (n * size + (n - 1) * sp) / 2;
            for (i, m) in methods.iter().enumerate() {
                let (icon, a) = match m {
                    Mode::Pin => (Icon::Keypad, MODE_PIN),
                    Mode::Password => (Icon::Key, MODE_PASSWORD),
                    Mode::Finger => (Icon::Fingerprint, MODE_FINGER),
                };
                let b = Rect::new(x0 + i as i32 * (size + sp), y, size, size);
                let act = Action::Lock(a);
                let on = *m == mode;
                let bg = if on { t.accent } else if ui.hot(act) { t.surface } else { t.surface.with_alpha(205) };
                ui.circle(b.x + size / 2, b.y + size / 2, size / 2, bg);
                ui.icon_in(icon, b, u(20), if on { t.on_accent } else { t.text });
                ui.zone(b, act);
            }
        }
        if let Some((panel, keys)) = kb {
            self.kb.render(ui, panel, &keys, tall, Action::Lock, Action::Lock(FIELD));
        }
    }

    /// No PIN or password: notifications and a hint.
    fn render_open(&self, ui: &mut Ui, r: Rect, sys: &Sys, card: Rect, tall: bool) {
        let t = ui.t;
        let u = |v: i32| if tall { v * r.w / 390 } else { v };
        let cx = r.x + r.w / 2;
        // phone notifications (titles only: bodies stay private while locked)
        let mut ny = card.b() + u(14);
        // (not while another account is chosen)
        let other = person(sys, self.who).map_or(false, |p| p.id != sys.user);
        let unread = if other { 0 } else { sys.link.unread() };
        let mut rows: Vec<(Icon, String, String)> = Vec::new();
        if unread > 0 {
            rows.push((Icon::Chat, String::from("Messages"), format!("{} unread conversation{}", unread, if unread == 1 { "" } else { "s" })));
        }
        for n in sys.link.notifs.iter().take(if other { 0 } else { 3 - rows.len().min(3) }) {
            rows.push((Icon::Bell, n.app.clone(), n.title.clone()));
        }
        let hy = r.b() - if tall { u(150) } else { 140 };
        let max_rows = ((hy - u(40) - ny) / u(64)).clamp(0, 3) as usize;
        for (ic, app, line) in rows.iter().take(max_rows) {
            let row = Rect::new(card.x, ny, card.w, u(56));
            ui.rrect(row, u(18), t.surface.with_alpha(215));
            ui.icon(*ic, row.x + u(16), row.y + u(18), u(20), t.accent);
            let a = ui.fit(Face::Semibold, u(12), app, row.w - u(70));
            ui.text(row.x + u(50), row.y + u(24), Face::Semibold, u(12), &a, t.text);
            let l = ui.fit(Face::Regular, u(13), line, row.w - u(70));
            ui.text(row.x + u(50), row.y + u(42), Face::Regular, u(13), &l, t.text2);
            ny += u(64);
        }
        let signin;
        let hint = match person(sys, self.who) {
            Some(p) => {
                signin = format!("{} to sign in as {}", if tall { "Tap" } else { "Click or press Enter" }, crate::profile::first_name(&p.name));
                signin.as_str()
            }
            None if tall => "Tap to unlock",
            None => "Click or press any key to unlock",
        };
        let fs = u(14);
        let hw = ui.tw(Face::Medium, fs, hint);
        let pill = Rect::new(cx - hw / 2 - u(20), hy - u(24), hw + u(40), u(36));
        ui.rrect(pill, u(18), t.surface.with_alpha(220));
        ui.text(cx - hw / 2, hy, Face::Medium, fs, hint, t.text);
    }

    fn shake(&self, now: u64) -> i32 {
        if now < self.shake_from + 40 {
            let k = (now - self.shake_from) as i32;
            sin_q14(k * 90) * (40 - k) / 40 * 10 / 16384
        } else {
            0
        }
    }

    /// The status line under the entry: lockout countdown, error or prompt.
    fn message(&self, ui: &mut Ui, cx: i32, y: i32, size: i32, prompt: &str, now: u64) {
        let t = ui.t;
        let msg = if self.waiting(now) {
            format!("Too many attempts. Try again in {} s", (self.wait_until - now + 99) / 100)
        } else if !self.error.is_empty() {
            self.error.clone()
        } else {
            String::from(prompt)
        };
        let col = if self.error.is_empty() && !self.waiting(now) { t.text2 } else { t.danger };
        let mw = ui.tw(Face::Medium, size, &msg);
        ui.text(cx - mw / 2, y, Face::Medium, size, &msg, col);
    }

    fn render_pin(&self, ui: &mut Ui, r: Rect, cx: i32, body: i32, (keypad, key, gap): (bool, i32, i32), now: u64) {
        let t = ui.t;
        let tall = r.h > r.w;
        let u = |v: i32| if tall { v * r.w / 390 } else { v };
        let dots_y = body + u(36);
        let msg_y = dots_y + u(28);
        let n = self.entry.len() as i32;
        let slots = n.max(4);
        let (dr, dp) = (u(7), u(22));
        let dx = cx - (slots * dp - (dp - 2 * dr)) / 2 + self.shake(now);
        for i in 0..slots {
            let (px, py) = (dx + i * dp + dr, dots_y);
            if i < n {
                ui.circle(px, py, dr, t.text);
            } else {
                ui.circle(px, py, dr, t.text.with_alpha(60));
                ui.circle(px, py, dr - u(2), t.surface);
            }
        }
        self.message(ui, cx, msg_y, u(13), if keypad { "Enter your PIN" } else { "Type your PIN and press Enter" }, now);
        if !keypad {
            return;
        }
        let base = msg_y + u(20);
        let labels = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", ""];
        let x0 = cx - (3 * key + 2 * gap) / 2;
        for (i, l) in labels.iter().enumerate() {
            let (col, row) = (i as i32 % 3, i as i32 / 3);
            let b = Rect::new(x0 + col * (key + gap), base + row * (key + gap), key, key);
            let a = match i {
                9 => Action::Lock(BACK),
                11 => Action::Lock(ENTER),
                _ => Action::Lock(DIGIT + l.as_bytes()[0] - b'0'),
            };
            let bg = if ui.hot(a) { t.surface } else { t.surface.with_alpha(205) };
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

    fn render_password(&self, ui: &mut Ui, r: Rect, cx: i32, body: i32, now: u64) {
        let t = ui.t;
        let tall = r.h > r.w;
        let u = |v: i32| if tall { v * r.w / 390 } else { v };
        let w = if tall { r.w - u(96) } else { 320 };
        let h = u(48);
        let f = Rect::new(cx - w / 2 + self.shake(now), body + u(28), w, h);
        ui.rrect(f, h / 2, t.surface.with_alpha(235));
        ui.stroke(f, h / 2, 1, t.accent.with_alpha(160));
        ui.zone(f, Action::Lock(FIELD));
        // masked characters as dots
        let go = Rect::new(f.r() - h + u(6), f.y + u(6), h - u(12), h - u(12));
        let kbd = Rect::new(go.x - go.w - u(4), go.y, go.w, go.h);
        let ka = Action::Lock(KB_TOGGLE);
        if self.osk || ui.hot(ka) {
            ui.circle(kbd.x + kbd.w / 2, kbd.y + kbd.h / 2, kbd.w / 2, t.hover);
        }
        ui.icon_in(Icon::Keyboard, kbd, kbd.w / 2 + u(2), if self.osk { t.accent } else { t.text2 });
        ui.zone(kbd, ka);
        let inner = Rect::new(f.x + u(20), f.y, kbd.x - f.x - u(28), h);
        let n = self.password.chars().count() as i32;
        let (dr, dp) = (u(4), u(14));
        let caret = (now / 50) % 2 == 0;
        if n == 0 {
            ui.text_in(inner, Face::Regular, u(14), "Password", t.text3, 0);
            if caret {
                ui.rect(Rect::new(inner.x, f.y + u(14), 1, h - u(28)), t.text);
            }
        } else {
            let shown = n.min((inner.w / dp).max(1));
            for i in 0..shown {
                ui.circle(inner.x + dr + i * dp, f.y + h / 2, dr, t.text);
            }
            if caret {
                ui.rect(Rect::new(inner.x + shown * dp + u(2), f.y + u(14), 1, h - u(28)), t.text);
            }
        }
        let a = Action::Lock(ENTER);
        ui.circle(go.x + go.w / 2, go.y + go.h / 2, go.w / 2, if ui.hot(a) { t.accent.mix(t.text, 30) } else { t.accent });
        ui.icon_in(Icon::ChevronRight, go, go.w / 2, t.on_accent);
        ui.zone(go, a);
        self.message(ui, cx, f.b() + u(30), u(13), "Enter your password", now);
    }

    fn render_finger(&self, ui: &mut Ui, r: Rect, cx: i32, body: i32, sys: &Sys, now: u64) {
        let t = ui.t;
        let tall = r.h > r.w;
        let u = |v: i32| if tall { v * r.w / 390 } else { v };
        let size = u(84);
        let cy = body + u(28) + size / 2;
        let pending = self.finger.is_some();
        // pulse while waiting for the phone
        if pending {
            let halo = u(4) + sin_q14((now % 120) as i32 * 1024 / 120).abs() * u(8) / 16384;
            ui.circle(cx, cy, size / 2 + halo, t.accent.with_alpha(60));
        }
        ui.circle(cx, cy, size / 2, t.surface.with_alpha(235));
        ui.icon_in(Icon::Fingerprint, Rect::new(cx - size / 2, cy - size / 2, size, size), u(44), if pending { t.accent } else { t.text });
        let phone = if sys.link.device.is_empty() { String::from("your phone") } else { sys.link.device.clone() };
        let (line1, line2) = if pending {
            (String::from("Check your phone"), format!("Touch the fingerprint sensor on {}", phone))
        } else if !self.error.is_empty() {
            (self.error.clone(), String::from("Tap the fingerprint to try again"))
        } else if !sys.link.online || sys.link.is_demo() {
            (String::from("Your phone isn't connected"), String::from("Open HydatekOS Link on your phone"))
        } else if !(finger(sys, self.who) && sys.phone_can_confirm()) {
            (format!("{} can't confirm fingerprints", phone), String::from("Set up a fingerprint on the phone first"))
        } else {
            (String::from("Unlock with your phone"), String::from("Tap the fingerprint to send a request"))
        };
        let col = if self.error.is_empty() || pending { t.text } else { t.danger };
        let y1 = cy + size / 2 + u(30);
        let l1 = ui.fit(Face::Semibold, u(15), &line1, r.w - u(48));
        let w1 = ui.tw(Face::Semibold, u(15), &l1);
        ui.text(cx - w1 / 2, y1, Face::Semibold, u(15), &l1, col);
        let l2 = ui.fit(Face::Regular, u(13), &line2, r.w - u(48));
        let w2 = ui.tw(Face::Regular, u(13), &l2);
        ui.text(cx - w2 / 2, y1 + u(22), Face::Regular, u(13), &l2, t.text2);
        if !pending {
            ui.zone(Rect::new(cx - size / 2, cy - size / 2, size, size), Action::Lock(RETRY));
        }
    }
}
