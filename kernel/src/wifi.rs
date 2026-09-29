//! Wi-Fi: what the air says, and WPA2's security.
//!
//! - beacons and probe responses: network name, channel and band, security
//!   (open, WEP, WPA, WPA2 / WPA3 Personal and Enterprise, OWE) and
//!   generation (Wi-Fi 4 to 7), signal bars;
//! - WPA2-Personal: the passphrase to a key (PBKDF2-HMAC-SHA1), the
//!   pairwise keys (IEEE 802.11 PRF), the 4-way handshake as the station
//!   (EAPOL-Key frames, MICs, the group key unwrapped with AES key wrap),
//!   and CCMP, the AES-CCM encryption of every data frame.
//!
//! This is the part of Wi-Fi that's the same on every chip. The chips
//! themselves (Intel, Qualcomm, MediaTek, Realtek) each need a driver and
//! the vendor's firmware; until those exist HydatekOS uses the firmware's
//! Wi-Fi where a laptop has it (uefiwifi.rs). Plain logic, host-tested
//! against IEEE 802.11 Annex J's test vectors and Python's cryptography.

// the security layer waits for a Wi-Fi chip driver to call it; host-tested
#![allow(dead_code)]

use crate::tls::aes::Aes;
use alloc::string::String;
use alloc::vec::Vec;

// ---- the networks around -------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Security {
    Open,
    Wep,
    Wpa,
    Wpa2Personal,
    Wpa3Personal,
    /// WPA2 and WPA3 both offered (transition mode)
    Wpa2Wpa3,
    Enterprise,
    /// Enhanced Open (encrypted, no password)
    Owe,
}

impl Security {
    pub fn name(self) -> &'static str {
        match self {
            Security::Open => "Open",
            Security::Wep => "WEP (not safe)",
            Security::Wpa => "WPA",
            Security::Wpa2Personal => "WPA2",
            Security::Wpa3Personal => "WPA3",
            Security::Wpa2Wpa3 => "WPA2/WPA3",
            Security::Enterprise => "Enterprise",
            Security::Owe => "Enhanced Open",
        }
    }

    pub fn needs_password(self) -> bool {
        matches!(self, Security::Wep | Security::Wpa | Security::Wpa2Personal | Security::Wpa3Personal | Security::Wpa2Wpa3)
    }
}

/// A network one access point offers.
#[derive(Clone, Debug, PartialEq)]
pub struct Bss {
    pub bssid: [u8; 6],
    pub ssid: String,
    pub channel: u8,
    /// 2 (2.4 GHz), 5 or 6 (GHz)
    pub band: u8,
    pub security: Security,
    /// Wi-Fi 4, 5, 6, 7 (0: older)
    pub generation: u8,
    pub rssi: i8,
}

impl Bss {
    /// Signal bars, 0-4.
    pub fn bars(&self) -> u8 {
        match self.rssi {
            r if r >= -55 => 4,
            r if r >= -67 => 3,
            r if r >= -75 => 2,
            r if r >= -85 => 1,
            _ => 0,
        }
    }

    pub fn generation_name(&self) -> String {
        match (self.generation, self.band) {
            (6, 6) => String::from("Wi-Fi 6E"),
            (0, _) => String::new(),
            (g, _) => alloc::format!("Wi-Fi {}", g),
        }
    }
}

/// The information elements of a frame body: (id, data), with extension
/// elements (255) as (255, [ext id, data…]).
pub fn elements(b: &[u8]) -> Vec<(u8, &[u8])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 <= b.len() {
        let (id, len) = (b[i], b[i + 1] as usize);
        if i + 2 + len > b.len() {
            break;
        }
        out.push((id, &b[i + 2..i + 2 + len]));
        i += 2 + len;
    }
    out
}

