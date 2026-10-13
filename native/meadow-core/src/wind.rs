//! The meadow's wind: the travelling gust that bends the grass and flowers (scene.wgsl's `gust`),
//! sways the trees and rocks the perched insects. Made up, it is the web's; with a city chosen
//! it leans with the real wind and grows with its speed and gusts, easing over some seconds.
use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

use libm::{atan2, cos, exp, sin, sqrt};

use crate::grass::BLADE_H;
use crate::weather::Weather;

const EASE: f64 = 20.0; // s: the wind takes this long (1/e) to settle to new weather
const CALM_KMH: f64 = 12.0; // the real wind that looks like the made-up one
const HEADING: f64 = -std::f64::consts::FRAC_PI_4; // the made-up lean, toward +x -z

/// Uploaded as globals.wind (lean, phase, strength) and params.w (flutter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wind {
    pub phase: f64,    // the gusts' clock, s: animation time in the made-up wind
    pub strength: f64, // the travelling gust's size, 1 made-up
    pub flutter: f64,  // the quick shiver's size, 1 made-up
    pub heading: f64,  // rad from +x toward +z: where the grass leans
}

impl Default for Wind {
    fn default() -> Self {
        Wind { phase: 0.0, strength: 1.0, flutter: 1.0, heading: HEADING }
    }
}

impl Wind {
    /// The made-up wind at animation time `t`.
    pub fn at(t: f64) -> Self {
        Wind { phase: t, ..Wind::default() }
    }

    /// Steps `dt` on, easing toward `weather`'s wind (or the made-up one).
    pub fn update(&mut self, dt: f64, weather: Option<&Weather>) {
        let target = weather.map_or(Wind::default(), |w| Wind {
            phase: 0.0,
            strength: (w.wind_kmh / CALM_KMH).clamp(0.35, 1.6),
            flutter: ((w.gust_kmh - w.wind_kmh) / 8.0).clamp(0.5, 2.5),
            heading: w.heading(),
        });
        let k = 1.0 - exp(-dt / EASE);
        self.strength += (target.strength - self.strength) * k;
        self.flutter += (target.flutter - self.flutter) * k;
        // The short way round.
        let turn = target.heading - self.heading;
        self.heading += atan2(sin(turn), cos(turn)) * k;
        // Stronger wind, quicker gusts.
        self.phase += dt * sqrt(self.strength);
    }

    /// Where the grass leans, x and z.
    pub fn lean(&self) -> [f64; 2] {
        [cos(self.heading), sin(self.heading)]
    }

    /// The xz offset of a point at local height y on something rooted at (x, z): the grass tip's
    /// at BLADE_H. Must match `gust` in scene.wgsl.
    pub fn sway(&self, x: f64, z: f64, y: f64) -> (f64, f64) {
        let [lx, lz] = self.lean();
        // The gust fronts run across the lean (as the web's x + z against its lean of x - z).
        let across = (x * -lz + z * lx) * 0.45 * SQRT_2;
        let sway = sin(self.phase * 1.6 - across) * 0.5 + 0.5;
        let sway = sway * 0.12 * self.strength + sin(self.phase * 3.1 + x * 2.0) * 0.02 * self.flutter;
        let k = sway * (y / BLADE_H);
        (lx * k, lz * k)
    }

    /// The gust at the meadow's centre, 0 (still) to 1 (blades bent their furthest in the made-up wind).
    pub fn gust(&self) -> f64 {
        let (x, z) = self.sway(0.0, 0.0, BLADE_H);
        (x.hypot(z) / 0.12).clamp(0.0, 1.0)
    }
}

/// The made-up lean, for the shader tests.
pub const MADE_UP_LEAN: [f64; 2] = [FRAC_1_SQRT_2, -FRAC_1_SQRT_2];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_made_up_wind_is_the_webs() {
        // The web's: sin(t·1.6 − (x + z)·0.45)·0.5 + 0.5, × 0.12, + sin(t·3.1 + 2x)·0.02, along (1, −1)/√2.
        let w = Wind::at(2.0);
        let [lx, lz] = w.lean();
        assert!((lx - MADE_UP_LEAN[0]).abs() < 1e-12 && (lz - MADE_UP_LEAN[1]).abs() < 1e-12);
        let (x, z) = w.sway(1.0, 3.0, BLADE_H);
        let sway = (sin(2.0 * 1.6 - 4.0 * 0.45) * 0.5 + 0.5) * 0.12 + sin(2.0 * 3.1 + 2.0) * 0.02;
        assert!((x - FRAC_1_SQRT_2 * sway).abs() < 1e-12 && (z + FRAC_1_SQRT_2 * sway).abs() < 1e-12);
        assert_eq!(w.sway(1.0, 3.0, 0.0), (0.0, -0.0), "roots stay put");
        let (x2, _) = w.sway(1.0, 3.0, BLADE_H * 2.0);
        assert!((x2 - 2.0 * x).abs() < 1e-12, "grows with height");
        // Stepping without weather keeps it the made-up wind on the animation clock.
        let mut stepped = Wind::default();
        for _ in 0..30 {
            stepped.update(1.0 / 15.0, None);
        }
        assert!((stepped.phase - 2.0).abs() < 1e-9 && stepped.strength == 1.0 && stepped.heading == HEADING);
    }

    #[test]
    fn real_wind_eases_in_and_leans_downwind() {
        let storm = Weather { cover: 1.0, wind_from: 270.0, wind_kmh: 30.0, gust_kmh: 55.0 }; // from the west
        let mut w = Wind::default();
        w.update(1.0, Some(&storm));
        assert!(w.strength > 1.0 && w.strength < 1.1, "eases: {}", w.strength);
        for _ in 0..15 * 240 {
            w.update(1.0 / 15.0, Some(&storm));
        }
        assert!((w.strength - 1.6).abs() < 1e-3 && (w.flutter - 2.5).abs() < 1e-3);
        let [lx, lz] = w.lean();
        assert!(lx > 0.999 && lz.abs() < 0.01, "leans east: {lx}, {lz}");
        // Calm: a gentle sway.
        let calm = Weather { cover: 0.0, wind_from: 90.0, wind_kmh: 2.0, gust_kmh: 4.0 };
        for _ in 0..15 * 300 {
            w.update(1.0 / 15.0, Some(&calm));
        }
        assert!((w.strength - 0.35).abs() < 1e-3 && (w.flutter - 0.5).abs() < 1e-3);
        assert!(w.lean()[0] < -0.999, "turned to lean west");
    }
}
