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

/// A Windows-style pen: tip, barrel, invert, eraser, in range; X 0..32767,
/// Y 0..16383; pressure 0..4095; tilt ±60°.
const PEN: &[u8] = &[
    0x05, 0x0D, 0x09, 0x02, 0xA1, 0x01, 0x85, 0x02, 0x09, 0x20, 0xA1, 0x00, 0x09, 0x42, 0x09, 0x44, 0x09, 0x3C, 0x09, 0x45, 0x09, 0x32, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x05, 0x81, 0x02, 0x95,
    0x03, 0x81, 0x03, 0x05, 0x01, 0x09, 0x30, 0x26, 0xFF, 0x7F, 0x75, 0x10, 0x95, 0x01, 0x81, 0x02, 0x09, 0x31, 0x26, 0xFF, 0x3F, 0x81, 0x02, 0x05, 0x0D, 0x09, 0x30, 0x26, 0xFF, 0x0F, 0x81, 0x02, 0x09,
    0x3D, 0x09, 0x3E, 0x15, 0xC4, 0x25, 0x3C, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, 0xC0, 0xC0,
];

/// A HID ambient light sensor: reporting and power state (features), and
/// illuminance in hundredths of a lux (unit exponent -2).
const LIGHT: &[u8] = &[
    0x05, 0x20, 0x09, 0x41, 0xA1, 0x01, 0x85, 0x03, 0x05, 0x20, 0x0A, 0x16, 0x03, 0x15, 0x00, 0x25, 0x05, 0x75, 0x08, 0x95, 0x01, 0xB1, 0x02, 0x0A, 0x19, 0x03, 0xB1, 0x02, 0x0A, 0xD1, 0x04, 0x15, 0x00,
    0x27, 0xFF, 0xFF, 0xFF, 0x7F, 0x55, 0x0E, 0x75, 0x20, 0x95, 0x01, 0x81, 0x02, 0xC0,
];

#[test]
fn pen() {
    let d = Descriptor::parse(PEN);
    assert_eq!(describe(&d), "Pen");
    let p = Pen::find(&d).expect("a pen");
    // tip + in range, X half way, Y a quarter, pressure 2048, tilt -30 / +15
    let mut r = vec![2, 0b1_0001, 0, 0];
    r[2..4].copy_from_slice(&16384u16.to_le_bytes());
    r.extend_from_slice(&4096u16.to_le_bytes());
    r.extend_from_slice(&2048u16.to_le_bytes());
    r.extend_from_slice(&[(-30i8) as u8, 15]);
    let rep = p.read(&r, d.ids).unwrap();
    assert!(rep.tip && rep.in_range && !rep.barrel && !rep.eraser);
    assert_eq!((rep.x, rep.y / 100), (16384, 81));
    assert_eq!(rep.pressure, 500);
    assert_eq!((rep.tilt_x, rep.tilt_y), (-30, 15));
    // turned round: the eraser
    r[1] = 0b1_0100;
    assert!(p.read(&r, d.ids).unwrap().eraser);

    // through hidin: a pointer position, pressure, and the tip as a click
    let mut h = crate::hidin::HidInput::new(PEN, false);
    let mut out = vec![];
    r[1] = 0b1_0001;
    h.report(&r, 0, &mut out);
    use crate::hidin::Event;
    assert!(matches!(out[0], Event::Pen { pressure: 500, tip: true, eraser: false, .. }));
    assert_eq!(out[1], Event::Place(16384, 8192));
    assert_eq!(out[2], Event::Buttons(1));
    // hovering, tip up: the button lets go
    out.clear();
    r[1] = 0b1_0000;
    h.report(&r, 10, &mut out);
    assert_eq!(out.last(), Some(&Event::Buttons(0)));
}

