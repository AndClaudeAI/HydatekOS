//! Hyda Grids spreadsheets: cells, references, the formula language and its
//! evaluator, and number formatting. File formats live in `gridio.rs`.
//!
//! Formulas start with `=` and use the familiar spreadsheet syntax:
//! `=SUM(B2:B9)*1.075`, `=IF(C4>100,"over","ok")`, `=A1&" "&B1`.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const MAX_ROWS: u32 = 1000;
pub const MAX_COLS: u32 = 100;
pub const DEFAULT_WIDTH: i32 = 96;

// ---- cells ------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Num {
    #[default]
    General,
    /// 1,234.50
    Number,
    /// ₦1,234.50 (with `Fmt::sym`)
    Currency,
    /// 12.5%
    Percent,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HAlign {
    /// text left, numbers right
    #[default]
    Auto,
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fmt {
    pub bold: bool,
    pub italic: bool,
    pub align: HAlign,
    pub num: Num,
    /// currency symbol
    pub sym: char,
}

impl Default for Fmt {
    fn default() -> Fmt {
        Fmt { bold: false, italic: false, align: HAlign::Auto, num: Num::General, sym: '₦' }
    }
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Cell {
    /// what was typed: a value, or a formula starting with '='
    pub input: String,
    pub fmt: Fmt,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Sheet {
    pub name: String,
    /// who made it (a profile name); empty when unknown
    pub author: String,
    /// (row, column), both from 0
    pub cells: BTreeMap<(u32, u32), Cell>,
    /// column widths that differ from the default (logical px)
    pub widths: BTreeMap<u32, i32>,
}

impl Default for Sheet {
    fn default() -> Sheet {
        Sheet::new()
    }
}

impl Sheet {
    pub fn new() -> Sheet {
        Sheet { name: String::from("Sheet1"), author: String::new(), cells: BTreeMap::new(), widths: BTreeMap::new() }
    }

    pub fn input(&self, r: u32, c: u32) -> &str {
        self.cells.get(&(r, c)).map(|x| x.input.as_str()).unwrap_or("")
    }

    pub fn fmt(&self, r: u32, c: u32) -> Fmt {
        self.cells.get(&(r, c)).map(|x| x.fmt).unwrap_or_default()
    }

    fn tidy(&mut self, r: u32, c: u32) {
        if self.cells.get(&(r, c)).map_or(false, |x| x.input.is_empty() && x.fmt == Fmt::default()) {
            self.cells.remove(&(r, c));
        }
    }

    pub fn set_input(&mut self, r: u32, c: u32, s: &str) {
        self.cells.entry((r, c)).or_default().input = s.to_string();
        self.tidy(r, c);
    }

    pub fn set_fmt(&mut self, r: u32, c: u32, f: Fmt) {
        self.cells.entry((r, c)).or_default().fmt = f;
        self.tidy(r, c);
    }

    pub fn width(&self, c: u32) -> i32 {
        self.widths.get(&c).copied().unwrap_or(DEFAULT_WIDTH)
    }

    /// (rows, cols) that hold anything, at least 1x1.
    pub fn used(&self) -> (u32, u32) {
        let mut rows = 0;
        let mut cols = 0;
        for &(r, c) in self.cells.keys() {
            rows = rows.max(r + 1);
            cols = cols.max(c + 1);
        }
        (rows.max(1), cols.max(1))
    }
}

// ---- references --------------------------------------------------------------------

pub fn col_name(mut c: u32) -> String {
    let mut s = Vec::new();
    loop {
        s.push((b'A' + (c % 26) as u8) as char);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.iter().rev().collect()
}

pub fn cell_name(r: u32, c: u32) -> String {
    format!("{}{}", col_name(c), r + 1)
}

/// "B12" / "$B$12" -> (row, col, row absolute, col absolute)
pub fn parse_ref(s: &str) -> Option<(u32, u32, bool, bool)> {
    let b = s.as_bytes();
    let mut i = 0;
    let cabs = b.first() == Some(&b'$');
    if cabs {
        i += 1;
    }
    let cs = i;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == cs || i - cs > 3 {
        return None;
    }
    let mut c: u32 = 0;
    for &ch in &b[cs..i] {
        c = c * 26 + (ch.to_ascii_uppercase() - b'A' + 1) as u32;
    }
    let rabs = b.get(i) == Some(&b'$');
    if rabs {
        i += 1;
    }
    let rs = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == rs || i != b.len() {
        return None;
    }
    let r: u32 = s[rs..].parse().ok()?;
    if r == 0 || r > MAX_ROWS || c > MAX_COLS {
        return None;
    }
    Some((r - 1, c - 1, rabs, cabs))
}

// ---- values ----------------------------------------------------------------------

#[derive(Clone, PartialEq, Debug)]
pub enum Val {
    Empty,
    Num(f64),
    Text(String),
    Bool(bool),
    Err(&'static str),
}

impl Val {
    fn num(&self) -> Result<f64, &'static str> {
        match self {
            Val::Empty => Ok(0.0),
            Val::Num(x) => Ok(*x),
            Val::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Val::Text(t) => parse_number(t).map(|p| p.0).ok_or("#VALUE!"),
            Val::Err(e) => Err(e),
        }
    }
    fn text(&self) -> Result<String, &'static str> {
        match self {
            Val::Empty => Ok(String::new()),
            Val::Num(x) => Ok(general(*x)),
            Val::Bool(b) => Ok(String::from(if *b { "TRUE" } else { "FALSE" })),
            Val::Text(t) => Ok(t.clone()),
            Val::Err(e) => Err(e),
        }
    }
    fn truth(&self) -> Result<bool, &'static str> {
        match self {
            Val::Bool(b) => Ok(*b),
            Val::Text(t) if t.eq_ignore_ascii_case("true") => Ok(true),
            Val::Text(t) if t.eq_ignore_ascii_case("false") => Ok(false),
            Val::Text(_) => Err("#VALUE!"),
            v => v.num().map(|x| x != 0.0),
        }
    }
}

// ---- numbers without libm (the UEFI target has no FPU math library) -----------------

pub fn floor(x: f64) -> f64 {
    if !(x.abs() < 4.5e15) {
        return x;
    }
    let t = x as i64 as f64;
    if t > x {
        t - 1.0
    } else {
        t
    }
}

/// Round half away from zero to `n` decimals.
pub fn round_to(x: f64, n: i32) -> f64 {
    let m = pow_i(10.0, n);
    let y = x * m;
    let r = if y < 0.0 { -floor(-y + 0.5) } else { floor(y + 0.5) };
    r / m
}

fn pow_i(mut b: f64, e: i32) -> f64 {
    let mut n = e.unsigned_abs();
    let mut acc = 1.0;
    while n > 0 {
        if n & 1 == 1 {
            acc *= b;
        }
        b *= b;
        n >>= 1;
    }
    if e < 0 {
        1.0 / acc
    } else {
        acc
    }
}

fn sqrt(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut g = if x > 1.0 { x / 2.0 } else { 1.0 };
    for _ in 0..80 {
        let n = 0.5 * (g + x / g);
        if n == g {
            break;
        }
        g = n;
    }
    g
}

const LN2: f64 = core::f64::consts::LN_2;

fn ln(x: f64) -> f64 {
    // x = m * 2^k with m in [0.75, 1.5); ln m by the atanh series
    let (mut m, mut k) = (x, 0i32);
    while m >= 1.5 {
        m /= 2.0;
        k += 1;
    }
    while m < 0.75 {
        m *= 2.0;
        k -= 1;
    }
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let (mut term, mut sum) = (z, 0.0);
    let mut n = 1.0;
    for _ in 0..40 {
        sum += term / n;
        term *= z2;
        n += 2.0;
    }
    2.0 * sum + k as f64 * LN2
}

fn exp(x: f64) -> f64 {
    let k = floor(x / LN2 + 0.5);
    let r = x - k * LN2;
    let (mut term, mut sum) = (1.0, 1.0);
    for n in 1..30 {
        term *= r / n as f64;
        sum += term;
    }
    sum * pow_i(2.0, k as i32)
}

fn pow(b: f64, e: f64) -> f64 {
    if floor(e) == e && e.abs() < 1e6 {
        return pow_i(b, e as i32);
    }
    if b < 0.0 {
        return f64::NAN;
    }
    if b == 0.0 {
        return 0.0;
    }
    exp(e * ln(b))
}

/// "1,234.5", "₦5,000", "12%", "-3e2" -> (value, the format it suggests)
pub fn parse_number(s: &str) -> Option<(f64, Option<(Num, char)>)> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.trim_start()),
        None => (false, t),
    };
    let mut hint = None;
    let mut t = t;
    for sym in ['₦', '$', '€', '£'] {
        if let Some(r) = t.strip_prefix(sym) {
            t = r.trim_start();
            hint = Some((Num::Currency, sym));
        }
    }
    let pct = t.ends_with('%');
    if pct {
        t = &t[..t.len() - 1];
        hint = Some((Num::Percent, '₦'));
    }
    // thousands separators only in the integer part, in groups of three
    let (int, frac) = match t.find(['.', 'e', 'E']) {
        Some(k) => (&t[..k], &t[k..]),
        None => (t, ""),
    };
    let int_clean: String = if int.contains(',') {
        let groups: Vec<&str> = int.split(',').collect();
        if groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3) {
            return None;
        }
        if hint.is_none() {
            hint = Some((Num::Number, '₦'));
        }
        groups.concat()
    } else {
        int.to_string()
    };
    let body = format!("{}{}", int_clean, frac);
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-')) || !body.as_bytes()[0].is_ascii_digit() && body.as_bytes()[0] != b'.' {
        return None;
    }
    let mut v: f64 = body.parse().ok()?;
    if pct {
        v /= 100.0;
    }
    Some((if neg { -v } else { v }, hint))
}

