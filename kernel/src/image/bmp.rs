//! BMP: uncompressed 1/4/8-bit palette, 16/24/32-bit and bitfield images.

use super::{check_size, Image, Result};
use alloc::vec::Vec;

const BAD: &str = "the BMP image is damaged";

pub fn decode(d: &[u8]) -> Result<Image> {
    let u32at = |p: usize| d.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or(BAD);
    let u16at = |p: usize| d.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or(BAD);
    let offset = u32at(10)? as usize;
    let hsize = u32at(14)? as usize;
    if hsize < 40 {
        return Err("old-style BMP images aren't supported");
    }
    let w = u32at(18)? as i32;
    let hraw = u32at(22)? as i32;
    let bpp = u16at(28)?;
    let comp = u32at(30)?;
    let top_down = hraw < 0;
    let (w, h) = (w.unsigned_abs(), hraw.unsigned_abs());
    check_size(w, h)?;
    if comp != 0 && comp != 3 {
        return Err("compressed BMP images aren't supported");
    }
    let masks: [u32; 4] = if comp == 3 {
        let m = |p| u32at(p).unwrap_or(0);
        if hsize >= 56 { [m(54), m(58), m(62), m(66)] } else { [m(54), m(58), m(62), 0] }
    } else if bpp == 16 {
        [0x7c00, 0x3e0, 0x1f, 0]
    } else {
        [0xff0000, 0xff00, 0xff, if bpp == 32 { 0xff00_0000 } else { 0 }]
    };
    let colors = u32at(46)? as usize;
    let pal_n = if colors == 0 && bpp <= 8 { 1 << bpp } else { colors };
    let pal: Vec<u32> = (0..pal_n.min(256))
        .map(|i| {
            let p = 14 + hsize + 4 * i;
            d.get(p..p + 3).map(|c| 0xff00_0000 | (c[2] as u32) << 16 | (c[1] as u32) << 8 | c[0] as u32).unwrap_or(0xff00_0000)
        })
        .collect();
    let stride = ((w as usize * bpp as usize + 31) / 32) * 4;
    let chan = |v: u32, m: u32| -> u32 {
        if m == 0 {
            return 255;
        }
        let shift = m.trailing_zeros();
        let max = m >> shift;
        ((v & m) >> shift) * 255 / max
    };
    let mut img = Image::new(w, h);
    let mut any_alpha = false;
    for y in 0..h as usize {
        let src = offset + (if top_down { y } else { h as usize - 1 - y }) * stride;
        let row = d.get(src..src + stride).ok_or(BAD)?;
        for x in 0..w as usize {
            let p = match bpp {
                1 | 4 | 8 => {
                    let bit = x * bpp as usize;
                    let i = (row[bit / 8] >> (8 - bpp as usize - bit % 8)) & ((1u16 << bpp) - 1) as u8;
                    pal.get(i as usize).copied().unwrap_or(0xff00_0000)
                }
                16 | 24 | 32 => {
                    let n = bpp as usize / 8;
                    let mut v = 0u32;
                    for k in 0..n {
                        v |= (row[x * n + k] as u32) << (8 * k);
                    }
                    let a = chan(v, masks[3]);
                    if a != 255 {
                        any_alpha = true;
                    }
                    a << 24 | chan(v, masks[0]) << 16 | chan(v, masks[1]) << 8 | chan(v, masks[2])
                }
                _ => return Err("this kind of BMP isn't supported"),
            };
            img.px[y * w as usize + x] = p;
        }
    }
    // many 32-bit BMPs leave the alpha byte at zero: treat them as opaque
    if bpp == 32 && any_alpha && img.px.iter().all(|p| p >> 24 == 0) {
        for p in img.px.iter_mut() {
            *p |= 0xff00_0000;
        }
    }
    Ok(img)
}
