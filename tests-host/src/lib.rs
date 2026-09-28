extern crate alloc;

#[path = "../../kernel/src/crypto.rs"]
pub mod crypto;
#[path = "../../kernel/src/qr.rs"]
pub mod qr;
#[path = "../../kernel/src/hlp.rs"]
pub mod hlp;
#[path = "../../kernel/src/zip.rs"]
pub mod zip;
#[path = "../../kernel/src/doc.rs"]
pub mod doc;
#[path = "../../kernel/src/deck.rs"]
pub mod deck;
#[path = "../../kernel/src/deckio.rs"]
pub mod deckio;
#[path = "../../kernel/src/grid.rs"]
pub mod grid;
#[path = "../../kernel/src/gridio.rs"]
pub mod gridio;
#[path = "../../kernel/src/image/mod.rs"]
pub mod image;
#[path = "../../kernel/src/tls/mod.rs"]
pub mod tls;
#[path = "../../kernel/src/web/mod.rs"]
pub mod web;
/// Text measurement stand-in for the kernel's font pack.
pub mod font {
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub enum Face {
        Regular,
        Medium,
        Semibold,
        Display,
        Mono,
        Italic,
        SemiboldItalic,
    }
    pub fn measure(_f: Face, px: i32, s: &str) -> i32 {
        s.chars().count() as i32 * px / 2
    }
}

#[cfg(test)]
mod doc_tests;
#[cfg(test)]
mod grid_tests;
#[cfg(test)]
mod deck_tests;
#[cfg(test)]
mod web_tests;
#[cfg(test)]
mod tls_tests;
#[cfg(test)]
mod image_tests;
#[cfg(test)]
mod tls_vectors;

#[cfg(test)]
mod tests {
    use super::crypto::*;
    use super::qr::*;

