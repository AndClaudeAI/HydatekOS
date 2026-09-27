//! Phone Link server on TCP port 7743.
//!
//! * `GET /`        the web companion (works in any phone browser)
//! * `GET /app.apk` the Android companion app, if installed on the disk
//! * `GET /hlp`     WebSocket carrying the encrypted Hydatek Link Protocol

use crate::crypto;
use crate::hlp::{Msg, Session};
use crate::link::Event;
use crate::net::Net;
use crate::sys::Sys;
use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const PORT: u16 = 7743;
const COMPANION_HTML: &str = include_str!("../../companion/web/index.html");
const COMPANION_JS: &str = include_str!("../../companion/web/hlp.js");
pub const APK_PATH: &str = "/apps/hydatek-link.apk";
const MAX_MESSAGE: usize = 48 << 20;

enum Hs {
    AwaitHello,
    Keyed { sess: Session, verified: bool },
}

enum Mode {
    Http,
    Ws { hs: Hs, frag: Vec<u8>, frag_op: u8 },
}

struct Client {
    id: u32,
    inbuf: Vec<u8>,
    out: VecDeque<u8>,
    mode: Mode,
    closing: bool,
    last_rx: u64,
    last_ping: u64,
}

pub struct LinkServer {
    clients: Vec<Client>,
    active: Option<u32>,
}

fn ws_frame(op: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 10);
    f.push(0x80 | op);
    let n = payload.len();
    if n < 126 {
        f.push(n as u8);
    } else if n < 65536 {
        f.push(126);
        f.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        f.push(127);
        f.extend_from_slice(&(n as u64).to_be_bytes());
    }
    f.extend_from_slice(payload);
    f
}

fn http_response(status: &str, ctype: &str, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut r = alloc::format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nServer: HydatekOS\r\n{}\r\n",
        status,
        ctype,
        body.len(),
        extra
    )
    .into_bytes();
    r.extend_from_slice(body);
    r
}

impl Client {
    fn send_ws(&mut self, op: u8, payload: &[u8]) {
        self.out.extend(ws_frame(op, payload));
    }

    fn send_msg(&mut self, m: &Msg) {
        if let Mode::Ws { hs: Hs::Keyed { sess, .. }, .. } = &mut self.mode {
            let f = sess.seal(&m.encode());
            self.out.extend(ws_frame(2, &f));
        }
    }
}

impl LinkServer {
    pub fn new(net: &mut Net) -> LinkServer {
        net.tcp.listen(PORT);
        LinkServer { clients: vec![], active: None }
    }

    /// Serve clients; returns true if anything the UI shows may have changed.
    pub fn poll(&mut self, net: &mut Net, sys: &mut Sys) -> bool {
        let now = net.now;
        let mut changed = false;
        while let Some(id) = net.tcp.accept(PORT) {
            self.clients.push(Client { id, inbuf: vec![], out: VecDeque::new(), mode: Mode::Http, closing: false, last_rx: now, last_ping: now });
        }
        let mut i = 0;
        while i < self.clients.len() {
            let data = net.tcp.recv(self.clients[i].id);
            if !data.is_empty() {
                changed |= matches!(self.clients[i].mode, Mode::Ws { .. });
                self.clients[i].last_rx = now;
                self.clients[i].inbuf.extend_from_slice(&data);
                self.process(i, sys, now);
            }
            let c = &mut self.clients[i];
            // keep-alive for phone sessions; drop silent peers
            if let Mode::Ws { .. } = c.mode {
                if now > c.last_ping + 15_000 && Some(c.id) == self.active {
                    c.last_ping = now;
                    c.send_msg(&Msg::new("ping"));
                }
                if now > c.last_rx + 60_000 {
                    c.closing = true;
                    c.out.clear();
                }
            } else if now > c.last_rx + 20_000 {
                c.closing = true;
            }
            // flush output as the TCP window allows
            while !c.out.is_empty() {
                let (a, b) = c.out.as_slices();
                let chunk = if a.is_empty() { b } else { a };
                let n = net.tcp.send(c.id, &chunk[..chunk.len().min(256 * 1024)]);
                if n == 0 {
                    break;
                }
                c.out.drain(..n);
            }
            if c.closing && c.out.is_empty() {
                net.tcp.close(c.id);
            }
            let drained = !net.tcp.is_open(c.id) && net.tcp.pending(c.id) == 0;
            let gone = !net.tcp.alive(c.id) || drained || (net.tcp.peer_closed(c.id) && c.out.is_empty());
            if gone {
                let id = c.id;
                net.tcp.close(id);
                net.tcp.reap(id);
                if self.active == Some(id) {
                    changed = true;
                    self.active = None;
                    sys.link.online = false;
                    sys.toast("Phone Link", &alloc::format!("{} disconnected", sys.link.device));
                }
                self.clients.remove(i);
                continue;
            }
            i += 1;
        }
        // deliver queued commands to the connected phone
        if let Some(active) = self.active {
            if !sys.link.outbox.is_empty() {
                let msgs: Vec<Msg> = sys.link.outbox.drain(..).collect();
                if let Some(c) = self.clients.iter_mut().find(|c| c.id == active) {
                    for m in &msgs {
                        c.send_msg(m);
                    }
                }
            }
        } else {
            sys.link.outbox.clear();
        }
        changed
    }

