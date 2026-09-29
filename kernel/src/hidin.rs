//! What a HID device's reports mean to the desktop, whatever carries them
//! (USB through the firmware, HydatekOS's own xHCI driver, I2C).
//!
//! One `HidInput` per device: built from its report descriptor (or as an
//! Xbox controller), it turns each report into pointer movement, clicks,
//! scrolling, touchpad gestures, touch-screen taps, pen strokes, media keys,
//! controller navigation and light readings, and builds the reports that
//! start a touchpad or a sensor and play haptic waveforms.

use crate::gamepad::{self, Nav, Navigator};
use crate::hid::{self, Consumer, Descriptor, HapticController, LightSensor, Mouse, Pen, Touchpad};
use crate::touchpad::{Gesture, Gestures};
use alloc::vec::Vec;

/// What the input loop gets from HID devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// movement (device counts)
    Move(i32, i32),
    /// a position, 0..=32767 on each axis (tablets, touch screens, pens)
    Place(i32, i32),
    /// buttons held: bit 0 left, 1 right, 2 middle
    Buttons(u32),
    Scroll(i32),
    /// a consumer (media) key pressed: its usage
    Media(u16),
    /// a game controller's D-pad, stick or button
    Nav(Nav),
    /// a pen: position 0..=32767, pressure 0..=1000, touching, eraser end
    Pen { x: i32, y: i32, pressure: i32, tip: bool, eraser: bool },
    /// ambient light, lux
    Light(u32),
}

pub enum PadKind {
    Hid(hid::Gamepad, bool),
    Xbox360,
    XboxOne,
}

pub struct HidInput {
    pub desc: Descriptor,
    pub mouse: Option<Mouse>,
    pub pad: Option<(Touchpad, Gestures)>,
    pub consumer: Option<(Consumer, Vec<u16>)>,
    pub haptic: Option<HapticController>,
    pub pen: Option<(Pen, bool)>,
    pub light: Option<LightSensor>,
    pub pad_kind: Option<PadKind>,
    nav: Navigator,
    pub buttons: u32,
}

impl HidInput {
    fn empty(desc: Descriptor) -> HidInput {
        HidInput { desc, mouse: None, pad: None, consumer: None, haptic: None, pen: None, light: None, pad_kind: None, nav: Navigator::new(), buttons: 0 }
    }

    /// From a report descriptor. `sony`: a Sony controller (its button order).
    pub fn new(report_descriptor: &[u8], sony: bool) -> HidInput {
        let desc = Descriptor::parse(report_descriptor);
        let mut h = HidInput::empty(Descriptor::default());
        h.mouse = Mouse::find(&desc);
        h.pad = Touchpad::find(&desc).map(|t| (t, Gestures::new()));
        h.consumer = Consumer::find(&desc).map(|c| (c, Vec::new()));
        h.haptic = HapticController::find(&desc);
        h.pen = Pen::find(&desc).map(|p| (p, false));
        h.light = LightSensor::find(&desc);
        h.pad_kind = hid::Gamepad::find(&desc).map(|g| PadKind::Hid(g, sony));
        h.desc = desc;
        h
    }

    pub fn xbox(one: bool) -> HidInput {
        let mut h = HidInput::empty(Descriptor::default());
        h.pad_kind = Some(if one { PadKind::XboxOne } else { PadKind::Xbox360 });
        h
    }

    /// Anything HydatekOS can use.
    pub fn useful(&self) -> bool {
        self.mouse.is_some() || self.pad.is_some() || self.consumer.is_some() || self.haptic.is_some() || self.pad_kind.is_some() || self.pen.is_some() || self.light.is_some()
    }

