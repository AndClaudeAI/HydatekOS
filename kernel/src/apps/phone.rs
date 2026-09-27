//! Phone (dialer) and Camera — mobile-shell apps.

use super::{App, AppKind};
use crate::font::Face;
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Ui};
use alloc::string::String;

const KEYS: [&str; 12] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "*", "0", "#"];

pub struct Phone {
    number: String,
    calling: bool,
}

impl Phone {
    pub fn new() -> Phone {
        Phone { number: String::new(), calling: false }
    }
}

impl App for Phone {
    fn kind(&self) -> AppKind {
        AppKind::Phone
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        let t = ui.t;
        let top = r.y + 30;
        let shown = if self.number.is_empty() { "Enter number" } else { &self.number };
        ui.text_in(Rect::new(r.x, top, r.w, 40), Face::Semibold, 24, shown, if self.number.is_empty() { t.text3 } else { t.text }, 1);
        if self.calling {
            ui.text_in(Rect::new(r.x, top + 40, r.w, 20), Face::Regular, 13, "No cellular modem on this device", t.text2, 1);
        }
        let kw = ((r.w - 60) / 3).min(76);
        let x0 = r.x + (r.w - kw * 3) / 2;
        for (i, k) in KEYS.iter().enumerate() {
            let (cx, cy) = (i as i32 % 3, i as i32 / 3);
            let b = Rect::new(x0 + cx * kw + 6, top + 80 + cy * (kw - 4), kw - 12, kw - 12);
            let a = Action::App(inst, i as u32);
            ui.rrect(b, b.w / 2, if ui.hot(a) { t.chip.mix(t.text, 30) } else { t.chip });
            ui.text_in(b, Face::Semibold, 20, k, t.text, 1);
            ui.zone(b, a);
        }
        let cb = Rect::new(r.x + r.w / 2 - 30, top + 80 + 4 * (kw - 4), 60, 60);
        let a = Action::App(inst, 20);
        ui.circle(cb.x + 30, cb.y + 30, 30, if self.calling { t.danger } else { t.accent });
        ui.icon_in(Icon::Phone, cb, 24, t.on_accent);
        ui.zone(cb, a);
        if !self.number.is_empty() {
            let d = Rect::new(cb.r() + 24, cb.y + 15, 30, 30);
            ui.icon_button(d, Icon::ChevronLeft, Action::App(inst, 21), 18);
        }
    }

    fn action(&mut self, code: u32, _double: bool, _sys: &mut Sys) {
        match code {
            20 => self.calling = !self.calling && !self.number.is_empty(),
            21 => {
                self.number.pop();
            }
            c if (c as usize) < KEYS.len() && self.number.len() < 15 => self.number.push_str(KEYS[c as usize]),
            _ => {}
        }
    }
}

pub struct Camera;

impl App for Camera {
    fn kind(&self) -> AppKind {
        AppKind::Camera
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, _inst: u32) {
        let t = ui.t;
        ui.rect(r, t.dune3);
        let c = Rect::new(r.x, r.y + r.h / 2 - 60, r.w, 120);
        ui.icon_in(Icon::Camera, Rect::new(c.x, c.y, c.w, 48), 40, t.sun);
        ui.text_in(Rect::new(c.x, c.y + 56, c.w, 24), Face::Semibold, 16, "No camera found", t.dock_icon, 1);
        ui.text_in(Rect::new(c.x, c.y + 80, c.w, 20), Face::Regular, 13, "USB video (UVC) support is planned", t.text3, 1);
    }
}
