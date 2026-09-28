//! X.509 certificates: reading them, and checking that a site's chain leads
//! to a trusted authority, is current, and names the site.

use super::der::{self, Der, Tlv};
use super::ec::{Curve, Group};
use super::rsa;
use super::sha2::Hash;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

const OID_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_RSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
const OID_RSA_SHA384: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0c];
const OID_RSA_SHA512: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0d];
const OID_RSA_PSS: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0a];
const OID_EC: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
const OID_P256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2b, 0x81, 0x04, 0x00, 0x22];
const OID_ECDSA_SHA256: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x02];
const OID_ECDSA_SHA384: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x03];
const OID_ECDSA_SHA512: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x04, 0x03, 0x04];
const OID_SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
const OID_SHA384: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02];
const OID_SHA512: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03];
const OID_SAN: &[u8] = &[0x55, 0x1d, 0x11];
const OID_BASIC: &[u8] = &[0x55, 0x1d, 0x13];
pub const OID_CN: &[u8] = &[0x55, 0x04, 0x03];
pub const OID_ORG: &[u8] = &[0x55, 0x04, 0x0a];

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Key {
    Rsa { n: Vec<u8>, e: Vec<u8> },
    Ec { curve: Curve, point: Vec<u8> },
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigAlg {
    RsaPkcs1(Hash),
    RsaPss(Hash),
    Ecdsa(Hash),
    Other,
}

#[derive(Clone, Debug)]
pub struct Cert {
    pub tbs: Vec<u8>,
    pub sig_alg: SigAlg,
    pub sig: Vec<u8>,
    pub issuer: Vec<u8>,
    pub subject: Vec<u8>,
    pub not_before: i64,
    pub not_after: i64,
    pub key: Key,
    pub dns: Vec<String>,
    pub ips: Vec<Vec<u8>>,
    pub ca: bool,
}

fn hash_oid(oid: &[u8]) -> Option<Hash> {
    match oid {
        OID_SHA256 => Some(Hash::Sha256),
        OID_SHA384 => Some(Hash::Sha384),
        OID_SHA512 => Some(Hash::Sha512),
        _ => None,
    }
}

fn sig_alg(t: Tlv) -> SigAlg {
    let mut d = t.inner();
    let Some(oid) = d.expect(der::OID) else { return SigAlg::Other };
    match oid.body {
        OID_RSA_SHA256 => SigAlg::RsaPkcs1(Hash::Sha256),
        OID_RSA_SHA384 => SigAlg::RsaPkcs1(Hash::Sha384),
        OID_RSA_SHA512 => SigAlg::RsaPkcs1(Hash::Sha512),
        OID_ECDSA_SHA256 => SigAlg::Ecdsa(Hash::Sha256),
        OID_ECDSA_SHA384 => SigAlg::Ecdsa(Hash::Sha384),
        OID_ECDSA_SHA512 => SigAlg::Ecdsa(Hash::Sha512),
        OID_RSA_PSS => {
            // RSASSA-PSS-params: [0] hashAlgorithm (the default, SHA-1, isn't accepted)
            let h = d
                .expect(der::SEQ)
                .and_then(|p| p.inner().optional(0xa0))
                .and_then(|h| h.inner().expect(der::SEQ))
                .and_then(|a| a.inner().expect(der::OID))
                .and_then(|o| hash_oid(o.body));
            h.map_or(SigAlg::Other, SigAlg::RsaPss)
        }
        _ => SigAlg::Other,
    }
}

/// A SubjectPublicKeyInfo.
pub fn parse_key(spki: &[u8]) -> Key {
    let f = || -> Option<Key> {
        let mut d = Der::new(spki).expect(der::SEQ)?.inner();
        let mut alg = d.expect(der::SEQ)?.inner();
        let oid = alg.expect(der::OID)?;
        let bits = d.expect(der::BITS)?.bit_bytes()?;
        match oid.body {
            OID_RSA => {
                let mut k = Der::new(bits).expect(der::SEQ)?.inner();
                let n = k.expect(der::INT)?.uint().to_vec();
                let e = k.expect(der::INT)?.uint().to_vec();
                Some(Key::Rsa { n, e })
            }
            OID_EC => {
                let curve = match alg.expect(der::OID)?.body {
                    OID_P256 => Curve::P256,
                    OID_P384 => Curve::P384,
                    _ => return Some(Key::Other),
                };
                Some(Key::Ec { curve, point: bits.to_vec() })
            }
            _ => Some(Key::Other),
        }
    };
    f().unwrap_or(Key::Other)
}

fn digits(s: &[u8]) -> Option<i64> {
    let mut v = 0i64;
    for &c in s {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (c - b'0') as i64;
    }
    Some(v)
}

/// Days since 1970-01-01 of a civil date.
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

pub fn date(unix: i64) -> String {
    let (y, m, d) = civil_from_days(unix.div_euclid(86400));
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    format!("{} {} {}", d, MONTHS[(m - 1) as usize], y)
}

fn time(t: Tlv) -> Option<i64> {
    let s = t.body;
    let (year, rest) = match t.tag {
        0x17 if s.len() == 13 => {
            let yy = digits(&s[..2])?;
            (if yy < 50 { 2000 + yy } else { 1900 + yy }, &s[2..])
        }
        0x18 if s.len() == 15 => (digits(&s[..4])?, &s[4..]),
        _ => return None,
    };
    if rest[10] != b'Z' {
        return None;
    }
    let f = |i: usize| digits(&rest[i..i + 2]);
    let days = days_from_civil(year, f(0)?, f(2)?);
    Some(days * 86400 + f(4)? * 3600 + f(6)? * 60 + f(8)?)
}

pub fn parse(data: &[u8]) -> Option<Cert> {
    let mut top = Der::new(data).expect(der::SEQ)?.inner();
    let tbs = top.expect(der::SEQ)?;
    let alg = sig_alg(top.expect(der::SEQ)?);
    let sig = top.expect(der::BITS)?.bit_bytes()?.to_vec();
    let mut t = tbs.inner();
    t.optional(0xa0);
    t.expect(der::INT)?;
    t.expect(der::SEQ)?;
    let issuer = t.expect(der::SEQ)?.raw.to_vec();
    let mut validity = t.expect(der::SEQ)?.inner();
    let not_before = time(validity.next()?)?;
    let not_after = time(validity.next()?)?;
    let subject = t.expect(der::SEQ)?.raw.to_vec();
    let key = parse_key(t.expect(der::SEQ)?.raw);
    t.optional(0x81);
    t.optional(0x82);
    let mut cert = Cert { tbs: tbs.raw.to_vec(), sig_alg: alg, sig, issuer, subject, not_before, not_after, key, dns: Vec::new(), ips: Vec::new(), ca: false };
    if let Some(ext) = t.optional(0xa3) {
        let mut list = ext.inner().expect(der::SEQ)?.inner();
        while let Some(e) = list.next() {
            let mut e = e.inner();
            let oid = e.expect(der::OID)?;
            e.optional(der::BOOL);
            let value = e.expect(der::OCTETS)?.body;
            match oid.body {
                OID_SAN => {
                    let mut names = Der::new(value).expect(der::SEQ)?.inner();
                    while let Some(n) = names.next() {
                        match n.tag {
                            0x82 => cert.dns.push(String::from_utf8_lossy(n.body).to_ascii_lowercase()),
                            0x87 => cert.ips.push(n.body.to_vec()),
                            _ => {}
                        }
                    }
                }
                OID_BASIC => {
                    let mut b = Der::new(value).expect(der::SEQ)?.inner();
                    if let Some(ca) = b.optional(der::BOOL) {
                        cert.ca = ca.body.first().is_some_and(|&v| v != 0);
                    }
                }
                _ => {}
            }
        }
    }
    Some(cert)
}

/// A text attribute (like the common name) of a Name.
pub fn name_field(name: &[u8], oid: &[u8]) -> Option<String> {
    let mut rdns = Der::new(name).expect(der::SEQ)?.inner();
    while let Some(set) = rdns.next() {
        let mut set = set.inner();
        while let Some(atv) = set.next() {
            let mut atv = atv.inner();
            if atv.expect(der::OID)?.body == oid {
                let v = atv.next()?;
                if v.tag == 0x1e {
                    // BMPString
                    let u: Vec<u16> = v.body.chunks(2).map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
                    return Some(String::from_utf16_lossy(&u));
                }
                return Some(String::from_utf8_lossy(v.body).to_string());
            }
        }
    }
    None
}

/// A readable name for a certificate's subject or issuer.
pub fn display_name(name: &[u8]) -> String {
    name_field(name, OID_ORG).or_else(|| name_field(name, OID_CN)).unwrap_or_else(|| String::from("an unnamed authority"))
}

/// Check `sig` over `msg` with `key`. ECDSA signatures are DER (r, s).
pub fn check(key: &Key, alg: SigAlg, msg: &[u8], sig: &[u8]) -> bool {
    match (key, alg) {
        (Key::Rsa { n, e }, SigAlg::RsaPkcs1(h)) => rsa::verify_pkcs1(n, e, h, &h.digest(msg), sig),
        (Key::Rsa { n, e }, SigAlg::RsaPss(h)) => rsa::verify_pss(n, e, h, &h.digest(msg), sig),
        (Key::Ec { curve, point }, SigAlg::Ecdsa(h)) => {
            let f = || -> Option<bool> {
                let mut d = Der::new(sig).expect(der::SEQ)?.inner();
                let r = d.expect(der::INT)?.uint();
                let s = d.expect(der::INT)?.uint();
                Some(Group::new(*curve).verify(point, &h.digest(msg), r, s))
            };
            f().unwrap_or(false)
        }
        _ => false,
    }
}

pub struct Anchor {
    pub subject: Vec<u8>,
    pub key: Key,
}

/// The certificate authorities HydatekOS trusts.
#[derive(Default)]
pub struct Roots {
    pub anchors: Vec<Anchor>,
}

impl Roots {
    /// The built-in list: Mozilla's CA list, as (subject, public key) pairs.
    pub fn builtin() -> Roots {
        let data: &[u8] = include_bytes!("roots.bin");
        let mut r = Roots::default();
        let mut p = 0;
        let take = |p: &mut usize| -> Option<&[u8]> {
            let n = u16::from_be_bytes([*data.get(*p)?, *data.get(*p + 1)?]) as usize;
            let s = data.get(*p + 2..*p + 2 + n)?;
            *p += 2 + n;
            Some(s)
        };
        while p < data.len() {
            let (Some(subject), Some(spki)) = (take(&mut p), take(&mut p)) else { break };
            r.anchors.push(Anchor { subject: subject.to_vec(), key: parse_key(spki) });
        }
        r
    }

    /// Trust the certificates in a file (PEM, possibly several, or DER).
    /// Returns how many were added.
    pub fn add_file(&mut self, data: &[u8]) -> usize {
        let text = String::from_utf8_lossy(data);
        if !text.contains("-----BEGIN CERTIFICATE-----") {
            return self.add(data) as usize;
        }
        let mut n = 0;
        for block in text.split("-----BEGIN CERTIFICATE-----").skip(1) {
            let b64: String = block.split("-----END").next().unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect();
            if let Some(der) = crate::crypto::base64_decode(&b64) {
                n += self.add(&der) as usize;
            }
        }
        n
    }

    /// Trust another authority (a certificate the user installed).
    pub fn add(&mut self, cert_der: &[u8]) -> bool {
        match parse(cert_der) {
            Some(c) => {
                self.anchors.push(Anchor { subject: c.subject, key: c.key });
                true
            }
            None => false,
        }
    }
}

fn ip4(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = host.split('.');
    for o in out.iter_mut() {
        *o = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

fn name_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim_end_matches('.');
    if let Some(rest) = pattern.strip_prefix("*.") {
        // one label, and not for a bare public suffix like *.com
        return rest.contains('.') && host.split_once('.').is_some_and(|(label, tail)| !label.is_empty() && tail == rest);
    }
    pattern == host
}

pub fn host_matches(c: &Cert, host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Some(ip) = ip4(&host) {
        return c.ips.iter().any(|i| i[..] == ip[..]);
    }
    c.dns.iter().any(|p| name_matches(p, &host))
}

/// What a verified chain says, for showing to the user.
#[derive(Clone, Debug)]
pub struct Verified {
    pub issuer: String,
    pub expires: i64,
}

/// Check a server's chain (leaf first) for `host` at unix time `now`.
pub fn verify(chain: &[Vec<u8>], host: &str, now: i64, roots: &Roots) -> Result<Verified, String> {
    let first = chain.first().ok_or_else(|| String::from("The site didn't send a certificate."))?;
    let leaf = parse(first).ok_or_else(|| String::from("HydatekOS couldn't read the site's certificate."))?;
    if !host_matches(&leaf, host) {
        let names: Vec<String> = leaf.dns.iter().take(3).cloned().collect();
        let names = if names.is_empty() { String::from("another site") } else { names.join(", ") };
        return Err(format!("The site's certificate is for {}, not {}. Someone may be pretending to be the site.", names, host));
    }
    let certs: Vec<Cert> = chain.iter().filter_map(|c| parse(c)).collect();
    let expires = leaf.not_after;
    let mut cur = leaf;
    let mut used = Vec::new();
    for depth in 0..10 {
        if now < cur.not_before {
            return Err(format!("A certificate for this site isn't valid until {}. Check that this computer's date is right.", date(cur.not_before)));
        }
        if now > cur.not_after {
            return Err(format!("The site's certificate expired on {}.", date(cur.not_after)));
        }
        if depth > 0 && !cur.ca {
            break;
        }
        for a in roots.anchors.iter().filter(|a| a.subject == cur.issuer) {
            if check(&a.key, cur.sig_alg, &cur.tbs, &cur.sig) {
                return Ok(Verified { issuer: display_name(&cur.issuer), expires });
            }
        }
        // the certificate itself may be a trusted authority
        if roots.anchors.iter().any(|a| a.subject == cur.subject && a.key == cur.key) {
            return Ok(Verified { issuer: display_name(&cur.subject), expires });
        }
        let next = certs
            .iter()
            .enumerate()
            .skip(1)
            .find(|(i, c)| !used.contains(i) && c.subject == cur.issuer && c.ca && check(&c.key, cur.sig_alg, &cur.tbs, &cur.sig));
        match next {
            Some((i, c)) => {
                used.push(i);
                cur = c.clone();
            }
            None => break,
        }
    }
    Err(format!("The site's certificate comes from {}, which HydatekOS doesn't trust.", display_name(&cur.issuer)))
}
