//! JPEG (ITU T.81): baseline and progressive Huffman-coded images, any
//! chroma subsampling, restart markers, greyscale, YCbCr, Adobe RGB/CMYK/YCCK,
//! and EXIF orientation. Integer IDCT (no floating point).

use super::{check_size, Image, Result};
use alloc::vec;
use alloc::vec::Vec;

const BAD: &str = "the JPEG image is damaged";

/// Zigzag position -> natural (row-major) position.
const ZZ: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44,
    51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

#[derive(Clone, Default)]
struct Huff {
    /// 9-bit lookahead: (code length, symbol); length 0 = not in the table
    fast: Vec<(u8, u8)>,
    maxcode: [i32; 18],
    valptr: [i32; 17],
    mincode: [i32; 17],
    vals: Vec<u8>,
}

const FAST: u32 = 9;

impl Huff {
    fn new(counts: &[u8; 16], vals: &[u8]) -> Huff {
        let mut h = Huff { fast: vec![(0, 0); 1 << FAST], maxcode: [-1; 18], valptr: [0; 17], mincode: [0; 17], vals: vals.to_vec() };
        let mut code = 0i32;
        let mut k = 0usize;
        for l in 1..=16 {
            let n = counts[l - 1] as usize;
            h.valptr[l] = k as i32;
            h.mincode[l] = code;
            for i in 0..n {
                if l as u32 <= FAST && k + i < vals.len() {
                    let c = (code + i as i32) as u32;
                    let shift = FAST - l as u32;
                    for fill in 0..(1u32 << shift) {
                        h.fast[((c << shift) | fill) as usize] = (l as u8, vals[k + i]);
                    }
                }
            }
            code += n as i32;
            k += n;
            h.maxcode[l] = if n > 0 { code - 1 } else { -1 };
            code <<= 1;
        }
        h.maxcode[17] = i32::MAX;
        h
    }
}

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u32,
    n: u32,
    /// hit a marker: feed zeros from now on
    stop: bool,
}

impl<'a> Bits<'a> {
    fn fill(&mut self) {
        while self.n <= 24 {
            let mut byte = 0u32;
            if !self.stop {
                match self.d.get(self.pos) {
                    Some(&0xff) => match self.d.get(self.pos + 1) {
                        Some(0) => {
                            byte = 0xff;
                            self.pos += 2;
                        }
                        _ => self.stop = true,
                    },
                    Some(&b) => {
                        byte = b as u32;
                        self.pos += 1;
                    }
                    None => self.stop = true,
                }
            }
            self.acc |= byte << (24 - self.n);
            self.n += 8;
        }
    }

    fn bits(&mut self, k: u32) -> u32 {
        // at most 16 bits at a time (damaged tables can ask for more)
        let k = k.min(16);
        if k == 0 {
            return 0;
        }
        self.fill();
        let v = self.acc >> (32 - k);
        self.acc <<= k;
        self.n -= k;
        v
    }

    fn bit(&mut self) -> bool {
        self.bits(1) == 1
    }

    fn extend(&mut self, s: u32) -> i32 {
        let s = s.min(16);
        if s == 0 {
            return 0;
        }
        let v = self.bits(s) as i32;
        if v < 1 << (s - 1) {
            v - (1 << s) + 1
        } else {
            v
        }
    }

    fn decode(&mut self, h: &Huff) -> Result<u8> {
        self.fill();
        let (len, sym) = h.fast[(self.acc >> (32 - FAST)) as usize];
        if len > 0 {
            self.acc <<= len;
            self.n -= len as u32;
            return Ok(sym);
        }
        let mut code = 0i32;
        for l in 1..=16 {
            code = (code << 1) | self.bits(1) as i32;
            if code <= h.maxcode[l] {
                let i = h.valptr[l] + code - h.mincode[l];
                return h.vals.get(i as usize).copied().ok_or(BAD);
            }
        }
        Err(BAD)
    }

    /// At a restart interval: drop buffered bits and skip the RSTn marker.
    fn restart(&mut self) {
        self.acc = 0;
        self.n = 0;
        self.stop = false;
        while self.pos + 1 < self.d.len() {
            if self.d[self.pos] == 0xff && (0xd0..=0xd7).contains(&self.d[self.pos + 1]) {
                self.pos += 2;
                return;
            }
            if self.d[self.pos] == 0xff && self.d[self.pos + 1] != 0 && self.d[self.pos + 1] != 0xff {
                return; // some other marker: let the caller see it
            }
            self.pos += 1;
        }
    }
}

