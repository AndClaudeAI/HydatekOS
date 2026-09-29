//! HID report descriptors from real devices, and touchpad gestures.

use crate::hid::*;
use crate::touchpad::{Gesture, Gestures};

/// A boot-protocol style 3-button wheel mouse.
const MOUSE: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05, 0x81, 0x03,
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7F, 0x75, 0x08, 0x95, 0x03, 0x81, 0x06, 0xC0, 0xC0,
];

/// QEMU's usb-tablet (hw/usb/dev-hid.c): absolute X/Y 0..32767.
const QEMU_TABLET: &[u8] = &[
    0x05, 0x01, 0x09, 0x02, 0xa1, 0x01, 0x09, 0x01, 0xa1, 0x00, 0x05, 0x09, 0x19, 0x01, 0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01, 0x75, 0x05, 0x81, 0x01,
    0x05, 0x01, 0x09, 0x30, 0x09, 0x31, 0x15, 0x00, 0x26, 0xff, 0x7f, 0x35, 0x00, 0x46, 0xff, 0x7f, 0x75, 0x10, 0x95, 0x02, 0x81, 0x02, 0x05, 0x01, 0x09, 0x38, 0x15, 0x81, 0x25, 0x7f, 0x35, 0x00,
    0x45, 0x00, 0x75, 0x08, 0x95, 0x01, 0x81, 0x06, 0xc0, 0xc0,
];

/// A two-finger Precision Touchpad (report id 1): per finger confidence, tip,
/// contact id, X 0..1000, Y 0..800; then scan time, contact count, button.
pub(crate) fn touchpad() -> Vec<u8> {
    let finger: &[u8] = &[
        0x05, 0x0D, 0x09, 0x22, 0xA1, 0x02, 0x15, 0x00, 0x25, 0x01, 0x09, 0x47, 0x09, 0x42, 0x95, 0x02, 0x75, 0x01, 0x81, 0x02, 0x95, 0x06, 0x81, 0x03, 0x25, 0x0F, 0x75, 0x04, 0x95, 0x01, 0x09, 0x51,
        0x81, 0x02, 0x75, 0x04, 0x81, 0x03, 0x05, 0x01, 0x15, 0x00, 0x26, 0xE8, 0x03, 0x75, 0x10, 0x95, 0x01, 0x09, 0x30, 0x81, 0x02, 0x26, 0x20, 0x03, 0x09, 0x31, 0x81, 0x02, 0xC0,
    ];
    let mut d = vec![0x05, 0x0D, 0x09, 0x05, 0xA1, 0x01, 0x85, 0x01];
    d.extend_from_slice(finger);
    d.extend_from_slice(finger);
    d.extend_from_slice(&[
        0x05, 0x0D, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x75, 0x10, 0x95, 0x01, 0x09, 0x56, 0x81, 0x02, 0x09, 0x54, 0x25, 0x7F, 0x95, 0x01, 0x75, 0x08, 0x81, 0x02, 0x05, 0x09, 0x09, 0x01, 0x25,
        0x01, 0x75, 0x01, 0x95, 0x01, 0x81, 0x02, 0x95, 0x07, 0x81, 0x03, 0xC0,
    ]);
    d
}

/// One touchpad report: fingers (confidence, tip, id, x, y), count, button.
pub(crate) fn pad_report(fingers: &[(bool, bool, u8, u16, u16)], count: u8, button: bool) -> Vec<u8> {
    let mut r = vec![1u8];
    for i in 0..2 {
        let (c, t, id, x, y) = fingers.get(i).copied().unwrap_or((false, false, 0, 0, 0));
        r.push(c as u8 | (t as u8) << 1);
        r.push(id & 0x0F);
        r.extend_from_slice(&x.to_le_bytes());
        r.extend_from_slice(&y.to_le_bytes());
    }
    r.extend_from_slice(&[0x10, 0x00, count, button as u8]);
    r
}

