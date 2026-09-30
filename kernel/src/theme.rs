//! HydatekOS colour system ("Dune" light and "Dusk" dark).

use crate::gfx::Color;

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub sky: Color,
    pub sky2: Color,
    pub surface: Color,
    pub sidebar: Color,
    pub tile: Color,
    pub chip: Color,
    pub text: Color,
    pub text2: Color,
    pub text3: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub line: Color,
    pub sun: Color,
    pub dune1: Color,
    pub dune2: Color,
    pub dune3: Color,
    pub dock: Color,
    pub dock_btn: Color,
    pub dock_icon: Color,
    pub bar: Color,
    pub hover: Color,
    pub danger: Color,
}

pub const ACCENTS: [(&str, u32, u32); 4] = [
    ("Ember", 0xB5581B, 0xD9793A),
    ("Ocean", 0x2F6690, 0x5B9BD5),
    ("Moss", 0x4F7942, 0x7FAF6B),
    ("Plum", 0x7B3F7A, 0xB072AE),
];

pub fn theme(dark: bool, accent: usize) -> Theme {
    let (_, al, ad) = ACCENTS[accent % ACCENTS.len()];
    theme_with(dark, (al, ad))
}

/// The theme with any accent: (for light, for dark).
pub fn theme_with(dark: bool, (al, ad): (u32, u32)) -> Theme {
    if !dark {
        Theme {
            dark,
            sky: Color::rgb(0xF0E9DE),
            sky2: Color::rgb(0xEBE2D4),
            surface: Color::rgb(0xF9F6F0),
            sidebar: Color::rgb(0xF3EDE4),
            tile: Color::rgb(0xF1E6D9),
            chip: Color::rgb(0xEAE3D7),
            text: Color::rgb(0x1E1B2C),
            text2: Color::rgb(0x5E5866),
            text3: Color::rgb(0x8C8590),
            accent: Color::rgb(al),
            on_accent: Color::rgb(0xFFFFFF),
            line: Color::rgb(0xE8E0D4),
            sun: Color::rgb(0xE4B783),
            dune1: Color::rgb(0xDCC8AB),
            dune2: Color::rgb(0xC4895E),
            dune3: Color::rgb(0x2B2A48),
            dock: Color::rgb(0x1F1D2D),
            dock_btn: Color::rgb(0x34324A),
            dock_icon: Color::rgb(0xF4EFE7),
            bar: Color::rgba(0xF8F4EE, 235),
            hover: Color::rgba(0x1E1B2C, 14),
            danger: Color::rgb(0xB8322A),
        }
    } else {
        Theme {
            dark,
            sky: Color::rgb(0x1F1D29),
            sky2: Color::rgb(0x25222F),
            surface: Color::rgb(0x23202C),
            sidebar: Color::rgb(0x1C1A24),
            tile: Color::rgb(0x322D38),
            chip: Color::rgb(0x322E3B),
            text: Color::rgb(0xF3EDE4),
            text2: Color::rgb(0xB8B0A6),
            text3: Color::rgb(0x837C78),
            accent: Color::rgb(ad),
            on_accent: Color::rgb(0x1A1712),
            line: Color::rgb(0x34303D),
            sun: Color::rgb(0xC98E55),
            dune1: Color::rgb(0x4E4038),
            dune2: Color::rgb(0x7E5236),
            dune3: Color::rgb(0x15142A),
            dock: Color::rgb(0x121119),
            dock_btn: Color::rgb(0x2A2838),
            dock_icon: Color::rgb(0xF4EFE7),
            bar: Color::rgba(0x1A1822, 235),
            hover: Color::rgba(0xFFFFFF, 18),
            danger: Color::rgb(0xE0625A),
        }
    }
}

