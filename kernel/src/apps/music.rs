//! Music: library and player UI. Audio output needs an Intel HDA driver
//! (milestone 4), so playback is visual only for now and says so.

use super::{App, AppKind, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Ui};
use alloc::format;

const TRACKS: [(&str, &str, u32); 5] = [
    ("Dune Sunrise", "Hydatek Sessions", 214),
    ("Oasis", "Hydatek Sessions", 187),
    ("Warm Circuits", "The Kernels", 242),
    ("Night Caravan", "Amber Fields", 201),
    ("Golden Hour", "Amber Fields", 176),
];

const C_PLAY: u32 = 1;
const C_NEXT: u32 = 2;
const C_PREV: u32 = 3;
const C_TRACK: u32 = 100;

pub struct Music {
    cur: usize,
    playing: bool,
    pos: u32, // in ticks (100/s)
}

impl Music {
    pub fn new() -> Music {
        Music { cur: 0, playing: false, pos: 0 }
    }
}

impl App for Music {
    fn kind(&self) -> AppKind {
        AppKind::Music
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        let t = ui.t;
        ui.text_in(Rect::new(r.x + 20, r.y, 200, HEADER), Face::Semibold, 15, "Music", t.text, 0);
        ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);
        let mut y = r.y + HEADER + 10;
        for (i, (name, artist, len)) in TRACKS.iter().enumerate() {
            let a = Action::App(inst, C_TRACK + i as u32);
            let row = Rect::new(r.x + 12, y, r.w - 24, 40);
            if i == self.cur {
                ui.rrect(row, 10, t.accent.with_alpha(36));
            } else if ui.hot(a) {
                ui.rrect(row, 10, t.hover);
            }
            ui.rrect(Rect::new(row.x + 8, row.y + 6, 28, 28), 8, t.tile);
            ui.icon_in(Icon::Music, Rect::new(row.x + 8, row.y + 6, 28, 28), 14, t.accent);
            ui.text(row.x + 48, row.y + 18, Face::Semibold, 13, name, t.text);
            ui.text(row.x + 48, row.y + 34, Face::Regular, 12, artist, t.text2);
            let d = format!("{}:{:02}", len / 60, len % 60);
            let dw = ui.tw(Face::Regular, 12, &d);
            ui.text(row.r() - dw - 12, row.y + 25, Face::Regular, 12, &d, t.text2);
            ui.zone(row, a);
            y += 44;
        }
        // Player bar
        let p = Rect::new(r.x + 12, r.b() - 84, r.w - 24, 72);
        ui.rrect(p, 16, t.dune3);
        let (name, artist, len) = TRACKS[self.cur];
        let white = Color::rgb(0xF4EFE7);
        ui.text(p.x + 18, p.y + 28, Face::Semibold, 14, name, white);
        ui.text(p.x + 18, p.y + 46, Face::Regular, 12, artist, Color::rgb(0xB9B3C8));
        let cx = p.x + p.w / 2;
        for (i, (ic, code)) in [(Icon::SkipPrev, C_PREV), (if self.playing { Icon::Pause } else { Icon::Play }, C_PLAY), (Icon::SkipNext, C_NEXT)].iter().enumerate() {
            let b = Rect::new(cx - 60 + i as i32 * 44, p.y + 8, 36, 36);
            let a = Action::App(inst, *code);
            if i == 1 {
                ui.circle(b.x + 18, b.y + 18, 18, t.accent);
            } else if ui.hot(a) {
                ui.circle(b.x + 18, b.y + 18, 18, Color::rgba(0xFFFFFF, 30));
            }
            ui.icon_in(*ic, b, 16, white);
            ui.zone(b, a);
        }
        let bar = Rect::new(cx - 100, p.y + 54, 200, 4);
        ui.rrect(bar, 2, Color::rgba(0xFFFFFF, 50));
        let secs = self.pos / 100;
        ui.rrect(Rect::new(bar.x, bar.y, (bar.w as u32 * secs / len) as i32, 4), 2, t.sun);
        let note = "No audio device driver yet (HDA arrives in M4)";
        let nw = ui.tw(Face::Regular, 11, note);
        if p.w > 560 {
            ui.text(p.r() - nw - 16, p.y + 40, Face::Regular, 11, note, Color::rgb(0xB9B3C8));
        }
    }

    fn action(&mut self, code: u32, _double: bool, _sys: &mut Sys) {
        match code {
            C_PLAY => self.playing = !self.playing,
            C_NEXT => {
                self.cur = (self.cur + 1) % TRACKS.len();
                self.pos = 0;
            }
            C_PREV => {
                self.cur = (self.cur + TRACKS.len() - 1) % TRACKS.len();
                self.pos = 0;
            }
            c if c >= C_TRACK => {
                self.cur = ((c - C_TRACK) as usize).min(TRACKS.len() - 1);
                self.pos = 0;
                self.playing = true;
            }
            _ => {}
        }
    }

    fn tick(&mut self, _sys: &mut Sys) {
        if self.playing {
            self.pos += 1;
            if self.pos / 100 >= TRACKS[self.cur].2 {
                self.cur = (self.cur + 1) % TRACKS.len();
                self.pos = 0;
            }
        }
    }

    fn animating(&self) -> bool {
        self.playing && self.pos % 100 == 0
    }

}