/// The security an RSN element (48) offers.
pub fn rsn_security(rsn: &[u8]) -> Security {
    // version, group cipher, then pairwise ciphers and AKMs
    if rsn.len() < 8 {
        return Security::Wpa2Personal;
    }
    let pc = u16::from_le_bytes([rsn[6], rsn[7]]) as usize;
    let at = 8 + 4 * pc;
    if rsn.len() < at + 2 {
        return Security::Wpa2Personal;
    }
    let ac = u16::from_le_bytes([rsn[at], rsn[at + 1]]) as usize;
    let akms: Vec<u8> = (0..ac).filter_map(|k| rsn.get(at + 2 + 4 * k..at + 6 + 4 * k)).filter(|s| s[..3] == [0x00, 0x0F, 0xAC]).map(|s| s[3]).collect();
    let psk = akms.iter().any(|a| *a == 2 || *a == 6);
    let sae = akms.iter().any(|a| *a == 8 || *a == 24);
    if akms.iter().any(|a| matches!(a, 1 | 5 | 11 | 12 | 13)) && !psk && !sae {
        Security::Enterprise
    } else if psk && sae {
        Security::Wpa2Wpa3
    } else if sae {
        Security::Wpa3Personal
    } else if akms.contains(&18) {
        Security::Owe
    } else {
        Security::Wpa2Personal
    }
}

/// A beacon or probe response (the whole 802.11 frame).
pub fn parse_beacon(f: &[u8], rssi: i8) -> Option<Bss> {
    // frame control: management (type 0), beacon (8) or probe response (5)
    if f.len() < 36 || f[0] & 0x0C != 0 || !matches!(f[0] >> 4, 8 | 5) {
        return None;
    }
    let mut bssid = [0u8; 6];
    bssid.copy_from_slice(&f[16..22]);
    let cap = u16::from_le_bytes([f[34], f[35]]);
    let ies = elements(&f[36..]);
    let mut ssid = String::new();
    let (mut channel, mut sec, mut gen, mut six) = (0u8, None, 0u8, false);
    for (id, d) in &ies {
        match (*id, d.first()) {
            (0, _) => ssid = String::from_utf8_lossy(d).into(),
            (3, Some(c)) => channel = *c,
            (48, _) => sec = Some(rsn_security(d)),
            (221, _) if d.len() >= 4 && d[..4] == [0x00, 0x50, 0xF2, 0x01] && sec.is_none() => sec = Some(Security::Wpa),
            (45, _) => gen = gen.max(4),
            (191, _) => gen = gen.max(5),
            (255, Some(35)) => gen = gen.max(6),
            (255, Some(36)) if d.len() > 7 && d[3] & 0x02 != 0 => six = true,
            (255, Some(108)) => gen = gen.max(7),
            (61, Some(c)) if channel == 0 => channel = *c,
            _ => {}
        }
    }
    let security = sec.unwrap_or(if cap & 0x10 != 0 { Security::Wep } else { Security::Open });
    let band = if six { 6 } else if channel >= 32 { 5 } else { 2 };
    Some(Bss { bssid, ssid, channel, band, security, generation: gen, rssi })
}

// ---- WPA2 keys ---------------------------------------------------------------------

pub fn hmac_sha1(key: &[u8], data: &[&[u8]]) -> [u8; 20] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..20].copy_from_slice(&crate::crypto::sha1(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    for d in data {
        inner.extend_from_slice(d);
    }
    let ih = crate::crypto::sha1(&inner);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5C).collect();
    outer.extend_from_slice(&ih);
    crate::crypto::sha1(&outer)
}

