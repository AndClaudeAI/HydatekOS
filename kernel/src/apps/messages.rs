//! Messages: text conversations synced from the linked phone.

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::{Req, Sys};
use crate::ui::{Action, Key, Ui};
use alloc::string::String;
use alloc::vec::Vec;

/// Conversation list + thread view, shared by Messages and Phone Link.
pub struct MsgView {
    pub thread: usize,
    pub input: LineEdit,
    pub focus: bool,
    /// Phone layout: conversation open (otherwise the thread list).
    pub open: bool,
}

pub const M_INPUT: u32 = 0;
pub const M_SEND: u32 = 1;
pub const M_PAIR: u32 = 2;
pub const M_BACK: u32 = 3;
pub const M_THREAD: u32 = 100;

impl MsgView {
    pub fn new() -> MsgView {
        MsgView { thread: 0, input: LineEdit::default(), focus: false, open: false }
    }

    pub fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32, base: u32) {
        let t = ui.t;
        let compact = super::compact(r);
        let list_w = if compact { if self.open { 0 } else { r.w } } else { 200.min(r.w / 3) };
        if list_w > 0 {
        super::panel(ui, Rect::new(r.x, r.y - 100, r.w, r.h + 100), Rect::new(r.x, r.y, list_w, r.h), t.sidebar);
        let mut y = r.y + 8;
        for (i, th) in sys.link.threads.iter().enumerate() {
            let a = Action::App(inst, base + M_THREAD + i as u32);
            let row = Rect::new(r.x + 8, y, list_w - 16, 52);
            if i == self.thread {
                ui.rrect(row, 10, t.accent.with_alpha(40));
            } else if ui.hot(a) {
                ui.rrect(row, 10, t.hover);
            }
            ui.circle(row.x + 22, row.y + 26, 16, t.tile);
            let ini: String = th.name.chars().take(1).collect();
            ui.text_in(Rect::new(row.x + 6, row.y + 10, 32, 32), Face::Semibold, 14, &ini, t.accent, 1);
            let nm = ui.fit(Face::Semibold, 13, &th.name, row.w - 60);
            ui.text(row.x + 46, row.y + 22, Face::Semibold, 13, &nm, t.text);
            if let Some(last) = th.msgs.last() {
                let prev = ui.fit(Face::Regular, 12, &last.text, row.w - 60);
                ui.text(row.x + 46, row.y + 40, Face::Regular, 12, &prev, if th.unread { t.text } else { t.text2 });
            }
            if th.unread {
                ui.circle(row.r() - 10, row.y + 18, 4, t.accent);
            }
            ui.zone(row, a);
            y += 56;
        }
        }
        if list_w >= r.w {
            return;
        }
        let c = Rect::new(r.x + list_w, r.y, r.w - list_w, r.h);
        let Some(th) = sys.link.threads.get(self.thread) else { return };
        let nx = if compact { c.x + 46 } else { c.x + 20 };
        if compact {
            ui.icon_button(Rect::new(c.x + 10, c.y + 14, 28, 28), Icon::ChevronLeft, Action::App(inst, base + M_BACK), 16);
        }
        ui.text(nx, c.y + 26, Face::Semibold, 15, &th.name, t.text);
        ui.text(nx, c.y + 44, Face::Regular, 12, "via your phone", t.text3);
        ui.rect(Rect::new(c.x, c.y + 56, c.w, 1), t.line);
        // bubbles, bottom-aligned
        let area = Rect::new(c.x + 16, c.y + 64, c.w - 32, c.h - 64 - 56);
        let old = ui.clip_in(area);
        let mut by = area.b() - 6;
        for m in th.msgs.iter().rev() {
            let maxw = area.w * 3 / 4;
            let lines = ui.wrap(Face::Regular, 13, &m.text, maxw - 26);
            let w = lines.iter().map(|l| ui.tw(Face::Regular, 13, l)).max().unwrap_or(0) + 26;
            let h = 16 + 18 * lines.len() as i32;
            by -= h + 18;
            if by + h < area.y {
                break;
            }
            let x = if m.me { area.r() - w } else { area.x };
            let (bg, fg) = if m.me { (t.accent, t.on_accent) } else { (t.tile, t.text) };
            ui.rrect(Rect::new(x, by, w, h), 16, bg);
            for (i, l) in lines.iter().enumerate() {
                ui.text(x + 13, by + 22 + i as i32 * 18, Face::Regular, 13, l, fg);
            }
            let tw = ui.tw(Face::Regular, 11, &m.time);
            let tx = if m.me { area.r() - tw - 4 } else { area.x + 4 };
            ui.text(tx, by + h + 13, Face::Regular, 11, &m.time, t.text3);
        }
        ui.set_clip(old);
        let ir = Rect::new(c.x + 16, c.b() - 46, c.w - 70, 34);
        ui.field(ir, &self.input.text, "Text message", self.focus, Action::App(inst, base + M_INPUT));
        let sb = Rect::new(c.r() - 48, c.b() - 46, 34, 34);
        let a = Action::App(inst, base + M_SEND);
        ui.rrect(sb, 17, if ui.hot(a) { t.accent.mix(t.text, 40) } else { t.accent });
        ui.icon_in(Icon::Send, sb, 16, t.on_accent);
        ui.zone(sb, a);
    }

    pub fn action(&mut self, code: u32, sys: &mut Sys) {
        self.focus = code == M_INPUT;
        match code {
            M_SEND => self.send(sys),
            M_PAIR => sys.reqs.push(Req::Open(AppKind::PhoneLink)),
            M_BACK => self.open = false,
            c if c >= M_THREAD => {
                self.open = true;
                self.thread = (c - M_THREAD) as usize;
                if let Some(t) = sys.link.threads.get_mut(self.thread) {
                    t.unread = false;
                }
            }
            _ => {}
        }
    }

    fn send(&mut self, sys: &mut Sys) {
        let text = self.input.text.trim();
        if text.is_empty() {
            return;
        }
        let now = sys.clock();
        let ticks = sys.ticks;
        sys.link.send(self.thread, text, now, ticks);
        self.input.text.clear();
    }

    pub fn key(&mut self, k: Key, sys: &mut Sys) {
        match k {
            Key::Enter => self.send(sys),
            Key::Esc => self.focus = false,
            _ => {
                self.focus = true;
                self.input.key(k);
            }
        }
    }
}

