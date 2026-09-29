//! HID: the Human Interface Device format that mice, keyboards, touchpads,
//! touch screens, pens, gamepads, media keys and haptic touchpads use to
//! describe themselves, over USB (usb.rs) and over I²C (i2c_hid.rs).
//!
//! A device sends a *report descriptor*: a small program saying which
//! controls it has ("X, relative, 8 bits"; "finger contact: tip switch, X, Y")
//! and where each sits in its reports. `Descriptor::parse` reads it into
//! fields; the `Mouse`, `Touchpad`, `Consumer` and `HapticController`
//! readers below pick the controls HydatekOS uses and decode reports.
//!
//! Plain functions over bytes: the host tests run real devices' descriptors
//! through them.

use alloc::vec::Vec;

/// A usage: page in the high 16 bits, id in the low.
pub type Usage = u32;

pub const fn usage(page: u16, id: u16) -> Usage {
    (page as u32) << 16 | id as u32
}

// pages
pub const GENERIC_DESKTOP: u16 = 0x01;
pub const BUTTON: u16 = 0x09;
pub const CONSUMER: u16 = 0x0C;
pub const DIGITIZER: u16 = 0x0D;
pub const HAPTICS: u16 = 0x0E;
pub const ORDINAL: u16 = 0x0A;

// application collections
pub const APP_POINTER: Usage = usage(GENERIC_DESKTOP, 0x01);
pub const APP_MOUSE: Usage = usage(GENERIC_DESKTOP, 0x02);
pub const APP_JOYSTICK: Usage = usage(GENERIC_DESKTOP, 0x04);
pub const APP_GAMEPAD: Usage = usage(GENERIC_DESKTOP, 0x05);
pub const APP_KEYBOARD: Usage = usage(GENERIC_DESKTOP, 0x06);
pub const APP_PEN: Usage = usage(DIGITIZER, 0x02);
pub const APP_TOUCHSCREEN: Usage = usage(DIGITIZER, 0x04);
pub const APP_TOUCHPAD: Usage = usage(DIGITIZER, 0x05);
pub const APP_CONSUMER: Usage = usage(CONSUMER, 0x01);
pub const APP_HAPTIC: Usage = usage(HAPTICS, 0x01);

// controls
pub const X: Usage = usage(GENERIC_DESKTOP, 0x30);
pub const Y: Usage = usage(GENERIC_DESKTOP, 0x31);
pub const WHEEL: Usage = usage(GENERIC_DESKTOP, 0x38);
pub const HAT: Usage = usage(GENERIC_DESKTOP, 0x39);
pub const SENSORS: u16 = 0x20;
pub const APP_SENSOR_HUB: Usage = usage(SENSORS, 0x01);
pub const APP_LIGHT: Usage = usage(SENSORS, 0x41);
pub const ILLUMINANCE: Usage = usage(SENSORS, 0x04D1);
pub const COLOR_TEMPERATURE: Usage = usage(SENSORS, 0x04D2);
pub const REPORTING_STATE: Usage = usage(SENSORS, 0x0316);
pub const POWER_STATE: Usage = usage(SENSORS, 0x0319);
pub const IN_RANGE: Usage = usage(DIGITIZER, 0x32);
pub const TIP_PRESSURE: Usage = usage(DIGITIZER, 0x30);
pub const BARREL: Usage = usage(DIGITIZER, 0x44);
pub const ERASER: Usage = usage(DIGITIZER, 0x45);
pub const INVERT: Usage = usage(DIGITIZER, 0x3C);
pub const X_TILT: Usage = usage(DIGITIZER, 0x3D);
pub const Y_TILT: Usage = usage(DIGITIZER, 0x3E);
pub const AC_PAN: Usage = usage(CONSUMER, 0x238);
pub const TIP: Usage = usage(DIGITIZER, 0x42);
pub const CONFIDENCE: Usage = usage(DIGITIZER, 0x47);
pub const CONTACT_ID: Usage = usage(DIGITIZER, 0x51);
pub const CONTACT_COUNT: Usage = usage(DIGITIZER, 0x54);
pub const FINGER: Usage = usage(DIGITIZER, 0x22);
pub const WAVEFORM_LIST: Usage = usage(HAPTICS, 0x10);
pub const MANUAL_TRIGGER: Usage = usage(HAPTICS, 0x21);
pub const INTENSITY: Usage = usage(HAPTICS, 0x23);
pub const REPEAT_COUNT: Usage = usage(HAPTICS, 0x24);
pub const RETRIGGER_PERIOD: Usage = usage(HAPTICS, 0x25);
// waveforms (Haptics page)
pub const WAVE_CLICK: u16 = 0x1003;
pub const WAVE_BUZZ: u16 = 0x1004;
pub const WAVE_RUMBLE: u16 = 0x1005;
pub const WAVE_PRESS: u16 = 0x1006;
#[allow(dead_code)]
pub const WAVE_RELEASE: u16 = 0x1007;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Input,
    Output,
    Feature,
}

