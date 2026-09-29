//! Ambience: the screen follows the room's light.
//!
//! A light sensor (a HID sensor over USB or I2C, or ACPI's _ALI) reports lux;
//! the brightness heads for a level that suits it, slowly, so a passing
//! shadow doesn't flicker the screen. Moving the brightness by hand shifts
//! the curve rather than switching it off, and that shift is kept.
//!
//! Plain logic, host-tested.

/// The dimmest it goes (as the brightness keys: sys::MIN_BRIGHTNESS).
const MIN_BRIGHTNESS: u8 = 10;

/// The brightness (percent) that suits `lux`, shifted by `bias`: a dark room
/// ~35 %, an office (300-500 lux) ~75 %, daylight 100 %. Logarithmic, as
/// eyes are.
pub fn target(lux: u32, bias: i32) -> u8 {
    // 25 percentage points per tenfold, from 35 % at 1 lux (log10 in
    // hundredths: whole digits, then straight between powers of ten)
    let lux = lux.max(1);
    let k = lux.ilog10();
    let p = 10u32.pow(k);
    let l100 = k as i32 * 100 + ((lux - p) as u64 * 100 / (9 * p as u64)) as i32;
    let base = 35 + 25 * l100 / 100;
    (base + bias).clamp(MIN_BRIGHTNESS as i32, 100) as u8
}

/// One step towards `want` (called ~ten times a second): a point at a time,
/// and nothing for a change of 2 points or less.
pub fn approach(cur: u8, want: u8) -> u8 {
    let d = want as i32 - cur as i32;
    if d.abs() <= 2 {
        cur
    } else {
        (cur as i32 + d.signum()) as u8
    }
}

/// A light sensor's reading, smoothed (each new reading counts a quarter).
pub fn smooth(prev: Option<u32>, lux: u32) -> u32 {
    match prev {
        Some(p) => ((p as u64 * 3 + lux as u64) / 4) as u32,
        None => lux,
    }
}
