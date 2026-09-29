//! System sounds: the notes they're made of, their shape, the volume curve.

use crate::sound::*;

/// The strongest frequency in `s` (a plain DFT over the bins asked about).
fn loudest(s: &[i16], candidates: &[u32]) -> u32 {
    let mut best = (0.0, 0);
    for &f in candidates {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, v) in s.iter().enumerate() {
            let a = 2.0 * std::f64::consts::PI * f as f64 * n as f64 / RATE as f64;
            re += *v as f64 * a.cos();
            im += *v as f64 * a.sin();
        }
        let m = re * re + im * im;
        if m > best.0 {
            best = (m, f);
        }
    }
    best.1
}

#[test]
fn notes() {
    let v = render(Sound::Volume);
    assert_eq!(v.len(), 60 * 48);
    assert_eq!(loudest(&v, &[440, 660, 880, 1100, 1320, 1760]), 880);
    // the error sound is low; the startup chord starts on middle C
    let e = render(Sound::Error);
    assert_eq!(loudest(&e[..4800], &[185, 220, 262, 440]), 220);
    let s = render(Sound::Startup);
    assert_eq!(loudest(&s[..4000], &[196, 262, 330, 392, 524]), 262);
}

#[test]
fn shape() {
    let n = render(Sound::Notify);
    // a soft start (5 ms attack), fading out to silence
    let peak = |s: &[i16]| s.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
    assert!(peak(&n[..24]) < peak(&n[240..480]));
    assert!(peak(&n[n.len() - 480..]) < peak(&n[..4800]) / 8);
    // never clips
    for s in [Sound::Startup, Sound::Notify, Sound::Error, Sound::Volume, Sound::Success] {
        assert!(peak(&render(s)) < 32767);
    }
}

#[test]
fn volume_and_mixing() {
    assert_eq!(gain(100, false), 1024);
    assert_eq!(gain(0, false), 0);
    assert_eq!(gain(80, true), 0);
    assert!(gain(50, false) > gain(40, false) && gain(40, false) > gain(10, false));
    // half volume sounds half as loud: about -20 dB
    assert!((90..=110).contains(&gain(50, false)));
    let mut m = Mixer::default();
    m.play(Sound::Volume);
    let mut out = vec![0i16; 2 * 48 * 70];
    m.fill(&mut out, 1024);
    // stereo: both channels the same
    assert!(out.chunks(2).all(|f| f[0] == f[1]));
    assert!(out.iter().any(|v| *v != 0));
    // done: silence after
    let mut more = vec![1i16; 64];
    m.fill(&mut more, 1024);
    assert!(more.iter().all(|v| *v == 0));
}
