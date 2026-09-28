//! The TLS client: TLS 1.3 (RFC 8446) and TLS 1.2 with ECDHE (RFC 5246,
//! 7627, 8422), AES-GCM and ChaCha20-Poly1305.
//!
//! It doesn't touch the network itself: the caller feeds it bytes from the
//! server and sends whatever `take_output` returns.

use super::aes::Gcm;
use super::ec::{Curve, Group};
use super::sha2::Hash;
use super::x25519;
use super::x509::{self, Roots, SigAlg, Verified};
use crate::crypto;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

const HANDSHAKE: u8 = 22;
const ALERT: u8 = 21;
const CCS: u8 = 20;
const APP: u8 = 23;
const MAX_RECORD: usize = 16384;

const X25519: u16 = 0x001d;
const SECP256R1: u16 = 0x0017;
const SECP384R1: u16 = 0x0018;

/// SHA-256("HelloRetryRequest"): the ServerHello random that means "retry".
const HRR_RANDOM: [u8; 32] = [
    0xcf, 0x21, 0xad, 0x74, 0xe5, 0x9a, 0x61, 0x11, 0xbe, 0x1d, 0x8c, 0x02, 0x1e, 0x65, 0xb8, 0x91, 0xc2, 0xa2, 0x11, 0x16, 0x7a, 0xbb, 0x8c, 0x5e, 0x07, 0x9e, 0x09, 0xe2,
    0xc8, 0xa8, 0x33, 0x9c,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Suite {
    id: u16,
    hash: Hash,
    key_len: usize,
    chacha: bool,
    tls13: bool,
}

const SUITES: [Suite; 9] = [
    Suite { id: 0x1301, hash: Hash::Sha256, key_len: 16, chacha: false, tls13: true },
    Suite { id: 0x1303, hash: Hash::Sha256, key_len: 32, chacha: true, tls13: true },
    Suite { id: 0x1302, hash: Hash::Sha384, key_len: 32, chacha: false, tls13: true },
    Suite { id: 0xc02b, hash: Hash::Sha256, key_len: 16, chacha: false, tls13: false },
    Suite { id: 0xc02f, hash: Hash::Sha256, key_len: 16, chacha: false, tls13: false },
    Suite { id: 0xcca9, hash: Hash::Sha256, key_len: 32, chacha: true, tls13: false },
    Suite { id: 0xcca8, hash: Hash::Sha256, key_len: 32, chacha: true, tls13: false },
    Suite { id: 0xc02c, hash: Hash::Sha384, key_len: 32, chacha: false, tls13: false },
    Suite { id: 0xc030, hash: Hash::Sha384, key_len: 32, chacha: false, tls13: false },
];

const SIG_SCHEMES: [u16; 9] = [0x0403, 0x0503, 0x0804, 0x0805, 0x0806, 0x0401, 0x0501, 0x0601, 0x0603];

fn scheme_alg(s: u16, tls13: bool) -> SigAlg {
    match s {
        0x0403 => SigAlg::Ecdsa(Hash::Sha256),
        0x0503 => SigAlg::Ecdsa(Hash::Sha384),
        0x0603 => SigAlg::Ecdsa(Hash::Sha512),
        0x0804 => SigAlg::RsaPss(Hash::Sha256),
        0x0805 => SigAlg::RsaPss(Hash::Sha384),
        0x0806 => SigAlg::RsaPss(Hash::Sha512),
        0x0401 if !tls13 => SigAlg::RsaPkcs1(Hash::Sha256),
        0x0501 if !tls13 => SigAlg::RsaPkcs1(Hash::Sha384),
        0x0601 if !tls13 => SigAlg::RsaPkcs1(Hash::Sha512),
        _ => SigAlg::Other,
    }
}

fn alert_text(code: u8) -> &'static str {
    match code {
        40 => "the handshake failed",
        42 | 43 | 44 | 45 | 46 => "a certificate problem",
        47 => "an illegal parameter",
        48 => "an unknown certificate authority",
        50 => "a decoding error",
        51 => "a decryption error",
        70 => "an unsupported protocol version",
        71 => "insufficient security",
        80 => "an internal error",
        112 => "an unrecognised site name",
        116 => "a missing client certificate",
        120 => "no common application protocol",
        _ => "an error",
    }
}

#[derive(Clone)]
enum Aead {
    Aes(Gcm),
    Chacha([u8; 32]),
}

#[derive(Clone)]
struct Dir {
    aead: Aead,
    iv: [u8; 12],
    seq: u64,
    /// TLS 1.2 AES-GCM: a 4-byte salt plus an explicit 8-byte nonce per record
    explicit: bool,
}