/// Up to 10 significant digits, no trailing zeros.
pub fn general(x: f64) -> String {
    if x.is_nan() || x.is_infinite() {
        return String::from("#NUM!");
    }
    if x == 0.0 {
        return String::from("0");
    }
    let a = x.abs();
    if !(1e-9..1e11).contains(&a) {
        let s = format!("{:.5E}", x);
        // 1.23450E11 -> 1.2345E+11
        let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
        let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{}E{}{:02}", m, if e < 0 { '-' } else { '+' }, e.abs());
    }
    // digits before the point (<= 0 for numbers below 1)
    let mut digits = 1;
    let mut t = a;
    while t >= 10.0 {
        t /= 10.0;
        digits += 1;
    }
    while t < 1.0 {
        t *= 10.0;
        digits -= 1;
    }
    let dec = (10 - digits).clamp(0, 15) as usize;
    let s = format!("{:.*}", dec, round_to(x, dec as i32));
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    if s == "-0" {
        String::from("0")
    } else {
        s
    }
}

fn grouped(x: f64, dec: usize) -> String {
    let s = format!("{:.*}", dec, round_to(x.abs(), dec as i32));
    let (int, frac) = s.split_once('.').map(|(a, b)| (a.to_string(), format!(".{}", b))).unwrap_or((s.clone(), String::new()));
    let mut out = String::new();
    for (k, ch) in int.chars().enumerate() {
        if k > 0 && (int.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out + &frac
}

/// What a cell shows.
pub fn display(v: &Val, f: &Fmt) -> String {
    match v {
        Val::Empty => String::new(),
        Val::Text(t) => t.clone(),
        Val::Bool(b) => String::from(if *b { "TRUE" } else { "FALSE" }),
        Val::Err(e) => e.to_string(),
        Val::Num(x) if x.is_nan() || x.is_infinite() => String::from("#NUM!"),
        Val::Num(x) => {
            let neg = *x < 0.0 && round_to(*x, 2) != 0.0;
            match f.num {
                Num::General => general(*x),
                Num::Number => format!("{}{}", if neg { "-" } else { "" }, grouped(*x, 2)),
                Num::Currency => format!("{}{}{}", if neg { "-" } else { "" }, f.sym, grouped(*x, 2)),
                Num::Percent => {
                    let p = general(round_to(*x * 100.0, 2));
                    format!("{}%", p)
                }
            }
        }
    }
}

// ---- formulas: tokens ----------------------------------------------------------------

#[derive(Clone, PartialEq, Debug)]
enum Tok {
    Num(f64),
    Str(String),
    /// row, col, row absolute, col absolute
    Ref(u32, u32, bool, bool),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Colon,
}

/// Tokens with their byte spans in the formula (without the '=').
fn tokenize(s: &str) -> Result<Vec<(Tok, usize, usize)>, &'static str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).map_or(false, |d| d.is_ascii_digit())) {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && (b[i] == b'e' || b[i] == b'E') && b.get(i + 1).map_or(false, |d| d.is_ascii_digit() || *d == b'+' || *d == b'-') {
                i += 2;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let v: f64 = s[start..i].parse().map_err(|_| "#VALUE!")?;
            out.push((Tok::Num(v), start, i));
            continue;
        }
        if c == b'"' {
            let mut t = String::new();
            i += 1;
            loop {
                let Some(ch) = s[i..].chars().next() else { return Err("#VALUE!") };
                i += ch.len_utf8();
                if ch == '"' {
                    if b.get(i) == Some(&b'"') {
                        t.push('"');
                        i += 1;
                        continue;
                    }
                    break;
                }
                t.push(ch);
            }
            out.push((Tok::Str(t), start, i));
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'$' || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'$' || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
            let word = &s[start..i];
            let tok = match parse_ref(word) {
                Some((r, cc, ra, ca)) => Tok::Ref(r, cc, ra, ca),
                None => Tok::Ident(word.to_ascii_uppercase()),
            };
            out.push((tok, start, i));
            continue;
        }
        let two = s.get(i..i + 2).unwrap_or("");
        let op2 = ["<=", ">=", "<>"].iter().find(|o| **o == two).copied();
        if let Some(o) = op2 {
            i += 2;
            out.push((Tok::Op(o), start, i));
            continue;
        }
        i += 1;
        let tok = match c {
            b'(' => Tok::LParen,
            b')' => Tok::RParen,
            b',' | b';' => Tok::Comma,
            b':' => Tok::Colon,
            b'+' => Tok::Op("+"),
            b'-' => Tok::Op("-"),
            b'*' => Tok::Op("*"),
            b'/' => Tok::Op("/"),
            b'^' => Tok::Op("^"),
            b'&' => Tok::Op("&"),
            b'%' => Tok::Op("%"),
            b'=' => Tok::Op("="),
            b'<' => Tok::Op("<"),
            b'>' => Tok::Op(">"),
            _ => return Err("#NAME?"),
        };
        out.push((tok, start, i));
    }
    Ok(out)
}

