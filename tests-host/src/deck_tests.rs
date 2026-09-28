//! Hyda Slides: the deck model, text layout, .hydp, and PowerPoint files.

use crate::deck::*;
use crate::deckio::*;
use crate::doc::{Pos, Style, BOLD, ITALIC, UNDERLINE};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/fixtures/slides/{}", env!("CARGO_MANIFEST_DIR"), name)).unwrap()
}

fn texts(s: &Slide) -> Vec<String> {
    s.shapes.iter().filter(|s| s.kind.has_text() && !s.is_empty()).map(|s| s.plain()).collect()
}

fn sample() -> Deck {
    let mut d = Deck::new();
    d.name = "Review \\ 2026".into();
    d.theme = theme("night");
    d.slides[0].shapes[0].set_plain("Quarterly review");
    d.slides[0].shapes[1].set_plain("Lagos, September");
    d.slides[0].notes = "Welcome.\nIntroduce the team \\ guests\tthen start".into();
    d.slides[0].trans = Trans::Fade;
    let mut s = d.new_slide(Layout::TitleContent);
    s.shapes[0].set_plain("What we shipped");
    let b = &mut s.shapes[1];
    b.text.insert(Pos::new(0, 0), "Solar mini-grids\nKano markets\nBold and italic\nSteps\nOne", 0);
    b.text.paras[1].level = 1;
    b.text.set_fmt(Pos::new(2, 0), Pos::new(2, 4), BOLD, true);
    b.text.set_fmt(Pos::new(2, 9), Pos::new(2, 15), ITALIC | UNDERLINE, true);
    b.text.paras[3].style = Style::Body;
    b.text.paras[4].style = Style::Number;
    b.text.paras[4].align = Align::Right;
    s.bg = Some(0x223344);
    s.trans = Trans::Push;
    d.slides.push(s);
    let mut s = d.new_slide(Layout::Blank);
    let mut r = Shape::new(Kind::Rect, 100, 120, 300, 150);
    r.set_plain("Revenue up 12%");
    r.fill = Some(0xC0622B);
    r.line = Some(0x111111);
    r.size = 28;
    s.shapes.push(r);
    let mut e = Shape::new(Kind::Ellipse, 500, 100, 200, 200);
    e.fill = Some(0x2F6FEB);
    e.color = Some(0xFFFF00);
    s.shapes.push(e);
    let mut t = Shape::new(Kind::Text, 100, 400, 500, 80);
    t.set_plain("A text box");
    t.anchor = Anchor::Bottom;
    s.shapes.push(t);
    let px: Vec<u32> = (0..32 * 16).map(|i| 0xFF000000 | (i as u32 * 7919 & 0xFFFFFF)).collect();
    d.pics.push(Pic { data: std::rc::Rc::new(png_encode(32, 16, &px)) });
    let mut p = Shape::new(Kind::Picture, 800, 300, 320, 160);
    p.pic = Some(0);
    s.shapes.push(p);
    d.slides.push(s);
    d
}

#[test]
fn hydp_round_trip() {
    let d = sample();
    let text = to_hydp(&d);
    assert!(text.starts_with("HYDP 1\napp Hyda Slides\n"));
    let back = from_hydp(text.as_bytes()).unwrap();
    assert_eq!(back, d);
    // a custom theme too
    let mut d2 = d.clone();
    d2.theme = Theme { id: "custom".into(), name: "Imported".into(), bg: 0x010203, title: 0x040506, text: 0x070809, accent: 0x0A0B0C, deco: vec![] };
    assert_eq!(from_hydp(to_hydp(&d2).as_bytes()).unwrap(), d2);
}

