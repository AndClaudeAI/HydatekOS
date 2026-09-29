//! The Hydatek Systems wordmark (`assets/boot-logo.png`, the lettering's
//! coverage), drawn at any size: on the boot screen and on the Hydatek key,
//! the key beside Gen that other keyboards print a logo of their own on.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::cell::UnsafeCell;

pub const LOGO: &[u8] = include_bytes!("../assets/boot-logo.png");

/// The decoded wordmark (white, coverage as alpha) and its sizes so far.
struct Marks {
    src: Vec<u32>,
    w: i32,
    h: i32,
    sized: BTreeMap<i32, Vec<u8>>,
}

struct Cell(UnsafeCell<Option<Marks>>);
unsafe impl Sync for Cell {}
static MARKS: Cell = Cell(UnsafeCell::new(None));

fn marks() -> Option<&'static mut Marks> {
    // HydatekOS draws on one thread
    let m = unsafe { &mut *MARKS.0.get() };
    if m.is_none() {
        let img = crate::image::decode(LOGO).ok()?;
        let src = img.px.iter().map(|p| (p & 0xFF) << 24 | 0xFF_FFFF).collect();
        *m = Some(Marks { src, w: img.w as i32, h: img.h as i32, sized: BTreeMap::new() });
    }
    m.as_mut()
}

/// Width over height of the wordmark (x1000).
pub fn aspect() -> i32 {
    marks().map_or(3200, |m| m.w * 1000 / m.h.max(1))
}

/// The wordmark's coverage (0-255 per pixel) at `w` pixels wide, and its
/// height.
pub fn coverage(w: i32) -> Option<(&'static [u8], i32)> {
    let m = marks()?;
    let w = w.clamp(8, 4096);
    let h = (m.h * w / m.w).max(1);
    if !m.sized.contains_key(&w) {
        if m.sized.len() > 16 {
            m.sized.clear();
        }
        let cov = crate::gfx::scale_argb(&m.src, m.w, m.h, w, h).iter().map(|p| (p >> 24) as u8).collect();
        m.sized.insert(w, cov);
    }
    Some((m.sized.get(&w)?.as_slice(), h))
}
