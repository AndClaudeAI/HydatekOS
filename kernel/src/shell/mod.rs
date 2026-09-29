//! The HydatekOS shell: desktop (menu bar, widgets, windows, dock, launcher,
//! notifications) and the mobile shell, plus event routing.

pub mod cursor;
pub mod lock;
pub mod mobile;
pub mod keys;
pub mod osk;
pub mod setup;
pub mod splash;
pub mod wallpaper;

use crate::apps::{self, App, AppKind, DESKTOP_APPS, HEADER};
use crate::efi;
use crate::font::Face;
use crate::gfx::{Canvas, Color, Rect};
use crate::icons::Icon;
use crate::input::Ev;
use crate::sys::{Req, Sys};
use crate::theme::{theme, Theme};
use crate::ui::{Action, Key, MobileAct, Ui, Zone};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use mobile::Mobile;

const BAR_H: i32 = 28;

/// The HydatekOS mark: an arch in a rounded square, `size` points wide.
pub fn logo(ui: &mut Ui, x: i32, y: i32, size: i32, bg: Color, fg: Color) {
    let k = |v: i32| v * size / 18;
    ui.rrect(Rect::new(x, y, size, size), k(5).max(2), bg);
    ui.rrect(Rect::new(x + k(5), y + k(4), k(8), k(11)), k(4), fg);
    ui.rrect(Rect::new(x + k(7), y + k(7), k(4), k(9)), k(2), bg);
    ui.rect(Rect::new(x + k(7), y + k(11), k(4), k(15) - k(11) + 1), bg);
}
const LOCAL_INST: u32 = 1_000_000;
const PHONE_INST: u32 = 1_000_001;
const DOCK_APPS: [AppKind; 11] = [AppKind::Files, AppKind::Browser, AppKind::Messages, AppKind::Mail, AppKind::Calendar, AppKind::Notes, AppKind::Scripts, AppKind::Grids, AppKind::Slides, AppKind::Music, AppKind::Settings];
const MENUS: [&str; 4] = ["File", "Edit", "View", "Go"];
/// The linked phone renders at its native size and is scaled into Phone Link.
const PHONE_W: i32 = 390;
const PHONE_H: i32 = 844;

struct Win {
    id: u32,
    app: Box<dyn App>,
    r: Rect,
    min: bool,
    max: bool,
}

#[derive(Clone, Copy)]
enum Drag {
    /// the pointer was pressed on an app's zone: it gets the moves
    App(u32),
    Move(u32, i32, i32),
    Resize(u32, i32, i32, Rect),
}

#[derive(Clone, Copy, PartialEq)]
enum KFocus {
    Top,
    Phone,
}

#[derive(Clone)]
enum Cmd {
    App(u32),
    CloseWin,
    MinWin,
    Lock,
    Dark,
    MobileShell,
    Launcher,
    Open(AppKind),
    GoHome,
    Restart,
    Shutdown,
    Profile,
    SignOut,
    Shortcuts,
    None,
}

/// A mobile shell rendered at its native 390x844 size (optionally 2x
/// supersampled) and scaled onto the desktop.
struct Mirror {
    canvas: Canvas,
    scale: i32,
    zones: Vec<Zone>,
    hover: Option<Action>,
    rect: Option<Rect>,
}

impl Mirror {
    fn new() -> Mirror {
        Mirror { canvas: Canvas::new(PHONE_W, PHONE_H), scale: 1, zones: vec![], hover: None, rect: None }
    }

    /// Map a desktop point to mobile coordinates.
    fn point(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        let r = self.rect?;
        Some(((x - r.x) * PHONE_W / r.w.max(1), (y - r.y) * PHONE_H / r.h.max(1)))
    }

    fn hit(&self, x: i32, y: i32) -> Option<(Action, i32, i32)> {
        let (px, py) = self.point(x, y)?;
        self.zones.iter().rev().find(|z| z.r.contains(px, py)).map(|z| (z.a, px, py))
    }

    /// Render `m` and composite it into `r` (logical) of `ui`.
    fn draw(&mut self, ui: &mut Ui, m: &mut Mobile, sys: &Sys, r: Rect, id: u8, radius: i32) {
        // Supersample when shown large so text stays crisp after scaling.
        let want = if r.h * ui.s > 600 { 2 } else { 1 };
        if want != self.scale {
            self.scale = want;
            self.canvas = Canvas::new(PHONE_W * want, PHONE_H * want);
        }
        let mut pui = Ui::new(&mut self.canvas, self.scale, ui.t, self.hover, ui.ticks);
        m.render(&mut pui, Rect::new(0, 0, PHONE_W, PHONE_H), sys);
        self.zones = core::mem::take(&mut pui.zones);
        drop(pui);
        let s = ui.s;
        ui.c.blit_scaled(&self.canvas, r.scale(s), radius * s);
        ui.zone(r, Action::Mirror(id));
        self.rect = Some(r);
    }
}

struct Toast {
    title: String,
    body: String,
    until: u64,
}

pub struct Shell {
    pub sys: Sys,
    wins: Vec<Win>,
    next_id: u32,
    phone: Mobile,
    local: Mobile,
    launcher: Option<String>,
    menu: Option<u8>,
    menu_items: Vec<(String, Cmd)>,
    toasts: Vec<Toast>,
    drag: Option<Drag>,
    pub mouse: (i32, i32),
    zones: Vec<Zone>,
    hover: Option<Action>,
    wall: Option<(Canvas, bool)>,
    last_click: Option<(Action, u64)>,
    kfocus: KFocus,
    mirrors: [Mirror; 2],
    pub w: i32,
    pub h: i32,
    pub s: i32,
    pub dirty: bool,
    last_anim: u64,
    pub locked: bool,
    lock: lock::Lock,
    /// tick when the unlock slide-away started
    unlocking: Option<u64>,
    last_input: u64,
    /// the setup assistant, while it's showing
    setup: Option<setup::Setup>,
    /// signed out: the next sign-in starts a fresh session
    signed_out: bool,
    /// the keyboard shortcuts sheet is showing
    sheet: bool,
}

impl Shell {
    pub fn new(sys: Sys, w: i32, h: i32, s: i32) -> Shell {
        let mut sh = Shell {
            sys,
            wins: vec![],
            next_id: 1,
            phone: Mobile::new(1, PHONE_INST),
            local: Mobile::new(0, LOCAL_INST),
            launcher: None,
            menu: None,
            menu_items: vec![],
            toasts: vec![],
            drag: None,
            mouse: (w / 2, h / 2),
            zones: vec![],
            hover: None,
            wall: None,
            last_click: None,
            kfocus: KFocus::Top,
            mirrors: [Mirror::new(), Mirror::new()],
            w,
            h,
            s,
            dirty: true,
            last_anim: 0,
            locked: false,
            lock: lock::Lock::default(),
            unlocking: None,
            last_input: 0,
            setup: None,
            signed_out: false,
            sheet: false,
        };
        sh.sys.screen = (w * s, h * s, s);
        if !sh.mobile_mode() {
            sh.open_app(AppKind::Files);
        }
        let first = sh.sys.needs_setup();
        if first {
            // a computer with a PIN or password stays locked until it's given
            sh.setup = Some(setup::Setup::new(&sh.sys));
        } else {
            let msg = if sh.sys.fs.persistent { "Your files are saved to this disk." } else { "Live session: files are kept in memory." };
            let hello = alloc::format!("{}!", sh.sys.greeting());
            sh.sys.toast(&hello, msg);
        }
        // with several accounts, everyone starts at the lock screen to choose
        sh.locked = (sh.sys.lock_on_boot && (!first || sh.sys.secured())) || sh.sys.people.len() > 1;
        sh.lock.select_current(&sh.sys);
        sh.lock.reset(&sh.sys);
        sh
    }

