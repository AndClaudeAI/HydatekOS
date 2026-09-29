//! Runs web requests on the HydatekOS network stack: DNS lookup, TCP
//! connection, TLS for https, HTTP exchange, redirects and cookies.

use super::url::Url;
use super::{dns, http, Progress, WebQueue};
use crate::net::{Net, UDP_CLIENT_PORTS};
use crate::tls::client::Client;
use crate::tls::x509::{self, Roots};
use alloc::collections::BTreeMap;
use alloc::rc::Rc;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const TIMEOUT_MS: u64 = 30_000;
const MAX_REDIRECTS: u32 = 8;

enum Step {
    Resolve,
    Connect,
    Secure,
    Exchange,
}

struct Job {
    id: u32,
    url: Url,
    method: &'static str,
    body: Vec<u8>,
    content_type: String,
    accept: &'static str,
    referer: String,
    headers: Vec<(String, String)>,
    patience: u64,
    redirects: u32,
    step: Step,
    ip: [u8; 4],
    conn: Option<u32>,
    sent: bool,
    parser: http::Parser,
    started: u64,
    tls: Option<Client>,
    /// bytes waiting for room in the TCP send buffer
    outq: Vec<u8>,
}

struct Lookup {
    qid: u16,
    port: u16,
    sent: u64,
    tries: u32,
}

pub struct Fetcher {
    roots: Rc<Roots>,
    jobs: Vec<Job>,
    cache: BTreeMap<String, ([u8; 4], u64)>,
    lookups: BTreeMap<String, Lookup>,
    failed: BTreeMap<String, (&'static str, u64)>,
    next_port: u16,
    /// host -> name -> value
    cookies: BTreeMap<String, BTreeMap<String, String>>,
}

impl Fetcher {
    pub fn new(roots: Roots) -> Fetcher {
        Fetcher {
            roots: Rc::new(roots),
            jobs: Vec::new(),
            cache: BTreeMap::new(),
            lookups: BTreeMap::new(),
            failed: BTreeMap::new(),
            next_port: 0,
            cookies: BTreeMap::new(),
        }
    }

    fn cookie_header(&self, host: &str) -> String {
        let mut parts = Vec::new();
        for (h, jar) in &self.cookies {
            if host == h || host.ends_with(&format!(".{}", h)) {
                for (k, v) in jar {
                    parts.push(format!("{}={}", k, v));
                }
            }
        }
        parts.join("; ")
    }

    fn store_cookies(&mut self, host: &str, r: &http::Response) {
        for (k, v) in &r.headers {
            if !k.eq_ignore_ascii_case("set-cookie") {
                continue;
            }
            let mut domain = host.to_string();
            let mut it = v.split(';');
            let Some((name, value)) = it.next().and_then(|p| p.split_once('=')) else { continue };
            for attr in it {
                if let Some((a, d)) = attr.split_once('=') {
                    if a.trim().eq_ignore_ascii_case("domain") {
                        let d = d.trim().trim_start_matches('.').to_ascii_lowercase();
                        // only the site itself or a parent of it
                        if host == d || host.ends_with(&format!(".{}", d)) {
                            domain = d;
                        }
                    }
                }
            }
            self.cookies.entry(domain).or_default().insert(name.trim().to_string(), value.trim().to_string());
        }
    }

    /// Resolve `host`: Some(Ok(ip)) when known, None while waiting.
    fn resolve(&mut self, net: &mut Net, host: &str, now: u64) -> Option<Result<[u8; 4], &'static str>> {
        if let Some(ip) = dns::parse_ip(host) {
            return Some(Ok(ip));
        }
        if let Some((ip, until)) = self.cache.get(host) {
            if now < *until {
                return Some(Ok(*ip));
            }
        }
        if let Some((why, until)) = self.failed.get(host) {
            if now < *until {
                return Some(Err(why));
            }
        }
        if net.dns == [0; 4] {
            return Some(Err("no DNS server (is the network connected?)"));
        }
        let resend = match self.lookups.get(host) {
            None => true,
            Some(l) => now > l.sent + 1500,
        };
        if resend {
            let tries = self.lookups.get(host).map_or(0, |l| l.tries);
            if tries >= 4 {
                self.lookups.remove(host);
                self.failed.insert(host.to_string(), ("the name server didn't answer", now + 10_000));
                return Some(Err("the name server didn't answer"));
            }
            self.next_port = if self.next_port < UDP_CLIENT_PORTS.start || self.next_port + 1 >= UDP_CLIENT_PORTS.end { UDP_CLIENT_PORTS.start } else { self.next_port + 1 };
            let qid = crate::rng::u32() as u16;
            let (dns_ip, port) = (net.dns, self.next_port);
            net.send_udp(dns_ip, port, 53, &dns::query(qid, host));
            self.lookups.insert(host.to_string(), Lookup { qid, port, sent: now, tries: tries + 1 });
        }
        None
    }

