//! TCP (RFC 793/9293): passive and active open, in-order delivery,
//! cumulative ACKs, go-back-N retransmission with exponential backoff,
//! flow control via the peer's window, zero-window probing and orderly close.

use super::{pseudo_csum, Ip, PROTO_TCP};
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

const MSS: usize = 1460;
const RX_CAP: usize = 256 * 1024;
const TX_CAP: usize = 4 * 1024 * 1024;
const MAX_CONNS: usize = 24;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    SynSent,
    SynRcvd,
    Established,
    CloseWait,
    FinWait1,
    FinWait2,
    Closing,
    LastAck,
    TimeWait,
    Closed,
}

fn lt(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) < 0
}
fn le(a: u32, b: u32) -> bool {
    a == b || lt(a, b)
}

pub struct Conn {
    pub id: u32,
    pub state: State,
    lport: u16,
    pub rip: Ip,
    pub rport: u16,
    lip: Ip,
    iss: u32,
    snd_una: u32,
    snd_nxt: u32,
    /// highest sequence number sent so far (snd_nxt rewinds on retransmit)
    snd_max: u32,
    snd_wnd: u32,
    rcv_nxt: u32,
    rx: VecDeque<u8>,
    /// out-of-order segments waiting for the gap before them: (seq, data)
    ooo: Vec<(u32, Vec<u8>)>,
    tx: VecDeque<u8>,
    /// bytes of `tx` already transmitted (from its front)
    sent: usize,
    syn_acked: bool,
    fin_sent: bool,
    fin_seq: Option<u32>,
    fin_acked: bool,
    probe: bool,
    app_closed: bool,
    accepted: bool,
    peer_mss: usize,
    rto: u64,
    timer: u64,
    retries: u32,
    need_ack: bool,
    adv_wnd: usize,
    idle_since: u64,
}

pub struct Tcp {
    listen: Vec<u16>,
    pub conns: Vec<Conn>,
    next_id: u32,
    next_port: u16,
}

type Out = Vec<(Ip, Vec<u8>)>;

impl Conn {
    fn window(&self) -> usize {
        (RX_CAP - self.rx.len()).min(65535)
    }

    fn segment(&mut self, flags: u8, seq: u32, data: &[u8], syn_opts: bool) -> (Ip, Vec<u8>) {
        let hl = if syn_opts { 24 } else { 20 };
        let mut s = vec![0u8; hl];
        s[0..2].copy_from_slice(&self.lport.to_be_bytes());
        s[2..4].copy_from_slice(&self.rport.to_be_bytes());
        s[4..8].copy_from_slice(&seq.to_be_bytes());
        let ack = if flags & ACK != 0 { self.rcv_nxt } else { 0 };
        s[8..12].copy_from_slice(&ack.to_be_bytes());
        s[12] = ((hl / 4) as u8) << 4;
        s[13] = flags;
        let w = self.window();
        self.adv_wnd = w;
        s[14..16].copy_from_slice(&(w as u16).to_be_bytes());
        if syn_opts {
            s[20..24].copy_from_slice(&[2, 4, (MSS >> 8) as u8, MSS as u8]);
        }
        s.extend_from_slice(data);
        let c = pseudo_csum(self.lip, self.rip, PROTO_TCP, &s);
        s[16..18].copy_from_slice(&c.to_be_bytes());
        self.need_ack = false;
        (self.rip, s)
    }

    fn arm(&mut self, now: u64) {
        self.timer = now + self.rto;
    }

