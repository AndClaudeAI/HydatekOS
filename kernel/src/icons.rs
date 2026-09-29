//! Vector line icons (24-unit design grid, 2-unit strokes) rasterised with
//! 4x4 integer supersampling and cached per pixel size.

use crate::gfx::{sin_q14, Canvas, Color};
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::UnsafeCell;

#[allow(dead_code)] // the full icon set; not every glyph is used yet
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Icon {
    Grid,
    Folder,
    Globe,
    Chat,
    Mail,
    Calendar,
    Doc,
    Music,
    Sliders,
    Search,
    Wifi,
    Battery,
    Phone,
    Camera,
    Image,
    Sheet,
    ChevronLeft,
    ChevronRight,
    Close,
    Minimize,
    Maximize,
    Plus,
    Trash,
    Send,
    Play,
    Pause,
    SkipNext,
    SkipPrev,
    Terminal,
    Link,
    Monitor,
    Power,
    Bluetooth,
    Home,
    User,
    Info,
    Check,
    Laptop,
    Bell,
    Save,
    Keypad,
    Key,
    Fingerprint,
    Keyboard,
    Shift,
    Backspace,
    AlignLeft,
    AlignCenter,
    AlignRight,
    ListBullet,
    ListNumber,
    Undo,
    Redo,
    Scripts,
    Lock,
    Slides,
    TextBox,
    Shapes,
    Table,
    Chart,
    Spark,
    Speaker,
    SpeakerOff,
    Sun,
}

