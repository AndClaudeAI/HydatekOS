//! Music: library and player UI, and the player the desktop card shares.
//! The tracks are samples with no audio files yet, so playback is visual
//! only and says so.

use super::{App, AppKind, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Ui};
use alloc::format;

pub const TRACKS: [(&str, &str, u32); 5] = [
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

/// What's playing. It lives in Sys, so the desktop's player card and the
/// Music app show and steer the same thing.
#[derive(Default)]
pub struct Player {
    pub cur: usize,
    pub playing: bool,
    /// in ticks (100/s)
    pub pos: u32,
    pub shuffle: bool,
    /// play the same track again
    pub repeat: bool,
}

impl Player {
    pub fn track(&self) -> (&'static str, &'static str, u32) {
        TRACKS[self.cur % TRACKS.len()]
    }

    pub fn secs(&self) -> u32 {
        self.pos / 100
    }

    pub fn toggle(&mut self) {
        self.playing = !self.playing;
    }

    pub fn next(&mut self) {
        self.cur = if self.shuffle {
            // any other track
            (self.cur + 1 + (crate::arch::ms() as usize / 7) % (TRACKS.len() - 1)) % TRACKS.len()
        } else {
            (self.cur + 1) % TRACKS.len()
        };
        self.pos = 0;
    }

    pub fn prev(&mut self) {
        // back to the start first, like every player
        if self.pos >= 300 {
            self.pos = 0;
            return;
        }
        self.cur = (self.cur + TRACKS.len() - 1) % TRACKS.len();
        self.pos = 0;
    }

    pub fn pick(&mut self, i: usize) {
        self.cur = i.min(TRACKS.len() - 1);
        self.pos = 0;
        self.playing = true;
    }

    /// One tick (10 ms) of playing.
    pub fn tick(&mut self) {
        if !self.playing {
            return;
        }
        self.pos += 1;
        if self.secs() >= self.track().2 {
            if self.repeat {
                self.pos = 0;
            } else {
                self.next();
            }
        }
    }
}

pub struct Music {
    /// playing, as of the last tick (for animating)
    playing: bool,
    pos: u32,
}

impl Music {
    pub fn new() -> Music {
        Music { playing: false, pos: 0 }
    }
}

impl App for Music {
    fn kind(&self) -> AppKind {
        AppKind::Music
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let pl = &sys.player;
        ui.text_in(Rect::new(r.x + 20, r.y, 200, HEADER), Face::Semibold, 15, "Music", t.text, 0);
        ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);
        let mut y = r.y + HEADER + 10;
        for (i, (name, artist, len)) in TRACKS.iter().enumerate() {
            let a = Action::App(inst, C_TRACK + i as u32);
            let row = Rect::new(r.x + 12, y, r.w - 24, 40);
            if i == pl.cur {
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
        let (name, artist, len) = pl.track();
        let white = Color::rgb(0xF4EFE7);
        ui.text(p.x + 18, p.y + 28, Face::Semibold, 14, name, white);
        ui.text(p.x + 18, p.y + 46, Face::Regular, 12, artist, Color::rgb(0xB9B3C8));
        let cx = p.x + p.w / 2;
        for (i, (ic, code)) in [(Icon::SkipPrev, C_PREV), (if pl.playing { Icon::Pause } else { Icon::Play }, C_PLAY), (Icon::SkipNext, C_NEXT)].iter().enumerate() {
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
        let secs = pl.secs();
        ui.rrect(Rect::new(bar.x, bar.y, (bar.w as u32 * secs / len) as i32, 4), 2, t.sun);
        let note = "Sample tracks: the player shows them, with no sound yet";
        let nw = ui.tw(Face::Regular, 11, note);
        if p.w > 560 {
            ui.text(p.r() - nw - 16, p.y + 40, Face::Regular, 11, note, Color::rgb(0xB9B3C8));
        }
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        let pl = &mut sys.player;
        match code {
            C_PLAY => pl.toggle(),
            C_NEXT => pl.next(),
            C_PREV => pl.prev(),
            c if c >= C_TRACK => pl.pick((c - C_TRACK) as usize),
            _ => {}
        }
    }

    fn tick(&mut self, sys: &mut Sys) {
        self.playing = sys.player.playing;
        self.pos = sys.player.pos;
    }

    fn animating(&self) -> bool {
        self.playing && self.pos % 100 == 0
    }

}
