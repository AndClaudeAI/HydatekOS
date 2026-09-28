//! Profile pictures: initials on a colour, one of the drawn motifs, or a
//! photo, rendered into a square canvas that the UI shows as a circle.

use crate::apps::slidedraw::{fill_poly, Paint};
use crate::font::{self, Face};
use crate::gfx::{Canvas, Color, Rect};
use crate::profile::{Avatar, COLOURS};

/// Render `avatar` for `name` into a `size` × `size` canvas. `photo` is the
/// `PIC_SIZE` square used for `Avatar::Picture` (initials when missing).
pub fn render(avatar: Avatar, name: &str, photo: Option<&[u32]>, size: i32) -> Canvas {
    let mut c = Canvas::new(size, size);
    match avatar {
        Avatar::Picture if photo.is_some() => {
            let px = photo.unwrap();
            let side = crate::profile::PIC_SIZE as i32;
            let mut src = Canvas::new(side, side);
            src.px.copy_from_slice(&px[..(side * side) as usize]);
            c.blit_scaled(&src, Rect::new(0, 0, size, size), 0);
        }
        Avatar::Motif(i) => motif(&mut c, i, size),
        Avatar::Initials(i) => initials(&mut c, name, COLOURS[i as usize % COLOURS.len()].1, size),
        Avatar::Picture => initials(&mut c, name, COLOURS[crate::profile::colour_for(name) as usize].1, size),
    }
    c
}

fn initials(c: &mut Canvas, name: &str, bg: u32, size: i32) {
    c.fill_rect(Rect::new(0, 0, size, size), Color::rgb(bg));
    let s = crate::profile::initials(name);
    if s.is_empty() {
        // no name yet: a head and shoulders
        let fg = Color::rgba(0xFFFFFF, 225);
        c.fill_circle(size / 2, size * 40 / 100, size * 17 / 100, fg);
        c.fill_circle(size / 2, size * 104 / 100, size * 38 / 100, fg);
        return;
    }
    let px = if s.chars().count() > 1 { size * 40 / 100 } else { size * 46 / 100 };
    let w = font::measure(Face::Semibold, px, &s);
    font::draw(c, (size - w) / 2, size / 2 + px * 36 / 100, Face::Semibold, px, &s, Color::rgb(0xFFFFFF));
}

/// The drawn pictures, designed on a 256 grid.
fn motif(c: &mut Canvas, i: u8, size: i32) {
    let k = |v: i32| v * size / 256;
    let circle = |c: &mut Canvas, x: i32, y: i32, r: i32, col: u32| c.fill_circle(k(x), k(y), k(r), Color::rgb(col));
    let poly = |c: &mut Canvas, pts: &[(i32, i32)], col: u32| {
        let p: alloc::vec::Vec<(i32, i32)> = pts.iter().map(|&(x, y)| (k(x) * 16, k(y) * 16)).collect();
        fill_poly(c, &p, &Paint::Solid(col), 255);
    };
    let bg = |c: &mut Canvas, col: u32| c.fill_rect(Rect::new(0, 0, size, size), Color::rgb(col));
    match i {
        // Sunrise: a sun over the dunes
        0 => {
            bg(c, 0xF6D9A8);
            c.fill_rect(Rect::new(0, k(96), size, size - k(96)), Color::rgb(0xF3C98E));
            c.fill_circle(k(128), k(142), k(70), Color::rgba(0xFFF4DC, 110));
            circle(c, 128, 142, 50, 0xFFF1D6);
            circle(c, 30, 330, 150, 0xD9793A);
            circle(c, 236, 360, 170, 0xB5581B);
        }
        // Night: a crescent moon and stars
        1 => {
            bg(c, 0x1F2A4A);
            circle(c, 150, 108, 56, 0xF4EFD8);
            circle(c, 174, 88, 50, 0x1F2A4A);
            for (x, y, r) in [(62, 70, 5), (96, 128, 3), (52, 150, 4), (206, 170, 4), (118, 52, 3), (220, 48, 3)] {
                circle(c, x, y, r, 0xF4EFD8);
            }
            circle(c, 128, 430, 220, 0x152038);
        }
        // Waves: layered sea under a pale sun
        2 => {
            bg(c, 0x9FD4CF);
            circle(c, 188, 74, 28, 0xFBEFD2);
            circle(c, -10, 290, 130, 0x57B5B0);
            circle(c, 220, 300, 140, 0x3FA3A0);
            circle(c, 90, 380, 160, 0x2A8C8C);
            circle(c, 250, 420, 150, 0x1F6F75);
        }
        // Hills: green hills and a sun
        3 => {
            bg(c, 0xDDEBC4);
            circle(c, 72, 78, 30, 0xF7D774);
            circle(c, 40, 320, 150, 0x7FAF6B);
            circle(c, 230, 300, 140, 0x5E8F4E);
            circle(c, 128, 400, 180, 0x4F7942);
        }
        // Peaks: mountains with snow
        4 => {
            bg(c, 0xCFDEEE);
            poly(c, &[(-20, 256), (90, 96), (200, 256)], 0x6C87A8);
            poly(c, &[(90, 96), (116, 134), (100, 128), (86, 140), (70, 125)], 0xF7F9FC);
            poly(c, &[(60, 256), (176, 70), (290, 256)], 0x4A6485);
            poly(c, &[(176, 70), (206, 118), (188, 110), (172, 124), (154, 106)], 0xFFFFFF);
            c.fill_rect(Rect::new(0, k(226), size, size - k(226)), Color::rgb(0x3A4F6B));
        }
        // Bloom: a flower
        _ => {
            bg(c, 0xF6DDE1);
            for j in 0..6 {
                let (s, co) = (crate::deck::sin_deg(j * 60), crate::deck::cos_deg(j * 60));
                circle(c, 128 + s * 52 / 16384, 128 - co * 52 / 16384, 40, 0xE07A8F);
            }
            circle(c, 128, 128, 30, 0xF2C14E);
            circle(c, 120, 120, 8, 0xF7D97F);
        }
    }
}