enum P {
    Poly(&'static [(i16, i16)]),
    Circle(i16, i16, i16),
    Arc(i16, i16, i16, i16, i16), // cx, cy, r, from deg, to deg (clockwise on screen)
    Ellipse(i16, i16, i16, i16),
    RRect(i16, i16, i16, i16, i16),
    Dot(i16, i16, i16),
    Fill(i16, i16, i16, i16),
}
use P::*;

// Coordinates are in tenths of a design unit.
fn shape(i: Icon) -> &'static [P] {
    match i {
        Icon::Grid => &[RRect(30, 30, 70, 70, 10), RRect(140, 30, 70, 70, 10), RRect(30, 140, 70, 70, 10), RRect(140, 140, 70, 70, 10)],
        Icon::Folder => &[Poly(&[(40, 200), (200, 200), (220, 180), (220, 80), (200, 60), (125, 60), (100, 30), (40, 30), (20, 50), (20, 180), (40, 200)])],
        Icon::Globe => &[Circle(120, 120, 100), Poly(&[(20, 120), (220, 120)]), Ellipse(120, 120, 42, 100)],
        Icon::Chat => &[Poly(&[(30, 210), (30, 50), (50, 30), (190, 30), (210, 50), (210, 150), (190, 170), (70, 170), (30, 210)])],
        Icon::Mail => &[RRect(20, 40, 200, 160, 20), Poly(&[(220, 70), (120, 130), (20, 70)])],
        Icon::Calendar => &[RRect(30, 40, 180, 180, 20), Poly(&[(160, 20), (160, 60)]), Poly(&[(80, 20), (80, 60)]), Poly(&[(30, 100), (210, 100)])],
        Icon::Doc => &[
            Poly(&[(150, 20), (60, 20), (40, 40), (40, 200), (60, 220), (180, 220), (200, 200), (200, 70), (150, 20)]),
            Poly(&[(140, 20), (140, 80), (200, 80)]),
            Poly(&[(85, 130), (155, 130)]),
            Poly(&[(85, 170), (155, 170)]),
            Poly(&[(85, 90), (100, 90)]),
        ],
        Icon::Music => &[Poly(&[(90, 180), (90, 50), (210, 30), (210, 160)]), Circle(60, 180, 30), Circle(180, 160, 30)],
        Icon::Sliders => &[
            Poly(&[(30, 75), (125, 75)]),
            Poly(&[(175, 75), (210, 75)]),
            Circle(150, 75, 25),
            Poly(&[(30, 165), (65, 165)]),
            Poly(&[(115, 165), (210, 165)]),
            Circle(90, 165, 25),
        ],
        Icon::Search => &[Circle(110, 110, 70), Poly(&[(162, 162), (210, 210)])],
        Icon::Wifi => &[Arc(120, 200, 150, 222, 318), Arc(120, 200, 100, 225, 315), Arc(120, 200, 50, 225, 315), Dot(120, 200, 14)],
        Icon::Battery => &[RRect(20, 70, 170, 100, 20), Poly(&[(215, 105), (215, 135)]), Fill(45, 95, 105, 50)],
        Icon::Phone => &[Poly(&[
            (50, 30), (85, 30), (100, 75), (75, 95), (95, 130), (145, 165), (165, 140), (210, 155), (210, 190), (190, 210),
            (160, 210), (110, 190), (65, 150), (40, 105), (30, 70), (30, 50), (50, 30),
        ])],
        Icon::Camera => &[
            Poly(&[(40, 70), (70, 70), (90, 40), (150, 40), (170, 70), (200, 70), (220, 90), (220, 180), (200, 200), (40, 200), (20, 180), (20, 90), (40, 70)]),
            Circle(120, 132, 38),
        ],
        Icon::Image => &[RRect(30, 30, 180, 180, 20), Circle(90, 90, 20), Poly(&[(210, 150), (160, 100), (50, 210)])],
        Icon::Sheet => &[RRect(30, 30, 180, 180, 20), Poly(&[(30, 90), (210, 90)]), Poly(&[(30, 150), (210, 150)]), Poly(&[(90, 30), (90, 210)])],
        Icon::ChevronLeft => &[Poly(&[(150, 50), (80, 120), (150, 190)])],
        Icon::ChevronRight => &[Poly(&[(90, 50), (160, 120), (90, 190)])],
        Icon::Close => &[Poly(&[(60, 60), (180, 180)]), Poly(&[(180, 60), (60, 180)])],
        Icon::Minimize => &[Poly(&[(60, 120), (180, 120)])],
        Icon::Maximize => &[RRect(55, 55, 130, 130, 10)],
        Icon::Plus => &[Poly(&[(120, 50), (120, 190)]), Poly(&[(50, 120), (190, 120)])],
        Icon::Trash => &[Poly(&[(30, 60), (210, 60)]), Poly(&[(190, 60), (180, 200), (160, 220), (80, 220), (60, 200), (50, 60)]), Poly(&[(90, 60), (90, 30), (150, 30), (150, 60)])],
        Icon::Send => &[Poly(&[(220, 20), (150, 220), (110, 130), (20, 90), (220, 20)]), Poly(&[(220, 20), (110, 130)])],
        Icon::Play => &[Poly(&[(60, 40), (200, 120), (60, 200), (60, 40)])],
        Icon::Pause => &[Poly(&[(80, 50), (80, 190)]), Poly(&[(160, 50), (160, 190)])],
        Icon::SkipNext => &[Poly(&[(50, 50), (160, 120), (50, 190), (50, 50)]), Poly(&[(190, 50), (190, 190)])],
        Icon::SkipPrev => &[Poly(&[(190, 50), (80, 120), (190, 190), (190, 50)]), Poly(&[(50, 50), (50, 190)])],
        Icon::Terminal => &[Poly(&[(40, 170), (100, 110), (40, 50)]), Poly(&[(120, 190), (200, 190)])],
        Icon::Link => &[RRect(20, 50, 130, 100, 10), Poly(&[(10, 185), (160, 185)]), RRect(165, 80, 65, 130, 12)],
        Icon::Monitor => &[RRect(20, 30, 200, 140, 20), Poly(&[(80, 210), (160, 210)]), Poly(&[(120, 170), (120, 210)])],
        Icon::Power => &[Arc(120, 130, 85, 300, 600), Poly(&[(120, 25), (120, 120)])],
        Icon::Bluetooth => &[Poly(&[(70, 70), (170, 170), (120, 220), (120, 20), (170, 70), (70, 170)])],
        Icon::Home => &[Poly(&[(30, 100), (120, 25), (210, 100)]), Poly(&[(50, 85), (50, 210), (190, 210), (190, 85)]), Poly(&[(95, 210), (95, 145), (145, 145), (145, 210)])],
        Icon::User => &[Circle(120, 80, 45), Arc(120, 230, 80, 180, 360)],
        Icon::Info => &[Circle(120, 120, 100), Poly(&[(120, 110), (120, 165)]), Dot(120, 75, 14)],
        Icon::Check => &[Poly(&[(40, 125), (95, 180), (200, 65)])],
        Icon::Laptop => &[RRect(40, 40, 160, 115, 12), Poly(&[(15, 195), (225, 195)])],
        Icon::Bell => &[
            Poly(&[(60, 170), (60, 100), (75, 60), (120, 35), (165, 60), (180, 100), (180, 170), (200, 180), (40, 180), (60, 170)]),
            Poly(&[(100, 210), (140, 210)]),
        ],
        Icon::Keypad => &[
            Dot(60, 40, 16), Dot(120, 40, 16), Dot(180, 40, 16),
            Dot(60, 100, 16), Dot(120, 100, 16), Dot(180, 100, 16),
            Dot(60, 160, 16), Dot(120, 160, 16), Dot(180, 160, 16),
            Dot(120, 215, 16),
        ],
        Icon::Lock => &[RRect(40, 105, 160, 115, 18), Arc(120, 95, 50, 180, 360), Poly(&[(70, 95), (70, 105)]), Poly(&[(170, 95), (170, 105)]), Dot(120, 160, 14)],
        Icon::Key => &[Circle(70, 120, 45), Poly(&[(115, 120), (220, 120)]), Poly(&[(195, 120), (195, 160)]), Poly(&[(160, 120), (160, 150)])],
        Icon::Fingerprint => &[
            Arc(120, 125, 100, 200, 340),
            Arc(120, 140, 72, 180, 360),
            Poly(&[(48, 140), (48, 170)]),
            Poly(&[(192, 140), (192, 205)]),
            Arc(120, 140, 42, 180, 360),
            Poly(&[(78, 140), (78, 215)]),
            Poly(&[(162, 140), (162, 185)]),
            Poly(&[(120, 120), (120, 220)]),
        ],
        Icon::Keyboard => &[
            RRect(15, 50, 210, 140, 20),
            Dot(60, 95, 11), Dot(100, 95, 11), Dot(140, 95, 11), Dot(180, 95, 11),
            Poly(&[(75, 145), (165, 145)]),
        ],
        Icon::Shift => &[Poly(&[(120, 30), (210, 125), (160, 125), (160, 205), (80, 205), (80, 125), (30, 125), (120, 30)])],
        Icon::Backspace => &[
            Poly(&[(85, 50), (215, 50), (215, 190), (85, 190), (20, 120), (85, 50)]),
            Poly(&[(120, 90), (175, 150)]),
            Poly(&[(175, 90), (120, 150)]),
        ],
        Icon::AlignLeft => &[Poly(&[(30, 50), (210, 50)]), Poly(&[(30, 95), (150, 95)]), Poly(&[(30, 140), (210, 140)]), Poly(&[(30, 185), (150, 185)])],
        Icon::AlignCenter => &[Poly(&[(30, 50), (210, 50)]), Poly(&[(70, 95), (170, 95)]), Poly(&[(30, 140), (210, 140)]), Poly(&[(70, 185), (170, 185)])],
        Icon::AlignRight => &[Poly(&[(30, 50), (210, 50)]), Poly(&[(90, 95), (210, 95)]), Poly(&[(30, 140), (210, 140)]), Poly(&[(90, 185), (210, 185)])],
        Icon::ListBullet => &[
            Dot(45, 60, 16), Poly(&[(90, 60), (215, 60)]),
            Dot(45, 120, 16), Poly(&[(90, 120), (215, 120)]),
            Dot(45, 180, 16), Poly(&[(90, 180), (215, 180)]),
        ],
        Icon::ListNumber => &[
            // "1", "2", "3" as tiny strokes beside the lines
            Poly(&[(35, 45), (48, 38), (48, 80)]),
            Poly(&[(30, 105), (40, 98), (55, 100), (58, 112), (30, 140), (60, 140)]),
            Poly(&[(30, 163), (58, 163), (42, 180), (58, 188), (55, 202), (30, 202)]),
            Poly(&[(90, 60), (215, 60)]),
            Poly(&[(90, 120), (215, 120)]),
            Poly(&[(90, 180), (215, 180)]),
        ],
        Icon::Undo => &[Poly(&[(80, 50), (35, 95), (80, 140)]), Poly(&[(35, 95), (145, 95)]), Arc(145, 145, 50, 270, 450), Poly(&[(145, 195), (90, 195)])],
        Icon::Redo => &[Poly(&[(160, 50), (205, 95), (160, 140)]), Poly(&[(205, 95), (95, 95)]), Arc(95, 145, 50, 90, 270), Poly(&[(95, 195), (150, 195)])],
        Icon::Slides => &[RRect(20, 36, 200, 136, 16), Poly(&[(120, 172), (120, 206)]), Poly(&[(78, 208), (162, 208)]), Poly(&[(102, 76), (102, 132), (150, 104), (102, 76)])],
        Icon::TextBox => &[RRect(30, 40, 180, 160, 14), Poly(&[(80, 85), (160, 85)]), Poly(&[(120, 85), (120, 160)])],
        Icon::Table => &[RRect(28, 40, 184, 160, 12), Poly(&[(28, 92), (212, 92)]), Poly(&[(28, 146), (212, 146)]), Poly(&[(90, 40), (90, 200)]), Poly(&[(150, 40), (150, 200)])],
        Icon::Chart => &[Poly(&[(30, 30), (30, 210), (212, 210)]), Poly(&[(75, 180), (75, 130)]), Poly(&[(120, 180), (120, 80)]), Poly(&[(165, 180), (165, 110)])],
        Icon::Shapes => &[RRect(28, 96, 112, 112, 10), Circle(150, 90, 62)],
        Icon::Speaker => &[Poly(&[(25, 95), (70, 95), (120, 50), (120, 190), (70, 145), (25, 145), (25, 95)]), Arc(135, 120, 35, 310, 410), Arc(135, 120, 70, 310, 410)],
        Icon::SpeakerOff => &[Poly(&[(25, 95), (70, 95), (120, 50), (120, 190), (70, 145), (25, 145), (25, 95)]), Poly(&[(155, 90), (215, 150)]), Poly(&[(215, 90), (155, 150)])],
        Icon::Sun => &[
            Circle(120, 120, 42),
            Poly(&[(120, 20), (120, 50)]),
            Poly(&[(120, 190), (120, 220)]),
            Poly(&[(20, 120), (50, 120)]),
            Poly(&[(190, 120), (220, 120)]),
            Poly(&[(49, 49), (70, 70)]),
            Poly(&[(170, 170), (191, 191)]),
            Poly(&[(191, 49), (170, 70)]),
            Poly(&[(70, 170), (49, 191)]),
        ],
        // the assistant: a starburst of rays
        Icon::Spark => &[
            Poly(&[(120, 20), (120, 220)]),
            Poly(&[(20, 120), (220, 120)]),
            Poly(&[(55, 55), (185, 185)]),
            Poly(&[(185, 55), (55, 185)]),
            Poly(&[(82, 28), (158, 212)]),
            Poly(&[(28, 158), (212, 82)]),
            Dot(120, 120, 22),
        ],
        Icon::Scripts => &[
            Poly(&[(150, 20), (60, 20), (40, 40), (40, 200), (60, 220), (180, 220), (200, 200), (200, 70), (150, 20)]),
            Poly(&[(80, 95), (160, 95)]),
            Poly(&[(80, 135), (160, 135)]),
            Poly(&[(80, 175), (125, 175)]),
            Poly(&[(80, 55), (115, 55)]),
        ],
        Icon::Save => &[
            Poly(&[(160, 30), (50, 30), (30, 50), (30, 190), (50, 210), (190, 210), (210, 190), (210, 80), (160, 30)]),
            Poly(&[(70, 210), (70, 140), (170, 140), (170, 210)]),
            Poly(&[(70, 30), (70, 75), (145, 75)]),
        ],
    }
}

