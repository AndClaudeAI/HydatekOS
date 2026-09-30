//! Personalisation: saved choices, automatic dark mode, accents from
//! wallpapers, the time of day, fitting pictures.

use crate::personal::*;

#[test]
fn choices_saved_and_loaded() {
    let mut l = Look::default();
    l.wall = Wall::Picture(String::from("/home/Pictures/Lagos at night.jpg"));
    l.fit = Fit::Tile;
    l.lock = Some(Wall::Gradient(0xF3B391, 0x6A4C93));
    l.time_of_day = true;
    l.mode = Mode::Auto;
    l.accent = Accent::FromWall;
    l.widgets = false;
    let text = l.save();
    let mut back = Look::default();
    for line in text.lines() {
        let (k, v) = line.split_once('=').unwrap();
        assert!(back.load(k, v), "{}", k);
    }
    assert_eq!(back, l);
    assert!(!back.load("volume", "40"));
    // every wallpaper round-trips; nonsense doesn't load
    for w in [Wall::Scene(Scene::Harmattan), Wall::Solid(0x1F3B4D), Wall::Gradient(1, 0xFFFFFF)] {
        assert_eq!(Wall::load(&w.save()), Some(w));
    }
    for bad in ["", "colour:#zz", "gradient:#123456", "volcano", "picture:"] {
        assert_eq!(Wall::load(bad), None, "{}", bad);
    }
    assert_eq!(Wall::Picture(String::from("/a/b/Sea.png")).name(), "Sea.png");
}

#[test]
fn automatic_dark_mode() {
    let l = Look { mode: Mode::Auto, ..Look::default() };
    assert!(l.dark_at(22 * 60));
    assert!(l.dark_at(3 * 60));
    assert!(!l.dark_at(7 * 60));
    assert!(!l.dark_at(18 * 60 + 59));
    assert!(l.dark_at(19 * 60));
    assert!(Look { mode: Mode::Dark, ..Look::default() }.dark_at(12 * 60));
    assert!(!Look::default().dark_at(23 * 60));
}

#[test]
fn accent_from_a_wallpaper() {
    // mostly grey sky with a teal sea and a little orange
    let mut px = vec![0xFF808080u32; 10_000];
    for p in px.iter_mut().skip(3000).take(4000) {
        *p = 0xFF1E8C8C;
    }
    for p in px.iter_mut().skip(8000).take(500) {
        *p = 0xFFE07020;
    }
    let d = dominant(&px);
    assert_eq!(d, 0x1E8C8C);
    let (light, dark) = accent_pair(d);
    assert!(contrast(light, 0xFFFFFF) >= 300, "{:06x}", light);
    assert!(contrast(dark, 0x23202C) >= 300, "{:06x}", dark);
    // a pale colour is darkened for white text; a dark one lightened for dark mode
    let (l2, _) = accent_pair(0xF5E0A0);
    assert!(luminance(l2) < luminance(0xF5E0A0));
    let (_, d2) = accent_pair(0x301040);
    assert!(luminance(d2) > luminance(0x301040));
    // nothing colourful: a neutral grey
    assert_eq!(dominant(&vec![0xFF777777; 100]), 0x6B6B6B);
    assert!(contrast(0x000000, 0xFFFFFF) >= 2000);
}

#[test]
fn time_of_day() {
    let (noon_top, _, _, noon_sun, noon_light) = sky(13 * 60);
    let (night_top, _, _, night_sun, night_light) = sky(1 * 60);
    assert!(luminance(noon_top) > luminance(night_top) * 4);
    assert!(noon_sun > 500 && night_sun < 0);
    assert!(noon_light > night_light);
    // dusk is warmer at the horizon than noon
    let (_, dusk_h, _, _, _) = sky(19 * 60);
    let (_, noon_h, _, _, _) = sky(13 * 60);
    assert!(rgb(dusk_h).0 - rgb(dusk_h).2 > rgb(noon_h).0 - rgb(noon_h).2);
    // it wraps round midnight without a jump
    assert_eq!(sky(1440), sky(0));
    let (a, ..) = sky(1439);
    let (b, ..) = sky(0);
    assert!((luminance(a) - luminance(b)).abs() < 5);
}

