use crate::image;

fn dir() -> String {
    format!("{}/fixtures/images", env!("CARGO_MANIFEST_DIR"))
}

/// Pillow's decoding of a file: (width, height, RGBA bytes).
fn reference(name: &str) -> (u32, u32, Vec<u8>) {
    let script = "import sys\nfrom PIL import Image, ImageOps\nim = ImageOps.exif_transpose(Image.open(sys.argv[1])).convert('RGBA')\nsys.stdout.buffer.write(im.size[0].to_bytes(4,'little') + im.size[1].to_bytes(4,'little') + im.tobytes())";
    let out = std::process::Command::new("python3").args(["-c", script, &format!("{}/{}", dir(), name)]).output().expect("python3 with Pillow");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let d = out.stdout;
    (u32::from_le_bytes(d[0..4].try_into().unwrap()), u32::from_le_bytes(d[4..8].try_into().unwrap()), d[8..].to_vec())
}

/// Decode with HydatekOS and compare: (mean, max) difference per channel.
fn compare(name: &str) -> (f64, u32) {
    compare_with(name, true)
}

fn compare_with(name: &str, alpha: bool) -> (f64, u32) {
    let data = std::fs::read(format!("{}/{}", dir(), name)).unwrap();
    let img = image::decode(&data).unwrap_or_else(|e| panic!("{}: {}", name, e));
    let (w, h, want) = reference(name);
    assert_eq!((img.w, img.h), (w, h), "{} size", name);
    let (mut sum, mut max) = (0u64, 0u32);
    for (i, p) in img.px.iter().enumerate() {
        let got = [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8];
        let exp = &want[4 * i..4 * i + 4];
        // colour doesn't matter where both are fully transparent
        let n = if got[3] == 0 && exp[3] == 0 { 3 } else { 0 };
        for c in n..if alpha { 4 } else { 3 } {
            let d = (got[c] as i32 - exp[c] as i32).unsigned_abs();
            sum += d as u64;
            max = max.max(d);
        }
    }
    (sum as f64 / (img.px.len() * 4) as f64, max)
}

#[test]
fn png_exact() {
    for f in ["rgb.png", "rgb-filters.png", "rgb-interlaced.png", "rgba.png", "rgba-interlaced.png", "grey.png", "grey-alpha.png", "mono.png", "pal4.png", "pal8-trns.png", "rgb-trns.png"] {
        assert_eq!(compare(f), (0.0, 0), "{}", f);
    }
    // 16-bit samples: Pillow clips them when converting, so compare with the top byte
    let img = image::decode(&std::fs::read(format!("{}/grey16.png", dir())).unwrap()).unwrap();
    let out = std::process::Command::new("python3")
        .args(["-c", "import sys\nfrom PIL import Image\nsys.stdout.buffer.write(Image.open(sys.argv[1]).tobytes())", &format!("{}/grey16.png", dir())])
        .output()
        .unwrap();
    for (i, p) in img.px.iter().enumerate() {
        let v = u16::from_le_bytes([out.stdout[2 * i], out.stdout[2 * i + 1]]);
        let g = (v >> 8) as u32;
        assert_eq!(*p, 0xff00_0000 | g << 16 | g << 8 | g);
    }
}

#[test]
fn gif_and_bmp_exact() {
    for f in ["pal.gif", "trns-interlaced.gif", "rgb24.bmp", "pal8.bmp"] {
        assert_eq!(compare(f), (0.0, 0), "{}", f);
    }
    // Pillow ignores a plain 32-bit BMP's alpha bytes; HydatekOS uses them, as browsers do
    assert_eq!(compare_with("rgba32.bmp", false), (0.0, 0));
}

#[test]
fn jpeg_close_to_libjpeg() {
    // IDCT and upsampling differ slightly between decoders
    for f in ["q90-444.jpg", "q75-420.jpg", "q80-422.jpg", "progressive.jpg", "progressive-444.jpg", "grey.jpg", "grey-progressive.jpg", "small-odd.jpg", "restart.jpg", "rotated.jpg"] {
        let (mean, max) = compare(f);
        println!("{}: mean {:.3} max {}", f, mean, max);
        assert!(mean < 0.3 && max <= 4, "{}: mean {:.3} max {}", f, mean, max);
    }
}

