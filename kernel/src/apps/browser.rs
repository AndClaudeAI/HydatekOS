//! Browser: HydatekOS's web browser. Pages are fetched by the HydatekOS network
//! stack (web/fetch.rs), parsed and laid out by its own engine (web/html.rs,
//! web/render.rs), and searches go to Hyda Search, the built-in search engine.

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use crate::web::css::{self, Cascade};
use crate::web::{engines, html};
use crate::web::render::{self, Control, Item, Page};
use crate::web::url::{self, Url};
use crate::image::{self as picture, Image};
use crate::web::Progress;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

const C_URL: u32 = 1;
const C_BACK: u32 = 2;
const C_FWD: u32 = 3;
const C_RELOAD: u32 = 4;
const C_HOME: u32 = 5;
const C_PAGE: u32 = 6;
const C_LOCK: u32 = 7;
const C_LINK: u32 = 1000;
const C_FIELD: u32 = 100_000;

const PAD: i32 = 24;
const STATUS: i32 = 24;

/// A picture scaled for the screen: size, animation frame, pixels.
struct Scaled {
    w: i32,
    h: i32,
    frame: usize,
    px: Vec<u32>,
}

/// An image on the page.
enum Pic {
    Queued,
    Loading(u32),
    /// a bitmap (maybe animated)
    Ready(Rc<Image>, Option<Scaled>),
    /// an SVG, drawn afresh at each size
    Vector(Rc<picture::svg::Svg>, Option<Scaled>),
    Broken,
}

impl Pic {
    fn size(&self) -> Option<(u32, u32)> {
        match self {
            Pic::Ready(i, _) => Some((i.w, i.h)),
            Pic::Vector(v, _) => Some(v.size()),
            _ => None,
        }
    }

    fn animated(&self) -> bool {
        matches!(self, Pic::Ready(i, _) if !i.frames.is_empty())
    }

    /// Pixels for a dw × dh box at time `ms` (for animations).
    fn pixels(&mut self, dw: i32, dh: i32, ms: u64) -> Option<&[u32]> {
        if dw <= 0 || dh <= 0 || dw as i64 * dh as i64 > 40_000_000 {
            return None;
        }
        match self {
            Pic::Ready(img, cache) => {
                let frame = if img.frames.is_empty() {
                    0
                } else {
                    let total: u64 = img.frames.iter().map(|f| f.delay as u64).sum::<u64>().max(1);
                    let mut t = ms % total;
                    let mut k = 0;
                    while k + 1 < img.frames.len() && t >= img.frames[k].delay as u64 {
                        t -= img.frames[k].delay as u64;
                        k += 1;
                    }
                    k
                };
                if cache.as_ref().map_or(true, |c| (c.w, c.h, c.frame) != (dw, dh, frame)) {
                    let src = if img.frames.is_empty() { &img.px } else { &img.frames[frame].px };
                    *cache = Some(Scaled { w: dw, h: dh, frame, px: crate::gfx::scale_argb(src, img.w as i32, img.h as i32, dw, dh) });
                }
                cache.as_ref().map(|c| c.px.as_slice())
            }
            Pic::Vector(svg, cache) => {
                if cache.as_ref().map_or(true, |c| (c.w, c.h) != (dw, dh)) {
                    *cache = Some(Scaled { w: dw, h: dh, frame: 0, px: svg.render(dw as u32, dh as u32).px });
                }
                cache.as_ref().map(|c| c.px.as_slice())
            }
            _ => None,
        }
    }
}

/// Images fetched at once.
const MAX_IMAGE_LOADS: usize = 6;

/// The key an image address is stored under: an absolute URL (or the
/// data:/file: address itself).
fn image_key(page: &Url, src: &str) -> Option<String> {
    if src.starts_with("data:") || src.starts_with("file:") {
        return Some(src.to_string());
    }
    page.join(src).filter(|u| u.scheme == "http" || u.scheme == "https").map(|u| u.without_fragment())
}

/// The bytes of a data: address (base64 or percent-encoded).
fn data_uri(s: &str) -> Option<Vec<u8>> {
    let (head, body) = s.strip_prefix("data:")?.split_once(',')?;
    if head.ends_with(";base64") {
        crate::crypto::base64_decode(&body.chars().filter(|c| !c.is_whitespace()).collect::<String>())
    } else {
        Some(url::decode(body).into_bytes())
    }
}

fn decode_pic(data: &[u8]) -> Pic {
    if picture::sniff(data) == Some(picture::Format::Svg) {
        return match picture::svg::parse(data) {
            Ok(svg) => Pic::Vector(Rc::new(svg), None),
            Err(_) => Pic::Broken,
        };
    }
    match picture::decode(data) {
        Ok(img) => Pic::Ready(Rc::new(img), None),
        Err(_) => Pic::Broken,
    }
}

/// Where a background's tiles go: tile size and the first tile's offset
/// inside a box of bw × bh.
fn bg_tiles(nat: (u32, u32), bw: i32, bh: i32, size: render::BgSize, pos: (render::Dim, render::Dim)) -> (i32, i32, i32, i32) {
    use render::{BgSize, Dim};
    let (nw, nh) = (nat.0.max(1) as f64, nat.1.max(1) as f64);
    let len = |d: Dim, full: i32| match d {
        Dim::Px(v) => Some(v as f64),
        Dim::Pct(p) => Some(full as f64 * p as f64 / 100.0),
        Dim::Auto => None,
    };
    let (tw, th) = match size {
        BgSize::Auto => (nw, nh),
        BgSize::Cover | BgSize::Contain => {
            let (sx, sy) = (bw as f64 / nw, bh as f64 / nh);
            let s = if size == BgSize::Cover { sx.max(sy) } else { sx.min(sy) };
            (nw * s, nh * s)
        }
        BgSize::Set(a, b) => match (len(a, bw), len(b, bh)) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * nh / nw),
            (None, Some(h)) => (h * nw / nh, h),
            _ => (nw, nh),
        },
    };
    let (tw, th) = ((tw + 0.5).max(1.0) as i32, (th + 0.5).max(1.0) as i32);
    let off = |d: Dim, free: i32| match d {
        Dim::Px(v) => v,
        Dim::Pct(p) => free * p / 100,
        Dim::Auto => 0,
    };
    (tw, th, off(pos.0, bw - tw), off(pos.1, bh - th))
}

/// A stylesheet from the web (its url()s made absolute).
enum SheetState {
    Loading(u32),
    Ready(Rc<String>),
    Failed,
}

/// Wait this long (in 10 ms ticks) for a page's stylesheets.
const SHEET_WAIT_TICKS: u64 = 1000;
/// Keep up to this much stylesheet text for the next pages.
const SHEET_CACHE_BYTES: usize = 8 << 20;

/// A page that arrived and is waiting for its stylesheets.
struct PendingPage {
    url: Url,
    html: String,
    security: Option<String>,
    since: u64,
}

fn sheet_key(base: &Url, href: &str) -> Option<String> {
    base.join(href).filter(|u| u.scheme == "http" || u.scheme == "https").map(|u| u.without_fragment())
}