// ---- the picker ----------------------------------------------------------------

use crate::profile::{MOTIFS, PIC_SIZE};
use crate::ui::{Action, Ui};
use alloc::string::String;
use alloc::vec::Vec;

/// Where photos for a profile picture are looked for.
const PHOTO_DIRS: [&str; 5] = ["/home/Pictures", "/home/Documents/Photos 2026", "/home/Downloads", "/home/Shared", "/home/Documents"];
const PHOTOS_MAX: usize = 8;
/// Pickers report `PICK + n`: initials colours, then motifs, then photos.
pub const PICKS_INITIALS: u16 = 0;
pub const PICKS_MOTIFS: u16 = COLOURS.len() as u16;
pub const PICKS_PHOTOS: u16 = PICKS_MOTIFS + MOTIFS.len() as u16;

/// A choice of picture: what it is, and the photo when it's one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Initials(u8),
    Motif(u8),
    Photo(usize),
}

/// The grid of pictures to choose from: initials in each colour, the drawn
/// motifs, and photos found in the home folders.
#[derive(Default)]
pub struct Picker {
    /// (file name, PIC_SIZE square)
    pub photos: Vec<(String, Vec<u32>)>,
    loaded: bool,
    thumbs: Vec<Canvas>,
    thumbs_for: Option<String>,
}

impl Picker {
    /// Find photos (once). `current` is the profile's own photo, offered first.
    pub fn load(&mut self, fs: &crate::fs::Vfs, current: Option<&[u32]>) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(px) = current {
            self.photos.push((String::from("Current photo"), px.to_vec()));
        }
        for dir in PHOTO_DIRS {
            for (name, is_dir, size) in fs.list(dir) {
                if self.photos.len() >= PHOTOS_MAX {
                    break;
                }
                let lower = name.to_ascii_lowercase();
                if is_dir || size > 12 << 20 || ![".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"].iter().any(|e| lower.ends_with(e)) {
                    continue;
                }
                let Some(data) = fs.read(&crate::fs::join(dir, &name)) else { continue };
                if let Ok(img) = crate::image::decode(&data) {
                    self.photos.push((name, crate::profile::square(&img.px, img.w, img.h, PIC_SIZE)));
                }
            }
        }
        self.thumbs_for = None;
    }

    pub fn choice(code: u16) -> Choice {
        if code < PICKS_MOTIFS {
            Choice::Initials(code as u8)
        } else if code < PICKS_PHOTOS {
            Choice::Motif((code - PICKS_MOTIFS) as u8)
        } else {
            Choice::Photo((code - PICKS_PHOTOS) as usize)
        }
    }

    fn thumbs(&mut self, name: &str) {
        if self.thumbs_for.as_deref() == Some(name) {
            return;
        }
        let d = 112;
        self.thumbs.clear();
        for i in 0..COLOURS.len() {
            self.thumbs.push(render(Avatar::Initials(i as u8), name, None, d));
        }
        for i in 0..MOTIFS.len() {
            self.thumbs.push(render(Avatar::Motif(i as u8), name, None, d));
        }
        for (_, px) in &self.photos {
            self.thumbs.push(render(Avatar::Picture, name, Some(px), d));
        }
        self.thumbs_for = Some(String::from(name));
    }

    /// Draw the grid in `r` (circles `d` points wide); `act` maps a pick to
    /// the owner's action. Returns the height used.
    pub fn render(&mut self, ui: &mut Ui, r: Rect, d: i32, name: &str, chosen: Choice, act: impl Fn(u16) -> Action) -> i32 {
        self.thumbs(name);
        let t = ui.t;
        let gap = d / 3;
        let per_row = ((r.w + gap) / (d + gap)).max(1) as usize;
        let lab = (d * 3 / 10).clamp(10, 13);
        let mut y = r.y;
        let sections: [(&str, u16, usize); 3] = [("INITIALS", PICKS_INITIALS, COLOURS.len()), ("PICTURES", PICKS_MOTIFS, MOTIFS.len()), ("YOUR PHOTOS", PICKS_PHOTOS, self.photos.len())];
        for (title, base, n) in sections {
            ui.label(r.x, y + lab, lab, title, t.text3);
            y += lab + d / 3;
            if n == 0 {
                let msg = ui.fit(Face::Regular, lab + 1, "Put photos in Pictures to use one here", r.w);
                ui.text(r.x, y + lab, Face::Regular, lab + 1, &msg, t.text2);
                y += lab + d / 2;
                continue;
            }
            for i in 0..n {
                let (col, row) = ((i % per_row) as i32, (i / per_row) as i32);
                let b = Rect::new(r.x + col * (d + gap), y + row * (d + gap), d, d);
                let code = base + i as u16;
                let a = act(code);
                let on = Picker::choice(code) == chosen;
                if on || ui.hot(a) {
                    let ring = if on { t.accent } else { t.line };
                    ui.circle(b.x + d / 2, b.y + d / 2, d / 2 + 4, ring);
                    ui.circle(b.x + d / 2, b.y + d / 2, d / 2 + 2, t.surface);
                }
                ui.avatar(b, &self.thumbs[code as usize]);
                ui.zone(b, a);
            }
            let rows = ((n + per_row - 1) / per_row) as i32;
            y += rows * (d + gap) + gap / 2;
        }
        y - r.y
    }
}