impl Dir {
    fn new(suite: Suite, key: &[u8], iv: &[u8], explicit: bool) -> Dir {
        let aead = if suite.chacha {
            let mut k = [0u8; 32];
            k.copy_from_slice(key);
            Aead::Chacha(k)
        } else {
            Aead::Aes(Gcm::new(key))
        };
        let mut v = [0u8; 12];
        v[..iv.len()].copy_from_slice(iv);
        Dir { aead, iv: v, seq: 0, explicit }
    }

    fn nonce(&self) -> [u8; 12] {
        let mut n = self.iv;
        let s = self.seq.to_be_bytes();
        if self.explicit {
            n[4..].copy_from_slice(&s);
        } else {
            for i in 0..8 {
                n[4 + i] ^= s[i];
            }
        }
        n
    }

    fn seal(&self, nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Vec<u8> {
        match &self.aead {
            Aead::Aes(g) => g.seal(nonce, aad, plain),
            Aead::Chacha(k) => crypto::seal(k, nonce, aad, plain),
        }
    }

    fn open(&self, nonce: &[u8; 12], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        match &self.aead {
            Aead::Aes(g) => g.open(nonce, aad, sealed),
            Aead::Chacha(k) => crypto::open(k, nonce, aad, sealed),
        }
    }
}

struct Reader<'a> {
    d: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(d: &'a [u8]) -> Reader<'a> {
        Reader { d, pos: 0 }
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.d.len()).ok_or_else(|| String::from("The site sent a malformed message."))?;
        let s = &self.d[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u24(&mut self) -> Result<usize, String> {
        let b = self.bytes(3)?;
        Ok(((b[0] as usize) << 16) | ((b[1] as usize) << 8) | b[2] as usize)
    }
    fn vec8(&mut self) -> Result<&'a [u8], String> {
        let n = self.u8()? as usize;
        self.bytes(n)
    }
    fn vec16(&mut self) -> Result<&'a [u8], String> {
        let n = self.u16()? as usize;
        self.bytes(n)
    }
    fn vec24(&mut self) -> Result<&'a [u8], String> {
        let n = self.u24()?;
        self.bytes(n)
    }
    fn done(&self) -> bool {
        self.pos >= self.d.len()
    }
}

fn u16b(v: usize) -> [u8; 2] {
    (v as u16).to_be_bytes()
}

fn u24b(v: usize) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn with_len16(body: &[u8]) -> Vec<u8> {
    let mut v = u16b(body.len()).to_vec();
    v.extend_from_slice(body);
    v
}

fn ext(kind: u16, body: &[u8]) -> Vec<u8> {
    let mut v = kind.to_be_bytes().to_vec();
    v.extend(with_len16(body));
    v
}

fn message(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![kind];
    v.extend_from_slice(&u24b(body.len()));
    v.extend_from_slice(body);
    v
}

fn expand_label(h: Hash, secret: &[u8], label: &str, ctx: &[u8], len: usize) -> Vec<u8> {
    let mut info = u16b(len).to_vec();
    info.push(6 + label.len() as u8);
    info.extend_from_slice(b"tls13 ");
    info.extend_from_slice(label.as_bytes());
    info.push(ctx.len() as u8);
    info.extend_from_slice(ctx);
    h.expand(secret, &info, len)
}

/// The TLS 1.2 PRF (P_hash).
fn prf(h: Hash, secret: &[u8], label: &str, seed: &[u8], len: usize) -> Vec<u8> {
    let mut s = label.as_bytes().to_vec();
    s.extend_from_slice(seed);
    let mut a = h.hmac(secret, &[&s]);
    let mut out = Vec::with_capacity(len + 64);
    while out.len() < len {
        out.extend(h.hmac(secret, &[&a, &s]));
        a = h.hmac(secret, &[&a]);
    }
    out.truncate(len);
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    ServerHello,
    // TLS 1.3
    EncryptedExtensions,
    Certificate,
    CertificateVerify,
    Finished,
    // TLS 1.2
    Certificate12,
    KeyExchange12,
    HelloDone12,
    ChangeCipher12,
    Finished12,
    Ready,
}

