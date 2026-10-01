//! The desktop's cards. On the left: the clock with what's up next, quick
//! settings with the weather, and a line to start the day with. On the
//! right, when the screen is wide enough: the music player, the month and
//! the Focus card.

use super::{Shell, BAR_H, RAIL_W};
use crate::font::Face;
use crate::gfx::{Canvas, Color, Rect};
use crate::icons::Icon;
use crate::ui::{Action, Ui};
use crate::web::weather::{Sky, Status};
use alloc::format;
use alloc::string::String;

// what the cards' clicks do (Action::Quick)
pub const Q_WIFI: u8 = 0;
/// the network's settings
pub const Q_NET: u8 = 5;
pub const Q_DND: u8 = 1;
pub const Q_BT: u8 = 3;
pub const Q_UPNEXT: u8 = 4;
pub const Q_PLAY: u8 = 10;
pub const Q_NEXT: u8 = 11;
pub const Q_PREV: u8 = 12;
pub const Q_SHUFFLE: u8 = 13;
pub const Q_REPEAT: u8 = 14;
pub const Q_MUSIC: u8 = 15;
pub const Q_CAL_BACK: u8 = 16;
pub const Q_CAL_ON: u8 = 17;
pub const Q_FOCUS: u8 = 18;
pub const Q_WEATHER: u8 = 19;
pub const Q_HOME: u8 = 20;
pub const Q_SEARCH: u8 = 21;
pub const Q_SEARCH_WEB: u8 = 22;
pub const Q_CAL_TODAY: u8 = 23;
/// a day on the calendar card: Q_DAY + day of the month
pub const Q_DAY: u8 = 32;

/// How long a focus session lasts (minutes).
pub const FOCUS_MINS: u64 = 25;

pub const LEFT_W: i32 = 256;
pub const RIGHT_W: i32 = 264;

/// A line to start the day with: one a day, in turn.
const LINES: [(&str, &str); 7] = [
    ("Better tools.", "A calmer mind."),
    ("Greater things", "are ahead."),
    ("Discipline", "builds freedom."),
    ("Gratitude changes", "everything."),
    ("The best is", "yet to come."),
    ("Bigger dreams,", "bolder steps."),
    ("Small steps,", "every day."),
];

fn card(ui: &mut Ui, r: Rect) {
    let t = ui.t;
    ui.shadow(r, 22, 14, 4, 22);
    ui.rrect(r, 22, t.surface.with_alpha(242));
    if t.dark {
        ui.stroke(r, 22, 1, t.line);
    }
}

/// Layered hills, like the Dune scene, across `c` from `top` (0-1000 of its
/// height) down: the cards' small pictures.
fn hills(c: &mut Canvas, layers: &[(Color, i32, i32, i32, i32)]) {
    let (w, h) = (c.w, c.h);
    for &(col, base, amp, period, phase) in layers {
        for x in 0..w {
            let a = (x * 1024 / (w * period / 1000).max(1) + phase) % 1024;
            let y0 = h * base / 1000 + (h * amp / 1000) * crate::gfx::sin_q14(a) / 16384;
            let y0 = y0.clamp(0, h);
            for y in y0..h {
                let i = (y * w + x) as usize;
                c.px[i] = col.0 | 0xFF00_0000;
            }
        }
    }
}

impl Shell {
    /// Where the desktop's cards are (left, right), if the screen has room.
    pub(super) fn widget_columns(&self) -> (Option<Rect>, Option<Rect>) {
        if !self.sys.look.widgets || self.w < 900 || self.h < 560 {
            return (None, None);
        }
        let left = Rect::new(RAIL_W + 22, BAR_H + 20, LEFT_W, self.h - BAR_H - 40);
        let right = (self.w >= 1180).then(|| Rect::new(self.w - 24 - RIGHT_W, BAR_H + 20, RIGHT_W, self.h - BAR_H - 40));
        (Some(left), right)
    }

