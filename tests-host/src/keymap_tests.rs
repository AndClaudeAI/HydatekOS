//! What the Aux key types.

use crate::keymap::*;

#[test]
fn aux_characters() {
    assert_eq!(aux_char('n'), Some('₦'));
    assert_eq!(aux_char('e'), Some('€'));
    assert_eq!(aux_char('3'), Some('£'));
    assert_eq!(aux_char(';'), Some('…'));
    // Shift: its own character where there is one…
    assert_eq!(aux_char('-'), Some('–'));
    assert_eq!(aux_char('_'), Some('—'));
    assert_eq!(aux_char('R'), Some('₹'));
    assert_eq!(aux_char('C'), Some('₵'));
    // …otherwise the same as without it
    assert_eq!(aux_char('N'), Some('₦'));
    assert_eq!(aux_char('G'), Some('©'));
    // keys with nothing to type
    assert_eq!(aux_char('q'), None);
    assert_eq!(aux_char('Q'), None);
    assert_eq!(aux_char(' '), None);
}

#[test]
fn aux_table_is_sound() {
    // each key once
    let mut keys: Vec<char> = AUX_CHARS.iter().map(|(k, _)| *k).collect();
    keys.sort();
    let n = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), n, "a key is listed twice");
    // every character Aux types is in HydatekOS's font (tools/fontgen.py)
    let gen = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/fontgen.py")).unwrap();
    for (k, c) in AUX_CHARS {
        assert!(gen.contains(*c), "Aux+{} types {} which the font lacks", k, c);
        assert!(k.is_ascii() && !k.is_ascii_control(), "Aux+{:?}", k);
    }
}

#[test]
fn key_names() {
    assert_eq!(key_name(0, 'a' as u16), "A");
    assert_eq!(key_name(0, '!' as u16), "1");
    assert_eq!(key_name(0, '"' as u16), "'");
    assert_eq!(key_name(0, ' ' as u16), "Space");
    assert_eq!(key_name(0, 0x0d), "Enter");
    assert_eq!(key_name(0, 0x08), "Backspace");
    assert_eq!(key_name(0, 0x03), "C");
    assert_eq!(key_name(0, 0), "");
    assert_eq!(key_name(0, '₦' as u16), "₦");
    assert_eq!(key_name(0x0b, 0), "F1");
    assert_eq!(key_name(0x16, 0), "F12");
    assert_eq!(key_name(0x68, 0), "F13");
    assert_eq!(key_name(0x73, 0), "F24");
    assert_eq!(key_name(0x80, 0), "Volume up");
    assert_eq!(key_name(0x17, 0), "Esc");
    assert_eq!(key_name(0x999, 0), "Key 0x999");
}

#[test]
fn tester_layout() {
    // every row is as wide as the others
    let widths: Vec<u32> = LAYOUT.iter().map(|r| r.iter().map(|k| k.2 as u32).sum()).collect();
    assert!(widths.iter().all(|w| *w == widths[0]), "{:?}", widths);
    // every character key on it is found by the name key_name gives
    for c in "abcdefghijklmnopqrstuvwxyz0123456789`-=[]\\;',./~!@#$%^&*()_+{}|:\"<>?".chars() {
        let n = key_name(0, c as u16);
        assert!(LAYOUT.iter().flat_map(|r| r.iter()).any(|k| k.0 == n), "{} ({})", c, n);
    }
    for scan in [0x01u16, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x16, 0x17] {
        let n = key_name(scan, 0);
        assert!(LAYOUT.iter().flat_map(|r| r.iter()).any(|k| k.0 == n), "{}", n);
    }
}
