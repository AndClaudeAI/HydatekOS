//! Phone Link state: what the desktop knows about the linked phone.
//!
//! The data is fed either by a real phone over the Hydatek Link Protocol
//! (see `hlp.rs`, `linksrv.rs` and docs/PHONE_LINK.md) or by the built-in demo
//! phone, a simulated HydatekOS Mobile device for trying Phone Link without
//! hardware. The UI reads this model and queues commands in `outbox`.

use crate::hlp::Msg as Wire;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone)]
pub struct Msg {
    pub me: bool,
    pub text: String,
    pub time: String,
}

pub struct Thread {
    pub id: String,
    pub name: String,
    pub number: String,
    pub msgs: Vec<Msg>,
    pub unread: bool,
}

pub struct Notif {
    pub id: String,
    pub app: String,
    pub title: String,
    pub body: String,
    pub time: String,
}

pub struct Call {
    pub name: String,
    pub number: String,
    pub when: String,
    pub missed: bool,
}

pub struct Photo {
    pub id: String,
    pub name: String,
    pub c1: u32,
    pub c2: u32,
    /// 64x64 RGB thumbnail from a real phone
    pub thumb: Option<Vec<u8>>,
}

/// Something shared between the phone and the PC (text or a file).
pub struct Shared {
    pub from_phone: bool,
    pub text: String,
    pub is_file: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    None,
    Demo,
    Phone,
}

/// Side effects the shell must carry out after a phone message.
pub enum Event {
    Toast(String, String),
    SaveFile(String, Vec<u8>),
    /// the phone answered an unlock request: (request id, approved)
    Unlock(String, bool),
}

pub struct Link {
    pub source: Source,
    /// a real phone is connected right now
    pub online: bool,
    pub paired: bool,
    pub device: String,
    pub kind: String,
    pub caps: Vec<String>,
    pub battery: u8,
    pub charging: bool,
    pub threads: Vec<Thread>,
    pub notifs: Vec<Notif>,
    pub photos: Vec<Photo>,
    pub calls: Vec<Call>,
    pub calling: Option<String>,
    pub shared: Vec<Shared>,
    /// pairing secret shared through the QR code, and its public id
    pub key: [u8; 32],
    pub pair_id: String,
    /// commands for the connected phone
    pub outbox: Vec<Wire>,
    pub desktop_name: String,
    /// (thread, due tick) for pending simulated replies
    pending: Vec<(usize, u64)>,
    next_event: u64,
    script: usize,
}

fn m(me: bool, text: &str, time: &str) -> Msg {
    Msg { me, text: text.to_string(), time: time.to_string() }
}

const REPLIES: [&str; 6] = ["Sounds good!", "On my way!", "Can you send me the file?", "Haha, yes. See you there.", "Perfect, thanks!", "Let me check and get back to you."];

const INCOMING: [(&str, &str, &str); 4] = [
    ("Messages", "Ada", "Are we still on for lunch tomorrow?"),
    ("Calendar", "Design review", "Starts in 30 minutes · Studio"),
    ("Mail", "Hydatek Team", "Milestone 1 build is ready to test"),
    ("Messages", "Mum", "Call me when you're free"),
];

fn thread(name: &str, unread: bool, msgs: Vec<Msg>) -> Thread {
    Thread { id: name.to_string(), name: name.to_string(), number: String::new(), msgs, unread }
}

impl Link {
    pub fn new() -> Link {
        Link {
            source: Source::None,
            online: false,
            paired: false,
            device: String::new(),
            kind: String::new(),
            caps: vec![],
            battery: 0,
            charging: false,
            threads: vec![],
            notifs: vec![],
            photos: vec![],
            calls: vec![],
            calling: None,
            shared: vec![],
            key: [0; 32],
            pair_id: String::new(),
            outbox: vec![],
            desktop_name: String::from("HydatekOS"),
            pending: vec![],
            next_event: 0,
            script: 0,
        }
    }

    pub fn has(&self, cap: &str) -> bool {
        self.source == Source::Demo || self.caps.iter().any(|c| c == cap)
    }

    pub fn is_demo(&self) -> bool {
        self.source == Source::Demo
    }