pub struct Client {
    host: String,
    now: i64,
    roots: Rc<Roots>,
    seed: [u8; 32],
    rng_ctr: u32,
    random: [u8; 32],
    session_id: [u8; 32],
    x25519_secret: [u8; 32],
    /// a P-256 or P-384 key, made if the server asks for one
    ec_secret: Option<(u16, Vec<u8>, Vec<u8>)>,
    offered_group: u16,
    retried: bool,
    state: State,
    suite: Option<Suite>,
    transcript: Vec<u8>,
    rbuf: Vec<u8>,
    hbuf: Vec<u8>,
    out: Vec<u8>,
    app_in: Vec<u8>,
    read: Option<Dir>,
    write: Option<Dir>,
    pending_read: Option<Dir>,
    // TLS 1.3 secrets
    hs_secret: Vec<u8>,
    c_hs: Vec<u8>,
    s_hs: Vec<u8>,
    s_ap: Vec<u8>,
    c_ap: Vec<u8>,
    // TLS 1.2
    server_random: [u8; 32],
    ems: bool,
    /// our ClientKeyExchange body
    cke: Vec<u8>,
    master: Vec<u8>,
    certs: Vec<Vec<u8>>,
    cert_request: Option<Vec<u8>>,
    pub verified: Option<Verified>,
    closed: bool,
    failed: Option<String>,
}

impl Client {
    /// Start a connection to `host` (checked against the certificate) at unix
    /// time `now`. `seed` must be fresh random bytes.
    pub fn new(host: &str, now: i64, roots: Rc<Roots>, seed: [u8; 32]) -> Client {
        let mut c = Client {
            host: host.trim_end_matches('.').to_ascii_lowercase(),
            now,
            roots,
            seed,
            rng_ctr: 0,
            random: [0; 32],
            session_id: [0; 32],
            x25519_secret: [0; 32],
            ec_secret: None,
            offered_group: X25519,
            retried: false,
            state: State::ServerHello,
            suite: None,
            transcript: Vec::new(),
            rbuf: Vec::new(),
            hbuf: Vec::new(),
            out: Vec::new(),
            app_in: Vec::new(),
            read: None,
            write: None,
            pending_read: None,
            hs_secret: Vec::new(),
            c_hs: Vec::new(),
            s_hs: Vec::new(),
            s_ap: Vec::new(),
            c_ap: Vec::new(),
            server_random: [0; 32],
            ems: false,
            cke: Vec::new(),
            master: Vec::new(),
            certs: Vec::new(),
            cert_request: None,
            verified: None,
            closed: false,
            failed: None,
        };
        c.random = c.rand32();
        c.session_id = c.rand32();
        c.x25519_secret = c.rand32();
        let share = x25519::public_key(&c.x25519_secret).to_vec();
        let hello = c.client_hello(X25519, &share, None);
        c.transcript.extend_from_slice(&hello);
        c.send_plain(HANDSHAKE, &hello, 0x0301);
        c
    }

    fn rand32(&mut self) -> [u8; 32] {
        self.rng_ctr += 1;
        let b = crypto::chacha20_block(&self.seed, self.rng_ctr, &[0; 12]);
        let mut out = [0u8; 32];
        out.copy_from_slice(&b[..32]);
        out
    }

    fn client_hello(&self, group: u16, share: &[u8], cookie: Option<&[u8]>) -> Vec<u8> {
        let mut b = vec![3, 3];
        b.extend_from_slice(&self.random);
        b.push(32);
        b.extend_from_slice(&self.session_id);
        let suites: Vec<u8> = SUITES.iter().flat_map(|s| s.id.to_be_bytes()).collect();
        b.extend(with_len16(&suites));
        b.extend_from_slice(&[1, 0]);
        let mut e = Vec::new();
        if x509_is_name(&self.host) {
            let mut name = vec![0u8];
            name.extend(with_len16(self.host.as_bytes()));
            e.extend(ext(0, &with_len16(&name)));
        }
        e.extend(ext(23, &[])); // extended master secret
        e.extend(ext(0xff01, &[0])); // renegotiation info
        let groups: Vec<u8> = [X25519, SECP256R1, SECP384R1].iter().flat_map(|g| g.to_be_bytes()).collect();
        e.extend(ext(10, &with_len16(&groups)));
        e.extend(ext(11, &[1, 0])); // uncompressed points
        let schemes: Vec<u8> = SIG_SCHEMES.iter().flat_map(|s| s.to_be_bytes()).collect();
        e.extend(ext(13, &with_len16(&schemes)));
        e.extend(ext(16, &with_len16(b"\x08http/1.1")));
        e.extend(ext(43, &[4, 3, 4, 3, 3]));
        e.extend(ext(45, &[1, 1])); // psk_dhe_ke
        let mut ks = group.to_be_bytes().to_vec();
        ks.extend(with_len16(share));
        e.extend(ext(51, &with_len16(&ks)));
        if let Some(c) = cookie {
            e.extend(ext(44, &with_len16(c)));
        }
        b.extend(with_len16(&e));
        message(1, &b)
    }

