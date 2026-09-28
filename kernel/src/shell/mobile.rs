//! HydatekOS Mobile shell: home screen, app grid, dock and full-screen apps.
//! Used for portrait devices, for the "Mobile shell" setting, and for the
//! virtual phone mirrored inside Phone Link.

use crate::apps::{self, App, AppKind};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, MobileAct, Ui};
use alloc::boxed::Box;

use super::wallpaper;

const GRID: [AppKind; 8] = [AppKind::Files, AppKind::Browser, AppKind::Mail, AppKind::Calendar, AppKind::Notes, AppKind::Music, AppKind::Camera, AppKind::Settings];
const DOCK: [AppKind; 4] = [AppKind::Phone, AppKind::Messages, AppKind::Browser, AppKind::Camera];

pub struct Mobile {
    pub id: u8,
    pub inst: u32,
    pub app: Option<Box<dyn App>>,
}

impl Mobile {
    pub fn new(id: u8, inst: u32) -> Mobile {
        Mobile { id, inst, app: None }
    }

    pub fn act(&mut self, a: MobileAct, sys: &mut Sys) {
        match a {
            MobileAct::Home => {
                if let Some(mut app) = self.app.take() {
                    app.close(sys);
                }
            }
            MobileAct::Open(k) => self.open(k, sys),
        }
    }

    pub fn open(&mut self, k: AppKind, sys: &mut Sys) {
        if self.app.as_ref().map(|a| a.kind()) == Some(k) {
            return;
        }
        if let Some(mut old) = self.app.take() {
            old.close(sys);
        }
        self.app = Some(apps::create(k, sys));
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys) {
        let t = ui.t;
        let u = |v: i32| v * r.w / 390;
        let old = ui.clip_in(r);
        let white = Color::rgb(0xF4EFE7);
        if let Some(app) = self.app.as_mut() {
            if app.fullscreen() {
                app.render(ui, r, sys, self.inst);
                ui.set_clip(old);
                return;
            }
            ui.rect(r, t.surface);
            // status bar
            let sb = Rect::new(r.x, r.y, r.w, u(34));
            ui.text_in(Rect::new(r.x + u(22), sb.y, u(80), sb.h), Face::Semibold, u(14), &sys.clock(), t.text, 0);
            ui.icon(Icon::Battery, r.r() - u(40), sb.y + u(9), u(18), t.text);
            ui.icon(Icon::Wifi, r.r() - u(64), sb.y + u(9), u(16), t.text);
            let area = Rect::new(r.x, r.y + u(34), r.w, r.h - u(34) - u(26));
            let inner = ui.clip_in(area);
            app.render(ui, area, sys, self.inst);
            ui.set_clip(inner);
            // home indicator
            let hb = Rect::new(r.x, r.b() - u(26), r.w, u(26));
            ui.rect(hb, t.surface);
            let a = Action::Mobile(self.id, MobileAct::Home);
            ui.rrect(Rect::new(r.x + (r.w - u(134)) / 2, hb.y + u(12), u(134), 5), 3, if ui.hot(a) { t.accent } else { t.text });
            ui.zone(hb, a);
            ui.set_clip(old);
            return;
        }

        wallpaper::draw(ui.c, r.scale(ui.s), &t, true);
        ui.icon(Icon::Battery, r.r() - u(40), r.y + u(12), u(18), t.text);
        ui.icon(Icon::Wifi, r.r() - u(64), r.y + u(12), u(16), t.text);
        // date + clock
        ui.text(r.x + u(24), r.y + u(90), Face::Regular, u(17), &sys.date_long(), t.text2);
        ui.text(r.x + u(20), r.y + u(178), Face::Display, u(96), &sys.clock(), t.text);
        // up next
        let card = Rect::new(r.x + u(24), r.y + u(213), r.w - u(48), u(94));
        ui.rrect(card, u(26), t.surface);
        let ic = Rect::new(card.x + u(20), card.y + u(25), u(44), u(44));
        ui.rrect(ic, u(12), t.accent);
        ui.icon_in(Icon::Calendar, ic, u(20), t.on_accent);
        ui.label(card.x + u(78), card.y + u(32), u(11), "UP NEXT", t.accent);
        let (title, sub) = match sys.next_event() {
            Some(e) => (e.title.clone(), alloc::format!("{}{}{}", sys.event_when(e), if e.place.is_empty() { "" } else { " · " }, e.place)),
            None => (alloc::string::String::from("Nothing scheduled"), alloc::string::String::from("Enjoy your day")),
        };
        let tw = card.w - u(96);
        let title = ui.fit(Face::Semibold, u(17), &title, tw);
        let sub = ui.fit(Face::Regular, u(13), &sub, tw);
        ui.text(card.x + u(78), card.y + u(56), Face::Semibold, u(17), &title, t.text);
        ui.text(card.x + u(78), card.y + u(76), Face::Regular, u(13), &sub, t.text2);
        ui.zone(card, Action::Mobile(self.id, MobileAct::Open(AppKind::Calendar)));
        // app grid
        let cw = (r.w - u(40)) / 4;
        for (i, k) in GRID.iter().enumerate() {
            let (cx, cy) = (i as i32 % 4, i as i32 / 4);
            let tile = Rect::new(r.x + u(20) + cx * cw + (cw - u(62)) / 2, r.y + u(335) + cy * u(106), u(62), u(62));
            let a = Action::Mobile(self.id, MobileAct::Open(*k));
            ui.shadow(tile, u(18), u(4), u(2), 30);
            ui.rrect(tile, u(18), if ui.hot(a) { t.surface.mix(t.accent, 20) } else { t.surface });
            ui.icon_in(k.icon(), tile, u(26), if k.warm() { t.accent } else { t.text });
            ui.text_in(Rect::new(tile.x - u(20), tile.b() + u(8), tile.w + u(40), u(20)), Face::Regular, u(14), k.name(), t.text, 1);
            ui.zone(Rect::new(tile.x - u(8), tile.y, tile.w + u(16), tile.h + u(30)), a);
        }
        // dock
        let dock = Rect::new(r.x + u(47), r.b() - u(110), r.w - u(94), u(80));
        ui.rrect(dock, u(40), t.dock.with_alpha(215));
        let bw = u(60);
        let gap = (dock.w - u(20) - bw * 4) / 3;
        for (i, k) in DOCK.iter().enumerate() {
            let b = Rect::new(dock.x + u(10) + i as i32 * (bw + gap), dock.y + u(10), bw, bw);
            let a = Action::Mobile(self.id, MobileAct::Open(*k));
            ui.rrect(b, u(20), if ui.hot(a) { t.dock_btn.mix(white, 30) } else { t.dock_btn });
            ui.icon_in(k.icon(), b, u(26), white);
            if *k == AppKind::Messages && sys.link.unread() > 0 {
                ui.circle(b.r() - u(8), b.y + u(8), u(6), t.accent);
            }
            ui.zone(b, a);
        }
        ui.set_clip(old);
    }
}
