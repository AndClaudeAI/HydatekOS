//! The single-line text editor behind every text box: names, addresses,
//! search boxes, the Terminal's command line, dialogs.
//!
//! Keys (see `apps::LineEdit::key`): typing inserts at the caret; ← and →
//! move by a character, Gen+← / Gen+→ (or Aux) by a word; Home and End go to
//! either end; Backspace and Delete remove the character before / after the
//! caret, and with Gen the word; Gen+V pastes.

use alloc::string::String;

/// The longest text a line holds, in characters.
pub const MAX: usize = 1000;

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct LineEdit {
    pub text: String,
    /// characters after the caret (0: at the end), so text set directly keeps
    /// the caret at its end
    back: usize,
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl LineEdit {
    /// A line holding `text`, with the caret at the end.
    pub fn new(text: impl Into<String>) -> LineEdit {
        LineEdit { text: text.into(), back: 0 }
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// The caret, in characters from the start.
    pub fn caret(&self) -> usize {
        self.len() - self.back.min(self.len())
    }

    fn set_caret(&mut self, at: usize) {
        let n = self.len();
        self.back = n - at.min(n);
    }

    /// Byte offset of character `i`.
    fn byte(&self, i: usize) -> usize {
        self.text.char_indices().nth(i).map(|(b, _)| b).unwrap_or(self.text.len())
    }

    pub fn set(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.back = 0;
    }

    pub fn clear(&mut self) {
        self.set(String::new());
    }

    /// Insert at the caret (control characters are dropped; newlines become
    /// spaces), up to `MAX` characters.
    pub fn insert(&mut self, s: &str) {
        let at = self.caret();
        let room = MAX.saturating_sub(self.len());
        let add: String = s.chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).filter(|c| !c.is_control()).take(room).collect();
        let b = self.byte(at);
        self.text.insert_str(b, &add);
        self.set_caret(at + add.chars().count());
    }

    pub fn insert_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.insert(c.encode_utf8(&mut buf));
    }

    /// Remove characters `from..to`, leaving the caret at `from`.
    fn remove(&mut self, from: usize, to: usize) {
        let (a, b) = (self.byte(from), self.byte(to));
        self.text.replace_range(a..b, "");
        self.set_caret(from);
    }

    /// The start of the word before `at`.
    fn word_left(&self, at: usize) -> usize {
        let cs: alloc::vec::Vec<char> = self.text.chars().collect();
        let mut i = at.min(cs.len());
        while i > 0 && !word_char(cs[i - 1]) {
            i -= 1;
        }
        while i > 0 && word_char(cs[i - 1]) {
            i -= 1;
        }
        i
    }

    /// The end of the word after `at`.
    fn word_right(&self, at: usize) -> usize {
        let cs: alloc::vec::Vec<char> = self.text.chars().collect();
        let mut i = at.min(cs.len());
        while i < cs.len() && !word_char(cs[i]) {
            i += 1;
        }
        while i < cs.len() && word_char(cs[i]) {
            i += 1;
        }
        i
    }

    pub fn left(&mut self, word: bool) {
        let at = self.caret();
        let to = if word { self.word_left(at) } else { at.saturating_sub(1) };
        self.set_caret(to);
    }

    pub fn right(&mut self, word: bool) {
        let at = self.caret();
        let to = if word { self.word_right(at) } else { (at + 1).min(self.len()) };
        self.set_caret(to);
    }

    pub fn home(&mut self) {
        self.set_caret(0);
    }

    pub fn end(&mut self) {
        self.back = 0;
    }

    /// Backspace: the character (or word) before the caret.
    pub fn backspace(&mut self, word: bool) {
        let at = self.caret();
        let from = if word { self.word_left(at) } else { at.saturating_sub(1) };
        self.remove(from, at);
    }

    /// Delete: the character (or word) after the caret.
    pub fn delete(&mut self, word: bool) {
        let at = self.caret();
        let to = if word { self.word_right(at) } else { (at + 1).min(self.len()) };
        self.remove(at, to);
        self.set_caret(at);
    }

    /// The text before the caret (for drawing it).
    pub fn before_caret(&self) -> &str {
        &self.text[..self.byte(self.caret())]
    }
}