    // ------------------------------------------------------------ records

    fn send_plain(&mut self, kind: u8, data: &[u8], version: u16) {
        for chunk in data.chunks(MAX_RECORD) {
            self.out.push(kind);
            self.out.extend_from_slice(&version.to_be_bytes());
            self.out.extend_from_slice(&u16b(chunk.len()));
            self.out.extend_from_slice(chunk);
        }
    }

    fn send(&mut self, kind: u8, data: &[u8]) {
        let tls13 = self.suite.is_some_and(|s| s.tls13);
        let Some(mut w) = self.write.take() else {
            self.send_plain(kind, data, 0x0303);
            return;
        };
        for chunk in data.chunks(MAX_RECORD) {
            let nonce = w.nonce();
            let record = if tls13 {
                let mut inner = chunk.to_vec();
                inner.push(kind);
                let aad = [APP, 3, 3, ((inner.len() + 16) >> 8) as u8, (inner.len() + 16) as u8];
                let mut r = aad.to_vec();
                r.extend(w.seal(&nonce, &aad, &inner));
                r
            } else {
                let mut aad = w.seq.to_be_bytes().to_vec();
                aad.extend_from_slice(&[kind, 3, 3]);
                aad.extend_from_slice(&u16b(chunk.len()));
                let mut body = if w.explicit { w.seq.to_be_bytes().to_vec() } else { Vec::new() };
                body.extend(w.seal(&nonce, &aad, chunk));
                let mut r = vec![kind, 3, 3];
                r.extend_from_slice(&u16b(body.len()));
                r.extend(body);
                r
            };
            self.out.extend(record);
            w.seq += 1;
        }
        self.write = Some(w);
    }

    /// Bytes from the server. An error means the connection must be closed.
    pub fn feed(&mut self, data: &[u8]) -> Result<(), String> {
        if let Some(e) = &self.failed {
            return Err(e.clone());
        }
        self.rbuf.extend_from_slice(data);
        let r = self.process();
        if let Err(e) = &r {
            self.failed = Some(e.clone());
        }
        r
    }

    fn process(&mut self) -> Result<(), String> {
        while self.rbuf.len() >= 5 {
            let len = u16::from_be_bytes([self.rbuf[3], self.rbuf[4]]) as usize;
            if len > MAX_RECORD + 2048 {
                return Err(String::from("The site sent a record that's too big."));
            }
            if self.rbuf.len() < 5 + len {
                break;
            }
            let rec: Vec<u8> = self.rbuf.drain(..5 + len).collect();
            self.record(rec[0], &rec[..5], &rec[5..])?;
            if self.closed {
                break;
            }
        }
        Ok(())
    }