    /// Transmit whatever the window allows.
    fn output(&mut self, now: u64, out: &mut Out) {
        match self.state {
            State::SynRcvd => {}
            State::Established | State::CloseWait | State::FinWait1 | State::Closing | State::LastAck => {
                let mss = self.peer_mss.min(MSS);
                loop {
                    let in_flight = self.sent;
                    let wnd = (self.snd_wnd as usize).max(if self.probe { in_flight + 1 } else { 0 });
                    let can = wnd.saturating_sub(in_flight).min(self.tx.len() - self.sent);
                    if can == 0 {
                        break;
                    }
                    let n = can.min(mss);
                    let data: Vec<u8> = self.tx.range(self.sent..self.sent + n).copied().collect();
                    let seq = self.snd_nxt;
                    let push = if self.sent + n == self.tx.len() { PSH } else { 0 };
                    out.push(self.segment(ACK | push, seq, &data, false));
                    if self.sent == 0 || self.snd_una == self.snd_nxt {
                        self.arm(now);
                    }
                    self.sent += n;
                    self.snd_nxt = self.snd_nxt.wrapping_add(n as u32);
                    if lt(self.snd_max, self.snd_nxt) {
                        self.snd_max = self.snd_nxt;
                    }
                    self.probe = false;
                }
                // persist timer: keep probing a zero window
                if self.snd_wnd == 0 && self.tx.len() > self.sent && self.timer == 0 {
                    self.arm(now);
                }
                let all_sent = self.sent == self.tx.len();
                if self.app_closed && all_sent && !self.fin_sent && matches!(self.state, State::Established | State::CloseWait) {
                    let seq = self.snd_nxt;
                    out.push(self.segment(FIN | ACK, seq, &[], false));
                    self.snd_nxt = self.snd_nxt.wrapping_add(1);
                    if lt(self.snd_max, self.snd_nxt) {
                        self.snd_max = self.snd_nxt;
                    }
                    self.fin_sent = true;
                    self.fin_seq = Some(seq);
                    self.arm(now);
                    self.state = if self.state == State::Established { State::FinWait1 } else { State::LastAck };
                }
            }
            _ => {}
        }
        if self.need_ack {
            let seq = self.snd_nxt;
            out.push(self.segment(ACK, seq, &[], false));
        }
    }

    fn timer_due(&self, now: u64) -> bool {
        self.timer != 0 && now >= self.timer
    }
}

impl Tcp {
    pub fn new() -> Tcp {
        Tcp { listen: vec![], conns: vec![], next_id: 1, next_port: 49152 + (crate::rng::u32() % 8192) as u16 }
    }

    pub fn listen(&mut self, port: u16) {
        if !self.listen.contains(&port) {
            self.listen.push(port);
        }
    }

    // ---- application API --------------------------------------------------

    /// Open a connection to (rip, rport) from `lip`. Returns its id and the SYN.
    pub fn connect(&mut self, lip: Ip, rip: Ip, rport: u16, now: u64) -> (u32, Out) {
        // make room: forget connections that are finished
        if self.conns.len() >= MAX_CONNS {
            if let Some(j) = self.conns.iter().position(|c| matches!(c.state, State::TimeWait | State::Closed)) {
                self.conns.remove(j);
            }
        }
        let mut lport = self.next_port;
        for _ in 0..16384 {
            lport = if lport >= 65000 { 49152 } else { lport + 1 };
            if !self.conns.iter().any(|c| c.lport == lport) {
                break;
            }
        }
        self.next_port = lport;
        let iss = crate::rng::u32();
        let mut c = Conn {
            id: self.next_id,
            state: State::SynSent,
            lport,
            rip,
            rport,
            lip,
            iss,
            snd_una: iss,
            snd_nxt: iss.wrapping_add(1),
            snd_max: iss.wrapping_add(1),
            snd_wnd: 0,
            rcv_nxt: 0,
            rx: VecDeque::new(),
            ooo: Vec::new(),
            tx: VecDeque::new(),
            sent: 0,
            syn_acked: false,
            fin_sent: false,
            fin_seq: None,
            fin_acked: false,
            probe: false,
            app_closed: false,
            accepted: true,
            peer_mss: 536,
            rto: 1000,
            timer: 0,
            retries: 0,
            need_ack: false,
            adv_wnd: 0,
            idle_since: now,
        };
        self.next_id += 1;
        let syn = c.segment(SYN, iss, &[], true);
        c.arm(now);
        let id = c.id;
        self.conns.push(c);
        (id, vec![syn])
    }

    /// Still connecting (SYN sent, no answer yet).
    pub fn connecting(&self, id: u32) -> bool {
        self.conns.iter().any(|c| c.id == id && c.state == State::SynSent)
    }

    pub fn accept(&mut self, port: u16) -> Option<u32> {
        let c = self.conns.iter_mut().find(|c| !c.accepted && c.lport == port && matches!(c.state, State::Established | State::CloseWait))?;
        c.accepted = true;
        Some(c.id)
    }

    fn get(&mut self, id: u32) -> Option<&mut Conn> {
        self.conns.iter_mut().find(|c| c.id == id)
    }

    /// Take everything received so far.
    pub fn recv(&mut self, id: u32) -> Vec<u8> {
        match self.get(id) {
            Some(c) => {
                let before = c.window();
                let v: Vec<u8> = c.rx.drain(..).collect();
                // re-open a closed/shrunken window promptly
                if !v.is_empty() && (before < MSS * 2 || c.adv_wnd < MSS * 2) {
                    c.need_ack = true;
                }
                v
            }
            None => Vec::new(),
        }
    }