#[test]
fn pictures_fitted() {
    // a 4 × 2 picture: left half red, right half blue
    let src: Vec<u32> = (0..8).map(|i| if i % 4 < 2 { 0xFFFF0000 } else { 0xFF0000FF }).collect();
    // stretch to 8 × 8: halves stay halves
    let s = compose(&src, 4, 2, 8, 8, Fit::Stretch);
    assert_eq!(s[0] & 0xFFFFFF, 0xFF0000);
    assert_eq!(s[7] & 0xFFFFFF, 0x0000FF);
    // fill a square: the middle 2 × 2 (half red, half blue) covers it
    let f = compose(&src, 4, 2, 4, 4, Fit::Fill);
    assert_eq!(f[0] & 0xFFFFFF, 0xFF0000);
    assert_eq!(f[3] & 0xFFFFFF, 0x0000FF);
    // fit into 4 × 4: 4 × 2 in the middle, bars above and below
    let t = compose(&src, 4, 2, 4, 4, Fit::Fit);
    assert_eq!(t[4] & 0xFFFFFF, 0xFF0000);
    assert_ne!(t[0] & 0xFFFFFF, 0xFF0000);
    // centre, small: at its own size in the middle
    let c = compose(&src, 4, 2, 8, 6, Fit::Centre);
    assert_eq!(c[2 * 8 + 2] & 0xFFFFFF, 0xFF0000);
    assert_eq!(c[0] & 0xFFFFFF, c[5 * 8 + 7] & 0xFFFFFF);
    // tile repeats
    let tl = compose(&src, 4, 2, 8, 4, Fit::Tile);
    assert_eq!(tl[4], tl[0]);
    assert_eq!(tl[2 * 8], tl[0]);
    // shrinking averages; nothing odd for empty input
    let r = resample(&[0xFF000000, 0xFFFFFFFF], 2, 1, 1, 1);
    assert_eq!(r[0] & 0xFFFFFF, 0x7F7F7F);
    assert_eq!(compose(&[], 0, 0, 2, 2, Fit::Fill).len(), 4);
}

fn hue_of(c: u32) -> i32 {
    let (r, g, b) = rgb(c);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = (mx - mn).max(1);
    (if mx == r { 60 * (g - b) / d } else if mx == g { 60 * (b - r) / d + 120 } else { 60 * (r - g) / d + 240 }).rem_euclid(360)
}

#[test]
fn photo_saved_and_loaded() {
    let w = Wall::Photo(Photo::Summit);
    assert_eq!(w.save(), "photo:summit");
    assert_eq!(Wall::load("photo:summit"), Some(w.clone()));
    assert_eq!(Wall::load("photo:nowhere"), None);
    assert_eq!(w.name(), "Summit");
}

#[test]
fn summit_palette_is_blue_sky_and_gold() {
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../kernel/assets/wallpapers/summit.jpg")).unwrap();
    let img = crate::image::decode(&data).unwrap();
    let small = resample(&img.px, img.w as usize, img.h as usize, 160, 90);
    let p = palette(&small);
    let tint = p.tint.expect("a colourful photo has a tint");
    // the sky: blue
    assert!((195..240).contains(&hue_of(tint)), "tint {:06x}", tint);
    // the sunset: gold to orange, and different from the tint
    let (light, dark) = p.accent;
    for a in [light, dark] {
        assert!((15..55).contains(&hue_of(a)), "accent {:06x}", a);
    }
    // readable: white text on the light accent, the dark accent on dark surfaces
    assert!(contrast(light, 0xFFFFFF) >= 300);
    assert!(contrast(dark, 0x23202C) >= 300);
}

#[test]
fn palette_of_one_colour_and_of_none() {
    // a single blue: tint and accent are the same colour
    let p = palette(&vec![0xFF2F6690; 1000]);
    assert_eq!(p.tint, Some(0x2F6690));
    assert!((195..225).contains(&hue_of(p.accent.1)));
    // a little orange on a lot of blue still gives an orange accent
    let mut px = vec![0xFF2F6690u32; 900];
    px.extend(vec![0xFFF08A24u32; 100]);
    let p = palette(&px);
    assert!((20..40).contains(&hue_of(p.accent.0)), "{:06x}", p.accent.0);
    // a speck of it isn't enough
    let mut px = vec![0xFF2F6690u32; 995];
    px.extend(vec![0xFFF08A24u32; 5]);
    assert_eq!(palette(&px).accent, accent_pair(0x2F6690));
    // greys: no tint
    assert_eq!(palette(&vec![0xFF808080; 100]).tint, None);
}

