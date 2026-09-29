//! Immediate-mode UI toolkit. Every frame the shell re-describes the screen in
//! logical units; `Ui` scales to physical pixels and records clickable zones.

use crate::apps::AppKind;
use crate::font::{self, Face};
use crate::gfx::{Canvas, Color, Rect};
use crate::icons::{self, Icon};
use crate::theme::Theme;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Background,
    Launch(AppKind),
    ToggleLauncher,
    Quick(u8),
    WinFocus(u32),
    WinDrag(u32),
    WinClose(u32),
    WinMin(u32),
    WinMax(u32),
    WinResize(u32),
    /// App-defined action: (app instance, code)
    App(u32, u32),
    Menu(u8),
    MenuItem(u8, u8),
    Mobile(u8, MobileAct),
    Toast(u32),
    /// The scaled mirror of the linked phone's screen.
    /// A scaled mobile screen: 0 = this device's mobile shell, 1 = linked phone.
    Mirror(u8),
    /// Lock screen: tap, keypad digit, backspace or enter (see shell::lock)
    Lock(u8),
    /// The setup assistant (see shell::setup)
    Setup(u16),
    Swallow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MobileAct {
    Open(AppKind),
    Home,
}

#[derive(Clone, Copy)]
pub struct Zone {
    pub r: Rect,
    pub a: Action,
}

/// Keys found on laptops and multimedia keyboards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Media {
    Mute,
    VolumeUp,
    VolumeDown,
    BrightnessUp,
    BrightnessDown,
    Sleep,
    Hibernate,
    Display,
    Recovery,
    Eject,
}

impl Media {
    pub fn name(self) -> &'static str {
        match self {
            Media::Mute => "Mute",
            Media::VolumeUp => "Volume up",
            Media::VolumeDown => "Volume down",
            Media::BrightnessUp => "Brightness up",
            Media::BrightnessDown => "Brightness down",
            Media::Sleep => "Sleep",
            Media::Hibernate => "Hibernate",
            Media::Display => "Display",
            Media::Recovery => "Recovery",
            Media::Eject => "Eject",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Char(char),
    /// Ctrl+letter while Ctrl isn't the Gen key (lower case)
    Ctrl(char),
    /// Aux+key that types no special character (lower case)
    Aux(char),
    Insert,
    Pause,
    /// volume, brightness and power keys
    Media(Media),
    /// a key the firmware reported that HydatekOS doesn't know (scan code)
    Other(u16),
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Esc,
    Tab,
    F(u8),
}

pub struct Ui<'a> {
    pub c: &'a mut Canvas,
    pub s: i32,
    pub t: Theme,
    pub zones: Vec<Zone>,
    pub hover: Option<Action>,
    pub clip: Rect,
    pub ticks: u64,
    /// Rect (logical) where a window asked for the linked phone to be drawn.
    pub phone_embed: Option<Rect>,
}

impl<'a> Ui<'a> {
    pub fn new(c: &'a mut Canvas, s: i32, t: Theme, hover: Option<Action>, ticks: u64) -> Ui<'a> {
        let clip = Rect::new(0, 0, c.w / s, c.h / s);
        c.clip = c.bounds();
        Ui { c, s, t, zones: Vec::new(), hover, clip, ticks, phone_embed: None }
    }

    pub fn set_clip(&mut self, r: Rect) -> Rect {
        let old = self.clip;
        self.clip = r;
        self.c.set_clip(r.scale(self.s));
        old
    }

    pub fn clip_in(&mut self, r: Rect) -> Rect {
        let n = r.intersect(&self.clip);
        self.set_clip(n)
    }

    pub fn zone(&mut self, r: Rect, a: Action) {
        let r = r.intersect(&self.clip);
        if !r.is_empty() {
            self.zones.push(Zone { r, a });
        }
    }

    pub fn hot(&self, a: Action) -> bool {
        self.hover == Some(a)
    }

    pub fn rect(&mut self, r: Rect, c: Color) {
        self.c.fill_rect(r.scale(self.s), c);
    }
    pub fn rrect(&mut self, r: Rect, rad: i32, c: Color) {
        self.c.fill_rrect(r.scale(self.s), rad * self.s, c);
    }
    pub fn stroke(&mut self, r: Rect, rad: i32, t: i32, c: Color) {
        self.c.stroke_rrect(r.scale(self.s), rad * self.s, t * self.s, c);
    }
    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, c: Color) {
        self.c.fill_circle(cx * self.s, cy * self.s, r * self.s, c);
    }
    pub fn shadow(&mut self, r: Rect, rad: i32, blur: i32, dy: i32, a: u8) {
        self.c.shadow(r.scale(self.s), rad * self.s, blur * self.s, dy * self.s, a);
    }
    pub fn icon(&mut self, i: Icon, x: i32, y: i32, size: i32, c: Color) {
        icons::draw(self.c, i, x * self.s, y * self.s, size * self.s, c);
    }
    /// Icon centred in `r`.
    pub fn icon_in(&mut self, i: Icon, r: Rect, size: i32, c: Color) {
        self.icon(i, r.x + (r.w - size) / 2, r.y + (r.h - size) / 2, size, c);
    }

