//! Hyda Grids: references, formulas, number formats, .hydg / CSV / .xlsx.

use crate::grid::*;
use crate::gridio::*;

fn sheet(cells: &[(&str, &str)]) -> Sheet {
    let mut s = Sheet::new();
    for (r, input) in cells {
        let (row, col, _, _) = parse_ref(r).unwrap();
        s.set_input(row, col, input);
    }
    s
}

fn val(s: &Sheet, r: &str) -> Val {
    let (row, col, _, _) = parse_ref(r).unwrap();
    Calc::new(s).value(row, col)
}

fn num(s: &Sheet, r: &str) -> f64 {
    match val(s, r) {
        Val::Num(x) => x,
        v => panic!("{} = {:?}", r, v),
    }
}

#[test]
fn references() {
    assert_eq!((col_name(0), col_name(25), col_name(26), col_name(51), col_name(52)), ("A".into(), "Z".into(), "AA".into(), "AZ".into(), "BA".into()));
    assert_eq!(parse_ref("B12"), Some((11, 1, false, false)));
    assert_eq!(parse_ref("$c$3"), Some((2, 2, true, true)));
    assert_eq!(parse_ref("AA1"), Some((0, 26, false, false)));
    assert_eq!(parse_ref("A0"), None);
    assert_eq!(parse_ref("SUM"), None);
    assert_eq!(parse_ref("A1B"), None);
}

#[test]
fn arithmetic_and_precedence() {
    let s = sheet(&[
        ("A1", "=1+2*3"),
        ("A2", "=(1+2)*3"),
        ("A3", "=-2^2"),
        ("A4", "=2^-1"),
        ("A5", "=10%*50"),
        ("A6", "=7/2"),
        ("A7", "=2^0.5"),
        ("A8", "=1/0"),
        ("A9", "=\"n=\"&A1"),
        ("A10", "=A1>A2"),
        ("A11", "=\"abc\"=\"ABC\""),
        ("A12", "=A99+1"),
        ("A13", "=NOPE(1)"),
        ("A14", "=1+"),
        ("A15", "=0.1+0.2"),
    ]);
    assert_eq!(num(&s, "A1"), 7.0);
    assert_eq!(num(&s, "A2"), 9.0);
    assert_eq!(num(&s, "A3"), 4.0);
    assert_eq!(num(&s, "A4"), 0.5);
    assert_eq!(num(&s, "A5"), 5.0);
    assert_eq!(num(&s, "A6"), 3.5);
    assert!((num(&s, "A7") - 1.4142135623730951).abs() < 1e-12);
    assert_eq!(val(&s, "A8"), Val::Err("#DIV/0!"));
    assert_eq!(val(&s, "A9"), Val::Text("n=7".into()));
    assert_eq!(val(&s, "A10"), Val::Bool(false));
    assert_eq!(val(&s, "A11"), Val::Bool(true));
    assert_eq!(num(&s, "A12"), 1.0);
    assert_eq!(val(&s, "A13"), Val::Err("#NAME?"));
    assert_eq!(val(&s, "A14"), Val::Err("#VALUE!"));
    assert_eq!(display(&val(&s, "A15"), &Fmt::default()), "0.3");
}

#[test]
fn functions() {
    let s = sheet(&[
        ("A1", "10"),
        ("A2", "20"),
        ("A3", "apples"),
        ("A4", "30"),
        ("A5", ""),
        ("B1", "=SUM(A1:A5)"),
        ("B2", "=AVERAGE(A1:A4)"),
        ("B3", "=MIN(A1:A4)+MAX(A1:A4)"),
        ("B4", "=COUNT(A1:A5)*100+COUNTA(A1:A5)"),
        ("B5", "=IF(B1>50,\"big\",\"small\")"),
        ("B6", "=IFERROR(1/0,\"none\")"),
        ("B7", "=ROUND(2.345,2)"),
        ("B8", "=ROUND(-2.5,0)"),
        ("B9", "=MOD(-7,3)"),
        ("B10", "=SQRT(16)+ABS(-1)+INT(-1.5)"),
        ("B11", "=SUMIF(A1:A4,\">15\")"),
        ("B12", "=COUNTIF(A1:A4,\"apples\")"),
        ("B13", "=AND(TRUE,A1>5)"),
        ("B14", "=OR(FALSE,A1>50)"),
        ("B15", "=CONCAT(\"a\",A1,\"b\")"),
        ("B16", "=UPPER(A3)&LEN(A3)"),
        ("B17", "=SUM(A1,A2;5)"),
        ("B18", "=PRODUCT(A1:A2)"),
        ("B19", "=POWER(2,10)"),
        ("C1", "Rent"),
        ("C2", "Food"),
        ("C3", "Rent"),
        ("D1", "100"),
        ("D2", "50"),
        ("D3", "25"),
        ("B20", "=SUMIF(C1:C3,\"Rent\",D1:D3)"),
    ]);
    assert_eq!(num(&s, "B1"), 60.0);
    assert_eq!(num(&s, "B2"), 20.0);
    assert_eq!(num(&s, "B3"), 40.0);
    assert_eq!(num(&s, "B4"), 304.0);
    assert_eq!(val(&s, "B5"), Val::Text("big".into()));
    assert_eq!(val(&s, "B6"), Val::Text("none".into()));
    assert_eq!(num(&s, "B7"), 2.35);
    assert_eq!(num(&s, "B8"), -3.0);
    assert_eq!(num(&s, "B9"), 2.0);
    assert_eq!(num(&s, "B10"), 3.0);
    assert_eq!(num(&s, "B11"), 50.0);
    assert_eq!(num(&s, "B12"), 1.0);
    assert_eq!(val(&s, "B13"), Val::Bool(true));
    assert_eq!(val(&s, "B14"), Val::Bool(false));
    assert_eq!(val(&s, "B15"), Val::Text("a10b".into()));
    assert_eq!(val(&s, "B16"), Val::Text("APPLES6".into()));
    assert_eq!(num(&s, "B17"), 35.0);
    assert_eq!(num(&s, "B18"), 200.0);
    assert_eq!(num(&s, "B19"), 1024.0);
    assert_eq!(num(&s, "B20"), 125.0);
}

