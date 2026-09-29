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

// ---- key names (the keyboard tester) ----------------------------------------

/// The firmware's name for a key with no character (UEFI scan codes).
fn scan_name(scan: u16) -> Option<&'static str> {
    const F: [&str; 24] = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24"];
    Some(match scan {
        0x01 => "↑",
        0x02 => "↓",
        0x03 => "→",
        0x04 => "←",
        0x05 => "Home",
        0x06 => "End",
        0x07 => "Insert",
        0x08 => "Delete",
        0x09 => "Page Up",
        0x0a => "Page Down",
        0x0b..=0x16 => F[(scan - 0x0b) as usize],
        0x17 => "Esc",
        0x48 => "Pause",
        0x68..=0x73 => F[(scan - 0x68 + 12) as usize],
        0x7f => "Mute",
        0x80 => "Volume up",
        0x81 => "Volume down",
        0x100 => "Brightness up",
        0x101 => "Brightness down",
        0x102 => "Sleep",
        0x103 => "Hibernate",
        0x104 => "Display",
        0x105 => "Recovery",
        0x106 => "Eject",
        _ => return None,
    })
}

/// The key a character is typed with on a US keyboard, Shift or not, as it's
/// printed on the key (letters in capitals): '!' is on "1", 'a' on "A".
pub fn key_of(c: char) -> char {
    const SHIFTED: &str = "~!@#$%^&*()_+{}|:\"<>?";
    const PLAIN: &str = "`1234567890-=[]\\;',./";
    match SHIFTED.chars().position(|s| s == c) {
        Some(i) => PLAIN.chars().nth(i).unwrap_or(c),
        None => c.to_ascii_uppercase(),
    }
}

/// A key stroke as the firmware reported it (a UEFI scan code and UTF-16
/// character), named as it's printed on the key: "A", "Enter", "F5",
/// "Volume up". Empty for a stroke of modifiers alone.
pub fn key_name(scan: u16, unicode: u16) -> alloc::string::String {
    use alloc::string::{String, ToString};
    if scan != 0 {
        return match scan_name(scan) {
            Some(n) => n.to_string(),
            None => alloc::format!("Key {:#04x}", scan),
        };
    }
    match unicode {
        0 => String::new(),
        0x08 => "Backspace".to_string(),
        0x09 => "Tab".to_string(),
        0x0a | 0x0d => "Enter".to_string(),
        0x1b => "Esc".to_string(),
        // Ctrl+letter, sent as a control character
        c @ 1..=26 => ((b'A' + c as u8 - 1) as char).to_string(),
        0x20 => "Space".to_string(),
        c => match char::from_u32(c as u32) {
            Some(ch) if ch.is_ascii() => key_of(ch).to_string(),
            Some(ch) => ch.to_string(),
            None => alloc::format!("U+{:04X}", c),
        },
    }
}

/// A picture of the keyboard for the tester: rows of (the key's name as
/// `key_name` gives it, its label, its width in quarter keys).
pub const LAYOUT: &[&[(&str, &str, u8)]] = &[
    &[("Esc", "Esc", 7), ("F1", "F1", 4), ("F2", "F2", 4), ("F3", "F3", 4), ("F4", "F4", 4), ("F5", "F5", 4), ("F6", "F6", 4), ("F7", "F7", 4), ("F8", "F8", 4), ("F9", "F9", 4), ("F10", "F10", 4), ("F11", "F11", 4), ("F12", "F12", 4), ("Insert", "Ins", 4), ("Delete", "Del", 5)],
    &[("`", "`", 4), ("1", "1", 4), ("2", "2", 4), ("3", "3", 4), ("4", "4", 4), ("5", "5", 4), ("6", "6", 4), ("7", "7", 4), ("8", "8", 4), ("9", "9", 4), ("0", "0", 4), ("-", "-", 4), ("=", "=", 4), ("Backspace", "Back", 7), ("Home", "Home", 5)],
    &[("Tab", "Tab", 6), ("Q", "Q", 4), ("W", "W", 4), ("E", "E", 4), ("R", "R", 4), ("T", "T", 4), ("Y", "Y", 4), ("U", "U", 4), ("I", "I", 4), ("O", "O", 4), ("P", "P", 4), ("[", "[", 4), ("]", "]", 4), ("\\", "\\", 5), ("Page Up", "PgUp", 5)],
    &[("Caps Lock", "Caps", 7), ("A", "A", 4), ("S", "S", 4), ("D", "D", 4), ("F", "F", 4), ("G", "G", 4), ("H", "H", 4), ("J", "J", 4), ("K", "K", 4), ("L", "L", 4), (";", ";", 4), ("'", "'", 4), ("Enter", "Enter", 8), ("Page Down", "PgDn", 5)],
    &[("Shift", "Shift", 9), ("Z", "Z", 4), ("X", "X", 4), ("C", "C", 4), ("V", "V", 4), ("B", "B", 4), ("N", "N", 4), ("M", "M", 4), (",", ",", 4), (".", ".", 4), ("/", "/", 4), ("Shift", "Shift", 6), ("↑", "↑", 4), ("End", "End", 5)],
    &[("Ctrl", "Ctrl", 6), ("Gen", "Gen", 5), ("Aux", "Aux", 5), ("Space", "", 24), ("Aux", "Aux", 5), ("Ctrl", "Ctrl", 6), ("←", "←", 4), ("↓", "↓", 4), ("→", "→", 5)],
];
