//! An ACPI Machine Language interpreter.
//!
//! The firmware describes the parts of a PC that aren't on PCI or USB (I2C
//! touchpads and their controllers, GPIOs, sensors, the embedded controller,
//! batteries, buttons) in ACPI tables, as AML bytecode: a tree of devices, each
//! with small programs (methods) that say what it is (_HID, _CID), whether it's
//! there (_STA), which resources it uses (_CRS) and device-specific facts
//! (_DSM, _DSD). This loads the DSDT and SSDTs into a namespace and runs those
//! methods.
//!
//! It covers what firmware writes in practice: every namespace object
//! (scopes, devices, names, methods, operation regions and their fields, index
//! and bank fields, buffer fields, mutexes, events, processors, power
//! resources, thermal zones, aliases, externals), and the full expression set
//! (arithmetic, logic, comparisons, conversions, strings, buffers, packages,
//! references, Index/DerefOf/RefOf/CondRefOf, Match, Mid, Concatenate,
//! ConcatenateResTemplate, If/Else, While, Break/Continue, Return, method
//! calls with arguments and locals). Hardware is reached through `Host`
//! (memory, I/O ports, PCI configuration space); the embedded controller,
//! SMBus and GPIO regions read as zero. _OSI answers like Windows, as Linux
//! does, so the firmware turns on the same devices it does for Windows.
//!
//! Plain logic, host-tested with tables compiled by iasl and QEMU's own.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// operation region spaces
pub const MEM: u8 = 0;
pub const IO: u8 = 1;
pub const PCI: u8 = 2;
pub const EC: u8 = 3;

