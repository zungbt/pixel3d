//! Time of day, ported 1:1 from the web's `src/sky.js`. One directional light is the sun by day
//! and the moon by night; the sky, ambient and fog colours follow the sun's elevation. Colours are
//! linear rgb, lerped in linear (three.js Colors are linear under its colour management).
use std::f64::consts::{FRAC_1_SQRT_2, PI};

use libm::{cos, pow, sin};

use crate::sun::SunTimes;
use crate::world::{lerp, smoothstep};

pub const DAY_LENGTH: f64 = 300.0; // seconds for a full day-night cycle (the web's manual mode)
const DAY_START: f64 = 0.08; // fraction of the cycle at day_t = 0 (early morning)
pub const NOON: f64 = (0.25 - DAY_START) * DAY_LENGTH; // the day_t with the sun at its highest

const SUN_DAY: u32 = 0xfff1d6;
const SUN_LOW: u32 = 0xffa060;
const MOON: u32 = 0x9fb4ff;
const SKY_DAY: u32 = 0x7fb2e5;
const SKY_DUSK: u32 = 0xe0875a;
const SKY_NIGHT: u32 = 0x0b0d1a;
const HEMI_DAY: u32 = 0xbbe4f4; // faint teal: shadows shift cool-green
const HEMI_NIGHT: u32 = 0x2a3a66;
const FOG_DAY: u32 = 0xdfe6ea;
const FOG_NIGHT: u32 = 0x323c58;
const SUN_DIST: f64 = 38.0; // keeps every visible ground point in front of the shadow camera's near plane

pub type Rgb = [f64; 3];

/// sRGB hex → linear rgb.
pub fn linear(hex: u32) -> Rgb {
    let c = |shift: u32| {
        let s = ((hex >> shift) & 0xff) as f64 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { pow((s + 0.055) / 1.055, 2.4) }
    };
    [c(16), c(8), c(0)]
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)]
}

fn scale(c: Rgb, k: f64) -> Rgb {
    [c[0] * k, c[1] * k, c[2] * k]
}

// Clock hour <-> sky phase, for a day with sunrise and sunset at sun.rise and sun.set: the day
// maps onto phase 0 -> 0.5 and the night onto 0.5 -> 1, each at an even pace.
pub fn hour_to_phase(hour: f64, sun: SunTimes) -> f64 {
    let day = (sun.set - sun.rise + 24.0) % 24.0;
    let since = ((hour - sun.rise) % 24.0 + 24.0) % 24.0;
    if since < day { 0.5 * since / day } else { 0.5 + 0.5 * (since - day) / (24.0 - day) }
}

pub fn phase_to_hour(phase: f64, sun: SunTimes) -> f64 {
    let day = (sun.set - sun.rise + 24.0) % 24.0;
    let hour = if phase < 0.5 { sun.rise + 2.0 * phase * day } else { sun.set + 2.0 * (phase - 0.5) * (24.0 - day) };
    (hour % 24.0 + 24.0) % 24.0
}

/// The day_t that shows this local clock hour.
pub fn clock_time(hour: f64, sun: SunTimes) -> f64 {
    ((hour_to_phase(hour, sun) - DAY_START) % 1.0 + 1.0) % 1.0 * DAY_LENGTH
}

/// The ground fog setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fog {
    Off,
    Auto,
    Always,
}

