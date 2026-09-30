//! Settings: profile, appearance, connectivity, Phone Link, display and
//! system info.

use super::{side_item, App, AppKind, HEADER};
use crate::font::Face;
use crate::gfx::{Canvas, Color, Rect};
use crate::sys::{Req, Sys};
use crate::theme::ACCENTS;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub const SECTIONS: [&str; 14] = ["Profile", "Accounts", "Personalisation", "Keyboard", "Network", "Bluetooth", "Phone Link", "Lock screen", "Browser", "Display", "Assistant", "Sound & haptics", "Devices", "About"];

const C_SECTION: u32 = 100;
const C_DARK: u32 = 1;
const C_MOBILE: u32 = 2;
const C_WIFI: u32 = 3;
const C_BT: u32 = 4;
const C_FOCUS: u32 = 5;
const C_PTR_DOWN: u32 = 6;
const C_PTR_UP: u32 = 7;
const C_OPEN_LINK: u32 = 8;
const C_UNPAIR: u32 = 9;
const C_RESTART: u32 = 10;
const C_SHUTDOWN: u32 = 11;
const C_ACCENT: u32 = 200;
const C_ENGINE: u32 = 300;

pub struct Settings {
    sec: usize,
    /// the name being edited (Profile)
    name: String,
    /// the picture grid is open (Profile)
    picking: bool,
    picker: crate::avatar::Picker,
    /// the picture last chosen in the grid
    chosen: Option<crate::avatar::Choice>,
    /// Accounts: the new account's name and type, the account waiting for
    /// "Remove?" to be confirmed, and the last message
    acc_name: String,
    acc_admin: bool,
    acc_confirm: Option<String>,
    acc_msg: String,
    /// Phone layout: a section page is open (otherwise the section list).
    page: bool,
    /// what's being typed into the PIN / password field
    pin: String,
    password: String,
    /// focused field: C_PIN_FIELD, C_PW_FIELD or 0
    focus: u32,
    pin_msg: String,
    /// Assistant: the API key being typed, the models the key can use (once
    /// asked for), the request asking, and the last message
    claude_key: super::LineEdit,
    models: Vec<crate::web::claude::Model>,
    listing: Option<u32>,
    claude_msg: String,
    /// Keyboard: the key tester, while it's open
    tester: Option<Tester>,
    /// Devices: strokes drawn on the pen pad (x, y relative to it, width),
    /// where it is, the pointer, and whether a stroke is being drawn
    ink: Vec<Vec<(i32, i32, u8)>>,
    pad: Rect,
    at: (i32, i32),
    inking: bool,
    /// Devices: the first row shown (scrolling)
    dev_top: usize,
    /// Personalisation: the tab (wallpaper, colours, desktop), what a
    /// wallpaper choice applies to (both, desktop, lock screen), drawn
    /// thumbnails (what, dark, drawing), and pictures found (path, thumbnail)
    ptab: u8,
    apply: u8,
    thumbs: Vec<(String, bool, Canvas)>,
    pics: Option<Vec<(String, Canvas)>>,
}

/// The keyboard tester: every key stroke the firmware reports, as it
/// reports it (input::raw_since), while shortcuts are paused.
struct Tester {
    /// input::raw_count() when last read
    since: u32,
    /// the latest strokes, newest last
    log: Vec<crate::input::Raw>,
    /// names of the keys pressed so far (keymap::key_name)
    seen: Vec<String>,
    /// modifiers seen: Shift, Ctrl, Aux, logo
    mods: [bool; 4],
    /// when Esc was first pressed (a second press soon after finishes)
    esc: Option<u64>,
}

impl Tester {
    fn new() -> Tester {
        Tester { since: crate::input::raw_count(), log: Vec::new(), seen: Vec::new(), mods: [false; 4], esc: None }
    }

    /// Read new strokes; true when Esc was pressed twice.
    fn read(&mut self, now: u64) -> bool {
        let strokes = crate::input::raw_since(self.since);
        self.since = crate::input::raw_count();
        for r in strokes {
            let name = crate::keymap::key_name(r.scan, r.unicode);
            for (i, on) in [r.shift, r.ctrl, r.aux, r.logo].iter().enumerate() {
                self.mods[i] |= *on;
            }
            if name == "Esc" {
                if self.esc.map_or(false, |t| now < t + 200) {
                    return true;
                }
                self.esc = Some(now);
            } else if !name.is_empty() {
                self.esc = None;
            }
            if !name.is_empty() && !self.seen.contains(&name) {
                self.seen.push(name);
            }
            // a modifier pressed on its own comes first when it's held for a
            // key: show the pair once ("Gen+C", not "Gen" then "Gen+C")
            let alone = |r: &crate::input::Raw| r.scan == 0 && r.unicode == 0;
            let carries = |l: &crate::input::Raw| (!l.shift || r.shift) && (!l.ctrl || r.ctrl) && (!l.aux || r.aux) && (!l.logo || r.logo);
            if self.log.last().map_or(false, |l| alone(l) && carries(l)) {
                self.log.pop();
            }
            self.log.push(r);
            if self.log.len() > 5 {
                self.log.remove(0);
            }
        }
        false
    }
}

/// "Gen+Shift+S": a stroke with its modifiers (Ctrl is Gen).
fn stroke_label(r: &crate::input::Raw) -> String {
    let mut s = String::new();
    for (on, name) in [(r.logo, "Hydatek"), (r.ctrl, "Gen"), (r.aux, "Aux"), (r.shift, "Shift")] {
        if on {
            s.push_str(name);
            s.push('+');
        }
    }
    let key = crate::keymap::key_name(r.scan, r.unicode);
    if key.is_empty() {
        // a modifier or lock key alone
        s.pop();
        if s.is_empty() {
            s.push_str("A key without a code (a lock key?)");
        }
    } else {
        s.push_str(&key);
    }
    s
}

impl Settings {
    pub fn new(sys: &mut Sys) -> Settings {
        let (sec, page) = match sys.settings_page.take() {
            Some(i) => (i.min(SECTIONS.len() - 1), true),
            None => (0, false),
        };
        Settings { sec, name: String::new(), picking: false, picker: Default::default(), chosen: None, acc_name: String::new(), acc_admin: false, acc_confirm: None, acc_msg: String::new(), page, pin: String::new(), password: String::new(), focus: 0, pin_msg: String::new(), claude_key: Default::default(), models: Vec::new(), listing: None, claude_msg: String::new(), tester: None, ink: Vec::new(), pad: Rect::new(0, 0, 0, 0), at: (0, 0), inking: false, dev_top: 0, ptab: 0, apply: 0, thumbs: Vec::new(), pics: None }
    }

    /// The Keyboard section: the Gen and Aux keys.
    fn render_keyboard(&mut self, ui: &mut Ui, m: Rect, sys: &Sys, inst: u32) {
        use crate::shell::keys::keycaps;
        if self.tester.is_some() {
            return self.render_tester(ui, m, sys, inst);
        }
        let t = ui.t;
        // a paragraph, then "<key> is the <a> key, or <b> on an Apple keyboard"
        let intro = |ui: &mut Ui, y: i32, title: &str, about: &str, key: &str, pc: &str, apple: &str| -> i32 {
            ui.text(m.x + 16, y + 30, Face::Semibold, 15, title, t.text);
            let mut ly = y + 52;
            for line in ui.wrap(Face::Regular, 13, about, m.w - 32) {
                ui.text(m.x + 16, ly, Face::Regular, 13, &line, t.text2);
                ly += 19;
            }
            let cy = ly + 12;
            let mut x = m.x + 16;
            x += keycaps(ui, x, cy, key, 13) + 10;
            x += ui.text(x, cy + 5, Face::Regular, 13, "is the", t.text2) + 10;
            x += keycaps(ui, x, cy, pc, 13) + 8;
            x += ui.text(x, cy + 5, Face::Regular, 13, "key, or", t.text2) + 10;
            x += keycaps(ui, x, cy, apple, 13) + 8;
            ui.text(x, cy + 5, Face::Regular, 13, "on an Apple keyboard", t.text2);
            cy + 24
        };
        // Gen
        let top = m.y;
        card(ui, Rect::new(m.x, top, m.w, 204));
        let gen_about = "Gen is HydatekOS's shortcut key: the key Windows calls Ctrl and a Mac calls Command. Hold it and press a letter: Gen+S saves, Gen+C copies, Gen+Space opens an app.";
        let y = intro(ui, top, "The Gen key", gen_about, "Gen", "Ctrl", "⌘");
        ui.rect(Rect::new(m.x + 16, y + 2, m.w - 32, 1), t.line);
        // the Hydatek key, beside Gen
        let hw = keycaps(ui, m.x + 16, y + 30, "Hydatek", 13);
        let hx = m.x + 16 + hw + 12;
        ui.text(hx, y + 26, Face::Semibold, 13, "The Hydatek key: HydatekOS's system key", t.text);
        let about = ui.fit(Face::Regular, 12, "Tap: start menu. Hold: +E Files, +L lock, +D desktop.", m.r() - 16 - hx);
        ui.text(hx, y + 44, Face::Regular, 12, &about, t.text2);

        // Aux
        let top = top + 218;
        card(ui, Rect::new(m.x, top, m.w, 222));
        let aux_about = "Aux is the second modifier, like Alt on Windows and Option on a Mac. Hold it to type special characters; Aux+Tab switches windows, Aux+← and Aux+→ move by word.";
        let y = intro(ui, top, "The Aux key", aux_about, "Aux", "Alt", "⌥");
        let chars = [('N', '₦'), ('E', '€'), ('3', '£'), ('Y', '¥'), ('4', '¢'), ('-', '–'), (';', '…'), ('8', '•'), ('0', '°'), ('G', '©'), ('2', '™'), ('/', '÷')];
        let per = 6;
        let cw = (m.w - 32) / per;
        for (i, (key, ch)) in chars.iter().enumerate() {
            let x = m.x + 16 + (i as i32 % per) * cw;
            let cy = y + 14 + (i as i32 / per) * 32;
            let mut b = [0u8; 4];
            let w = keycaps(ui, x, cy, key.encode_utf8(&mut b), 12);
            let mut b2 = [0u8; 4];
            ui.text(x + w + 10, cy + 6, Face::Semibold, 16, ch.encode_utf8(&mut b2), t.text);
        }

        let y = top + 238;
        ui.button(Rect::new(m.x, y, 190, 34), "Show all shortcuts", Action::App(inst, C_SHORTCUTS), true);
        let mut x = m.x + 204;
        x += ui.text(x, y + 22, Face::Regular, 13, "or press", t.text3) + 8;
        keycaps(ui, x, y + 17, "Gen+/", 12);
        ui.button(Rect::new(m.x, y + 40, 190, 30), "Test your keyboard", Action::App(inst, C_KEY_TEST), false);
        ui.text(m.x + 204, y + 60, Face::Regular, 13, "See what every key sends", t.text3);
    }

