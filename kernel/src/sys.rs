//! State shared by every app and shell: settings, clock, files, calendar and
//! the Phone Link session.

use crate::apps::AppKind;
use crate::efi::Time;
use crate::fs::Vfs;
use crate::link::Link;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub enum Req {
    Open(AppKind),
    OpenPath(String),
    Shutdown,
    Reboot,
    Toast(String, String),
    SaveSettings,
}

#[derive(Clone)]
pub struct CalEvent {
    pub y: u16,
    pub m: u8,
    pub d: u8,
    pub hh: u8,
    pub mm: u8,
    pub title: String,
    pub place: String,
}

impl CalEvent {
    pub fn key(&self) -> u64 {
        ((self.y as u64) << 32) | ((self.m as u64) << 24) | ((self.d as u64) << 16) | ((self.hh as u64) << 8) | self.mm as u64
    }
}

pub struct Sys {
    pub dark: bool,
    pub accent: usize,
    pub wifi: bool,
    pub bt: bool,
    pub focus: bool,
    pub mobile_shell: bool,
    pub pointer_speed: i32,
    pub fs: Vfs,
    pub now: Time,
    pub events: Vec<CalEvent>,
    pub link: Link,
    pub reqs: Vec<Req>,
    pub screen: (i32, i32, i32),
    pub firmware: String,
    pub mem_total: u64,
    pub ticks: u64,
}

pub const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
pub const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// 0 = Sunday
pub fn weekday(y: i32, m: i32, d: i32) -> usize {
    const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d).rem_euclid(7)) as usize
}

pub fn days_in_month(y: i32, m: i32) -> i32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Sys {
    pub fn new(fs: Vfs, now: Time) -> Sys {
        let mut s = Sys {
            dark: false,
            accent: 0,
            wifi: true,
            bt: true,
            focus: false,
            mobile_shell: false,
            pointer_speed: 3,
            fs,
            now,
            events: Vec::new(),
            link: Link::new(),
            reqs: Vec::new(),
            screen: (0, 0, 1),
            firmware: String::new(),
            mem_total: 0,
            ticks: 0,
        };
        s.load_settings();
        s.load_events();
        s
    }

    pub fn toast(&mut self, title: &str, body: &str) {
        self.reqs.push(Req::Toast(title.to_string(), body.to_string()));
    }

    fn load_settings(&mut self) {
        let Some(data) = self.fs.read("/system/settings.txt") else { return };
        let text = String::from_utf8_lossy(&data).to_string();
        for line in text.lines() {
            let mut kv = line.splitn(2, '=');
            let (k, v) = (kv.next().unwrap_or(""), kv.next().unwrap_or("").trim());
            let b = v == "1";
            match k {
                "dark" => self.dark = b,
                "accent" => self.accent = v.parse().unwrap_or(0),
                "wifi" => self.wifi = b,
                "bluetooth" => self.bt = b,
                "focus" => self.focus = b,
                "mobile" => self.mobile_shell = b,
                "pointer" => self.pointer_speed = v.parse().unwrap_or(3),
                "paired" => self.link.paired = b,
                _ => {}
            }
        }
    }

    pub fn save_settings(&mut self) {
        let s = format!(
            "dark={}\naccent={}\nwifi={}\nbluetooth={}\nfocus={}\nmobile={}\npointer={}\npaired={}\n",
            self.dark as u8, self.accent, self.wifi as u8, self.bt as u8, self.focus as u8, self.mobile_shell as u8, self.pointer_speed, self.link.paired as u8
        );
        self.fs.write("/system/settings.txt", s.as_bytes());
    }

    fn load_events(&mut self) {
        if let Some(data) = self.fs.read("/system/calendar.txt") {
            let text = String::from_utf8_lossy(&data).to_string();
            for line in text.lines() {
                let f: Vec<&str> = line.split('|').collect();
                if f.len() < 3 || f[0].len() < 16 {
                    continue;
                }
                let n = |a: usize, b: usize| f[0].get(a..b).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);
                self.events.push(CalEvent { y: n(0, 4), m: n(5, 7) as u8, d: n(8, 10) as u8, hh: n(11, 13) as u8, mm: n(14, 16) as u8, title: f[1].to_string(), place: f[2].to_string() });
            }
        } else {
            // First boot: a few sample events around today.
            let t = self.now;
            let mut add = |dd: i32, hh: u8, mm: u8, title: &str, place: &str| {
                let (mut y, mut m, mut d) = (t.year as i32, t.month as i32, t.day as i32 + dd);
                if d > days_in_month(y, m) {
                    d -= days_in_month(y, m);
                    m += 1;
                    if m > 12 {
                        m = 1;
                        y += 1;
                    }
                }
                self.events.push(CalEvent { y: y as u16, m: m as u8, d: d as u8, hh, mm, title: title.to_string(), place: place.to_string() });
            };
            add(0, 16, 30, "Design review", "Studio");
            add(1, 9, 0, "Team stand-up", "Online");
            add(2, 13, 0, "Lunch with Ada", "Café Dune");
            add(5, 18, 30, "Phone Link demo", "Lab 2");
            self.save_events();
        }
        self.events.sort_by_key(|e| e.key());
    }

    pub fn save_events(&mut self) {
        let mut s = String::new();
        for e in &self.events {
            s.push_str(&format!("{:04}-{:02}-{:02} {:02}:{:02}|{}|{}\n", e.y, e.m, e.d, e.hh, e.mm, e.title, e.place));
        }
        self.fs.write("/system/calendar.txt", s.as_bytes());
    }

    pub fn add_event(&mut self, e: CalEvent) {
        self.events.push(e);
        self.events.sort_by_key(|e| e.key());
        self.save_events();
    }

    pub fn next_event(&self) -> Option<&CalEvent> {
        let t = self.now;
        let now = CalEvent { y: t.year, m: t.month, d: t.day, hh: t.hour, mm: t.minute, title: String::new(), place: String::new() }.key();
        self.events.iter().find(|e| e.key() >= now)
    }

    pub fn event_when(&self, e: &CalEvent) -> String {
        let t = self.now;
        let day = if e.y == t.year && e.m == t.month && e.d == t.day {
            String::from("Today")
        } else if e.y == t.year && e.m == t.month && e.d as i32 == t.day as i32 + 1 {
            String::from("Tomorrow")
        } else {
            format!("{} {}", e.d, &MONTHS[(e.m as usize).saturating_sub(1) % 12][..3])
        };
        format!("{}, {:02}:{:02}", day, e.hh, e.mm)
    }

    pub fn clock(&self) -> String {
        format!("{:02}:{:02}", self.now.hour, self.now.minute)
    }

    pub fn date_long(&self) -> String {
        let t = self.now;
        format!("{}, {} {}", DAYS[weekday(t.year as i32, t.month as i32, t.day as i32)], t.day, MONTHS[(t.month as usize).max(1) - 1])
    }

    pub fn date_short(&self) -> String {
        let t = self.now;
        format!("{} {} {}", &DAYS[weekday(t.year as i32, t.month as i32, t.day as i32)][..3], t.day, &MONTHS[(t.month as usize).max(1) - 1][..3])
    }
}