struct Comp {
    id: u8,
    h: usize,
    v: usize,
    tq: usize,
    bw: usize,
    bh: usize,
    coefs: Vec<i16>,
    pred: i32,
    dc: usize,
    ac: usize,
}

// stb_image-style integer IDCT (12-bit fixed point constants), in 64-bit so
// damaged files can't overflow it
fn idct_1d(s: [i64; 8]) -> ([i64; 4], [i64; 4]) {
    let (p2, p3) = (s[2], s[6]);
    let p1 = (p2 + p3) * 2217;
    let t2 = p1 + p3 * -7567;
    let t3 = p1 + p2 * 3135;
    let (p2, p3) = (s[0], s[4]);
    let t0 = (p2 + p3) << 12;
    let t1 = (p2 - p3) << 12;
    let x = [t0 + t3, t1 + t2, t1 - t2, t0 - t3];
    let (mut t0, mut t1, mut t2, mut t3) = (s[7], s[5], s[3], s[1]);
    let p3 = t0 + t2;
    let p4 = t1 + t3;
    let p1 = t0 + t3;
    let p2 = t1 + t2;
    let p5 = (p3 + p4) * 4816;
    t0 *= 1223;
    t1 *= 8410;
    t2 *= 12586;
    t3 *= 6149;
    let p1 = p5 + p1 * -3685;
    let p2 = p5 + p2 * -10497;
    let p3 = p3 * -8034;
    let p4 = p4 * -1597;
    t3 += p1 + p4;
    t2 += p2 + p3;
    t1 += p2 + p4;
    t0 += p1 + p3;
    (x, [t3, t2, t1, t0])
}

fn idct(c: &[i32; 64], out: &mut [u8], stride: usize) {
    let mut v = [0i64; 64];
    for col in 0..8 {
        let s: [i64; 8] = core::array::from_fn(|r| c[r * 8 + col] as i64);
        if s[1..].iter().all(|&x| x == 0) {
            let dc = s[0] << 2;
            for r in 0..8 {
                v[r * 8 + col] = dc;
            }
            continue;
        }
        let (mut x, t) = idct_1d(s);
        for xi in x.iter_mut() {
            *xi += 512;
        }
        v[col] = (x[0] + t[0]) >> 10;
        v[56 + col] = (x[0] - t[0]) >> 10;
        v[8 + col] = (x[1] + t[1]) >> 10;
        v[48 + col] = (x[1] - t[1]) >> 10;
        v[16 + col] = (x[2] + t[2]) >> 10;
        v[40 + col] = (x[2] - t[2]) >> 10;
        v[24 + col] = (x[3] + t[3]) >> 10;
        v[32 + col] = (x[3] - t[3]) >> 10;
    }
    for row in 0..8 {
        let s: [i64; 8] = core::array::from_fn(|i| v[row * 8 + i]);
        let (mut x, t) = idct_1d(s);
        for xi in x.iter_mut() {
            *xi += 65536 + (128 << 17);
        }
        let o = &mut out[row * stride..row * stride + 8];
        let clamp = |v: i64| (v >> 17).clamp(0, 255) as u8;
        o[0] = clamp(x[0] + t[0]);
        o[7] = clamp(x[0] - t[0]);
        o[1] = clamp(x[1] + t[1]);
        o[6] = clamp(x[1] - t[1]);
        o[2] = clamp(x[2] + t[2]);
        o[5] = clamp(x[2] - t[2]);
        o[3] = clamp(x[3] + t[3]);
        o[4] = clamp(x[3] - t[3]);
    }
}

struct Scan<'s> {
    comps: Vec<usize>,
    ss: usize,
    se: usize,
    ah: u32,
    al: u32,
    dc: &'s [Huff; 4],
    ac: &'s [Huff; 4],
}

