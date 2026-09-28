//! GIF (87a/89a): the first frame, with its transparency and interlacing.

use super::{check_size, Image, Result};
use alloc::vec;
use alloc::vec::Vec;

const BAD: &str = "the GIF image is damaged";

fn palette(d: &[u8], n: usize) -> Vec<u32> {
    d.chunks(3).take(n).map(|c| 0xff00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect()
}

/// Variable-width LZW as GIF uses it.
fn lzw(data: &[u8], min: u32, max_out: usize) -> Result<Vec<u8>> {
    if !(2..=11).contains(&min) {
        return Err(BAD);
    }
    let clear = 1usize << min;
    let end = clear + 1;
    // each code: (prefix code, last byte, length)
    let mut prefix = vec![0u16; 4096];
    let mut suffix = vec![0u8; 4096];
    let mut first = vec![0u8; 4096];
    let mut lens = vec![0u16; 4096];
    for i in 0..clear {
        suffix[i] = i as u8;
        first[i] = i as u8;
        lens[i] = 1;
    }
    let mut out = Vec::with_capacity(max_out);
    let mut size = min + 1;
    let mut next = end + 1;
    let mut prev: Option<usize> = None;
    let (mut acc, mut nbits, mut pos) = (0u32, 0u32, 0usize);
    let mut stack: Vec<u8> = Vec::with_capacity(4096);
    loop {
        while nbits < size {
            let Some(&b) = data.get(pos) else { return Ok(out) };
            acc |= (b as u32) << nbits;
            nbits += 8;
            pos += 1;
        }
        let code = (acc & ((1 << size) - 1)) as usize;
        acc >>= size;
        nbits -= size;
        if code == clear {
            size = min + 1;
            next = end + 1;
            prev = None;
            continue;
        }
        if code == end || out.len() >= max_out {
            return Ok(out);
        }
        let Some(p) = prev else {
            if code >= clear {
                return Err(BAD);
            }
            out.push(code as u8);
            prev = Some(code);
            continue;
        };
        let known = code < next;
        if !known && code != next {
            return Err(BAD);
        }
        // the string for `code` (for a new code: prev's string + its first byte)
        let (start, extra) = if known { (code, None) } else { (p, Some(first[p])) };
        stack.clear();
        let mut c = start;
        loop {
            stack.push(suffix[c]);
            if lens[c] <= 1 {
                break;
            }
            c = prefix[c] as usize;
        }
        out.extend(stack.iter().rev());
        if let Some(e) = extra {
            out.push(e);
        }
        if next < 4096 {
            prefix[next] = p as u16;
            suffix[next] = if known { first[code] } else { first[p] };
            first[next] = first[p];
            lens[next] = lens[p] + 1;
            next += 1;
            if next == 1 << size && size < 12 {
                size += 1;
            }
        }
        prev = Some(code);
    }
}

pub fn decode(d: &[u8]) -> Result<Image> {
    if d.len() < 13 {
        return Err(BAD);
    }
    let sw = u16::from_le_bytes([d[6], d[7]]) as u32;
    let sh = u16::from_le_bytes([d[8], d[9]]) as u32;
    let flags = d[10];
    let mut pos = 13;
    let mut global = Vec::new();
    if flags & 0x80 != 0 {
        let n = 2usize << (flags & 7);
        global = palette(d.get(pos..pos + 3 * n).ok_or(BAD)?, n);
        pos += 3 * n;
    }
    let mut transparent: Option<u8> = None;
    while pos < d.len() {
        match d[pos] {
            0x21 => {
                let label = *d.get(pos + 1).ok_or(BAD)?;
                pos += 2;
                // graphic control extension: transparency
                if label == 0xf9 && d.get(pos) == Some(&4) {
                    let b = d.get(pos + 1..pos + 5).ok_or(BAD)?;
                    if b[0] & 1 != 0 {
                        transparent = Some(b[3]);
                    }
                }
                while let Some(&n) = d.get(pos) {
                    pos += 1 + n as usize;
                    if n == 0 {
                        break;
                    }
                }
            }
            0x2c => {
                let b = d.get(pos + 1..pos + 10).ok_or(BAD)?;
                let (ix, iy) = (u16::from_le_bytes([b[0], b[1]]) as usize, u16::from_le_bytes([b[2], b[3]]) as usize);
                let (iw, ih) = (u16::from_le_bytes([b[4], b[5]]) as usize, u16::from_le_bytes([b[6], b[7]]) as usize);
                let f = b[8];
                pos += 10;
                let pal = if f & 0x80 != 0 {
                    let n = 2usize << (f & 7);
                    let p = palette(d.get(pos..pos + 3 * n).ok_or(BAD)?, n);
                    pos += 3 * n;
                    p
                } else {
                    global.clone()
                };
                let (w, h) = (if sw == 0 { iw as u32 } else { sw }, if sh == 0 { ih as u32 } else { sh });
                check_size(w, h)?;
                let min = *d.get(pos).ok_or(BAD)? as u32;
                pos += 1;
                let mut data = Vec::new();
                while let Some(&n) = d.get(pos) {
                    pos += 1;
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(d.get(pos..pos + n as usize).ok_or(BAD)?);
                    pos += n as usize;
                }
                let idx = lzw(&data, min, iw * ih)?;
                let mut img = Image::new(w, h);
                // interlaced rows come in four passes
                let rows: Vec<usize> = if f & 0x40 != 0 {
                    let mut r = Vec::with_capacity(ih);
                    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
                        r.extend((start..ih).step_by(step));
                    }
                    r
                } else {
                    (0..ih).collect()
                };
                for (k, &ry) in rows.iter().enumerate() {
                    for x in 0..iw {
                        let Some(&c) = idx.get(k * iw + x) else { break };
                        let (px, py) = (ix + x, iy + ry);
                        if px >= w as usize || py >= h as usize || Some(c) == transparent {
                            continue;
                        }
                        img.px[py * w as usize + px] = pal.get(c as usize).copied().unwrap_or(0xff00_0000);
                    }
                }
                return Ok(img);
            }
            0x3b => break,
            _ => return Err(BAD),
        }
    }
    Err(BAD)
}
