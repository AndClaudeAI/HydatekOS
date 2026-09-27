//! Text rendering from the pre-rasterised font pack (see tools/fontgen.py).

use crate::gfx::{Canvas, Color};
use alloc::vec::Vec;
use core::cell::UnsafeCell;

static PACK: &[u8] = include_bytes!("../assets/fonts.bin");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    Regular = 0,
    Medium = 1,
    Semibold = 2,
    Display = 3,
    Mono = 4,
    /// slanted renderings of Regular and Semibold (documents)
    Italic = 5,
    SemiboldItalic = 6,
}

#[derive(Clone, Copy)]
struct Glyph {
    cp: u32,
    left: i16,
    top: i16,
    w: u16,
    h: u16,
    adv64: u16,
    off: u32,
}

struct FaceData {
    id: u8,
    px: u16,
    glyphs: Vec<Glyph>,
    data: &'static [u8],
}

struct Fonts(UnsafeCell<Vec<FaceData>>);
unsafe impl Sync for Fonts {}
static FONTS: Fonts = Fonts(UnsafeCell::new(Vec::new()));

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

pub fn init() {
    let faces = unsafe { &mut *FONTS.0.get() };
    assert!(&PACK[0..4] == b"HFPK");
    let n = rd16(PACK, 4) as usize;
    let mut o = 6;
    for _ in 0..n {
        let id = PACK[o];
        let px = rd16(PACK, o + 2);
        let count = rd16(PACK, o + 8) as usize;
        let dlen = rd32(PACK, o + 10) as usize;
        o += 14;
        let mut glyphs = Vec::with_capacity(count);
        let mut off = 0u32;
        for _ in 0..count {
            let g = Glyph {
                cp: rd32(PACK, o),
                left: rd16(PACK, o + 4) as i16,
                top: rd16(PACK, o + 6) as i16,
                w: rd16(PACK, o + 8),
                h: rd16(PACK, o + 10),
                adv64: rd16(PACK, o + 12),
                off,
            };
            off += g.w as u32 * g.h as u32;
            glyphs.push(g);
            o += 14;
        }
        faces.push(FaceData { id, px, glyphs, data: &PACK[o..o + dlen] });
        o += dlen;
    }
}

fn face(f: Face, px: i32) -> &'static FaceData {
    let faces = unsafe { &*FONTS.0.get() };
    let mut best: Option<&FaceData> = None;
    for fd in faces.iter().filter(|fd| fd.id == f as u8) {
        let better = match best {
            None => true,
            Some(b) => (fd.px as i32 - px).abs() < (b.px as i32 - px).abs(),
        };
        if better {
            best = Some(fd);
        }
    }
    best.expect("font face")
}

fn glyph(fd: &FaceData, ch: char) -> Option<&Glyph> {
    let cp = ch as u32;
    if fd.id != Face::Display as u8 && (32..127).contains(&cp) {
        return fd.glyphs.get((cp - 32) as usize);
    }
    fd.glyphs.iter().find(|g| g.cp == cp).or_else(|| fd.glyphs.iter().find(|g| g.cp == '?' as u32))
}

/// Width in pixels of `s` rendered at `px`.
pub fn measure(f: Face, px: i32, s: &str) -> i32 {
    let fd = face(f, px);
    let mut pen = 0i32;
    for ch in s.chars() {
        if let Some(g) = glyph(fd, ch) {
            pen += g.adv64 as i32;
        }
    }
    (pen + 32) >> 6
}

/// Draw `s` with its baseline at `y`. Returns the advance width.
pub fn draw(c: &mut Canvas, x: i32, y: i32, f: Face, px: i32, s: &str, col: Color) -> i32 {
    let fd = face(f, px);
    let mut pen = x << 6;
    for ch in s.chars() {
        if let Some(g) = glyph(fd, ch) {
            if g.w > 0 {
                let gx = ((pen + 32) >> 6) + g.left as i32;
                let gy = y - g.top as i32;
                let len = g.w as usize * g.h as usize;
                let m = &fd.data[g.off as usize..g.off as usize + len];
                c.mask(gx, gy, g.w as i32, g.h as i32, m, col);
            }
            pen += g.adv64 as i32;
        }
    }
    ((pen + 32) >> 6) - x
}

/// Advance of one character in 1/64 pixel.
pub fn advance64(f: Face, px: i32, ch: char) -> i32 {
    glyph(face(f, px), ch).map(|g| g.adv64 as i32).unwrap_or(0)
}
