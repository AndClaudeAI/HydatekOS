//! Hyda Scripts documents: zip/inflate, editing, Word, Markdown.

use crate::doc::*;
use crate::zip;

const SRC: &[u8] = include_bytes!("../fixtures/inflate-src.bin");

#[test]
fn inflate_matches_zlib() {
    for (name, data) in [
        ("stored", &include_bytes!("../fixtures/inflate-0.bin")[..]),
        ("level 1", &include_bytes!("../fixtures/inflate-1.bin")[..]),
        ("level 6", &include_bytes!("../fixtures/inflate-6.bin")[..]),
        ("level 9", &include_bytes!("../fixtures/inflate-9.bin")[..]),
        ("fixed Huffman", &include_bytes!("../fixtures/inflate-fixed.bin")[..]),
    ] {
        assert_eq!(zip::inflate(data, 0).as_deref(), Some(SRC), "{}", name);
    }
    assert!(zip::inflate(&[0xff, 0xff, 0xff], 0).is_none() || true, "garbage must not panic");
}

#[test]
fn zip_round_trip() {
    let mut w = zip::Writer::new();
    w.add("a.txt", b"hello");
    w.add("dir/b.xml", "₦ naira".as_bytes());
    let z = w.finish();
    assert_eq!(zip::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(zip::read(&z, "a.txt").unwrap(), b"hello");
    assert_eq!(zip::read(&z, "dir/b.xml").unwrap(), "₦ naira".as_bytes());
    assert!(zip::read(&z, "missing").is_none());
}

fn sample() -> Doc {
    let mut d = Doc { paras: vec![], author: String::new() };
    d.paras.push(Para::plain("Quarterly report", Style::Title));
    d.paras.push(Para::plain("Summary", Style::H1));
    let mut p = Para::plain("Sales grew by ₦5,000 & costs < plan.", Style::Body);
    for i in 0..5 {
        p.fmt[i] = BOLD;
    }
    p.fmt[6] = ITALIC | UNDERLINE;
    p.fmt[10] = STRIKE;
    p.align = Align::Center;
    d.paras.push(p);
    d.paras.push(Para::plain("First point", Style::Bullet));
    d.paras.push(Para::plain("Second point", Style::Bullet));
    d.paras.push(Para::plain("Step one", Style::Number));
    d.paras.push(Para::plain("Step two", Style::Number));
    d.paras.push(Para::plain("A wise quote", Style::Quote));
    d.paras.push(Para::plain("Details", Style::H2));
    let mut r = Para::plain("Right\taligned", Style::Body);
    r.align = Align::Right;
    d.paras.push(r);
    d.paras.push(Para::plain("", Style::Body));
    d
}

#[test]
fn docx_round_trip() {
    let d = sample();
    let bytes = d.to_docx();
    let back = Doc::from_docx(&bytes).expect("parse");
    assert_eq!(back, d);
}

#[test]
fn markdown_round_trip() {
    let mut d = sample();
    // Markdown has no underline or alignment
    for p in d.paras.iter_mut() {
        p.align = Align::Left;
        for f in p.fmt.iter_mut() {
            *f &= !UNDERLINE;
        }
    }
    let md = d.to_markdown();
    assert!(md.starts_with("# Quarterly report\n## Summary\n**Sales** *g*"), "{}", md);
    assert_eq!(Doc::from_markdown(&md), d);
    assert_eq!(Doc::from_text(&Doc::from_text("a\nb\n").to_text()), Doc::from_text("a\nb"));
}

#[test]
fn editing() {
    let mut d = Doc::new();
    let p = d.insert(Pos::new(0, 0), "Hello world", 0);
    assert_eq!(p, Pos::new(0, 11));
    d.paras[0].style = Style::H1;
    // Enter at the end of a heading starts body text
    let p = d.split(p);
    assert_eq!(d.paras[1].style, Style::Body);
    let p = d.insert(p, "second\nthird", BOLD);
    assert_eq!(p, Pos::new(2, 5));
    assert_eq!(d.paras.len(), 3);
    assert!(d.all_have(Pos::new(1, 0), Pos::new(2, 5), BOLD));
    d.set_fmt(Pos::new(1, 0), Pos::new(1, 3), BOLD, false);
    assert!(!d.all_have(Pos::new(1, 0), Pos::new(2, 5), BOLD));
    // cut across paragraphs and paste back
    let frag = d.slice(Pos::new(0, 6), Pos::new(2, 2));
    assert_eq!(Doc::plain(&frag), "world\nsecond\nth");
    d.delete(Pos::new(0, 6), Pos::new(2, 2));
    assert_eq!(d.paras.len(), 1);
    assert_eq!(d.paras[0].string(), "Hello ird");
    let end = d.paste(Pos::new(0, 6), &frag);
    assert_eq!(end, Pos::new(2, 2));
    assert_eq!(d.paras.iter().map(|p| p.string()).collect::<Vec<_>>(), ["Hello world", "second", "third"]);
    assert_eq!(d.paras[0].style, Style::H1);
    assert_eq!(d.word_at(Pos::new(0, 8)), (Pos::new(0, 6), Pos::new(0, 11)));
    assert_eq!(d.words(), 4);
}

/// HYDA_DOCX_OUT=path cargo test write_sample_docx -- writes the sample for
/// checking with other office software.
#[test]
fn write_sample_docx() {
    if let Ok(p) = std::env::var("HYDA_DOCX_OUT") {
        let mut d = sample();
        d.author = "Ada Obi".into();
        std::fs::write(p, d.to_docx()).unwrap();
    }
}

/// sample() written by Hyda Scripts, opened and re-saved by LibreOffice Writer
/// (deflate-compressed, LibreOffice's own styles and numbering).
#[test]
fn reads_libreoffice_docx() {
    let d = Doc::from_docx(include_bytes!("../fixtures/libreoffice-resaved.docx")).expect("parse");
    let want = sample();
    let got: Vec<_> = d.paras.iter().map(|p| (p.string(), p.style, p.align)).collect();
    let exp: Vec<_> = want.paras.iter().map(|p| (p.string(), p.style, p.align)).collect();
    assert_eq!(got, exp);
    assert_eq!(d.paras[2].fmt, want.paras[2].fmt);
}

/// Made with python-docx, which builds on Microsoft Word's default template
/// (Word's style ids, "List Bullet"/"List Number" styles, deflated parts).
#[test]
fn reads_word_template_docx() {
    let d = Doc::from_docx(include_bytes!("../fixtures/word-template.docx")).expect("parse");
    let got: Vec<_> = d.paras.iter().map(|p| (p.string(), p.style, p.align)).collect();
    use Style::*;
    assert_eq!(
        got,
        [
            ("Budget 2026".to_string(), Title, Align::Left),
            ("Overview".to_string(), H1, Align::Left),
            ("Total: ₦1,200,000 approved.".to_string(), Body, Align::Center),
            ("Rent".to_string(), Bullet, Align::Left),
            ("Salaries".to_string(), Bullet, Align::Left),
            ("Plan".to_string(), Number, Align::Left),
            ("Notes".to_string(), H2, Align::Left),
            ("Keep receipts.".to_string(), Quote, Align::Left),
        ]
    );
    let f = &d.paras[2].fmt;
    assert_eq!((f[0], f[7], f[18]), (0, BOLD, ITALIC));
}

#[test]
fn hyds_round_trip() {
    let mut d = sample();
    // awkward text: backslashes, tabs, leading spaces, the Naira sign
    d.paras.push(Para::plain("  C:\\path\\to\tfile ₦ \\t literal", Style::Body));
    let s = d.to_hyds();
    assert!(s.starts_with("HYDS 1\napp Hyda Scripts\nparas 12\np title left\nt Quarterly report\n"), "{}", s);
    assert!(s.contains("\nf 0:5:b 6:1:iu 10:1:s\n"), "{}", s);
    assert_eq!(Doc::from_hyds(s.as_bytes()), Ok(d.clone()));
    // an empty document
    assert_eq!(Doc::from_hyds(Doc::new().to_hyds().as_bytes()), Ok(Doc::new()));
}

#[test]
fn hyds_rejects_bad_files() {
    let good = sample().to_hyds();
    assert_eq!(Doc::from_hyds(b"PK\x03\x04 a zip"), Err("not a Hyda Scripts document"));
    assert_eq!(Doc::from_hyds(good.replace("HYDS 1", "HYDS 2").as_bytes()), Err("made by a newer Hyda Scripts"));
    // truncated: no end line
    let cut = &good[..good.len() / 2];
    assert_eq!(Doc::from_hyds(cut.as_bytes()), Err("the file is incomplete"));
    // one changed character fails the checksum
    let bad = good.replacen("Quarterly", "Quarterlx", 1);
    assert_eq!(Doc::from_hyds(bad.as_bytes()), Err("the file is damaged"));
}

#[test]
fn hyds_skips_unknown_records() {
    // a later version may add records; this reader ignores them
    let mut body = String::from("HYDS 1\napp Future\nparas 1\np h1 center\ncolor 336699\nt Hi\nf 0:2:b\n");
    let crc = zip::crc32(body.as_bytes());
    body.push_str(&format!("end {:08x}\n", crc));
    let d = Doc::from_hyds(body.as_bytes()).unwrap();
    assert_eq!(d.paras.len(), 1);
    assert_eq!((d.paras[0].style, d.paras[0].align, d.paras[0].string()), (Style::H1, Align::Center, "Hi".to_string()));
    assert_eq!(d.paras[0].fmt, vec![BOLD, BOLD]);
}