    /// The keyboard tester: a picture of the keyboard lighting up, the lock
    /// keys and the last strokes as the firmware reported them.
    fn render_tester(&mut self, ui: &mut Ui, m: Rect, _sys: &Sys, inst: u32) {
        let Some(ts) = self.tester.as_ref() else { return };
        let t = ui.t;
        ui.text(m.x, m.y + 18, Face::Semibold, 15, "Keyboard test", t.text);
        let about = "Press keys to see what HydatekOS receives. Shortcuts are paused until you press Done, or Esc twice.";
        let mut y = m.y + 40;
        for line in ui.wrap(Face::Regular, 12, about, m.w - 110) {
            ui.text(m.x, y, Face::Regular, 12, &line, t.text2);
            y += 17;
        }
        ui.button(Rect::new(m.r() - 90, m.y + 4, 90, 32), "Done", Action::App(inst, C_KEY_TEST_DONE), true);

        // the keyboard
        let last = ts.log.last().map(|r| crate::keymap::key_name(r.scan, r.unicode)).unwrap_or_default();
        let caps = crate::input::caps_lock();
        let lit = |name: &str| -> bool {
            match name {
                "Shift" => ts.mods[0],
                "Ctrl" => ts.mods[1],
                "Aux" => ts.mods[2],
                "Logo" => ts.mods[3],
                "Caps Lock" => caps == Some(true),
                n => ts.seen.iter().any(|s| s == n),
            }
        };
        let q = m.w / 64;
        let kh = 26;
        let x0 = m.x + (m.w - q * 64) / 2;
        y += 8;
        for row in crate::keymap::LAYOUT {
            let mut x = x0;
            for (name, label, w) in row.iter() {
                let r = Rect::new(x, y, *w as i32 * q - 3, kh);
                let (bg, fg) = if !last.is_empty() && *name == last {
                    (t.accent, t.on_accent)
                } else if lit(name) {
                    (t.accent.with_alpha(70), t.text)
                } else {
                    (t.chip, t.text2)
                };
                ui.rrect(r, 5, bg);
                if *label == "Hydatek" {
                    ui.brand(Rect::new(r.x + 2, r.y + 3, r.w - 4, r.h - 6), fg);
                } else if !label.is_empty() {
                    // smaller type for the long labels on narrow keys
                    let size = if ui.tw(Face::Medium, 10, label) > r.w - 4 { 8 } else { 10 };
                    let l = ui.fit(Face::Medium, size, label, r.w - 2);
                    ui.text_in(r, Face::Medium, size, &l, fg, 1);
                }
                x += *w as i32 * q;
            }
            y += kh + 3;
        }

        // the lock keys
        y += 10;
        let mut x = m.x;
        for (name, state) in [("Caps Lock", caps), ("Num Lock", crate::input::num_lock()), ("Scroll Lock", crate::input::scroll_lock())] {
            let st = match state {
                Some(true) => "on",
                Some(false) => "off",
                None => "not reported",
            };
            let label = format!("{} {}", name, st);
            let w = ui.tw(Face::Medium, 12, &label) + 24;
            ui.rrect(Rect::new(x, y, w, 26), 13, if state == Some(true) { t.accent.with_alpha(60) } else { t.chip });
            ui.text_in(Rect::new(x, y, w, 26), Face::Medium, 12, &label, t.text, 1);
            x += w + 8;
        }
        y += 40;

        // the last strokes, newest first
        let rh = 30;
        card(ui, Rect::new(m.x, y, m.w, rh * 5 + 8));
        if ts.log.is_empty() {
            ui.text(m.x + 16, y + 26, Face::Regular, 13, "Nothing pressed yet", t.text3);
        }
        for (i, r) in ts.log.iter().rev().enumerate() {
            let ry = y + 4 + i as i32 * rh;
            let label = ui.fit(Face::Semibold, 13, &stroke_label(r), m.w - 230);
            ui.text(m.x + 16, ry + 20, Face::Semibold, 13, &label, if i == 0 { t.text } else { t.text2 });
            let code = format!("scan {:#06x}  char {:#06x}", r.scan, r.unicode);
            let cw = ui.tw(Face::Mono, 11, &code);
            ui.text(m.r() - 16 - cw, ry + 20, Face::Mono, 11, &code, t.text3);
        }
        y += rh * 5 + 22;
        let note = if ts.esc.is_some() { "Press Esc again to finish." } else { "Keys HydatekOS doesn't know show their scan code, so they can be added." };
        let note = ui.fit(Face::Regular, 12, note, m.w);
        ui.text(m.x, y, Face::Regular, 12, &note, if ts.esc.is_some() { t.accent } else { t.text3 });
    }

    /// Sound & haptics: the volume, and haptic feedback with a picture of
    /// each pattern as it plays.
    /// A wallpaper drawn small (kept until the theme or the time changes).
    fn thumb(&mut self, sys: &Sys, t: &crate::theme::Theme, w: &crate::personal::Wall, fit: crate::personal::Fit, pw: i32, ph: i32) -> usize {
        let key = alloc::format!("{}|{}|{}|{}|{}", w.save(), fit.name(), pw, ph, sys.scene_time().map_or(9999, |m| m / 10));
        if let Some(i) = self.thumbs.iter().position(|x| x.0 == key && x.1 == t.dark) {
            return i;
        }
        let c = crate::shell::wallpaper::thumbnail(&sys.fs, w, fit, t, sys.scene_time(), pw, ph);
        if self.thumbs.len() > 40 {
            self.thumbs.clear();
        }
        self.thumbs.push((key, t.dark, c));
        self.thumbs.len() - 1
    }

    fn draw_thumb(&mut self, ui: &mut Ui, sys: &Sys, w: &crate::personal::Wall, fit: crate::personal::Fit, r: Rect, selected: bool, a: Action) {
        let t = ui.t;
        let i = self.thumb(sys, &t, w, fit, r.w * ui.s, r.h * ui.s);
        if selected {
            ui.rrect(r.inset(-3), 11, t.accent);
        }
        let pr = r.scale(ui.s);
        ui.c.blit_scaled(&self.thumbs[i].2, pr, 8 * ui.s);
        if ui.hot(a) && !selected {
            ui.rrect(r, 8, t.hover);
        }
        ui.zone(r, a);
    }

    /// Settings › Personalisation: wallpaper, colours, the desktop.
    fn render_personal(&mut self, ui: &mut Ui, m: Rect, sys: &Sys, inst: u32) {
        use crate::personal::{Accent, Fit, Mode, Scene, Wall, GRADIENTS, SOLIDS};
        let t = ui.t;
        let sw_x = m.r() - 54;
        // tabs
        let tabs = ["Wallpaper", "Colours", "Desktop"];
        let tw = 100;
        for (i, name) in tabs.iter().enumerate() {
            ui.button(Rect::new(m.x + i as i32 * (tw + 6), m.y, tw, 28), name, Action::App(inst, C_PTAB + i as u32), self.ptab == i as u8);
        }
        let top = m.y + 42;
        let look = sys.look.clone();
        match self.ptab {
            0 => {
                // choosing one of your pictures
                if self.pics.is_some() {
                    ui.text(m.x, top + 14, Face::Semibold, 14, "Your pictures", t.text);
                    ui.button(Rect::new(m.r() - 70, top, 70, 26), "Back", Action::App(inst, C_PICS_BACK), false);
                    let pics = self.pics.take().unwrap_or_default();
                    if pics.is_empty() {
                        let l = ui.fit(Face::Regular, 13, "Put pictures in Pictures or Downloads (PNG, JPEG, WebP, GIF or BMP) to use one here.", m.w);
                        ui.text(m.x, top + 50, Face::Regular, 13, &l, t.text2);
                    }
                    let (cols, gap) = (3, 10);
                    let cw = (m.w - gap * (cols - 1)) / cols;
                    let ch = cw * 10 / 16;
                    for (i, (path, thumb)) in pics.iter().enumerate() {
                        let (cx, cy) = (m.x + (i as i32 % cols) * (cw + gap), top + 36 + (i as i32 / cols) * (ch + 30));
                        if cy + ch > m.b() {
                            break;
                        }
                        let r = Rect::new(cx, cy, cw, ch);
                        let a = Action::App(inst, C_PIC + i as u32);
                        ui.c.blit_scaled(thumb, r.scale(ui.s), 8 * ui.s);
                        if ui.hot(a) {
                            ui.rrect(r, 8, t.hover);
                        }
                        ui.zone(r, a);
                        let name = ui.fit(Face::Regular, 12, crate::fs::basename(path), cw);
                        ui.text(cx, cy + ch + 16, Face::Regular, 12, &name, t.text2);
                    }
                    self.pics = Some(pics);
                    return;
                }
                // the desktop and the lock screen as they are
                let pw = (m.w - 12) / 2;
                let ph = (pw * 10 / 16).min(118);
                let lock = look.lock.clone().unwrap_or(look.wall.clone());
                for (k, (w, label)) in [(look.wall.clone(), "Desktop"), (lock, "Lock screen")].iter().enumerate() {
                    let r = Rect::new(m.x + k as i32 * (pw + 12), top, pw, ph);
                    let a = Action::App(inst, C_APPLY + 1 + k as u32);
                    self.draw_thumb(ui, sys, w, look.fit, r, self.apply == 1 + k as u8, a);
                    let cap = alloc::format!("{} · {}", label, w.name());
                    let cap = ui.fit(Face::Medium, 12, &cap, pw);
                    ui.text(r.x, r.b() + 16, Face::Medium, 12, &cap, t.text2);
                }
                let mut y = top + ph + 28;
                // what a choice applies to
                let applies = ["Both", "Desktop", "Lock screen"];
                ui.text(m.x, y + 17, Face::Regular, 12, "Apply to", t.text2);
                for (i, n) in applies.iter().enumerate() {
                    ui.button(Rect::new(m.x + 64 + i as i32 * 94, y, 88, 26), n, Action::App(inst, C_APPLY + i as u32), self.apply == i as u8);
                }
                y += 36;
                let target = if self.apply == 2 { look.lock.clone().unwrap_or(look.wall.clone()) } else { look.wall.clone() };
                // scenes, then your pictures
                let (cols, gap) = (4, 8);
                let cw = (m.w - gap * (cols - 1)) / cols;
                let ch = cw * 10 / 16;
                for (i, sc) in Scene::ALL.iter().enumerate() {
                    let r = Rect::new(m.x + (i as i32 % cols) * (cw + gap), y + (i as i32 / cols) * (ch + 22), cw, ch);
                    let w = Wall::Scene(*sc);
                    self.draw_thumb(ui, sys, &w, Fit::Fill, r, target == w, Action::App(inst, C_SCENE + i as u32));
                    ui.text(r.x, r.b() + 14, Face::Regular, 11, sc.name(), t.text2);
                }
                let i = Scene::ALL.len() as i32;
                let r = Rect::new(m.x + (i % cols) * (cw + gap), y + (i / cols) * (ch + 22), cw, ch);
                let a = Action::App(inst, C_PICS);
                ui.rrect(r, 8, if ui.hot(a) { t.hover } else { t.chip });
                if matches!(target, Wall::Picture(_)) {
                    ui.rrect(r.inset(-3), 11, t.accent);
                    self.draw_thumb(ui, sys, &target, Fit::Fill, r, true, a);
                } else {
                    ui.text_in(r, Face::Medium, 12, "Pictures…", t.text, 1);
                    ui.zone(r, a);
                }
                ui.text(r.x, r.b() + 14, Face::Regular, 11, "Your pictures", t.text2);
                y += 2 * (ch + 22) + 6;
                // colours and gradients
                let d = 26;
                let mut x = m.x;
                for (i, c) in SOLIDS.iter().enumerate() {
                    let a = Action::App(inst, C_SOLID + i as u32);
                    if target == Wall::Solid(*c) {
                        ui.circle(x + d / 2, y + d / 2, d / 2 + 3, t.accent);
                    }
                    ui.circle(x + d / 2, y + d / 2, d / 2, Color::rgb(*c));
                    ui.zone(Rect::new(x, y, d, d), a);
                    x += d + 8;
                }
                for (i, g) in GRADIENTS.iter().enumerate() {
                    let r = Rect::new(x, y, d, d);
                    let w = Wall::Gradient(g.0, g.1);
                    self.draw_thumb(ui, sys, &w, Fit::Fill, r, target == w, Action::App(inst, C_GRAD + i as u32));
                    x += d + 8;
                }
                y += d + 12;
                // pictures: how they fit
                if let Wall::Picture(_) = target {
                    ui.text(m.x, y + 17, Face::Regular, 12, "Fit", t.text2);
                    for (i, f) in Fit::ALL.iter().enumerate() {
                        ui.button(Rect::new(m.x + 40 + i as i32 * 72, y, 66, 26), f.name(), Action::App(inst, C_FIT + i as u32), look.fit == *f);
                    }
                    y += 34;
                }
                if y + 40 <= m.b() {
                    row(ui, Rect::new(m.x, y - 8, m.w - 32, 40), y - 8, "Follow the time of day", "Scenes show dawn, day, dusk and night skies");
                    ui.switch(sw_x, y, look.time_of_day, Action::App(inst, C_TOD));
                }
            }
            1 => {
                card(ui, Rect::new(m.x, top, m.w, 116));
                let inner = Rect::new(m.x + 16, top, m.w - 32, 116);
                let sub = match look.mode {
                    Mode::Auto => "Dark from 19:00 to 07:00",
                    Mode::Dark => "Dusk: easy on the eyes at night",
                    Mode::Light => "Dune: warm and bright",
                };
                row(ui, inner, top + 6, "Theme", sub);
                for (i, md) in Mode::ALL.iter().enumerate() {
                    ui.button(Rect::new(m.x + 16 + i as i32 * 104, top + 66, 98, 30), md.name(), Action::App(inst, C_MODE + i as u32), look.mode == *md);
                }
                let top = top + 130;
                card(ui, Rect::new(m.x, top, m.w, 150));
                let inner = Rect::new(m.x + 16, top, m.w - 32, 150);
                row(ui, inner, top + 6, "Accent colour", "Highlights, folders and buttons");
                let step = ((m.w - 32) / 5).min(84);
                for (i, (name, l, d)) in ACCENTS.iter().enumerate() {
                    let cx = m.x + 16 + i as i32 * step;
                    let a = Action::App(inst, C_ACCENT + i as u32);
                    if look.accent == Accent::Preset(i as u8) {
                        ui.circle(cx + 14, top + 82, 17, t.text);
                        ui.circle(cx + 14, top + 82, 15, t.surface);
                    }
                    ui.circle(cx + 14, top + 82, 13, Color::rgb(if sys.dark { *d } else { *l }));
                    ui.text_in(Rect::new(cx - 10, top + 102, 48, 20), Face::Regular, 12, name, t.text2, 1);
                    ui.zone(Rect::new(cx - 4, top + 62, 40, 60), a);
                }
                // the wallpaper's own colour
                let cx = m.x + 16 + 4 * step;
                let a = Action::App(inst, C_ACC_WALL);
                let from = sys.accent_rgb.map(|p| if sys.dark { p.1 } else { p.0 });
                if look.accent == Accent::FromWall {
                    ui.circle(cx + 14, top + 82, 17, t.text);
                    ui.circle(cx + 14, top + 82, 15, t.surface);
                }
                match from {
                    Some(c) => ui.circle(cx + 14, top + 82, 13, Color::rgb(c)),
                    None => {
                        // a quartered swatch: "from the picture"
                        for (k, c) in [0xF3A683u32, 0x7FD1C7, 0xB39DDB, 0xF6D365].iter().enumerate() {
                            ui.circle(cx + 8 + (k as i32 % 2) * 12, top + 76 + (k as i32 / 2) * 12, 6, Color::rgb(*c));
                        }
                    }
                }
                ui.text_in(Rect::new(cx - 18, top + 102, 64, 20), Face::Regular, 12, "Wallpaper", t.text2, 1);
                ui.zone(Rect::new(cx - 4, top + 62, 40, 60), a);
            }
            _ => {
                card(ui, Rect::new(m.x, top, m.w, 250));
                let inner = Rect::new(m.x + 16, top, m.w - 32, 250);
                row(ui, inner, top + 6, "Desktop widgets", "The clock, what's next and quick settings");
                ui.switch(sw_x, top + 14, look.widgets, Action::App(inst, C_WIDGETS));
                row(ui, inner, top + 54, "Mobile shell", "Use the HydatekOS Mobile home screen on this device");
                ui.switch(sw_x, top + 62, sys.mobile_shell, Action::App(inst, C_MOBILE));
                row(ui, inner, top + 102, "Focus", "Silence Phone Link notifications");
                ui.switch(sw_x, top + 110, sys.focus, Action::App(inst, C_FOCUS));
                row(ui, inner, top + 150, "Reduce motion", "Windows and menus appear and go without animating");
                ui.switch(sw_x, top + 158, sys.reduce_motion, Action::App(inst, C_MOTION));
                row(ui, inner, top + 198, "Pointer speed", "");
                let ps = format!("{}", sys.pointer_speed);
                ui.button(Rect::new(sw_x - 44, top + 204, 30, 26), "-", Action::App(inst, C_PTR_DOWN), false);
                ui.text_in(Rect::new(sw_x - 12, top + 204, 20, 26), Face::Semibold, 13, &ps, t.text, 1);
                ui.button(Rect::new(sw_x + 12, top + 204, 30, 26), "+", Action::App(inst, C_PTR_UP), false);
            }
        }
    }

