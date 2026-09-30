//! Mouse pointer sprite: an anti-aliased arrow with a light outline.

use alloc::vec;
use alloc::vec::Vec;

// Arrow outline in 1/10 logical units (tip at 0,0).
const ARROW: [(i64, i64); 7] = [(10, 10), (10, 175), (52, 136), (82, 200), (112, 187), (84, 124), (140, 124)];

pub struct Cursor {
    pub w: i32,
    pub h: i32,
    /// 0x00RRGGBB colour + coverage per pixel
    pub px: Vec<(u32, u8)>,
}

fn inside(x: i64, y: i64) -> bool {
    let mut c = false;
    let n = ARROW.len();
    for i in 0..n {
        let (x1, y1) = ARROW[i];
        let (x2, y2) = ARROW[(i + 1) % n];
        if (y1 > y) != (y2 > y) && x < (x2 - x1) * (y - y1) / (y2 - y1) + x1 {
            c = !c;
        }
    }
    c
}

fn edge_dist2(x: i64, y: i64) -> i64 {
    let n = ARROW.len();
    let mut best = i64::MAX;
    for i in 0..n {
        let (x1, y1) = ARROW[i];
        let (x2, y2) = ARROW[(i + 1) % n];
        let (dx, dy) = (x2 - x1, y2 - y1);
        let (ex, ey) = (x - x1, y - y1);
        let l = dx * dx + dy * dy;
        let t = (ex * dx + ey * dy).clamp(0, l);
        let qx = ex - dx * t / l;
        let qy = ey - dy * t / l;
        best = best.min(qx * qx + qy * qy);
    }
    best
}

impl Cursor {
    pub fn new(scale: i32) -> Cursor {
        let w = 16 * scale;
        let h = 22 * scale;
        let mut px = vec![(0u32, 0u8); (w * h) as usize];
        let s = scale as i64;
        let border = 16i64; // outline thickness in 1/10 units
        for y in 0..h {
            for x in 0..w {
                let (mut inner, mut outer) = (0, 0);
                for sy in 0..4 {
                    for sx in 0..4 {
                        let fx = (x as i64 * 40 + sx * 10 + 5) / (4 * s);
                        let fy = (y as i64 * 40 + sy * 10 + 5) / (4 * s);
                        if inside(fx, fy) {
                            outer += 1;
                            if edge_dist2(fx, fy) > border * border {
                                inner += 1;
                            }
                        } else if edge_dist2(fx, fy) <= 100 {
                            outer += 1;
                        }
                    }
                }
                if outer > 0 {
                    // mix white outline and dark fill
                    let k = inner * 255 / outer.max(1);
                    let v = 255 - (k as u32 * (255 - 0x1e) / 255);
                    let col = (v << 16) | (v << 8) | v;
                    px[(y * w + x) as usize] = (col, (outer * 255 / 16) as u8);
                }
            }
        }
        Cursor { w, h, px }
    }

    /// Blend the pointer into `c` with its tip at (x, y), keeping the
    /// pixels it covers in `under` for `unstamp`.
    pub fn stamp(&self, c: &mut crate::gfx::Canvas, x: i32, y: i32, under: &mut Vec<u32>) {
        under.clear();
        for cy in 0..self.h {
            for cx in 0..self.w {
                let (px, py) = (x + cx, y + cy);
                if px < 0 || py < 0 || px >= c.w || py >= c.h {
                    continue;
                }
                let i = (py * c.w + px) as usize;
                under.push(c.px[i]);
                let (col, a) = self.px[(cy * self.w + cx) as usize];
                if a > 0 {
                    let d = c.px[i];
                    let a = a as u32;
                    let mix = |s: u32, dd: u32| (s * a + dd * (255 - a)) / 255;
                    c.px[i] = (d & 0xFF00_0000) | mix(col >> 16 & 255, d >> 16 & 255) << 16 | mix(col >> 8 & 255, d >> 8 & 255) << 8 | mix(col & 255, d & 255);
                }
            }
        }
    }

    /// Put back what `stamp` covered.
    pub fn unstamp(&self, c: &mut crate::gfx::Canvas, x: i32, y: i32, under: &[u32]) {
        let mut k = 0;
        for cy in 0..self.h {
            for cx in 0..self.w {
                let (px, py) = (x + cx, y + cy);
                if px < 0 || py < 0 || px >= c.w || py >= c.h {
                    continue;
                }
                if let Some(v) = under.get(k) {
                    c.px[(py * c.w + px) as usize] = *v;
                }
                k += 1;
            }
        }
    }
}

