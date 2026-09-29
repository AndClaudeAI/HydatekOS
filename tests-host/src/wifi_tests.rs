//! Wi-Fi: networks from beacons, WPA2 keys, the 4-way handshake, CCMP.

use crate::tls::aes::Aes;
use crate::wifi::*;
use crate::wifi_vectors as v;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn aes_decrypt_and_key_unwrap() {
    // FIPS 197 appendix C.1, both ways
    let aes = Aes::new(&hex("000102030405060708090a0b0c0d0e0f"));
    let mut b: [u8; 16] = hex("00112233445566778899aabbccddeeff").try_into().unwrap();
    aes.encrypt(&mut b);
    assert_eq!(b.to_vec(), hex("69c4e0d86a7b0430d8cdb78070b4c55a"));
    aes.decrypt(&mut b);
    assert_eq!(b.to_vec(), hex("00112233445566778899aabbccddeeff"));
    // AES-256 (C.3)
    let aes = Aes::new(&hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"));
    let mut b: [u8; 16] = hex("8ea2b7ca516745bfeafc49904b496089").try_into().unwrap();
    aes.decrypt(&mut b);
    assert_eq!(b.to_vec(), hex("00112233445566778899aabbccddeeff"));
    // RFC 3394 4.1
    let w = hex("1fa68b0a8112b447aef34bd8fb5a7b829d3e862371d2cfe5");
    assert_eq!(key_unwrap(&hex("000102030405060708090a0b0c0d0e0f"), &w), Some(hex("00112233445566778899aabbccddeeff")));
    let mut bad = w.clone();
    bad[5] ^= 1;
    assert_eq!(key_unwrap(&hex("000102030405060708090a0b0c0d0e0f"), &bad), None);
}

#[test]
fn passphrase_to_key() {
    // IEEE 802.11 Annex J.4
    assert_eq!(psk("password", b"IEEE").to_vec(), hex("f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"));
    assert_eq!(psk("ThisIsAPassword", b"ThisIsASSID").to_vec(), hex("0dc0d6eb90555ed6419756b9a15ec3e3209b63df707dd508d14581f8982721af"));
    assert_eq!(psk("correct horse", b"HydatekNet").to_vec(), v::PMK);
}

#[test]
fn four_way_handshake() {
    let aa: [u8; 6] = v::AA.try_into().unwrap();
    let spa: [u8; 6] = v::SPA.try_into().unwrap();
    let pmk: [u8; 32] = v::PMK.try_into().unwrap();
    let mut h = Handshake::new(pmk, aa, spa, v::SNONCE.try_into().unwrap());
    // message 1 in: message 2 out, exactly as the access point expects
    let m2 = h.message1(v::M1).expect("message 2");
    assert_eq!(h.ptk.unwrap().to_vec(), v::PTK);
    assert_eq!(m2, v::M2);
    // message 3: MIC checked, the group key unwrapped; message 4 out
    let m4 = h.message3(v::M3).expect("message 4");
    assert_eq!(m4, v::M4);
    assert_eq!(h.gtk, Some((1, v::GTK.to_vec())));
    // a forged message 3 is refused
    let mut forged = v::M3.to_vec();
    forged[40] ^= 1;
    let mut h2 = Handshake::new(pmk, aa, spa, v::SNONCE.try_into().unwrap());
    h2.message1(v::M1).unwrap();
    assert!(h2.message3(&forged).is_none());
    // and a wrong password gives the wrong MIC
    let mut h3 = Handshake::new(psk("wrong horse", b"HydatekNet"), aa, spa, v::SNONCE.try_into().unwrap());
    h3.message1(v::M1).unwrap();
    assert!(h3.message3(v::M3).is_none());
}

#[test]
fn ccmp() {
    let tk = &v::PTK[32..48];
    let sealed = ccmp_encrypt(tk, v::CCMP_HDR, v::CCMP_PN, v::CCMP_PLAIN);
    assert_eq!(sealed, v::CCMP_SEALED);
    assert_eq!(ccmp_decrypt(tk, v::CCMP_HDR, v::CCMP_PN, v::CCMP_SEALED).as_deref(), Some(v::CCMP_PLAIN));
    let mut bad = sealed.clone();
    bad[3] ^= 0x80;
    assert!(ccmp_decrypt(tk, v::CCMP_HDR, v::CCMP_PN, &bad).is_none());
    assert!(ccmp_decrypt(tk, v::CCMP_HDR, v::CCMP_PN + 1, &sealed).is_none());
    assert_eq!(ccmp_pn(&[0x78, 0x56, 0, 0x20, 0x34, 0x12, 0, 0]), 0x1234_5678);
}

fn beacon(ies: &[u8], privacy: bool) -> Vec<u8> {
    let mut f = vec![0x80, 0, 0, 0];
    f.extend_from_slice(&[0xFF; 6]);
    f.extend_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    f.extend_from_slice(&[0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(&[0; 8]);
    f.extend_from_slice(&[100, 0]);
    f.extend_from_slice(&(if privacy { 0x0411u16 } else { 0x0401 }).to_le_bytes());
    f.extend_from_slice(ies);
    f
}

#[test]
fn networks_from_beacons() {
    // WPA2-Personal, channel 6, HT: Wi-Fi 4 on 2.4 GHz
    let mut ies = vec![0, 10];
    ies.extend_from_slice(b"HydatekNet");
    ies.extend_from_slice(&[3, 1, 6]);
    ies.extend_from_slice(&rsn_element()[..]);
    ies.extend_from_slice(&[45, 2, 0, 0]);
    let b = parse_beacon(&beacon(&ies, true), -50).unwrap();
    assert_eq!((b.ssid.as_str(), b.channel, b.band, b.security, b.generation), ("HydatekNet", 6, 2, Security::Wpa2Personal, 4));
    assert_eq!(b.bssid, [0x02, 0x11, 0x22, 0x33, 0x44, 0x55]);
    assert_eq!(b.bars(), 4);
    // WPA2/WPA3 transition, channel 36, HE: Wi-Fi 6 on 5 GHz
    let rsn = [48, 24, 1, 0, 0, 0x0F, 0xAC, 4, 1, 0, 0, 0x0F, 0xAC, 4, 2, 0, 0, 0x0F, 0xAC, 2, 0, 0x0F, 0xAC, 8, 0x80, 0];
    let mut ies = vec![0, 4];
    ies.extend_from_slice(b"Cafe");
    ies.extend_from_slice(&[3, 1, 36]);
    ies.extend_from_slice(&rsn);
    ies.extend_from_slice(&[191, 2, 0, 0, 255, 2, 35, 0]);
    let b = parse_beacon(&beacon(&ies, true), -72).unwrap();
    assert_eq!((b.band, b.security, b.generation, b.bars()), (5, Security::Wpa2Wpa3, 6, 2));
    assert_eq!(b.generation_name(), "Wi-Fi 6");
    // WPA3 only; Enterprise; open; WEP
    let sae = [48, 20, 1, 0, 0, 0x0F, 0xAC, 4, 1, 0, 0, 0x0F, 0xAC, 4, 1, 0, 0, 0x0F, 0xAC, 8, 0xC0, 0];
    assert_eq!(rsn_security(&sae[2..]), Security::Wpa3Personal);
    let eap = [1, 0, 0, 0x0F, 0xAC, 4, 1, 0, 0, 0x0F, 0xAC, 4, 1, 0, 0, 0x0F, 0xAC, 1, 0, 0];
    assert_eq!(rsn_security(&eap), Security::Enterprise);
    let b = parse_beacon(&beacon(&[0, 3, b'F', b'o', b'o'], false), -90).unwrap();
    assert_eq!((b.security, b.bars()), (Security::Open, 0));
    assert_eq!(parse_beacon(&beacon(&[0, 0], true), -60).unwrap().security, Security::Wep);
    assert!(Security::Wpa2Personal.needs_password() && !Security::Owe.needs_password());
    // not a beacon; truncated
    assert!(parse_beacon(&[0x08, 0, 0, 0], -60).is_none());
    let mut t = beacon(&ies, true);
    t.truncate(40);
    assert!(parse_beacon(&t, -60).is_some());
}
