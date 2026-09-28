//! HTTP/1.1 (RFC 9112) client messages: requests, and an incremental response
//! parser handling Content-Length, chunked and close-delimited bodies, and
//! gzip / deflate content encoding.

use super::url::Url;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const USER_AGENT: &str = "Mozilla/5.0 (HydatekOS 0.1) Hyda/0.1";

pub fn request(method: &str, url: &Url, body: &[u8], content_type: &str, cookies: &str) -> Vec<u8> {
    let mut h = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: {}\r\nAccept: text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.8\r\nAccept-Language: en\r\nAccept-Encoding: gzip, deflate\r\nConnection: close\r\n",
        method,
        url.path,
        url.host_header(),
        USER_AGENT
    );
    if !cookies.is_empty() {
        h.push_str(&format!("Cookie: {}\r\n", cookies));
    }
    if method == "POST" {
        h.push_str(&format!("Content-Type: {}\r\nContent-Length: {}\r\n", content_type, body.len()));
    }
    h.push_str("\r\n");
    let mut out = h.into_bytes();
    out.extend_from_slice(body);
    out
}

#[derive(Clone, Debug, Default)]
pub struct Response {
    pub url: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// for https: how the connection was secured
    pub security: Option<String>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn content_type(&self) -> String {
        self.header("content-type").unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase()
    }
}

enum Body {
    Length(usize),
    Chunked,
    Close,
}

/// Feed bytes as they arrive; `done()` says when the response is complete.
#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
    head: Option<(u16, Vec<(String, String)>)>,
    body_start: usize,
    body: Option<Body>,
    complete: bool,
}

impl Default for Body {
    fn default() -> Body {
        Body::Close
    }
}

impl Parser {
    pub fn feed(&mut self, data: &[u8]) -> Result<(), &'static str> {
        self.buf.extend_from_slice(data);
        if self.buf.len() > 64 << 20 {
            return Err("the page is too large");
        }
        if self.head.is_none() {
            let Some(end) = find(&self.buf, b"\r\n\r\n") else {
                if self.buf.len() > 64 * 1024 {
                    return Err("the server sent a malformed reply");
                }
                return Ok(());
            };
            let head = String::from_utf8_lossy(&self.buf[..end]).into_owned();
            let mut lines = head.split("\r\n");
            let status_line = lines.next().unwrap_or("");
            let mut it = status_line.split(' ');
            let ver = it.next().unwrap_or("");
            if !ver.starts_with("HTTP/") {
                return Err("the server didn't reply with HTTP");
            }
            let status: u16 = it.next().and_then(|s| s.parse().ok()).ok_or("the server sent a malformed reply")?;
            let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
            let get = |n: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.clone());
            self.body = Some(if get("transfer-encoding").map_or(false, |v| v.to_ascii_lowercase().contains("chunked")) {
                Body::Chunked
            } else if let Some(n) = get("content-length").and_then(|v| v.parse().ok()) {
                Body::Length(n)
            } else if status == 204 || status == 304 || (100..200).contains(&status) {
                Body::Length(0)
            } else {
                Body::Close
            });
            self.head = Some((status, headers));
            self.body_start = end + 4;
        }
        let body = &self.buf[self.body_start..];
        self.complete = match self.body {
            Some(Body::Length(n)) => body.len() >= n,
            Some(Body::Chunked) => dechunk(body).map_or(false, |(_, done)| done),
            _ => false,
        };
        Ok(())
    }

    pub fn done(&self) -> bool {
        self.complete
    }

    pub fn received(&self) -> (usize, Option<usize>) {
        let got = self.buf.len().saturating_sub(self.body_start);
        match self.body {
            Some(Body::Length(n)) => (got, Some(n)),
            _ => (got, None),
        }
    }

    /// The response, once complete (or when the connection closed).
    pub fn finish(self, url: &str) -> Result<Response, &'static str> {
        let (status, headers) = self.head.ok_or("the server closed the connection without replying")?;
        let raw = &self.buf[self.body_start..];
        let body = match self.body {
            Some(Body::Length(n)) => {
                if raw.len() < n {
                    return Err("the connection closed before the page finished");
                }
                raw[..n].to_vec()
            }
            Some(Body::Chunked) => dechunk(raw).map(|x| x.0).ok_or("the server sent a malformed reply")?,
            _ => raw.to_vec(),
        };
        let enc = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-encoding")).map(|(_, v)| v.to_ascii_lowercase()).unwrap_or_default();
        let body = match enc.as_str() {
            "gzip" | "x-gzip" => gunzip(&body).ok_or("couldn't unpack the page (gzip)")?,
            "deflate" => inflate_any(&body).ok_or("couldn't unpack the page (deflate)")?,
            _ => body,
        };
        Ok(Response { url: url.to_string(), status, headers, body, security: None })
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// Decode a chunked body: (data so far, complete?). None if malformed.
fn dechunk(mut b: &[u8]) -> Option<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = find(b, b"\r\n") else { return Some((out, false)) };
        let line = core::str::from_utf8(&b[..eol]).ok()?;
        let size = usize::from_str_radix(line.split(';').next()?.trim(), 16).ok()?;
        if size == 0 {
            return Some((out, true));
        }
        let start = eol + 2;
        if b.len() < start + size + 2 {
            return Some((out, false));
        }
        out.extend_from_slice(&b[start..start + size]);
        b = &b[start + size + 2..];
    }
}

pub fn gunzip(d: &[u8]) -> Option<Vec<u8>> {
    if d.len() < 18 || d[0] != 0x1f || d[1] != 0x8b || d[2] != 8 {
        return None;
    }
    let flags = d[3];
    let mut i = 10;
    if flags & 4 != 0 {
        i += 2 + u16::from_le_bytes([d[i], d[i + 1]]) as usize;
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            while *d.get(i)? != 0 {
                i += 1;
            }
            i += 1;
        }
    }
    if flags & 2 != 0 {
        i += 2;
    }
    let size = u32::from_le_bytes(d[d.len() - 4..].try_into().ok()?) as usize;
    crate::zip::inflate(d.get(i..d.len() - 8)?, size)
}

/// "deflate" is zlib-wrapped in practice, raw in some servers.
fn inflate_any(d: &[u8]) -> Option<Vec<u8>> {
    if d.len() > 2 && d[0] & 0x0F == 8 && (u16::from_be_bytes([d[0], d[1]]) % 31 == 0) {
        if let Some(v) = crate::zip::inflate(&d[2..], 0) {
            return Some(v);
        }
    }
    crate::zip::inflate(d, 0)
}