    /// Fresh pairing secret (revokes any phone paired before).
    pub fn new_key(&mut self) {
        crate::rng::fill(&mut self.key);
        let mut id = [0u8; 3];
        crate::rng::fill(&mut id);
        self.pair_id = crate::crypto::hex(&id);
    }

    /// The URL encoded in the pairing QR code.
    pub fn pair_url(&self, ip: &str) -> String {
        alloc::format!("http://{}:7743/#k={}&d={}", ip, crate::crypto::base64url(&self.key), self.pair_id)
    }

    pub fn forget(&mut self) {
        let key = self.key;
        let id = core::mem::take(&mut self.pair_id);
        let name = core::mem::take(&mut self.desktop_name);
        *self = Link::new();
        self.key = key;
        self.pair_id = id;
        self.desktop_name = name;
    }

    /// Switch to the built-in demo phone.
    pub fn demo(&mut self) {
        self.forget();
        self.source = Source::Demo;
        self.paired = true;
        self.device = String::from("Demo phone");
        self.kind = String::from("demo");
        self.battery = 82;
        self.threads = vec![
            thread("Ada", true, vec![m(false, "Hey! Did you see the new dock design?", "09:12"), m(true, "Yes, it looks great in dark mode too", "09:15"), m(false, "Lunch tomorrow?", "10:30")]),
            thread("Mum", false, vec![m(false, "Don't forget Sunday dinner", "Yesterday"), m(true, "I won't!", "Yesterday")]),
            thread("Studio group", false, vec![m(false, "Review moved to 16:30", "08:02"), m(true, "Noted, thanks", "08:05")]),
        ];
        self.notifs = vec![
            Notif { id: "n1".into(), app: "Mail".into(), title: "Invoice INV-0042".into(), body: "Your invoice is ready".into(), time: "08:40".into() },
            Notif { id: "n2".into(), app: "Weather".into(), title: "Sunny, 27°".into(), body: "Clear skies all day".into(), time: "07:00".into() },
        ];
        let p = |n: &str, a: u32, b: u32| Photo { id: n.to_string(), name: n.to_string(), c1: a, c2: b, thumb: None };
        self.photos = vec![p("Dunes at dawn", 0xE4B783, 0xC4895E), p("Night sky", 0x2B2A48, 0x5A4F7C), p("Oasis", 0x7FAF6B, 0x2F6690), p("Sunset", 0xD9793A, 0x7B3F7A), p("Market", 0xDCC8AB, 0xB5581B), p("Studio", 0xF1E6D9, 0x5E5866)];
        let c = |n: &str, w: &str, missed: bool| Call { name: n.to_string(), number: String::new(), when: w.to_string(), missed };
        self.calls = vec![c("Ada", "Today, 08:10", false), c("Mum", "Yesterday, 19:32", true), c("Studio", "Mon, 11:05", false)];
    }

    pub fn unread(&self) -> usize {
        self.threads.iter().filter(|t| t.unread).count()
    }

    // ---- commands from the desktop UI --------------------------------------

    pub fn send(&mut self, thread: usize, text: &str, time: String, now: u64) {
        let Some(t) = self.threads.get_mut(thread) else { return };
        t.msgs.push(Msg { me: true, text: text.to_string(), time });
        match self.source {
            Source::Demo => self.pending.push((thread, now + 300)),
            Source::Phone => {
                let w = Wire::new("sms").with("thread", &t.id).with("number", &t.number).with("text", text);
                self.outbox.push(w);
            }
            Source::None => {}
        }
    }

    pub fn dial(&mut self, i: usize) {
        if let Some(c) = self.calls.get(i) {
            self.calling = Some(if c.name.is_empty() { c.number.clone() } else { c.name.clone() });
            if self.source == Source::Phone {
                let n = if c.number.is_empty() { c.name.clone() } else { c.number.clone() };
                self.outbox.push(Wire::new("dial").with("number", &n));
            }
        }
    }

    pub fn hangup(&mut self) {
        self.calling = None;
        if self.source == Source::Phone {
            self.outbox.push(Wire::new("hangup"));
        }
    }

    pub fn dismiss_all(&mut self) {
        if self.source == Source::Phone {
            for n in &self.notifs {
                self.outbox.push(Wire::new("notif_dismiss").with("id", &n.id));
            }
        }
        self.notifs.clear();
    }

