//! PNG (ISO/IEC 15948): every colour type and bit depth, palettes with
//! transparency, tRNS colour keys and Adam7 interlacing.

use super::{check_size, Image, Result};
use alloc::vec;
use alloc::vec::Vec;

fn be32(d: &[u8]) -> u32 {
    u32::from_be_bytes([d[0], d[1], d[2], d[3]])
}

pub fn decode(d: &[u8]) -> Result<Image> {
    let bad = "the PNG image is damaged";
    let mut pos = 8;
    let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
    let mut palette: Vec<u32> = Vec::new();
    let mut trns: Vec<u8> = Vec::new();
    let mut idat: Vec<u8> = Vec::new();
    while pos + 8 <= d.len() {
        let len = be32(&d[pos..]) as usize;
        let kind = &d[pos + 4..pos + 8];
        let body = d.get(pos + 8..pos + 8 + len).ok_or(bad)?;
        match kind {
            b"IHDR" => {
                if len < 13 {
                    return Err(bad);
                }
                w = be32(body);
                h = be32(&body[4..]);
                depth = body[8];
                ctype = body[9];
                interlace = body[12];
            }
            b"PLTE" => palette = body.chunks(3).filter(|c| c.len() == 3).map(|c| 0xff00_0000 | (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32).collect(),
            b"tRNS" => trns = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos += 12 + len;
    }
    check_size(w, h)?;
    let channels = match ctype {
        0 => 1,
        2 => 3,
        3 => 1,
        4 => 2,
        6 => 4,
        _ => return Err("the PNG image uses an unknown colour type"),
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) || (ctype != 0 && ctype != 3 && depth < 8) || (ctype == 3 && depth == 16) {
        return Err(bad);
    }
    if idat.len() < 6 {
        return Err(bad);
    }
    // zlib: 2-byte header, deflate data, Adler-32
    let bits_pp = channels * depth as usize;
    let row_bytes = |width: usize| (width * bits_pp).div_ceil(8);
    let raw = crate::zip::inflate(&idat[2..], (row_bytes(w as usize) + 1) * h as usize).ok_or(bad)?;
    let bpp = bits_pp.div_ceil(8).max(1);
    let mut img = Image::new(w, h);
    // tRNS colour key for grey / RGB (in sample units)
    let key: Option<[u16; 3]> = match (ctype, trns.len()) {
        (0, n) if n >= 2 => Some([u16::from_be_bytes([trns[0], trns[1]]); 3]),
        (2, n) if n >= 6 => Some([u16::from_be_bytes([trns[0], trns[1]]), u16::from_be_bytes([trns[2], trns[3]]), u16::from_be_bytes([trns[4], trns[5]])]),
        _ => None,
    };
    for (i, &a) in trns.iter().enumerate() {
        if ctype == 3 && i < palette.len() {
            palette[i] = (palette[i] & 0xff_ffff) | (a as u32) << 24;
        }
    }
    const PASSES: [(usize, usize, usize, usize); 7] = [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
    let passes: &[(usize, usize, usize, usize)] = if interlace == 1 { &PASSES } else { &[(0, 0, 1, 1)] };
    let mut at = 0usize;
    let (wu, hu) = (w as usize, h as usize);
    for &(x0, y0, dx, dy) in passes {
        if x0 >= wu || y0 >= hu {
            continue;
        }
        let pw = (wu - x0).div_ceil(dx);
        let ph = (hu - y0).div_ceil(dy);
        let rb = row_bytes(pw);
        let mut prev = vec![0u8; rb];
        let mut cur = vec![0u8; rb];
        for py in 0..ph {
            let filter = *raw.get(at).ok_or(bad)?;
            let line = raw.get(at + 1..at + 1 + rb).ok_or(bad)?;
            at += 1 + rb;
            for i in 0..rb {
                let a = if i >= bpp { cur[i - bpp] as i32 } else { 0 };
                let b = prev[i] as i32;
                let c = if i >= bpp { prev[i - bpp] as i32 } else { 0 };
                let x = line[i] as i32;
                cur[i] = match filter {
                    0 => x,
                    1 => x + a,
                    2 => x + b,
                    3 => x + (a + b) / 2,
                    4 => {
                        let p = a + b - c;
                        let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
                        x + if pa <= pb && pa <= pc {
                            a
                        } else if pb <= pc {
                            b
                        } else {
                            c
                        }
                    }
                    _ => return Err(bad),
                } as u8;
            }
            let y = y0 + py * dy;
            for px in 0..pw {
                let sample = |k: usize| -> u16 {
                    // k-th sample of this pixel
                    let idx = px * channels + k;
                    match depth {
                        16 => u16::from_be_bytes([cur[2 * idx], cur[2 * idx + 1]]),
                        8 => cur[idx] as u16,
                        _ => {
                            let bit = idx * depth as usize;
                            ((cur[bit / 8] >> (8 - depth as usize - bit % 8)) & ((1 << depth) - 1)) as u16
                        }
                    }
                };
                let to8 = |v: u16| -> u32 {
                    match depth {
                        16 => (v >> 8) as u32,
                        8 => v as u32,
                        1 => v as u32 * 255,
                        2 => v as u32 * 85,
                        _ => v as u32 * 17,
                    }
                };
                let argb = match ctype {
                    3 => palette.get(sample(0) as usize).copied().unwrap_or(0xff00_0000),
                    0 => {
                        let v = sample(0);
                        let g = to8(v);
                        let a = if key.is_some_and(|k| k[0] == v) { 0 } else { 255 };
                        a << 24 | g << 16 | g << 8 | g
                    }
                    4 => {
                        let g = to8(sample(0));
                        to8(sample(1)) << 24 | g << 16 | g << 8 | g
                    }
                    2 => {
                        let (r, g, b) = (sample(0), sample(1), sample(2));
                        let a = if key.is_some_and(|k| k == [r, g, b]) { 0 } else { 255 };
                        a << 24 | to8(r) << 16 | to8(g) << 8 | to8(b)
                    }
                    _ => to8(sample(3)) << 24 | to8(sample(0)) << 16 | to8(sample(1)) << 8 | to8(sample(2)),
                };
                img.px[y * wu + x0 + px * dx] = argb;
            }
            core::mem::swap(&mut prev, &mut cur);
        }
    }
    Ok(img)
}
