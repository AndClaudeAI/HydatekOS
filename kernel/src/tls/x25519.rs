//! X25519 key agreement (RFC 7748), with constant-time field arithmetic
//! on five 51-bit limbs.

type Fe = [u64; 5];
const MASK: u64 = (1 << 51) - 1;

fn carry(mut h: [u128; 5]) -> Fe {
    for _ in 0..2 {
        let mut c: u128;
        for i in 0..4 {
            c = h[i] >> 51;
            h[i] &= MASK as u128;
            h[i + 1] += c;
        }
        c = h[4] >> 51;
        h[4] &= MASK as u128;
        h[0] += c * 19;
    }
    [h[0] as u64, h[1] as u64, h[2] as u64, h[3] as u64, h[4] as u64]
}

fn add(a: &Fe, b: &Fe) -> Fe {
    carry([0, 1, 2, 3, 4].map(|i| (a[i] + b[i]) as u128))
}

fn sub(a: &Fe, b: &Fe) -> Fe {
    // add 4p so every limb stays positive
    const P4: [u64; 5] = [0x1f_ffff_ffff_ffb4, 0x1f_ffff_ffff_fffc, 0x1f_ffff_ffff_fffc, 0x1f_ffff_ffff_fffc, 0x1f_ffff_ffff_fffc];
    carry([0, 1, 2, 3, 4].map(|i| (a[i] + P4[i] - b[i]) as u128))
}

fn mul(a: &Fe, b: &Fe) -> Fe {
    let m = |x: u64, y: u64| x as u128 * y as u128;
    let b19 = [b[0], b[1] * 19, b[2] * 19, b[3] * 19, b[4] * 19];
    carry([
        m(a[0], b[0]) + m(a[1], b19[4]) + m(a[2], b19[3]) + m(a[3], b19[2]) + m(a[4], b19[1]),
        m(a[0], b[1]) + m(a[1], b[0]) + m(a[2], b19[4]) + m(a[3], b19[3]) + m(a[4], b19[2]),
        m(a[0], b[2]) + m(a[1], b[1]) + m(a[2], b[0]) + m(a[3], b19[4]) + m(a[4], b19[3]),
        m(a[0], b[3]) + m(a[1], b[2]) + m(a[2], b[1]) + m(a[3], b[0]) + m(a[4], b19[4]),
        m(a[0], b[4]) + m(a[1], b[3]) + m(a[2], b[2]) + m(a[3], b[1]) + m(a[4], b[0]),
    ])
}

fn invert(a: &Fe) -> Fe {
    // a^(p-2), p-2 = 2^255 - 21
    let mut r: Fe = [1, 0, 0, 0, 0];
    for bit in (0..255).rev() {
        r = mul(&r, &r);
        let set = !(bit == 2 || bit == 4); // 2^255-21 = ...11101011
        if set {
            r = mul(&r, a);
        }
    }
    r
}

fn load(s: &[u8; 32]) -> Fe {
    let mut b = *s;
    b[31] &= 127;
    let x = |i: usize| u64::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3], b[i + 4], b[i + 5], b[i + 6], b[i + 7]]);
    [x(0) & MASK, (x(6) >> 3) & MASK, (x(12) >> 6) & MASK, (x(19) >> 1) & MASK, (x(24) >> 12) & MASK]
}

fn store(a: &Fe) -> [u8; 32] {
    let mut h = carry(a.map(|v| v as u128));
    // h < 2^255 + small; subtract p when h >= p
    let mut t = [0u64; 5];
    let mut c = 19u64;
    for i in 0..5 {
        let v = h[i] + c;
        t[i] = v & MASK;
        c = v >> 51;
    }
    let ge = 0u64.wrapping_sub(c); // all ones when h + 19 >= 2^255
    for i in 0..5 {
        h[i] = (t[i] & ge) | (h[i] & !ge);
    }
    let mut out = [0u8; 32];
    let mut acc: u128 = 0;
    let mut bits = 0;
    let mut k = 0;
    for limb in h {
        acc += (limb as u128) << bits;
        bits += 51;
        while bits >= 8 && k < 32 {
            out[k] = acc as u8;
            acc >>= 8;
            bits -= 8;
            k += 1;
        }
    }
    if k < 32 {
        out[k] = acc as u8;
    }
    out
}

fn cswap(swap: u64, a: &mut Fe, b: &mut Fe) {
    let m = 0u64.wrapping_sub(swap);
    for i in 0..5 {
        let t = m & (a[i] ^ b[i]);
        a[i] ^= t;
        b[i] ^= t;
    }
}

pub fn x25519(scalar: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    let mut k = *scalar;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let x1 = load(u);
    let (mut x2, mut z2, mut x3, mut z3): (Fe, Fe, Fe, Fe) = ([1, 0, 0, 0, 0], [0; 5], x1, [1, 0, 0, 0, 0]);
    let a24: Fe = [121665, 0, 0, 0, 0];
    let mut swap = 0u64;
    for t in (0..255).rev() {
        let kt = ((k[t / 8] >> (t % 8)) & 1) as u64;
        swap ^= kt;
        cswap(swap, &mut x2, &mut x3);
        cswap(swap, &mut z2, &mut z3);
        swap = kt;
        let a = add(&x2, &z2);
        let aa = mul(&a, &a);
        let b = sub(&x2, &z2);
        let bb = mul(&b, &b);
        let e = sub(&aa, &bb);
        let c = add(&x3, &z3);
        let d = sub(&x3, &z3);
        let da = mul(&d, &a);
        let cb = mul(&c, &b);
        let s = add(&da, &cb);
        x3 = mul(&s, &s);
        let s = sub(&da, &cb);
        z3 = mul(&x1, &mul(&s, &s));
        x2 = mul(&aa, &bb);
        z2 = mul(&e, &add(&aa, &mul(&a24, &e)));
    }
    cswap(swap, &mut x2, &mut x3);
    cswap(swap, &mut z2, &mut z3);
    store(&mul(&x2, &invert(&z2)))
}

pub fn public_key(secret: &[u8; 32]) -> [u8; 32] {
    let mut nine = [0u8; 32];
    nine[0] = 9;
    x25519(secret, &nine)
}
