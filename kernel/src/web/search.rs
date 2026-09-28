//! Hyda Search: HydatekOS's own search engine. An inverted index with BM25
//! ranking over the pages you visit, the sites its crawler reads, and your
//! files. Everything stays on this computer.

use super::html::Dom;
use super::url::Url;
use super::WebQueue;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const TEXT_CAP: usize = 8000;
const MAX_DOCS: usize = 20_000;

const STOP: [&str; 32] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "he", "in", "is", "it", "its", "of", "on", "or", "that", "the", "this", "to", "was", "were", "will", "with", "i", "you", "we", "they", "what",
];

pub fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && w.chars().count() <= 40)
        .map(|w| w.to_lowercase())
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub url: String,
    pub title: String,
    pub text: String,
    len: u32,
}

#[derive(Clone, Debug)]
pub struct Hit {
    pub url: String,
    pub title: String,
    /// snippet text and the byte ranges of matched words in it
    pub snippet: String,
    pub marks: Vec<(usize, usize)>,
}

#[derive(Default)]
pub struct Index {
    docs: Vec<Option<Doc>>,
    by_url: BTreeMap<String, usize>,
    /// term -> (doc, count in body, in title)
    post: BTreeMap<String, Vec<(u32, u16, bool)>>,
    total_len: u64,
    live: usize,
    pub changed: bool,
}

impl Index {
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn sites(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for d in self.docs.iter().flatten() {
            let site = Url::parse(&d.url).map(|u| u.host).unwrap_or_else(|| String::from("Your files"));
            *m.entry(site).or_insert(0) += 1;
        }
        m
    }

    pub fn add(&mut self, url: &str, title: &str, text: &str) {
        if let Some(&old) = self.by_url.get(url) {
            if let Some(d) = self.docs[old].take() {
                self.total_len -= d.len as u64;
                self.live -= 1;
            }
        }
        if self.live >= MAX_DOCS {
            return;
        }
        let text: String = {
            let mut t = String::new();
            for (k, w) in text.split_whitespace().enumerate() {
                if t.len() + w.len() + 1 > TEXT_CAP {
                    break;
                }
                if k > 0 {
                    t.push(' ');
                }
                t.push_str(w);
            }
            t
        };
        let id = self.docs.len() as u32;
        let body = words(&text);
        let mut tf: BTreeMap<String, (u16, bool)> = BTreeMap::new();
        for w in &body {
            tf.entry(w.clone()).or_insert((0, false)).0 += 1;
        }
        for w in words(title) {
            tf.entry(w).or_insert((0, false)).1 = true;
        }
        for (w, (n, t)) in tf {
            self.post.entry(w).or_default().push((id, n, t));
        }
        self.total_len += body.len() as u64;
        self.docs.push(Some(Doc { url: url.to_string(), title: title.to_string(), text, len: body.len() as u32 }));
        self.by_url.insert(url.to_string(), id as usize);
        self.live += 1;
        self.changed = true;
    }

    pub fn remove_site(&mut self, host: &str) {
        let urls: Vec<String> = self.docs.iter().flatten().filter(|d| Url::parse(&d.url).map_or(false, |u| u.host == host)).map(|d| d.url.clone()).collect();
        for u in urls {
            if let Some(&i) = self.by_url.get(&u) {
                if let Some(d) = self.docs[i].take() {
                    self.total_len -= d.len as u64;
                    self.live -= 1;
                }
                self.by_url.remove(&u);
            }
        }
        self.changed = true;
    }

