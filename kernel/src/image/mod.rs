//! Image decoders, written for HydatekOS: PNG, JPEG (baseline and
//! progressive), GIF and BMP. Every decoder produces an `Image` of ARGB
//! pixels (alpha in the top byte, not premultiplied).

pub mod bmp;
pub mod gif;
pub mod jpeg;
pub mod png;

use alloc::vec::Vec;

/// Larger images are refused (25 megapixels, 100 MB of pixels).
pub const MAX_PIXELS: u64 = 25_000_000;

#[derive(Clone, Debug)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

impl Image {
    pub fn new(w: u32, h: u32) -> Image {
        Image { w, h, px: alloc::vec![0; (w as usize) * (h as usize)] }
    }
}

pub type Result<T> = core::result::Result<T, &'static str>;

pub fn check_size(w: u32, h: u32) -> Result<()> {
    if w == 0 || h == 0 {
        return Err("the image is empty");
    }
    if w as u64 * h as u64 > MAX_PIXELS {
        return Err("the image is too big");
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    Bmp,
    Webp,
    Svg,
}

/// Recognise an image by its first bytes.
pub fn sniff(d: &[u8]) -> Option<Format> {
    if d.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if d.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(Format::Jpeg)
    } else if d.starts_with(b"GIF87a") || d.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if d.starts_with(b"BM") && d.len() > 26 {
        Some(Format::Bmp)
    } else if d.len() > 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        Some(Format::Webp)
    } else {
        let head = core::str::from_utf8(&d[..d.len().min(512)]).unwrap_or("");
        head.contains("<svg").then_some(Format::Svg)
    }
}

pub fn decode(d: &[u8]) -> Result<Image> {
    match sniff(d) {
        Some(Format::Png) => png::decode(d),
        Some(Format::Jpeg) => jpeg::decode(d),
        Some(Format::Gif) => gif::decode(d),
        Some(Format::Bmp) => bmp::decode(d),
        Some(Format::Webp) => Err("WebP images aren't supported yet"),
        Some(Format::Svg) => Err("SVG images aren't supported yet"),
        None => Err("not an image HydatekOS knows"),
    }
}