/// A haptic touchpad's Simple Haptic Controller: a waveform list (feature
/// report 3, ordinals 3..5) and the manual trigger (output report 4).
pub(crate) const HAPTIC: &[u8] = &[
    0x05, 0x0E, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x03, 0x09, 0x10, 0xA1, 0x02, 0x05, 0x0A, 0x19, 0x03, 0x29, 0x05, 0x15, 0x00, 0x27, 0xFF, 0xFF, 0x00, 0x00, 0x75, 0x10, 0x95, 0x03, 0xB1, 0x02, 0xC0,
    0x05, 0x0E, 0x85, 0x04, 0x09, 0x21, 0x15, 0x00, 0x25, 0x05, 0x75, 0x08, 0x95, 0x01, 0x91, 0x02, 0x09, 0x23, 0x25, 0x64, 0x91, 0x02, 0x09, 0x24, 0x25, 0x05, 0x91, 0x02, 0x09, 0x25, 0x26, 0xFF,
    0x7F, 0x75, 0x10, 0x91, 0x02, 0xC0,
];

#[test]
fn mouse() {
    let d = Descriptor::parse(MOUSE);
    assert_eq!(describe(&d), "Mouse");
    let m = Mouse::find(&d).unwrap();
    assert!(!m.absolute && m.wheel.is_some() && m.buttons.len() == 3);
    assert_eq!(d.report_len(Kind::Input, 0), 4);
    // right button, x -3, y +5, wheel -1
    let r = m.read(&[0b010, 0xFD, 5, 0xFF], false).unwrap();
    assert_eq!(r, MouseReport { x: -3, y: 5, wheel: -1, pan: 0, buttons: 2 });
}

#[test]
fn qemu_tablet() {
    let d = Descriptor::parse(QEMU_TABLET);
    let m = Mouse::find(&d).unwrap();
    assert!(m.absolute);
    let r = m.read(&[1, 0xFF, 0x3F, 0x00, 0x40, 0], false).unwrap();
    assert_eq!((r.x, r.y, r.buttons), (0x3FFF, 0x4000, 1));
}

#[test]
fn precision_touchpad() {
    let d = Descriptor::parse(&touchpad());
    assert_eq!(describe(&d), "Touchpad");
    assert!(d.ids);
    let t = Touchpad::find(&d).unwrap();
    assert_eq!((t.max_x, t.max_y), (1000, 800));
    assert_eq!(d.report_len(Kind::Input, 1), 17);
    let r = t.read(&pad_report(&[(true, true, 3, 500, 400), (false, true, 4, 900, 100)], 2, false), true).unwrap();
    assert_eq!(r.count, 2);
    assert_eq!(r.contacts[0], Contact { id: 3, tip: true, confident: true, x: 500, y: 400 });
    // the second is a palm
    assert!(!r.contacts[1].confident);
    // one finger: only the first slot counts
    let r = t.read(&pad_report(&[(true, true, 3, 10, 20)], 1, true), true).unwrap();
    assert_eq!((r.contacts.len(), r.button), (1, true));
}

#[test]
fn gestures() {
    let d = Descriptor::parse(&touchpad());
    let t = Touchpad::find(&d).unwrap();
    let mut g = Gestures::new();
    let mut feed = |fingers: &[(bool, bool, u8, u16, u16)], button: bool, now: u64| {
        let r = t.read(&pad_report(fingers, fingers.len() as u8, button), true).unwrap();
        g.feed(&r, t.max_x, now)
    };
    // a quick tap: a click
    assert!(feed(&[(true, true, 1, 500, 400)], false, 0).is_empty());
    assert_eq!(feed(&[], false, 80), vec![Gesture::Click(0)]);
    // a finger moving right: the pointer moves right (pad width = 1400 px)
    feed(&[(true, true, 1, 100, 400)], false, 1000);
    let m = feed(&[(true, true, 1, 110, 400)], false, 1010);
    assert_eq!(m, vec![Gesture::Move(13, 0)]); // 13.98 px: the fraction carries over
    // a long touch that moved isn't a tap
    assert!(feed(&[], false, 1500).is_empty());
    // two fingers tapped: a right click
    feed(&[(true, true, 1, 300, 300), (true, true, 2, 500, 300)], false, 2000);
    assert_eq!(feed(&[], false, 2100), vec![Gesture::Click(1)]);
    // two fingers moving up: scrolling down the content (natural scrolling)
    feed(&[(true, true, 1, 300, 600), (true, true, 2, 500, 600)], false, 3000);
    let s = feed(&[(true, true, 1, 300, 560), (true, true, 2, 500, 560)], false, 3020);
    assert!(s.iter().all(|x| *x == Gesture::Scroll(1)) && !s.is_empty(), "{:?}", s);
    feed(&[], false, 3500);
    // pressing the pad: press and release, not also a tap
    feed(&[(true, true, 1, 500, 400)], false, 4000);
    assert_eq!(feed(&[(true, true, 1, 500, 400)], true, 4020), vec![Gesture::Press(0)]);
    assert_eq!(feed(&[(true, true, 1, 500, 400)], false, 4060), vec![Gesture::Release(0)]);
    assert!(feed(&[], false, 4080).is_empty());
    // a palm alone does nothing
    assert!(feed(&[(false, true, 1, 500, 400)], false, 5000).is_empty());
    assert!(feed(&[], false, 5050).is_empty());
}

