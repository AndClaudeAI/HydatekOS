//! Bluetooth: talking to an adapter through HCI, and finding what's nearby.
//!
//! Every Bluetooth adapter (USB dongles and the ones built into laptops,
//! which are also on USB inside) speaks the same Host Controller Interface.
//! This is HydatekOS's host side of it:
//! - starting the adapter: reset, its address, version and name;
//! - finding devices: classic inquiry (with names from extended inquiry
//!   results) and Bluetooth LE scanning (advertising reports);
//! - what each device is: its name, its kind (from the classic class of
//!   device, or LE appearance and services), its maker, its signal.
//!
//! Plain logic, host-tested; usb.rs moves the packets. Pairing and
//! connecting (L2CAP, SMP, profiles) come next.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// An HCI command packet (without the USB/UART packet type).
pub fn command(ogf: u16, ocf: u16, params: &[u8]) -> Vec<u8> {
    let op = ogf << 10 | ocf;
    let mut v = alloc::vec![op as u8, (op >> 8) as u8, params.len() as u8];
    v.extend_from_slice(params);
    v
}

pub const RESET: (u16, u16) = (0x03, 0x003);
pub const READ_BD_ADDR: (u16, u16) = (0x04, 0x009);
pub const READ_VERSION: (u16, u16) = (0x04, 0x001);
pub const READ_NAME: (u16, u16) = (0x03, 0x014);
pub const WRITE_INQUIRY_MODE: (u16, u16) = (0x03, 0x045);
pub const SET_EVENT_MASK: (u16, u16) = (0x03, 0x001);
pub const LE_SET_EVENT_MASK: (u16, u16) = (0x08, 0x001);
pub const LE_SCAN_PARAMS: (u16, u16) = (0x08, 0x00B);
pub const LE_SCAN_ENABLE: (u16, u16) = (0x08, 0x00C);
pub const INQUIRY: (u16, u16) = (0x01, 0x001);

fn op(c: (u16, u16)) -> u16 {
    c.0 << 10 | c.1
}

/// What kind of device it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Phone,
    Computer,
    Headphones,
    Speaker,
    Keyboard,
    Mouse,
    Gamepad,
    Watch,
    Tv,
    Unknown,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Phone => "Phone",
            Kind::Computer => "Computer",
            Kind::Headphones => "Headphones",
            Kind::Speaker => "Speaker",
            Kind::Keyboard => "Keyboard",
            Kind::Mouse => "Mouse",
            Kind::Gamepad => "Game controller",
            Kind::Watch => "Watch",
            Kind::Tv => "TV",
            Kind::Unknown => "Device",
        }
    }
}

/// A classic device's kind from its class of device.
pub fn kind_from_class(cod: u32) -> Kind {
    let (major, minor) = ((cod >> 8) & 0x1F, (cod >> 2) & 0x3F);
    match major {
        1 => Kind::Computer,
        2 => Kind::Phone,
        4 => match minor {
            1 | 2 | 6 => Kind::Headphones,
            5 | 7 => Kind::Speaker,
            12 | 15 => Kind::Tv,
            _ => Kind::Speaker,
        },
        5 => match (minor >> 4) & 3 {
            1 => Kind::Keyboard,
            2 => Kind::Mouse,
            _ if minor & 0xF == 1 || minor & 0xF == 2 => Kind::Gamepad,
            _ => Kind::Keyboard,
        },
        7 => Kind::Watch,
        _ => Kind::Unknown,
    }
}

/// An LE device's kind from its appearance.
pub fn kind_from_appearance(a: u16) -> Kind {
    match (a >> 6, a & 0x3F) {
        (1, _) => Kind::Phone,
        (2, _) => Kind::Computer,
        (3, _) => Kind::Watch,
        (0x0F, 1) => Kind::Keyboard,
        (0x0F, 2) => Kind::Mouse,
        (0x0F, 3) | (0x0F, 4) => Kind::Gamepad,
        (0x21, 1) | (0x21, 3) => Kind::Speaker,
        (0x21, _) | (0x25, _) => Kind::Headphones,
        (0x0A, _) => Kind::Tv,
        _ => Kind::Unknown,
    }
}

/// A maker, from the company id in manufacturer data.
pub fn company(id: u16) -> &'static str {
    match id {
        0x004C => "Apple",
        0x0006 => "Microsoft",
        0x0075 => "Samsung",
        0x00E0 => "Google",
        0x012D => "Sony",
        0x009E => "Bose",
        0x0046 => "Logitech",
        0x0157 => "Huawei",
        0x038F => "Xiaomi",
        0x02E5 => "Espressif",
        0x0059 => "Nordic",
        0x000F => "Broadcom",
        0x001D => "Qualcomm",
        0x0002 => "Intel",
        _ => "",
    }
}

/// A device found nearby.
#[derive(Clone, Debug, PartialEq)]
pub struct Nearby {
    pub addr: [u8; 6],
    pub name: String,
    pub kind: Kind,
    pub maker: &'static str,
    pub rssi: i8,
    /// found by LE advertising (else classic inquiry)
    pub le: bool,
}