#[test]
fn hydp_damage() {
    let text = to_hydp(&sample());
    let mut bad = text.clone().into_bytes();
    let k = text.find("What we").unwrap();
    bad[k] = b'w';
    assert_eq!(from_hydp(&bad), Err("the file is damaged"));
    assert_eq!(from_hydp(&text.as_bytes()[..text.len() / 2]), Err("the file is incomplete"));
    assert_eq!(from_hydp(b"HYDS 1\n"), Err("not a Hyda Slides presentation"));
    let newer = text.replacen("HYDP 1", "HYDP 2", 1);
    assert_eq!(from_hydp(newer.as_bytes()), Err("made by a newer Hyda Slides"));
    // a wrong slide count (checksum recomputed)
    let body = text[..text.rfind("end ").unwrap()].replacen("slides 3", "slides 4", 1);
    let fixed = format!("{}end {:08x}\n", body, crate::zip::crc32(body.as_bytes()));
    assert_eq!(from_hydp(fixed.as_bytes()), Err("the file is damaged"));
    // unknown records are skipped
    let body = text[..text.rfind("end ").unwrap()].replacen("slides 3\n", "slides 3\nsparkle 7\n", 1);
    let fixed = format!("{}end {:08x}\n", body, crate::zip::crc32(body.as_bytes()));
    assert_eq!(from_hydp(fixed.as_bytes()).unwrap(), sample());
}

#[test]
fn png_encoder() {
    let px: Vec<u32> = (0..300 * 250).map(|i| (i as u32).wrapping_mul(2654435761) | 0x01000000).collect();
    let png = png_encode(300, 250, &px);
    let img = crate::image::decode(&png).unwrap();
    assert_eq!((img.w, img.h), (300, 250));
    // straight alpha survives (colour of fully transparent pixels aside)
    for (a, b) in img.px.iter().zip(px.iter()) {
        assert_eq!(a >> 24, b >> 24);
        if b >> 24 == 255 {
            assert_eq!(a, b);
        }
    }
    assert!(picture_bytes(&png).is_some());
    assert!(picture_bytes(b"not a picture").is_none());
}

#[test]
fn text_layout() {
    // the host font stand-in: every character is px/2 wide
    let mut sh = Shape::new(Kind::Text, 0, 0, 200, 300);
    sh.size = 24; // 32 units
    sh.set_plain("hello world again");
    let tb = layout(&sh);
    assert_eq!(tb.scale, 100);
    let lines: Vec<(usize, usize)> = tb.lines.iter().map(|l| (l.start, l.end)).collect();
    // 176 units inside the insets: 11 characters of 16
    assert_eq!(lines, vec![(0, 12), (12, 17)]);
    assert_eq!(tb.lines[0].px, 32);
    assert_eq!(tb.lines[1].top - tb.lines[0].top, 38);
    // a word longer than the box breaks by characters
    sh.set_plain("abcdefghijklmnopqrstuvwxyz");
    let tb = layout(&sh);
    assert_eq!(tb.lines.iter().map(|l| l.end - l.start).collect::<Vec<_>>(), vec![11, 11, 4]);
    // caret hits and positions
    sh.set_plain("hello world again");
    let tb = layout(&sh);
    let p = hit(&sh, &tb, INSET_X + 16 * 3 + 2, tb.lines[1].top + 5);
    assert_eq!(p, Pos::new(0, 15));
    assert_eq!(caret_xy(&sh, &tb, Pos::new(0, 15)), (1, INSET_X + 48));
    assert_eq!(caret_xy(&sh, &tb, Pos::new(0, 17)).0, 1);
    // centred and anchored text
    sh.text.paras[0].align = Align::Center;
    sh.anchor = Anchor::Middle;
    sh.set_plain("hi");
    sh.text.paras[0].align = Align::Center;
    let tb = layout(&sh);
    assert_eq!(tb.lines[0].x, INSET_X + (176 - 32) / 2);
    assert_eq!(tb.lines[0].top, INSET_Y + (300 - 2 * INSET_Y - 38) / 2);
    // bullets indent, and numbering counts per level
    let mut b = Shape::new(Kind::Body, 0, 0, 1000, 600);
    b.text.insert(Pos::new(0, 0), "a\nb\nc\nd", 0);
    for p in b.text.paras.iter_mut() {
        p.style = Style::Number;
    }
    b.text.paras[1].level = 1;
    let tb = layout(&b);
    let marks: Vec<String> = tb.lines.iter().map(|l| l.bullet.clone().unwrap().1).collect();
    assert_eq!(marks, vec!["1.", "1.", "2.", "3."]);
    assert_eq!(tb.lines[1].x - tb.lines[0].x, 44);
    // too much text shrinks to fit
    let mut full = Shape::new(Kind::Body, 0, 0, 400, 200);
    full.set_plain(&"many words in a row ".repeat(30));
    let tb = layout(&full);
    assert!(tb.scale < 100 && tb.scale >= 40, "{}", tb.scale);
}