struct Loaded {
    url: Url,
    title: String,
    dom: html::Dom,
    /// all the page's CSS, in order, imports inlined
    css: String,
    cascade: Cascade,
    /// the window width the cascade's media queries were decided for
    cascade_w: i32,
    page: Page,
    images: BTreeMap<String, Pic>,
    /// an image arrived: lay the page out again
    relayout: bool,
    width: i32,
    /// edited values of text fields (by field index)
    values: Vec<(usize, String)>,
    /// https: how the connection was secured
    security: Option<String>,
}

pub struct Browser {
    /// address bar text while editing
    edit: Option<LineEdit>,
    history: Vec<String>,
    forward: Vec<String>,
    loading: Option<(u32, String)>,
    cur: Option<Loaded>,
    error: Option<(String, String)>,
    scroll: i32,
    focus_field: Option<usize>,
    area: Rect,
    pending_fragment: Option<String>,
    /// the address is selected: typing replaces it
    fresh: bool,
    /// the connection details panel is open
    show_security: bool,
    /// the address that failed, while an error shows
    error_url: Option<String>,
    pump_ticks: u64,
    sheets: BTreeMap<String, SheetState>,
    pending: Option<PendingPage>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const PAGE_CSS: &str = "body{margin:0} .wrap{max-width:720px;margin:0 auto;padding:24px}
.logo{font-size:44px;font-weight:bold;color:#b5581b;text-align:center;margin:32px 0 4px}
.tag{text-align:center;color:#6b655e;margin:0 0 20px}
.hit{margin:0 0 20px} .hit a{font-size:19px;text-decoration:none}
.u{color:#2e7d4f;font-size:13px} .s{color:#403b44;font-size:14px}
.muted{color:#77716a;font-size:14px} .count{color:#77716a;font-size:13px;margin:0 0 18px}
h2{font-size:18px;margin:26px 0 8px} .box{background:#f6f1ea;padding:14px 18px;margin:18px 0}
.small{font-size:13px;color:#77716a}";

impl Loaded {
    fn layout(&mut self, width: i32) {
        let (url, images) = (&self.url, &self.images);
        let status = |src: &str| match image_key(url, src).and_then(|k| images.get(&k)) {
            Some(Pic::Broken) => render::ImgStatus::Broken,
            Some(p) => p.size().map_or(render::ImgStatus::Loading, |(w, h)| render::ImgStatus::Ready(w, h)),
            None => render::ImgStatus::Loading,
        };
        let media = render::media_for(width);
        if media.width != self.cascade_w {
            self.cascade = Cascade::new(&self.css, media);
            self.cascade_w = media.width;
        }
        self.page = render::layout_styled(&self.dom, &self.cascade, width, &status);
        self.width = width;
        // images the page uses that we haven't asked for yet
        for src in core::mem::take(&mut self.page.wanted) {
            let Some(key) = image_key(&self.url, &src) else { continue };
            if self.images.contains_key(&key) {
                continue;
            }
            let pic = if key.starts_with("data:") {
                data_uri(&key).map_or(Pic::Broken, |d| decode_pic(&d))
            } else if key.starts_with("file:") {
                Pic::Broken // filled in by show_image
            } else {
                Pic::Queued
            };
            let known = !matches!(pic, Pic::Queued);
            self.images.insert(key, pic);
            self.relayout |= known;
        }
    }

    fn animated(&self) -> bool {
        self.images.values().any(|p| p.animated())
    }

    fn images_left(&self) -> usize {
        self.images.values().filter(|p| matches!(p, Pic::Queued | Pic::Loading(_))).count()
    }
}

impl Browser {
    pub fn new() -> Browser {
        let mut b = Browser { edit: None, history: vec![], forward: vec![], loading: None, cur: None, error: None, scroll: 0, focus_field: None, area: Rect::default(), pending_fragment: None, fresh: false, show_security: false, error_url: None, pump_ticks: 0, sheets: BTreeMap::new(), pending: None };
        b.pending_fragment = None;
        b
    }

    fn current_url(&self) -> String {
        if let (Some(_), Some(u)) = (&self.error, &self.error_url) {
            return u.clone();
        }
        self.cur.as_ref().map(|c| c.url.to_string()).unwrap_or_default()
    }

    /// Open a URL (pushing the current page onto the history).
    fn go(&mut self, sys: &mut Sys, u: &str, push: bool) {
        let Some(url) = Url::parse(u) else { return };
        // same page, other #fragment: just scroll
        if let Some(c) = &self.cur {
            if c.url.without_fragment() == url.without_fragment() && !url.fragment.is_empty() && url.scheme != "hydatek" {
                if push {
                    self.history.push(c.url.to_string());
                    self.forward.clear();
                }
                self.scroll_to(&url.fragment);
                if let Some(c) = &mut self.cur {
                    c.url = url;
                }
                return;
            }
            if push {
                self.history.push(c.url.to_string());
                self.forward.clear();
            }
        }
        self.focus_field = None;
        self.error = None;
        self.pending = None;
        if let Some((id, _)) = self.loading.take() {
            sys.web.stop(id);
        }
        if url.scheme == "hydatek" {
            self.drop_images(sys);
            self.internal(sys, url);
            return;
        }
        let id = sys.web.get(&url.without_fragment());
        self.pending_fragment = Some(url.fragment.clone()).filter(|f| !f.is_empty());
        self.loading = Some((id, url.to_string()));
    }

    fn post(&mut self, sys: &mut Sys, u: &str, body: String) {
        if let Some(c) = &self.cur {
            self.history.push(c.url.to_string());
            self.forward.clear();
        }
        let id = sys.web.post(u, body.into_bytes(), "application/x-www-form-urlencoded");
        self.loading = Some((id, u.to_string()));
    }

    fn show(&mut self, url: Url, html: String) {
        let dom = html::parse(&html);
        let title = dom.title();
        let width = (self.area.w - 2 * PAD).max(200);
        let css = self.assemble_css(&url, &dom);
        let mut c = Loaded { url, title, dom, css, cascade: Cascade::default(), cascade_w: -1, page: Page::default(), images: BTreeMap::new(), relayout: false, width, values: vec![], security: None };
        c.layout(width);
        self.cur = Some(c);
        self.scroll = 0;
        if let Some(f) = self.pending_fragment.take() {
            self.scroll_to(&f);
        }
    }

    /// Show a picture on its own (an image address, or a file).
    fn show_image(&mut self, url: Url, key: String, data: Option<&[u8]>) {
        let name = url::decode(key.rsplit('/').next().unwrap_or("")).split('?').next().unwrap_or("").to_string();
        let pic = data.map_or(Pic::Broken, decode_pic);
        match pic.size() {
            Some((w, h)) => {
                let kind = match &pic {
                    Pic::Vector(..) => " · SVG",
                    p if p.animated() => " · animated",
                    _ => "",
                };
                let title = format!("{} ({} × {}{})", if name.is_empty() { "Image" } else { &name }, w, h, kind);
                let html = format!("<html><head><title>{}</title><style>body{{margin:0;background:#2b2733}}</style></head><body><center><img src=\"{}\"></center></body></html>", esc(&title), esc(&key));
                self.show(url, html);
                if let Some(c) = self.cur.as_mut() {
                    c.images.insert(key, pic);
                    let w = c.width;
                    c.layout(w);
                }
            }
            None => {
                let why = data.map_or("wasn't found", |d| picture::decode(d).err().unwrap_or("can't be shown"));
                self.error = Some((String::from("Can't show this image"), format!("{} {}.", name, why)));
                self.error_url = Some(url.to_string());
            }
        }
    }

    // ---- stylesheets ---------------------------------------------------------------

    /// Ask for a page's stylesheets that aren't cached; true if any are
    /// still on their way.
    fn request_sheets(&mut self, sys: &mut Sys, base: &Url, dom: &html::Dom) -> bool {
        let referer = base.without_fragment();
        for src in css::sources(dom) {
            let href = match src {
                Err((href, media)) if css::media_matches(&media, render::media_for(self.area.w - 2 * PAD)) || media.is_empty() => href,
                Ok(text) => {
                    // @import in a <style> block
                    for (imp, _) in css::imports(&text) {
                        if let Some(k) = sheet_key(base, &imp) {
                            self.want_sheet(sys, k, &referer);
                        }
                    }
                    continue;
                }
                _ => continue,
            };
            if let Some(k) = sheet_key(base, &href) {
                self.want_sheet(sys, k, &referer);
            }
        }
        self.sheets.values().any(|s| matches!(s, SheetState::Loading(_)))
    }

    fn want_sheet(&mut self, sys: &mut Sys, key: String, referer: &str) {
        if !self.sheets.contains_key(&key) {
            let id = sys.web.get_css(&key, referer);
            self.sheets.insert(key, SheetState::Loading(id));
        }
    }

    /// Take finished stylesheets (and ask for what they @import).
    fn pump_sheets(&mut self, sys: &mut Sys) {
        let mut arrived: Vec<(String, Result<crate::web::Response, String>)> = Vec::new();
        for (k, s) in self.sheets.iter() {
            if let SheetState::Loading(id) = s {
                if let Some(r) = sys.web.take(*id) {
                    arrived.push((k.clone(), r));
                }
            }
        }
        for (key, r) in arrived {
            let state = match r {
                Ok(resp) if resp.status < 400 => {
                    let text = String::from_utf8_lossy(&resp.body).into_owned();
                    let base = Url::parse(&key);
                    let abs = match &base {
                        Some(b) => css::absolutize(&text, &|u| b.join(u).map(|x| x.to_string())),
                        None => text,
                    };
                    for (imp, _) in css::imports(&abs) {
                        if let Some(b) = &base {
                            if let Some(k) = sheet_key(b, &imp) {
                                self.want_sheet(sys, k, &key);
                            }
                        }
                    }
                    SheetState::Ready(Rc::new(abs))
                }
                _ => SheetState::Failed,
            };
            self.sheets.insert(key, state);
        }
        // keep the cache in bounds
        let bytes: usize = self.sheets.values().map(|s| if let SheetState::Ready(t) = s { t.len() } else { 0 }).sum();
        if bytes > SHEET_CACHE_BYTES && self.pending.is_none() {
            self.sheets.retain(|_, s| matches!(s, SheetState::Loading(_)));
        }
    }

    /// A sheet's text with its @imports in front (recursively).
    fn expand_sheet(&self, text: &str, depth: u32) -> String {
        let mut out = String::new();
        if depth < 6 {
            for (imp, media) in css::imports(text) {
                if let Some(SheetState::Ready(t)) = self.sheets.get(&imp) {
                    out.push_str(&css::with_media(&self.expand_sheet(t, depth + 1), &media));
                    out.push('\n');
                }
            }
        }
        out.push_str(text);
        out
    }

    /// All the page's CSS in document order: <style> blocks and the
    /// stylesheets we have.
    fn assemble_css(&self, base: &Url, dom: &html::Dom) -> String {
        let mut out = String::new();
        for src in css::sources(dom) {
            match src {
                Ok(text) => {
                    let abs = css::absolutize(&text, &|u| base.join(u).map(|x| x.to_string()));
                    out.push_str(&self.expand_sheet(&abs, 0));
                }
                Err((href, media)) => {
                    if let Some(SheetState::Ready(t)) = sheet_key(base, &href).and_then(|k| self.sheets.get(&k)) {
                        out.push_str(&css::with_media(&self.expand_sheet(t, 0), &media));
                    }
                }
            }
            out.push('\n');
        }
        out
    }

    /// Show the page that was waiting for its styles.
    fn show_pending(&mut self) {
        if let Some(p) = self.pending.take() {
            self.show(p.url, p.html);
            if let Some(c) = self.cur.as_mut() {
                c.security = p.security;
            }
        }
    }

    /// Cancel the current page's image downloads.
    fn drop_images(&mut self, sys: &mut Sys) {
        if let Some(c) = &self.cur {
            for p in c.images.values() {
                if let Pic::Loading(id) = p {
                    sys.web.stop(*id);
                }
            }
        }
    }

    /// Fetch queued images, take finished ones, and relayout when they change.
    fn pump_images(&mut self, sys: &mut Sys) {
        self.pump_ticks += 1;
        let Some(c) = &mut self.cur else { return };
        let mut changed = false;
        let mut active = 0;
        for p in c.images.values_mut() {
            if let Pic::Loading(id) = p {
                match sys.web.take(*id) {
                    Some(Ok(r)) if r.status < 400 => *p = decode_pic(&r.body),
                    Some(_) => *p = Pic::Broken,
                    None => active += 1,
                }
                if !matches!(p, Pic::Loading(_)) {
                    changed = true;
                }
            }
        }
        let referer = c.url.without_fragment();
        for (k, p) in c.images.iter_mut() {
            if active >= MAX_IMAGE_LOADS {
                break;
            }
            if matches!(p, Pic::Queued) {
                *p = Pic::Loading(sys.web.get_image(k, &referer));
                active += 1;
            }
        }
        c.relayout |= changed;
        // lay out again when images arrive, at most a few times a second
        if c.relayout && (active == 0 || self.pump_ticks % 30 == 0) {
            let w = c.width;
            c.layout(w);
            c.relayout = false;
        }
    }

    fn scroll_to(&mut self, frag: &str) {
        if let Some(c) = &self.cur {
            if let Some((_, y)) = c.page.anchors.iter().find(|(id, _)| id == frag) {
                self.scroll = (*y - 8).max(0);
            }
        }
    }

    // ---- built-in pages -------------------------------------------------------------

    fn internal(&mut self, sys: &mut Sys, url: Url) {
        let html = match url.host.as_str() {
            "search" => {
                let q = url.param("q").unwrap_or_default();
                self.results_page(sys, &q)
            }
            "crawl" => {
                if let Some(site) = url.param("url") {
                    let site = url::from_input(&site, engines::by_id(engines::HYDA));
                    if site.scheme == "http" || site.scheme == "https" {
                        sys.search.crawler.add(&site.to_string());
                    }
                }
                self.index_page(sys)
            }
            "forget" => {
                if let Some(h) = url.param("site") {
                    sys.search.index.remove_site(&h);
                }
                self.index_page(sys)
            }
            "index" => self.index_page(sys),
            "view" => {
                let path = url.param("path").unwrap_or_default();
                let data = sys.fs.read(&path);
                self.show_image(url, format!("file:{}", path), data.as_deref());
                return;
            }
            "engine" => {
                if let Some(id) = url.param("id") {
                    sys.search_engine = engines::by_id(&id).id.to_string();
                    sys.reqs.push(crate::sys::Req::SaveSettings);
                }
                self.engines_page(sys)
            }
            "engines" => self.engines_page(sys),
            "open" => {
                if let Some(p) = url.param("path") {
                    sys.reqs.push(crate::sys::Req::OpenPath(p));
                }
                return;
            }
            "about" => format!("<html><head><title>About</title></head><body><div class=wrap><h1>HydatekOS Browser</h1><p>The browser and <b>Hyda Search</b> are part of HydatekOS and written from scratch: the network stack, DNS, HTTP, TLS encryption and certificate checks, the HTML parser, CSS and layout, and the search engine.</p><h2>What works</h2><ul><li>Web pages over HTTP and secure HTTPS (TLS 1.3 and 1.2, with certificates checked against Mozilla's list of authorities), with links, forms, headings, lists, simple tables, colours and text styles from CSS</li><li>Hyda Search: search the pages you visit, sites you add, and your files</li></ul><h2>Not yet</h2><ul><li>Images, JavaScript, fonts from the web, and external stylesheets</li></ul><p><a href=\"hydatek://start\">Back to Hyda Search</a></p></div></body></html>"),
            _ => self.start_page(sys),
        };
        self.show(url, html.replace("</head>", &format!("<style>{}</style></head>", PAGE_CSS)));
    }

    fn search_box(q: &str) -> String {
        format!(
            "<form action=\"hydatek://search\" method=get><input type=text name=q size=48 value=\"{}\"> <input type=submit value=\"Search\"></form>",
            esc(q)
        )
    }

    fn start_page(&self, sys: &Sys) -> String {
        let ix = &sys.search.index;
        let sites = ix.sites();
        let mut recent = String::new();
        for (site, n) in sites.iter().take(12) {
            recent.push_str(&format!("<li>{} <span class=small>· {} page{}</span></li>", esc(site), n, if *n == 1 { "" } else { "s" }));
        }
        format!(
            "<html><head><title>Hyda Search</title></head><body><div class=wrap><p class=logo>Hyda Search</p><p class=tag>Search the web pages you visit, the sites you add, and your files. Private: it all stays on this computer.</p><center>{}</center><div class=box><b>{} pages</b> from <b>{} sites</b> are in your index.<br><span class=small>Add a site and Hyda Search reads it for you (up to {} pages each, politely, following the site's rules for crawlers).</span><form action=\"hydatek://crawl\"><input type=text name=url size=36 value=\"\"> <input type=submit value=\"Add a site\"></form></div>{}<p class=small>The address bar searches with <b>{}</b> · <a href=\"hydatek://engines\">Search engines</a> · <a href=\"hydatek://index\">Manage your index</a> · <a href=\"hydatek://about\">About this browser</a></p></div></body></html>",
            Self::search_box(""),
            ix.len(),
            sites.len(),
            crate::web::search::MAX_PAGES_PER_SITE,
            if recent.is_empty() { String::new() } else { format!("<h2>In your index</h2><ul>{}</ul>", recent) },
            esc(engines::by_id(&sys.search_engine).name)
        )
    }

    fn results_page(&self, sys: &Sys, q: &str) -> String {
        let hits = sys.search.index.search(q, 30);
        let mut body = String::new();
        for h in &hits {
            let mut snip = String::new();
            let mut last = 0;
            for &(a, b) in &h.marks {
                if a < last || b > h.snippet.len() {
                    continue;
                }
                snip.push_str(&esc(&h.snippet[last..a]));
                snip.push_str(&format!("<b>{}</b>", esc(&h.snippet[a..b])));
                last = b;
            }
            snip.push_str(&esc(&h.snippet[last.min(h.snippet.len())..]));
            let (href, shown) = match h.url.strip_prefix("file://") {
                Some(p) => (format!("hydatek://open?path={}", url::encode(p)), format!("Your files › {}", p.trim_start_matches("/home/"))),
                None => (h.url.clone(), h.url.clone()),
            };
            body.push_str(&format!("<div class=hit><a href=\"{}\">{}</a><br><span class=u>{}</span><br><span class=s>{}</span></div>", esc(&href), esc(&h.title), esc(&shown), snip));
        }
        let none = if hits.is_empty() {
            format!(
                "<p>No pages in your index match <b>{}</b>.</p><p class=muted>Hyda Search only knows the pages you've visited, the sites you've added and your files. Add a site where the answer might be:</p><form action=\"hydatek://crawl\"><input type=text name=url size=36> <input type=submit value=\"Add a site\"></form>",
                esc(q)
            )
        } else {
            String::new()
        };
        format!(
            "<html><head><title>{} - Hyda Search</title></head><body><div class=wrap><p style=\"font-size:24px;font-weight:bold;color:#b5581b;margin:4px 0 12px\">Hyda Search</p>{}<p class=count>{} result{} from {} pages</p><p class=small>Search the web for this: {}</p>{}{}</div></body></html>",
            esc(q),
            Self::search_box(q),
            hits.len(),
            if hits.len() == 1 { "" } else { "s" },
            sys.search.index.len(),
            Self::web_links(q),
            body,
            none
        )
    }

    /// Links that run `q` on each web search engine.
    fn web_links(q: &str) -> String {
        engines::ENGINES
            .iter()
            .filter(|e| e.id != engines::HYDA)
            .map(|e| format!("<a href=\"{}\">{}</a>", esc(&engines::search_url(e, q)), esc(e.name)))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn engines_page(&self, sys: &Sys) -> String {
        let mut rows = String::new();
        for e in engines::ENGINES {
            let current = sys.search_engine == e.id;
            let choose = if current { String::from("<b>Your search engine</b>") } else { format!("<a href=\"hydatek://engine?id={}\">Use this</a>", e.id) };
            rows.push_str(&format!(
                "<tr><td><b>{}</b><br><span class=small>{}</span></td><td>!{}</td><td>{}</td></tr>",
                esc(e.name),
                esc(e.about),
                e.key,
                choose
            ));
        }
        format!(
            "<html><head><title>Search engines</title></head><body><div class=wrap><h1>Search engines</h1><p>What you type in the address bar that isn't an address is searched with <b>{}</b>. Pick another below, or in Settings › Browser.</p><p class=muted>To use a different one just once, add its shortcut: <b>!d jollof rice</b> searches DuckDuckGo, <b>lagos weather !w</b> searches Wikipedia. A shortcut on its own opens the engine.</p><table><tr><th>Engine</th><th>Shortcut</th><th></th></tr>{}</table><p class=small>Web search engines are other companies' services: what you search is sent to them. Hyda Search stays on this computer. <a href=\"hydatek://start\">Back to Hyda Search</a></p></div></body></html>",
            esc(engines::by_id(&sys.search_engine).name),
            rows
        )
    }

    fn index_page(&self, sys: &Sys) -> String {
        let mut rows = String::new();
        for (site, n) in sys.search.index.sites() {
            let forget = if site == "Your files" { String::new() } else { format!(" · <a href=\"hydatek://forget?site={}\">remove</a>", url::encode(&site)) };
            rows.push_str(&format!("<li><b>{}</b> <span class=small>{} page{}{}</span></li>", esc(&site), n, if n == 1 { "" } else { "s" }, forget));
        }
        let c = &sys.search.crawler;
        let status = if c.busy() { format!("<p class=box>Reading… {} pages waiting. {}</p>", c.queued(), esc(&c.status)) } else if !c.status.is_empty() { format!("<p class=small>{}</p>", esc(&c.status)) } else { String::new() };
        format!(
            "<html><head><title>Your index - Hyda Search</title></head><body><div class=wrap><h1>Your search index</h1>{}<form action=\"hydatek://crawl\"><input type=text name=url size=36> <input type=submit value=\"Add a site\"></form><ul>{}</ul><p class=small>Reload this page to see the crawler's progress. <a href=\"hydatek://start\">Back to Hyda Search</a></p></div></body></html>",
            status, rows
        )
    }

    // ---- forms ----------------------------------------------------------------------

    fn field_value(&self, i: usize) -> String {
        let Some(c) = &self.cur else { return String::new() };
        if let Some((_, v)) = c.values.iter().find(|(k, _)| *k == i) {
            return v.clone();
        }
        match &c.page.fields[i].ctl {
            Control::Text { value, .. } => value.clone(),
            _ => String::new(),
        }
    }

    fn submit(&mut self, sys: &mut Sys, field: usize) {
        let Some(c) = &self.cur else { return };
        let form = c.page.fields[field].form;
        let mut pairs: Vec<(String, String)> = Vec::new();
        for (i, f) in c.page.fields.iter().enumerate() {
            if f.form != form {
                continue;
            }
            match &f.ctl {
                Control::Text { name, .. } if !name.is_empty() => pairs.push((name.clone(), self.field_value(i))),
                Control::Hidden { name, value } if !name.is_empty() => pairs.push((name.clone(), value.clone())),
                Control::Check { name, value, on } if *on && !name.is_empty() => pairs.push((name.clone(), value.clone())),
                Control::Submit { name, value } if i == field && !name.is_empty() => pairs.push((name.clone(), value.clone())),
                _ => {}
            }
        }
        let qs: Vec<String> = pairs.iter().map(|(k, v)| format!("{}={}", url::encode(k), url::encode(v))).collect();
        let qs = qs.join("&");
        let f = &c.page.forms[form];
        let action = c.url.join(&f.action).unwrap_or_else(|| c.url.clone());
        if f.post {
            let target = action.to_string();
            self.post(sys, &target, qs);
        } else {
            let mut a = action;
            a.fragment.clear();
            let base = a.path.split('?').next().unwrap_or("/").to_string();
            a.path = format!("{}?{}", base, qs);
            let target = a.to_string();
            self.go(sys, &target, true);
        }
    }

    // ---- drawing ---------------------------------------------------------------------

    /// The connection details panel under the address bar.
    fn draw_security(&self, ui: &mut Ui, at: Rect) {
        let t = ui.t;
        let Some(c) = &self.cur else { return };
        let (title, mut lines) = match &c.security {
            Some(sec) => {
                let mut v: Vec<String> = sec.split(" · ").map(|p| p.to_string()).collect();
                v.push(String::from("HydatekOS checked the site's certificate against its list of trusted authorities, so this is the real site, and what you send and receive is encrypted."));
                ("Connection is secure", v)
            }
            None => ("Connection is not secure", vec![String::from("This page came over plain HTTP. Others on the network could read or change it; don't enter passwords or card numbers here.")]),
        };
        let w = at.w;
        let mut wrapped = Vec::new();
        for l in lines.drain(..) {
            wrapped.extend(ui.wrap(Face::Regular, 13, &l, w - 32));
        }
        let h = 52 + wrapped.len() as i32 * 20 + 12;
        let panel = Rect::new(at.x, at.y, w, h);
        ui.shadow(panel, 10, 16, 4, 60);
        ui.rrect(panel, 10, t.surface);
        let icon = if c.security.is_some() { Icon::Lock } else { Icon::Info };
        ui.icon(icon, panel.x + 16, panel.y + 16, 18, if c.security.is_some() { t.accent } else { t.text2 });
        ui.text(panel.x + 44, panel.y + 30, Face::Semibold, 15, title, t.text);
        let mut y = panel.y + 58;
        for l in wrapped {
            ui.text(panel.x + 16, y, Face::Regular, 13, &l, t.text2);
            y += 20;
        }
    }

    fn draw_page(&mut self, ui: &mut Ui, area: Rect, inst: u32) {
        let Some(c) = &mut self.cur else { return };
        let width = (area.w - 2 * PAD).max(200);
        if width != c.width {
            // the window was resized: lay the page out again
            c.layout(width);
        }
        let (page, images, page_url) = (&c.page, &mut c.images, &c.url);
        let ms = ui.ticks * 10;
        let max = (page.height - area.h + PAD * 2).max(0);
        self.scroll = self.scroll.clamp(0, max);
        ui.rect(area, Color::rgb(page.bg));
        let old = ui.clip_in(area);
        let (ox, oy) = (area.x + PAD, area.y + PAD - self.scroll);
        let s = ui.s;
        let visible = |y: i32, h: i32| oy + y + h >= area.y && oy + y <= area.b();
        for it in &page.items {
            match it {
                Item::Rect { x, y, w, h, color } if visible(*y, *h) => ui.rect(Rect::new(ox + x, oy + y, *w, *h), Color::rgb(*color)),
                Item::Frame { x, y, w, h, color } if visible(*y, *h) => ui.stroke(Rect::new(ox + x, oy + y, *w, *h), 0, 1, Color::rgb(*color)),
                Item::Image { x, y, w, h, img } if visible(*y, *h) => match image_key(page_url, &page.images[*img]).and_then(|k| images.get_mut(&k)) {
                    Some(p @ (Pic::Ready(..) | Pic::Vector(..))) => {
                        let (dw, dh) = (w * s, h * s);
                        if let Some(px) = p.pixels(dw, dh, ms) {
                            ui.c.blend_argb(px, dw, dh, (ox + x) * s, (oy + y) * s);
                        }
                    }
                    _ => ui.rect(Rect::new(ox + x, oy + y, *w, *h), Color::rgb(0xece6dd)),
                },
                Item::Background { x, y, w, h, img, size, pos, repeat } if visible(*y, *h) => {
                    let Some(p) = image_key(page_url, &page.images[*img]).and_then(|k| images.get_mut(&k)) else { continue };
                    let Some(nat) = p.size() else { continue };
                    let (tw, th, offx, offy) = bg_tiles(nat, *w, *h, *size, *pos);
                    let bx = Rect::new(ox + x, oy + y, *w, *h);
                    let clip = ui.clip_in(bx);
                    // tile positions across and down (one each without repeat)
                    let first = |off: i32, t: i32, on: bool| if on { off - ((off + t - 1).div_euclid(t)) * t } else { off };
                    let (fx, fy) = (first(offx, tw, repeat.0), first(offy, th, repeat.1));
                    let (nx, ny) = (if repeat.0 { (*w - fx + tw - 1) / tw } else { 1 }, if repeat.1 { (*h - fy + th - 1) / th } else { 1 });
                    if nx * ny <= 4000 {
                        if let Some(px) = p.pixels(tw * s, th * s, ms) {
                            for j in 0..ny {
                                for i in 0..nx {
                                    ui.c.blend_argb(px, tw * s, th * s, (bx.x + fx + i * tw) * s, (bx.y + fy + j * th) * s);
                                }
                            }
                        }
                    }
                    ui.set_clip(clip);
                }
                Item::Text { x, y, size, face, color, text, underline, strike } if visible(*y - size, size + 4) => {
                    let w = crate::font::draw(ui.c, (ox + x) * s, (oy + y) * s, *face, size * s, text, Color::rgb(*color)) / s;
                    if *underline {
                        ui.rect(Rect::new(ox + x, oy + y + 2, w, 1), Color::rgb(*color));
                    }
                    if *strike {
                        ui.rect(Rect::new(ox + x, oy + y - size * 3 / 10, w, 1), Color::rgb(*color));
                    }
                }
                _ => {}
            }
        }
        ui.zone(area, Action::App(inst, C_PAGE));
        // links
        for (k, &(x, y, w, h, _)) in page.links.iter().enumerate() {
            if visible(y, h) {
                let a = Action::App(inst, C_LINK + k as u32);
                if ui.hot(a) {
                    ui.rect(Rect::new(ox + x, oy + y + h - 3, w, 1), Color::rgb(render::LINK_COLOR));
                }
                ui.zone(Rect::new(ox + x, oy + y, w, h), a);
            }
        }
        // form controls
        let t = ui.t;
        for (i, f) in page.fields.iter().enumerate() {
            if f.w == 0 || !visible(f.y, f.h) {
                continue;
            }
            let r = Rect::new(ox + f.x, oy + f.y, f.w, f.h);
            let a = Action::App(inst, C_FIELD + i as u32);
            match &f.ctl {
                Control::Text { password, .. } => {
                    let focused = self.focus_field == Some(i);
                    ui.rrect(r, 6, Color::rgb(0xffffff));
                    ui.stroke(r, 6, if focused { 2 } else { 1 }, if focused { t.accent } else { Color::rgb(0xb9b2a8) });
                    let v = c.values.iter().find(|(k, _)| *k == i).map(|x| x.1.clone()).unwrap_or_else(|| match &f.ctl {
                        Control::Text { value, .. } => value.clone(),
                        _ => String::new(),
                    });
                    let shown = if *password { "•".repeat(v.chars().count()) } else { v };
                    let inner = Rect::new(r.x + 8, r.y, r.w - 16, r.h.min(34));
                    let clip = ui.clip_in(inner);
                    let w = ui.text_in(inner, Face::Regular, 14, &shown, Color::rgb(0x1e1b2c), 0);
                    if focused && (ui.ticks / 50) % 2 == 0 {
                        ui.rect(Rect::new(inner.x + w + 1, r.y + 6, 1, inner.h - 12), Color::rgb(0x1e1b2c));
                    }
                    ui.set_clip(clip);
                }
                Control::Submit { value, .. } => {
                    let hot = ui.hot(a);
                    match f.paint {
                        // the page's own button colours
                        Some((bg, fg, border)) => {
                            if let Some(c) = bg {
                                ui.rrect(r, 6, if hot { Color::rgb(c).mix(Color::rgb(0x000000), 20) } else { Color::rgb(c) });
                            }
                            if let Some(c) = border {
                                ui.stroke(r, 6, 1, Color::rgb(c));
                            }
                            ui.text_in(r, Face::Semibold, 14, value, Color::rgb(fg), 1);
                        }
                        None => {
                            ui.rrect(r, 8, if hot { t.accent.mix(Color::rgb(0x000000), 20) } else { t.accent });
                            ui.text_in(r, Face::Semibold, 14, value, t.on_accent, 1);
                        }
                    }
                }
                Control::Check { .. } => {
                    ui.rrect(r, 4, Color::rgb(0xffffff));
                    ui.stroke(r, 4, 1, Color::rgb(0x8a847c));
                }
                Control::Hidden { .. } => {}
            }
            ui.zone(r, a);
        }
        ui.set_clip(old);
        // scroll bar
        if max > 0 {
            let track = area.h - 8;
            let th = (track * area.h / page.height.max(1)).max(30);
            let ty = area.y + 4 + (track - th) * self.scroll / max;
            ui.rrect(Rect::new(area.r() - 7, ty, 4, th), 2, Color::rgb(0x000000).with_alpha(60));
        }
    }
}

impl App for Browser {
    fn kind(&self) -> AppKind {
        AppKind::Browser
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        // toolbar: back, forward, reload/stop, address, home
        let by = r.y + 8;
        ui.icon_button(Rect::new(r.x + 12, by, 28, 28), Icon::ChevronLeft, Action::App(inst, C_BACK), 16);
        ui.icon_button(Rect::new(r.x + 42, by, 28, 28), Icon::ChevronRight, Action::App(inst, C_FWD), 16);
        let reload_icon = if self.loading.is_some() || self.pending.is_some() { Icon::Close } else { Icon::Redo };
        ui.icon_button(Rect::new(r.x + 72, by, 28, 28), reload_icon, Action::App(inst, C_RELOAD), 15);
        let mut bar = Rect::new(r.x + 108, by, r.w - 108 - 158, 28);
        let scheme_icon = match (&self.edit, &self.loading, if self.error.is_some() { None } else { self.cur.as_ref() }) {
            (None, None, Some(c)) if c.url.scheme == "https" => Some((Icon::Lock, t.text2)),
            (None, None, Some(c)) if c.url.scheme == "http" => Some((Icon::Info, t.text3)),
            _ => None,
        };
        if let Some((icon, color)) = scheme_icon {
            let z = Rect::new(bar.x, by, 28, 28);
            ui.zone(z, Action::App(inst, C_LOCK));
            if ui.hover == Some(Action::App(inst, C_LOCK)) || self.show_security {
                ui.rrect(z, 6, t.hover);
            }
            ui.icon(icon, z.x + 6, z.y + 6, 16, color);
            bar.x += 32;
            bar.w -= 32;
        }
        let (text, focus) = match &self.edit {
            Some(e) => (e.text.clone(), true),
            None => match &self.loading {
                Some((_, u)) => (u.clone(), false),
                None => (self.current_url(), false),
            },
        };
        let shown = if focus || !text.starts_with("hydatek://start") { text } else { String::new() };
        let hint = format!("Search with {} or type an address", engines::by_id(&sys.search_engine).name);
        match &self.edit {
            Some(e) => ui.line(bar, e, &hint, true, Action::App(inst, C_URL)),
            None => ui.field(bar, &shown, &hint, false, Action::App(inst, C_URL)),
        }
        if focus && self.fresh && !shown.is_empty() {
            // show the address as selected
            let w = ui.tw(Face::Regular, 13, &shown).min(bar.w - 24);
            ui.rect(Rect::new(bar.x + 11, bar.y + 6, w + 2, bar.h - 12), t.accent.with_alpha(60));
        }
        ui.icon_button(Rect::new(bar.r() + 6, by, 28, 28), Icon::Home, Action::App(inst, C_HOME), 16);
        // progress line under the toolbar
        let line_y = r.y + HEADER;
        ui.rect(Rect::new(r.x, line_y, r.w, 1), t.line);
        if let Some((id, _)) = &self.loading {
            let frac = match sys.web.progress.get(id) {
                Some(Progress::Resolving) => 8,
                Some(Progress::Connecting) => 18,
                Some(Progress::Securing) => 28,
                Some(Progress::Waiting) => 38,
                Some(Progress::Loading(got, Some(total))) if *total > 0 => 40 + (60 * *got / *total) as i32,
                Some(Progress::Loading(_, _)) => 70,
                None => 90,
            };
            ui.rect(Rect::new(r.x, line_y - 1, r.w * frac / 100, 3), t.accent);
        } else if self.pending.is_some() {
            ui.rect(Rect::new(r.x, line_y - 1, r.w * 95 / 100, 3), t.accent);
        }
        let area = Rect::new(r.x, line_y + 1, r.w, r.h - HEADER - 1 - STATUS);
        self.area = area;
        if let Some((title, detail)) = &self.error {
            ui.rect(area, t.surface);
            let icon = if title.contains("isn't private") { Icon::Lock } else { Icon::Globe };
            ui.icon(icon, area.x + area.w / 2 - 20, area.y + 60, 40, t.text3);
            let w = ui.tw(Face::Semibold, 20, title);
            ui.text(area.x + (area.w - w) / 2, area.y + 140, Face::Semibold, 20, title, t.text);
            let mut y = area.y + 172;
            for line in ui.wrap(Face::Regular, 14, detail, (area.w - 80).min(560)) {
                let lw = ui.tw(Face::Regular, 14, &line);
                ui.text(area.x + (area.w - lw) / 2, y, Face::Regular, 14, &line, t.text2);
                y += 22;
            }
        } else if self.cur.is_some() {
            self.draw_page(ui, area, inst);
        } else {
            ui.rect(area, t.surface);
        }
        if self.show_security && self.error.is_none() {
            self.draw_security(ui, Rect::new(r.x + 108, line_y + 4, 380, 0));
        }
        // status bar: the hovered link, or the page title
        let sy = r.b() - STATUS;
        ui.rect(Rect::new(r.x, sy, r.w, STATUS), t.surface);
        ui.rect(Rect::new(r.x, sy, r.w, 1), t.line);
        let mut status = String::new();
        if let (Some(c), Some(Action::App(i, code))) = (&self.cur, ui.hover) {
            if i == inst && code >= C_LINK && code < C_FIELD {
                if let Some(&(_, _, _, _, hi)) = c.page.links.get((code - C_LINK) as usize) {
                    let href = &c.page.hrefs[hi];
                    status = c.url.join(href).map(|u| u.to_string()).unwrap_or_else(|| href.clone());
                }
            }
        }
        if status.is_empty() {
            status = match (&self.loading, &self.cur) {
                (Some((id, u)), _) => match sys.web.progress.get(id) {
                    Some(Progress::Resolving) => format!("Looking up {}…", Url::parse(u).map(|x| x.host).unwrap_or_default()),
                    Some(Progress::Connecting) => String::from("Connecting…"),
                    Some(Progress::Securing) => String::from("Setting up a secure connection…"),
                    Some(Progress::Loading(got, _)) => format!("Loading… {} KB", got / 1024),
                    _ => String::from("Waiting for the site…"),
                },
                (None, Some(c)) if ui.hover == Some(Action::App(inst, C_LOCK)) => match &c.security {
                    Some(s) => format!("Secure connection: {}", s),
                    None if c.url.scheme == "http" => String::from("Not secure: this page came over plain HTTP, so others on the network could read or change it"),
                    None => String::new(),
                },
                (None, _) if self.error.is_some() => String::new(),
                (None, _) if self.pending.is_some() => {
                    let n = self.sheets.values().filter(|s| matches!(s, SheetState::Loading(_))).count();
                    format!("Loading the page's styles… {} to go", n)
                }
                (None, Some(c)) if c.images_left() > 0 => format!("Loading images… {} to go", c.images_left()),
                (None, Some(c)) => c.title.clone(),
                _ => String::new(),
            };
        }
        let status = ui.fit(Face::Regular, 12, &status, r.w - 32);
        ui.text(r.x + 16, sy + 17, Face::Regular, 12, &status, t.text2);
    }

    fn tick(&mut self, sys: &mut Sys) {
        if self.cur.is_none() && self.loading.is_none() && self.error.is_none() {
            self.go(sys, "hydatek://start", false);
        }
        self.pump_images(sys);
        self.pump_sheets(sys);
        if let Some(p) = &self.pending {
            let waiting = self.sheets.values().any(|s| matches!(s, SheetState::Loading(_)));
            if !waiting || self.pump_ticks > p.since + SHEET_WAIT_TICKS {
                self.show_pending();
            }
        }
        let Some((id, requested)) = self.loading.clone() else { return };
        let Some(res) = sys.web.take(id) else { return };
        self.loading = None;
        self.drop_images(sys);
        match res {
            Ok(resp) => {
                let url = Url::parse(&resp.url).or_else(|| Url::parse(&requested)).unwrap();
                let mut url = url;
                if let Some(f) = &self.pending_fragment {
                    url.fragment = f.clone();
                }
                let ctype = resp.content_type();
                let text = match core::str::from_utf8(&resp.body) {
                    Ok(s) => s.to_string(),
                    // not UTF-8: read it as Latin-1
                    Err(_) => resp.body.iter().map(|&b| b as char).collect(),
                };
                if resp.status >= 400 && text.trim().is_empty() {
                    self.error = Some((format!("The site answered {}", resp.status), format!("{} couldn't show this page.", url.host)));
                    self.error_url = Some(requested.clone());
                    return;
                }
                let sniffed = picture::sniff(&resp.body);
                let is_picture = ctype.starts_with("image/") || matches!(sniffed, Some(picture::Format::Png | picture::Format::Jpeg | picture::Format::Gif | picture::Format::Bmp | picture::Format::Webp));
                if is_picture {
                    let key = image_key(&url, &url.to_string()).unwrap_or_else(|| url.to_string());
                    let security = resp.security.clone();
                    self.show_image(url, key, Some(&resp.body));
                    if let Some(c) = self.cur.as_mut() {
                        c.security = security;
                    }
                } else if ctype.contains("html") || ctype.is_empty() && text.trim_start().starts_with('<') {
                    let dom = html::parse(&text);
                    sys.search.visited(&url.without_fragment(), &dom);
                    // wait (a while) for the page's stylesheets before showing it
                    let waiting = self.request_sheets(sys, &url, &dom);
                    self.pending = Some(PendingPage { url, html: text, security: resp.security.clone(), since: self.pump_ticks });
                    if !waiting {
                        self.show_pending();
                    }
                } else if ctype.starts_with("text/") || ctype.contains("json") || ctype.contains("xml") {
                    let html = format!("<html><head><title>{}</title></head><body><pre>{}</pre></body></html>", esc(&url.to_string()), esc(&text));
                    self.show(url, html);
                    if let Some(c) = self.cur.as_mut() {
                        c.security = resp.security.clone();
                    }
                } else {
                    // anything else is a download
                    let name = url.path.split('?').next().unwrap_or("").rsplit('/').next().unwrap_or("").to_string();
                    let name = crate::link::sanitize(if name.is_empty() { "download" } else { &name });
                    let (stem, ext) = match name.rfind('.') {
                        Some(k) if k > 0 => (name[..k].to_string(), name[k..].to_string()),
                        _ => (name.clone(), String::new()),
                    };
                    let path = sys.fs.unique("/home/Downloads", &stem, &ext);
                    sys.fs.write(&path, &resp.body);
                    sys.toast("Browser", &format!("Downloaded {} to Downloads", crate::fs::basename(&path)));
                    if let Some(prev) = self.history.pop() {
                        let _ = prev;
                    }
                }
            }
            Err(e) => {
                let host = Url::parse(&requested).map(|u| u.host).unwrap_or_default();
                let title = if e.contains("certificate") || e.contains("trust") || e.contains("pretending") {
                    format!("Your connection to {} isn't private", host)
                } else {
                    format!("Can't open {}", host)
                };
                self.error = Some((title, e));
                self.error_url = Some(requested.clone());
            }
        }
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        if code != C_URL {
            self.edit = None;
        }
        self.show_security = code == C_LOCK && !self.show_security;
        if !(C_FIELD..C_FIELD + 10_000).contains(&code) {
            self.focus_field = None;
        }
        match code {
            C_URL => {
                if self.edit.is_none() {
                    let cur = self.current_url();
                    let text = if cur.starts_with("hydatek://start") { String::new() } else { cur };
                    self.fresh = !text.is_empty();
                    self.edit = Some(LineEdit::new(text));
                }
            }
            C_BACK => {
                if let Some(u) = self.history.pop() {
                    let cur = self.current_url();
                    if !cur.is_empty() {
                        self.forward.push(cur);
                    }
                    self.go(sys, &u, false);
                }
            }
            C_FWD => {
                if let Some(u) = self.forward.pop() {
                    let cur = self.current_url();
                    if !cur.is_empty() {
                        self.history.push(cur);
                    }
                    self.go(sys, &u, false);
                }
            }
            C_RELOAD => {
                if let Some((id, _)) = self.loading.take() {
                    sys.web.stop(id);
                } else if self.pending.is_some() {
                    // stop waiting for styles: show the page as it is
                    self.show_pending();
                } else {
                    let u = self.current_url();
                    if !u.is_empty() {
                        let keep = self.scroll;
                        self.go(sys, &u, false);
                        self.scroll = keep;
                    }
                }
            }
            C_HOME => self.go(sys, "hydatek://start", true),
            c if (C_LINK..C_FIELD).contains(&c) => {
                let target = self.cur.as_ref().and_then(|cur| {
                    let (_, _, _, _, hi) = *cur.page.links.get((c - C_LINK) as usize)?;
                    let href = cur.page.hrefs.get(hi)?.clone();
                    Some((cur.url.clone(), href))
                });
                if let Some((base, href)) = target {
                    let h = href.trim().to_ascii_lowercase();
                    if h.starts_with("javascript:") {
                        return;
                    }
                    if h.starts_with("mailto:") {
                        sys.toast("Browser", "Email links open in Mail once accounts arrive");
                        return;
                    }
                    if let Some(u) = base.join(&href) {
                        let s = u.to_string();
                        self.go(sys, &s, true);
                    }
                }
            }
            c if c >= C_FIELD => {
                let i = (c - C_FIELD) as usize;
                let ctl = self.cur.as_ref().and_then(|cur| cur.page.fields.get(i)).map(|f| f.ctl.clone());
                match ctl {
                    Some(Control::Text { .. }) => self.focus_field = Some(i),
                    Some(Control::Submit { .. }) => self.submit(sys, i),
                    Some(Control::Check { .. }) => {
                        if let Some(cur) = &mut self.cur {
                            if let Control::Check { on, .. } = &mut cur.page.fields[i].ctl {
                                *on = !*on;
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, gen: bool, sys: &mut Sys) {
        if k == Key::F(5) {
            return self.action(C_RELOAD, false, sys);
        }
        if gen && matches!(k, Key::Char('l') | Key::Char('r')) {
            return self.action(if k == Key::Char('l') { C_URL } else { C_RELOAD }, false, sys);
        }
        if let Some(e) = &mut self.edit {
            if self.fresh {
                // the whole address is selected: typing, pasting or deleting replaces it
                self.fresh = false;
                match k {
                    Key::Char(c) if !c.is_control() && (!gen || c == 'v' || c == 'x') => e.clear(),
                    Key::Backspace | Key::Delete => {
                        e.clear();
                        return;
                    }
                    Key::Left | Key::Home => e.home(),
                    _ => {}
                }
                if matches!(k, Key::Left | Key::Home) {
                    return;
                }
            }
            match k {
                Key::Enter => {
                    let t = e.text.clone();
                    self.edit = None;
                    if !t.trim().is_empty() {
                        let u = url::from_input(&t, engines::by_id(&sys.search_engine)).to_string();
                        self.go(sys, &u, true);
                    }
                }
                Key::Esc => self.edit = None,
                _ => {
                    e.key_sys(k, gen, sys);
                }
            }
            return;
        }
        if gen && self.focus_field.is_none() {
            // Gen+← / Gen+→ (and Aux, as elsewhere): back and forward
            match k {
                Key::Left => self.action(C_BACK, false, sys),
                Key::Right => self.action(C_FWD, false, sys),
                _ => {}
            }
            return;
        }
        if let Some(i) = self.focus_field {
            let v = self.field_value(i);
            let mut v = v;
            match k {
                Key::Enter => {
                    self.focus_field = None;
                    // submit the form this field belongs to (with its first button)
                    let form = self.cur.as_ref().map(|c| c.page.fields[i].form).unwrap_or(0);
                    let btn = self.cur.as_ref().and_then(|c| c.page.fields.iter().position(|f| f.form == form && matches!(f.ctl, Control::Submit { .. })));
                    self.submit(sys, btn.unwrap_or(i));
                    return;
                }
                Key::Esc => {
                    self.focus_field = None;
                    return;
                }
                Key::Backspace => {
                    v.pop();
                }
                Key::Char('v') if gen => v.extend(sys.clipboard.chars().filter(|c| !c.is_control())),
                Key::Char(ch) if !gen && !ch.is_control() => v.push(ch),
                _ => return,
            }
            if let Some(c) = &mut self.cur {
                c.values.retain(|(k, _)| *k != i);
                c.values.push((i, v));
            }
            return;
        }
        match k {
            Key::Down => self.scroll += 48,
            Key::Up => self.scroll -= 48,
            // Space pages down, Shift+Space up
            Key::Char(' ') if crate::input::shift() => self.scroll -= self.area.h - 60,
            Key::PageDown | Key::Char(' ') => self.scroll += self.area.h - 60,
            Key::PageUp => self.scroll -= self.area.h - 60,
            Key::Home => self.scroll = 0,
            Key::End => self.scroll = i32::MAX / 2,
            Key::Backspace => self.action(C_BACK, false, sys),
            Key::Char(c) if !c.is_control() => {
                // typing on a page starts an address/search
                let mut e = LineEdit::default();
                e.text.push(c);
                self.edit = Some(e);
            }
            _ => {}
        }
    }

    fn scroll(&mut self, dy: i32) {
        self.scroll += dy * 60;
        if self.scroll < 0 {
            self.scroll = 0;
        }
    }

    fn open_path(&mut self, p: &str, sys: &mut Sys) {
        // a HydatekOS page (Hyda Search from the top bar), or a file to view
        let u = if p.starts_with("hydatek://") { String::from(p) } else { format!("hydatek://view?path={}", url::encode(p)) };
        self.go(sys, &u, true);
    }

    fn animating(&self) -> bool {
        self.edit.is_some() || self.loading.is_some() || self.pending.is_some() || self.focus_field.is_some() || self.cur.as_ref().is_some_and(|c| c.relayout || c.images_left() > 0 || (self.error.is_none() && c.animated()))
    }
}
