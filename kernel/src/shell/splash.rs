//! Boot screen: after the PC maker's logo (drawn by the firmware), the screen
//! goes black and the Hydatek Systems wordmark fades in, in white, with a thin
//! progress bar under it while the system comes up. It then crossfades into
//! the first frame of the session.
//!
//! The wordmark is `assets/boot-logo.png`, made from
//! `assets/branding/hydatek-systems.jpg` by `tools/bootlogo.py`: a greyscale
//! picture of how much each pixel is covered by the lettering.

use crate::gfx::{scale_argb, Canvas, Color, Rect};
use alloc::vec::Vec;

use crate::brand::LOGO;

pub struct Splash {
    /// the wordmark's coverage at its size on screen (0-255 per pixel)
    logo: Vec<u8>,
    lw: i32,
    lh: i32,
    pub frame: Canvas,
    s: i32,
    progress: i32,
}

impl Splash {
    /// `w`/`h` are physical pixels, `s` the display scale.
    pub fn new(w: i32, h: i32, s: i32) -> Splash {
        let (mut logo, mut lw, mut lh) = (Vec::new(), 0, 0);
        if let Ok(img) = crate::image::decode(LOGO) {
            // wide screens: about a third of the width; phones: most of it
            lw = if h > w { w * 70 / 100 } else { (w * 36 / 100).min(640 * s) };
            lh = img.h as i32 * lw / img.w as i32;
            // white with the coverage as alpha, so scaling averages cleanly
            let src: Vec<u32> = img.px.iter().map(|p| (p & 0xFF) << 24 | 0xFF_FFFF).collect();
            logo = scale_argb(&src, img.w as i32, img.h as i32, lw, lh).iter().map(|p| (p >> 24) as u8).collect();
        }
        Splash { logo, lw, lh, frame: Canvas::new(w, h), s, progress: 0 }
    }

    /// The wordmark fading in from black (`fade` 0-255), before any progress.
    pub fn fade_in(&mut self, fade: u32) -> &Canvas {
        self.draw(fade.min(255), false);
        &self.frame
    }

    /// Redraw with `progress` (0-100). `status` goes to the serial log only:
    /// the screen stays black and white.
    pub fn step(&mut self, progress: i32, status: &str) -> &Canvas {
        log!("boot: {} ({}%)", status, progress);
        self.progress = progress.clamp(0, 100);
        self.draw(255, true);
        &self.frame
    }

    fn draw(&mut self, fade: u32, bar: bool) {
        let (w, h) = (self.frame.w, self.frame.h);
        self.frame.fill_rect(Rect::new(0, 0, w, h), Color::rgb(0x000000));
        let x0 = (w - self.lw) / 2;
        let y0 = h * 46 / 100 - self.lh / 2;
        // white on black: each pixel's grey level is its coverage
        for y in 0..self.lh {
            let (py, row) = (y0 + y, (y * self.lw) as usize);
            if py < 0 || py >= h {
                continue;
            }
            for x in 0..self.lw {
                let a = self.logo[row + x as usize] as u32 * fade / 255;
                if a > 0 {
                    self.frame.px[(py * w + x0 + x) as usize] = 0xFF00_0000 | a * 0x01_0101;
                }
            }
        }
        if bar {
            // a thin track under the wordmark, filling with progress
            let s = self.s;
            let bw = (self.lw * 36 / 100).max(120 * s);
            let track = Rect::new((w - bw) / 2, y0 + self.lh + self.lh * 45 / 100, bw, 3 * s);
            self.frame.fill_rrect(track, track.h / 2, Color::rgba(0xFFFFFF, 40));
            let fill = bw * self.progress / 100;
            if fill > 0 {
                self.frame.fill_rrect(Rect::new(track.x, track.y, fill.max(track.h), track.h), track.h / 2, Color::rgba(0xFFFFFF, 215));
            }
        }
    }
}