/// One control in a report.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Field {
    pub kind: Kind,
    pub report_id: u8,
    /// position in the report after the report id byte, in bits
    pub bit: u32,
    pub size: u8,
    pub usage: Usage,
    /// an array field: the value is a usage between `usage` and `usage_max`
    pub usage_max: Usage,
    pub min: i32,
    pub max: i32,
    /// constant (padding), variable, relative
    pub constant: bool,
    pub variable: bool,
    pub relative: bool,
    /// the application collection it's in (APP_MOUSE, APP_TOUCHPAD…)
    pub app: Usage,
    /// which logical / physical collection (a finger's contact) it's in
    pub coll: u16,
    /// that collection's usage (FINGER, WAVEFORM_LIST…)
    pub coll_usage: Usage,
    /// unit exponent: the value is value × 10^exp (sensors)
    pub exp: i8,
}

#[derive(Clone, Debug, Default)]
pub struct Descriptor {
    pub fields: Vec<Field>,
    /// reports start with a report id byte
    pub ids: bool,
    /// the application collections, in order
    pub apps: Vec<Usage>,
}

#[derive(Clone, Copy, Default)]
struct Globals {
    page: u16,
    min: i32,
    max: i32,
    size: u32,
    count: u32,
    id: u8,
    exp: i8,
}

fn sext(v: u32, bytes: usize) -> i32 {
    match bytes {
        1 => v as u8 as i8 as i32,
        2 => v as u16 as i16 as i32,
        _ => v as i32,
    }
}