/// The machine the AML runs on.
pub trait Host {
    /// Read `bits` (8, 16, 32 or 64) from a region space. For PCI
    /// configuration space `addr` is bus << 48 | device << 40 | function << 32 | offset.
    fn read(&mut self, space: u8, addr: u64, bits: u32) -> u64;
    fn write(&mut self, space: u8, addr: u64, bits: u32, v: u64);
    fn sleep_ms(&mut self, _ms: u64) {}
    /// A timer in 100 ns units.
    fn timer(&mut self) -> u64 {
        0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    None,
    Int(u64),
    Str(String),
    Buf(Vec<u8>),
    Pkg(Vec<Value>),
    Ref(Ref),
}

/// A reference: to a named object, or to one element of something.
#[derive(Clone, Debug, PartialEq)]
pub enum Ref {
    Named(String),
    Elem(Box<Loc>, usize),
}

/// Where a value lives.
#[derive(Clone, Debug, PartialEq)]
pub enum Loc {
    Named(String),
    Local(usize, u8),
    Arg(usize, u8),
    Temp(Box<Value>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Region(String),
    Index { index: String, data: String },
    Bank { region: String, bank: String, value: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub kind: FieldKind,
    pub bit: u64,
    pub bits: u64,
    /// FieldFlags: access type (bits 0-3), lock (4), update rule (5-6)
    pub flags: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Obj {
    Scope,
    Device,
    Processor,
    PowerRes,
    Thermal,
    Name(Value),
    Method { code: usize, start: usize, end: usize, args: u8 },
    /// built into the interpreter (_OSI)
    Builtin(u8),
    /// declared by External in one table, defined in another
    External { args: u8 },
    Region { space: u8, base: u64, len: u64 },
    Field(Field),
    BufField { loc: Loc, bit: u64, bits: u64 },
    Mutex,
    Event,
    Alias(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// bad bytecode at this offset
    Parse(usize),
    Unsupported(u16),
    NotFound(String),
    Type,
    /// too much work: a loop that won't end (waiting on hardware that isn't there)
    Budget,
    Depth,
    Fatal,
}

enum Stop {
    Ret(Value),
    Break,
    Continue,
    Err(Error),
}

impl From<Error> for Stop {
    fn from(e: Error) -> Stop {
        Stop::Err(e)
    }
}

type R<T> = Result<T, Stop>;

struct Frame {
    locals: Vec<Value>,
    args: Vec<Value>,
    /// objects the method created, removed when it returns
    temps: Vec<String>,
}

struct Ctx {
    code: Rc<[u8]>,
    ci: usize,
    p: usize,
    frame: usize,
    scope: String,
    /// loading a table (not running a method): unknown names are forward references
    load: bool,
}

enum Target {
    Null,
    Debug,
    Local(u8),
    Arg(u8),
    Named(String),
    Ref(Ref),
}

const BUILTIN_OSI: u8 = 1;

/// The Windows versions _OSI says yes to (Linux answers the same way).
const OSI: [&str; 20] = [
    "Windows 2000", "Windows 2001", "Windows 2001 SP1", "Windows 2001.1", "Windows 2001 SP2", "Windows 2001.1 SP1", "Windows 2006", "Windows 2006.1", "Windows 2006 SP1", "Windows 2006 SP2",
    "Windows 2009", "Windows 2012", "Windows 2013", "Windows 2015", "Windows 2016", "Windows 2017", "Windows 2017.2", "Windows 2018", "Windows 2018.2", "Windows 2019",
];
const OSI_MORE: [&str; 6] = ["Windows 2020", "Windows 2021", "Windows 2022", "Module Device", "Processor Device", "3.0 _SCP Extensions"];

/// Ops before an evaluation gives up (a While polling hardware that never answers).
const BUDGET: u64 = 4_000_000;
const MAX_DEPTH: usize = 48;

pub struct Aml<H: Host> {
    pub ns: BTreeMap<String, Obj>,
    pub host: H,
    codes: Vec<Rc<[u8]>>,
    frames: Vec<Frame>,
    /// integers are 32 bits in tables of revision 1
    ones: u64,
    budget: u64,
    /// terms that failed while loading (skipped)
    pub errors: u32,
    pub debug: Vec<String>,
}

pub fn join(scope: &str, seg: &str) -> String {
    if scope == "\\" {
        format!("\\{}", seg)
    } else {
        format!("{}.{}", scope, seg)
    }
}

pub fn parent(path: &str) -> String {
    match path.rfind('.') {
        Some(i) => path[..i].to_string(),
        None => String::from("\\"),
    }
}

/// The last name in a path ("\\_SB_.PCI0" → "PCI0").
pub fn leaf(path: &str) -> &str {
    let s = path.rsplit('.').next().unwrap_or(path);
    s.trim_start_matches('\\')
}

/// A path as written in strings ("\\_SB.PCI0.I2C1") in namespace form, each
/// name padded to four characters ("\\_SB_.PCI0.I2C1").
pub fn normalize(s: &str) -> String {
    let (pre, rest) = match s.strip_prefix('\\') {
        Some(r) => ("\\", r),
        None => ("", s),
    };
    let mut out = String::from(pre);
    let carets = rest.chars().take_while(|c| *c == '^').count();
    out.extend(core::iter::repeat('^').take(carets));
    let segs: Vec<String> = rest[carets..].split('.').filter(|x| !x.is_empty()).map(|x| {
        let mut t: String = x.chars().take(4).collect();
        while t.len() < 4 {
            t.push('_');
        }
        t
    }).collect();
    out.push_str(&segs.join("."));
    out
}

/// A compressed EISA id ("PNP0C50" from 0x500CD041).
pub fn eisa_id(v: u64) -> String {
    let id = (v as u32).swap_bytes();
    let c = |s: u32| (((id >> s) & 0x1F) as u8 + 0x40) as char;
    format!("{}{}{}{:04X}", c(26), c(21), c(16), id & 0xFFFF)
}

/// A UUID as ToUUID lays it out in a buffer.
pub fn uuid(s: &str) -> Vec<u8> {
    let hex: Vec<u8> = s.bytes().filter(|b| *b != b'-').collect();
    let byte = |i: usize| -> u8 {
        let h = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
        h(hex[2 * i]) << 4 | h(hex[2 * i + 1])
    };
    if hex.len() != 32 {
        return vec![0; 16];
    }
    let order = [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15];
    order.iter().map(|&i| byte(i)).collect()
}

impl<H: Host> Aml<H> {
    pub fn new(host: H) -> Aml<H> {
        let mut ns = BTreeMap::new();
        ns.insert(String::from("\\"), Obj::Scope);
        for s in ["_SB_", "_GPE", "_PR_", "_TZ_", "_SI_"] {
            ns.insert(join("\\", s), Obj::Scope);
        }
        ns.insert(String::from("\\_OSI"), Obj::Builtin(BUILTIN_OSI));
        ns.insert(String::from("\\_OS_"), Obj::Name(Value::Str(String::from("Microsoft Windows NT"))));
        ns.insert(String::from("\\_REV"), Obj::Name(Value::Int(2)));
        ns.insert(String::from("\\_GL_"), Obj::Mutex);
        Aml { ns, host, codes: Vec::new(), frames: vec![Frame { locals: vec![Value::None; 8], args: vec![], temps: vec![] }], ones: u64::MAX, budget: 0, errors: 0, debug: Vec::new() }
    }

    /// Load a DSDT or SSDT (with its 36-byte header).
    pub fn load(&mut self, table: &[u8]) -> Result<(), Error> {
        if table.len() < 36 {
            return Err(Error::Parse(0));
        }
        let len = (u32::from_le_bytes([table[4], table[5], table[6], table[7]]) as usize).min(table.len());
        if &table[0..4] == b"DSDT" && table[8] < 2 {
            self.ones = 0xFFFF_FFFF;
        }
        let code: Rc<[u8]> = Rc::from(&table[..len]);
        self.codes.push(code.clone());
        let mut c = Ctx { code, ci: self.codes.len() - 1, p: 36, frame: 0, scope: String::from("\\"), load: true };
        self.budget = BUDGET;
        self.load_list(&mut c, len);
        Ok(())
    }

    /// Run a term list while loading: a term that fails skips the rest of its
    /// list (where the next term starts can't be known).
    fn load_list(&mut self, c: &mut Ctx, end: usize) {
        while c.p < end {
            match self.op(c) {
                Ok(_) => {}
                Err(Stop::Err(e)) => {
                    self.errors += 1;
                    if self.debug.len() < 32 {
                        self.debug.push(format!("load: {:?} in {}", e, c.scope));
                    }
                    c.p = end;
                }
                Err(_) => c.p = end,
            }
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.ns.contains_key(path)
    }

    /// The objects directly inside `path`.
    #[allow(dead_code)]
    pub fn children(&self, path: &str) -> Vec<String> {
        let pre = if path == "\\" { String::from("\\") } else { format!("{}.", path) };
        self.ns.range(pre.clone()..).take_while(|(k, _)| k.starts_with(&pre)).filter(|(k, _)| k.len() > pre.len() && !k[pre.len()..].contains('.')).map(|(k, _)| k.clone()).collect()
    }

    pub fn devices(&self) -> Vec<String> {
        self.ns.iter().filter(|(_, o)| matches!(o, Obj::Device)).map(|(k, _)| k.clone()).collect()
    }

    /// Evaluate an object by absolute path: run a method, read a name or field.
    pub fn eval(&mut self, path: &str, args: Vec<Value>) -> Result<Value, Error> {
        self.budget = BUDGET;
        let obj = self.ns.get(path).cloned().ok_or_else(|| Error::NotFound(path.to_string()))?;
        let r = match obj {
            Obj::Method { .. } | Obj::Builtin(_) => self.invoke(path, args),
            _ => self.read_named(path),
        };
        match r {
            Ok(v) => Ok(v),
            Err(Stop::Err(e)) => Err(e),
            Err(Stop::Ret(v)) => Ok(v),
            Err(_) => Err(Error::Type),
        }
    }

    /// Evaluate `name` in device `dev` if it exists.
    pub fn child(&mut self, dev: &str, name: &str) -> Option<Result<Value, Error>> {
        let p = join(dev, name);
        if !self.exists(&p) {
            return None;
        }
        Some(self.eval(&p, vec![]).and_then(|v| self.deref_value(v)))
    }

    fn deref_value(&mut self, v: Value) -> Result<Value, Error> {
        match v {
            Value::Ref(Ref::Named(p)) if matches!(self.ns.get(&p), Some(Obj::Name(_)) | Some(Obj::Field(_)) | Some(Obj::BufField { .. })) => self.read_named(&p).map_err(stop_err),
            v => Ok(v),
        }
    }

    pub fn int(&mut self, dev: &str, name: &str) -> Option<u64> {
        match self.child(dev, name)? {
            Ok(v) => self.to_int(&v).ok(),
            Err(_) => None,
        }
    }

    /// _STA: present (bit 0), enabled (1), shown (2), working (3). 0x0F when
    /// a device has no _STA.
    pub fn sta(&mut self, dev: &str) -> u64 {
        self.int(dev, "_STA").unwrap_or(0x0F)
    }

    /// Run _INI on present devices, as every OS does before using them (it's
    /// where firmware reads _OSI and turns devices on).
    pub fn init(&mut self) {
        if self.exists("\\_SB_._INI") {
            let _ = self.eval("\\_SB_._INI", vec![]);
        }
        let devs = self.devices();
        let mut absent: Vec<String> = Vec::new();
        for d in devs {
            if absent.iter().any(|a| d.starts_with(a.as_str()) && d.as_bytes().get(a.len()) == Some(&b'.')) {
                continue;
            }
            let sta = self.sta(&d);
            if sta & 1 == 0 {
                if sta & 8 == 0 {
                    absent.push(d);
                }
                continue;
            }
            let ini = join(&d, "_INI");
            if matches!(self.ns.get(&ini), Some(Obj::Method { .. })) {
                let _ = self.eval(&ini, vec![]);
            }
        }
    }

    // ---- parsing -----------------------------------------------------------------

    fn byte(&self, c: &mut Ctx) -> Result<u8, Error> {
        let b = *c.code.get(c.p).ok_or(Error::Parse(c.p))?;
        c.p += 1;
        Ok(b)
    }

    fn peek(&self, c: &Ctx) -> Option<u8> {
        c.code.get(c.p).copied()
    }

    fn le(&self, c: &mut Ctx, n: usize) -> Result<u64, Error> {
        let mut v = 0u64;
        for i in 0..n {
            v |= (self.byte(c)? as u64) << (8 * i);
        }
        Ok(v)
    }

    /// A PkgLength: where the package ends.
    fn pkg_len(&self, c: &mut Ctx) -> Result<usize, Error> {
        let start = c.p;
        let b0 = self.byte(c)?;
        let n = b0 >> 6;
        let len = if n == 0 {
            (b0 & 0x3F) as usize
        } else {
            let mut l = (b0 & 0x0F) as usize;
            for i in 0..n {
                l |= (self.byte(c)? as usize) << (4 + 8 * i as usize);
            }
            l
        };
        let end = start + len;
        if end > c.code.len() || end < c.p {
            return Err(Error::Parse(start));
        }
        Ok(end)
    }

    fn seg(&self, c: &mut Ctx) -> Result<String, Error> {
        let mut s = String::new();
        for _ in 0..4 {
            let b = self.byte(c)?;
            if !(b == b'_' || b.is_ascii_uppercase() || b.is_ascii_digit()) {
                return Err(Error::Parse(c.p - 1));
            }
            s.push(b as char);
        }
        Ok(s)
    }

    /// A NameString, as written: "\\_SB_.PCI0", "^^FOO_", "ABCD", "" (null).
    fn name(&self, c: &mut Ctx) -> Result<String, Error> {
        let mut out = String::new();
        match self.peek(c) {
            Some(b'\\') => {
                c.p += 1;
                out.push('\\');
            }
            Some(b'^') => {
                while self.peek(c) == Some(b'^') {
                    c.p += 1;
                    out.push('^');
                }
            }
            _ => {}
        }
        let segs = match self.peek(c) {
            Some(0x00) => {
                c.p += 1;
                0
            }
            Some(0x2E) => {
                c.p += 1;
                2
            }
            Some(0x2F) => {
                c.p += 1;
                self.byte(c)? as usize
            }
            _ => 1,
        };
        for i in 0..segs {
            if i > 0 {
                out.push('.');
            }
            let s = self.seg(c)?;
            out.push_str(&s);
        }
        Ok(out)
    }

    fn is_name(b: u8) -> bool {
        b == b'\\' || b == b'^' || b == b'_' || b.is_ascii_uppercase() || b == 0x2E || b == 0x2F
    }

    /// Where a name written in `scope` points (for creating objects).
    pub fn abs(scope: &str, raw: &str) -> String {
        if let Some(r) = raw.strip_prefix('\\') {
            return if r.is_empty() { String::from("\\") } else { format!("\\{}", r) };
        }
        let mut s = scope.to_string();
        let mut rest = raw;
        while let Some(r) = rest.strip_prefix('^') {
            s = parent(&s);
            rest = r;
        }
        if rest.is_empty() {
            return s;
        }
        join(&s, rest)
    }

    /// Find the object a name refers to from `scope` (with the search rules
    /// for a single name).
    pub fn resolve(&self, scope: &str, raw: &str) -> Option<String> {
        let direct = raw.starts_with('\\') || raw.starts_with('^') || raw.contains('.');
        let found = if direct {
            let p = Self::abs(scope, raw);
            self.ns.contains_key(&p).then_some(p)
        } else {
            let mut s = scope.to_string();
            loop {
                let p = join(&s, raw);
                if self.ns.contains_key(&p) {
                    break Some(p);
                }
                if s == "\\" {
                    break None;
                }
                s = parent(&s);
            }
        };
        let mut p = found?;
        for _ in 0..8 {
            match self.ns.get(&p) {
                Some(Obj::Alias(t)) => p = t.clone(),
                _ => break,
            }
        }
        Some(p)
    }

    fn create(&mut self, c: &Ctx, path: String, o: Obj) {
        if !c.load && !self.ns.contains_key(&path) {
            self.frames[c.frame].temps.push(path.clone());
        }
        self.ns.insert(path, o);
    }

    // ---- evaluation ----------------------------------------------------------------

    fn tick(&mut self) -> Result<(), Error> {
        if self.budget == 0 {
            return Err(Error::Budget);
        }
        self.budget -= 1;
        Ok(())
    }

    fn term_list(&mut self, c: &mut Ctx, end: usize) -> R<()> {
        while c.p < end {
            self.op(c)?;
        }
        Ok(())
    }

    fn arg_int(&mut self, c: &mut Ctx) -> R<u64> {
        let v = self.op(c)?;
        Ok(self.to_int(&v)?)
    }

    /// Evaluate one term.
    fn op(&mut self, c: &mut Ctx) -> R<Value> {
        self.tick()?;
        let at = c.p;
        let b = self.byte(c)?;
        let ones = self.ones;
        Ok(match b {
            0x00 => Value::Int(0),
            0x01 => Value::Int(1),
            0xFF => Value::Int(ones),
            0x0A => Value::Int(self.le(c, 1)?),
            0x0B => Value::Int(self.le(c, 2)?),
            0x0C => Value::Int(self.le(c, 4)?),
            0x0E => Value::Int(self.le(c, 8)? & ones),
            0x0D => {
                let mut s = String::new();
                loop {
                    let ch = self.byte(c)?;
                    if ch == 0 {
                        break;
                    }
                    s.push(ch as char);
                }
                Value::Str(s)
            }
            0x11 => {
                let end = self.pkg_len(c)?;
                let size = self.arg_int(c)? as usize;
                if c.p > end || size > 1 << 20 {
                    return Err(Error::Parse(at).into());
                }
                let mut v = c.code[c.p..end].to_vec();
                if v.len() < size {
                    v.resize(size, 0);
                }
                c.p = end;
                Value::Buf(v)
            }
            0x12 | 0x13 => {
                let end = self.pkg_len(c)?;
                let n = if b == 0x12 { self.byte(c)? as usize } else { self.arg_int(c)? as usize };
                let mut v = Vec::new();
                while c.p < end {
                    v.push(self.pkg_elem(c)?);
                }
                if v.len() < n && n < 4096 {
                    v.resize(n, Value::None);
                }
                Value::Pkg(v)
            }
            0x06 => {
                let src = self.name(c)?;
                let dst = self.name(c)?;
                let target = self.resolve(&c.scope, &src).unwrap_or_else(|| Self::abs(&c.scope, &src));
                let p = Self::abs(&c.scope, &dst);
                self.create(c, p, Obj::Alias(target));
                Value::None
            }
            0x08 => {
                let n = self.name(c)?;
                let v = self.op(c)?;
                let p = Self::abs(&c.scope, &n);
                self.create(c, p, Obj::Name(v));
                Value::None
            }
            0x10 => {
                let end = self.pkg_len(c)?;
                let n = self.name(c)?;
                let p = self.resolve(&c.scope, &n).unwrap_or_else(|| Self::abs(&c.scope, &n));
                if !self.ns.contains_key(&p) {
                    self.ns.insert(p.clone(), Obj::Scope);
                }
                self.scoped(c, p, end)?;
                Value::None
            }
            0x14 => {
                let end = self.pkg_len(c)?;
                let n = self.name(c)?;
                let flags = self.byte(c)?;
                let p = Self::abs(&c.scope, &n);
                let (ci, start) = (c.ci, c.p);
                self.create(c, p, Obj::Method { code: ci, start, end, args: flags & 7 });
                c.p = end;
                Value::None
            }
            0x15 => {
                let n = self.name(c)?;
                let kind = self.byte(c)?;
                let args = self.byte(c)?;
                let p = Self::abs(&c.scope, &n);
                if !self.ns.contains_key(&p) && kind == 8 {
                    self.ns.insert(p, Obj::External { args: args & 7 });
                }
                Value::None
            }
            0x5B => return self.ext_op(c, at),
            0x60..=0x67 => self.frames[c.frame].locals[(b - 0x60) as usize].clone(),
            0x68..=0x6E => self.frames[c.frame].args.get((b - 0x68) as usize).cloned().unwrap_or(Value::None),
            0x70 => {
                let v = self.op(c)?;
                let v = self.plain(v)?;
                let t = self.target(c)?;
                self.store(c, &t, v.clone())?;
                v
            }
            0x71 => {
                let t = self.supername(c)?;
                Value::Ref(self.target_ref(c, t)?)
            }
            0x72 | 0x74 | 0x77 | 0x79 | 0x7A | 0x7B | 0x7C | 0x7D | 0x7E | 0x7F | 0x85 => {
                let a = self.arg_int(c)?;
                let bb = self.arg_int(c)?;
                let t = self.target(c)?;
                let r = match b {
                    0x72 => a.wrapping_add(bb),
                    0x74 => a.wrapping_sub(bb),
                    0x77 => a.wrapping_mul(bb),
                    0x79 => {
                        if bb >= 64 {
                            0
                        } else {
                            a << bb
                        }
                    }
                    0x7A => {
                        if bb >= 64 {
                            0
                        } else {
                            a >> bb
                        }
                    }
                    0x7B => a & bb,
                    0x7C => !(a & bb),
                    0x7D => a | bb,
                    0x7E => !(a | bb),
                    0x7F => a ^ bb,
                    _ => {
                        if bb == 0 {
                            return Err(Error::Fatal.into());
                        }
                        a % bb
                    }
                } & ones;
                self.store(c, &t, Value::Int(r))?;
                Value::Int(r)
            }
            0x73 => {
                let a = self.op(c)?;
                let a = self.plain(a)?;
                let bb = self.op(c)?;
                let bb = self.plain(bb)?;
                let t = self.target(c)?;
                let r = self.concat(&a, &bb)?;
                self.store(c, &t, r.clone())?;
                r
            }
            0x75 | 0x76 => {
                let t = self.supername(c)?;
                let v = self.load_target(c, &t)?;
                let v = self.to_int(&v)?;
                let r = if b == 0x75 { v.wrapping_add(1) } else { v.wrapping_sub(1) } & ones;
                self.store(c, &t, Value::Int(r))?;
                Value::Int(r)
            }
            0x78 => {
                let a = self.arg_int(c)?;
                let d = self.arg_int(c)?;
                let rt = self.target(c)?;
                let qt = self.target(c)?;
                if d == 0 {
                    return Err(Error::Fatal.into());
                }
                self.store(c, &rt, Value::Int(a % d))?;
                self.store(c, &qt, Value::Int(a / d))?;
                Value::Int(a / d)
            }
            0x80 | 0x81 | 0x82 => {
                let a = self.arg_int(c)?;
                let t = self.target(c)?;
                let r = match b {
                    0x80 => !a & ones,
                    0x81 => {
                        if a == 0 {
                            0
                        } else {
                            64 - a.leading_zeros() as u64
                        }
                    }
                    _ => {
                        if a == 0 {
                            0
                        } else {
                            a.trailing_zeros() as u64 + 1
                        }
                    }
                };
                self.store(c, &t, Value::Int(r))?;
                Value::Int(r)
            }
            0x83 => {
                let v = self.op(c)?;
                self.deref(c, v)?
            }
            0x84 => {
                let a = self.op(c)?;
                let a = self.plain(a)?;
                let a = self.to_buf(&a)?;
                let bb = self.op(c)?;
                let bb = self.plain(bb)?;
                let bb = self.to_buf(&bb)?;
                let t = self.target(c)?;
                let mut r = strip_end(&a);
                r.extend_from_slice(&strip_end(&bb));
                r.extend_from_slice(&[0x79, 0x00]);
                self.store(c, &t, Value::Buf(r.clone()))?;
                Value::Buf(r)
            }
            0x86 => {
                let _ = self.supername(c)?;
                let _ = self.op(c)?;
                Value::None
            }
            0x87 => {
                let t = self.supername(c)?;
                let v = self.load_target(c, &t)?;
                let v = self.deref_plain(c, v)?;
                Value::Int(match v {
                    Value::Str(s) => s.len() as u64,
                    Value::Buf(b) => b.len() as u64,
                    Value::Pkg(p) => p.len() as u64,
                    _ => return Err(Error::Type.into()),
                })
            }
            0x88 => {
                let loc = self.loc(c)?;
                let i = self.arg_int(c)? as usize;
                let t = self.target(c)?;
                let r = Value::Ref(Ref::Elem(Box::new(loc), i));
                self.store(c, &t, r.clone())?;
                r
            }
            0x89 => {
                let pkg = self.op(c)?;
                let pkg = self.deref_plain(c, pkg)?;
                let op1 = self.byte(c)?;
                let v1 = self.arg_int(c)?;
                let op2 = self.byte(c)?;
                let v2 = self.arg_int(c)?;
                let start = self.arg_int(c)? as usize;
                let Value::Pkg(p) = pkg else { return Err(Error::Type.into()) };
                let test = |op: u8, e: u64, v: u64| match op {
                    0 => true,
                    1 => e == v,
                    2 => e <= v,
                    3 => e < v,
                    4 => e >= v,
                    _ => e > v,
                };
                let mut r = ones;
                for (i, e) in p.iter().enumerate().skip(start) {
                    if let Ok(e) = self.to_int(e) {
                        if test(op1, e, v1) && test(op2, e, v2) {
                            r = i as u64;
                            break;
                        }
                    }
                }
                Value::Int(r)
            }
            0x8A | 0x8B | 0x8C | 0x8D | 0x8F => {
                let loc = self.loc(c)?;
                let idx = self.arg_int(c)?;
                let n = self.name(c)?;
                let (bit, bits) = match b {
                    0x8A => (idx * 8, 32),
                    0x8B => (idx * 8, 16),
                    0x8C => (idx * 8, 8),
                    0x8D => (idx, 1),
                    _ => (idx * 8, 64),
                };
                let p = Self::abs(&c.scope, &n);
                self.create(c, p, Obj::BufField { loc, bit, bits });
                Value::None
            }
            0x8E => {
                let t = self.supername(c)?;
                let code = match &t {
                    Target::Named(p) => match self.ns.get(p) {
                        Some(Obj::Name(v)) => type_code(v),
                        Some(Obj::Field(_)) => 5,
                        Some(Obj::Device) => 6,
                        Some(Obj::Event) => 7,
                        Some(Obj::Method { .. }) | Some(Obj::Builtin(_)) => 8,
                        Some(Obj::Mutex) => 9,
                        Some(Obj::Region { .. }) => 10,
                        Some(Obj::PowerRes) => 11,
                        Some(Obj::Processor) => 12,
                        Some(Obj::Thermal) => 13,
                        Some(Obj::BufField { .. }) => 14,
                        _ => 0,
                    },
                    Target::Debug => 16,
                    _ => {
                        let v = self.load_target(c, &t)?;
                        type_code(&v)
                    }
                };
                Value::Int(code)
            }
            0x90 | 0x91 => {
                let a = self.arg_int(c)?;
                let bb = self.arg_int(c)?;
                let r = if b == 0x90 { a != 0 && bb != 0 } else { a != 0 || bb != 0 };
                self.boolean(r)
            }
            0x92 => {
                let a = self.arg_int(c)?;
                self.boolean(a == 0)
            }
            0x93 | 0x94 | 0x95 => {
                let a = self.op(c)?;
                let a = self.plain(a)?;
                let bb = self.op(c)?;
                let bb = self.plain(bb)?;
                let ord = self.compare(&a, &bb)?;
                let r = match b {
                    0x93 => ord == core::cmp::Ordering::Equal,
                    0x94 => ord == core::cmp::Ordering::Greater,
                    _ => ord == core::cmp::Ordering::Less,
                };
                self.boolean(r)
            }
            0x96 | 0x97 | 0x98 | 0x99 => {
                let v = self.op(c)?;
                let v = self.plain(v)?;
                let t = self.target(c)?;
                let r = match b {
                    0x96 => Value::Buf(self.to_buf(&v)?),
                    0x97 => Value::Str(match &v {
                        Value::Buf(bs) => bs.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","),
                        Value::Str(s) => s.clone(),
                        _ => self.to_int(&v)?.to_string(),
                    }),
                    0x98 => Value::Str(match &v {
                        Value::Buf(bs) => bs.iter().map(|x| format!("0x{:02X}", x)).collect::<Vec<_>>().join(","),
                        Value::Str(s) => s.clone(),
                        _ => format!("0x{:X}", self.to_int(&v)?),
                    }),
                    _ => Value::Int(match &v {
                        Value::Str(s) => parse_int(s) & ones,
                        _ => self.to_int(&v)?,
                    }),
                };
                self.store(c, &t, r.clone())?;
                r
            }
            0x9C => {
                let v = self.op(c)?;
                let v = self.plain(v)?;
                let v = self.to_buf(&v)?;
                let n = self.arg_int(c)?;
                let t = self.target(c)?;
                let s: String = v.iter().take(n.min(v.len() as u64) as usize).take_while(|b| **b != 0).map(|b| *b as char).collect();
                self.store(c, &t, Value::Str(s.clone()))?;
                Value::Str(s)
            }
            0x9D => {
                let v = self.op(c)?;
                let t = self.supername(c)?;
                match &t {
                    Target::Named(p) if matches!(self.ns.get(p), Some(Obj::Name(_))) => {
                        self.ns.insert(p.clone(), Obj::Name(v.clone()));
                    }
                    _ => self.store(c, &t, v.clone())?,
                }
                v
            }
            0x9E => {
                let v = self.op(c)?;
                let v = self.plain(v)?;
                let i = self.arg_int(c)? as usize;
                let n = self.arg_int(c)? as usize;
                let t = self.target(c)?;
                let r = match v {
                    Value::Str(s) => Value::Str(s.chars().skip(i).take(n).collect()),
                    other => {
                        let bs = self.to_buf(&other)?;
                        Value::Buf(bs.iter().skip(i).take(n).copied().collect())
                    }
                };
                self.store(c, &t, r.clone())?;
                r
            }
            0x9F => return Err(Stop::Continue),
            0xA5 => return Err(Stop::Break),
            0xA0 => {
                let end = self.pkg_len(c)?;
                let pred = self.arg_int(c)?;
                if pred != 0 {
                    self.term_list(c, end)?;
                }
                c.p = end;
                if self.peek(c) == Some(0xA1) {
                    c.p += 1;
                    let eend = self.pkg_len(c)?;
                    if pred == 0 {
                        self.term_list(c, eend)?;
                    }
                    c.p = eend;
                }
                Value::None
            }
            0xA1 => {
                let end = self.pkg_len(c)?;
                c.p = end;
                Value::None
            }
            0xA2 => {
                let end = self.pkg_len(c)?;
                let start = c.p;
                loop {
                    c.p = start;
                    if self.arg_int(c)? == 0 {
                        break;
                    }
                    match self.term_list(c, end) {
                        Ok(()) | Err(Stop::Continue) => {}
                        Err(Stop::Break) => break,
                        Err(e) => return Err(e),
                    }
                }
                c.p = end;
                Value::None
            }
            0xA3 | 0xCC => Value::None,
            0xA4 => {
                let v = self.op(c)?;
                return Err(Stop::Ret(v));
            }
            b if Self::is_name(b) => {
                c.p -= 1;
                let n = self.name(c)?;
                self.name_term(c, &n)?
            }
            _ => return Err(Error::Unsupported(b as u16).into()),
        })
    }

    fn ext_op(&mut self, c: &mut Ctx, at: usize) -> R<Value> {
        let b = self.byte(c)?;
        Ok(match b {
            0x01 => {
                let n = self.name(c)?;
                self.byte(c)?;
                let p = Self::abs(&c.scope, &n);
                self.create(c, p, Obj::Mutex);
                Value::None
            }
            0x02 => {
                let n = self.name(c)?;
                let p = Self::abs(&c.scope, &n);
                self.create(c, p, Obj::Event);
                Value::None
            }
            0x12 => {
                // CondRefOf: the name may not exist
                let save = c.p;
                let found = match self.peek(c) {
                    Some(x) if Self::is_name(x) => {
                        let n = self.name(c)?;
                        self.resolve(&c.scope, &n).map(|p| Ref::Named(p))
                    }
                    _ => {
                        c.p = save;
                        let t = self.supername(c)?;
                        Some(self.target_ref(c, t)?)
                    }
                };
                let t = self.target(c)?;
                match found {
                    Some(r) => {
                        self.store(c, &t, Value::Ref(r))?;
                        Value::Int(self.ones)
                    }
                    None => Value::Int(0),
                }
            }
            0x13 => {
                let loc = self.loc(c)?;
                let bit = self.arg_int(c)?;
                let bits = self.arg_int(c)?;
                let n = self.name(c)?;
                let p = Self::abs(&c.scope, &n);
                self.create(c, p, Obj::BufField { loc, bit, bits });
                Value::None
            }
            0x1F => {
                for _ in 0..6 {
                    self.op(c)?;
                }
                Value::Int(0)
            }
            0x20 => {
                self.name(c)?;
                self.target(c)?;
                Value::Int(0)
            }
            0x21 => {
                let us = self.arg_int(c)?;
                self.host.sleep_ms(us / 1000);
                Value::None
            }
            0x22 => {
                let ms = self.arg_int(c)?;
                self.host.sleep_ms(ms.min(1000));
                Value::None
            }
            0x23 => {
                self.supername(c)?;
                self.le(c, 2)?;
                Value::Int(0)
            }
            0x24 | 0x26 | 0x27 => {
                self.supername(c)?;
                Value::None
            }
            0x25 => {
                self.supername(c)?;
                self.op(c)?;
                Value::Int(0)
            }
            0x28 | 0x29 => {
                let v = self.arg_int(c)?;
                let t = self.target(c)?;
                let r = if b == 0x28 { from_bcd(v) } else { to_bcd(v) };
                self.store(c, &t, Value::Int(r))?;
                Value::Int(r)
            }
            0x30 => Value::Int(0x2023_0628),
            0x31 => Value::None,
            0x32 => {
                self.byte(c)?;
                self.le(c, 4)?;
                self.op(c)?;
                return Err(Error::Fatal.into());
            }
            0x33 => Value::Int(self.host.timer()),
            0x80 => {
                let n = self.name(c)?;
                let space = self.byte(c)?;
                let off = self.arg_int(c)?;
                let len = self.arg_int(c)?;
                let p = Self::abs(&c.scope, &n);
                let base = if space == PCI { self.pci_address(&c.scope) | (off & 0xFFFF) } else { off };
                self.create(c, p, Obj::Region { space, base, len });
                Value::None
            }
            0x81 | 0x86 | 0x87 => {
                let end = self.pkg_len(c)?;
                let n1 = self.name(c)?;
                let kind = match b {
                    0x81 => FieldKind::Region(self.resolve(&c.scope, &n1).unwrap_or_else(|| Self::abs(&c.scope, &n1))),
                    0x86 => {
                        let n2 = self.name(c)?;
                        FieldKind::Index { index: self.resolve(&c.scope, &n1).unwrap_or_else(|| Self::abs(&c.scope, &n1)), data: self.resolve(&c.scope, &n2).unwrap_or_else(|| Self::abs(&c.scope, &n2)) }
                    }
                    _ => {
                        let n2 = self.name(c)?;
                        let value = self.arg_int(c)?;
                        FieldKind::Bank { region: self.resolve(&c.scope, &n1).unwrap_or_else(|| Self::abs(&c.scope, &n1)), bank: self.resolve(&c.scope, &n2).unwrap_or_else(|| Self::abs(&c.scope, &n2)), value }
                    }
                };
                let flags = self.byte(c)?;
                self.field_list(c, end, kind, flags)?;
                c.p = end;
                Value::None
            }
            0x82 | 0x83 | 0x84 | 0x85 => {
                let end = self.pkg_len(c)?;
                let n = self.name(c)?;
                let o = match b {
                    0x82 => Obj::Device,
                    0x83 => {
                        self.byte(c)?;
                        self.le(c, 4)?;
                        self.byte(c)?;
                        Obj::Processor
                    }
                    0x84 => {
                        self.byte(c)?;
                        self.le(c, 2)?;
                        Obj::PowerRes
                    }
                    _ => Obj::Thermal,
                };
                let p = Self::abs(&c.scope, &n);
                self.create(c, p.clone(), o);
                self.scoped(c, p, end)?;
                Value::None
            }
            0x88 => {
                let n = self.name(c)?;
                self.op(c)?;
                self.op(c)?;
                self.op(c)?;
                let p = Self::abs(&c.scope, &n);
                self.create(c, p, Obj::Region { space: MEM, base: 0, len: 0 });
                Value::None
            }
            _ => return Err(Error::Parse(at).into()),
        })
    }

    /// Run a term list inside another scope (Scope, Device…).
    fn scoped(&mut self, c: &mut Ctx, scope: String, end: usize) -> R<()> {
        let old = core::mem::replace(&mut c.scope, scope);
        let r = if c.load {
            self.load_list(c, end);
            Ok(())
        } else {
            self.term_list(c, end)
        };
        c.scope = old;
        c.p = end;
        r
    }

    fn field_list(&mut self, c: &mut Ctx, end: usize, kind: FieldKind, mut flags: u8) -> R<()> {
        let mut bit = 0u64;
        while c.p < end {
            match self.peek(c) {
                Some(0x00) => {
                    c.p += 1;
                    bit += self.pkg_len_value(c)?;
                }
                Some(0x01) => {
                    c.p += 1;
                    let t = self.byte(c)?;
                    self.byte(c)?;
                    flags = flags & 0xF0 | t & 0x0F;
                }
                Some(0x02) => {
                    // a connection (GPIO, serial bus): the fields after it aren't memory
                    c.p += 1;
                    if self.peek(c) == Some(0x11) {
                        self.op(c)?;
                    } else {
                        self.name(c)?;
                    }
                }
                Some(0x03) => {
                    c.p += 1;
                    let t = self.byte(c)?;
                    self.byte(c)?;
                    self.byte(c)?;
                    flags = flags & 0xF0 | t & 0x0F;
                }
                _ => {
                    let seg = self.seg(c)?;
                    let bits = self.pkg_len_value(c)?;
                    let p = join(&c.scope, &seg);
                    self.create(c, p, Obj::Field(Field { kind: kind.clone(), bit, bits, flags }));
                    bit += bits;
                }
            }
        }
        Ok(())
    }

    /// A PkgLength used as a number (field widths).
    fn pkg_len_value(&self, c: &mut Ctx) -> Result<u64, Error> {
        let b0 = self.byte(c)?;
        let n = b0 >> 6;
        if n == 0 {
            return Ok((b0 & 0x3F) as u64);
        }
        let mut l = (b0 & 0x0F) as u64;
        for i in 0..n {
            l |= (self.byte(c)? as u64) << (4 + 8 * i as u64);
        }
        Ok(l)
    }

    fn pkg_elem(&mut self, c: &mut Ctx) -> R<Value> {
        match self.peek(c) {
            Some(b) if Self::is_name(b) => {
                let n = self.name(c)?;
                Ok(Value::Ref(Ref::Named(self.resolve(&c.scope, &n).unwrap_or_else(|| Self::abs(&c.scope, &n)))))
            }
            _ => self.op(c),
        }
    }

    /// A name in term position: invoke a method, read an object.
    fn name_term(&mut self, c: &mut Ctx, n: &str) -> R<Value> {
        let Some(p) = self.resolve(&c.scope, n) else {
            if c.load {
                return Ok(Value::Ref(Ref::Named(Self::abs(&c.scope, n))));
            }
            return Err(Error::NotFound(n.to_string()).into());
        };
        match self.ns.get(&p).cloned() {
            Some(Obj::Method { args, .. }) => {
                let mut a = Vec::new();
                for _ in 0..args {
                    let v = self.op(c)?;
                    a.push(v);
                }
                self.invoke(&p, a)
            }
            Some(Obj::Builtin(_)) => {
                let v = self.op(c)?;
                self.invoke(&p, vec![v])
            }
            Some(Obj::External { args }) => {
                for _ in 0..args {
                    self.op(c)?;
                }
                Err(Error::NotFound(p).into())
            }
            Some(Obj::Name(_)) | Some(Obj::Field(_)) | Some(Obj::BufField { .. }) => self.read_named(&p),
            _ => Ok(Value::Ref(Ref::Named(p))),
        }
    }

    fn invoke(&mut self, path: &str, args: Vec<Value>) -> R<Value> {
        match self.ns.get(path).cloned() {
            Some(Obj::Builtin(BUILTIN_OSI)) => {
                let s = match args.first() {
                    Some(Value::Str(s)) => s.clone(),
                    _ => String::new(),
                };
                let yes = OSI.contains(&s.as_str()) || OSI_MORE.contains(&s.as_str());
                Ok(self.boolean(yes))
            }
            Some(Obj::Method { code, start, end, .. }) => {
                if self.frames.len() >= MAX_DEPTH {
                    return Err(Error::Depth.into());
                }
                // arguments are passed by value
                let mut plain = Vec::new();
                for a in args {
                    plain.push(match a {
                        Value::Ref(Ref::Named(ref p)) if matches!(self.ns.get(p), Some(Obj::Name(_)) | Some(Obj::Field(_)) | Some(Obj::BufField { .. })) => self.read_named(p)?,
                        v => v,
                    });
                }
                self.frames.push(Frame { locals: vec![Value::None; 8], args: plain, temps: vec![] });
                let fi = self.frames.len() - 1;
                let mut c = Ctx { code: self.codes[code].clone(), ci: code, p: start, frame: fi, scope: path.to_string(), load: false };
                let r = self.term_list(&mut c, end);
                let f = self.frames.pop().unwrap();
                for t in f.temps {
                    self.ns.remove(&t);
                }
                match r {
                    Ok(()) => Ok(Value::Int(0)),
                    Err(Stop::Ret(v)) => {
                        // a reference to a local dies with the method
                        Ok(match v {
                            Value::Ref(Ref::Elem(l, i)) if matches!(*l, Loc::Local(..) | Loc::Arg(..)) => {
                                let _ = (l, i);
                                Value::None
                            }
                            v => v,
                        })
                    }
                    Err(Stop::Break) | Err(Stop::Continue) => Ok(Value::Int(0)),
                    Err(e) => Err(e),
                }
            }
            _ => Err(Error::NotFound(path.to_string()).into()),
        }
    }

    fn boolean(&self, b: bool) -> Value {
        Value::Int(if b { self.ones } else { 0 })
    }

    // ---- targets and references ---------------------------------------------------

    fn target(&mut self, c: &mut Ctx) -> R<Target> {
        if self.peek(c) == Some(0x00) {
            c.p += 1;
            return Ok(Target::Null);
        }
        self.supername(c)
    }

    fn supername(&mut self, c: &mut Ctx) -> R<Target> {
        let b = self.peek(c).ok_or(Error::Parse(c.p))?;
        Ok(match b {
            0x60..=0x67 => {
                c.p += 1;
                Target::Local(b - 0x60)
            }
            0x68..=0x6E => {
                c.p += 1;
                Target::Arg(b - 0x68)
            }
            0x5B if c.code.get(c.p + 1) == Some(&0x31) => {
                c.p += 2;
                Target::Debug
            }
            b if Self::is_name(b) => {
                let n = self.name(c)?;
                match self.resolve(&c.scope, &n) {
                    Some(p) => Target::Named(p),
                    None if c.load => Target::Named(Self::abs(&c.scope, &n)),
                    None => return Err(Error::NotFound(n).into()),
                }
            }
            _ => match self.op(c)? {
                Value::Ref(r) => Target::Ref(r),
                _ => return Err(Error::Type.into()),
            },
        })
    }

    fn target_ref(&mut self, c: &Ctx, t: Target) -> R<Ref> {
        Ok(match t {
            Target::Named(p) => Ref::Named(p),
            Target::Ref(r) => r,
            Target::Local(i) => match &self.frames[c.frame].locals[i as usize] {
                Value::Ref(r) => r.clone(),
                _ => Ref::Elem(Box::new(Loc::Local(c.frame, i)), usize::MAX),
            },
            Target::Arg(i) => match self.frames[c.frame].args.get(i as usize) {
                Some(Value::Ref(r)) => r.clone(),
                _ => Ref::Elem(Box::new(Loc::Arg(c.frame, i)), usize::MAX),
            },
            _ => return Err(Error::Type.into()),
        })
    }

    /// The object Index or CreateField works on: in place when it's named,
    /// a local or an argument; a copy otherwise.
    fn loc(&mut self, c: &mut Ctx) -> R<Loc> {
        let b = self.peek(c).ok_or(Error::Parse(c.p))?;
        Ok(match b {
            0x60..=0x67 => {
                c.p += 1;
                match self.frames[c.frame].locals[(b - 0x60) as usize].clone() {
                    Value::Ref(Ref::Named(p)) => Loc::Named(p),
                    _ => Loc::Local(c.frame, b - 0x60),
                }
            }
            0x68..=0x6E => {
                c.p += 1;
                match self.frames[c.frame].args.get((b - 0x68) as usize).cloned() {
                    Some(Value::Ref(Ref::Named(p))) => Loc::Named(p),
                    _ => Loc::Arg(c.frame, b - 0x68),
                }
            }
            b if Self::is_name(b) => {
                let n = self.name(c)?;
                let p = self.resolve(&c.scope, &n).ok_or_else(|| Error::NotFound(n.clone()))?;
                match self.ns.get(&p) {
                    Some(Obj::Name(_)) => Loc::Named(p),
                    Some(Obj::Method { .. }) => {
                        let v = self.name_term_path(c, &p)?;
                        Loc::Temp(Box::new(v))
                    }
                    _ => {
                        let v = self.read_named(&p)?;
                        Loc::Temp(Box::new(v))
                    }
                }
            }
            _ => {
                let v = self.op(c)?;
                match v {
                    Value::Ref(Ref::Named(p)) => Loc::Named(p),
                    Value::Ref(r) => {
                        let v = self.read_ref(&r)?;
                        Loc::Temp(Box::new(v))
                    }
                    v => Loc::Temp(Box::new(v)),
                }
            }
        })
    }

    fn name_term_path(&mut self, c: &mut Ctx, p: &str) -> R<Value> {
        let args = match self.ns.get(p) {
            Some(Obj::Method { args, .. }) => *args,
            _ => 0,
        };
        let mut a = Vec::new();
        for _ in 0..args {
            let v = self.op(c)?;
            a.push(v);
        }
        self.invoke(p, a)
    }

    fn load_target(&mut self, c: &Ctx, t: &Target) -> R<Value> {
        Ok(match t {
            Target::Local(i) => self.frames[c.frame].locals[*i as usize].clone(),
            Target::Arg(i) => self.frames[c.frame].args.get(*i as usize).cloned().unwrap_or(Value::None),
            Target::Named(p) => self.read_named(p)?,
            Target::Ref(r) => self.read_ref(r)?,
            _ => Value::None,
        })
    }

    /// A value with named references to data read (for arithmetic, stores).
    fn plain(&mut self, v: Value) -> R<Value> {
        match v {
            Value::Ref(Ref::Named(ref p)) if matches!(self.ns.get(p), Some(Obj::Name(_)) | Some(Obj::Field(_)) | Some(Obj::BufField { .. })) => self.read_named(p),
            v => Ok(v),
        }
    }

    fn deref(&mut self, c: &Ctx, v: Value) -> R<Value> {
        match v {
            Value::Ref(r) => self.read_ref(&r),
            Value::Str(s) => {
                let p = self.resolve(&c.scope, &s).ok_or(Error::NotFound(s))?;
                self.read_named(&p)
            }
            v => Ok(v),
        }
    }

    fn deref_plain(&mut self, c: &Ctx, v: Value) -> R<Value> {
        match v {
            Value::Ref(_) => self.deref(c, v),
            v => Ok(v),
        }
    }

    fn read_ref(&mut self, r: &Ref) -> R<Value> {
        match r {
            Ref::Named(p) => self.read_named(p),
            Ref::Elem(loc, i) => {
                let whole = self.read_loc(loc)?;
                if *i == usize::MAX {
                    return Ok(whole);
                }
                Ok(match whole {
                    Value::Pkg(p) => match p.get(*i).cloned().ok_or(Error::Type)? {
                        Value::Ref(Ref::Named(n)) if matches!(self.ns.get(&n), Some(Obj::Name(_))) => self.read_named(&n)?,
                        v => v,
                    },
                    Value::Buf(b) => Value::Int(*b.get(*i).ok_or(Error::Type)? as u64),
                    Value::Str(s) => Value::Int(*s.as_bytes().get(*i).ok_or(Error::Type)? as u64),
                    _ => return Err(Error::Type.into()),
                })
            }
        }
    }

    fn read_loc(&mut self, l: &Loc) -> R<Value> {
        Ok(match l {
            Loc::Named(p) => self.read_named(p)?,
            Loc::Local(f, i) => self.frames.get(*f).map(|fr| fr.locals[*i as usize].clone()).unwrap_or(Value::None),
            Loc::Arg(f, i) => self.frames.get(*f).and_then(|fr| fr.args.get(*i as usize).cloned()).unwrap_or(Value::None),
            Loc::Temp(v) => (**v).clone(),
        })
    }

    fn loc_mut(&mut self, l: &Loc) -> Option<&mut Value> {
        match l {
            Loc::Named(p) => match self.ns.get_mut(p) {
                Some(Obj::Name(v)) => Some(v),
                _ => None,
            },
            Loc::Local(f, i) => self.frames.get_mut(*f).map(|fr| &mut fr.locals[*i as usize]),
            Loc::Arg(f, i) => self.frames.get_mut(*f).and_then(|fr| fr.args.get_mut(*i as usize)),
            Loc::Temp(_) => None,
        }
    }

    fn read_named(&mut self, p: &str) -> R<Value> {
        match self.ns.get(p).cloned() {
            Some(Obj::Name(v)) => Ok(v),
            Some(Obj::Field(f)) => self.read_field(&f),
            Some(Obj::BufField { loc, bit, bits }) => {
                let buf = self.read_loc(&loc)?;
                let buf = self.to_buf(&buf)?;
                Ok(bits_to_value(&get_bits(&buf, bit, bits), bits))
            }
            Some(Obj::Method { .. }) | Some(Obj::Builtin(_)) => self.invoke(p, vec![]),
            Some(_) => Ok(Value::Ref(Ref::Named(p.to_string()))),
            None => Err(Error::NotFound(p.to_string()).into()),
        }
    }

    fn store(&mut self, c: &Ctx, t: &Target, v: Value) -> R<()> {
        match t {
            Target::Null => {}
            Target::Debug => {
                if self.debug.len() < 64 {
                    self.debug.push(format!("{:?}", v));
                }
            }
            Target::Local(i) => self.frames[c.frame].locals[*i as usize] = v,
            Target::Arg(i) => {
                let cur = self.frames[c.frame].args.get(*i as usize).cloned();
                match cur {
                    Some(Value::Ref(r)) => self.write_ref(&r, v)?,
                    _ => {
                        let a = &mut self.frames[c.frame].args;
                        if a.len() <= *i as usize {
                            a.resize(*i as usize + 1, Value::None);
                        }
                        a[*i as usize] = v;
                    }
                }
            }
            Target::Named(p) => self.write_named(p, v)?,
            Target::Ref(r) => self.write_ref(r, v)?,
        }
        Ok(())
    }

    fn write_ref(&mut self, r: &Ref, v: Value) -> R<()> {
        match r {
            Ref::Named(p) => self.write_named(p, v),
            Ref::Elem(loc, i) => {
                if *i == usize::MAX {
                    if let Some(slot) = self.loc_mut(loc) {
                        *slot = v;
                    }
                    return Ok(());
                }
                let byte = self.to_int(&v).unwrap_or(0) as u8;
                if let Some(slot) = self.loc_mut(loc) {
                    match slot {
                        Value::Pkg(p) if *i < p.len() => p[*i] = v,
                        Value::Buf(b) if *i < b.len() => b[*i] = byte,
                        _ => return Err(Error::Type.into()),
                    }
                }
                Ok(())
            }
        }
    }

    fn write_named(&mut self, p: &str, v: Value) -> R<()> {
        match self.ns.get(p).cloned() {
            Some(Obj::Name(old)) => {
                let new = match old {
                    Value::Int(_) => Value::Int(self.to_int(&v)?),
                    Value::Buf(ob) => {
                        let mut nb = self.to_buf(&v)?;
                        nb.resize(ob.len(), 0);
                        Value::Buf(nb)
                    }
                    Value::Str(_) => Value::Str(self.to_str(&v)?),
                    _ => v,
                };
                self.ns.insert(p.to_string(), Obj::Name(new));
            }
            Some(Obj::Field(f)) => self.write_field(&f, &v)?,
            Some(Obj::BufField { loc, bit, bits }) => {
                let data = self.to_buf(&v)?;
                if let Some(Value::Buf(buf)) = self.loc_mut(&loc) {
                    set_bits(buf, bit, bits, &data);
                }
            }
            Some(_) => {}
            None => {
                self.ns.insert(p.to_string(), Obj::Name(v));
            }
        }
        Ok(())
    }

    // ---- fields -----------------------------------------------------------------

    fn access_bits(f: &Field) -> u64 {
        match f.flags & 0x0F {
            2 => 16,
            3 => 32,
            4 => 64,
            _ => 8,
        }
    }

    fn region(&self, path: &str) -> R<(u8, u64)> {
        match self.ns.get(path) {
            Some(Obj::Region { space, base, .. }) => Ok((*space, *base)),
            _ => Err(Error::NotFound(path.to_string()).into()),
        }
    }

    /// One access unit of a field's backing store (the unit's byte offset).
    fn unit_read(&mut self, f: &Field, off: u64, w: u64) -> R<u64> {
        match &f.kind {
            FieldKind::Region(r) => {
                let (space, base) = self.region(r)?;
                Ok(self.space_read(space, base + off, w as u32))
            }
            FieldKind::Bank { region, bank, value } => {
                let (region, bank, value) = (region.clone(), bank.clone(), *value);
                self.write_named(&bank, Value::Int(value))?;
                let (space, base) = self.region(&region)?;
                Ok(self.space_read(space, base + off, w as u32))
            }
            FieldKind::Index { index, data } => {
                let (index, data) = (index.clone(), data.clone());
                self.write_named(&index, Value::Int(off))?;
                let v = self.read_named(&data)?;
                Ok(self.to_int(&v)?)
            }
        }
    }

    fn unit_write(&mut self, f: &Field, off: u64, w: u64, v: u64) -> R<()> {
        match &f.kind {
            FieldKind::Region(r) => {
                let (space, base) = self.region(r)?;
                self.space_write(space, base + off, w as u32, v);
            }
            FieldKind::Bank { region, bank, value } => {
                let (region, bank, value) = (region.clone(), bank.clone(), *value);
                self.write_named(&bank, Value::Int(value))?;
                let (space, base) = self.region(&region)?;
                self.space_write(space, base + off, w as u32, v);
            }
            FieldKind::Index { index, data } => {
                let (index, data) = (index.clone(), data.clone());
                self.write_named(&index, Value::Int(off))?;
                self.write_named(&data, Value::Int(v))?;
            }
        }
        Ok(())
    }

    fn space_read(&mut self, space: u8, addr: u64, bits: u32) -> u64 {
        match space {
            MEM | IO | PCI => self.host.read(space, addr, bits),
            _ => 0,
        }
    }

    fn space_write(&mut self, space: u8, addr: u64, bits: u32, v: u64) {
        if matches!(space, MEM | IO | PCI) {
            self.host.write(space, addr, bits, v);
        }
    }

    fn read_field(&mut self, f: &Field) -> R<Value> {
        let w = Self::access_bits(f);
        let nbytes = f.bits.div_ceil(8) as usize;
        let mut out = vec![0u8; nbytes.max(1)];
        let mut unit_idx = u64::MAX;
        let mut unit = 0u64;
        for i in 0..f.bits {
            let b = f.bit + i;
            let u = b / w;
            if u != unit_idx {
                unit_idx = u;
                unit = self.unit_read(f, u * w / 8, w)?;
            }
            if unit >> (b % w) & 1 != 0 {
                out[(i / 8) as usize] |= 1 << (i % 8);
            }
        }
        Ok(bits_to_value(&out, f.bits))
    }

    fn write_field(&mut self, f: &Field, v: &Value) -> R<()> {
        let w = Self::access_bits(f);
        let data = self.to_buf(v)?;
        let first = f.bit / w;
        let last = (f.bit + f.bits.max(1) - 1) / w;
        for u in first..=last {
            let lo = u * w;
            let hi = lo + w;
            let covers = f.bit <= lo && f.bit + f.bits >= hi;
            let mut val = if covers {
                0
            } else {
                match (f.flags >> 5) & 3 {
                    1 => u64::MAX,
                    2 => 0,
                    _ => self.unit_read(f, lo / 8, w)?,
                }
            };
            for b in lo.max(f.bit)..hi.min(f.bit + f.bits) {
                let i = b - f.bit;
                let bit = data.get((i / 8) as usize).map_or(0, |x| (x >> (i % 8)) & 1) as u64;
                let m = 1u64 << (b - lo);
                val = if bit != 0 { val | m } else { val & !m };
            }
            if w < 64 {
                val &= (1u64 << w) - 1;
            }
            self.unit_write(f, lo / 8, w, val)?;
        }
        Ok(())
    }

    /// The PCI address of the device a PCI_Config region is in: _ADR of the
    /// nearest device, and the bus of its root bridge (_BBN).
    fn pci_address(&mut self, scope: &str) -> u64 {
        let mut s = scope.to_string();
        let mut adr = None;
        loop {
            if matches!(self.ns.get(&s), Some(Obj::Device)) {
                if adr.is_none() {
                    adr = self.int(&s, "_ADR");
                }
                let hid = self.child(&s, "_HID").and_then(|r| r.ok());
                let is_root = matches!(&hid, Some(Value::Int(v)) if eisa_id(*v) == "PNP0A03" || eisa_id(*v) == "PNP0A08") || matches!(&hid, Some(Value::Str(h)) if h == "PNP0A03" || h == "PNP0A08");
                if is_root {
                    let bus = self.int(&s, "_BBN").unwrap_or(0);
                    let a = adr.unwrap_or(0);
                    return bus << 48 | (a >> 16 & 0x1F) << 40 | (a & 7) << 32;
                }
            }
            if s == "\\" {
                break;
            }
            s = parent(&s);
        }
        let a = adr.unwrap_or(0);
        (a >> 16 & 0x1F) << 40 | (a & 7) << 32
    }

    // ---- conversions ---------------------------------------------------------------

    pub fn to_int(&self, v: &Value) -> Result<u64, Error> {
        Ok(match v {
            Value::Int(i) => *i,
            Value::Buf(b) => {
                let mut x = 0u64;
                for (i, byte) in b.iter().take(8).enumerate() {
                    x |= (*byte as u64) << (8 * i);
                }
                x & self.ones
            }
            Value::Str(s) => {
                let h = s.trim_start_matches("0x").trim_start_matches("0X");
                u64::from_str_radix(&h.chars().take_while(|c| c.is_ascii_hexdigit()).collect::<String>(), 16).unwrap_or(0) & self.ones
            }
            Value::None => 0,
            Value::Ref(Ref::Named(p)) => match self.ns.get(p) {
                Some(Obj::Name(v)) => return self.to_int(&v.clone()),
                _ => return Err(Error::Type),
            },
            _ => return Err(Error::Type),
        })
    }

    pub fn to_buf(&self, v: &Value) -> Result<Vec<u8>, Error> {
        Ok(match v {
            Value::Buf(b) => b.clone(),
            Value::Int(i) => {
                let n = if self.ones == u64::MAX { 8 } else { 4 };
                i.to_le_bytes()[..n].to_vec()
            }
            Value::Str(s) => {
                let mut b = s.as_bytes().to_vec();
                if !b.is_empty() {
                    b.push(0);
                }
                b
            }
            Value::None => vec![],
            Value::Ref(Ref::Named(p)) => match self.ns.get(p) {
                Some(Obj::Name(v)) => return self.to_buf(&v.clone()),
                _ => return Err(Error::Type),
            },
            _ => return Err(Error::Type),
        })
    }

    fn to_str(&self, v: &Value) -> Result<String, Error> {
        Ok(match v {
            Value::Str(s) => s.clone(),
            Value::Int(i) => format!("{:X}", i),
            Value::Buf(b) => b.iter().map(|x| format!("{:02X}", x)).collect::<Vec<_>>().join(" "),
            _ => return Err(Error::Type),
        })
    }

    fn concat(&self, a: &Value, b: &Value) -> Result<Value, Error> {
        Ok(match a {
            Value::Int(_) => {
                let mut x = self.to_buf(a)?;
                x.extend_from_slice(&self.to_buf(&Value::Int(self.to_int(b)?))?);
                Value::Buf(x)
            }
            Value::Str(s) => {
                let rhs = match b {
                    Value::Str(t) => t.clone(),
                    Value::Int(i) => format!("{:X}", i),
                    Value::Buf(bs) => bs.iter().map(|x| format!("{:02X}", x)).collect::<Vec<_>>().join(" "),
                    _ => String::new(),
                };
                Value::Str(format!("{}{}", s, rhs))
            }
            _ => {
                let mut x = self.to_buf(a)?;
                x.extend_from_slice(&self.to_buf(b)?);
                Value::Buf(x)
            }
        })
    }

    fn compare(&self, a: &Value, b: &Value) -> Result<core::cmp::Ordering, Error> {
        Ok(match a {
            Value::Str(s) => {
                let t = match b {
                    Value::Str(t) => t.clone(),
                    _ => String::from_utf8_lossy(&self.to_buf(b)?).trim_end_matches('\0').to_string(),
                };
                s.as_str().cmp(t.as_str())
            }
            Value::Buf(x) => x.as_slice().cmp(self.to_buf(b)?.as_slice()),
            _ => self.to_int(a)?.cmp(&self.to_int(b)?),
        })
    }
}

fn stop_err(s: Stop) -> Error {
    match s {
        Stop::Err(e) => e,
        _ => Error::Type,
    }
}

fn type_code(v: &Value) -> u64 {
    match v {
        Value::None => 0,
        Value::Int(_) => 1,
        Value::Str(_) => 2,
        Value::Buf(_) => 3,
        Value::Pkg(_) => 4,
        Value::Ref(_) => 0,
    }
}

fn parse_int(s: &str) -> u64 {
    let s = s.trim();
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(&h.chars().take_while(|c| c.is_ascii_hexdigit()).collect::<String>(), 16).unwrap_or(0)
    } else {
        s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0)
    }
}

fn from_bcd(v: u64) -> u64 {
    let (mut r, mut m) = (0, 1);
    let mut x = v;
    while x > 0 {
        r += (x & 0xF) * m;
        m *= 10;
        x >>= 4;
    }
    r
}

fn to_bcd(v: u64) -> u64 {
    let (mut r, mut s) = (0, 0);
    let mut x = v;
    while x > 0 && s < 64 {
        r |= (x % 10) << s;
        s += 4;
        x /= 10;
    }
    r
}

/// A resource template without its end tag.
fn strip_end(b: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i < b.len() {
        let t = b[i];
        if t & 0x80 != 0 {
            if i + 3 > b.len() {
                break;
            }
            i += 3 + u16::from_le_bytes([b[i + 1], b[i + 2]]) as usize;
        } else {
            if t >> 3 == 0x0F {
                return b[..i].to_vec();
            }
            i += 1 + (t & 7) as usize;
        }
    }
    b[..i.min(b.len())].to_vec()
}

fn get_bits(buf: &[u8], bit: u64, bits: u64) -> Vec<u8> {
    let mut out = vec![0u8; bits.div_ceil(8).max(1) as usize];
    for i in 0..bits {
        let b = bit + i;
        if buf.get((b / 8) as usize).map_or(0, |x| x >> (b % 8) & 1) != 0 {
            out[(i / 8) as usize] |= 1 << (i % 8);
        }
    }
    out
}

fn set_bits(buf: &mut [u8], bit: u64, bits: u64, data: &[u8]) {
    for i in 0..bits {
        let b = bit + i;
        let Some(byte) = buf.get_mut((b / 8) as usize) else { return };
        let v = data.get((i / 8) as usize).map_or(0, |x| x >> (i % 8) & 1);
        if v != 0 {
            *byte |= 1 << (b % 8);
        } else {
            *byte &= !(1 << (b % 8));
        }
    }
}

fn bits_to_value(bytes: &[u8], bits: u64) -> Value {
    if bits <= 64 {
        let mut x = 0u64;
        for (i, b) in bytes.iter().take(8).enumerate() {
            x |= (*b as u64) << (8 * i);
        }
        Value::Int(x)
    } else {
        Value::Buf(bytes.to_vec())
    }
}
