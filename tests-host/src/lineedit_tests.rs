//! The line editor behind every text box.

use crate::lineedit::{LineEdit, MAX};

#[test]
fn typing_and_moving() {
    let mut e = LineEdit::default();
    for c in "héllo world".chars() {
        e.insert_char(c);
    }
    assert_eq!(e.text, "héllo world");
    assert_eq!(e.caret(), 11);
    e.left(true);
    assert_eq!(e.before_caret(), "héllo ");
    e.left(false);
    e.insert_char('!');
    assert_eq!(e.text, "héllo! world");
    e.home();
    e.insert("¡");
    assert_eq!(e.text, "¡héllo! world");
    e.right(true);
    assert_eq!(e.before_caret(), "¡héllo");
    e.end();
    assert_eq!(e.caret(), e.text.chars().count());
    // text set directly keeps the caret at the end
    e.text = String::from("abc");
    assert_eq!(e.before_caret(), "abc");
}

#[test]
fn deleting() {
    let mut e = LineEdit::new("one two_three  four");
    e.backspace(true);
    assert_eq!(e.text, "one two_three  ");
    e.backspace(true);
    assert_eq!(e.text, "one ");
    e.home();
    e.delete(false);
    assert_eq!(e.text, "ne ");
    e.delete(true);
    assert_eq!(e.text, " ");
    assert_eq!(e.caret(), 0);
    e.backspace(false);
    assert_eq!(e.text, " ");
}

#[test]
fn pasting_is_cleaned_and_limited() {
    let mut e = LineEdit::new("ab");
    e.left(false);
    e.insert("x\ny\tz\u{7}");
    assert_eq!(e.text, "ax y zb");
    assert_eq!(e.before_caret(), "ax y z");
    e.insert(&"q".repeat(MAX * 2));
    assert_eq!(e.text.chars().count(), MAX);
    assert!(e.text.ends_with('b'));
}