    pub fn what(&self) -> &'static str {
        match self.pad_kind {
            Some(PadKind::Xbox360) => "Xbox 360 controller",
            Some(PadKind::XboxOne) => "Xbox controller",
            _ => hid::describe(&self.desc),
        }
    }

    /// Feature reports that start the device: a touchpad reporting fingers
    /// (input mode 3), a sensor reporting. (report id, report)
    pub fn start_reports(&self) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        let d = &self.desc;
        if self.pad.is_some() {
            if let Some(f) = d.fields.iter().find(|f| f.kind == hid::Kind::Feature && f.usage == hid::usage(hid::DIGITIZER, 0x52)) {
                let mut r = alloc::vec![0u8; d.report_len(hid::Kind::Feature, f.report_id)];
                if d.ids {
                    r[0] = f.report_id;
                }
                hid::put(f, &mut r, d.ids, 3);
                out.push((f.report_id, r));
            }
        }
        if let Some(l) = &self.light {
            if let Some(r) = l.start(d) {
                out.push((hid::report_id(&r, d.ids), r));
            }
        }
        out
    }

    /// The feature report to ask for a haptic touchpad's waveforms: (id, length).
    pub fn waveform_request(&self) -> Option<(u8, usize)> {
        let rid = self.haptic.as_ref()?.list_report()?;
        Some((rid, self.desc.report_len(hid::Kind::Feature, rid)))
    }

    pub fn set_waveforms(&mut self, r: &[u8]) {
        let ids = self.desc.ids;
        if let Some(h) = self.haptic.as_mut() {
            h.read_list(r, ids);
        }
    }

    pub fn has_haptics(&self) -> bool {
        self.haptic.as_ref().map_or(false, |h| !h.waveforms.is_empty())
    }

    /// The output report playing a waveform.
    pub fn haptic_report(&self, wave: u16, strength: u32, repeat: u32, period: u32) -> Option<Vec<u8>> {
        self.haptic.as_ref()?.play(&self.desc, wave, strength, repeat, period)
    }

    /// One report from the device. `now` in ms.
    pub fn report(&mut self, r: &[u8], now: u64, out: &mut Vec<Event>) {
        let ids = self.desc.ids;
        // game controllers
        let pad = match &self.pad_kind {
            Some(PadKind::Xbox360) => gamepad::xbox360(r),
            Some(PadKind::XboxOne) => match gamepad::xbox_one_guide(r) {
                Some(g) => Some(gamepad::Pad { buttons: if g { self.buttons | gamepad::GUIDE } else { self.buttons & !gamepad::GUIDE }, ..Default::default() }),
                None => gamepad::xbox_one(r),
            },
            Some(PadKind::Hid(g, sony)) => g.read(r, ids).map(|p| gamepad::from_hid(&p, *sony)),
            None => None,
        };
        if let Some(p) = pad {
            self.buttons = p.buttons;
            for n in self.nav.feed(&p, now) {
                out.push(Event::Nav(n));
            }
            return;
        }
        if let Some((pen, was_tip)) = self.pen.as_mut() {
            if let Some(p) = pen.read(r, ids) {
                if p.in_range {
                    out.push(Event::Pen { x: p.x, y: p.y, pressure: p.pressure, tip: p.tip, eraser: p.eraser });
                    out.push(Event::Place(p.x, p.y));
                }
                // the tip is the left button, the side button the right
                let b = (p.tip && !p.barrel) as u32 | ((p.tip && p.barrel) as u32) << 1;
                if b != self.buttons || p.tip != *was_tip {
                    self.buttons = b;
                    *was_tip = p.tip;
                    out.push(Event::Buttons(b));
                }
                return;
            }
        }
        if let Some(l) = &self.light {
            if let Some((lux, _)) = l.read(r, ids) {
                out.push(Event::Light(lux));
                return;
            }
        }
        if let Some((rep, abs)) = self.mouse.as_ref().and_then(|m| m.read(r, ids).map(|x| (x, m.absolute))) {
            if abs {
                out.push(Event::Place(rep.x, rep.y));
            } else if rep.x != 0 || rep.y != 0 {
                out.push(Event::Move(rep.x, rep.y));
            }
            if rep.buttons != self.buttons {
                self.buttons = rep.buttons;
                out.push(Event::Buttons(rep.buttons));
            }
            if rep.wheel != 0 {
                // wheel up is positive in HID, "scroll up" to HydatekOS is negative
                out.push(Event::Scroll(-rep.wheel));
            }
            return;
        }
        if let Some((t, g)) = self.pad.as_mut() {
            if let Some(rep) = t.read(r, ids) {
                if t.app == hid::APP_TOUCHSCREEN {
                    // a touch screen points where it's touched
                    let first = rep.contacts.iter().find(|c| c.tip);
                    if let Some(c) = first {
                        let sx = (c.x as i64 * 32767 / t.max_x.max(1) as i64) as i32;
                        let sy = (c.y as i64 * 32767 / t.max_y.max(1) as i64) as i32;
                        out.push(Event::Place(sx, sy));
                    }
                    let b = first.is_some() as u32;
                    if b != self.buttons {
                        self.buttons = b;
                        out.push(Event::Buttons(b));
                    }
                    return;
                }
                for gst in g.feed(&rep, t.max_x, now) {
                    match gst {
                        Gesture::Move(x, y) => out.push(Event::Move(x, y)),
                        Gesture::Scroll(n) => out.push(Event::Scroll(n)),
                        Gesture::ScrollX(_) => {}
                        Gesture::Press(b) => {
                            self.buttons |= 1 << b;
                            out.push(Event::Buttons(self.buttons));
                        }
                        Gesture::Release(b) => {
                            self.buttons &= !(1 << b);
                            out.push(Event::Buttons(self.buttons));
                        }
                        Gesture::Click(b) => {
                            out.push(Event::Buttons(self.buttons | 1 << b));
                            out.push(Event::Buttons(self.buttons));
                        }
                    }
                }
                return;
            }
        }
        if let Some((c, held)) = self.consumer.as_mut() {
            if let Some(now_held) = c.read(r, ids) {
                for k in &now_held {
                    if !held.contains(k) {
                        out.push(Event::Media(*k));
                    }
                }
                *held = now_held;
            }
        }
    }
}