    /// Queue data; returns how many bytes were accepted.
    pub fn send(&mut self, id: u32, data: &[u8]) -> usize {
        match self.get(id) {
            Some(c) if matches!(c.state, State::Established | State::CloseWait) && !c.app_closed => {
                let n = data.len().min(TX_CAP - c.tx.len());
                c.tx.extend(&data[..n]);
                n
            }
            _ => 0,
        }
    }

    pub fn pending(&self, id: u32) -> usize {
        self.conns.iter().find(|c| c.id == id).map(|c| c.tx.len()).unwrap_or(0)
    }

    pub fn close(&mut self, id: u32) {
        if let Some(c) = self.get(id) {
            c.app_closed = true;
        }
    }

    /// Connection still usable for sending (not reset or closed by us).
    pub fn is_open(&self, id: u32) -> bool {
        self.conns.iter().any(|c| c.id == id && matches!(c.state, State::Established | State::CloseWait) && !c.app_closed)
    }

    /// The connection exists and hasn't been torn down.
    pub fn alive(&self, id: u32) -> bool {
        self.conns.iter().any(|c| c.id == id && !matches!(c.state, State::Closed | State::TimeWait))
    }

    /// Peer has finished sending (or the connection is gone).
    pub fn peer_closed(&self, id: u32) -> bool {
        self.conns.iter().find(|c| c.id == id).map(|c| !matches!(c.state, State::Established | State::FinWait1 | State::FinWait2 | State::SynRcvd | State::SynSent)).unwrap_or(true)
    }

    // ---- segment input ------------------------------------------------------

