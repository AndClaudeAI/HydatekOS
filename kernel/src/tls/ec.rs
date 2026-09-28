//! NIST P-256 and P-384 (y² = x³ − 3x + b): ECDSA signature checks and ECDH.
//! Points use Jacobian coordinates with Montgomery-form field elements.

use super::bignum::{self, Big, Mont};
use alloc::vec::Vec;
use core::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Curve {
    P256,
    P384,
}

struct Params {
    p: &'static str,
    n: &'static str,
    b: &'static str,
    gx: &'static str,
    gy: &'static str,
}

const P256: Params = Params {
    p: "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff",
    n: "ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551",
    b: "5ac635d8aa3a93e7b3ebbd55769886bc651d06b0cc53b0f63bce3c3e27d2604b",
    gx: "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
    gy: "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
};

const P384: Params = Params {
    p: "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeffffffff0000000000000000ffffffff",
    n: "ffffffffffffffffffffffffffffffffffffffffffffffffc7634d81f4372ddf581a0db248b0a77aecec196accc52973",
    b: "b3312fa7e23ee7e4988e056be3f82d19181d9c6efe8141120314088f5013875ac656398d8a2ed19d2a85c8edd3ec2aef",
    gx: "aa87ca22be8b05378eb1c71ef320ad746e1d3b628ba79b9859f741e082542a385502f25dbf55296c3a545e3872760ab7",
    gy: "3617de4a96262c6f5d9e98bf9292dc29f8f41dbd289a147ce9da3113b5f0b8c00a60b1ce1d7e819d7a431d7c90ea0e5f",
};

fn hex_big(s: &str, limbs: usize) -> Big {
    let bytes: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0)).collect();
    bignum::from_be(&bytes, limbs)
}

#[derive(Clone)]
struct Point {
    x: Big,
    y: Big,
    z: Big,
    inf: bool,
}

pub struct Group {
    f: Mont,
    pub order: Mont,
    b: Big,
    g: Point,
    pub size: usize,
}

impl Group {
    pub fn new(curve: Curve) -> Group {
        let (pr, size) = match curve {
            Curve::P256 => (&P256, 32),
            Curve::P384 => (&P384, 48),
        };
        let limbs = size / 8;
        let f = Mont::new(hex_big(pr.p, limbs));
        let order = Mont::new(hex_big(pr.n, limbs));
        let b = f.to_mont(&hex_big(pr.b, limbs));
        let g = Point { x: f.to_mont(&hex_big(pr.gx, limbs)), y: f.to_mont(&hex_big(pr.gy, limbs)), z: f.one(), inf: false };
        Group { f, order, b, g, size }
    }

    fn infinity(&self) -> Point {
        let z = alloc::vec![0u64; self.f.len()];
        Point { x: z.clone(), y: z.clone(), z, inf: true }
    }

    fn double(&self, p: &Point) -> Point {
        let f = &self.f;
        if p.inf || bignum::is_zero(&p.y) {
            return self.infinity();
        }
        let delta = f.mul(&p.z, &p.z);
        let gamma = f.mul(&p.y, &p.y);
        let beta = f.mul(&p.x, &gamma);
        let t = f.mul(&f.subm(&p.x, &delta), &f.addm(&p.x, &delta));
        let alpha = f.addm(&f.addm(&t, &t), &t);
        let beta4 = f.addm(&f.addm(&beta, &beta), &f.addm(&beta, &beta));
        let beta8 = f.addm(&beta4, &beta4);
        let x3 = f.subm(&f.mul(&alpha, &alpha), &beta8);
        let yz = f.addm(&p.y, &p.z);
        let z3 = f.subm(&f.subm(&f.mul(&yz, &yz), &gamma), &delta);
        let g2 = f.mul(&gamma, &gamma);
        let g4 = f.addm(&f.addm(&g2, &g2), &f.addm(&g2, &g2));
        let g8 = f.addm(&g4, &g4);
        let y3 = f.subm(&f.mul(&alpha, &f.subm(&beta4, &x3)), &g8);
        Point { x: x3, y: y3, z: z3, inf: false }
    }

    fn add(&self, p: &Point, q: &Point) -> Point {
        if p.inf {
            return q.clone();
        }
        if q.inf {
            return p.clone();
        }
        let f = &self.f;
        let z1z1 = f.mul(&p.z, &p.z);
        let z2z2 = f.mul(&q.z, &q.z);
        let u1 = f.mul(&p.x, &z2z2);
        let u2 = f.mul(&q.x, &z1z1);
        let s1 = f.mul(&f.mul(&p.y, &q.z), &z2z2);
        let s2 = f.mul(&f.mul(&q.y, &p.z), &z1z1);
        let h = f.subm(&u2, &u1);
        let sd = f.subm(&s2, &s1);
        if bignum::is_zero(&h) {
            return if bignum::is_zero(&sd) { self.double(p) } else { self.infinity() };
        }
        let r = f.addm(&sd, &sd);
        let h2 = f.addm(&h, &h);
        let i = f.mul(&h2, &h2);
        let j = f.mul(&h, &i);
        let v = f.mul(&u1, &i);
        let x3 = f.subm(&f.subm(&f.mul(&r, &r), &j), &f.addm(&v, &v));
        let s1j = f.mul(&s1, &j);
        let y3 = f.subm(&f.mul(&r, &f.subm(&v, &x3)), &f.addm(&s1j, &s1j));
        let zz = f.addm(&p.z, &q.z);
        let z3 = f.mul(&f.subm(&f.subm(&f.mul(&zz, &zz), &z1z1), &z2z2), &h);
        Point { x: x3, y: y3, z: z3, inf: false }
    }

