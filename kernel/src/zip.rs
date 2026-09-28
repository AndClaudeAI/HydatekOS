//! Zip archives, enough for office documents (.docx is a zip of XML parts).
//!
//! Writing compresses entries with a small deflate encoder (fixed Huffman
//! codes and LZ77 matching), or stores them when that isn't smaller. Reading
//! handles stored and deflated (method 8) entries, with a small inflate
//! implementation (RFC 1951).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---- CRC-32 (IEEE) ----------------------------------------------------------

pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = !0u32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    !crc
}

// ---- writing ----------------------------------------------------------------

pub struct Writer {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

fn le16(v: &mut Vec<u8>, x: u16) {
    v.extend_from_slice(&x.to_le_bytes());
}
fn le32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_le_bytes());
}

impl Writer {
    pub fn new() -> Writer {
        Writer { out: Vec::new(), central: Vec::new(), count: 0 }
    }

    /// Add an entry (deflated when that makes it smaller).
    pub fn add(&mut self, name: &str, data: &[u8]) {
        let crc = crc32(data);
        let packed = deflate(data);
        let (method, body): (u16, &[u8]) = if packed.len() < data.len() { (8, &packed) } else { (0, data) };
        let off = self.out.len() as u32;
        // DOS time/date: 2026-01-01 00:00
        let (time, date) = (0u16, ((2026 - 1980) << 9 | 1 << 5 | 1) as u16);
        let o = &mut self.out;
        le32(o, 0x0403_4b50);
        le16(o, 20); // version needed
        le16(o, 0x0800); // flags: UTF-8 names
        le16(o, method);
        le16(o, time);
        le16(o, date);
        le32(o, crc);
        le32(o, body.len() as u32);
        le32(o, data.len() as u32);
        le16(o, name.len() as u16);
        le16(o, 0);
        o.extend_from_slice(name.as_bytes());
        o.extend_from_slice(body);
        let c = &mut self.central;
        le32(c, 0x0201_4b50);
        le16(c, 20); // made by
        le16(c, 20);
        le16(c, 0x0800);
        le16(c, method);
        le16(c, time);
        le16(c, date);
        le32(c, crc);
        le32(c, body.len() as u32);
        le32(c, data.len() as u32);
        le16(c, name.len() as u16);
        le16(c, 0); // extra
        le16(c, 0); // comment
        le16(c, 0); // disk
        le16(c, 0); // internal attrs
        le32(c, 0); // external attrs
        le32(c, off);
        c.extend_from_slice(name.as_bytes());
        self.count += 1;
    }

    pub fn finish(mut self) -> Vec<u8> {
        let cd_off = self.out.len() as u32;
        let cd_len = self.central.len() as u32;
        self.out.extend_from_slice(&self.central);
        let o = &mut self.out;
        le32(o, 0x0605_4b50);
        le16(o, 0);
        le16(o, 0);
        le16(o, self.count);
        le16(o, self.count);
        le32(o, cd_len);
        le32(o, cd_off);
        le16(o, 0);
        self.out
    }
}

// ---- deflate ----------------------------------------------------------------