    /// Pictures that could be wallpapers, drawn small (once).
    fn find_pictures(&mut self, sys: &Sys, t: &crate::theme::Theme) {
        let mut out = Vec::new();
        for dir in ["/home/Pictures", "/home/Downloads", "/home/Documents/Photos 2026", "/home/Shared"] {
            for (name, is_dir, size) in sys.fs.list(dir) {
                let lower = name.to_ascii_lowercase();
                if out.len() >= 9 || is_dir || size > 24 << 20 || ![".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"].iter().any(|e| lower.ends_with(e)) {
                    continue;
                }
                let path = crate::fs::join(dir, &name);
                let w = crate::personal::Wall::Picture(path.clone());
                // drawn only if it decodes
                if crate::shell::wallpaper::picture(&sys.fs, &path).is_some() {
                    out.push((path, crate::shell::wallpaper::thumbnail(&sys.fs, &w, crate::personal::Fit::Fill, t, None, 240, 150)));
                }
            }
        }
        self.pics = Some(out);
    }

    /// A wallpaper chosen: for the desktop, the lock screen or both.
    fn set_wall(&mut self, sys: &mut Sys, w: crate::personal::Wall) {
        let look = &mut sys.look;
        match self.apply {
            1 => {
                // the lock screen keeps what it showed
                if look.lock.is_none() {
                    look.lock = Some(look.wall.clone());
                }
                look.wall = w;
            }
            2 => look.lock = Some(w),
            _ => {
                look.wall = w;
                look.lock = None;
            }
        }
        if look.lock.as_ref() == Some(&look.wall) {
            look.lock = None;
        }
        sys.reqs.push(Req::SaveSettings);
    }

    /// A stroke's width now: from a pen's pressure (1-9), or 3 for a mouse or
    /// finger. And whether it's the eraser end.
    fn ink_width() -> (u8, bool) {
        match crate::input::pen() {
            Some((p, e)) => ((1 + p * 8 / 1000) as u8, e),
            None => (3, false),
        }
    }

