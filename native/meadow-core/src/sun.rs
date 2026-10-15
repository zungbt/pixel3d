//! Sunrise and sunset in local clock hours, ported from the web's `src/sun.js`. The place is
//! guessed from the time zone (an IANA zone is named after a city) unless settings.txt names one.
//! Dates and UTC offsets come in as plain values, never the machine's, so tests run the same
//! anywhere.
use std::f64::consts::PI;

use libm::{acos, asin, atan2, cos, sin, sqrt, tan};

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
    let (decl, eq_time) = declination_and_eq_time(day_of_year(date) as f64 - 1.0);
    let cos_h = (cos(90.833 * RAD) - sin(lat * RAD) * sin(decl)) / (cos(lat * RAD) * cos(decl));
    let half = (acos(cos_h.clamp(-1.0, 1.0)) / RAD / 15.0).clamp(0.5, 11.5); // hours
    let noon = 12.0 - eq_time / 60.0 - lon / 15.0 + utc_offset_min as f64 / 60.0;
    SunTimes { rise: (noon - half).rem_euclid(24.0), set: (noon + half).rem_euclid(24.0) }
}

/// NOAA's fractional-year series: the sun's declination (radians) and the equation of time
/// (minutes), `days` into the year.
fn declination_and_eq_time(days: f64) -> (f64, f64) {
    let g = 2.0 * PI / 365.0 * days;
    let decl = 0.006918 - 0.399912 * cos(g) + 0.070257 * sin(g) - 0.006758 * cos(2.0 * g) + 0.000907 * sin(2.0 * g);
    let eq_time = 229.18 * (0.000075 + 0.001868 * cos(g) - 0.032077 * sin(g) - 0.014615 * cos(2.0 * g) - 0.040849 * sin(2.0 * g));
    (decl, eq_time)
}

/// Where the sun stands at local clock `hour` (NOAA, no refraction): (elevation, azimuth) in
/// radians, the azimuth clockwise from north. `utc_offset_min` as sun_times.
pub fn sun_position(date: Date, hour: f64, utc_offset_min: i32, (lat, lon): (f64, f64)) -> (f64, f64) {
    let (decl, eq_time) = declination_and_eq_time(day_of_year(date) as f64 - 1.0 + (hour - 12.0) / 24.0);
    let solar_min = hour * 60.0 + eq_time + 4.0 * lon - utc_offset_min as f64;
    let ha = (solar_min / 4.0 - 180.0) * RAD; // hour angle: 0 at solar noon, west positive
    let lat = lat * RAD;
    let elevation = asin((sin(lat) * sin(decl) + cos(lat) * cos(decl) * cos(ha)).clamp(-1.0, 1.0));
    let azimuth = (atan2(sin(ha), cos(ha) * sin(lat) - tan(decl) * cos(lat)) + PI).rem_euclid(2.0 * PI);
    (elevation, azimuth)
}

/// 0 in midsummer, 1 in midwinter (the fog season), flipped south of the equator.
pub fn winterness(date: Date, lat: f64) -> f64 {
    let n = day_of_year(date) as f64 + if lat < 0.0 { 182.0 } else { 0.0 };
    0.5 + 0.5 * cos((n - 15.0) / 365.0 * 2.0 * PI)
}

/// The `place = lat, lon` line of settings.txt (degrees, north and east positive). Ok(None): no
/// such line. Err: a line that can't be used, with a message for the log. Never panics.
pub fn parse_place(settings: &str) -> Result<Option<(f64, f64)>, String> {
    let line = settings.trim_start_matches('\u{feff}').lines().map(str::trim).filter(|l| !l.starts_with('#')).find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == "place"));
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

const EARTH_KM: f64 = 6371.0;

fn unit_vector((lat, lon): (f64, f64)) -> [f64; 3] {
    let (lat, lon) = (lat * RAD, lon * RAD);
    [cos(lat) * cos(lon), cos(lat) * sin(lon), sin(lat)]
}

/// The point `f` (0-1) of the way from `a` to `b` along the great circle (the "as the crow flies"
/// path), (lat, lon) in degrees. The same place and antipodes have no unique path: `b`.
pub fn great_circle(a: (f64, f64), b: (f64, f64), f: f64) -> (f64, f64) {
    let (u, v) = (unit_vector(a), unit_vector(b));
    let omega = acos((u[0] * v[0] + u[1] * v[1] + u[2] * v[2]).clamp(-1.0, 1.0));
    if omega < 1e-9 || PI - omega < 1e-6 {
        return b;
    }
    let (wa, wb) = (sin((1.0 - f) * omega) / sin(omega), sin(f * omega) / sin(omega));
    let p = [wa * u[0] + wb * v[0], wa * u[1] + wb * v[1], wa * u[2] + wb * v[2]];
    (asin(p[2].clamp(-1.0, 1.0)) / RAD, atan2(p[1], p[0]) / RAD)
}

