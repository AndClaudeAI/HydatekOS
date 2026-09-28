//! WebP: the RIFF container (simple, extended and animated files), VP8L
//! lossless images, ALPH alpha planes, and VP8 lossy images (in `vp8.rs`).

use super::{check_size, vp8, Frame, Image, Result, MAX_ANIM_PIXELS};
use alloc::vec;
use alloc::vec::Vec;

const BAD: &str = "the WebP image is damaged";

// ---------------------------------------------------------------- VP8L bits

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8]) -> Bits<'a> {
        Bits { d, pos: 0, acc: 0, n: 0 }
    }

    fn fill(&mut self) {
        while self.n <= 56 {
            let b = self.d.get(self.pos).copied().unwrap_or(0);
            self.pos += 1;
            self.acc |= (b as u64) << self.n;
            self.n += 8;
        }
    }

    fn bits(&mut self, k: u32) -> u32 {
        if k == 0 {
            return 0;
        }
        self.fill();
        let v = (self.acc & ((1u64 << k) - 1)) as u32;
        self.acc >>= k;
        self.n -= k;
        v
    }

    fn bit(&mut self) -> bool {
        self.bits(1) == 1
    }

    fn peek8(&mut self) -> u32 {
        self.fill();
        (self.acc & 0xff) as u32
    }

    fn skip(&mut self, k: u32) {
        self.acc >>= k;
        self.n -= k;
    }

    /// Read past the end of the data: the stream is damaged.
    fn overrun(&self) -> bool {
        self.pos > self.d.len() + 8
    }
}

// ---------------------------------------------------------------- prefix codes

