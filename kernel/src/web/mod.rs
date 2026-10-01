//! The web layer: URLs, DNS, HTTP, and the queue apps use to fetch pages.
//! `fetch.rs` does the network work from the main loop.

pub mod css;
pub mod claude;
pub mod dns;
pub mod engines;
#[cfg(target_os = "uefi")]
pub mod fetch;
pub mod html;
pub mod http;
pub mod json;
pub mod render;
pub mod search;
pub mod url;
pub mod weather;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
pub use http::Response;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Progress {
    Resolving,
    Connecting,
    /// TLS handshake (https)
    Securing,
    Waiting,
    /// bytes received, total if known
    Loading(usize, Option<usize>),
}

pub struct Request {
    pub id: u32,
    pub url: String,
    pub method: &'static str,
    pub body: Vec<u8>,
    pub content_type: String,
    pub accept: &'static str,
    /// the page that asked (for images)
    pub referer: String,
    /// extra request headers (an API's key and version)
    pub headers: Vec<(String, String)>,
    /// how long to wait without progress (ms); 0 = the usual 30 s
    pub patience: u64,
}

/// Requests from apps and their results; the main loop's fetcher drains it.
#[derive(Default)]
pub struct WebQueue {
    next: u32,
    pub queue: Vec<Request>,
    pub done: Vec<(u32, Result<Response, String>)>,
    pub progress: BTreeMap<u32, Progress>,
    pub cancel: Vec<u32>,
}

impl WebQueue {
    pub fn get(&mut self, url: &str) -> u32 {
        self.push(url, "GET", Vec::new(), String::new(), http::ACCEPT_PAGE, String::new())
    }

    /// An image for the page at `referer`.
    pub fn get_image(&mut self, url: &str, referer: &str) -> u32 {
        self.push(url, "GET", Vec::new(), String::new(), http::ACCEPT_IMAGE, String::from(referer))
    }

    /// A stylesheet for the page at `referer`.
    pub fn get_css(&mut self, url: &str, referer: &str) -> u32 {
        self.push(url, "GET", Vec::new(), String::new(), http::ACCEPT_CSS, String::from(referer))
    }

    pub fn post(&mut self, url: &str, body: Vec<u8>, content_type: &str) -> u32 {
        self.push(url, "POST", body, String::from(content_type), http::ACCEPT_PAGE, String::new())
    }

    /// A POST to a web API: JSON in and out, with its own headers and a
    /// longer wait for the answer.
    pub fn post_api(&mut self, url: &str, body: Vec<u8>, headers: Vec<(String, String)>, patience: u64) -> u32 {
        self.api("POST", url, body, headers, patience)
    }

    /// A GET from a web API (JSON back).
    pub fn get_api(&mut self, url: &str, headers: Vec<(String, String)>) -> u32 {
        self.api("GET", url, Vec::new(), headers, 0)
    }

    fn api(&mut self, method: &'static str, url: &str, body: Vec<u8>, headers: Vec<(String, String)>, patience: u64) -> u32 {
        let id = self.push(url, method, body, String::from("application/json"), "application/json", String::new());
        if let Some(r) = self.queue.last_mut() {
            r.headers = headers;
            r.patience = patience;
        }
        id
    }

    fn push(&mut self, url: &str, method: &'static str, body: Vec<u8>, content_type: String, accept: &'static str, referer: String) -> u32 {
        self.next += 1;
        let id = self.next;
        self.queue.push(Request { id, url: String::from(url), method, body, content_type, accept, referer, headers: Vec::new(), patience: 0 });
        self.progress.insert(id, Progress::Resolving);
        id
    }

    pub fn take(&mut self, id: u32) -> Option<Result<Response, String>> {
        let k = self.done.iter().position(|d| d.0 == id)?;
        self.progress.remove(&id);
        Some(self.done.remove(k).1)
    }

    pub fn stop(&mut self, id: u32) {
        self.queue.retain(|r| r.id != id);
        self.progress.remove(&id);
        self.cancel.push(id);
    }
}