    fn unhex(s: &str) -> Vec<u8> {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn sha256_vectors() {
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let long = vec![b'a'; 1_000_000];
        assert_eq!(hex(&sha256(&long)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }

    #[test]
    fn hmac_rfc4231() {
        let key = [0x0bu8; 20];
        assert_eq!(hex(&hmac_sha256(&key, &[b"Hi There"])), "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7");
        assert_eq!(hex(&hmac_sha256(b"Jefe", &[b"what do ya want ", b"for nothing?"])), "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn hkdf_rfc5869() {
        let mut okm = [0u8; 42];
        hkdf(&unhex("000102030405060708090a0b0c"), &[0x0b; 22], &unhex("f0f1f2f3f4f5f6f7f8f9"), &mut okm);
        assert_eq!(hex(&okm), "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865");
    }

    #[test]
    fn poly1305_rfc8439() {
        let key: [u8; 32] = unhex("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b").try_into().unwrap();
        assert_eq!(hex(&poly1305(&key, b"Cryptographic Forum Research Group")), "a8061dc1305136c6c22b8baf0c0127a9");
    }

    #[test]
    fn chacha20_rfc8439_block() {
        let key: [u8; 32] = (0u8..32).collect::<Vec<_>>().try_into().unwrap();
        let nonce: [u8; 12] = unhex("000000090000004a00000000").try_into().unwrap();
        let b = chacha20_block(&key, 1, &nonce);
        assert_eq!(hex(&b[..16]), "10f1e7e4d13b5915500fdd1fa32071c4");
    }

    #[test]
    fn aead_rfc8439() {
        let key: [u8; 32] = (0x80u8..0xa0).collect::<Vec<_>>().try_into().unwrap();
        let nonce: [u8; 12] = unhex("070000004041424344454647").try_into().unwrap();
        let aad = unhex("50515253c0c1c2c3c4c5c6c7");
        let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let sealed = seal(&key, &nonce, &aad, pt);
        assert_eq!(hex(&sealed[..16]), "d31a8d34648e60db7b86afbc53ef7ec2");
        assert_eq!(hex(&sealed[sealed.len() - 16..]), "1ae10b594f09e26a7e902ecbd0600691");
        assert_eq!(open(&key, &nonce, &aad, &sealed).unwrap(), pt.to_vec());
        let mut bad = sealed.clone();
        bad[3] ^= 1;
        assert!(open(&key, &nonce, &aad, &bad).is_none());
    }

    #[test]
    fn sha1_and_base64() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        // RFC 6455 handshake example
        let acc = base64(&sha1(b"dGhlIHNhbXBsZSBub25jZQ==258EAFA5-E914-47DA-95CA-C5AB0DC85B11"));
        assert_eq!(acc, "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
        for n in 0..40 {
            let d: Vec<u8> = (0..n as u8).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(base64_decode(&base64(&d)).unwrap(), d);
            assert_eq!(base64_decode(&base64url(&d)).unwrap(), d);
        }
    }

    /// Writes QR codes as PGM images for the external decoder check.
    #[test]
    fn qr_export() {
        let dir = std::env::var("QR_OUT").unwrap_or_default();
        if dir.is_empty() {
            return;
        }
        let cases: Vec<(String, Ecc)> = vec![
            ("HELLO".into(), Ecc::Medium),
            ("http://192.168.1.23:7743/#k=q83vEjRWeJq83vEjRWeJq83vEjRWeJq83vEjRWeJq8&d=a1b2".into(), Ecc::Medium),
            ("http://10.0.2.15:7743/#k=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&d=0000".into(), Ecc::Low),
            ("x".repeat(200), Ecc::Medium),
            ("0123456789abcdef".repeat(40), Ecc::Low),
        ];
        for (i, (text, e)) in cases.iter().enumerate() {
            let q = Qr::encode(text.as_bytes(), *e).unwrap();
            let border = 4;
            let scale = 6;
            let n = (q.size + 2 * border) * scale;
            let mut img = format!("P5\n{} {}\n255\n", n, n).into_bytes();
            for y in 0..n {
                for x in 0..n {
                    let (mx, my) = ((x / scale) as isize - border as isize, (y / scale) as isize - border as isize);
                    let dark = mx >= 0 && my >= 0 && (mx as usize) < q.size && (my as usize) < q.size && q.get(mx as usize, my as usize);
                    img.push(if dark { 0 } else { 255 });
                }
            }
            std::fs::write(format!("{}/qr{}.pgm", dir, i), img).unwrap();
            std::fs::write(format!("{}/qr{}.txt", dir, i), text).unwrap();
        }
    }

    #[test]
    fn hlp_message_roundtrip() {
        use super::hlp::Msg;
        let m = Msg::new("msg").with("thread", "42").with("text", "line one\nline \\two = ok").blob(vec![0, 10, 10, 255]);
        let d = Msg::decode(&m.encode()).unwrap();
        assert_eq!(d, m);
        assert_eq!(d.get("text"), "line one\nline \\two = ok");
        let plain = Msg::new("battery").with("level", "80");
        assert_eq!(Msg::decode(&plain.encode()).unwrap(), plain);
    }

    #[test]
    fn hlp_session() {
        use super::hlp::Session;
        let key = [7u8; 32];
        let (cn, sn) = ([1u8; 16], [2u8; 16]);
        let mut c = Session::client(&key, &cn, &sn);
        let mut s = Session::server(&key, &cn, &sn);
        for i in 0..5u8 {
            let f = c.seal(&[i; 100]);
            assert_eq!(s.open(&f).unwrap(), vec![i; 100]);
            let g = s.seal(b"pong");
            assert_eq!(c.open(&g).unwrap(), b"pong".to_vec());
        }
        // replay and tamper are rejected
        let f = c.seal(b"x");
        let mut bad = f.clone();
        bad[10] ^= 1;
        assert!(s.open(&bad).is_none());
        assert!(s.open(&f).is_some());
        assert!(s.open(&f).is_none());
        // wrong key fails
        let mut other = Session::server(&[8u8; 32], &cn, &sn);
        assert!(other.open(&c.seal(b"y")).is_none());
    }

    /// Interop vectors consumed by the web and Android companion tests.
    #[test]
    fn hlp_vectors() {
        use super::hlp::{Msg, Session};
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let cn: Vec<u8> = (0x10..0x20).collect();
        let sn: Vec<u8> = (0x20..0x30).collect();
        let mut c = Session::client(&key, &cn, &sn);
        let mut s = Session::server(&key, &cn, &sn);
        let m1 = Msg::new("device").with("name", "Test phone").with("battery", "77").encode();
        let m2 = Msg::new("file").with("name", "a.bin").blob((0..=255u8).collect()).encode();
        let r1 = Msg::new("welcome").with("name", "HydatekOS").encode();
        let v = format!(
            "{{\n  \"key\": \"{}\",\n  \"client_nonce\": \"{}\",\n  \"server_nonce\": \"{}\",\n  \"c2s\": [\"{}\", \"{}\"],\n  \"c2s_plain\": [\"{}\", \"{}\"],\n  \"s2c\": [\"{}\"],\n  \"s2c_plain\": [\"{}\"]\n}}\n",
            hex(&key), hex(&cn), hex(&sn),
            hex(&c.seal(&m1)), hex(&c.seal(&m2)), hex(&m1), hex(&m2),
            hex(&s.seal(&r1)), hex(&r1)
        );
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../companion/test-vectors.json");
        if let Ok(old) = std::fs::read_to_string(path) {
            assert_eq!(old, v, "HLP vectors changed; delete the file to regenerate");
        } else {
            std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/../companion")).unwrap();
            std::fs::write(path, v).unwrap();
        }
    }
}
