//! HydatekOS network stack: Ethernet, ARP, IPv4, ICMP, UDP, DHCP, TCP and
//! mDNS. Single-threaded and polled from the main loop.

pub mod mdns;
pub mod snp;
pub mod tcp;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use snp::FirmwareNic;
use tcp::Tcp;

pub type Ip = [u8; 4];
pub type Mac = [u8; 6];

const BROADCAST_MAC: Mac = [0xff; 6];
const ETH_ARP: u16 = 0x0806;
const ETH_IP: u16 = 0x0800;
pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;
pub const MDNS_GROUP: Ip = [224, 0, 0, 251];

pub fn ip_str(ip: Ip) -> String {
    alloc::format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
}

/// Internet checksum over `data`, continuing from `sum`.
pub fn csum_add(mut sum: u32, data: &[u8]) -> u32 {
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    sum
}

pub fn csum_fold(mut sum: u32) -> u16 {
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Checksum including the TCP/UDP pseudo-header.
pub fn pseudo_csum(src: Ip, dst: Ip, proto: u8, seg: &[u8]) -> u16 {
    let mut s = csum_add(0, &src);
    s = csum_add(s, &dst);
    s += proto as u32 + seg.len() as u32;
    csum_fold(csum_add(s, seg))
}

#[derive(PartialEq, Clone, Copy)]
enum DhcpState {
    Init,
    Selecting,
    Requesting,
    Bound,
}

struct Dhcp {
    state: DhcpState,
    xid: u32,
    offer: Ip,
    server: Ip,
    next_ms: u64,
    renew_ms: u64,
    tries: u32,
}

pub struct Net {
    nic: FirmwareNic,
    pub mac: Mac,
    pub ip: Ip,
    pub mask: Ip,
    pub gw: Ip,
    pub dns: Ip,
    pub hostname: String,
    pub if_name: String,
    dhcp: Dhcp,
    arp: Vec<(Ip, Mac, u64)>,
    arp_wait: Vec<(Ip, Vec<u8>, u64)>,
    arp_asked: Vec<(Ip, u64)>,
    pub tcp: Tcp,
    pub mdns: mdns::Mdns,
    ip_id: u16,
    pub now: u64,
    rxbuf: Vec<u8>,
    pub rx_packets: u64,
    pub tx_packets: u64,
}

impl Net {
    pub fn up() -> Option<Net> {
        let nic = FirmwareNic::open()?;
        let mac = nic.mac;
        let hostname = alloc::format!("hydatek-{:02x}{:02x}", mac[4], mac[5]);
        let if_name = nic.name.clone();
        let mut xid = [0u8; 4];
        crate::rng::fill(&mut xid);
        Some(Net {
            nic,
            mac,
            ip: [0; 4],
            mask: [0; 4],
            gw: [0; 4],
            dns: [0; 4],
            mdns: mdns::Mdns::new(&hostname),
            hostname,
            if_name,
            dhcp: Dhcp { state: DhcpState::Init, xid: u32::from_le_bytes(xid), offer: [0; 4], server: [0; 4], next_ms: 0, renew_ms: 0, tries: 0 },
            arp: vec![],
            arp_wait: vec![],
            arp_asked: vec![],
            tcp: Tcp::new(),
            ip_id: 1,
            now: 0,
            rxbuf: vec![0u8; 2048],
            rx_packets: 0,
            tx_packets: 0,
        })
    }

    /// Firmware event signalled when a packet is waiting.
    pub fn wait_event(&self) -> crate::efi::Event {
        self.nic.wait_event()
    }

    pub fn configured(&self) -> bool {
        self.dhcp.state == DhcpState::Bound
    }

    pub fn link_up(&self) -> bool {
        self.nic.link_up()
    }

    /// Process received frames and timers. `now` is in milliseconds.
    pub fn poll(&mut self, now: u64) {
        self.now = now;
        for _ in 0..256 {
            let mut buf = core::mem::take(&mut self.rxbuf);
            let n = self.nic.recv(&mut buf);
            if let Some(n) = n {
                self.rx_packets += 1;
                self.frame(&buf[..n]);
            }
            self.rxbuf = buf;
            if n.is_none() {
                break;
            }
        }
        self.dhcp_timer();
        let out = self.tcp.poll(now);
        for (dst, seg) in out {
            self.send_ip(dst, PROTO_TCP, &seg);
        }
        if self.configured() {
            if let Some(pkt) = self.mdns.timer(now, self.ip) {
                self.send_udp(MDNS_GROUP, 5353, 5353, &pkt);
            }
        }
        self.arp_wait.retain(|w| now < w.2 + 3000);
        self.arp_asked.retain(|a| now < a.1 + 1000);
    }

    fn frame(&mut self, f: &[u8]) {
        if f.len() < 14 {
            return;
        }
        let ty = u16::from_be_bytes([f[12], f[13]]);
        let p = &f[14..];
        match ty {
            ETH_ARP => self.arp_in(p),
            ETH_IP => self.ip_in(p),
            _ => {}
        }
    }

    // ---- Ethernet / ARP -------------------------------------------------

    fn send_eth(&mut self, dst: Mac, ty: u16, payload: &[u8]) {
        let mut f = Vec::with_capacity(14 + payload.len());
        f.extend_from_slice(&dst);
        f.extend_from_slice(&self.mac);
        f.extend_from_slice(&ty.to_be_bytes());
        f.extend_from_slice(payload);
        if self.nic.send(&f) {
            self.tx_packets += 1;
        }
    }

    fn arp_packet(&self, op: u16, tha: Mac, tpa: Ip) -> Vec<u8> {
        let mut a = Vec::with_capacity(28);
        a.extend_from_slice(&[0, 1, 8, 0, 6, 4]);
        a.extend_from_slice(&op.to_be_bytes());
        a.extend_from_slice(&self.mac);
        a.extend_from_slice(&self.ip);
        a.extend_from_slice(&tha);
        a.extend_from_slice(&tpa);
        a
    }

    fn arp_learn(&mut self, ip: Ip, mac: Mac) {
        if ip == [0; 4] {
            return;
        }
        self.arp.retain(|e| e.0 != ip);
        self.arp.push((ip, mac, self.now));
        if self.arp.len() > 64 {
            self.arp.remove(0);
        }
        let waiting: Vec<Vec<u8>> = {
            let (w, keep): (Vec<_>, Vec<_>) = core::mem::take(&mut self.arp_wait).into_iter().partition(|w| w.0 == ip);
            self.arp_wait = keep;
            w.into_iter().map(|w| w.1).collect()
        };
        for pkt in waiting {
            self.send_eth(mac, ETH_IP, &pkt);
        }
    }

    fn arp_in(&mut self, p: &[u8]) {
        if p.len() < 28 || p[0..6] != [0, 1, 8, 0, 6, 4] {
            return;
        }
        let op = u16::from_be_bytes([p[6], p[7]]);
        let sha: Mac = p[8..14].try_into().unwrap();
        let spa: Ip = p[14..18].try_into().unwrap();
        let tpa: Ip = p[24..28].try_into().unwrap();
        let for_us = self.ip != [0; 4] && tpa == self.ip;
        if for_us || self.arp.iter().any(|e| e.0 == spa) {
            self.arp_learn(spa, sha);
        }
        if op == 1 && for_us {
            let r = self.arp_packet(2, sha, spa);
            self.send_eth(sha, ETH_ARP, &r);
        }
    }

    fn arp_request(&mut self, ip: Ip) {
        if self.arp_asked.iter().any(|a| a.0 == ip) {
            return;
        }
        self.arp_asked.push((ip, self.now));
        let r = self.arp_packet(1, [0; 6], ip);
        self.send_eth(BROADCAST_MAC, ETH_ARP, &r);
    }

    // ---- IPv4 -----------------------------------------------------------

    fn same_subnet(&self, ip: Ip) -> bool {
        (0..4).all(|i| ip[i] & self.mask[i] == self.ip[i] & self.mask[i])
    }

    pub fn send_ip(&mut self, dst: Ip, proto: u8, payload: &[u8]) {
        let mut h = vec![0u8; 20];
        let total = (20 + payload.len()) as u16;
        h[0] = 0x45;
        h[2..4].copy_from_slice(&total.to_be_bytes());
        h[4..6].copy_from_slice(&self.ip_id.to_be_bytes());
        self.ip_id = self.ip_id.wrapping_add(1);
        h[6] = 0x40; // don't fragment
        h[8] = if dst == MDNS_GROUP { 255 } else { 64 };
        h[9] = proto;
        h[12..16].copy_from_slice(&self.ip);
        h[16..20].copy_from_slice(&dst);
        let c = csum_fold(csum_add(0, &h));
        h[10..12].copy_from_slice(&c.to_be_bytes());
        h.extend_from_slice(payload);
        let bcast = dst == [255; 4] || (self.mask != [0; 4] && (0..4).all(|i| dst[i] | self.mask[i] == 255));
        if bcast {
            return self.send_eth(BROADCAST_MAC, ETH_IP, &h);
        }
        if dst[0] >= 224 && dst[0] <= 239 {
            let m = [0x01, 0x00, 0x5e, dst[1] & 0x7f, dst[2], dst[3]];
            return self.send_eth(m, ETH_IP, &h);
        }
        let hop = if self.same_subnet(dst) || self.gw == [0; 4] { dst } else { self.gw };
        if let Some(e) = self.arp.iter().find(|e| e.0 == hop) {
            let mac = e.1;
            self.send_eth(mac, ETH_IP, &h);
        } else {
            if self.arp_wait.len() < 64 {
                self.arp_wait.push((hop, h, self.now));
            }
            self.arp_request(hop);
        }
    }

    fn ip_in(&mut self, p: &[u8]) {
        if p.len() < 20 || p[0] >> 4 != 4 {
            return;
        }
        let ihl = ((p[0] & 15) as usize) * 4;
        let total = u16::from_be_bytes([p[2], p[3]]) as usize;
        if ihl < 20 || total > p.len() || total < ihl {
            return;
        }
        let frag = u16::from_be_bytes([p[6], p[7]]);
        if frag & 0x3fff != 0 {
            return; // fragments are not supported
        }
        if csum_fold(csum_add(0, &p[..ihl])) != 0 {
            return;
        }
        let proto = p[9];
        let src: Ip = p[12..16].try_into().unwrap();
        let dst: Ip = p[16..20].try_into().unwrap();
        let body = &p[ihl..total];
        let ours = dst == self.ip || dst == [255; 4] || dst == MDNS_GROUP || (self.mask != [0; 4] && (0..4).all(|i| dst[i] | self.mask[i] == 255));
        if !ours && !(self.ip == [0; 4] && proto == PROTO_UDP) {
            return;
        }
        crate::rng::stir(self.now ^ (src[3] as u64) << 32);
        match proto {
            PROTO_ICMP if dst == self.ip => self.icmp_in(src, body),
            PROTO_UDP => self.udp_in(src, body),
            PROTO_TCP if dst == self.ip => {
                if pseudo_csum(src, dst, PROTO_TCP, body) != 0 {
                    return;
                }
                let out = self.tcp.input(src, self.ip, body, self.now);
                for (d, seg) in out {
                    self.send_ip(d, PROTO_TCP, &seg);
                }
            }
            _ => {}
        }
    }

    fn icmp_in(&mut self, src: Ip, p: &[u8]) {
        if p.len() >= 8 && p[0] == 8 {
            let mut r = p.to_vec();
            r[0] = 0;
            r[2] = 0;
            r[3] = 0;
            let c = csum_fold(csum_add(0, &r));
            r[2..4].copy_from_slice(&c.to_be_bytes());
            self.send_ip(src, PROTO_ICMP, &r);
        }
    }

    // ---- UDP --------------------------------------------------------------

    pub fn send_udp(&mut self, dst: Ip, sport: u16, dport: u16, data: &[u8]) {
        let mut u = Vec::with_capacity(8 + data.len());
        u.extend_from_slice(&sport.to_be_bytes());
        u.extend_from_slice(&dport.to_be_bytes());
        u.extend_from_slice(&((8 + data.len()) as u16).to_be_bytes());
        u.extend_from_slice(&[0, 0]);
        u.extend_from_slice(data);
        let mut c = pseudo_csum(self.ip, dst, PROTO_UDP, &u);
        if c == 0 {
            c = 0xffff;
        }
        u[6..8].copy_from_slice(&c.to_be_bytes());
        self.send_ip(dst, PROTO_UDP, &u);
    }

    fn udp_in(&mut self, src: Ip, p: &[u8]) {
        if p.len() < 8 {
            return;
        }
        let sport = u16::from_be_bytes([p[0], p[1]]);
        let dport = u16::from_be_bytes([p[2], p[3]]);
        let len = (u16::from_be_bytes([p[4], p[5]]) as usize).min(p.len());
        if len < 8 {
            return;
        }
        let data = &p[8..len];
        match dport {
            68 => self.dhcp_in(data),
            5353 if self.configured() => {
                if let Some((resp, unicast)) = self.mdns.query(data, self.ip) {
                    let (dst, port) = if unicast || sport != 5353 { (src, sport) } else { (MDNS_GROUP, 5353) };
                    self.send_udp(dst, 5353, port, &resp);
                }
            }
            _ => {}
        }
    }

    // ---- DHCP ---------------------------------------------------------------

    fn dhcp_send(&mut self, msg: u8) {
        let mut d = vec![0u8; 240];
        d[0] = 1;
        d[1] = 1;
        d[2] = 6;
        d[4..8].copy_from_slice(&self.dhcp.xid.to_be_bytes());
        d[10] = 0x80; // broadcast replies
        d[28..34].copy_from_slice(&self.mac);
        d[236..240].copy_from_slice(&[99, 130, 83, 99]);
        d.extend_from_slice(&[53, 1, msg]);
        d.extend_from_slice(&[61, 7, 1]);
        d.extend_from_slice(&self.mac);
        if msg == 3 {
            d.extend_from_slice(&[50, 4]);
            d.extend_from_slice(&self.dhcp.offer);
            if self.dhcp.server != [0; 4] {
                d.extend_from_slice(&[54, 4]);
                d.extend_from_slice(&self.dhcp.server);
            }
        }
        let hn = self.hostname.clone();
        d.extend_from_slice(&[12, hn.len() as u8]);
        d.extend_from_slice(hn.as_bytes());
        d.extend_from_slice(&[55, 4, 1, 3, 6, 15, 255]);
        let saved = self.ip;
        self.ip = [0; 4];
        self.send_udp([255; 4], 68, 67, &d);
        self.ip = saved;
    }

    fn dhcp_timer(&mut self) {
        let now = self.now;
        match self.dhcp.state {
            DhcpState::Init => {
                if now >= self.dhcp.next_ms {
                    self.dhcp_send(1);
                    self.dhcp.state = DhcpState::Selecting;
                    self.dhcp.tries += 1;
                    self.dhcp.next_ms = now + 3000.min(1000 * self.dhcp.tries as u64);
                }
            }
            DhcpState::Selecting | DhcpState::Requesting => {
                if now >= self.dhcp.next_ms {
                    self.dhcp.state = DhcpState::Init;
                }
            }
            DhcpState::Bound => {
                if now >= self.dhcp.renew_ms {
                    self.dhcp.offer = self.ip;
                    self.dhcp_send(3);
                    self.dhcp.renew_ms = now + 60_000;
                }
            }
        }
    }

    fn dhcp_in(&mut self, d: &[u8]) {
        if d.len() < 240 || d[0] != 2 || u32::from_be_bytes([d[4], d[5], d[6], d[7]]) != self.dhcp.xid || d[236..240] != [99, 130, 83, 99] {
            return;
        }
        let yiaddr: Ip = d[16..20].try_into().unwrap();
        let (mut msg, mut mask, mut gw, mut dns, mut server, mut lease) = (0u8, [255, 255, 255, 0], [0u8; 4], [0u8; 4], [0u8; 4], 3600u32);
        let mut i = 240;
        while i < d.len() {
            let code = d[i];
            if code == 255 {
                break;
            }
            if code == 0 {
                i += 1;
                continue;
            }
            if i + 1 >= d.len() {
                break;
            }
            let len = d[i + 1] as usize;
            let v = &d[(i + 2).min(d.len())..(i + 2 + len).min(d.len())];
            match code {
                53 if !v.is_empty() => msg = v[0],
                1 if v.len() >= 4 => mask.copy_from_slice(&v[..4]),
                3 if v.len() >= 4 => gw.copy_from_slice(&v[..4]),
                6 if v.len() >= 4 => dns.copy_from_slice(&v[..4]),
                54 if v.len() >= 4 => server.copy_from_slice(&v[..4]),
                51 if v.len() >= 4 => lease = u32::from_be_bytes([v[0], v[1], v[2], v[3]]),
                _ => {}
            }
            i += 2 + len;
        }
        match (msg, self.dhcp.state) {
            (2, DhcpState::Selecting) => {
                self.dhcp.offer = yiaddr;
                self.dhcp.server = server;
                self.dhcp.state = DhcpState::Requesting;
                self.dhcp.next_ms = self.now + 3000;
                self.dhcp_send(3);
            }
            (5, DhcpState::Requesting) | (5, DhcpState::Bound) => {
                let fresh = self.ip != yiaddr;
                self.ip = yiaddr;
                self.mask = mask;
                self.gw = gw;
                self.dns = dns;
                self.dhcp.state = DhcpState::Bound;
                self.dhcp.tries = 0;
                self.dhcp.renew_ms = self.now + (lease as u64).clamp(60, 86_400) * 500;
                if fresh {
                    log!("net: DHCP lease {} gw {} ({}s)", ip_str(self.ip), ip_str(self.gw), lease);
                    // gratuitous ARP, then announce ourselves over mDNS
                    let g = self.arp_packet(1, [0; 6], self.ip);
                    self.send_eth(BROADCAST_MAC, ETH_ARP, &g);
                    self.mdns.announce(self.now);
                }
            }
            (6, _) => {
                self.dhcp.state = DhcpState::Init;
                self.dhcp.next_ms = self.now + 1000;
            }
            _ => {}
        }
    }
}