#[test]
fn relayout_keeps_text() {
    let d = sample();
    let mut s = d.slides[1].clone();
    d.relayout(&mut s, Layout::TwoContent);
    assert_eq!(s.layout, Layout::TwoContent);
    assert_eq!(s.shapes[0].plain(), "What we shipped");
    assert_eq!(s.shapes[1].text.paras.len(), 5);
    assert!(s.shapes[2].is_empty());
    d.relayout(&mut s, Layout::Title);
    assert_eq!(s.shapes[0].plain(), "What we shipped");
    assert_eq!(s.shapes[1].kind, Kind::Subtitle);
    assert!(s.shapes[1].text.paras.iter().all(|p| p.style == Style::Body && p.level == 0));
    d.relayout(&mut s, Layout::Blank);
    // text survives as text boxes
    assert_eq!(s.shapes.iter().filter(|s| s.kind == Kind::Text).count(), 2);
}

#[test]
fn pptx_round_trip() {
    let d = sample();
    let z = to_pptx(&d);
    let back = from_pptx(&z).unwrap();
    assert_eq!(back.slides.len(), 3);
    assert_eq!((back.w, back.h), (1280, 720));
    assert_eq!(back.theme.bg, d.theme.bg);
    assert_eq!(back.theme.accent, d.theme.accent);
    assert_eq!(back.name, "Review \\ 2026");
    for (a, b) in d.slides.iter().zip(back.slides.iter()) {
        assert_eq!(texts(a), texts(b));
        assert_eq!(a.notes, b.notes);
        assert_eq!(a.trans, b.trans);
        assert_eq!(a.bg, b.bg);
        let shown: Vec<&Shape> = a.shapes.iter().filter(|s| !(s.kind.placeholder() && s.is_empty())).collect();
        assert_eq!(shown.len(), b.shapes.len());
        for (x, y) in shown.iter().zip(b.shapes.iter()) {
            assert_eq!((x.kind, x.x, x.y, x.w, x.h), (y.kind, y.x, y.y, y.w, y.h));
            assert_eq!(x.anchor, y.anchor);
            if x.kind.has_text() && !x.is_empty() {
                assert_eq!(x.size, y.size, "{}", x.plain());
                for (p, q) in x.text.paras.iter().zip(y.text.paras.iter()) {
                    assert_eq!((p.style, p.level, p.align), (q.style, q.level, q.align), "{}", p.string());
                    assert_eq!(p.fmt, q.fmt);
                }
            }
            if matches!(x.kind, Kind::Rect | Kind::Ellipse) {
                assert_eq!(x.fill, y.fill);
            }
            assert_eq!(x.line, y.line);
        }
    }
    assert_eq!(back.pics, d.pics);
    assert_eq!(back.slides[2].shapes[1].color, Some(0xFFFF00));
    // and it's a real zip with the parts PowerPoint expects
    for part in ["[Content_Types].xml", "ppt/presentation.xml", "ppt/slides/slide3.xml", "ppt/slideMasters/slideMaster1.xml", "ppt/theme/theme1.xml", "ppt/notesSlides/notesSlide1.xml", "ppt/media/image1.png"] {
        assert!(crate::zip::read(&z, part).is_some(), "{}", part);
    }
}