impl Nearby {
    pub fn address(&self) -> String {
        let a = self.addr;
        format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", a[5], a[4], a[3], a[2], a[1], a[0])
    }

    /// What Settings shows: its name, or its maker and kind.
    pub fn label(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if !self.maker.is_empty() {
            format!("{} {}", self.maker, self.kind.name().to_lowercase())
        } else {
            format!("{} ({})", self.kind.name(), self.address())
        }
    }
}

/// Advertising / extended inquiry data: name, kind, maker.
pub fn advertising(d: &[u8], n: &mut Nearby) {
    let mut i = 0;
    while i < d.len() {
        let len = d[i] as usize;
        if len == 0 || i + 1 + len > d.len() {
            break;
        }
        let (t, v) = (d[i + 1], &d[i + 2..i + 1 + len]);
        match t {
            0x08 if n.name.is_empty() => n.name = String::from_utf8_lossy(v).into(),
            0x09 => n.name = String::from_utf8_lossy(v).into(),
            0x19 if v.len() >= 2 => {
                let k = kind_from_appearance(u16::from_le_bytes([v[0], v[1]]));
                if k != Kind::Unknown {
                    n.kind = k;
                }
            }
            0x02 | 0x03 => {
                for u in v.chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_le_bytes([c[0], c[1]])) {
                    match u {
                        0x1812 if n.kind == Kind::Unknown => n.kind = Kind::Keyboard,
                        0x110B | 0x110D | 0x111E if n.kind == Kind::Unknown => n.kind = Kind::Headphones,
                        _ => {}
                    }
                }
            }
            0xFF if v.len() >= 2 => n.maker = company(u16::from_le_bytes([v[0], v[1]])),
            _ => {}
        }
        i += 1 + len;
    }
}

/// An HCI event.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Complete { opcode: u16, status: u8, ret: Vec<u8> },
    Status { opcode: u16, status: u8 },
    Found(Vec<Nearby>),
    InquiryDone,
    Other(u8),
}

fn addr(b: &[u8]) -> [u8; 6] {
    b[..6].try_into().unwrap()
}

/// Read an HCI event packet (code, length, parameters).
pub fn event(p: &[u8]) -> Option<Event> {
    if p.len() < 2 || p.len() < 2 + p[1] as usize {
        return None;
    }
    let (code, d) = (p[0], &p[2..2 + p[1] as usize]);
    Some(match code {
        0x0E if d.len() >= 4 => Event::Complete { opcode: u16::from_le_bytes([d[1], d[2]]), status: d[3], ret: d[4..].to_vec() },
        0x0F if d.len() >= 4 => Event::Status { status: d[0], opcode: u16::from_le_bytes([d[2], d[3]]) },
        0x01 => Event::InquiryDone,
        // inquiry results: plain (0x02) and with RSSI (0x22) list each field
        // for every device in turn; extended (0x2F) is one device and its data
        0x02 | 0x22 if !d.is_empty() => {
            let n = d[0] as usize;
            let rssi_at = code == 0x22;
            // bytes per field: address, page scan mode, reserved, class, clock, (rssi)
            let reserved = if rssi_at { 1 } else { 2 };
            let per = 6 + 1 + reserved + 3 + 2 + rssi_at as usize;
            if d.len() < 1 + n * per {
                return Some(Event::Found(Vec::new()));
            }
            let (a0, cod0) = (1, 1 + n * (7 + reserved));
            let rssi0 = cod0 + n * 5;
            let out = (0..n)
                .map(|k| {
                    let c = &d[cod0 + 3 * k..];
                    let cod = c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16;
                    let rssi = if rssi_at { d[rssi0 + k] as i8 } else { -127 };
                    Nearby { addr: addr(&d[a0 + 6 * k..]), name: String::new(), kind: kind_from_class(cod), maker: "", rssi, le: false }
                })
                .collect();
            Event::Found(out)
        }
        0x2F if d.len() >= 15 => {
            let e = &d[1..];
            let cod = e[8] as u32 | (e[9] as u32) << 8 | (e[10] as u32) << 16;
            let mut dev = Nearby { addr: addr(e), name: String::new(), kind: kind_from_class(cod), maker: "", rssi: e[13] as i8, le: false };
            advertising(&d[15..], &mut dev);
            Event::Found(alloc::vec![dev])
        }
        // LE advertising reports
        0x3E if d.first() == Some(&0x02) && d.len() >= 2 => {
            let mut out = Vec::new();
            let mut i = 2;
            for _ in 0..d[1] {
                if i + 9 > d.len() {
                    break;
                }
                let len = d[i + 8] as usize;
                if i + 9 + len + 1 > d.len() {
                    break;
                }
                let mut dev = Nearby { addr: addr(&d[i + 2..]), name: String::new(), kind: Kind::Unknown, maker: "", rssi: d[i + 9 + len] as i8, le: true };
                advertising(&d[i + 9..i + 9 + len], &mut dev);
                out.push(dev);
                i += 10 + len;
            }
            Event::Found(out)
        }
        c => Event::Other(c),
    })
}

