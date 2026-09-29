//! The DesignWare I2C driver and HID over I2C, against a simulated
//! controller with a simulated haptic Precision Touchpad on its bus.

use crate::hid::{self, Descriptor, HapticController, Touchpad};
use crate::hid_tests::{pad_report, touchpad, HAPTIC};
use crate::i2c::*;
use std::collections::VecDeque;

const ADDR: u16 = 0x2C;
const DESC_REG: u16 = 0x0020;
const RD_REG: u16 = 0x0021;
const IN_REG: u16 = 0x0022;
const OUT_REG: u16 = 0x0023;
const CMD_REG: u16 = 0x0024;
const DATA_REG: u16 = 0x0025;

/// A HID over I2C touchpad, as its bus sees it.
struct Pad {
    report_desc: Vec<u8>,
    /// input reports waiting (each read gives the next)
    inputs: VecDeque<Vec<u8>>,
    powered: bool,
    resets: u32,
    /// what the next read returns (set by the write before it)
    answer: VecDeque<u8>,
    outputs: Vec<Vec<u8>>,
    features: Vec<(u8, Vec<u8>)>,
}

impl Pad {
    fn new() -> Pad {
        let mut rd = touchpad();
        rd.extend_from_slice(HAPTIC);
        Pad { report_desc: rd, inputs: VecDeque::new(), powered: false, resets: 0, answer: VecDeque::new(), outputs: vec![], features: vec![] }
    }

    fn hid_desc(&self) -> Vec<u8> {
        let mut d = Vec::new();
        for v in [30u16, 0x0100, self.report_desc.len() as u16, RD_REG, IN_REG, 64, OUT_REG, 16, CMD_REG, DATA_REG, 0x06CB, 0xCE7E, 0x0001, 0, 0] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        d
    }

    fn with_len(r: &[u8]) -> Vec<u8> {
        let mut v = ((r.len() + 2) as u16).to_le_bytes().to_vec();
        v.extend_from_slice(r);
        v
    }

    /// A write (and whether a read follows it).
    fn write(&mut self, w: &[u8]) {
        let reg = u16::from_le_bytes([w[0], w[1]]);
        self.answer.clear();
        match reg {
            DESC_REG => self.answer.extend(self.hid_desc()),
            RD_REG => self.answer.extend(self.report_desc.clone()),
            OUT_REG => {
                let n = u16::from_le_bytes([w[2], w[3]]) as usize;
                self.outputs.push(w[4..2 + n].to_vec());
            }
            CMD_REG => {
                let (kind, mut id, op) = (w[2] >> 4, w[2] & 0x0F, w[3]);
                let mut i = 4;
                if id == 15 {
                    id = w[4];
                    i = 5;
                }
                match op {
                    1 => {
                        self.resets += 1;
                        self.inputs.push_front(vec![0, 0]);
                    }
                    8 => self.powered = id == 0,
                    2 => {
                        // GET_REPORT: the waveform list (feature 3)
                        assert_eq!(u16::from_le_bytes([w[i], w[i + 1]]), DATA_REG);
                        assert_eq!((kind, id), (FEATURE, 3));
                        let mut r = vec![3u8];
                        for wave in [hid::WAVE_CLICK, hid::WAVE_BUZZ, hid::WAVE_PRESS] {
                            r.extend_from_slice(&wave.to_le_bytes());
                        }
                        self.answer.extend(Pad::with_len(&r));
                    }
                    3 => {
                        assert_eq!(u16::from_le_bytes([w[i], w[i + 1]]), DATA_REG);
                        let n = u16::from_le_bytes([w[i + 2], w[i + 3]]) as usize;
                        self.features.push((id, w[i + 4..i + 2 + n].to_vec()));
                    }
                    _ => panic!("unknown opcode {}", op),
                }
            }
            _ => panic!("write to register {:#x}", reg),
        }
    }

    fn read_byte(&mut self) -> u8 {
        if self.answer.is_empty() {
            // a read on its own: the next input report, or "nothing" (length 0)
            let r = self.inputs.pop_front().map(|r| if r == [0, 0] { r } else { Pad::with_len(&r) }).unwrap_or(vec![0, 0]);
            let mut r = r;
            r.resize(66, 0);
            self.answer.extend(r);
        }
        self.answer.pop_front().unwrap()
    }
}

/// A DesignWare controller, register by register.
struct Sim {
    pad: Pad,
    enabled: bool,
    tar: u32,
    wbuf: Vec<u8>,
    reading: bool,
    rx: VecDeque<u8>,
    abort: bool,
    con: u32,
}

impl Sim {
    fn new() -> Sim {
        Sim { pad: Pad::new(), enabled: false, tar: 0, wbuf: vec![], reading: false, rx: VecDeque::new(), abort: false, con: 0 }
    }
}