#[test]
fn pptx_from_python() {
    let d = from_pptx(&fixture("python-made.pptx")).unwrap();
    // a 4:3 deck: 10 inches wide, so 128 units to the inch
    assert_eq!((d.w, d.h), (1280, 960));
    assert_eq!(d.slides.len(), 4);
    let s = &d.slides[0];
    assert_eq!(s.layout, Layout::Title);
    assert_eq!(s.title(), "Quarterly review");
    assert_eq!(texts(s), vec!["Quarterly review", "Lagos office, September 2026"]);
    assert_eq!(s.notes, "Welcome everyone.\nIntroduce the team.");
    let t = s.shapes.iter().find(|s| s.kind == Kind::Title).unwrap();
    assert_eq!(t.text.paras[0].align, Align::Center);
    assert_eq!(t.size, 59); // 44 pt scaled to the wider slide
    let s = &d.slides[1];
    assert_eq!(s.layout, Layout::TitleContent);
    let body = s.shapes.iter().find(|s| s.kind == Kind::Body).unwrap();
    let lv: Vec<(String, u8, Style)> = body.text.paras.iter().map(|p| (p.string(), p.level, p.style)).collect();
    assert_eq!(lv, vec![("Solar mini-grids".into(), 0, Style::Bullet), ("Kano markets".into(), 1, Style::Bullet), ("Bold and italic".into(), 0, Style::Bullet), ("Mobile money".into(), 0, Style::Bullet)]);
    assert_eq!(body.text.paras[2].fmt[0], BOLD);
    assert_eq!(body.text.paras[2].fmt[6], ITALIC);
    let s = &d.slides[2];
    let rect = s.shapes.iter().find(|s| s.kind == Kind::Rect && s.plain() == "Revenue up 12%").unwrap();
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (128, 128, 384, 192));
    assert_eq!(rect.fill, Some(0xC0622B));
    assert_eq!(rect.color, Some(0xFFFFFF));
    assert_eq!(rect.size, 32);
    let ov = s.shapes.iter().find(|s| s.kind == Kind::Ellipse).unwrap();
    assert_eq!((ov.x, ov.fill), (640, Some(0x2F6FEB)));
    let tb = s.shapes.iter().find(|s| s.kind == Kind::Text).unwrap();
    assert_eq!((tb.plain().as_str(), tb.text.paras[0].align), ("A plain text box", Align::Center));
    let pic = s.shapes.iter().find(|s| s.kind == Kind::Picture).unwrap();
    assert_eq!((pic.x, pic.y, pic.w, pic.h), (768, 512, 256, 128));
    let img = crate::image::decode(&d.pics[pic.pic.unwrap()].data).unwrap();
    assert_eq!((img.w, img.h, img.px[0] & 0xFFFFFF, img.px[40] & 0xFFFFFF), (64, 32, 0xF2B544, 0x1E1B2C));
    // the group's rectangles land where they were drawn
    let greens: Vec<(i32, i32)> = s.shapes.iter().filter(|s| s.fill == Some(0x0E5A43) || s.fill == Some(0xF5C542)).map(|s| (s.x, s.y)).collect();
    assert_eq!(greens, vec![(128, 640), (320, 640)]);
}

#[test]
fn pptx_from_office() {
    let d = from_pptx(&fixture("office-made.pptx")).unwrap();
    assert_eq!(d.slides.len(), 4);
    assert_eq!(d.slides[0].title(), "Quarterly review");
    assert_eq!(d.slides[0].notes, "Welcome everyone.\nIntroduce the team.");
    let body = d.slides[1].shapes.iter().find(|s| s.kind == Kind::Body).unwrap();
    assert_eq!(body.text.paras.iter().map(|p| p.level).collect::<Vec<_>>(), vec![0, 1, 0, 0]);
    assert!(body.text.paras.iter().all(|p| p.style == Style::Bullet));
    let s = &d.slides[2];
    assert!(s.shapes.iter().any(|s| s.kind == Kind::Rect && s.fill == Some(0xC0622B) && s.plain() == "Revenue up 12%"));
    assert!(s.shapes.iter().any(|s| s.kind == Kind::Ellipse && s.fill == Some(0x2F6FEB)));
    assert!(s.shapes.iter().any(|s| s.kind == Kind::Picture));
    assert_eq!(s.shapes.iter().filter(|s| s.fill == Some(0x0E5A43) || s.fill == Some(0xF5C542)).count(), 2);
}

#[test]
fn not_a_presentation() {
    assert!(from_pptx(b"PK nonsense").is_err());
    let docx = crate::doc::Doc::from_text("hello").to_docx();
    assert!(from_pptx(&docx).is_err());
}

/// `HYDA_OUT=/path/deck.pptx cargo test --release export_sample -- --ignored`
/// writes the sample deck, to open in other programs.
#[test]
#[ignore]
fn export_sample() {
    let out = std::env::var("HYDA_OUT").unwrap();
    std::fs::write(&out, to_pptx(&sample())).unwrap();
}

