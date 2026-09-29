//! Claude: HydatekOS's assistant, made by Anthropic. A conversation sent to
//! the Anthropic API with the account's own API key (web/claude.rs builds
//! the requests; the main loop's fetcher carries them over https).

use super::{App, AppKind, LineEdit, HEADER};
use crate::font::Face;
use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::{Req, Sys, DAYS, MONTHS};
use crate::ui::{Action, Key, Ui};
use crate::web::claude::{self, Turn};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

const C_INPUT: u32 = 1;
const C_SEND: u32 = 2;
const C_NEW: u32 = 3;
const C_KEY_FIELD: u32 = 4;
const C_KEY_SAVE: u32 = 5;
const C_RETRY: u32 = 6;
const C_STOP: u32 = 7;
const C_SETTINGS: u32 = 8;
const C_SUGGEST: u32 = 20;

/// Index of Settings › Assistant (apps::settings::SECTIONS).
pub const SETTINGS_SECTION: usize = 10;

const SUGGESTIONS: [&str; 3] = ["What can the Gen and Aux keys do?", "Help me write a thank-you note", "Plan a week of simple dinners"];

pub struct Assistant {
    turns: Vec<Turn>,
    input: LineEdit,
    focus: bool,
    /// waiting for Claude's answer
    pending: Option<u32>,
    /// waiting for the list of models (to choose one the first time)
    listing: Option<u32>,
    /// the last request failed: why
    error: String,
    /// a note under Claude's last answer (it stopped early)
    note: Option<&'static str>,
    /// pixels scrolled up from the newest message, and the most there is
    scroll: i32,
    max_scroll: i32,
    /// the API key being typed (before one is set up)
    key: LineEdit,
    key_focus: bool,
    key_msg: String,
}

impl Assistant {
    pub fn new(sys: &Sys) -> Assistant {
        Assistant {
            turns: Vec::new(),
            input: LineEdit::default(),
            focus: sys.has_claude(),
            pending: None,
            listing: None,
            error: String::new(),
            note: None,
            scroll: 0,
            max_scroll: 0,
            key: LineEdit::default(),
            key_focus: !sys.has_claude(),
            key_msg: String::new(),
        }
    }

    fn busy(&self) -> bool {
        self.pending.is_some() || self.listing.is_some()
    }

    fn send(&mut self, sys: &mut Sys) {
        let text = String::from(self.input.text.trim());
        if text.is_empty() || self.busy() || !sys.has_claude() {
            return;
        }
        self.turns.push(Turn { user: true, text });
        self.input.clear();
        self.ask(sys);
    }

    /// Send the conversation (it ends with the person's turn).
    fn ask(&mut self, sys: &mut Sys) {
        self.error.clear();
        self.note = None;
        self.scroll = 0;
        if sys.claude_model.is_empty() {
            // first use: find the models this key can use
            self.listing = Some(sys.web.get_api(claude::MODELS_URL, claude::headers(&sys.claude_key)));
            return;
        }
        let now = sys.now;
        let date = format!(
            "{} {} {} {}",
            DAYS[crate::sys::weekday(now.year as i32, now.month as i32, now.day as i32)],
            now.day,
            MONTHS[(now.month as usize).clamp(1, 12) - 1],
            now.year
        );
        let system = claude::system_prompt(&sys.profile.name, &date);
        let body = claude::request(&sys.claude_model, &system, &self.turns);
        self.pending = Some(sys.web.post_api(claude::MESSAGES_URL, body, claude::headers(&sys.claude_key), claude::PATIENCE_MS));
    }

    fn stop(&mut self, sys: &mut Sys) {
        for id in [self.pending.take(), self.listing.take()].into_iter().flatten() {
            sys.web.stop(id);
        }
        self.error = String::from("Stopped.");
    }

    fn new_chat(&mut self, sys: &mut Sys) {
        if self.busy() {
            self.stop(sys);
        }
        self.turns.clear();
        self.error.clear();
        self.note = None;
        self.scroll = 0;
        self.focus = true;
    }

    fn save_key(&mut self, sys: &mut Sys) {
        let k = String::from(self.key.text.trim());
        if !claude::key_ok(&k) {
            self.key_msg = String::from("That isn't an Anthropic API key: they start with sk-ant-");
            self.key_focus = true;
            return;
        }
        sys.claude_key = k;
        sys.claude_model.clear();
        sys.save_assistant();
        self.key.clear();
        self.key_msg.clear();
        self.key_focus = false;
        self.focus = true;
    }

