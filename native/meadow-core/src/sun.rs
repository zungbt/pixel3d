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
