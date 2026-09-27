//! Browser: renders built-in hydatek:// pages. Web pages need the network
//! stack (milestone 3); the address bar reports that honestly.

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const C_URL: u32 = 1;
const C_BACK: u32 = 2;
const C_LINK: u32 = 100;

fn page(url: &str) -> Vec<String> {
    let lines: &[&str] = match url {
        "hydatek://start" => &[
            "# Welcome to HydatekOS",
            "A calm desktop and mobile operating system, written from scratch.",
            "",
            "## Explore",
            "[hydatek://about] About HydatekOS",
            "[hydatek://roadmap] Roadmap",
            "[hydatek://shortcuts] Keyboard and mouse shortcuts",
            "[hydatek://link] How Phone Link works",
        ],
        "hydatek://about" => &[
            "# About HydatekOS",
            "HydatekOS 0.1 \"Dune\" boots directly from UEFI firmware on any x86-64 PC.",
            "The kernel, memory allocator, compositor, window manager, font and icon",
            "renderers, file system layer and every app are HydatekOS code.",
            "",
            "- Language: Rust (no_std, zero third-party crates)",
            "- Graphics: software compositor on the UEFI framebuffer",
            "- Storage: files live in \\HYDATEK on the boot disk",
            "",
            "[hydatek://start] Back to start",
        ],
        "hydatek://roadmap" => &[
            "# Roadmap",
            "- M1 (now): desktop + mobile shells, apps, persistent files, Phone Link UI",
            "- M2: own interrupts, timers, PS/2 and USB HID drivers, exit boot services",
            "- M3: network stack (virtio-net, e1000, TCP/IP, DHCP, DNS), real web pages",
            "- M4: audio (Intel HDA), Bluetooth, native Phone Link transport",
            "- M5: installer to internal NVMe/SATA, users and permissions",
            "",
            "[hydatek://start] Back to start",
        ],
        "hydatek://shortcuts" => &[
            "# Shortcuts",
            "- Drag a window by its header; double-click the header to maximise",
            "- Drag the bottom-right corner to resize",
            "- Ctrl+S saves in Notes; Tab completes paths in Terminal",
            "- Esc closes menus and the app launcher",
            "- F2 renames the selected file; Delete moves it to the Bin",
            "",
            "[hydatek://start] Back to start",
        ],
        "hydatek://link" => &[
            "# Phone Link",
            "Phone Link pairs HydatekOS with a phone using the Hydatek Link Protocol.",
            "Channels: messages, notifications, photos, calls and screen mirroring.",
            "Transport: Wi-Fi (TCP) or Bluetooth once the drivers ship.",
            "",
            "[hydatek://start] Back to start",
        ],
        _ => &[],
    };
    if lines.is_empty() {
        return vec![
            "# Can't reach this page".to_string(),
            format!("HydatekOS can't open {} yet.", url),
            "The network stack (drivers + TCP/IP) arrives in milestone 3.".to_string(),
            "".to_string(),
            "[hydatek://start] Go to the start page".to_string(),
        ];
    }
    lines.iter().map(|s| s.to_string()).collect()
}

pub struct Browser {
    url: String,
    hist: Vec<String>,
    edit: Option<LineEdit>,
    links: Vec<String>,
}

impl Browser {
    pub fn new() -> Browser {
        Browser { url: "hydatek://start".to_string(), hist: vec![], edit: None, links: vec![] }
    }
    fn go(&mut self, u: &str) {
        let mut u = u.trim().to_string();
        if !u.contains("://") {
            u = if u.contains('.') { format!("https://{}", u) } else { format!("hydatek://{}", u) };
        }
        self.hist.push(core::mem::replace(&mut self.url, u));
    }
}

impl App for Browser {
    fn kind(&self) -> AppKind {
        AppKind::Browser
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        let t = ui.t;
        ui.icon_button(Rect::new(r.x + 14, r.y + 8, 28, 28), Icon::ChevronLeft, Action::App(inst, C_BACK), 16);
        let bar = Rect::new(r.x + 50, r.y + 8, r.w - 180, 28);
        let (text, focus) = match &self.edit {
            Some(e) => (e.text.clone(), true),
            None => (self.url.clone(), false),
        };
        ui.field(bar, &text, "Search or enter address", focus, Action::App(inst, C_URL));
        ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);
        let body = Rect::new(r.x + 40, r.y + HEADER + 24, r.w - 80, r.h - HEADER - 30);
        let old = ui.clip_in(body);
        let mut y = body.y;
        self.links.clear();
        for l in page(&self.url) {
            if let Some(h) = l.strip_prefix("# ") {
                y += 30;
                ui.text(body.x, y, Face::Semibold, 28, h, t.text);
                y += 16;
            } else if let Some(h) = l.strip_prefix("## ") {
                y += 26;
                ui.text(body.x, y, Face::Semibold, 16, h, t.text);
                y += 4;
            } else if let Some(rest) = l.strip_prefix('[') {
                let (u, label) = rest.split_once("] ").unwrap_or((rest, rest));
                y += 30;
                let a = Action::App(inst, C_LINK + self.links.len() as u32);
                let w = ui.tw(Face::Medium, 14, label);
                let lr = Rect::new(body.x - 4, y - 18, w + 8, 24);
                if ui.hot(a) {
                    ui.rrect(lr, 6, t.hover);
                }
                ui.text(body.x, y, Face::Medium, 14, label, t.accent);
                ui.zone(lr, a);
                self.links.push(u.to_string());
            } else if let Some(b) = l.strip_prefix("- ") {
                y += 24;
                ui.circle(body.x + 4, y - 5, 2, t.text2);
                ui.text(body.x + 16, y, Face::Regular, 14, b, t.text);
            } else {
                y += 24;
                ui.text(body.x, y, Face::Regular, 14, &l, t.text2);
            }
        }
        ui.set_clip(old);
    }

    fn action(&mut self, code: u32, _double: bool, _sys: &mut Sys) {
        if code != C_URL {
            self.edit = None;
        }
        match code {
            C_URL => {
                if self.edit.is_none() {
                    self.edit = Some(LineEdit { text: String::new() });
                }
            }
            C_BACK => {
                if let Some(u) = self.hist.pop() {
                    self.url = u;
                }
            }
            c if c >= C_LINK => {
                if let Some(u) = self.links.get((c - C_LINK) as usize).cloned() {
                    self.go(&u);
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, _ctrl: bool, _sys: &mut Sys) {
        if let Some(e) = &mut self.edit {
            match k {
                Key::Enter => {
                    let u = e.text.clone();
                    self.edit = None;
                    if !u.trim().is_empty() {
                        self.go(&u);
                    }
                }
                Key::Esc => self.edit = None,
                _ => {
                    e.key(k);
                }
            }
        } else if let Key::Char(c) = k {
            let mut e = LineEdit::default();
            e.text.push(c);
            self.edit = Some(e);
        }
    }

    fn animating(&self) -> bool {
        self.edit.is_some()
    }

}