    /// Settings › Devices: everything HydatekOS found, and who drives it.
    fn render_devices(&mut self, ui: &mut Ui, m: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let mut list: Vec<(String, String, &str)> = Vec::new();
        let mut heads: Vec<(usize, &str)> = Vec::new();
        heads.push((list.len(), "USB"));
        for d in &sys.usb {
            list.push((d.name.clone(), format!("{} · {}", d.kind, d.ids), d.driver));
        }
        if sys.usb.is_empty() {
            list.push((String::from("No USB devices"), String::new(), ""));
        }
        if !sys.disks.is_empty() {
            heads.push((list.len(), "Storage"));
            for d in &sys.disks {
                list.push((format!("{} · {}", d.name, crate::storage::size_text(d.bytes)), format!("{} · {}", d.kind, crate::storage::summary(&d.partitions)), d.driver));
            }
        }
        heads.push((list.len(), "PCI"));
        for d in &sys.hw.pci {
            list.push((d.name.clone(), format!("{} · {}", d.kind, d.ids), d.driver));
        }
        if sys.hw.pci.is_empty() {
            list.push((String::from("No PCI bus"), String::from("The processor's devices are built into the chip"), ""));
        }
        if !sys.acpi_devices.is_empty() {
            heads.push((list.len(), "ACPI"));
            for (name, kind, driver) in &sys.acpi_devices {
                let drv: &'static str = match driver.as_str() {
                    "HydatekOS ACPI" => "HydatekOS ACPI",
                    "HydatekOS I2C" => "HydatekOS I2C",
                    "HydatekOS I2C HID" => "HydatekOS I2C HID",
                    "Firmware (ACPI)" => "Firmware (ACPI)",
                    "Controller not responding" => "Controller not responding",
                    "Controller not found" => "Controller not found",
                    "Device not answering" => "Device not answering",
                    "No driver for its I2C controller" => "No driver yet",
                    _ => "No driver yet",
                };
                list.push((name.clone(), kind.clone(), drv));
            }
        }
        // the pen pad at the bottom
        const PAD: i32 = 124;
        let pad = Rect::new(m.x, m.b() - PAD, m.w, PAD);
        card(ui, pad);
        self.pad = Rect::new(pad.x + 8, pad.y + 26, pad.w - 16, pad.h - 34);
        ui.text(pad.x + 14, pad.y + 18, Face::Semibold, 12, "Try your pen, finger or mouse", t.text2);
        let hint = match crate::input::pen() {
            Some((p, true)) => format!("Eraser · {}%", p / 10),
            Some((p, false)) => format!("Pen pressure {}%", p / 10),
            None => String::from("Clear"),
        };
        let hw_ = ui.tw(Face::Medium, 11, &hint) + 18;
        ui.button(Rect::new(pad.r() - 12 - hw_, pad.y + 5, hw_, 20), &hint, Action::App(inst, C_INK_CLEAR), false);
        ui.rrect(self.pad, 8, t.surface);
        for stroke in &self.ink {
            for w in stroke.windows(2) {
                let (a, b) = (w[0], w[1]);
                let r = (b.2 as i32).max(1);
                // a line of dots, as wide as the pressure
                let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) / 1).max(1);
                for k in 0..=n {
                    let px = self.pad.x + a.0 + (b.0 - a.0) * k / n;
                    let py = self.pad.y + a.1 + (b.1 - a.1) * k / n;
                    ui.circle(px, py, r, t.accent);
                }
            }
            if stroke.len() == 1 {
                let p = stroke[0];
                ui.circle(self.pad.x + p.0, self.pad.y + p.1, p.2 as i32, t.accent);
            }
        }
        ui.zone(self.pad, Action::App(inst, C_INK));
        let m = Rect::new(m.x, m.y, m.w, m.h - PAD - 8);
        const ROW: i32 = 40;
        const HEAD: i32 = 30;
        let mut y = m.y;
        let mut shown = 0;
        self.dev_top = self.dev_top.min(list.len().saturating_sub(1));
        let top = self.dev_top;
        for (i, (name, sub, driver)) in list.iter().enumerate().skip(top) {
            // a section's heading, or the one it's in when scrolled into it
            let head = heads.iter().find(|(at, _)| *at == i).or_else(|| if i == top { heads.iter().rev().find(|(at, _)| *at <= i) } else { None });
            if let Some((_, h)) = head {
                if y + HEAD + ROW > m.b() {
                    break;
                }
                ui.label(m.x + 4, y + 20, 11, h, t.text2);
                y += HEAD;
            }
            if y + ROW > m.b() - 20 && i + 1 < list.len() {
                break;
            }
            card(ui, Rect::new(m.x, y, m.w, ROW - 4));
            let dw = if driver.is_empty() { 0 } else { ui.tw(Face::Medium, 11, driver) + 18 };
            let nm = ui.fit(Face::Medium, 13, name, m.w - 40 - dw);
            ui.text(m.x + 14, y + 16, Face::Medium, 13, &nm, t.text);
            if !sub.is_empty() {
                let sb = ui.fit(Face::Regular, 11, sub, m.w - 40 - dw);
                ui.text(m.x + 14, y + 30, Face::Regular, 11, &sb, t.text2);
            }
            if !driver.is_empty() {
                let ours = driver.starts_with("HydatekOS") || driver.contains("+ HydatekOS");
                let none = driver.starts_with("No driver") || driver.starts_with("Waits");
                let (bg, fg) = if ours {
                    (t.accent, t.on_accent)
                } else if none {
                    (t.chip.mix(t.danger, 40), t.text)
                } else {
                    (t.chip, t.text2)
                };
                let chip = Rect::new(m.r() - 12 - dw, y + 8, dw, 20);
                ui.rrect(chip, 10, bg);
                ui.text_in(chip, Face::Medium, 11, driver, fg, 1);
            }
            y += ROW;
            shown += 1;
        }
        let rows = list.iter().filter(|l| !l.2.is_empty()).count();
        if top + shown < list.len() {
            ui.text(m.x + 4, m.b() - 4, Face::Regular, 12, &format!("…and {} more (scroll)", list.len() - top - shown), t.text2);
        } else if rows > 0 {
            let ours = list.iter().filter(|l| l.2.contains("HydatekOS")).count();
            ui.text(m.x + 4, (y + 18).min(m.b() - 4), Face::Regular, 12, &format!("{} devices · {} with HydatekOS drivers", rows, ours), t.text2);
        }
    }

    fn render_sound(&mut self, ui: &mut Ui, m: Rect, sys: &Sys, inst: u32) {
        use crate::haptics::{self, Haptic, Strength};
        let t = ui.t;
        let sw_x = m.r() - 54;
        // sound
        card(ui, Rect::new(m.x, m.y, m.w, 110));
        let inner = Rect::new(m.x + 16, m.y, m.w - 32, 110);
        let dev = match &sys.audio {
            Some(a) => a.clone(),
            None => String::from("No sound device found"),
        };
        row(ui, inner, m.y + 6, "Volume", if sys.muted { "Muted" } else { &dev });
        let bar = Rect::new(m.r() - 16 - 40 - 150, m.y + 28, 150, 6);
        ui.button(Rect::new(bar.x - 40, m.y + 18, 30, 26), "-", Action::App(inst, C_VOL_DOWN), false);
        ui.rrect(bar, 3, t.chip.mix(t.text, 30));
        let level = if sys.muted { 0 } else { sys.volume as i32 };
        if level > 0 {
            ui.rrect(Rect::new(bar.x, bar.y, (bar.w * level / 100).max(6), bar.h), 3, t.accent);
        }
        ui.button(Rect::new(bar.r() + 10, m.y + 18, 30, 26), "+", Action::App(inst, C_VOL_UP), false);
        row(ui, inner, m.y + 56, "Mute", "The mute key does this too");
        ui.switch(sw_x, m.y + 62, sys.muted, Action::App(inst, C_MUTE));

        // haptics
        let h = &sys.haptics;
        let top = m.y + 124;
        card(ui, Rect::new(m.x, top, m.w, 160));
        let inner = Rect::new(m.x + 16, top, m.w - 32, 160);
        let on = match (sys.haptic_pads, sys.motors) {
            (0, 0) => String::from("A tap for keys and switches, a buzz for mistakes"),
            (p, 0) => format!("Played on your haptic touchpad{}", if p > 1 { "s" } else { "" }),
            (0, c) => format!("Played on {} controller{}'s rumble motors", c, if c > 1 { "s" } else { "" }),
            (p, c) => format!("Played on {} haptic touchpad{} and {} controller{}", p, if p > 1 { "s" } else { "" }, c, if c > 1 { "s" } else { "" }),
        };
        row(ui, inner, top + 6, "Haptic feedback", &on);
        ui.switch(sw_x, top + 12, h.on, Action::App(inst, C_HAPTICS));
        row(ui, inner, top + 56, "Strength", "");
        let segw = 76;
        for (i, s) in Strength::ALL.iter().enumerate() {
            let b = Rect::new(m.r() - 16 - 3 * segw + i as i32 * segw, top + 62, segw - 4, 28);
            ui.button(b, s.name(), Action::App(inst, C_HSTRENGTH + i as u32), h.strength == *s);
        }
        let phone = if !sys.link.paired {
            "Pair an Android phone in Phone Link to feel it there"
        } else if sys.phone_haptics() {
            "Your phone plays each pattern with its vibration motor"
        } else {
            "Your phone's HydatekOS Link app needs updating for this"
        };
        row(ui, inner, top + 106, "Vibrate my phone", phone);
        ui.switch(sw_x, top + 112, h.phone, Action::App(inst, C_HPHONE));

        // try each pattern; the latest one is drawn as it plays
        let top = top + 174;
        ui.text(m.x, top + 14, Face::Semibold, 14, "Try it", t.text);
        let mut x = m.x;
        let mut y = top + 26;
        for (i, hk) in Haptic::ALL.iter().enumerate() {
            let w = ui.tw(Face::Medium, 12, hk.name()) + 24;
            if x + w > m.r() {
                x = m.x;
                y += 34;
            }
            ui.button(Rect::new(x, y, w, 28), hk.name(), Action::App(inst, C_HAPTIC + i as u32), false);
            x += w + 6;
        }
        let wave = Rect::new(m.x, y + 40, m.w, 56);
        card(ui, wave);
        match h.last {
            Some((kind, at)) => {
                let pat = haptics::pattern(kind, h.strength);
                let total = haptics::duration(&pat).max(1);
                // 300 ms across the card
                let span = 300u32.max(total);
                let px = |ms: u32| wave.x + 12 + ((wave.w - 24) as u32 * ms / span) as i32;
                let mut at_ms = 0u32;
                let base = wave.b() - 10;
                for q in &pat {
                    let hgt = (wave.h - 34) * q.amp as i32 / 255;
                    let r = Rect::new(px(at_ms), base - hgt, (px(at_ms + q.ms as u32) - px(at_ms)).max(2), hgt);
                    ui.rrect(r, 2, t.accent);
                    at_ms += q.ms as u32 + q.gap as u32;
                }
                ui.rect(Rect::new(wave.x + 12, base, wave.w - 24, 1), t.line);
                // the playhead while it plays
                let since = crate::arch::ms().saturating_sub(at) as u32;
                if since < total {
                    ui.rect(Rect::new(px(since), wave.y + 6, 2, wave.h - 12), t.text);
                }
                let label = format!("{} · {} ms · {}", kind.name(), total, h.strength.name());
                let lw = ui.tw(Face::Medium, 12, &label);
                ui.text(wave.r() - 12 - lw, wave.y + 20, Face::Medium, 12, &label, t.text2);
            }
            None => {
                ui.text(wave.x + 14, wave.y + 33, Face::Regular, 13, "Press one to see its pattern", t.text3);
            }
        }
        let note = "This computer has no vibration motor or haptic touchpad HydatekOS can drive yet.";
        let note = ui.fit(Face::Regular, 12, note, m.w);
        ui.text(m.x, wave.b() + 22, Face::Regular, 12, &note, t.text3);
    }

    /// The Assistant section: Claude, its API key and model.
    fn render_assistant(&mut self, ui: &mut Ui, m: Rect, sys: &Sys, inst: u32) {
        use crate::web::claude;
        let t = ui.t;
        // about
        card(ui, Rect::new(m.x, m.y, m.w, 104));
        let badge = Rect::new(m.x + 16, m.y + 16, 40, 40);
        ui.rrect(badge, 12, t.accent);
        ui.icon_in(crate::icons::Icon::Spark, badge, 22, t.on_accent);
        ui.text(m.x + 70, m.y + 32, Face::Semibold, 15, "Claude", t.text);
        ui.text(m.x + 70, m.y + 50, Face::Regular, 12, "HydatekOS's assistant, made by Anthropic", t.text2);
        let about = "Your messages go to Anthropic's API with your own key. Nothing else on this computer is sent.";
        let about = ui.fit(Face::Regular, 12, about, m.w - 32);
        ui.text(m.x + 16, m.y + 86, Face::Regular, 12, &about, t.text3);

        // the key
        let top = m.y + 118;
        card(ui, Rect::new(m.x, top, m.w, 64));
        let inner = Rect::new(m.x + 16, top, m.w - 32, 64);
        let right = inner.r();
        if self.focus == C_CLAUDE_KEY {
            row(ui, inner, top + 6, "API key", "");
            let f = Rect::new(right - 88 - 240, top + 16, 240, 32);
            let dots: String = self.claude_key.text.chars().map(|_| '•').collect();
            let before: String = self.claude_key.before_caret().chars().map(|_| '•').collect();
            ui.field_at(f, &dots, &before, "Paste with Gen+V", true, Action::App(inst, C_CLAUDE_KEY));
            ui.button(Rect::new(right - 80, top + 16, 80, 32), "Save", Action::App(inst, C_CLAUDE_SAVE), true);
        } else if sys.has_claude() {
            row(ui, inner, top + 6, "API key", &claude::key_hint(&sys.claude_key));
            ui.button(Rect::new(right - 88 - 90, top + 16, 90, 32), "Change", Action::App(inst, C_CLAUDE_KEY), false);
            ui.button(Rect::new(right - 80, top + 16, 80, 32), "Remove", Action::App(inst, C_CLAUDE_REMOVE), false);
        } else {
            row(ui, inner, top + 6, "API key", "Not set up: make one at console.anthropic.com");
            ui.button(Rect::new(right - 110, top + 16, 110, 32), "Add key", Action::App(inst, C_CLAUDE_KEY), true);
        }

        // the model
        let top = top + 78;
        let n = self.models.len().min(6) as i32;
        let h = 64 + if n > 0 { n * 36 + 8 } else { 0 };
        card(ui, Rect::new(m.x, top, m.w, h));
        let inner = Rect::new(m.x + 16, top, m.w - 32, 64);
        let chosen = if sys.claude_model.is_empty() { String::from("The newest Opus your key can use") } else { sys.claude_model.clone() };
        row(ui, inner, top + 6, "Model", &chosen);
        if sys.has_claude() {
            let label = if self.listing.is_some() { "Checking…" } else if self.models.is_empty() { "Choose…" } else { "Refresh" };
            ui.button(Rect::new(inner.r() - 110, top + 16, 110, 32), label, Action::App(inst, C_CLAUDE_MODELS), false);
        }
        for (i, (id, name)) in self.models.iter().take(6).enumerate() {
            let y = top + 64 + i as i32 * 36;
            let rr = Rect::new(m.x + 4, y, m.w - 8, 34);
            let a = Action::App(inst, C_MODEL + i as u32);
            if ui.hot(a) {
                ui.rrect(rr, 10, t.hover);
            }
            let on = sys.claude_model == *id;
            ui.circle(m.x + 26, y + 17, 9, if on { t.accent } else { t.line });
            ui.circle(m.x + 26, y + 17, if on { 4 } else { 7 }, if on { Color::rgb(0xFFFFFF) } else { t.surface });
            let label = ui.fit(Face::Medium, 13, name, m.w - 70);
            ui.text(m.x + 46, y + 22, Face::Medium, 13, &label, t.text);
            ui.zone(rr, a);
        }
        let msg = if self.claude_msg.is_empty() { "Open Claude from the dock, or press Gen+Space and type Claude." } else { self.claude_msg.as_str() };
        let msg = ui.fit(Face::Regular, 12, msg, m.w - 16);
        ui.text(m.x + 8, top + h + 22, Face::Regular, 12, &msg, t.text2);
        ui.button(Rect::new(m.x, top + h + 36, 130, 32), "Open Claude", Action::App(inst, C_CLAUDE_OPEN), true);
    }

    /// The Accounts section: everyone who uses this computer.
    fn render_accounts(&mut self, ui: &mut Ui, m: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let admin = sys.is_admin();
        let rh = 60;
        let n = sys.people.len().max(1) as i32;
        card(ui, Rect::new(m.x, m.y, m.w, n * rh + 8));
        if sys.people.is_empty() {
            ui.text(m.x + 16, m.y + 36, Face::Regular, 13, "Finish setting up this computer to add accounts.", t.text2);
        }
        for (i, p) in sys.people.iter().enumerate() {
            let y = m.y + 4 + i as i32 * rh;
            if i > 0 {
                ui.rect(Rect::new(m.x + 16, y, m.w - 32, 1), t.line);
            }
            ui.avatar(Rect::new(m.x + 16, y + 12, 36, 36), &p.avatar);
            let me = p.id == sys.user;
            let right = m.r() - 16;
            let tx = m.x + 64;
            if self.acc_confirm.as_deref() == Some(p.id.as_str()) {
                let q = format!("Remove {} and their files?", crate::profile::first_name(&p.name));
                let q = ui.fit(Face::Semibold, 13, &q, right - 184 - tx);
                ui.text(tx, y + 35, Face::Semibold, 13, &q, t.danger);
                ui.button(Rect::new(right - 176, y + 14, 84, 32), "Cancel", Action::App(inst, C_ACC_CANCEL), false);
                ui.button(Rect::new(right - 84, y + 14, 84, 32), "Remove", Action::App(inst, C_ACC_CONFIRM + i as u32), true);
                continue;
            }
            let buttons = if admin && !me { 208 } else { 0 };
            let name = ui.fit(Face::Semibold, 14, &p.name, right - buttons - tx);
            ui.text(tx, y + 27, Face::Semibold, 14, &name, t.text);
            let mut sub = String::from(if p.admin { "Administrator" } else { "Standard" });
            if me {
                sub.push_str(" · You");
            }
            if p.new {
                sub.push_str(" · Not set up yet");
            }
            let sub = ui.fit(Face::Regular, 12, &sub, right - buttons - tx);
            ui.text(tx, y + 45, Face::Regular, 12, &sub, t.text2);
            if buttons > 0 {
                let label = if p.admin { "Make standard" } else { "Make admin" };
                ui.button(Rect::new(right - 208, y + 14, 120, 32), label, Action::App(inst, C_ACC_TYPE + i as u32), false);
                ui.button(Rect::new(right - 80, y + 14, 80, 32), "Remove", Action::App(inst, C_ACC_REMOVE + i as u32), false);
            }
        }
        let mut y = m.y + n * rh + 22;
        if admin && !sys.people.is_empty() {
            card(ui, Rect::new(m.x, y, m.w, 106));
            ui.text(m.x + 16, y + 26, Face::Semibold, 14, "Add an account", t.text);
            let f = Rect::new(m.x + 16, y + 42, (m.w - 32 - 96 - 150).max(120), 34);
            ui.field(f, &self.acc_name, "Their name", self.focus == C_ACC_NAME, Action::App(inst, C_ACC_NAME));
            ui.text(f.r() + 14, y + 64, Face::Regular, 13, "Administrator", t.text);
            ui.switch(f.r() + 104, y + 48, self.acc_admin, Action::App(inst, C_ACC_ADMIN));
            ui.button(Rect::new(m.r() - 16 - 80, y + 43, 80, 32), "Add", Action::App(inst, C_ACC_ADD), true);
            let hint = "They set up their picture and sign-in the first time they sign in.";
            let hint = ui.fit(Face::Regular, 12, hint, m.w - 32);
            ui.text(m.x + 16, y + 96, Face::Regular, 12, &hint, t.text3);
            y += 120;
        }
        let msg = if !self.acc_msg.is_empty() {
            self.acc_msg.clone()
        } else if admin {
            String::from("Each account has its own files, settings and sign-in. Shared is for everyone.")
        } else {
            String::from("Only an administrator can add or remove accounts.")
        };
        let msg = ui.fit(Face::Regular, 12, &msg, m.w - 16);
        ui.text(m.x + 8, y + 8, Face::Regular, 12, &msg, t.text2);
    }

    /// The Profile section.
    fn render_profile(&mut self, ui: &mut Ui, m: Rect, sys: &Sys, inst: u32) {
        use crate::avatar::Choice;
        use crate::profile::Avatar;
        let t = ui.t;
        let p = &sys.profile;
        let top = m.y;
        card(ui, Rect::new(m.x, top, m.w, 132));
        let d = 84;
        ui.avatar(Rect::new(m.x + 20, top + 24, d, d), &sys.avatar);
        let tx = m.x + 20 + d + 20;
        let tw = m.r() - 16 - tx;
        if self.focus == C_NAME_FIELD {
            let f = Rect::new(tx, top + 24, (tw - 92).min(260), 34);
            ui.field(f, &self.name, "Your name", true, Action::App(inst, C_NAME_FIELD));
            ui.button(Rect::new(f.r() + 8, top + 25, 80, 32), "Save", Action::App(inst, C_NAME_SAVE), true);
        } else {
            let name = if p.ready() { p.name.as_str() } else { "No profile yet" };
            let name = ui.fit(Face::Semibold, 20, name, tw);
            ui.text(tx, top + 46, Face::Semibold, 20, &name, t.text);
        }
        let since = match p.since {
            (0, _, _) => String::from("Local profile on this computer"),
            (y, mo, d) => format!("On this computer since {} {} {}", d, crate::sys::MONTHS[(mo as usize).clamp(1, 12) - 1], y),
        };
        let since = ui.fit(Face::Regular, 12, &since, tw);
        ui.text(tx, top + 70, Face::Regular, 12, &since, t.text2);
        let bw = 118;
        if self.focus != C_NAME_FIELD {
            ui.button(Rect::new(tx, top + 84, bw, 30), "Edit name", Action::App(inst, C_NAME_FIELD), false);
        }
        ui.button(Rect::new(tx + bw + 8, top + 84, 136, 30), if self.picking { "Done" } else { "Change picture" }, Action::App(inst, C_PICTURE), self.picking);
        let mut y = top + 146;
        if self.picking {
            let chosen = self.chosen.unwrap_or(match p.avatar {
                Avatar::Initials(i) => Choice::Initials(i),
                Avatar::Motif(i) => Choice::Motif(i),
                Avatar::Picture => Choice::Photo(0),
            });
            let h = 236;
            card(ui, Rect::new(m.x, y, m.w, h));
            let old = ui.clip_in(Rect::new(m.x, y, m.w, h));
            self.picker.render(ui, Rect::new(m.x + 20, y + 16, m.w - 40, h - 24), 34, &p.name, chosen, |c| Action::App(inst, C_PICK + c as u32));
            ui.set_clip(old);
            // the rest waits until the picture is chosen
            return;
        }
        card(ui, Rect::new(m.x, y, m.w, 64));
        let inner = Rect::new(m.x + 16, y, m.w - 32, 64);
        let st = match (sys.has_pin(), sys.has_password()) {
            (true, true) => "PIN and password",
            (true, false) => "PIN",
            (false, true) => "Password",
            _ => "None: any key or click unlocks",
        };
        row(ui, inner, y + 12, "Sign-in", st);
        ui.button(Rect::new(m.r() - 16 - 150, y + 16, 150, 32), "Sign-in options", Action::App(inst, C_SECTION + 7), false);
        y += 78;
        card(ui, Rect::new(m.x, y, m.w, 64));
        let inner = Rect::new(m.x + 16, y, m.w - 32, 64);
        row(ui, inner, y + 12, "Setup assistant", "Name, picture, sign-in and look");
        ui.button(Rect::new(m.r() - 16 - 150, y + 16, 150, 32), "Set up again", Action::App(inst, C_SETUP), false);
    }
}