/// An adapter being brought up and scanning.
#[derive(Default)]
pub struct Adapter {
    /// commands not yet sent, and the one waiting for its answer
    queue: Vec<Vec<u8>>,
    waiting: Option<u16>,
    pub addr: [u8; 6],
    pub name: String,
    /// Bluetooth version (HCI version: 6 = 4.0 … 12 = 5.3, 13 = 5.4)
    pub version: u8,
    pub ready: bool,
    pub nearby: Vec<Nearby>,
    pub error: Option<String>,
}

pub fn version_name(v: u8) -> &'static str {
    match v {
        0..=3 => "Bluetooth 2.0 or older",
        4 => "Bluetooth 2.1",
        5 => "Bluetooth 3.0",
        6 => "Bluetooth 4.0",
        7 => "Bluetooth 4.1",
        8 => "Bluetooth 4.2",
        9 => "Bluetooth 5.0",
        10 => "Bluetooth 5.1",
        11 => "Bluetooth 5.2",
        12 => "Bluetooth 5.3",
        _ => "Bluetooth 5.4 or newer",
    }
}

impl Adapter {
    pub fn new() -> Adapter {
        let mut a = Adapter::default();
        a.queue = alloc::vec![
            command(RESET.0, RESET.1, &[]),
            command(READ_BD_ADDR.0, READ_BD_ADDR.1, &[]),
            command(READ_VERSION.0, READ_VERSION.1, &[]),
            command(READ_NAME.0, READ_NAME.1, &[]),
            // all events, including LE meta events
            command(SET_EVENT_MASK.0, SET_EVENT_MASK.1, &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBF, 0x3D]),
            command(LE_SET_EVENT_MASK.0, LE_SET_EVENT_MASK.1, &[0x1F, 0, 0, 0, 0, 0, 0, 0]),
            // names come with extended inquiry results
            command(WRITE_INQUIRY_MODE.0, WRITE_INQUIRY_MODE.1, &[2]),
        ];
        a
    }

    /// Look for devices: LE scanning (active, so names come too) and a
    /// classic inquiry of about ten seconds.
    pub fn scan(&mut self) {
        self.queue.push(command(LE_SCAN_ENABLE.0, LE_SCAN_ENABLE.1, &[0, 0]));
        self.queue.push(command(LE_SCAN_PARAMS.0, LE_SCAN_PARAMS.1, &[1, 0x60, 0, 0x30, 0, 0, 0]));
        self.queue.push(command(LE_SCAN_ENABLE.0, LE_SCAN_ENABLE.1, &[1, 0]));
        self.queue.push(command(INQUIRY.0, INQUIRY.1, &[0x33, 0x8B, 0x9E, 8, 0]));
    }

    /// The next command to send, when the adapter's ready for it.
    pub fn next(&mut self) -> Option<Vec<u8>> {
        if self.waiting.is_some() || self.queue.is_empty() {
            return None;
        }
        let c = self.queue.remove(0);
        self.waiting = Some(u16::from_le_bytes([c[0], c[1]]));
        Some(c)
    }

    /// An event from the adapter.
    pub fn event(&mut self, p: &[u8]) {
        let Some(e) = event(p) else { return };
        match e {
            Event::Complete { opcode, status, ret } => {
                if self.waiting == Some(opcode) {
                    self.waiting = None;
                }
                if status != 0 && opcode == op(RESET) {
                    self.error = Some(format!("the adapter refused to reset ({:#04x})", status));
                    return;
                }
                if status != 0 {
                    return;
                }
                match opcode {
                    o if o == op(READ_BD_ADDR) && ret.len() >= 6 => self.addr = addr(&ret),
                    o if o == op(READ_VERSION) && !ret.is_empty() => self.version = ret[0],
                    o if o == op(READ_NAME) => self.name = String::from_utf8_lossy(&ret[..ret.iter().position(|b| *b == 0).unwrap_or(ret.len())]).into(),
                    o if o == op(WRITE_INQUIRY_MODE) => self.ready = true,
                    _ => {}
                }
            }
            Event::Status { opcode, .. } => {
                if self.waiting == Some(opcode) {
                    self.waiting = None;
                }
            }
            Event::Found(list) => {
                for d in list {
                    let room = self.nearby.len() < 64;
                    match self.nearby.iter_mut().find(|n| n.addr == d.addr) {
                        Some(n) => {
                            n.rssi = d.rssi;
                            if !d.name.is_empty() {
                                n.name = d.name;
                            }
                            if d.kind != Kind::Unknown {
                                n.kind = d.kind;
                            }
                            if !d.maker.is_empty() {
                                n.maker = d.maker;
                            }
                        }
                        None if room => self.nearby.push(d),
                        None => {}
                    }
                }
                // strongest first
                self.nearby.sort_by(|a, b| b.rssi.cmp(&a.rssi));
            }
            _ => {}
        }
    }

    pub fn address(&self) -> String {
        let a = self.addr;
        format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", a[5], a[4], a[3], a[2], a[1], a[0])
    }
}
