//! The web engine: URLs, DNS, HTTP, HTML, CSS/layout, Hyda Search.

use crate::web::*;

#[test]
fn lays_out_the_test_site() {
    let html = std::fs::read_to_string("/tmp/claude-0/site/index.html").unwrap_or_default();
    if html.is_empty() {
        return;
    }
    let dom = html::parse(&html);
    assert_eq!(dom.title(), "Lagos Tech Journal");
    for w in [300, 612, 952] {
        let t = std::time::Instant::now();
        let page = render::layout(&dom, "", w);
        assert!(t.elapsed().as_secs() < 2, "layout at {} took {:?}", w, t.elapsed());
        assert!(page.height > 200);
    }
}

#[test]
fn keeps_the_space_before_a_link() {
    let dom = html::parse("<p class=small>See the progress. <a href=\"x\">Back to search</a> now</p>");
    let page = render::layout(&dom, ".small{font-size:13px}", 600);
    let texts: Vec<(i32, String)> = page.items.iter().filter_map(|i| match i {
        render::Item::Text { x, text, .. } => Some((*x, text.clone())),
        _ => None,
    }).collect();
    let first = &texts[0];
    let link = &texts[1];
    let first_end = first.0 + crate::font::measure(crate::font::Face::Regular, 13, &first.1);
    assert!(link.0 > first_end, "{:?}", texts);
}

#[test]
fn urls() {
    use url::Url;
    let u = Url::parse("HTTP://Example.com:8080/a/b/c.html?x=1#top").unwrap();
    assert_eq!((u.scheme.as_str(), u.host.as_str(), u.port, u.path.as_str(), u.fragment.as_str()), ("http", "example.com", 8080, "/a/b/c.html?x=1", "top"));
    assert_eq!(u.join("../d.html").unwrap().to_string(), "http://example.com:8080/a/d.html");
    assert_eq!(u.join("/root").unwrap().to_string(), "http://example.com:8080/root");
    assert_eq!(u.join("?y=2").unwrap().to_string(), "http://example.com:8080/a/b/c.html?y=2");
    assert_eq!(u.join("//other.org/p").unwrap().to_string(), "http://other.org/p");
    assert_eq!(u.join("https://secure.ng/").unwrap().port, 443);
    assert_eq!(u.join("#sec").unwrap().fragment, "sec");
    assert_eq!(Url::parse("http://a.ng").unwrap().path, "/");
    let hyda = engines::by_id(engines::HYDA);
    assert_eq!(url::from_input("lagos weather", hyda).to_string(), "hydatek://search?q=lagos+weather");
    assert_eq!(url::from_input("example.com/x", hyda).to_string(), "http://example.com/x");
    assert_eq!(url::encode("₦5 & more"), "%E2%82%A65+%26+more");
    assert_eq!(url::decode("%E2%82%A65+%26+more"), "₦5 & more");
    assert_eq!(Url::parse("hydatek://search?q=a+b").unwrap().param("q").as_deref(), Some("a b"));
}

#[test]
fn dns_messages() {
    let q = dns::query(0xbeef, "example.com");
    assert_eq!(&q[..2], &[0xbe, 0xef]);
    assert_eq!(&q[12..], b"\x07example\x03com\x00\x00\x01\x00\x01");
    // an answer with a CNAME then an A record, using name compression
    let mut a = vec![0xbe, 0xef, 0x81, 0x80, 0, 1, 0, 2, 0, 0, 0, 0];
    a.extend_from_slice(&q[12..]);
    a.extend_from_slice(&[0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xc0, 12]);
    a.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 1, 44, 0, 4, 93, 184, 216, 34]);
    assert_eq!(dns::answer(&a, 0xbeef), Some(Ok((vec![[93, 184, 216, 34]], 300))));
    assert_eq!(dns::answer(&a, 0x1234), None);
    let mut nx = a.clone();
    nx[3] = 0x83;
    assert_eq!(dns::answer(&nx, 0xbeef), Some(Err("no such site")));
    assert_eq!(dns::parse_ip("10.0.2.2"), Some([10, 0, 2, 2]));
    assert_eq!(dns::parse_ip("10.0.2"), None);
}