    fn process(&mut self, i: usize, sys: &mut Sys, now: u64) {
        if let Mode::Http = self.clients[i].mode {
            self.http(i, sys);
        }
        if let Mode::Ws { .. } = self.clients[i].mode {
            self.ws(i, sys, now);
        }
    }

    fn http(&mut self, i: usize, sys: &mut Sys) {
        let c = &mut self.clients[i];
        let Some(end) = c.inbuf.windows(4).position(|w| w == b"\r\n\r\n") else {
            if c.inbuf.len() > 16384 {
                c.closing = true;
            }
            return;
        };
        let head = String::from_utf8_lossy(&c.inbuf[..end]).to_string();
        c.inbuf.drain(..end + 4);
        let mut lines = head.split("\r\n");
        let req = lines.next().unwrap_or("");
        let mut parts = req.split(' ');
        let method = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("/").split('?').next().unwrap_or("/");
        let mut ws_key = String::new();
        let mut upgrade = false;
        for l in lines {
            if let Some((k, v)) = l.split_once(':') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim();
                if k == "sec-websocket-key" {
                    ws_key = v.to_string();
                } else if k == "upgrade" && v.eq_ignore_ascii_case("websocket") {
                    upgrade = true;
                }
            }
        }
        if method != "GET" {
            c.out.extend(http_response("405 Method Not Allowed", "text/plain", "", b"GET only\n"));
            c.closing = true;
            return;
        }
        match path {
            "/hlp" if upgrade && !ws_key.is_empty() => {
                let mut k = ws_key.clone();
                k.push_str("258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
                let accept = crypto::base64(&crypto::sha1(k.as_bytes()));
                let r = alloc::format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", accept);
                c.out.extend(r.as_bytes());
                c.mode = Mode::Ws { hs: Hs::AwaitHello, frag: vec![], frag_op: 0 };
            }
            "/" | "/index.html" => {
                c.out.extend(http_response("200 OK", "text/html; charset=utf-8", "", COMPANION_HTML.as_bytes()));
                c.closing = true;
            }
            "/hlp.js" => {
                c.out.extend(http_response("200 OK", "text/javascript; charset=utf-8", "", COMPANION_JS.as_bytes()));
                c.closing = true;
            }
            "/app.apk" | "/hydatek-link.apk" => match sys.fs.read(APK_PATH) {
                Some(apk) => {
                    c.out.extend(http_response("200 OK", "application/vnd.android.package-archive", "Content-Disposition: attachment; filename=\"hydatek-link.apk\"\r\n", &apk));
                    c.closing = true;
                }
                None => {
                    c.out.extend(http_response("404 Not Found", "text/plain", "", b"The Android app is not installed on this PC.\n"));
                    c.closing = true;
                }
            },
            _ => {
                c.out.extend(http_response("404 Not Found", "text/plain", "", b"Not found\n"));
                c.closing = true;
            }
        }
    }

    fn ws(&mut self, i: usize, sys: &mut Sys, now: u64) {
        loop {
            let c = &mut self.clients[i];
            let b = &c.inbuf;
            if b.len() < 2 {
                return;
            }
            let fin = b[0] & 0x80 != 0;
            let op = b[0] & 0x0f;
            let masked = b[1] & 0x80 != 0;
            let mut len = (b[1] & 0x7f) as usize;
            let mut pos = 2;
            if len == 126 {
                if b.len() < 4 {
                    return;
                }
                len = u16::from_be_bytes([b[2], b[3]]) as usize;
                pos = 4;
            } else if len == 127 {
                if b.len() < 10 {
                    return;
                }
                len = u64::from_be_bytes(b[2..10].try_into().unwrap()) as usize;
                pos = 10;
            }
            if len > MAX_MESSAGE || !masked {
                c.closing = true;
                c.inbuf.clear();
                return;
            }
            if b.len() < pos + 4 + len {
                return;
            }
            let mask = [b[pos], b[pos + 1], b[pos + 2], b[pos + 3]];
            pos += 4;
            let mut payload: Vec<u8> = b[pos..pos + len].to_vec();
            for (j, x) in payload.iter_mut().enumerate() {
                *x ^= mask[j & 3];
            }
            c.inbuf.drain(..pos + len);
            match op {
                0x8 => {
                    c.send_ws(0x8, &[]);
                    c.closing = true;
                    return;
                }
                0x9 => {
                    c.send_ws(0xA, &payload);
                    continue;
                }
                0xA => continue,
                _ => {}
            }
            let Mode::Ws { frag, frag_op, .. } = &mut c.mode else { return };
            let msg_op;
            let message = if op == 0 {
                frag.extend_from_slice(&payload);
                if frag.len() > MAX_MESSAGE {
                    c.closing = true;
                    return;
                }
                if !fin {
                    continue;
                }
                msg_op = *frag_op;
                core::mem::take(frag)
            } else if !fin {
                *frag = payload;
                *frag_op = op;
                continue;
            } else {
                msg_op = op;
                payload
            };
            self.message(i, msg_op, message, sys, now);
            if self.clients[i].closing {
                return;
            }
        }
    }

