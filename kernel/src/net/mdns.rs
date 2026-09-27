//! Multicast DNS responder (RFC 6762) with DNS-SD (RFC 6763): answers for
//! `<host>.local` and advertises the Phone Link service `_hydatek-link._tcp`.

use super::Ip;
use alloc::string::String;
use alloc::vec::Vec;

pub const SERVICE: &str = "_hydatek-link._tcp.local";
pub const LINK_PORT: u16 = 7743;

const T_A: u16 = 1;
const T_PTR: u16 = 12;
const T_TXT: u16 = 16;
const T_SRV: u16 = 33;
const T_ANY: u16 = 255;

pub struct Mdns {
    host: String,
    instance: String,
    pub txt: Vec<String>,
    announce_at: [u64; 2],
}

fn put_name(out: &mut Vec<u8>, name: &str) {
    for label in name.split('.').filter(|l| !l.is_empty()) {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
}

/// Read a (possibly compressed) name starting at `pos`; returns (name, next pos).
fn read_name(p: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut name = String::new();
    let mut end = None;
    for _ in 0..64 {
        let len = *p.get(pos)? as usize;
        if len == 0 {
            return Some((name, end.unwrap_or(pos + 1)));
        }
        if len & 0xc0 == 0xc0 {
            let ptr = ((len & 0x3f) << 8) | *p.get(pos + 1)? as usize;
            if end.is_none() {
                end = Some(pos + 2);
            }
            pos = ptr;
            continue;
        }
        let label = p.get(pos + 1..pos + 1 + len)?;
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(&String::from_utf8_lossy(label));
        pos += 1 + len;
    }
    None
}

fn record(out: &mut Vec<u8>, name: &str, ty: u16, flush: bool, ttl: u32, rdata: &[u8]) {
    put_name(out, name);
    out.extend_from_slice(&ty.to_be_bytes());
    out.extend_from_slice(&(if flush { 0x8001u16 } else { 1u16 }).to_be_bytes());
    out.extend_from_slice(&ttl.to_be_bytes());
    out.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    out.extend_from_slice(rdata);
}

impl Mdns {
    pub fn new(host: &str) -> Mdns {
        Mdns { host: alloc::format!("{}.local", host), instance: alloc::format!("HydatekOS {}.{}", host, SERVICE), txt: Vec::new(), announce_at: [0, 0] }
    }

    pub fn announce(&mut self, now: u64) {
        self.announce_at = [now + 200, now + 1200];
    }

    pub fn timer(&mut self, now: u64, ip: Ip) -> Option<Vec<u8>> {
        for t in self.announce_at.iter_mut() {
            if *t != 0 && now >= *t {
                *t = 0;
                let mut v = Vec::new();
                let n = self.answers(&mut v, T_ANY, SERVICE, ip);
                return Some(self.packet(0, n, v));
            }
        }
        None
    }

    fn packet(&self, id: u16, answers: u16, body: Vec<u8>) -> Vec<u8> {
        let mut p = Vec::with_capacity(12 + body.len());
        p.extend_from_slice(&id.to_be_bytes());
        p.extend_from_slice(&[0x84, 0x00, 0, 0]);
        p.extend_from_slice(&answers.to_be_bytes());
        p.extend_from_slice(&[0, 0, 0, 0]);
        p.extend_from_slice(&body);
        p
    }

    /// Append the records answering (`ty`, `name`); returns how many.
    fn answers(&self, out: &mut Vec<u8>, ty: u16, name: &str, ip: Ip) -> u16 {
        let mut n = 0;
        let want = |t: u16| ty == t || ty == T_ANY;
        let is_host = name.eq_ignore_ascii_case(&self.host);
        let is_service = name.eq_ignore_ascii_case(SERVICE);
        let is_instance = name.eq_ignore_ascii_case(&self.instance);
        let is_meta = name.eq_ignore_ascii_case("_services._dns-sd._udp.local");
        if is_meta && want(T_PTR) {
            let mut rd = Vec::new();
            put_name(&mut rd, SERVICE);
            record(out, name, T_PTR, false, 4500, &rd);
            n += 1;
        }
        if is_service && want(T_PTR) {
            let mut rd = Vec::new();
            put_name(&mut rd, &self.instance);
            record(out, SERVICE, T_PTR, false, 4500, &rd);
            n += 1;
        }
        if is_service || is_instance {
            if want(T_SRV) || is_service {
                let mut rd = Vec::new();
                rd.extend_from_slice(&[0, 0, 0, 0]);
                rd.extend_from_slice(&LINK_PORT.to_be_bytes());
                put_name(&mut rd, &self.host);
                record(out, &self.instance, T_SRV, true, 120, &rd);
                n += 1;
            }
            if want(T_TXT) || is_service {
                let mut rd = Vec::new();
                for t in self.txt.iter().chain(core::iter::once(&String::from("v=1"))) {
                    rd.push(t.len() as u8);
                    rd.extend_from_slice(t.as_bytes());
                }
                record(out, &self.instance, T_TXT, true, 4500, &rd);
                n += 1;
            }
        }
        if (is_host && want(T_A)) || is_service || is_instance {
            record(out, &self.host, T_A, true, 120, &ip);
            n += 1;
        }
        n
    }

    /// Handle a query; returns (response, unicast?) when we have answers.
    pub fn query(&self, p: &[u8], ip: Ip) -> Option<(Vec<u8>, bool)> {
        if p.len() < 12 || p[2] & 0x80 != 0 {
            return None; // not a query
        }
        let id = u16::from_be_bytes([p[0], p[1]]);
        let qd = u16::from_be_bytes([p[4], p[5]]);
        let mut pos = 12;
        let mut body = Vec::new();
        let mut count = 0;
        let mut unicast = false;
        for _ in 0..qd.min(16) {
            let (name, next) = read_name(p, pos)?;
            if next + 4 > p.len() {
                return None;
            }
            let ty = u16::from_be_bytes([p[next], p[next + 1]]);
            let class = u16::from_be_bytes([p[next + 2], p[next + 3]]);
            unicast |= class & 0x8000 != 0;
            pos = next + 4;
            count += self.answers(&mut body, ty, &name, ip);
        }
        if count == 0 {
            return None;
        }
        Some((self.packet(if unicast { id } else { 0 }, count, body), unicast))
    }
}