// ---- formulas: parser ----------------------------------------------------------------

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Ref(u32, u32),
    Range(u32, u32, u32, u32),
    Neg(Box<Expr>),
    Pct(Box<Expr>),
    Bin(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

struct Parser {
    t: Vec<Tok>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }
    fn op(&self) -> Option<&'static str> {
        match self.peek() {
            Some(Tok::Op(o)) => Some(o),
            _ => None,
        }
    }
    fn bin(&mut self, ops: &[&str], next: fn(&mut Parser) -> Result<Expr, &'static str>) -> Result<Expr, &'static str> {
        let mut l = next(self)?;
        while let Some(o) = self.op().filter(|o| ops.contains(o)) {
            self.i += 1;
            let r = next(self)?;
            l = Expr::Bin(o, Box::new(l), Box::new(r));
        }
        Ok(l)
    }
    fn expr(&mut self) -> Result<Expr, &'static str> {
        self.bin(&["=", "<>", "<", ">", "<=", ">="], Parser::concat)
    }
    fn concat(&mut self) -> Result<Expr, &'static str> {
        self.bin(&["&"], Parser::add)
    }
    fn add(&mut self) -> Result<Expr, &'static str> {
        self.bin(&["+", "-"], Parser::mul)
    }
    fn mul(&mut self) -> Result<Expr, &'static str> {
        self.bin(&["*", "/"], Parser::pow)
    }
    fn pow(&mut self) -> Result<Expr, &'static str> {
        self.bin(&["^"], Parser::unary)
    }
    fn unary(&mut self) -> Result<Expr, &'static str> {
        match self.op() {
            Some("-") => {
                self.i += 1;
                Ok(Expr::Neg(Box::new(self.unary()?)))
            }
            Some("+") => {
                self.i += 1;
                self.unary()
            }
            _ => self.postfix(),
        }
    }
    fn postfix(&mut self) -> Result<Expr, &'static str> {
        let mut e = self.primary()?;
        while self.op() == Some("%") {
            self.i += 1;
            e = Expr::Pct(Box::new(e));
        }
        Ok(e)
    }
    fn primary(&mut self) -> Result<Expr, &'static str> {
        let t = self.peek().cloned().ok_or("#VALUE!")?;
        self.i += 1;
        match t {
            Tok::Num(v) => Ok(Expr::Num(v)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Ref(r, c, _, _) => {
                if self.peek() == Some(&Tok::Colon) {
                    self.i += 1;
                    match self.peek().cloned() {
                        Some(Tok::Ref(r2, c2, _, _)) => {
                            self.i += 1;
                            Ok(Expr::Range(r.min(r2), c.min(c2), r.max(r2), c.max(c2)))
                        }
                        _ => Err("#REF!"),
                    }
                } else {
                    Ok(Expr::Ref(r, c))
                }
            }
            Tok::Ident(name) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() == Some(&Tok::RParen) {
                        self.i += 1;
                    } else {
                        loop {
                            args.push(self.expr()?);
                            match self.peek() {
                                Some(Tok::Comma) => self.i += 1,
                                Some(Tok::RParen) => {
                                    self.i += 1;
                                    break;
                                }
                                _ => return Err("#VALUE!"),
                            }
                        }
                    }
                    Ok(Expr::Call(name, args))
                } else if name == "TRUE" || name == "FALSE" {
                    Ok(Expr::Bool(name == "TRUE"))
                } else if name == "#REF!" {
                    Err("#REF!")
                } else {
                    Err("#NAME?")
                }
            }
            Tok::LParen => {
                let e = self.expr()?;
                if self.peek() != Some(&Tok::RParen) {
                    return Err("#VALUE!");
                }
                self.i += 1;
                Ok(e)
            }
            _ => Err("#VALUE!"),
        }
    }
}