#[test]
fn jpeg_cmyk() {
    let (mean, max) = compare("cmyk.jpg");
    println!("cmyk: mean {:.3} max {}", mean, max);
    assert!(mean < 0.3 && max <= 4, "cmyk: mean {:.3} max {}", mean, max);
}

#[test]
fn damaged_files_fail_cleanly() {
    for f in ["rgb.png", "q75-420.jpg", "progressive.jpg", "pal.gif", "rgb24.bmp", "lossy-q50.webp", "lossless-photo.webp", "lossy-alpha.webp", "anim.webp", "anim.gif"] {
        let data = std::fs::read(format!("{}/{}", dir(), f)).unwrap();
        for cut in [10, data.len() / 3, data.len() / 2, data.len() - 3] {
            let _ = image::decode(&data[..cut]);
        }
        // many randomly damaged copies: a decoder must fail, never crash
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for round in 0..300 {
            let mut junk = data.clone();
            for _ in 0..1 + round % 8 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let i = 2 + (seed as usize) % (junk.len() - 2);
                junk[i] = (seed >> 32) as u8;
            }
            let _ = image::decode(&junk);
        }
    }
    assert!(image::decode(b"hello").is_err());
    assert_eq!(image::sniff(b"<?xml version=\"1.0\"?><svg xmlns=...>"), Some(image::Format::Svg));
}

/// Go's image test files (a real photo in every JPEG subsampling, progressive
/// and restart variants, GIFs, and the PNG suite), when Go is installed:
/// `HYDATEK_GO_IMAGES=/usr/local/go/src/image cargo test --release suites -- --ignored`
#[test]
#[ignore]
fn go_image_suites() {
    let root = std::env::var("HYDATEK_GO_IMAGES").unwrap_or_else(|_| String::from("/usr/local/go/src/image"));
    let mut files = Vec::new();
    for d in ["testdata", "png/testdata/pngsuite"] {
        for e in std::fs::read_dir(format!("{}/{}", root, d)).expect("Go image test data") {
            files.push(e.unwrap().path());
        }
    }
    files.sort();
    let mut checked = 0;
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        if !["png", "jpeg", "gif"].contains(&ext.as_str()) {
            continue;
        }
        let data = std::fs::read(&path).unwrap();
        if name.contains("truncated") {
            let _ = image::decode(&data); // must not crash
            continue;
        }
        // 16-bit greyscale: Pillow clips instead of scaling (see png_exact)
        let grey16 = name.ends_with("g16.png");
        // Pillow also compares a low-bit-depth grey tRNS key with the scaled
        // 8-bit value, so it never matches: check colour only there
        let low_grey_key = ext == "png" && data[25] == 0 && data[24] < 8 && data.windows(4).any(|w| w == b"tRNS");
        let script = "import sys\nfrom PIL import Image\nim = Image.open(sys.argv[1]).convert('RGBA')\nsys.stdout.buffer.write(im.size[0].to_bytes(4,'little') + im.size[1].to_bytes(4,'little') + im.tobytes())";
        let out = std::process::Command::new("python3").args(["-c", script, path.to_str().unwrap()]).output().unwrap();
        let ours = image::decode(&data);
        if !out.status.success() {
            continue; // Pillow can't read it either
        }
        let img = ours.unwrap_or_else(|e| panic!("{}: {}", name, e));
        let d = out.stdout;
        let (w, h) = (u32::from_le_bytes(d[0..4].try_into().unwrap()), u32::from_le_bytes(d[4..8].try_into().unwrap()));
        assert_eq!((img.w, img.h), (w, h), "{}", name);
        if grey16 {
            continue;
        }
        let (mut sum, mut max) = (0u64, 0u32);
        for (i, p) in img.px.iter().enumerate() {
            let got = [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8];
            let exp = &d[8 + 4 * i..12 + 4 * i];
            let from = if got[3] == 0 && exp[3] == 0 { 3 } else { 0 };
            for c in from..if low_grey_key { 3 } else { 4 } {
                let diff = (got[c] as i32 - exp[c] as i32).unsigned_abs();
                sum += diff as u64;
                max = max.max(diff);
            }
        }
        let mean = sum as f64 / (img.px.len() * 4) as f64;
        println!("{:45} {}x{} mean {:.3} max {}", name, w, h, mean, max);
        if ext == "jpeg" {
            assert!(mean < 1.0 && max < 40, "{}: mean {} max {}", name, mean, max);
        } else {
            assert_eq!(max, 0, "{}", name);
        }
        checked += 1;
    }
    assert!(checked > 60, "checked {}", checked);
}

