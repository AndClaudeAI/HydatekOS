//! Hyda Grids files: the native .hydg format, and CSV and Excel (.xlsx)
//! import and export.

use crate::doc::{attr, esc, unesc, Tok, Tokens};
use crate::grid::*;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---- .hydg ----------------------------------------------------------------------------
//
//   HYDG 1                          magic and format version
//   app Hyda Grids                  written by (informational)
//   sheet Budget                    sheet name
//   w 0 160                         column A is 160 px wide
//   c A1 b,al=c Item                cell: reference, format, input
//   c B2 n=cur,sym=₦ 12000
//   c B5 - =SUM(B2:B4)              "-" = default format
//   end 1a2b3c4d                    CRC-32 of every byte before this line
//
// Format flags: b (bold), i (italic), al=l|c|r, n=num|cur|pct, sym=<char>.
// Inputs escape \\ (backslash), \t (tab) and \n (newline). Unknown records are
// skipped so later versions can add some.

fn esc_line(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\t' => o.push_str("\\t"),
            '\n' => o.push_str("\\n"),
            '\r' => {}
            c => o.push(c),
        }
    }
    o
}

fn unesc_line(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut cs = s.chars();
    while let Some(c) = cs.next() {
        if c == '\\' {
            match cs.next() {
                Some('t') => o.push('\t'),
                Some('n') => o.push('\n'),
                Some(x) => o.push(x),
                None => {}
            }
        } else {
            o.push(c);
        }
    }
    o
}

fn fmt_code(f: &Fmt) -> String {
    let mut parts: Vec<String> = Vec::new();
    if f.bold {
        parts.push("b".into());
    }
    if f.italic {
        parts.push("i".into());
    }
    match f.align {
        HAlign::Auto => {}
        HAlign::Left => parts.push("al=l".into()),
        HAlign::Center => parts.push("al=c".into()),
        HAlign::Right => parts.push("al=r".into()),
    }
    match f.num {
        Num::General => {}
        Num::Number => parts.push("n=num".into()),
        Num::Currency => {
            parts.push("n=cur".into());
            parts.push(format!("sym={}", f.sym));
        }
        Num::Percent => parts.push("n=pct".into()),
    }
    if parts.is_empty() {
        String::from("-")
    } else {
        parts.join(",")
    }
}

fn parse_fmt(s: &str) -> Fmt {
    let mut f = Fmt::default();
    for p in s.split(',') {
        match p {
            "b" => f.bold = true,
            "i" => f.italic = true,
            "al=l" => f.align = HAlign::Left,
            "al=c" => f.align = HAlign::Center,
            "al=r" => f.align = HAlign::Right,
            "n=num" => f.num = Num::Number,
            "n=cur" => f.num = Num::Currency,
            "n=pct" => f.num = Num::Percent,
            _ => {
                if let Some(c) = p.strip_prefix("sym=").and_then(|v| v.chars().next()) {
                    f.sym = c;
                }
            }
        }
    }
    f
}

pub fn to_hydg(s: &Sheet) -> String {
    let mut out = String::from("HYDG 1\napp Hyda Grids\n");
    out.push_str(&format!("sheet {}\n", esc_line(&s.name)));
    for (c, w) in &s.widths {
        out.push_str(&format!("w {} {}\n", c, w));
    }
    for (&(r, c), cell) in &s.cells {
        out.push_str(&format!("c {} {} {}\n", cell_name(r, c), fmt_code(&cell.fmt), esc_line(&cell.input)));
    }
    let crc = crate::zip::crc32(out.as_bytes());
    out.push_str(&format!("end {:08x}\n", crc));
    out
}