impl Descriptor {
    /// Read a report descriptor. Malformed ones give what could be read.
    pub fn parse(d: &[u8]) -> Descriptor {
        let mut out = Descriptor::default();
        let mut g = Globals::default();
        let mut stack: Vec<Globals> = Vec::new();
        let mut usages: Vec<Usage> = Vec::new();
        let (mut umin, mut umax): (Option<Usage>, Option<Usage>) = (None, None);
        // bit positions per (kind, report id)
        let mut pos: Vec<(Kind, u8, u32)> = Vec::new();
        // collections: (type, usage, instance)
        let mut colls: Vec<(u8, Usage, u16)> = Vec::new();
        let mut next_coll: u16 = 0;
        let mut i = 0;
        while i < d.len() {
            let b = d[i];
            if b == 0xFE {
                // long item: skip it
                let len = *d.get(i + 1).unwrap_or(&0) as usize;
                i += 3 + len;
                continue;
            }
            let n = [0usize, 1, 2, 4][(b & 3) as usize];
            if i + 1 + n > d.len() {
                break;
            }
            let mut v = 0u32;
            for k in 0..n {
                v |= (d[i + 1 + k] as u32) << (8 * k);
            }
            let (typ, tag) = ((b >> 2) & 3, b >> 4);
            let full = |u: u32, page: u16| if n == 4 { u } else { usage(page, u as u16) };
            match (typ, tag) {
                // global items
                (1, 0) => g.page = v as u16,
                (1, 1) => g.min = sext(v, n),
                (1, 2) => g.max = if g.min < 0 { sext(v, n) } else { v as i32 },
                (1, 5) => g.exp = if n == 1 && v < 16 { ((v as i8) << 4) >> 4 } else { sext(v, n) as i8 },
                (1, 7) => g.size = v.min(32),
                (1, 8) => {
                    g.id = v as u8;
                    out.ids = true;
                }
                (1, 9) => g.count = v.min(4096),
                (1, 10) => stack.push(g),
                (1, 11) => g = stack.pop().unwrap_or(g),
                // local items
                (2, 0) => usages.push(full(v, g.page)),
                (2, 1) => umin = Some(full(v, g.page)),
                (2, 2) => umax = Some(full(v, g.page)),
                // main items
                (0, 0xA) => {
                    let u = usages.first().copied().or(umin).unwrap_or(0);
                    if v == 1 && !out.apps.contains(&u) {
                        out.apps.push(u);
                    }
                    next_coll += 1;
                    colls.push((v as u8, u, next_coll));
                    usages.clear();
                    umin = None;
                    umax = None;
                }
                (0, 0xC) => {
                    colls.pop();
                }
                (0, 8) | (0, 9) | (0, 0xB) => {
                    let kind = match tag {
                        8 => Kind::Input,
                        9 => Kind::Output,
                        _ => Kind::Feature,
                    };
                    let flags = v;
                    let (constant, variable, relative) = (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0);
                    let app = colls.iter().find(|c| c.0 == 1).map(|c| c.1).unwrap_or(0);
                    let (coll, coll_usage) = colls.iter().rev().find(|c| c.0 != 1).map(|c| (c.2, c.1)).unwrap_or((0, 0));
                    let at = match pos.iter().position(|p| p.0 == kind && p.1 == g.id) {
                        Some(k) => k,
                        None => {
                            pos.push((kind, g.id, 0));
                            pos.len() - 1
                        }
                    };
                    let base = Field { kind, report_id: g.id, bit: 0, size: g.size as u8, usage: 0, usage_max: 0, min: g.min, max: g.max, constant, variable, relative, app, coll, coll_usage, exp: g.exp };
                    if !constant && !variable {
                        // an array: `count` slots, each holding a usage
                        let lo = umin.or(usages.first().copied()).unwrap_or(0);
                        let hi = umax.or(usages.last().copied()).unwrap_or(lo);
                        for _ in 0..g.count {
                            out.fields.push(Field { bit: pos[at].2, usage: lo, usage_max: hi, ..base });
                            pos[at].2 += g.size;
                        }
                    } else {
                        for k in 0..g.count {
                            let u = if let Some(u) = usages.get(k as usize) {
                                *u
                            } else if let (Some(lo), Some(hi)) = (umin, umax) {
                                (lo + k).min(hi)
                            } else {
                                usages.last().copied().unwrap_or(0)
                            };
                            if !constant && u != 0 {
                                out.fields.push(Field { bit: pos[at].2, usage: u, usage_max: u, ..base });
                            }
                            pos[at].2 += g.size;
                        }
                    }
                    usages.clear();
                    umin = None;
                    umax = None;
                }
                _ => {}
            }
            i += 1 + n;
        }
        out
    }

    /// A report's length in bytes (with its id byte).
    pub fn report_len(&self, kind: Kind, id: u8) -> usize {
        let bits = self.fields.iter().filter(|f| f.kind == kind && f.report_id == id).map(|f| f.bit + f.size as u32).max().unwrap_or(0);
        ((bits + 7) / 8) as usize + self.ids as usize
    }

    pub fn find(&self, kind: Kind, u: Usage) -> Option<&Field> {
        self.fields.iter().find(|f| f.kind == kind && f.usage == u)
    }

    /// The input fields of report `id` in application `app`.
    fn inputs(&self, app: Usage) -> impl Iterator<Item = &Field> {
        self.fields.iter().filter(move |f| f.kind == Kind::Input && f.app == app)
    }
}