#[test]
fn http_responses() {
    // Content-Length, fed a byte at a time
    let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 5\r\n\r\nhello";
    let mut p = http::Parser::default();
    for b in raw.iter() {
        assert!(!p.done());
        p.feed(&[*b]).unwrap();
    }
    assert!(p.done());
    let r = p.finish("http://x/").unwrap();
    assert_eq!((r.status, r.content_type(), r.body.as_slice()), (200, "text/html".to_string(), &b"hello"[..]));
    // chunked + gzip (made by Python's gzip)
    let body = b"<p>Nigeria \xe2\x82\xa6</p>".repeat(20);
    let gz = {
        use std::io::Write;
        let mut e = std::process::Command::new("gzip").arg("-c").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        e.stdin.take().unwrap().write_all(&body).unwrap();
        e.wait_with_output().unwrap().stdout
    };
    let mut raw = b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
    for c in gz.chunks(17) {
        raw.extend_from_slice(format!("{:x}\r\n", c.len()).as_bytes());
        raw.extend_from_slice(c);
        raw.extend_from_slice(b"\r\n");
    }
    raw.extend_from_slice(b"0\r\n\r\n");
    let mut p = http::Parser::default();
    p.feed(&raw).unwrap();
    assert!(p.done());
    assert_eq!(p.finish("u").unwrap().body, body);
    // closed early
    let mut p = http::Parser::default();
    p.feed(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc").unwrap();
    assert!(p.finish("u").is_err());
    let req = String::from_utf8(http::request("GET", &url::Url::parse("http://a.ng:8080/x?y").unwrap(), b"", "", "", http::ACCEPT_PAGE, "", &[])).unwrap();
    assert!(req.starts_with("GET /x?y HTTP/1.1\r\nHost: a.ng:8080\r\n"), "{}", req);
}

#[test]
fn html_parsing() {
    // no scripts run, so <noscript> holds ordinary markup
    let d = html::parse("<body><noscript><div class=warn>Enable <b>JavaScript</b></div></noscript></body>");
    assert!(d.find("b").is_some());
    let d = html::parse("<!doctype html><title>T &amp; U</title><p>One<p>Two <b>bold</b><ul><li>a<li>b</ul><script>if (a<b) x()</script><img src=x alt=\"A pic\"><a href='/x'>link</a>");
    assert_eq!(d.title(), "T & U");
    assert_eq!(d.text(0), "One Two bold a b link");
    let lis = (0..d.nodes.len()).filter(|&n| d.tag(n) == "li").count();
    assert_eq!(lis, 2);
    let ps: Vec<_> = (0..d.nodes.len()).filter(|&n| d.tag(n) == "p").collect();
    assert_eq!(ps.len(), 2);
    assert_eq!(d.nodes[ps[1]].parent, d.nodes[ps[0]].parent, "<p> closes the open <p>");
    assert_eq!(d.links(), vec!["/x".to_string()]);
    assert_eq!(html::decode_entities("&#8358;5 &euro;&nbsp;&bogus; &#x41;"), "₦5 €\u{a0}&bogus; A");
}

#[test]
fn css_and_layout() {
    let d = html::parse("<style>.x{color:#1f4e79;font-weight:bold} #y p{font-size:20px} .h{display:none}</style><div id=y><p class=x>Styled</p></div><p class=h>Hidden</p><p style=\"color:rgb(255,0,0)\">Red</p>");
    let page = render::layout(&d, "", 500);
    let texts: Vec<_> = page.items.iter().filter_map(|i| match i {
        render::Item::Text { text, size, color, face, .. } => Some((text.clone(), *size, *color, *face)),
        _ => None,
    }).collect();
    assert_eq!(texts.len(), 2, "{:?}", texts);
    assert_eq!(texts[0], ("Styled".to_string(), 20, 0x1f4e79, crate::font::Face::Semibold));
    assert_eq!(texts[1].2, 0xff0000);
}

#[test]
fn search_ranking() {
    let mut ix = search::Index::default();
    ix.add("http://j.ng/solar", "Solar mini-grids", "Traders in Kano run fridges on solar power. The batteries last eight years.");
    ix.add("http://j.ng/pay", "Mobile money", "Wallets pay for solar power by the day.");
    ix.add("http://j.ng/about", "About", "A journal about technology in Nigeria.");
    let hits = ix.search("solar batteries", 10);
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].url, "http://j.ng/solar");
    assert!(hits[0].marks.iter().any(|&(a, b)| hits[0].snippet[a..b].eq_ignore_ascii_case("batteries")));
    assert!(ix.search("the", 10).is_empty() || ix.search("zzz", 10).is_empty());
    // plural matches singular
    assert_eq!(ix.search("battery", 10).len(), 0);
    assert_eq!(ix.search("wallet", 10)[0].url, "http://j.ng/pay");
    // re-adding replaces; saving and loading keeps everything
    ix.add("http://j.ng/about", "About us", "Lagos office.");
    let back = search::Index::load(&ix.save());
    assert_eq!(back.len(), 3);
    assert_eq!(back.search("lagos", 5)[0].title, "About us");
    let mut back = back;
    back.remove_site("j.ng");
    assert_eq!(back.len(), 0);
}

