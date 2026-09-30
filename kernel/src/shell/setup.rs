//! The setup assistant: shown the first time HydatekOS starts (and again
//! from Settings › Profile). It asks for the name of the person using the
//! computer, a picture, how they sign in, introduces Claude (HydatekOS's
//! assistant, made by Anthropic) and asks how the desktop looks.

use super::osk::{Osk, Typed};
use super::{logo, wallpaper};
use crate::avatar::{Choice, Picker};
use crate::font::Face;
use crate::gfx::{Canvas, Color, Rect};
use crate::icons::Icon;
use crate::profile::{self, Avatar, Profile};
use crate::sys::Sys;
use crate::theme::ACCENTS;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Welcome,
    Name,
    Picture,
    SignIn,
    /// Claude, the assistant: an API key, or later
    Assistant,
    Look,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    Pin,
    Password,
    Nothing,
}

pub enum Outcome {
    Stay,
    Finished,
}

// action codes
const NEXT: u16 = 1;
const BACK: u16 = 2;
const F_NAME: u16 = 3;
const F_SECRET: u16 = 4;
const F_CONFIRM: u16 = 5;
const M_PIN: u16 = 6;
const M_PASSWORD: u16 = 7;
const M_NOTHING: u16 = 8;
const LIGHT: u16 = 9;
const DARK: u16 = 10;
const KB_TOGGLE: u16 = 11;
const DYNAMIC: u16 = 12;
const SWALLOW: u16 = 13;
const LATER: u16 = 14;
const F_KEY: u16 = 15;
const SKIP_KEY: u16 = 16;
const ACCENT: u16 = 20;
const PICK: u16 = 100;
const OSK: u16 = 1000;

pub struct Setup {
    pub step: Step,
    name: String,
    choice: Choice,
    /// the name the colour of the initials was last picked for
    auto_colour: bool,
    picker: Picker,
    method: Method,
    secret: String,
    confirm: String,
    /// the Anthropic API key for Claude (optional)
    claude: String,
    /// focused field: F_NAME, F_SECRET, F_CONFIRM, F_KEY or 0
    field: u16,
    error: String,
    /// the computer already has a PIN or password: no sign-in step
    secured: bool,
    /// running again from Settings (can be left without changes)
    pub again: bool,
    /// a new account on a computer someone else set up
    joining: bool,
    kb: Osk,
    osk: bool,
    preview: Option<(Choice, String, Canvas)>,
}

fn touch(sys: &Sys) -> bool {
    sys.screen.1 > sys.screen.0
}

impl Setup {
    pub fn new(sys: &Sys) -> Setup {
        let p = &sys.profile;
        let choice = match p.avatar {
            Avatar::Initials(i) => Choice::Initials(i),
            Avatar::Motif(i) => Choice::Motif(i),
            Avatar::Picture => Choice::Photo(0),
        };
        let mut picker = Picker::default();
        if p.avatar == Avatar::Picture {
            // the current photo comes first
            picker.load(&sys.fs, sys.photo.as_deref());
        }
        Setup {
            step: Step::Welcome,
            name: p.name.clone(),
            choice,
            auto_colour: !p.ready(),
            picker,
            method: if touch(sys) { Method::Pin } else { Method::Password },
            secret: String::new(),
            confirm: String::new(),
            claude: String::new(),
            field: 0,
            error: String::new(),
            secured: sys.secured(),
            again: !sys.needs_setup(),
            joining: sys.accounts.list.len() > 1 && sys.needs_setup(),
            kb: Osk::default(),
            osk: touch(sys),
            preview: None,
        }
    }

    fn steps(&self) -> Vec<Step> {
        let mut v = alloc::vec![Step::Name, Step::Picture];
        if !self.secured {
            v.push(Step::SignIn);
        }
        v.push(Step::Assistant);
        v.push(Step::Look);
        v
    }

    fn go(&mut self, step: Step, sys: &Sys) {
        self.step = step;
        self.error.clear();
        self.field = match step {
            Step::Name => F_NAME,
            Step::SignIn if self.method != Method::Nothing => F_SECRET,
            Step::Assistant => F_KEY,
            _ => 0,
        };
        self.kb.reset();
        if step == Step::Name && self.name.is_empty() {
            self.kb.capitalise_next();
        }
        if step == Step::SignIn && self.method == Method::Pin {
            self.kb.press(super::osk::KB_PAGE, 0);
        }
        self.osk = touch(sys) && self.field != 0;
        if step == Step::Picture {
            self.picker.load(&sys.fs, None);
        }
    }

