//! DNS (RFC 1035) stub resolver messages: A-record queries and answers.

use alloc::vec::Vec;

pub fn query(id: u16, name: &str) -> Vec<u8> {
    let mut q = Vec::with_capacity(32 + name.len());
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00]); // recursion desired
    q.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]); // 1 question
    for label in name.trim_end_matches('.').split('.') {
        let l = label.as_bytes();
        q.push(l.len().min(63) as u8);
        q.extend_from_slice(&l[..l.len().min(63)]);
    }
    q.push(0);
    q.extend_from_slice(&[0, 1, 0, 1]); // type A, class IN
    q
}

fn skip_name(p: &[u8], mut i: usize) -> Option<usize> {
    loop {
        let l = *p.get(i)? as usize;
        if l == 0 {
            return Some(i + 1);
        }
        if l & 0xC0 == 0xC0 {
            return Some(i + 2);
        }
        i += 1 + l;
    }
}

/// The answer to query `id`: IPv4 addresses and the shortest TTL (seconds).
pub fn answer(p: &[u8], id: u16) -> Option<Result<(Vec<[u8; 4]>, u32), &'static str>> {
    if p.len() < 12 || u16::from_be_bytes([p[0], p[1]]) != id || p[2] & 0x80 == 0 {
        return None;
    }
    match p[3] & 0x0F {
        0 => {}
        3 => return Some(Err("no such site")),
        _ => return Some(Err("the name server failed")),
    }
    let qd = u16::from_be_bytes([p[4], p[5]]) as usize;
    let an = u16::from_be_bytes([p[6], p[7]]) as usize;
    let mut i = 12;
    for _ in 0..qd {
        i = skip_name(p, i)? + 4;
    }
    let mut ips = Vec::new();
    let mut ttl = u32::MAX;
    for _ in 0..an {
        i = skip_name(p, i)?;
        let ty = u16::from_be_bytes([*p.get(i)?, *p.get(i + 1)?]);
        let t = u32::from_be_bytes([*p.get(i + 4)?, *p.get(i + 5)?, *p.get(i + 6)?, *p.get(i + 7)?]);
        let len = u16::from_be_bytes([*p.get(i + 8)?, *p.get(i + 9)?]) as usize;
        let data = p.get(i + 10..i + 10 + len)?;
        if ty == 1 && len == 4 {
            ips.push([data[0], data[1], data[2], data[3]]);
            ttl = ttl.min(t);
        }
        i += 10 + len;
    }
    if ips.is_empty() {
        return Some(Err("no such site"));
    }
    Some(Ok((ips, ttl.clamp(30, 3600))))
}

pub fn parse_ip(s: &str) -> Option<[u8; 4]> {
    let mut ip = [0u8; 4];
    let mut n = 0;
    for part in s.split('.') {
        if n == 4 || part.is_empty() || part.len() > 3 {
            return None;
        }
        ip[n] = part.parse().ok()?;
        n += 1;
    }
    if n == 4 {
        Some(ip)
    } else {
        None
    }
}