fn decode_block(b: &mut Bits, c: &mut Comp, blk: usize, s: &Scan, progressive: bool, eobrun: &mut u32) -> Result<()> {
    let coefs = &mut c.coefs[blk * 64..blk * 64 + 64];
    if !progressive {
        let t = b.decode(&s.dc[c.dc])?;
        c.pred += b.extend(t as u32);
        coefs[0] = c.pred as i16;
        let mut k = 1;
        while k < 64 {
            let rs = b.decode(&s.ac[c.ac])?;
            let (r, sz) = ((rs >> 4) as usize, (rs & 15) as u32);
            if sz == 0 {
                if r == 15 {
                    k += 16;
                    continue;
                }
                break;
            }
            k += r;
            if k > 63 {
                return Err(BAD);
            }
            coefs[ZZ[k]] = b.extend(sz) as i16;
            k += 1;
        }
        return Ok(());
    }
    if s.ss == 0 {
        // DC scans
        if s.ah == 0 {
            let t = b.decode(&s.dc[c.dc])?;
            c.pred += b.extend(t as u32);
            coefs[0] = (c.pred << s.al) as i16;
        } else if b.bit() {
            coefs[0] |= 1 << s.al;
        }
        return Ok(());
    }
    if s.ah == 0 {
        // first AC scan
        if *eobrun > 0 {
            *eobrun -= 1;
            return Ok(());
        }
        let mut k = s.ss;
        while k <= s.se {
            let rs = b.decode(&s.ac[c.ac])?;
            let (r, sz) = ((rs >> 4) as u32, (rs & 15) as u32);
            if sz == 0 {
                if r < 15 {
                    *eobrun = (1 << r) - 1;
                    if r > 0 {
                        *eobrun += b.bits(r);
                    }
                    break;
                }
                k += 16;
                continue;
            }
            k += r as usize;
            if k > 63 {
                return Err(BAD);
            }
            coefs[ZZ[k]] = (b.extend(sz) * (1 << s.al)) as i16;
            k += 1;
        }
        return Ok(());
    }
    // AC refinement
    let p1 = 1i16 << s.al;
    let m1 = -1i16 << s.al;
    let mut k = s.ss;
    let refine = |b: &mut Bits, v: &mut i16| {
        if b.bit() && (*v & p1) == 0 {
            *v += if *v >= 0 { p1 } else { m1 };
        }
    };
    if *eobrun == 0 {
        while k <= s.se {
            let rs = b.decode(&s.ac[c.ac])?;
            let (mut r, sz) = ((rs >> 4) as i32, (rs & 15) as u32);
            let mut val = 0i16;
            if sz == 0 {
                if r < 15 {
                    *eobrun = 1 << r;
                    if r > 0 {
                        *eobrun += b.bits(r as u32);
                    }
                    break;
                }
            } else {
                val = if b.bit() { p1 } else { m1 };
            }
            while k <= s.se {
                let z = ZZ[k];
                k += 1;
                if coefs[z] != 0 {
                    refine(b, &mut coefs[z]);
                } else {
                    if r == 0 {
                        coefs[z] = val;
                        break;
                    }
                    r -= 1;
                }
            }
        }
    }
    if *eobrun > 0 {
        while k <= s.se {
            let z = ZZ[k];
            if coefs[z] != 0 {
                refine(b, &mut coefs[z]);
            }
            k += 1;
        }
        *eobrun -= 1;
    }
    Ok(())
}

