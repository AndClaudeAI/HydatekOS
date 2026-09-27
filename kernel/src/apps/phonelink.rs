//! Phone Link: HydatekOS's "link to phone" hub — messages, notifications,
//! photos, calls, sharing and (for the demo phone) live screen mirroring.

use super::messages::MsgView;
use super::{side_item, App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::link::Source;
use crate::qr::{Ecc, Qr};
use crate::sys::{Req, Sys};
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const TABS: [&str; 6] = ["Messages", "Notifications", "Photos", "Calls", "Shared", "Phone screen"];

const C_DEMO: u32 = 1;
const C_UNPAIR: u32 = 2;
const C_HANGUP: u32 = 3;
const C_CLEAR: u32 = 4;
const C_CLIP_FIELD: u32 = 5;
const C_CLIP_SEND: u32 = 6;
const C_TAB: u32 = 10;
const C_NOTIF: u32 = 200;
const C_PHOTO: u32 = 300;
const C_CALL: u32 = 400;
const C_MSG: u32 = 1000;

pub struct PhoneLink {
    tab: usize,
    msgs: MsgView,
    qr: Option<(String, Qr)>,
    clip: LineEdit,
    clip_focus: bool,
}

impl PhoneLink {
    pub fn new() -> PhoneLink {
        PhoneLink { tab: 0, msgs: MsgView::new(), qr: None, clip: LineEdit::default(), clip_focus: false }
    }
}

/// Draw a QR code filling `r` (quiet zone included).
fn draw_qr(ui: &mut Ui, r: Rect, q: &Qr) {
    let n = q.size as i32 + 8;
    let cell = (r.w / n).max(1);
    let total = cell * n;
    let ox = r.x + (r.w - total) / 2 + 4 * cell;
    let oy = r.y + (r.h - total) / 2 + 4 * cell;
    ui.rrect(Rect::new(r.x + (r.w - total) / 2, r.y + (r.h - total) / 2, total, total), 12, Color::rgb(0xFFFFFF));
    let dark = Color::rgb(0x111018);
    for y in 0..q.size {
        let mut x = 0;
        while x < q.size {
            if q.get(x, y) {
                let start = x;
                while x < q.size && q.get(x, y) {
                    x += 1;
                }
                ui.rect(Rect::new(ox + start as i32 * cell, oy + y as i32 * cell, (x - start) as i32 * cell, cell), dark);
            } else {
                x += 1;
            }
        }
    }
}

/// Explains a feature the connected companion can't provide.
fn needs_app(ui: &mut Ui, r: Rect, what: &str, sys: &Sys) {
    let t = ui.t;
    let c = Rect::new(r.x + 24, r.y + 24, r.w - 48, 150);
    ui.rrect(c, 14, t.tile);
    ui.text(c.x + 18, c.y + 32, Face::Semibold, 15, what, t.text);
    let lines = [
        "Your phone is linked through its web browser, which can share",
        "photos, files and text but can't read texts, notifications or calls.",
        "Install the HydatekOS Link app for Android to add them:",
    ];
    for (i, l) in lines.iter().enumerate() {
        let l = ui.fit(Face::Regular, 13, l, c.w - 36);
        ui.text(c.x + 18, c.y + 58 + i as i32 * 20, Face::Regular, 13, &l, t.text2);
    }
    let url = match sys.net.ip {
        Some(ip) => format!("http://{}:7743/app.apk", crate::net::ip_str(ip)),
        None => String::from("(connect this PC to a network first)"),
    };
    ui.text(c.x + 18, c.y + 130, Face::Semibold, 13, &url, t.accent);
}

impl App for PhoneLink {
    fn kind(&self) -> AppKind {
        AppKind::PhoneLink
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let link = &sys.link;
        if !link.paired {
            ui.text_in(Rect::new(r.x + 20, r.y, 300, HEADER), Face::Semibold, 15, "Phone Link", t.text, 0);
            ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);
            let c = Rect::new(r.x + 36, r.y + HEADER + 22, r.w - 72, r.h - HEADER - 44);
            let qr_w = 220.min(c.h - 40);
            let tw = c.w - qr_w - 36;
            ui.text(c.x, c.y + 26, Face::Semibold, 24, "Link your phone", t.text);
            let sub = ui.fit(Face::Regular, 14, "Texts, notifications, calls, photos and files on your PC.", tw);
            ui.text(c.x, c.y + 52, Face::Regular, 14, &sub, t.text2);
            let net_line = match (sys.net.present, sys.net.ip) {
                (false, _) => String::from("No network adapter found. Plug in Ethernet and restart."),
                (true, None) if !sys.net.link_up => String::from("Network cable unplugged."),
                (true, None) => String::from("Getting a network address..."),
                (true, Some(ip)) => format!("This PC is {} on your network.", crate::net::ip_str(ip)),
            };
            let steps = [
                String::from("1. Put your phone on the same network as this PC."),
                String::from("2. Scan the code with your phone's camera."),
                String::from("3. For texts, notifications and calls, install"),
                String::from("    the Android app from the page that opens."),
            ];
            for (i, s) in steps.iter().enumerate() {
                let s = ui.fit(Face::Regular, 14, s, tw);
                ui.text(c.x, c.y + 96 + i as i32 * 26, Face::Regular, 14, &s, t.text);
            }
            let ok = sys.net.ip.is_some();
            ui.circle(c.x + 5, c.y + 212, 4, if ok { Color::rgb(0x4F9A55) } else { t.accent });
            let nl = ui.fit(Face::Medium, 13, &net_line, tw - 16);
            ui.text(c.x + 16, c.y + 217, Face::Medium, 13, &nl, t.text2);
            if let Some(ip) = sys.net.ip {
                let url = link.pair_url(&crate::net::ip_str(ip));
                if self.qr.as_ref().map(|q| q.0 != url).unwrap_or(true) {
                    self.qr = Qr::encode(url.as_bytes(), Ecc::Medium).map(|q| (url.clone(), q));
                }
                if let Some((_, q)) = &self.qr {
                    draw_qr(ui, Rect::new(c.r() - qr_w, c.y + 10, qr_w, qr_w), q);
                }
                // the same link as text, for phones without a QR scanner
                let per = ((qr_w + 20) / ui.tw(Face::Mono, 12, "m").max(1)).max(8) as usize;
                let chars: Vec<char> = url.chars().collect();
                let lines: Vec<String> = chars.chunks(per).map(|c| c.iter().collect()).collect();
                for (i, l) in lines.iter().take(4).enumerate() {
                    let lw = ui.tw(Face::Mono, 12, l);
                    ui.text(c.r() - qr_w / 2 - lw / 2, c.y + qr_w + 34 + i as i32 * 16, Face::Mono, 12, l, t.text3);
                }
            } else {
                let q = Rect::new(c.r() - qr_w, c.y + 10, qr_w, qr_w);
                ui.rrect(q, 14, t.tile);
                ui.icon_in(Icon::Globe, Rect::new(q.x, q.y, q.w, q.h - 30), 40, t.text3);
                ui.text_in(Rect::new(q.x, q.b() - 60, q.w, 20), Face::Medium, 13, "Waiting for network", t.text3, 1);
            }
            ui.button(Rect::new(c.x, c.y + 250, 190, 34), "Try the demo phone", Action::App(inst, C_DEMO), false);
            let note = ui.fit(Face::Regular, 12, "A simulated phone, for exploring without a device.", tw);
            ui.text(c.x, c.y + 306, Face::Regular, 12, &note, t.text3);
            return;
        }

        // Sidebar: device card and tabs
        let side_w = 200;
        super::panel(ui, r, Rect::new(r.x, r.y, side_w, r.h), t.sidebar);
        ui.text_in(Rect::new(r.x + 18, r.y, 170, HEADER), Face::Semibold, 15, "Phone Link", t.text, 0);
        let card = Rect::new(r.x + 12, r.y + HEADER, side_w - 24, 92);
        ui.rrect(card, 14, t.surface);
        ui.rrect(Rect::new(card.x + 12, card.y + 14, 30, 50), 7, t.dune3);
        ui.rrect(Rect::new(card.x + 15, card.y + 18, 24, 40), 4, t.sun);
        let dn = if link.device.is_empty() { String::from("Your phone") } else { link.device.clone() };
        let dn = ui.fit(Face::Semibold, 13, &dn, card.w - 64);
        ui.text(card.x + 54, card.y + 30, Face::Semibold, 13, &dn, t.text);
        let (status, col) = match link.source {
            Source::Demo => ("Demo phone", t.accent),
            _ if link.online => ("Connected", Color::rgb(0x4F9A55)),
            _ => ("Offline", t.text3),
        };
        ui.circle(card.x + 58, card.y + 45, 4, col);
        ui.text(card.x + 68, card.y + 50, Face::Regular, 12, status, t.text2);
        if link.battery > 0 {
            ui.icon(Icon::Battery, card.x + 54, card.y + 60, 16, t.text2);
            let b = format!("{}%{}", link.battery, if link.charging { " · charging" } else { "" });
            ui.text(card.x + 74, card.y + 73, Face::Regular, 12, &b, t.text2);
        }
        let mut y = card.b() + 12;
        for (i, name) in TABS.iter().enumerate() {
            let mut label = String::from(*name);
            if i == 0 && link.unread() > 0 {
                label = format!("{}  ({})", name, link.unread());
            }
            side_item(ui, Rect::new(r.x + 8, y, side_w - 16, 30), &label, i == self.tab, Action::App(inst, C_TAB + i as u32));
            y += 32;
        }
        let leave = if link.is_demo() { "Leave demo" } else { "Unpair phone" };
        ui.button(Rect::new(r.x + 12, r.b() - 44, side_w - 24, 30), leave, Action::App(inst, C_UNPAIR), false);

        let m = Rect::new(r.x + side_w, r.y, r.w - side_w, r.h);
        ui.text_in(Rect::new(m.x + 20, r.y, 300, HEADER), Face::Semibold, 15, TABS[self.tab], t.text, 0);
        ui.rect(Rect::new(m.x, r.y + HEADER, m.w, 1), t.line);
        let body = Rect::new(m.x, m.y + HEADER + 1, m.w, m.h - HEADER - 1);
        if !link.online && !link.is_demo() && self.tab != 4 {
            let b = Rect::new(body.x + 16, body.y + 10, body.w - 32, 34);
            ui.rrect(b, 10, t.tile);
            let msg = ui.fit(Face::Regular, 12, "Phone offline. Open the HydatekOS Link app or companion page on your phone to reconnect.", b.w - 24);
            ui.text_in(Rect::new(b.x + 12, b.y, b.w - 24, b.h), Face::Regular, 12, &msg, t.text2, 0);
        }
        let body = if !link.online && !link.is_demo() && self.tab != 4 { Rect::new(body.x, body.y + 50, body.w, body.h - 50) } else { body };
        match self.tab {
            0 if !link.has("sms") => needs_app(ui, body, "Texts need the Android app", sys),
            0 => self.msgs.render(ui, body, sys, inst, C_MSG),
            1 if !link.has("notif") => needs_app(ui, body, "Notifications need the Android app", sys),
            1 => {
                let mut y = body.y + 14;
                if link.notifs.is_empty() {
                    ui.text_in(Rect::new(body.x, body.y + 40, body.w, 20), Face::Regular, 14, "You're all caught up", t.text3, 1);
                }
                for (i, n) in link.notifs.iter().enumerate() {
                    if y + 56 > body.b() - 50 {
                        break;
                    }
                    let c = Rect::new(body.x + 16, y, body.w - 32, 56);
                    ui.rrect(c, 12, t.tile);
                    let app = ui.fit(Face::Semibold, 12, &n.app, c.w - 120);
                    ui.text(c.x + 14, c.y + 22, Face::Semibold, 12, &app, t.accent);
                    let tw = ui.tw(Face::Regular, 11, &n.time);
                    ui.text(c.r() - tw - 44, c.y + 22, Face::Regular, 11, &n.time, t.text3);
                    let line = ui.fit(Face::Regular, 13, &format!("{} — {}", n.title, n.body), c.w - 60);
                    ui.text(c.x + 14, c.y + 42, Face::Regular, 13, &line, t.text);
                    ui.icon_button(Rect::new(c.r() - 34, c.y + 8, 24, 24), Icon::Close, Action::App(inst, C_NOTIF + i as u32), 12);
                    y += 64;
                }
                if !link.notifs.is_empty() {
                    ui.button(Rect::new(body.r() - 116, body.b() - 44, 100, 30), "Clear all", Action::App(inst, C_CLEAR), false);
                }
            }
            2 => {
                if link.photos.is_empty() {
                    let msg = if link.has("photos") { "No photos shared yet" } else { "Share photos from the companion on your phone" };
                    ui.text_in(Rect::new(body.x, body.y + 40, body.w, 20), Face::Regular, 14, msg, t.text3, 1);
                }
                let cols = ((body.w - 32) / 150).max(1);
                let cw = (body.w - 32) / cols;
                for (i, p) in link.photos.iter().enumerate() {
                    let (cx, cy) = (i as i32 % cols, i as i32 / cols);
                    let cell = Rect::new(body.x + 16 + cx * cw, body.y + 16 + cy * 150, cw - 12, 118);
                    if cell.b() > body.b() {
                        break;
                    }
                    let a = Action::App(inst, C_PHOTO + i as u32);
                    match &p.thumb {
                        Some(th) => ui.image_rgb(cell, 64, 64, th, 12),
                        None => {
                            ui.rrect(cell, 12, Color::rgb(p.c1));
                            ui.rrect(Rect::new(cell.x, cell.y + cell.h / 2, cell.w, cell.h / 2), 12, Color::rgb(p.c2));
                            ui.rect(Rect::new(cell.x, cell.y + cell.h / 2, cell.w, 12), Color::rgb(p.c2));
                            ui.circle(cell.r() - 30, cell.y + 26, 12, Color::rgb(0xFFFFFF).with_alpha(170));
                        }
                    }
                    if ui.hot(a) {
                        ui.rrect(cell, 12, Color::rgba(0, 70));
                        ui.text_in(cell, Face::Semibold, 13, "Copy to Pictures", Color::rgb(0xFFFFFF), 1);
                    }
                    let name = ui.fit(Face::Regular, 12, &p.name, cell.w);
                    ui.text(cell.x + 2, cell.b() + 18, Face::Regular, 12, &name, t.text2);
                    ui.zone(cell, a);
                }
            }
            3 if !link.has("calls") => needs_app(ui, body, "Calls need the Android app", sys),
            3 => {
                let mut y = body.y + 12;
                if link.calls.is_empty() {
                    ui.text_in(Rect::new(body.x, body.y + 40, body.w, 20), Face::Regular, 14, "No recent calls", t.text3, 1);
                }
                for (i, c) in link.calls.iter().enumerate() {
                    if y + 50 > body.b() {
                        break;
                    }
                    let row = Rect::new(body.x + 16, y, body.w - 32, 50);
                    ui.rrect(row, 12, t.tile);
                    ui.icon(Icon::User, row.x + 14, row.y + 13, 22, t.text2);
                    let who = if c.name.is_empty() { &c.number } else { &c.name };
                    let who = ui.fit(Face::Semibold, 13, who, row.w - 150);
                    ui.text(row.x + 48, row.y + 22, Face::Semibold, 13, &who, if c.missed { t.danger } else { t.text });
                    let sub = format!("{}{}", c.when, if c.missed { " · missed" } else { "" });
                    ui.text(row.x + 48, row.y + 39, Face::Regular, 12, &sub, t.text2);
                    ui.button(Rect::new(row.r() - 84, row.y + 10, 72, 30), "Call", Action::App(inst, C_CALL + i as u32), false);
                    y += 58;
                }
                if let Some(who) = &link.calling {
                    let o = Rect::new(body.x + body.w / 2 - 160, body.b() - 120, 320, 100);
                    ui.shadow(o, 16, 10, 4, 60);
                    ui.rrect(o, 16, t.dune3);
                    let title = ui.fit(Face::Semibold, 15, &format!("Calling {}...", who), o.w - 40);
                    ui.text(o.x + 20, o.y + 34, Face::Semibold, 15, &title, Color::rgb(0xFFFFFF));
                    ui.text(o.x + 20, o.y + 54, Face::Regular, 12, "Talk on your phone", Color::rgb(0xC9C3D6));
                    ui.button(Rect::new(o.x + 20, o.y + 64, 100, 28), "Hang up", Action::App(inst, C_HANGUP), true);
                }
            }
            4 => {
                // share text; list what went back and forth
                let f = Rect::new(body.x + 16, body.y + 14, body.w - 120, 34);
                ui.field(f, &self.clip.text, "Type text to send to your phone", self.clip_focus, Action::App(inst, C_CLIP_FIELD));
                ui.button(Rect::new(f.r() + 8, f.y, 80, 34), "Send", Action::App(inst, C_CLIP_SEND), true);
                let hint = ui.fit(Face::Regular, 12, "To send a file, select it in Files and choose File › Send to Phone.", body.w - 32);
                ui.text(body.x + 16, f.b() + 22, Face::Regular, 12, &hint, t.text3);
                let mut y = f.b() + 36;
                for s in &link.shared {
                    if y + 44 > body.b() {
                        break;
                    }
                    let row = Rect::new(body.x + 16, y, body.w - 32, 40);
                    ui.rrect(row, 10, t.tile);
                    let ic = if s.is_file { Icon::Doc } else { Icon::Chat };
                    ui.icon(ic, row.x + 12, row.y + 12, 16, t.accent);
                    let dir = if s.from_phone { "From phone" } else { "To phone" };
                    let dw = ui.tw(Face::Regular, 11, dir);
                    ui.text(row.r() - dw - 12, row.y + 25, Face::Regular, 11, dir, t.text3);
                    let text = ui.fit(Face::Regular, 13, &s.text.replace('\n', " "), row.w - dw - 60);
                    ui.text(row.x + 38, row.y + 25, Face::Regular, 13, &text, t.text);
                    y += 46;
                }
            }
            _ => {
                if link.is_demo() {
                    // Live mirror: the shell draws the demo phone into this rect.
                    let ph = (body.h - 40).min(560);
                    let pw = ph * 390 / 844;
                    let frame = Rect::new(body.x + (body.w - pw) / 2 - 8, body.y + 12, pw + 16, ph + 16);
                    ui.rrect(frame, 34, Color::rgb(0x121119));
                    ui.phone_embed = Some(Rect::new(frame.x + 8, frame.y + 8, pw, ph));
                } else {
                    let c = Rect::new(body.x + 24, body.y + 24, body.w - 48, 110);
                    ui.rrect(c, 14, t.tile);
                    ui.text(c.x + 18, c.y + 32, Face::Semibold, 15, "Screen mirroring isn't available yet", t.text);
                    let l1 = ui.fit(Face::Regular, 13, "Mirroring a real phone needs Android screen capture, planned for", c.w - 36);
                    let l2 = ui.fit(Face::Regular, 13, "the next version of the app. Try it now with the demo phone.", c.w - 36);
                    ui.text(c.x + 18, c.y + 58, Face::Regular, 13, &l1, t.text2);
                    ui.text(c.x + 18, c.y + 78, Face::Regular, 13, &l2, t.text2);
                }
            }
        }
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        self.clip_focus = code == C_CLIP_FIELD;
        match code {
            C_DEMO => {
                sys.link.demo();
                sys.reqs.push(Req::SaveSettings);
                sys.toast("Phone Link", "Demo phone connected");
            }
            C_UNPAIR => {
                sys.unpair();
                self.tab = 0;
                self.qr = None;
            }
            C_HANGUP => sys.link.hangup(),
            C_CLEAR => sys.link.dismiss_all(),
            C_CLIP_SEND => {
                let text = self.clip.text.trim().to_string();
                if !text.is_empty() {
                    sys.link.send_clip(&text);
                    self.clip.text.clear();
                    if !sys.link.is_demo() && !sys.link.online {
                        sys.toast("Phone Link", "Your phone is offline; it will not receive this");
                    }
                }
            }
            c if c >= C_MSG => self.msgs.action(c - C_MSG, sys),
            c if c >= C_CALL => sys.link.dial((c - C_CALL) as usize),
            c if c >= C_PHOTO => {
                let i = (c - C_PHOTO) as usize;
                if sys.link.request_photo(i) {
                    sys.toast("Phone Link", "Copying the photo from your phone...");
                } else if let Some(p) = sys.link.photos.get(i) {
                    let n = p.name.clone();
                    let dst = sys.fs.unique("/home/Pictures", &n, ".img");
                    sys.fs.write(&dst, b"HYDATEK-IMAGE");
                    sys.toast("Phone Link", &format!("Saved \"{}\" to Pictures", n));
                }
            }
            c if c >= C_NOTIF => {
                let i = (c - C_NOTIF) as usize;
                if i < sys.link.notifs.len() {
                    let n = sys.link.notifs.remove(i);
                    if !sys.link.is_demo() {
                        sys.link.outbox.push(crate::hlp::Msg::new("notif_dismiss").with("id", &n.id));
                    }
                }
            }
            c if c >= C_TAB => self.tab = ((c - C_TAB) as usize).min(TABS.len() - 1),
            _ => {}
        }
    }

    fn key(&mut self, k: Key, _ctrl: bool, sys: &mut Sys) {
        if self.tab == 4 && self.clip_focus {
            match k {
                Key::Enter => self.action(C_CLIP_SEND, false, sys),
                Key::Esc => self.clip_focus = false,
                _ => {
                    self.clip.key(k);
                }
            }
            return;
        }
        if self.tab == 0 && sys.link.paired {
            self.msgs.key(k, sys);
        }
    }

    fn animating(&self) -> bool {
        self.msgs.focus || self.clip_focus || self.tab == 5
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            3 => TABS.iter().enumerate().map(|(i, t)| (*t, C_TAB + i as u32)).collect(),
            _ => vec![],
        }
    }
}