    pub fn input(&mut self, src: Ip, dst: Ip, seg: &[u8], now: u64) -> Out {
        let mut out = Vec::new();
        if seg.len() < 20 {
            return out;
        }
        let sport = u16::from_be_bytes([seg[0], seg[1]]);
        let dport = u16::from_be_bytes([seg[2], seg[3]]);
        let seq = u32::from_be_bytes([seg[4], seg[5], seg[6], seg[7]]);
        let ack = u32::from_be_bytes([seg[8], seg[9], seg[10], seg[11]]);
        let off = ((seg[12] >> 4) as usize) * 4;
        if off < 20 || off > seg.len() {
            return out;
        }
        let flags = seg[13];
        let wnd = u16::from_be_bytes([seg[14], seg[15]]) as u32;
        let data = &seg[off..];
        let idx = self.conns.iter().position(|c| c.rip == src && c.rport == sport && c.lport == dport);

        let Some(i) = idx else {
            if flags & RST != 0 {
                return out;
            }
            if flags & SYN != 0 && flags & ACK == 0 && self.listen.contains(&dport) {
                if self.conns.len() >= MAX_CONNS {
                    // drop the oldest idle unaccepted/finished connection
                    if let Some(j) = self.conns.iter().position(|c| !c.accepted || matches!(c.state, State::TimeWait | State::Closed)) {
                        self.conns.remove(j);
                    } else {
                        return out;
                    }
                }
                let mut mss = 536;
                let mut o = 20;
                while o < off {
                    match seg[o] {
                        0 => break,
                        1 => o += 1,
                        k => {
                            if o + 1 >= off {
                                break;
                            }
                            let l = seg[o + 1] as usize;
                            if k == 2 && l == 4 && o + 3 < off {
                                mss = u16::from_be_bytes([seg[o + 2], seg[o + 3]]) as usize;
                            }
                            o += l.max(2);
                        }
                    }
                }
                let iss = crate::rng::u32();
                let mut c = Conn {
                    id: self.next_id,
                    state: State::SynRcvd,
                    lport: dport,
                    rip: src,
                    rport: sport,
                    lip: dst,
                    iss,
                    snd_una: iss,
                    snd_nxt: iss.wrapping_add(1),
                    snd_max: iss.wrapping_add(1),
                    snd_wnd: wnd,
                    rcv_nxt: seq.wrapping_add(1),
                    rx: VecDeque::new(),
                    ooo: Vec::new(),
                    tx: VecDeque::new(),
                    sent: 0,
                    syn_acked: false,
                    fin_sent: false,
                    fin_seq: None,
                    fin_acked: false,
                    probe: false,
                    app_closed: false,
                    accepted: false,
                    peer_mss: mss.max(64),
                    rto: 1000,
                    timer: 0,
                    retries: 0,
                    need_ack: false,
                    adv_wnd: 0,
                    idle_since: now,
                };
                self.next_id += 1;
                out.push(c.segment(SYN | ACK, iss, &[], true));
                c.arm(now);
                self.conns.push(c);
            } else {
                // RST for anything else
                let mut r = vec![0u8; 20];
                r[0..2].copy_from_slice(&dport.to_be_bytes());
                r[2..4].copy_from_slice(&sport.to_be_bytes());
                let seg_len = data.len() as u32 + (flags & SYN != 0) as u32 + (flags & FIN != 0) as u32;
                if flags & ACK != 0 {
                    r[4..8].copy_from_slice(&ack.to_be_bytes());
                    r[13] = RST;
                } else {
                    r[8..12].copy_from_slice(&seq.wrapping_add(seg_len).to_be_bytes());
                    r[13] = RST | ACK;
                }
                r[12] = 5 << 4;
                let c = pseudo_csum(dst, src, PROTO_TCP, &r);
                r[16..18].copy_from_slice(&c.to_be_bytes());
                out.push((src, r));
            }
            return out;
        };

        let c = &mut self.conns[i];
        c.idle_since = now;
        if c.state == State::SynSent {
            // active open: expect SYN+ACK for our SYN
            if flags & ACK != 0 && ack != c.iss.wrapping_add(1) {
                return out;
            }
            if flags & RST != 0 {
                if flags & ACK != 0 {
                    c.state = State::Closed; // connection refused
                }
                return out;
            }
            if flags & SYN != 0 && flags & ACK != 0 {
                let mut o = 20;
                while o < off {
                    match seg[o] {
                        0 => break,
                        1 => o += 1,
                        k => {
                            if o + 1 >= off {
                                break;
                            }
                            let l = seg[o + 1] as usize;
                            if k == 2 && l == 4 && o + 3 < off {
                                c.peer_mss = (u16::from_be_bytes([seg[o + 2], seg[o + 3]]) as usize).max(64);
                            }
                            o += l.max(2);
                        }
                    }
                }
                c.rcv_nxt = seq.wrapping_add(1);
                c.snd_una = ack;
                c.snd_wnd = wnd;
                c.syn_acked = true;
                c.state = State::Established;
                c.timer = 0;
                c.retries = 0;
                c.rto = 1000;
                c.need_ack = true;
                c.output(now, &mut out);
            }
            return out;
        }
        if flags & RST != 0 {
            if le(c.rcv_nxt, seq) && lt(seq, c.rcv_nxt.wrapping_add(c.window().max(1) as u32)) || seq == c.rcv_nxt {
                c.state = State::Closed;
            }
            return out;
        }
        if flags & SYN != 0 {
            if c.state == State::SynRcvd && seq.wrapping_add(1) == c.rcv_nxt {
                let iss = c.iss;
                out.push(c.segment(SYN | ACK, iss, &[], true));
            }
            return out;
        }
        if flags & ACK == 0 {
            return out;
        }
        // ACK processing
        if c.state == State::SynRcvd {
            if ack == c.iss.wrapping_add(1) {
                c.state = State::Established;
                c.snd_una = ack;
                c.syn_acked = true;
                c.timer = 0;
                c.retries = 0;
                c.rto = 1000;
            } else {
                return out;
            }
        } else if lt(c.snd_una, ack) && le(ack, c.snd_max) {
            let n = ack.wrapping_sub(c.snd_una) as usize;
            let d = n.min(c.tx.len());
            c.tx.drain(..d);
            c.sent = c.sent.saturating_sub(d);
            if let Some(f) = c.fin_seq {
                if lt(f, ack) {
                    c.fin_acked = true;
                }
            }
            c.snd_una = ack;
            if lt(c.snd_nxt, ack) {
                c.snd_nxt = ack;
            }
            c.retries = 0;
            c.rto = 1000;
            c.timer = if c.snd_una == c.snd_nxt { 0 } else { now + c.rto };
        }
        c.snd_wnd = wnd;
        // Data: accept in-order bytes (trimming any already-received prefix),
        // park out-of-order segments, and drain the parked ones that now fit.
        let mut fin_now = false;
        let receiving = matches!(c.state, State::Established | State::FinWait1 | State::FinWait2);
        if !data.is_empty() || flags & FIN != 0 {
            c.need_ack = true;
        }
        if le(seq, c.rcv_nxt) {
            let skip = c.rcv_nxt.wrapping_sub(seq) as usize;
            if receiving && skip < data.len() {
                let fresh = &data[skip..];
                let take = fresh.len().min(RX_CAP - c.rx.len());
                c.rx.extend(&fresh[..take]);
                c.rcv_nxt = c.rcv_nxt.wrapping_add(take as u32);
                fin_now = flags & FIN != 0 && take == fresh.len();
                // pull in parked segments that are now contiguous
                loop {
                    let nxt = c.rcv_nxt;
                    let Some(j) = c.ooo.iter().position(|(s, d)| le(*s, nxt) && lt(nxt, s.wrapping_add(d.len() as u32))) else { break };
                    let (s, d) = c.ooo.remove(j);
                    let off = nxt.wrapping_sub(s) as usize;
                    let room = RX_CAP - c.rx.len();
                    let n = (d.len() - off).min(room);
                    c.rx.extend(&d[off..off + n]);
                    c.rcv_nxt = c.rcv_nxt.wrapping_add(n as u32);
                    if n < d.len() - off {
                        break;
                    }
                }
                let nxt = c.rcv_nxt;
                c.ooo.retain(|(s, d)| lt(nxt, s.wrapping_add(d.len() as u32)));
            } else if skip == data.len() {
                fin_now = flags & FIN != 0;
            }
        } else if receiving && !data.is_empty() {
            let ahead = seq.wrapping_sub(c.rcv_nxt) as usize;
            let parked: usize = c.ooo.iter().map(|o| o.1.len()).sum();
            if ahead + data.len() <= RX_CAP - c.rx.len() && parked + data.len() <= RX_CAP && !c.ooo.iter().any(|o| o.0 == seq) {
                c.ooo.push((seq, data.to_vec()));
            }
        }
        if fin_now {
            c.rcv_nxt = c.rcv_nxt.wrapping_add(1);
            c.need_ack = true;
            c.state = match c.state {
                State::Established => State::CloseWait,
                State::FinWait1 if c.fin_acked => State::TimeWait,
                State::FinWait1 => State::Closing,
                State::FinWait2 => State::TimeWait,
                s => s,
            };
            if c.state == State::TimeWait {
                c.timer = now + 2000;
            }
        }
        if c.fin_acked {
            c.state = match c.state {
                State::FinWait1 => State::FinWait2,
                State::Closing => State::TimeWait,
                State::LastAck => State::Closed,
                s => s,
            };
            if c.state == State::TimeWait {
                c.timer = now + 2000;
            }
        }
        c.output(now, &mut out);
        out
    }