    /// k·P, k as big-endian bytes. A ladder: the same work for every bit.
    fn mul(&self, p: &Point, k: &[u8]) -> Point {
        let mut r0 = self.infinity();
        let mut r1 = p.clone();
        for &byte in k {
            for bit in (0..8).rev() {
                if (byte >> bit) & 1 == 1 {
                    r0 = self.add(&r0, &r1);
                    r1 = self.double(&r1);
                } else {
                    r1 = self.add(&r0, &r1);
                    r0 = self.double(&r0);
                }
            }
        }
        r0
    }

    /// Affine x and y (plain form).
    fn affine(&self, p: &Point) -> Option<(Big, Big)> {
        if p.inf {
            return None;
        }
        let f = &self.f;
        let zi = f.to_mont(&f.inv_prime(&f.from_mont(&p.z)));
        let zi2 = f.mul(&zi, &zi);
        let x = f.from_mont(&f.mul(&p.x, &zi2));
        let y = f.from_mont(&f.mul(&p.y, &f.mul(&zi2, &zi)));
        Some((x, y))
    }

    /// Decode an uncompressed point (04 || x || y) and check it's on the curve.
    fn decode(&self, data: &[u8]) -> Option<Point> {
        let s = self.size;
        if data.len() != 1 + 2 * s || data[0] != 4 {
            return None;
        }
        let limbs = s / 8;
        let x = bignum::from_be(&data[1..1 + s], limbs);
        let y = bignum::from_be(&data[1 + s..], limbs);
        let f = &self.f;
        if bignum::cmp(&x, &f.n) != Ordering::Less || bignum::cmp(&y, &f.n) != Ordering::Less {
            return None;
        }
        let (xm, ym) = (f.to_mont(&x), f.to_mont(&y));
        let lhs = f.mul(&ym, &ym);
        let x3 = f.mul(&f.mul(&xm, &xm), &xm);
        let x3m = f.addm(&f.addm(&xm, &xm), &xm);
        let rhs = f.addm(&f.subm(&x3, &x3m), &self.b);
        if lhs != rhs {
            return None;
        }
        Some(Point { x: xm, y: ym, z: f.one(), inf: false })
    }

    fn encode(&self, x: &[u64], y: &[u64]) -> Vec<u8> {
        let mut out = alloc::vec![4u8];
        out.extend(bignum::to_be(x, self.size));
        out.extend(bignum::to_be(y, self.size));
        out
    }

    /// The leftmost bits of a hash as a number below the order.
    fn hash_int(&self, hash: &[u8]) -> Big {
        let h = &hash[..hash.len().min(self.size)];
        let mut e = bignum::from_be(h, self.size / 8);
        if bignum::cmp(&e, &self.order.n) != Ordering::Less {
            bignum::sub_in(&mut e, &self.order.n);
        }
        e
    }

    /// Check an ECDSA signature (r, s as big-endian bytes) over `hash`.
    pub fn verify(&self, public: &[u8], hash: &[u8], r: &[u8], s: &[u8]) -> bool {
        let Some(q) = self.decode(public) else { return false };
        let limbs = self.size / 8;
        let strip = |v: &[u8]| -> Option<Big> {
            let v: &[u8] = &v[v.iter().position(|&b| b != 0).unwrap_or(v.len())..];
            if v.len() > self.size {
                return None;
            }
            Some(bignum::from_be(v, limbs))
        };
        let (Some(r), Some(s)) = (strip(r), strip(s)) else { return false };
        let n = &self.order;
        for v in [&r, &s] {
            if bignum::is_zero(v) || bignum::cmp(v, &n.n) != Ordering::Less {
                return false;
            }
        }
        let e = self.hash_int(hash);
        let w = n.inv_prime(&s);
        let u1 = n.mulmod(&e, &w);
        let u2 = n.mulmod(&r, &w);
        let pt = self.add(&self.mul(&self.g, &bignum::to_be(&u1, self.size)), &self.mul(&q, &bignum::to_be(&u2, self.size)));
        let Some((mut x, _)) = self.affine(&pt) else { return false };
        if bignum::cmp(&x, &n.n) != Ordering::Less {
            bignum::sub_in(&mut x, &n.n);
        }
        x == r
    }

    /// A key pair from 64 random bytes: (secret, public point).
    pub fn keypair(&self, random: &[u8]) -> (Vec<u8>, Vec<u8>) {
        // reduce a double-width number mod n - 1, then add 1
        let limbs = self.size / 8;
        let mut nm1 = self.order.n.clone();
        bignum::sub_in(&mut nm1, &[1]);
        let m = Mont::new(self.order.n.clone());
        let hi = bignum::from_be(&random[..self.size], limbs);
        let lo = bignum::from_be(&random[self.size..2 * self.size], limbs);
        let mut hi = hi;
        while bignum::cmp(&hi, &nm1) != Ordering::Less {
            bignum::sub_in(&mut hi, &nm1);
        }
        let mut lo = lo;
        while bignum::cmp(&lo, &nm1) != Ordering::Less {
            bignum::sub_in(&mut lo, &nm1);
        }
        let mut d = m.addm(&hi, &lo);
        if bignum::cmp(&d, &nm1) != Ordering::Less {
            bignum::sub_in(&mut d, &nm1);
        }
        bignum::add_in(&mut d, &[1]);
        let secret = bignum::to_be(&d, self.size);
        let (x, y) = self.affine(&self.mul(&self.g, &secret)).unwrap_or_default();
        (secret, self.encode(&x, &y))
    }

    /// ECDH: the shared x coordinate, or None for a bad peer point.
    pub fn shared(&self, secret: &[u8], peer: &[u8]) -> Option<Vec<u8>> {
        let q = self.decode(peer)?;
        let (x, _) = self.affine(&self.mul(&q, secret))?;
        Some(bignum::to_be(&x, self.size))
    }
}