struct BitOut {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl BitOut {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= (v as u64) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// A Huffman code (sent most significant bit first).
    fn code(&mut self, c: u32, bits: u32) {
        let mut r = 0;
        for i in 0..bits {
            r |= ((c >> i) & 1) << (bits - 1 - i);
        }
        self.put(r, bits);
    }
    fn lit(&mut self, v: u32) {
        match v {
            0..=143 => self.code(0x30 + v, 8),
            144..=255 => self.code(0x190 + v - 144, 9),
            256..=279 => self.code(v - 256, 7),
            _ => self.code(0xC0 + v - 280, 8),
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// Compress to a raw deflate stream (RFC 1951): one block of fixed Huffman
/// codes, with matches found through hash chains over a 32 KB window.
pub fn deflate(data: &[u8]) -> Vec<u8> {
    const WIN: usize = 32768;
    const HBITS: u32 = 15;
    const CHAIN: usize = 48;
    let mut b = BitOut { out: Vec::with_capacity(data.len() / 2 + 16), acc: 0, n: 0 };
    b.put(1, 1); // final block
    b.put(1, 2); // fixed Huffman codes
    let n = data.len();
    let mut head = vec![u32::MAX; 1 << HBITS];
    let mut prev = vec![u32::MAX; WIN];
    let hash = |i: usize| -> usize { ((data[i] as u32) << 10 ^ (data[i + 1] as u32) << 5 ^ data[i + 2] as u32).wrapping_mul(2654435761) as usize >> (32 - HBITS) & ((1 << HBITS) - 1) };
    let insert = |head: &mut Vec<u32>, prev: &mut Vec<u32>, i: usize| {
        if i + 2 < n {
            let h = hash(i);
            prev[i % WIN] = head[h];
            head[h] = i as u32;
        }
    };
    let mut i = 0;
    while i < n {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + 2 < n {
            let mut cand = head[hash(i)];
            let mut steps = 0;
            let max = (n - i).min(258);
            while cand != u32::MAX && steps < CHAIN {
                let c = cand as usize;
                if i - c > WIN - 1 || c >= i {
                    break;
                }
                if data[c + best_len.min(max - 1)] == data[i + best_len.min(max - 1)] {
                    let mut l = 0;
                    while l < max && data[c + l] == data[i + l] {
                        l += 1;
                    }
                    if l > best_len {
                        best_len = l;
                        best_dist = i - c;
                        if l == max {
                            break;
                        }
                    }
                }
                cand = prev[c % WIN];
                steps += 1;
            }
        }
        if best_len >= 3 {
            let li = LBASE.iter().rposition(|&x| x as usize <= best_len).unwrap();
            b.lit(257 + li as u32);
            b.put((best_len - LBASE[li] as usize) as u32, LEXT[li] as u32);
            let di = DBASE.iter().rposition(|&x| x as usize <= best_dist).unwrap();
            b.code(di as u32, 5);
            b.put((best_dist - DBASE[di] as usize) as u32, DEXT[di] as u32);
            for k in i..i + best_len {
                insert(&mut head, &mut prev, k);
            }
            i += best_len;
        } else {
            b.lit(data[i] as u32);
            insert(&mut head, &mut prev, i);
            i += 1;
        }
    }
    b.lit(256);
    b.finish()
}

/// Adler-32, the zlib checksum.
pub fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// A zlib stream (RFC 1950): header, deflate data, Adler-32.
pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01];
    z.extend_from_slice(&deflate(data));
    z.extend_from_slice(&adler32(data).to_be_bytes());
    z
}

// ---- reading ----------------------------------------------------------------

fn rd16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}
fn rd32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

pub struct Entry {
    pub name: String,
    method: u16,
    csize: usize,
    usize_: usize,
    local: usize,
}

/// List the entries of a zip archive (from its central directory).
pub fn entries(z: &[u8]) -> Option<Vec<Entry>> {
    // end of central directory: last 22..(22+65535) bytes
    let min = z.len().saturating_sub(22 + 65535);
    let mut eocd = None;
    let mut i = z.len().checked_sub(22)?;
    loop {
        if rd32(z, i)? == 0x0605_4b50 {
            eocd = Some(i);
            break;
        }
        if i == min {
            break;
        }
        i -= 1;
    }
    let e = eocd?;
    let n = rd16(z, e + 10)? as usize;
    let mut p = rd32(z, e + 16)? as usize;
    let mut out = Vec::new();
    for _ in 0..n {
        if rd32(z, p)? != 0x0201_4b50 {
            return None;
        }
        let method = rd16(z, p + 10)?;
        let csize = rd32(z, p + 20)? as usize;
        let usize_ = rd32(z, p + 24)? as usize;
        let nl = rd16(z, p + 28)? as usize;
        let xl = rd16(z, p + 30)? as usize;
        let cl = rd16(z, p + 32)? as usize;
        let local = rd32(z, p + 42)? as usize;
        let name = String::from_utf8_lossy(z.get(p + 46..p + 46 + nl)?).into_owned();
        out.push(Entry { name, method, csize, usize_, local });
        p += 46 + nl + xl + cl;
    }
    Some(out)
}

/// Read one entry's contents by name.
pub fn read(z: &[u8], name: &str) -> Option<Vec<u8>> {
    let es = entries(z)?;
    let e = es.iter().find(|e| e.name == name)?;
    if rd32(z, e.local)? != 0x0403_4b50 {
        return None;
    }
    let nl = rd16(z, e.local + 26)? as usize;
    let xl = rd16(z, e.local + 28)? as usize;
    let start = e.local + 30 + nl + xl;
    let data = z.get(start..start + e.csize)?;
    match e.method {
        0 => Some(data.to_vec()),
        8 => inflate(data, e.usize_),
        _ => None,
    }
}

// ---- inflate (RFC 1951) -------------------------------------------------------

struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u32,
    n: u32,
}