#[test]
fn haptic_touchpad() {
    let d = Descriptor::parse(HAPTIC);
    assert_eq!(describe(&d), "Haptic controller");
    let mut h = HapticController::find(&d).unwrap();
    assert_eq!(h.list_report(), Some(3));
    // the device's waveform list: ordinal 3 click, 4 buzz, 5 press
    h.read_list(&[3, 0x03, 0x10, 0x04, 0x10, 0x06, 0x10], true);
    assert_eq!(h.ordinal(WAVE_CLICK), Some(3));
    assert_eq!(h.ordinal(WAVE_PRESS), Some(5));
    assert_eq!(h.ordinal(WAVE_RUMBLE), None);
    // click at half strength; buzz three times, 100 ms apart
    assert_eq!(h.play(&d, WAVE_CLICK, 50, 0, 0).unwrap(), vec![4, 3, 50, 0, 0, 0]);
    assert_eq!(h.play(&d, WAVE_BUZZ, 100, 2, 100).unwrap(), vec![4, 4, 100, 2, 100, 0]);
    assert!(h.play(&d, WAVE_RUMBLE, 100, 0, 0).is_none());
}

#[test]
fn media_keys() {
    // one bit per key: mute, volume up, volume down
    let bits = [0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x03, 0x09, 0xE2, 0x09, 0xE9, 0x09, 0xEA, 0x81, 0x02, 0x95, 0x05, 0x81, 0x03, 0xC0];
    let d = Descriptor::parse(&bits);
    assert_eq!(describe(&d), "Media keys");
    let c = Consumer::find(&d).unwrap();
    assert_eq!(c.read(&[0b010], false).unwrap(), vec![0xE9]);
    assert!(c.read(&[0], false).unwrap().is_empty());
    // an array of usages (most keyboards' media keys), report id 2
    let array = [0x05, 0x0C, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x02, 0x19, 0x00, 0x2A, 0x3C, 0x02, 0x15, 0x00, 0x26, 0x3C, 0x02, 0x95, 0x01, 0x75, 0x10, 0x81, 0x00, 0xC0];
    let d = Descriptor::parse(&array);
    let c = Consumer::find(&d).unwrap();
    assert_eq!(c.read(&[2, 0xEA, 0x00], true).unwrap(), vec![0xEA]);
    assert_eq!(c.read(&[1, 0xEA, 0x00], true), None);
}

#[test]
fn malformed_descriptors_dont_panic() {
    let t = touchpad();
    for n in 0..t.len() {
        let d = Descriptor::parse(&t[..n]);
        let _ = Touchpad::find(&d).map(|p| p.read(&[1, 2, 3], true));
        let _ = Mouse::find(&d).map(|m| m.read(&[], true));
    }
    let _ = Descriptor::parse(&[0xFE, 0xFF, 0x00]);
    let _ = Descriptor::parse(&[0x27, 0xFF]);
}