    pub fn text(&mut self, x: i32, y: i32, f: Face, size: i32, s: &str, c: Color) -> i32 {
        font::draw(self.c, x * self.s, y * self.s, f, size * self.s, s, c) / self.s
    }
    pub fn tw(&self, f: Face, size: i32, s: &str) -> i32 {
        (font::measure(f, size * self.s, s) + self.s - 1) / self.s
    }
    /// Draw text vertically centred in `r`; align: 0 left, 1 centre, 2 right.
    pub fn text_in(&mut self, r: Rect, f: Face, size: i32, s: &str, c: Color, align: u8) -> i32 {
        let w = self.tw(f, size, s);
        let x = match align {
            0 => r.x,
            1 => r.x + (r.w - w) / 2,
            _ => r.r() - w,
        };
        let base = r.y + (r.h + size * 7 / 10) / 2;
        self.text(x, base, f, size, s, c)
    }
    /// Letter-spaced uppercase label.
    pub fn label(&mut self, x: i32, y: i32, size: i32, s: &str, c: Color) -> i32 {
        let mut pen = x;
        let mut buf = [0u8; 4];
        for ch in s.chars() {
            pen += self.text(pen, y, Face::Semibold, size, ch.encode_utf8(&mut buf), c) + 1;
        }
        pen - x
    }

    /// Truncate `s` with an ellipsis so it fits in `w`.
    pub fn fit(&self, f: Face, size: i32, s: &str, w: i32) -> String {
        if self.tw(f, size, s) <= w {
            return String::from(s);
        }
        let mut out = String::new();
        for ch in s.chars() {
            out.push(ch);
            let mut t = out.clone();
            t.push('…');
            if self.tw(f, size, &t) > w {
                out.pop();
                break;
            }
        }
        out.push('…');
        out
    }

    /// Greedy word wrap of `s` into lines no wider than `w`.
    pub fn wrap(&self, f: Face, size: i32, s: &str, w: i32) -> Vec<String> {
        let mut lines = Vec::new();
        let mut cur = String::new();
        for word in s.split(' ') {
            let cand = if cur.is_empty() { String::from(word) } else { alloc::format!("{} {}", cur, word) };
            if self.tw(f, size, &cand) <= w || cur.is_empty() {
                cur = cand;
            } else {
                lines.push(core::mem::replace(&mut cur, String::from(word)));
            }
        }
        lines.push(cur);
        lines
    }

    /// Draw a raw RGB image (w x h, 3 bytes per pixel) scaled into `r`,
    /// with bilinear filtering when enlarging.
    pub fn image_rgb(&mut self, r: Rect, w: i32, h: i32, rgb: &[u8], radius: i32) {
        if rgb.len() < (w * h * 3) as usize || w < 2 || h < 2 {
            return;
        }
        let pr = r.scale(self.s);
        let (dw, dh) = (pr.w.max(w), pr.h.max(h));
        let mut c = Canvas::new(dw, dh);
        let px = |x: i32, y: i32, k: usize| rgb[((y * w + x) * 3) as usize + k] as i32;
        for y in 0..dh {
            // source position in 1/256 pixels, sampling pixel centres
            let sy = ((y * 2 + 1) * h * 128 / dh - 128).clamp(0, (h - 1) * 256);
            let (y0, fy) = (sy >> 8, sy & 255);
            let y1 = (y0 + 1).min(h - 1);
            for x in 0..dw {
                let sx = ((x * 2 + 1) * w * 128 / dw - 128).clamp(0, (w - 1) * 256);
                let (x0, fx) = (sx >> 8, sx & 255);
                let x1 = (x0 + 1).min(w - 1);
                let mut out = 0u32;
                for k in 0..3 {
                    let top = px(x0, y0, k) * (256 - fx) + px(x1, y0, k) * fx;
                    let bot = px(x0, y1, k) * (256 - fx) + px(x1, y1, k) * fx;
                    let v = (top * (256 - fy) + bot * fy) >> 16;
                    out = out << 8 | v.clamp(0, 255) as u32;
                }
                c.px[(y * dw + x) as usize] = out;
            }
        }
        self.c.blit_scaled(&c, pr, radius * self.s);
    }