pub struct Messages {
    view: MsgView,
}

impl Messages {
    pub fn new() -> Messages {
        Messages { view: MsgView::new() }
    }
}

/// Placeholder shown when no phone is paired.
pub fn not_paired(ui: &mut Ui, r: Rect, inst: u32, code: u32, title: &str, sub: &str) {
    let t = ui.t;
    let c = Rect::new(r.x + (r.w - 320) / 2, r.y + (r.h - 190) / 2, 320, 190);
    ui.rrect(Rect::new(c.x + 132, c.y, 56, 56), 16, t.tile);
    ui.icon_in(Icon::Link, Rect::new(c.x + 132, c.y, 56, 56), 26, t.accent);
    ui.text_in(Rect::new(c.x, c.y + 70, c.w, 24), Face::Semibold, 16, title, t.text, 1);
    let sub = ui.fit(Face::Regular, 13, sub, r.w - 40);
    ui.text_in(Rect::new(r.x, c.y + 96, r.w, 20), Face::Regular, 13, &sub, t.text2, 1);
    ui.button(Rect::new(c.x + 90, c.y + 130, 140, 34), "Open Phone Link", Action::App(inst, code), true);
}

impl App for Messages {
    fn kind(&self) -> AppKind {
        AppKind::Messages
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        ui.text_in(Rect::new(r.x + 20, r.y, 200, HEADER), Face::Semibold, 15, "Messages", t.text, 0);
        ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);
        let body = Rect::new(r.x, r.y + HEADER + 1, r.w, r.h - HEADER - 1);
        if !sys.link.paired {
            return not_paired(ui, body, inst, M_PAIR, "Link your phone", "Messages from your phone appear here");
        }
        if !sys.link.has("sms") {
            return not_paired(ui, body, inst, M_PAIR, "Texts need the Android app", "Install HydatekOS Link on your phone to read and reply here");
        }
        self.view.render(ui, body, sys, inst, 0);
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        self.view.action(code, sys);
    }

    fn key(&mut self, k: Key, _gen: bool, sys: &mut Sys) {
        self.view.key(k, sys);
    }

    fn animating(&self) -> bool {
        self.view.focus
    }

    fn menu(&self, _idx: usize) -> Vec<(&'static str, u32)> {
        Vec::new()
    }
}