    /// The last turn is the person's and has no answer yet.
    fn unanswered(&self) -> bool {
        self.turns.last().map_or(false, |t| t.user)
    }

    // ---- drawing -------------------------------------------------------------

    fn render_setup(&mut self, ui: &mut Ui, r: Rect, inst: u32) {
        let t = ui.t;
        let w = (r.w - 48).min(440);
        let x = r.x + (r.w - w) / 2;
        let mut y = r.y + ((r.h - 330) / 2).max(16);
        let badge = Rect::new(x + (w - 64) / 2, y, 64, 64);
        ui.rrect(badge, 18, t.accent);
        ui.icon_in(Icon::Spark, badge, 34, t.on_accent);
        y += 96;
        ui.text_in(Rect::new(x, y - 20, w, 26), Face::Semibold, 18, "Claude is your assistant", t.text, 1);
        y += 16;
        let about = "HydatekOS's assistant is Claude, made by Anthropic. To start, paste an Anthropic API key: make one at console.anthropic.com under API keys.";
        for line in ui.wrap(Face::Regular, 13, about, w) {
            let lw = ui.tw(Face::Regular, 13, &line);
            ui.text(x + (w - lw) / 2, y, Face::Regular, 13, &line, t.text2);
            y += 19;
        }
        y += 10;
        let f = Rect::new(x, y, w - 90, 36);
        let masked: String = self.key.text.chars().map(|_| '•').collect();
        let before: String = self.key.before_caret().chars().map(|_| '•').collect();
        ui.field_at(f, &masked, &before, "sk-ant-…", self.key_focus, Action::App(inst, C_KEY_FIELD));
        ui.button(Rect::new(f.r() + 8, y + 1, 82, 34), "Save", Action::App(inst, C_KEY_SAVE), true);
        y += 58;
        let msg = if self.key_msg.is_empty() { "The key stays in your account on this computer. Messages go to Anthropic's API only." } else { self.key_msg.as_str() };
        let col = if self.key_msg.is_empty() { t.text3 } else { t.danger };
        for line in ui.wrap(Face::Regular, 12, msg, w) {
            let lw = ui.tw(Face::Regular, 12, &line);
            ui.text(x + (w - lw) / 2, y, Face::Regular, 12, &line, col);
            y += 18;
        }
    }

    /// The lines of a message wrapped to `w` (paragraphs kept).
    fn lines(ui: &Ui, text: &str, w: i32) -> Vec<String> {
        let mut out = Vec::new();
        for para in text.split('\n') {
            if para.trim().is_empty() {
                out.push(String::new());
            } else {
                out.extend(ui.wrap(Face::Regular, 13, para, w));
            }
        }
        out
    }