fn exif_orientation(d: &[u8]) -> u16 {
    // "Exif\0\0" then a TIFF header
    if d.len() < 14 || &d[..6] != b"Exif\0\0" {
        return 1;
    }
    let t = &d[6..];
    let le = &t[..2] == b"II";
    let u16at = |p: usize| -> Option<u16> {
        let b = t.get(p..p + 2)?;
        Some(if le { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    };
    let u32at = |p: usize| -> Option<u32> {
        let b = t.get(p..p + 4)?;
        Some(if le { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) })
    };
    let f = || -> Option<u16> {
        let ifd = u32at(4)? as usize;
        let n = u16at(ifd)? as usize;
        for i in 0..n {
            let e = ifd + 2 + 12 * i;
            if u16at(e)? == 0x0112 {
                return u16at(e + 8);
            }
        }
        None
    };
    f().unwrap_or(1)
}

fn orient(img: Image, o: u16) -> Image {
    if !(2..=8).contains(&o) {
        return img;
    }
    let (w, h) = (img.w as usize, img.h as usize);
    let swap = o >= 5;
    let (nw, nh) = if swap { (h, w) } else { (w, h) };
    let mut out = Image::new(nw as u32, nh as u32);
    for y in 0..nh {
        for x in 0..nw {
            // where this output pixel comes from
            let (sx, sy) = match o {
                2 => (w - 1 - x, y),
                3 => (w - 1 - x, h - 1 - y),
                4 => (x, h - 1 - y),
                5 => (y, x),
                6 => (y, h - 1 - x),
                7 => (w - 1 - y, h - 1 - x),
                _ => (w - 1 - y, x),
            };
            out.px[y * nw + x] = img.px[sy * w + sx];
        }
    }
    out
}

pub fn decode(d: &[u8]) -> Result<Image> {
    let mut pos = 2;
    let mut qt = [[0i32; 64]; 4];
    let mut dc: [Huff; 4] = Default::default();
    let mut ac: [Huff; 4] = Default::default();
    let mut comps: Vec<Comp> = Vec::new();
    let (mut w, mut h) = (0usize, 0usize);
    let mut progressive = false;
    let mut restart = 0usize;
    let mut adobe: Option<u8> = None;
    let mut orientation = 1;
    let (mut hmax, mut vmax) = (1usize, 1usize);
    let (mut mcux, mut mcuy) = (0usize, 0usize);
    let mut eobrun;
    loop {
        // find the next marker
        while pos < d.len() && d[pos] != 0xff {
            pos += 1;
        }
        while pos < d.len() && d[pos] == 0xff {
            pos += 1;
        }
        let Some(&marker) = d.get(pos) else { break };
        pos += 1;
        if marker == 0xd9 {
            break;
        }
        if (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            continue;
        }
        let len = u16::from_be_bytes([*d.get(pos).ok_or(BAD)?, *d.get(pos + 1).ok_or(BAD)?]) as usize;
        let seg = d.get(pos + 2..pos + len).ok_or(BAD)?;
        let next = pos + len;
        match marker {
            0xdb => {
                let mut p = 0;
                while p < seg.len() {
                    let (pq, tq) = (seg[p] >> 4, (seg[p] & 3) as usize);
                    p += 1;
                    for k in 0..64 {
                        let v = if pq == 1 {
                            let v = u16::from_be_bytes([*seg.get(p + 2 * k).ok_or(BAD)?, *seg.get(p + 2 * k + 1).ok_or(BAD)?]);
                            v as i32
                        } else {
                            *seg.get(p + k).ok_or(BAD)? as i32
                        };
                        qt[tq][ZZ[k]] = v;
                    }
                    p += if pq == 1 { 128 } else { 64 };
                }
            }
            0xc4 => {
                let mut p = 0;
                while p + 17 <= seg.len() {
                    let (tc, th) = (seg[p] >> 4, (seg[p] & 3) as usize);
                    let mut counts = [0u8; 16];
                    counts.copy_from_slice(&seg[p + 1..p + 17]);
                    let n: usize = counts.iter().map(|&c| c as usize).sum();
                    let vals = seg.get(p + 17..p + 17 + n).ok_or(BAD)?;
                    let t = Huff::new(&counts, vals);
                    if tc == 0 {
                        dc[th] = t;
                    } else {
                        ac[th] = t;
                    }
                    p += 17 + n;
                }
            }
            0xc0 | 0xc1 | 0xc2 => {
                progressive = marker == 0xc2;
                if seg.first() != Some(&8) {
                    return Err("12-bit JPEG images aren't supported");
                }
                h = u16::from_be_bytes([seg[1], seg[2]]) as usize;
                w = u16::from_be_bytes([seg[3], seg[4]]) as usize;
                check_size(w as u32, h as u32)?;
                let n = seg[5] as usize;
                if !(n == 1 || n == 3 || n == 4) {
                    return Err(BAD);
                }
                for i in 0..n {
                    let c = seg.get(6 + 3 * i..9 + 3 * i).ok_or(BAD)?;
                    let (hs, vs) = ((c[1] >> 4) as usize, (c[1] & 15) as usize);
                    if !(1..=4).contains(&hs) || !(1..=4).contains(&vs) {
                        return Err(BAD);
                    }
                    comps.push(Comp { id: c[0], h: hs, v: vs, tq: (c[2] & 3) as usize, bw: 0, bh: 0, coefs: Vec::new(), pred: 0, dc: 0, ac: 0 });
                }
                if n == 1 {
                    // one component: its blocks are the MCUs, whatever it says
                    comps[0].h = 1;
                    comps[0].v = 1;
                }
                hmax = comps.iter().map(|c| c.h).max().unwrap_or(1);
                vmax = comps.iter().map(|c| c.v).max().unwrap_or(1);
                mcux = w.div_ceil(8 * hmax);
                mcuy = h.div_ceil(8 * vmax);
                for c in comps.iter_mut() {
                    c.bw = mcux * c.h;
                    c.bh = mcuy * c.v;
                    c.coefs = vec![0; c.bw * c.bh * 64];
                }
            }
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => return Err("this kind of JPEG (lossless or arithmetic-coded) isn't supported"),
            0xdd => restart = u16::from_be_bytes([seg[0], seg[1]]) as usize,
            0xee if seg.starts_with(b"Adobe") && seg.len() >= 12 => adobe = Some(seg[11]),
            0xe1 => orientation = exif_orientation(seg),
            0xda => {
                if comps.is_empty() {
                    return Err(BAD);
                }
                let ns = seg[0] as usize;
                let mut idx = Vec::new();
                for i in 0..ns {
                    let id = *seg.get(1 + 2 * i).ok_or(BAD)?;
                    let t = *seg.get(2 + 2 * i).ok_or(BAD)?;
                    let k = comps.iter().position(|c| c.id == id).ok_or(BAD)?;
                    comps[k].dc = (t >> 4) as usize & 3;
                    comps[k].ac = (t & 15) as usize & 3;
                    idx.push(k);
                }
                let p = 1 + 2 * ns;
                let (ss, se, a) = (*seg.get(p).ok_or(BAD)? as usize, *seg.get(p + 1).ok_or(BAD)? as usize, *seg.get(p + 2).ok_or(BAD)?);
                if se > 63 || ss > se {
                    return Err(BAD);
                }
                let scan = Scan { comps: idx, ss, se, ah: (a >> 4) as u32, al: (a & 15) as u32, dc: &dc, ac: &ac };
                let mut b = Bits { d, pos: next, acc: 0, n: 0, stop: false };
                for c in comps.iter_mut() {
                    c.pred = 0;
                }
                eobrun = 0;
                let single = scan.comps.len() == 1;
                let (ux, uy) = if single {
                    let c = &comps[scan.comps[0]];
                    ((w * c.h).div_ceil(hmax).div_ceil(8), (h * c.v).div_ceil(vmax).div_ceil(8))
                } else {
                    (mcux, mcuy)
                };
                let mut count = 0usize;
                'mcus: for my in 0..uy {
                    for mx in 0..ux {
                        if restart > 0 && count > 0 && count % restart == 0 {
                            b.restart();
                            for c in comps.iter_mut() {
                                c.pred = 0;
                            }
                            eobrun = 0;
                        }
                        count += 1;
                        if single {
                            let c = &mut comps[scan.comps[0]];
                            let blk = my * c.bw + mx;
                            decode_block(&mut b, c, blk, &scan, progressive, &mut eobrun)?;
                        } else {
                            for &k in &scan.comps {
                                let c = &mut comps[k];
                                for v in 0..c.v {
                                    for hh in 0..c.h {
                                        let blk = (my * c.v + v) * c.bw + mx * c.h + hh;
                                        decode_block(&mut b, c, blk, &scan, progressive, &mut eobrun)?;
                                    }
                                }
                            }
                        }
                        if b.stop && b.pos >= d.len() {
                            break 'mcus;
                        }
                    }
                }
                // continue after the entropy-coded data
                pos = b.pos;
                continue;
            }
            _ => {}
        }
        pos = next;
    }
    if comps.is_empty() {
        return Err(BAD);
    }
    // dequantise, inverse DCT
    let mut planes: Vec<Vec<u8>> = Vec::new();
    for c in &comps {
        let stride = c.bw * 8;
        let mut plane = vec![0u8; stride * c.bh * 8];
        let q = &qt[c.tq];
        let mut blk = [0i32; 64];
        for by in 0..c.bh {
            for bx in 0..c.bw {
                let co = &c.coefs[(by * c.bw + bx) * 64..][..64];
                for i in 0..64 {
                    blk[i] = (co[i] as i32).saturating_mul(q[i]);
                }
                idct(&blk, &mut plane[by * 8 * stride + bx * 8..], stride);
            }
        }
        planes.push(plane);
    }
    // upsample (bilinear for 2x, like libjpeg's "fancy" upsampling) and convert
    let mut img = Image::new(w as u32, h as u32);
    let n = comps.len();
    let mut row: Vec<[u8; 4]> = vec![[0; 4]; w];
    let transform = adobe.unwrap_or(if n == 3 && comps[0].id == b'R' && comps[1].id == b'G' { 0 } else { 1 });
    for y in 0..h {
        for (ci, c) in comps.iter().enumerate() {
            let stride = c.bw * 8;
            let plane = &planes[ci];
            let (pw, ph) = (stride as i32, (c.bh * 8) as i32);
            let full = c.h == hmax && c.v == vmax;
            // like libjpeg: interpolate a 2x subsampled axis, repeat otherwise
            let (lin_x, lin_y) = (hmax == 2 * c.h, vmax == 2 * c.v);
            // vertical sample position in 1/256ths
            let fy = if lin_y { ((2 * y as i32 + 1) * c.v as i32 * 128) / vmax as i32 - 128 } else { (y * c.v / vmax) as i32 * 256 };
            let (y0, wy) = (fy.div_euclid(256), fy.rem_euclid(256));
            let (ya, yb) = (y0.clamp(0, ph - 1) as usize, (y0 + 1).clamp(0, ph - 1) as usize);
            for x in 0..w {
                row[x][ci] = if full {
                    plane[y * stride + x]
                } else {
                    let fx = if lin_x { ((2 * x as i32 + 1) * c.h as i32 * 128) / hmax as i32 - 128 } else { (x * c.h / hmax) as i32 * 256 };
                    let (x0, wx) = (fx.div_euclid(256), fx.rem_euclid(256));
                    let (xa, xb) = (x0.clamp(0, pw - 1) as usize, (x0 + 1).clamp(0, pw - 1) as usize);
                    let top = plane[ya * stride + xa] as i32 * (256 - wx) + plane[ya * stride + xb] as i32 * wx;
                    let bot = plane[yb * stride + xa] as i32 * (256 - wx) + plane[yb * stride + xb] as i32 * wx;
                    ((top * (256 - wy) + bot * wy + (1 << 15)) >> 16) as u8
                };
            }
        }
        for x in 0..w {
            let p = row[x];
            let ycc = |yv: u8, cb: u8, cr: u8| -> (u32, u32, u32) {
                let (yv, cb, cr) = ((yv as i32) << 16, cb as i32 - 128, cr as i32 - 128);
                let r = (yv + 91881 * cr + 32768) >> 16;
                let g = (yv - 22554 * cb - 46802 * cr + 32768) >> 16;
                let b = (yv + 116130 * cb + 32768) >> 16;
                (r.clamp(0, 255) as u32, g.clamp(0, 255) as u32, b.clamp(0, 255) as u32)
            };
            let (r, g, b) = match n {
                1 => (p[0] as u32, p[0] as u32, p[0] as u32),
                3 if transform == 0 => (p[0] as u32, p[1] as u32, p[2] as u32),
                3 => ycc(p[0], p[1], p[2]),
                _ => {
                    // Adobe CMYK is stored inverted; YCCK is YCbCr-coded CMY
                    let (c, m, yy) = if transform == 2 { ycc(p[0], p[1], p[2]) } else { (p[0] as u32, p[1] as u32, p[2] as u32) };
                    let (c, m, yy) = if transform == 2 { (255 - c, 255 - m, 255 - yy) } else { (c, m, yy) };
                    let k = p[3] as u32;
                    (c * k / 255, m * k / 255, yy * k / 255)
                }
            };
            img.px[y * w + x] = 0xff00_0000 | r << 16 | g << 8 | b;
        }
    }
    Ok(orient(img, orientation))
}