#[test]
fn deflate_round_trip() {
    use crate::zip::{deflate, inflate};
    let mut seed = 12345u32;
    let mut rnd = || {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        (seed >> 16) as u8
    };
    let random: Vec<u8> = (0..70_000).map(|_| rnd()).collect();
    let text = "Hyda Slides makes presentations. ".repeat(4000).into_bytes();
    let mixed: Vec<u8> = (0..200_000u32).map(|i| if i % 1000 < 700 { (i % 7) as u8 } else { rnd() }).collect();
    let runs = vec![0u8; 100_000];
    for data in [vec![], vec![7u8], b"ab".to_vec(), random, text.clone(), mixed, runs] {
        let z = deflate(&data);
        assert_eq!(inflate(&z, data.len()).unwrap(), data, "{} bytes", data.len());
    }
    assert!(deflate(&text).len() < text.len() / 20);
    // zip entries are compressed when it helps, and read back
    let mut w = crate::zip::Writer::new();
    w.add("a.txt", &text);
    w.add("b.bin", &[1, 2, 3]);
    let z = w.finish();
    assert!(z.len() < text.len() / 10);
    assert_eq!(crate::zip::read(&z, "a.txt").unwrap(), text);
    assert_eq!(crate::zip::read(&z, "b.bin").unwrap(), vec![1, 2, 3]);
}

/// A deck using every kind of shape and setting.
fn features() -> Deck {
    let mut d = Deck::new();
    d.name = "Features".into();
    d.theme = theme("slate");
    d.footer = "Hydatek · Lagos".into();
    d.numbers = true;
    d.slides[0].shapes[0].set_plain("Every feature");
    let mut s = d.new_slide(Layout::TitleOnly);
    s.shapes[0].set_plain("Tables and charts");
    s.bg_grad = Some((0x445566, 45));
    let mut tb = Shape::new(Kind::Table, 80, 180, 500, 150);
    let mut t = Table::new(3, 2, 500, 150);
    for (i, (a, b)) in [("App", "Users"), ("Browser", "41,200"), ("Slides", "9,800")].iter().enumerate() {
        t.cell_mut(i, 0).insert(Pos::new(0, 0), a, 0);
        t.cell_mut(i, 1).insert(Pos::new(0, 0), b, 0);
    }
    t.cell_mut(1, 0).set_fmt(Pos::new(0, 0), Pos::new(0, 7), BOLD, true);
    t.banded = false;
    tb.table = Some(t);
    tb.size = 20;
    tb.fit_table();
    s.shapes.push(tb);
    let mut ch = Shape::new(Kind::Chart, 640, 170, 560, 420);
    ch.chart = Some(Chart { kind: ChartKind::Bar, title: "Sales".into(), cats: vec!["North".into(), "South".into(), "East".into()], series: vec![Series { name: "2025".into(), vals: vec![1.5, 2.25, -0.5] }, Series { name: "2026".into(), vals: vec![3.0, 4.0, 5.5] }], legend: true });
    s.shapes.push(ch);
    let mut pie = Shape::new(Kind::Chart, 80, 400, 400, 280);
    pie.chart = Some(Chart::sample(ChartKind::Pie));
    pie.chart.as_mut().unwrap().legend = false;
    s.shapes.push(pie);
    d.slides.push(s);
    let mut s = d.new_slide(Layout::Blank);
    let mut star = Shape::new(Kind::Rect, 100, 100, 200, 200);
    star.geom = Geom::Star5;
    star.rot = 30;
    star.fill = Some(0xF2B544);
    star.anim = Anim::Fade;
    star.anim_order = 2;
    s.shapes.push(star);
    let mut arrow = Shape::new(Kind::Rect, 400, 100, 300, 120);
    arrow.geom = Geom::RightArrow;
    arrow.fill = Some(0x2F6FEB);
    arrow.grad = Some((0x8E6CB5, 90));
    arrow.flip_h = true;
    arrow.line = Some(0x111111);
    arrow.line_w = 6;
    arrow.set_plain("Go");
    arrow.anim = Anim::Fly;
    arrow.anim_order = 1;
    s.shapes.push(arrow);
    let mut ln = Shape::new(Kind::Line, 0, 0, 0, 0);
    set_line_ends(&mut ln, (900, 500), (300, 400));
    ln.line = Some(0xFF0000);
    ln.line_w = 4;
    ln.head = true;
    ln.tail = true;
    ln.anim = Anim::Appear;
    ln.anim_order = 3;
    s.shapes.push(ln);
    let mut rr = Shape::new(Kind::Rect, 800, 200, 250, 150);
    rr.geom = Geom::RoundRect;
    s.shapes.push(rr);
    s.notes = "Stars and arrows".into();
    d.slides.push(s);
    d
}