    fn record(&mut self, kind: u8, header: &[u8], body: &[u8]) -> Result<(), String> {
        let tls13 = self.suite.is_some_and(|s| s.tls13);
        if kind == CCS {
            if tls13 || body != [1] {
                return Ok(()); // TLS 1.3 sends it only for old middleboxes
            }
            if self.state != State::ChangeCipher12 {
                return Err(String::from("The site changed ciphers at the wrong moment."));
            }
            self.read = self.pending_read.take();
            self.state = State::Finished12;
            return Ok(());
        }
        let (kind, plain) = match self.read.take() {
            None => (kind, body.to_vec()),
            Some(mut r) => {
                let nonce;
                let opened = if tls13 {
                    if kind != APP {
                        return Err(String::from("The site sent an unencrypted record after the handshake."));
                    }
                    nonce = r.nonce();
                    r.open(&nonce, header, body)
                } else if r.explicit {
                    if body.len() < 8 {
                        return Err(String::from("The site sent a malformed record."));
                    }
                    let mut n = r.iv;
                    n[4..].copy_from_slice(&body[..8]);
                    let mut aad = r.seq.to_be_bytes().to_vec();
                    aad.extend_from_slice(&[kind, 3, 3]);
                    aad.extend_from_slice(&u16b(body.len().saturating_sub(24)));
                    r.open(&n, &aad, &body[8..])
                } else {
                    nonce = r.nonce();
                    let mut aad = r.seq.to_be_bytes().to_vec();
                    aad.extend_from_slice(&[kind, 3, 3]);
                    aad.extend_from_slice(&u16b(body.len().saturating_sub(16)));
                    r.open(&nonce, &aad, body)
                };
                let Some(mut p) = opened else {
                    return Err(String::from("A record from the site failed its integrity check."));
                };
                r.seq += 1;
                self.read = Some(r);
                if tls13 {
                    while p.last() == Some(&0) {
                        p.pop();
                    }
                    let Some(k) = p.pop() else { return Err(String::from("The site sent an empty record.")) };
                    (k, p)
                } else {
                    (kind, p)
                }
            }
        };
        match kind {
            ALERT => {
                if plain.len() >= 2 && plain[1] == 0 {
                    self.closed = true;
                    return Ok(());
                }
                let code = plain.get(1).copied().unwrap_or(0);
                Err(format!("The site ended the secure connection ({}, alert {}).", alert_text(code), code))
            }
            HANDSHAKE => {
                self.hbuf.extend_from_slice(&plain);
                while self.hbuf.len() >= 4 {
                    let n = ((self.hbuf[1] as usize) << 16) | ((self.hbuf[2] as usize) << 8) | self.hbuf[3] as usize;
                    if n > 1 << 17 {
                        return Err(String::from("The site sent a handshake message that's too big."));
                    }
                    if self.hbuf.len() < 4 + n {
                        break;
                    }
                    let raw: Vec<u8> = self.hbuf.drain(..4 + n).collect();
                    self.handshake(raw[0], &raw)?;
                }
                Ok(())
            }
            APP => {
                if self.state != State::Ready {
                    return Err(String::from("The site sent data before the handshake finished."));
                }
                self.app_in.extend_from_slice(&plain);
                Ok(())
            }
            _ => Err(format!("The site sent an unknown record type ({}).", kind)),
        }
    }

    fn th(&self) -> Vec<u8> {
        self.suite.map_or(Hash::Sha256, |s| s.hash).digest(&self.transcript)
    }

    fn unexpected(&self, kind: u8) -> String {
        format!("The site sent an unexpected handshake message ({} while waiting for {:?}).", kind, self.state)
    }

    fn handshake(&mut self, kind: u8, raw: &[u8]) -> Result<(), String> {
        let body = &raw[4..];
        let tls13 = self.suite.is_some_and(|s| s.tls13);
        match (self.state, kind) {
            (State::ServerHello, 2) => self.server_hello(raw),
            (State::EncryptedExtensions, 8) => {
                self.transcript.extend_from_slice(raw);
                self.state = State::Certificate;
                Ok(())
            }
            (State::Certificate, 13) | (State::HelloDone12, 13) => {
                let mut r = Reader::new(body);
                self.cert_request = Some(if tls13 { r.vec8()?.to_vec() } else { Vec::new() });
                self.transcript.extend_from_slice(raw);
                Ok(())
            }
            (State::Certificate, 11) | (State::Certificate12, 11) => {
                let mut r = Reader::new(body);
                if tls13 {
                    r.vec8()?;
                }
                let mut list = Reader::new(r.vec24()?);
                while !list.done() {
                    self.certs.push(list.vec24()?.to_vec());
                    if tls13 {
                        list.vec16()?;
                    }
                }
                self.verified = Some(x509::verify(&self.certs, &self.host, self.now, &self.roots)?);
                self.transcript.extend_from_slice(raw);
                self.state = if tls13 { State::CertificateVerify } else { State::KeyExchange12 };
                Ok(())
            }
            (State::CertificateVerify, 15) => {
                let mut r = Reader::new(body);
                let scheme = r.u16()?;
                let sig = r.vec16()?;
                let mut signed = vec![0x20u8; 64];
                signed.extend_from_slice(b"TLS 1.3, server CertificateVerify\0");
                signed.extend(self.th());
                self.check_signature(scheme, true, &signed, sig)?;
                self.transcript.extend_from_slice(raw);
                self.state = State::Finished;
                Ok(())
            }
            (State::Finished, 20) => self.finished13(raw),
            (State::KeyExchange12, 12) => self.key_exchange12(raw),
            (State::HelloDone12, 14) => self.hello_done12(raw),
            (State::Finished12, 20) => {
                let s = self.suite.unwrap_or(SUITES[0]);
                let want = prf(s.hash, &self.master, "server finished", &self.th(), 12);
                if body != &want[..] {
                    return Err(String::from("The site's handshake didn't check out (Finished)."));
                }
                self.transcript.clear();
                self.state = State::Ready;
                Ok(())
            }
            (State::Ready, 4) => Ok(()), // session tickets: not kept
            (State::Ready, 0) if !tls13 => Ok(()), // renegotiation: declined by ignoring
            (State::Ready, 24) if tls13 => self.key_update(body),
            (State::ChangeCipher12, 4) => Ok(()),
            _ => Err(self.unexpected(kind)),
        }
    }