    fn take_dns_replies(&mut self, net: &mut Net, now: u64) {
        for (_, sport, dport, data) in core::mem::take(&mut net.udp_rx) {
            if sport != 53 {
                continue;
            }
            let host = self.lookups.iter().find(|(_, l)| l.port == dport).map(|(h, _)| h.clone());
            let Some(host) = host else { continue };
            let qid = self.lookups[&host].qid;
            match dns::answer(&data, qid) {
                Some(Ok((ips, ttl))) => {
                    self.lookups.remove(&host);
                    self.cache.insert(host, (ips[0], now + ttl as u64 * 1000));
                }
                Some(Err(why)) => {
                    self.lookups.remove(&host);
                    self.failed.insert(host, (why, now + 30_000));
                }
                None => {}
            }
        }
    }

    /// Advance every request. Returns true when something finished.
    pub fn poll(&mut self, net: &mut Net, q: &mut WebQueue, now: u64) -> bool {
        for r in core::mem::take(&mut q.queue) {
            match Url::parse(&r.url) {
                Some(url) if url.scheme == "http" || url.scheme == "https" => self.jobs.push(Job {
                    id: r.id,
                    url,
                    method: r.method,
                    body: r.body,
                    content_type: r.content_type,
                    accept: r.accept,
                    referer: r.referer,
                    headers: r.headers,
                    patience: if r.patience == 0 { TIMEOUT_MS } else { r.patience },
                    redirects: 0,
                    step: Step::Resolve,
                    ip: [0; 4],
                    conn: None,
                    sent: false,
                    parser: http::Parser::default(),
                    started: now,
                    tls: None,
                    outq: Vec::new(),
                }),
                _ => q.done.push((r.id, Err(format!("HydatekOS can't open \"{}\"", r.url)))),
            }
        }
        for id in core::mem::take(&mut q.cancel) {
            if let Some(k) = self.jobs.iter().position(|j| j.id == id) {
                let j = self.jobs.remove(k);
                if let Some(c) = j.conn {
                    net.tcp.close(c);
                }
            }
        }
        self.take_dns_replies(net, now);
        let mut finished = false;
        let mut k = 0;
        while k < self.jobs.len() {
            let res = self.step(net, k, q, now);
            match res {
                None => k += 1,
                Some(result) => {
                    let j = self.jobs.remove(k);
                    if let Some(c) = j.conn {
                        net.tcp.close(c);
                        net.tcp.reap(c);
                    }
                    match result {
                        Ok(resp) => {
                            self.store_cookies(&j.url.host, &resp);
                            let loc = resp.header("location").map(|s| s.to_string());
                            if matches!(resp.status, 301 | 302 | 303 | 307 | 308) && loc.is_some() && j.redirects < MAX_REDIRECTS {
                                match loc.and_then(|l| j.url.join(&l)) {
                                    Some(next) if next.scheme == "http" || next.scheme == "https" => {
                                        let keep = matches!(resp.status, 307 | 308);
                                        let same_site = next.host == j.url.host;
                                        self.jobs.push(Job {
                                            id: j.id,
                                            url: next,
                                            method: if keep { j.method } else { "GET" },
                                            body: if keep { j.body } else { Vec::new() },
                                            content_type: j.content_type,
                                            accept: j.accept,
                                            referer: j.referer,
                                            // the headers go to the same site only
                                            headers: if same_site { j.headers } else { Vec::new() },
                                            patience: j.patience,
                                            redirects: j.redirects + 1,
                                            step: Step::Resolve,
                                            ip: [0; 4],
                                            conn: None,
                                            sent: false,
                                            parser: http::Parser::default(),
                                            started: now,
                                            tls: None,
                                            outq: Vec::new(),
                                        });
                                        continue;
                                    }
                                    _ => q.done.push((j.id, Ok(resp))),
                                }
                            } else {
                                q.done.push((j.id, Ok(resp)));
                            }
                        }
                        Err(e) => q.done.push((j.id, Err(e))),
                    }
                    finished = true;
                }
            }
        }
        finished
    }

