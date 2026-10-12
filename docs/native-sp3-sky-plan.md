# pixel3d native — sub-project 3: sky (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The web's time of day in the native wallpaper. It covers:
- night, dawn, day and dusk colours;
- the sun turning into the moon at night;
- cloud shadows and ground fog;
- sunrise and sunset from the time zone, with a `settings.txt` place that overrides the guess;
- following the PC clock, with `--hour` and `--cycle` for testing.

**Architecture:**
- **`meadow-core`** gets `sun.rs` (NOAA sun times, the zone table, winterness, the `place` setting) and `sky.rs` (the web's `sky.js` maths, giving a `SkyState`). Both are pure functions over explicit dates and offsets, so they are tested on Linux.
- **The `meadow` crate:**
  - `sky_clock.rs` reads the system zone and the local time with `jiff` every frame and turns them into a `SkyState`;
  - `render.rs` uploads it (lights, shadow camera, water tint, fog, clear colour);
  - the shaders add the web's `cloud_shade` and `ground_fog` inside `lighting()`.

**Tech Stack:** Rust 2024, wgpu 30, glam 0.34, bytemuck, libm (core maths), jiff 0.2 (time zone), naga (shader tests). Cross-compiled from WSL to `x86_64-pc-windows-gnu`.

**Spec:** `docs/native-sp3-sky.md`. Read it first; this plan argues from it.

## Global Constraints

- **Branch and commits:**
  - Branch `feat/native-sky` (it exists, with the spec commit `55ac0c9` on top of `main` 223c8dc).
  - One idea per commit; subject style `native: …` (code) or `docs: …`.
  - **No `Co-authored-by` or any other trailer** (user rule).
  - No push and no PR without the user's OK.
- **Cargo:** every cargo command runs from `native/` with `export PATH=$HOME/.cargo/bin:$PATH` first.
- **Maths in `meadow-core`:** use `libm` for every transcendental (`sin`, `cos`, `acos`, `pow`, `exp`), never the `f64` std methods. Plain arithmetic, `%`, `rem_euclid`, `abs` and `clamp` are fine.
- **Formatting:** don't run `cargo fmt`. Match the surrounding style by hand (long lines up to ~150 columns are normal here).
- **Web constants carried over unchanged:**
  - `DAY_LENGTH` 300, `DAY_START` 0.08, `SUN_DIST` 38;
  - all palette hex values in `src/sky.js`;
  - the NOAA terms in `src/sun.js`;
  - the cloud and fog GLSL numbers;
  - shadow box ±22, near/far 1..80.
- **Colours** are linear rgb and are lerped in linear, as three.js `Color` does under colour management.
- **The user's wallpaper** runs `C:\Users\d\pixel3d\wallpaper\meadow.exe`:
  - never kill every `meadow.exe`, and **don't use `run.sh`** (it does `taskkill /IM meadow.exe`);
  - test builds run as `meadow-sky.exe`, with `$env:APPDATA` pointed at a scratch dir, so the wallpaper's log isn't truncated.
- **Never `pkill -f <pattern>`** with the pattern in the same command line.
- **Bench sparingly:** a RAM row in `docs/perf-baseline.md` only after the user OKs a run, with its duration stated.

## Review Focus

1. **Midnight, a DST change, or days of uptime:** the sky must use *today's* date and offset. That means reading the clock every frame, never caching sunrise at startup.
   - `SkyClock::state` reads the clock each call (Task 3, by design).
   - Pinned by `day_t_follows_the_mode` for the pure part.
2. **A broken `settings.txt`** (typo, `lat` only, out of range, `NaN`) must never panic the console-less wallpaper: it gives a warning and the zone guess. Pinned by `place_comes_from_the_settings_line` (Task 1).
3. **An unknown or unmappable zone** must give the offset-based place, including negative half-hour offsets. Pinned by `sun_times_follow_the_date_and_the_place` and `unknown_zones_sit_on_their_own_meridian` (Task 1).
4. **Low sun at dawn and dusk:** the moving shadow camera must keep every visible ground point between its near and far planes. Pinned by `visible_ground_stays_between_the_shadow_cameras_near_and_far_planes` (Task 2).
   - **Ruling against the spec's test list:** the spec asked that "the shadow box holds the visible ground". It doesn't, even on the web at noon: the 21:9 screen corners land 27.7 units out (24.1 on 16:9), and the box is ±22. So the far screen corners get no shadows, as they already don't on the web. The test pins what the web's `SUN_DIST` guarantees (depth only); the lateral cut is a visual check in Task 4.
5. **The log line is the exit check** for zone and rise/set, so its formatting must be right for negative offsets and times near midnight. Pinned by `log_formats_offsets_and_times` (Task 3).

---

### Task 1: Core `sun.rs` + `sky.rs`

**Files:**
- Create: `native/meadow-core/src/sun.rs`
- Create: `native/meadow-core/src/sky.rs`
- Modify: `native/meadow-core/src/lib.rs` (add `pub mod sky; pub mod sun;`)
- Modify: `native/meadow-core/src/world.rs:34` (make `lerp` `pub`, used by `sky.rs`)

**Interfaces:**
- Produces (`meadow_core::sun`):
  - `pub struct Date { pub year: i32, pub month: u8, pub day: u8 }` (Copy, Debug, PartialEq)
  - `pub struct SunTimes { pub rise: f64, pub set: f64 }` (local clock hours, Copy, Debug, PartialEq)
  - `pub fn day_of_year(d: Date) -> u32`
  - `pub fn zone_place(zone: Option<&str>, utc_offset_min: i32) -> (f64, f64)`
  - `pub fn sun_times(date: Date, utc_offset_min: i32, place: (f64, f64)) -> SunTimes`
  - `pub fn winterness(date: Date, lat: f64) -> f64`
  - `pub fn parse_place(settings: &str) -> Result<Option<(f64, f64)>, String>`
- Produces (`meadow_core::sky`):
  - `pub type Rgb = [f64; 3]`
  - `pub fn linear(hex: u32) -> Rgb`
  - `pub const DAY_LENGTH: f64`, `pub const NOON: f64`
  - `pub fn hour_to_phase(hour: f64, sun: SunTimes) -> f64`, `pub fn phase_to_hour(phase: f64, sun: SunTimes) -> f64`
  - `pub fn clock_time(hour: f64, sun: SunTimes) -> f64`, `pub fn fog_amount(elevation: f64, winter: f64) -> f64`
  - `pub struct SkyState { pub phase: f64, pub elevation: f64, pub daylight: f64, pub sun_pos: [f64; 3], pub sun_color: Rgb, pub hemi_sky: Rgb, pub background: Rgb, pub fog_color: Rgb, pub fog: f64 }` (Clone, Copy, Debug)
  - `pub fn sky_state(day_t: f64, winter: f64) -> SkyState`

- [ ] **Step 1: Write the failing tests.**

Create `native/meadow-core/src/sun.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const OCT7: Date = Date { year: 2026, month: 10, day: 7 };
    const JUNE21: Date = Date { year: 2026, month: 6, day: 21 };
    const DEC21: Date = Date { year: 2026, month: 12, day: 21 };

    fn day_length(s: SunTimes) -> f64 {
        (s.set - s.rise + 24.0).rem_euclid(24.0)
    }

    #[test]
    fn sun_times_follow_the_date_and_the_place() {
        // The web's cases (test/sky.test.js), each at the place's own UTC offset.
        let rise = sun_times(OCT7, 420, zone_place(Some("Asia/Ho_Chi_Minh"), 420)).rise;
        assert!((rise - 5.68).abs() < 0.1, "Saigon rises at 05:41, got {rise}");
        let london = zone_place(Some("Europe/London"), 60);
        assert!(day_length(sun_times(JUNE21, 60, london)) > 16.0, "long summer days");
        assert!(day_length(sun_times(DEC21, 0, london)) < 8.0, "short winter ones");
        let unknown = zone_place(Some("Mars/Olympus"), 420);
        assert!((day_length(sun_times(JUNE21, 420, unknown)) - 12.0).abs() < 0.2, "unknown zone: the equator");
        assert!(winterness(Date { year: 2026, month: 1, day: 15 }, 21.0) > 0.99 && winterness(Date { year: 2026, month: 7, day: 15 }, 21.0) < 0.01);
        assert!(winterness(Date { year: 2026, month: 7, day: 15 }, -33.0) > 0.99, "seasons flip south of the equator");
    }

    #[test]
    fn unknown_zones_sit_on_their_own_meridian() {
        assert_eq!(zone_place(None, 420), (0.0, 105.0));
        assert_eq!(zone_place(Some("Mars/Olympus"), -210), (0.0, -52.5));
        assert_eq!(zone_place(Some("Asia/Bangkok"), 420), (13.75, 100.5));
        // On its own meridian the sun rises near 06:00, whatever the offset's sign.
        for offset in [-210, 0, 330, 420] {
            let s = sun_times(OCT7, offset, zone_place(None, offset));
            assert!((s.rise - 6.0).abs() < 0.3 && (s.set - 18.0).abs() < 0.3, "offset {offset}: {s:?}");
        }
    }

    #[test]
    fn day_of_year_counts_leap_days() {
        assert_eq!(day_of_year(Date { year: 2026, month: 1, day: 1 }), 1);
        assert_eq!(day_of_year(Date { year: 2026, month: 3, day: 1 }), 60);
        assert_eq!(day_of_year(Date { year: 2024, month: 3, day: 1 }), 61);
        assert_eq!(day_of_year(Date { year: 2024, month: 12, day: 31 }), 366);
        assert_eq!(day_of_year(Date { year: 2100, month: 3, day: 1 }), 60, "2100 is not a leap year");
    }

    #[test]
    fn place_comes_from_the_settings_line() {
        assert_eq!(parse_place("place = 21.03, 105.85\n"), Ok(Some((21.03, 105.85))));
        assert_eq!(parse_place("# Hanoi\nfps = 15\n  place=21.03,105.85  \n"), Ok(Some((21.03, 105.85))));
        assert_eq!(parse_place("place = -33.87, 151.2"), Ok(Some((-33.87, 151.2))));
        assert_eq!(parse_place("# place = 1, 2\nfps = 15"), Ok(None), "a comment is not a setting");
        assert_eq!(parse_place(""), Ok(None));
        for bad in ["place = 21.03", "place = hanoi", "place = 91, 0", "place = 0, 181", "place = NaN, 0", "place = 21.03, 105.85, 3", "place ="] {
            assert!(parse_place(bad).is_err(), "{bad} should be rejected");
        }
    }
}
```

Create `native/meadow-core/src/sky.rs` with only the test module:

```rust
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
    fn auto_fog_dusk_to_dawn_thicker_in_winter() {
        // The web's 'off' and 'always' modes arrive with the settings in sub-project 5.
        assert_eq!(fog_amount(1.0, 1.0), 0.0, "none at noon");
        assert_eq!(fog_amount(-0.5, 1.0), 1.0, "full on a winter night");
        assert!(fog_amount(-0.5, 0.0) < 0.5, "thin on a summer night");
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
```

Add both modules to `native/meadow-core/src/lib.rs` (alphabetical, as the file is):

```rust
pub mod frame_pacer;
pub mod grass;
pub mod rng;
pub mod sky;
pub mod sun;
pub mod world;
```

- [ ] **Step 2: Run the tests and check that they fail.**

Run: `cd native && cargo test -p meadow-core 2>&1 | tail -20`
Expected: FAIL to compile, with errors like `cannot find type 'Date' in this scope` and `cannot find function 'sky_state'`.

- [ ] **Step 3: Write `sun.rs`** above its test module.

```rust
//! Sunrise and sunset in local clock hours, ported from the web's `src/sun.js`. The place is
//! guessed from the time zone (an IANA zone is named after a city) unless settings.txt names one.
//! Dates and UTC offsets come in as plain values, never the machine's, so tests run the same
//! anywhere.
use std::f64::consts::PI;

use libm::{acos, cos, sin};

// IANA zone → (lat, lon), as the web's table.
const ZONES: &[(&str, f64, f64)] = &[
    ("Asia/Ho_Chi_Minh", 10.8, 106.7),
    ("Asia/Saigon", 10.8, 106.7),
    ("Asia/Bangkok", 13.75, 100.5),
    ("Asia/Phnom_Penh", 11.55, 104.9),
    ("Asia/Vientiane", 17.97, 102.6),
    ("Asia/Jakarta", -6.2, 106.8),
    ("Asia/Singapore", 1.35, 103.8),
    ("Asia/Kuala_Lumpur", 3.14, 101.7),
    ("Asia/Manila", 14.6, 121.0),
    ("Asia/Hong_Kong", 22.3, 114.2),
    ("Asia/Shanghai", 31.2, 121.5),
    ("Asia/Taipei", 25.03, 121.6),
    ("Asia/Seoul", 37.57, 127.0),
    ("Asia/Tokyo", 35.68, 139.7),
    ("Asia/Kolkata", 22.57, 88.4),
    ("Asia/Calcutta", 22.57, 88.4),
    ("Asia/Dubai", 25.2, 55.3),
    ("Europe/London", 51.5, -0.13),
    ("Europe/Paris", 48.86, 2.35),
    ("Europe/Berlin", 52.52, 13.4),
    ("Europe/Madrid", 40.42, -3.7),
    ("Europe/Rome", 41.9, 12.5),
    ("Europe/Amsterdam", 52.37, 4.9),
    ("Europe/Warsaw", 52.23, 21.0),
    ("Europe/Prague", 50.08, 14.4),
    ("Europe/Stockholm", 59.33, 18.07),
    ("Europe/Helsinki", 60.17, 24.94),
    ("Europe/Moscow", 55.76, 37.6),
    ("America/New_York", 40.71, -74.0),
    ("America/Chicago", 41.88, -87.6),
    ("America/Denver", 39.74, -105.0),
    ("America/Los_Angeles", 34.05, -118.2),
    ("America/Toronto", 43.65, -79.4),
    ("America/Vancouver", 49.28, -123.1),
    ("America/Mexico_City", 19.43, -99.1),
    ("America/Sao_Paulo", -23.55, -46.6),
    ("America/Buenos_Aires", -34.6, -58.4),
    ("Australia/Sydney", -33.87, 151.2),
    ("Australia/Melbourne", -37.81, 145.0),
    ("Pacific/Auckland", -36.85, 174.8),
    ("Africa/Cairo", 30.04, 31.2),
    ("Africa/Johannesburg", -26.2, 28.0),
];

const RAD: f64 = PI / 180.0;

/// A local calendar date.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Date {
    pub year: i32,
    pub month: u8, // 1-12
    pub day: u8,
}

/// Local clock hours, 0-24.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunTimes {
    pub rise: f64,
    pub set: f64,
}

pub fn day_of_year(d: Date) -> u32 {
    const BEFORE: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = d.year % 4 == 0 && (d.year % 100 != 0 || d.year % 400 == 0);
    BEFORE[d.month as usize - 1] + d.day as u32 + (leap && d.month > 2) as u32
}

/// (lat, lon) for a time zone. Unknown zones get the equator at the zone's own meridian: sunrise
/// near 06:00 and sunset near 18:00 all year.
pub fn zone_place(zone: Option<&str>, utc_offset_min: i32) -> (f64, f64) {
    match zone.and_then(|z| ZONES.iter().find(|(name, ..)| *name == z)) {
        Some(&(_, lat, lon)) => (lat, lon),
        None => (0.0, utc_offset_min as f64 / 60.0 * 15.0),
    }
}

/// NOAA's approximation, a few minutes off at most. Polar day and night are clamped to 1-23 h of
/// daylight, so the sky keeps a sunrise and a sunset. `utc_offset_min` is east-positive (+420 for
/// UTC+7): the opposite sign of the web's `getTimezoneOffset()`.
pub fn sun_times(date: Date, utc_offset_min: i32, (lat, lon): (f64, f64)) -> SunTimes {
    let n = day_of_year(date) as f64;
    let g = 2.0 * PI / 365.0 * (n - 1.0);
    let decl = 0.006918 - 0.399912 * cos(g) + 0.070257 * sin(g) - 0.006758 * cos(2.0 * g) + 0.000907 * sin(2.0 * g);
    let eq_time = 229.18 * (0.000075 + 0.001868 * cos(g) - 0.032077 * sin(g) - 0.014615 * cos(2.0 * g) - 0.040849 * sin(2.0 * g));
    let cos_h = (cos(90.833 * RAD) - sin(lat * RAD) * sin(decl)) / (cos(lat * RAD) * cos(decl));
    let half = (acos(cos_h.clamp(-1.0, 1.0)) / RAD / 15.0).clamp(0.5, 11.5); // hours
    let noon = 12.0 - eq_time / 60.0 - lon / 15.0 + utc_offset_min as f64 / 60.0;
    SunTimes { rise: (noon - half).rem_euclid(24.0), set: (noon + half).rem_euclid(24.0) }
}

/// 0 in midsummer, 1 in midwinter (the fog season), flipped south of the equator.
pub fn winterness(date: Date, lat: f64) -> f64 {
    let n = day_of_year(date) as f64 + if lat < 0.0 { 182.0 } else { 0.0 };
    0.5 + 0.5 * cos((n - 15.0) / 365.0 * 2.0 * PI)
}

/// The `place = lat, lon` line of settings.txt (degrees, north and east positive). Ok(None): no
/// such line. Err: a line that can't be used, with a message for the log. Never panics.
pub fn parse_place(settings: &str) -> Result<Option<(f64, f64)>, String> {
    let line = settings.lines().map(str::trim).filter(|l| !l.starts_with('#')).find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == "place"));
    let Some((_, value)) = line else { return Ok(None) };
    let value = value.trim();
    let bad = || format!("place = {value:?}: expected `lat, lon` in degrees, like `21.03, 105.85`");
    let (lat, lon) = value.split_once(',').ok_or_else(bad)?;
    let (lat, lon): (f64, f64) = (lat.trim().parse().map_err(|_| bad())?, lon.trim().parse().map_err(|_| bad())?);
    // Written so NaN fails too.
    if !(lat.abs() <= 90.0 && lon.abs() <= 180.0) {
        return Err(bad());
    }
    Ok(Some((lat, lon)))
}
```

- [ ] **Step 4: Write `sky.rs`** above its test module, and make `world::lerp` public.

In `native/meadow-core/src/world.rs`, change `pub(crate) fn lerp` to `pub fn lerp`.

```rust
//! Time of day, ported 1:1 from the web's `src/sky.js`. One directional light is the sun by day
//! and the moon by night; the sky, ambient and fog colours follow the sun's elevation. Colours are
//! linear rgb, lerped in linear (three.js Colors are linear under its colour management).
use std::f64::consts::PI;

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

/// The web's 'auto' ground fog: rolls in at dusk and burns off after sunrise, thicker in winter
/// (winterness, 0-1).
pub fn fog_amount(elevation: f64, winter: f64) -> f64 {
    (1.0 - smoothstep(elevation, -0.05, 0.2)) * lerp(0.35, 1.0, winter)
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

/// The web's `update(t, dayT, fog)` for `fog.mode = 'auto'`.
pub fn sky_state(day_t: f64, winter: f64) -> SkyState {
    let phase = (day_t / DAY_LENGTH + DAY_START) % 1.0;
    let angle = (day_t / DAY_LENGTH + DAY_START) * PI * 2.0;
    let elevation = sin(angle);
    let is_day = elevation >= 0.0;
    let dir = if is_day { 1.0 } else { -1.0 };
    let sun_pos = [cos(angle) * SUN_DIST * dir, elevation.abs() * SUN_DIST, SUN_DIST * 0.36];
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
    SkyState { phase, elevation, daylight, sun_pos, sun_color, hemi_sky, background, fog_color, fog: fog_amount(elevation, winter) }
}
```

- [ ] **Step 5: Run the tests and check that they pass.**

Run: `cd native && cargo test -p meadow-core 2>&1 | tail -15`
Expected: `test result: ok. 30 passed` (21 old + 4 sun + 5 sky), no warnings.
- If `the_moon_takes_over_at_night`'s exact `assert_eq!` on `sun_pos[2]` fails by rounding, that is a bug in the port: `SUN_DIST * 0.36` must be computed the same way in both places.

- [ ] **Step 6: Run the whole suite, then commit.**

Run: `cd native && cargo test 2>&1 | grep 'test result'`
Expected: two `ok` lines, 30 + 11 = 41 tests.

```bash
git add native/meadow-core/src/sun.rs native/meadow-core/src/sky.rs native/meadow-core/src/lib.rs native/meadow-core/src/world.rs
git commit -m "native: sky and sun maths in core (port of sky.js, sun.js and their tests)"
```

---

### Task 2: The sky on the GPU (moving light, cloud shadows, ground fog)

**Files:**
- Modify: `native/meadow/src/camera.rs`: `light_view_proj(sun)`; remove `SUN_POS`; add a test module.
- Modify: `native/meadow/src/render.rs`:
  - Globals `fog`; `set_sky`; `render(…, sky)`; clear colour;
  - remove `BACKGROUND`, `SUN`, `HEMI_SKY`;
  - `linear` wraps the core one;
  - a new test.
- Modify: `native/meadow/src/shaders/common.wgsl`: the Globals `fog` field, `cloud_shade` and `ground_fog`.
- Modify: `native/meadow/src/shaders/scene.wgsl`: `lighting(…, world)` and its 4 callers.
- Modify: `native/meadow/src/main.rs`: pass a noon `SkyState` until Task 3.

**Interfaces:**
- Consumes: `meadow_core::sky::{sky_state, SkyState, NOON, linear, Rgb}` from Task 1.
- Produces:
  - `camera::light_view_proj(sun: Vec3) -> Mat4`
  - `Renderer::render(&mut self, device, queue, out: &wgpu::TextureView, time: f32, sky: &SkyState)`

- [ ] **Step 1: Write the failing tests.**

Append to `native/meadow/src/camera.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use meadow_core::sky;

    // Where the corners of a 21:9 screen (the widest the grass culling plans for) meet y = 0.
    fn ground_corners() -> Vec<Vec3> {
        let inv = view().inverse();
        let forward = inv.transform_vector3(Vec3::NEG_Z);
        [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
            .into_iter()
            .map(|(sx, sy)| {
                let o = inv.transform_point3(Vec3::new(sx * half_width(21, 9), sy * VIEW_HEIGHT / 2.0, 0.0));
                o + forward * (-o.y / forward.y)
            })
            .collect()
    }

    #[test]
    fn visible_ground_stays_between_the_shadow_cameras_near_and_far_planes() {
        // The corners land ~27.7 units out: past the ±22 box sideways (as on the web), but every
        // depth must stay in 1..80, or the ground there would shadow or light wrongly.
        let corners = ground_corners();
        assert!(corners.iter().all(|c| (27.0..28.5).contains(&c.x.hypot(c.z))), "{corners:?}");
        for day_t in 0..300 {
            let p = sky::sky_state(day_t as f64, 0.0).sun_pos;
            let light = light_view_proj(Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32));
            for c in &corners {
                let z = light.project_point3(*c).z;
                assert!((0.05..0.95).contains(&z), "day_t {day_t}: corner {c} at shadow depth {z}");
            }
        }
    }
}
```

Append to the test module in `native/meadow/src/render.rs`:

```rust
    #[test]
    fn globals_match_the_wgsl_struct() {
        let src = include_str!("shaders/common.wgsl");
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        let span = module
            .types
            .iter()
            .find_map(|(_, t)| match t.inner {
                naga::TypeInner::Struct { span, .. } if t.name.as_deref() == Some("Globals") => Some(span),
                _ => None,
            })
            .expect("Globals in common.wgsl");
        assert_eq!(span as usize, size_of::<Globals>());
        assert_eq!(size_of::<Globals>(), 304, "three mat4 and seven vec4");
    }
```

- [ ] **Step 2: Run the tests and check that they fail.**

Run: `cd native && cargo test -p meadow 2>&1 | tail -15`
Expected: FAIL to compile: `light_view_proj` takes 0 arguments but 1 was supplied.
- The depth range 0.05..0.95 is wide on purpose. Measured beforehand: depth 13.7–67.1 units, i.e. 0.16–0.84.

- [ ] **Step 3: Move the light in `camera.rs`.**

Delete the `SUN_POS` constant and its doc comment (lines 11–12). Correct the `SHADOW_HALF` comment, which the new test disproves:

```rust
const SHADOW_HALF: f32 = 22.0; // as the web; the 21:9 screen corners land ~27.7 units out, past it (no shadows there)
```

Replace `light_view_proj`:

```rust
/// The sun's (or moon's) shadow camera, as the web's: ±22 box, near/far 1..80. The light's z is
/// always 13.68 (meadow_core::sky), so it never sits on the y axis and look_at with +Y is defined.
pub fn light_view_proj(sun: Vec3) -> Mat4 {
    let proj = orthographic(-SHADOW_HALF, SHADOW_HALF, -SHADOW_HALF, SHADOW_HALF, 1.0, 80.0);
    proj * look_at_mat4(sun, Vec3::ZERO, Vec3::Y)
}
```

- [ ] **Step 4: Add the shader side.**

`common.wgsl`: in `struct Globals`, after `params: vec4f,      // x: time (s)`, add:

```wgsl
    fog: vec4f,         // rgb: ground fog colour, a: amount (0: none)
```

`common.wgsl`: append after `value_noise`:

```wgsl
// Hard-edged value-noise blobs drifting with the wind (the web's sky.js); they only dim direct light.
fn cloud_shade(world: vec3f) -> f32 {
    let t = g.params.x;
    let p = world.xz * 0.15 + vec2f(t * 0.05, t * 0.02);
    let n = value_noise(p) * 0.65 + value_noise(p * 2.3) * 0.35;
    return mix(1.0, 0.35, smoothstep(0.58, 0.6, n));
}

// Low fog by absolute height, thinning upward: hollows and the pond fill first, the crowns get a
// light haze; drifting patches.
fn ground_fog(world: vec3f) -> f32 {
    let t = g.params.x;
    let layer = exp(-0.5 * max(world.y, 0.0));
    let p = world.xz * 0.18 + vec2f(t * 0.04, t * 0.015);
    let n = value_noise(p + 40.0) * 0.65 + value_noise(p * 2.3 + 7.0) * 0.35;
    return g.fog.a * layer * mix(0.45, 1.0, n) * 0.5;
}
```

`scene.wgsl`: replace `lighting` and its comment:

```wgsl
// Sun (or moon) through the toon ramp, shadow and cloud shade, plus the sky/ground hemisphere light
// (Lambert, /π); then the ground fog over it, as the web's patched toon material.
fn lighting(diffuse: vec3f, normal: vec3f, ramp: f32, lit: f32, world: vec3f) -> vec3f {
    let hemi = mix(g.hemi_ground.rgb, g.hemi_sky.rgb, normal.y * 0.5 + 0.5);
    let color = diffuse / PI * (g.sun_color.rgb * ramp * lit * cloud_shade(world) + hemi);
    return mix(color, g.fog.rgb, ground_fog(world));
}
```

Then append `, in.world` as the last argument of the four `lighting(` calls (`fs_toon`, `fs_ground`, `fs_water`, `fs_grass`):

```bash
sed -i 's/\(let color = lighting(.*\), lit);/\1, lit, in.world);/' native/meadow/src/shaders/scene.wgsl
grep -n 'lighting(' native/meadow/src/shaders/scene.wgsl
```
Expected: 5 lines (the definition and 4 calls), each call ending `lit, in.world);`.

- [ ] **Step 5: Update the Rust side in `render.rs`.**

Imports: add `use meadow_core::sky::{self, SkyState};` next to `use meadow_core::world;`.

Replace the palette head (the comment line and the `BACKGROUND`, `SUN`, `HEMI_SKY`, `HEMI_GROUND` constants):

```rust
// Web palette (sRGB hex). The time-of-day lights come from meadow_core::sky each frame; the ground
// half of the hemisphere light stays fixed, as the web's.
const HEMI_GROUND: (u32, f32) = (0x3a3326, 1.2);
```

Replace `fn linear` (its test stays as is):

```rust
fn linear(hex: u32) -> Vec3 {
    vec3(sky::linear(hex))
}

fn vec3(c: sky::Rgb) -> Vec3 {
    Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32)
}
```

In `struct Globals`, add after `params: [f32; 4],`:

```rust
    fog: [f32; 4], // rgb: ground fog colour, a: amount
```

In `struct Renderer`, add after `globals_group`:

```rust
    background: Vec3, // this frame's sky: the scene pass's clear colour
```

In `Renderer::new`, replace the `let (sun, sky, ground) = …;` line and the `let globals = Globals { … };` block:

```rust
        let ground = linear(HEMI_GROUND.0) * HEMI_GROUND.1;
        // The time-of-day fields are set by set_sky, before every upload.
        let globals = Globals {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(), // set by resize
            view: camera::view().to_cols_array_2d(),
            hemi_ground: ground.extend(0.0).into(),
            ..Zeroable::zeroed()
        };
```

and add `background: Vec3::ZERO,` to the `Self { … }` literal after `globals_group,`.

Add the method after `set_camera`:

```rust
    /// This frame's time of day: lights, shadow camera, water tint, ground fog and the sky colour.
    fn set_sky(&mut self, sky: &SkyState) {
        let sun = vec3(sky.sun_pos);
        self.globals.sun_dir = sun.normalize().extend(0.0).into();
        self.globals.light_view_proj = camera::light_view_proj(sun).to_cols_array_2d();
        self.globals.sun_color = vec3(sky.sun_color).extend(0.0).into();
        self.globals.hemi_sky = vec3(sky.hemi_sky).extend(0.0).into();
        self.background = vec3(sky.background);
        // White lerped 10% toward the sky, as the web's water material.
        self.globals.water_tint = Vec3::ONE.lerp(self.background, 0.1).extend(0.0).into();
        self.globals.fog = vec3(sky.fog_color).extend(sky.fog as f32).into();
    }
```

Change `render`'s signature and first line:

```rust
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, out: &wgpu::TextureView, time: f32, sky: &SkyState) {
        self.set_sky(sky);
        self.globals.params[0] = time;
```

In the scene pass, replace `linear(BACKGROUND).extend(1.0)` with `self.background.extend(1.0)`.

- [ ] **Step 6: Keep `main.rs` at noon until Task 3.**

In `App::redraw`, replace the `match gpu.frame(…)` head:

```rust
        let sky = meadow_core::sky::sky_state(meadow_core::sky::NOON, 0.0); // the clock arrives in the next commit
        match gpu.frame(|device, queue, view| renderer.render(device, queue, view, time, &sky)) {
```

- [ ] **Step 7: Run the tests and check that they pass.**

Run: `cd native && cargo test 2>&1 | grep -E 'test result|warning|error|FAILED'`
Expected: two `ok` lines, 30 (core) + 13 (meadow: 11 old, `shaders_validate` among them, + `globals_match_the_wgsl_struct` + `visible_ground_stays_…`) = 43 tests. No warnings: no unused `linear` or `SUN_POS`.
- If `grep -rn 'SUN_POS\|BACKGROUND' native/meadow/src` prints anything, remove it.

- [ ] **Step 8: Build for Windows (catches target-only errors), then commit.**

Run: `cd native && cargo build --release --target x86_64-pc-windows-gnu 2>&1 | tail -3`
Expected: `Finished`.

```bash
git add native/meadow/src/camera.rs native/meadow/src/render.rs native/meadow/src/main.rs native/meadow/src/shaders/common.wgsl native/meadow/src/shaders/scene.wgsl
git commit -m "native: sky state on the GPU — moving sun/moon and shadow camera, cloud shadows, ground fog"
```

---

### Task 3: The clock (`sky_clock.rs`, jiff, `--hour` / `--cycle`, `settings.txt`, startup log)

**Files:**
- Create: `native/meadow/src/sky_clock.rs`
- Modify: `native/meadow/Cargo.toml` (add `jiff`)
- Modify: `native/meadow/src/main.rs`:
  - `mod sky_clock;`;
  - the flags and `settings_text()`;
  - an `App.sky` field;
  - `redraw` uses it;
  - the arg comment.

**Interfaces:**
- Consumes:
  - `meadow_core::sun::{Date, SunTimes, sun_times, winterness, zone_place, parse_place}`
  - `meadow_core::sky::{clock_time, sky_state, SkyState, DAY_LENGTH}`
  - `Renderer::render(…, time, &SkyState)`
- Produces:
  - `sky_clock::Mode { Clock, Hour(f64), Cycle(Option<f64>) }`
  - `SkyClock::new(mode: Mode, settings: Option<&str>) -> SkyClock`
  - `SkyClock::state(&self, anim_time: f64) -> SkyState`

- [ ] **Step 1: Write the failing tests.**

Create `native/meadow/src/sky_clock.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SUN: SunTimes = SunTimes { rise: 6.0, set: 18.0 };

    fn shown_hour(day_t: f64) -> f64 {
        sky::phase_to_hour(sky::sky_state(day_t, 0.0).phase, SUN)
    }

    #[test]
    fn day_t_follows_the_mode() {
        // The clock follows the local hour; a pinned hour ignores it.
        assert!((shown_hour(day_t(Mode::Clock, 13.0, SUN, 0.0, 99.0)) - 13.0).abs() < 1e-6);
        assert!((shown_hour(day_t(Mode::Hour(6.25), 13.0, SUN, 0.0, 99.0)) - 6.25).abs() < 1e-6);
        // The cycle starts at its hour and comes back to it after one 300 s day.
        let start = sky::clock_time(18.5, SUN);
        let cycle = |t| shown_hour(day_t(Mode::Cycle(Some(18.5)), 13.0, SUN, start, t));
        assert!((cycle(0.0) - 18.5).abs() < 1e-6);
        assert!((cycle(sky::DAY_LENGTH) - 18.5).abs() < 1e-6, "a full day later");
        assert!((cycle(sky::DAY_LENGTH / 4.0) - 18.5).abs() > 1.0, "and moves in between");
    }

    #[test]
    fn log_formats_offsets_and_times() {
        assert_eq!(utc_offset(420), "+07:00");
        assert_eq!(utc_offset(-210), "-03:30");
        assert_eq!(utc_offset(0), "+00:00");
        assert_eq!(hhmm(5.68), "05:41");
        assert_eq!(hhmm(17.999), "18:00");
        assert_eq!(hhmm(23.999), "00:00");
    }
}
```

and add `mod sky_clock;` to `main.rs` after `mod render;`.

- [ ] **Step 2: Run the tests and check that they fail.**

Run: `cd native && cargo test -p meadow 2>&1 | tail -10`
Expected: FAIL to compile: `cannot find function 'day_t'`, `cannot find type 'Mode'`.

- [ ] **Step 3: Add jiff and write `sky_clock.rs`.**

In `native/meadow/Cargo.toml`, after the `meadow-core` line:

```toml
# The system time zone (Windows zone → IANA via CLDR) and local time, DST included. Default
# features: on Windows that bundles the tzdb (~100–200 KB of exe).
jiff = "0.2"
```

Above the test module in `sky_clock.rs`:

```rust
//! The time of day on screen: the PC's local clock (or, for testing, a pinned hour or the web's
//! 300 s day), at the place guessed from the system time zone unless settings.txt names one.
//! Read every frame, so midnight, DST changes and long pauses need nothing special.
use jiff::Timestamp;
use jiff::tz::TimeZone;
use meadow_core::sky::{self, SkyState};
use meadow_core::sun::{self, Date, SunTimes};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Clock,              // the PC's local time
    Hour(f64),          // --hour H: pinned
    Cycle(Option<f64>), // --cycle: the web's 300 s day, from --hour H or from the clock
}

pub struct SkyClock {
    mode: Mode,
    tz: TimeZone,
    place: (f64, f64),
    cycle_start: f64, // cycle mode: day_t at animation time 0
}

struct Now {
    date: Date,
    hour: f64,
    offset_min: i32,
}

fn now(tz: &TimeZone) -> Now {
    let z = Timestamp::now().to_zoned(tz.clone());
    let seconds = z.second() as f64 + z.subsec_nanosecond() as f64 * 1e-9;
    Now {
        date: Date { year: z.year() as i32, month: z.month() as u8, day: z.day() as u8 },
        hour: z.hour() as f64 + z.minute() as f64 / 60.0 + seconds / 3600.0,
        offset_min: z.offset().seconds() / 60,
    }
}

fn day_t(mode: Mode, now_hour: f64, sun: SunTimes, cycle_start: f64, anim_time: f64) -> f64 {
    match mode {
        Mode::Clock => sky::clock_time(now_hour, sun),
        Mode::Hour(h) => sky::clock_time(h, sun),
        Mode::Cycle(_) => (cycle_start + anim_time) % sky::DAY_LENGTH,
    }
}

fn utc_offset(min: i32) -> String {
    format!("{}{:02}:{:02}", if min < 0 { '-' } else { '+' }, min.abs() / 60, min.abs() % 60)
}

fn hhmm(hour: f64) -> String {
    let m = (hour * 60.0).round() as u32 % (24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

impl SkyClock {
    /// `settings`: the text of %APPDATA%\pixel3d\settings.txt, if there is one.
    pub fn new(mode: Mode, settings: Option<&str>) -> Self {
        let tz = TimeZone::try_system().unwrap_or_else(|e| {
            log::warn!("sky: no system time zone ({e}), using UTC");
            TimeZone::UTC
        });
        let now = now(&tz);
        let zone = tz.iana_name();
        let guess = || (sun::zone_place(zone, now.offset_min), "zone guess");
        let (place, source) = match settings.map_or(Ok(None), sun::parse_place) {
            Ok(Some(place)) => (place, "settings.txt"),
            Ok(None) => guess(),
            Err(e) => {
                log::warn!("settings.txt: {e}; using the zone guess");
                guess()
            }
        };
        let today = sun::sun_times(now.date, now.offset_min, place);
        let cycle_start = match mode {
            Mode::Cycle(start) => sky::clock_time(start.unwrap_or(now.hour), today),
            _ => 0.0,
        };
        let mode_name = match mode {
            Mode::Clock => "clock".to_string(),
            Mode::Hour(h) => format!("hour {h:.2}"),
            Mode::Cycle(_) => "cycle".to_string(),
        };
        log::info!(
            "sky: zone {} (UTC{}), place {:.2}, {:.2} ({source}), rise {}, set {}, mode {mode_name}",
            zone.unwrap_or("unknown"),
            utc_offset(now.offset_min),
            place.0,
            place.1,
            hhmm(today.rise),
            hhmm(today.set),
        );
        Self { mode, tz, place, cycle_start }
    }

    /// The sky for right now; `anim_time` (s) only drives the cycle mode.
    pub fn state(&self, anim_time: f64) -> SkyState {
        let now = now(&self.tz);
        let sun = sun::sun_times(now.date, now.offset_min, self.place);
        sky::sky_state(day_t(self.mode, now.hour, sun, self.cycle_start, anim_time), sun::winterness(now.date, self.place.0))
    }
}
```

- [ ] **Step 4: Wire it into `main.rs`.**

Replace the arg comment (lines 1–3):

```rust
// The meadow: ground, paths, pond, trees, rocks and grass, toon-lit with pixel outlines, at the
// PC's local time of day. Args: --fps N (60/30/20/15, default 15; 0 = no cap, vsync),
// --pixel-size 2|3|4 (default 3), --wallpaper (Windows: behind the desktop icons, with a tray
// menu), --hour H (pin the sky at local hour H, 0-24, e.g. 18.5), --cycle (the web's 300 s day,
// from --hour or the clock). %APPDATA%\pixel3d\settings.txt: `place = lat, lon` overrides the
// place guessed from the time zone (sunrise and sunset).
```

In `struct App`, add after `time: f64, // animation clock, s`:

```rust
    sky: sky_clock::SkyClock,
```

In `App::redraw`, replace the Task 2 noon line:

```rust
        let sky = self.sky.state(self.time + dt);
```

After `log_file()`, add:

```rust
fn settings_text() -> Option<String> {
    std::fs::read_to_string(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("pixel3d").join("settings.txt")).ok()
}
```

In `main()`, after the `pixel_size` match, add:

```rust
    let hour = arg(&args, "--hour").map(|v| v.parse().ok().filter(|h: &f64| (0.0..=24.0).contains(h)).unwrap_or_else(|| panic!("--hour must be 0 to 24, like 18.5, got {v}")));
    let mode = if args.iter().any(|a| a == "--cycle") { sky_clock::Mode::Cycle(hour) } else { hour.map_or(sky_clock::Mode::Clock, sky_clock::Mode::Hour) };
```

and in the `App { … }` literal, after `time: 0.0,`:

```rust
        sky: sky_clock::SkyClock::new(mode, settings_text().as_deref()),
```

- [ ] **Step 5: Run the tests and check that they pass.**

Run: `cd native && cargo test 2>&1 | grep -E 'test result|warning|error|FAILED'`
Expected: two `ok` lines, 30 + 15 = 45 tests, no warnings.

- [ ] **Step 6: Windows build and log check.**

```bash
cd native && cargo build --release --target x86_64-pc-windows-gnu 2>&1 | tail -2
cp target/x86_64-pc-windows-gnu/release/meadow.exe /mnt/c/Users/d/pixel3d/meadow-sky.exe
cat > /mnt/c/Users/d/pixel3d/sky-run.ps1 <<'EOF'
param([string]$hour, [switch]$cycle)
$env:APPDATA = "C:\Users\d\pixel3d\appdata-sky"   # the wallpaper keeps its own log
New-Item -ItemType Directory -Force "$env:APPDATA\pixel3d" | Out-Null
$a = @('--pixel-size', '2')
if ($hour) { $a += @('--hour', $hour) }
if ($cycle) { $a += '--cycle' }
Start-Process C:\Users\d\pixel3d\meadow-sky.exe -ArgumentList $a
EOF
setsid nohup powershell.exe -NoProfile -File 'C:\Users\d\pixel3d\sky-run.ps1' -hour 12 >/dev/null 2>&1 </dev/null &
sleep 8; cat /mnt/c/Users/d/pixel3d/appdata-sky/pixel3d/log.txt
```

Expected:
- a log line like `sky: zone Asia/Bangkok (UTC+07:00), place 13.75, 100.50 (zone guess), rise 05:5x, set 17:4x, mode hour 12.00`;
- `adapter…`, `grass…` and `art…` lines;
- no `ERROR` or panic.

Then stop the test exe by name only: `timeout 20 powershell.exe -NoProfile -c "Stop-Process -Name meadow-sky"`.

- [ ] **Step 7: Commit.**

```bash
git add native/meadow/Cargo.toml native/Cargo.lock native/meadow/src/sky_clock.rs native/meadow/src/main.rs
git commit -m "native: sky follows the local clock — zone guess or settings.txt place, --hour and --cycle"
```

---

### Task 4: Windows check against the web (the exit criteria)

**Files:**
- Create (Windows scratch, not in git): `C:\Users\d\pixel3d\sky-*.png`, `web-sky-*.png`, and `/tmp/web-sky.mjs`.

- [ ] **Step 1: Native screenshots at the four hours.**

For each `H` in `6 12 18.5 2`:

```bash
setsid nohup powershell.exe -NoProfile -File 'C:\Users\d\pixel3d\sky-run.ps1' -hour $H >/dev/null 2>&1 </dev/null &
sleep 8
timeout 60 powershell.exe -NoProfile -File 'C:\Users\d\pixel3d\check.ps1' -out "C:\Users\d\pixel3d\sky-$H.png" -w 1280 -h 720 -name meadow-sky
timeout 20 powershell.exe -NoProfile -c "Stop-Process -Name meadow-sky"
```
Expected: `client 1264x681` or similar each time, and four PNGs.

- [ ] **Step 2: Web screenshots at the same hours and zone.**

Use the zone from the native log line (Task 3, Step 6), e.g. `Asia/Bangkok`. Start the dev server from the repo root:

```bash
npm run dev -- --host 127.0.0.1 --port 5173 --strictPort
```

(run it in the background). Then write `/tmp/web-sky.mjs`. It is `/tmp/web-shot.mjs` plus a `timezoneId` (WSL runs in UTC), an hour argument and an output path:

```js
const { chromium } = await import('/home/dung/.npm/_npx/e41f203b7505f1fb/node_modules/playwright-core/index.mjs');
const [w, h, hour, zone, out] = [+process.argv[2], +process.argv[3], process.argv[4], process.argv[5], process.argv[6]];
const browser = await chromium.launch({
  executablePath: process.env.HOME + '/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell',
  args: ['--use-angle=gl', '--ignore-gpu-blocklist', '--enable-gpu', '--no-sandbox'],
  env: { ...process.env, GALLIUM_DRIVER: 'd3d12', MESA_LOADER_DRIVER_OVERRIDE: 'd3d12', LD_LIBRARY_PATH: '/usr/lib/wsl/lib' },
});
const page = await browser.newPage({ viewport: { width: w, height: h }, timezoneId: zone });
await page.goto('http://127.0.0.1:5173/?px=2');
await page.waitForTimeout(3000); // load first: the slider's sky keeps moving at 300 s a day (~14 min per 3 s)
await page.evaluate((v) => { const s = document.querySelector('#time input'); s.value = v; s.dispatchEvent(new Event('input')); }, hour);
await page.waitForTimeout(200);
await page.screenshot({ path: out });
await browser.close();
```

For each `H`: `node /tmp/web-sky.mjs 1264 681 $H Asia/Bangkok /mnt/c/Users/d/pixel3d/web-sky-$H.png`.
Expected: four PNGs.

When done, stop vite by PID: find it with `pgrep -a node`, then use `kill <pid>`. Never `pkill -f vite` in the same command line.

- [ ] **Step 3: Compare.**

Read each pair (`sky-H.png` beside `web-sky-H.png`) with the Read tool. Check:
- sky colour (night blue / dusk orange / day blue);
- sun vs moon light and the shadow direction (morning shadows point away from the sun's side, and flip in the evening);
- cloud shadow blobs present (their positions differ: the cloud clocks differ);
- fog in the hollows at 18:30 and 02:00 (thicker in winter, thin now in October);
- far-corner shadow cut at 06:00 / 18:30 (expected as on the web; report if it's more visible).
- expected web-only extras: insects and the post clock (sub-projects 4 and 5).

Report the differences to the user with the pairs; their visual OK is the gate.

- [ ] **Step 4: Rise/set check against the web's code.**

```bash
cd /home/dung/Study/pixel3d && TZ=Asia/Bangkok node --input-type=module -e "import { placeFor, sunTimes } from './src/sun.js'; const d = new Date(); const s = sunTimes(d, placeFor('Asia/Bangkok', d)); const f = (h) => { const m = Math.round(h * 60) % 1440; return String(Math.floor(m / 60)).padStart(2, '0') + ':' + String(m % 60).padStart(2, '0'); }; console.log('web rise', f(s.rise), 'set', f(s.set));"
```

Expected: the same HH:MM as the native log line (both round total minutes).

- [ ] **Step 5: One `--cycle` run through dusk.**

Run `sky-run.ps1 -cycle -hour 17`. Capture at +0 s, +25 s and +50 s with `check.ps1` (`-out …sky-cycle-0.png` etc.). That covers about 17:00 to past sunset, at 12 s of cycle per clock hour around dusk. Expected: the light warms, dims and turns blue, with no pop at the sun/moon swap. Stop it by name.

- [ ] **Step 6: Clean up and report.**

- Remove the scratch files: `sky-run.ps1`, the `appdata-sky` folder, `meadow-sky.exe` and the PNGs, after the user has seen them.
- Ask the user:
  - (a) visual OK?
  - (b) update the wallpaper exe (keeping `.bak`)?
  - (c) a RAM row (one ~2 min run: 60 s settle + 20 s CPU)?
  - (d) push + PR?

---

## After the last task

- Whole-branch review: one fresh reviewer, focused on this plan's Review Focus list.
- Fix Critical/Important findings TDD-style; minor ones are listed to the user.
- Then follow superpowers:finishing-a-development-branch. Push and PR only on the user's OK.
