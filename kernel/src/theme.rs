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

/// The theme matched to a wallpaper: its accent, and neutral surfaces that
/// lean a little towards the wallpaper's main colour. Text isn't tinted,
/// so it keeps its contrast.
pub fn theme_matched(dark: bool, p: &crate::personal::Palette) -> Theme {
    let mut t = theme_with(dark, p.accent);
    let Some(tint) = p.tint else { return t };
    use crate::personal::mix;
    // how far each surface leans (/256): more in the dark, where it shows less
    let lean = |c: Color, k: i32| Color::rgba(mix(c.0 & 0xFF_FFFF, tint, if dark { k * 2 } else { k }), c.a() as u8);
    t.sky = lean(t.sky, 22);
    t.sky2 = lean(t.sky2, 22);
    t.surface = lean(t.surface, 10);
    t.sidebar = lean(t.sidebar, 18);
    t.tile = lean(t.tile, 20);
    t.chip = lean(t.chip, 20);
    t.line = lean(t.line, 18);
    t.bar = lean(t.bar, 14);
    t.dock = lean(t.dock, 34);
    t.dock_btn = lean(t.dock_btn, 34);
    t
}