    /// BM25 over body text, with matches in the title counting extra.
    pub fn search(&self, q: &str, limit: usize) -> Vec<Hit> {
        let mut terms: Vec<String> = words(q);
        terms.dedup();
        let content: Vec<String> = terms.iter().filter(|t| !STOP.contains(&t.as_str())).cloned().collect();
        let terms = if content.is_empty() { terms } else { content };
        if terms.is_empty() || self.live == 0 {
            return vec![];
        }
        let n = self.live as f64;
        let avg = (self.total_len as f64 / n).max(1.0);
        let (k1, b) = (1.2, 0.75);
        let mut scores: BTreeMap<u32, (f64, usize)> = BTreeMap::new();
        for t in &terms {
            // exact word, plus simple plural/-ing variants
            let mut lists: Vec<&Vec<(u32, u16, bool)>> = Vec::new();
            for v in [t.clone(), format!("{}s", t), format!("{}es", t), t.trim_end_matches('s').to_string()] {
                if let Some(l) = self.post.get(&v) {
                    if !lists.iter().any(|x| core::ptr::eq(*x, l)) {
                        lists.push(l);
                    }
                }
            }
            let df = lists.iter().flat_map(|l| l.iter()).filter(|p| self.docs[p.0 as usize].is_some()).map(|p| p.0).collect::<BTreeSet<_>>().len() as f64;
            if df == 0.0 {
                continue;
            }
            let idf = ln_1p((n - df + 0.5) / (df + 0.5));
            let mut seen = BTreeSet::new();
            for l in lists {
                for &(d, tf, in_title) in l {
                    let Some(doc) = &self.docs[d as usize] else { continue };
                    if !seen.insert(d) {
                        continue;
                    }
                    let tf = tf as f64;
                    let norm = tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * doc.len as f64 / avg));
                    let e = scores.entry(d).or_insert((0.0, 0));
                    e.0 += idf * (norm + if in_title { 1.5 } else { 0.0 });
                    e.1 += 1;
                }
            }
        }
        let want = terms.len();
        let mut ranked: Vec<(u32, f64)> = scores
            .into_iter()
            .map(|(d, (s, matched))| {
                // documents with every term first
                let bonus = if matched >= want { 100.0 } else { 0.0 };
                (d, s + bonus)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(core::cmp::Ordering::Equal));
        ranked
            .into_iter()
            .take(limit)
            .filter_map(|(d, score)| {
                let doc = self.docs[d as usize].as_ref()?;
                let (snippet, marks) = snippet(&doc.text, &terms);
                let _ = score;
                Some(Hit { url: doc.url.clone(), title: if doc.title.is_empty() { doc.url.clone() } else { doc.title.clone() }, snippet, marks })
            })
            .collect()
    }

    // ---- persistence: /system/search.hydx -------------------------------------------------

    pub fn save(&self) -> Vec<u8> {
        let mut out = String::from("HYDX 1\n");
        for d in self.docs.iter().flatten() {
            let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
            out.push_str(&format!("d {}\t{}\t{}\n", clean(&d.url), clean(&d.title), clean(&d.text)));
        }
        out.into_bytes()
    }

    pub fn load(data: &[u8]) -> Index {
        let mut ix = Index::default();
        let s = String::from_utf8_lossy(data);
        if !s.starts_with("HYDX 1\n") {
            return ix;
        }
        for line in s.lines().skip(1) {
            if let Some(rest) = line.strip_prefix("d ") {
                let mut it = rest.splitn(3, '\t');
                if let (Some(u), Some(t), Some(x)) = (it.next(), it.next(), it.next()) {
                    ix.add(u, t, x);
                }
            }
        }
        ix.changed = false;
        ix
    }
}

/// ln(1 + x) for x >= 0 without libm.
fn ln_1p(x: f64) -> f64 {
    let y = 1.0 + x.max(0.0);
    let (mut m, mut k) = (y, 0i32);
    while m >= 2.0 {
        m /= 2.0;
        k += 1;
    }
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let (mut term, mut sum, mut n) = (z, 0.0, 1.0);
    for _ in 0..30 {
        sum += term / n;
        term *= z2;
        n += 2.0;
    }
    2.0 * sum + k as f64 * core::f64::consts::LN_2
}

