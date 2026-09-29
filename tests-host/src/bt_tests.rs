//! Bluetooth HCI: commands out, events in, an adapter starting and scanning
//! (the packets as the Bluetooth Core specification lays them out).

use crate::bt::*;

/// A Command Complete event for `cmd` with these return parameters.
fn complete(cmd: (u16, u16), ret: &[u8]) -> Vec<u8> {
    let op = cmd.0 << 10 | cmd.1;
    let mut p = vec![0x0E, (3 + ret.len()) as u8, 1, op as u8, (op >> 8) as u8];
    p.extend_from_slice(ret);
    p
}

#[test]
fn commands() {
    assert_eq!(command(RESET.0, RESET.1, &[]), vec![0x03, 0x0C, 0]);
    assert_eq!(command(LE_SCAN_ENABLE.0, LE_SCAN_ENABLE.1, &[1, 0]), vec![0x0C, 0x20, 2, 1, 0]);
    assert_eq!(command(INQUIRY.0, INQUIRY.1, &[0x33, 0x8B, 0x9E, 8, 0])[..3], [0x01, 0x04, 5]);
}

#[test]
fn adapter_starts() {
    let mut a = Adapter::new();
    // one command at a time: the next waits for the answer
    let c = a.next().unwrap();
    assert_eq!(c, vec![0x03, 0x0C, 0]);
    assert!(a.next().is_none());
    a.event(&complete(RESET, &[0]));
    assert_eq!(a.next().unwrap()[..2], [0x09, 0x10]);
    a.event(&complete(READ_BD_ADDR, &[0, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]));
    assert_eq!(a.address(), "11:22:33:44:55:66");
    a.next().unwrap();
    a.event(&complete(READ_VERSION, &[0, 12, 0, 0, 12, 0x0A, 0, 0, 0]));
    assert_eq!(version_name(a.version), "Bluetooth 5.3");
    a.next().unwrap();
    let mut name = vec![0u8; 249];
    name[1..8].copy_from_slice(b"Hydatek");
    a.event(&complete(READ_NAME, &name));
    assert_eq!(a.name, "Hydatek");
    while let Some(c) = a.next() {
        let op = u16::from_le_bytes([c[0], c[1]]);
        a.event(&complete((op >> 10, op & 0x3FF), &[0]));
    }
    assert!(a.ready);
    a.scan();
    assert_eq!(a.next().unwrap()[..2], [0x0C, 0x20]);
}

#[test]
fn devices_nearby() {
    let mut a = Adapter::new();
    // an LE advertisement: flags, complete name, appearance (mouse), Logitech data
    let mut data = vec![2, 0x01, 0x06];
    data.extend_from_slice(&[9, 0x09]);
    data.extend_from_slice(b"MX Mouse");
    data.extend_from_slice(&[3, 0x19, 0xC2, 0x03, 5, 0xFF, 0x46, 0x00, 1, 2]);
    let mut ev = vec![0x3E, 0, 0x02, 1, 0x00, 0x01, 1, 2, 3, 4, 5, 6, data.len() as u8];
    ev.extend_from_slice(&data);
    ev.push((-48i8) as u8);
    ev[1] = (ev.len() - 2) as u8;
    a.event(&ev);
    // an extended inquiry result: headphones (class 0x240404), named in the EIR
    let mut ex = vec![0x2F, 0, 1, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 1, 0, 0x04, 0x04, 0x24, 0, 0, (-70i8) as u8];
    ex.extend_from_slice(&[12, 0x09]);
    ex.extend_from_slice(b"Studio Buds");
    ex.resize(2 + 255, 0);
    ex[1] = 255;
    a.event(&ex);
    // two phones in one plain inquiry result (fields listed device by device per field)
    let mut two = vec![0x02, 0, 2];
    two.extend_from_slice(&[1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2]);
    two.extend_from_slice(&[0, 0]);
    two.extend_from_slice(&[0, 0, 0, 0]);
    two.extend_from_slice(&[0x0C, 0x02, 0x5A, 0x0C, 0x01, 0x5A]);
    two.extend_from_slice(&[0, 0, 0, 0]);
    two[1] = (two.len() - 2) as u8;
    a.event(&two);
    assert_eq!(a.nearby.len(), 4, "{:?}", a.nearby);
    let mouse = a.nearby.iter().find(|n| n.le).unwrap();
    assert_eq!((mouse.label().as_str(), mouse.kind, mouse.maker, mouse.rssi), ("MX Mouse", Kind::Mouse, "Logitech", -48));
    assert_eq!(mouse.address(), "06:05:04:03:02:01");
    let buds = a.nearby.iter().find(|n| n.name == "Studio Buds").unwrap();
    assert_eq!((buds.kind, buds.rssi), (Kind::Headphones, -70));
    // the strongest first
    assert_eq!(a.nearby[0].name, "MX Mouse");
    assert_eq!(a.nearby.iter().filter(|n| n.kind == Kind::Phone).count(), 1);
    assert_eq!(a.nearby.iter().filter(|n| n.kind == Kind::Computer).count(), 1);
    // seen again: updated, not added
    a.event(&ev);
    assert_eq!(a.nearby.len(), 4);
    // truncated packets don't panic
    for n in 0..ev.len() {
        let _ = event(&ev[..n]);
    }
    for n in 0..two.len() {
        let mut t = two[..n].to_vec();
        if t.len() > 1 {
            t[1] = (t.len() - 2) as u8;
        }
        let _ = event(&t);
    }
}

#[test]
fn kinds() {
    assert_eq!(kind_from_class(0x5A020C), Kind::Phone);
    assert_eq!(kind_from_class(0x002540), Kind::Keyboard);
    assert_eq!(kind_from_class(0x002580), Kind::Mouse);
    assert_eq!(kind_from_appearance(0x03C4), Kind::Gamepad);
    assert_eq!(kind_from_appearance(0x00C0), Kind::Watch);
    assert_eq!(company(0x004C), "Apple");
}
