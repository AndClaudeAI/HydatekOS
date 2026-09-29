//! JSON (RFC 8259): a parser into `Value` and a string writer, enough for
//! talking to web APIs.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(BTreeMap<String, Value>),
}

impl Value {
    /// A member of an object (Null when absent or not an object).
    pub fn get(&self, key: &str) -> &Value {
        match self {
            Value::Obj(m) => m.get(key).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn arr(&self) -> &[Value] {
        match self {
            Value::Arr(a) => a,
            _ => &[],
        }
    }
    #[allow(dead_code)]
    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }
}

static NULL: Value = Value::Null;

/// `s` as a JSON string, quotes included.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&alloc::format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn parse(text: &str) -> Option<Value> {
    let mut p = Parser { s: text.as_bytes(), i: 0, depth: 0 };
    let v = p.value()?;
    p.ws();
    if p.i == p.s.len() {
        Some(v)
    } else {
        None
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> bool {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.ws();
        let c = *self.s.get(self.i)?;
        match c {
            b'{' | b'[' => {
                self.depth += 1;
                if self.depth > 128 {
                    return None;
                }
                let v = if c == b'{' { self.object() } else { self.array() };
                self.depth -= 1;
                v
            }
            b'"' => self.string().map(Value::Str),
            b't' if self.eat(b"true") => Some(Value::Bool(true)),
            b'f' if self.eat(b"false") => Some(Value::Bool(false)),
            b'n' if self.eat(b"null") => Some(Value::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.i += 1;
        let mut m = BTreeMap::new();
        self.ws();
        if self.eat(b"}") {
            return Some(Value::Obj(m));
        }
        loop {
            self.ws();
            if self.s.get(self.i) != Some(&b'"') {
                return None;
            }
            let k = self.string()?;
            self.ws();
            if !self.eat(b":") {
                return None;
            }
            let v = self.value()?;
            m.insert(k, v);
            self.ws();
            if self.eat(b",") {
                continue;
            }
            return if self.eat(b"}") { Some(Value::Obj(m)) } else { None };
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.i += 1;
        let mut a = Vec::new();
        self.ws();
        if self.eat(b"]") {
            return Some(Value::Arr(a));
        }
        loop {
            a.push(self.value()?);
            self.ws();
            if self.eat(b",") {
                continue;
            }
            return if self.eat(b"]") { Some(Value::Arr(a)) } else { None };
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = self.s.get(self.i..self.i + 4)?;
        let v = u32::from_str_radix(core::str::from_utf8(h).ok()?, 16).ok()?;
        self.i += 4;
        Some(v)
    }

    fn string(&mut self) -> Option<String> {
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.s.get(self.i)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let cp = if (0xD800..0xDC00).contains(&hi) {
                                // a surrogate pair
                                if !self.eat(b"\\u") {
                                    return None;
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return None;
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(cp).unwrap_or('\u{FFFD}')
                        }
                        _ => return None,
                    };
                    let mut b = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                c if c < 0x20 => return None,
                c => out.push(c),
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.i;
        while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
            self.i += 1;
        }
        let t = core::str::from_utf8(&self.s[start..self.i]).ok()?;
        t.parse::<f64>().ok().map(Value::Num)
    }
}
