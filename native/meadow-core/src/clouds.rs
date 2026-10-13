//! The cloud shadows' weather: how much of the sky is cloud, and where the two layers have drifted
//! to. Their shapes are noise in the scene shader (common.wgsl's cloud_shade). With a city chosen,
//! the real cover and wind (weather.rs) replace the made-up ones.
use std::f64::consts::TAU;

use libm::{cos, floor, sin};

use crate::weather::Weather;
use crate::wind::Wind;
use crate::world::{lerp, smoothstep};

const BIG_SPEED: f64 = 0.33; // world units/s, as the web's drift
const WISP_SPEED: f64 = 0.7; // the thin upper layer, faster
const SHEAR: f64 = 0.45; // rad: the upper layer's wind turned from the lower's
const HEADING: f64 = 0.38; // rad from +x: the web's drift direction
const VEER: f64 = 0.5; // rad: the wind swings this far either way...
const VEER_PERIOD: f64 = 1800.0; // ...over half an hour
const GUST_PULL: f64 = 0.15; // a full grass gust drifts the clouds this much faster
const SPELL: f64 = 1200.0; // s: the weather changes over some twenty minutes
const REAL_VEER: f64 = 0.15; // rad: with the real wind, only a little swing about it
const WIND_KMH: f64 = 12.0; // the real wind that drifts the clouds at the made-up speeds

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clouds {
    pub big: [f64; 2],   // the lower, big clouds' drift (world x, z)
    pub wisps: [f64; 2], // the upper wisps'
}

impl Clouds {
    /// Drifts both layers `dt` on at animation time `t` (s): the wind veers slowly, and the grass's
    /// gusts push the clouds along a little faster. Real `weather` sets the heading and speed.
    pub fn update(&mut self, t: f64, dt: f64, weather: Option<&Weather>, wind: &Wind) {
        let (base, veer, speed) = match weather {
            Some(w) => (w.heading(), REAL_VEER, (w.wind_kmh / WIND_KMH).clamp(0.3, 2.5)),
            None => (HEADING, VEER, 1.0),
        };
        let heading = base + veer * sin(t * TAU / VEER_PERIOD);
        let pull = speed * (1.0 + GUST_PULL * wind.gust());
        for (layer, layer_speed, turn) in [(&mut self.big, BIG_SPEED, 0.0), (&mut self.wisps, WISP_SPEED, SHEAR)] {
            layer[0] += cos(heading + turn) * layer_speed * pull * dt;
            layer[1] += sin(heading + turn) * layer_speed * pull * dt;
        }
    }
}

/// Share of the sky in cloud, 0-1, at unix time `unix` (s) with the sun at `elevation`: clear and
/// cloudy spells some minutes long, building toward midday; none at night. Real `weather` gives the
/// cover, still fading out at night.
pub fn cover(unix: f64, elevation: f64, weather: Option<&Weather>) -> f64 {
    if let Some(w) = weather {
        return w.cover * smoothstep(elevation, 0.0, 0.15);
    }
    let s = unix / SPELL;
    let spell = value_noise(s) * 0.7 + value_noise(s * 2.3 + 17.0) * 0.3;
    let build = lerp(0.4, 1.0, smoothstep(elevation, 0.1, 0.7));
    ((spell - 0.3) / 0.45).clamp(0.0, 1.0) * build * smoothstep(elevation, 0.0, 0.15)
}

// Smooth 1-D value noise, 0-1.
fn value_noise(x: f64) -> f64 {
    let i = floor(x);
    let f = x - i;
    lerp(hash(i), hash(i + 1.0), f * f * (3.0 - 2.0 * f))
}

fn hash(i: f64) -> f64 {
    let mut h = (i as i64 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    h ^= h >> 31;
    h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    h ^= h >> 29;
    (h >> 11) as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: f64 = 1.0;
    const DAY: f64 = 86_400.0;

    #[test]
    fn the_weather_has_clear_and_cloudy_spells() {
        let covers: Vec<f64> = (0..2000).map(|k| cover(1.8e9 + k as f64 * 60.0, NOON, None)).collect();
        let share = |f: fn(f64) -> bool| covers.iter().filter(|&&c| f(c)).count() as f64 / covers.len() as f64;
        assert!(covers.iter().all(|c| (0.0..=1.0).contains(c)));
        let (clear, cloudy) = (share(|c| c < 0.05), share(|c| c > 0.6));
        assert!((0.15..0.5).contains(&clear), "clear {clear}");
        assert!((0.1..0.5).contains(&cloudy), "cloudy {cloudy}");
        // Clear to cloudy takes five minutes or more.
        assert!(covers.windows(2).all(|w| (w[0] - w[1]).abs() < 0.2));
    }

    #[test]
    fn clouds_build_toward_midday_and_leave_the_night() {
        for k in 0..200 {
            let unix = 1.8e9 + k as f64 * DAY / 7.0;
            assert!(cover(unix, 0.3, None) <= cover(unix, NOON, None));
            assert_eq!(cover(unix, -0.2, None), 0.0);
        }
    }

    #[test]
    fn the_layers_drift_steadily_with_a_little_gust() {
        let mut clouds = Clouds::default();
        let dt = 1.0 / 15.0;
        let mut t = 0.0;
        for _ in 0..15 * 60 {
            let before = clouds;
            clouds.update(t, dt, None, &Wind::at(t));
            t += dt;
            let step = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) / dt;
            let (big, wisps) = (step(clouds.big, before.big), step(clouds.wisps, before.wisps));
            assert!((BIG_SPEED - 1e-9..=BIG_SPEED * (1.0 + GUST_PULL) + 1e-9).contains(&big), "{big}");
            assert!((WISP_SPEED - 1e-9..=WISP_SPEED * (1.0 + GUST_PULL) + 1e-9).contains(&wisps), "{wisps}");
        }
        // After a minute, about a minute's drift, the upper layer further and turned.
        let dist = clouds.big[0].hypot(clouds.big[1]);
        assert!((BIG_SPEED * 60.0..BIG_SPEED * 60.0 * 1.15).contains(&dist), "{dist}");
        let turn = clouds.wisps[1].atan2(clouds.wisps[0]) - clouds.big[1].atan2(clouds.big[0]);
        assert!((turn - SHEAR).abs() < 1e-6);
    }

    #[test]
    fn real_weather_sets_the_cover_and_the_drift() {
        let w = Weather { cover: 0.8, wind_from: 270.0, wind_kmh: 24.0, gust_kmh: 30.0 }; // a west wind
        assert_eq!(cover(1.8e9, NOON, Some(&w)), 0.8);
        assert_eq!(cover(1.8e9, -0.2, Some(&w)), 0.0, "none at night");
        let mut clouds = Clouds::default();
        for k in 0..60 * 15 {
            clouds.update(k as f64 / 15.0, 1.0 / 15.0, Some(&w), &Wind::at(k as f64 / 15.0));
        }
        // Eastward (+x), at twice the made-up speed and within the small swing.
        let [x, z] = clouds.big;
        assert!(x > BIG_SPEED * 2.0 * 60.0 * 0.95 && z.abs() < x * 0.16, "{x}, {z}");
    }
}
