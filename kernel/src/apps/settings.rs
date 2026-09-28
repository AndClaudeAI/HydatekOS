//! Settings: appearance, connectivity, Phone Link, display and system info.

use super::{side_item, App, AppKind, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::sys::{Req, Sys};
use crate::theme::ACCENTS;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const SECTIONS: [&str; 8] = ["Appearance", "Network", "Bluetooth", "Phone Link", "Lock screen", "Browser", "Display", "About"];

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
    /// Phone layout: a section page is open (otherwise the section list).
    page: bool,
    /// what's being typed into the PIN / password field
    pin: String,
    password: String,
    /// focused field: C_PIN_FIELD, C_PW_FIELD or 0
    focus: u32,
    pin_msg: String,
}

impl Settings {
    pub fn new() -> Settings {
        Settings { sec: 0, page: false, pin: String::new(), password: String::new(), focus: 0, pin_msg: String::new() }
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
            0 => {
                card(ui, Rect::new(m.x, m.y, m.w, 190));
                let inner = Rect::new(m.x + 16, m.y, m.w - 32, 190);
                row(ui, inner, m.y + 12, "Dark mode", "Dusk palette for evenings");
                ui.switch(sw_x, m.y + 18, sys.dark, Action::App(inst, C_DARK));
                row(ui, inner, m.y + 62, "Accent colour", "Used for highlights, folders and buttons");
                let step = ((m.w - 32) / 4).min(78);
                for (i, (name, l, d)) in ACCENTS.iter().enumerate() {
                    let cx = m.x + 16 + i as i32 * step;
                    let c = Color::rgb(if sys.dark { *d } else { *l });
                    let a = Action::App(inst, C_ACCENT + i as u32);
                    if sys.accent == i {
                        ui.circle(cx + 12, m.y + 128, 14, t.text);
                        ui.circle(cx + 12, m.y + 128, 12, t.surface);
                    }
                    ui.circle(cx + 12, m.y + 128, 10, c);
                    if step >= 70 {
                        ui.text(cx + 30, m.y + 133, Face::Regular, 13, name, t.text);
                    }
                    ui.zone(Rect::new(cx, m.y + 112, 72, 32), a);
                }
                row(ui, inner, m.y + 150, "Pointer speed", "");
                let ps = format!("{}", sys.pointer_speed);
                ui.button(Rect::new(sw_x - 44, m.y + 154, 30, 26), "-", Action::App(inst, C_PTR_DOWN), false);
                ui.text_in(Rect::new(sw_x - 12, m.y + 154, 20, 26), Face::Semibold, 13, &ps, t.text, 1);
                ui.button(Rect::new(sw_x + 12, m.y + 154, 30, 26), "+", Action::App(inst, C_PTR_UP), false);

                card(ui, Rect::new(m.x, m.y + 204, m.w, 110));
                let inner = Rect::new(m.x + 16, m.y + 204, m.w - 32, 110);
                row(ui, inner, m.y + 216, "Mobile shell", "Use the HydatekOS Mobile home screen on this device");
                ui.switch(sw_x, m.y + 222, sys.mobile_shell, Action::App(inst, C_MOBILE));
                row(ui, inner, m.y + 264, "Focus", "Silence Phone Link notifications");
                ui.switch(sw_x, m.y + 270, sys.focus, Action::App(inst, C_FOCUS));
            }
            1 => {
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
                row(ui, Rect::new(m.x + 16, m.y + 224, m.w - 32, 64), m.y + 236, "Wi-Fi", "Wi-Fi adapter drivers are not included yet; use Ethernet");
                ui.switch(sw_x, m.y + 244, sys.wifi, Action::App(inst, C_WIFI));
            }
            2 => {
                card(ui, Rect::new(m.x, m.y, m.w, 64));
                row(ui, Rect::new(m.x + 16, m.y, m.w, 64), m.y + 12, "Bluetooth", if sys.bt { "On" } else { "Off" });
                ui.switch(sw_x, m.y + 20, sys.bt, Action::App(inst, C_BT));
                card(ui, Rect::new(m.x, m.y + 78, m.w, 120));
                ui.text(m.x + 16, m.y + 104, Face::Semibold, 13, "No adapter driver yet", t.text);
                let lines = ["Bluetooth needs a USB (xHCI) host driver and an HCI stack,", "planned for milestone 4. Phone Link already works over", "your home network."];
                for (i, l) in lines.iter().enumerate() {
                    let l = ui.fit(Face::Regular, 13, l, m.w - 32);
                    ui.text(m.x + 16, m.y + 128 + i as i32 * 20, Face::Regular, 13, &l, t.text2);
                }
            }
            3 => {
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
            4 => {
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
                let note = if self.pin_msg.is_empty() {
                    if sys.secured() { "Keeps people out of your session; it doesn't encrypt files." } else { "No PIN or password: any key or click unlocks." }
                } else {
                    self.pin_msg.as_str()
                };
                let note = ui.fit(Face::Regular, 12, note, m.w - 16);
                ui.text(m.x + 8, top + 186, Face::Regular, 12, &note, t.text2);
                ui.button(Rect::new(m.x, top + 202, 130, 32), "Lock now", Action::App(inst, C_LOCK_NOW), true);
                ui.text(m.x + 142, top + 222, Face::Regular, 12, "or press F12", t.text3);
            }
            5 => {
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
            6 => {
                card(ui, Rect::new(m.x, m.y, m.w, 130));
                let (w, h, s) = sys.screen;
                kv(ui, m.x + 16, m.y + 30, m.w - 32, "Resolution", &format!("{} × {}", w, h));
                kv(ui, m.x + 16, m.y + 56, m.w - 32, "Scale", &format!("{}×", s));
                kv(ui, m.x + 16, m.y + 82, m.w - 32, "Layout", &format!("{} × {} points", w / s, h / s));
                kv(ui, m.x + 16, m.y + 108, m.w - 32, "Renderer", "HydatekOS software compositor");
            }
            _ => {
                card(ui, Rect::new(m.x, m.y, m.w, 214));
                ui.text(m.x + 16, m.y + 36, Face::Semibold, 24, "HydatekOS", t.text);
                ui.text(m.x + 16, m.y + 58, Face::Regular, 13, "Version 0.1 \"Dune\" · milestone 1", t.text2);
                let (used, total) = crate::heap::HEAP.stats();
                kv(ui, m.x + 16, m.y + 92, m.w - 32, "System memory", &format!("{} MB", sys.mem_total >> 20));
                kv(ui, m.x + 16, m.y + 116, m.w - 32, "Kernel heap", &format!("{} / {} MB", used >> 20, total >> 20));
                kv(ui, m.x + 16, m.y + 140, m.w - 32, "Storage", if sys.fs.persistent { "Boot disk \\HYDATEK" } else { "Live session (read-only disk)" });
                kv(ui, m.x + 16, m.y + 164, m.w - 32, "Firmware", &sys.firmware);
                kv(ui, m.x + 16, m.y + 188, m.w - 32, "Architecture", "x86-64 UEFI");
                ui.button(Rect::new(m.x, m.y + 230, 110, 32), "Restart", Action::App(inst, C_RESTART), false);
                ui.button(Rect::new(m.x + 120, m.y + 230, 110, 32), "Shut down", Action::App(inst, C_SHUTDOWN), true);
            }
        }
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        self.focus = if code == C_PIN_FIELD || code == C_PW_FIELD { code } else { 0 };
        match code {
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
            C_DARK => sys.dark = !sys.dark,
            C_MOBILE => sys.mobile_shell = !sys.mobile_shell,
            C_WIFI => sys.wifi = !sys.wifi,
            C_BT => sys.bt = !sys.bt,
            C_FOCUS => sys.focus = !sys.focus,
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
            c if c >= C_ACCENT => sys.accent = (c - C_ACCENT) as usize,
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

    fn key(&mut self, k: Key, _ctrl: bool, sys: &mut Sys) {
        let (text, save, max) = match self.focus {
            C_PIN_FIELD => (&mut self.pin, C_PIN_SAVE, 8),
            C_PW_FIELD => (&mut self.password, C_PW_SAVE, 64),
            _ => return,
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
        self.focus != 0
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            3 => SECTIONS.iter().enumerate().map(|(i, s)| (*s, C_SECTION + i as u32)).collect(),
            _ => vec![],
        }
    }

}