    fn next(&mut self, sys: &mut Sys) -> Outcome {
        match self.step {
            Step::Welcome => self.go(Step::Name, sys),
            Step::Name => match profile::clean_name(&self.name) {
                Some(n) => {
                    self.name = n;
                    if self.auto_colour {
                        if let Choice::Initials(_) = self.choice {
                            self.choice = Choice::Initials(profile::colour_for(&self.name));
                        }
                    }
                    self.go(Step::Picture, sys);
                }
                None => self.error = String::from("Type your name to continue"),
            },
            Step::Picture => self.go(if self.secured { Step::Assistant } else { Step::SignIn }, sys),
            Step::Assistant => {
                let k = self.claude.trim();
                if k.is_empty() || crate::web::claude::key_ok(k) {
                    self.go(Step::Look, sys);
                } else {
                    self.error = String::from("That isn't an Anthropic API key: they start with sk-ant-");
                }
            }
            Step::SignIn => {
                let too_short = match self.method {
                    Method::Pin if !Sys::pin_ok(&self.secret) => Some("Use 4 to 8 digits"),
                    Method::Password if !Sys::password_ok(&self.secret) => Some("Use 6 to 64 characters"),
                    _ => None,
                };
                if let Some(e) = too_short {
                    // fix the first box
                    self.error = String::from(e);
                    self.confirm.clear();
                    self.field = F_SECRET;
                } else if self.method != Method::Nothing && self.confirm != self.secret {
                    // type the second one again
                    self.error = String::from("The two don't match. Type it again.");
                    self.confirm.clear();
                    self.field = F_CONFIRM;
                } else {
                    self.go(Step::Assistant, sys);
                }
            }
            Step::Look => {
                self.apply(sys);
                self.go(Step::Done, sys);
            }
            Step::Done => return Outcome::Finished,
        }
        Outcome::Stay
    }

    fn back(&mut self, sys: &Sys) {
        let prev = match self.step {
            Step::Name => Step::Welcome,
            Step::Picture => Step::Name,
            Step::SignIn => Step::Picture,
            Step::Assistant if self.secured => Step::Picture,
            Step::Assistant => Step::SignIn,
            Step::Look => Step::Assistant,
            _ => return,
        };
        self.go(prev, sys);
    }

    /// Keep what was chosen: the profile, the sign-in secret, the look.
    fn apply(&mut self, sys: &mut Sys) {
        let avatar = match self.choice {
            Choice::Initials(i) => Avatar::Initials(i),
            Choice::Motif(i) => Avatar::Motif(i),
            Choice::Photo(i) => {
                sys.photo = self.picker.photos.get(i).map(|p| p.1.clone());
                if sys.photo.is_some() { Avatar::Picture } else { Avatar::Initials(profile::colour_for(&self.name)) }
            }
        };
        let since = sys.profile.since;
        sys.profile = Profile { name: self.name.clone(), avatar, since };
        sys.save_profile();
        sys.setup_done();
        if !self.secured {
            match self.method {
                Method::Pin => {
                    sys.set_pin(Some(&self.secret));
                }
                Method::Password => {
                    sys.set_password(Some(&self.secret));
                }
                Method::Nothing => {}
            }
        }
        self.secret.clear();
        self.confirm.clear();
        let key = self.claude.trim();
        if crate::web::claude::key_ok(key) {
            sys.claude_key = String::from(key);
            sys.save_assistant();
        }
        self.claude.clear();
        sys.save_settings();
    }

    fn set_method(&mut self, m: Method, sys: &Sys) {
        if self.method != m {
            self.method = m;
            self.secret.clear();
            self.confirm.clear();
        }
        self.go(Step::SignIn, sys);
    }

    fn type_char(&mut self, c: char) {
        self.error.clear();
        match self.field {
            F_NAME if self.name.chars().count() < profile::NAME_MAX && !c.is_control() => self.name.push(c),
            // keys are letters, digits, - and _
            F_KEY if self.claude.len() < 256 && c.is_ascii_graphic() => self.claude.push(c),
            F_SECRET | F_CONFIRM => {
                let (pin, max) = (self.method == Method::Pin, if self.method == Method::Pin { 8 } else { 64 });
                let s = if self.field == F_SECRET { &mut self.secret } else { &mut self.confirm };
                if s.chars().count() < max && !c.is_control() && (!pin || c.is_ascii_digit()) {
                    s.push(c);
                }
            }
            _ => {}
        }
    }

    fn backspace(&mut self) {
        match self.field {
            F_NAME => {
                self.name.pop();
            }
            F_SECRET => {
                self.secret.pop();
            }
            F_CONFIRM => {
                self.confirm.pop();
            }
            F_KEY => {
                self.claude.pop();
            }
            _ => {}
        }
    }

    /// Enter in a field: on to the next field, or the next step.
    fn enter(&mut self, sys: &mut Sys) -> Outcome {
        if self.field == F_SECRET && self.method != Method::Nothing {
            self.field = F_CONFIRM;
            return Outcome::Stay;
        }
        self.next(sys)
    }

