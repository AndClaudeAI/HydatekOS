//! VP8 key frames (RFC 6386), the lossy half of WebP: the boolean entropy
//! decoder, coefficient tokens, intra prediction, the inverse transforms and
//! the loop filter, then YUV 4:2:0 to RGB.

use super::vp8_tables::{AC_Q, BMODE_PROBS, COEFF_PROBS, COEFF_UPDATE_PROBS, DC_Q};
use super::{check_size, Image, Result};
use alloc::vec;
use alloc::vec::Vec;

const BAD: &str = "the WebP image is damaged";

struct Bool<'a> {
    d: &'a [u8],
    pos: usize,
    value: u32,
    range: u32,
    count: i32,
}

impl<'a> Bool<'a> {
    fn new(d: &'a [u8]) -> Bool<'a> {
        let b = |i: usize| d.get(i).copied().unwrap_or(0) as u32;
        Bool { d, pos: 2, value: (b(0) << 8) | b(1), range: 255, count: 0 }
    }

    fn read(&mut self, prob: u32) -> bool {
        let split = 1 + (((self.range - 1) * prob) >> 8);
        let big = split << 8;
        let bit = if self.value >= big {
            self.range -= split;
            self.value -= big;
            true
        } else {
            self.range = split;
            false
        };
        while self.range < 128 {
            self.value <<= 1;
            self.range <<= 1;
            self.count += 1;
            if self.count == 8 {
                self.count = 0;
                self.value |= self.d.get(self.pos).copied().unwrap_or(0) as u32;
                self.pos += 1;
            }
        }
        bit
    }

    fn lit(&mut self, n: u32) -> u32 {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | self.read(128) as u32;
        }
        v
    }

    fn signed(&mut self, n: u32) -> i32 {
        let v = self.lit(n) as i32;
        if self.read(128) {
            -v
        } else {
            v
        }
    }

    fn opt_signed(&mut self, n: u32) -> i32 {
        if self.read(128) {
            self.signed(n)
        } else {
            0
        }
    }

    fn past_end(&self) -> bool {
        self.pos > self.d.len() + 2
    }
}

const ZIGZAG: [usize; 16] = [0, 1, 4, 8, 5, 2, 3, 6, 9, 12, 13, 10, 7, 11, 14, 15];
const BANDS: [usize; 17] = [0, 1, 2, 3, 6, 4, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7, 0];
const CAT3456: [&[u32]; 4] = [&[173, 148, 140], &[176, 155, 140, 135], &[180, 157, 141, 134, 130], &[254, 254, 243, 230, 196, 177, 153, 140, 133, 130, 129]];

// modes, in libwebp's numbering (the order of the probability tables)
const B_DC: u8 = 0;
const B_TM: u8 = 1;
const B_VE: u8 = 2;
const B_HE: u8 = 3;
const B_RD: u8 = 4;
const B_VR: u8 = 5;
const B_LD: u8 = 6;
const B_VL: u8 = 7;
const B_HD: u8 = 8;
const B_HU: u8 = 9;
// 16x16 and chroma modes share the first four numbers
const DC_PRED: u8 = B_DC;
const TM_PRED: u8 = B_TM;
const V_PRED: u8 = B_VE;
const H_PRED: u8 = B_HE;

type Probs = [u8; 1056];

fn pidx(t: usize, band: usize, ctx: usize) -> usize {
    ((t * 8 + band) * 3 + ctx) * 11
}

/// Decode one block's tokens; returns the index after the last one read.
fn coeffs(b: &mut Bool, probs: &Probs, t: usize, ctx: usize, dq: (i32, i32), first: usize, out: &mut [i32]) -> usize {
    let mut n = first;
    let mut p = pidx(t, BANDS[n], ctx);
    while n < 16 {
        if !b.read(probs[p] as u32) {
            return n;
        }
        while !b.read(probs[p + 1] as u32) {
            n += 1;
            if n == 16 {
                return 16;
            }
            p = pidx(t, BANDS[n], 0);
        }
        let v: i32;
        let next_ctx;
        if !b.read(probs[p + 2] as u32) {
            v = 1;
            next_ctx = 1;
        } else {
            v = if !b.read(probs[p + 3] as u32) {
                if !b.read(probs[p + 4] as u32) {
                    2
                } else {
                    3 + b.read(probs[p + 5] as u32) as i32
                }
            } else if !b.read(probs[p + 6] as u32) {
                if !b.read(probs[p + 7] as u32) {
                    5 + b.read(159) as i32
                } else {
                    7 + 2 * b.read(165) as i32 + b.read(145) as i32
                }
            } else {
                let b1 = b.read(probs[p + 8] as u32) as usize;
                let b0 = b.read(probs[p + 9 + b1] as u32) as usize;
                let cat = 2 * b1 + b0;
                let mut v = 0i32;
                for &pr in CAT3456[cat] {
                    v = 2 * v + b.read(pr) as i32;
                }
                v + 3 + (8 << cat)
            };
            next_ctx = 2;
        }
        let q = if n > 0 { dq.1 } else { dq.0 };
        // stored as 16 bits, like the reference decoder
        out[ZIGZAG[n]] = (if b.read(128) { -v } else { v } * q) as i16 as i32;
        n += 1;
        p = pidx(t, BANDS[n], next_ctx);
    }
    16
}