fn parse(formula: &str) -> Result<Expr, &'static str> {
    if formula.contains("#REF!") {
        return Err("#REF!");
    }
    let toks = tokenize(formula)?;
    let mut p = Parser { t: toks.into_iter().map(|t| t.0).collect(), i: 0 };
    let e = p.expr()?;
    if p.i != p.t.len() {
        return Err("#VALUE!");
    }
    Ok(e)
}

// ---- formulas: evaluation ------------------------------------------------------------

/// Computes cell values, caching them; recursion through the same cell is
/// reported as a circular reference.
pub struct Calc<'a> {
    sheet: &'a Sheet,
    cache: BTreeMap<(u32, u32), Val>,
    busy: BTreeSet<(u32, u32)>,
}

pub fn literal(input: &str) -> Val {
    let t = input.trim();
    if t.is_empty() {
        return Val::Empty;
    }
    if let Some((v, _)) = parse_number(t) {
        return Val::Num(v);
    }
    if t.eq_ignore_ascii_case("true") {
        return Val::Bool(true);
    }
    if t.eq_ignore_ascii_case("false") {
        return Val::Bool(false);
    }
    // a leading apostrophe forces text: '0123
    Val::Text(input.strip_prefix('\'').unwrap_or(input).to_string())
}

impl<'a> Calc<'a> {
    pub fn new(sheet: &'a Sheet) -> Calc<'a> {
        Calc { sheet, cache: BTreeMap::new(), busy: BTreeSet::new() }
    }

    pub fn value(&mut self, r: u32, c: u32) -> Val {
        if let Some(v) = self.cache.get(&(r, c)) {
            return v.clone();
        }
        if self.busy.contains(&(r, c)) {
            return Val::Err("#CIRC!");
        }
        let input = self.sheet.input(r, c);
        let v = match input.strip_prefix('=') {
            Some(f) if !f.trim().is_empty() => {
                self.busy.insert((r, c));
                let v = match parse(f) {
                    Ok(e) => self.eval(&e),
                    Err(e) => Val::Err(e),
                };
                self.busy.remove(&(r, c));
                match v {
                    Val::Num(x) if x.is_nan() || x.is_infinite() => Val::Err("#NUM!"),
                    Val::Empty => Val::Num(0.0),
                    v => v,
                }
            }
            _ => literal(input),
        };
        self.cache.insert((r, c), v.clone());
        v
    }

    fn eval(&mut self, e: &Expr) -> Val {
        match e {
            Expr::Num(v) => Val::Num(*v),
            Expr::Str(s) => Val::Text(s.clone()),
            Expr::Bool(b) => Val::Bool(*b),
            Expr::Ref(r, c) => self.value(*r, *c),
            Expr::Range(..) => Val::Err("#VALUE!"),
            Expr::Neg(x) => match self.eval(x).num() {
                Ok(v) => Val::Num(-v),
                Err(e) => Val::Err(e),
            },
            Expr::Pct(x) => match self.eval(x).num() {
                Ok(v) => Val::Num(v / 100.0),
                Err(e) => Val::Err(e),
            },
            Expr::Bin(op, a, b) => {
                let (a, b) = (self.eval(a), self.eval(b));
                self.binary(op, a, b)
            }
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    fn binary(&self, op: &str, a: Val, b: Val) -> Val {
        if let Val::Err(e) = a {
            return Val::Err(e);
        }
        if let Val::Err(e) = b {
            return Val::Err(e);
        }
        if op == "&" {
            return match (a.text(), b.text()) {
                (Ok(x), Ok(y)) => Val::Text(x + &y),
                (Err(e), _) | (_, Err(e)) => Val::Err(e),
            };
        }
        if matches!(op, "=" | "<>" | "<" | ">" | "<=" | ">=") {
            let ord = compare(&a, &b);
            return Val::Bool(match op {
                "=" => ord == core::cmp::Ordering::Equal,
                "<>" => ord != core::cmp::Ordering::Equal,
                "<" => ord == core::cmp::Ordering::Less,
                ">" => ord == core::cmp::Ordering::Greater,
                "<=" => ord != core::cmp::Ordering::Greater,
                _ => ord != core::cmp::Ordering::Less,
            });
        }
        let (x, y) = match (a.num(), b.num()) {
            (Ok(x), Ok(y)) => (x, y),
            (Err(e), _) | (_, Err(e)) => return Val::Err(e),
        };
        Val::Num(match op {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            "/" => {
                if y == 0.0 {
                    return Val::Err("#DIV/0!");
                }
                x / y
            }
            _ => pow(x, y),
        })
    }

    /// Arguments flattened: ranges become their cells' values.
    fn flat(&mut self, args: &[Expr]) -> Vec<(Val, bool)> {
        let mut out = Vec::new();
        for a in args {
            match a {
                Expr::Range(r1, c1, r2, c2) => {
                    for r in *r1..=*r2 {
                        for c in *c1..=*c2 {
                            out.push((self.value(r, c), true));
                        }
                    }
                }
                e => {
                    let v = self.eval(e);
                    out.push((v, false));
                }
            }
        }
        out
    }

    /// Numbers for SUM-like functions: from ranges only real numbers count,
    /// typed arguments are converted.
    fn numbers(&mut self, args: &[Expr]) -> Result<Vec<f64>, &'static str> {
        let mut out = Vec::new();
        for (v, from_range) in self.flat(args) {
            match v {
                Val::Err(e) => return Err(e),
                Val::Num(x) => out.push(x),
                v if !from_range => out.push(v.num()?),
                _ => {}
            }
        }
        Ok(out)
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Val {
        let n = |v: Result<f64, &'static str>| match v {
            Ok(x) => Val::Num(x),
            Err(e) => Val::Err(e),
        };
        let arg = |s: &mut Calc, k: usize| -> Val {
            match args.get(k) {
                Some(e) => s.eval(e),
                None => Val::Err("#VALUE!"),
            }
        };
        match name {
            "SUM" => n(self.numbers(args).map(|v| v.iter().sum())),
            "PRODUCT" => n(self.numbers(args).map(|v| v.iter().product())),
            "AVERAGE" => n(self.numbers(args).and_then(|v| if v.is_empty() { Err("#DIV/0!") } else { Ok(v.iter().sum::<f64>() / v.len() as f64) })),
            "MIN" => n(self.numbers(args).map(|v| v.iter().copied().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x)))).unwrap_or(0.0))),
            "MAX" => n(self.numbers(args).map(|v| v.iter().copied().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))).unwrap_or(0.0))),
            "COUNT" => Val::Num(self.flat(args).iter().filter(|(v, _)| matches!(v, Val::Num(_))).count() as f64),
            "COUNTA" => Val::Num(self.flat(args).iter().filter(|(v, _)| !matches!(v, Val::Empty)).count() as f64),
            "ROUND" => {
                let x = arg(self, 0).num();
                let d = if args.len() > 1 { arg(self, 1).num() } else { Ok(0.0) };
                n(x.and_then(|x| d.map(|d| round_to(x, floor(d) as i32))))
            }
            "INT" => n(arg(self, 0).num().map(floor)),
            "ABS" => n(arg(self, 0).num().map(|x| x.abs())),
            "SQRT" => n(arg(self, 0).num().and_then(|x| if x < 0.0 { Err("#NUM!") } else { Ok(sqrt(x)) })),
            "POWER" => n(arg(self, 0).num().and_then(|b| arg(self, 1).num().map(|e| pow(b, e)))),
            "MOD" => n(arg(self, 0).num().and_then(|a| {
                let b = arg(self, 1).num()?;
                if b == 0.0 {
                    return Err("#DIV/0!");
                }
                Ok(a - b * floor(a / b))
            })),
            "IF" => match arg(self, 0).truth() {
                Ok(true) => arg(self, 1),
                Ok(false) => {
                    if args.len() > 2 {
                        arg(self, 2)
                    } else {
                        Val::Bool(false)
                    }
                }
                Err(e) => Val::Err(e),
            },
            "IFERROR" => match arg(self, 0) {
                Val::Err(_) => arg(self, 1),
                v => v,
            },
            "AND" | "OR" => {
                let mut acc = name == "AND";
                for (v, _) in self.flat(args) {
                    if matches!(v, Val::Empty) {
                        continue;
                    }
                    match v.truth() {
                        Ok(b) => acc = if name == "AND" { acc && b } else { acc || b },
                        Err(e) => return Val::Err(e),
                    }
                }
                Val::Bool(acc)
            }
            "NOT" => match arg(self, 0).truth() {
                Ok(b) => Val::Bool(!b),
                Err(e) => Val::Err(e),
            },
            "CONCAT" | "CONCATENATE" => {
                let mut s = String::new();
                for (v, _) in self.flat(args) {
                    match v.text() {
                        Ok(t) => s.push_str(&t),
                        Err(e) => return Val::Err(e),
                    }
                }
                Val::Text(s)
            }
            "LEN" => match arg(self, 0).text() {
                Ok(t) => Val::Num(t.chars().count() as f64),
                Err(e) => Val::Err(e),
            },
            "UPPER" | "LOWER" | "TRIM" => match arg(self, 0).text() {
                Ok(t) => Val::Text(match name {
                    "UPPER" => t.to_uppercase(),
                    "LOWER" => t.to_lowercase(),
                    _ => t.split_whitespace().collect::<Vec<_>>().join(" "),
                }),
                Err(e) => Val::Err(e),
            },
            "COUNTIF" | "SUMIF" => {
                let crit = arg(self, 1);
                let Some(Expr::Range(r1, c1, r2, c2)) = args.first() else { return Val::Err("#VALUE!") };
                let sum_range = match args.get(2) {
                    Some(Expr::Range(a, b, _, _)) => Some((*a as i64 - *r1 as i64, *b as i64 - *c1 as i64)),
                    Some(_) => return Val::Err("#VALUE!"),
                    None => None,
                };
                let (mut count, mut sum) = (0.0, 0.0);
                for r in *r1..=*r2 {
                    for c in *c1..=*c2 {
                        let v = self.value(r, c);
                        if matches_criteria(&v, &crit) {
                            count += 1.0;
                            let sv = match sum_range {
                                Some((dr, dc)) => self.value((r as i64 + dr) as u32, (c as i64 + dc) as u32),
                                None => v,
                            };
                            if let Val::Num(x) = sv {
                                sum += x;
                            }
                        }
                    }
                }
                Val::Num(if name == "COUNTIF" { count } else { sum })
            }
            _ => Val::Err("#NAME?"),
        }
    }
}