pub fn from_hydg(data: &[u8]) -> Result<Sheet, &'static str> {
    let s = core::str::from_utf8(data).map_err(|_| "not a Hyda Grids sheet")?;
    let rest = s.strip_prefix("HYDG ").ok_or("not a Hyda Grids sheet")?;
    let ver: u32 = rest.split('\n').next().unwrap_or("").trim().parse().map_err(|_| "not a Hyda Grids sheet")?;
    if ver != 1 {
        return Err("made by a newer Hyda Grids");
    }
    let end_at = s.rfind("\nend ").ok_or("the file is incomplete")? + 1;
    let want = u32::from_str_radix(s[end_at + 4..].trim(), 16).map_err(|_| "the file is damaged")?;
    if crate::zip::crc32(&data[..end_at]) != want {
        return Err("the file is damaged");
    }
    let mut sheet = Sheet::new();
    for line in s[..end_at].lines().skip(1) {
        let (tag, val) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "sheet" => sheet.name = unesc_line(val),
            "w" => {
                if let Some((c, w)) = val.split_once(' ') {
                    if let (Ok(c), Ok(w)) = (c.parse::<u32>(), w.parse::<i32>()) {
                        sheet.widths.insert(c, w.clamp(24, 600));
                    }
                }
            }
            "c" => {
                let mut it = val.splitn(3, ' ');
                let (Some(rf), Some(fm)) = (it.next(), it.next()) else { return Err("the file is damaged") };
                let Some((r, c, _, _)) = parse_ref(rf) else { return Err("the file is damaged") };
                let cell = Cell { input: unesc_line(it.next().unwrap_or("")), fmt: parse_fmt(fm) };
                sheet.cells.insert((r, c), cell);
            }
            _ => {}
        }
    }
    Ok(sheet)
}

// ---- CSV ------------------------------------------------------------------------------

pub fn from_csv(text: &str) -> Sheet {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    // the delimiter: whichever of , ; tab is most common in the first line
    let first = text.lines().next().unwrap_or("");
    let delim = [',', ';', '\t'].into_iter().max_by_key(|d| first.matches(*d).count()).unwrap_or(',');
    let mut sheet = Sheet::new();
    let (mut r, mut c) = (0u32, 0u32);
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let put = |sheet: &mut Sheet, r: u32, c: u32, f: &mut String| {
        if r < MAX_ROWS && c < MAX_COLS && !f.is_empty() {
            sheet.set_input(r, c, f);
        }
        f.clear();
    };
    while let Some(ch) = chars.next() {
        if quoted {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            quoted = true;
        } else if ch == delim {
            put(&mut sheet, r, c, &mut field);
            c += 1;
        } else if ch == '\n' {
            put(&mut sheet, r, c, &mut field);
            r += 1;
            c = 0;
        } else if ch != '\r' {
            field.push(ch);
        }
    }
    put(&mut sheet, r, c, &mut field);
    sheet
}

