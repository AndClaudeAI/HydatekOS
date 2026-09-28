//! A small PDF writer (PDF 1.4): pages made of a picture (deflated RGB) and
//! text in the standard Helvetica font, either visible or invisible (a text
//! layer over the picture, so the PDF can be searched and copied from).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// Text on a page. Coordinates are points from the top-left.
pub struct Text {
    pub x: i32,
    /// baseline
    pub y: i32,
    pub size: i32,
    pub s: String,
    /// stretch the text to this width (0: natural width)
    pub w: i32,
    pub visible: bool,
    /// 0xRRGGBB
    pub color: u32,
}

pub struct Page {
    /// size in points
    pub w: i32,
    pub h: i32,
    /// a picture drawn at (x, y, w, h) points: (pixels wide, high, RGB bytes)
    pub image: Option<(i32, i32, i32, i32, u32, u32, Vec<u8>)>,
    pub texts: Vec<Text>,
}

/// Helvetica's widths (1/1000 em) for characters 32..=126.
const HELV: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

/// The WinAnsi byte for a character ('?' when it has none).
fn win_ansi(c: char) -> u8 {
    match c as u32 {
        32..=126 => c as u8,
        0xA0..=0xFF => c as u32 as u8,
        _ => match c {
            '€' => 0x80,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '•' => 0x95,
            '–' => 0x96,
            '—' => 0x97,
            '™' => 0x99,
            '\t' => b' ',
            _ => b'?',
        },
    }
}

/// Width in 1/1000 points of `s` at `size`.
pub fn text_width(s: &str, size: i32) -> i32 {
    let mut w: i64 = 0;
    for c in s.chars() {
        let b = win_ansi(c);
        w += match b {
            32..=126 => HELV[(b - 32) as usize] as i64,
            0x95 => 350,
            0x96 => 556,
            0x97 => 1000,
            0x85 => 1000,
            _ => 556,
        };
    }
    (w * size as i64) as i32
}

/// Break `s` into lines at most `width` points wide.
pub fn wrap(s: &str, size: i32, width: i32) -> Vec<String> {
    let mut out = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() { String::from(word) } else { format!("{} {}", line, word) };
            if text_width(&cand, size) > width * 1000 && !line.is_empty() {
                out.push(core::mem::take(&mut line));
                line = String::from(word);
            } else {
                line = cand;
            }
        }
        out.push(line);
    }
    out
}

fn pdf_string(s: &str) -> Vec<u8> {
    let mut o = Vec::with_capacity(s.len() + 2);
    o.push(b'(');
    for c in s.chars() {
        let b = win_ansi(c);
        match b {
            b'(' | b')' | b'\\' => {
                o.push(b'\\');
                o.push(b);
            }
            32..=126 => o.push(b),
            _ => o.extend_from_slice(format!("\\{:03o}", b).as_bytes()),
        }
    }
    o.push(b')');
    o
}

fn rgb(c: u32) -> String {
    let f = |v: u32| {
        let t = v * 1000 / 255;
        format!("{}.{:03}", t / 1000, t % 1000)
    };
    format!("{} {} {}", f((c >> 16) & 255), f((c >> 8) & 255), f(c & 255))
}

/// Write the PDF.
pub fn write(title: &str, author: &str, pages: &[Page]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<usize> = Vec::new();
    // objects: 1 catalog, 2 pages, 3 font, 4 info, then per page: page, content, image
    let n = pages.len();
    let page_obj = |i: usize| 5 + i * 3;
    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, id: usize, body: &[u8]| {
        while offsets.len() < id {
            offsets.push(0);
        }
        offsets[id - 1] = out.len();
        out.extend_from_slice(format!("{} 0 obj\n", id).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    let stream = |dict: &str, data: &[u8]| -> Vec<u8> {
        let mut b = format!("<< {} /Length {} >>\nstream\n", dict, data.len()).into_bytes();
        b.extend_from_slice(data);
        b.extend_from_slice(b"\nendstream");
        b
    };
    obj(&mut out, &mut offsets, 1, b"<< /Type /Catalog /Pages 2 0 R >>");
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", page_obj(i))).collect();
    obj(&mut out, &mut offsets, 2, format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), n).as_bytes());
    obj(&mut out, &mut offsets, 3, b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    let mut info = b"<< /Producer (Hyda Slides) /Title ".to_vec();
    info.extend_from_slice(&pdf_string(title));
    if !author.is_empty() {
        info.extend_from_slice(b" /Author ");
        info.extend_from_slice(&pdf_string(author));
    }
    info.extend_from_slice(b" >>");
    obj(&mut out, &mut offsets, 4, &info);
    for (i, p) in pages.iter().enumerate() {
        let (pid, cid, iid) = (page_obj(i), page_obj(i) + 1, page_obj(i) + 2);
        let mut c: Vec<u8> = Vec::new();
        let mut xobj = String::new();
        if let Some((x, y, w, h, pw, ph, data)) = &p.image {
            c.extend_from_slice(format!("q {} 0 0 {} {} {} cm /Im0 Do Q\n", w, h, x, p.h - y - h).as_bytes());
            xobj = format!("/XObject << /Im0 {} 0 R >>", iid);
            let z = crate::zip::zlib(data);
            obj(&mut out, &mut offsets, iid, &stream(&format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode", pw, ph), &z));
        } else {
            // keep the numbering regular
            obj(&mut out, &mut offsets, iid, b"null");
        }
        for t in &p.texts {
            if t.s.trim().is_empty() {
                continue;
            }
            let natural = text_width(&t.s, t.size).max(1);
            let tz = if t.w > 0 { ((t.w as i64 * 100_000) / natural as i64).clamp(10, 1000) } else { 100 };
            c.extend_from_slice(format!("BT {} Tr {} rg /F1 {} Tf {} Tz {} {} Td ", if t.visible { 0 } else { 3 }, rgb(t.color), t.size, tz, t.x, p.h - t.y).as_bytes());
            c.extend_from_slice(&pdf_string(&t.s));
            c.extend_from_slice(b" Tj ET\n");
        }
        let zc = crate::zip::zlib(&c);
        obj(&mut out, &mut offsets, cid, &stream("/Filter /FlateDecode", &zc));
        obj(&mut out, &mut offsets, pid, format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 3 0 R >> {} >> /Contents {} 0 R >>", p.w, p.h, xobj, cid).as_bytes());
    }
    let xref = out.len();
    let total = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", total).as_bytes());
    for o in &offsets {
        out.extend_from_slice(format!("{:010} 00000 n \n", o).as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R >>\nstartxref\n{}\n%%EOF\n", total, xref).as_bytes());
    out
}
