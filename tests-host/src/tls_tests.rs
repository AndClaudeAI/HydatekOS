use crate::tls::sha2::Hash;
use crate::tls::*;
use crate::tls_vectors::*;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn sha384_512() {
    for (m, h384, h512) in SHA {
        let m = unhex(m);
        assert_eq!(Hash::Sha384.digest(&m), unhex(h384));
        assert_eq!(Hash::Sha512.digest(&m), unhex(h512));
        // in pieces
        let mut h = Hash::Sha384.start();
        for c in m.chunks(7) {
            h.update(c);
        }
        assert_eq!(h.finish(), unhex(h384));
    }
    // RFC 4231 test case 2
    assert_eq!(
        Hash::Sha384.hmac(b"Jefe", &[b"what do ya want ", b"for nothing?"]),
        unhex("af45d2e376484031617f78d2b58a6b1b9c7ef464f5a01b47e42ec3736322445e8e2240ca5e69e2c78b3239ecfab21649")
    );
    assert_eq!(Hash::Sha256.hmac(b"Jefe", &[b"what do ya want for nothing?"]), unhex("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"));
}

#[test]
fn aes_gcm() {
    // FIPS 197 C.1 / C.3
    let a = aes::Aes::new(&unhex("000102030405060708090a0b0c0d0e0f"));
    let mut b: [u8; 16] = unhex("00112233445566778899aabbccddeeff").try_into().unwrap();
    a.encrypt(&mut b);
    assert_eq!(b.to_vec(), unhex("69c4e0d86a7b0430d8cdb78070b4c55a"));
    let a = aes::Aes::new(&unhex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"));
    let mut b: [u8; 16] = unhex("00112233445566778899aabbccddeeff").try_into().unwrap();
    a.encrypt(&mut b);
    assert_eq!(b.to_vec(), unhex("8ea2b7ca516745bfeafc49904b496089"));
    for (k, n, ad, p, c) in GCM {
        let g = aes::Gcm::new(&unhex(k));
        let n: [u8; 12] = unhex(n).try_into().unwrap();
        assert_eq!(g.seal(&n, &unhex(ad), &unhex(p)), unhex(c));
        assert_eq!(g.open(&n, &unhex(ad), &unhex(c)), Some(unhex(p)));
        let mut bad = unhex(c);
        bad[0] ^= 1;
        assert_eq!(g.open(&n, &unhex(ad), &bad), None);
    }
}

#[test]
fn x25519_vectors() {
    let k: [u8; 32] = unhex("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4").try_into().unwrap();
    let u: [u8; 32] = unhex("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c").try_into().unwrap();
    assert_eq!(x25519::x25519(&k, &u).to_vec(), unhex("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"));
    let a: [u8; 32] = unhex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a").try_into().unwrap();
    assert_eq!(x25519::public_key(&a).to_vec(), unhex("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"));
    for (a, bp, shared) in X25519 {
        let a: [u8; 32] = unhex(a).try_into().unwrap();
        let bp: [u8; 32] = unhex(bp).try_into().unwrap();
        assert_eq!(x25519::x25519(&a, &bp).to_vec(), unhex(shared));
    }
}

#[test]
fn ecdsa_vectors() {
    let g256 = ec::Group::new(ec::Curve::P256);
    let g384 = ec::Group::new(ec::Curve::P384);
    for (bits, public, digest, r, s, ok) in ECDSA {
        let g = if *bits == 256 { &g256 } else { &g384 };
        assert_eq!(g.verify(&unhex(public), &unhex(digest), &unhex(r), &unhex(s)), *ok, "P-{} {}", bits, digest);
    }
}

#[test]
fn ecdh_agrees() {
    for c in [ec::Curve::P256, ec::Curve::P384] {
        let g = ec::Group::new(c);
        let (sa, pa) = g.keypair(&[7u8; 96]);
        let (sb, pb) = g.keypair(&[9u8; 96]);
        assert_eq!(g.shared(&sa, &pb), g.shared(&sb, &pa));
        assert!(g.shared(&sa, &[4u8; 65]).is_none());
    }
}

#[test]
fn rsa_vectors() {
    for (n, e, digest, sig, pss, ok) in RSA {
        let d = unhex(digest);
        let h = match d.len() {
            32 => Hash::Sha256,
            48 => Hash::Sha384,
            _ => Hash::Sha512,
        };
        let f = if *pss { rsa::verify_pss } else { rsa::verify_pkcs1 };
        assert_eq!(f(&unhex(n), &unhex(e), h, &d, &unhex(sig)), *ok);
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/fixtures/tls/{}", env!("CARGO_MANIFEST_DIR"), name)).unwrap()
}

fn pem_certs(pem: &str) -> Vec<Vec<u8>> {
    pem.split("-----BEGIN CERTIFICATE-----")
        .skip(1)
        .map(|b| crate::crypto::base64_decode(&b.split("-----END").next().unwrap().replace(['\n', '\r'], "")).unwrap())
        .collect()
}

// 2026-01-01
const NOW: i64 = 1_767_225_600;

#[test]
fn mozilla_roots_parse_and_self_verify() {
    let mut checked = 0;
    let mut total = 0;
    for e in std::fs::read_dir("/usr/share/ca-certificates/mozilla").unwrap() {
        let pem = std::fs::read_to_string(e.unwrap().path()).unwrap();
        for der in pem_certs(&pem) {
            total += 1;
            let c = x509::parse(&der).expect("parse root");
            assert!(c.key != x509::Key::Other, "key of {}", x509::display_name(&c.subject));
            if c.subject == c.issuer && c.sig_alg != x509::SigAlg::Other {
                assert!(x509::check(&c.key, c.sig_alg, &c.tbs, &c.sig), "self-signature of {}", x509::display_name(&c.subject));
                checked += 1;
            }
        }
    }
    let roots = x509::Roots::builtin();
    assert_eq!(roots.anchors.len(), total);
    assert!(roots.anchors.iter().all(|a| a.key != x509::Key::Other));
    assert!(checked > 110, "checked {} of {}", checked, total);
}

#[test]
fn chains() {
    let mut roots = x509::Roots::builtin();
    assert!(roots.add(&fixture("root.der")));
    let chain = |name: &str| pem_certs(&String::from_utf8(fixture(name)).unwrap());
    let v = x509::verify(&chain("ec256-chain.pem"), "localhost", NOW, &roots).unwrap();
    assert_eq!(v.issuer, "HydatekOS Test");
    assert!(x509::verify(&chain("ec256-chain.pem"), "10.0.2.2", NOW, &roots).is_ok());
    assert!(x509::verify(&chain("ec256-chain.pem"), "shop.test.hydatek", NOW, &roots).is_ok());
    assert!(x509::verify(&chain("ec256-chain.pem"), "a.shop.test.hydatek", NOW, &roots).unwrap_err().contains("not a.shop.test.hydatek"));
    assert!(x509::verify(&chain("rsa2048.pem"), "127.0.0.1", NOW, &roots).is_ok());
    assert!(x509::verify(&chain("ec384.pem"), "LOCALHOST.", NOW, &roots).is_ok());
    // no intermediate sent
    assert!(x509::verify(&chain("ec256.pem"), "localhost", NOW, &roots).unwrap_err().contains("doesn't trust"));
    assert!(x509::verify(&chain("expired-chain.pem"), "localhost", NOW, &roots).unwrap_err().contains("expired on 31 December 2020"));
    assert!(x509::verify(&chain("ec256-chain.pem"), "localhost", 1_600_000_000, &roots).unwrap_err().contains("isn't valid until"));
    assert!(x509::verify(&chain("stranger.pem"), "localhost", NOW, &roots).unwrap_err().contains("doesn't trust"));
    assert!(x509::verify(&chain("ec256-chain.pem"), "example.com", NOW, &roots).unwrap_err().contains("is for localhost"));
    // without the test root nothing is trusted
    let builtin = x509::Roots::builtin();
    assert!(x509::verify(&chain("ec256-chain.pem"), "localhost", NOW, &builtin).is_err());
    // a self-signed certificate the user chose to trust
    roots.add(&fixture("stranger.der"));
    assert!(x509::verify(&chain("stranger.pem"), "localhost", NOW, &roots).is_ok());
    assert_eq!(x509::date(0), "1 January 1970");
    assert_eq!(x509::days_from_civil(2024, 2, 29), 19782);
}

// ---------------------------------------------------------------- handshakes against OpenSSL

use std::io::{Read as _, Write as _};
use std::sync::atomic::{AtomicU16, Ordering};

static PORT: AtomicU16 = AtomicU16::new(0);

struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn test_roots() -> std::rc::Rc<x509::Roots> {
    let mut r = x509::Roots::builtin();
    r.add(&fixture("root.der"));
    std::rc::Rc::new(r)
}

/// Run `openssl s_server` with `args`, fetch `path` from it as `host`.
fn fetch_with(args: &[&str], host: &str, path: &str, roots: std::rc::Rc<x509::Roots>) -> Result<(String, String), String> {
    let base = 20000 + (std::process::id() % 20000) as u16;
    let port = base + PORT.fetch_add(1, Ordering::SeqCst);
    let dir = format!("{}/fixtures/tls", env!("CARGO_MANIFEST_DIR"));
    let accept = format!("127.0.0.1:{}", port);
    let mut cmd = std::process::Command::new("openssl");
    cmd.current_dir(&dir).args(["s_server", "-accept", &accept, "-naccept", "1", "-quiet"]).args(args);
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let _server = Server(cmd.spawn().expect("openssl"));
    let mut sock = None;
    for _ in 0..100 {
        if let Ok(s) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            sock = Some(s);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let mut sock = sock.expect("connect to s_server");
    sock.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut c = client::Client::new(host, NOW, roots, [5u8; 32]);
    let mut sent = false;
    let mut buf = vec![0u8; 8192];
    let mut body = Vec::new();
    loop {
        let out = c.take_output();
        if !out.is_empty() {
            sock.write_all(&out).unwrap();
        }
        if c.ready() && !sent {
            c.write(format!("GET {} HTTP/1.0\r\n\r\n", path).as_bytes());
            sent = true;
            continue;
        }
        let n = match sock.read(&mut buf) {
            Ok(n) => n,
            Err(e) => return Err(format!("socket: {}", e)),
        };
        if n == 0 {
            break;
        }
        c.feed(&buf[..n])?;
        body.extend(c.read());
        if c.closed() {
            break;
        }
    }
    Ok((c.summary(), String::from_utf8_lossy(&body).into_owned()))
}

fn fetch(args: &[&str]) -> Result<(String, String), String> {
    fetch_with(args, "localhost", "/", test_roots())
}

const EC256: [&str; 6] = ["-cert", "ec256.pem", "-key", "ec256.key", "-cert_chain", "inter.pem"];
const RSAK: [&str; 4] = ["-cert", "rsa2048.pem", "-key", "rsa2048.key"];
const EC384: [&str; 4] = ["-cert", "ec384.pem", "-key", "ec384.key"];

fn args(a: &[&[&str]]) -> Vec<&'static str> {
    a.iter().flat_map(|x| x.iter()).map(|s| &*Box::leak(s.to_string().into_boxed_str())).collect()
}

#[test]
fn tls13_suites_and_keys() {
    for (suite, name) in [("TLS_AES_128_GCM_SHA256", "AES-128-GCM"), ("TLS_AES_256_GCM_SHA384", "AES-256-GCM"), ("TLS_CHACHA20_POLY1305_SHA256", "ChaCha20-Poly1305")] {
        for key in [&EC256[..], &RSAK[..], &EC384[..]] {
            let (summary, body) = fetch(&args(&[key, &["-www", "-tls1_3", "-ciphersuites", suite]])).unwrap_or_else(|e| panic!("{} {:?}: {}", suite, key[1], e));
            assert_eq!(summary, format!("TLS 1.3, X25519, {}", name));
            assert!(body.starts_with("HTTP/1.0 200 ok"), "{}", body);
            assert!(body.contains(suite), "{}", body);
        }
    }
}

#[test]
fn tls13_hello_retry() {
    for (group, name) in [("P-256", "P-256"), ("P-384", "P-384")] {
        let (summary, body) = fetch(&args(&[&EC256, &["-www", "-tls1_3", "-groups", group]])).unwrap();
        assert!(summary.contains(name), "{}", summary);
        assert!(body.starts_with("HTTP/1.0 200 ok"));
    }
}

#[test]
fn tls12_suites() {
    for (key, cipher, group) in [
        (&EC256[..], "ECDHE-ECDSA-AES128-GCM-SHA256", "X25519"),
        (&EC256[..], "ECDHE-ECDSA-AES256-GCM-SHA384", "P-256"),
        (&EC256[..], "ECDHE-ECDSA-CHACHA20-POLY1305", "P-384"),
        (&RSAK[..], "ECDHE-RSA-AES128-GCM-SHA256", "X25519"),
        (&RSAK[..], "ECDHE-RSA-AES256-GCM-SHA384", "P-256"),
        (&RSAK[..], "ECDHE-RSA-CHACHA20-POLY1305", "X25519"),
    ] {
        let a = args(&[key, &["-www", "-tls1_2", "-cipher", cipher, "-groups", group]]);
        let (summary, body) = fetch(&a).unwrap_or_else(|e| panic!("{}: {}", cipher, e));
        assert!(summary.starts_with("TLS 1.2"), "{}", summary);
        assert!(body.starts_with("HTTP/1.0 200 ok") && body.contains(cipher), "{}", body);
    }
    // RSA PKCS#1 signatures on the key exchange
    let a = args(&[&RSAK, &["-www", "-tls1_2", "-sigalgs", "RSA+SHA256"]]);
    assert!(fetch(&a).unwrap().1.starts_with("HTTP/1.0 200 ok"));
}

#[test]
fn client_certificate_requests() {
    for v in ["-tls1_3", "-tls1_2"] {
        let (_, body) = fetch(&args(&[&EC256, &["-www", v, "-verify", "1"]])).unwrap();
        assert!(body.starts_with("HTTP/1.0 200 ok"), "{}", v);
    }
}

#[test]
fn big_download() {
    let dir = format!("{}/fixtures/tls", env!("CARGO_MANIFEST_DIR"));
    let data: String = (0..40000).map(|i| format!("{} ", i)).collect();
    std::fs::write(format!("{}/big.txt", dir), &data).unwrap();
    for v in ["-tls1_3", "-tls1_2"] {
        let (_, body) = fetch_with(&args(&[&EC256, &["-WWW", v]]), "localhost", "/big.txt", test_roots()).unwrap();
        assert!(body.ends_with(&data), "{} got {} bytes", v, body.len());
    }
    std::fs::remove_file(format!("{}/big.txt", dir)).unwrap();
}

#[test]
fn refuses_bad_certificates() {
    let e = fetch_with(&args(&[&EC256, &["-www"]]), "example.com", "/", test_roots()).unwrap_err();
    assert!(e.contains("is for localhost"), "{}", e);
    let e = fetch(&args(&[&["-cert", "stranger.pem", "-key", "stranger.key"], &["-www"]])).unwrap_err();
    assert!(e.contains("doesn't trust"), "{}", e);
    let e = fetch(&args(&[&["-cert", "expired.pem", "-key", "expired.key", "-cert_chain", "inter.pem"], &["-www"]])).unwrap_err();
    assert!(e.contains("expired"), "{}", e);
    // the test authority isn't in the built-in list
    let e = fetch_with(&args(&[&EC256, &["-www"]]), "localhost", "/", std::rc::Rc::new(x509::Roots::builtin())).unwrap_err();
    assert!(e.contains("doesn't trust"), "{}", e);
    // TLS 1.1 only
    let e = fetch(&args(&[&EC256, &["-www", "-tls1_1", "-cipher", "ALL:@SECLEVEL=0"]])).unwrap_err();
    assert!(e.contains("old versions") || e.contains("alert"), "{}", e);
}

/// A real site: `HYDATEK_LIVE=host cargo test --release live -- --ignored`
/// (`HYDATEK_EXTRA_CA=file.der` trusts one more authority, as a user can).
#[test]
#[ignore]
fn live_site() {
    use std::net::ToSocketAddrs;
    let host = std::env::var("HYDATEK_LIVE").unwrap_or_else(|_| String::from("example.com"));
    let mut roots = x509::Roots::builtin();
    if let Ok(ca) = std::env::var("HYDATEK_EXTRA_CA") {
        assert!(roots.add(&std::fs::read(ca).unwrap()));
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let addr = (host.as_str(), 443).to_socket_addrs().unwrap().next().unwrap();
    let mut sock = std::net::TcpStream::connect(addr).unwrap();
    sock.set_read_timeout(Some(std::time::Duration::from_secs(15))).unwrap();
    let mut c = client::Client::new(&host, now, std::rc::Rc::new(roots), [3u8; 32]);
    let mut sent = false;
    let mut buf = vec![0u8; 16384];
    let mut body = Vec::new();
    loop {
        let out = c.take_output();
        if !out.is_empty() {
            sock.write_all(&out).unwrap();
        }
        if c.ready() && !sent {
            c.write(format!("GET / HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: HydatekOS\r\n\r\n", host).as_bytes());
            sent = true;
            continue;
        }
        let n = sock.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        if let Err(e) = c.feed(&buf[..n]) {
            panic!("{}", e);
        }
        body.extend(c.read());
        if c.closed() {
            break;
        }
    }
    let text = String::from_utf8_lossy(&body);
    println!("{} | verified by {:?}\n{}", c.summary(), c.verified.as_ref().map(|v| &v.issuer), &text[..text.len().min(300)]);
    assert!(text.starts_with("HTTP/1.1 "));
}