#[test]
fn circular_references() {
    let s = sheet(&[("A1", "=B1+1"), ("B1", "=A1+1"), ("C1", "=C1")]);
    assert_eq!(val(&s, "A1"), Val::Err("#CIRC!"));
    assert_eq!(val(&s, "C1"), Val::Err("#CIRC!"));
}

#[test]
fn number_formats() {
    let f = |num, sym| Fmt { num, sym, ..Fmt::default() };
    assert_eq!(general(1234567.891), "1234567.891");
    assert_eq!(general(1e12), "1E+12");
    assert_eq!(general(-0.000123), "-0.000123");
    assert_eq!(general(2.0 / 3.0), "0.6666666667");
    assert_eq!(display(&Val::Num(1234.5), &f(Num::Number, '₦')), "1,234.50");
    assert_eq!(display(&Val::Num(1234567.0), &f(Num::Currency, '₦')), "₦1,234,567.00");
    assert_eq!(display(&Val::Num(-5.5), &f(Num::Currency, '$')), "-$5.50");
    assert_eq!(display(&Val::Num(0.125), &f(Num::Percent, '₦')), "12.5%");
    assert_eq!(parse_number("₦5,000"), Some((5000.0, Some((Num::Currency, '₦')))));
    assert_eq!(parse_number("12%"), Some((0.12, Some((Num::Percent, '₦')))));
    assert_eq!(parse_number("1,234.5"), Some((1234.5, Some((Num::Number, '₦')))));
    assert_eq!(parse_number("-3e2"), Some((-300.0, None)));
    assert_eq!(parse_number("12,34"), None);
    assert_eq!(parse_number("abc"), None);
    assert_eq!(typed("₦1,200"), ("1200".to_string(), Some((Num::Currency, '₦'))));
    assert_eq!(literal("'0123"), Val::Text("0123".into()));
}

#[test]
fn shifting_references() {
    assert_eq!(shift("=A1+$B$2+B$3+$C4", 1, 1), "=B2+$B$2+C$3+$C5");
    assert_eq!(shift("=SUM(A1:A3)", 0, 2), "=SUM(C1:C3)");
    assert_eq!(shift("=A1", -1, 0), "=#REF!");
    assert_eq!(shift("=\"A1\"&A1", 1, 0), "=\"A1\"&A2");
    assert_eq!(shift("plain A1", 1, 0), "plain A1");
    let s = sheet(&[("A1", "=#REF!+1")]);
    assert_eq!(val(&s, "A1"), Val::Err("#REF!"));
}

fn budget() -> Sheet {
    let mut s = sheet(&[
        ("A1", "Item"),
        ("B1", "Q3"),
        ("A2", "Rent\twith tab"),
        ("B2", "12000"),
        ("A3", "Back\\slash"),
        ("B3", "=B2*1.5"),
        ("A4", "'007"),
        ("B4", "=SUM(B2:B3)"),
        ("C4", "=B4>1"),
    ]);
    s.name = "Budget 2026".into();
    let mut f = s.fmt(0, 0);
    f.bold = true;
    f.align = HAlign::Center;
    s.set_fmt(0, 0, f);
    s.set_fmt(1, 1, Fmt { num: Num::Currency, sym: '₦', ..Fmt::default() });
    s.set_fmt(2, 1, Fmt { num: Num::Percent, italic: true, ..Fmt::default() });
    s.set_fmt(3, 1, Fmt { num: Num::Number, align: HAlign::Right, ..Fmt::default() });
    s.widths.insert(0, 160);
    s
}

