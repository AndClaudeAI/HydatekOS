//! HydatekOS's modifier keys, and what Aux types.
//!
//! - **Gen** (general) is the shortcut key, like Ctrl on Windows and Command
//!   on a Mac: the logo key (⊞, or ⌘ on an Apple keyboard), and Ctrl too
//!   unless that's turned off in Settings › Keyboard.
//! - **Aux** (auxiliary) is the second modifier, like Alt on Windows and
//!   Option on a Mac: the Alt key (⌥ Option on an Apple keyboard). Held with a
//!   key it types a special character, following the Mac's Option layout where
//!   it can (Aux+3 £, Aux+8 •, Aux+; …) and adding the Naira: Aux+N ₦.

/// Aux + a key: (the character the key gives, what Aux makes it type). The
/// key's character is what it types with Shift held, so Aux+Shift+- is `_`.
pub const AUX_CHARS: &[(char, char)] = &[
    // currencies
    ('n', '₦'),
    ('e', '€'),
    ('3', '£'),
    ('y', '¥'),
    ('4', '¢'),
    ('R', '₹'),
    ('C', '₵'),
    // marks
    ('g', '©'),
    ('r', '®'),
    ('2', '™'),
    ('6', '§'),
    ('7', '¶'),
    ('8', '•'),
    ('0', '°'),
    ('*', '°'),
    ('m', 'µ'),
    // punctuation
    (';', '…'),
    ('-', '–'),
    ('_', '—'),
    ('[', '“'),
    ('{', '”'),
    (']', '‘'),
    ('}', '’'),
    ('\\', '«'),
    ('|', '»'),
    ('1', '¡'),
    ('?', '¿'),
    // maths
    ('x', '×'),
    ('/', '÷'),
    ('=', '≠'),
    ('+', '±'),
    (',', '≤'),
    ('.', '≥'),
    ('w', 'Σ'),
    ('l', '¬'),
];

/// What Aux+`c` types, if anything. A capital letter with no entry of its own
/// types what the small letter does (Aux+Shift+N is ₦ too).
pub fn aux_char(c: char) -> Option<char> {
    let find = |k: char| AUX_CHARS.iter().find(|(key, _)| *key == k).map(|(_, v)| *v);
    find(c).or_else(|| if c.is_ascii_uppercase() { find(c.to_ascii_lowercase()) } else { None })
}
