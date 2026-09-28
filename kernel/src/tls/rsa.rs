//! RSA signature checks (RFC 8017): PKCS #1 v1.5 and PSS.

use super::bignum::{self, Mont};
use super::sha2::Hash;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// Keys below this are refused.
const MIN_BITS: usize = 2048;

/// sig^e mod n as bytes the size of n.
fn public_op(n: &[u8], e: &[u8], sig: &[u8]) -> Option<Vec<u8>> {
    let n = &n[n.iter().position(|&b| b != 0)?..];
    if sig.len() != n.len() || n.len() > 1024 || n[n.len() - 1] & 1 == 0 {
        return None;
    }
    let limbs = n.len().div_ceil(8);
    let nb = bignum::from_be(n, limbs);
    if bignum::bits(&nb) < MIN_BITS {
        return None;
    }
    let s = bignum::from_be(sig, limbs);
    if bignum::cmp(&s, &nb) != Ordering::Less {
        return None;
    }
    let m = Mont::new(nb);
    Some(bignum::to_be(&m.pow(&s, e), n.len()))
}

fn digest_info(h: Hash) -> &'static [u8] {
    match h {
        Hash::Sha256 => &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20],
        Hash::Sha384 => &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30],
        Hash::Sha512 => &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40],
    }
}

/// RSASSA-PKCS1-v1_5 over a message digest.
pub fn verify_pkcs1(n: &[u8], e: &[u8], h: Hash, digest: &[u8], sig: &[u8]) -> bool {
    let Some(em) = public_op(n, e, sig) else { return false };
    let di = digest_info(h);
    let t = di.len() + digest.len();
    if em.len() < t + 11 {
        return false;
    }
    let mut want = Vec::with_capacity(em.len());
    want.extend_from_slice(&[0, 1]);
    want.resize(em.len() - t - 1, 0xff);
    want.push(0);
    want.extend_from_slice(di);
    want.extend_from_slice(digest);
    want == em
}

fn mgf1(h: Hash, seed: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len + 64);
    let mut c = 0u32;
    while out.len() < len {
        let mut x = seed.to_vec();
        x.extend_from_slice(&c.to_be_bytes());
        out.extend(h.digest(&x));
        c += 1;
    }
    out.truncate(len);
    out
}

/// RSASSA-PSS with MGF1 of the same hash; any salt length.
pub fn verify_pss(n: &[u8], e: &[u8], h: Hash, digest: &[u8], sig: &[u8]) -> bool {
    let Some(m) = public_op(n, e, sig) else { return false };
    let nz = &n[n.iter().position(|&b| b != 0).unwrap_or(0)..];
    let mod_bits = 8 * nz.len() - nz[0].leading_zeros() as usize;
    let em_bits = mod_bits - 1;
    let em_len = em_bits.div_ceil(8);
    if m.len() > em_len && m[0] != 0 {
        return false;
    }
    let em = &m[m.len() - em_len..];
    let hl = h.len();
    if em_len < hl + 2 || em[em_len - 1] != 0xbc {
        return false;
    }
    let (masked, rest) = em.split_at(em_len - hl - 1);
    let hh = &rest[..hl];
    let zero_bits = 8 * em_len - em_bits;
    if zero_bits > 0 && masked[0] >> (8 - zero_bits) != 0 {
        return false;
    }
    let mask = mgf1(h, hh, masked.len());
    let mut db: Vec<u8> = masked.iter().zip(mask.iter()).map(|(a, b)| a ^ b).collect();
    if zero_bits > 0 {
        db[0] &= 0xff >> zero_bits;
    }
    let Some(one) = db.iter().position(|&b| b != 0) else { return false };
    if db[one] != 1 {
        return false;
    }
    let salt = &db[one + 1..];
    let mut mp = alloc::vec![0u8; 8];
    mp.extend_from_slice(digest);
    mp.extend_from_slice(salt);
    h.digest(&mp) == hh
}