#[test]
fn hydg_round_trip_and_checks() {
    let s = budget();
    let text = to_hydg(&s);
    assert!(text.starts_with("HYDG 1\napp Hyda Grids\nsheet Budget 2026\nw 0 160\nc A1 b,al=c Item\n"), "{}", text);
    assert_eq!(from_hydg(text.as_bytes()), Ok(s));
    assert_eq!(from_hydg(b"HYDS 1\n"), Err("not a Hyda Grids sheet"));
    assert_eq!(from_hydg(text.replace("HYDG 1", "HYDG 2").as_bytes()), Err("made by a newer Hyda Grids"));
    assert_eq!(from_hydg(text[..text.find("c B2").unwrap()].as_bytes()), Err("the file is incomplete"));
    assert_eq!(from_hydg(text.replacen("12000", "12001", 1).as_bytes()), Err("the file is damaged"));
}

#[test]
fn csv_import_export() {
    let s = from_csv("Name;Amount;Note\r\nAda;1200.5;\"says \"\"hi\"\"; twice\"\r\nBola;;\n");
    assert_eq!(s.input(0, 0), "Name");
    assert_eq!(s.input(1, 1), "1200.5");
    assert_eq!(s.input(1, 2), "says \"hi\"; twice");
    assert_eq!(s.input(2, 1), "");
    let out = to_csv(&budget());
    assert_eq!(out.lines().next(), Some("Item,Q3,"));
    assert!(out.contains("\r\nBack\\slash,18000,\r\n"), "{}", out);
    assert!(out.contains("\r\n007,30000,TRUE\r\n"), "{}", out);
}

#[test]
fn xlsx_round_trip() {
    let s = budget();
    let back = from_xlsx(&to_xlsx(&s)).expect("parse");
    assert_eq!(back.name, s.name);
    assert_eq!(back.cells, s.cells);
    assert_eq!(back.width(0), 160);
}

/// Made by openpyxl (another program): shared strings, formulas, ₦ currency,
/// percent and date formats, bold/italic, alignment, a column width.
#[test]
fn reads_openpyxl_xlsx() {
    let s = from_xlsx(include_bytes!("../fixtures/openpyxl-budget.xlsx")).expect("parse");
    assert_eq!(s.name, "Budget");
    assert_eq!(s.input(0, 0), "Item");
    assert!(s.fmt(0, 0).bold);
    assert_eq!(s.input(1, 3), "=B2+C2");
    assert_eq!(s.input(4, 1), "=SUM(B2:B4)");
    assert!(s.fmt(4, 0).italic);
    assert_eq!((s.fmt(1, 1).num, s.fmt(1, 1).sym), (Num::Currency, '₦'));
    assert_eq!(s.fmt(5, 1).num, Num::Percent);
    assert_eq!(s.input(6, 1), "2026-09-27");
    assert_eq!(s.input(7, 1), "'007");
    assert_eq!(s.fmt(7, 2).align, HAlign::Center);
    assert_eq!(s.width(0), 18 * 7 + 5);
    assert_eq!(display(&val(&s, "D4"), &s.fmt(3, 3)), "₦6,200.00");
    assert_eq!(display(&val(&s, "D5"), &s.fmt(4, 3)), "44700");
    assert_eq!(display(&val(&s, "B6"), &s.fmt(5, 1)), "12.86%");
}

/// Shared formulas (one formula text reused down a column), as Excel writes them.
#[test]
fn reads_shared_formulas() {
    let sheet_xml = r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>
<row r="1"><c r="A1"><v>1</v></c><c r="B1"><f t="shared" ref="B1:B3" si="0">A1*2</f><v>2</v></c></row>
<row r="2"><c r="A2"><v>2</v></c><c r="B2"><f t="shared" si="0"/><v>4</v></c></row>
<row r="3"><c r="A3"><v>3</v></c><c r="B3"><f t="shared" si="0"/><v>6</v></c></row>
</sheetData></worksheet>"#;
    let wb = r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let rels = r#"<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>"#;
    let mut z = crate::zip::Writer::new();
    z.add("xl/workbook.xml", wb.as_bytes());
    z.add("xl/_rels/workbook.xml.rels", rels.as_bytes());
    z.add("xl/worksheets/sheet1.xml", sheet_xml.as_bytes());
    let s = from_xlsx(&z.finish()).unwrap();
    assert_eq!((s.input(0, 1), s.input(1, 1), s.input(2, 1)), ("=A1*2", "=A2*2", "=A3*2"));
}

/// HYDA_XLSX_OUT=path cargo test write_sample_xlsx -- writes budget() as .xlsx.
#[test]
fn write_sample_xlsx() {
    if let Ok(p) = std::env::var("HYDA_XLSX_OUT") {
        std::fs::write(p, to_xlsx(&budget())).unwrap();
    }
}