    fn mobile_mode(&self) -> bool {
        self.sys.mobile_shell || self.h > self.w
    }

    fn theme(&self) -> Theme {
        theme(self.sys.dark, self.sys.accent)
    }

    // ---- window management ------------------------------------------------

    fn work_area(&self) -> Rect {
        Rect::new(8, BAR_H + 8, self.w - 16, self.h - BAR_H - 90)
    }

    fn open_app(&mut self, k: AppKind) {
        if self.mobile_mode() {
            self.local.open(k, &mut self.sys);
            return;
        }
        if let Some(i) = self.wins.iter().position(|w| w.app.kind() == k) {
            self.wins[i].min = false;
            let w = self.wins.remove(i);
            self.wins.push(w);
            self.kfocus = KFocus::Top;
            return;
        }
        let app = apps::create(k, &mut self.sys);
        let (mut ww, mut wh) = k.size();
        let wa = self.work_area();
        ww = ww.min(wa.w);
        wh = wh.min(wa.h);
        let n = self.wins.len() as i32;
        let mut x = wa.x + (wa.w - ww) / 2 + 60 + n * 26;
        let mut y = BAR_H + 34 + n * 26;
        if x + ww > wa.r() {
            x = wa.x + (wa.w - ww) / 2;
        }
        if y + wh > wa.b() {
            y = BAR_H + 20;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.wins.push(Win { id, app, r: Rect::new(x, y, ww, wh), min: false, max: false });
        self.kfocus = KFocus::Top;
    }

    fn win_idx(&self, id: u32) -> Option<usize> {
        self.wins.iter().position(|w| w.id == id)
    }

    fn raise(&mut self, id: u32) {
        if let Some(i) = self.win_idx(id) {
            let w = self.wins.remove(i);
            self.wins.push(w);
        }
    }

    fn raise_phonelink(&mut self) {
        if let Some(pl) = self.wins.iter().find(|w| w.app.kind() == AppKind::PhoneLink).map(|w| w.id) {
            self.raise(pl);
        }
    }

    fn close_win(&mut self, id: u32) {
        if let Some(i) = self.win_idx(id) {
            let mut w = self.wins.remove(i);
            w.app.close(&mut self.sys);
        }
    }

    fn top(&self) -> Option<usize> {
        self.wins.iter().rposition(|w| !w.min)
    }

    fn win_rect(&self, w: &Win) -> Rect {
        if w.max {
            let wa = self.work_area();
            Rect::new(wa.x, wa.y, wa.w, wa.h + 10)
        } else {
            w.r
        }
    }

    fn focused_inst(&self) -> Option<u32> {
        if self.mobile_mode() {
            return Some(LOCAL_INST);
        }
        if self.kfocus == KFocus::Phone {
            return Some(PHONE_INST);
        }
        self.top().map(|i| self.wins[i].id)
    }

    /// Run `f` on app instance `inst` with mutable access to the system state.
    fn with_app(&mut self, inst: u32, f: impl FnOnce(&mut Box<dyn App>, &mut Sys)) {
        let Shell { wins, local, phone, sys, .. } = self;
        let app = match inst {
            LOCAL_INST => local.app.as_mut(),
            PHONE_INST => phone.app.as_mut(),
            id => wins.iter_mut().find(|w| w.id == id).map(|w| &mut w.app),
        };
        if let Some(a) = app {
            f(a, sys);
        }
    }

    // ---- requests and ticks -------------------------------------------------

    pub fn lock_now(&mut self) {
        self.locked = true;
        self.unlocking = None;
        self.lock.select_current(&self.sys);
        self.lock.reset(&self.sys);
        self.menu = None;
        self.launcher = None;
        self.drag = None;
        self.dirty = true;
    }

    fn unlock(&mut self) {
        // someone else chose their account on the lock screen: sign them in
        let chosen = self.lock.chosen(&self.sys).map(String::from);
        if let Some(id) = chosen.filter(|id| *id != self.sys.user) {
            self.sign_in(&id);
        } else if self.signed_out {
            self.start_session();
        }
        self.unlocking = Some(self.sys.ticks);
        self.lock.cancel_finger(&mut self.sys);
        self.lock.reset(&self.sys);
        self.dirty = true;
    }

    /// Close every app (each saves its work), as when signing out.
    fn close_apps(&mut self) {
        for mut w in core::mem::take(&mut self.wins) {
            w.app.close(&mut self.sys);
        }
        self.local.act(MobileAct::Home, &mut self.sys);
        self.menu = None;
        self.launcher = None;
        self.drag = None;
        self.toasts.clear();
        self.kfocus = KFocus::Top;
    }

    /// Sign out: close the apps and show the lock screen, where anyone can
    /// choose their account.
    fn sign_out(&mut self) {
        self.close_apps();
        self.signed_out = true;
        self.lock_now();
    }

    /// Switch the session to account `id`.
    fn sign_in(&mut self, id: &str) {
        self.close_apps();
        self.sys.switch_user(id);
        self.start_session();
    }

    /// A fresh session for the signed-in account: the setup assistant if it
    /// hasn't been set up, otherwise the desktop with Files.
    fn start_session(&mut self) {
        self.signed_out = false;
        if self.sys.needs_setup() {
            self.setup = Some(setup::Setup::new(&self.sys));
            return;
        }
        self.setup = None;
        if !self.mobile_mode() {
            self.open_app(AppKind::Files);
        }
        let hello = alloc::format!("{}!", self.sys.greeting());
        self.toast(&hello, "Welcome back.");
    }

    /// Advance one tick (10 ms). Returns true if the screen needs redrawing.
    pub fn tick(&mut self, ticks: u64) -> bool {
        self.sys.ticks = ticks;
        if let Some(start) = self.unlocking {
            self.dirty = true;
            if ticks >= start + 28 {
                self.unlocking = None;
                self.locked = false;
            }
        } else if self.locked {
            if self.lock.tick(&mut self.sys, ticks) || (self.lock.animating(ticks) && ticks % 4 == 0) {
                self.dirty = true;
            }
        }
        if !self.locked && self.sys.lock_idle > 0 && ticks > self.last_input + self.sys.lock_idle as u64 * 6000 {
            self.lock_now();
        }
        if ticks % 50 == 0 {
            let t = efi::now();
            if t.minute != self.sys.now.minute || t.hour != self.sys.now.hour {
                self.dirty = true;
            }
            self.sys.now = t;
        }
        let clock = self.sys.clock();
        if let Some((app, title, body)) = self.sys.link.tick(ticks, clock) {
            if !self.sys.focus {
                self.toast(&app, &alloc::format!("{}: {}", title, body));
            }
            self.dirty = true;
        }
        let mut anim = self.launcher.is_some() || self.setup.as_ref().map_or(false, |s| s.animating());
        let mut changed = false;
        for w in self.wins.iter_mut() {
            let was = w.app.animating();
            w.app.tick(&mut self.sys);
            let now = w.app.animating();
            // an app that just stopped (a page finished loading) needs one more frame
            changed |= was != now && !w.min;
            anim |= !w.min && now;
        }
        for m in [&mut self.local, &mut self.phone] {
            if let Some(a) = m.app.as_mut() {
                let was = a.animating();
                a.tick(&mut self.sys);
                let now = a.animating();
                changed |= was != now;
                anim |= now;
            }
        }
        if changed {
            self.dirty = true;
        }
        let before = self.toasts.len();
        self.toasts.retain(|t| t.until > ticks);
        if before != self.toasts.len() {
            self.dirty = true;
        }
        self.sys.web_tick();
        self.process_reqs();
        if anim && ticks >= self.last_anim + 10 {
            self.last_anim = ticks;
            self.dirty = true;
        }
        self.dirty
    }

    fn toast(&mut self, title: &str, body: &str) {
        self.toasts.push(Toast { title: title.to_string(), body: body.to_string(), until: self.sys.ticks + 500 });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    fn process_reqs(&mut self) {
        while !self.sys.reqs.is_empty() {
            let r = self.sys.reqs.remove(0);
            self.dirty = true;
            match r {
                Req::Open(k) => self.open_app(k),
                Req::Lock => self.lock_now(),
                Req::PhoneUnlock(id, ok) => {
                    if self.locked && self.unlocking.is_none() {
                        if let lock::Outcome::Unlock = self.lock.phone_answer(&id, ok, &self.sys, self.sys.ticks) {
                            self.unlock();
                        }
                    }
                }
                Req::OpenPath(p) => self.open_path(&p),
                Req::Settings(i) => {
                    self.sys.settings_page = Some(i);
                    self.open_app(AppKind::Settings);
                }
                Req::Shortcuts => {
                    self.sheet = true;
                    self.menu = None;
                    self.launcher = None;
                }
                Req::Setup => {
                    self.setup = Some(setup::Setup::new(&self.sys));
                    self.menu = None;
                    self.launcher = None;
                }
                Req::Toast(t, b) => self.toast(&t, &b),
                Req::SaveSettings => self.sys.save_settings(),
                Req::Shutdown | Req::Reboot => {
                    for w in self.wins.iter_mut() {
                        w.app.close(&mut self.sys);
                    }
                    self.sys.save_settings();
                    efi::reset(if matches!(r, Req::Shutdown) { 2 } else { 0 });
                }
            }
        }
    }

    fn open_path(&mut self, p: &str) {
        let kind = if self.sys.fs.is_dir(p) {
            AppKind::Files
        } else if [".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp", ".svg"].iter().any(|e| p.to_ascii_lowercase().ends_with(e)) {
            AppKind::Browser
        } else if [".hydg", ".xlsx", ".csv"].iter().any(|e| p.to_ascii_lowercase().ends_with(e)) {
            AppKind::Grids
        } else if [".hyds", ".docx"].iter().any(|e| p.to_ascii_lowercase().ends_with(e)) {
            AppKind::Scripts
        } else if [".hydp", ".pptx"].iter().any(|e| p.to_ascii_lowercase().ends_with(e)) {
            AppKind::Slides
        } else {
            AppKind::Notes
        };
        self.open_app(kind);
        let target: Option<&mut Box<dyn App>> = if self.mobile_mode() { self.local.app.as_mut() } else { self.wins.last_mut().map(|w| &mut w.app) };
        if let Some(a) = target {
            a.open_path(p, &mut self.sys);
        }
    }

    // ---- events -------------------------------------------------------------

    fn hit(&self, x: i32, y: i32) -> Option<Action> {
        self.zones.iter().rev().find(|z| z.r.contains(x, y)).map(|z| z.a)
    }



    pub fn event(&mut self, ev: Ev, px: i32, py: i32) {
        let (x, y) = (px / self.s, py / self.s);
        self.mouse = (x, y);
        self.last_input = self.sys.ticks;
        if self.locked {
            return self.lock_event(ev, x, y);
        }
        if self.setup.is_some() {
            return self.setup_event(ev, x, y);
        }
        match ev {
            Ev::Move => {
                if let Some(d) = self.drag {
                    self.drag_to(d, x, y);
                    self.dirty = true;
                } else {
                    let h = self.hit(x, y);
                    if h != self.hover {
                        self.hover = h;
                        self.dirty = true;
                    }
                    for (i, m) in self.mirrors.iter_mut().enumerate() {
                        let ph = if h == Some(Action::Mirror(i as u8)) { m.hit(x, y).map(|p| p.0) } else { None };
                        if ph != m.hover {
                            m.hover = ph;
                            self.dirty = true;
                        }
                    }
                }
            }
            Ev::Up => {
                if self.drag.take().is_some() {
                    self.dirty = true;
                }
            }
            Ev::Down | Ev::RightDown => {
                self.dirty = true;
                let Some(a) = self.hit(x, y) else { return };
                let double = matches!(self.last_click, Some((la, t)) if la == a && self.sys.ticks - t < 45);
                self.last_click = Some((a, self.sys.ticks));
                if self.menu.is_some() && !matches!(a, Action::Menu(_) | Action::MenuItem(..)) {
                    self.menu = None;
                    return;
                }
                self.click(a, double, x, y);
                self.hover = self.hit(x, y);
            }
            Ev::Scroll(dz) => {
                self.dirty = true;
                let hit = match self.hit(x, y) {
                    Some(Action::Mirror(i)) => self.mirrors[i as usize].hit(x, y).map(|p| p.0),
                    h => h,
                };
                let target = match hit {
                    Some(Action::App(inst, _)) => Some(inst),
                    Some(Action::WinFocus(id)) | Some(Action::WinDrag(id)) => Some(id),
                    _ => None,
                };
                if let Some(inst) = target {
                    self.with_app(inst, |a, _| a.scroll(dz));
                }
            }
            Ev::Key(k, gen) => {
                self.dirty = true;
                self.key(k, gen);
            }
        }
    }

    fn lock_event(&mut self, ev: Ev, x: i32, y: i32) {
        if self.unlocking.is_some() {
            return;
        }
        let now = self.sys.ticks;
        let outcome = match ev {
            Ev::Move => {
                let h = self.hit(x, y);
                if h != self.hover {
                    self.hover = h;
                    self.dirty = true;
                }
                return;
            }
            Ev::Down => {
                self.dirty = true;
                match self.hit(x, y) {
                    Some(Action::Lock(a)) => self.lock.action(a, &mut self.sys, now),
                    _ => return,
                }
            }
            Ev::Key(k, _) => {
                self.dirty = true;
                self.lock.key(k, &mut self.sys, now)
            }
            _ => return,
        };
        if let lock::Outcome::Unlock = outcome {
            self.unlock();
        }
    }

    fn setup_event(&mut self, ev: Ev, x: i32, y: i32) {
        let now = self.sys.ticks;
        let Some(st) = self.setup.as_mut() else { return };
        let outcome = match ev {
            Ev::Move => {
                let h = self.zones.iter().rev().find(|z| z.r.contains(x, y)).map(|z| z.a);
                if h != self.hover {
                    self.hover = h;
                    self.dirty = true;
                }
                return;
            }
            Ev::Down => {
                self.dirty = true;
                match self.zones.iter().rev().find(|z| z.r.contains(x, y)).map(|z| z.a) {
                    Some(Action::Setup(c)) => st.action(c, &mut self.sys, now),
                    _ => return,
                }
            }
            Ev::Key(k, _) => {
                self.dirty = true;
                st.key(k, &mut self.sys)
            }
            _ => return,
        };
        if let setup::Outcome::Finished = outcome {
            let first = self.setup.as_ref().map_or(false, |s| !s.again && s.step == setup::Step::Done);
            self.setup = None;
            self.hover = None;
            if self.wins.is_empty() && !self.mobile_mode() {
                self.open_app(AppKind::Files);
            }
            if first {
                let msg = if self.sys.fs.persistent { "Your files are saved to this disk." } else { "Live session: files are kept in memory." };
                let hello = alloc::format!("Welcome, {}", self.sys.profile.first_name());
                self.toast(&hello, msg);
            }
        }
    }

    fn drag_to(&mut self, d: Drag, x: i32, y: i32) {
        let (w, h) = (self.w, self.h);
        match d {
            Drag::App(inst) => self.with_app(inst, |a, _| a.drag(x, y)),
            Drag::Move(id, ox, oy) => {
                if let Some(i) = self.win_idx(id) {
                    let win = &mut self.wins[i];
                    win.max = false;
                    win.r.x = (x - ox).clamp(-win.r.w + 80, w - 80);
                    win.r.y = (y - oy).clamp(BAR_H, h - 40);
                }
            }
            Drag::Resize(id, sx, sy, r0) => {
                if let Some(i) = self.win_idx(id) {
                    let win = &mut self.wins[i];
                    win.r.w = (r0.w + x - sx).clamp(360, w);
                    win.r.h = (r0.h + y - sy).clamp(240, h);
                }
            }
        }
    }

    fn click(&mut self, a: Action, double: bool, x: i32, y: i32) {
        // any click inside a window focuses it
        match a {
            Action::WinFocus(id) | Action::WinDrag(id) | Action::WinMax(id) | Action::WinResize(id) => {
                self.raise(id);
                self.kfocus = KFocus::Top;
            }
            Action::App(inst, _) if inst < LOCAL_INST => {
                self.raise(inst);
                self.kfocus = KFocus::Top;
            }
            Action::App(PHONE_INST, _) | Action::Mobile(1, _) => {
                self.kfocus = KFocus::Phone;
            }
            _ => {}
        }
        match a {
            Action::Lock(_) | Action::Setup(_) => {}
            Action::Background | Action::Swallow => {
                self.launcher = None;
                self.sheet = false;
            }
            Action::Launch(k) => {
                self.launcher = None;
                if let Some(i) = self.wins.iter().position(|w| w.app.kind() == k) {
                    let top = self.top();
                    if top == Some(i) && !self.wins[i].min {
                        self.wins[i].min = true;
                        return;
                    }
                }
                self.open_app(k);
            }
            Action::ToggleLauncher => {
                self.launcher = if self.launcher.is_some() { None } else { Some(String::new()) };
            }
            Action::Quick(i) => {
                match i {
                    0 => self.sys.wifi = !self.sys.wifi,
                    1 => self.sys.focus = !self.sys.focus,
                    2 => self.sys.dark = !self.sys.dark,
                    3 => self.sys.bt = !self.sys.bt,
                    4 => self.open_app(AppKind::Calendar),
                    9 => self.sys.mobile_shell = false,
                    _ => {}
                }
                self.sys.save_settings();
            }
            Action::WinFocus(_) => {}
            Action::WinDrag(id) => {
                if double {
                    if let Some(i) = self.win_idx(id) {
                        self.wins[i].max = !self.wins[i].max;
                    }
                } else if let Some(i) = self.win_idx(id) {
                    let r = self.win_rect(&self.wins[i]);
                    if self.wins[i].max {
                        // un-maximise under the pointer
                        let w = &mut self.wins[i];
                        w.max = false;
                        w.r.x = x - w.r.w / 2;
                        w.r.y = y - 20;
                        self.drag = Some(Drag::Move(id, w.r.w / 2, 20));
                    } else {
                        self.drag = Some(Drag::Move(id, x - r.x, y - r.y));
                    }
                }
            }
            Action::WinResize(id) => {
                if let Some(i) = self.win_idx(id) {
                    let r = self.win_rect(&self.wins[i]);
                    self.wins[i].max = false;
                    self.wins[i].r = r;
                    self.drag = Some(Drag::Resize(id, x, y, r));
                }
            }
            Action::WinClose(id) => self.close_win(id),
            Action::WinMin(id) => {
                if let Some(i) = self.win_idx(id) {
                    self.wins[i].min = true;
                }
            }
            Action::WinMax(id) => {
                if let Some(i) = self.win_idx(id) {
                    self.wins[i].max = !self.wins[i].max;
                }
            }
            Action::App(inst, code) => {
                let (mx, my) = self.mouse;
                self.with_app(inst, |a, sys| {
                    a.mouse(mx, my);
                    a.action(code, double, sys);
                });
                // desktop windows get the pointer moves until release (text selection)
                if self.win_idx(inst).is_some() && (mx, my) == (x, y) {
                    self.drag = Some(Drag::App(inst));
                }
            }
            Action::Menu(i) => {
                self.menu = if self.menu == Some(i) { None } else { Some(i) };
                self.launcher = None;
            }
            Action::MenuItem(_, i) => {
                let cmd = self.menu_items.get(i as usize).map(|c| c.1.clone()).unwrap_or(Cmd::None);
                self.menu = None;
                self.run_cmd(cmd);
            }
            Action::Mobile(id, act) => {
                let m = if id == 0 { &mut self.local } else { &mut self.phone };
                m.act(act, &mut self.sys);
            }
            Action::Mirror(i) => {
                if let Some((pa, px, py)) = self.mirrors[i as usize].hit(x, y) {
                    if i == 1 {
                        self.raise_phonelink();
                        self.kfocus = KFocus::Phone;
                    }
                    self.mouse = (px, py);
                    self.click(pa, double, px, py);
                }
            }
            Action::Toast(i) => {
                if (i as usize) < self.toasts.len() {
                    self.toasts.remove(i as usize);
                }
            }
        }
    }

    fn run_cmd(&mut self, c: Cmd) {
        match c {
            Cmd::App(code) => {
                if let Some(inst) = self.focused_inst() {
                    self.with_app(inst, |a, sys| a.action(code, false, sys));
                }
            }
            Cmd::CloseWin => {
                if let Some(i) = self.top() {
                    let id = self.wins[i].id;
                    self.close_win(id);
                }
            }
            Cmd::MinWin => {
                if let Some(i) = self.top() {
                    self.wins[i].min = true;
                }
            }
            Cmd::Dark => {
                self.sys.dark = !self.sys.dark;
                self.sys.save_settings();
            }
            Cmd::MobileShell => {
                self.sys.mobile_shell = !self.sys.mobile_shell;
                self.sys.save_settings();
            }
            Cmd::Launcher => self.launcher = Some(String::new()),
            Cmd::Open(k) => self.open_app(k),
            Cmd::GoHome => {
                self.open_app(AppKind::Files);
                self.open_path("/home");
            }
            Cmd::Restart => self.sys.reqs.push(Req::Reboot),
            Cmd::Shutdown => self.sys.reqs.push(Req::Shutdown),
            Cmd::Lock => self.lock_now(),
            Cmd::Profile => self.sys.reqs.push(Req::Settings(0)),
            Cmd::SignOut => self.sign_out(),
            Cmd::Shortcuts => self.sheet = true,
            Cmd::None => {}
        }
    }

    fn launcher_matches(&self) -> Vec<AppKind> {
        let q = self.launcher.as_deref().unwrap_or("").to_lowercase();
        DESKTOP_APPS.iter().copied().filter(|k| k.name().to_lowercase().contains(&q)).collect()
    }

    fn key(&mut self, k: Key, gen: bool) {
        if self.sheet {
            // any key closes the shortcuts
            self.sheet = false;
            return;
        }
        if gen && k == Key::Char('/') {
            self.sheet = true;
            self.menu = None;
            self.launcher = None;
            return;
        }
        if let Some(q) = self.launcher.as_mut() {
            match k {
                Key::Esc => self.launcher = None,
                Key::Enter => {
                    if let Some(k) = self.launcher_matches().first().copied() {
                        self.launcher = None;
                        self.open_app(k);
                    }
                }
                Key::Backspace => {
                    q.pop();
                }
                Key::Char(' ') if gen => self.launcher = None,
                Key::Char(c) if !c.is_control() && !gen => q.push(c),
                _ => {}
            }
            return;
        }
        if self.menu.is_some() {
            if k == Key::Esc {
                self.menu = None;
            }
            return;
        }
        if k == Key::F(12) {
            return self.lock_now();
        }
        if !self.mobile_mode() {
            // window switching: the logo key or Aux (Ctrl+Tab stays with the
            // app: some firmware sends Ctrl+I as Tab)
            if k == Key::Tab && (crate::input::logo() || crate::input::aux()) {
                return self.cycle_windows(crate::input::shift());
            }
            match (k, gen) {
                (Key::Char('w'), true) | (Key::Char('q'), true) => return self.run_cmd(Cmd::CloseWin),
                (Key::Char(' '), true) | (Key::F(1), _) => {
                    self.launcher = Some(String::new());
                    return;
                }
                (Key::Char(','), true) => return self.sys.reqs.push(Req::Settings(0)),
                _ => {}
            }
        } else if k == Key::Esc && !self.local.app.as_ref().map_or(false, |a| a.fullscreen()) {
            return self.local.act(MobileAct::Home, &mut self.sys);
        }
        if let Some(inst) = self.focused_inst() {
            self.with_app(inst, |a, sys| a.key(k, gen, sys));
        }
    }

    /// Bring the next window forward (Gen+Tab / Alt+Tab), or with `back` the
    /// one before; minimised windows come back too.
    fn cycle_windows(&mut self, back: bool) {
        if self.wins.len() < 2 && self.wins.iter().all(|w| !w.min) {
            return;
        }
        if back {
            if let Some(w) = self.wins.pop() {
                self.wins.insert(0, w);
            }
        } else {
            let w = self.wins.remove(0);
            self.wins.push(w);
        }
        if let Some(w) = self.wins.last_mut() {
            w.min = false;
        }
        self.kfocus = KFocus::Top;
    }

    // ---- rendering ----------------------------------------------------------

    pub fn render(&mut self, canvas: &mut Canvas, ticks: u64) {
        let full = Rect::new(0, 0, self.w, self.h);
        if self.locked && self.unlocking.is_none() {
            let mut ui = Ui::new(canvas, self.s, self.theme(), self.hover, ticks);
            self.lock.render(&mut ui, full, &self.sys, ticks);
            self.zones = core::mem::take(&mut ui.zones);
            return;
        }
        self.render_session(canvas, ticks);
        if let Some(start) = self.unlocking {
            // slide the lock screen up and away (ease-out cubic)
            let p = ((ticks - start) as i32 * 1000 / 28).clamp(0, 1000);
            let inv = 1000 - p;
            let eased = 1000 - inv * inv / 1000 * inv / 1000;
            let off = self.h * eased / 1000;
            let mut ui = Ui::new(canvas, self.s, self.theme(), None, ticks);
            let old = ui.set_clip(Rect::new(0, 0, self.w, self.h - off));
            self.lock.render(&mut ui, Rect::new(0, -off, self.w, self.h), &self.sys, ticks);
            ui.set_clip(old);
        }
    }

    fn render_session(&mut self, canvas: &mut Canvas, ticks: u64) {
        let t = self.theme();
        let s = self.s;
        let (w, h) = (self.w, self.h);
        if let Some(st) = self.setup.as_mut() {
            let mut ui = Ui::new(canvas, s, t, self.hover, ticks);
            st.render(&mut ui, Rect::new(0, 0, w, h), &self.sys);
            self.zones = core::mem::take(&mut ui.zones);
            return;
        }
        let mobile = self.mobile_mode();
        if self.wall.as_ref().map(|w| w.1) != Some(t.dark) {
            let mut c = Canvas::new(w * s, h * s);
            let b = c.bounds();
            wallpaper::draw(&mut c, b, &t, false);
            self.wall = Some((c, t.dark));
        }
        let mut ui = Ui::new(canvas, s, t, self.hover, ticks);
        ui.zone(Rect::new(0, 0, w, h), Action::Background);
        if mobile {
            if h > w {
                self.local.render(&mut ui, Rect::new(0, 0, w, h), &self.sys);
            } else {
                let b = ui.c.bounds();
                ui.c.copy_from(&self.wall.as_ref().unwrap().0, b);
                let ph = h - 48;
                let pw = ph * 390 / 844;
                let r = Rect::new((w - pw) / 2, 24, pw, ph);
                ui.shadow(r, 36, 16, 8, 80);
                ui.rrect(r.inset(-8), 40, Color::rgb(0x121119));
                self.mirrors[0].draw(&mut ui, &mut self.local, &self.sys, r, 0, 32);
                let hr = Rect::new(r.r() + 40, h / 2 - 18, 170, 36);
                if hr.r() < w {
                    ui.button(hr, "Back to desktop", Action::Quick(9), true);
                }
            }
            self.zones = core::mem::take(&mut ui.zones);
            return;
        }
        // a full-screen app (a slideshow) covers everything
        if let Some(i) = self.top().filter(|&i| self.wins[i].app.fullscreen()) {
            let Shell { wins, sys, .. } = self;
            let id = wins[i].id;
            wins[i].app.render(&mut ui, Rect::new(0, 0, w, h), sys, id);
            self.zones = core::mem::take(&mut ui.zones);
            return;
        }
        let b = ui.c.bounds();
        ui.c.copy_from(&self.wall.as_ref().unwrap().0, b);
        self.draw_widgets(&mut ui);
        self.draw_windows(&mut ui);
        self.draw_dock(&mut ui);
        self.draw_bar(&mut ui);
        // notifications under the launcher and open menus
        self.draw_toasts(&mut ui);
        if self.launcher.is_some() {
            self.draw_launcher(&mut ui);
        }
        if self.menu.is_some() {
            self.draw_menu(&mut ui);
        }
        if self.sheet {
            self.draw_sheet(&mut ui);
        }
        self.zones = core::mem::take(&mut ui.zones);
    }

    fn draw_widgets(&self, ui: &mut Ui) {
        let t = ui.t;
        let sys = &self.sys;
        if self.w < 900 {
            return;
        }
        let card = Rect::new(28, BAR_H + 26, 200, 182);
        ui.shadow(card, 22, 10, 3, 18);
        ui.rrect(card, 22, t.surface);
        let top_line = if sys.profile.ready() { sys.greeting() } else { sys.date_long() };
        let top_line = ui.fit(Face::Regular, 12, &top_line, card.w - 36);
        ui.text(card.x + 18, card.y + 30, Face::Regular, 12, &top_line, t.text2);
        ui.text(card.x + 15, card.y + 90, Face::Display, 64, &sys.clock(), t.text);
        ui.rect(Rect::new(card.x + 18, card.y + 106, card.w - 36, 1), t.line);
        ui.label(card.x + 18, card.y + 128, 10, "UP NEXT", t.accent);
        let (title, sub) = match sys.next_event() {
            Some(e) => (e.title.clone(), alloc::format!("{}{}{}", sys.event_when(e), if e.place.is_empty() { "" } else { " · " }, e.place)),
            None => ("Nothing scheduled".to_string(), "Add events in Calendar".to_string()),
        };
        let title = ui.fit(Face::Semibold, 14, &title, card.w - 36);
        let sub = ui.fit(Face::Regular, 12, &sub, card.w - 36);
        ui.text(card.x + 18, card.y + 150, Face::Semibold, 14, &title, t.text);
        ui.text(card.x + 18, card.y + 168, Face::Regular, 12, &sub, t.text2);
        ui.zone(Rect::new(card.x, card.y + 110, card.w, 72), Action::Quick(4));

        let qc = Rect::new(28, card.b() + 12, 200, 98);
        ui.shadow(qc, 22, 10, 3, 18);
        ui.rrect(qc, 22, t.surface);
        let items = [("Wi-Fi", sys.wifi), ("Focus", sys.focus), ("Dark mode", sys.dark), ("Bluetooth", sys.bt)];
        for (i, (name, on)) in items.iter().enumerate() {
            let r = Rect::new(qc.x + 12 + (i as i32 % 2) * 92, qc.y + 12 + (i as i32 / 2) * 40, 84, 33);
            let a = Action::Quick(i as u8);
            let bg = if *on { t.accent } else { t.chip };
            ui.rrect(r, 12, if ui.hot(a) { bg.mix(t.text, 25) } else { bg });
            ui.text_in(r, Face::Semibold, 13, name, if *on { t.on_accent } else { t.text }, 1);
            ui.zone(r, a);
        }
    }

    fn draw_windows(&mut self, ui: &mut Ui) {
        let t = ui.t;
        let top = self.top();
        let kfocus = self.kfocus;
        let wa = self.work_area();
        self.mirrors[1].rect = None;
        let Shell { wins, sys, phone, mirrors, .. } = self;
        for (i, win) in wins.iter_mut().enumerate() {
            if win.min {
                continue;
            }
            let r = if win.max { Rect::new(wa.x, wa.y, wa.w, wa.h + 10) } else { win.r };
            let focused = Some(i) == top && kfocus == KFocus::Top;
            ui.shadow(r, 14, if focused { 20 } else { 12 }, 10, if focused { 70 } else { 38 });
            ui.rrect(r, 14, t.surface);
            if t.dark {
                ui.stroke(r.inset(-1), 15, 1, t.line);
            }
            ui.zone(r, Action::WinFocus(win.id));
            ui.zone(Rect::new(r.x, r.y, r.w, HEADER), Action::WinDrag(win.id));
            let old = ui.clip_in(r);
            win.app.render(ui, r, sys, win.id);
            if let Some(pr) = ui.phone_embed.take() {
                // Render the phone at native resolution, then scale it in.
                mirrors[1].draw(ui, phone, sys, pr, 1, 26);
            }
            // window controls
            let cy = r.y + 10;
            let ctrls = [(Icon::Minimize, Action::WinMin(win.id)), (Icon::Maximize, Action::WinMax(win.id)), (Icon::Close, Action::WinClose(win.id))];
            for (k, (ic, a)) in ctrls.iter().enumerate() {
                let b = Rect::new(r.r() - 98 + k as i32 * 30, cy, 24, 24);
                let close = k == 2;
                let bg = if close { t.accent } else { t.chip };
                ui.circle(b.x + 12, b.y + 12, 12, if ui.hot(*a) { bg.mix(t.text, 35) } else { bg });
                ui.icon_in(*ic, b, 12, if close { t.on_accent } else { t.text });
                ui.zone(b, *a);
            }
            ui.zone(Rect::new(r.r() - 16, r.b() - 16, 16, 16), Action::WinResize(win.id));
            ui.set_clip(old);
            if !focused {
                // subtle dim for background windows
                ui.rrect(r, 14, Color::rgba(if t.dark { 0 } else { 0xFFFFFF }, 18));
            }
        }
    }

    fn draw_dock(&self, ui: &mut Ui) {
        let t = ui.t;
        let extra: Vec<AppKind> = self.wins.iter().map(|w| w.app.kind()).filter(|k| !DOCK_APPS.contains(k)).fold(vec![], |mut v, k| {
            if !v.contains(&k) {
                v.push(k);
            }
            v
        });
        let n = 1 + DOCK_APPS.len() as i32 + extra.len() as i32;
        let bw = 40;
        let gap = 7;
        let dw = n * bw + (n - 1) * gap + 16 + if extra.is_empty() { 0 } else { 12 };
        let dock = Rect::new((self.w - dw) / 2, self.h - 16 - 56, dw, 56);
        ui.shadow(dock, 20, 10, 4, 50);
        ui.rrect(dock, 20, t.dock);
        let mut x = dock.x + 8;
        let mut tip: Option<(Rect, &str)> = None;
        let mut items: Vec<(Icon, Action, &str, bool)> = vec![(Icon::Grid, Action::ToggleLauncher, "All apps", false)];
        for k in DOCK_APPS.iter().chain(extra.iter()) {
            let running = self.wins.iter().any(|w| w.app.kind() == *k);
            items.push((k.icon(), Action::Launch(*k), k.name(), running));
        }
        for (i, (ic, a, name, running)) in items.iter().enumerate() {
            if i == 1 + DOCK_APPS.len() {
                ui.rect(Rect::new(x + 1, dock.y + 14, 1, 28), Color::rgba(0xFFFFFF, 40));
                x += 12;
            }
            let b = Rect::new(x, dock.y + 8, bw, bw);
            let hot = ui.hot(*a);
            ui.rrect(b, 12, if hot { t.dock_btn.mix(t.dock_icon, 40) } else { t.dock_btn });
            ui.icon_in(*ic, b, 20, t.dock_icon);
            if *running {
                ui.circle(b.x + bw / 2, b.b() - 4, 2, Color::rgb(0xE8A15F));
            }
            if hot {
                tip = Some((b, name));
            }
            ui.zone(b, *a);
            x += bw + gap;
        }
        if let Some((b, name)) = tip {
            let tw = ui.tw(Face::Medium, 12, name) + 20;
            let r = Rect::new(b.x + b.w / 2 - tw / 2, b.y - 40, tw, 24);
            ui.rrect(r, 8, t.dock);
            ui.text_in(r, Face::Medium, 12, name, t.dock_icon, 1);
        }
    }

    fn draw_bar(&self, ui: &mut Ui) {
        let t = ui.t;
        let bar = Rect::new(0, 0, self.w, BAR_H);
        ui.rect(bar, t.bar);
        ui.zone(bar, Action::Swallow);
        // logo
        let lr = Rect::new(10, 5, 18, 18);
        let la = Action::Menu(0);
        if ui.hot(la) || self.menu == Some(0) {
            ui.rrect(Rect::new(6, 3, 26, 22), 6, t.hover);
        }
        logo(ui, lr.x, lr.y, lr.w, t.accent, t.on_accent);
        ui.zone(Rect::new(6, 3, 26, 22), la);
        let mut x = 38;
        x += ui.text(x, 19, Face::Semibold, 13, "HydatekOS", t.text) + 18;
        let app_name = match self.top() {
            Some(i) if !self.mobile_mode() => self.wins[i].app.kind().name(),
            _ => "Desktop",
        };
        x += ui.text(x, 19, Face::Semibold, 13, app_name, t.text) + 16;
        for (i, m) in MENUS.iter().enumerate() {
            let a = Action::Menu(i as u8 + 1);
            let w = ui.tw(Face::Regular, 13, m) + 16;
            let r = Rect::new(x - 8, 3, w, 22);
            if ui.hot(a) || self.menu == Some(i as u8 + 1) {
                ui.rrect(r, 6, t.hover);
            }
            ui.text(x, 19, Face::Regular, 13, m, t.text);
            ui.zone(r, a);
            x += w + 4;
        }
        // status area
        let clock = alloc::format!("{}  {}", self.sys.date_short(), self.sys.clock());
        let cw = ui.tw(Face::Medium, 13, &clock);
        let mut rx = self.w - 14 - cw;
        ui.text(rx, 19, Face::Medium, 13, &clock, t.text);
        rx -= 30;
        ui.icon(Icon::Battery, rx, 6, 16, t.text);
        rx -= 26;
        let wifi_col = if self.sys.wifi { t.text } else { t.text3 };
        ui.icon(Icon::Wifi, rx, 6, 16, wifi_col);
        if self.sys.link.paired {
            rx -= 26;
            ui.icon(Icon::Phone, rx, 6, 16, t.text);
            ui.zone(Rect::new(rx - 4, 2, 24, 24), Action::Launch(AppKind::PhoneLink));
        }
        if self.sys.profile.ready() {
            rx -= 30;
            let pa = Action::Menu(5);
            if ui.hot(pa) || self.menu == Some(5) {
                ui.rrect(Rect::new(rx - 4, 2, 28, 24), 6, t.hover);
            }
            ui.avatar(Rect::new(rx, 4, 20, 20), &self.sys.avatar);
            ui.zone(Rect::new(rx - 4, 2, 28, 24), pa);
        }
        rx -= 26;
        let sa = Action::ToggleLauncher;
        if ui.hot(sa) {
            ui.rrect(Rect::new(rx - 4, 3, 24, 22), 6, t.hover);
        }
        ui.icon(Icon::Search, rx, 6, 16, t.text);
        ui.zone(Rect::new(rx - 4, 3, 24, 22), sa);
    }

    fn draw_launcher(&self, ui: &mut Ui) {
        let t = ui.t;
        ui.rect(Rect::new(0, 0, self.w, self.h), Color::rgba(0x100e18, 90));
        ui.zone(Rect::new(0, 0, self.w, self.h), Action::Background);
        let pw = 580.min(self.w - 40);
        let panel = Rect::new((self.w - pw) / 2, (self.h - 440) / 2, pw, 400);
        ui.shadow(panel, 24, 20, 10, 90);
        ui.rrect(panel, 24, t.surface);
        ui.zone(panel, Action::Swallow);
        let q = self.launcher.as_deref().unwrap_or("");
        let sr = Rect::new(panel.x + 24, panel.y + 22, panel.w - 48, 38);
        ui.rrect(sr, 12, t.chip);
        ui.icon(Icon::Search, sr.x + 14, sr.y + 11, 16, t.text2);
        let tr = Rect::new(sr.x + 40, sr.y, sr.w - 50, sr.h);
        if q.is_empty() {
            ui.text_in(tr, Face::Regular, 15, "Search apps", t.text3, 0);
        }
        let w = ui.text_in(tr, Face::Regular, 15, q, t.text, 0);
        if (ui.ticks / 50) % 2 == 0 {
            ui.rect(Rect::new(tr.x + w + 1, sr.y + 10, 1, 18), t.text);
        }
        let cols = 5;
        let cw = (panel.w - 48) / cols;
        for (i, k) in self.launcher_matches().iter().enumerate() {
            let (cx, cy) = (i as i32 % cols, i as i32 / cols);
            let cell = Rect::new(panel.x + 24 + cx * cw, panel.y + 84 + cy * 108, cw, 100);
            let a = Action::Launch(*k);
            if ui.hot(a) || (i == 0 && !q.is_empty()) {
                ui.rrect(cell.inset(4), 14, t.hover);
            }
            let tile = Rect::new(cell.x + (cw - 58) / 2, cell.y + 12, 58, 58);
            ui.rrect(tile, 16, t.tile);
            ui.icon_in(k.icon(), tile, 26, if k.warm() { t.accent } else { t.text });
            ui.text_in(Rect::new(cell.x, cell.y + 74, cw, 20), Face::Regular, 13, k.name(), t.text, 1);
            ui.zone(cell, a);
        }
        let hint = "Enter opens the first match · Esc closes";
        ui.text_in(Rect::new(panel.x, panel.b() - 34, panel.w, 20), Face::Regular, 12, hint, t.text3, 1);
    }

    fn draw_menu(&mut self, ui: &mut Ui) {
        let t = ui.t;
        let m = self.menu.unwrap_or(0);
        let mut items: Vec<(String, Cmd)> = vec![];
        let focused = self.top().filter(|_| self.kfocus == KFocus::Top || m == 0);
        let app_items: Vec<(&'static str, u32)> = match focused {
            Some(i) if (1..=4).contains(&m) => self.wins[i].app.menu(m as usize - 1),
            _ => vec![],
        };
        for (l, c) in &app_items {
            items.push((l.to_string(), Cmd::App(*c)));
        }
        match m {
            0 => {
                items.push(("About HydatekOS".to_string(), Cmd::Open(AppKind::Settings)));
                items.push(("Settings…\tGen+,".to_string(), Cmd::Profile));
                items.push(("App launcher\tGen+Space".to_string(), Cmd::Launcher));
                items.push(("Keyboard Shortcuts\tGen+/".to_string(), Cmd::Shortcuts));
                items.push(("Terminal".to_string(), Cmd::Open(AppKind::Terminal)));
                items.push(("Phone Link".to_string(), Cmd::Open(AppKind::PhoneLink)));
                items.push(("-".to_string(), Cmd::None));
                items.push(("Restart".to_string(), Cmd::Restart));
                items.push(("Shut Down".to_string(), Cmd::Shutdown));
                items.push(("-".to_string(), Cmd::None));
                items.push(("Lock Screen\tF12".to_string(), Cmd::Lock));
            }
            1 => {
                if focused.is_some() {
                    items.push(("Minimise Window".to_string(), Cmd::MinWin));
                    items.push(("Close Window\tGen+W".to_string(), Cmd::CloseWin));
                }
                items.push(("New Notes Window".to_string(), Cmd::Open(AppKind::Notes)));
            }
            2 => {
                if items.is_empty() {
                    items.push(("Nothing to edit here".to_string(), Cmd::None));
                }
            }
            5 => {
                // the header row (drawn with the picture) then the commands
                items.push((self.sys.profile.name.clone(), Cmd::None));
                items.push(("-".to_string(), Cmd::None));
                items.push(("Profile…".to_string(), Cmd::Profile));
                items.push(("Lock Screen\tF12".to_string(), Cmd::Lock));
                if self.sys.people.len() > 1 {
                    items.push(("-".to_string(), Cmd::None));
                    items.push(("Sign Out".to_string(), Cmd::SignOut));
                }
            }
            3 => {
                items.push((if self.sys.dark { "Light Mode" } else { "Dark Mode" }.to_string(), Cmd::Dark));
                items.push(("Mobile Shell".to_string(), Cmd::MobileShell));
                items.push(("All Apps\tGen+Space".to_string(), Cmd::Launcher));
            }
            _ => {
                items.push(("Home Folder".to_string(), Cmd::GoHome));
            }
        }
        // position under the title
        let mut x = 6;
        if m >= 1 {
            x = 38 + ui.tw(Face::Semibold, 13, "HydatekOS") + 18;
            let app_name = match self.top() {
                Some(i) => self.wins[i].app.kind().name(),
                None => "Desktop",
            };
            x += ui.tw(Face::Semibold, 13, app_name) + 16;
            for mm in MENUS.iter().take(m as usize - 1) {
                x += ui.tw(Face::Regular, 13, mm) + 20;
            }
            x -= 8;
        }
        // wide enough for the longest label and its shortcut
        let mut w = 220;
        for (label, _) in &items {
            let (l, hint) = label.split_once('\t').unwrap_or((label.as_str(), ""));
            let need = ui.tw(Face::Regular, 13, l) + if hint.is_empty() { 0 } else { ui.tw(Face::Regular, 12, hint) + 28 } + 44;
            w = w.max(need.min(360));
        }
        let head = if m == 5 { 26 } else { 0 };
        let h = items.iter().map(|i| if i.0 == "-" { 9 } else { 30 }).sum::<i32>() + 12 + head;
        if m == 5 {
            x = self.w - w - 8;
        }
        let r = Rect::new(x, BAR_H + 2, w, h);
        ui.shadow(r, 12, 12, 6, 60);
        ui.rrect(r, 12, t.surface);
        if t.dark {
            ui.stroke(r, 12, 1, t.line);
        }
        ui.zone(r, Action::Swallow);
        let mut y = r.y + 6;
        for (i, (label, cmd)) in items.iter().enumerate() {
            if label == "-" {
                ui.rect(Rect::new(r.x + 12, y + 4, r.w - 24, 1), t.line);
                y += 9;
                continue;
            }
            if m == 5 && i == 0 {
                // who's signed in
                ui.avatar(Rect::new(r.x + 14, y + 6, 40, 40), &self.sys.avatar);
                let name = ui.fit(Face::Semibold, 14, label, r.w - 80);
                ui.text(r.x + 64, y + 24, Face::Semibold, 14, &name, t.text);
                let g = ui.fit(Face::Regular, 12, crate::profile::greeting(self.sys.now.hour), r.w - 80);
                ui.text(r.x + 64, y + 42, Face::Regular, 12, &g, t.text2);
                y += 30 + head;
                continue;
            }
            let a = Action::MenuItem(m, i as u8);
            let ir = Rect::new(r.x + 6, y, r.w - 12, 30);
            let enabled = !matches!(cmd, Cmd::None);
            if ui.hot(a) && enabled {
                ui.rrect(ir, 8, t.accent);
            }
            let col = if !enabled { t.text3 } else if ui.hot(a) { t.on_accent } else { t.text };
            let (label, hint) = label.split_once('\t').unwrap_or((label.as_str(), ""));
            ui.text_in(Rect::new(ir.x + 10, ir.y, ir.w - 20, ir.h), Face::Regular, 13, label, col, 0);
            if !hint.is_empty() {
                let hc = if ui.hot(a) && enabled { t.on_accent } else { t.text3 };
                ui.text_in(Rect::new(ir.x + 10, ir.y, ir.w - 20, ir.h), Face::Regular, 12, hint, hc, 2);
            }
            ui.zone(ir, a);
            y += 30;
        }
        self.menu_items = items;
    }

    /// The keyboard shortcuts (Gen+/): any key or click closes it.
    fn draw_sheet(&self, ui: &mut Ui) {
        let t = ui.t;
        ui.rect(Rect::new(0, 0, self.w, self.h), Color::rgba(0x100e18, 110));
        ui.zone(Rect::new(0, 0, self.w, self.h), Action::Background);
        let cols = if self.w >= 1180 { 3 } else if self.w >= 900 { 2 } else { 1 };
        let pw = match cols {
            3 => 1120.min(self.w - 32),
            2 => 820.min(self.w - 32),
            _ => self.w - 32,
        };
        let row = 30;
        let groups = keys::SHEET;
        let per_col = (groups.len() + cols - 1) / cols;
        let col_h: i32 = (0..cols).map(|c| groups.iter().skip(c * per_col).take(per_col).map(|g| 34 + g.keys.len() as i32 * row + 12).sum::<i32>()).max().unwrap_or(0);
        let ph = (116 + col_h).min(self.h - 32);
        let panel = Rect::new((self.w - pw) / 2, (self.h - ph) / 2, pw, ph);
        ui.shadow(panel, 24, 20, 10, 90);
        ui.rrect(panel, 24, t.surface);
        ui.text(panel.x + 32, panel.y + 46, Face::Semibold, 22, "Keyboard shortcuts", t.text);
        let what = if crate::input::ctrl_is_gen() { "Gen is the ⊞ or ⌘ key (Ctrl works too). Aux is Alt, or ⌥ Option." } else { "Gen is the ⊞ or ⌘ key. Aux is Alt, or ⌥ Option." };
        let what = ui.fit(Face::Regular, 13, what, pw - 64);
        ui.text(panel.x + 32, panel.y + 70, Face::Regular, 13, &what, t.text2);
        let cw = (pw - 64) / cols as i32;
        for c in 0..cols {
            let mut y = panel.y + 100;
            let x = panel.x + 32 + c as i32 * cw;
            for g in groups.iter().skip(c * per_col).take(per_col) {
                ui.label(x, y + 14, 11, &g.title.to_uppercase(), t.accent);
                y += 34;
                for (combo, what) in g.keys {
                    keys::keycaps(ui, x, y + row / 2 - 6, combo, 12);
                    let d = ui.fit(Face::Regular, 13, what, cw - 150);
                    ui.text(x + 136, y + row / 2 - 1, Face::Regular, 13, &d, t.text);
                    y += row;
                }
                y += 12;
            }
        }
        ui.text_in(Rect::new(panel.x, panel.b() - 30, panel.w, 20), Face::Regular, 12, "Press any key to close", t.text3, 1);
    }

    fn draw_toasts(&self, ui: &mut Ui) {
        let t = ui.t;
        for (i, toast) in self.toasts.iter().enumerate() {
            let r = Rect::new(self.w - 336, BAR_H + 14 + i as i32 * 78, 320, 66);
            ui.shadow(r, 16, 12, 6, 55);
            ui.rrect(r, 16, t.surface);
            if t.dark {
                ui.stroke(r, 16, 1, t.line);
            }
            let ic = Rect::new(r.x + 14, r.y + 14, 38, 38);
            ui.rrect(ic, 11, t.accent);
            let icon = if toast.title.contains("Message") || toast.title == "Ada" { Icon::Chat } else if toast.title.contains("Phone") { Icon::Link } else { Icon::Bell };
            ui.icon_in(icon, ic, 18, t.on_accent);
            let tt = ui.fit(Face::Semibold, 13, &toast.title, r.w - 80);
            ui.text(r.x + 64, r.y + 28, Face::Semibold, 13, &tt, t.text);
            let bb = ui.fit(Face::Regular, 12, &toast.body, r.w - 80);
            ui.text(r.x + 64, r.y + 47, Face::Regular, 12, &bb, t.text2);
            ui.zone(r, Action::Toast(i as u32));
        }
    }
}
