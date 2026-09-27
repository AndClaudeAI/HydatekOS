//! Mail: local mailbox (IMAP/SMTP accounts arrive with TLS in milestone 3).

use super::{App, AppKind, HEADER};
use crate::font::Face;
use crate::gfx::Rect;
use crate::sys::Sys;
use crate::ui::{Action, Ui};

const MAILS: [(&str, &str, &str, &[&str]); 3] = [
    ("Hydatek Team", "Welcome to HydatekOS", "09:00", &[
        "Hi there,",
        "",
        "Thanks for trying HydatekOS 0.1 \"Dune\". Everything you see — the kernel, compositor, fonts, icons and apps — is HydatekOS code running directly on your PC's firmware.",
        "",
        "Start with Files, Notes and Phone Link, then open Terminal and type 'help'.",
        "",
        "— The Hydatek team",
    ]),
    ("Phone Link", "Your phone, on your desktop", "Yesterday", &[
        "Scan the code in Phone Link with your phone to share photos, files and text. Install the Android app from the page that opens to read and reply to texts, see notifications and make calls.",
        "",
        "Open Phone Link from the dock to get started.",
    ]),
    ("Hydatek Team", "What's next: the roadmap", "Mon", &[
        "Milestone 2: own interrupt/timer handling and PS/2 + USB input.",
        "Milestone 3: networking. TCP/IP and Phone Link are done; DNS, TLS and Mail sync are next.",
        "Milestone 4: audio, Bluetooth and Phone Link calls over Bluetooth.",
        "",
        "See docs/ROADMAP.md in the source tree.",
    ]),
];

pub struct Mail {
    sel: usize,
    /// Phone layout: reading a message (otherwise the inbox list).
    reading: bool,
}

impl Mail {
    pub fn new() -> Mail {
        Mail { sel: 0, reading: false }
    }
}

const C_BACK: u32 = 99;

impl App for Mail {
    fn kind(&self) -> AppKind {
        AppKind::Mail
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        let t = ui.t;
        let compact = super::compact(r);
        let list_w = if compact { if self.reading { 0 } else { r.w } } else { 240 };
        if list_w > 0 {
        super::panel(ui, r, Rect::new(r.x, r.y, list_w, r.h), t.sidebar);
        ui.text_in(Rect::new(r.x + 18, r.y, 150, HEADER), Face::Semibold, 15, "Inbox", t.text, 0);
        let mut y = r.y + HEADER;
        for (i, (from, subj, when, _)) in MAILS.iter().enumerate() {
            let a = Action::App(inst, i as u32);
            let row = Rect::new(r.x + 8, y, list_w - 16, 62);
            if i == self.sel {
                ui.rrect(row, 10, t.accent.with_alpha(40));
            } else if ui.hot(a) {
                ui.rrect(row, 10, t.hover);
            }
            ui.text(row.x + 12, row.y + 22, Face::Semibold, 13, from, t.text);
            let ww = ui.tw(Face::Regular, 11, when);
            ui.text(row.r() - ww - 10, row.y + 22, Face::Regular, 11, when, t.text3);
            let s = ui.fit(Face::Regular, 12, subj, row.w - 24);
            ui.text(row.x + 12, row.y + 42, Face::Regular, 12, &s, t.text2);
            ui.zone(row, a);
            y += 66;
        }
        ui.text(r.x + 18, r.b() - 16, Face::Regular, 11, "Accounts need TLS (milestone 3)", t.text3);
        }
        if list_w >= r.w {
            return;
        }
        let pad = if compact { 16 } else { 28 };
        let m = Rect::new(r.x + list_w + pad, r.y, r.w - list_w - 2 * pad, r.h);
        if compact {
            ui.icon_button(Rect::new(r.x + 10, r.y + 8, 28, 28), crate::icons::Icon::ChevronLeft, Action::App(inst, C_BACK), 16);
        }
        ui.rect(Rect::new(r.x + list_w, r.y + HEADER, r.w - list_w, 1), t.line);
        let (from, subj, when, body) = MAILS[self.sel];
        let subj = ui.fit(Face::Semibold, 20, subj, m.w);
        ui.text(m.x, r.y + HEADER + 36, Face::Semibold, 20, &subj, t.text);
        ui.text(m.x, r.y + HEADER + 58, Face::Regular, 13, &alloc::format!("{} · {}", from, when), t.text2);
        let mut y = r.y + HEADER + 96;
        let old = ui.clip_in(Rect::new(m.x, r.y, m.w, r.h));
        for l in body.iter() {
            if l.is_empty() {
                y += 12;
                continue;
            }
            for wl in ui.wrap(Face::Regular, 14, l, m.w.min(560)) {
                ui.text(m.x, y, Face::Regular, 14, &wl, t.text);
                y += 22;
            }
        }
        ui.set_clip(old);
    }

    fn action(&mut self, code: u32, _double: bool, _sys: &mut Sys) {
        if code == C_BACK {
            self.reading = false;
            return;
        }
        self.reading = true;
        self.sel = (code as usize).min(MAILS.len() - 1);
    }

}