#[test]
fn features_hydp_round_trip() {
    let d = features();
    let back = from_hydp(to_hydp(&d).as_bytes()).unwrap();
    assert_eq!(back, d);
}

fn same_features(d: &Deck, back: &Deck) {
    assert_eq!((back.footer.as_str(), back.numbers), (d.footer.as_str(), d.numbers));
    assert_eq!(back.slides[1].bg_grad, d.slides[1].bg_grad);
    let t = back.slides[1].shapes.iter().find(|s| s.kind == Kind::Table).unwrap();
    let tt = t.table.as_ref().unwrap();
    assert_eq!((tt.nrows(), tt.ncols(), tt.header, tt.banded), (3, 2, true, false));
    assert_eq!(crate::doc::Doc::plain(&tt.cell(2, 1).paras), "9,800");
    assert_eq!(tt.cell(1, 0).paras[0].fmt[0], BOLD);
    assert_eq!(t.size, 20);
    let charts: Vec<&Chart> = back.slides[1].shapes.iter().filter_map(|s| s.chart.as_ref()).collect();
    assert_eq!(charts.len(), 2);
    assert_eq!(charts[0], d.slides[1].shapes[2].chart.as_ref().unwrap());
    assert_eq!(charts[1].kind, ChartKind::Pie);
    assert_eq!(charts[1].series[0].vals, vec![45.0, 25.0, 18.0, 12.0]);
    let s = &back.slides[2];
    let star = s.shapes.iter().find(|x| x.geom == Geom::Star5).unwrap();
    assert_eq!((star.rot, star.fill, star.anim), (30, Some(0xF2B544), Anim::Fade));
    let arrow = s.shapes.iter().find(|x| x.geom == Geom::RightArrow).unwrap();
    assert_eq!((arrow.flip_h, arrow.grad, arrow.line, arrow.line_w, arrow.anim), (true, Some((0x8E6CB5, 90)), Some(0x111111), 6, Anim::Fly));
    let ln = s.shapes.iter().find(|x| x.kind == Kind::Line).unwrap();
    assert_eq!(line_ends(ln), ((900, 500), (300, 400)));
    assert_eq!((ln.head, ln.tail, ln.line, ln.line_w, ln.anim), (true, true, Some(0xFF0000), 4, Anim::Appear));
    assert!(s.shapes.iter().any(|x| x.geom == Geom::RoundRect));
    // the play order: arrow, star, line
    let mut v: Vec<&Shape> = s.shapes.iter().filter(|x| x.anim != Anim::None).collect();
    v.sort_by_key(|x| x.anim_order);
    let order: Vec<Geom> = v.iter().map(|x| if x.kind == Kind::Line { Geom::Diamond } else { x.geom }).collect();
    assert_eq!(order, vec![Geom::RightArrow, Geom::Star5, Geom::Diamond]);
}

#[test]
fn features_pptx_round_trip() {
    let d = features();
    let z = to_pptx(&d);
    for part in ["ppt/charts/chart1.xml", "ppt/charts/chart2.xml", "ppt/embeddings/Microsoft_Excel_Worksheet1.xlsx"] {
        assert!(crate::zip::read(&z, part).is_some(), "{}", part);
    }
    // the chart's workbook holds its data
    let wb = crate::gridio::from_xlsx(&crate::zip::read(&z, "ppt/embeddings/Microsoft_Excel_Worksheet1.xlsx").unwrap()).unwrap();
    // (a name that looks like a number is kept as text)
    assert_eq!((wb.input(1, 0), wb.input(0, 2), wb.input(3, 1)), ("North", "'2026", "-0.5"));
    let back = from_pptx(&z).unwrap();
    same_features(&d, &back);
}