    fn check_signature(&self, scheme: u16, tls13: bool, msg: &[u8], sig: &[u8]) -> Result<(), String> {
        let leaf = self.certs.first().and_then(|c| x509::parse(c)).ok_or_else(|| String::from("The site's certificate is missing."))?;
        let alg = scheme_alg(scheme, tls13);
        if alg == SigAlg::Other || !x509::check(&leaf.key, alg, msg, sig) {
            return Err(String::from("The site couldn't prove it owns its certificate (bad signature)."));
        }
        Ok(())
    }

    fn server_hello(&mut self, raw: &[u8]) -> Result<(), String> {
        let mut r = Reader::new(&raw[4..]);
        let legacy = r.u16()?;
        let random = r.bytes(32)?;
        let echo = r.vec8()?.to_vec();
        let suite_id = r.u16()?;
        r.u8()?;
        let mut version = legacy;
        let mut share: Option<(u16, Vec<u8>)> = None;
        let mut hrr_group: Option<u16> = None;
        let mut cookie: Option<Vec<u8>> = None;
        let mut ems = false;
        let hrr = random == HRR_RANDOM;
        if !r.done() {
            let mut e = Reader::new(r.vec16()?);
            while !e.done() {
                let kind = e.u16()?;
                let mut b = Reader::new(e.vec16()?);
                match kind {
                    43 => version = b.u16()?,
                    51 if hrr => hrr_group = Some(b.u16()?),
                    51 => {
                        let g = b.u16()?;
                        share = Some((g, b.vec16()?.to_vec()));
                    }
                    44 => cookie = Some(b.vec16()?.to_vec()),
                    23 => ems = true,
                    _ => {}
                }
            }
        }
        let suite = SUITES.iter().copied().find(|s| s.id == suite_id).ok_or_else(|| String::from("The site chose a cipher HydatekOS didn't offer."))?;
        if version == 0x0304 {
            if !suite.tls13 || echo != self.session_id {
                return Err(String::from("The site's TLS 1.3 hello didn't match what HydatekOS sent."));
            }
            self.suite = Some(suite);
            if hrr {
                if self.retried {
                    return Err(String::from("The site asked HydatekOS to retry the handshake twice."));
                }
                self.retried = true;
                let group = hrr_group.ok_or_else(|| String::from("The site asked for a retry without saying why."))?;
                let share = match group {
                    SECP256R1 | SECP384R1 => self.ec_key(group).1,
                    _ => return Err(String::from("The site wants a key exchange HydatekOS doesn't support.")),
                };
                self.offered_group = group;
                // the transcript restarts with a hash of the first hello
                let h = suite.hash.digest(&self.transcript);
                let mut t = message(254, &h);
                t.extend_from_slice(raw);
                let hello = self.client_hello(group, &share, cookie.as_deref());
                t.extend_from_slice(&hello);
                self.transcript = t;
                self.send_plain(HANDSHAKE, &hello, 0x0303);
                return Ok(());
            }
            let (group, key) = share.ok_or_else(|| String::from("The site didn't send its key."))?;
            if group != self.offered_group {
                return Err(String::from("The site used a key exchange HydatekOS didn't offer."));
            }
            let shared = self.agree(group, &key)?;
            self.transcript.extend_from_slice(raw);
            let h = suite.hash;
            let zero = vec![0u8; h.len()];
            let early = h.extract(&zero, &zero);
            let derived = expand_label(h, &early, "derived", &h.digest(&[]), h.len());
            self.hs_secret = h.extract(&derived, &shared);
            let th = self.th();
            self.c_hs = expand_label(h, &self.hs_secret, "c hs traffic", &th, h.len());
            self.s_hs = expand_label(h, &self.hs_secret, "s hs traffic", &th, h.len());
            self.read = Some(self.traffic(&self.s_hs.clone()));
            self.state = State::EncryptedExtensions;
            Ok(())
        } else if version == 0x0303 && legacy == 0x0303 {
            if hrr || suite.tls13 {
                return Err(String::from("The site's hello didn't match what HydatekOS sent."));
            }
            if random[24..] == *b"DOWNGRD\x01" {
                return Err(String::from("Someone tried to force an older, weaker version of TLS."));
            }
            self.suite = Some(suite);
            self.server_random.copy_from_slice(random);
            self.ems = ems;
            self.transcript.extend_from_slice(raw);
            self.state = State::Certificate12;
            Ok(())
        } else {
            Err(String::from("The site only offers old versions of TLS, which aren't safe."))
        }
    }

