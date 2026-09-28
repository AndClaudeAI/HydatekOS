//! Terminal: the HydatekOS shell (hsh).

use super::{App, AppKind, DESKTOP_APPS, HEADER};
use crate::font::Face;
use crate::fs::{join, split};
use crate::gfx::{Color, Rect};
use crate::sys::{Req, Sys};
use crate::ui::{Action, Key, Ui};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub struct Terminal {
    /// the signed-in account's name, for the prompt
    login: String,
    cwd: String,
    out: Vec<String>,
    input: String,
    history: Vec<String>,
    hpos: usize,
    scroll: i32,
}

const HELP: &[&str] = &[
    "Commands:",
    "  ls [dir]          list a directory      cd <dir>        change directory",
    "  cat <file>        print a file          pwd             current directory",
    "  echo <text> [> f] print / write a file  touch <file>    create empty file",
    "  mkdir <dir>       create a directory    rm <path>       delete (to Bin)",
    "  mv <from> <to>    move or rename        open <app|file> launch an app",
    "  date              date and time         mem             memory usage",
    "  uname             system name           neofetch        system summary",
    "  theme dark|light  switch palette        clear           clear the screen",
    "  reboot            restart               shutdown        power off",
    "  lock              lock the screen       whoami          who is signed in",
    "  users             everyone's accounts",
];

impl Terminal {
    pub fn new(sys: &Sys) -> Terminal {
        // the prompt's name: the first name in lower case, or the account id
        let first: String = crate::profile::first_name(&sys.profile.name).chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
        Terminal {
            login: if first.is_empty() { sys.user.clone() } else { first },
            cwd: "/home".to_string(),
            out: vec!["HydatekOS shell (hsh) 0.1 — type 'help' for commands.".to_string(), String::new()],
            input: String::new(),
            history: vec![],
            hpos: 0,
            scroll: 0,
        }
    }

    fn resolve(&self, p: &str) -> String {
        let base = if p.starts_with('/') {
            String::new()
        } else if let Some(rest) = p.strip_prefix('~') {
            return self.resolve(&format!("/home{}", rest));
        } else {
            self.cwd.clone()
        };
        let mut parts: Vec<&str> = split(&base);
        for s in split(p) {
            match s {
                "." => {}
                ".." => {
                    parts.pop();
                }
                x => parts.push(x),
            }
        }
        format!("/{}", parts.join("/"))
    }

    fn prompt(&self) -> String {
        let d = if self.cwd == "/home" { "~".to_string() } else if let Some(r) = self.cwd.strip_prefix("/home/") { format!("~/{}", r) } else { self.cwd.clone() };
        format!("{}@hydatek:{}$ ", self.login, d)
    }

    fn print(&mut self, s: &str) {
        for l in s.split('\n') {
            self.out.push(l.to_string());
        }
    }

