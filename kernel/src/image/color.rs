//! CSS colours: #hex, rgb()/rgba(), hsl()/hsla() and the named colours.

/// The CSS named colours.
const NAMED: [(&str, u32); 148] = [
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff), ("aquamarine", 0x7fffd4), ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc), ("bisque", 0xffe4c4), ("black", 0x000000), ("blanchedalmond", 0xffebcd), ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a), ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e), ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed), ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c),
    ("cyan", 0x00ffff), ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b), ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9), ("darkkhaki", 0xbdb76b), ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc), ("darkred", 0x8b0000), ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b), ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1), ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff), ("dimgray", 0x696969), ("dimgrey", 0x696969), ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22), ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff), ("gold", 0xffd700), ("goldenrod", 0xdaa520), ("gray", 0x808080), ("green", 0x008000),
    ("greenyellow", 0xadff2f), ("grey", 0x808080), ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c), ("lavender", 0xe6e6fa), ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00), ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6), ("lightcoral", 0xf08080), ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3), ("lightgreen", 0x90ee90), ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a), ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa), ("lightslategray", 0x778899), ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de), ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32), ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff), ("maroon", 0x800000), ("mediumaquamarine", 0x66cdaa), ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371), ("mediumslateblue", 0x7b68ee), ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585), ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1), ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead), ("navy", 0x000080), ("oldlace", 0xfdf5e6), ("olive", 0x808000), ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6), ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee), ("palevioletred", 0xdb7093), ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9), ("peru", 0xcd853f),
    ("pink", 0xffc0cb), ("plum", 0xdda0dd), ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xff0000), ("rosybrown", 0xbc8f8f), ("royalblue", 0x4169e1), ("saddlebrown", 0x8b4513), ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460), ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee), ("sienna", 0xa0522d), ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd), ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f), ("steelblue", 0x4682b4), ("tan", 0xd2b48c), ("teal", 0x008080), ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347), ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("wheat", 0xf5deb3), ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00), ("yellowgreen", 0x9acd32),
];

/// A number, or a percentage of `full`.
fn num(s: &str, full: f64) -> Option<f64> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        return p.trim().parse::<f64>().ok().map(|v| v * full / 100.0);
    }
    s.parse::<f64>().ok()
}

fn hue_to(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 0.5 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

/// Parse a colour: (0xRRGGBB, alpha 0..=255). "transparent" is (0, 0).
pub fn parse(v: &str) -> Option<(u32, u8)> {
    let v = v.trim().to_ascii_lowercase();
    if v == "transparent" {
        return Some((0, 0));
    }
    if let Some(h) = v.strip_prefix('#') {
        let d: alloc::vec::Vec<u32> = h.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
        return match d.len() {
            3 => Some(((d[0] * 17) << 16 | (d[1] * 17) << 8 | d[2] * 17, 255)),
            4 => Some(((d[0] * 17) << 16 | (d[1] * 17) << 8 | d[2] * 17, (d[3] * 17) as u8)),
            6 => Some((d.iter().fold(0, |a, x| a << 4 | x), 255)),
            8 => Some((d[..6].iter().fold(0, |a, x| a << 4 | x), (d[6] * 16 + d[7]) as u8)),
            _ => None,
        };
    }
    let func = |name: &str| -> Option<alloc::vec::Vec<&str>> {
        let args = v.strip_prefix(name)?.trim_start().strip_prefix('(')?.strip_suffix(')')?;
        Some(args.split([',', ' ', '/']).filter(|s| !s.is_empty()).collect())
    };
    let alpha = |a: Option<&&str>| -> u8 { a.and_then(|a| num(a, 1.0)).map_or(255, |a| (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u8) };
    if let Some(n) = func("rgba").or_else(|| func("rgb")) {
        if n.len() < 3 {
            return None;
        }
        let ch = |s: &str| num(s, 255.0).map(|x| (x.clamp(0.0, 255.0) + 0.5) as u32);
        return Some((ch(n[0])? << 16 | ch(n[1])? << 8 | ch(n[2])?, alpha(n.get(3))));
    }
    if let Some(n) = func("hsla").or_else(|| func("hsl")) {
        if n.len() < 3 {
            return None;
        }
        let h = n[0].trim_end_matches("deg").parse::<f64>().ok()? / 360.0;
        let h = h - (h as i64) as f64 + if h < 0.0 { 1.0 } else { 0.0 };
        let s = num(n[1], 1.0)?.clamp(0.0, 1.0);
        let l = num(n[2], 1.0)?.clamp(0.0, 1.0);
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let c = |t: f64| (hue_to(p, q, t) * 255.0 + 0.5) as u32;
        return Some((c(h + 1.0 / 3.0) << 16 | c(h) << 8 | c(h - 1.0 / 3.0), alpha(n.get(3))));
    }
    NAMED.iter().find(|(name, _)| *name == v).map(|(_, c)| (*c, 255))
}