/// The dynamic theme: built from a wallpaper's palette. The accent is the
/// wallpaper's most vivid colour; the surfaces, lines, menu bar, dock and
/// secondary ink are tones of its main colour, at the lightness the plain
/// theme uses, so the layout reads the same. Text keeps its contrast.
pub fn theme_matched(dark: bool, p: &crate::personal::Palette) -> Theme {
    use crate::personal::{readable, sat_light, tone};
    let mut t = theme_with(dark, p.accent);
    let Some(tint) = p.tint else { return t };
    // a muted wallpaper gives muted surfaces
    let k = sat_light(tint).0.clamp(250, 700);
    let at = |s: i32, l: i32| Color::rgb(tone(tint, s * k / 700, l));
    let keep_alpha = |c: Color, a: u32| Color::rgba(c.0, a as u8);
    if !dark {
        t.sky = at(220, 930);
        t.sky2 = at(240, 905);
        t.surface = at(170, 972);
        t.sidebar = at(240, 948);
        t.tile = at(320, 925);
        t.chip = at(260, 915);
        t.line = at(220, 880);
        t.bar = keep_alpha(at(200, 965), 235);
        t.dock = at(420, 130);
        t.dock_btn = at(360, 220);
        t.hover = keep_alpha(at(400, 200), 16);
        t.text2 = Color::rgb(readable(tone(tint, 160 * k / 700, 380), t.surface.0 & 0xFF_FFFF, 450));
        t.text3 = Color::rgb(readable(tone(tint, 120 * k / 700, 560), t.surface.0 & 0xFF_FFFF, 300));
    } else {
        t.sky = at(260, 115);
        t.sky2 = at(260, 135);
        t.surface = at(270, 125);
        t.sidebar = at(290, 100);
        t.tile = at(230, 195);
        t.chip = at(230, 200);
        t.line = at(210, 215);
        t.bar = keep_alpha(at(270, 100), 235);
        t.dock = at(340, 70);
        t.dock_btn = at(280, 165);
        t.text2 = Color::rgb(readable(tone(tint, 160 * k / 700, 700), t.surface.0 & 0xFF_FFFF, 450));
        t.text3 = Color::rgb(readable(tone(tint, 110 * k / 700, 520), t.surface.0 & 0xFF_FFFF, 300));
    }
    // the dark accent was made to stand out on the plain dark surface: make
    // sure it still does on this one
    if dark {
        t.accent = Color::rgb(readable(t.accent.0 & 0xFF_FFFF, t.surface.0 & 0xFF_FFFF, 300));
    }
    t
}

impl Theme {
    /// Part way from `self` to `o` (`k` of 1000): a theme changing colour.
    pub fn blend(&self, o: &Theme, k: i32) -> Theme {
        let k = k.clamp(0, 1000) as u32;
        let m = |a: Color, b: Color| {
            let al = (a.a() * (1000 - k) + b.a() * k) / 1000;
            Color::rgba(crate::gfx::lerp(a.0 & 0xFF_FFFF, b.0 & 0xFF_FFFF, k * 256 / 1000), al as u8)
        };
        Theme {
            dark: if k < 500 { self.dark } else { o.dark },
            sky: m(self.sky, o.sky),
            sky2: m(self.sky2, o.sky2),
            surface: m(self.surface, o.surface),
            sidebar: m(self.sidebar, o.sidebar),
            tile: m(self.tile, o.tile),
            chip: m(self.chip, o.chip),
            text: m(self.text, o.text),
            text2: m(self.text2, o.text2),
            text3: m(self.text3, o.text3),
            accent: m(self.accent, o.accent),
            on_accent: m(self.on_accent, o.on_accent),
            line: m(self.line, o.line),
            sun: m(self.sun, o.sun),
            dune1: m(self.dune1, o.dune1),
            dune2: m(self.dune2, o.dune2),
            dune3: m(self.dune3, o.dune3),
            dock: m(self.dock, o.dock),
            dock_btn: m(self.dock_btn, o.dock_btn),
            dock_icon: m(self.dock_icon, o.dock_icon),
            bar: m(self.bar, o.bar),
            hover: m(self.hover, o.hover),
            danger: m(self.danger, o.danger),
        }
    }
}
