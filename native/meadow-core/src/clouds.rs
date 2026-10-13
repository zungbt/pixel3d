//! The cloud shadows' weather: how much of the sky is cloud, and where the clouds have drifted to.
//! Their shapes are in the scene shader (common.wgsl's cloud_shade). With a city chosen,
//! the real cover and wind (weather.rs) replace the made-up ones.
use std::f64::consts::TAU;

use libm::{cos, floor, sin};

use crate::sky::{Rgb, SkyState};
use crate::weather::Weather;
use crate::wind::Wind;
use crate::world::{lerp, smoothstep};

const SPEED: f64 = 0.15; // world units/s: half the web's, so they sail slowly over
const HEADING: f64 = 0.38; // rad from +x: the web's drift direction
const VEER: f64 = 0.5; // rad: the wind swings this far either way...
const VEER_PERIOD: f64 = 1800.0; // ...over half an hour
const GUST_PULL: f64 = 0.15; // a full grass gust drifts the clouds this much faster
const SPELL: f64 = 1200.0; // s: the weather changes over some twenty minutes
const REAL_VEER: f64 = 0.15; // rad: with the real wind, only a little swing about it
const WIND_KMH: f64 = 12.0; // the real wind that drifts the clouds at the made-up speeds
const OVERCAST: (f64, f64) = (0.6, 1.0); // cover over which the sun goes behind the cloud

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clouds {
    pub drift: [f64; 2], // world x, z
}

impl Clouds {
    /// Drifts the clouds `dt` on at animation time `t` (s): the wind veers slowly, and the grass's
    /// gusts push the clouds along a little faster. Real `weather` sets the heading and speed.
    pub fn update(&mut self, t: f64, dt: f64, weather: Option<&Weather>, wind: &Wind) {
        let (base, veer, speed) = match weather {
            Some(w) => (w.heading(), REAL_VEER, (w.wind_kmh / WIND_KMH).clamp(0.3, 2.5)),
            None => (HEADING, VEER, 1.0),
        };
        let heading = base + veer * sin(t * TAU / VEER_PERIOD);
        let pull = speed * (1.0 + GUST_PULL * wind.gust());
        self.drift[0] += cos(heading) * SPEED * pull * dt;
        self.drift[1] += sin(heading) * SPEED * pull * dt;
    }
}

/// `sky` under `cover`: past OVERCAST.0 the sun fades behind the cloud (to 15% at full cover)
/// and the light turns flat, greyer and a little brighter all round, so an overcast meadow has
/// no sunny patches and hardly any shadows.
pub fn overcast(sky: &SkyState, cover: f64) -> SkyState {
    let o = smoothstep(cover, OVERCAST.0, OVERCAST.1);
    let grey = |c: Rgb, k: f64| {
        let l = (c[0] + c[1] + c[2]) / 3.0;
        c.map(|v| lerp(v, l, 0.25 * o) * k)
    };
    SkyState {
        sun_color: sky.sun_color.map(|v| v * (1.0 - 0.85 * o)),
        hemi_sky: grey(sky.hemi_sky, 1.0 + 0.2 * o),
        fog_color: grey(sky.fog_color, 1.0),
        ..*sky
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
    use crate::weather::tests::CLEAR;

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
    fn the_clouds_drift_steadily_with_a_little_gust() {
        let mut clouds = Clouds::default();
        let dt = 1.0 / 15.0;
        let mut t = 0.0;
        for _ in 0..15 * 60 {
            let before = clouds;
            clouds.update(t, dt, None, &Wind::at(t));
            t += dt;
            let step = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) / dt;
            let speed = step(clouds.drift, before.drift);
            assert!((SPEED - 1e-9..=SPEED * (1.0 + GUST_PULL) + 1e-9).contains(&speed), "{speed}");
        }
        // After a minute, about a minute's drift.
        let dist = clouds.drift[0].hypot(clouds.drift[1]);
        assert!((SPEED * 60.0..SPEED * 60.0 * 1.15).contains(&dist), "{dist}");
    }

    #[test]
    fn an_overcast_sky_hides_the_sun() {
        let sky = crate::sky::sky_state(crate::sky::NOON, 0.0);
        assert_eq!(format!("{:?}", overcast(&sky, 0.6)), format!("{sky:?}"), "broken cloud: as before");
        let grey = overcast(&sky, 1.0);
        assert!((grey.sun_color[0] - sky.sun_color[0] * 0.15).abs() < 1e-12);
        assert!(grey.hemi_sky.iter().sum::<f64>() > sky.hemi_sky.iter().sum::<f64>() * 1.15);
        assert!(grey.hemi_sky[2] - grey.hemi_sky[0] < sky.hemi_sky[2] - sky.hemi_sky[0], "greyer");
    }

    #[test]
    fn real_weather_sets_the_cover_and_the_drift() {
        let w = Weather { cover: 0.8, wind_from: 270.0, wind_kmh: 24.0, gust_kmh: 30.0, ..CLEAR }; // a west wind
        assert_eq!(cover(1.8e9, NOON, Some(&w)), 0.8);
        assert_eq!(cover(1.8e9, -0.2, Some(&w)), 0.0, "none at night");
        let mut clouds = Clouds::default();
        for k in 0..60 * 15 {
            clouds.update(k as f64 / 15.0, 1.0 / 15.0, Some(&w), &Wind::at(k as f64 / 15.0));
        }
        // Eastward (+x), at twice the made-up speed and within the small swing.
        let [x, z] = clouds.drift;
        assert!(x > SPEED * 2.0 * 60.0 * 0.95 && z.abs() < x * 0.16, "{x}, {z}");
    }
}