    pub(super) fn draw_widgets(&self, ui: &mut Ui) {
        let (left, right) = self.widget_columns();
        if let Some(col) = left {
            let mut y = col.y;
            y = self.clock_card(ui, Rect::new(col.x, y, col.w, 194)) + 12;
            y = self.quick_card(ui, Rect::new(col.x, y, col.w, 150)) + 12;
            if y + 88 <= col.b() {
                self.line_card(ui, Rect::new(col.x, y, col.w, 88));
            }
        }
        if let Some(col) = right {
            let mut y = col.y;
            y = self.player_card(ui, Rect::new(col.x, y, col.w, 196)) + 12;
            if y + 226 <= col.b() {
                y = self.calendar_card(ui, Rect::new(col.x, y, col.w, 226)) + 12;
            }
            if y + 76 <= col.b() {
                self.focus_card(ui, Rect::new(col.x, y, col.w, 76));
            }
        }
    }

    fn clock_card(&self, ui: &mut Ui, r: Rect) -> i32 {
        let (t, sys) = (ui.t, &self.sys);
        card(ui, r);
        let w = r.w - 40;
        let top = ui.fit(Face::Regular, 12, &sys.date_long(), w);
        ui.text(r.x + 20, r.y + 30, Face::Regular, 12, &top, t.text2);
        ui.text(r.x + 17, r.y + 92, Face::Display, 60, &sys.clock(), t.text);
        ui.rrect(Rect::new(r.x + 20, r.y + 108, 48, 2), 1, t.accent);
        ui.rect(Rect::new(r.x + 74, r.y + 108, r.w - 94, 1), t.line);
        ui.label(r.x + 20, r.y + 134, 10, "UP NEXT", t.accent);
        let (title, sub) = match sys.next_event() {
            Some(e) => (e.title.clone(), format!("{}{}{}", sys.event_when(e), if e.place.is_empty() { "" } else { " · " }, e.place)),
            None => (String::from("Nothing scheduled"), String::from("Add events in Calendar")),
        };
        ui.icon(Icon::Calendar, r.x + 20, r.y + 145, 14, t.accent);
        let title = ui.fit(Face::Semibold, 14, &title, w - 22);
        ui.text(r.x + 42, r.y + 157, Face::Semibold, 14, &title, t.text);
        let sub = ui.fit(Face::Regular, 12, &sub, w - 22);
        ui.text(r.x + 42, r.y + 177, Face::Regular, 12, &sub, t.text2);
        let a = Action::Quick(Q_UPNEXT);
        if ui.hot(a) {
            ui.rrect(Rect::new(r.x + 8, r.y + 118, r.w - 16, 68), 14, t.hover);
        }
        ui.zone(Rect::new(r.x, r.y + 116, r.w, 78), a);
        r.b()
    }