/// A field's value in a report (which starts with its id byte when the
/// device uses ids). Signed when its logical minimum is negative.
pub fn get(f: &Field, report: &[u8], ids: bool) -> i32 {
    let data = if ids { report.get(1..).unwrap_or(&[]) } else { report };
    let mut v: u64 = 0;
    for k in 0..f.size as u32 {
        let bit = f.bit + k;
        let byte = (bit / 8) as usize;
        if byte >= data.len() {
            break;
        }
        if data[byte] >> (bit % 8) & 1 != 0 {
            v |= 1 << k;
        }
    }
    if f.min < 0 && f.size > 0 && f.size < 32 && v >> (f.size - 1) & 1 != 0 {
        (v as i64 - (1i64 << f.size)) as i32
    } else {
        v as i32
    }
}

/// Write a field's value into a report being built.
pub fn put(f: &Field, report: &mut [u8], ids: bool, value: i32) {
    let off = ids as usize;
    for k in 0..f.size as u32 {
        let bit = f.bit + k;
        let byte = off + (bit / 8) as usize;
        if byte >= report.len() {
            break;
        }
        let mask = 1u8 << (bit % 8);
        if (value as u32) >> k & 1 != 0 {
            report[byte] |= mask;
        } else {
            report[byte] &= !mask;
        }
    }
}

/// The report id a report belongs to.
pub fn report_id(report: &[u8], ids: bool) -> u8 {
    if ids {
        report.first().copied().unwrap_or(0)
    } else {
        0
    }
}

// ---- what the device is -------------------------------------------------------

/// What a HID device is, for Settings › Devices.
pub fn describe(d: &Descriptor) -> &'static str {
    for app in &d.apps {
        let name = match *app {
            APP_TOUCHPAD => "Touchpad",
            APP_TOUCHSCREEN => "Touch screen",
            APP_PEN => "Pen",
            APP_MOUSE => "Mouse",
            APP_KEYBOARD => "Keyboard",
            APP_GAMEPAD => "Gamepad",
            APP_JOYSTICK => "Joystick",
            APP_HAPTIC => "Haptic controller",
            APP_LIGHT => "Light sensor",
            APP_SENSOR_HUB => "Sensor hub",
            _ => continue,
        };
        return name;
    }
    if d.apps.contains(&APP_POINTER) {
        return "Pointer";
    }
    if d.apps.contains(&APP_CONSUMER) {
        return "Media keys";
    }
    "HID device"
}

// ---- mice and tablets ---------------------------------------------------------------

/// A mouse, trackball or tablet (a pointer with absolute X/Y).
#[derive(Clone, Debug)]
pub struct Mouse {
    pub x: Field,
    pub y: Field,
    pub wheel: Option<Field>,
    pub pan: Option<Field>,
    pub buttons: Vec<Field>,
    /// X/Y are positions (a tablet, a virtual machine's pointer), not movements
    pub absolute: bool,
}

/// One report from a mouse or tablet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseReport {
    /// relative: movement; absolute: position scaled to 0..=32767
    pub x: i32,
    pub y: i32,
    pub wheel: i32,
    pub pan: i32,
    /// bit 0 left, 1 right, 2 middle…
    pub buttons: u32,
}

impl Mouse {
    pub fn find(d: &Descriptor) -> Option<Mouse> {
        for &app in &[APP_MOUSE, APP_POINTER] {
            let x = d.inputs(app).find(|f| f.usage == X).copied();
            let y = d.inputs(app).find(|f| f.usage == Y).copied();
            if let (Some(x), Some(y)) = (x, y) {
                let buttons = d.inputs(app).filter(|f| f.usage >> 16 == BUTTON as u32 && f.variable).copied().collect();
                return Some(Mouse {
                    absolute: !x.relative,
                    x,
                    y,
                    wheel: d.inputs(app).find(|f| f.usage == WHEEL).copied(),
                    pan: d.inputs(app).find(|f| f.usage == AC_PAN).copied(),
                    buttons,
                });
            }
        }
        None
    }

