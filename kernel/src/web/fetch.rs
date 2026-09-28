//! Runs web requests on the HydatekOS network stack: DNS lookup, TCP
//! connection, HTTP exchange, redirects and cookies.

use super::url::Url;
use super::{dns, http, Progress, WebQueue};
use crate::net::{Net, UDP_CLIENT_PORTS};
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const TIMEOUT_MS: u64 = 30_000;
const MAX_REDIRECTS: u32 = 8;

enum Step {
    Resolve,
    Connect,
    Exchange,
}

struct Job {
    id: u32,
    url: Url,
    method: &'static str,
    body: Vec<u8>,
    content_type: String,
    redirects: u32,
    step: Step,
    ip: [u8; 4],
    conn: Option<u32>,
    sent: bool,
    parser: http::Parser,
    started: u64,
}

struct Lookup {
    qid: u16,
    port: u16,
    sent: u64,
    tries: u32,
}

#[derive(Default)]
pub struct Fetcher {
    jobs: Vec<Job>,
    cache: BTreeMap<String, ([u8; 4], u64)>,
    lookups: BTreeMap<String, Lookup>,
    failed: BTreeMap<String, (&'static str, u64)>,
    next_port: u16,
    /// host -> name -> value
    cookies: BTreeMap<String, BTreeMap<String, String>>,
}

impl Fetcher {
    pub fn new() -> Fetcher {
        Fetcher::default()
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
                Some(url) if url.scheme == "http" => self.jobs.push(Job {
                    id: r.id,
                    url,
                    method: r.method,
                    body: r.body,
                    content_type: r.content_type,
                    redirects: 0,
                    step: Step::Resolve,
                    ip: [0; 4],
                    conn: None,
                    sent: false,
                    parser: http::Parser::default(),
                    started: now,
                }),
                Some(url) if url.scheme == "https" => q.done.push((r.id, Err(String::from("Secure (https) sites need HydatekOS's TLS support, which is being built")))),
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
                                    Some(next) if next.scheme == "http" => {
                                        let keep = matches!(resp.status, 307 | 308);
                                        self.jobs.push(Job {
                                            id: j.id,
                                            url: next,
                                            method: if keep { j.method } else { "GET" },
                                            body: if keep { j.body } else { Vec::new() },
                                            content_type: j.content_type,
                                            redirects: j.redirects + 1,
                                            step: Step::Resolve,
                                            ip: [0; 4],
                                            conn: None,
                                            sent: false,
                                            parser: http::Parser::default(),
                                            started: now,
                                        });
                                        continue;
                                    }
                                    Some(next) if next.scheme == "https" => {
                                        q.done.push((j.id, Err(format!("{} moved to a secure (https) address, which needs HydatekOS's TLS support (being built)", j.url.host))));
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
        if now > self.jobs[k].started + TIMEOUT_MS {
            return Some(Err(format!("{} took too long to answer", self.jobs[k].url.host)));
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
                j.step = Step::Exchange;
                None
            }
            Step::Exchange => {
                let cookies = self.cookie_header(&self.jobs[k].url.host);
                let j = &mut self.jobs[k];
                let c = j.conn?;
                if !j.sent {
                    let req = http::request(j.method, &j.url, &j.body, &j.content_type, &cookies);
                    net.tcp.send(c, &req);
                    net.flush();
                    j.sent = true;
                    q.progress.insert(j.id, Progress::Waiting);
                }
                let data = net.tcp.recv(c);
                if !data.is_empty() {
                    if let Err(e) = j.parser.feed(&data) {
                        return Some(Err(e.to_string()));
                    }
                    let (got, total) = j.parser.received();
                    q.progress.insert(j.id, Progress::Loading(got, total));
                    j.started = now; // still making progress
                }
                if j.parser.done() || net.tcp.peer_closed(c) {
                    let p = core::mem::take(&mut j.parser);
                    return Some(p.finish(&j.url.to_string()).map_err(|e| e.to_string()));
                }
                None
            }
        }
    }
}