struct Code {
    /// one symbol and no bits
    single: Option<u16>,
    fast: Vec<(u8, u16)>,
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Code {
    fn from_lengths(lengths: &[u8]) -> Result<Code> {
        let used: Vec<usize> = (0..lengths.len()).filter(|&i| lengths[i] != 0).collect();
        if used.is_empty() {
            return Ok(Code { single: Some(0), fast: Vec::new(), counts: [0; 16], symbols: Vec::new() });
        }
        if used.len() == 1 {
            return Ok(Code { single: Some(used[0] as u16), fast: Vec::new(), counts: [0; 16], symbols: Vec::new() });
        }
        let mut counts = [0u16; 16];
        for &i in &used {
            if lengths[i] > 15 {
                return Err(BAD);
            }
            counts[lengths[i] as usize] += 1;
        }
        // symbols sorted by (length, value): canonical order
        let mut symbols = Vec::with_capacity(used.len());
        for len in 1..16 {
            for &i in &used {
                if lengths[i] as usize == len {
                    symbols.push(i as u16);
                }
            }
        }
        // 8-bit lookahead table; codes arrive most significant bit first
        let mut fast = vec![(0u8, 0u16); 256];
        let mut code = 0u32;
        let mut k = 0usize;
        for len in 1..16u32 {
            for _ in 0..counts[len as usize] {
                if len <= 8 {
                    let mut rev = 0u32;
                    for b in 0..len {
                        rev |= ((code >> b) & 1) << (len - 1 - b);
                    }
                    let mut fill = rev;
                    while fill < 256 {
                        fast[fill as usize] = (len as u8, symbols[k]);
                        fill += 1 << len;
                    }
                }
                code += 1;
                k += 1;
            }
            code <<= 1;
        }
        Ok(Code { single: None, fast, counts, symbols })
    }

    fn read(&self, b: &mut Bits) -> u16 {
        if let Some(s) = self.single {
            return s;
        }
        let (len, sym) = self.fast[b.peek8() as usize];
        if len > 0 {
            b.skip(len as u32);
            return sym;
        }
        // longer codes, a bit at a time
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.bits(1) as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return self.symbols.get((index + code - first) as usize).copied().unwrap_or(0);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        0
    }
}

const CODE_LENGTH_ORDER: [usize; 19] = [17, 18, 0, 1, 2, 3, 4, 5, 16, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

fn read_code(b: &mut Bits, alphabet: usize) -> Result<Code> {
    let mut lengths = vec![0u8; alphabet];
    if b.bit() {
        // simple code: one or two symbols
        let two = b.bit();
        let first8 = b.bit();
        let s0 = b.bits(if first8 { 8 } else { 1 }) as usize;
        if s0 >= alphabet {
            return Err(BAD);
        }
        lengths[s0] = 1;
        if two {
            let s1 = b.bits(8) as usize;
            if s1 >= alphabet {
                return Err(BAD);
            }
            lengths[s1] = 1;
        }
        return Code::from_lengths(&lengths);
    }
    let n = 4 + b.bits(4) as usize;
    if n > 19 {
        return Err(BAD);
    }
    let mut cl = [0u8; 19];
    for &i in CODE_LENGTH_ORDER.iter().take(n) {
        cl[i] = b.bits(3) as u8;
    }
    let clc = Code::from_lengths(&cl)?;
    let mut max_symbol = alphabet;
    if b.bit() {
        let nbits = 2 + 2 * b.bits(3);
        max_symbol = 2 + b.bits(nbits) as usize;
        if max_symbol > alphabet {
            return Err(BAD);
        }
    }
    let mut prev = 8u8;
    let mut sym = 0usize;
    while sym < alphabet {
        if max_symbol == 0 {
            break;
        }
        max_symbol -= 1;
        let c = clc.read(b);
        if c < 16 {
            lengths[sym] = c as u8;
            sym += 1;
            if c != 0 {
                prev = c as u8;
            }
        } else {
            let (reps, v) = match c {
                16 => (3 + b.bits(2) as usize, prev),
                17 => (3 + b.bits(3) as usize, 0),
                _ => (11 + b.bits(7) as usize, 0),
            };
            if sym + reps > alphabet {
                return Err(BAD);
            }
            for _ in 0..reps {
                lengths[sym] = v;
                sym += 1;
            }
        }
        if b.overrun() {
            return Err(BAD);
        }
    }
    Code::from_lengths(&lengths)
}

// ---------------------------------------------------------------- VP8L images

fn div_up(a: usize, bits: u32) -> usize {
    (a + (1 << bits) - 1) >> bits
}

fn add_px(a: u32, b: u32) -> u32 {
    let ag = (a & 0xff00ff00).wrapping_add(b & 0xff00ff00) & 0xff00ff00;
    let rb = (a & 0x00ff00ff).wrapping_add(b & 0x00ff00ff) & 0x00ff00ff;
    ag | rb
}

fn prefix_value(b: &mut Bits, p: u32) -> usize {
    if p < 4 {
        return p as usize + 1;
    }
    let extra = (p - 2) >> 1;
    let offset = (2 + (p & 1)) << extra;
    (offset + b.bits(extra)) as usize + 1
}

/// Distance codes 1..=120 name nearby pixels as (dx, dy).
const DIST_MAP: [(i8, i8); 120] = [
    (0, 1), (1, 0), (1, 1), (-1, 1), (0, 2), (2, 0), (1, 2), (-1, 2), (2, 1), (-2, 1), (2, 2), (-2, 2), (0, 3), (3, 0), (1, 3), (-1, 3), (3, 1), (-3, 1), (2, 3), (-2, 3),
    (3, 2), (-3, 2), (0, 4), (4, 0), (1, 4), (-1, 4), (4, 1), (-4, 1), (3, 3), (-3, 3), (2, 4), (-2, 4), (4, 2), (-4, 2), (0, 5), (3, 4), (-3, 4), (4, 3), (-4, 3), (5, 0),
    (1, 5), (-1, 5), (5, 1), (-5, 1), (2, 5), (-2, 5), (5, 2), (-5, 2), (4, 4), (-4, 4), (3, 5), (-3, 5), (5, 3), (-5, 3), (0, 6), (6, 0), (1, 6), (-1, 6), (6, 1), (-6, 1),
    (2, 6), (-2, 6), (6, 2), (-6, 2), (4, 5), (-4, 5), (5, 4), (-5, 4), (3, 6), (-3, 6), (6, 3), (-6, 3), (0, 7), (7, 0), (1, 7), (-1, 7), (5, 5), (-5, 5), (7, 1), (-7, 1),
    (4, 6), (-4, 6), (6, 4), (-6, 4), (2, 7), (-2, 7), (7, 2), (-7, 2), (3, 7), (-3, 7), (7, 3), (-7, 3), (5, 6), (-5, 6), (6, 5), (-6, 5), (8, 0), (4, 7), (-4, 7), (7, 4),
    (-7, 4), (8, 1), (8, 2), (6, 6), (-6, 6), (8, 3), (5, 7), (-5, 7), (7, 5), (-7, 5), (8, 4), (6, 7), (-6, 7), (7, 6), (-7, 6), (8, 5), (7, 7), (-7, 7), (8, 6), (8, 7),
];

/// An entropy-coded image (the main image when `main`, else a transform's
/// or the prefix-code map's sub-image).
fn decode_image(b: &mut Bits, w: usize, h: usize, main: bool) -> Result<Vec<u32>> {
    let total = w.checked_mul(h).filter(|&t| t <= super::MAX_PIXELS as usize).ok_or(BAD)?;
    let cache_bits = if b.bit() {
        let c = b.bits(4);
        if !(1..=11).contains(&c) {
            return Err(BAD);
        }
        c
    } else {
        0
    };
    let (meta_bits, meta) = if main && b.bit() {
        let mb = b.bits(3) + 2;
        let m = decode_image(b, div_up(w, mb), div_up(h, mb), false)?;
        (mb, m)
    } else {
        (0, Vec::new())
    };
    let groups = if meta.is_empty() { 1 } else { meta.iter().map(|p| ((p >> 8) & 0xffff) as usize).max().unwrap_or(0) + 1 };
    if groups > 4096 {
        return Err(BAD);
    }
    let cache_size = if cache_bits > 0 { 1usize << cache_bits } else { 0 };
    let mut codes: Vec<[Code; 5]> = Vec::with_capacity(groups);
    for _ in 0..groups {
        codes.push([read_code(b, 256 + 24 + cache_size)?, read_code(b, 256)?, read_code(b, 256)?, read_code(b, 256)?, read_code(b, 40)?]);
        if b.overrun() {
            return Err(BAD);
        }
    }
    let meta_w = if meta_bits > 0 { div_up(w, meta_bits) } else { 1 };
    let mut cache = vec![0u32; cache_size];
    let mut out = vec![0u32; total];
    let (mut pos, mut cached) = (0usize, 0usize);
    while pos < total {
        let (x, y) = (pos % w, pos / w);
        let g = if meta.is_empty() { 0 } else { ((meta[(y >> meta_bits) * meta_w + (x >> meta_bits)] >> 8) & 0xffff) as usize };
        let c = &codes[g];
        let s = c[0].read(b) as u32;
        if s < 256 {
            let red = c[1].read(b) as u32;
            let blue = c[2].read(b) as u32;
            let alpha = c[3].read(b) as u32;
            out[pos] = alpha << 24 | red << 16 | s << 8 | blue;
            pos += 1;
        } else if s < 256 + 24 {
            let len = prefix_value(b, s - 256);
            let dsym = c[4].read(b) as u32;
            let dcode = prefix_value(b, dsym);
            let dist = if dcode > 120 {
                dcode - 120
            } else {
                let (dx, dy) = DIST_MAP[dcode - 1];
                let d = dx as isize + dy as isize * w as isize;
                if d < 1 { 1 } else { d as usize }
            };
            if dist > pos || pos + len > total {
                return Err(BAD);
            }
            for i in 0..len {
                out[pos + i] = out[pos + i - dist];
            }
            pos += len;
        } else {
            let k = (s - 280) as usize;
            if k >= cache_size {
                return Err(BAD);
            }
            // bring the cache up to date first
            while cached < pos {
                let p = out[cached];
                cache[(0x1e35a7bdu32.wrapping_mul(p) >> (32 - cache_bits)) as usize] = p;
                cached += 1;
            }
            out[pos] = cache[k];
            pos += 1;
        }
        if cache_size > 0 {
            while cached < pos {
                let p = out[cached];
                cache[(0x1e35a7bdu32.wrapping_mul(p) >> (32 - cache_bits)) as usize] = p;
                cached += 1;
            }
        }
        if b.overrun() {
            return Err(BAD);
        }
    }
    Ok(out)
}

enum Transform {
    Predictor { bits: u32, data: Vec<u32>, w: usize },
    Color { bits: u32, data: Vec<u32>, w: usize },
    SubtractGreen,
    Indexing { palette: Vec<u32>, bits: u32, w: usize },
}

fn avg(a: u32, b: u32) -> u32 {
    (((a ^ b) & 0xfefefefe) >> 1) + (a & b)
}

fn channels(p: u32) -> [i32; 4] {
    [(p >> 24) as i32, ((p >> 16) & 255) as i32, ((p >> 8) & 255) as i32, (p & 255) as i32]
}

fn pack(c: [i32; 4]) -> u32 {
    (c[0].clamp(0, 255) as u32) << 24 | (c[1].clamp(0, 255) as u32) << 16 | (c[2].clamp(0, 255) as u32) << 8 | c[3].clamp(0, 255) as u32
}

fn select(l: u32, t: u32, tl: u32) -> u32 {
    let (cl, ct, ctl) = (channels(l), channels(t), channels(tl));
    let mut pl = 0;
    let mut pt = 0;
    for i in 0..4 {
        pl += (ct[i] - ctl[i]).abs();
        pt += (cl[i] - ctl[i]).abs();
    }
    if pl < pt {
        l
    } else {
        t
    }
}

fn predict(mode: u32, l: u32, t: u32, tr: u32, tl: u32) -> u32 {
    match mode {
        0 => 0xff000000,
        1 => l,
        2 => t,
        3 => tr,
        4 => tl,
        5 => avg(avg(l, tr), t),
        6 => avg(l, tl),
        7 => avg(l, t),
        8 => avg(tl, t),
        9 => avg(t, tr),
        10 => avg(avg(l, tl), avg(t, tr)),
        11 => select(l, t, tl),
        12 => {
            let (a, b, c) = (channels(l), channels(t), channels(tl));
            pack([a[0] + b[0] - c[0], a[1] + b[1] - c[1], a[2] + b[2] - c[2], a[3] + b[3] - c[3]])
        }
        13 => {
            let (a, c) = (channels(avg(l, t)), channels(tl));
            let f = |i: usize| a[i] + (a[i] - c[i]) / 2;
            pack([f(0), f(1), f(2), f(3)])
        }
        _ => 0xff000000,
    }
}

fn delta(t: u32, c: u32) -> i32 {
    ((t as u8 as i8) as i32 * (c as u8 as i8) as i32) >> 5
}

/// A VP8L image stream (after the 5-byte header): transforms, then pixels.
fn decode_stream(b: &mut Bits, w: usize, h: usize) -> Result<Vec<u32>> {
    let mut xs = w;
    let mut transforms: Vec<Transform> = Vec::new();
    let mut seen = 0u8;
    while b.bit() {
        let t = b.bits(2);
        if seen & (1 << t) != 0 {
            return Err(BAD);
        }
        seen |= 1 << t;
        match t {
            0 | 1 => {
                let bits = b.bits(3) + 2;
                let data = decode_image(b, div_up(xs, bits), div_up(h, bits), false)?;
                transforms.push(if t == 0 { Transform::Predictor { bits, data, w: xs } } else { Transform::Color { bits, data, w: xs } });
            }
            2 => transforms.push(Transform::SubtractGreen),
            _ => {
                let n = b.bits(8) as usize + 1;
                let mut palette = decode_image(b, n, 1, false)?;
                for i in 1..n {
                    palette[i] = add_px(palette[i], palette[i - 1]);
                }
                let bits = if n <= 2 {
                    3
                } else if n <= 4 {
                    2
                } else if n <= 16 {
                    1
                } else {
                    0
                };
                transforms.push(Transform::Indexing { palette, bits, w: xs });
                xs = div_up(xs, bits);
            }
        }
    }
    let mut px = decode_image(b, xs, h, true)?;
    for t in transforms.iter().rev() {
        match t {
            Transform::SubtractGreen => {
                for p in px.iter_mut() {
                    let g = (*p >> 8) & 0xff;
                    *p = (*p & 0xff00ff00) | ((((*p >> 16) & 0xff) + g) & 0xff) << 16 | (((*p & 0xff) + g) & 0xff);
                }
            }
            Transform::Predictor { bits, data, w } => {
                let (w, bw) = (*w, div_up(*w, *bits));
                for y in 0..h {
                    for x in 0..w {
                        let i = y * w + x;
                        let pred = if y == 0 && x == 0 {
                            0xff000000
                        } else if y == 0 {
                            px[i - 1]
                        } else if x == 0 {
                            px[i - w]
                        } else {
                            let mode = (data[(y >> bits) * bw + (x >> bits)] >> 8) & 0xf;
                            predict(mode, px[i - 1], px[i - w], px[i - w + 1], px[i - w - 1])
                        };
                        px[i] = add_px(px[i], pred);
                    }
                }
            }
            Transform::Color { bits, data, w } => {
                let (w, bw) = (*w, div_up(*w, *bits));
                for y in 0..h {
                    for x in 0..w {
                        let e = data[(y >> bits) * bw + (x >> bits)];
                        let p = px[y * w + x];
                        let (g, r0, b0) = ((p >> 8) & 0xff, (p >> 16) & 0xff, p & 0xff);
                        let r = (r0 as i32 + delta(e, g)) & 0xff;
                        let mut bl = b0 as i32 + delta(e >> 8, g);
                        bl = (bl + delta(e >> 16, r as u32)) & 0xff;
                        px[y * w + x] = (p & 0xff00ff00) | (r as u32) << 16 | bl as u32;
                    }
                }
            }
            Transform::Indexing { palette, bits, w } => {
                let packed_w = div_up(*w, *bits);
                let per = 8 >> bits;
                let mask = (1u32 << per) - 1;
                let mut out = vec![0u32; w * h];
                for y in 0..h {
                    for x in 0..*w {
                        let p = px[y * packed_w + (x >> bits)];
                        let idx = ((p >> 8) >> ((x & ((1 << bits) - 1)) as u32 * per)) & mask;
                        out[y * w + x] = palette.get(idx as usize).copied().unwrap_or(0);
                    }
                }
                px = out;
            }
        }
    }
    Ok(px)
}

fn vp8l(d: &[u8]) -> Result<Image> {
    if d.len() < 5 || d[0] != 0x2f {
        return Err(BAD);
    }
    let mut b = Bits::new(&d[1..]);
    let w = b.bits(14) as usize + 1;
    let h = b.bits(14) as usize + 1;
    b.bits(1); // alpha hint
    if b.bits(3) != 0 {
        return Err(BAD);
    }
    check_size(w as u32, h as u32)?;
    let px = decode_stream(&mut b, w, h)?;
    Ok(Image { w: w as u32, h: h as u32, px, frames: Vec::new() })
}

/// An ALPH chunk: the alpha plane of a lossy image.
fn alpha_plane(d: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    let head = *d.first().ok_or(BAD)?;
    let (method, filter) = (head & 3, (head >> 2) & 3);
    let mut a = match method {
        0 => d.get(1..1 + w * h).ok_or(BAD)?.to_vec(),
        1 => {
            let mut b = Bits::new(&d[1..]);
            decode_stream(&mut b, w, h)?.iter().map(|p| (p >> 8) as u8).collect()
        }
        _ => return Err(BAD),
    };
    // undo the prediction filter
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let left = if x > 0 { a[i - 1] } else { 0 };
            let top = if y > 0 { a[i - w] } else { 0 };
            let pred = match filter {
                0 => 0,
                1 => {
                    if x == 0 {
                        top
                    } else {
                        left
                    }
                }
                2 => {
                    if y == 0 {
                        left
                    } else {
                        top
                    }
                }
                _ => {
                    if y == 0 {
                        left
                    } else if x == 0 {
                        top
                    } else {
                        (left as i32 + top as i32 - a[i - w - 1] as i32).clamp(0, 255) as u8
                    }
                }
            };
            a[i] = a[i].wrapping_add(pred);
        }
    }
    Ok(a)
}