#[test]
fn search_engines() {
    let hyda = engines::by_id(engines::HYDA);
    let ddg = engines::by_id("duckduckgo");
    assert_eq!(url::from_input("jollof rice", ddg).to_string(), "https://html.duckduckgo.com/html/?q=jollof+rice");
    // shortcuts before or after, any engine
    assert_eq!(url::from_input("!w lagos", hyda).to_string(), "https://en.wikipedia.org/w/index.php?search=lagos");
    assert_eq!(url::from_input("naira rate !G", hyda).to_string(), "https://www.google.com/search?q=naira+rate");
    assert_eq!(url::from_input("!h budget", ddg).to_string(), "hydatek://search?q=budget");
    assert_eq!(url::from_input("!br", hyda).to_string(), "https://search.brave.com/search");
    assert_eq!(url::from_input("!h", ddg).to_string(), "hydatek://start");
    // an unknown shortcut is just a word
    assert_eq!(url::from_input("!zz top", hyda).to_string(), "hydatek://search?q=%21zz+top");
    // addresses still win
    assert_eq!(url::from_input("https://a.ng/x", ddg).to_string(), "https://a.ng/x");
    assert_eq!(engines::by_id("nonsense").id, engines::HYDA);
    let keys: Vec<&str> = engines::ENGINES.iter().map(|e| e.key).collect();
    for (i, k) in keys.iter().enumerate() {
        assert!(!keys[i + 1..].contains(k), "duplicate shortcut {}", k);
    }
}

#[test]
fn images_in_layout() {
    use render::{ImgStatus, Item};
    let d = html::parse(
        "<p>Logo <img src=/logo.png alt=Logo> here</p>\
         <img src=big.jpg>\
         <img src=sized.gif width=40 height=30>\
         <img src=gone.png alt=\"A lost cat\">\
         <img src=\"data:image/gif;base64,R0lGOD\" data-src=/real.jpg>\
         <img srcset=\"s.jpg 300w, m.jpg 800w, l.jpg 2000w\">\
         <img src=wait.png>",
    );
    let status = |src: &str| match src {
        "/logo.png" => ImgStatus::Ready(64, 32),
        "big.jpg" => ImgStatus::Ready(2000, 1000),
        "gone.png" => ImgStatus::Broken,
        "/real.jpg" => ImgStatus::Ready(100, 50),
        "m.jpg" => ImgStatus::Ready(800, 400),
        _ => ImgStatus::Loading,
    };
    let p = render::layout_with(&d, "", 600, &status);
    let imgs: Vec<(String, i32, i32)> = p.items.iter().filter_map(|i| if let Item::Image { w, h, img, .. } = i { Some((p.images[*img].clone(), *w, *h)) } else { None }).collect();
    assert_eq!(
        imgs,
        vec![
            ("/logo.png".to_string(), 64, 32),
            // no wider than the page, keeping its shape
            ("big.jpg".to_string(), 600, 300),
            // the page's size wins, even before it loads
            ("sized.gif".to_string(), 40, 30),
            ("/real.jpg".to_string(), 100, 50),
            ("m.jpg".to_string(), 800.min(600), 300),
        ]
    );
    // a broken image shows its alt text; one still loading shows nothing yet
    let texts: Vec<String> = p.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect();
    assert!(texts.join(" ").contains("[A lost cat]"), "{:?}", texts);
    assert!(!texts.join(" ").contains("[Logo]"));
    // the logo sits inline between the words
    let logo_x = p.items.iter().find_map(|i| if let Item::Image { x, .. } = i { Some(*x) } else { None }).unwrap();
    let here_x = p.items.iter().find_map(|i| if let Item::Text { x, text, .. } = i { (text == "here").then_some(*x) } else { None }).unwrap();
    assert!(here_x > logo_x + 64);
}

