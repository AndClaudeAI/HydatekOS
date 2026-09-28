//! The profile (name, picture choice, date) and document authors.

use crate::profile::*;

#[test]
fn text_round_trip() {
    for avatar in [Avatar::Initials(3), Avatar::Motif(5), Avatar::Picture] {
        let p = Profile { name: "Ada Obi".into(), avatar, since: (2026, 9, 28) };
        let text = p.to_text();
        assert_eq!(Profile::parse(&text), p, "{}", text);
    }
    let p = Profile::parse("name=Ada Obi\navatar=initials 3\nsince=2026-09-28\n");
    assert_eq!(p.to_text(), "name=Ada Obi\navatar=initials 3\nsince=2026-09-28\n");
    assert!(p.ready());
    assert!(!Profile::default().ready());
}

#[test]
fn damaged_files_fall_back() {
    // unknown lines, bad numbers and out-of-range choices never stop a start
    let p = Profile::parse("junk\nname=  Chinwe   Eze \navatar=motif 99\nsince=2026-13-40\nfuture=1\n");
    assert_eq!(p.name, "Chinwe Eze");
    assert_eq!(p.avatar, Avatar::Initials(0));
    assert_eq!(p.since, (0, 0, 0));
    assert_eq!(Profile::parse("avatar=initials 11").avatar, Avatar::Initials(11 % COLOURS.len() as u8));
    assert_eq!(Profile::parse("avatar=initials x").avatar, Avatar::Initials(0));
    assert!(!Profile::parse("").ready());
    assert!(!Profile::parse("name=   \n").ready());
}

#[test]
fn names() {
    assert_eq!(clean_name("  Ada   Obi  "), Some("Ada Obi".into()));
    assert_eq!(clean_name("Ada\tObi\n"), Some("Ada Obi".into()));
    assert_eq!(clean_name("   "), None);
    assert_eq!(clean_name(""), None);
    let long = "x".repeat(60);
    assert_eq!(clean_name(&long).unwrap().chars().count(), NAME_MAX);
    // cut at a space: no trailing space left
    let spaced = format!("{} {}", "a".repeat(NAME_MAX - 1), "bcd");
    assert_eq!(clean_name(&spaced).unwrap(), "a".repeat(NAME_MAX - 1));
    assert_eq!(initials("Ada Obi"), "AO");
    assert_eq!(initials("chinwe"), "C");
    assert_eq!(initials("Ngozi Okonjo-Iweala"), "NO");
    assert_eq!(initials("Ada Chioma Obi"), "AO");
    assert_eq!(initials("émile zola"), "ÉZ");
    assert_eq!(initials("(Ada) Obi"), "AO");
    assert_eq!(initials("- Ada"), "A");
    assert_eq!(initials(""), "");
    assert_eq!(first_name("Ada Obi"), "Ada");
    assert_eq!(first_name(""), "");
}

#[test]
fn greetings_and_colours() {
    assert_eq!(greeting(4), "Good evening");
    assert_eq!(greeting(5), "Good morning");
    assert_eq!(greeting(11), "Good morning");
    assert_eq!(greeting(12), "Good afternoon");
    assert_eq!(greeting(16), "Good afternoon");
    assert_eq!(greeting(17), "Good evening");
    assert_eq!(greeting(23), "Good evening");
    // the same name always gets the same colour, whatever its case
    assert_eq!(colour_for("Ada Obi"), colour_for("ada obi"));
    assert!((colour_for("Ada Obi") as usize) < COLOURS.len());
    let used: std::collections::BTreeSet<u8> = ["Ada", "Chinwe", "Tunde", "Ngozi", "Emeka", "Funke", "Bola", "Kemi", "Ife", "Zara"].iter().map(|n| colour_for(n)).collect();
    assert!(used.len() >= 4, "names spread over the colours: {:?}", used);
}

