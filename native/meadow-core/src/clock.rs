//! The park post clock beside the path, ported from the web's `src/clock.js`: where it stands and
//! where its hands point at the viewer's local time.
use core::f64::consts::TAU;

pub const X: f64 = -3.25;
pub const Z: f64 = -2.75; // open grass left of the path, no tree within 2.8 units
pub const POST_H: f64 = 1.6;
pub const R: f64 = 0.5; // face radius before SCALE
pub const SCALE: f64 = 1.3; // face ~40 art pixels across at pixel size 2 on 1080p

/// (hour, minute) hand angles in radians, clockwise from 12, at local time h:m:s.
pub fn hand_angles(h: u32, m: u32, s: f64) -> (f64, f64) {
    let m = m as f64 + s / 60.0;
    (((h % 12) as f64 + m / 60.0) / 12.0 * TAU, m / 60.0 * TAU)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_point_at_the_local_time_clockwise_from_12() {
        for (h, m, hour, minute) in [
            (0, 0, 0.0, 0.0),
            (3, 0, 0.25, 0.0),
            (6, 30, 6.5 / 12.0, 0.5),
            (15, 0, 0.25, 0.0), // 12-hour dial
            (23, 45, 11.75 / 12.0, 0.75),
        ] {
            let (a_hour, a_minute) = hand_angles(h, m, 0.0);
            assert!((a_hour - hour * TAU).abs() < 1e-9, "{h}:{m} hour hand {a_hour}");
            assert!((a_minute - minute * TAU).abs() < 1e-9, "{h}:{m} minute hand {a_minute}");
        }
        assert!((hand_angles(0, 0, 30.0).1 - TAU / 120.0).abs() < 1e-9, "seconds move the minute hand");
    }
}