/// How far (km) and which way (radians clockwise from north, at `a`) it is from `a` to `b`.
pub fn step(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (p1, p2, dl) = (a.0 * RAD, b.0 * RAD, (b.1 - a.1) * RAD);
    let hav = sin((p2 - p1) / 2.0).powi(2) + cos(p1) * cos(p2) * sin(dl / 2.0).powi(2);
    let km = 2.0 * EARTH_KM * asin(sqrt(hav).min(1.0));
    let bearing = atan2(sin(dl) * cos(p2), cos(p1) * sin(p2) - sin(p1) * cos(p2) * cos(dl));
    (km, bearing)
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
    fn the_sun_stands_where_it_should_over_da_nang_and_the_equator() {
        let da_nang = (16.07, 108.22);
        let deg = |(e, a): (f64, f64)| (e / RAD, a / RAD);
        let noon = |date| {
            let s = sun_times(date, 420, da_nang);
            (s.rise + s.set) / 2.0
        };
        // Solar noon: 90 - |lat - declination| high, due south in winter, north at midsummer.
        let (e, a) = deg(sun_position(DEC21, noon(DEC21), 420, da_nang));
        assert!((e - 50.5).abs() < 0.5 && (a - 180.0).abs() < 2.0, "dec 21 noon: {e} {a}");
        let (e, a) = deg(sun_position(JUNE21, noon(JUNE21), 420, da_nang));
        assert!((e - 82.6).abs() < 0.5 && (a.min(360.0 - a)) < 2.0, "june 21 noon: {e} {a}");
        // Rising in the east-north-east at midsummer, the east-south-east at midwinter, and on the
        // horizon at sunrise (sun_times counts the refraction, so a hair below).
        for (date, east_of) in [(JUNE21, 50.0..80.0), (DEC21, 105.0..130.0)] {
            let rise = sun_times(date, 420, da_nang).rise;
            let (e, a) = deg(sun_position(date, rise, 420, da_nang));
            assert!((-1.2..0.2).contains(&e) && east_of.contains(&a), "{date:?} sunrise: {e} {a}");
            let (_, a) = deg(sun_position(date, noon(date) + 3.0, 420, da_nang));
            assert!((190.0..350.0).contains(&a), "{date:?} afternoon: west, got {a}");
        }
        // The equinox at the equator on Greenwich: straight up at noon, below at midnight.
        let march20 = Date { year: 2026, month: 3, day: 20 };
        assert!(deg(sun_position(march20, 12.1, 0, (0.0, 0.0))).0 > 88.5);
        assert!(deg(sun_position(march20, 0.1, 0, (0.0, 0.0))).0 < -85.0);
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
        assert_eq!(parse_place("\u{feff}place = 10.8, 106.7\r\n"), Ok(Some((10.8, 106.7))), "Notepad's UTF-8 BOM and CRLF");
        for bad in ["place = 21.03", "place = hanoi", "place = 91, 0", "place = 0, 181", "place = NaN, 0", "place = 21.03, 105.85, 3", "place ="] {
            assert!(parse_place(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn the_great_circle_runs_from_a_to_b_the_short_way() {
        let (saigon, london) = ((10.8, 106.7), (51.5, -0.13));
        let near = |p: (f64, f64), q: (f64, f64)| (p.0 - q.0).abs() < 1e-6 && (p.1 - q.1).abs() < 1e-6;
        assert!(near(great_circle(saigon, london, 0.0), saigon));
        assert!(near(great_circle(saigon, london, 1.0), london));
        // On the equator, halfway from 0° to 90°E is 45°E.
        let m = great_circle((0.0, 0.0), (0.0, 90.0), 0.5);
        assert!(m.0.abs() < 1e-9 && (m.1 - 45.0).abs() < 1e-9, "{m:?}");
        // Across the date line the short way goes through 180°, not 0°.
        let m = great_circle((0.0, 170.0), (0.0, -170.0), 0.5);
        assert!((m.1.abs() - 180.0).abs() < 1e-9, "{m:?}");
        // Half the parameter is half the distance.
        let (total, _) = step(saigon, london);
        let (half, _) = step(saigon, great_circle(saigon, london, 0.5));
        assert!((half - total / 2.0).abs() < 0.5, "{half} of {total}");
    }

    #[test]
    fn a_trip_without_a_unique_path_snaps_to_the_end_and_stays_finite() {
        let saigon = (10.8, 106.7);
        assert_eq!(great_circle(saigon, saigon, 0.3), saigon, "same place");
        assert_eq!(great_circle((0.0, 0.0), (0.0, 180.0), 0.3), (0.0, 180.0), "antipodes");
        // Over the pole: 80°N 0° to 80°N 180° passes through 90°N.
        let m = great_circle((80.0, 0.0), (80.0, 180.0), 0.5);
        assert!(m.0.is_finite() && m.1.is_finite() && (m.0 - 90.0).abs() < 1e-6, "{m:?}");
    }

    #[test]
    fn step_gives_km_and_a_bearing_clockwise_from_north() {
        let (km, b) = step((0.0, 0.0), (0.0, 90.0));
        assert!((km - 10007.5).abs() < 5.0 && (b - PI / 2.0).abs() < 1e-9, "due east: {km} {b}");
        let (km, b) = step((0.0, 0.0), (10.0, 0.0));
        assert!((km - 1111.9).abs() < 1.0 && b.abs() < 1e-9, "due north: {km} {b}");
        let (_, b) = step((0.0, 0.0), (-10.0, 0.0));
        assert!((b.abs() - PI).abs() < 1e-9, "due south: {b}");
        assert_eq!(step((5.0, 5.0), (5.0, 5.0)), (0.0, 0.0), "same place: no distance, no NaN");
    }
}
