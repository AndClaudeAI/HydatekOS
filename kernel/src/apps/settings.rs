//! Settings: appearance, connectivity, Phone Link, display and system info.

use super::{side_item, App, AppKind, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::sys::{Req, Sys};
use crate::theme::ACCENTS;
use crate::ui::{Action, Ui};
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

const SECTIONS: [&str; 6] = ["Appearance", "Wi-Fi", "Bluetooth", "Phone Link", "Display", "About"];

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

pub struct Settings {
    sec: usize,
    /// Phone layout: a section page is open (otherwise the section list).
    page: bool,
}

impl Settings {
    pub fn new() -> Settings {
        Settings { sec: 0, page: false }
    }
}

const C_BACK: u32 = 12;

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
            1 | 2 => {
                let (on, code, what) = if self.sec == 1 { (sys.wifi, C_WIFI, "Wi-Fi") } else { (sys.bt, C_BT, "Bluetooth") };
                card(ui, Rect::new(m.x, m.y, m.w, 64));
                row(ui, Rect::new(m.x + 16, m.y, m.w, 64), m.y + 12, what, if on { "On" } else { "Off" });
                ui.switch(sw_x, m.y + 20, on, Action::App(inst, code));
                card(ui, Rect::new(m.x, m.y + 78, m.w, 120));
                ui.text(m.x + 16, m.y + 104, Face::Semibold, 13, "No adapter driver yet", t.text);
                let lines = if self.sec == 1 {
                    ["HydatekOS milestone 1 has no network stack. Drivers for", "virtio-net, Intel e1000 and common Wi-Fi chipsets, plus", "TCP/IP, are scheduled for milestone 3 (see docs/ROADMAP.md)."]
                } else {
                    ["Bluetooth needs a USB (xHCI) host driver and an HCI stack,", "planned for milestone 4. Phone Link will use it for", "proximity pairing and calls."]
                };
                for (i, l) in lines.iter().enumerate() {
                    let l = ui.fit(Face::Regular, 13, l, m.w - 32);
                    ui.text(m.x + 16, m.y + 128 + i as i32 * 20, Face::Regular, 13, &l, t.text2);
                }
            }
            3 => {
                card(ui, Rect::new(m.x, m.y, m.w, 130));
                let st = if sys.link.paired { "Connected" } else { "Not paired" };
                row(ui, Rect::new(m.x + 16, m.y, m.w, 64), m.y + 12, &sys.link.device, st);
                if sys.link.paired {
                    kv(ui, m.x + 16, m.y + 78, m.w - 32, "Battery", &format!("{}%", sys.link.battery));
                    kv(ui, m.x + 16, m.y + 102, m.w - 32, "Unread conversations", &format!("{}", sys.link.unread()));
                } else {
                    ui.text(m.x + 16, m.y + 84, Face::Regular, 13, "Open Phone Link to pair a phone.", t.text2);
                }
                ui.button(Rect::new(m.x, m.y + 146, 150, 32), "Open Phone Link", Action::App(inst, C_OPEN_LINK), true);
                if sys.link.paired {
                    ui.button(Rect::new(m.x + 160, m.y + 146, 100, 32), "Unpair", Action::App(inst, C_UNPAIR), false);
                }
            }
            4 => {
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
        match code {
            C_DARK => sys.dark = !sys.dark,
            C_MOBILE => sys.mobile_shell = !sys.mobile_shell,
            C_WIFI => sys.wifi = !sys.wifi,
            C_BT => sys.bt = !sys.bt,
            C_FOCUS => sys.focus = !sys.focus,
            C_PTR_DOWN => sys.pointer_speed = (sys.pointer_speed - 1).max(1),
            C_PTR_UP => sys.pointer_speed = (sys.pointer_speed + 1).min(9),
            C_OPEN_LINK => sys.reqs.push(Req::Open(AppKind::PhoneLink)),
            C_UNPAIR => sys.link.paired = false,
            C_BACK => {
                self.page = false;
                return;
            }
            C_RESTART => sys.reqs.push(Req::Reboot),
            C_SHUTDOWN => sys.reqs.push(Req::Shutdown),
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

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            3 => SECTIONS.iter().enumerate().map(|(i, s)| (*s, C_SECTION + i as u32)).collect(),
            _ => vec![],
        }
    }

}
