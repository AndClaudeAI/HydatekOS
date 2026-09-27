//! QR Code encoder (ISO/IEC 18004): byte mode, error correction L or M,
//! versions 1-40, with penalty-scored mask selection. Used for the Phone Link
//! pairing code.

use alloc::vec;
use alloc::vec::Vec;

#[allow(dead_code)] // Low is used by the host tests
#[derive(Clone, Copy, PartialEq)]
pub enum Ecc {
    Low,
    Medium,
}

const ECC_PER_BLOCK: [[i16; 41]; 2] = [
    [-1, 7, 10, 15, 20, 26, 18, 20, 24, 30, 18, 20, 24, 26, 30, 22, 24, 28, 30, 28, 28, 28, 28, 30, 30, 26, 28, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30, 30],
    [-1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28],
];
const NUM_BLOCKS: [[i16; 41]; 2] = [
    [-1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 4, 4, 4, 4, 4, 6, 6, 6, 6, 7, 8, 8, 9, 9, 10, 12, 12, 12, 13, 14, 15, 16, 17, 18, 19, 19, 20, 21, 22, 24, 25],
    [-1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49],
];

pub struct Qr {
    pub size: usize,
    modules: Vec<bool>,
    function: Vec<bool>,
}

fn raw_modules(ver: usize) -> usize {
    let mut r = (16 * ver + 128) * ver + 64;
    if ver >= 2 {
        let n = ver / 7 + 2;
        r -= (25 * n - 10) * n - 55;
        if ver >= 7 {
            r -= 36;
        }
    }
    r
}

fn data_codewords(ver: usize, e: Ecc) -> usize {
    let i = e as usize;
    raw_modules(ver) / 8 - ECC_PER_BLOCK[i][ver] as usize * NUM_BLOCKS[i][ver] as usize
}

fn gf_mul(x: u8, y: u8) -> u8 {
    let mut z: u16 = 0;
    for i in (0..8).rev() {
        z = (z << 1) ^ ((z >> 7) * 0x11d);
        z ^= ((y >> i) & 1) as u16 * x as u16;
    }
    z as u8
}

fn rs_divisor(degree: usize) -> Vec<u8> {
    let mut r = vec![0u8; degree];
    r[degree - 1] = 1;
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            r[j] = gf_mul(r[j], root);
            if j + 1 < degree {
                r[j] ^= r[j + 1];
            }
        }
        root = gf_mul(root, 2);
    }
    r
}

fn rs_remainder(data: &[u8], div: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; div.len()];
    for &b in data {
        let f = b ^ r.remove(0);
        r.push(0);
        for i in 0..r.len() {
            r[i] ^= gf_mul(div[i], f);
        }
    }
    r
}

impl Qr {
    /// Encode `data` choosing the smallest version that fits.
    pub fn encode(data: &[u8], e: Ecc) -> Option<Qr> {
        let mut ver = 1;
        loop {
            let cc_bits = if ver < 10 { 8 } else { 16 };
            let need = 4 + cc_bits + data.len() * 8;
            if need <= data_codewords(ver, e) * 8 {
                break;
            }
            ver += 1;
            if ver > 40 {
                return None;
            }
        }
        // bit stream
        let mut bits: Vec<bool> = Vec::new();
        let push = |v: u32, n: usize, bits: &mut Vec<bool>| {
            for i in (0..n).rev() {
                bits.push((v >> i) & 1 != 0);
            }
        };
        push(4, 4, &mut bits);
        push(data.len() as u32, if ver < 10 { 8 } else { 16 }, &mut bits);
        for &b in data {
            push(b as u32, 8, &mut bits);
        }
        let cap = data_codewords(ver, e) * 8;
        let term = (cap - bits.len()).min(4);
        push(0, term, &mut bits);
        let pad = (8 - bits.len() % 8) % 8;
        push(0, pad, &mut bits);
        let mut pb = 0xEC;
        while bits.len() < cap {
            push(pb, 8, &mut bits);
            pb ^= 0xEC ^ 0x11;
        }
        let mut cw = vec![0u8; bits.len() / 8];
        for (i, b) in bits.iter().enumerate() {
            if *b {
                cw[i >> 3] |= 1 << (7 - (i & 7));
            }
        }
        let all = Self::add_ecc(&cw, ver, e);
        let size = ver * 4 + 17;
        let mut q = Qr { size, modules: vec![false; size * size], function: vec![false; size * size] };
        q.draw_function_patterns(ver, e);
        q.draw_codewords(&all);
        let mut best = (usize::MAX, 0);
        for mask in 0..8 {
            q.apply_mask(mask);
            q.draw_format(e, mask);
            let p = q.penalty();
            if p < best.0 {
                best = (p, mask);
            }
            q.apply_mask(mask); // undo (XOR)
        }
        q.apply_mask(best.1);
        q.draw_format(e, best.1);
        Some(q)
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        self.modules[y * self.size + x]
    }

