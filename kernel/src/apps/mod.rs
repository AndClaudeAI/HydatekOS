//! Built-in HydatekOS applications.

use crate::gfx::Rect;
use crate::icons::Icon;
use crate::sys::Sys;
use crate::ui::{Key, Ui};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

pub mod browser;
pub mod calendar;
pub mod files;
pub mod grids;
pub mod mail;
pub mod messages;
pub mod music;
pub mod notes;
pub mod phone;
pub mod phonelink;
pub mod scripts;
pub mod settings;
pub mod slidedraw;
pub mod slides;
pub mod terminal;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AppKind {
    Files,
    Browser,
    Messages,
    Mail,
    Calendar,
    Notes,
    Music,
    Settings,
    Terminal,
    PhoneLink,
    Phone,
    Camera,
    Scripts,
    Grids,
    Slides,
}

pub const DESKTOP_APPS: [AppKind; 13] = [
    AppKind::Files,
    AppKind::Browser,
    AppKind::Messages,
    AppKind::Mail,
    AppKind::Calendar,
    AppKind::Notes,
    AppKind::Scripts,
    AppKind::Grids,
    AppKind::Slides,
    AppKind::Music,
    AppKind::Settings,
    AppKind::Terminal,
    AppKind::PhoneLink,
];

impl AppKind {
    pub fn name(self) -> &'static str {
        match self {
            AppKind::Files => "Files",
            AppKind::Browser => "Browser",
            AppKind::Messages => "Messages",
            AppKind::Mail => "Mail",
            AppKind::Calendar => "Calendar",
            AppKind::Notes => "Notes",
            AppKind::Music => "Music",
            AppKind::Settings => "Settings",
            AppKind::Terminal => "Terminal",
            AppKind::PhoneLink => "Phone Link",
            AppKind::Phone => "Phone",
            AppKind::Camera => "Camera",
            AppKind::Scripts => "Hyda Scripts",
            AppKind::Grids => "Hyda Grids",
            AppKind::Slides => "Hyda Slides",
        }
    }
    pub fn icon(self) -> Icon {
        match self {
            AppKind::Files => Icon::Folder,
            AppKind::Browser => Icon::Globe,
            AppKind::Messages => Icon::Chat,
            AppKind::Mail => Icon::Mail,
            AppKind::Calendar => Icon::Calendar,
            AppKind::Notes => Icon::Doc,
            AppKind::Music => Icon::Music,
            AppKind::Settings => Icon::Sliders,
            AppKind::Terminal => Icon::Terminal,
            AppKind::PhoneLink => Icon::Link,
            AppKind::Phone => Icon::Phone,
            AppKind::Camera => Icon::Camera,
            AppKind::Scripts => Icon::Scripts,
            AppKind::Grids => Icon::Sheet,
            AppKind::Slides => Icon::Slides,
        }
    }
    /// Accent-coloured icon (as on the mobile home screen)?
    pub fn warm(self) -> bool {
        matches!(self, AppKind::Files | AppKind::Calendar)
    }
    /// Default window size (logical units).
    pub fn size(self) -> (i32, i32) {
        match self {
            AppKind::Files => (640, 400),
            AppKind::Settings => (700, 590),
            AppKind::Terminal => (600, 380),
            AppKind::PhoneLink => (760, 500),
            AppKind::Calendar => (700, 460),
            AppKind::Music => (560, 400),
            AppKind::Scripts => (900, 600),
            AppKind::Grids => (920, 600),
            AppKind::Slides => (1080, 680),
            AppKind::Browser => (1000, 640),
            _ => (660, 430),
        }
    }
}

pub const HEADER: i32 = 44;

/// Phone-width layout?
pub fn compact(r: Rect) -> bool {
    r.w < 540
}