/// One picture: VP8 or VP8L data, and optionally an ALPH chunk.
fn still(image: (&[u8], &[u8]), alpha: Option<&[u8]>) -> Result<Image> {
    match image {
        (b"VP8L", d) => vp8l(d),
        (b"VP8 ", d) => {
            let mut img = vp8::decode(d)?;
            if let Some(a) = alpha {
                // a damaged alpha plane leaves the picture opaque
                if let Ok(plane) = alpha_plane(a, img.w as usize, img.h as usize) {
                    for (p, a) in img.px.iter_mut().zip(plane) {
                        *p = (*p & 0xffffff) | (a as u32) << 24;
                    }
                }
            }
            Ok(img)
        }
        _ => Err(BAD),
    }
}

fn chunks(d: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 8 <= d.len() {
        let size = u32::from_le_bytes([d[p + 4], d[p + 5], d[p + 6], d[p + 7]]) as usize;
        let end = (p + 8).saturating_add(size).min(d.len());
        out.push((&d[p..p + 4], &d[p + 8..end]));
        p = p + 8 + size + (size & 1);
    }
    out
}

fn le24(b: &[u8]) -> usize {
    b[0] as usize | (b[1] as usize) << 8 | (b[2] as usize) << 16
}

pub fn decode(d: &[u8]) -> Result<Image> {
    if d.len() < 20 || &d[..4] != b"RIFF" || &d[8..12] != b"WEBP" {
        return Err(BAD);
    }
    let list = chunks(&d[12..]);
    let find = |k: &[u8]| list.iter().find(|c| c.0 == k).map(|c| c.1);
    let Some(vp8x) = find(b"VP8X") else {
        // a simple file: one VP8 or VP8L chunk
        let c = list.iter().find(|c| c.0 == b"VP8 " || c.0 == b"VP8L").ok_or(BAD)?;
        return still(*c, None);
    };
    if vp8x.len() < 10 {
        return Err(BAD);
    }
    let (cw, ch) = (le24(&vp8x[4..]) + 1, le24(&vp8x[7..]) + 1);
    check_size(cw as u32, ch as u32)?;
    if vp8x[0] & 0x02 == 0 {
        // still image, maybe with alpha
        let c = list.iter().find(|c| c.0 == b"VP8 " || c.0 == b"VP8L").ok_or(BAD)?;
        return still(*c, find(b"ALPH"));
    }
    // animation: compose each frame onto the canvas
    let mut canvas = vec![0u32; cw * ch];
    let mut frames: Vec<Frame> = Vec::new();
    let mut kept = 0usize;
    for (kind, body) in &list {
        if *kind != b"ANMF" || body.len() < 16 {
            continue;
        }
        let (fx, fy) = (le24(&body[0..]) * 2, le24(&body[3..]) * 2);
        let delay = le24(&body[12..]) as u32;
        let flags = body[15];
        let inner = chunks(&body[16..]);
        let Some(pic) = inner.iter().find(|c| c.0 == b"VP8 " || c.0 == b"VP8L") else { continue };
        let alpha = inner.iter().find(|c| c.0 == b"ALPH").map(|c| c.1);
        let Ok(img) = still(*pic, alpha) else { break };
        let blend = flags & 0x02 == 0;
        for y in 0..img.h as usize {
            for x in 0..img.w as usize {
                let (cx, cy) = (fx + x, fy + y);
                if cx >= cw || cy >= ch {
                    continue;
                }
                let s = img.px[y * img.w as usize + x];
                let dst = &mut canvas[cy * cw + cx];
                *dst = if blend { over(s, *dst) } else { s };
            }
        }
        frames.push(Frame { px: canvas.clone(), delay: if delay == 0 { 100 } else { delay } });
        kept += cw * ch;
        if flags & 0x01 != 0 {
            // dispose to transparent after showing
            for y in fy..(fy + img.h as usize).min(ch) {
                for x in fx..(fx + img.w as usize).min(cw) {
                    canvas[y * cw + x] = 0;
                }
            }
        }
        if kept > MAX_ANIM_PIXELS {
            break;
        }
    }
    let first = frames.first().ok_or(BAD)?.px.clone();
    Ok(Image { w: cw as u32, h: ch as u32, px: first, frames: if frames.len() > 1 { frames } else { Vec::new() } })
}

/// Straight-alpha "source over destination".
pub fn over(s: u32, d: u32) -> u32 {
    let sa = s >> 24;
    if sa == 255 {
        return s;
    }
    if sa == 0 {
        return d;
    }
    let da = d >> 24;
    let oa = sa + da * (255 - sa) / 255;
    if oa == 0 {
        return 0;
    }
    let ch = |sh: u32| {
        let sc = (s >> sh) & 255;
        let dc = (d >> sh) & 255;
        ((sc * sa + dc * da * (255 - sa) / 255) / oa).min(255)
    };
    oa << 24 | ch(16) << 16 | ch(8) << 8 | ch(0)
}