const C_BACK: u32 = 12;
const C_LOCK_BOOT: u32 = 13;
const C_IDLE: u32 = 14;
const C_PIN_FIELD: u32 = 15;
const C_PIN_SAVE: u32 = 16;
const C_PIN_REMOVE: u32 = 17;
const C_LOCK_NOW: u32 = 18;
const C_PW_FIELD: u32 = 19;
const C_PW_SAVE: u32 = 20;
const C_PW_REMOVE: u32 = 21;
const C_FINGER: u32 = 22;
const C_NAME_FIELD: u32 = 23;
const C_NAME_SAVE: u32 = 24;
const C_PICTURE: u32 = 25;
const C_SETUP: u32 = 26;
const C_ACC_NAME: u32 = 27;
const C_ACC_ADD: u32 = 28;
const C_ACC_ADMIN: u32 = 29;
const C_ACC_CANCEL: u32 = 30;
const C_SHORTCUTS: u32 = 32;
const C_CLAUDE_KEY: u32 = 33;
const C_CLAUDE_SAVE: u32 = 34;
const C_CLAUDE_REMOVE: u32 = 35;
const C_CLAUDE_MODELS: u32 = 36;
const C_CLAUDE_OPEN: u32 = 37;
const C_KEY_TEST: u32 = 38;
const C_MOTION: u32 = 40;
const C_VOL_DOWN: u32 = 41;
const C_VOL_UP: u32 = 42;
const C_MUTE: u32 = 43;
const C_HAPTICS: u32 = 44;
const C_HPHONE: u32 = 45;
/// + strength index
const C_HSTRENGTH: u32 = 50;
/// + Haptic::ALL index
const C_HAPTIC: u32 = 60;
const C_KEY_TEST_DONE: u32 = 39;
const C_INK: u32 = 46;
const C_INK_CLEAR: u32 = 47;
const C_AUTOBRIGHT: u32 = 48;
const C_BT_SCAN: u32 = 49;
/// Personalisation: + tab, + scene, + colour, + gradient, + picture, + fit,
/// + what it applies to, + theme mode
const C_PTAB: u32 = 1500;
const C_SCENE: u32 = 1510;
const C_SOLID: u32 = 1520;
const C_GRAD: u32 = 1530;
const C_PICS: u32 = 1540;
const C_PICS_BACK: u32 = 1541;
const C_FIT: u32 = 1550;
const C_APPLY: u32 = 1560;
const C_TOD: u32 = 1570;
const C_MODE: u32 = 1580;
const C_ACC_WALL: u32 = 1590;
const C_WIDGETS: u32 = 1591;
const C_PIC: u32 = 1600;
const C_PICK: u32 = 1000;
/// + model index
const C_MODEL: u32 = 1400;
/// + account index
const C_ACC_TYPE: u32 = 1100;
const C_ACC_REMOVE: u32 = 1200;
const C_ACC_CONFIRM: u32 = 1300;
const IDLE_STEPS: [u32; 6] = [0, 2, 5, 10, 15, 30];

fn row(ui: &mut Ui, r: Rect, y: i32, title: &str, sub: &str) {
    let t = ui.t;
    ui.text(r.x, y + 16, Face::Medium, 14, title, t.text);
    if !sub.is_empty() {
        let s = ui.fit(Face::Regular, 12, sub, r.w - 70);
        ui.text(r.x, y + 34, Face::Regular, 12, &s, t.text2);
    }
}

fn card(ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.rrect(r, 14, if t.dark { t.tile } else { Color::rgb(0xFFFFFF).mix(t.surface, 128) });
}

fn kv(ui: &mut Ui, x: i32, y: i32, w: i32, k: &str, v: &str) {
    let t = ui.t;
    ui.text(x, y, Face::Regular, 13, k, t.text2);
    let vw = ui.tw(Face::Medium, 13, v);
    ui.text(x + w - vw, y, Face::Medium, 13, v, t.text);
}