#[test]
fn square_crop() {
    // a 6×4 picture: red left third, green middle, blue right; the centre
    // square (columns 1-4) scaled to 2×2 averages each half
    let (r, g, b) = (0xFFFF0000u32, 0xFF00FF00u32, 0xFF0000FFu32);
    let mut px = vec![];
    for _ in 0..4 {
        for x in 0..6 {
            px.push(if x < 2 { r } else if x < 4 { g } else { b });
        }
    }
    let sq = square(&px, 6, 4, 2);
    assert_eq!(sq.len(), 4);
    // left half: columns 1-2 (red, green) averaged
    assert_eq!(sq[0], 0xFF7F7F00);
    // right half: columns 3-4 (green, blue)
    assert_eq!(sq[1], 0xFF007F7F);
    assert_eq!(sq[0], sq[2]);
    // transparency lies on white; a portrait picture keeps its middle
    let clear = vec![0x00000000u32; 3 * 9];
    assert!(square(&clear, 3, 9, 3).iter().all(|&p| p == 0xFFFFFFFF));
    let tall: Vec<u32> = (0..9).flat_map(|y| std::iter::repeat(if (3..6).contains(&y) { g } else { r }).take(3)).collect();
    assert!(square(&tall, 3, 9, 3).iter().all(|&p| p == g));
    // enlarging a tiny picture repeats pixels
    assert_eq!(square(&[b], 1, 1, 4), vec![b; 16]);
}

#[test]
fn authors_round_trip() {
    use crate::doc::{core_creator, Doc};
    // Hyda Scripts: .hyds and .docx
    let mut d = Doc::from_text("hello");
    d.author = "Ada Obi".into();
    assert_eq!(Doc::from_hyds(d.to_hyds().as_bytes()).unwrap().author, "Ada Obi");
    let docx = d.to_docx();
    assert_eq!(core_creator(&docx), "Ada Obi");
    assert_eq!(Doc::from_docx(&docx).unwrap().author, "Ada Obi");
    // no author: no line, and older files read fine
    let plain = Doc::from_text("hello");
    assert!(!plain.to_hyds().contains("author"));
    assert_eq!(Doc::from_hyds(plain.to_hyds().as_bytes()).unwrap().author, "");
    assert_eq!(core_creator(&plain.to_docx()), "");
    // markup in a name is escaped
    d.author = "Ada <A&B> Obi".into();
    assert_eq!(Doc::from_docx(&d.to_docx()).unwrap().author, "Ada <A&B> Obi");
    // Hyda Grids: .hydg and .xlsx
    let mut s = crate::grid::Sheet::new();
    s.author = "Ada Obi".into();
    assert_eq!(crate::gridio::from_hydg(crate::gridio::to_hydg(&s).as_bytes()).unwrap().author, "Ada Obi");
    assert_eq!(crate::gridio::from_xlsx(&crate::gridio::to_xlsx(&s)).unwrap().author, "Ada Obi");
    // Hyda Slides: .hydp and .pptx
    let mut deck = crate::deck::Deck::new();
    deck.author = "Ada Obi".into();
    assert_eq!(crate::deckio::from_hydp(crate::deckio::to_hydp(&deck).as_bytes()).unwrap().author, "Ada Obi");
    assert_eq!(crate::deckio::from_pptx(&crate::deckio::to_pptx(&deck)).unwrap().author, "Ada Obi");
}

/// Files written by other programs name their authors too.
#[test]
fn authors_from_office() {
    let fx = |f: &str| std::fs::read(format!("{}/fixtures/{}", env!("CARGO_MANIFEST_DIR"), f)).unwrap();
    // python-docx names itself; openpyxl's tag declares its namespace inline
    assert_eq!(crate::doc::Doc::from_docx(&fx("word-template.docx")).unwrap().author, "python-docx");
    assert_eq!(crate::gridio::from_xlsx(&fx("openpyxl-budget.xlsx")).unwrap().author, "openpyxl");
    // empty authors: <dc:creator/> (python-pptx) and <dc:creator></dc:creator> (LibreOffice)
    assert_eq!(crate::deckio::from_pptx(&fx("slides/python-made.pptx")).unwrap().author, "");
    assert_eq!(crate::deckio::from_pptx(&fx("slides/office-made.pptx")).unwrap().author, "");
    assert_eq!(crate::doc::Doc::from_docx(&fx("libreoffice-resaved.docx")).unwrap().author, "");
}