/// ~200 characters around the first matching word, with the matches marked.
fn snippet(text: &str, terms: &[String]) -> (String, Vec<(usize, usize)>) {
    let lower = text.to_lowercase();
    let mut first = None;
    for t in terms {
        let mut from = 0;
        while let Some(k) = lower[from..].find(t.as_str()).map(|k| k + from) {
            let before_ok = k == 0 || !lower[..k].chars().next_back().map_or(false, |c| c.is_alphanumeric());
            if before_ok {
                first = Some(first.map_or(k, |f: usize| f.min(k)));
                break;
            }
            from = k + t.len();
        }
    }
    let start = first.map(|k| k.saturating_sub(60)).unwrap_or(0);
    let mut s = start.min(text.len());
    while s > 0 && !text.is_char_boundary(s) {
        s -= 1;
    }
    // start at a word boundary
    if s > 0 {
        if let Some(sp) = text[s..].find(' ') {
            s += sp + 1;
        }
    }
    let mut e = (s + 200).min(text.len());
    while e < text.len() && !text.is_char_boundary(e) {
        e += 1;
    }
    if e < text.len() {
        if let Some(sp) = text[..e].rfind(' ') {
            if sp > s {
                e = sp;
            }
        }
    }
    let mut out = String::new();
    if s > 0 {
        out.push_str("… ");
    }
    let base = out.len();
    out.push_str(&text[s..e]);
    if e < text.len() {
        out.push_str(" …");
    }
    let mut marks = Vec::new();
    let body_lower = text[s..e].to_lowercase();
    if body_lower.len() == e - s {
        for t in terms {
            let mut from = 0;
            while let Some(k) = body_lower[from..].find(t.as_str()).map(|k| k + from) {
                let a = k;
                let mut b = k + t.len();
                while b < body_lower.len() && body_lower[b..].chars().next().map_or(false, |c| c.is_alphanumeric()) {
                    b += body_lower[b..].chars().next().unwrap().len_utf8();
                }
                let before_ok = a == 0 || !body_lower[..a].chars().next_back().map_or(false, |c| c.is_alphanumeric());
                if before_ok {
                    marks.push((base + a, base + b));
                }
                from = b;
            }
        }
        marks.sort();
    }
    (out, marks)
}

// ---- the crawler -----------------------------------------------------------------------

struct Active {
    id: u32,
    url: String,
    depth: u32,
    robots_for: Option<String>,
}

#[derive(Default)]
pub struct Crawler {
    queue: VecDeque<(String, u32)>,
    seen: BTreeSet<String>,
    active: Option<Active>,
    /// host -> disallowed path prefixes (None while fetching robots.txt)
    robots: BTreeMap<String, Vec<String>>,
    next_at: u64,
    /// pages fetched per host
    pages: BTreeMap<String, u32>,
    pub status: String,
}

pub const MAX_PAGES_PER_SITE: u32 = 60;
const MAX_DEPTH: u32 = 3;
const DELAY_TICKS: u64 = 100; // one page per second

fn robots_rules(txt: &str) -> Vec<String> {
    let mut rules = Vec::new();
    let mut applies = false;
    let mut in_group_agents = false;
    for line in txt.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match k.as_str() {
            "user-agent" => {
                if !in_group_agents {
                    applies = false;
                }
                in_group_agents = true;
                if v == "*" || v.to_ascii_lowercase().contains("hyda") {
                    applies = true;
                }
            }
            "disallow" => {
                in_group_agents = false;
                if applies && !v.is_empty() {
                    rules.push(v.to_string());
                }
            }
            _ => in_group_agents = false,
        }
    }
    rules
}

impl Crawler {
    /// Start reading a site from `url`.
    pub fn add(&mut self, url: &str) {
        if let Some(u) = Url::parse(url) {
            let s = u.without_fragment();
            self.seen.remove(&s);
            self.pages.remove(&u.host);
            self.queue.push_front((s, 0));
        }
    }

    pub fn busy(&self) -> bool {
        self.active.is_some() || !self.queue.is_empty()
    }

    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    fn allowed(&self, u: &Url) -> bool {
        match self.robots.get(&u.host) {
            Some(rules) => !rules.iter().any(|p| u.path.starts_with(p.as_str())),
            None => true,
        }
    }

