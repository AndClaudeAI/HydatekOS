//! Haptic patterns.

use crate::haptics::*;

#[test]
fn patterns() {
    for h in Haptic::ALL {
        for s in Strength::ALL {
            let p = pattern(h, s);
            assert!(!p.is_empty());
            // short enough to feel like feedback, not an alarm
            assert!(duration(&p) <= 300, "{:?} {:?}", h, s);
            assert!(p.iter().all(|q| q.amp > 0 && q.ms > 0));
            // what's sent to the phone reads back the same
            assert_eq!(decode(&encode(&p)).unwrap(), p);
        }
        // stronger is stronger
        let amp = |s| pattern(h, s)[0].amp;
        assert!(amp(Strength::Light) < amp(Strength::Medium) && amp(Strength::Medium) <= amp(Strength::Strong));
    }
    // an error is felt as three buzzes, a success as two
    assert_eq!(pattern(Haptic::Error, Strength::Medium).len(), 3);
    assert_eq!(pattern(Haptic::Success, Strength::Medium).len(), 2);
    assert_eq!(encode(&pattern(Haptic::Tap, Strength::Strong)), "10:180:0");
    // the phone refuses nonsense
    for bad in ["", "10", "10:0:0", "0:100:0", "5000:100:0", "a:b:c", &"10:100:10,".repeat(17)] {
        assert!(decode(bad).is_none(), "{}", bad);
    }
    assert_eq!(Strength::from_id("light"), Strength::Light);
    assert_eq!(Strength::from_id("??"), Strength::Medium);
}

#[test]
fn engine() {
    let mut h = Haptics::default();
    h.feel(Haptic::Tap, 1000);
    // typing fast: taps closer than 25 ms are one
    h.feel(Haptic::Tap, 1010);
    assert_eq!(h.pending.len(), 1);
    // but an error always comes through
    h.feel(Haptic::Error, 1015);
    assert_eq!(h.pending.len(), 2);
    assert_eq!(h.last.unwrap().0, Haptic::Error);
    // switched off: nothing
    h.on = false;
    h.feel(Haptic::Success, 2000);
    assert_eq!(h.pending.len(), 2);
    // the queue doesn't grow without bound
    h.on = true;
    for i in 0..100 {
        h.feel(Haptic::Warning, 3000 + i * 100);
    }
    assert!(h.pending.len() <= 8);
}