    fn run(&mut self, line: &str, sys: &mut Sys) {
        let p = self.prompt();
        self.out.push(format!("{}{}", p, line));
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.history.push(line.to_string());
        self.hpos = self.history.len();
        let (cmd, rest) = match line.find(' ') {
            Some(i) => (&line[..i], line[i + 1..].trim()),
            None => (line, ""),
        };
        let args: Vec<&str> = rest.split_whitespace().collect();
        match cmd {
            "help" => {
                for l in HELP {
                    self.out.push(l.to_string());
                }
            }
            "clear" => self.out.clear(),
            "pwd" => self.print(&self.cwd.clone()),
            "ls" => {
                let path = self.resolve(args.first().copied().unwrap_or("."));
                if !sys.fs.is_dir(&path) {
                    return self.print(&format!("ls: {}: no such directory", path));
                }
                let items = sys.fs.list(&path);
                if items.is_empty() {
                    return;
                }
                let mut line = String::new();
                for (n, d, _) in items {
                    let e = if d { format!("{}/", n) } else { n };
                    if line.len() + e.len() > 70 {
                        self.out.push(core::mem::take(&mut line));
                    }
                    line.push_str(&format!("{:<22}", e));
                }
                self.out.push(line);
            }
            "cd" => {
                let path = self.resolve(args.first().copied().unwrap_or("~"));
                if sys.fs.is_dir(&path) {
                    self.cwd = path;
                } else {
                    self.print(&format!("cd: {}: no such directory", path));
                }
            }
            "cat" => {
                for a in args {
                    let path = self.resolve(a);
                    match sys.fs.read(&path) {
                        Some(d) => {
                            let s = String::from_utf8_lossy(&d).to_string();
                            self.print(&s);
                        }
                        None => self.print(&format!("cat: {}: no such file", a)),
                    }
                }
            }
            "echo" => {
                if let Some(i) = rest.find('>') {
                    let append = rest[i..].starts_with(">>");
                    let text = rest[..i].trim();
                    let path = self.resolve(rest[i..].trim_start_matches('>').trim());
                    let mut data = if append { sys.fs.read(&path).unwrap_or_default() } else { vec![] };
                    data.extend_from_slice(text.as_bytes());
                    data.push(b'\n');
                    if !sys.fs.write(&path, &data) {
                        self.print("echo: cannot write file");
                    }
                } else {
                    self.print(rest);
                }
            }
            "touch" => {
                for a in args {
                    let p = self.resolve(a);
                    if !sys.fs.exists(&p) {
                        sys.fs.write(&p, b"");
                    }
                }
            }
            "mkdir" => {
                for a in args {
                    let p = self.resolve(a);
                    sys.fs.mkdir(&p);
                }
            }
            "rm" => {
                for a in args {
                    let p = self.resolve(a);
                    if !sys.fs.exists(&p) || p == "/" || p == "/home" {
                        self.print(&format!("rm: {}: cannot remove", a));
                    } else if p.starts_with("/trash") {
                        sys.fs.remove(&p);
                    } else {
                        let dst = sys.fs.unique("/trash", crate::fs::basename(&p), "");
                        sys.fs.rename(&p, &dst);
                    }
                }
            }
            "mv" if args.len() == 2 => {
                let (a, mut b) = (self.resolve(args[0]), self.resolve(args[1]));
                if sys.fs.is_dir(&b) {
                    b = join(&b, crate::fs::basename(&a));
                }
                if !sys.fs.rename(&a, &b) {
                    self.print("mv: failed");
                }
            }
            "date" => {
                let t = sys.now;
                self.print(&format!("{} {:02}:{:02}:{:02}", sys.date_long(), t.hour, t.minute, t.second));
            }
            "mem" => {
                let (u, t) = crate::heap::HEAP.stats();
                self.print(&format!("heap: {} KB used of {} MB · RAM: {} MB", u >> 10, t >> 20, sys.mem_total >> 20));
            }
            "uname" => self.print("HydatekOS 0.1 Dune x86_64-uefi"),
            "whoami" => {
                let kind = if sys.is_admin() { "administrator" } else { "standard account" };
                let name = if sys.profile.ready() { sys.profile.name.clone() } else { sys.user.clone() };
                self.print(&format!("{} ({}, {})", name, sys.user, kind));
            }
            "users" => {
                if sys.people.is_empty() {
                    self.print("one account (not set up yet)");
                }
                for p in &sys.people {
                    let line = format!("{} {:<24} {:<5} {}{}", if p.id == sys.user { "*" } else { " " }, p.name, p.id, if p.admin { "administrator" } else { "standard" }, if p.new { ", not set up yet" } else { "" });
                    self.out.push(line);
                }
            }
            "neofetch" => {
                let (w, h, _) = sys.screen;
                let (u, _) = crate::heap::HEAP.stats();
                let info = [
                    format!("hydatek@{}", if sys.mobile_shell { "mobile" } else { "desktop" }),
                    "-------------".to_string(),
                    "OS: HydatekOS 0.1 \"Dune\"".to_string(),
                    "Kernel: hydatek (Rust, no_std)".to_string(),
                    format!("Resolution: {}x{}", w, h),
                    format!("Theme: {}", if sys.dark { "Dusk" } else { "Dune" }),
                    format!("Memory: {} MB / {} MB", u >> 20, sys.mem_total >> 20),
                    format!("Firmware: {}", sys.firmware),
                ];
                let logo = ["   .----.   ", "  /  ..  \\  ", " |  |  |  | ", " |  |  |  | ", " |__|  |__| ", "            ", "            ", "            "];
                for i in 0..info.len() {
                    self.out.push(format!("{}  {}", logo[i], info[i]));
                }
            }
            "theme" => match args.first().copied() {
                Some("dark") => {
                    sys.dark = true;
                    sys.reqs.push(Req::SaveSettings);
                }
                Some("light") => {
                    sys.dark = false;
                    sys.reqs.push(Req::SaveSettings);
                }
                _ => self.print("usage: theme dark|light"),
            },
            "open" => {
                let target = rest;
                if let Some(k) = DESKTOP_APPS.iter().find(|k| k.name().eq_ignore_ascii_case(target) || k.name().replace(' ', "").eq_ignore_ascii_case(target)) {
                    sys.reqs.push(Req::Open(*k));
                } else {
                    let p = self.resolve(target);
                    if sys.fs.exists(&p) {
                        sys.reqs.push(Req::OpenPath(p));
                    } else {
                        self.print(&format!("open: {}: not an app or file", target));
                    }
                }
            }
            "lock" => sys.reqs.push(Req::Lock),
            "reboot" => sys.reqs.push(Req::Reboot),
            "shutdown" | "poweroff" => sys.reqs.push(Req::Shutdown),
            _ => self.print(&format!("hsh: {}: command not found", cmd)),
        }
    }
}