fn clip8(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Inverse 4x4 DCT, added to the prediction at `dst` (stride `s`).
fn idct_add(c: &[i32], dst: &mut [u8], at: usize, s: usize) {
    // (64-bit so damaged files can't overflow)
    let mul1 = |a: i32| (((a as i64 * 20091) >> 16) + a as i64) as i32;
    let mul2 = |a: i32| ((a as i64 * 35468) >> 16) as i32;
    let mut tmp = [0i32; 16];
    for i in 0..4 {
        let a = c[i] + c[8 + i];
        let b = c[i] - c[8 + i];
        let cc = mul2(c[4 + i]) - mul1(c[12 + i]);
        let d = mul1(c[4 + i]) + mul2(c[12 + i]);
        tmp[i * 4] = a + d;
        tmp[i * 4 + 1] = b + cc;
        tmp[i * 4 + 2] = b - cc;
        tmp[i * 4 + 3] = a - d;
    }
    for i in 0..4 {
        let dc = tmp[i] + 4;
        let a = dc + tmp[8 + i];
        let b = dc - tmp[8 + i];
        let cc = mul2(tmp[4 + i]) - mul1(tmp[12 + i]);
        let d = mul1(tmp[4 + i]) + mul2(tmp[12 + i]);
        let row = at + i * s;
        for (x, v) in [a + d, b + cc, b - cc, a - d].into_iter().enumerate() {
            dst[row + x] = clip8(dst[row + x] as i32 + (v >> 3));
        }
    }
}

/// Inverse Walsh-Hadamard transform of the Y2 block into each Y block's DC.
fn iwht(i: &[i32; 16], y: &mut [[i32; 16]; 16]) {
    let mut t = [0i32; 16];
    for k in 0..4 {
        let a0 = i[k] + i[12 + k];
        let a1 = i[4 + k] + i[8 + k];
        let a2 = i[4 + k] - i[8 + k];
        let a3 = i[k] - i[12 + k];
        t[k] = a0 + a1;
        t[8 + k] = a0 - a1;
        t[4 + k] = a3 + a2;
        t[12 + k] = a3 - a2;
    }
    for k in 0..4 {
        let dc = t[k * 4] + 3;
        let a0 = dc + t[k * 4 + 3];
        let a1 = t[k * 4 + 1] + t[k * 4 + 2];
        let a2 = t[k * 4 + 1] - t[k * 4 + 2];
        let a3 = dc - t[k * 4 + 3];
        y[k * 4][0] = ((a0 + a1) >> 3) as i16 as i32;
        y[k * 4 + 1][0] = ((a3 + a2) >> 3) as i16 as i32;
        y[k * 4 + 2][0] = ((a0 - a1) >> 3) as i16 as i32;
        y[k * 4 + 3][0] = ((a3 - a2) >> 3) as i16 as i32;
    }
}

fn avg2(a: u8, b: u8) -> u8 {
    ((a as u32 + b as u32 + 1) >> 1) as u8
}

fn avg3(a: u8, b: u8, c: u8) -> u8 {
    ((a as u32 + 2 * b as u32 + c as u32 + 2) >> 2) as u8
}

/// Work area for one macroblock: a border row above (with 4 extra pixels
/// to the right) and a border column to the left.
const WS: usize = 1 + 16 + 4;

/// 4x4 prediction at (x, y) inside the work area `w` of stride `s`.
fn predict4(w: &mut [u8], s: usize, x: usize, y: usize, mode: u8) {
    let o = (y + 1) * s + x + 1;
    let top: [u8; 8] = core::array::from_fn(|i| w[o - s + i]);
    let left: [u8; 4] = core::array::from_fn(|i| w[o + i * s - 1]);
    let p = w[o - s - 1];
    let mut d = [[0u8; 4]; 4]; // d[y][x]
    match mode {
        B_DC => {
            let mut dc = 4u32;
            for i in 0..4 {
                dc += top[i] as u32 + left[i] as u32;
            }
            d = [[(dc >> 3) as u8; 4]; 4];
        }
        B_TM => {
            for (yy, row) in d.iter_mut().enumerate() {
                for (xx, v) in row.iter_mut().enumerate() {
                    *v = clip8(left[yy] as i32 + top[xx] as i32 - p as i32);
                }
            }
        }
        B_VE => {
            let v = [avg3(p, top[0], top[1]), avg3(top[0], top[1], top[2]), avg3(top[1], top[2], top[3]), avg3(top[2], top[3], top[4])];
            d = [v; 4];
        }
        B_HE => {
            let (a, b, c, dd, e) = (p, left[0], left[1], left[2], left[3]);
            d = [[avg3(a, b, c); 4], [avg3(b, c, dd); 4], [avg3(c, dd, e); 4], [avg3(dd, e, e); 4]];
        }
        B_RD => {
            let (i, j, k, l, xx) = (left[0], left[1], left[2], left[3], p);
            let (a, b, c, dd) = (top[0], top[1], top[2], top[3]);
            let v = [avg3(j, k, l), avg3(i, j, k), avg3(xx, i, j), avg3(a, xx, i), avg3(b, a, xx), avg3(c, b, a), avg3(dd, c, b)];
            // DST(x, y) = v[3 - y + x]
            for yy in 0..4 {
                for x2 in 0..4 {
                    d[yy][x2] = v[3 - yy + x2];
                }
            }
        }
        B_LD => {
            let t = &top;
            let v = [avg3(t[0], t[1], t[2]), avg3(t[1], t[2], t[3]), avg3(t[2], t[3], t[4]), avg3(t[3], t[4], t[5]), avg3(t[4], t[5], t[6]), avg3(t[5], t[6], t[7]), avg3(t[6], t[7], t[7])];
            for yy in 0..4 {
                for x2 in 0..4 {
                    d[yy][x2] = v[x2 + yy];
                }
            }
        }
        B_VR => {
            let (i, j, k, xx) = (left[0], left[1], left[2], p);
            let (a, b, c, dd) = (top[0], top[1], top[2], top[3]);
            d[0] = [avg2(xx, a), avg2(a, b), avg2(b, c), avg2(c, dd)];
            d[1] = [avg3(i, xx, a), avg3(xx, a, b), avg3(a, b, c), avg3(b, c, dd)];
            d[2] = [avg3(j, i, xx), avg2(xx, a), avg2(a, b), avg2(b, c)];
            d[3] = [avg3(k, j, i), avg3(i, xx, a), avg3(xx, a, b), avg3(a, b, c)];
        }
        B_VL => {
            let t = &top;
            d[0] = [avg2(t[0], t[1]), avg2(t[1], t[2]), avg2(t[2], t[3]), avg2(t[3], t[4])];
            d[1] = [avg3(t[0], t[1], t[2]), avg3(t[1], t[2], t[3]), avg3(t[2], t[3], t[4]), avg3(t[3], t[4], t[5])];
            d[2] = [avg2(t[1], t[2]), avg2(t[2], t[3]), avg2(t[3], t[4]), avg3(t[4], t[5], t[6])];
            d[3] = [avg3(t[1], t[2], t[3]), avg3(t[2], t[3], t[4]), avg3(t[3], t[4], t[5]), avg3(t[5], t[6], t[7])];
        }
        B_HD => {
            let (i, j, k, l, xx) = (left[0], left[1], left[2], left[3], p);
            let (a, b, c) = (top[0], top[1], top[2]);
            d[0] = [avg2(i, xx), avg3(i, xx, a), avg3(xx, a, b), avg3(a, b, c)];
            d[1] = [avg2(j, i), avg3(j, i, xx), avg2(i, xx), avg3(i, xx, a)];
            d[2] = [avg2(k, j), avg3(k, j, i), avg2(j, i), avg3(j, i, xx)];
            d[3] = [avg2(l, k), avg3(l, k, j), avg2(k, j), avg3(k, j, i)];
        }
        _ => {
            // B_HU
            let (i, j, k, l) = (left[0], left[1], left[2], left[3]);
            d[0] = [avg2(i, j), avg3(i, j, k), avg2(j, k), avg3(j, k, l)];
            d[1] = [avg2(j, k), avg3(j, k, l), avg2(k, l), avg3(k, l, l)];
            d[2] = [avg2(k, l), avg3(k, l, l), l, l];
            d[3] = [l; 4];
        }
    }
    for yy in 0..4 {
        w[o + yy * s..o + yy * s + 4].copy_from_slice(&d[yy]);
    }
}

/// Whole-block prediction (16x16 luma or 8x8 chroma) in a work area.
fn predict_block(w: &mut [u8], s: usize, n: usize, mode: u8, has_top: bool, has_left: bool) {
    let o = s + 1;
    let shift = if n == 16 { 4 } else { 3 };
    match mode {
        DC_PRED => {
            let top: u32 = (0..n).map(|i| w[o - s + i] as u32).sum();
            let left: u32 = (0..n).map(|i| w[o + i * s - 1] as u32).sum();
            let v = match (has_top, has_left) {
                (true, true) => (top + left + n as u32) >> (shift + 1),
                (true, false) => (top + (n as u32 >> 1)) >> shift,
                (false, true) => (left + (n as u32 >> 1)) >> shift,
                _ => 128,
            } as u8;
            for y in 0..n {
                w[o + y * s..o + y * s + n].fill(v);
            }
        }
        V_PRED => {
            for y in 0..n {
                w.copy_within(o - s..o - s + n, o + y * s);
            }
        }
        H_PRED => {
            for y in 0..n {
                let v = w[o + y * s - 1];
                w[o + y * s..o + y * s + n].fill(v);
            }
        }
        _ => {
            let p = w[o - s - 1] as i32;
            for y in 0..n {
                let l = w[o + y * s - 1] as i32;
                for x in 0..n {
                    w[o + y * s + x] = clip8(l + w[o - s + x] as i32 - p);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- loop filter

fn sclip1(v: i32) -> i32 {
    v.clamp(-128, 127)
}
fn sclip2(v: i32) -> i32 {
    v.clamp(-16, 15)
}

fn filter2(p: &mut [u8], i: usize, st: usize) {
    let (p1, p0, q0, q1) = (p[i - 2 * st] as i32, p[i - st] as i32, p[i] as i32, p[i + st] as i32);
    let a = 3 * (q0 - p0) + sclip1(p1 - q1);
    let a1 = sclip2((a + 4) >> 3);
    let a2 = sclip2((a + 3) >> 3);
    p[i - st] = clip8(p0 + a2);
    p[i] = clip8(q0 - a1);
}

fn filter4(p: &mut [u8], i: usize, st: usize) {
    let (p1, p0, q0, q1) = (p[i - 2 * st] as i32, p[i - st] as i32, p[i] as i32, p[i + st] as i32);
    let a = 3 * (q0 - p0);
    let a1 = sclip2((a + 4) >> 3);
    let a2 = sclip2((a + 3) >> 3);
    let a3 = (a1 + 1) >> 1;
    p[i - 2 * st] = clip8(p1 + a3);
    p[i - st] = clip8(p0 + a2);
    p[i] = clip8(q0 - a1);
    p[i + st] = clip8(q1 - a3);
}

fn filter6(p: &mut [u8], i: usize, st: usize) {
    let (p2, p1, p0) = (p[i - 3 * st] as i32, p[i - 2 * st] as i32, p[i - st] as i32);
    let (q0, q1, q2) = (p[i] as i32, p[i + st] as i32, p[i + 2 * st] as i32);
    let a = sclip1(3 * (q0 - p0) + sclip1(p1 - q1));
    let a1 = (27 * a + 63) >> 7;
    let a2 = (18 * a + 63) >> 7;
    let a3 = (9 * a + 63) >> 7;
    p[i - 3 * st] = clip8(p2 + a3);
    p[i - 2 * st] = clip8(p1 + a2);
    p[i - st] = clip8(p0 + a1);
    p[i] = clip8(q0 - a1);
    p[i + st] = clip8(q1 - a2);
    p[i + 2 * st] = clip8(q2 - a3);
}

fn hev(p: &[u8], i: usize, st: usize, t: i32) -> bool {
    let (p1, p0, q0, q1) = (p[i - 2 * st] as i32, p[i - st] as i32, p[i] as i32, p[i + st] as i32);
    (p1 - p0).abs() > t || (q1 - q0).abs() > t
}

fn needs(p: &[u8], i: usize, st: usize, t: i32) -> bool {
    let (p1, p0, q0, q1) = (p[i - 2 * st] as i32, p[i - st] as i32, p[i] as i32, p[i + st] as i32);
    4 * (p0 - q0).abs() + (p1 - q1).abs() <= t
}

fn needs2(p: &[u8], i: usize, st: usize, t: i32, it: i32) -> bool {
    let g = |k: isize| p[(i as isize + k * st as isize) as usize] as i32;
    let (p3, p2, p1, p0, q0, q1, q2, q3) = (g(-4), g(-3), g(-2), g(-1), g(0), g(1), g(2), g(3));
    if 4 * (p0 - q0).abs() + (p1 - q1).abs() > t {
        return false;
    }
    (p3 - p2).abs() <= it && (p2 - p1).abs() <= it && (p1 - p0).abs() <= it && (q3 - q2).abs() <= it && (q2 - q1).abs() <= it && (q1 - q0).abs() <= it
}

/// Filter `n` pixels across an edge: `st` steps across it, `along` along it.
#[allow(clippy::too_many_arguments)]
fn edge(p: &mut [u8], start: usize, st: usize, along: usize, n: usize, t: i32, it: i32, hv: i32, mb_edge: bool) {
    let t2 = 2 * t + 1;
    for k in 0..n {
        let i = start + k * along;
        if needs2(p, i, st, t2, it) {
            if hev(p, i, st, hv) {
                filter2(p, i, st);
            } else if mb_edge {
                filter6(p, i, st);
            } else {
                filter4(p, i, st);
            }
        }
    }
}

fn simple_edge(p: &mut [u8], start: usize, st: usize, along: usize, t: i32) {
    let t2 = 2 * t + 1;
    for k in 0..16 {
        let i = start + k * along;
        if needs(p, i, st, t2) {
            filter2(p, i, st);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Filter {
    limit: i32,
    ilevel: i32,
    hev: i32,
}

pub fn decode(d: &[u8]) -> Result<Image> {
    if d.len() < 10 {
        return Err(BAD);
    }
    let tag = d[0] as u32 | (d[1] as u32) << 8 | (d[2] as u32) << 16;
    if tag & 1 != 0 || (tag >> 1) & 7 > 3 || d[3..6] != [0x9d, 0x01, 0x2a] {
        return Err(BAD);
    }
    let first_size = (tag >> 5) as usize;
    let w = (u16::from_le_bytes([d[6], d[7]]) & 0x3fff) as usize;
    let h = (u16::from_le_bytes([d[8], d[9]]) & 0x3fff) as usize;
    check_size(w as u32, h as u32)?;
    let part0 = d.get(10..10 + first_size).ok_or(BAD)?;
    let mut b = Bool::new(part0);
    b.lit(2); // colour space, clamping
    // segmentation
    let segments = b.read(128);
    let mut seg_update_map = false;
    let mut seg_abs = false;
    let mut seg_q = [0i32; 4];
    let mut seg_lf = [0i32; 4];
    let mut seg_probs = [255u32; 3];
    if segments {
        seg_update_map = b.read(128);
        if b.read(128) {
            seg_abs = b.read(128);
            for q in seg_q.iter_mut() {
                *q = b.opt_signed(7);
            }
            for l in seg_lf.iter_mut() {
                *l = b.opt_signed(6);
            }
        }
        if seg_update_map {
            for p in seg_probs.iter_mut() {
                *p = if b.read(128) { b.lit(8) } else { 255 };
            }
        }
    }
    let simple = b.read(128);
    let level = b.lit(6) as i32;
    let sharpness = b.lit(3) as i32;
    let mut ref_delta = 0;
    let mut mode_delta = 0;
    let lf_delta = b.read(128);
    if lf_delta && b.read(128) {
        let mut rd = [0i32; 4];
        let mut md = [0i32; 4];
        for v in rd.iter_mut() {
            *v = b.opt_signed(6);
        }
        for v in md.iter_mut() {
            *v = b.opt_signed(6);
        }
        ref_delta = rd[0];
        mode_delta = md[0];
    }
    let nparts = 1usize << b.lit(2);
    // token partitions
    let mut parts: Vec<Bool> = Vec::new();
    let sizes_at = 10 + first_size;
    let mut at = sizes_at + 3 * (nparts - 1);
    for p in 0..nparts {
        let size = if p + 1 < nparts {
            let s = d.get(sizes_at + 3 * p..sizes_at + 3 * p + 3).ok_or(BAD)?;
            s[0] as usize | (s[1] as usize) << 8 | (s[2] as usize) << 16
        } else {
            d.len().saturating_sub(at)
        };
        let end = (at + size).min(d.len());
        parts.push(Bool::new(d.get(at..end).ok_or(BAD)?));
        at = end;
    }
    // quantisers
    let base_q = b.lit(7) as i32;
    let (ydc, y2dc, y2ac, uvdc, uvac) = (b.opt_signed(4), b.opt_signed(4), b.opt_signed(4), b.opt_signed(4), b.opt_signed(4));
    let mut quant = [[(0i32, 0i32); 3]; 4]; // [segment][y1, y2, uv] = (dc, ac)
    for (s, q) in quant.iter_mut().enumerate() {
        let qs = if segments { if seg_abs { seg_q[s] } else { base_q + seg_q[s] } } else { base_q };
        let dcq = |x: i32| DC_Q[(qs + x).clamp(0, 127) as usize] as i32;
        let acq = |x: i32| AC_Q[(qs + x).clamp(0, 127) as usize] as i32;
        q[0] = (dcq(ydc), acq(0));
        q[1] = (dcq(y2dc) * 2, (acq(y2ac) * 101581 >> 16).max(8));
        q[2] = (dcq(uvdc).min(132), acq(uvac));
    }
    b.read(128); // refresh entropy probs (one frame only)
    let mut probs: Probs = COEFF_PROBS;
    for i in 0..1056 {
        if b.read(COEFF_UPDATE_PROBS[i] as u32) {
            probs[i] = b.lit(8) as u8;
        }
    }
    let skip_prob = if b.read(128) { Some(b.lit(8)) } else { None };
    // loop filter strengths per segment, for 16x16 and 4x4 macroblocks
    let mut filters = [[Filter::default(); 2]; 4];
    for s in 0..4 {
        let base = if segments { if seg_abs { seg_lf[s] } else { level + seg_lf[s] } } else { level };
        for i4 in 0..2 {
            let mut l = base;
            if lf_delta {
                l += ref_delta;
                if i4 == 1 {
                    l += mode_delta;
                }
            }
            let l = l.clamp(0, 63);
            if l > 0 {
                let mut il = l;
                if sharpness > 0 {
                    il >>= if sharpness > 4 { 2 } else { 1 };
                    il = il.min(9 - sharpness);
                }
                let il = il.max(1);
                filters[s][i4] = Filter { limit: 2 * l + il, ilevel: il, hev: if l >= 40 { 2 } else if l >= 15 { 1 } else { 0 } };
            }
        }
    }

    let (mbw, mbh) = (w.div_ceil(16), h.div_ceil(16));
    let (yw, uvw) = (mbw * 16, mbw * 8);
    let mut yp = vec![0u8; yw * mbh * 16];
    let mut up = vec![0u8; uvw * mbh * 8];
    let mut vp = vec![0u8; uvw * mbh * 8];
    // per macroblock: filter (segment, 4x4, inner edges)
    let mut mbinfo: Vec<(usize, usize, bool)> = Vec::with_capacity(mbw * mbh);
    // contexts: modes and non-zero flags above (per column) and to the left
    let mut top_modes = vec![[B_DC; 4]; mbw];
    let mut top_nz = vec![[false; 9]; mbw]; // 4 y, 2 u, 2 v, dc
    let mut segs = vec![0usize; mbw * mbh];
    for my in 0..mbh {
        let mut left_modes = [B_DC; 4];
        let mut left_nz = [false; 9];
        let pi = my % nparts;
        for mx in 0..mbw {
            // macroblock header (first partition)
            let seg = if seg_update_map {
                if !b.read(seg_probs[0]) {
                    b.read(seg_probs[1]) as usize
                } else {
                    2 + b.read(seg_probs[2]) as usize
                }
            } else {
                segs[my * mbw + mx]
            };
            segs[my * mbw + mx] = seg;
            let skip = skip_prob.is_some_and(|p| b.read(p));
            let i4 = !b.read(145);
            let mut bmodes = [0u8; 16];
            let ymode;
            if !i4 {
                ymode = if b.read(156) {
                    if b.read(128) {
                        TM_PRED
                    } else {
                        H_PRED
                    }
                } else if b.read(163) {
                    V_PRED
                } else {
                    DC_PRED
                };
                top_modes[mx] = [ymode; 4];
                left_modes = [ymode; 4];
            } else {
                ymode = B_DC;
                for y in 0..4 {
                    let mut left = left_modes[y];
                    for x in 0..4 {
                        let pr = &BMODE_PROBS[(top_modes[mx][x] as usize * 10 + left as usize) * 9..][..9];
                        let r = |b: &mut Bool, k: usize| b.read(pr[k] as u32);
                        let m = if !r(&mut b, 0) {
                            B_DC
                        } else if !r(&mut b, 1) {
                            B_TM
                        } else if !r(&mut b, 2) {
                            B_VE
                        } else if !r(&mut b, 3) {
                            if !r(&mut b, 4) {
                                B_HE
                            } else if !r(&mut b, 5) {
                                B_RD
                            } else {
                                B_VR
                            }
                        } else if !r(&mut b, 6) {
                            B_LD
                        } else if !r(&mut b, 7) {
                            B_VL
                        } else if !r(&mut b, 8) {
                            B_HD
                        } else {
                            B_HU
                        };
                        bmodes[y * 4 + x] = m;
                        top_modes[mx][x] = m;
                        left = m;
                    }
                    left_modes[y] = left;
                }
            }
            let uvmode = if !b.read(142) {
                DC_PRED
            } else if !b.read(114) {
                V_PRED
            } else if b.read(183) {
                TM_PRED
            } else {
                H_PRED
            };
            // residuals (token partition)
            let mut ycoef = [[0i32; 16]; 16];
            let mut ucoef = [[0i32; 16]; 4];
            let mut vcoef = [[0i32; 16]; 4];
            let mut any = false;
            let q = quant[seg];
            if !skip {
                let tb = &mut parts[pi];
                let first;
                let ytype;
                if !i4 {
                    let mut y2 = [0i32; 16];
                    let ctx = top_nz[mx][8] as usize + left_nz[8] as usize;
                    let n = coeffs(tb, &probs, 1, ctx, q[1], 0, &mut y2);
                    top_nz[mx][8] = n > 0;
                    left_nz[8] = n > 0;
                    any |= n > 0;
                    iwht(&y2, &mut ycoef);
                    first = 1;
                    ytype = 0;
                } else {
                    first = 0;
                    ytype = 3;
                }
                for y in 0..4 {
                    for x in 0..4 {
                        let ctx = top_nz[mx][x] as usize + left_nz[y] as usize;
                        let n = coeffs(tb, &probs, ytype, ctx, q[0], first, &mut ycoef[y * 4 + x]);
                        let nz = n > first;
                        top_nz[mx][x] = nz;
                        left_nz[y] = nz;
                        any |= n > first;
                    }
                }
                for (ch, coef) in [(0usize, &mut ucoef), (1, &mut vcoef)] {
                    for y in 0..2 {
                        for x in 0..2 {
                            let ctx = top_nz[mx][4 + ch * 2 + x] as usize + left_nz[4 + ch * 2 + y] as usize;
                            let n = coeffs(tb, &probs, 2, ctx, q[2], 0, &mut coef[y * 2 + x]);
                            top_nz[mx][4 + ch * 2 + x] = n > 0;
                            left_nz[4 + ch * 2 + y] = n > 0;
                            any |= n > 0;
                        }
                    }
                }
            } else {
                top_nz[mx][..8].fill(false);
                left_nz[..8].fill(false);
                if !i4 {
                    top_nz[mx][8] = false;
                    left_nz[8] = false;
                }
            }
            mbinfo.push((seg, i4 as usize, i4 || any));
            // reconstruction in a work area with borders
            let (x0, y0) = (mx * 16, my * 16);
            let mut wk = [0u8; WS * 17];
            // top row (+4 to the right) and top-left
            for i in 0..20 {
                wk[1 + i] = if my == 0 {
                    127
                } else if i < 16 {
                    yp[(y0 - 1) * yw + x0 + i]
                } else if mx + 1 < mbw {
                    yp[(y0 - 1) * yw + x0 + i]
                } else {
                    yp[(y0 - 1) * yw + x0 + 15]
                };
            }
            wk[0] = if my == 0 {
                127
            } else if mx == 0 {
                129
            } else {
                yp[(y0 - 1) * yw + x0 - 1]
            };
            for y in 0..16 {
                wk[(y + 1) * WS] = if mx == 0 { 129 } else { yp[(y0 + y) * yw + x0 - 1] };
            }
            if i4 {
                // above-right of the right column's lower blocks: the MB's top-right
                for r in [4usize, 8, 12] {
                    for i in 0..4 {
                        wk[r * WS + 17 + i] = wk[1 + 16 + i];
                    }
                }
                for k in 0..16 {
                    let (bx, by) = (k % 4, k / 4);
                    predict4(&mut wk, WS, bx * 4, by * 4, bmodes[k]);
                    if ycoef[k].iter().any(|&c| c != 0) {
                        idct_add(&ycoef[k], &mut wk, (by * 4 + 1) * WS + bx * 4 + 1, WS);
                    }
                }
            } else {
                predict_block(&mut wk, WS, 16, ymode, my > 0, mx > 0);
                for k in 0..16 {
                    let (bx, by) = (k % 4, k / 4);
                    if ycoef[k].iter().any(|&c| c != 0) {
                        idct_add(&ycoef[k], &mut wk, (by * 4 + 1) * WS + bx * 4 + 1, WS);
                    }
                }
            }
            for y in 0..16 {
                yp[(y0 + y) * yw + x0..(y0 + y) * yw + x0 + 16].copy_from_slice(&wk[(y + 1) * WS + 1..(y + 1) * WS + 17]);
            }
            // chroma
            for (plane, coef) in [(&mut up, &ucoef), (&mut vp, &vcoef)] {
                const CS: usize = 9;
                let (cx, cy) = (mx * 8, my * 8);
                let mut wc = [0u8; CS * 9];
                for i in 0..8 {
                    wc[1 + i] = if my == 0 { 127 } else { plane[(cy - 1) * uvw + cx + i] };
                }
                wc[0] = if my == 0 {
                    127
                } else if mx == 0 {
                    129
                } else {
                    plane[(cy - 1) * uvw + cx - 1]
                };
                for y in 0..8 {
                    wc[(y + 1) * CS] = if mx == 0 { 129 } else { plane[(cy + y) * uvw + cx - 1] };
                }
                predict_block(&mut wc, CS, 8, uvmode, my > 0, mx > 0);
                for k in 0..4 {
                    let (bx, by) = (k % 2, k / 2);
                    if coef[k].iter().any(|&c| c != 0) {
                        idct_add(&coef[k], &mut wc, (by * 4 + 1) * CS + bx * 4 + 1, CS);
                    }
                }
                for y in 0..8 {
                    plane[(cy + y) * uvw + cx..(cy + y) * uvw + cx + 8].copy_from_slice(&wc[(y + 1) * CS + 1..(y + 1) * CS + 9]);
                }
            }
            if b.past_end() {
                return Err(BAD);
            }
        }
    }
    // loop filter, macroblock by macroblock
    if level > 0 {
        for my in 0..mbh {
            for mx in 0..mbw {
                let (seg, i4, inner) = mbinfo[my * mbw + mx];
                let f = filters[seg][i4];
                if f.limit == 0 {
                    continue;
                }
                let yo = my * 16 * yw + mx * 16;
                if simple {
                    if mx > 0 {
                        simple_edge(&mut yp, yo, 1, yw, f.limit + 4);
                    }
                    if inner {
                        for k in [4, 8, 12] {
                            simple_edge(&mut yp, yo + k, 1, yw, f.limit);
                        }
                    }
                    if my > 0 {
                        simple_edge(&mut yp, yo, yw, 1, f.limit + 4);
                    }
                    if inner {
                        for k in [4, 8, 12] {
                            simple_edge(&mut yp, yo + k * yw, yw, 1, f.limit);
                        }
                    }
                    continue;
                }
                let co = my * 8 * uvw + mx * 8;
                if mx > 0 {
                    edge(&mut yp, yo, 1, yw, 16, f.limit + 4, f.ilevel, f.hev, true);
                    edge(&mut up, co, 1, uvw, 8, f.limit + 4, f.ilevel, f.hev, true);
                    edge(&mut vp, co, 1, uvw, 8, f.limit + 4, f.ilevel, f.hev, true);
                }
                if inner {
                    for k in [4, 8, 12] {
                        edge(&mut yp, yo + k, 1, yw, 16, f.limit, f.ilevel, f.hev, false);
                    }
                    edge(&mut up, co + 4, 1, uvw, 8, f.limit, f.ilevel, f.hev, false);
                    edge(&mut vp, co + 4, 1, uvw, 8, f.limit, f.ilevel, f.hev, false);
                }
                if my > 0 {
                    edge(&mut yp, yo, yw, 1, 16, f.limit + 4, f.ilevel, f.hev, true);
                    edge(&mut up, co, uvw, 1, 8, f.limit + 4, f.ilevel, f.hev, true);
                    edge(&mut vp, co, uvw, 1, 8, f.limit + 4, f.ilevel, f.hev, true);
                }
                if inner {
                    for k in [4, 8, 12] {
                        edge(&mut yp, yo + k * yw, yw, 1, 16, f.limit, f.ilevel, f.hev, false);
                    }
                    edge(&mut up, co + 4 * uvw, uvw, 1, 8, f.limit, f.ilevel, f.hev, false);
                    edge(&mut vp, co + 4 * uvw, uvw, 1, 8, f.limit, f.ilevel, f.hev, false);
                }
            }
        }
    }
    // YUV 4:2:0 to RGB, with libwebp's "fancy" chroma upsampling
    let mut img = Image::new(w as u32, h as u32);
    let (cw, chh) = ((w + 1) / 2, (h + 1) / 2);
    let chroma = |plane: &[u8], x: usize, y: usize| -> i32 {
        // nearest chroma sample (3/4) and its neighbour (1/4) on each axis
        let (xa, xb) = if x % 2 == 0 { (x / 2, (x / 2).saturating_sub(1)) } else { (x / 2, (x / 2 + 1).min(cw - 1)) };
        let (ya, yb) = if y % 2 == 0 { (y / 2, (y / 2).saturating_sub(1)) } else { (y / 2, (y / 2 + 1).min(chh - 1)) };
        let g = |xx: usize, yy: usize| plane[yy * uvw + xx] as i32;
        (9 * g(xa, ya) + 3 * g(xb, ya) + 3 * g(xa, yb) + g(xb, yb) + 8) >> 4
    };
    let mult = |v: i32, c: i32| (v * c) >> 8;
    let clip = |v: i32| -> u32 { (v >> 6).clamp(0, 255) as u32 };
    for y in 0..h {
        for x in 0..w {
            let yy = yp[y * yw + x] as i32;
            let (u, v) = (chroma(&up, x, y), chroma(&vp, x, y));
            let r = clip(mult(yy, 19077) + mult(v, 26149) - 14234);
            let g = clip(mult(yy, 19077) - mult(u, 6419) - mult(v, 13320) + 8708);
            let bl = clip(mult(yy, 19077) + mult(u, 33050) - 17685);
            img.px[y * w + x] = 0xff00_0000 | r << 16 | g << 8 | bl;
        }
    }
    Ok(img)
}
