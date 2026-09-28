//! SHA-384 and SHA-512 (FIPS 180-4), and the hash choice TLS needs: one of
//! SHA-256, SHA-384 or SHA-512, with HMAC and HKDF on top.

use crate::crypto::Sha256;
use alloc::vec;
use alloc::vec::Vec;

const K512: [u64; 80] = [
    0x428a2f98d728ae22, 0x7137449123ef65cd, 0xb5c0fbcfec4d3b2f, 0xe9b5dba58189dbbc,
    0x3956c25bf348b538, 0x59f111f1b605d019, 0x923f82a4af194f9b, 0xab1c5ed5da6d8118,
    0xd807aa98a3030242, 0x12835b0145706fbe, 0x243185be4ee4b28c, 0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f, 0x80deb1fe3b1696b1, 0x9bdc06a725c71235, 0xc19bf174cf692694,
    0xe49b69c19ef14ad2, 0xefbe4786384f25e3, 0x0fc19dc68b8cd5b5, 0x240ca1cc77ac9c65,
    0x2de92c6f592b0275, 0x4a7484aa6ea6e483, 0x5cb0a9dcbd41fbd4, 0x76f988da831153b5,
    0x983e5152ee66dfab, 0xa831c66d2db43210, 0xb00327c898fb213f, 0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2, 0xd5a79147930aa725, 0x06ca6351e003826f, 0x142929670a0e6e70,
    0x27b70a8546d22ffc, 0x2e1b21385c26c926, 0x4d2c6dfc5ac42aed, 0x53380d139d95b3df,
    0x650a73548baf63de, 0x766a0abb3c77b2a8, 0x81c2c92e47edaee6, 0x92722c851482353b,
    0xa2bfe8a14cf10364, 0xa81a664bbc423001, 0xc24b8b70d0f89791, 0xc76c51a30654be30,
    0xd192e819d6ef5218, 0xd69906245565a910, 0xf40e35855771202a, 0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8, 0x1e376c085141ab53, 0x2748774cdf8eeb99, 0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63, 0x4ed8aa4ae3418acb, 0x5b9cca4f7763e373, 0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc, 0x78a5636f43172f60, 0x84c87814a1f0ab72, 0x8cc702081a6439ec,
    0x90befffa23631e28, 0xa4506cebde82bde9, 0xbef9a3f7b2c67915, 0xc67178f2e372532b,
    0xca273eceea26619c, 0xd186b8c721c0c207, 0xeada7dd6cde0eb1e, 0xf57d4f7fee6ed178,
    0x06f067aa72176fba, 0x0a637dc5a2c898a6, 0x113f9804bef90dae, 0x1b710b35131c471b,
    0x28db77f523047d84, 0x32caab7b40c72493, 0x3c9ebe0a15c9bebc, 0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6, 0x597f299cfc657e2a, 0x5fcb6fab3ad6faec, 0x6c44198c4a475817
];

#[derive(Clone)]
pub struct Sha512 {
    h: [u64; 8],
    buf: [u8; 128],
    n: usize,
    len: u128,
    out: usize,
}

impl Sha512 {
    pub fn new512() -> Sha512 {
        Sha512::with(
            [0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1, 0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179],
            64,
        )
    }

    pub fn new384() -> Sha512 {
        Sha512::with(
            [0xcbbb9d5dc1059ed8, 0x629a292a367cd507, 0x9159015a3070dd17, 0x152fecd8f70e5939, 0x67332667ffc00b31, 0x8eb44a8768581511, 0xdb0c2e0d64f98fa7, 0x47b5481dbefa4fa4],
            48,
        )
    }

    fn with(h: [u64; 8], out: usize) -> Sha512 {
        Sha512 { h, buf: [0; 128], n: 0, len: 0, out }
    }

    fn block(&mut self) {
        let mut w = [0u64; 80];
        for i in 0..16 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&self.buf[8 * i..8 * i + 8]);
            w[i] = u64::from_be_bytes(b);
        }
        for i in 16..80 {
            let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
            let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = self.h;
        for i in 0..80 {
            let s1 = v[4].rotate_right(14) ^ v[4].rotate_right(18) ^ v[4].rotate_right(41);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K512[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(28) ^ v[0].rotate_right(34) ^ v[0].rotate_right(39);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            self.h[i] = self.h[i].wrapping_add(v[i]);
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len += data.len() as u128;
        while !data.is_empty() {
            let n = (128 - self.n).min(data.len());
            self.buf[self.n..self.n + n].copy_from_slice(&data[..n]);
            self.n += n;
            data = &data[n..];
            if self.n == 128 {
                self.block();
                self.n = 0;
            }
        }
    }

    pub fn finish(mut self) -> Vec<u8> {
        let bits = self.len * 8;
        self.update(&[0x80]);
        while self.n != 112 {
            self.update(&[0]);
        }
        self.update(&bits.to_be_bytes());
        let mut out = Vec::with_capacity(64);
        for h in self.h {
            out.extend_from_slice(&h.to_be_bytes());
        }
        out.truncate(self.out);
        out
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hash {
    Sha256,
    Sha384,
    Sha512,
}

#[derive(Clone)]
pub enum Hasher {
    S256(Sha256),
    S512(Sha512),
}

impl Hasher {
    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::S256(h) => h.update(data),
            Hasher::S512(h) => h.update(data),
        }
    }

    pub fn finish(self) -> Vec<u8> {
        match self {
            Hasher::S256(h) => h.finish().to_vec(),
            Hasher::S512(h) => h.finish(),
        }
    }
}

impl Hash {
    pub fn len(self) -> usize {
        match self {
            Hash::Sha256 => 32,
            Hash::Sha384 => 48,
            Hash::Sha512 => 64,
        }
    }

    fn block(self) -> usize {
        match self {
            Hash::Sha256 => 64,
            _ => 128,
        }
    }

    pub fn start(self) -> Hasher {
        match self {
            Hash::Sha256 => Hasher::S256(Sha256::new()),
            Hash::Sha384 => Hasher::S512(Sha512::new384()),
            Hash::Sha512 => Hasher::S512(Sha512::new512()),
        }
    }

    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        let mut h = self.start();
        h.update(data);
        h.finish()
    }

    pub fn hmac(self, key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
        let mut k = vec![0u8; self.block()];
        if key.len() > k.len() {
            let d = self.digest(key);
            k[..d.len()].copy_from_slice(&d);
        } else {
            k[..key.len()].copy_from_slice(key);
        }
        let mut inner = self.start();
        let mut outer = self.start();
        let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
        let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
        inner.update(&ipad);
        for p in parts {
            inner.update(p);
        }
        outer.update(&opad);
        outer.update(&inner.finish());
        outer.finish()
    }

    /// HKDF-Extract (RFC 5869).
    pub fn extract(self, salt: &[u8], ikm: &[u8]) -> Vec<u8> {
        self.hmac(salt, &[ikm])
    }

    /// HKDF-Expand (RFC 5869).
    pub fn expand(self, prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(len);
        let mut t: Vec<u8> = Vec::new();
        let mut ctr = 1u8;
        while out.len() < len {
            t = self.hmac(prk, &[&t, info, &[ctr]]);
            let n = (len - out.len()).min(t.len());
            out.extend_from_slice(&t[..n]);
            ctr += 1;
        }
        out
    }
}