impl App for Settings {
    fn kind(&self) -> AppKind {
        AppKind::Settings
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let compact = super::compact(r);
        if compact && !self.page {
            ui.text_in(Rect::new(r.x + 18, r.y, 150, HEADER), Face::Semibold, 18, "Settings", t.text, 0);
            let mut y = r.y + HEADER + 8;
            for (i, s) in SECTIONS.iter().enumerate() {
                let row = Rect::new(r.x + 12, y, r.w - 24, 44);
                let a = Action::App(inst, C_SECTION + i as u32);
                ui.rrect(row, 12, if ui.hot(a) { t.chip } else { t.sidebar });
                ui.text_in(Rect::new(row.x + 16, row.y, row.w - 40, row.h), Face::Medium, 14, s, t.text, 0);
                ui.icon(crate::icons::Icon::ChevronRight, row.r() - 28, row.y + 14, 16, t.text3);
                ui.zone(row, a);
                y += 50;
            }
            return;
        }
        let side_w = if compact { 0 } else { 180 };
        let mut tx = r.x + side_w + 28;
        if compact {
            ui.icon_button(Rect::new(r.x + 10, r.y + 8, 28, 28), crate::icons::Icon::ChevronLeft, Action::App(inst, C_BACK), 16);
            tx = r.x + 44;
        } else {
            super::panel(ui, r, Rect::new(r.x, r.y, side_w, r.h), t.sidebar);
            ui.text_in(Rect::new(r.x + 18, r.y, 150, HEADER), Face::Semibold, 15, "Settings", t.text, 0);
            let mut y = r.y + HEADER + 4;
            for (i, s) in SECTIONS.iter().enumerate() {
                side_item(ui, Rect::new(r.x + 8, y, side_w - 16, 28), s, i == self.sec, Action::App(inst, C_SECTION + i as u32));
                y += 30;
            }
        }
        let pad = if compact { 14 } else { 28 };
        let m = Rect::new(r.x + side_w + pad, r.y + HEADER + 10, r.w - side_w - 2 * pad, r.h - HEADER - 20);
        ui.text_in(Rect::new(tx, r.y, 300, HEADER), Face::Semibold, 15, SECTIONS[self.sec], t.text, 0);
        ui.rect(Rect::new(r.x + side_w, r.y + HEADER, r.w - side_w, 1), t.line);
        let sw_x = m.r() - 54;
        match self.sec {
            0 => self.render_profile(ui, m, sys, inst),
            1 => self.render_accounts(ui, m, sys, inst),
            3 => self.render_keyboard(ui, m, sys, inst),
            10 => self.render_assistant(ui, m, sys, inst),
            11 => self.render_sound(ui, m, sys, inst),
            12 => self.render_devices(ui, m, sys, inst),
            2 => self.render_personal(ui, m, sys, inst),
            4 => {
                let n = &sys.net;
                card(ui, Rect::new(m.x, m.y, m.w, 210));
                let (status, sub) = match (n.present, n.ip) {
                    (false, _) => ("No network adapter", String::from("Plug in Ethernet; Wi-Fi drivers are on the roadmap")),
                    (true, None) if !n.link_up => ("Cable unplugged", n.name.clone()),
                    (true, None) => ("Connecting...", n.name.clone()),
                    (true, Some(_)) => ("Connected", n.name.clone()),
                };
                row(ui, Rect::new(m.x + 16, m.y, m.w - 32, 64), m.y + 12, status, &sub);
                let ip = n.ip.map(crate::net::ip_str).unwrap_or_else(|| String::from("-"));
                kv(ui, m.x + 16, m.y + 84, m.w - 32, "IP address", &ip);
                kv(ui, m.x + 16, m.y + 108, m.w - 32, "Router", &if n.ip.is_some() { crate::net::ip_str(n.gw) } else { String::from("-") });
                kv(ui, m.x + 16, m.y + 132, m.w - 32, "DNS", &if n.ip.is_some() { crate::net::ip_str(n.dns) } else { String::from("-") });
                kv(ui, m.x + 16, m.y + 156, m.w - 32, "Name", &if n.host.is_empty() { String::from("-") } else { alloc::format!("{}.local", n.host) });
                kv(ui, m.x + 16, m.y + 180, m.w - 32, "Packets in / out", &alloc::format!("{} / {}", n.rx, n.tx));
                card(ui, Rect::new(m.x, m.y + 224, m.w, 64));
                let wsub = match &sys.wifi_nets {
                    Some(n) if n.is_empty() => String::from("Scanning through the firmware's Wi-Fi driver..."),
                    Some(n) => format!("{} networks nearby", n.len()),
                    None => String::from("This Wi-Fi chip has no driver yet; use Ethernet"),
                };
                row(ui, Rect::new(m.x + 16, m.y + 224, m.w - 32, 64), m.y + 236, "Wi-Fi", &wsub);
                ui.switch(sw_x, m.y + 244, sys.wifi, Action::App(inst, C_WIFI));
                if let Some(nets) = &sys.wifi_nets {
                    let mut y = m.y + 300;
                    for (ssid, sec, q) in nets.iter() {
                        if y + 36 > m.b() {
                            break;
                        }
                        card(ui, Rect::new(m.x, y, m.w, 32));
                        let label = if ssid.is_empty() { "(hidden network)" } else { ssid.as_str() };
                        ui.text(m.x + 14, y + 21, Face::Medium, 13, label, t.text);
                        let bars = match q { 75.. => "▂▄▆█", 50..=74 => "▂▄▆", 25..=49 => "▂▄", _ => "▂" };
                        ui.text_in(Rect::new(m.r() - 200, y, 186, 32), Face::Regular, 12, &format!("{} · {}", sec, bars), t.text2, 2);
                        y += 36;
                    }
                }
            }
            5 => {
                card(ui, Rect::new(m.x, m.y, m.w, 64));
                let sub = match &sys.bt_adapter {
                    Some(a) if sys.bt => a.clone(),
                    Some(_) => String::from("Off"),
                    None => String::from("No Bluetooth adapter found"),
                };
                row(ui, Rect::new(m.x + 16, m.y, m.w - 32, 64), m.y + 12, "Bluetooth", &sub);
                ui.switch(sw_x, m.y + 20, sys.bt, Action::App(inst, C_BT));
                if sys.bt_adapter.is_none() {
                    card(ui, Rect::new(m.x, m.y + 78, m.w, 100));
                    let lines = ["HydatekOS drives USB Bluetooth adapters (and the ones inside", "laptops that sit on USB). Plug one in and it appears here.", "Phone Link also works over your home network."];
                    for (i, l) in lines.iter().enumerate() {
                        let l = ui.fit(Face::Regular, 13, l, m.w - 32);
                        ui.text(m.x + 16, m.y + 104 + i as i32 * 22, Face::Regular, 13, &l, t.text2);
                    }
                } else if sys.bt {
                    let top = m.y + 78;
                    ui.text(m.x + 4, top + 16, Face::Semibold, 14, "Nearby", t.text);
                    ui.button(Rect::new(m.r() - 90, top, 90, 26), "Scan again", Action::App(inst, C_BT_SCAN), false);
                    let mut y = top + 34;
                    if sys.bt_nearby.is_empty() {
                        ui.text(m.x + 4, y + 16, Face::Regular, 13, "Looking for devices...", t.text2);
                    }
                    for (name, kind, rssi) in sys.bt_nearby.iter() {
                        if y + 40 > m.b() {
                            break;
                        }
                        card(ui, Rect::new(m.x, y, m.w, 36));
                        let nm = ui.fit(Face::Medium, 13, name, m.w - 180);
                        ui.text(m.x + 14, y + 23, Face::Medium, 13, &nm, t.text);
                        let right = if *rssi > -127 { format!("{} · {} dBm", kind, rssi) } else { String::from(*kind) };
                        ui.text_in(Rect::new(m.r() - 170, y, 156, 36), Face::Regular, 12, &right, t.text2, 2);
                        y += 40;
                    }
                }
            }
            6 => {
                let l = &sys.link;
                card(ui, Rect::new(m.x, m.y, m.w, 130));
                let st = match l.source {
                    crate::link::Source::None => "Not paired",
                    crate::link::Source::Demo => "Demo phone",
                    _ if l.online => "Connected",
                    _ => "Paired, offline",
                };
                let name = if l.device.is_empty() { "No phone" } else { l.device.as_str() };
                row(ui, Rect::new(m.x + 16, m.y, m.w, 64), m.y + 12, name, st);
                if l.paired {
                    kv(ui, m.x + 16, m.y + 78, m.w - 32, "Battery", &format!("{}%", l.battery));
                    let kind = match l.kind.as_str() {
                        "android" => "HydatekOS Link for Android",
                        "web" => "Browser companion",
                        "demo" => "Simulated",
                        _ => "-",
                    };
                    kv(ui, m.x + 16, m.y + 102, m.w - 32, "Connection", kind);
                } else {
                    ui.text(m.x + 16, m.y + 84, Face::Regular, 13, "Open Phone Link and scan the code with your phone.", t.text2);
                }
                ui.button(Rect::new(m.x, m.y + 146, 150, 32), "Open Phone Link", Action::App(inst, C_OPEN_LINK), true);
                if l.paired {
                    ui.button(Rect::new(m.x + 160, m.y + 146, 110, 32), "Unpair", Action::App(inst, C_UNPAIR), false);
                }
            }
            7 => {
                card(ui, Rect::new(m.x, m.y, m.w, 124));
                let inner = Rect::new(m.x + 16, m.y, m.w - 32, 124);
                row(ui, inner, m.y + 12, "Show at startup", "Lock the screen when HydatekOS starts");
                ui.switch(sw_x, m.y + 18, sys.lock_on_boot, Action::App(inst, C_LOCK_BOOT));
                let idle = if sys.lock_idle == 0 { String::from("Never") } else { alloc::format!("After {} min", sys.lock_idle) };
                row(ui, inner, m.y + 62, "Lock when idle", "Lock after no keyboard or mouse input");
                ui.button(Rect::new(m.r() - 128, m.y + 70, 112, 30), &idle, Action::App(inst, C_IDLE), false);

                // sign-in options: one row each; Set/Change opens the field in place
                let top = m.y + 136;
                card(ui, Rect::new(m.x, top, m.w, 166));
                let inner = Rect::new(m.x + 16, top, m.w - 32, 166);
                let focus = self.focus;
                let secret = |ui: &mut Ui, y: i32, title: &str, status: &str, typed: &str, hint: &str, set: bool, codes: (u32, u32, u32)| {
                    let right = inner.r();
                    if focus == codes.0 {
                        row(ui, inner, y, title, "");
                        let f = Rect::new(right - 88 - 170, y + 9, 170, 32);
                        let dots: String = typed.chars().map(|_| '•').collect();
                        ui.field(f, &dots, hint, true, Action::App(inst, codes.0));
                        ui.button(Rect::new(right - 80, y + 9, 80, 32), "Save", Action::App(inst, codes.1), true);
                    } else {
                        row(ui, inner, y, title, status);
                        let rm = if set { 88 } else { 0 };
                        ui.button(Rect::new(right - rm - 90, y + 9, 90, 32), if set { "Change" } else { "Set" }, Action::App(inst, codes.0), !set);
                        if set {
                            ui.button(Rect::new(right - 80, y + 9, 80, 32), "Remove", Action::App(inst, codes.2), false);
                        }
                    }
                };
                let pin_st = if sys.has_pin() { "Set" } else { "Not set" };
                secret(ui, top + 8, "PIN", pin_st, &self.pin, "4-8 digits", sys.has_pin(), (C_PIN_FIELD, C_PIN_SAVE, C_PIN_REMOVE));
                let pw_st = if sys.has_password() { "Set" } else { "Not set" };
                secret(ui, top + 58, "Password", pw_st, &self.password, "6+ characters", sys.has_password(), (C_PW_FIELD, C_PW_SAVE, C_PW_REMOVE));
                let fp_st = if !sys.secured() {
                    String::from("Set a PIN or password first")
                } else if !sys.lock_finger {
                    String::from("Confirm on your paired Android phone")
                } else if sys.finger_ready() {
                    format!("{} is ready", sys.link.device)
                } else {
                    String::from("Needs a paired phone with a fingerprint")
                };
                row(ui, inner, top + 108, "Fingerprint (phone)", &fp_st);
                ui.switch(sw_x, top + 114, sys.lock_finger, Action::App(inst, C_FINGER));
                let note = if self.focus == C_PW_FIELD && crate::input::caps_lock() == Some(true) {
                    "Caps Lock is on."
                } else if self.pin_msg.is_empty() {
                    if sys.secured() { "Keeps people out of your session; it doesn't encrypt files." } else { "No PIN or password: any key or click unlocks." }
                } else {
                    self.pin_msg.as_str()
                };
                let note = ui.fit(Face::Regular, 12, note, m.w - 16);
                ui.text(m.x + 8, top + 186, Face::Regular, 12, &note, t.text2);
                ui.button(Rect::new(m.x, top + 202, 130, 32), "Lock now", Action::App(inst, C_LOCK_NOW), true);
                ui.text(m.x + 142, top + 222, Face::Regular, 12, "or press F12", t.text3);
            }
            8 => {
                use crate::web::engines::ENGINES;
                ui.text(m.x, m.y + 14, Face::Semibold, 14, "Search engine", t.text);
                let tip = ui.fit(Face::Regular, 12, "For searches typed in the address bar. Add !d, !g, !w... to a search to use another once.", m.w);
                ui.text(m.x, m.y + 34, Face::Regular, 12, &tip, t.text2);
                let rh = 42;
                let top = m.y + 50;
                card(ui, Rect::new(m.x, top, m.w, rh * ENGINES.len() as i32 + 8));
                for (i, e) in ENGINES.iter().enumerate() {
                    let y = top + 4 + i as i32 * rh;
                    let rr = Rect::new(m.x + 4, y, m.w - 8, rh);
                    let a = Action::App(inst, C_ENGINE + i as u32);
                    if ui.hot(a) {
                        ui.rrect(rr, 10, t.hover);
                    }
                    let on = sys.search_engine == e.id;
                    ui.circle(m.x + 26, y + rh / 2, 9, if on { t.accent } else { t.line });
                    ui.circle(m.x + 26, y + rh / 2, if on { 4 } else { 7 }, if on { Color::rgb(0xFFFFFF) } else { t.surface });
                    let title = format!("{}   !{}", e.name, e.key);
                    ui.text(m.x + 46, y + 18, Face::Medium, 14, &title, t.text);
                    let sub = ui.fit(Face::Regular, 12, e.about, m.w - 70);
                    ui.text(m.x + 46, y + 34, Face::Regular, 12, &sub, t.text2);
                    ui.zone(rr, a);
                }
            }
            9 => {
                let hw = &sys.hw;
                card(ui, Rect::new(m.x, m.y, m.w, 130));
                let (w, h, s) = sys.screen;
                kv(ui, m.x + 16, m.y + 30, m.w - 32, "Resolution", &format!("{} × {}", w, h));
                kv(ui, m.x + 16, m.y + 56, m.w - 32, "Scale", &format!("{}×", s));
                kv(ui, m.x + 16, m.y + 82, m.w - 32, "Layout", &format!("{} × {} points", w / s, h / s));
                let cores = crate::par::count();
                let simd = if sys.hw.features.contains(&"AVX2") { "AVX2" } else if cfg!(target_arch = "aarch64") { "NEON" } else { "SSE2" };
                let fr = if sys.frames > 0 { format!(" · frame {:.1} + {:.1} ms", sys.frame_us.0 as f32 / 1000.0, sys.frame_us.1 as f32 / 1000.0) } else { String::new() };
                let _ = cores;
                #[allow(static_mut_refs)]
                let how = unsafe { crate::par::DECISION };
                kv(ui, m.x + 16, m.y + 108, m.w - 32, "Renderer", &format!("{}, {}{}", how, simd, fr));
                // the graphics hardware
                let top = m.y + 144;
                let n = hw.gpus.len().max(1) as i32;
                card(ui, Rect::new(m.x, top, m.w, 44 + n * 24));
                ui.text(m.x + 16, top + 26, Face::Semibold, 14, "Graphics", t.text);
                for (i, g) in hw.gpus.iter().enumerate() {
                    let y = top + 50 + i as i32 * 24;
                    let name = ui.fit(Face::Regular, 13, &g.name, m.w - 150);
                    ui.text(m.x + 16, y, Face::Regular, 13, &name, t.text2);
                    let ids = if g.ids.is_empty() { "built in" } else { g.ids.as_str() };
                    let iw = ui.tw(Face::Mono, 12, ids);
                    ui.text(m.r() - 16 - iw, y, Face::Mono, 12, ids, t.text3);
                }
                // the firmware's screen modes
                let top = top + 58 + n * 24;
                ui.text(m.x, top + 14, Face::Semibold, 14, "Screen modes", t.text);
                let mut x = m.x;
                let mut y = top + 26;
                for &(mw, mh) in hw.modes.iter().take(12) {
                    let label = format!("{} × {}", mw, mh);
                    let cw = ui.tw(Face::Medium, 12, &label) + 22;
                    if x + cw > m.r() {
                        x = m.x;
                        y += 32;
                    }
                    let on = (mw, mh) == hw.mode;
                    ui.rrect(Rect::new(x, y, cw, 26), 13, if on { t.accent } else { t.chip });
                    ui.text_in(Rect::new(x, y, cw, 26), Face::Medium, 12, &label, if on { t.on_accent } else { t.text }, 1);
                    x += cw + 6;
                }
                let note = "The firmware's driver draws the screen; the mode is chosen at start-up.";
                let note = ui.fit(Face::Regular, 12, note, m.w);
                ui.text(m.x, y + 48, Face::Regular, 12, &note, t.text3);
                // brightness and the room's light
                let top = y + 62;
                card(ui, Rect::new(m.x, top, m.w, 56));
                let sub = match sys.lux {
                    Some(l) if sys.auto_brightness => format!("{}% · {} lux in the room", sys.brightness, l),
                    Some(l) => format!("{}% · the light sensor reads {} lux", sys.brightness, l),
                    None => format!("{}% · no light sensor found", sys.brightness),
                };
                row(ui, Rect::new(m.x + 16, top, m.w - 32, 56), top + 2, "Follow the room's light", &sub);
                ui.switch(m.r() - 54, top + 14, sys.auto_brightness, Action::App(inst, C_AUTOBRIGHT));
            }
            _ => {
                let hw = &sys.hw;
                let (used, total) = crate::heap::HEAP.stats();
                let cores = match (hw.cores, hw.threads) {
                    (0, _) => String::from("-"),
                    (c, t) if t > c => format!("{} cores, {} threads", c, t),
                    (c, _) => format!("{} cores", c),
                };
                let cores = if hw.max_mhz >= 500 { format!("{}, up to {}.{} GHz", cores, hw.max_mhz / 1000, hw.max_mhz % 1000 / 100) } else { cores };
                let mut cpu = hw.cpu.clone();
                if !hw.core.is_empty() {
                    cpu = format!("{} ({})", cpu, hw.core);
                }
                let gpu = hw.gpus.first().map(|g| g.name.clone()).unwrap_or_else(|| String::from("-"));
                let feats = hw.features.join(" · ");
                let mut rows: Vec<(&str, String)> = Vec::new();
                if !hw.machine.is_empty() {
                    rows.push(("Computer", hw.machine.clone()));
                }
                rows.push(("Processor", cpu));
                rows.push(("Cores", cores));
                if !feats.is_empty() {
                    rows.push(("Features", feats));
                }
                rows.push(("Graphics", gpu));
                rows.push(("System memory", format!("{} MB", sys.mem_total >> 20)));
                rows.push(("Kernel heap", format!("{} / {} MB", used >> 20, total >> 20)));
                rows.push(("Storage", String::from(if sys.fs.persistent { "Boot disk \\HYDATEK" } else { "Live session (read-only disk)" })));
                rows.push(("Firmware", if hw.bios.is_empty() { sys.firmware.clone() } else { hw.bios.clone() }));
                rows.push(("Architecture", format!("{} UEFI{}", hw.arch, if hw.snapdragon { " · Snapdragon" } else { "" })));
                let h = 80 + rows.len() as i32 * 24;
                card(ui, Rect::new(m.x, m.y, m.w, h));
                ui.text(m.x + 16, m.y + 36, Face::Semibold, 24, "HydatekOS", t.text);
                ui.text(m.x + 16, m.y + 58, Face::Regular, 13, "Version 0.1 \"Dune\" · milestone 1", t.text2);
                for (i, (k, v)) in rows.iter().enumerate() {
                    // long values (a processor's full name) are cut to fit
                    let v = ui.fit(Face::Medium, 13, v, m.w - 32 - 130);
                    kv(ui, m.x + 16, m.y + 92 + i as i32 * 24, m.w - 32, k, &v);
                }
                ui.button(Rect::new(m.x, m.y + h + 16, 110, 32), "Restart", Action::App(inst, C_RESTART), false);
                ui.button(Rect::new(m.x + 120, m.y + h + 16, 110, 32), "Shut down", Action::App(inst, C_SHUTDOWN), true);
            }
        }
    }

