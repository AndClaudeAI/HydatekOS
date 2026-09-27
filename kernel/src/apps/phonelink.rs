//! Phone Link: HydatekOS's "link to phone" hub (messages, notifications,
//! photos, calls and live screen mirroring).

use super::messages::MsgView;
use super::{side_item, App, AppKind, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::sys::{Req, Sys};
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const TABS: [(&str, Icon); 5] = [("Messages", Icon::Chat), ("Notifications", Icon::Bell), ("Photos", Icon::Image), ("Calls", Icon::Phone), ("Phone screen", Icon::Monitor)];

const C_PAIR: u32 = 1;
const C_UNPAIR: u32 = 2;
const C_HANGUP: u32 = 3;
const C_CLEAR: u32 = 4;
const C_TAB: u32 = 10;
const C_PHOTO: u32 = 300;
const C_CALL: u32 = 400;
const C_MSG: u32 = 1000;

pub struct PhoneLink {
    tab: usize,
    msgs: MsgView,
    pairing: u32, // animation ticks while "pairing"
}

impl PhoneLink {
    pub fn new() -> PhoneLink {
        PhoneLink { tab: 0, msgs: MsgView::new(), pairing: 0 }
    }
}

/// A deterministic dot pattern standing in for a QR code (encodes the code).
fn pair_pattern(ui: &mut Ui, r: Rect, code: u32, fg: Color) {
    let n = 17;
    let cell = r.w / n;
    let mut s = code ^ 0x9e37_79b9;
    for y in 0..n {
        for x in 0..n {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let local = [(0, 0), (n - 5, 0), (0, n - 5)].iter().find_map(|&(ox, oy)| {
                if x >= ox && x < ox + 5 && y >= oy && y < oy + 5 { Some((x - ox, y - oy)) } else { None }
            });
            let on = match local {
                Some((fx, fy)) => fx == 0 || fy == 0 || fx == 4 || fy == 4 || (fx == 2 && fy == 2),
                None => s & 3 == 0,
            };
            if on {
                ui.rrect(Rect::new(r.x + x * cell, r.y + y * cell, cell - 1, cell - 1), 1, fg);
            }
        }
    }
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
            let c = Rect::new(r.x + 40, r.y + HEADER + 30, r.w - 80, r.h - HEADER - 60);
            ui.text(c.x, c.y + 24, Face::Semibold, 24, "Link your phone to HydatekOS", t.text);
            ui.text(c.x, c.y + 50, Face::Regular, 14, "See messages, notifications, photos and calls on your desktop.", t.text2);
            let steps = [
                "1. On your phone, open Settings › Phone Link.",
                "2. Choose \"Link to a desktop\" and scan this code.",
                "3. Confirm the pairing code matches on both screens.",
            ];
            for (i, s) in steps.iter().enumerate() {
                ui.text(c.x, c.y + 96 + i as i32 * 26, Face::Regular, 14, s, t.text);
            }
            let code = format!("{:03} {:03}", link.code / 1000, link.code % 1000);
            ui.text(c.x, c.y + 200, Face::Regular, 12, "PAIRING CODE", t.text2);
            ui.text(c.x, c.y + 230, Face::Semibold, 28, &code, t.accent);
            let label = if self.pairing > 0 { "Connecting…" } else { "Connect virtual phone" };
            ui.button(Rect::new(c.x, c.y + 256, 200, 36), label, Action::App(inst, C_PAIR), true);
            ui.text(c.x, c.y + 314, Face::Regular, 12, "Real phones connect over Wi-Fi/Bluetooth once those drivers land (milestone 3).", t.text3);
            ui.text(c.x, c.y + 330, Face::Regular, 12, "Until then, a virtual HydatekOS Mobile device runs inside this computer.", t.text3);
            let q = Rect::new(c.r() - 190, c.y + 90, 170, 170);
            ui.rrect(q.inset(-14), 18, Color::rgb(0xFFFFFF));
            pair_pattern(ui, q, link.code, Color::rgb(0x1E1B2C));
            return;
        }

        // Sidebar with device card and tabs
        let side_w = 200;
        let side = Rect::new(r.x, r.y, side_w, r.h);
        super::panel(ui, r, side, t.sidebar);
        ui.text_in(Rect::new(r.x + 18, r.y, 170, HEADER), Face::Semibold, 15, "Phone Link", t.text, 0);
        let card = Rect::new(r.x + 12, r.y + HEADER, side_w - 24, 92);
        ui.rrect(card, 14, t.surface);
        ui.rrect(Rect::new(card.x + 12, card.y + 14, 30, 50), 7, t.dune3);
        ui.rrect(Rect::new(card.x + 15, card.y + 18, 24, 40), 4, t.sun);
        let dn = ui.fit(Face::Semibold, 13, &link.device, card.w - 64);
        ui.text(card.x + 54, card.y + 30, Face::Semibold, 13, &dn, t.text);
        ui.circle(card.x + 58, card.y + 45, 4, Color::rgb(0x4F9A55));
        ui.text(card.x + 68, card.y + 50, Face::Regular, 12, "Connected", t.text2);
        ui.icon(Icon::Battery, card.x + 54, card.y + 60, 16, t.text2);
        ui.text(card.x + 74, card.y + 73, Face::Regular, 12, &format!("{}%", link.battery), t.text2);
        let mut y = card.b() + 12;
        for (i, (name, _)) in TABS.iter().enumerate() {
            let mut label = String::from(*name);
            if i == 0 && link.unread() > 0 {
                label = format!("{}  ({})", name, link.unread());
            }
            side_item(ui, Rect::new(r.x + 8, y, side_w - 16, 30), &label, i == self.tab, Action::App(inst, C_TAB + i as u32));
            y += 32;
        }
        ui.button(Rect::new(r.x + 12, r.b() - 44, side_w - 24, 30), "Disconnect", Action::App(inst, C_UNPAIR), false);

        let m = Rect::new(r.x + side_w, r.y, r.w - side_w, r.h);
        ui.text_in(Rect::new(m.x + 20, r.y, 300, HEADER), Face::Semibold, 15, TABS[self.tab].0, t.text, 0);
        ui.rect(Rect::new(m.x, r.y + HEADER, m.w, 1), t.line);
        let body = Rect::new(m.x, m.y + HEADER + 1, m.w, m.h - HEADER - 1);
        match self.tab {
            0 => self.msgs.render(ui, body, sys, inst, C_MSG),
            1 => {
                let mut y = body.y + 14;
                if link.notifs.is_empty() {
                    ui.text_in(Rect::new(body.x, body.y + 40, body.w, 20), Face::Regular, 14, "You're all caught up", t.text3, 1);
                }
                for n in link.notifs.iter().take(6) {
                    let c = Rect::new(body.x + 16, y, body.w - 32, 56);
                    ui.rrect(c, 12, t.tile);
                    ui.text(c.x + 14, c.y + 22, Face::Semibold, 12, &n.app, t.accent);
                    let tw = ui.tw(Face::Regular, 11, &n.time);
                    ui.text(c.r() - tw - 14, c.y + 22, Face::Regular, 11, &n.time, t.text3);
                    let line = ui.fit(Face::Regular, 13, &format!("{} — {}", n.title, n.body), c.w - 28);
                    ui.text(c.x + 14, c.y + 42, Face::Regular, 13, &line, t.text);
                    y += 64;
                }
                if !link.notifs.is_empty() {
                    ui.button(Rect::new(body.r() - 116, body.b() - 44, 100, 30), "Clear all", Action::App(inst, C_CLEAR), false);
                }
            }
            2 => {
                let cols = ((body.w - 32) / 150).max(1);
                let cw = (body.w - 32) / cols;
                for (i, (name, c1, c2)) in link.photos.iter().enumerate() {
                    let (cx, cy) = (i as i32 % cols, i as i32 / cols);
                    let cell = Rect::new(body.x + 16 + cx * cw, body.y + 16 + cy * 150, cw - 12, 118);
                    let a = Action::App(inst, C_PHOTO + i as u32);
                    // two-tone "photo": sky + dune
                    ui.rrect(cell, 12, Color::rgb(*c1));
                    ui.rrect(Rect::new(cell.x, cell.y + cell.h / 2, cell.w, cell.h / 2), 12, Color::rgb(*c2));
                    ui.rect(Rect::new(cell.x, cell.y + cell.h / 2, cell.w, 12), Color::rgb(*c2));
                    ui.circle(cell.r() - 30, cell.y + 26, 12, Color::rgb(0xFFFFFF).with_alpha(170));
                    if ui.hot(a) {
                        ui.rrect(cell, 12, Color::rgba(0, 60));
                        ui.text_in(cell, Face::Semibold, 13, "Save to Pictures", Color::rgb(0xFFFFFF), 1);
                    }
                    ui.text(cell.x + 2, cell.b() + 18, Face::Regular, 12, name, t.text2);
                    ui.zone(cell, a);
                }
            }
            3 => {
                let mut y = body.y + 12;
                for (i, c) in link.calls.iter().enumerate() {
                    let row = Rect::new(body.x + 16, y, body.w - 32, 50);
                    ui.rrect(row, 12, t.tile);
                    ui.icon(Icon::User, row.x + 14, row.y + 13, 22, t.text2);
                    ui.text(row.x + 48, row.y + 22, Face::Semibold, 13, &c.name, if c.missed { t.danger } else { t.text });
                    let sub = format!("{}{}", c.when, if c.missed { " · missed" } else { "" });
                    ui.text(row.x + 48, row.y + 39, Face::Regular, 12, &sub, t.text2);
                    ui.button(Rect::new(row.r() - 84, row.y + 10, 72, 30), "Call", Action::App(inst, C_CALL + i as u32), false);
                    y += 58;
                }
                if let Some(who) = &link.calling {
                    let o = Rect::new(body.x + body.w / 2 - 150, body.b() - 120, 300, 100);
                    ui.shadow(o, 16, 10, 4, 60);
                    ui.rrect(o, 16, t.dune3);
                    ui.text(o.x + 20, o.y + 34, Face::Semibold, 15, &format!("Calling {}…", who), Color::rgb(0xFFFFFF));
                    ui.text(o.x + 20, o.y + 54, Face::Regular, 12, "Audio routes through your phone", Color::rgb(0xC9C3D6));
                    ui.button(Rect::new(o.x + 20, o.y + 64, 100, 28), "Hang up", Action::App(inst, C_HANGUP), true);
                }
            }
            _ => {
                // Live mirror: the shell draws the virtual phone into this rect.
                let ph = (body.h - 40).min(560);
                let pw = ph * 390 / 844;
                let frame = Rect::new(body.x + (body.w - pw) / 2 - 8, body.y + 12, pw + 16, ph + 16);
                ui.rrect(frame, 34, Color::rgb(0x121119));
                ui.phone_embed = Some(Rect::new(frame.x + 8, frame.y + 8, pw, ph));
            }
        }
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        match code {
            C_PAIR => {
                if self.pairing == 0 {
                    self.pairing = 1;
                }
            }
            C_UNPAIR => {
                sys.link.paired = false;
                sys.reqs.push(Req::SaveSettings);
            }
            C_HANGUP => sys.link.calling = None,
            C_CLEAR => sys.link.notifs.clear(),
            c if c >= C_MSG => self.msgs.action(c - C_MSG, sys),
            c if c >= C_CALL => {
                let i = (c - C_CALL) as usize;
                if let Some(call) = sys.link.calls.get(i) {
                    sys.link.calling = Some(call.name.clone());
                }
            }
            c if c >= C_PHOTO => {
                let i = (c - C_PHOTO) as usize;
                if let Some((name, _, _)) = sys.link.photos.get(i) {
                    let n = name.clone();
                    let dst = sys.fs.unique("/home/Pictures", &n, ".img");
                    sys.fs.write(&dst, b"HYDATEK-IMAGE");
                    sys.toast("Phone Link", &format!("Saved \"{}\" to Pictures", n));
                }
            }
            c if c >= C_TAB => self.tab = ((c - C_TAB) as usize).min(TABS.len() - 1),
            _ => {}
        }
    }

    fn key(&mut self, k: Key, _ctrl: bool, sys: &mut Sys) {
        if self.tab == 0 && sys.link.paired {
            self.msgs.key(k, sys);
        }
    }

    fn animating(&self) -> bool {
        self.pairing > 0 || self.msgs.focus || self.tab == 4
    }

    fn tick(&mut self, sys: &mut Sys) {
        self.pairing_tick(sys);
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            3 => TABS.iter().enumerate().map(|(i, t)| (t.0, C_TAB + i as u32)).collect(),
            _ => vec![],
        }
    }

}

impl PhoneLink {
    fn pairing_tick(&mut self, sys: &mut Sys) {
        if self.pairing > 0 {
            self.pairing += 1;
        }
        if self.pairing > 120 {
            self.pairing = 0;
            sys.link.paired = true;
            sys.reqs.push(Req::SaveSettings);
            sys.toast("Phone Link", "HydatekOS Mobile is connected");
        }
    }
}