/// Ground fog: `Auto` rolls in at dusk and burns off after sunrise, thicker in winter
/// (winterness, 0-1); `Always` is full fog day and night.
pub fn fog_amount(mode: Fog, elevation: f64, winter: f64) -> f64 {
    match mode {
        Fog::Off => 0.0,
        Fog::Always => 1.0,
        Fog::Auto => (1.0 - smoothstep(elevation, -0.05, 0.2)) * lerp(0.35, 1.0, winter),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SkyState {
    pub phase: f64,        // 0 -> 0.5 is day, 0.5 is sunset
    pub elevation: f64,    // > 0 day, < 0 night
    pub daylight: f64,     // 0 night, 1 day
    pub sun_pos: [f64; 3], // the light's position: the moon, mirrored across the horizon, at night
    pub sun_color: Rgb,    // × intensity
    pub hemi_sky: Rgb,     // × intensity
    pub background: Rgb,
    pub fog_color: Rgb,
    pub fog: f64, // ground fog amount, 0-1
}

/// The web's `update(t, dayT, fog)` for `fog.mode = 'auto'`; other modes: set `fog` with fog_amount.
pub fn sky_state(day_t: f64, winter: f64) -> SkyState {
    let angle = (day_t / DAY_LENGTH + DAY_START) * PI * 2.0;
    let elevation = sin(angle);
    let dir = if elevation >= 0.0 { 1.0 } else { -1.0 };
    let sun_pos = [cos(angle) * SUN_DIST * dir, elevation.abs() * SUN_DIST, SUN_DIST * 0.36];
    lit(phase(day_t), elevation, sun_pos, winter)
}

/// The sky phase (0 -> 0.5 day) a day_t shows.
pub fn phase(day_t: f64) -> f64 {
    (day_t / DAY_LENGTH + DAY_START) % 1.0
}

/// The compass in world x, z: north is the top of the screen (the camera looks down -x -z), east
/// its right.
pub const NORTH: [f64; 2] = [-FRAC_1_SQRT_2, -FRAC_1_SQRT_2];
pub const EAST: [f64; 2] = [FRAC_1_SQRT_2, -FRAC_1_SQRT_2];

/// The sky with the sun where it really stands: `elevation` and `azimuth` (radians, clockwise from
/// north) as sun::sun_position, on the NORTH / EAST compass. The moon stands across the sky from
/// the sun, as high as the sun is low.
pub fn sky_at(phase: f64, elevation: f64, azimuth: f64, winter: f64) -> SkyState {
    let (up, flat) = (sin(elevation), cos(elevation) * SUN_DIST);
    let k = if up >= 0.0 { flat } else { -flat };
    let (e, n) = (sin(azimuth) * k, cos(azimuth) * k);
    lit(phase, up, [e * EAST[0] + n * NORTH[0], up.abs() * SUN_DIST, e * EAST[1] + n * NORTH[1]], winter)
}

/// Lights and colours for the sun at `elevation` (its sine) with the light at `sun_pos`.
fn lit(phase: f64, elevation: f64, sun_pos: [f64; 3], winter: f64) -> SkyState {
    let is_day = elevation >= 0.0;
    // Light fades to 0 at the horizon, so swapping sun <-> moon there doesn't pop.
    let fade = smoothstep(elevation.abs(), 0.0, 0.2);
    let sun_color = if is_day {
        scale(mix(linear(SUN_LOW), linear(SUN_DAY), smoothstep(elevation, 0.0, 0.5)), 2.5 * fade)
    } else {
        scale(linear(MOON), 0.12 * fade) // faint, so moon shadows stay soft
    };
    let daylight = smoothstep(elevation, -0.1, 0.25);
    let dusk = 1.0 - smoothstep(elevation.abs(), 0.0, 0.35);
    // The ambient brightens from an hour before sunrise (and dims until an hour after sunset), so
    // twilight is never darker than midnight. The night blue is dim, hence the higher intensity.
    let twilight = smoothstep(elevation, -0.25, 0.25);
    let hemi_sky = scale(mix(linear(HEMI_NIGHT), linear(HEMI_DAY), twilight), lerp(2.1, 1.2, twilight));
    let background = mix(mix(linear(SKY_NIGHT), linear(SKY_DAY), daylight), linear(SKY_DUSK), dusk * 0.6);
    let fog_color = mix(mix(linear(FOG_NIGHT), linear(FOG_DAY), daylight), linear(SKY_DUSK), dusk * 0.3);
    SkyState { phase, elevation, daylight, sun_pos, sun_color, hemi_sky, background, fog_color, fog: fog_amount(Fog::Auto, elevation, winter) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EQUINOX: SunTimes = SunTimes { rise: 6.0, set: 18.0 };
    const WINTER: SunTimes = SunTimes { rise: 6.6, set: 17.25 }; // a short day

    #[test]
    fn clock_time_puts_the_sky_at_the_local_clock_time() {
        for sun in [EQUINOX, WINTER] {
            for (h, m) in [(0, 0), (5, 59), (6, 36), (12, 0), (17, 15), (18, 0), (23, 30)] {
                let want = h as f64 + m as f64 / 60.0;
                let hour = phase_to_hour(sky_state(clock_time(want, sun), 0.0).phase, sun);
                let off = (hour - want).abs();
                assert!(off.min(24.0 - off) < 1e-6, "{h}:{m} showed {hour}"); // 23.999... is midnight
            }
        }
        assert!(sky_state(clock_time(12.0, EQUINOX), 0.0).daylight > 0.99, "noon is full daylight");
        assert_eq!(sky_state(clock_time(0.0, EQUINOX), 0.0).daylight, 0.0, "midnight is dark");
    }

    #[test]
    fn sunrise_and_sunset_sit_at_the_horizon_whatever_the_day_length() {
        for sun in [EQUINOX, WINTER, SunTimes { rise: 4.7, set: 21.3 }] {
            assert_eq!(hour_to_phase(sun.rise, sun), 0.0);
            assert!((hour_to_phase(sun.set, sun) - 0.5).abs() < 1e-9);
            let mut h = 0.0;
            while h < 24.0 {
                assert!((phase_to_hour(hour_to_phase(h, sun), sun) - h).abs() < 1e-9, "{h}");
                h += 0.7;
            }
        }
    }

    #[test]
    fn twilight_is_never_darker_than_midnight() {
        // The web's score: hemisphere plus direct light, each colour × intensity.
        let lum = |c: Rgb| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let light = |h: f64| {
            let s = sky_state(clock_time(h, EQUINOX), 0.0);
            lum(s.hemi_sky) + lum(s.sun_color)
        };
        let midnight = light(0.0);
        for i in 0..=48 {
            let (dawn, dusk) = (4.0 + i as f64 / 12.0, 16.0 + i as f64 / 12.0);
            assert!(light(dawn) >= midnight - 1e-9, "dawn {dawn}");
            assert!(light(dusk) >= midnight - 1e-9, "dusk {dusk}");
        }
    }

    #[test]
    fn auto_fog_dusk_to_dawn_thicker_in_winter_off_and_always() {
        assert_eq!(fog_amount(Fog::Off, -1.0, 1.0), 0.0);
        assert_eq!(fog_amount(Fog::Always, 1.0, 0.0), 1.0);
        assert_eq!(fog_amount(Fog::Auto, 1.0, 1.0), 0.0, "none at noon");
        assert_eq!(fog_amount(Fog::Auto, -0.5, 1.0), 1.0, "full on a winter night");
        assert!(fog_amount(Fog::Auto, -0.5, 0.0) < 0.5, "thin on a summer night");
    }

    #[test]
    fn the_real_sun_shines_from_its_compass_point() {
        // Noon in the south: the light sits toward the viewer (+x +z), high up.
        let noon = sky_at(0.25, 60f64.to_radians(), PI, 0.0);
        let [x, y, z] = noon.sun_pos;
        assert!(x > 0.0 && (x - z).abs() < 1e-9 && y > 32.0 && noon.sun_color[0] > 1.0, "{:?}", noon.sun_pos);
        // Morning in the east: the screen's right (+x -z).
        let [x, _, z] = sky_at(0.1, 20f64.to_radians(), PI / 2.0, 0.0).sun_pos;
        assert!(x > 0.0 && (x + z).abs() < 1e-9);
        // Night: the moon across the sky, the sun's colour gone.
        let night = sky_at(0.75, -50f64.to_radians(), PI, 0.0);
        let [x, y, _] = night.sun_pos;
        assert!(x < 0.0 && y > 29.0 && night.daylight == 0.0);
    }

    #[test]
    fn the_moon_takes_over_at_night() {
        let noon = sky_state(NOON, 0.0);
        assert!((noon.elevation - 1.0).abs() < 1e-9 && noon.sun_pos[1] > 37.9);
        let midnight = sky_state(clock_time(0.0, EQUINOX), 0.0);
        assert!(midnight.elevation < -0.99);
        assert!(midnight.sun_pos[1] > 37.9, "the moon shines from above, mirrored across the horizon");
        let moon = linear(0x9fb4ff);
        assert!((0..3).all(|i| (midnight.sun_color[i] - moon[i] * 0.12).abs() < 1e-6), "{:?}", midnight.sun_color);
        for s in [noon, midnight] {
            assert_eq!(s.sun_pos[2], 38.0 * 0.36, "z never changes, so the shadow camera's look_at never degenerates");
        }
    }
}