#[test]
fn features_from_office() {
    // our export, opened and saved again by LibreOffice Impress
    let back = from_pptx(&fixture("office-features.pptx")).unwrap();
    same_features(&features(), &back);
}

#[test]
fn python_tables_and_charts() {
    let d = from_pptx(&fixture("python-made.pptx")).unwrap();
    let s = &d.slides[3];
    let t = s.shapes.iter().find(|s| s.kind == Kind::Table).unwrap().table.as_ref().unwrap();
    assert_eq!((t.nrows(), t.ncols()), (3, 3));
    assert_eq!(crate::doc::Doc::plain(&t.cell(0, 1).paras), "Q1");
    assert_eq!(crate::doc::Doc::plain(&t.cell(2, 2).paras), "7");
    let charts: Vec<&Chart> = s.shapes.iter().filter_map(|s| s.chart.as_ref()).collect();
    assert_eq!(charts[0].kind, ChartKind::Column);
    assert_eq!(charts[0].cats, vec!["East", "West", "Midwest"]);
    assert_eq!(charts[0].series[1].name, "Q2 Sales");
    assert_eq!(charts[0].series[1].vals, vec![22.3, 28.6, 15.2]);
    assert_eq!(charts[1].kind, ChartKind::Pie);
    assert_eq!(charts[2].kind, ChartKind::Line);
    let ln = s.shapes.iter().find(|s| s.kind == Kind::Line).unwrap();
    assert!(ln.tail && !ln.head);
    let r = s.shapes.iter().find(|s| s.geom == Geom::Star5).unwrap();
    assert_eq!(r.rot, 45);
    let g = s.shapes.iter().find(|s| s.grad.is_some()).unwrap();
    assert_eq!((g.fill, g.grad.map(|x| x.0)), (Some(0xFF0000), Some(0x0000FF)));
}

#[test]
fn geometry_and_numbers() {
    assert_eq!((sin_deg(0), sin_deg(90), sin_deg(180), sin_deg(270), cos_deg(0), cos_deg(180)), (0, 16384, 0, -16384, 16384, -16384));
    assert_eq!(rotate(10, 0, 0, 0, 90), (0, 10));
    assert_eq!(rotate(10, 0, 0, 0, -90), (0, -10));
    for g in GEOMS {
        let o = outline(Kind::Rect, g, 100, 60);
        assert!(o.len() >= 3, "{:?}", g);
        assert!(o.iter().all(|&(x, y)| (-2..=1602).contains(&x) && (-2..=962).contains(&y)), "{:?}", g);
        assert_eq!(Geom::from_id(g.id()), Some(g));
    }
    assert_eq!(outline(Kind::Ellipse, Geom::Rect, 100, 100).len(), 90);
    // hit testing turned shapes and lines
    let mut sh = Shape::new(Kind::Rect, 0, 0, 200, 20);
    assert!(sh.contains(190, 10));
    sh.rot = 90;
    assert!(!sh.contains(190, 10) && sh.contains(100, 90));
    let mut ln = Shape::new(Kind::Line, 0, 0, 0, 0);
    set_line_ends(&mut ln, (100, 100), (0, 0));
    assert_eq!((ln.flip_h, ln.flip_v), (true, true));
    assert!(ln.contains(50, 52) && !ln.contains(50, 80));
    // axis steps
    assert_eq!(nice_ticks(0.0, 62.5), (0.0, 20.0, 4));
    assert_eq!(nice_ticks(-0.5, 5.5), (-2.0, 2.0, 4));
    assert_eq!((fmt_num(1234.5), fmt_num(-0.25), fmt_num(3.0), fmt_num(0.001)), ("1234.5".to_string(), "-0.25".to_string(), "3".to_string(), "0".to_string()));
    assert_eq!((parse_num("1,250.5"), parse_num("-3"), parse_num("₦40"), parse_num("x"), parse_num("")), (Some(1250.5), Some(-3.0), Some(40.0), None, None));
    // tables grow and shrink
    let mut t = Table::new(2, 2, 400, 100);
    t.cell_mut(1, 1).insert(Pos::new(0, 0), "x", 0);
    t.insert_row(0);
    t.insert_col(2);
    assert_eq!((t.nrows(), t.ncols(), t.cols.iter().sum::<i32>()), (3, 3, 400));
    assert_eq!(crate::doc::Doc::plain(&t.cell(2, 1).paras), "x");
    t.delete_col(0);
    t.delete_row(0);
    assert_eq!((t.nrows(), t.ncols()), (2, 2));
    assert_eq!(crate::doc::Doc::plain(&t.cell(1, 0).paras), "x");
    // rows grow to fit their text
    let mut sh = Shape::new(Kind::Table, 0, 0, 200, 60);
    sh.table.as_mut().unwrap().cell_mut(0, 0).insert(Pos::new(0, 0), &"word ".repeat(20), 0);
    sh.fit_table();
    let tt = sh.table.as_ref().unwrap();
    assert!(tt.rows[0] > tt.rows[1] * 3, "{:?}", tt.rows);
    assert_eq!(sh.h, tt.rows.iter().sum::<i32>());
}