    fn message(&mut self, i: usize, op: u8, data: Vec<u8>, sys: &mut Sys, _now: u64) {
        let c = &mut self.clients[i];
        if matches!(c.mode, Mode::Ws { hs: Hs::AwaitHello, .. }) {
            let hello = if op == 1 { Msg::decode(&data) } else { None };
            let Some(hello) = hello.filter(|m| m.op == "hello") else {
                c.closing = true;
                return;
            };
            let cn = crypto::base64_decode(hello.get("nonce")).unwrap_or_default();
            if hello.get("pair") != sys.link.pair_id || cn.len() != 16 {
                c.send_ws(1, &Msg::new("error").with("reason", "This phone isn't paired with this PC. Scan the code in Phone Link again.").encode());
                c.closing = true;
                return;
            }
            let mut sn = [0u8; 16];
            crate::rng::fill(&mut sn);
            let w = Msg::new("welcome").with("nonce", &crypto::base64url(&sn)).with("name", &sys.link.desktop_name);
            c.send_ws(1, &w.encode());
            if let Mode::Ws { hs, .. } = &mut c.mode {
                *hs = Hs::Keyed { sess: Session::server(&sys.link.key, &cn, &sn), verified: false };
            }
            return;
        }
        let Mode::Ws { hs: Hs::Keyed { sess, verified }, .. } = &mut c.mode else { return };
        let plain = if op == 2 { sess.open(&data) } else { None };
        let Some(plain) = plain else {
            // wrong key, replay or tampering: end the session
            c.closing = true;
            c.out.clear();
            return;
        };
        let first = !*verified;
        *verified = true;
        let id = c.id;
        if first {
            // the phone proved it holds the pairing key
            if let Some(old) = self.active.replace(id) {
                if let Some(o) = self.clients.iter_mut().find(|x| x.id == old) {
                    o.closing = true;
                }
            }
            sys.link.phone_connected();
            self.clients[i].send_msg(&Msg::new("welcome").with("name", &sys.link.desktop_name));
        }
        {
            {
                let Some(m) = Msg::decode(&plain) else { return };
                match m.op.as_str() {
                    "ping" => self.clients[i].send_msg(&Msg::new("pong")),
                    "pong" => {}
                    _ => {
                        let announce = m.op == "device" && first;
                        for e in sys.link.apply(&m) {
                            match e {
                                Event::Toast(t, b) => {
                                    if !sys.focus {
                                        sys.toast(&t, &b);
                                    }
                                }
                                Event::Unlock(id, ok) => sys.reqs.push(crate::sys::Req::PhoneUnlock(id, ok)),
                                Event::SaveFile(name, bytes) => {
                                    let lower = name.to_ascii_lowercase();
                                    let img = [".jpg", ".jpeg", ".png", ".heic", ".webp", ".gif"].iter().any(|e| lower.ends_with(e));
                                    let dir = if img { "/home/Pictures" } else { "/home/Downloads" };
                                    let (stem, ext) = match name.rfind('.') {
                                        Some(p) if p > 0 => (&name[..p], &name[p..]),
                                        _ => (name.as_str(), ""),
                                    };
                                    let path = sys.fs.unique(dir, stem, ext);
                                    sys.fs.write(&path, &bytes);
                                    sys.toast("Received from your phone", &alloc::format!("{} saved to {}", crate::fs::basename(&path), if img { "Pictures" } else { "Downloads" }));
                                }
                            }
                        }
                        if announce {
                            sys.save_link();
                            sys.toast("Phone Link", &alloc::format!("{} is connected", sys.link.device));
                        }
                    }
                }
            }
        }
    }
}