    /// A profile picture (a square canvas) shown as a circle filling `r`.
    pub fn avatar(&mut self, r: Rect, pic: &Canvas) {
        self.c.blit_scaled(pic, r.scale(self.s), r.w * self.s / 2);
    }

    // ---- common widgets -------------------------------------------------

    /// Pill/rounded button; returns true if hovered.
    pub fn button(&mut self, r: Rect, label: &str, a: Action, primary: bool) -> bool {
        let hot = self.hot(a);
        let t = self.t;
        let (bg, fg) = if primary { (t.accent, t.on_accent) } else { (t.chip, t.text) };
        self.rrect(r, 10.min(r.h / 2), if hot { bg.mix(t.text, 30) } else { bg });
        self.text_in(r, Face::Semibold, 13, label, fg, 1);
        self.zone(r, a);
        hot
    }

    pub fn icon_button(&mut self, r: Rect, i: Icon, a: Action, size: i32) {
        let t = self.t;
        if self.hot(a) {
            self.rrect(r, 8, t.hover);
        }
        self.icon_in(i, r, size, t.text);
        self.zone(r, a);
    }

    /// Single-line text field. `focused` draws a caret.
    pub fn field(&mut self, r: Rect, value: &str, placeholder: &str, focused: bool, a: Action) {
        self.field_at(r, value, value, placeholder, focused, a);
    }

    /// A text box showing a line editor, with its caret where it is.
    pub fn line(&mut self, r: Rect, e: &crate::lineedit::LineEdit, placeholder: &str, focused: bool, a: Action) {
        self.field_at(r, &e.text, e.before_caret(), placeholder, focused, a);
    }

    /// A text box; the caret goes after `before` (the part of `value` before
    /// it), and long text scrolls to keep the caret in view.
    pub fn field_at(&mut self, r: Rect, value: &str, before: &str, placeholder: &str, focused: bool, a: Action) {
        let t = self.t;
        self.rrect(r, 10, t.chip);
        if focused {
            self.stroke(r, 10, 1, t.accent);
        }
        let inner = Rect::new(r.x + 12, r.y, r.w - 24, r.h);
        let old = self.clip_in(inner);
        let blink = focused && (self.ticks / 50) % 2 == 0;
        if value.is_empty() {
            self.text_in(inner, Face::Regular, 13, placeholder, t.text3, 0);
            if blink {
                self.rect(Rect::new(inner.x, r.y + 8, 1, r.h - 16), t.text);
            }
        } else {
            let cx = self.tw(Face::Regular, 13, before);
            let shift = (cx - (inner.w - 4)).max(0);
            let w = self.tw(Face::Regular, 13, value);
            self.text_in(Rect::new(inner.x - shift, inner.y, w + 4, inner.h), Face::Regular, 13, value, t.text, 0);
            if blink {
                self.rect(Rect::new(inner.x - shift + cx, r.y + 8, 1, r.h - 16), t.text);
            }
        }
        self.set_clip(old);
        self.zone(r, a);
    }

    /// Toggle switch.
    pub fn switch(&mut self, x: i32, y: i32, on: bool, a: Action) {
        let t = self.t;
        let r = Rect::new(x, y, 38, 22);
        self.rrect(r, 11, if on { t.accent } else { t.chip.mix(t.text, 40) });
        let kx = if on { x + 19 } else { x + 3 };
        self.circle(kx + 8, y + 11, 8, Color::rgb(0xFFFFFF));
        self.zone(r, a);
    }
}