fn arc_points(cx: i64, cy: i64, r: i64, a0: i32, a1: i32, out: &mut Vec<(i64, i64)>) {
    // angles in degrees; convert to 1024-step circle units
    let s0 = a0 * 1024 / 360;
    let s1 = a1 * 1024 / 360;
    let steps = ((s1 - s0).abs() / 12).max(4);
    for k in 0..=steps {
        let a = s0 + (s1 - s0) * k / steps;
        let c = sin_q14(a + 256) as i64;
        let s = sin_q14(a) as i64;
        out.push((cx + r * c / 16384, cy + r * s / 16384));
    }
}

struct Geo {
    segs: Vec<[i64; 4]>,
    dots: Vec<[i64; 3]>,
    fills: Vec<[i64; 4]>,
}

fn build(i: Icon) -> Geo {
    let k = 100i64; // tenths -> thousandths
    let mut g = Geo { segs: vec![], dots: vec![], fills: vec![] };
    let polyline = |pts: &[(i64, i64)], g: &mut Geo| {
        for w in pts.windows(2) {
            g.segs.push([w[0].0, w[0].1, w[1].0, w[1].1]);
        }
    };
    for p in shape(i) {
        let mut pts: Vec<(i64, i64)> = vec![];
        match *p {
            Poly(v) => {
                for &(x, y) in v {
                    pts.push((x as i64 * k, y as i64 * k));
                }
            }
            Circle(cx, cy, r) => arc_points(cx as i64 * k, cy as i64 * k, r as i64 * k, 0, 360, &mut pts),
            Arc(cx, cy, r, a0, a1) => arc_points(cx as i64 * k, cy as i64 * k, r as i64 * k, a0 as i32, a1 as i32, &mut pts),
            Ellipse(cx, cy, rx, ry) => {
                for s in 0..=48 {
                    let a = s * 1024 / 48;
                    let c = sin_q14(a + 256) as i64;
                    let sn = sin_q14(a) as i64;
                    pts.push((cx as i64 * k + rx as i64 * k * c / 16384, cy as i64 * k + ry as i64 * k * sn / 16384));
                }
            }
            RRect(x, y, w, h, r) => {
                let (x, y, w, h, r) = (x as i64 * k, y as i64 * k, w as i64 * k, h as i64 * k, r as i64 * k);
                arc_points(x + w - r, y + r, r, 270, 360, &mut pts);
                arc_points(x + w - r, y + h - r, r, 0, 90, &mut pts);
                arc_points(x + r, y + h - r, r, 90, 180, &mut pts);
                arc_points(x + r, y + r, r, 180, 270, &mut pts);
                pts.push(pts[0]);
            }
            Dot(cx, cy, r) => g.dots.push([cx as i64 * k, cy as i64 * k, r as i64 * k]),
            Fill(x, y, w, h) => g.fills.push([x as i64 * k, y as i64 * k, (x + w) as i64 * k, (y + h) as i64 * k]),
        }
        if !pts.is_empty() {
            polyline(&pts, &mut g);
        }
    }
    g
}

