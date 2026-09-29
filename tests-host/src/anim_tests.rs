//! Motion curves and tweens.

use crate::anim::*;
use crate::gfx::Rect;

#[test]
fn curves_start_and_end_in_place() {
    for f in [ease_out, ease_in, ease_in_out, ease_out_back] {
        assert_eq!(f(0), 0);
        assert_eq!(f(1000), 1000);
        // out of range is held at the ends
        assert_eq!(f(-50), 0);
        assert_eq!(f(1500), 1000);
    }
    // ease-out is ahead of linear, ease-in behind, ease-in-out even at half
    assert!(ease_out(300) > 300 && ease_in(300) < 300);
    assert_eq!(ease_in_out(500), 500);
    // the curves only go forwards, except the spring's overshoot
    for f in [ease_out, ease_in, ease_in_out] {
        let mut last = 0;
        for p in (0..=1000).step_by(10) {
            assert!(f(p) >= last, "p={}", p);
            last = f(p);
        }
    }
    // the spring overshoots a little, then settles
    let peak = (0..=1000).map(ease_out_back).max().unwrap();
    assert!(peak > 1000 && peak < 1150, "{}", peak);
}

#[test]
fn tweens() {
    assert_eq!(progress(100, 20, 90), 0);
    assert_eq!(progress(100, 20, 110), 500);
    assert_eq!(progress(100, 20, 130), 1000);
    // Reduce motion: no duration, already there
    assert_eq!(progress(100, 0, 100), 1000);
    assert_eq!(mix(10, 20, 500), 15);
    let a = Rect::new(0, 0, 100, 100);
    let b = Rect::new(100, 200, 300, 50);
    assert_eq!(mix_rect(a, b, 0), a);
    assert_eq!(mix_rect(a, b, 1000), b);
    assert_eq!(scale_rect(Rect::new(0, 0, 200, 100), 500), Rect::new(50, 25, 100, 50));
}