#[test]
fn light_sensor() {
    let d = Descriptor::parse(LIGHT);
    assert_eq!(describe(&d), "Light sensor");
    let l = LightSensor::find(&d).expect("a light sensor");
    // started: reporting all events, full power
    assert_eq!(l.start(&d), Some(vec![3, 1, 1]));
    let mut r = vec![3];
    r.extend_from_slice(&32050u32.to_le_bytes());
    assert_eq!(l.read(&r, d.ids), Some((320, None)));
    let mut h = crate::hidin::HidInput::new(LIGHT, false);
    assert_eq!(h.start_reports(), vec![(3, vec![3, 1, 1])]);
    let mut out = vec![];
    h.report(&r, 0, &mut out);
    assert_eq!(out, vec![crate::hidin::Event::Light(320)]);
}

#[test]
fn ambient_brightness() {
    use crate::ambient::*;
    assert_eq!(target(0, 0), 35);
    assert_eq!(target(10, 0), 60);
    assert_eq!(target(400, 0), 93);
    assert_eq!(target(50_000, 0), 100);
    // the person likes it dimmer: the curve moves
    assert_eq!(target(10, -20), 40);
    assert_eq!(target(0, -40), 10);
    // slow steps, no flicker for small changes
    assert_eq!(approach(50, 80), 51);
    assert_eq!(approach(50, 20), 49);
    assert_eq!(approach(50, 52), 50);
    assert_eq!(smooth(None, 100), 100);
    assert_eq!(smooth(Some(100), 500), 200);
}

/// The boot keyboard descriptor (USB HID spec, appendix B.1): modifiers,
/// a reserved byte, LEDs out, six key slots.
const KEYBOARD: &[u8] = &[
    0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x05, 0x07, 0x19, 0xE0, 0x29, 0xE7, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x08, 0x81, 0x02, 0x95, 0x01, 0x75, 0x08, 0x81, 0x01, 0x95, 0x05, 0x75, 0x01, 0x05,
    0x08, 0x19, 0x01, 0x29, 0x05, 0x91, 0x02, 0x95, 0x01, 0x75, 0x03, 0x91, 0x01, 0x95, 0x06, 0x75, 0x08, 0x15, 0x00, 0x25, 0x65, 0x05, 0x07, 0x19, 0x00, 0x29, 0x65, 0x81, 0x00, 0xC0,
];

#[test]
fn keyboard() {
    use crate::hidin::{usage_char, Event, HidInput};
    let d = Descriptor::parse(KEYBOARD);
    assert_eq!(describe(&d), "Keyboard");
    let k = Keyboard::find(&d).expect("a keyboard");
    // left Shift + 'a' + '1'
    assert_eq!(k.read(&[0x02, 0, 0x04, 0x1E, 0, 0, 0, 0], d.ids), Some((0x02, vec![0x04, 0x1E])));
    let mut h = HidInput::new(KEYBOARD, false);
    let mut out = vec![];
    h.report(&[0, 0, 0x0B, 0, 0, 0, 0, 0], 0, &mut out);
    assert_eq!(out, vec![Event::Key { usage: 0x0B, down: true, mods: 0 }]);
    out.clear();
    // Ctrl pressed as well, then everything let go
    h.report(&[0x01, 0, 0x0B, 0, 0, 0, 0, 0], 5, &mut out);
    assert_eq!(out, vec![Event::Key { usage: 0xE0, down: true, mods: 1 }]);
    out.clear();
    h.report(&[0, 0, 0, 0, 0, 0, 0, 0], 9, &mut out);
    assert_eq!(out, vec![Event::Key { usage: 0x0B, down: false, mods: 0 }, Event::Key { usage: 0xE0, down: false, mods: 0 }]);
    // the US layout
    assert_eq!(usage_char(0x04, false), Some('a'));
    assert_eq!(usage_char(0x1D, true), Some('Z'));
    assert_eq!(usage_char(0x1F, true), Some('@'));
    assert_eq!(usage_char(0x27, false), Some('0'));
    assert_eq!(usage_char(0x34, true), Some('"'));
    assert_eq!(usage_char(0x38, true), Some('?'));
    assert_eq!(usage_char(0x62, false), Some('0'));
    assert_eq!(usage_char(0x28, false), None);
}
