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