fn compare(a: &Val, b: &Val) -> core::cmp::Ordering {
    use core::cmp::Ordering::*;
    let rank = |v: &Val| match v {
        Val::Num(_) | Val::Empty => 0,
        Val::Text(_) => 1,
        Val::Bool(_) => 2,
        Val::Err(_) => 3,
    };
    match (a, b) {
        (Val::Text(x), Val::Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Val::Empty, Val::Text(y)) => {
            if y.is_empty() {
                Equal
            } else {
                Less
            }
        }
        (Val::Text(x), Val::Empty) => {
            if x.is_empty() {
                Equal
            } else {
                Greater
            }
        }
        (Val::Bool(x), Val::Bool(y)) => x.cmp(y),
        _ if rank(a) == rank(b) => {
            let (x, y) = (a.num().unwrap_or(0.0), b.num().unwrap_or(0.0));
            x.partial_cmp(&y).unwrap_or(Equal)
        }
        _ => rank(a).cmp(&rank(b)),
    }
}

/// COUNTIF/SUMIF criteria: 5, "apples", ">10", "<>0".
fn matches_criteria(v: &Val, crit: &Val) -> bool {
    let (op, rhs) = match crit {
        Val::Text(t) => {
            let ops = ["<=", ">=", "<>", "<", ">", "="];
            match ops.iter().find(|o| t.starts_with(**o)) {
                Some(o) => (*o, literal(&t[o.len()..])),
                None => ("=", literal(t)),
            }
        }
        c => ("=", c.clone()),
    };
    if matches!(v, Val::Empty) && op == "=" && !matches!(rhs, Val::Empty) {
        return false;
    }
    let ord = compare(v, &rhs);
    let same_kind = matches!((v, &rhs), (Val::Num(_), Val::Num(_)) | (Val::Text(_), Val::Text(_)) | (Val::Bool(_), Val::Bool(_)) | (Val::Empty, _));
    use core::cmp::Ordering::*;
    match op {
        "=" => same_kind && ord == Equal,
        "<>" => !(same_kind && ord == Equal),
        "<" => same_kind && ord == Less,
        ">" => same_kind && ord == Greater,
        "<=" => same_kind && ord != Greater,
        _ => same_kind && ord != Less,
    }
}

