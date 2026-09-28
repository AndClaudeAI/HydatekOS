//! Unsigned big integers as little-endian 64-bit limbs, and Montgomery
//! arithmetic modulo an odd number (for RSA and the elliptic curves).

use alloc::vec;
use alloc::vec::Vec;
use core::cmp::Ordering;

pub type Big = Vec<u64>;

pub fn from_be(bytes: &[u8], limbs: usize) -> Big {
    let mut out = vec![0u64; limbs];
    for (i, &b) in bytes.iter().rev().enumerate() {
        if i / 8 < limbs {
            out[i / 8] |= (b as u64) << (8 * (i % 8));
        } else if b != 0 {
            // too big: callers check with cmp against their modulus
            out.iter_mut().for_each(|l| *l = u64::MAX);
            return out;
        }
    }
    out
}

pub fn to_be(a: &[u64], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    for i in 0..len {
        let limb = i / 8;
        if limb < a.len() {
            out[len - 1 - i] = (a[limb] >> (8 * (i % 8))) as u8;
        }
    }
    out
}

pub fn cmp(a: &[u64], b: &[u64]) -> Ordering {
    for i in (0..a.len().max(b.len())).rev() {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    Ordering::Equal
}

pub fn is_zero(a: &[u64]) -> bool {
    a.iter().all(|&l| l == 0)
}

/// a -= b, returning the borrow.
pub fn sub_in(a: &mut [u64], b: &[u64]) -> u64 {
    let mut borrow = 0u64;
    for i in 0..a.len() {
        let (d1, b1) = a[i].overflowing_sub(b.get(i).copied().unwrap_or(0));
        let (d2, b2) = d1.overflowing_sub(borrow);
        a[i] = d2;
        borrow = (b1 | b2) as u64;
    }
    borrow
}

/// a += b, returning the carry.
pub fn add_in(a: &mut [u64], b: &[u64]) -> u64 {
    let mut carry = 0u64;
    for i in 0..a.len() {
        let (s1, c1) = a[i].overflowing_add(b.get(i).copied().unwrap_or(0));
        let (s2, c2) = s1.overflowing_add(carry);
        a[i] = s2;
        carry = (c1 | c2) as u64;
    }
    carry
}

pub fn bits(a: &[u64]) -> usize {
    for i in (0..a.len()).rev() {
        if a[i] != 0 {
            return 64 * i + 64 - a[i].leading_zeros() as usize;
        }
    }
    0
}

/// Arithmetic modulo an odd `n`.
#[derive(Clone)]
pub struct Mont {
    pub n: Big,
    n0: u64,
    r2: Big,
}

impl Mont {
    pub fn new(n: Big) -> Mont {
        let len = n.len();
        let mut inv = 1u64;
        for _ in 0..7 {
            inv = inv.wrapping_mul(2u64.wrapping_sub(n[0].wrapping_mul(inv)));
        }
        // R^2 mod n by doubling 1 (2 * 64 * len) times
        let mut r: Big = vec![0; len];
        r[0] = 1;
        for _ in 0..128 * len {
            let top = r[len - 1] >> 63;
            for i in (1..len).rev() {
                r[i] = (r[i] << 1) | (r[i - 1] >> 63);
            }
            r[0] <<= 1;
            if top == 1 || cmp(&r, &n) != Ordering::Less {
                sub_in(&mut r, &n);
            }
        }
        Mont { n0: inv.wrapping_neg(), r2: r, n }
    }

    pub fn len(&self) -> usize {
        self.n.len()
    }

    /// a * b / R mod n (inputs below n).
    pub fn mul(&self, a: &[u64], b: &[u64]) -> Big {
        let len = self.n.len();
        let mut t = vec![0u64; len + 2];
        for i in 0..len {
            let mut c: u128 = 0;
            for j in 0..len {
                let v = t[j] as u128 + a[j] as u128 * b[i] as u128 + c;
                t[j] = v as u64;
                c = v >> 64;
            }
            let v = t[len] as u128 + c;
            t[len] = v as u64;
            t[len + 1] = (v >> 64) as u64;
            let m = t[0].wrapping_mul(self.n0);
            let v = t[0] as u128 + m as u128 * self.n[0] as u128;
            let mut c = v >> 64;
            for j in 1..len {
                let v = t[j] as u128 + m as u128 * self.n[j] as u128 + c;
                t[j - 1] = v as u64;
                c = v >> 64;
            }
            let v = t[len] as u128 + c;
            t[len - 1] = v as u64;
            t[len] = t[len + 1] + (v >> 64) as u64;
        }
        let mut r: Big = t[..len].to_vec();
        if t[len] != 0 || cmp(&r, &self.n) != Ordering::Less {
            sub_in(&mut r, &self.n);
        }
        r
    }

    pub fn to_mont(&self, a: &[u64]) -> Big {
        self.mul(a, &self.r2)
    }

    pub fn from_mont(&self, a: &[u64]) -> Big {
        let mut one = vec![0u64; self.n.len()];
        one[0] = 1;
        self.mul(a, &one)
    }

    pub fn one(&self) -> Big {
        let mut one = vec![0u64; self.n.len()];
        one[0] = 1;
        self.to_mont(&one)
    }

    /// Plain (non-Montgomery) a * b mod n.
    pub fn mulmod(&self, a: &[u64], b: &[u64]) -> Big {
        self.mul(&self.mul(a, b), &self.r2)
    }

    /// base^exp mod n on Montgomery-form base; exp as big-endian bytes.
    pub fn pow_m(&self, base: &[u64], exp: &[u8]) -> Big {
        let mut r = self.one();
        for &byte in exp {
            for bit in (0..8).rev() {
                r = self.mul(&r, &r);
                if (byte >> bit) & 1 == 1 {
                    r = self.mul(&r, base);
                }
            }
        }
        r
    }

    /// Plain base^exp mod n.
    pub fn pow(&self, base: &[u64], exp: &[u8]) -> Big {
        self.from_mont(&self.pow_m(&self.to_mont(base), exp))
    }

    /// Inverse of a (plain) modulo a prime n, by Fermat.
    pub fn inv_prime(&self, a: &[u64]) -> Big {
        let mut e = self.n.clone();
        let two = [2u64];
        sub_in(&mut e, &two);
        self.pow(a, &to_be(&e, 8 * self.n.len()))
    }

    pub fn addm(&self, a: &[u64], b: &[u64]) -> Big {
        let mut r = a.to_vec();
        let c = add_in(&mut r, b);
        if c != 0 || cmp(&r, &self.n) != Ordering::Less {
            sub_in(&mut r, &self.n);
        }
        r
    }

    pub fn subm(&self, a: &[u64], b: &[u64]) -> Big {
        let mut r = a.to_vec();
        if sub_in(&mut r, b) != 0 {
            add_in(&mut r, &self.n);
        }
        r
    }
}