#[test]
fn webp_lossless_exact() {
    for f in ["lossless.webp", "lossless-pal.webp", "lossless-alpha.webp", "lossless-photo.webp"] {
        assert_eq!(compare(f), (0.0, 0), "{}", f);
    }
}

#[test]
fn webp_lossy_close_to_libwebp() {
    for f in ["lossy-q10.webp", "lossy-q50.webp", "lossy-q90.webp", "lossy-photo.webp", "lossy-small.webp", "lossy-alpha.webp"] {
        let (mean, max) = compare(f);
        println!("{}: mean {:.3} max {}", f, mean, max);
        assert!(mean < 0.5 && max <= 8, "{}: mean {:.3} max {}", f, mean, max);
    }
}

/// Every frame of an animation, as Pillow composes them.
fn reference_frames(name: &str) -> Vec<(Vec<u8>, u32)> {
    let script = "import sys\nfrom PIL import Image, ImageSequence\nim = Image.open(sys.argv[1])\nfor f in ImageSequence.Iterator(im):\n    d = f.convert('RGBA').tobytes()\n    sys.stdout.buffer.write(len(d).to_bytes(4,'little') + int(f.info.get('duration', 0)).to_bytes(4,'little') + d)";
    let out = std::process::Command::new("python3").args(["-c", script, &format!("{}/{}", dir(), name)]).output().unwrap();
    let d = out.stdout;
    let mut frames = Vec::new();
    let mut p = 0;
    while p + 8 <= d.len() {
        let n = u32::from_le_bytes(d[p..p + 4].try_into().unwrap()) as usize;
        let delay = u32::from_le_bytes(d[p + 4..p + 8].try_into().unwrap());
        frames.push((d[p + 8..p + 8 + n].to_vec(), delay));
        p += 8 + n;
    }
    frames
}

#[test]
fn animations() {
    for f in ["anim.gif", "anim.webp"] {
        let img = image::decode(&std::fs::read(format!("{}/{}", dir(), f)).unwrap()).unwrap();
        let want = reference_frames(f);
        assert_eq!(img.frames.len(), want.len(), "{} frames", f);
        assert_eq!(img.frames.iter().map(|fr| fr.delay).collect::<Vec<_>>(), vec![80, 120, 80, 120, 80, 200], "{}", f);
        for (k, (fr, (exp, _))) in img.frames.iter().zip(&want).enumerate() {
            for (i, p) in fr.px.iter().enumerate() {
                let got = [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8];
                let e = &exp[4 * i..4 * i + 4];
                if got[3] == 0 && e[3] == 0 {
                    continue;
                }
                assert_eq!(&got[..], e, "{} frame {} pixel {}", f, k, i);
            }
        }
        assert_eq!(img.px, img.frames[0].px);
    }
}

