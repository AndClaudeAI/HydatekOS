//! A reader for DER, the binary encoding certificates use.

pub const SEQ: u8 = 0x30;
pub const INT: u8 = 0x02;
pub const BITS: u8 = 0x03;
pub const OCTETS: u8 = 0x04;
pub const OID: u8 = 0x06;
pub const BOOL: u8 = 0x01;

#[derive(Clone, Copy)]
pub struct Der<'a> {
    d: &'a [u8],
    pos: usize,
}

/// One element: its tag, contents and whole encoding.
#[derive(Clone, Copy)]
pub struct Tlv<'a> {
    pub tag: u8,
    pub body: &'a [u8],
    pub raw: &'a [u8],
}

impl<'a> Der<'a> {
    pub fn new(d: &'a [u8]) -> Der<'a> {
        Der { d, pos: 0 }
    }

    pub fn peek_tag(&self) -> Option<u8> {
        self.d.get(self.pos).copied()
    }

    pub fn next(&mut self) -> Option<Tlv<'a>> {
        let start = self.pos;
        let tag = *self.d.get(self.pos)?;
        if tag & 0x1f == 0x1f {
            return None; // multi-byte tags aren't used in certificates
        }
        let first = *self.d.get(self.pos + 1)?;
        let mut p = self.pos + 2;
        let len = if first < 0x80 {
            first as usize
        } else {
            let n = (first & 0x7f) as usize;
            if n == 0 || n > 4 {
                return None;
            }
            let mut l = 0usize;
            for _ in 0..n {
                l = (l << 8) | *self.d.get(p)? as usize;
                p += 1;
            }
            l
        };
        let end = p.checked_add(len)?;
        if end > self.d.len() {
            return None;
        }
        self.pos = end;
        Some(Tlv { tag, body: &self.d[p..end], raw: &self.d[start..end] })
    }

    /// The next element, which must have `tag`.
    pub fn expect(&mut self, tag: u8) -> Option<Tlv<'a>> {
        let t = self.next()?;
        (t.tag == tag).then_some(t)
    }

    /// The next element if it has `tag`.
    pub fn optional(&mut self, tag: u8) -> Option<Tlv<'a>> {
        if self.peek_tag() == Some(tag) {
            self.next()
        } else {
            None
        }
    }
}

impl<'a> Tlv<'a> {
    pub fn inner(&self) -> Der<'a> {
        Der::new(self.body)
    }

    /// A BIT STRING's bytes (no unused bits allowed).
    pub fn bit_bytes(&self) -> Option<&'a [u8]> {
        if self.tag != BITS || self.body.first() != Some(&0) {
            return None;
        }
        Some(&self.body[1..])
    }

    /// An INTEGER's magnitude, without leading zeros.
    pub fn uint(&self) -> &'a [u8] {
        let z = self.body.iter().position(|&b| b != 0).unwrap_or(self.body.len());
        &self.body[z..]
    }
}
