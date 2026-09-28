//! Browser: HydatekOS's web browser. Pages are fetched by the HydatekOS network
//! stack (web/fetch.rs), parsed and laid out by its own engine (web/html.rs,
//! web/render.rs), and searches go to Hyda Search, the built-in search engine.

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::gfx::{Color, Rect};
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Action, Key, Ui};
use crate::web::html;
use crate::web::render::{self, Control, Item, Page};
use crate::web::url::{self, Url};
use crate::web::Progress;
use alloc::format;
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

struct Loaded {
    url: Url,
    title: String,
    html: String,
    page: Page,
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

impl Browser {
    pub fn new() -> Browser {
        let mut b = Browser { edit: None, history: vec![], forward: vec![], loading: None, cur: None, error: None, scroll: 0, focus_field: None, area: Rect::default(), pending_fragment: None, fresh: false, show_security: false, error_url: None };
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
        if let Some((id, _)) = self.loading.take() {
            sys.web.stop(id);
        }
        if url.scheme == "hydatek" {
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
        let page = render::layout(&dom, "", width);
        self.cur = Some(Loaded { url, title, html, page, width, values: vec![], security: None });
        self.scroll = 0;
        if let Some(f) = self.pending_fragment.take() {
            self.scroll_to(&f);
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
                    let site = url::from_input(&site);
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
            "<html><head><title>Hyda Search</title></head><body><div class=wrap><p class=logo>Hyda Search</p><p class=tag>Search the web pages you visit, the sites you add, and your files. Private: it all stays on this computer.</p><center>{}</center><div class=box><b>{} pages</b> from <b>{} sites</b> are in your index.<br><span class=small>Add a site and Hyda Search reads it for you (up to {} pages each, politely, following the site's rules for crawlers).</span><form action=\"hydatek://crawl\"><input type=text name=url size=36 value=\"\"> <input type=submit value=\"Add a site\"></form></div>{}<p class=small><a href=\"hydatek://index\">Manage your index</a> · <a href=\"hydatek://about\">About this browser</a></p></div></body></html>",
            Self::search_box(""),
            ix.len(),
            sites.len(),
            crate::web::search::MAX_PAGES_PER_SITE,
            if recent.is_empty() { String::new() } else { format!("<h2>In your index</h2><ul>{}</ul>", recent) }
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
            "<html><head><title>{} - Hyda Search</title></head><body><div class=wrap><p style=\"font-size:24px;font-weight:bold;color:#b5581b;margin:4px 0 12px\">Hyda Search</p>{}<p class=count>{} result{} from {} pages</p>{}{}</div></body></html>",
            esc(q),
            Self::search_box(q),
            hits.len(),
            if hits.len() == 1 { "" } else { "s" },
            sys.search.index.len(),
            body,
            none
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
            let dom = html::parse(&c.html);
            c.page = render::layout(&dom, "", width);
            c.width = width;
        }
        let page = &c.page;
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
                    ui.rrect(r, 8, if hot { t.accent.mix(Color::rgb(0x000000), 20) } else { t.accent });
                    ui.text_in(r, Face::Semibold, 14, value, t.on_accent, 1);
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
        let reload_icon = if self.loading.is_some() { Icon::Close } else { Icon::Redo };
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
        ui.field(bar, &shown, "Search with Hyda Search or type an address", focus, Action::App(inst, C_URL));
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
        let Some((id, requested)) = self.loading.clone() else { return };
        let Some(res) = sys.web.take(id) else { return };
        self.loading = None;
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
                if ctype.contains("html") || ctype.is_empty() && text.trim_start().starts_with('<') {
                    let dom = html::parse(&text);
                    sys.search.visited(&url.without_fragment(), &dom);
                    self.show(url, text);
                    if let Some(c) = self.cur.as_mut() {
                        c.security = resp.security.clone();
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
                    self.edit = Some(LineEdit { text });
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

    fn key(&mut self, k: Key, ctrl: bool, sys: &mut Sys) {
        if ctrl {
            match k {
                Key::Char('l') | Key::Char('L') => self.action(C_URL, false, sys),
                Key::Char('r') | Key::Char('R') => self.action(C_RELOAD, false, sys),
                _ => {}
            }
            return;
        }
        if let Some(e) = &mut self.edit {
            if self.fresh {
                self.fresh = false;
                match k {
                    Key::Char(c) if !c.is_control() => e.text.clear(),
                    Key::Backspace | Key::Delete => {
                        e.text.clear();
                        return;
                    }
                    _ => {}
                }
            }
            match k {
                Key::Enter => {
                    let t = e.text.clone();
                    self.edit = None;
                    if !t.trim().is_empty() {
                        let u = url::from_input(&t).to_string();
                        self.go(sys, &u, true);
                    }
                }
                Key::Esc => self.edit = None,
                _ => {
                    e.key(k);
                }
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
                Key::Char(ch) if !ch.is_control() => v.push(ch),
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

    fn animating(&self) -> bool {
        self.edit.is_some() || self.loading.is_some() || self.focus_field.is_some()
    }
}
