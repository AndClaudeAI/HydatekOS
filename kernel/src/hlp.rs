//! Hydatek Link Protocol (HLP/1) message format and encrypted session.
//!
//! Transport: WebSocket (`ws://<pc>:7743/hlp`).
//!
//! Handshake (text frames):
//!   phone → PC   "hello\nnonce=<b64url 16B>\npair=<pairing id>"
//!   PC → phone   "welcome\nnonce=<b64url 16B>\nname=<desktop name>"
//! Both sides derive, from the 32-byte pairing key K shared through the QR code:
//!   c2s = HKDF-SHA256(salt = client_nonce || server_nonce, ikm = K, info = "hlp1 c2s")
//!   s2c = HKDF-SHA256(same salt, ikm = K, info = "hlp1 s2c")
//! After that every message is a binary frame:
//!   counter (u64 big-endian, starts at 0, +1 per message and direction)
//!   || ChaCha20-Poly1305(key, nonce = 0u32 || counter, aad = "hlp1", payload)
//! A frame that fails to authenticate, or arrives with an unexpected counter,
//! ends the session.
//!
//! Payload: UTF-8 header lines — the operation name, then `key=value` lines
//! (values escape `\` as `\\` and newline as `\n`) — optionally followed by a
//! blank line and a binary blob.

use crate::crypto;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const AAD: &[u8] = b"hlp1";

#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub op: String,
    pub fields: Vec<(String, String)>,
    pub blob: Vec<u8>,
}

fn escape(v: &str) -> String {
    let mut s = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => {}
            c => s.push(c),
        }
    }
    s
}

fn unescape(v: &str) -> String {
    let mut s = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            match it.next() {
                Some('n') => s.push('\n'),
                Some(o) => s.push(o),
                None => {}
            }
        } else {
            s.push(c);
        }
    }
    s
}

impl Msg {
    pub fn new(op: &str) -> Msg {
        Msg { op: op.to_string(), fields: Vec::new(), blob: Vec::new() }
    }

    pub fn with(mut self, k: &str, v: &str) -> Msg {
        self.fields.push((k.to_string(), v.to_string()));
        self
    }

    pub fn blob(mut self, b: Vec<u8>) -> Msg {
        self.blob = b;
        self
    }

    pub fn get(&self, k: &str) -> &str {
        self.fields.iter().find(|f| f.0 == k).map(|f| f.1.as_str()).unwrap_or("")
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut s = String::new();
        s.push_str(&self.op);
        for (k, v) in &self.fields {
            s.push('\n');
            s.push_str(k);
            s.push('=');
            s.push_str(&escape(v));
        }
        let mut out = s.into_bytes();
        if !self.blob.is_empty() {
            out.extend_from_slice(b"\n\n");
            out.extend_from_slice(&self.blob);
        }
        out
    }

    pub fn decode(p: &[u8]) -> Option<Msg> {
        let split = p.windows(2).position(|w| w == b"\n\n");
        let (head, blob) = match split {
            Some(i) => (&p[..i], p[i + 2..].to_vec()),
            None => (p, Vec::new()),
        };
        let head = core::str::from_utf8(head).ok()?;
        let mut lines = head.split('\n');
        let op = lines.next()?.trim().to_string();
        if op.is_empty() {
            return None;
        }
        let mut fields = Vec::new();
        for l in lines {
            if let Some((k, v)) = l.split_once('=') {
                fields.push((k.to_string(), unescape(v)));
            }
        }
        Some(Msg { op, fields, blob })
    }
}

pub struct Session {
    tx_key: [u8; 32],
    rx_key: [u8; 32],
    tx_ctr: u64,
    rx_ctr: u64,
}

fn nonce(ctr: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&ctr.to_be_bytes());
    n
}

impl Session {
    /// Server-side session keys.
    pub fn server(key: &[u8; 32], client_nonce: &[u8], server_nonce: &[u8]) -> Session {
        let (c2s, s2c) = derive(key, client_nonce, server_nonce);
        Session { tx_key: s2c, rx_key: c2s, tx_ctr: 0, rx_ctr: 0 }
    }

    /// Client-side session keys (used by tests; phones implement the same).
    #[allow(dead_code)]
    pub fn client(key: &[u8; 32], client_nonce: &[u8], server_nonce: &[u8]) -> Session {
        let (c2s, s2c) = derive(key, client_nonce, server_nonce);
        Session { tx_key: c2s, rx_key: s2c, tx_ctr: 0, rx_ctr: 0 }
    }

    pub fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let ctr = self.tx_ctr;
        self.tx_ctr += 1;
        let mut out = ctr.to_be_bytes().to_vec();
        out.extend_from_slice(&crypto::seal(&self.tx_key, &nonce(ctr), AAD, plain));
        out
    }

    pub fn open(&mut self, frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < 8 + 16 {
            return None;
        }
        let ctr = u64::from_be_bytes(frame[..8].try_into().ok()?);
        if ctr != self.rx_ctr {
            return None;
        }
        let p = crypto::open(&self.rx_key, &nonce(ctr), AAD, &frame[8..])?;
        self.rx_ctr += 1;
        Some(p)
    }
}

fn derive(key: &[u8; 32], cn: &[u8], sn: &[u8]) -> ([u8; 32], [u8; 32]) {
    let mut salt = cn.to_vec();
    salt.extend_from_slice(sn);
    let mut c2s = [0u8; 32];
    let mut s2c = [0u8; 32];
    crypto::hkdf(&salt, key, b"hlp1 c2s", &mut c2s);
    crypto::hkdf(&salt, key, b"hlp1 s2c", &mut s2c);
    (c2s, s2c)
}