fn raster(i: Icon, size: i32) -> Vec<u8> {
    let g = build(i);
    let hw: i64 = 1000 + if size <= 16 { 150 } else { 0 }; // half stroke width in thousandths
    let hw2 = hw * hw;
    let n = size as i64;
    let mut m = vec![0u8; (size * size) as usize];
    let segs: Vec<([i64; 4], [i64; 4])> = g
        .segs
        .iter()
        .map(|s| (*s, [s[0].min(s[2]) - hw, s[1].min(s[3]) - hw, s[0].max(s[2]) + hw, s[1].max(s[3]) + hw]))
        .collect();
    for py in 0..n {
        for px in 0..n {
            let mut cnt = 0;
            for sy in 0..4 {
                let y = (8 * py + 2 * sy + 1) * 24000 / (8 * n);
                for sx in 0..4 {
                    let x = (8 * px + 2 * sx + 1) * 24000 / (8 * n);
                    let mut hit = g.fills.iter().any(|f| x >= f[0] && x <= f[2] && y >= f[1] && y <= f[3])
                        || g.dots.iter().any(|d| (x - d[0]).pow(2) + (y - d[1]).pow(2) <= d[2] * d[2]);
                    if !hit {
                        for (s, bb) in &segs {
                            if x < bb[0] || x > bb[2] || y < bb[1] || y > bb[3] {
                                continue;
                            }
                            let (dx, dy) = (s[2] - s[0], s[3] - s[1]);
                            let (ex, ey) = (x - s[0], y - s[1]);
                            let len2 = dx * dx + dy * dy;
                            let d2 = if len2 == 0 {
                                ex * ex + ey * ey
                            } else {
                                let t = (ex * dx + ey * dy).clamp(0, len2);
                                let qx = ex - dx * t / len2;
                                let qy = ey - dy * t / len2;
                                qx * qx + qy * qy
                            };
                            if d2 <= hw2 {
                                hit = true;
                                break;
                            }
                        }
                    }
                    if hit {
                        cnt += 1;
                    }
                }
            }
            m[(py * n + px) as usize] = (cnt * 255 / 16) as u8;
        }
    }
    m
}

struct Cache(UnsafeCell<BTreeMap<(Icon, i32), Vec<u8>>>);
unsafe impl Sync for Cache {}
static CACHE: Cache = Cache(UnsafeCell::new(BTreeMap::new()));

pub fn draw(c: &mut Canvas, i: Icon, x: i32, y: i32, size: i32, col: Color) {
    let cache = unsafe { &mut *CACHE.0.get() };
    let m = cache.entry((i, size)).or_insert_with(|| raster(i, size));
    c.mask(x, y, size, size, m, col);
}