    /// Called every tick: at most one request in flight, one per second.
    pub fn tick(&mut self, web: &mut WebQueue, index: &mut Index, now: u64) {
        if let Some(a) = &self.active {
            let Some(res) = web.take(a.id) else { return };
            let a = self.active.take().unwrap();
            self.next_at = now + DELAY_TICKS;
            if let Some(host) = a.robots_for {
                let rules = match res {
                    Ok(r) if r.status == 200 => robots_rules(&String::from_utf8_lossy(&r.body)),
                    _ => vec![],
                };
                self.robots.insert(host, rules);
                return;
            }
            match res {
                Ok(r) if r.status == 200 && (r.content_type().contains("html") || r.content_type().is_empty()) => {
                    let html = String::from_utf8_lossy(&r.body).into_owned();
                    let dom = super::html::parse(&html);
                    let (index_ok, follow) = dom.robots();
                    let base = Url::parse(&r.url).or_else(|| Url::parse(&a.url));
                    if index_ok {
                        index.add(&r.url, &dom.title(), &dom.text(0));
                        self.status = format!("Indexed {}", r.url);
                    }
                    if follow && a.depth < MAX_DEPTH {
                        if let Some(base) = base {
                            for href in dom.links() {
                                if let Some(u) = base.join(&href) {
                                    if u.host == base.host && (u.scheme == "http" || u.scheme == "https") {
                                        let s = u.without_fragment();
                                        let lower = s.to_ascii_lowercase();
                                        let skip = [".jpg", ".png", ".gif", ".pdf", ".zip", ".mp4", ".mp3", ".css", ".js", ".svg", ".webp", ".ico"].iter().any(|e| lower.ends_with(e));
                                        if !skip && !self.seen.contains(&s) && self.queue.len() < 2000 {
                                            self.queue.push_back((s, a.depth + 1));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(r) => self.status = format!("Skipped {} ({})", r.url, r.status),
                Err(e) => self.status = format!("Couldn't read {}: {}", a.url, e),
            }
            return;
        }
        if now < self.next_at {
            return;
        }
        while let Some((url, depth)) = self.queue.pop_front() {
            let Some(u) = Url::parse(&url) else { continue };
            if self.seen.contains(&url) {
                continue;
            }
            if !self.robots.contains_key(&u.host) {
                // read the site's robots.txt first
                self.queue.push_front((url, depth));
                let robots = format!("{}/robots.txt", u.origin());
                let id = web.get(&robots);
                self.active = Some(Active { id, url: robots, depth: 0, robots_for: Some(u.host.clone()) });
                self.status = format!("Reading {}'s rules for crawlers", u.host);
                return;
            }
            let count = self.pages.get(&u.host).copied().unwrap_or(0);
            if count >= MAX_PAGES_PER_SITE || !self.allowed(&u) {
                self.seen.insert(url);
                continue;
            }
            self.seen.insert(url.clone());
            self.pages.insert(u.host.clone(), count + 1);
            let id = web.get(&url);
            self.status = format!("Reading {}", url);
            self.active = Some(Active { id, url, depth, robots_for: None });
            return;
        }
        if self.status.starts_with("Reading") || self.status.starts_with("Indexed") {
            self.status = String::from("Finished reading");
        }
    }
}

/// The search engine: index + crawler.
#[derive(Default)]
pub struct Search {
    pub index: Index,
    pub crawler: Crawler,
    saved_at: u64,
}

impl Search {
    pub fn tick(&mut self, web: &mut WebQueue, now: u64) {
        self.crawler.tick(web, &mut self.index, now);
    }

    /// Something to save (at most every 10 s)?
    pub fn to_save(&mut self, now: u64) -> Option<Vec<u8>> {
        if self.index.changed && now > self.saved_at + 1000 {
            self.index.changed = false;
            self.saved_at = now;
            return Some(self.index.save());
        }
        None
    }

    /// Index a page someone looked at.
    pub fn visited(&mut self, url: &str, dom: &Dom) {
        let (index_ok, _) = dom.robots();
        if index_ok {
            self.index.add(url, &dom.title(), &dom.text(0));
        }
    }
}