/// The passphrase and network name to the pairwise master key.
pub fn psk(passphrase: &str, ssid: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (block, chunk) in out.chunks_mut(20).enumerate() {
        let mut u = hmac_sha1(passphrase.as_bytes(), &[ssid, &(block as u32 + 1).to_be_bytes()]);
        let mut t = u;
        for _ in 1..4096 {
            u = hmac_sha1(passphrase.as_bytes(), &[&u]);
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
    out
}

/// IEEE 802.11's PRF: `bits` of HMAC-SHA1(key, label 0 data counter).
pub fn prf(key: &[u8], label: &str, data: &[u8], bits: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0u8;
    while out.len() * 8 < bits {
        out.extend_from_slice(&hmac_sha1(key, &[label.as_bytes(), &[0], data, &[i]]));
        i += 1;
    }
    out.truncate(bits / 8);
    out
}

/// The pairwise transient key: KCK (16), KEK (16), TK (16).
pub fn ptk(pmk: &[u8; 32], aa: &[u8; 6], spa: &[u8; 6], anonce: &[u8; 32], snonce: &[u8; 32]) -> [u8; 48] {
    let (amin, amax) = if aa < spa { (aa, spa) } else { (spa, aa) };
    let (nmin, nmax) = if anonce < snonce { (anonce, snonce) } else { (snonce, anonce) };
    let mut data = Vec::new();
    data.extend_from_slice(amin);
    data.extend_from_slice(amax);
    data.extend_from_slice(nmin);
    data.extend_from_slice(nmax);
    let p = prf(pmk, "Pairwise key expansion", &data, 384);
    let mut out = [0u8; 48];
    out.copy_from_slice(&p);
    out
}

/// RFC 3394 AES key unwrap.
pub fn key_unwrap(kek: &[u8], data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 24 || data.len() % 8 != 0 {
        return None;
    }
    let aes = Aes::new(kek);
    let n = data.len() / 8 - 1;
    let mut a = [0u8; 8];
    a.copy_from_slice(&data[..8]);
    let mut r: Vec<[u8; 8]> = data[8..].chunks(8).map(|c| c.try_into().unwrap()).collect();
    for j in (0..6).rev() {
        for i in (0..n).rev() {
            let t = (n * j + i + 1) as u64;
            let mut b = [0u8; 16];
            for k in 0..8 {
                b[k] = a[k] ^ t.to_be_bytes()[k];
            }
            b[8..].copy_from_slice(&r[i]);
            aes.decrypt(&mut b);
            a.copy_from_slice(&b[..8]);
            r[i].copy_from_slice(&b[8..]);
        }
    }
    (a == [0xA6; 8]).then(|| r.concat())
}

// ---- the 4-way handshake ------------------------------------------------------------

const KI_PAIRWISE: u16 = 1 << 3;
const KI_INSTALL: u16 = 1 << 6;
const KI_ACK: u16 = 1 << 7;
const KI_MIC: u16 = 1 << 8;
const KI_SECURE: u16 = 1 << 9;
const KI_ENCRYPTED: u16 = 1 << 12;

/// An EAPOL-Key frame (from the EAPOL header on).
pub struct KeyFrame<'a>(pub &'a [u8]);

impl<'a> KeyFrame<'a> {
    fn ok(&self) -> bool {
        self.0.len() >= 99 && self.0[1] == 3 && self.0[4] == 2
    }
    pub fn info(&self) -> u16 {
        u16::from_be_bytes([self.0[5], self.0[6]])
    }
    pub fn replay(&self) -> [u8; 8] {
        self.0[9..17].try_into().unwrap()
    }
    pub fn nonce(&self) -> [u8; 32] {
        self.0[17..49].try_into().unwrap()
    }
    pub fn mic(&self) -> &[u8] {
        &self.0[81..97]
    }
    pub fn data(&self) -> &[u8] {
        let n = u16::from_be_bytes([self.0[97], self.0[98]]) as usize;
        &self.0[99..(99 + n).min(self.0.len())]
    }
}

/// The MIC of an EAPOL-Key frame (key descriptor version 2: HMAC-SHA1-128),
/// computed with the MIC field zeroed.
pub fn eapol_mic(kck: &[u8], frame: &[u8]) -> [u8; 16] {
    let mut f = frame.to_vec();
    f[81..97].fill(0);
    let h = hmac_sha1(kck, &[&f]);
    h[..16].try_into().unwrap()
}

fn key_frame(info: u16, replay: &[u8; 8], nonce: &[u8; 32], data: &[u8]) -> Vec<u8> {
    let body = 95 + data.len();
    let mut f = Vec::with_capacity(4 + body);
    f.extend_from_slice(&[2, 3]);
    f.extend_from_slice(&(body as u16).to_be_bytes());
    f.push(2);
    f.extend_from_slice(&info.to_be_bytes());
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(replay);
    f.extend_from_slice(nonce);
    f.extend_from_slice(&[0u8; 16 + 8 + 8 + 16]);
    f.extend_from_slice(&(data.len() as u16).to_be_bytes());
    f.extend_from_slice(data);
    f
}

/// The station's side of WPA2-Personal's 4-way handshake.
pub struct Handshake {
    pmk: [u8; 32],
    aa: [u8; 6],
    spa: [u8; 6],
    snonce: [u8; 32],
    /// our RSN element, sent in message 2
    rsn: Vec<u8>,
    pub ptk: Option<[u8; 48]>,
    pub gtk: Option<(u8, Vec<u8>)>,
}

/// An RSN element for WPA2-Personal with CCMP.
pub fn rsn_element() -> Vec<u8> {
    alloc::vec![48, 20, 1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, 4, 1, 0, 0x00, 0x0F, 0xAC, 2, 0, 0]
}

impl Handshake {
    pub fn new(pmk: [u8; 32], aa: [u8; 6], spa: [u8; 6], snonce: [u8; 32]) -> Handshake {
        Handshake { pmk, aa, spa, snonce, rsn: rsn_element(), ptk: None, gtk: None }
    }

    /// Message 1 (the access point's nonce) in, message 2 out.
    pub fn message1(&mut self, m1: &[u8]) -> Option<Vec<u8>> {
        let k = KeyFrame(m1);
        if !k.ok() || k.info() & (KI_PAIRWISE | KI_ACK) != KI_PAIRWISE | KI_ACK || k.info() & KI_MIC != 0 {
            return None;
        }
        let ptk = ptk(&self.pmk, &self.aa, &self.spa, &k.nonce(), &self.snonce);
        self.ptk = Some(ptk);
        let mut m2 = key_frame(2 | KI_PAIRWISE | KI_MIC, &k.replay(), &self.snonce, &self.rsn);
        let mic = eapol_mic(&ptk[..16], &m2);
        m2[81..97].copy_from_slice(&mic);
        Some(m2)
    }

    /// Message 3 (checked, the group key unwrapped) in, message 4 out.
    pub fn message3(&mut self, m3: &[u8]) -> Option<Vec<u8>> {
        let ptk = self.ptk?;
        let k = KeyFrame(m3);
        let want = KI_PAIRWISE | KI_ACK | KI_MIC | KI_INSTALL;
        if !k.ok() || k.info() & want != want || eapol_mic(&ptk[..16], m3) != k.mic() {
            return None;
        }
        if k.info() & KI_ENCRYPTED != 0 {
            let kd = key_unwrap(&ptk[16..32], k.data())?;
            // the GTK key data element: dd len 00-0F-AC 01 keyid rsvd gtk…
            for (id, d) in elements(&kd) {
                if id == 0xDD && d.len() > 6 && d[..4] == [0x00, 0x0F, 0xAC, 0x01] {
                    self.gtk = Some((d[4] & 3, d[6..].to_vec()));
                }
            }
        }
        let mut m4 = key_frame(2 | KI_PAIRWISE | KI_MIC | KI_SECURE, &k.replay(), &[0u8; 32], &[]);
        let mic = eapol_mic(&ptk[..16], &m4);
        m4[81..97].copy_from_slice(&mic);
        Some(m4)
    }
}

// ---- CCMP ---------------------------------------------------------------------------

/// The nonce and additional data CCMP authenticates for a data frame's header.
fn ccmp_nonce_aad(hdr: &[u8], pn: u64) -> ([u8; 13], Vec<u8>) {
    let fc1 = hdr[1];
    let (to_ds, from_ds) = (fc1 & 1 != 0, fc1 & 2 != 0);
    let a4 = to_ds && from_ds;
    let qos = hdr[0] & 0x0C == 0x08 && hdr[0] & 0x80 != 0;
    let qc_at = if a4 { 30 } else { 24 };
    let tid = if qos { hdr[qc_at] & 0x0F } else { 0 };
    let mut nonce = [0u8; 13];
    nonce[0] = tid;
    nonce[1..7].copy_from_slice(&hdr[10..16]);
    nonce[7..13].copy_from_slice(&pn.to_be_bytes()[2..]);
    let mut aad = Vec::new();
    // frame control with the subtype's QoS bits, retry, power and more-data masked
    aad.push(if hdr[0] & 0x0C == 0x08 { hdr[0] & 0x8F } else { hdr[0] });
    aad.push((fc1 & 0xC7) | 0x40);
    aad.extend_from_slice(&hdr[4..22]);
    // sequence control: fragment number only
    aad.push(hdr[22] & 0x0F);
    aad.push(0);
    if a4 {
        aad.extend_from_slice(&hdr[24..30]);
    }
    if qos {
        aad.push(hdr[qc_at] & 0x0F);
        aad.push(0);
    }
    (nonce, aad)
}

fn ccm_blocks(aes: &Aes, nonce: &[u8; 13], aad: &[u8], data: &[u8]) -> [u8; 16] {
    // CBC-MAC over B0, the additional data, the message
    let mut x = [0u8; 16];
    x[0] = 0x59;
    x[1..14].copy_from_slice(nonce);
    x[14..16].copy_from_slice(&(data.len() as u16).to_be_bytes());
    aes.encrypt(&mut x);
    let mut a = Vec::with_capacity(aad.len() + 2);
    a.extend_from_slice(&(aad.len() as u16).to_be_bytes());
    a.extend_from_slice(aad);
    for part in [&a[..], data] {
        for chunk in part.chunks(16) {
            for (i, b) in chunk.iter().enumerate() {
                x[i] ^= b;
            }
            aes.encrypt(&mut x);
        }
    }
    x
}

fn ccm_ctr(aes: &Aes, nonce: &[u8; 13], i: u16) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[0] = 0x01;
    a[1..14].copy_from_slice(nonce);
    a[14..16].copy_from_slice(&i.to_be_bytes());
    aes.encrypt(&mut a);
    a
}