/// Values (not formulas), as other programs expect from a CSV file.
pub fn to_csv(s: &Sheet) -> String {
    let (rows, cols) = s.used();
    let mut calc = Calc::new(s);
    let mut out = String::new();
    for r in 0..rows {
        for c in 0..cols {
            if c > 0 {
                out.push(',');
            }
            let v = calc.value(r, c);
            let t = match &v {
                Val::Num(x) => general_exact(*x),
                v => display(v, &Fmt::default()),
            };
            if t.contains([',', '"', '\n', '\r']) || t.starts_with(' ') || t.ends_with(' ') {
                out.push('"');
                out.push_str(&t.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push_str("\r\n");
    }
    out
}

// ---- Excel (.xlsx) --------------------------------------------------------------------

const EXCEL_ERRORS: [&str; 6] = ["#DIV/0!", "#VALUE!", "#REF!", "#NAME?", "#NUM!", "#N/A"];

/// Our formula text as Excel stores it: ',' separators, newer functions prefixed.
fn formula_out(f: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    for ch in f.chars() {
        if ch == '"' {
            in_str = !in_str;
        }
        out.push(if ch == ';' && !in_str { ',' } else { ch });
    }
    let upper = out.to_ascii_uppercase();
    let mut res = String::new();
    let mut i = 0;
    while let Some(k) = upper[i..].find("CONCAT(") {
        let at = i + k;
        let before = upper[..at].chars().last();
        res.push_str(&out[i..at]);
        if !before.map_or(false, |c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
            res.push_str("_xlfn.");
        }
        res.push_str(&out[at..at + 7]);
        i = at + 7;
    }
    res.push_str(&out[i..]);
    res
}

fn formula_in(f: &str) -> String {
    let mut s = String::new();
    let mut rest = f;
    loop {
        let lower = rest.to_ascii_lowercase();
        match lower.find("_xlfn.") {
            Some(k) => {
                s.push_str(&rest[..k]);
                rest = &rest[k + 6..];
            }
            None => break,
        }
    }
    s.push_str(rest);
    s
}

pub fn to_xlsx(s: &Sheet) -> Vec<u8> {
    // styles: every distinct cell format becomes one cellXfs entry
    let mut styles: Vec<Fmt> = alloc::vec![Fmt::default()];
    let mut currency: Vec<char> = Vec::new();
    for cell in s.cells.values() {
        if !styles.contains(&cell.fmt) {
            styles.push(cell.fmt);
        }
        if cell.fmt.num == Num::Currency && !currency.contains(&cell.fmt.sym) {
            currency.push(cell.fmt.sym);
        }
    }
    let style_of = |f: &Fmt| styles.iter().position(|x| x == f).unwrap_or(0);
    let mut calc = Calc::new(s);
    let mut rows = String::new();
    let mut cur_row = None;
    for (&(r, c), cell) in &s.cells {
        if cur_row != Some(r) {
            if cur_row.is_some() {
                rows.push_str("</row>");
            }
            rows.push_str(&format!("<row r=\"{}\">", r + 1));
            cur_row = Some(r);
        }
        let rf = cell_name(r, c);
        let st = style_of(&cell.fmt);
        let sa = if st > 0 { format!(" s=\"{}\"", st) } else { String::new() };
        let v = calc.value(r, c);
        match cell.input.strip_prefix('=') {
            Some(f) => {
                let f = esc(&formula_out(f));
                let (t, val) = match &v {
                    Val::Num(x) => ("", general_exact(*x)),
                    Val::Bool(b) => (" t=\"b\"", String::from(if *b { "1" } else { "0" })),
                    Val::Err(e) if EXCEL_ERRORS.contains(e) => (" t=\"e\"", e.to_string()),
                    v => (" t=\"str\"", esc(&display(v, &Fmt::default()))),
                };
                rows.push_str(&format!("<c r=\"{}\"{}{}><f>{}</f><v>{}</v></c>", rf, sa, t, f, val));
            }
            None => match v {
                Val::Empty => rows.push_str(&format!("<c r=\"{}\"{}/>", rf, sa)),
                Val::Num(x) => rows.push_str(&format!("<c r=\"{}\"{}><v>{}</v></c>", rf, sa, general_exact(x))),
                Val::Bool(b) => rows.push_str(&format!("<c r=\"{}\"{} t=\"b\"><v>{}</v></c>", rf, sa, b as u8)),
                v => rows.push_str(&format!("<c r=\"{}\"{} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", rf, sa, esc(&display(&v, &Fmt::default())))),
            },
        }
    }
    if cur_row.is_some() {
        rows.push_str("</row>");
    }
    let mut cols = String::new();
    if !s.widths.is_empty() {
        cols.push_str("<cols>");
        for (c, w) in &s.widths {
            let chars = (*w as f64 - 5.0) / 7.0;
            cols.push_str(&format!("<col min=\"{0}\" max=\"{0}\" width=\"{1:.4}\" customWidth=\"1\"/>", c + 1, chars));
        }
        cols.push_str("</cols>");
    }
    let sheet_xml = format!(
        "{}<worksheet xmlns=\"{}\" xmlns:r=\"{}\">{}<sheetData>{}</sheetData></worksheet>",
        XML_HEAD, MAIN_NS, REL_NS, cols, rows
    );
    // styles.xml
    let mut numfmts = String::new();
    for (k, sym) in currency.iter().enumerate() {
        numfmts.push_str(&format!("<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>", 164 + k, esc(&format!("\"{}\"#,##0.00", sym))));
    }
    let mut xfs = String::new();
    for f in &styles {
        let font = f.bold as u8 + 2 * f.italic as u8;
        let num = match f.num {
            Num::General => 0,
            Num::Number => 4,
            Num::Percent => 10,
            Num::Currency => 164 + currency.iter().position(|c| *c == f.sym).unwrap_or(0),
        };
        let align = match f.align {
            HAlign::Auto => "",
            HAlign::Left => "left",
            HAlign::Center => "center",
            HAlign::Right => "right",
        };
        let al = if align.is_empty() { String::new() } else { format!("<alignment horizontal=\"{}\"/>", align) };
        xfs.push_str(&format!(
            "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"0\" borderId=\"0\" xfId=\"0\"{}{}{}>{}</xf>",
            num,
            font,
            if font > 0 { " applyFont=\"1\"" } else { "" },
            if num > 0 { " applyNumberFormat=\"1\"" } else { "" },
            if al.is_empty() { "" } else { " applyAlignment=\"1\"" },
            al
        ));
    }
    let font = |b: bool, i: bool| format!("<font>{}{}<sz val=\"11\"/><name val=\"Figtree\"/></font>", if b { "<b/>" } else { "" }, if i { "<i/>" } else { "" });
    let styles_xml = format!(
        "{}<styleSheet xmlns=\"{}\">{}<fonts count=\"4\">{}{}{}{}</fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"{}\">{}</cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>",
        XML_HEAD,
        MAIN_NS,
        if currency.is_empty() { String::new() } else { format!("<numFmts count=\"{}\">{}</numFmts>", currency.len(), numfmts) },
        font(false, false),
        font(true, false),
        font(false, true),
        font(true, true),
        styles.len(),
        xfs
    );
    let workbook = format!(
        "{}<workbook xmlns=\"{}\" xmlns:r=\"{}\"><sheets><sheet name=\"{}\" sheetId=\"1\" r:id=\"rId1\"/></sheets><calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>",
        XML_HEAD,
        MAIN_NS,
        REL_NS,
        esc(&s.name)
    );
    let mut z = crate::zip::Writer::new();
    z.add("[Content_Types].xml", CONTENT_TYPES.as_bytes());
    z.add("_rels/.rels", RELS.as_bytes());
    z.add("docProps/app.xml", APP_XML.as_bytes());
    z.add("xl/workbook.xml", workbook.as_bytes());
    z.add("xl/_rels/workbook.xml.rels", WB_RELS.as_bytes());
    z.add("xl/styles.xml", styles_xml.as_bytes());
    z.add("xl/worksheets/sheet1.xml", sheet_xml.as_bytes());
    z.finish()
}

/// Days since 1899-12-30 (Excel's day 0) -> "YYYY-MM-DD".
fn excel_date(serial: f64) -> String {
    let days = floor(serial) as i64 - 25569; // to days since 1970-01-01
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// What a number format code means to Hyda Grids (plus: is it a date?).
fn map_numfmt(id: u32, code: &str) -> (Num, char, bool) {
    match id {
        3 | 4 => return (Num::Number, '₦', false),
        5..=8 => return (Num::Currency, '$', false),
        9 | 10 => return (Num::Percent, '₦', false),
        14..=22 | 45..=47 => return (Num::General, '₦', true),
        _ => {}
    }
    // look at the code outside quotes and [brackets]
    let mut plain = String::new();
    let (mut q, mut br) = (false, false);
    for ch in code.chars() {
        match ch {
            '"' => q = !q,
            '[' if !q => br = true,
            ']' if !q => br = false,
            c if !q && !br => plain.push(c),
            _ => {}
        }
    }
    for (needle, sym) in [("₦", '₦'), ("NGN", '₦'), ("$", '$'), ("€", '€'), ("£", '£')] {
        if code.contains(needle) {
            return (Num::Currency, sym, false);
        }
    }
    if plain.contains('%') {
        return (Num::Percent, '₦', false);
    }
    let lower = plain.to_ascii_lowercase();
    if !lower.contains('#') && !lower.contains('0') && (lower.contains('y') || lower.contains('d') || lower.contains('m')) {
        return (Num::General, '₦', true);
    }
    if plain.contains("#,##0") || plain.contains("0.0") {
        return (Num::Number, '₦', false);
    }
    (Num::General, '₦', false)
}

pub fn from_xlsx(data: &[u8]) -> Result<Sheet, &'static str> {
    let text = |name: &str| crate::zip::read(data, name).map(|b| String::from_utf8_lossy(&b).into_owned());
    let wb = text("xl/workbook.xml").ok_or("not an Excel workbook")?;
    // the first sheet, found through the workbook's relationships
    let mut name = String::from("Sheet1");
    let mut rid = String::new();
    for t in Tokens::new(&wb) {
        if let Tok::Open("sheet", a, _) = t {
            name = unesc(attr(a, "name"));
            rid = attr(a, "r:id").to_string();
            break;
        }
    }
    let mut path = String::from("xl/worksheets/sheet1.xml");
    if let Some(rels) = text("xl/_rels/workbook.xml.rels") {
        for t in Tokens::new(&rels) {
            if let Tok::Open("Relationship", a, _) = t {
                if attr(a, "Id") == rid {
                    let target = attr(a, "Target");
                    path = match target.strip_prefix('/') {
                        Some(abs) => abs.to_string(),
                        None => format!("xl/{}", target),
                    };
                }
            }
        }
    }
    let sheet_xml = text(&path).ok_or("the workbook has no sheet")?;
    // shared strings
    let mut shared: Vec<String> = Vec::new();
    if let Some(sst) = text("xl/sharedStrings.xml") {
        let (mut in_si, mut in_t, mut in_rph) = (false, false, false);
        let mut cur = String::new();
        for t in Tokens::new(&sst) {
            match t {
                Tok::Open("si", _, false) => {
                    in_si = true;
                    cur.clear();
                }
                Tok::Close("si") => {
                    in_si = false;
                    shared.push(core::mem::take(&mut cur));
                }
                Tok::Open("si", _, true) => shared.push(String::new()),
                Tok::Open("rPh", _, false) => in_rph = true,
                Tok::Close("rPh") => in_rph = false,
                Tok::Open("t", _, false) => in_t = true,
                Tok::Close("t") => in_t = false,
                Tok::Text(x) if in_si && in_t && !in_rph => cur.push_str(&unesc(x)),
                _ => {}
            }
        }
    }
    // styles: number format id + font bold/italic + alignment per xf
    let mut xf_fmt: Vec<(Fmt, bool)> = Vec::new();
    if let Some(st) = text("xl/styles.xml") {
        let mut codes: BTreeMap<u32, String> = BTreeMap::new();
        let mut fonts: Vec<(bool, bool)> = Vec::new();
        let (mut in_fonts, mut in_font, mut in_xfs) = (false, false, false);
        let mut cur_font = (false, false);
        let mut cur_xf: Option<(Fmt, bool)> = None;
        for t in Tokens::new(&st) {
            match t {
                Tok::Open("numFmt", a, _) => {
                    if let Ok(id) = attr(a, "numFmtId").parse::<u32>() {
                        codes.insert(id, unesc(attr(a, "formatCode")));
                    }
                }
                Tok::Open("fonts", _, false) => in_fonts = true,
                Tok::Close("fonts") => in_fonts = false,
                Tok::Open("font", _, empty) if in_fonts => {
                    cur_font = (false, false);
                    if empty {
                        fonts.push(cur_font);
                    } else {
                        in_font = true;
                    }
                }
                Tok::Close("font") if in_font => {
                    in_font = false;
                    fonts.push(cur_font);
                }
                Tok::Open("b", a, _) if in_font => cur_font.0 = !matches!(attr(a, "val"), "0" | "false"),
                Tok::Open("i", a, _) if in_font => cur_font.1 = !matches!(attr(a, "val"), "0" | "false"),
                Tok::Open("cellXfs", _, false) => in_xfs = true,
                Tok::Close("cellXfs") => in_xfs = false,
                Tok::Open("xf", a, empty) if in_xfs => {
                    let id: u32 = attr(a, "numFmtId").parse().unwrap_or(0);
                    let (num, sym, date) = map_numfmt(id, codes.get(&id).map(|s| s.as_str()).unwrap_or(""));
                    let (b, i) = fonts.get(attr(a, "fontId").parse::<usize>().unwrap_or(0)).copied().unwrap_or((false, false));
                    let f = Fmt { bold: b, italic: i, align: HAlign::Auto, num, sym };
                    if empty {
                        xf_fmt.push((f, date));
                    } else {
                        cur_xf = Some((f, date));
                    }
                }
                Tok::Open("alignment", a, _) if in_xfs => {
                    if let Some(x) = cur_xf.as_mut() {
                        x.0.align = match attr(a, "horizontal") {
                            "left" => HAlign::Left,
                            "center" | "centerContinuous" => HAlign::Center,
                            "right" => HAlign::Right,
                            _ => HAlign::Auto,
                        };
                    }
                }
                Tok::Close("xf") if in_xfs => {
                    if let Some(x) = cur_xf.take() {
                        xf_fmt.push(x);
                    }
                }
                _ => {}
            }
        }
    }
    // the sheet
    let mut sheet = Sheet::new();
    sheet.name = name;
    // shared formulas: si -> (row, col, formula text)
    let mut shared_f: BTreeMap<String, (u32, u32, String)> = BTreeMap::new();
    let mut cell: Option<(u32, u32, String, usize)> = None; // row, col, type, style
    let (mut val, mut formula, mut istr) = (String::new(), None::<String>, String::new());
    let (mut in_v, mut in_f, mut in_is_t) = (false, false, false);
    let mut f_shared: Option<String> = None;
    for t in Tokens::new(&sheet_xml) {
        match t {
            Tok::Open("col", a, _) => {
                let (lo, hi) = (attr(a, "min").parse::<u32>().unwrap_or(1), attr(a, "max").parse::<u32>().unwrap_or(0));
                if let Ok(w) = attr(a, "width").parse::<f64>() {
                    let px = floor(w * 7.0 + 5.0 + 0.5) as i32;
                    for c in lo.max(1)..=hi.min(MAX_COLS) {
                        if px != DEFAULT_WIDTH {
                            sheet.widths.insert(c - 1, px.clamp(24, 600));
                        }
                    }
                }
            }
            Tok::Open("c", a, empty) => {
                let Some((r, c, _, _)) = parse_ref(attr(a, "r")) else { continue };
                let style = attr(a, "s").parse::<usize>().unwrap_or(0);
                if empty {
                    if let Some((f, _)) = xf_fmt.get(style) {
                        if *f != Fmt::default() {
                            sheet.set_fmt(r, c, *f);
                        }
                    }
                    continue;
                }
                cell = Some((r, c, attr(a, "t").to_string(), style));
                val.clear();
                istr.clear();
                formula = None;
                f_shared = None;
            }
            Tok::Open("v", _, false) => in_v = true,
            Tok::Close("v") => in_v = false,
            Tok::Open("f", a, empty) => {
                if attr(a, "t") == "shared" {
                    f_shared = Some(attr(a, "si").to_string());
                }
                if empty {
                    formula = Some(String::new());
                } else {
                    in_f = true;
                    formula = Some(String::new());
                }
            }
            Tok::Close("f") => in_f = false,
            Tok::Open("t", _, false) => in_is_t = true,
            Tok::Close("t") => in_is_t = false,
            Tok::Text(x) => {
                if in_v {
                    val.push_str(&unesc(x));
                } else if in_f {
                    if let Some(f) = formula.as_mut() {
                        f.push_str(&unesc(x));
                    }
                } else if in_is_t {
                    istr.push_str(&unesc(x));
                }
            }
            Tok::Close("c") => {
                let Some((r, c, ty, style)) = cell.take() else { continue };
                if r >= MAX_ROWS || c >= MAX_COLS {
                    continue;
                }
                let (mut fmt, date) = xf_fmt.get(style).copied().unwrap_or((Fmt::default(), false));
                let input = match formula.take() {
                    Some(f) => {
                        let f = if f.is_empty() {
                            // a shared formula: shift the first cell's formula
                            match f_shared.as_ref().and_then(|si| shared_f.get(si)) {
                                Some((r0, c0, f0)) => shift(&format!("={}", f0), r as i64 - *r0 as i64, c as i64 - *c0 as i64)[1..].to_string(),
                                None => val.clone(),
                            }
                        } else {
                            if let Some(si) = &f_shared {
                                shared_f.insert(si.clone(), (r, c, f.clone()));
                            }
                            f
                        };
                        format!("={}", formula_in(&f))
                    }
                    None => match ty.as_str() {
                        "s" => shared.get(val.trim().parse::<usize>().unwrap_or(usize::MAX)).cloned().unwrap_or_default(),
                        "inlineStr" => istr.clone(),
                        "b" => String::from(if val.trim() == "1" { "TRUE" } else { "FALSE" }),
                        "str" | "e" => val.clone(),
                        _ => {
                            if date {
                                if let Ok(x) = val.trim().parse::<f64>() {
                                    fmt.num = Num::General;
                                    excel_date(x)
                                } else {
                                    val.clone()
                                }
                            } else {
                                val.trim().to_string()
                            }
                        }
                    },
                };
                // text that would read back as a number or formula stays text
                let is_text = matches!(ty.as_str(), "s" | "inlineStr" | "str") && !input.starts_with('=');
                let is_text = is_text || (matches!(ty.as_str(), "s" | "inlineStr") && input.starts_with('='));
                let input = if is_text && (input.starts_with(['=', '\'']) || !matches!(literal(&input), Val::Text(_))) {
                    format!("'{}", input)
                } else {
                    input
                };
                if !(input.is_empty() && fmt == Fmt::default()) {
                    sheet.cells.insert((r, c), Cell { input, fmt });
                }
            }
            _ => {}
        }
    }
    Ok(sheet)
}

const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const CONTENT_TYPES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>";

const RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>";

const WB_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";

const APP_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Hyda Grids</Application></Properties>";