    /// Decode a report (None if it's another report id).
    pub fn read(&self, r: &[u8], ids: bool) -> Option<MouseReport> {
        if report_id(r, ids) != self.x.report_id {
            return None;
        }
        let scale = |f: &Field, v: i32| -> i32 {
            let span = (f.max - f.min).max(1) as i64;
            ((v - f.min).clamp(0, span as i32) as i64 * 32767 / span) as i32
        };
        let (mut x, mut y) = (get(&self.x, r, ids), get(&self.y, r, ids));
        if self.absolute {
            x = scale(&self.x, x);
            y = scale(&self.y, y);
        }
        let mut buttons = 0;
        for b in &self.buttons {
            let n = (b.usage & 0xFFFF).saturating_sub(1);
            if n < 32 && get(b, r, ids) != 0 {
                buttons |= 1 << n;
            }
        }
        Some(MouseReport {
            x,
            y,
            wheel: self.wheel.as_ref().map_or(0, |f| get(f, r, ids)),
            pan: self.pan.as_ref().map_or(0, |f| get(f, r, ids)),
            buttons,
        })
    }
}

// ---- pens -------------------------------------------------------------------------

/// A pen (stylus) on a pen tablet or a pen-enabled touch screen.
#[derive(Clone, Debug)]
pub struct Pen {
    x: Field,
    y: Field,
    tip: Option<Field>,
    in_range: Option<Field>,
    barrel: Option<Field>,
    eraser: Option<Field>,
    invert: Option<Field>,
    pressure: Option<Field>,
    tilt: (Option<Field>, Option<Field>),
}

/// One pen report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PenReport {
    /// position, 0..=32767 on each axis
    pub x: i32,
    pub y: i32,
    /// 0..=1000 (1000 when the pen reports no pressure but touches)
    pub pressure: i32,
    pub tip: bool,
    /// hovering over the screen
    pub in_range: bool,
    /// the side button
    pub barrel: bool,
    /// the eraser end (or the pen turned round)
    pub eraser: bool,
    /// degrees, -90..=90
    pub tilt_x: i32,
    pub tilt_y: i32,
}

impl Pen {
    pub fn find(d: &Descriptor) -> Option<Pen> {
        let ins = |u: Usage| d.fields.iter().find(|f| f.kind == Kind::Input && f.app == APP_PEN && f.usage == u).copied();
        let (x, y) = (ins(X)?, ins(Y)?);
        Some(Pen { x, y, tip: ins(TIP), in_range: ins(IN_RANGE), barrel: ins(BARREL), eraser: ins(ERASER), invert: ins(INVERT), pressure: ins(TIP_PRESSURE), tilt: (ins(X_TILT), ins(Y_TILT)) })
    }

    pub fn read(&self, r: &[u8], ids: bool) -> Option<PenReport> {
        if report_id(r, ids) != self.x.report_id {
            return None;
        }
        let scale = |f: &Field, top: i64| {
            let span = (f.max - f.min).max(1) as i64;
            ((get(f, r, ids) - f.min).clamp(0, span as i32) as i64 * top / span) as i32
        };
        let on = |f: &Option<Field>| f.as_ref().map_or(false, |f| get(f, r, ids) != 0);
        let tip = on(&self.tip);
        let pressure = match &self.pressure {
            Some(f) => scale(f, 1000),
            None => if tip { 1000 } else { 0 },
        };
        let tilt = |f: &Option<Field>| f.as_ref().map_or(0, |f| (get(f, r, ids)).clamp(-90, 90));
        Some(PenReport {
            x: scale(&self.x, 32767),
            y: scale(&self.y, 32767),
            pressure,
            tip,
            in_range: self.in_range.as_ref().map_or(true, |f| get(f, r, ids) != 0),
            barrel: on(&self.barrel),
            eraser: on(&self.eraser) || on(&self.invert),
            tilt_x: tilt(&self.tilt.0),
            tilt_y: tilt(&self.tilt.1),
        })
    }
}

// ---- light sensors ----------------------------------------------------------------

/// An ambient light sensor (a HID sensor, on its own or in a sensor hub).
#[derive(Clone, Debug)]
pub struct LightSensor {
    lux: Field,
    color: Option<Field>,
    reporting: Option<Field>,
    power: Option<Field>,
}

