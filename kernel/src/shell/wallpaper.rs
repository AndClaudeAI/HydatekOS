//! The "Dune" wallpaper: sky gradient, sun and three rolling dunes.

use crate::gfx::{sin_q14, Canvas, Color, Rect};
use crate::theme::Theme;

/// Draw the scene into `r` (physical pixels). `tall` selects the mobile layout.
pub fn draw(c: &mut Canvas, r: Rect, t: &Theme, tall: bool) {
    let old = c.set_clip(r.intersect(&c.clip));
    // sky
    for y in r.y..r.b() {
        let k = ((y - r.y) * 256 / r.h.max(1)) as u32;
        c.fill_rect(Rect::new(r.x, y, r.w, 1), t.sky.mix(t.sky2, k));
    }
    // sun
    let (sx, sy, sr) = if tall { (r.x + r.w * 80 / 100, r.y + r.h * 18 / 100, r.w * 18 / 100) } else { (r.x + r.w * 86 / 100, r.y + r.h * 29 / 100, r.w * 10 / 100) };
    c.fill_circle(sx, sy, sr, t.sun);
    // dunes: base (per mille of height), two sine components (amp ‰, freq, phase)
    let layers: [(Color, i32, i32, i32, i32, i32, i32, i32); 3] = if tall {
        [(t.dune1, 715, 22, 1, 600, 10, 3, 100), (t.dune2, 812, 28, 1, 150, 8, 2, 400), (t.dune3, 895, 25, 1, 900, 6, 3, 200)]
    } else {
        [(t.dune1, 640, 22, 1, 700, 8, 3, 100), (t.dune2, 770, 30, 1, 100, 10, 2, 300), (t.dune3, 890, 26, 1, 850, 8, 3, 500)]
    };
    for (col, base, a1, f1, p1, a2, f2, p2) in layers {
        for x in r.x..r.r() {
            let u = ((x - r.x) as i64 * 1024 / r.w.max(1) as i64) as i32;
            let s1 = sin_q14(u * f1 + p1) as i64;
            let s2 = sin_q14(u * f2 + p2) as i64;
            let ypm = base as i64 * 256 + (a1 as i64 * s1 + a2 as i64 * s2) * 256 / 16384;
            let yq8 = r.y * 256 + (ypm * r.h as i64 / 1000) as i32;
            c.vspan_aa(x, yq8, r.b(), col);
        }
    }
    c.set_clip(old);
}