    fn mouse(&mut self, x: i32, y: i32) {
        self.at = (x, y);
    }

    fn scroll(&mut self, dy: i32) {
        if self.sec == 12 {
            self.dev_top = if dy > 0 { self.dev_top + 1 } else { self.dev_top.saturating_sub(1) };
        }
    }

    fn drag(&mut self, x: i32, y: i32) {
        self.at = (x, y);
        if self.inking {
            let (w, _) = Self::ink_width();
            let p = ((x - self.pad.x).clamp(0, self.pad.w), (y - self.pad.y).clamp(0, self.pad.h), w);
            if let Some(s) = self.ink.last_mut() {
                if s.len() < 2000 && s.last() != Some(&p) {
                    s.push(p);
                }
            }
        }
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        let was = self.focus;
        self.focus = if [C_PIN_FIELD, C_PW_FIELD, C_NAME_FIELD, C_NAME_SAVE, C_ACC_NAME, C_ACC_ADMIN, C_CLAUDE_KEY].contains(&code) { code } else { 0 };
        if self.focus == C_ACC_ADMIN {
            self.focus = C_ACC_NAME;
        }
        if !(C_ACC_REMOVE..C_ACC_REMOVE + 16).contains(&code) {
            self.acc_confirm = None;
        }
        // switches click
        if code != C_INK {
            self.inking = false;
        }
        if [C_DARK, C_MOBILE, C_WIFI, C_BT, C_FOCUS, C_MOTION, C_LOCK_BOOT, C_FINGER, C_MUTE, C_HPHONE, C_ACC_ADMIN, C_AUTOBRIGHT, C_TOD, C_WIDGETS].contains(&code) {
            sys.feel(crate::haptics::Haptic::Click);
        }
        match code {
            C_VOL_DOWN | C_VOL_UP | C_MUTE => {
                use crate::ui::Media;
                sys.reqs.push(Req::Media(match code {
                    C_VOL_DOWN => Media::VolumeDown,
                    C_VOL_UP => Media::VolumeUp,
                    _ => Media::Mute,
                }));
                return;
            }
            C_HAPTICS => {
                sys.haptics.on = !sys.haptics.on;
                sys.feel(crate::haptics::Haptic::Click);
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            C_INK => {
                self.inking = true;
                let (w, _) = Self::ink_width();
                self.ink.push(alloc::vec![(self.at.0 - self.pad.x, self.at.1 - self.pad.y, w)]);
                if self.ink.len() > 60 {
                    self.ink.remove(0);
                }
                return;
            }
            C_INK_CLEAR => {
                self.ink.clear();
                return;
            }
            C_BT_SCAN => {
                sys.bt_scan = true;
                return;
            }
            c if (C_PTAB..C_PTAB + 3).contains(&c) => {
                self.ptab = (c - C_PTAB) as u8;
                self.pics = None;
                return;
            }
            c if (C_APPLY..C_APPLY + 3).contains(&c) => {
                self.apply = (c - C_APPLY) as u8;
                return;
            }
            c if (C_SCENE..C_SCENE + 7).contains(&c) => {
                let sc = crate::personal::Scene::ALL[(c - C_SCENE) as usize];
                self.set_wall(sys, crate::personal::Wall::Scene(sc));
                return;
            }
            c if (C_SOLID..C_SOLID + 6).contains(&c) => {
                self.set_wall(sys, crate::personal::Wall::Solid(crate::personal::SOLIDS[(c - C_SOLID) as usize]));
                return;
            }
            c if (C_GRAD..C_GRAD + 4).contains(&c) => {
                let g = crate::personal::GRADIENTS[(c - C_GRAD) as usize];
                self.set_wall(sys, crate::personal::Wall::Gradient(g.0, g.1));
                return;
            }
            C_PICS => {
                let t = crate::theme::theme(sys.dark, sys.accent);
                self.find_pictures(sys, &t);
                return;
            }
            C_PICS_BACK => {
                self.pics = None;
                return;
            }
            c if (C_PIC..C_PIC + 16).contains(&c) => {
                let path = self.pics.as_ref().and_then(|p| p.get((c - C_PIC) as usize)).map(|p| p.0.clone());
                if let Some(p) = path {
                    self.set_wall(sys, crate::personal::Wall::Picture(p));
                }
                self.pics = None;
                return;
            }
            c if (C_FIT..C_FIT + 5).contains(&c) => {
                sys.look.fit = crate::personal::Fit::ALL[(c - C_FIT) as usize];
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            C_TOD => {
                sys.look.time_of_day = !sys.look.time_of_day;
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            c if (C_MODE..C_MODE + 3).contains(&c) => {
                sys.look.mode = crate::personal::Mode::ALL[(c - C_MODE) as usize];
                sys.follow_mode();
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            C_ACC_WALL => {
                sys.look.accent = crate::personal::Accent::FromWall;
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            C_WIDGETS => {
                sys.look.widgets = !sys.look.widgets;
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            C_AUTOBRIGHT => {
                sys.auto_brightness = !sys.auto_brightness;
                sys.bright_bias = 0;
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            C_HPHONE => {
                sys.haptics.phone = !sys.haptics.phone;
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            c if (C_HSTRENGTH..C_HSTRENGTH + 3).contains(&c) => {
                sys.haptics.strength = crate::haptics::Strength::ALL[(c - C_HSTRENGTH) as usize];
                sys.feel(crate::haptics::Haptic::Tap);
                sys.reqs.push(Req::SaveSettings);
                return;
            }
            c if (C_HAPTIC..C_HAPTIC + 7).contains(&c) => {
                sys.feel(crate::haptics::Haptic::ALL[(c - C_HAPTIC) as usize]);
                return;
            }
            C_KEY_TEST => {
                self.tester = Some(Tester::new());
                sys.key_test = true;
                return;
            }
            C_KEY_TEST_DONE => {
                self.tester = None;
                sys.key_test = false;
                return;
            }
            C_SHORTCUTS => {
                sys.reqs.push(Req::Shortcuts);
                return;
            }
            C_ACC_NAME | C_ACC_CANCEL => return,
            C_ACC_ADMIN => {
                self.acc_admin = !self.acc_admin;
                return;
            }
            C_ACC_ADD => {
                let admin = self.acc_admin;
                self.acc_msg = match sys.add_account(&self.acc_name, admin) {
                    Ok(_) => {
                        let msg = format!("Added {}. They can sign in from the lock screen.", crate::profile::first_name(&crate::profile::clean_name(&self.acc_name).unwrap_or_default()));
                        self.acc_name.clear();
                        self.acc_admin = false;
                        msg
                    }
                    Err(e) => {
                        self.focus = C_ACC_NAME;
                        String::from(e)
                    }
                };
                return;
            }
            c if (C_ACC_TYPE..C_ACC_TYPE + 16).contains(&c) => {
                if let Some(p) = sys.people.get((c - C_ACC_TYPE) as usize) {
                    let (id, admin) = (p.id.clone(), !p.admin);
                    self.acc_msg = match sys.set_admin(&id, admin) {
                        Ok(()) => String::new(),
                        Err(e) => String::from(e),
                    };
                }
                return;
            }
            c if (C_ACC_REMOVE..C_ACC_REMOVE + 16).contains(&c) => {
                if let Some(p) = sys.people.get((c - C_ACC_REMOVE) as usize) {
                    if sys.accounts.removable(&p.id) && p.id != sys.user {
                        self.acc_confirm = Some(p.id.clone());
                        self.acc_msg.clear();
                    } else if p.id == crate::accounts::FIRST {
                        self.acc_msg = String::from("The first account holds the computer's original folders, so it stays.");
                    } else {
                        self.acc_msg = String::from("There must be at least one administrator.");
                    }
                }
                return;
            }
            c if (C_ACC_CONFIRM..C_ACC_CONFIRM + 16).contains(&c) => {
                if let Some(p) = sys.people.get((c - C_ACC_CONFIRM) as usize) {
                    let (id, name) = (p.id.clone(), String::from(crate::profile::first_name(&p.name)));
                    self.acc_msg = match sys.remove_account(&id) {
                        Ok(()) => format!("Removed {}'s account and files.", name),
                        Err(e) => String::from(e),
                    };
                }
                return;
            }
            C_CLAUDE_KEY => {
                if was != C_CLAUDE_KEY {
                    self.claude_key.clear();
                }
                return;
            }
            C_CLAUDE_SAVE => {
                let k = String::from(self.claude_key.text.trim());
                if crate::web::claude::key_ok(&k) {
                    sys.claude_key = k;
                    sys.claude_model.clear();
                    sys.save_assistant();
                    self.claude_key.clear();
                    self.models.clear();
                    self.claude_msg = String::from("Key saved. Claude is ready.");
                } else {
                    self.focus = C_CLAUDE_KEY;
                    self.claude_msg = String::from("That isn't an Anthropic API key: they start with sk-ant-");
                }
                return;
            }
            C_CLAUDE_REMOVE => {
                sys.claude_key.clear();
                sys.claude_model.clear();
                sys.save_assistant();
                self.models.clear();
                self.claude_msg = String::from("Key removed from this computer.");
                return;
            }
            C_CLAUDE_MODELS => {
                if sys.has_claude() && self.listing.is_none() {
                    self.listing = Some(sys.web.get_api(crate::web::claude::MODELS_URL, crate::web::claude::headers(&sys.claude_key)));
                    self.claude_msg.clear();
                }
                return;
            }
            C_CLAUDE_OPEN => {
                sys.reqs.push(Req::Open(AppKind::Assistant));
                return;
            }
            c if (C_MODEL..C_MODEL + 6).contains(&c) => {
                if let Some(m) = self.models.get((c - C_MODEL) as usize) {
                    sys.claude_model = m.0.clone();
                    sys.save_assistant();
                    self.claude_msg = format!("Claude will use {}.", m.1);
                }
                return;
            }
            C_NAME_FIELD => {
                if was != C_NAME_FIELD {
                    self.name = sys.profile.name.clone();
                }
                return;
            }
            C_NAME_SAVE => {
                self.focus = 0;
                if let Some(n) = crate::profile::clean_name(&self.name) {
                    sys.profile.name = n;
                    sys.save_profile();
                    self.chosen = None;
                } else {
                    self.focus = C_NAME_FIELD;
                }
                return;
            }
            C_PICTURE => {
                self.picking = !self.picking;
                if self.picking {
                    self.picker = Default::default();
                    self.chosen = None;
                    self.picker.load(&sys.fs, sys.photo.as_deref());
                }
                return;
            }
            C_SETUP => {
                sys.reqs.push(Req::Setup);
                return;
            }
            c if c >= C_PICK => {
                use crate::avatar::{Choice, Picker};
                use crate::profile::Avatar;
                let ch = Picker::choice((c - C_PICK) as u16);
                sys.profile.avatar = match ch {
                    Choice::Initials(i) => Avatar::Initials(i),
                    Choice::Motif(i) => Avatar::Motif(i),
                    Choice::Photo(i) => match self.picker.photos.get(i) {
                        Some(ph) => {
                            sys.photo = Some(ph.1.clone());
                            Avatar::Picture
                        }
                        None => return,
                    },
                };
                self.chosen = Some(ch);
                if sys.profile.ready() {
                    sys.save_profile();
                } else {
                    sys.refresh_avatar();
                }
                return;
            }
            C_LOCK_BOOT => sys.lock_on_boot = !sys.lock_on_boot,
            C_IDLE => {
                let i = IDLE_STEPS.iter().position(|v| *v == sys.lock_idle).unwrap_or(0);
                sys.lock_idle = IDLE_STEPS[(i + 1) % IDLE_STEPS.len()];
            }
            C_PIN_FIELD | C_PW_FIELD => return,
            C_PIN_SAVE => {
                let pin = core::mem::take(&mut self.pin);
                self.pin_msg = if sys.set_pin(Some(&pin)) {
                    String::from("PIN saved. You'll need it to unlock.")
                } else {
                    String::from("Use 4 to 8 digits.")
                };
                return;
            }
            C_PIN_REMOVE => {
                sys.set_pin(None);
                self.pin_msg = String::from("PIN removed.");
                return;
            }
            C_PW_SAVE => {
                let pw = core::mem::take(&mut self.password);
                self.pin_msg = if sys.set_password(Some(&pw)) {
                    String::from("Password saved. You can use it to unlock.")
                } else {
                    String::from("Use 6 to 64 characters.")
                };
                return;
            }
            C_PW_REMOVE => {
                sys.set_password(None);
                self.pin_msg = String::from("Password removed.");
                return;
            }
            C_FINGER => {
                let on = !sys.lock_finger;
                self.pin_msg = if !sys.set_finger(on) {
                    String::from("Set a PIN or password first: it's needed when the phone isn't around.")
                } else if on {
                    String::from("Fingerprint on. Choose it under Sign-in options on the lock screen.")
                } else {
                    String::from("Fingerprint unlock off.")
                };
                return;
            }
            C_LOCK_NOW => {
                sys.reqs.push(Req::Lock);
                return;
            }
            C_DARK => { let d = !sys.dark; sys.set_dark(d) }
            C_MOBILE => sys.mobile_shell = !sys.mobile_shell,
            C_WIFI => sys.wifi = !sys.wifi,
            C_BT => sys.bt = !sys.bt,
            C_FOCUS => sys.focus = !sys.focus,
            C_MOTION => sys.reduce_motion = !sys.reduce_motion,
            C_PTR_DOWN => sys.pointer_speed = (sys.pointer_speed - 1).max(1),
            C_PTR_UP => sys.pointer_speed = (sys.pointer_speed + 1).min(9),
            C_OPEN_LINK => sys.reqs.push(Req::Open(AppKind::PhoneLink)),
            C_UNPAIR => sys.unpair(),
            C_BACK => {
                self.page = false;
                return;
            }
            C_RESTART => sys.reqs.push(Req::Reboot),
            C_SHUTDOWN => sys.reqs.push(Req::Shutdown),
            c if c >= C_ENGINE => {
                if let Some(e) = crate::web::engines::ENGINES.get((c - C_ENGINE) as usize) {
                    sys.search_engine = alloc::string::ToString::to_string(e.id);
                }
            }
            c if c >= C_ACCENT => {
                sys.accent = (c - C_ACCENT) as usize;
                sys.look.accent = crate::personal::Accent::Preset(sys.accent as u8);
            }
            c if c >= C_SECTION => {
                self.sec = ((c - C_SECTION) as usize).min(SECTIONS.len() - 1);
                self.page = true;
            }
            _ => return,
        }
        if code < C_SECTION || code >= C_ACCENT {
            sys.reqs.push(Req::SaveSettings);
        }
    }

    fn key(&mut self, k: Key, gen: bool, sys: &mut Sys) {
        if self.tester.is_some() {
            // the tester reads keys as the firmware sends them (tick)
            return;
        }
        if self.focus == C_CLAUDE_KEY {
            match k {
                Key::Enter => self.action(C_CLAUDE_SAVE, false, sys),
                Key::Esc => self.focus = 0,
                Key::Char(' ') => {}
                _ => {
                    self.claude_key.key_sys(k, gen, sys);
                }
            }
            return;
        }
        if self.focus == C_ACC_NAME {
            match k {
                Key::Char(c) if !c.is_control() && self.acc_name.chars().count() < crate::profile::NAME_MAX => self.acc_name.push(c),
                Key::Backspace => {
                    self.acc_name.pop();
                }
                Key::Enter => self.action(C_ACC_ADD, false, sys),
                Key::Esc => self.focus = 0,
                _ => {}
            }
            return;
        }
        if self.focus == C_NAME_FIELD {
            match k {
                Key::Char(c) if !c.is_control() && self.name.chars().count() < crate::profile::NAME_MAX => self.name.push(c),
                Key::Backspace => {
                    self.name.pop();
                }
                Key::Enter => self.action(C_NAME_SAVE, false, sys),
                Key::Esc => self.focus = 0,
                _ => {}
            }
            return;
        }
        let (text, save, max) = match self.focus {
            C_PIN_FIELD => (&mut self.pin, C_PIN_SAVE, 8),
            C_PW_FIELD => (&mut self.password, C_PW_SAVE, 64),
            _ => {
                // no box being typed in: ↑ and ↓ go through the sections
                // (keyboards without a pointer, like ARM firmware's)
                match k {
                    Key::Up => self.sec = self.sec.saturating_sub(1),
                    Key::Down => self.sec = (self.sec + 1).min(SECTIONS.len() - 1),
                    _ => return,
                }
                self.page = true;
                self.picking = false;
                return;
            }
        };
        match k {
            Key::Char(c) if text.chars().count() < max && (save == C_PW_SAVE && !c.is_control() || c.is_ascii_digit()) => text.push(c),
            Key::Backspace => {
                text.pop();
            }
            Key::Enter => self.action(save, false, sys),
            Key::Esc => self.focus = 0,
            _ => {}
        }
    }

    fn animating(&self) -> bool {
        // the Sound & haptics pattern's playhead moves too
        self.focus != 0 || self.tester.is_some() || self.sec == 11
    }

    fn close(&mut self, sys: &mut Sys) {
        if self.tester.take().is_some() {
            sys.key_test = false;
        }
    }

    fn tick(&mut self, sys: &mut Sys) {
        if let Some(ts) = self.tester.as_mut() {
            if ts.read(sys.ticks) || !sys.key_test {
                // Esc twice, or the shell stopped the test (another window)
                self.tester = None;
                sys.key_test = false;
            }
        }
        if let Some(id) = self.listing {
            if let Some(res) = sys.web.take(id) {
                self.listing = None;
                match res.and_then(|r| crate::web::claude::models(r.status, &r.body)) {
                    // Claude models only, newest first
                    Ok(list) => {
                        self.models = list.into_iter().filter(|m| m.0.starts_with("claude")).collect();
                        self.claude_msg = String::from("Choose a model. The newest Opus is the default.");
                    }
                    Err(e) => self.claude_msg = e,
                }
            }
        }
        if let Some(i) = sys.settings_page.take() {
            self.sec = i.min(SECTIONS.len() - 1);
            self.page = true;
        }
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            3 => SECTIONS.iter().enumerate().map(|(i, s)| (*s, C_SECTION + i as u32)).collect(),
            _ => vec![],
        }
    }

}