#[test]
fn fill_keeps_its_focus() {
    // 100 × 10, each pixel numbered by its column
    let src: Vec<u32> = (0..1000).map(|i| 0xFF000000 | (i % 100)).collect();
    // a tall 2 × 10 screen takes a slice 2 columns wide, around 80%
    let out = compose_at(&src, 100, 10, 2, 10, Fit::Fill, 800);
    assert!(out.iter().all(|p| (79..=80).contains(&(p & 0xFF))), "{:?}", &out[..2]);
    // at the edge it stops at the picture's side
    let out = compose_at(&src, 100, 10, 2, 10, Fit::Fill, 1000);
    assert!(out.iter().all(|p| (98..=99).contains(&(p & 0xFF))));
    // the middle, as before
    assert_eq!(compose(&src, 100, 10, 2, 10, Fit::Fill), compose_at(&src, 100, 10, 2, 10, Fit::Fill, 500));
}

#[test]
fn every_photo_loads_and_has_colours() {
    for p in Photo::ALL {
        let w = Wall::Photo(p);
        assert_eq!(Wall::load(&w.save()), Some(w.clone()), "{}", p.id());
        assert!(p.focus() <= 1000);
        let path = format!("{}/../kernel/assets/wallpapers/{}.jpg", env!("CARGO_MANIFEST_DIR"), p.id());
        let img = crate::image::decode(&std::fs::read(&path).unwrap()).unwrap();
        assert!(img.w >= 500 && img.h >= 300, "{} is {}×{}", p.id(), img.w, img.h);
        let pal = palette(&resample(&img.px, img.w as usize, img.h as usize, 160, 90));
        assert!(pal.tint.is_some(), "{} has no colour", p.id());
        assert!(contrast(pal.accent.0, 0xFFFFFF) >= 300 && contrast(pal.accent.1, 0x23202C) >= 300, "{}", p.id());
    }
    let mut ids: Vec<_> = Photo::ALL.iter().map(|p| p.id()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), Photo::ALL.len());
}

#[test]
fn tones_keep_the_hue() {
    let blue = 0x244D7C; // Summit's sky
    // light, dark and in between: the same hue, the asked lightness
    for (s, l) in [(170, 972), (420, 130), (270, 125), (300, 500)] {
        let c = tone(blue, s, l);
        assert!((hue_of(c) - hue_of(blue)).abs() <= 6, "{:06x} for {} {}", c, s, l);
        let (_, got) = sat_light(c);
        assert!((got - l).abs() <= 8, "{:06x}: lightness {} not {}", c, got, l);
    }
    // the extremes
    assert_eq!(tone(blue, 500, 1000), 0xFFFFFF);
    assert_eq!(tone(blue, 500, 0), 0x000000);
    assert_eq!(tone(blue, 0, 500) & 0xFF, (tone(blue, 0, 500) >> 16) & 0xFF); // grey
    // saturation and lightness of a known colour: pure red is 1000, 500
    assert_eq!(sat_light(0xFF0000), (1000, 500));
}

#[test]
fn readable_ink() {
    // pale ink on a pale surface is darkened until it reads
    let c = readable(0xC8C0B8, 0xF7F4EE, 450);
    assert!(contrast(c, 0xF7F4EE) >= 450);
    // dim ink on a dark surface is lightened
    let c = readable(0x403848, 0x1E1C28, 450);
    assert!(contrast(c, 0x1E1C28) >= 450);
    // ink that already reads is left alone
    assert_eq!(readable(0x1E1B2C, 0xFFFFFF, 450), 0x1E1B2C);
}

#[test]
fn dynamic_colour_is_the_default() {
    assert_eq!(Look::default().accent, Accent::FromWall);
}

#[test]
fn dark_colours_are_not_accents() {
    // sand and terracotta dunes over a band of dark navy shadow
    let mut px = vec![0xFFDCC8ABu32; 400];
    px.extend(vec![0xFFC4895Eu32; 300]);
    px.extend(vec![0xFF2B2A48u32; 300]);
    let p = palette(&px);
    let h = hue_of(p.accent.0);
    assert!((15..45).contains(&h), "the accent is warm, not navy: {:06x}", p.accent.0);
}