impl LightSensor {
    pub fn find(d: &Descriptor) -> Option<LightSensor> {
        let lux = *d.fields.iter().find(|f| f.kind == Kind::Input && f.usage == ILLUMINANCE)?;
        let feat = |u: Usage| d.fields.iter().find(|f| f.kind == Kind::Feature && f.usage == u && f.report_id == lux.report_id).copied();
        Some(LightSensor { lux, color: d.fields.iter().find(|f| f.kind == Kind::Input && f.usage == COLOR_TEMPERATURE && f.report_id == lux.report_id).copied(), reporting: feat(REPORTING_STATE), power: feat(POWER_STATE) })
    }

    /// The feature report that starts it: all events reported, full power.
    pub fn start(&self, d: &Descriptor) -> Option<Vec<u8>> {
        let f = self.reporting.or(self.power)?;
        let mut r = alloc::vec![0u8; d.report_len(Kind::Feature, f.report_id)];
        if d.ids {
            r[0] = f.report_id;
        }
        // the selectors are listed in order: "no events", "all events"…;
        // "D0" (full power) is the second power state
        if let Some(f) = &self.reporting {
            put(f, &mut r, d.ids, f.min + 1);
        }
        if let Some(f) = &self.power {
            put(f, &mut r, d.ids, f.min + 1);
        }
        Some(r)
    }

    /// Lux (and colour temperature in kelvin, if it says).
    pub fn read(&self, r: &[u8], ids: bool) -> Option<(u32, Option<u32>)> {
        if report_id(r, ids) != self.lux.report_id {
            return None;
        }
        let val = |f: &Field| {
            let v = get(f, r, ids).max(0) as i64;
            let v = match f.exp {
                e if e < 0 => v / 10i64.pow((-e) as u32),
                e => v * 10i64.pow(e as u32),
            };
            v.clamp(0, u32::MAX as i64) as u32
        };
        Some((val(&self.lux), self.color.as_ref().map(val)))
    }
}

// ---- gamepads and joysticks ------------------------------------------------------

/// A HID gamepad or joystick: a stick, a hat switch (D-pad) and buttons.
#[derive(Clone, Debug)]
pub struct Gamepad {
    x: Option<Field>,
    y: Option<Field>,
    hat: Option<Field>,
    buttons: Vec<Field>,
    report_id: u8,
}

/// One gamepad report.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadReport {
    /// the stick, -1000..=1000 each way (right and down positive)
    pub x: i32,
    pub y: i32,
    /// the D-pad: 0 up, then clockwise in eighths to 7 up-left; None centred
    pub hat: Option<u8>,
    /// button n (1-based in HID) is bit n-1
    pub buttons: u32,
}

impl Gamepad {
    pub fn find(d: &Descriptor) -> Option<Gamepad> {
        for &app in &[APP_GAMEPAD, APP_JOYSTICK] {
            let first = match d.inputs(app).find(|f| !f.constant) {
                Some(f) => *f,
                None => continue,
            };
            let rid = first.report_id;
            let ours = |u: Usage| d.inputs(app).find(|f| f.usage == u && f.report_id == rid).copied();
            let buttons: Vec<Field> = d.inputs(app).filter(|f| f.usage >> 16 == BUTTON as u32 && f.variable && f.report_id == rid).copied().collect();
            let (x, y, hat) = (ours(X), ours(Y), ours(HAT));
            if buttons.is_empty() && hat.is_none() && x.is_none() {
                continue;
            }
            return Some(Gamepad { x, y, hat, buttons, report_id: rid });
        }
        None
    }

    pub fn read(&self, r: &[u8], ids: bool) -> Option<PadReport> {
        if report_id(r, ids) != self.report_id {
            return None;
        }
        let axis = |f: &Option<Field>| -> i32 {
            let Some(f) = f else { return 0 };
            let span = (f.max - f.min).max(1) as i64;
            let v = (get(f, r, ids) - f.min).clamp(0, span as i32) as i64;
            (v * 2000 / span - 1000) as i32
        };
        let hat = self.hat.as_ref().and_then(|f| {
            let v = get(f, r, ids);
            // out of range means centred; some pads count 8 positions from 1
            (v >= f.min && v <= f.max && f.max - f.min == 7).then(|| (v - f.min) as u8)
        });
        let mut buttons = 0;
        for b in &self.buttons {
            let n = (b.usage & 0xFFFF).saturating_sub(1);
            if n < 32 && get(b, r, ids) != 0 {
                buttons |= 1 << n;
            }
        }
        Some(PadReport { x: axis(&self.x), y: axis(&self.y), hat, buttons })
    }
}

