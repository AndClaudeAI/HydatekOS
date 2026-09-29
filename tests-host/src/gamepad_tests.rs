//! Game controllers: Xbox and HID reports, navigation, rumble packets and
//! motor timing.

use crate::gamepad::*;
use crate::haptics::{pattern, Haptic, Strength};
use crate::hid::{Descriptor, Gamepad};

/// A generic HID gamepad: X/Y bytes, an 8-way hat (null state), 12 buttons.
const HID_PAD: &[u8] = &[
    0x05, 0x01, 0x09, 0x05, 0xA1, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x09, 0x30, 0x09, 0x31, 0x75, 0x08, 0x95, 0x02, 0x81, 0x02, 0x09, 0x39, 0x15, 0x00, 0x25, 0x07, 0x75, 0x04, 0x95, 0x01, 0x81,
    0x42, 0x75, 0x04, 0x81, 0x03, 0x05, 0x09, 0x19, 0x01, 0x29, 0x0C, 0x15, 0x00, 0x25, 0x01, 0x75, 0x01, 0x95, 0x0C, 0x81, 0x02, 0x75, 0x04, 0x95, 0x01, 0x81, 0x03, 0xC0,
];

#[test]
fn hid_gamepad() {
    let d = Descriptor::parse(HID_PAD);
    let g = Gamepad::find(&d).expect("a gamepad");
    // stick hard left, hat right (2), buttons 1 and 10
    let r = g.read(&[0x00, 0x80, 0x02, 0x01, 0x02], d.ids).unwrap();
    assert_eq!((r.x, r.hat, r.buttons), (-1000, Some(2), 1 | 1 << 9));
    assert!(r.y.abs() < 10);
    // hat out of range: centred
    let r = g.read(&[0x80, 0x80, 0x0F, 0, 0], d.ids).unwrap();
    assert_eq!(r.hat, None);
    // Sony order: button 1 is X, 2 is A, 10 is Start
    let p = from_hid(&g.read(&[0x80, 0x80, 0x0F, 0x02, 0x02], d.ids).unwrap(), true);
    assert_eq!(p.buttons, A | START);
    let p = from_hid(&g.read(&[0x80, 0x80, 0x06, 0x01, 0], d.ids).unwrap(), false);
    assert_eq!(p.buttons, A | LEFT);
}

#[test]
fn xbox_reports() {
    // Xbox 360: D-pad up, A, left stick full right and up
    let mut r = [0u8; 20];
    r[1] = 20;
    r[2] = 0x01;
    r[3] = 0x10;
    r[6..8].copy_from_slice(&32767i16.to_le_bytes());
    r[8..10].copy_from_slice(&32767i16.to_le_bytes());
    r[5] = 255;
    let p = xbox360(&r).unwrap();
    assert_eq!(p.buttons, UP | A);
    assert_eq!((p.lx, p.ly, p.rt), (1000, -1000, 1000));
    assert_eq!(xbox360(&[1, 3, 0]), None);

    // Xbox One: Menu, B, D-pad down, LB
    let mut r = [0u8; 18];
    r[0] = 0x20;
    r[3] = 14;
    r[4] = 0x04 | 0x20;
    r[5] = 0x02 | 0x10;
    r[12..14].copy_from_slice(&(-32768i16).to_le_bytes());
    let p = xbox_one(&r).unwrap();
    assert_eq!(p.buttons, START | B | DOWN | LB);
    assert_eq!(p.ly, 1000);
    assert_eq!(xbox_one_guide(&[0x07, 0x20, 1, 2, 1, 0x5B]), Some(true));
    assert_eq!(xbox_one_guide(&r), None);
}

#[test]
fn navigation() {
    let mut n = Navigator::new();
    let pad = |buttons, ly| Pad { buttons, ly, ..Default::default() };
    assert_eq!(n.feed(&pad(A, 0), 0), vec![Nav::Accept]);
    // held: not again
    assert_eq!(n.feed(&pad(A, 0), 16), vec![]);
    assert_eq!(n.feed(&pad(0, 0), 32), vec![]);
    // the stick pushed down, held: once, then repeats after 400 ms
    assert_eq!(n.feed(&pad(0, 900), 100), vec![Nav::Down]);
    assert_eq!(n.feed(&pad(0, 900), 300), vec![]);
    assert_eq!(n.feed(&pad(0, 900), 500), vec![Nav::Down]);
    assert_eq!(n.feed(&pad(0, 900), 560), vec![]);
    assert_eq!(n.feed(&pad(0, 900), 620), vec![Nav::Down]);
    // a gentle lean is ignored
    assert_eq!(n.feed(&pad(0, 300), 700), vec![]);
    assert_eq!(n.feed(&pad(START | B, 0), 720), vec![Nav::Back, Nav::Menu]);
}

#[test]
fn rumble_packets() {
    assert_eq!(rumble(Rumble::Xbox360, 200, 100, 0), vec![0, 8, 0, 200, 100, 0, 0, 0]);
    let one = rumble(Rumble::XboxOne, 255, 51, 7);
    assert_eq!(&one[..6], &[0x09, 0x00, 7, 0x09, 0x00, 0x0F]);
    assert_eq!((one[8], one[9]), (100, 20));
    assert_eq!(xbox_one_start(0), vec![0x05, 0x20, 0x00, 0x01, 0x00]);
    let ds4 = rumble(Rumble::DualShock4, 255, 128, 0);
    assert_eq!((ds4.len(), ds4[0], ds4[4], ds4[5]), (32, 0x05, 128, 255));
    let ds = rumble(Rumble::DualSense, 90, 30, 0);
    assert_eq!((ds.len(), ds[0], ds[3], ds[4]), (48, 0x02, 30, 90));
}

#[test]
fn motor_timing() {
    let mut m = Motor::default();
    // Success: two pulses, 12 ms then a 70 ms gap, then 18 ms
    let p = pattern(Haptic::Success, Strength::Strong);
    m.play(&p, 1000);
    assert_eq!(m.due(1000), Some(p[0].amp));
    assert_eq!(m.due(1020), None);
    // short pulses are stretched to 40 ms so a motor spins up
    assert_eq!(m.due(1040), Some(0));
    assert_eq!(m.due(1110), Some(p[1].amp));
    assert_eq!(m.due(1150), Some(0));
    assert_eq!(m.due(5000), None);
    // late polls skip to the latest state
    m.play(&p, 0);
    assert_eq!(m.due(10_000), Some(0));
    assert_eq!(split(0), (0, 0));
    assert_eq!(split(100), (0, 100));
    assert_eq!(split(255), (255, 255));
}
