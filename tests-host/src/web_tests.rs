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
    assert_eq!(url::from_input("lagos weather").to_string(), "hydatek://search?q=lagos+weather");
    assert_eq!(url::from_input("example.com/x").to_string(), "http://example.com/x");
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
    let req = String::from_utf8(http::request("GET", &url::Url::parse("http://a.ng:8080/x?y").unwrap(), b"", "", "")).unwrap();
    assert!(req.starts_with("GET /x?y HTTP/1.1\r\nHost: a.ng:8080\r\n"), "{}", req);
}

#[test]
fn html_parsing() {
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