    /// Ask the phone for the full-size photo (arrives as a `file`).
    pub fn request_photo(&mut self, i: usize) -> bool {
        if self.source != Source::Phone {
            return false;
        }
        if let Some(p) = self.photos.get(i) {
            self.outbox.push(Wire::new("get_photo").with("id", &p.id));
            return true;
        }
        false
    }

    pub fn send_clip(&mut self, text: &str) {
        self.shared.insert(0, Shared { from_phone: false, text: text.to_string(), is_file: false });
        if self.source == Source::Phone {
            self.outbox.push(Wire::new("clip").with("text", text));
        }
    }

    pub fn send_file(&mut self, name: &str, data: Vec<u8>) -> bool {
        if self.source != Source::Phone || !self.online {
            return false;
        }
        self.shared.insert(0, Shared { from_phone: false, text: name.to_string(), is_file: true });
        self.outbox.push(Wire::new("file").with("name", name).with("size", &alloc::format!("{}", data.len())).blob(data));
        true
    }

    /// Ask the phone to confirm an unlock with its fingerprint sensor.
    pub fn request_unlock(&mut self, id: &str) -> bool {
        if self.source != Source::Phone || !self.online {
            return false;
        }
        self.outbox.push(Wire::new("unlock_req").with("id", id).with("name", &self.desktop_name.clone()));
        true
    }

    /// The PC unlocked another way or gave up: close the prompt on the phone.
    pub fn cancel_unlock(&mut self, id: &str) {
        if self.source == Source::Phone && self.online {
            self.outbox.push(Wire::new("unlock_cancel").with("id", id));
        }
    }

    // ---- messages from a real phone ----------------------------------------

    /// A phone authenticated: start from a clean slate for its data.
    pub fn phone_connected(&mut self) {
        // every connection starts a fresh sync from the phone
        self.threads.clear();
        self.notifs.clear();
        self.calls.clear();
        self.photos.clear();
        if self.source != Source::Phone {
            let key = self.key;
            let id = core::mem::take(&mut self.pair_id);
            let name = core::mem::take(&mut self.desktop_name);
            *self = Link::new();
            self.key = key;
            self.pair_id = id;
            self.desktop_name = name;
        }
        self.source = Source::Phone;
        self.paired = true;
        self.online = true;
        self.outbox.clear();
    }