pub trait App {
    fn kind(&self) -> AppKind;
    /// Draw into `r` (the whole window, including its 44-unit header row whose
    /// right-hand 110 units belong to the window controls).
    fn render(&mut self, ui: &mut Ui, r: Rect, sys: &Sys, inst: u32);
    fn action(&mut self, _code: u32, _double: bool, _sys: &mut Sys) {}
    fn key(&mut self, _k: Key, _gen: bool, _sys: &mut Sys) {}
    fn scroll(&mut self, _dy: i32) {}
    /// Pointer position (logical) at the time of the next `action` call.
    fn mouse(&mut self, _x: i32, _y: i32) {}
    /// The pointer moved while the button, pressed on one of this app's
    /// zones, is still down.
    fn drag(&mut self, _x: i32, _y: i32) {}
    /// Menu bar entries: (menu index 0..3 = File/Edit/View/Go) -> items.
    fn menu(&self, _idx: usize) -> Vec<(&'static str, u32)> {
        Vec::new()
    }
    /// Called every tick (~100 Hz).
    fn tick(&mut self, _sys: &mut Sys) {}
    /// Called before the window closes.
    fn close(&mut self, _sys: &mut Sys) {}
    /// Wants a redraw every tick (animations, caret blink)?
    fn animating(&self) -> bool {
        false
    }
    fn open_path(&mut self, _path: &str, _sys: &mut Sys) {}
    /// Wants the whole screen (a slideshow): drawn without window, menu bar or dock.
    fn fullscreen(&self) -> bool {
        false
    }
}

pub fn create(kind: AppKind, sys: &mut Sys) -> Box<dyn App> {
    match kind {
        AppKind::Files => Box::new(files::Files::new()),
        AppKind::Notes => Box::new(notes::Notes::new(sys)),
        AppKind::Settings => Box::new(settings::Settings::new(sys)),
        AppKind::Calendar => Box::new(calendar::Calendar::new(sys)),
        AppKind::Terminal => Box::new(terminal::Terminal::new(sys)),
        AppKind::PhoneLink => Box::new(phonelink::PhoneLink::new()),
        AppKind::Messages => Box::new(messages::Messages::new()),
        AppKind::Mail => Box::new(mail::Mail::new()),
        AppKind::Browser => Box::new(browser::Browser::new()),
        AppKind::Music => Box::new(music::Music::new()),
        AppKind::Phone => Box::new(phone::Phone::new()),
        AppKind::Camera => Box::new(phone::Camera),
        AppKind::Scripts => Box::new(scripts::Scripts::new()),
        AppKind::Grids => Box::new(grids::Grids::new()),
        AppKind::Slides => Box::new(slides::Slides::new()),
    }
}

// ---- shared helpers for app layouts ---------------------------------------

/// Sidebar list item. Returns nothing; registers the zone.
pub fn side_item(ui: &mut Ui, r: Rect, label: &str, selected: bool, a: crate::ui::Action) {
    let t = ui.t;
    if selected {
        ui.rrect(r, 8, t.accent);
        ui.text_in(Rect::new(r.x + 12, r.y, r.w - 16, r.h), crate::font::Face::Semibold, 13, label, t.on_accent, 0);
    } else {
        if ui.hot(a) {
            ui.rrect(r, 8, t.hover);
        }
        ui.text_in(Rect::new(r.x + 12, r.y, r.w - 16, r.h), crate::font::Face::Regular, 13, label, t.text, 0);
    }
    ui.zone(r, a);
}

/// Simple line editor used by text fields.
#[derive(Default, Clone)]
pub struct LineEdit {
    pub text: String,
}

impl LineEdit {
    pub fn key(&mut self, k: Key) -> bool {
        match k {
            Key::Char(c) if !c.is_control() => {
                if self.text.len() < 200 {
                    self.text.push(c);
                }
                true
            }
            Key::Backspace => {
                self.text.pop();
                true
            }
            _ => false,
        }
    }
}

pub const WIN_RADIUS: i32 = 14;

/// Fill `p` inside window `win`, rounding only the corners `p` shares with
/// the window so panels follow the window's rounded outline.
pub fn panel(ui: &mut Ui, win: Rect, p: Rect, c: crate::gfx::Color) {
    let rad = WIN_RADIUS;
    ui.rrect(p, rad, c);
    let corners = [(p.x, p.y, win.x, win.y), (p.r() - rad, p.y, win.r() - rad, win.y), (p.x, p.b() - rad, win.x, win.b() - rad), (p.r() - rad, p.b() - rad, win.r() - rad, win.b() - rad)];
    for (x, y, wx, wy) in corners {
        if x != wx || y != wy {
            ui.rect(Rect::new(x, y, rad, rad), c);
        }
    }
}
