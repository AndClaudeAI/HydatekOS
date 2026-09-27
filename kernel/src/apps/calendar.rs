//! Calendar: month view with events stored on disk; feeds the "Up next" widget.

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::{days_in_month, weekday, CalEvent, Sys, MONTHS};
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

const C_PREV: u32 = 1;
const C_NEXT: u32 = 2;
const C_TODAY: u32 = 3;
const C_ADD: u32 = 4;
const C_FIELD: u32 = 10; // +0 title, +1 time, +2 place
const C_DEL: u32 = 50;
const C_DAY: u32 = 100;

pub struct Calendar {
    y: i32,
    m: i32,
    sel: i32,
    fields: [LineEdit; 3],
    focus: Option<usize>,
}

impl Calendar {
    pub fn new(sys: &mut Sys) -> Calendar {
        let t = sys.now;
        let mut c = Calendar { y: t.year as i32, m: t.month as i32, sel: t.day as i32, fields: Default::default(), focus: None };
        c.fields[1].text = "12:00".to_string();
        c
    }

    fn add(&mut self, sys: &mut Sys) {
        let title = self.fields[0].text.trim().to_string();
        if title.is_empty() {
            self.focus = Some(0);
            return;
        }
        let tm = &self.fields[1].text;
        let mut parts = tm.split(':');
        let hh = parts.next().and_then(|s| s.trim().parse::<u8>().ok()).unwrap_or(12).min(23);
        let mm = parts.next().and_then(|s| s.trim().parse::<u8>().ok()).unwrap_or(0).min(59);
        sys.add_event(CalEvent { y: self.y as u16, m: self.m as u8, d: self.sel as u8, hh, mm, title: title.replace('|', "/"), place: self.fields[2].text.replace('|', "/") });
        self.fields[0].text.clear();
        self.fields[2].text.clear();
        self.focus = None;
        sys.toast("Calendar", "Event added");
    }

    fn day_events<'a>(&self, sys: &'a Sys) -> Vec<(usize, &'a CalEvent)> {
        sys.events.iter().enumerate().filter(|(_, e)| e.y as i32 == self.y && e.m as i32 == self.m && e.d as i32 == self.sel).collect()
    }
}

impl App for Calendar {
    fn kind(&self) -> AppKind {
        AppKind::Calendar
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let title = format!("{} {}", MONTHS[(self.m - 1) as usize], self.y);
        ui.text_in(Rect::new(r.x + 20, r.y, 260, HEADER), Face::Semibold, 16, &title, t.text, 0);
        let bx = r.x + 20 + ui.tw(Face::Semibold, 16, &title) + 8;
        ui.icon_button(Rect::new(bx, r.y + 8, 28, 28), Icon::ChevronLeft, Action::App(inst, C_PREV), 16);
        ui.icon_button(Rect::new(bx + 30, r.y + 8, 28, 28), Icon::ChevronRight, Action::App(inst, C_NEXT), 16);
        ui.button(Rect::new(bx + 66, r.y + 9, 64, 26), "Today", Action::App(inst, C_TODAY), false);
        ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);