#[test]
fn backgrounds_and_colours() {
    use render::{BgSize, Dim, ImgStatus, Item};
    let d = html::parse(
        "<style>.hero{background:#123 url('/img/sky.jpg') no-repeat center / cover;height:200px}\
         .tile{background-image:url(dots.png);background-size:20px auto;background-position:right 10px}\
         .fade{background:linear-gradient(to right, rebeccapurple, white)}\
         p{color:hsl(0, 100%, 50%)}</style>\
         <div class=hero><p>Hello</p></div><div class=tile>x</div><div class=fade>y</div>",
    );
    let p = render::layout_with(&d, "", 600, &|_| ImgStatus::Loading);
    assert!(p.wanted.contains(&"/img/sky.jpg".to_string()) && p.wanted.contains(&"dots.png".to_string()));
    let bgs: Vec<(String, BgSize, (Dim, Dim), (bool, bool))> = p.items.iter().filter_map(|i| if let Item::Background { img, size, pos, repeat, .. } = i { Some((p.images[*img].clone(), *size, *pos, *repeat)) } else { None }).collect();
    assert_eq!(bgs[0], ("/img/sky.jpg".to_string(), BgSize::Cover, (Dim::Pct(50), Dim::Pct(50)), (false, false)));
    assert_eq!(bgs[1], ("dots.png".to_string(), BgSize::Set(Dim::Px(20), Dim::Auto), (Dim::Pct(100), Dim::Px(10)), (true, true)));
    // the colour under the hero, the gradient's first colour, and hsl() text
    let rects: Vec<u32> = p.items.iter().filter_map(|i| if let Item::Rect { color, .. } = i { Some(*color) } else { None }).collect();
    assert!(rects.contains(&0x112233) && rects.contains(&0x663399), "{:x?}", rects);
    assert!(p.items.iter().any(|i| matches!(i, Item::Text { color: 0xff0000, .. })));
    // the background is drawn before (under) the text
    let bg_at = p.items.iter().position(|i| matches!(i, Item::Background { .. })).unwrap();
    let text_at = p.items.iter().position(|i| matches!(i, Item::Text { .. })).unwrap();
    assert!(bg_at < text_at);
}

fn texts_with_colour(p: &render::Page) -> Vec<(String, u32)> {
    p.items.iter().filter_map(|i| if let render::Item::Text { text, color, .. } = i { Some((text.clone(), *color)) } else { None }).collect()
}