// ---- touchpads and touch screens ------------------------------------------------

/// A contact on a touchpad (Windows Precision Touchpad format).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Contact {
    pub id: u32,
    /// touching (tip switch)
    pub tip: bool,
    /// a finger, not a palm
    pub confident: bool,
    /// position in the device's units
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug)]
struct Slot {
    tip: Field,
    id: Option<Field>,
    confidence: Option<Field>,
    x: Field,
    y: Field,
}

/// A multitouch touchpad or touch screen.
#[derive(Clone, Debug)]
pub struct Touchpad {
    pub app: Usage,
    report_id: u8,
    slots: Vec<Slot>,
    count: Option<Field>,
    button: Option<Field>,
    /// the device's X / Y range
    pub max_x: i32,
    pub max_y: i32,
}

/// One report: the contacts it carries, and the pad's button.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TouchReport {
    pub contacts: Vec<Contact>,
    /// how many contacts touch in all (a frame may be split across reports)
    pub count: u32,
    pub button: bool,
}

impl Touchpad {
    pub fn find(d: &Descriptor) -> Option<Touchpad> {
        for &app in &[APP_TOUCHPAD, APP_TOUCHSCREEN] {
            let mut slots = Vec::new();
            let mut colls: Vec<u16> = d.inputs(app).filter(|f| f.coll_usage == FINGER).map(|f| f.coll).collect();
            colls.dedup();
            for c in colls {
                let in_c = |u: Usage| d.inputs(app).find(|f| f.coll == c && f.usage == u).copied();
                if let (Some(tip), Some(x), Some(y)) = (in_c(TIP), in_c(X), in_c(Y)) {
                    slots.push(Slot { tip, id: in_c(CONTACT_ID), confidence: in_c(CONFIDENCE), x, y });
                }
            }
            if let Some(first) = slots.first() {
                let (max_x, max_y, report_id) = (first.x.max, first.y.max, first.x.report_id);
                return Some(Touchpad {
                    app,
                    report_id,
                    count: d.inputs(app).find(|f| f.usage == CONTACT_COUNT && f.report_id == report_id).copied(),
                    button: d.inputs(app).find(|f| f.usage == usage(BUTTON, 1) && f.report_id == report_id).copied(),
                    slots,
                    max_x,
                    max_y,
                });
            }
        }
        None
    }

    pub fn read(&self, r: &[u8], ids: bool) -> Option<TouchReport> {
        if report_id(r, ids) != self.report_id {
            return None;
        }
        let count = self.count.as_ref().map_or(self.slots.len() as u32, |f| get(f, r, ids) as u32);
        let mut contacts = Vec::new();
        for (i, s) in self.slots.iter().enumerate() {
            // later slots of a frame split across reports are empty
            if self.count.is_some() && i as u32 >= count.max(1) && count <= self.slots.len() as u32 {
                break;
            }
            contacts.push(Contact {
                id: s.id.as_ref().map_or(i as u32, |f| get(f, r, ids) as u32),
                tip: get(&s.tip, r, ids) != 0,
                confident: s.confidence.as_ref().map_or(true, |f| get(f, r, ids) != 0),
                x: get(&s.x, r, ids),
                y: get(&s.y, r, ids),
            });
        }
        Some(TouchReport { contacts, count, button: self.button.as_ref().map_or(false, |f| get(f, r, ids) != 0) })
    }
}

// ---- media keys -------------------------------------------------------------------------

/// Consumer controls: volume, mute, brightness, play/pause keys.
#[derive(Clone, Debug)]
pub struct Consumer {
    fields: Vec<Field>,
    report_id: u8,
}

impl Consumer {
    pub fn find(d: &Descriptor) -> Option<Consumer> {
        let fields: Vec<Field> = d.inputs(APP_CONSUMER).filter(|f| f.usage >> 16 == CONSUMER as u32).copied().collect();
        let report_id = fields.first()?.report_id;
        Some(Consumer { fields, report_id })
    }