    fn ec_key(&mut self, group: u16) -> (Vec<u8>, Vec<u8>) {
        if let Some((g, s, p)) = &self.ec_secret {
            if *g == group {
                return (s.clone(), p.clone());
            }
        }
        let curve = if group == SECP384R1 { Curve::P384 } else { Curve::P256 };
        let mut random = self.rand32().to_vec();
        random.extend(self.rand32());
        random.extend(self.rand32());
        let (s, p) = Group::new(curve).keypair(&random);
        self.ec_secret = Some((group, s.clone(), p.clone()));
        (s, p)
    }

    fn agree(&mut self, group: u16, peer: &[u8]) -> Result<Vec<u8>, String> {
        let bad = || String::from("The site sent a bad key.");
        match group {
            X25519 => {
                let p: [u8; 32] = peer.try_into().map_err(|_| bad())?;
                let s = x25519::x25519(&self.x25519_secret, &p);
                if s == [0; 32] {
                    return Err(bad());
                }
                Ok(s.to_vec())
            }
            SECP256R1 | SECP384R1 => {
                let (secret, _) = self.ec_key(group);
                let curve = if group == SECP384R1 { Curve::P384 } else { Curve::P256 };
                Group::new(curve).shared(&secret, peer).ok_or_else(bad)
            }
            _ => Err(String::from("The site wants a key exchange HydatekOS doesn't support.")),
        }
    }

    fn traffic(&self, secret: &[u8]) -> Dir {
        let s = self.suite.unwrap_or(SUITES[0]);
        let key = expand_label(s.hash, secret, "key", &[], s.key_len);
        let iv = expand_label(s.hash, secret, "iv", &[], 12);
        Dir::new(s, &key, &iv, false)
    }

    fn finished13(&mut self, raw: &[u8]) -> Result<(), String> {
        let h = self.suite.unwrap_or(SUITES[0]).hash;
        let key = expand_label(h, &self.s_hs, "finished", &[], h.len());
        if raw[4..] != h.hmac(&key, &[&self.th()])[..] {
            return Err(String::from("The site's handshake didn't check out (Finished)."));
        }
        self.transcript.extend_from_slice(raw);
        let zero = vec![0u8; h.len()];
        let derived = expand_label(h, &self.hs_secret, "derived", &h.digest(&[]), h.len());
        let master = h.extract(&derived, &zero);
        let th = self.th();
        self.c_ap = expand_label(h, &master, "c ap traffic", &th, h.len());
        self.s_ap = expand_label(h, &master, "s ap traffic", &th, h.len());
        // middlebox compatibility, then our Finished under the handshake keys
        self.send_plain(CCS, &[1], 0x0303);
        self.write = Some(self.traffic(&self.c_hs.clone()));
        if let Some(ctx) = self.cert_request.take() {
            let mut b = vec![ctx.len() as u8];
            b.extend_from_slice(&ctx);
            b.extend_from_slice(&[0, 0, 0]);
            let m = message(11, &b);
            self.transcript.extend_from_slice(&m);
            self.send(HANDSHAKE, &m);
        }
        let key = expand_label(h, &self.c_hs, "finished", &[], h.len());
        let fin = message(20, &h.hmac(&key, &[&self.th()]));
        self.send(HANDSHAKE, &fin);
        self.write = Some(self.traffic(&self.c_ap.clone()));
        self.read = Some(self.traffic(&self.s_ap.clone()));
        self.transcript.clear();
        self.state = State::Ready;
        Ok(())
    }

    fn key_update(&mut self, body: &[u8]) -> Result<(), String> {
        let h = self.suite.unwrap_or(SUITES[0]).hash;
        self.s_ap = expand_label(h, &self.s_ap, "traffic upd", &[], h.len());
        self.read = Some(self.traffic(&self.s_ap.clone()));
        if body.first() == Some(&1) {
            self.send(HANDSHAKE, &message(24, &[0]));
            self.c_ap = expand_label(h, &self.c_ap, "traffic upd", &[], h.len());
            self.write = Some(self.traffic(&self.c_ap.clone()));
        }
        Ok(())
    }