/// Our SVG drawing against Chromium's (the .png beside each .svg), on RGBA:
/// (mean difference, share of pixels off by more than 64).
fn svg_vs_chromium(name: &str) -> (f64, f64) {
    let d = format!("{}/fixtures/svg", env!("CARGO_MANIFEST_DIR"));
    let img = image::decode(&std::fs::read(format!("{}/{}.svg", d, name)).unwrap()).unwrap_or_else(|e| panic!("{}: {}", name, e));
    let out = std::process::Command::new("python3")
        .args(["-c", "import sys\nfrom PIL import Image\nim = Image.open(sys.argv[1]).convert('RGBA')\nsys.stdout.buffer.write(im.size[0].to_bytes(4,'little') + im.size[1].to_bytes(4,'little') + im.tobytes())", &format!("{}/{}.png", d, name)])
        .output()
        .unwrap();
    let r = out.stdout;
    let (w, h) = (u32::from_le_bytes(r[0..4].try_into().unwrap()), u32::from_le_bytes(r[4..8].try_into().unwrap()));
    assert_eq!((img.w, img.h), (w, h), "{}", name);
    // compare premultiplied, so colour under transparency doesn't count
    let (mut sum, mut bad) = (0u64, 0usize);
    let mut side = Vec::new();
    for (i, p) in img.px.iter().enumerate() {
        let a = (p >> 24) as u32;
        let got = [((p >> 16) & 255) * a / 255, ((p >> 8) & 255) * a / 255, (p & 255) * a / 255, a];
        let e = &r[8 + 4 * i..12 + 4 * i];
        let ea = e[3] as u32;
        let exp = [e[0] as u32 * ea / 255, e[1] as u32 * ea / 255, e[2] as u32 * ea / 255, ea];
        let diff = (0..4).map(|c| (got[c] as i32 - exp[c] as i32).unsigned_abs()).max().unwrap();
        sum += diff as u64;
        if diff > 64 {
            bad += 1;
        }
        side.push((got, exp));
    }
    if std::env::var("HYDATEK_SVG_DUMP").is_ok() {
        // ours | Chromium's, over white, for looking at
        let mut ppm = format!("P6 {} {} 255\n", 2 * w + 4, h).into_bytes();
        for y in 0..h as usize {
            for half in 0..2 {
                for x in 0..w as usize {
                    let (g, e) = &side[y * w as usize + x];
                    let c = if half == 0 { g } else { e };
                    for k in 0..3 {
                        ppm.push((c[k] + 255 - c[3]) as u8);
                    }
                }
                if half == 0 {
                    ppm.extend_from_slice(&[255, 0, 0].repeat(4));
                }
            }
        }
        std::fs::write(format!("/tmp/claude-0/svg-{}.ppm", name), ppm).unwrap();
    }
    (sum as f64 / img.px.len() as f64, bad as f64 / img.px.len() as f64)
}

#[test]
fn svg_matches_chromium() {
    for name in ["shapes", "paths", "gradients", "structure", "logo"] {
        let (mean, bad) = svg_vs_chromium(name);
        println!("{}: mean {:.2}, {:.2}% of pixels differ", name, mean, bad * 100.0);
        assert!(mean < 1.5 && bad < 0.005, "{}: mean {:.2} bad {:.3}", name, mean, bad);
    }
}

#[test]
fn svg_survives_nonsense() {
    let d = format!("{}/fixtures/svg", env!("CARGO_MANIFEST_DIR"));
    let chars = b"0123456789.-+e,MmLlCcQqAaZz <>\"=/#()%";
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    for name in ["shapes", "paths", "gradients", "structure", "logo"] {
        let src = std::fs::read(format!("{}/{}.svg", d, name)).unwrap();
        for round in 0..200 {
            let mut junk = src.clone();
            for _ in 0..1 + round % 12 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let i = (seed as usize) % junk.len();
                junk[i] = chars[(seed >> 40) as usize % chars.len()];
            }
            if let Ok(svg) = image::svg::parse(&junk) {
                let _ = svg.render(64, 48);
            }
        }
    }
    // extreme numbers
    for body in [
        "<path d='M1e300 1e300 L-1e300 5 Z' fill='red' stroke='blue' stroke-width='1e200'/>",
        "<circle r='NaN' cx='inf'/><rect width='-5' height='1e9'/>",
        "<path d='M0 0 A1e-300 1e-300 0 1 1 5 5 A0 0 0 0 0 1 1' stroke='black' stroke-dasharray='0.0000001'/>",
        "<g id='a'><use href='#a'/></g><use href='#a'/>",
        "<path d='M0 0 L10 10' stroke='red' stroke-width='3' stroke-dasharray='1e-9 1e-9'/>",
    ] {
        let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='50' height='40'>{}</svg>", body);
        let img = image::decode(svg.as_bytes()).unwrap();
        assert_eq!((img.w, img.h), (50, 40));
    }
}

#[test]
#[ignore]
fn svg_speed() {
    let d = format!("{}/fixtures/svg", env!("CARGO_MANIFEST_DIR"));
    for name in ["structure", "gradients", "logo"] {
        let svg = image::svg::parse(&std::fs::read(format!("{}/{}.svg", d, name)).unwrap()).unwrap();
        let t = std::time::Instant::now();
        let _ = svg.render(1000, 600);
        println!("{} at 1000x600: {:?}", name, t.elapsed());
    }
}