    /// The consumer usages held down in a report (0xE2 mute, 0xE9 volume up…).
    pub fn read(&self, r: &[u8], ids: bool) -> Option<Vec<u16>> {
        if report_id(r, ids) != self.report_id {
            return None;
        }
        let mut held = Vec::new();
        for f in &self.fields {
            let v = get(f, r, ids);
            if f.variable {
                if v != 0 {
                    held.push(f.usage as u16);
                }
            } else if v != 0 {
                // array: the value is an index from the usage minimum
                let u = (f.usage & 0xFFFF) as i32 + v - f.min;
                if u > 0 && u as u32 <= (f.usage_max & 0xFFFF) {
                    held.push(u as u16);
                }
            }
        }
        Some(held)
    }
}

// ---- haptic touchpads -----------------------------------------------------------------

/// A haptic touchpad's (or pen's) Simple Haptic Controller: HydatekOS picks a
/// waveform, a strength and a repeat count, and the device plays it.
#[derive(Clone, Debug)]
pub struct HapticController {
    pub trigger: Field,
    pub intensity: Option<Field>,
    pub repeat: Option<Field>,
    pub retrigger: Option<Field>,
    /// the feature report listing the device's waveforms: (ordinal, field)
    list: Vec<(u32, Field)>,
    /// ordinal -> waveform, once read (ordinal 1 is "none", 2 "stop")
    pub waveforms: Vec<(u32, u16)>,
}

impl HapticController {
    pub fn find(d: &Descriptor) -> Option<HapticController> {
        let trigger = *d.find(Kind::Output, MANUAL_TRIGGER)?;
        let out = |u: Usage| d.fields.iter().find(|f| f.kind == Kind::Output && f.usage == u && f.report_id == trigger.report_id).copied();
        let list = d.fields.iter().filter(|f| f.kind == Kind::Feature && f.coll_usage == WAVEFORM_LIST && f.usage >> 16 == ORDINAL as u32).map(|f| (f.usage & 0xFFFF, *f)).collect();
        Some(HapticController { trigger, intensity: out(INTENSITY), repeat: out(REPEAT_COUNT), retrigger: out(RETRIGGER_PERIOD), list, waveforms: Vec::new() })
    }

    /// The waveform list's feature report id (to ask the device for it).
    pub fn list_report(&self) -> Option<u8> {
        self.list.first().map(|l| l.1.report_id)
    }

    /// Read the waveform list the device sent.
    pub fn read_list(&mut self, r: &[u8], ids: bool) {
        self.waveforms = self.list.iter().map(|(ord, f)| (*ord, get(f, r, ids) as u16)).filter(|w| w.1 != 0).collect();
    }

    /// The ordinal for a waveform, if the device has it.
    pub fn ordinal(&self, wave: u16) -> Option<u32> {
        self.waveforms.iter().find(|w| w.1 == wave).map(|w| w.0)
    }

    /// The output report that plays `wave` at `strength` (0-100) `repeat`
    /// more times, `period_ms` apart. None if the device lacks that waveform.
    pub fn play(&self, d: &Descriptor, wave: u16, strength: u32, repeat: u32, period_ms: u32) -> Option<Vec<u8>> {
        let ord = self.ordinal(wave)?;
        let mut r = alloc::vec![0u8; d.report_len(Kind::Output, self.trigger.report_id)];
        if d.ids {
            r[0] = self.trigger.report_id;
        }
        put(&self.trigger, &mut r, d.ids, ord as i32);
        if let Some(f) = &self.intensity {
            put(f, &mut r, d.ids, f.min + ((f.max - f.min) as i64 * strength.min(100) as i64 / 100) as i32);
        }
        if let Some(f) = &self.repeat {
            put(f, &mut r, d.ids, repeat.min(f.max.max(0) as u32) as i32);
        }
        if let Some(f) = &self.retrigger {
            // the period is in units of the field; ms for Windows' devices
            put(f, &mut r, d.ids, period_ms.min(f.max.max(0) as u32) as i32);
        }
        Some(r)
    }
}
