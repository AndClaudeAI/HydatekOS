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