    fn render_chat(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let input_h = 58;
        let status_h = if self.error.is_empty() && self.note.is_none() { 0 } else { 30 };
        let area = Rect::new(r.x + 16, r.y + 8, r.w - 32, r.h - input_h - status_h - 12);
        if self.turns.is_empty() {
            // a welcome and some things to ask
            let first = sys.profile.first_name();
            let hello = if first.is_empty() { String::from("How can I help?") } else { format!("How can I help, {}?", first) };
            let cy = area.y + area.h / 2 - 90;
            let badge = Rect::new(area.x + (area.w - 52) / 2, cy, 52, 52);
            ui.rrect(badge, 15, t.accent);
            ui.icon_in(Icon::Spark, badge, 28, t.on_accent);
            ui.text_in(Rect::new(area.x, cy + 64, area.w, 28), Face::Semibold, 18, &hello, t.text, 1);
            let mut y = cy + 104;
            for (i, s) in SUGGESTIONS.iter().enumerate() {
                let sw = (ui.tw(Face::Regular, 13, s) + 32).min(area.w);
                let sr = Rect::new(area.x + (area.w - sw) / 2, y, sw, 32);
                let a = Action::App(inst, C_SUGGEST + i as u32);
                ui.rrect(sr, 16, if ui.hot(a) { t.hover } else { t.chip });
                let label = ui.fit(Face::Regular, 13, s, sw - 24);
                ui.text_in(sr, Face::Regular, 13, &label, t.text, 1);
                ui.zone(sr, a);
                y += 40;
            }
        } else {
            let old = ui.clip_in(area);
            let maxw = area.w * 4 / 5;
            // newest at the bottom; `scroll` pushes everything down
            let mut by = area.b() - 6 + self.scroll;
            if self.busy() {
                let dots = [".", "..", "..."][(ui.ticks / 40 % 3) as usize];
                let label = format!("Claude is thinking{}", dots);
                let w = ui.tw(Face::Regular, 13, "Claude is thinking...") + 28;
                by -= 34;
                ui.rrect(Rect::new(area.x, by, w, 34), 17, t.tile);
                ui.text(area.x + 14, by + 22, Face::Regular, 13, &label, t.text2);
                by -= 12;
            }
            let mut height = 0;
            for turn in self.turns.iter().rev() {
                let lines = Assistant::lines(ui, &turn.text, maxw - 28);
                let w = lines.iter().map(|l| ui.tw(Face::Regular, 13, l)).max().unwrap_or(0) + 28;
                let h = 16 + 19 * lines.len() as i32;
                by -= h;
                height += h + 12;
                if by < area.b() && by + h > area.y {
                    let x = if turn.user { area.r() - w } else { area.x };
                    let (bg, fg) = if turn.user { (t.accent, t.on_accent) } else { (t.tile, t.text) };
                    ui.rrect(Rect::new(x, by, w, h), 16, bg);
                    for (i, l) in lines.iter().enumerate() {
                        ui.text(x + 14, by + 22 + i as i32 * 19, Face::Regular, 13, l, fg);
                    }
                }
                by -= 12;
            }
            ui.set_clip(old);
            self.max_scroll = (height - area.h + 40).max(0);
            self.scroll = self.scroll.min(self.max_scroll);
        }
        // why the last request failed, or a note on the answer
        if status_h > 0 {
            let sy = area.b() + 4;
            let (msg, col) = if self.error.is_empty() { (self.note.unwrap_or(""), t.text2) } else { (self.error.as_str(), t.danger) };
            let retry = !self.error.is_empty() && self.unanswered() && !self.busy();
            let mw = r.w - 32 - if retry { 90 } else { 0 };
            let m = ui.fit(Face::Regular, 12, msg, mw);
            ui.text(r.x + 18, sy + 18, Face::Regular, 12, &m, col);
            if retry {
                ui.button(Rect::new(r.r() - 16 - 80, sy + 1, 80, 26), "Try again", Action::App(inst, C_RETRY), false);
            }
        }
        // the message box
        let ir = Rect::new(r.x + 16, r.b() - input_h + 10, r.w - 32 - 44, 36);
        ui.line(ir, &self.input, "Ask Claude", self.focus, Action::App(inst, C_INPUT));
        let sb = Rect::new(r.r() - 16 - 36, ir.y, 36, 36);
        if self.busy() {
            let a = Action::App(inst, C_STOP);
            ui.rrect(sb, 18, if ui.hot(a) { t.chip.mix(t.text, 30) } else { t.chip });
            ui.rrect(Rect::new(sb.x + 12, sb.y + 12, 12, 12), 3, t.text);
            ui.zone(sb, a);
        } else {
            let a = Action::App(inst, C_SEND);
            let ready = !self.input.text.trim().is_empty();
            let bg = if !ready { t.chip } else if ui.hot(a) { t.accent.mix(t.text, 40) } else { t.accent };
            ui.rrect(sb, 18, bg);
            ui.icon_in(Icon::Send, sb, 16, if ready { t.on_accent } else { t.text3 });
            ui.zone(sb, a);
        }
    }
}