impl<'a> Bits<'a> {
    fn need(&mut self, k: u32) -> Option<()> {
        while self.n < k {
            let b = *self.d.get(self.pos)?;
            self.pos += 1;
            self.bit |= (b as u32) << self.n;
            self.n += 8;
        }
        Some(())
    }
    fn get(&mut self, k: u32) -> Option<u32> {
        if k == 0 {
            return Some(0);
        }
        self.need(k)?;
        let v = self.bit & ((1u32 << k) - 1);
        self.bit >>= k;
        self.n -= k;
        Some(v)
    }
    fn align(&mut self) {
        let r = self.n % 8;
        self.bit >>= r;
        self.n -= r;
    }
}

/// Canonical Huffman decoding table: counts per length and symbols in order.
struct Huff {
    count: [u16; 16],
    sym: Vec<u16>,
}

impl Huff {
    fn new(lengths: &[u8]) -> Huff {
        let mut count = [0u16; 16];
        for &l in lengths {
            count[l as usize] += 1;
        }
        count[0] = 0;
        let mut offs = [0u16; 16];
        for i in 1..16 {
            offs[i] = offs[i - 1] + count[i - 1];
        }
        let mut sym = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                sym[offs[l as usize] as usize] = s as u16;
                offs[l as usize] += 1;
            }
        }
        Huff { count, sym }
    }

    fn decode(&self, b: &mut Bits) -> Option<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= b.get(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return self.sym.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Decompress a raw deflate stream. `hint` presizes the output.
pub fn inflate(data: &[u8], hint: usize) -> Option<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(hint.min(64 << 20));
    let mut b = Bits { d: data, pos: 0, bit: 0, n: 0 };
    loop {
        let last = b.get(1)?;
        match b.get(2)? {
            0 => {
                b.align();
                let len = b.get(16)? as usize;
                let _nlen = b.get(16)?;
                // the bit buffer is empty after align + 32 bits
                let s = b.d.get(b.pos..b.pos + len)?;
                out.extend_from_slice(s);
                b.pos += len;
            }
            t @ (1 | 2) => {
                let (lit, dist) = if t == 1 {
                    let mut l = [0u8; 288];
                    for (i, v) in l.iter_mut().enumerate() {
                        *v = match i {
                            0..=143 => 8,
                            144..=255 => 9,
                            256..=279 => 7,
                            _ => 8,
                        };
                    }
                    (Huff::new(&l), Huff::new(&[5u8; 30]))
                } else {
                    let hlit = b.get(5)? as usize + 257;
                    let hdist = b.get(5)? as usize + 1;
                    let hclen = b.get(4)? as usize + 4;
                    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                    let mut cl = [0u8; 19];
                    for &o in ORDER.iter().take(hclen) {
                        cl[o] = b.get(3)? as u8;
                    }
                    let ch = Huff::new(&cl);
                    let mut lens = vec![0u8; hlit + hdist];
                    let mut i = 0;
                    while i < lens.len() {
                        let s = ch.decode(&mut b)?;
                        match s {
                            0..=15 => {
                                lens[i] = s as u8;
                                i += 1;
                            }
                            16 => {
                                let prev = *lens.get(i.checked_sub(1)?)?;
                                for _ in 0..3 + b.get(2)? {
                                    *lens.get_mut(i)? = prev;
                                    i += 1;
                                }
                            }
                            17 => i += 3 + b.get(3)? as usize,
                            _ => i += 11 + b.get(7)? as usize,
                        }
                    }
                    if i > lens.len() {
                        return None;
                    }
                    (Huff::new(&lens[..hlit]), Huff::new(&lens[hlit..]))
                };
                loop {
                    let s = lit.decode(&mut b)? as usize;
                    if s < 256 {
                        out.push(s as u8);
                    } else if s == 256 {
                        break;
                    } else {
                        let k = s - 257;
                        let len = *LBASE.get(k)? as usize + b.get(*LEXT.get(k)? as u32)? as usize;
                        let d = dist.decode(&mut b)? as usize;
                        let back = *DBASE.get(d)? as usize + b.get(*DEXT.get(d)? as u32)? as usize;
                        let start = out.len().checked_sub(back)?;
                        for j in 0..len {
                            let v = out[start + j];
                            out.push(v);
                        }
                    }
                }
            }
            _ => return None,
        }
        if last == 1 {
            break;
        }
    }
    Some(out)
}