    fn set_fn(&mut self, x: usize, y: usize, dark: bool) {
        self.modules[y * self.size + x] = dark;
        self.function[y * self.size + x] = true;
    }

    fn add_ecc(data: &[u8], ver: usize, e: Ecc) -> Vec<u8> {
        let i = e as usize;
        let nb = NUM_BLOCKS[i][ver] as usize;
        let ecl = ECC_PER_BLOCK[i][ver] as usize;
        let raw = raw_modules(ver) / 8;
        let short = nb - raw % nb;
        let short_len = raw / nb;
        let div = rs_divisor(ecl);
        let mut blocks: Vec<Vec<u8>> = Vec::new();
        let mut k = 0;
        for b in 0..nb {
            let n = short_len - ecl + if b < short { 0 } else { 1 };
            let mut dat = data[k..k + n].to_vec();
            k += n;
            let ecc = rs_remainder(&dat, &div);
            if b < short {
                dat.push(0);
            }
            dat.extend_from_slice(&ecc);
            blocks.push(dat);
        }
        let mut out = Vec::new();
        for i in 0..blocks[0].len() {
            for (j, blk) in blocks.iter().enumerate() {
                if i != short_len - ecl || j >= short {
                    out.push(blk[i]);
                }
            }
        }
        out
    }

    fn draw_function_patterns(&mut self, ver: usize, e: Ecc) {
        let s = self.size;
        for i in 0..s {
            self.set_fn(6, i, i % 2 == 0);
            self.set_fn(i, 6, i % 2 == 0);
        }
        for (cx, cy) in [(3, 3), (s - 4, 3), (3, s - 4)] {
            for dy in -4i32..=4 {
                for dx in -4i32..=4 {
                    let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                    if x >= 0 && y >= 0 && (x as usize) < s && (y as usize) < s {
                        let d = dx.abs().max(dy.abs());
                        self.set_fn(x as usize, y as usize, d != 2 && d != 4);
                    }
                }
            }
        }
        if ver >= 2 {
            let n = ver / 7 + 2;
            let step = if ver == 32 { 26 } else { (ver * 4 + n * 2 + 1) / (n * 2 - 2) * 2 };
            let mut pos = vec![6usize];
            for i in 0..n - 1 {
                pos.insert(1, s - 7 - i * step);
            }
            for (i, &a) in pos.iter().enumerate() {
                for (j, &b) in pos.iter().enumerate() {
                    let corner = (i == 0 && j == 0) || (i == 0 && j == n - 1) || (i == n - 1 && j == 0);
                    if !corner {
                        for dy in -2i32..=2 {
                            for dx in -2i32..=2 {
                                let d = dx.abs().max(dy.abs());
                                self.set_fn((a as i32 + dx) as usize, (b as i32 + dy) as usize, d != 1);
                            }
                        }
                    }
                }
            }
        }
        self.draw_format(e, 0); // reserve
        if ver >= 7 {
            let mut rem = ver as u32;
            for _ in 0..12 {
                rem = (rem << 1) ^ ((rem >> 11) * 0x1f25);
            }
            let bits = (ver as u32) << 12 | rem;
            for i in 0..18 {
                let bit = (bits >> i) & 1 != 0;
                let a = s - 11 + i % 3;
                let b = i / 3;
                self.set_fn(a, b, bit);
                self.set_fn(b, a, bit);
            }
        }
    }