impl App for Assistant {
    fn kind(&self) -> AppKind {
        AppKind::Assistant
    }

    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32) {
        let t = ui.t;
        let badge = Rect::new(r.x + 16, r.y + 9, 26, 26);
        ui.rrect(badge, 8, t.accent);
        ui.icon_in(Icon::Spark, badge, 15, t.on_accent);
        ui.text(r.x + 50, r.y + 21, Face::Semibold, 14, "Claude", t.text);
        ui.text(r.x + 50, r.y + 36, Face::Regular, 11, "Your assistant, by Anthropic", t.text3);
        if sys.has_claude() && r.w > 380 {
            ui.button(Rect::new(r.r() - 110 - 96, r.y + 8, 88, 28), "New chat", Action::App(inst, C_NEW), false);
            ui.icon_button(Rect::new(r.r() - 110 - 132, r.y + 8, 28, 28), Icon::Sliders, Action::App(inst, C_SETTINGS), 15);
        }
        ui.rect(Rect::new(r.x, r.y + HEADER, r.w, 1), t.line);
        let body = Rect::new(r.x, r.y + HEADER + 1, r.w, r.h - HEADER - 1);
        if sys.has_claude() {
            self.render_chat(ui, body, sys, inst);
        } else {
            self.render_setup(ui, body, inst);
        }
    }

    fn action(&mut self, code: u32, _double: bool, sys: &mut Sys) {
        self.focus = code == C_INPUT;
        self.key_focus = code == C_KEY_FIELD;
        match code {
            C_SEND => self.send(sys),
            C_NEW => self.new_chat(sys),
            C_STOP => self.stop(sys),
            C_RETRY if self.unanswered() && !self.busy() => self.ask(sys),
            C_KEY_SAVE => self.save_key(sys),
            C_SETTINGS => sys.reqs.push(Req::Settings(SETTINGS_SECTION)),
            c if (C_SUGGEST..C_SUGGEST + SUGGESTIONS.len() as u32).contains(&c) => {
                self.input.set(SUGGESTIONS[(c - C_SUGGEST) as usize]);
                self.send(sys);
                self.focus = true;
            }
            _ => {}
        }
    }

    fn key(&mut self, k: Key, gen: bool, sys: &mut Sys) {
        if !sys.has_claude() {
            match k {
                Key::Enter => self.save_key(sys),
                Key::Esc => self.key.clear(),
                _ => {
                    // a key has no spaces
                    if !matches!(k, Key::Char(' ')) && self.key.key_sys(k, gen, sys) {
                        self.key_focus = true;
                        self.key_msg.clear();
                    }
                }
            }
            return;
        }
        match k {
            Key::Enter => self.send(sys),
            Key::Esc if self.busy() => self.stop(sys),
            Key::Esc => self.input.clear(),
            Key::Char('n') if gen => self.new_chat(sys),
            Key::PageUp => self.scroll = (self.scroll + 200).min(self.max_scroll),
            Key::PageDown => self.scroll = (self.scroll - 200).max(0),
            _ => {
                if self.input.key_sys(k, gen, sys) {
                    self.focus = true;
                }
            }
        }
    }

    fn scroll(&mut self, dy: i32) {
        self.scroll = (self.scroll - dy * 40).clamp(0, self.max_scroll);
    }

    fn tick(&mut self, sys: &mut Sys) {
        if let Some(id) = self.listing {
            if let Some(res) = sys.web.take(id) {
                self.listing = None;
                let list = res.and_then(|resp| claude::models(resp.status, &resp.body));
                match list {
                    Ok(list) => {
                        if let Some(m) = claude::pick(&list) {
                            sys.claude_model = m.0.clone();
                            sys.save_assistant();
                            self.ask(sys);
                        }
                    }
                    Err(e) => self.error = e,
                }
            }
        }
        if let Some(id) = self.pending {
            if let Some(res) = sys.web.take(id) {
                self.pending = None;
                match res {
                    Ok(resp) => {
                        if resp.status == 404 {
                            // that model went away: choose again next time
                            sys.claude_model.clear();
                            sys.save_assistant();
                        }
                        match claude::answer(resp.status, &resp.body) {
                            Ok(a) => {
                                if !a.text.is_empty() {
                                    self.turns.push(Turn { user: false, text: a.text });
                                }
                                self.note = a.note;
                                self.scroll = 0;
                            }
                            Err(e) => self.error = e,
                        }
                    }
                    Err(e) => self.error = e,
                }
            }
        }
    }

    fn animating(&self) -> bool {
        self.focus || self.key_focus || self.busy()
    }

    fn menu(&self, idx: usize) -> Vec<(&'static str, u32)> {
        match idx {
            0 => alloc::vec![("New Chat", C_NEW)],
            _ => Vec::new(),
        }
    }
}
