//! Keyboard shortcuts: the list shown by Gen+/ (and Settings › Keyboard), and
//! keycaps for drawing them.
//!
//! **Gen** is HydatekOS's shortcut key, like Ctrl on Windows and Command on a
//! Mac. It's the logo key (⊞ on PC keyboards, ⌘ on Apple keyboards); Ctrl
//! works as Gen too, unless it's turned off in Settings › Keyboard.

use crate::font::Face;
use crate::gfx::Rect;
use crate::ui::Ui;

pub struct Group {
    pub title: &'static str,
    pub keys: &'static [(&'static str, &'static str)],
}

pub const SHEET: &[Group] = &[
    Group {
        title: "Everywhere",
        keys: &[
            ("Gen+Space", "Open an app"),
            ("Gen+Tab", "Next window (Aux+Tab too)"),
            ("Gen+W", "Close the window"),
            ("Gen+Q", "Quit the app"),
            ("Gen+,", "Settings"),
            ("Gen+/", "These shortcuts"),
            ("F11", "Maximise the window"),
            ("F12", "Lock the screen"),
        ],
    },
    Group {
        title: "Editing",
        keys: &[
            ("Gen+Z", "Undo"),
            ("Gen+Y", "Redo"),
            ("Gen+X", "Cut"),
            ("Gen+C", "Copy"),
            ("Gen+V", "Paste"),
            ("Gen+A", "Select all"),
            ("Aux+→", "Next word (← back)"),
        ],
    },
    Group {
        title: "Documents",
        keys: &[
            ("Gen+N", "New"),
            ("Gen+O", "Open"),
            ("Gen+S", "Save"),
            ("Gen+B", "Bold"),
            ("Gen+I", "Italic"),
            ("Gen+U", "Underline"),
            ("Gen+E", "Centre (L left, R right)"),
        ],
    },
    Group {
        title: "Apps",
        keys: &[
            ("Gen+M", "Slides: new slide"),
            ("Gen+D", "Slides: duplicate"),
            ("F5", "Slides: play"),
            ("Gen+=", "Scripts: zoom in (− out)"),
            ("Gen+L", "Browser: address bar"),
            ("Gen+R", "Browser: reload"),
        ],
    },
    Group {
        title: "Aux characters",
        keys: &[
            ("Aux+N", "₦  Naira"),
            ("Aux+E", "€  Euro (3 £, Y ¥, 4 ¢)"),
            ("Aux+-", "–  en dash (Shift: — em)"),
            ("Aux+;", "…  ellipsis"),
            ("Aux+8", "•  bullet (0 °)"),
            ("Aux+[", "“  quotes (Shift: ”)"),
            ("Aux+G", "©  (R ®, 2 ™)"),
        ],
    },
];

/// Draw `combo` ("Gen+S") as keycaps with their left edge at `x`, centred on
/// `cy`; returns the width. Gen is in the accent colour, Aux in the text
/// colour.
pub fn keycaps(ui: &mut Ui, x: i32, cy: i32, combo: &str, size: i32) -> i32 {
    let t = ui.t;
    let h = size + 10;
    let mut pen = x;
    for (i, part) in combo.split('+').filter(|p| !p.is_empty()).enumerate() {
        if i > 0 {
            pen += 4;
        }
        let w = (ui.tw(Face::Medium, size, part) + 12).max(h);
        let r = Rect::new(pen, cy - h / 2, w, h);
        let (bg, fg) = match part {
            "Gen" => (Some(t.accent), t.on_accent),
            "Aux" => (Some(t.text), t.surface),
            _ => (None, t.text),
        };
        ui.rrect(r, 6, bg.unwrap_or(t.chip));
        if bg.is_none() {
            ui.stroke(r, 6, 1, t.line);
        }
        ui.text_in(r, if bg.is_some() { Face::Semibold } else { Face::Medium }, size, part, fg, 1);
        pen += w;
    }
    pen - x
}