    // ---- timers and transmission ---------------------------------------------

    pub fn poll(&mut self, now: u64) -> Out {
        let mut out = Vec::new();
        for c in self.conns.iter_mut() {
            match c.state {
                State::TimeWait => {
                    if c.timer_due(now) {
                        c.state = State::Closed;
                    }
                    continue;
                }
                State::Closed => continue,
                _ => {}
            }
            if c.timer_due(now) {
                if c.snd_wnd == 0 && c.snd_una == c.snd_nxt && !matches!(c.state, State::SynRcvd | State::SynSent) {
                    // zero-window probe, not a loss
                    c.probe = true;
                    c.rto = (c.rto * 2).min(8_000);
                    c.timer = 0;
                    c.output(now, &mut out);
                    continue;
                }
                c.retries += 1;
                if c.retries > 10 {
                    c.state = State::Closed;
                    continue;
                }
                c.rto = (c.rto * 2).min(16_000);
                if c.state == State::SynRcvd {
                    let iss = c.iss;
                    out.push(c.segment(SYN | ACK, iss, &[], true));
                } else if c.state == State::SynSent {
                    if c.retries > 5 {
                        c.state = State::Closed;
                        continue;
                    }
                    let iss = c.iss;
                    out.push(c.segment(SYN, iss, &[], true));
                } else {
                    // go back N: resend from the first unacknowledged byte
                    c.snd_nxt = c.snd_una;
                    c.sent = 0;
                    if c.fin_sent && !c.fin_acked {
                        c.fin_sent = false;
                        c.fin_seq = None;
                        c.state = match c.state {
                            State::FinWait1 => State::Established,
                            State::LastAck => State::CloseWait,
                            State::Closing => State::CloseWait,
                            s => s,
                        };
                    }
                }
                c.timer = now + c.rto;
            }
            c.output(now, &mut out);
            // reap connections the peer abandoned mid-handshake or while half-closed
            if (c.state == State::SynRcvd || c.state == State::FinWait2) && now > c.idle_since + 60_000 {
                c.state = State::Closed;
            }
        }
        // closed connections the application never saw can go now; accepted
        // ones stay until the application calls `reap`
        self.conns.retain(|c| c.state != State::Closed || (c.accepted && !c.app_closed));
        out
    }

    /// Drop closed connections once the application has seen them close.
    pub fn reap(&mut self, id: u32) {
        self.conns.retain(|c| !(c.id == id && matches!(c.state, State::Closed | State::TimeWait)));
    }
}