    pub fn apply(&mut self, w: &Wire) -> Vec<Event> {
        let mut ev = Vec::new();
        let flag = |v: &str| v == "1" || v == "true";
        match w.op.as_str() {
            "device" => {
                self.device = w.get("name").to_string();
                self.kind = w.get("kind").to_string();
                self.caps = w.get("caps").split(',').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
                self.battery = w.get("battery").parse().unwrap_or(self.battery);
                self.charging = flag(w.get("charging"));
            }
            "battery" => {
                self.battery = w.get("level").parse().unwrap_or(self.battery);
                self.charging = flag(w.get("charging"));
            }
            "thread" => {
                let id = w.get("id");
                let (name, number) = (w.get("name").to_string(), w.get("number").to_string());
                match self.threads.iter_mut().find(|t| t.id == id) {
                    Some(t) => {
                        t.name = name;
                        t.number = number;
                    }
                    None => self.threads.push(Thread { id: id.to_string(), name, number, msgs: vec![], unread: false }),
                }
            }
            "msg" => {
                let id = w.get("thread").to_string();
                let me = flag(w.get("me"));
                let live = flag(w.get("live"));
                let idx = match self.threads.iter().position(|t| t.id == id) {
                    Some(i) => i,
                    None => {
                        let name = if w.get("name").is_empty() { id.clone() } else { w.get("name").to_string() };
                        self.threads.insert(0, Thread { id: id.clone(), name, number: w.get("number").to_string(), msgs: vec![], unread: false });
                        0
                    }
                };
                let t = &mut self.threads[idx];
                t.msgs.push(Msg { me, text: w.get("text").to_string(), time: w.get("time").to_string() });
                if t.msgs.len() > 200 {
                    t.msgs.remove(0);
                }
                if live && !me {
                    t.unread = true;
                    ev.push(Event::Toast(t.name.clone(), w.get("text").to_string()));
                    // newest conversation first
                    let th = self.threads.remove(idx);
                    self.threads.insert(0, th);
                }
            }
            "notif" => {
                let id = w.get("id").to_string();
                self.notifs.retain(|n| n.id != id);
                let n = Notif { id, app: w.get("app").to_string(), title: w.get("title").to_string(), body: w.get("body").to_string(), time: w.get("time").to_string() };
                if flag(w.get("live")) {
                    ev.push(Event::Toast(alloc::format!("{} · {}", n.app, n.title), n.body.clone()));
                }
                self.notifs.insert(0, n);
                self.notifs.truncate(50);
            }
            "notif_rm" => {
                let id = w.get("id");
                self.notifs.retain(|n| n.id != id);
            }
            "call" => {
                self.calls.push(Call { name: w.get("name").to_string(), number: w.get("number").to_string(), when: w.get("when").to_string(), missed: flag(w.get("missed")) });
                self.calls.truncate(50);
            }
            "call_state" => {
                let who = if w.get("name").is_empty() { w.get("number") } else { w.get("name") };
                match w.get("state") {
                    "ringing" => {
                        self.calling = Some(who.to_string());
                        ev.push(Event::Toast(String::from("Incoming call"), who.to_string()));
                    }
                    "active" => self.calling = Some(who.to_string()),
                    _ => self.calling = None,
                }
            }
            "photo" => {
                let id = w.get("id").to_string();
                let thumb = if w.blob.len() == 64 * 64 * 3 { Some(w.blob.clone()) } else { None };
                self.photos.retain(|p| p.id != id);
                self.photos.insert(0, Photo { id, name: w.get("name").to_string(), c1: 0xDCC8AB, c2: 0xC4895E, thumb });
                self.photos.truncate(60);
            }
            "file" => {
                let name = sanitize(w.get("name"));
                self.shared.insert(0, Shared { from_phone: true, text: name.clone(), is_file: true });
                ev.push(Event::SaveFile(name, w.blob.clone()));
            }
            "unlock" => ev.push(Event::Unlock(w.get("id").to_string(), flag(w.get("ok")))),
            "clip" => {
                let text = w.get("text").to_string();
                ev.push(Event::Toast(String::from("From your phone"), text.clone()));
                self.shared.insert(0, Shared { from_phone: true, text, is_file: false });
            }
            _ => {}
        }
        self.shared.truncate(30);
        ev
    }

    // ---- demo phone -------------------------------------------------------

    /// Advance the demo phone. Returns a notification to surface, if any.
    pub fn tick(&mut self, now: u64, time: String) -> Option<(String, String, String)> {
        if self.source != Source::Demo {
            return None;
        }
        if let Some(i) = self.pending.iter().position(|p| p.1 <= now) {
            let (th, _) = self.pending.remove(i);
            let text = REPLIES[(now as usize / 7 + th) % REPLIES.len()];
            let t = &mut self.threads[th];
            t.msgs.push(m(false, text, &time));
            t.unread = true;
            return Some(("Messages".to_string(), t.name.clone(), text.to_string()));
        }
        if self.next_event == 0 {
            self.next_event = now + 4500;
        }
        if now >= self.next_event {
            self.next_event = now + 6000;
            let (app, title, body) = INCOMING[self.script % INCOMING.len()];
            self.script += 1;
            if app == "Messages" {
                if let Some(t) = self.threads.iter_mut().find(|t| t.name == title) {
                    t.msgs.push(m(false, body, &time));
                    t.unread = true;
                }
            } else {
                self.notifs.insert(0, Notif { id: alloc::format!("d{}", self.script), app: app.to_string(), title: title.to_string(), body: body.to_string(), time: time.clone() });
            }
            self.battery = self.battery.saturating_sub(1).max(15);
            return Some((app.to_string(), title.to_string(), body.to_string()));
        }
        None
    }
}

/// Make a received file name safe for the HydatekOS file system.
pub fn sanitize(name: &str) -> String {
    let base = name.rsplit(|c| c == '/' || c == '\\').next().unwrap_or("");
    let mut s: String = base.chars().filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|')).take(80).collect();
    s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() {
        s = String::from("file");
    }
    s
}