    fn quick_card(&self, ui: &mut Ui, r: Rect) -> i32 {
        let (t, sys) = (ui.t, &self.sys);
        card(ui, r);
        let split = r.x + 164;
        // a cable connection says so (HydatekOS has no Wi-Fi chip drivers yet)
        let wired = sys.net.ip.is_some();
        let rows = [
            if wired { (Icon::Link, "Ethernet", true, "Connected", Q_NET) } else { (Icon::Wifi, "Wi-Fi", sys.wifi, if sys.wifi { "On" } else { "Off" }, Q_WIFI) },
            (Icon::Bluetooth, "Bluetooth", sys.bt, if sys.bt { "On" } else { "Off" }, Q_BT),
            (Icon::Moon, "Do Not Disturb", sys.focus, if sys.focus_until.is_some() { "Focusing" } else if sys.focus { "On" } else { "Off" }, Q_DND),
        ];
        for (i, (ic, name, on, state, q)) in rows.iter().enumerate() {
            let row = Rect::new(r.x + 8, r.y + 10 + i as i32 * 44, split - r.x - 14, 42);
            let a = Action::Quick(*q);
            if ui.hot(a) {
                ui.rrect(row, 12, t.hover);
            }
            let (cx, cy) = (row.x + 22, row.y + 21);
            ui.circle(cx, cy, 16, if *on { t.accent } else { t.chip });
            ui.icon(*ic, cx - 8, cy - 8, 16, if *on { t.on_accent } else { t.text2 });
            let name = ui.fit(Face::Semibold, 13, name, row.w - 50);
            ui.text(row.x + 46, row.y + 18, Face::Semibold, 13, &name, t.text);
            ui.text(row.x + 46, row.y + 34, Face::Regular, 11, state, t.text2);
            ui.zone(row, a);
        }
        ui.rect(Rect::new(split, r.y + 18, 1, r.h - 36), t.line);
        // the weather
        let wr = Rect::new(split + 1, r.y, r.r() - split - 1, r.h);
        let a = Action::Quick(Q_WEATHER);
        if ui.hot(a) {
            ui.rrect(wr.inset(6), 14, t.hover);
        }
        ui.zone(wr, a);
        let wx = &sys.weather;
        let mid = |ui: &mut Ui, y: i32, f: Face, size: i32, s: &str, c: Color| {
            let s = ui.fit(f, size, s, wr.w - 12);
            ui.text_in(Rect::new(wr.x, y - size, wr.w, size + 4), f, size, &s, c, 1);
        };
        match (&wx.status, wx.now) {
            (Status::Off, _) => {
                ui.icon_in(Icon::PartCloud, Rect::new(wr.x, r.y + 22, wr.w, 34), 34, t.text3);
                mid(ui, r.y + 86, Face::Semibold, 14, "Weather", t.text);
                mid(ui, r.y + 106, Face::Regular, 12, "Add a town", t.accent);
            }
            (_, Some(now)) => {
                let (ic, col) = match crate::web::weather::sky(now.code) {
                    Sky::Clear if now.day => (Icon::Sun, t.sun.mix(t.accent, 90)),
                    Sky::Clear => (Icon::Moon, t.text2),
                    Sky::PartCloud => (Icon::PartCloud, t.text2),
                    Sky::Cloud => (Icon::Cloud, t.text2),
                    Sky::Fog => (Icon::Fog, t.text2),
                    Sky::Rain => (Icon::Rain, t.text2),
                    Sky::Snow => (Icon::Snow, t.text2),
                    Sky::Storm => (Icon::Storm, t.text2),
                };
                ui.icon_in(ic, Rect::new(wr.x, r.y + 18, wr.w, 34), 34, col);
                mid(ui, r.y + 86, Face::Semibold, 26, &format!("{}°C", now.temp), t.text);
                mid(ui, r.y + 106, Face::Regular, 12, crate::web::weather::describe(now.code), t.text2);
                let town = wx.place.as_ref().map_or(wx.town.as_str(), |p| p.name.as_str());
                let town = ui.fit(Face::Regular, 12, town, wr.w - 36);
                let tw = ui.tw(Face::Regular, 12, &town) + 16;
                let x = wr.x + (wr.w - tw) / 2;
                ui.icon(Icon::Pin, x - 2, r.y + 119, 13, t.text2);
                ui.text(x + 15, r.y + 130, Face::Regular, 12, &town, t.text);
            }
            (s, None) => {
                let msg = match s {
                    Status::Unknown => "Town not found",
                    Status::Failed(_) => "Offline for now",
                    _ if sys.net.ip.is_none() => "Waiting for a network",
                    _ => "Looking outside…",
                };
                ui.icon_in(Icon::PartCloud, Rect::new(wr.x, r.y + 22, wr.w, 34), 34, t.text3);
                mid(ui, r.y + 86, Face::Semibold, 14, &wx.town, t.text);
                mid(ui, r.y + 106, Face::Regular, 12, msg, t.text2);
            }
        }
        r.b()
    }

    fn line_card(&self, ui: &mut Ui, r: Rect) {
        let t = ui.t;
        let s = ui.s;
        // the card is drawn whole, picture included, then put down with its
        // rounded corners
        let mut c = Canvas::new(r.w * s, r.h * s);
        let bg = t.surface;
        c.fill_rect(c.bounds(), bg);
        let sand = t.sun.mix(bg, 120);
        hills(&mut c, &[(sand, 520, 260, 1500, 700), (t.dune2.mix(bg, 50), 700, 220, 1250, 120), (t.accent.mix(t.dune2, 120), 860, 140, 1000, 800)]);
        // keep the text side plain: the hills fade in from the middle
        let w = c.w;
        let (x0, x1) = (w * 9 / 20, w * 3 / 4);
        for y in 0..c.h {
            for x in 0..x1 {
                let k = if x < x0 { 0 } else { ((x - x0) * 256 / (x1 - x0)) as u32 };
                let i = (y * w + x) as usize;
                c.px[i] = crate::gfx::lerp(bg.0 & 0xFF_FFFF, c.px[i] & 0xFF_FFFF, k * k / 256) | 0xFF00_0000;
            }
        }
        ui.shadow(r, 22, 14, 4, 22);
        ui.c.blit_scaled_alpha(&c, r.scale(s), 22 * s, 242);
        if t.dark {
            ui.stroke(r, 22, 1, t.line);
        }
        let day = self.sys.now.day as usize + self.sys.now.month as usize * 31;
        let (a, b) = LINES[day % LINES.len()];
        ui.icon(Icon::Spark, r.x + 20, r.y + 16, 15, t.accent);
        ui.text(r.x + 20, r.y + 52, Face::Medium, 14, a, t.text);
        ui.text(r.x + 20, r.y + 71, Face::Medium, 14, b, t.text);
    }