impl App for Terminal {
    fn kind(&self) -> AppKind {
        AppKind::Terminal
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, _sys: &Sys, inst: u32) {
        let t = ui.t;
        let bg = Color::rgb(0x1B1A24);
        let body = Rect::new(r.x, r.y + HEADER, r.w, r.h - HEADER);
        super::panel(ui, r, body, bg);
        ui.text_in(Rect::new(r.x + 20, r.y, 300, HEADER), Face::Semibold, 15, "Terminal", t.text, 0);
        let lh = 18;
        let inner = body.inset(14);
        let rows = inner.h / lh;
        let total = self.out.len() as i32 + 1;
        let max_scroll = (total - rows).max(0);
        self.scroll = self.scroll.clamp(0, max_scroll);
        let first = (max_scroll - self.scroll).max(0) as usize;
        let old = ui.clip_in(inner);
        let mut y = inner.y + 13;
        let fg = Color::rgb(0xECE5DA);
        for l in self.out.iter().skip(first).take(rows as usize) {
            let col = if l.starts_with("hydatek:") { Color::rgb(0xE4B783) } else { fg };
            ui.text(inner.x, y, Face::Mono, 13, l, col);
            y += lh;
        }
        if self.scroll == 0 {
            let p = self.prompt();
            let w = ui.text(inner.x, y, Face::Mono, 13, &p, Color::rgb(0xE4B783));
            let w2 = ui.text(inner.x + w, y, Face::Mono, 13, &self.input, fg);
            if (ui.ticks / 50) % 2 == 0 {
                ui.rect(Rect::new(inner.x + w + w2 + 1, y - 12, 8, 15), t.accent);
            }
        }
        ui.set_clip(old);
        ui.zone(body, Action::App(inst, 0));
    }

    fn key(&mut self, k: Key, ctrl: bool, sys: &mut Sys) {
        self.scroll = 0;
        match k {
            Key::Char('l') if ctrl => self.out.clear(),
            Key::Char('c') if ctrl => {
                let p = self.prompt();
                self.out.push(format!("{}{}^C", p, self.input));
                self.input.clear();
            }
            Key::Char(c) if !c.is_control() && !ctrl => self.input.push(c),
            Key::Backspace => {
                self.input.pop();
            }
            Key::Enter => {
                let line = core::mem::take(&mut self.input);
                self.run(&line, sys);
                if self.out.len() > 500 {
                    self.out.drain(0..100);
                }
            }
            Key::Up if self.hpos > 0 => {
                self.hpos -= 1;
                self.input = self.history[self.hpos].clone();
            }
            Key::Down => {
                if self.hpos + 1 < self.history.len() {
                    self.hpos += 1;
                    self.input = self.history[self.hpos].clone();
                } else {
                    self.hpos = self.history.len();
                    self.input.clear();
                }
            }
            Key::Tab => {
                // complete the last word against the current directory
                let word_start = self.input.rfind(' ').map(|i| i + 1).unwrap_or(0);
                let word = self.input[word_start..].to_string();
                let (dir, stem) = match word.rfind('/') {
                    Some(i) => (self.resolve(&word[..i + 1]), word[i + 1..].to_string()),
                    None => (self.cwd.clone(), word.clone()),
                };
                let m: Vec<String> = sys.fs.list(&dir).into_iter().filter(|e| e.0.starts_with(&stem)).map(|e| e.0).collect();
                if m.len() == 1 {
                    self.input.push_str(&m[0][stem.len()..]);
                }
            }
            _ => {}
        }
    }

    fn scroll(&mut self, dy: i32) {
        self.scroll -= dy * 3;
    }

    fn animating(&self) -> bool {
        true
    }

}