    pub fn action(&mut self, code: u16, sys: &mut Sys, now: u64) -> Outcome {
        match code {
            NEXT => return self.next(sys),
            BACK => self.back(sys),
            LATER => return Outcome::Finished,
            SKIP_KEY => {
                self.claude.clear();
                return self.next(sys);
            }
            F_NAME | F_SECRET | F_CONFIRM | F_KEY => {
                self.field = code;
                self.osk = self.osk || touch(sys);
            }
            M_PIN => self.set_method(Method::Pin, sys),
            M_PASSWORD => self.set_method(Method::Password, sys),
            M_NOTHING => self.set_method(Method::Nothing, sys),
            LIGHT | DARK => {
                sys.set_dark(code == DARK);
                sys.save_settings();
            }
            KB_TOGGLE => self.osk = !self.osk,
            c if (ACCENT..ACCENT + ACCENTS.len() as u16).contains(&c) => {
                sys.accent = (c - ACCENT) as usize;
                sys.look.accent = crate::personal::Accent::Preset(sys.accent as u8);
                sys.save_settings();
            }
            DYNAMIC => {
                sys.look.accent = crate::personal::Accent::FromWall;
                sys.save_settings();
            }
            c if c >= OSK => match self.kb.press((c - OSK) as u8, now) {
                Typed::Char(ch) => self.type_char(ch),
                Typed::Back => self.backspace(),
                Typed::Enter => return self.enter(sys),
                Typed::None => {}
            },
            c if c >= PICK => {
                self.choice = Picker::choice(c - PICK);
                self.auto_colour = false;
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn key(&mut self, k: Key, gen: bool, sys: &mut Sys) -> Outcome {
        match k {
            // Gen+V pastes (an API key is long to type)
            Key::Char('v') if gen => {
                let clip = sys.clipboard.clone();
                for c in clip.trim().chars() {
                    self.type_char(c);
                }
            }
            Key::Char(_) if gen => {}
            Key::Enter => return self.enter(sys),
            Key::Esc => self.back(sys),
            Key::Tab if self.step == Step::SignIn && self.method != Method::Nothing => {
                self.field = if self.field == F_SECRET { F_CONFIRM } else { F_SECRET };
            }
            Key::Backspace => self.backspace(),
            Key::Char(c) => {
                self.type_char(c);
                self.kb.typed();
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// Something needs fixing (for feedback).
    pub fn has_error(&self) -> bool {
        !self.error.is_empty()
    }

    /// Redraw for the caret?
    pub fn animating(&self) -> bool {
        self.field != 0
    }

    // ---- drawing -------------------------------------------------------------

    fn preview(&mut self) -> &Canvas {
        let key = (self.choice, self.name.clone());
        if self.preview.as_ref().map(|p| (p.0, p.1.clone())) != Some(key.clone()) {
            let (avatar, photo) = match self.choice {
                Choice::Initials(i) => (Avatar::Initials(i), None),
                Choice::Motif(i) => (Avatar::Motif(i), None),
                Choice::Photo(i) => (Avatar::Picture, self.picker.photos.get(i).map(|p| p.1.as_slice())),
            };
            let c = crate::avatar::render(avatar, &self.name, photo, crate::sys::AVATAR_PX);
            self.preview = Some((key.0, key.1, c));
        }
        &self.preview.as_ref().unwrap().2
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys) {
        let t = ui.t;
        let tall = r.h > r.w;
        wallpaper::draw(ui.c, r.scale(ui.s), &t, tall);
        ui.zone(r, Action::Setup(SWALLOW));
        let u = |v: i32| if tall { v * r.w / 390 } else { v };
        let kb = if self.osk && self.field != 0 { Some(self.kb.layout(r)) } else { None };
        let kb_h = kb.as_ref().map(|k| k.0.h + u(8)).unwrap_or(0);
        let card = if tall {
            Rect::new(r.x + u(12), r.y + u(44), r.w - u(24), r.h - u(44) - u(12) - kb_h)
        } else {
            let (w, h) = (640.min(r.w - 32), 580.min(r.h - 32 - kb_h));
            Rect::new(r.x + (r.w - w) / 2, r.y + (r.h - kb_h - h) / 2, w, h)
        };
        ui.shadow(card, u(28), u(24), u(8), 70);
        ui.rrect(card, u(28), t.surface.with_alpha(248));
        ui.zone(card, Action::Setup(SWALLOW));
        let pad = u(if tall { 22 } else { 44 });
        let inner = Rect::new(card.x + pad, card.y + pad, card.w - 2 * pad, card.h - 2 * pad);
        // progress: one dot per step
        let steps = self.steps();
        if let Some(pos) = steps.iter().position(|s| *s == self.step) {
            let n = steps.len() as i32;
            let (dw, gap) = (u(22), u(6));
            let x0 = card.x + (card.w - (n * dw + (n - 1) * gap)) / 2;
            for i in 0..n {
                let col = if i as usize <= pos { t.accent } else { t.line };
                ui.rrect(Rect::new(x0 + i * (dw + gap), card.y + u(20), dw, u(4)), u(2), col);
            }
        }
        let foot_h = u(44);
        let foot = Rect::new(inner.x, inner.b() - foot_h, inner.w, foot_h);
        let body = Rect::new(inner.x, inner.y + u(10), inner.w, foot.y - inner.y - u(10) - u(12));
        match self.step {
            Step::Welcome => self.render_welcome(ui, body, foot, sys, tall),
            Step::Name => self.render_name(ui, body, foot, tall),
            Step::Picture => self.render_picture(ui, body, foot, tall),
            Step::SignIn => self.render_signin(ui, body, foot, tall),
            Step::Assistant => self.render_assistant(ui, body, foot, tall),
            Step::Look => self.render_look(ui, body, foot, sys, tall),
            Step::Done => self.render_done(ui, body, foot, sys, tall),
        }
        if let Some((panel, keys)) = kb {
            self.kb.render(ui, panel, &keys, tall, |c| Action::Setup(OSK + c as u16), Action::Setup(SWALLOW));
        }
    }

    fn u(tall: bool, r: Rect) -> impl Fn(i32) -> i32 {
        // body widths: the phone card is about 300 points wide
        let k = if tall { r.w.max(1) } else { 0 };
        move |v: i32| if k > 0 { v * k / 300 } else { v }
    }

    /// A heading and a wrapped line under it; returns the y after them.
    fn heading(ui: &mut Ui, r: Rect, title: &str, sub: &str, tall: bool, centre: bool) -> i32 {
        let t = ui.t;
        let u = Setup::u(tall, r);
        let (ts, ss) = (u(if tall { 22 } else { 26 }), u(if tall { 13 } else { 14 }));
        let mut y = r.y + ts;
        let tw = ui.tw(Face::Semibold, ts, title);
        let tx = if centre { r.x + (r.w - tw) / 2 } else { r.x };
        ui.text(tx, y, Face::Semibold, ts, title, t.text);
        y += u(12);
        for line in ui.wrap(Face::Regular, ss, sub, r.w) {
            y += ss + u(6);
            let lw = ui.tw(Face::Regular, ss, &line);
            ui.text(if centre { r.x + (r.w - lw) / 2 } else { r.x }, y, Face::Regular, ss, &line, t.text2);
        }
        y + u(10)
    }

    /// Back on the left (from the second step), `label` on the right.
    fn footer(&self, ui: &mut Ui, foot: Rect, label: &str, back: bool, tall: bool) {
        let t = ui.t;
        let u = Setup::u(tall, foot);
        let nw = (ui.tw(Face::Semibold, u(14), label) + u(44)).max(u(120));
        let nr = Rect::new(foot.r() - nw, foot.y, nw, foot.h);
        let a = Action::Setup(NEXT);
        ui.rrect(nr, foot.h / 2, if ui.hot(a) { t.accent.mix(t.text, 30) } else { t.accent });
        ui.text_in(nr, Face::Semibold, u(14), label, t.on_accent, 1);
        ui.zone(nr, a);
        if back {
            let br = Rect::new(foot.x, foot.y, u(100), foot.h);
            let a = Action::Setup(BACK);
            ui.rrect(br, foot.h / 2, if ui.hot(a) { t.chip.mix(t.text, 20) } else { t.chip });
            ui.icon(Icon::ChevronLeft, br.x + u(16), br.y + (br.h - u(16)) / 2, u(16), t.text);
            ui.text_in(Rect::new(br.x + u(36), br.y, br.w - u(44), br.h), Face::Semibold, u(14), "Back", t.text, 0);
            ui.zone(br, a);
        }
    }

    fn error_line(&self, ui: &mut Ui, x: i32, y: i32, w: i32, size: i32) {
        if !self.error.is_empty() {
            let e = ui.fit(Face::Medium, size, &self.error, w);
            let t = ui.t;
            ui.text(x, y, Face::Medium, size, &e, t.danger);
        }
    }

    /// A text field; `masked` shows dots.
    fn field(&self, ui: &mut Ui, f: Rect, value: &str, placeholder: &str, masked: bool, code: u16, tall: bool) {
        let t = ui.t;
        let u = Setup::u(tall, f);
        let focused = self.field == code;
        ui.rrect(f, u(12), t.chip);
        ui.stroke(f, u(12), 1, if focused { t.accent } else { t.line });
        let inner = Rect::new(f.x + u(14), f.y, f.w - u(28), f.h);
        let size = u(15);
        let shown: String = if masked { value.chars().map(|_| '•').collect() } else { String::from(value) };
        let old = ui.clip_in(inner);
        let w = if shown.is_empty() {
            ui.text_in(inner, Face::Regular, size, placeholder, t.text3, 0);
            0
        } else {
            // keep the end in view
            let full = ui.tw(Face::Regular, size, &shown);
            let x = if full > inner.w - u(4) { inner.r() - u(4) - full } else { inner.x };
            ui.text_in(Rect::new(x, inner.y, full + u(4), inner.h), Face::Regular, size, &shown, t.text, 0);
            (x - inner.x) + full
        };
        if focused && (ui.ticks / 50) % 2 == 0 {
            ui.rect(Rect::new(inner.x + w + 1, f.y + u(12), 1, f.h - u(24)), t.text);
        }
        ui.set_clip(old);
        ui.zone(f, Action::Setup(code));
    }

    fn render_welcome(&mut self, ui: &mut Ui, body: Rect, foot: Rect, sys: &Sys, tall: bool) {
        let t = ui.t;
        let u = Setup::u(tall, body);
        let size = u(76);
        let cx = body.x + body.w / 2;
        let top = if tall { body.y + u(30) } else { body.y + ((body.h - 250) / 2).max(20) };
        ui.shadow(Rect::new(cx - size / 2, top, size, size), u(20), u(14), u(6), 50);
        logo(ui, cx - size / 2, top, size, t.accent, t.on_accent);
        let hello;
        let (title, sub) = if self.joining {
            hello = format!("Welcome, {}", profile::first_name(&self.name));
            (hello.as_str(), "An account has been made for you on this computer. Let's make it yours: your name, a picture, how you sign in and how it looks.")
        } else if self.again {
            ("Set up your profile", "Change your name, picture and look. Your files and apps stay as they are.")
        } else {
            ("Welcome to HydatekOS", "Let's make this computer yours: your name, a picture, how you sign in and how it looks. It takes about a minute.")
        };
        let mut y = Setup::heading(ui, Rect::new(body.x, top + size + u(20), body.w, body.h), title, sub, tall, true);
        let note = if !sys.fs.persistent {
            "This is a live session: what you set up lasts until you restart."
        } else {
            "Everything stays on this computer. Nothing is sent anywhere."
        };
        y += u(14);
        for line in ui.wrap(Face::Regular, u(12), note, body.w) {
            let lw = ui.tw(Face::Regular, u(12), &line);
            ui.text(cx - lw / 2, y, Face::Regular, u(12), &line, t.text3);
            y += u(18);
        }
        self.footer(ui, foot, if self.again { "Start" } else { "Get started" }, false, tall);
        if self.again {
            let lr = Rect::new(foot.x, foot.y, u(110), foot.h);
            let a = Action::Setup(LATER);
            ui.rrect(lr, foot.h / 2, if ui.hot(a) { t.chip.mix(t.text, 20) } else { t.chip });
            ui.text_in(lr, Face::Semibold, u(14), "Not now", t.text, 1);
            ui.zone(lr, a);
        }
    }

    fn render_name(&mut self, ui: &mut Ui, body: Rect, foot: Rect, tall: bool) {
        let u = Setup::u(tall, body);
        let d = u(96);
        let cx = body.x + body.w / 2;
        let pic = self.preview();
        let top = body.y + u(if tall { 8 } else { 36 });
        ui.avatar(Rect::new(cx - d / 2, top, d, d), &pic);
        let y = Setup::heading(ui, Rect::new(body.x, top + d + u(24), body.w, body.h), "What's your name?", "It's shown on the lock screen and the desktop, and saved as the author of documents you make.", tall, true);
        let fw = body.w.min(u(380));
        let f = Rect::new(cx - fw / 2, y + u(6), fw, u(46));
        let name = self.name.clone();
        self.field(ui, f, &name, "Your name", false, F_NAME, tall);
        self.error_line(ui, f.x + u(4), f.b() + u(22), f.w, u(13));
        self.footer(ui, foot, "Continue", true, tall);
    }

    fn render_picture(&mut self, ui: &mut Ui, body: Rect, foot: Rect, tall: bool) {
        let t = ui.t;
        let u = Setup::u(tall, body);
        let d = u(if tall { 64 } else { 84 });
        let pic = self.preview();
        let y = if tall {
            let cx = body.x + body.w / 2;
            ui.avatar(Rect::new(cx - d / 2, body.y, d, d), &pic);
            Setup::heading(ui, Rect::new(body.x, body.y + d + u(12), body.w, body.h), "Choose a picture", "Change it any time in Settings.", tall, true)
        } else {
            ui.avatar(Rect::new(body.x, body.y + u(4), d, d), &pic);
            let hx = body.x + d + u(24);
            let y = Setup::heading(ui, Rect::new(hx, body.y + u(12), body.w - d - u(24), body.h), "Choose a picture", "Initials, one of ours or a photo of your own. Change it any time in Settings.", tall, false);
            y.max(body.y + d + u(20))
        };
        let grid = Rect::new(body.x, y + u(4), body.w, body.b() - y);
        let old = ui.clip_in(Rect::new(grid.x - u(8), grid.y - u(4), grid.w + u(16), grid.h + u(8)));
        let name = self.name.clone();
        let cd = u(if tall { 38 } else { 46 });
        self.picker.render(ui, grid, cd, &name, self.choice, |c| Action::Setup(PICK + c));
        ui.set_clip(old);
        let _ = t;
        self.footer(ui, foot, "Continue", true, tall);
    }

    fn render_signin(&mut self, ui: &mut Ui, body: Rect, foot: Rect, tall: bool) {
        let t = ui.t;
        let u = Setup::u(tall, body);
        // a phone with its keyboard up has room for the essentials only
        let compact = tall && self.osk && self.field != 0;
        let sub = if compact { "" } else { "The lock screen asks for this when HydatekOS starts and after you've been away." };
        let mut y = Setup::heading(ui, body, "Keep your session private", sub, tall, false);
        // the three choices
        let opts = [(Method::Pin, M_PIN, Icon::Keypad, "PIN", "4 to 8 digits"), (Method::Password, M_PASSWORD, Icon::Key, "Password", "6 or more characters"), (Method::Nothing, M_NOTHING, Icon::Lock, "No sign-in", "Anyone can open it")];
        let gap = u(10);
        let cw = (body.w - 2 * gap) / 3;
        let ch = u(if compact { 48 } else if tall { 82 } else { 86 });
        y += u(6);
        for (i, (m, code, icon, title, sub)) in opts.iter().enumerate() {
            let b = Rect::new(body.x + i as i32 * (cw + gap), y, cw, ch);
            let a = Action::Setup(*code);
            let on = self.method == *m;
            ui.rrect(b, u(16), if on { t.accent.with_alpha(28) } else if ui.hot(a) { t.hover } else { t.chip });
            if on {
                ui.stroke(b, u(16), 2, t.accent);
            }
            if compact {
                // icon and title on one line
                ui.icon(*icon, b.x + u(10), b.y + (ch - u(16)) / 2, u(16), if on { t.accent } else { t.text2 });
                let tt = ui.fit(Face::Semibold, u(13), title, b.w - u(36));
                ui.text(b.x + u(30), b.y + ch / 2 + u(5), Face::Semibold, u(13), &tt, t.text);
            } else {
                ui.icon(*icon, b.x + u(14), b.y + u(14), u(20), if on { t.accent } else { t.text2 });
                let tt = ui.fit(Face::Semibold, u(14), title, b.w - u(24));
                ui.text(b.x + u(14), b.y + u(56), Face::Semibold, u(14), &tt, t.text);
                let ss = ui.fit(Face::Regular, u(11), sub, b.w - u(24));
                ui.text(b.x + u(14), b.y + u(73), Face::Regular, u(11), &ss, t.text2);
            }
            ui.zone(b, a);
        }
        y += ch + u(if compact { 12 } else { 18 });
        match self.method {
            Method::Nothing => {
                let msg = "Anyone at this computer can open your session and files. You can add a PIN or password later in Settings.";
                for line in ui.wrap(Face::Regular, u(13), msg, body.w) {
                    y += u(19);
                    ui.text(body.x, y, Face::Regular, u(13), &line, t.text2);
                }
            }
            m => {
                let what = if m == Method::Pin { "PIN" } else { "password" };
                let fh = u(44);
                let (secret, confirm) = (self.secret.clone(), self.confirm.clone());
                let (fw, f2) = if tall {
                    (body.w, Rect::new(body.x, y + fh + u(10), body.w, fh))
                } else {
                    let fw = (body.w - gap) / 2;
                    (fw, Rect::new(body.x + fw + gap, y, fw, fh))
                };
                let f1 = Rect::new(body.x, y, fw, fh);
                self.field(ui, f1, &secret, &format!("New {}", what), true, F_SECRET, tall);
                self.field(ui, f2, &confirm, &format!("Type it again"), true, F_CONFIRM, tall);
                y = f2.b() + u(24);
                if compact {
                    // no hint: the fields and the error only
                    y -= u(6);
                    self.error_line(ui, body.x, y, body.w, u(13));
                } else if self.error.is_empty() {
                    let hint = if m == Method::Pin {
                        "Digits only. Tab or Enter moves to the second box."
                    } else if crate::input::caps_lock() == Some(true) {
                        "Caps Lock is on."
                    } else {
                        "Letters, numbers and symbols. Tab or Enter moves to the second box."
                    };
                    let h = ui.fit(Face::Regular, u(12), hint, body.w);
                    ui.text(body.x, y, Face::Regular, u(12), &h, t.text3);
                } else {
                    self.error_line(ui, body.x, y, body.w, u(13));
                }
                if tall && !compact {
                    // show or hide the keyboard
                    let kr = Rect::new(body.r() - u(36), y - u(22), u(36), u(30));
                    ui.icon_button(kr, Icon::Keyboard, Action::Setup(KB_TOGGLE), u(18));
                }
            }
        }
        self.footer(ui, foot, "Continue", true, tall);
    }

    /// Claude, HydatekOS's assistant: paste an Anthropic API key, or skip.
    fn render_assistant(&mut self, ui: &mut Ui, body: Rect, foot: Rect, tall: bool) {
        let t = ui.t;
        let u = Setup::u(tall, body);
        let compact = tall && self.osk && self.field != 0;
        let d = u(if compact { 0 } else if tall { 56 } else { 64 });
        let mut y = body.y;
        if d > 0 {
            let badge = Rect::new(body.x, y, d, d);
            ui.rrect(badge, d / 4, t.accent);
            ui.icon_in(Icon::Spark, badge, d * 5 / 9, t.on_accent);
            y += d + u(8);
        }
        let sub = if compact {
            "Paste your Anthropic API key, or skip."
        } else {
            "HydatekOS's assistant is Claude, made by Anthropic. Ask it to explain, write, plan or summarise. It uses your own Anthropic API key, which stays in your account on this computer."
        };
        y = Setup::heading(ui, Rect::new(body.x, y, body.w, body.h), "Meet Claude, your assistant", sub, tall, false);
        let f = Rect::new(body.x, y + u(4), body.w.min(u(460)), u(44));
        // the key shows as dots but for its start and end
        let k = self.claude.clone();
        let shown: String = if k.chars().count() > 14 {
            let head: String = k.chars().take(7).collect();
            let tail: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            format!("{}••••••••{}", head, tail)
        } else {
            k
        };
        self.field(ui, f, &shown, "sk-ant-…", false, F_KEY, tall);
        let mut ny = f.b() + u(22);
        if self.error.is_empty() {
            if !compact {
                let hint = "Make a key at console.anthropic.com under API keys, then paste it here with Gen+V. You can add it later in Settings › Assistant.";
                for line in ui.wrap(Face::Regular, u(12), hint, body.w) {
                    ui.text(body.x, ny, Face::Regular, u(12), &line, t.text3);
                    ny += u(18);
                }
            }
        } else {
            self.error_line(ui, body.x, ny, body.w, u(13));
        }
        self.footer(ui, foot, if self.claude.trim().is_empty() { "Skip" } else { "Continue" }, true, tall);
        if !self.claude.trim().is_empty() {
            // "Skip" beside Back, to go on without the key typed
            let sr = Rect::new(foot.x + u(108), foot.y, u(96), foot.h);
            let a = Action::Setup(SKIP_KEY);
            ui.rrect(sr, foot.h / 2, if ui.hot(a) { t.chip.mix(t.text, 20) } else { t.chip });
            ui.text_in(sr, Face::Semibold, u(14), "Skip", t.text, 1);
            ui.zone(sr, a);
        }
    }

    fn render_look(&mut self, ui: &mut Ui, body: Rect, foot: Rect, sys: &Sys, tall: bool) {
        let t = ui.t;
        let u = Setup::u(tall, body);
        let mut y = Setup::heading(ui, body, "Choose your look", "Light for daytime, Dark for evenings. Dynamic colour takes its colours from your wallpaper, and changes with it.", tall, false);
        let gap = u(14);
        let cw = (body.w - gap) / 2;
        let ch = cw * 9 / 16;
        y += u(4);
        for (i, dark) in [false, true].iter().enumerate() {
            let b = Rect::new(body.x + i as i32 * (cw + gap), y, cw, ch);
            let a = Action::Setup(if *dark { DARK } else { LIGHT });
            let on = sys.dark == *dark;
            // a small desktop in that theme, drawn apart then rounded
            let th = sys.theme_for(*dark);
            if on || ui.hot(a) {
                ui.rrect(b.inset(-u(4)), u(18), if on { t.accent } else { t.line });
            }
            let sc = ui.s;
            let mut mini = Canvas::new(b.w * sc, b.h * sc);
            {
                let mut mu = Ui::new(&mut mini, sc, th, None, 0);
                let m = Rect::new(0, 0, b.w, b.h);
                mu.rect(m, th.sky);
                mu.circle(m.w * 3 / 4, m.h * 2 / 5, m.h / 7, th.sun);
                mu.circle(m.w / 5, m.h + m.h / 2, m.h * 3 / 4, th.dune2);
                mu.circle(m.w * 4 / 5, m.h + m.h * 2 / 3, m.h * 5 / 6, th.dune1);
                let win = Rect::new(m.w / 6, m.h / 5, m.w / 2, m.h / 2);
                mu.shadow(win, u(6), u(6), u(2), 40);
                mu.rrect(win, u(6), th.surface);
                mu.rrect(Rect::new(win.x + u(8), win.y + u(10), win.w / 2, u(5)), u(2), th.text);
                mu.rrect(Rect::new(win.x + u(8), win.y + u(20), win.w * 2 / 3, u(4)), u(2), th.text3);
                mu.rrect(Rect::new(win.x + u(8), win.b() - u(18), u(34), u(10)), u(5), th.accent);
                mu.rrect(Rect::new(m.w / 3, m.h - u(16), m.w / 3, u(10)), u(5), th.dock);
            }
            ui.c.blit_scaled(&mini, b.scale(sc), u(14) * sc);
            let label = if *dark { "Dark" } else { "Light" };
            ui.text(b.x + u(2), b.b() + u(24), Face::Semibold, u(14), label, t.text);
            ui.zone(Rect::new(b.x, b.y, b.w, b.h + u(30)), a);
        }
        y += ch + u(50);
        ui.label(body.x, y, u(11), "COLOUR", t.text3);
        y += u(14);
        let step = (body.w / (ACCENTS.len() as i32 + 1)).min(u(140));
        let dynamic = sys.look.accent == crate::personal::Accent::FromWall;
        {
            // dynamic: the wallpaper's accent, on its main colour
            let a = Action::Setup(DYNAMIC);
            let r = u(13);
            let (ccx, ccy) = (body.x + r + u(4), y + r + u(4));
            if dynamic {
                ui.circle(ccx, ccy, r + u(4), t.text);
                ui.circle(ccx, ccy, r + u(2), t.surface);
            }
            let (acc, tint) = match sys.palette {
                Some(p) => (if sys.dark { p.accent.1 } else { p.accent.0 }, p.tint.unwrap_or(0x6B6B6B)),
                None => (0xB5581B, 0xDCC8AB),
            };
            ui.circle(ccx, ccy, r, Color::rgb(tint));
            ui.circle(ccx, ccy, r * 3 / 5, Color::rgb(acc));
            if step >= u(90) {
                ui.text(ccx + r + u(10), ccy + u(5), Face::Regular, u(14), "Dynamic", t.text);
            }
            ui.zone(Rect::new(body.x, y, step - u(4), 2 * r + u(8)), a);
        }
        for (i, (name, l, d)) in ACCENTS.iter().enumerate() {
            let x = body.x + (i as i32 + 1) * step;
            let a = Action::Setup(ACCENT + i as u16);
            let c = Color::rgb(if sys.dark { *d } else { *l });
            let r = u(13);
            let (ccx, ccy) = (x + r + u(4), y + r + u(4));
            if !dynamic && sys.accent == i {
                ui.circle(ccx, ccy, r + u(4), t.text);
                ui.circle(ccx, ccy, r + u(2), t.surface);
            }
            ui.circle(ccx, ccy, r, c);
            if step >= u(90) {
                ui.text(ccx + r + u(10), ccy + u(5), Face::Regular, u(14), name, t.text);
            }
            ui.zone(Rect::new(x, y, step - u(4), 2 * r + u(8)), a);
        }
        self.footer(ui, foot, "Finish", true, tall);
    }

    fn render_done(&mut self, ui: &mut Ui, body: Rect, foot: Rect, sys: &Sys, tall: bool) {
        let u = Setup::u(tall, body);
        let d = u(112);
        let cx = body.x + body.w / 2;
        let top = if tall { body.y + u(30) } else { body.y + ((body.h - 240) / 2).max(20) };
        ui.shadow(Rect::new(cx - d / 2, top, d, d), d / 2, u(14), u(6), 50);
        ui.avatar(Rect::new(cx - d / 2, top, d, d), &sys.avatar);
        let title = format!("You're all set, {}", sys.profile.first_name());
        let sign = if sys.has_pin() && sys.has_password() {
            "Unlock with your PIN or password."
        } else if sys.has_pin() {
            "Unlock with your PIN."
        } else if sys.has_password() {
            "Unlock with your password."
        } else {
            "No sign-in: any key or click opens your session."
        };
        let claude = if sys.has_claude() { " Claude, your assistant, is first in the dock." } else { "" };
        let sub = format!("{}{} Change your profile any time in Settings › Profile.", sign, claude);
        Setup::heading(ui, Rect::new(body.x, top + d + u(22), body.w, body.h), &title, &sub, tall, true);
        let label = if self.again { "Done" } else { "Start using HydatekOS" };
        self.footer(ui, foot, label, false, tall);
    }
}