    fn player_card(&self, ui: &mut Ui, r: Rect) -> i32 {
        let t = ui.t;
        let pl = &self.sys.player;
        let bg = t.dock;
        let ink = t.dock_icon;
        let muted = ink.mix(bg, 110);
        ui.shadow(r, 22, 16, 6, 60);
        ui.rrect(r, 22, bg.with_alpha(246));
        // the cover: a small Dune scene in the theme's colours
        let art = Rect::new(r.x + 16, r.y + 16, 76, 76);
        let s = ui.s;
        let mut c = Canvas::new(art.w * s, art.h * s);
        let cb = c.bounds();
        super::wallpaper::scene(&mut c, cb, crate::personal::Scene::Dune, &t, Some(17 * 60 + 30 + pl.cur as u32 * 25), false);
        ui.c.blit_scaled_alpha(&c, art.scale(s), 12 * s, 255);
        let (name, artist, len) = pl.track();
        let tx = art.r() + 16;
        let tw = r.r() - tx - 36;
        let name = ui.fit(Face::Semibold, 16, name, tw);
        ui.text(tx, r.y + 46, Face::Semibold, 16, &name, ink);
        let artist = ui.fit(Face::Regular, 12, artist, tw);
        ui.text(tx, r.y + 66, Face::Regular, 12, &artist, muted);
        let more = Action::Quick(Q_MUSIC);
        let mr = Rect::new(r.r() - 36, r.y + 12, 24, 24);
        if ui.hot(more) {
            ui.circle(mr.x + 12, mr.y + 12, 12, Color::rgba(0xFFFFFF, 30));
        }
        ui.icon(Icon::More, mr.x + 4, mr.y + 4, 16, ink);
        ui.zone(mr, more);
        // where it's got to
        let bar = Rect::new(r.x + 16, r.y + 110, r.w - 32, 4);
        ui.rrect(bar, 2, ink.with_alpha(50));
        let done = (bar.w as u32 * pl.secs().min(len) / len.max(1)) as i32;
        ui.rrect(Rect::new(bar.x, bar.y, done.max(4), 4), 2, ink.mix(t.accent, 40));
        let mmss = |v: u32| format!("{}:{:02}", v / 60, v % 60);
        ui.text(bar.x, bar.y + 22, Face::Regular, 11, &mmss(pl.secs()), muted);
        let total = mmss(len);
        let w = ui.tw(Face::Regular, 11, &total);
        ui.text(bar.r() - w, bar.y + 22, Face::Regular, 11, &total, muted);
        // the controls
        let cy = r.y + 164;
        let cx = r.x + r.w / 2;
        let ctl = [(Icon::Shuffle, Q_SHUFFLE, -96, pl.shuffle), (Icon::SkipPrev, Q_PREV, -50, false), (Icon::SkipNext, Q_NEXT, 50, false), (Icon::Repeat, Q_REPEAT, 96, pl.repeat)];
        for (ic, q, dx, on) in ctl {
            let a = Action::Quick(q);
            let b = Rect::new(cx + dx - 16, cy - 16, 32, 32);
            if ui.hot(a) {
                ui.circle(cx + dx, cy, 16, Color::rgba(0xFFFFFF, 26));
            }
            ui.icon(ic, cx + dx - 9, cy - 9, 18, if on { t.accent.mix(ink, 60) } else { ink });
            ui.zone(b, a);
        }
        let a = Action::Quick(Q_PLAY);
        let pc = if ui.hot(a) { ink.mix(t.accent, 50) } else { ink };
        ui.circle(cx, cy, 21, pc);
        ui.icon(if pl.playing { Icon::Pause } else { Icon::Play }, cx - 8 + if pl.playing { 0 } else { 1 }, cy - 8, 16, bg);
        ui.zone(Rect::new(cx - 22, cy - 22, 44, 44), a);
        r.b()
    }