    fn draw_format(&mut self, e: Ecc, mask: usize) {
        let s = self.size;
        let ecl_bits = match e {
            Ecc::Low => 1,
            Ecc::Medium => 0,
        };
        let data = (ecl_bits << 3 | mask) as u32;
        let mut rem = data;
        for _ in 0..10 {
            rem = (rem << 1) ^ ((rem >> 9) * 0x537);
        }
        let bits = (data << 10 | rem) ^ 0x5412;
        let bit = |i: usize| (bits >> i) & 1 != 0;
        for i in 0..6 {
            self.set_fn(8, i, bit(i));
        }
        self.set_fn(8, 7, bit(6));
        self.set_fn(8, 8, bit(7));
        self.set_fn(7, 8, bit(8));
        for i in 9..15 {
            self.set_fn(14 - i, 8, bit(i));
        }
        for i in 0..8 {
            self.set_fn(s - 1 - i, 8, bit(i));
        }
        for i in 8..15 {
            self.set_fn(8, s - 15 + i, bit(i));
        }
        self.set_fn(8, s - 8, true);
    }

    fn draw_codewords(&mut self, data: &[u8]) {
        let s = self.size;
        let mut i = 0usize;
        let mut right = s as i32 - 1;
        while right >= 1 {
            if right == 6 {
                right = 5;
            }
            for vert in 0..s {
                for j in 0..2 {
                    let x = (right - j) as usize;
                    let upward = (right + 1) & 2 == 0;
                    let y = if upward { s - 1 - vert } else { vert };
                    if !self.function[y * s + x] && i < data.len() * 8 {
                        self.modules[y * s + x] = (data[i >> 3] >> (7 - (i & 7))) & 1 != 0;
                        i += 1;
                    }
                }
            }
            right -= 2;
        }
    }

    fn apply_mask(&mut self, mask: usize) {
        let s = self.size;
        for y in 0..s {
            for x in 0..s {
                let inv = match mask {
                    0 => (x + y) % 2 == 0,
                    1 => y % 2 == 0,
                    2 => x % 3 == 0,
                    3 => (x + y) % 3 == 0,
                    4 => (x / 3 + y / 2) % 2 == 0,
                    5 => x * y % 2 + x * y % 3 == 0,
                    6 => (x * y % 2 + x * y % 3) % 2 == 0,
                    _ => ((x + y) % 2 + x * y % 3) % 2 == 0,
                };
                if inv && !self.function[y * s + x] {
                    self.modules[y * s + x] ^= true;
                }
            }
        }
    }

    fn penalty(&self) -> usize {
        let s = self.size;
        let g = |x: usize, y: usize| self.modules[y * s + x];
        let mut p = 0;
        // rule 1 (runs) and rule 3 (finder-like 1011101 patterns) in both directions
        for horiz in [true, false] {
            for a in 0..s {
                let mut run = 0;
                let mut prev = false;
                let mut line = Vec::with_capacity(s);
                for b in 0..s {
                    let v = if horiz { g(b, a) } else { g(a, b) };
                    line.push(v);
                    if b > 0 && v == prev {
                        run += 1;
                    } else {
                        if run >= 5 {
                            p += 3 + run - 5;
                        }
                        run = 1;
                    }
                    prev = v;
                }
                if run >= 5 {
                    p += 3 + run - 5;
                }
                let pat = [true, false, true, true, true, false, true];
                for b in 0..s.saturating_sub(6) {
                    if (0..7).all(|k| line[b + k] == pat[k]) {
                        let light_before = b >= 4 && (b - 4..b).all(|k| !line[k]);
                        let light_after = b + 11 <= s && (b + 7..b + 11).all(|k| !line[k]);
                        if light_before || light_after {
                            p += 40;
                        }
                    }
                }
            }
        }
        // rule 2: 2x2 blocks
        for y in 0..s - 1 {
            for x in 0..s - 1 {
                let c = g(x, y);
                if c == g(x + 1, y) && c == g(x, y + 1) && c == g(x + 1, y + 1) {
                    p += 3;
                }
            }
        }
        // rule 4: dark balance
        let dark = self.modules.iter().filter(|m| **m).count();
        let total = s * s;
        let k = ((dark * 20).abs_diff(total * 10) + total - 1) / total;
        p + k.saturating_sub(1) * 10
    }
}