impl Regs for &mut Sim {
    fn rd(&mut self, off: usize) -> u32 {
        match off {
            IC_COMP_TYPE => DW_COMP_TYPE,
            IC_ENABLE_STATUS => self.enabled as u32,
            IC_STATUS => 0b110 | if self.rx.is_empty() { 0 } else { 1 << 3 },
            IC_DATA_CMD => self.rx.pop_front().expect("read from an empty FIFO") as u32,
            IC_RAW_INTR_STAT => (self.abort as u32) << 6,
            IC_CLR_TX_ABRT => {
                self.abort = false;
                0
            }
            IC_CON => self.con,
            _ => 0,
        }
    }

    fn wr(&mut self, off: usize, v: u32) {
        match off {
            IC_ENABLE => self.enabled = v & 1 != 0,
            IC_TAR => {
                assert!(!self.enabled, "IC_TAR written while enabled");
                self.tar = v;
            }
            IC_CON => self.con = v,
            IC_DATA_CMD => {
                assert!(self.enabled);
                if self.tar != ADDR as u32 {
                    // nobody answers: the controller aborts
                    self.abort = true;
                    return;
                }
                if v & CMD_READ != 0 {
                    if !self.reading && !self.wbuf.is_empty() {
                        assert!(v & CMD_RESTART != 0, "write then read needs a repeated start");
                        let w = std::mem::take(&mut self.wbuf);
                        self.pad.write(&w);
                    }
                    self.reading = true;
                    let b = self.pad.read_byte();
                    self.rx.push_back(b);
                } else {
                    self.wbuf.push(v as u8);
                }
                if v & CMD_STOP != 0 {
                    if !self.wbuf.is_empty() {
                        let w = std::mem::take(&mut self.wbuf);
                        self.pad.write(&w);
                    }
                    if self.reading {
                        self.pad.answer.clear();
                    }
                    self.reading = false;
                }
            }
            _ => {}
        }
    }
}

#[test]
fn designware_controller() {
    let mut sim = Sim::new();
    let mut dw = DesignWare::new(&mut sim, 133).expect("a DesignWare controller");
    // nobody at 0x50: the transfer fails instead of hanging
    assert!(!dw.xfer(0x50, &[0x00], &mut [0u8; 4]));
    // fast mode master
    assert_eq!(dw.regs.rd(IC_CON) & 0x67, 0x65);
    // a register read: write the register, repeated start, read 30 bytes
    let mut b = [0u8; 30];
    assert!(dw.xfer(ADDR, &DESC_REG.to_le_bytes(), &mut b));
    assert_eq!(&b[..4], &[30, 0, 0x00, 0x01]);
    // not a DesignWare controller
    struct Other;
    impl Regs for Other {
        fn rd(&mut self, _: usize) -> u32 {
            0xFFFF_FFFF
        }
        fn wr(&mut self, _: usize, _: u32) {}
    }
    assert!(DesignWare::new(Other, 100).is_none());
}

#[test]
fn hid_over_i2c_touchpad() {
    let mut sim = Sim::new();
    {
        let mut dw = DesignWare::new(&mut sim, 133).unwrap();
        let dev = I2cHid::start(&mut dw, ADDR, DESC_REG).expect("the touchpad starts");
        assert_eq!((dev.desc.vendor, dev.desc.product), (0x06CB, 0xCE7E));
        assert_eq!(dev.desc.input_reg, IN_REG);

        // its report descriptor, through the same HID code as USB
        let rd = dev.report_descriptor(&mut dw).unwrap();
        let d = Descriptor::parse(&rd);
        assert_eq!(hid::describe(&d), "Touchpad");
        let t = Touchpad::find(&d).expect("a touchpad");
        let mut hc = HapticController::find(&d).expect("haptics");

        // the waveform list
        let rid = hc.list_report().unwrap();
        let r = dev.get_report(&mut dw, FEATURE, rid, d.report_len(hid::Kind::Feature, rid)).unwrap();
        hc.read_list(&r, d.ids);
        assert_eq!(hc.ordinal(hid::WAVE_BUZZ), Some(4));

        // a haptic click goes out on the output register
        let click = hc.play(&d, hid::WAVE_CLICK, 100, 0, 0).unwrap();
        assert!(dev.output(&mut dw, &click));

        // input mode: report fingers
        assert!(dev.set_report(&mut dw, FEATURE, 5, &[5, 3]));

        // nothing to read; then a finger
        assert_eq!(dev.read_input(&mut dw), None);
        dw.regs.pad.inputs.push_back(pad_report(&[(true, true, 1, 500, 400)], 1, false));
        let r = dev.read_input(&mut dw).expect("an input report");
        let rep = t.read(&r, d.ids).unwrap();
        assert_eq!(rep.contacts[0].x, 500);
        assert!(rep.contacts[0].tip);

        // ids of 15 and more take a byte of their own
        assert!(dev.set_report(&mut dw, FEATURE, 20, &[20, 1]));
        assert!(dev.power(&mut dw, false));
    }
    assert_eq!(sim.pad.resets, 1);
    assert!(!sim.pad.powered);
    assert_eq!(sim.pad.outputs, vec![vec![4, 3, 100, 0, 0, 0]]);
    assert_eq!(sim.pad.features, vec![(5, vec![5, 3]), (20, vec![20, 1])]);
}
