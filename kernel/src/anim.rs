//! Motion: easing curves and tweens for the shell's animations (windows
//! opening, closing, minimising into the dock, maximising and snapping; the
//! launcher, menus and notifications coming in).
//!
//! Progress is in thousandths (0..=1000), so everything stays in integers.
//! Durations are in ticks (10 ms). With "Reduce motion" on, every tween is
//! finished the moment it starts.

use crate::gfx::Rect;

/// Linear progress `now` into a tween that began at `start` and lasts `dur`.
pub fn progress(start: u64, dur: u64, now: u64) -> i32 {
    if dur == 0 || now >= start + dur {
        1000
    } else if now <= start {
        0
    } else {
        ((now - start) * 1000 / dur) as i32
    }
}

/// Decelerating: fast start, gentle landing (things arriving).
pub fn ease_out(p: i32) -> i32 {
    let p = p.clamp(0, 1000) as i64;
    let inv = 1000 - p;
    (1000 - inv * inv / 1000 * inv / 1000) as i32
}

/// Accelerating: gentle start, fast finish (things leaving).
pub fn ease_in(p: i32) -> i32 {
    let p = p.clamp(0, 1000) as i64;
    (p * p / 1000 * p / 1000) as i32
}

/// Slow at both ends (things moving from one place to another).
pub fn ease_in_out(p: i32) -> i32 {
    let p = p.clamp(0, 1000);
    if p < 500 {
        ease_in(p * 2) / 2
    } else {
        500 + ease_out(p * 2 - 1000) / 2
    }
}

/// Overshoots a little and settles, like a spring (things popping open).
pub fn ease_out_back(p: i32) -> i32 {
    // 1 + (c+1)(p-1)^3 + c(p-1)^2 with c = 1.3, in thousandths
    let x = p.clamp(0, 1000) as i64 - 1000;
    let c = 1300i64;
    (1000 + (c + 1000) * x / 1000 * x / 1000 * x / 1000 + c * x / 1000 * x / 1000) as i32
}

/// `a` to `b` at `p` thousandths.
pub fn mix(a: i32, b: i32, p: i32) -> i32 {
    a + ((b - a) as i64 * p as i64 / 1000) as i32
}

/// A rectangle between `a` and `b`.
pub fn mix_rect(a: Rect, b: Rect, p: i32) -> Rect {
    Rect::new(mix(a.x, b.x, p), mix(a.y, b.y, p), mix(a.w, b.w, p), mix(a.h, b.h, p))
}

/// `r` scaled by `k` thousandths about its centre.
pub fn scale_rect(r: Rect, k: i32) -> Rect {
    let (w, h) = ((r.w as i64 * k as i64 / 1000) as i32, (r.h as i64 * k as i64 / 1000) as i32);
    Rect::new(r.x + (r.w - w) / 2, r.y + (r.h - h) / 2, w, h)
}

/// How long each kind of motion takes (ticks).
pub const OPEN: u64 = 22;
pub const CLOSE: u64 = 16;
pub const MINIMISE: u64 = 26;
pub const MOVE: u64 = 20;
pub const POPUP: u64 = 16;
