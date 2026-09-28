//! URLs (RFC 3986): parsing, resolving relative references, form encoding.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Url {
    /// "http", "https" or "hydatek"
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// path and query, always starting with '/'
    pub path: String,
    pub fragment: String,
}

fn default_port(scheme: &str) -> u16 {
    match scheme {
        "https" => 443,
        _ => 80,
    }
}

impl Url {
    pub fn parse(s: &str) -> Option<Url> {
        let s = s.trim();
        let (scheme, rest) = s.split_once(':')?;
        let scheme = scheme.to_ascii_lowercase();
        if scheme.is_empty() || !scheme.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.') {
            return None;
        }
        if scheme == "hydatek" {
            // hydatek://search?q=... ; hydatek:start
            let rest = rest.trim_start_matches('/');
            let (body, frag) = rest.split_once('#').unwrap_or((rest, ""));
            let (host, path) = match body.find(['/', '?']) {
                Some(k) => (&body[..k], &body[k..]),
                None => (body, ""),
            };
            let path = if path.starts_with('?') { format!("/{}", path) } else if path.is_empty() { String::from("/") } else { path.to_string() };
            return Some(Url { scheme, host: host.to_ascii_lowercase(), port: 0, path, fragment: frag.to_string() });
        }
        if scheme != "http" && scheme != "https" {
            return None;
        }
        let rest = rest.strip_prefix("//")?;
        let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
        let (auth, path) = match rest.find(['/', '?']) {
            Some(k) => (&rest[..k], &rest[k..]),
            None => (rest, "/"),
        };
        let auth = auth.rsplit('@').next().unwrap_or(auth); // drop user info
        let (host, port) = match auth.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h, p.parse().ok()?),
            _ => (auth, default_port(&scheme)),
        };
        if host.is_empty() || host.contains(|c: char| c.is_whitespace() || c == '/' || c == '\\') {
            return None;
        }
        let path = if path.starts_with('?') { format!("/{}", path) } else { path.to_string() };
        Some(Url { scheme, host: host.to_ascii_lowercase(), port, path: normalize_path(&path), fragment: fragment.to_string() })
    }

    /// Resolve `href` against this URL (as a link on this page would).
    pub fn join(&self, href: &str) -> Option<Url> {
        let href = href.trim();
        if href.is_empty() {
            let mut u = self.clone();
            u.fragment.clear();
            return Some(u);
        }
        if let Some(frag) = href.strip_prefix('#') {
            let mut u = self.clone();
            u.fragment = frag.to_string();
            return Some(u);
        }
        // absolute URL?
        if let Some(k) = href.find(':') {
            let sch = &href[..k];
            if !sch.is_empty() && sch.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.') && !href[..k].contains('/') {
                return Url::parse(href);
            }
        }
        if let Some(rest) = href.strip_prefix("//") {
            return Url::parse(&format!("{}://{}", self.scheme, rest));
        }
        let (body, frag) = href.split_once('#').unwrap_or((href, ""));
        let path = if body.starts_with('/') {
            body.to_string()
        } else if body.starts_with('?') {
            let base = self.path.split('?').next().unwrap_or("/");
            format!("{}{}", base, body)
        } else {
            let base = self.path.split('?').next().unwrap_or("/");
            let dir = &base[..base.rfind('/').map(|k| k + 1).unwrap_or(0)];
            format!("{}{}", if dir.is_empty() { "/" } else { dir }, body)
        };
        Some(Url { scheme: self.scheme.clone(), host: self.host.clone(), port: self.port, path: normalize_path(&path), fragment: frag.to_string() })
    }

    pub fn origin(&self) -> String {
        if self.port == default_port(&self.scheme) || self.scheme == "hydatek" {
            format!("{}://{}", self.scheme, self.host)
        } else {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        }
    }

    /// Host with a non-default port, for the Host header.
    pub fn host_header(&self) -> String {
        if self.port == default_port(&self.scheme) {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    pub fn query(&self) -> &str {
        self.path.split_once('?').map(|x| x.1).unwrap_or("")
    }

    /// A query parameter, decoded.
    pub fn param(&self, key: &str) -> Option<String> {
        self.query().split('&').filter_map(|kv| kv.split_once('=').or(Some((kv, "")))).find(|(k, _)| decode(k) == key).map(|(_, v)| decode(v))
    }

    pub fn without_fragment(&self) -> String {
        let mut u = self.clone();
        u.fragment.clear();
        u.to_string()
    }
}

impl core::fmt::Display for Url {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        if self.scheme == "hydatek" {
            let p = if self.path == "/" { "" } else if self.path.starts_with("/?") { &self.path[1..] } else { &self.path };
            write!(f, "hydatek://{}{}", self.host, p)?;
        } else {
            write!(f, "{}{}", self.origin(), self.path)?;
        }
        if !self.fragment.is_empty() {
            write!(f, "#{}", self.fragment)?;
        }
        Ok(())
    }
}

/// Remove "." and ".." segments.
fn normalize_path(p: &str) -> String {
    let (path, query) = match p.find('?') {
        Some(k) => (&p[..k], &p[k..]),
        None => (p, ""),
    };
    let mut out: Vec<&str> = Vec::new();
    let trailing = path.ends_with('/') || path.ends_with("/.") || path.ends_with("/..");
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    let mut s = String::from("/");
    s.push_str(&out.join("/"));
    if trailing && s.len() > 1 {
        s.push('/');
    }
    s + query
}

/// Percent-decode (and '+' as space, as forms send it).
pub fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() + 0 && i + 2 <= b.len() - 1 || (b[i] == b'%' && i + 2 < b.len() + 1 && i + 2 <= b.len() - 1) => {
                match u8::from_str_radix(core::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// application/x-www-form-urlencoded
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// What was typed in the address bar: a URL, a bare host, or a search.
pub fn from_input(s: &str) -> Url {
    let t = s.trim();
    if let Some(u) = Url::parse(t) {
        return u;
    }
    let looks_like_host = !t.contains(' ') && (t.contains('.') || t.starts_with("localhost")) && !t.ends_with('.');
    if looks_like_host {
        if let Some(u) = Url::parse(&format!("http://{}", t)) {
            return u;
        }
    }
    Url::parse(&format!("hydatek://search?q={}", encode(t))).unwrap()
}