    fn step(&mut self, net: &mut Net, k: usize, q: &mut WebQueue, now: u64) -> Option<Result<http::Response, String>> {
        if now > self.jobs[k].started + self.jobs[k].patience {
            return Some(Err(format!("{} took too long to answer", self.jobs[k].url.host)));
        }
        if let Some(c) = self.jobs[k].conn {
            let j = &mut self.jobs[k];
            if !j.outq.is_empty() {
                let n = net.tcp.send(c, &j.outq);
                j.outq.drain(..n);
                net.flush();
            }
        }
        match self.jobs[k].step {
            Step::Resolve => {
                q.progress.insert(self.jobs[k].id, Progress::Resolving);
                let host = self.jobs[k].url.host.clone();
                match self.resolve(net, &host, now)? {
                    Ok(ip) => {
                        let j = &mut self.jobs[k];
                        j.ip = ip;
                        j.conn = Some(net.connect(ip, j.url.port));
                        j.step = Step::Connect;
                        None
                    }
                    Err(why) => Some(Err(format!("Couldn't find {}: {}", host, why))),
                }
            }
            Step::Connect => {
                let j = &mut self.jobs[k];
                q.progress.insert(j.id, Progress::Connecting);
                let c = j.conn?;
                if net.tcp.connecting(c) {
                    return None;
                }
                if !net.tcp.is_open(c) {
                    return Some(Err(format!("{} refused the connection", j.url.host)));
                }
                if j.url.scheme == "https" {
                    let mut seed = [0u8; 32];
                    crate::rng::fill(&mut seed);
                    let mut t = Client::new(&j.url.host, unix_now(), self.roots.clone(), seed);
                    j.outq.extend(t.take_output());
                    j.tls = Some(t);
                    j.step = Step::Secure;
                } else {
                    j.step = Step::Exchange;
                }
                None
            }
            Step::Secure => {
                let j = &mut self.jobs[k];
                q.progress.insert(j.id, Progress::Securing);
                let c = j.conn?;
                let t = j.tls.as_mut()?;
                let data = net.tcp.recv(c);
                if !data.is_empty() {
                    if let Err(e) = t.feed(&data) {
                        return Some(Err(format!("Couldn't connect securely to {}. {}", j.url.host, e)));
                    }
                    j.outq.extend(t.take_output());
                }
                if t.ready() {
                    j.step = Step::Exchange;
                } else if net.tcp.peer_closed(c) {
                    return Some(Err(format!("{} closed the connection during the secure handshake.", j.url.host)));
                }
                None
            }
            Step::Exchange => {
                let cookies = self.cookie_header(&self.jobs[k].url.host);
                let j = &mut self.jobs[k];
                let c = j.conn?;
                if !j.sent {
                    let req = http::request(j.method, &j.url, &j.body, &j.content_type, &cookies, j.accept, &j.referer, &j.headers);
                    match j.tls.as_mut() {
                        Some(t) => {
                            t.write(&req);
                            j.outq.extend(t.take_output());
                        }
                        None => j.outq.extend(req),
                    }
                    j.sent = true;
                    q.progress.insert(j.id, Progress::Waiting);
                }
                let mut data = net.tcp.recv(c);
                let mut closed = net.tcp.peer_closed(c);
                if let Some(t) = j.tls.as_mut() {
                    if !data.is_empty() {
                        if let Err(e) = t.feed(&data) {
                            return Some(Err(format!("The secure connection to {} failed. {}", j.url.host, e)));
                        }
                        j.outq.extend(t.take_output());
                    }
                    data = t.read();
                    closed |= t.closed();
                }
                if !data.is_empty() {
                    if let Err(e) = j.parser.feed(&data) {
                        return Some(Err(e.to_string()));
                    }
                    let (got, total) = j.parser.received();
                    q.progress.insert(j.id, Progress::Loading(got, total));
                    j.started = now; // still making progress
                }
                if j.parser.done() || closed {
                    let p = core::mem::take(&mut j.parser);
                    let mut r = p.finish(&j.url.to_string()).map_err(|e| e.to_string());
                    if let (Ok(resp), Some(t)) = (r.as_mut(), j.tls.as_mut()) {
                        if let Some(v) = &t.verified {
                            resp.security = Some(format!("{} · Certificate from {}, valid until {}", t.summary(), v.issuer, x509::date(v.expires)));
                        }
                        t.close();
                        net.tcp.send(c, &t.take_output());
                        net.flush();
                    }
                    return Some(r);
                }
                None
            }
        }
    }
}

/// The firmware clock as seconds since 1970 (UTC).
fn unix_now() -> i64 {
    let t = crate::efi::now();
    let days = x509::days_from_civil(t.year as i64, t.month as i64, t.day as i64);
    // The zone offset's sign differs between firmware; a few hours don't
    // matter for certificate dates.
    days * 86400 + t.hour as i64 * 3600 + t.minute as i64 * 60 + t.second as i64
}