/// Move a formula's relative references by (dr, dc), as copy/paste does.
/// References that fall off the sheet become #REF!.
pub fn shift(input: &str, dr: i64, dc: i64) -> String {
    let Some(f) = input.strip_prefix('=') else { return input.to_string() };
    let Ok(toks) = tokenize(f) else { return input.to_string() };
    let mut out = String::from("=");
    let mut last = 0;
    for (t, s, e) in toks {
        if let Tok::Ref(r, c, ra, ca) = t {
            out.push_str(&f[last..s]);
            let nr = if ra { r as i64 } else { r as i64 + dr };
            let nc = if ca { c as i64 } else { c as i64 + dc };
            if nr < 0 || nc < 0 || nr >= MAX_ROWS as i64 || nc >= MAX_COLS as i64 {
                out.push_str("#REF!");
            } else {
                out.push_str(&format!("{}{}{}{}", if ca { "$" } else { "" }, col_name(nc as u32), if ra { "$" } else { "" }, nr + 1));
            }
            last = e;
        }
    }
    out.push_str(&f[last..]);
    out
}

/// The input a new value typed into a cell gets, plus a number format it
/// suggests ("₦5,000" -> 5000 shown as currency).
pub fn typed(s: &str) -> (String, Option<(Num, char)>) {
    if s.starts_with('=') || s.starts_with('\'') {
        return (s.to_string(), None);
    }
    match parse_number(s) {
        Some((v, Some(hint))) => (general_exact(v), Some(hint)),
        _ => (s.to_string(), None),
    }
}

/// Shortest text that reads back as exactly `v`.
pub fn general_exact(v: f64) -> String {
    let g = general(v);
    if g.parse::<f64>().ok() == Some(v) {
        g
    } else {
        format!("{}", v)
    }
}

pub fn range_text(r1: u32, c1: u32, r2: u32, c2: u32) -> String {
    if (r1, c1) == (r2, c2) {
        cell_name(r1, c1)
    } else {
        format!("{}:{}", cell_name(r1, c1), cell_name(r2, c2))
    }
}