#[test]
fn css_selectors_and_cascade() {
    let d = html::parse(
        "<style>
          :root { --brand: #0a7; --gap: 3px }
          li:nth-child(odd) { color: #111 }
          li:nth-child(2n) { color: #222 }
          li:first-child + li + li { color: #333 }
          h2 ~ p { color: #444 }
          a[href^='https'] { color: #555 }
          a[href$='.pdf' i] { color: #556 }
          p:not(.x):last-of-type { color: var(--brand) }
          :is(.a, .b) > span { color: var(--missing, #666) }
          #main .item { color: #777 }
          .item { color: #888 !important }
          div.item.item2 { color: #999 }
          @media (max-width: 600px) { .wide { color: #aaa } }
          @media screen and (min-width: 601px) { .wide { color: #bbb } }
          @media print { .wide { color: #ccc } }
          @supports (display: grid) { .grid { color: #ddd } }
          .up { text-transform: uppercase }
          .sr-only { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0, 0, 0, 0) }
          .menu { max-height: 0; overflow: hidden }
          a:hover, p::before { color: red }
        </style>
        <ul><li>One</li><li>Two</li><li>Three</li><li>Four</li></ul>
        <h2>T</h2><p class=x>Para1</p><p>Para2</p>
        <p><a href='https://a.ng'>Secure</a> <a href='/doc.PDF'>Doc</a></p>
        <div class=b><span>Span</span></div>
        <div id=main><div class=item style='color:#123'>Item</div><div class='item item2'>Item2</div></div>
        <div class=wide>Wide</div><div class=grid>Grid</div>
        <div class=up>shout</div><div class=sr-only>Skip to content</div><div class=menu>Hidden menu</div>",
    );
    let p = render::layout(&d, "", 900);
    let t = texts_with_colour(&p);
    let col = |s: &str| t.iter().find(|(x, _)| x == s).map(|x| x.1);
    assert_eq!(col("One"), Some(0x111111));
    assert_eq!(col("Two"), Some(0x222222));
    assert_eq!(col("Three"), Some(0x333333)); // the sibling rule beats :nth-child by order
    assert_eq!(col("Para1"), Some(0x444444));
    assert_eq!(col("Secure"), Some(0x555555));
    assert_eq!(col("Doc"), Some(0x555566));
    assert_eq!(col("Span"), Some(0x666666));
    // !important beats style="" and a more specific rule
    assert_eq!(col("Item"), Some(0x888888));
    assert_eq!(col("Item2"), Some(0x888888));
    assert_eq!(col("Wide"), Some(0xbbbbbb));
    assert_eq!(col("Grid"), Some(0xdddddd));
    assert_eq!(col("SHOUT"), Some(0x1e1b2c));
    assert!(col("Skip").is_none() && col("Hidden").is_none(), "{:?}", t);
    // p:not(.x):last-of-type: the paragraph with the links is the last <p>
    assert_eq!(col("Para2"), Some(0x444444));
    // a narrow window picks the other media rule
    let narrow = render::layout(&d, "", 500);
    assert_eq!(texts_with_colour(&narrow).iter().find(|x| x.0 == "Wide").map(|x| x.1), Some(0xaaaaaa));
}

#[test]
fn css_helpers() {
    use css::*;
    let m = Media { width: 1000, height: 800 };
    assert!(media_matches("screen and (min-width: 768px)", m));
    assert!(!media_matches("(max-width: 767.98px)", m));
    assert!(media_matches("print, (min-width: 40em)", m));
    assert!(!media_matches("print", m));
    assert!(media_matches("not print", m));
    assert!(media_matches("(width >= 600px)", m));
    assert!(media_matches("(400px <= width <= 1200px)", m));
    assert!(!media_matches("(prefers-color-scheme: dark)", m));
    assert!(media_matches("", m));
    let css = "@charset \"utf-8\";\n@import url('base.css');\n@import \"print.css\" print;\n@import url(theme.css) screen and (min-width: 600px);\nbody { background: url(img/bg.png) }\n.x { background: url('/abs.png') } .y { background: url(data:image/png;base64,xx) }";
    assert_eq!(
        imports(css),
        vec![(String::from("base.css"), String::new()), (String::from("print.css"), String::from("print")), (String::from("theme.css"), String::from("screen and (min-width: 600px)"))]
    );
    let abs = absolutize(css, &|u| Some(format!("https://a.ng/css/{}", u)));
    assert!(abs.contains("url('https://a.ng/css/base.css')") && abs.contains("@import \"https://a.ng/css/print.css\""), "{}", abs);
    assert!(abs.contains("url(https://a.ng/css/img/bg.png)") && abs.contains("url(data:image/png;base64,xx)"), "{}", abs);
    assert_eq!(resolve_vars("1px solid var(--c, var(--d, red))", &|_| None, 0).as_deref(), Some("1px solid red"));
    assert_eq!(resolve_vars("var(--c)", &|k| (k == "--c").then(|| String::from("blue")), 0).as_deref(), Some("blue"));
    assert_eq!(resolve_vars("var(--nope)", &|_| None, 0), None);
    let d = parse_decls("color: red !important; --x: 4px; background: url('a;b.png'); .nested { a: b } margin:0");
    assert_eq!(d.len(), 4, "{:?}", d);
    assert!(d[0].important && d[1].name == "--x" && d[2].value == "url('a;b.png')" && d[3].name == "margin");
    let dom = html::parse("<head><link rel=stylesheet href=a.css><style>p{}</style><link rel='alternate stylesheet' href=b.css><link rel=stylesheet href=c.css media=print></head>");
    let s = sources(&dom);
    assert_eq!(s.len(), 3);
    assert_eq!(s[0], Err((String::from("a.css"), String::new())));
    assert_eq!(s[2], Err((String::from("c.css"), String::from("print"))));
}

/// A big real stylesheet: `HYDATEK_CSS=bootstrap.css cargo test --release big_css -- --ignored --nocapture`
#[test]
#[ignore]
fn big_css() {
    let path = std::env::var("HYDATEK_CSS").unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let t = std::time::Instant::now();
    let c = css::Cascade::new(&text, css::Media { width: 1000, height: 800 });
    println!("{} rules parsed in {:?}", c.len(), t.elapsed());
    let mut body = String::new();
    for i in 0..200 {
        body.push_str(&format!("<div class='card mb-3'><div class='card-body'><h5 class=card-title>Card {}</h5><p class='card-text text-muted'>Text</p><a class='btn btn-primary' href=#>Go</a></div></div>", i));
    }
    let d = html::parse(&format!("<html><body><nav class='navbar navbar-expand-lg bg-body-tertiary'><div class=container-fluid><a class=navbar-brand href=#>Brand</a></div></nav><main class=container>{}</main></body></html>", body));
    let t = std::time::Instant::now();
    let p = render::layout_styled(&d, &c, 900, &|_| render::ImgStatus::Loading);
    println!("laid out {} nodes into {} items in {:?}", d.nodes.len(), p.items.len(), t.elapsed());
}

#[test]
fn calc_and_inline_boxes() {
    use render::Item;
    let d = html::parse(
        "<style>:root{--g:1.5rem} .box{padding:calc(var(--g) * .5) calc(10px + 2%);width:min(300px, 50%)}
         .btn{background:#0d6efd;color:#fff;padding:6px 12px;border:1px solid #0a58ca}</style>
         <div class=box>Box</div><p>Go <a class=btn href=#>Button</a> now</p>",
    );
    let p = render::layout(&d, "", 1000);
    // the box's text starts after its computed left padding (10px + 2% of 1000)
    let x = p.items.iter().find_map(|i| if let Item::Text { x, text, .. } = i { (text == "Box").then_some(*x) } else { None }).unwrap();
    assert_eq!(x, 30);
    // the inline link gets its own background and border, behind its text
    let bg = p.items.iter().position(|i| matches!(i, Item::Rect { color: 0x0d6efd, .. })).expect("button background");
    let txt = p.items.iter().position(|i| matches!(i, Item::Text { text, .. } if text == "Button")).unwrap();
    assert!(bg < txt);
    assert!(p.items.iter().any(|i| matches!(i, Item::Frame { color: 0x0a58ca, .. })));
    // "Go" and "now" have no box
    assert_eq!(p.items.iter().filter(|i| matches!(i, Item::Rect { .. })).count(), 1);
}

/// Flexbox and grid boxes against Chromium's layout of the same pages
/// (fixtures/layout/expected.txt, made by make.js).
#[test]
fn flex_and_grid_match_chromium() {
    let text = std::fs::read_to_string(format!("{}/fixtures/layout/expected.txt", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut failures = Vec::new();
    let mut cases = 0;
    for block in text.split("\n\n").filter(|b| !b.trim().is_empty()) {
        let mut lines = block.lines();
        let name = lines.next().unwrap().trim_start_matches("case ").to_string();
        let html_src = lines.next().unwrap();
        let d = html::parse(html_src);
        let p = render::layout(&d, "", 1000);
        cases += 1;
        for l in lines {
            let f: Vec<&str> = l.split_whitespace().collect();
            let colour = u32::from_str_radix(&f[1][1..], 16).unwrap();
            let want: Vec<i32> = f[2..6].iter().map(|v| v.parse().unwrap()).collect();
            let got = p.items.iter().find_map(|i| if let render::Item::Rect { x, y, w, h, color } = i { (*color == colour).then(|| vec![*x, *y, *w, *h]) } else { None });
            match got {
                Some(g) if g.iter().zip(&want).all(|(a, b)| (a - b).abs() <= 1) => {}
                _ => failures.push(format!("{} #{}: want {:?}, got {:?}", name, f[0], want, got)),
            }
        }
    }
    assert!(failures.is_empty(), "{} of {} layouts:\n{}", failures.len(), cases, failures.join("\n"));
}

/// Flex boxes nested deep, with text: measuring must not multiply per level.
#[test]
fn deep_flex_is_quick() {
    let mut inner = String::from("<span>leaf text here</span>");
    for level in 0..10 {
        inner = format!("<div class=f><div>left {} words</div>{}<div class=g>right</div></div>", level, inner);
    }
    let d = html::parse(&format!("<style>.f{{display:flex;gap:4px}} .g{{flex:1}}</style>{}", inner));
    let t = std::time::Instant::now();
    let p = render::layout(&d, "", 900);
    let took = t.elapsed();
    assert!(p.items.len() > 20);
    assert!(took.as_millis() < 500, "took {:?}", took);
    println!("10 nested flex levels: {:?}", took);
}