/// Encrypt a data frame's body: ciphertext followed by the 8-byte MIC.
pub fn ccmp_encrypt(tk: &[u8], hdr: &[u8], pn: u64, plain: &[u8]) -> Vec<u8> {
    let aes = Aes::new(tk);
    let (nonce, aad) = ccmp_nonce_aad(hdr, pn);
    let t = ccm_blocks(&aes, &nonce, &aad, plain);
    let mut out = Vec::with_capacity(plain.len() + 8);
    for (k, chunk) in plain.chunks(16).enumerate() {
        let s = ccm_ctr(&aes, &nonce, k as u16 + 1);
        out.extend(chunk.iter().zip(s.iter()).map(|(a, b)| a ^ b));
    }
    let s0 = ccm_ctr(&aes, &nonce, 0);
    out.extend((0..8).map(|i| t[i] ^ s0[i]));
    out
}

/// Decrypt and check a data frame's body (ciphertext + MIC).
pub fn ccmp_decrypt(tk: &[u8], hdr: &[u8], pn: u64, sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 8 {
        return None;
    }
    let aes = Aes::new(tk);
    let (nonce, aad) = ccmp_nonce_aad(hdr, pn);
    let (ct, mic) = sealed.split_at(sealed.len() - 8);
    let mut plain = Vec::with_capacity(ct.len());
    for (k, chunk) in ct.chunks(16).enumerate() {
        let s = ccm_ctr(&aes, &nonce, k as u16 + 1);
        plain.extend(chunk.iter().zip(s.iter()).map(|(a, b)| a ^ b));
    }
    let t = ccm_blocks(&aes, &nonce, &aad, &plain);
    let s0 = ccm_ctr(&aes, &nonce, 0);
    let ok = (0..8).fold(0u8, |d, i| d | (t[i] ^ s0[i] ^ mic[i])) == 0;
    ok.then_some(plain)
}

/// A CCMP header's packet number (the 8 bytes after the 802.11 header).
pub fn ccmp_pn(h: &[u8]) -> u64 {
    h[0] as u64 | (h[1] as u64) << 8 | (h[4] as u64) << 16 | (h[5] as u64) << 24 | (h[6] as u64) << 32 | (h[7] as u64) << 40
}