        let compact = super::compact(r);
        let panel_w = if compact { 0 } else { 230 };
        let g = if compact { Rect::new(r.x + 8, r.y + HEADER + 8, r.w - 16, (r.h - HEADER) * 45 / 100) } else { Rect::new(r.x + 16, r.y + HEADER + 12, r.w - panel_w - 32, r.h - HEADER - 24) };
        let cw = g.w / 7;
        for (i, d) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"].iter().enumerate() {
            ui.text_in(Rect::new(g.x + i as i32 * cw, g.y, cw, 20), Face::Semibold, 11, d, t.text2, 1);
        }
        let first = weekday(self.y, self.m, 1) as i32;
        let dim = days_in_month(self.y, self.m);
        let rows = (first + dim + 6) / 7;
        let ch = ((g.h - 26) / rows).min(62);
        let now = sys.now;
        for d in 1..=dim {
            let idx = first + d - 1;
            let cell = Rect::new(g.x + (idx % 7) * cw, g.y + 26 + (idx / 7) * ch, cw, ch);
            let a = Action::App(inst, C_DAY + d as u32);
            let today = self.y == now.year as i32 && self.m == now.month as i32 && d == now.day as i32;
            if d == self.sel {
                ui.rrect(cell.inset(2), 10, t.accent.with_alpha(40));
            } else if ui.hot(a) {
                ui.rrect(cell.inset(2), 10, t.hover);
            }
            let num = format!("{}", d);
            let nr = Rect::new(cell.x + (cw - 26) / 2, cell.y + 6, 26, 26);
            if today {
                ui.circle(nr.x + 13, nr.y + 13, 13, t.accent);
                ui.text_in(nr, Face::Semibold, 13, &num, t.on_accent, 1);
            } else {
                ui.text_in(nr, Face::Medium, 13, &num, t.text, 1);
            }
            let n = sys.events.iter().filter(|e| e.y as i32 == self.y && e.m as i32 == self.m && e.d as i32 == d).count() as i32;
            for k in 0..n.min(3) {
                ui.circle(cell.x + cw / 2 - (n.min(3) - 1) * 4 + k * 8, cell.y + 40, 2, t.accent);
            }
            ui.zone(cell, a);
        }

        // Day panel
        let p = if compact { Rect::new(r.x, g.b() + 6, r.w, r.b() - g.b() - 6) } else { Rect::new(r.r() - panel_w, r.y + HEADER + 1, panel_w, r.h - HEADER - 1) };
        super::panel(ui, r, p, t.sidebar);
        let wd = weekday(self.y, self.m, self.sel);
        let head = format!("{}, {} {}", &crate::sys::DAYS[wd][..3], self.sel, &MONTHS[(self.m - 1) as usize][..3]);
        ui.text(p.x + 18, p.y + 30, Face::Semibold, 15, &head, t.text);
        let mut y = p.y + 44;
        let evs = self.day_events(sys);
        if evs.is_empty() {
            ui.text(p.x + 18, y + 18, Face::Regular, 13, "No events", t.text3);
            y += 30;
        }
        let max_ev = ((p.b() - 176 - y) / 52).max(1) as usize;
        for (i, e) in evs.iter().take(max_ev) {
            let cr = Rect::new(p.x + 12, y, p.w - 24, 46);
            ui.rrect(cr, 10, t.surface);
            ui.rect(Rect::new(cr.x + 8, cr.y + 10, 3, 26), t.accent);
            let tt = ui.fit(Face::Semibold, 13, &e.title, cr.w - 50);
            ui.text(cr.x + 18, cr.y + 20, Face::Semibold, 13, &tt, t.text);
            let sub = format!("{:02}:{:02}{}{}", e.hh, e.mm, if e.place.is_empty() { "" } else { " · " }, e.place);
            let sub = ui.fit(Face::Regular, 12, &sub, cr.w - 50);
            ui.text(cr.x + 18, cr.y + 37, Face::Regular, 12, &sub, t.text2);
            ui.icon_button(Rect::new(cr.r() - 30, cr.y + 11, 24, 24), Icon::Close, Action::App(inst, C_DEL + *i as u32), 12);
            y += 52;
        }
        y = y.max(p.b() - 170);
        ui.label(p.x + 18, y, 10, "NEW EVENT", t.accent);
        let labels = ["Title", "Time (HH:MM)", "Place"];
        for k in 0..3 {
            ui.field(Rect::new(p.x + 12, y + 10 + k as i32 * 36, p.w - 24, 30), &self.fields[k].text, labels[k], self.focus == Some(k), Action::App(inst, C_FIELD + k as u32));
        }
        ui.button(Rect::new(p.x + 12, y + 118, p.w - 24, 30), "Add event", Action::App(inst, C_ADD), true);
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        if !(C_FIELD..C_FIELD + 3).contains(&code) {
            self.focus = None;
        }
        match code {
            C_PREV => {
                self.m -= 1;
                if self.m == 0 {
                    self.m = 12;
                    self.y -= 1;
                }
                self.sel = self.sel.min(days_in_month(self.y, self.m));
            }
            C_NEXT => {
                self.m += 1;
                if self.m == 13 {
                    self.m = 1;
                    self.y += 1;
                }
                self.sel = self.sel.min(days_in_month(self.y, self.m));
            }
            C_TODAY => {
                self.y = sys.now.year as i32;
                self.m = sys.now.month as i32;
                self.sel = sys.now.day as i32;
            }
            C_ADD => self.add(sys),
            c if (C_FIELD..C_FIELD + 3).contains(&c) => self.focus = Some((c - C_FIELD) as usize),
            c if (C_DEL..C_DAY).contains(&c) => {
                let i = (c - C_DEL) as usize;
                if i < sys.events.len() {
                    sys.events.remove(i);
                    sys.save_events();
                }
            }
            c if c >= C_DAY => self.sel = (c - C_DAY) as i32,
            _ => {}
        }
    }

    fn key(&mut self, k: Key, _ctrl: bool, sys: &mut Sys) {
        if let Some(f) = self.focus {
            match k {
                Key::Enter => self.add(sys),
                Key::Tab => self.focus = Some((f + 1) % 3),
                Key::Esc => self.focus = None,
                _ => {
                    self.fields[f].key(k);
                }
            }
            return;
        }
        let dim = days_in_month(self.y, self.m);
        match k {
            Key::Left => self.sel = (self.sel - 1).max(1),
            Key::Right => self.sel = (self.sel + 1).min(dim),
            Key::Up => self.sel = (self.sel - 7).max(1),
            Key::Down => self.sel = (self.sel + 7).min(dim),
            Key::PageUp => self.action(C_PREV, false, sys),
            Key::PageDown => self.action(C_NEXT, false, sys),
            _ => {}
        }
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => vec![("New Event", C_FIELD)],
            3 => vec![("Today", C_TODAY), ("Previous Month", C_PREV), ("Next Month", C_NEXT)],
            _ => vec![],
        }
    }

    fn animating(&self) -> bool {
        self.focus.is_some()
    }

}