#[test]
fn pdf_writer() {
    use crate::pdf::*;
    let px = vec![0x10u8, 0x20, 0x30].repeat(40 * 20);
    let pages = vec![
        Page { w: 960, h: 540, image: Some((0, 0, 960, 540, 40, 20, px.clone())), texts: vec![Text { x: 10, y: 30, size: 24, s: "Hello (world) \\ ₦ •".into(), w: 200, visible: false, color: 0 }] },
        Page { w: 595, h: 842, image: None, texts: vec![Text { x: 50, y: 60, size: 12, s: "Notes".into(), w: 0, visible: true, color: 0x112233 }] },
    ];
    let pdf = write("A (title)", "Ada Obi", &pages);
    assert!(pdf.starts_with(b"%PDF-1.4"));
    let find = |pat: &[u8], from: usize| pdf[from..].windows(pat.len()).position(|w| w == pat).map(|p| p + from);
    let rfind = |pat: &[u8]| pdf.windows(pat.len()).rposition(|w| w == pat);
    // every xref entry points at its object
    let xref = rfind(b"\nxref\n").unwrap() + 1;
    let sx = rfind(b"startxref\n").unwrap() + 10;
    let start: usize = String::from_utf8_lossy(&pdf[sx..]).lines().next().unwrap().parse().unwrap();
    assert_eq!(start, xref);
    let tail = String::from_utf8_lossy(&pdf[xref..]).to_string();
    let entries: Vec<&str> = tail.lines().skip(3).take_while(|l| l.ends_with(" n ")).collect();
    assert_eq!(entries.len(), 10);
    for (i, e) in entries.iter().enumerate() {
        let off: usize = e[..10].parse().unwrap();
        assert!(pdf[off..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {}", i + 1);
    }
    let _ = find;
    let text = String::from_utf8_lossy(&pdf).to_string();
    assert!(text.contains("/Count 2") && text.contains("(A \\(title\\))"));
    assert!(text.contains("/Author (Ada Obi)"));
    // the picture's pixels come back out of the stream
    let at = pdf.windows(10).position(|w| w == b"/Width 40 ").unwrap();
    let s = at + pdf[at..].windows(7).position(|w| w == b"stream\n").unwrap() + 7;
    let e = s + pdf[s..].windows(10).position(|w| w == b"\nendstream").unwrap();
    assert_eq!(crate::zip::inflate(&pdf[s + 2..e - 4], 0).unwrap(), px);
    // Helvetica widths and wrapping
    assert_eq!(text_width("Hi", 10), (722 + 222) * 10);
    let lines = wrap("one two three four five six", 12, 60);
    assert!(lines.len() > 1 && lines.iter().all(|l| text_width(l, 12) <= 60 * 1000 || !l.contains(' ')));
}

/// `HYDA_OUT=/path/features.pptx cargo test --release export_features -- --ignored`
#[test]
#[ignore]
fn export_features() {
    std::fs::write(std::env::var("HYDA_OUT").unwrap(), to_pptx(&features())).unwrap();
}
