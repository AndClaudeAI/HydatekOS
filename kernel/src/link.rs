//! Phone Link: the desktop side of the Hydatek Link Protocol (HLP).
//!
//! A linked phone exposes four channels — messages, notifications, photos and
//! calls — plus screen mirroring. Milestone 1 ships the full desktop
//! experience against a *virtual* HydatekOS Mobile device that runs inside the
//! OS itself; the wire transport (Wi-Fi/Bluetooth) arrives with the network
//! stack. See docs/PHONE_LINK.md for the protocol.

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
    pub name: String,
    pub msgs: Vec<Msg>,
    pub unread: bool,
}

pub struct Notif {
    pub app: String,
    pub title: String,
    pub body: String,
    pub time: String,
}

pub struct Call {
    pub name: String,
    pub when: String,
    pub missed: bool,
}

pub struct Link {
    pub paired: bool,
    pub device: String,
    pub battery: u8,
    pub code: u32,
    pub threads: Vec<Thread>,
    pub notifs: Vec<Notif>,
    pub photos: Vec<(String, u32, u32)>,
    pub calls: Vec<Call>,
    pub calling: Option<String>,
    /// (thread, due tick) for pending simulated replies
    pending: Vec<(usize, u64)>,
    next_event: u64,
    script: usize,
}

fn m(me: bool, text: &str, time: &str) -> Msg {
    Msg { me, text: text.to_string(), time: time.to_string() }
}

const REPLIES: [&str; 6] = [
    "Sounds good!",
    "On my way!",
    "Can you send me the file?",
    "Haha, yes. See you there.",
    "Perfect, thanks!",
    "Let me check and get back to you.",
];

const INCOMING: [(&str, &str, &str); 4] = [
    ("Messages", "Ada", "Are we still on for lunch tomorrow?"),
    ("Calendar", "Design review", "Starts in 30 minutes · Studio"),
    ("Mail", "Hydatek Team", "Milestone 1 build is ready to test"),
    ("Messages", "Mum", "Call me when you're free"),
];

impl Link {
    pub fn new() -> Link {
        Link {
            paired: false,
            device: String::from("HydatekOS Mobile"),
            battery: 82,
            code: 482913,
            threads: vec![
                Thread { name: "Ada".to_string(), unread: true, msgs: vec![m(false, "Hey! Did you see the new dock design?", "09:12"), m(true, "Yes, it looks great in dark mode too", "09:15"), m(false, "Lunch tomorrow?", "10:30")] },
                Thread { name: "Mum".to_string(), unread: false, msgs: vec![m(false, "Don't forget Sunday dinner", "Yesterday"), m(true, "I won't!", "Yesterday")] },
                Thread { name: "Studio group".to_string(), unread: false, msgs: vec![m(false, "Review moved to 16:30", "08:02"), m(true, "Noted, thanks", "08:05")] },
            ],
            notifs: vec![
                Notif { app: "Mail".to_string(), title: "Invoice INV-0042".to_string(), body: "Your invoice is ready".to_string(), time: "08:40".to_string() },
                Notif { app: "Weather".to_string(), title: "Sunny, 27°".to_string(), body: "Clear skies all day".to_string(), time: "07:00".to_string() },
            ],
            photos: vec![
                ("Dunes at dawn".to_string(), 0xE4B783, 0xC4895E),
                ("Night sky".to_string(), 0x2B2A48, 0x5A4F7C),
                ("Oasis".to_string(), 0x7FAF6B, 0x2F6690),
                ("Sunset".to_string(), 0xD9793A, 0x7B3F7A),
                ("Market".to_string(), 0xDCC8AB, 0xB5581B),
                ("Studio".to_string(), 0xF1E6D9, 0x5E5866),
            ],
            calls: vec![
                Call { name: "Ada".to_string(), when: "Today, 08:10".to_string(), missed: false },
                Call { name: "Mum".to_string(), when: "Yesterday, 19:32".to_string(), missed: true },
                Call { name: "Studio".to_string(), when: "Mon, 11:05".to_string(), missed: false },
            ],
            calling: None,
            pending: vec![],
            next_event: 0,
            script: 0,
        }
    }

    pub fn unread(&self) -> usize {
        self.threads.iter().filter(|t| t.unread).count()
    }

    pub fn send(&mut self, thread: usize, text: &str, time: String, now: u64) {
        if let Some(t) = self.threads.get_mut(thread) {
            t.msgs.push(Msg { me: true, text: text.to_string(), time });
            self.pending.push((thread, now + 300));
        }
    }

    /// Advance the virtual phone. Returns a notification to surface, if any.
    pub fn tick(&mut self, now: u64, time: String) -> Option<(String, String, String)> {
        if !self.paired {
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
                self.notifs.insert(0, Notif { app: app.to_string(), title: title.to_string(), body: body.to_string(), time: time.clone() });
            }
            self.battery = self.battery.saturating_sub(1).max(15);
            return Some((app.to_string(), title.to_string(), body.to_string()));
        }
        None
    }
}