    fn key_exchange12(&mut self, raw: &[u8]) -> Result<(), String> {
        let mut r = Reader::new(&raw[4..]);
        if r.u8()? != 3 {
            return Err(String::from("The site wants a key exchange HydatekOS doesn't support."));
        }
        let group = r.u16()?;
        let point = r.vec8()?.to_vec();
        let params_len = r.pos;
        let scheme = r.u16()?;
        let sig = r.vec16()?.to_vec();
        let mut signed = self.random.to_vec();
        signed.extend_from_slice(&self.server_random);
        signed.extend_from_slice(&raw[4..4 + params_len]);
        self.check_signature(scheme, false, &signed, &sig)?;
        self.transcript.extend_from_slice(raw);
        let public = match group {
            X25519 => x25519::public_key(&self.x25519_secret).to_vec(),
            SECP256R1 | SECP384R1 => self.ec_key(group).1,
            _ => return Err(String::from("The site wants a key exchange HydatekOS doesn't support.")),
        };
        let pms = self.agree(group, &point)?;
        self.master = pms; // the pre-master secret until ServerHelloDone
        self.offered_group = group;
        self.cke = vec![public.len() as u8];
        self.cke.extend(public);
        self.state = State::HelloDone12;
        Ok(())
    }

    fn hello_done12(&mut self, raw: &[u8]) -> Result<(), String> {
        self.transcript.extend_from_slice(raw);
        let s = self.suite.unwrap_or(SUITES[3]);
        if self.cert_request.take().is_some() {
            let m = message(11, &[0, 0, 0]);
            self.transcript.extend_from_slice(&m);
            self.send_plain(HANDSHAKE, &m, 0x0303);
        }
        let cke = message(16, &core::mem::take(&mut self.cke));
        self.transcript.extend_from_slice(&cke);
        self.send_plain(HANDSHAKE, &cke, 0x0303);
        let pms = core::mem::take(&mut self.master);
        self.master = if self.ems {
            prf(s.hash, &pms, "extended master secret", &self.th(), 48)
        } else {
            let mut seed = self.random.to_vec();
            seed.extend_from_slice(&self.server_random);
            prf(s.hash, &pms, "master secret", &seed, 48)
        };
        let iv_len = if s.chacha { 12 } else { 4 };
        let mut seed = self.server_random.to_vec();
        seed.extend_from_slice(&self.random);
        let kb = prf(s.hash, &self.master, "key expansion", &seed, 2 * s.key_len + 2 * iv_len);
        let (ck, rest) = kb.split_at(s.key_len);
        let (sk, rest) = rest.split_at(s.key_len);
        let (civ, siv) = rest.split_at(iv_len);
        self.send_plain(CCS, &[1], 0x0303);
        self.write = Some(Dir::new(s, ck, civ, !s.chacha));
        self.pending_read = Some(Dir::new(s, sk, siv, !s.chacha));
        let fin = message(20, &prf(s.hash, &self.master, "client finished", &self.th(), 12));
        self.transcript.extend_from_slice(&fin);
        self.send(HANDSHAKE, &fin);
        self.state = State::ChangeCipher12;
        Ok(())
    }

    // ------------------------------------------------------------ for callers

    pub fn ready(&self) -> bool {
        self.state == State::Ready
    }

    /// The server closed the connection cleanly.
    pub fn closed(&self) -> bool {
        self.closed
    }

    /// Encrypt application data (after `ready`).
    pub fn write(&mut self, data: &[u8]) {
        if self.ready() {
            self.send(APP, data);
        }
    }

    /// Decrypted application data received so far.
    pub fn read(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.app_in)
    }

    /// Bytes to send to the server.
    pub fn take_output(&mut self) -> Vec<u8> {
        core::mem::take(&mut self.out)
    }

    /// Say goodbye (close_notify).
    pub fn close(&mut self) {
        if self.write.is_some() {
            self.send(ALERT, &[1, 0]);
        }
    }

    /// For the user: protocol and cipher.
    pub fn summary(&self) -> String {
        let Some(s) = self.suite else { return String::new() };
        let cipher = if s.chacha {
            "ChaCha20-Poly1305"
        } else if s.key_len == 16 {
            "AES-128-GCM"
        } else {
            "AES-256-GCM"
        };
        let kx = match self.offered_group {
            SECP256R1 => "P-256",
            SECP384R1 => "P-384",
            _ => "X25519",
        };
        format!("{}, {}, {}", if s.tls13 { "TLS 1.3" } else { "TLS 1.2" }, kx, cipher)
    }
}

/// Server names go in the hello; IP addresses don't.
fn x509_is_name(host: &str) -> bool {
    !host.is_empty() && !host.chars().all(|c| c.is_ascii_digit() || c == '.') && !host.contains(':')
}
