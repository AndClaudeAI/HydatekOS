//! Boot splash: the HydatekOS mark on the dune wallpaper, with a progress bar
//! and the current startup step, shown while the system comes up.

use super::{logo, wallpaper};
use crate::font::Face;
use crate::gfx::{Canvas, Color, Rect};
use crate::theme::theme;
use crate::ui::Ui;

pub struct Splash {
    bg: Canvas,
    pub frame: Canvas,
    s: i32,
}

impl Splash {
    /// `w`/`h` are physical pixels, `s` the display scale.
    pub fn new(w: i32, h: i32, s: i32) -> Splash {
        let mut bg = Canvas::new(w, h);
        let b = bg.bounds();
        wallpaper::draw(&mut bg, b, &theme(false, 0), h > w);
        Splash { frame: Canvas::new(w, h), bg, s }
    }

    /// Redraw with `progress` (0-100) and a short status line.
    pub fn step(&mut self, progress: i32, status: &str) -> &Canvas {
        let b = self.frame.bounds();
        self.frame.copy_from(&self.bg, b);
        let t = theme(false, 0);
        let (w, h) = (self.frame.w / self.s, self.frame.h / self.s);
        let mut ui = Ui::new(&mut self.frame, self.s, t, None, 0);
        let size = 88;
        let cx = w / 2;
        let top = h * 36 / 100 - size / 2;
        ui.shadow(Rect::new(cx - size / 2, top, size, size), 24, 16, 8, 60);
        logo(&mut ui, cx - size / 2, top, size, t.accent, t.on_accent);
        let name = "HydatekOS";
        let nw = ui.tw(Face::Semibold, 28, name);
        ui.text(cx - nw / 2, top + size + 48, Face::Semibold, 28, name, t.text);
        let bar = Rect::new(cx - 110, top + size + 82, 220, 4);
        ui.rrect(bar, 2, Color::rgba(0x1E1B2C, 28));
        let fill = bar.w * progress.clamp(0, 100) / 100;
        if fill > 0 {
            ui.rrect(Rect::new(bar.x, bar.y, fill.max(4), bar.h), 2, t.accent);
        }
        let sw = ui.tw(Face::Regular, 12, status);
        ui.text(cx - sw / 2, bar.b() + 24, Face::Regular, 12, status, t.text2);
        &self.frame
    }
}
