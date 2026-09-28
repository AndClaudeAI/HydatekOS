//! State shared by every app and shell: settings, clock, files, calendar and
//! the Phone Link session.

use crate::apps::AppKind;
use crate::efi::Time;
use crate::fs::Vfs;

/// Extra certificate authorities to trust (PEM or DER files).
pub const CERTS_DIR: &str = "/system/certs";
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
    Lock,
    /// the phone answered a fingerprint unlock request: (request id, approved)
    PhoneUnlock(String, bool),
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

/// Network status shown in Settings and Phone Link.
#[derive(Default, Clone)]
pub struct NetStatus {
    pub present: bool,
    pub name: String,
    pub ip: Option<[u8; 4]>,
    pub gw: [u8; 4],
    pub dns: [u8; 4],
    pub link_up: bool,
    pub host: String,
    pub rx: u64,
    pub tx: u64,
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
    pub net: NetStatus,
    pub rng_source: &'static str,
    pub lock_on_boot: bool,
    /// minutes of inactivity before locking; 0 = never
    pub lock_idle: u32,
    /// (salt, stretched hash) of the lock-screen PIN and password
    lock_pin: Option<([u8; 16], [u8; 32])>,
    lock_pw: Option<([u8; 16], [u8; 32])>,
    /// the paired phone's fingerprint sensor may unlock (needs a PIN or password)
    pub lock_finger: bool,
    pub reqs: Vec<Req>,
    /// text copied with Cut/Copy (shared by apps)
    pub clipboard: String,
    /// web requests from apps (carried out by the main loop's fetcher)
    pub web: crate::web::WebQueue,
    /// Hyda Search: the index and its crawler
    pub search: crate::web::search::Search,
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
            net: NetStatus::default(),
            rng_source: "",
            lock_on_boot: true,
            lock_idle: 10,
            lock_pin: None,
            lock_pw: None,
            lock_finger: false,
            reqs: Vec::new(),
            clipboard: String::new(),
            web: crate::web::WebQueue::default(),
            search: crate::web::search::Search::default(),
            screen: (0, 0, 1),
            firmware: String::new(),
            mem_total: 0,
            ticks: 0,
        };
        s.load_settings();
        s.load_link();
        s.load_lock();
        s.load_search();
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
                "demo" if b => self.link.demo(),
                "lockboot" => self.lock_on_boot = b,
                "lockidle" => self.lock_idle = v.parse().unwrap_or(10),
                _ => {}
            }
        }
    }

    pub fn save_settings(&mut self) {
        let s = format!(
            "dark={}\naccent={}\nwifi={}\nbluetooth={}\nfocus={}\nmobile={}\npointer={}\ndemo={}\nlockboot={}\nlockidle={}\n",
            self.dark as u8,
            self.accent,
            self.wifi as u8,
            self.bt as u8,
            self.focus as u8,
            self.mobile_shell as u8,
            self.pointer_speed,
            self.link.is_demo() as u8,
            self.lock_on_boot as u8,
            self.lock_idle
        );
        self.fs.write("/system/settings.txt", s.as_bytes());
    }

    // ---- Hyda Search ---------------------------------------------------------------

    fn load_search(&mut self) {
        if let Some(d) = self.fs.read("/system/search.hydx") {
            self.search.index = crate::web::search::Index::load(&d);
        }
        self.index_files();
    }

    /// Put the text of your documents in the search index.
    pub fn index_files(&mut self) {
        let mut stack = alloc::vec![String::from("/home")];
        let mut n = 0;
        while let Some(dir) = stack.pop() {
            for (name, is_dir, size) in self.fs.list(&dir) {
                let path = crate::fs::join(&dir, &name);
                if is_dir {
                    stack.push(path);
                    continue;
                }
                if size > 2 << 20 || n > 500 {
                    continue;
                }
                let lower = name.to_ascii_lowercase();
                let Some(data) = self.fs.read(&path) else { continue };
                let text = if lower.ends_with(".txt") || lower.ends_with(".md") || lower.ends_with(".doc") || lower.ends_with(".csv") {
                    String::from_utf8_lossy(&data).into_owned()
                } else if lower.ends_with(".hyds") {
                    match crate::doc::Doc::from_hyds(&data) {
                        Ok(d) => d.to_text(),
                        Err(_) => continue,
                    }
                } else if lower.ends_with(".hydg") {
                    match crate::gridio::from_hydg(&data) {
                        Ok(sh) => sh.cells.values().map(|c| c.input.clone()).collect::<Vec<_>>().join(" "),
                        Err(_) => continue,
                    }
                } else {
                    continue;
                };
                let title = name.rsplit_once('.').map(|x| x.0).unwrap_or(&name).to_string();
                self.search.index.add(&alloc::format!("file://{}", path), &title, &alloc::format!("{} {}", title, text));
                n += 1;
            }
        }
    }

    /// Run the crawler and save the index now and then (every tick).
    pub fn web_tick(&mut self) {
        let now = self.ticks;
        self.search.tick(&mut self.web, now);
        if let Some(data) = self.search.to_save(now) {
            self.fs.write("/system/search.hydx", &data);
        }
    }

    // ---- lock-screen sign-in: PIN, password, phone fingerprint ----------------
    //
    // PIN and password are stored as salt + a stretched SHA-256 hash in
    // /system/lock.txt. They keep people out of a running session; anyone with
    // the disk can still read files, since they aren't encrypted.

    fn secret_hash(salt: &[u8; 16], secret: &str) -> [u8; 32] {
        let mut h = [0u8; 32];
        for _ in 0..20_000 {
            let mut s = crate::crypto::Sha256::new();
            s.update(salt);
            s.update(secret.as_bytes());
            s.update(&h);
            h = s.finish();
        }
        h
    }

    fn new_secret(secret: &str) -> ([u8; 16], [u8; 32]) {
        let mut salt = [0u8; 16];
        crate::rng::fill(&mut salt);
        (salt, Self::secret_hash(&salt, secret))
    }

    fn matches(cred: &Option<([u8; 16], [u8; 32])>, secret: &str) -> bool {
        let Some((salt, hash)) = cred else { return false };
        let h = Self::secret_hash(salt, secret);
        let mut diff = 0u8;
        for i in 0..32 {
            diff |= h[i] ^ hash[i];
        }
        diff == 0
    }

    fn load_lock(&mut self) {
        let Some(data) = self.fs.read("/system/lock.txt") else { return };
        let text = String::from_utf8_lossy(&data).to_string();
        let mut v: [Option<Vec<u8>>; 4] = Default::default();
        for line in text.lines() {
            let Some((k, val)) = line.split_once('=') else { continue };
            // "salt"/"hash" are the PIN (the original format)
            let (i, len) = match k {
                "salt" => (0, 16),
                "hash" => (1, 32),
                "pw_salt" => (2, 16),
                "pw_hash" => (3, 32),
                "finger" => {
                    self.lock_finger = val == "1";
                    continue;
                }
                _ => continue,
            };
            v[i] = crate::crypto::base64_decode(val).filter(|b| b.len() == len);
        }
        let pair = |a: &Option<Vec<u8>>, b: &Option<Vec<u8>>| match (a, b) {
            (Some(s), Some(h)) => Some((s.as_slice().try_into().unwrap(), h.as_slice().try_into().unwrap())),
            _ => None,
        };
        self.lock_pin = pair(&v[0], &v[1]);
        self.lock_pw = pair(&v[2], &v[3]);
        if !self.secured() {
            self.lock_finger = false;
        }
    }

    fn save_lock(&mut self) {
        if !self.secured() {
            self.lock_finger = false;
            self.fs.remove("/system/lock.txt");
            return;
        }
        let b = crate::crypto::base64;
        let mut s = String::new();
        if let Some((salt, hash)) = &self.lock_pin {
            s += &format!("salt={}\nhash={}\n", b(salt), b(hash));
        }
        if let Some((salt, hash)) = &self.lock_pw {
            s += &format!("pw_salt={}\npw_hash={}\n", b(salt), b(hash));
        }
        s += &format!("finger={}\n", self.lock_finger as u8);
        self.fs.write("/system/lock.txt", s.as_bytes());
    }

    pub fn has_pin(&self) -> bool {
        self.lock_pin.is_some()
    }

    pub fn has_password(&self) -> bool {
        self.lock_pw.is_some()
    }

    /// A PIN or password is required to unlock.
    pub fn secured(&self) -> bool {
        self.lock_pin.is_some() || self.lock_pw.is_some()
    }

    pub fn check_pin(&self, pin: &str) -> bool {
        Self::matches(&self.lock_pin, pin)
    }

    pub fn check_password(&self, pw: &str) -> bool {
        Self::matches(&self.lock_pw, pw)
    }

    pub fn pin_ok(pin: &str) -> bool {
        (4..=8).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
    }

    pub fn password_ok(pw: &str) -> bool {
        let n = pw.chars().count();
        (6..=64).contains(&n) && !pw.chars().any(|c| c.is_control())
    }

    /// Set (Some) or remove (None) the PIN. Digits only, 4-8 long.
    pub fn set_pin(&mut self, pin: Option<&str>) -> bool {
        match pin {
            Some(p) if Self::pin_ok(p) => self.lock_pin = Some(Self::new_secret(p)),
            Some(_) => return false,
            None => self.lock_pin = None,
        }
        self.save_lock();
        true
    }

    /// Set (Some) or remove (None) the password: 6-64 characters.
    pub fn set_password(&mut self, pw: Option<&str>) -> bool {
        match pw {
            Some(p) if Self::password_ok(p) => self.lock_pw = Some(Self::new_secret(p)),
            Some(_) => return false,
            None => self.lock_pw = None,
        }
        self.save_lock();
        true
    }

    /// Allow unlocking with the paired phone's fingerprint sensor. Needs a PIN
    /// or password to fall back on.
    pub fn set_finger(&mut self, on: bool) -> bool {
        if on && !self.secured() {
            return false;
        }
        self.lock_finger = on;
        self.save_lock();
        true
    }

    /// Fingerprint unlock can be offered right now: allowed, and a real paired
    /// phone with a fingerprint sensor is connected.
    pub fn finger_ready(&self) -> bool {
        self.lock_finger && !self.link.is_demo() && self.link.online && self.link.caps.iter().any(|c| c == "bio")
    }

    /// Pairing secret and the last paired phone (`/system/link.txt`).
    fn load_link(&mut self) {
        let mut have_key = false;
        if let Some(data) = self.fs.read("/system/link.txt") {
            let text = String::from_utf8_lossy(&data).to_string();
            let mut phone = false;
            for line in text.lines() {
                let Some((k, v)) = line.split_once('=') else { continue };
                match k {
                    "key" => {
                        if let Some(b) = crate::crypto::base64_decode(v).filter(|b| b.len() == 32) {
                            self.link.key.copy_from_slice(&b);
                            have_key = true;
                        }
                    }
                    "id" => self.link.pair_id = v.to_string(),
                    "phone" => phone = v == "1",
                    "device" => self.link.device = v.to_string(),
                    "kind" => self.link.kind = v.to_string(),
                    "caps" => self.link.caps = v.split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect(),
                    _ => {}
                }
            }
            if phone && !self.link.is_demo() {
                self.link.source = crate::link::Source::Phone;
                self.link.paired = true;
            }
        }
        if !have_key || self.link.pair_id.is_empty() {
            self.link.new_key();
            self.save_link();
        }
    }

    pub fn save_link(&mut self) {
        let l = &self.link;
        let s = format!(
            "key={}\nid={}\nphone={}\ndevice={}\nkind={}\ncaps={}\n",
            crate::crypto::base64(&l.key),
            l.pair_id,
            (l.source == crate::link::Source::Phone) as u8,
            l.device.replace('\n', " "),
            l.kind,
            l.caps.join(",")
        );
        self.fs.write("/system/link.txt", s.as_bytes());
    }

    /// Forget the paired phone and issue a new pairing code.
    pub fn unpair(&mut self) {
        self.link.forget();
        self.link.new_key();
        self.save_link();
        self.save_settings();
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

    /// Certificate authorities for https: the built-in list plus any
    /// certificates the user put in /system/certs.
    pub fn trust_store(&self) -> crate::tls::x509::Roots {
        let mut roots = crate::tls::x509::Roots::builtin();
        let builtin = roots.anchors.len();
        for (name, dir, _) in self.fs.list(CERTS_DIR) {
            if !dir {
                if let Some(data) = self.fs.read(&crate::fs::join(CERTS_DIR, &name)) {
                    roots.add_file(&data);
                }
            }
        }
        crate::log!("tls: {} built-in authorities, {} added", builtin, roots.anchors.len() - builtin);
        roots
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