    fn calendar_card(&self, ui: &mut Ui, r: Rect) -> i32 {
        let t = ui.t;
        let sys = &self.sys;
        card(ui, r);
        let now = sys.now;
        let k = now.year as i32 * 12 + now.month as i32 - 1 + self.cal_shift;
        let (y, m) = (k / 12, k % 12 + 1);
        let title = format!("{} {}", crate::sys::MONTHS[(m - 1) as usize], y);
        let ta = Action::Quick(Q_CAL_TODAY);
        if self.cal_shift != 0 && ui.hot(ta) {
            ui.rrect(Rect::new(r.x + 12, r.y + 14, ui.tw(Face::Semibold, 15, &title) + 16, 28), 8, t.hover);
        }
        ui.text(r.x + 20, r.y + 34, Face::Semibold, 15, &title, t.text);
        if self.cal_shift != 0 {
            ui.zone(Rect::new(r.x + 12, r.y + 14, 160, 28), ta);
        }
        for (ic, q, x) in [(Icon::ChevronLeft, Q_CAL_BACK, r.r() - 70), (Icon::ChevronRight, Q_CAL_ON, r.r() - 38)] {
            let a = Action::Quick(q);
            let b = Rect::new(x, r.y + 16, 26, 26);
            if ui.hot(a) {
                ui.circle(b.x + 13, b.y + 13, 13, t.hover);
            }
            ui.icon_in(ic, b, 14, t.text2);
            ui.zone(b, a);
        }
        let cw = (r.w - 24) / 7;
        let x0 = r.x + 12;
        for (i, d) in ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].iter().enumerate() {
            ui.text_in(Rect::new(x0 + i as i32 * cw, r.y + 48, cw, 16), Face::Regular, 11, d, t.text2, 1);
        }
        // Monday first
        let first = (crate::sys::weekday(y, m, 1) + 6) % 7;
        let days = crate::sys::days_in_month(y, m);
        for d in 1..=days {
            let cell = first as i32 + d - 1;
            let (cx, cy) = (x0 + (cell % 7) * cw + cw / 2, r.y + 84 + (cell / 7) * 24);
            let today = y == now.year as i32 && m == now.month as i32 && d == now.day as i32;
            let a = Action::Quick(Q_DAY + d as u8);
            if today {
                ui.circle(cx, cy, 12, t.accent);
            } else if ui.hot(a) {
                ui.circle(cx, cy, 12, t.hover);
            }
            let n = format!("{}", d);
            ui.text_in(Rect::new(cx - 14, cy - 9, 28, 18), if today { Face::Semibold } else { Face::Regular }, 12, &n, if today { t.on_accent } else { t.text }, 1);
            if sys.events.iter().any(|e| e.y as i32 == y && e.m as i32 == m && e.d as i32 == d) {
                ui.circle(cx, cy + 10, 2, if today { t.on_accent } else { t.accent });
            }
            ui.zone(Rect::new(cx - cw / 2, cy - 12, cw, 24), a);
        }
        r.b()
    }

    fn focus_card(&self, ui: &mut Ui, r: Rect) {
        let t = ui.t;
        let sys = &self.sys;
        card(ui, r);
        let a = Action::Quick(Q_FOCUS);
        if ui.hot(a) {
            ui.rrect(r.inset(4), 18, t.hover);
        }
        ui.icon(Icon::Star, r.x + 18, r.y + 16, 16, t.accent);
        let title = match sys.focus_left() {
            Some(m) => format!("Focus · {} min left", m),
            None => String::from("Focus"),
        };
        ui.text(r.x + 44, r.y + 29, Face::Semibold, 13, &title, t.text);
        let lines = ui.wrap(Face::Regular, 12, &sys.intention, r.w - 88);
        for (i, l) in lines.iter().take(2).enumerate() {
            ui.text(r.x + 44, r.y + 48 + i as i32 * 16, Face::Regular, 12, l, t.text2);
        }
        let hint = if sys.focus_until.is_some() { Icon::Close } else { Icon::ChevronRight };
        ui.icon(hint, r.r() - 30, r.y + r.h / 2 - 7, 14, t.text3);
        ui.zone(r, a);
    }
}
