//! The time of day on screen: the PC's local clock (or, for testing, a pinned hour or the web's
//! 300 s day), at the place guessed from the system time zone unless settings.txt names one,
//! with the sun where it stands there on that date.
//! Read every frame, so midnight, DST changes and long pauses need nothing special.
use jiff::Timestamp;
use jiff::tz::TimeZone;
use meadow_core::clock;
use meadow_core::sky::{self, Fog, SkyState};
use meadow_core::sun::{self, Date, SunTimes};
use meadow_core::weather::Weather;
use meadow_core::world::smoothstep;

const TRAVEL_SECS: f64 = 6.0; // a new city is reached in this long
const CLOUD_PER_KM: f64 = 0.02; // world units the clouds slide per km the sky travels

/// A trip from where the sky was last drawn to the chosen place.
#[derive(Clone, Copy)]
struct Travel {
    from: (f64, f64),
    start: f64, // animation time
}

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
    travel: Option<Travel>, // None once arrived
    shown: (f64, f64),      // where the sky was last drawn: a new trip starts here
    cycle_start: f64, // cycle mode: day_t at animation time 0
    date: Option<(u8, u8)>, // --date: (month, day) pinned
    pub fog: Fog,
}

struct Now {
    date: Date,
    hour: f64,
    offset_min: i32,
    hms: (u32, u32, f64),
}

fn now(tz: &TimeZone) -> Now {
    at(tz, Timestamp::now())
}

/// Today, or the --date pinned this run.
fn today(tz: &TimeZone, date: Option<(u8, u8)>) -> Now {
    let mut now = now(tz);
    if let Some((month, day)) = date {
        now.date = Date { month, day, ..now.date };
    }
    now
}

fn at(tz: &TimeZone, t: Timestamp) -> Now {
    let z = t.to_zoned(tz.clone());
    let seconds = z.second() as f64 + z.subsec_nanosecond() as f64 * 1e-9;
    Now {
        date: Date { year: z.year() as i32, month: z.month() as u8, day: z.day() as u8 },
        hour: z.hour() as f64 + z.minute() as f64 / 60.0 + seconds / 3600.0,
        offset_min: z.offset().seconds() / 60,
        hms: (z.hour() as u32, z.minute() as u32, seconds),
    }
}

fn day_t(mode: Mode, now_hour: f64, sun: SunTimes, cycle_start: f64, anim_time: f64) -> f64 {
    match mode {
        Mode::Clock => sky::clock_time(now_hour, sun),
        Mode::Hour(h) => sky::clock_time(h, sun),
        Mode::Cycle(_) => (cycle_start + anim_time).rem_euclid(sky::DAY_LENGTH),
    }
}

/// The cycle start that shows `day_t` at `anim_time`.
fn carry_on(day_t: f64, anim_time: f64) -> f64 {
    (day_t - anim_time).rem_euclid(sky::DAY_LENGTH)
}

fn utc_offset(min: i32) -> String {
    format!("{}{:02}:{:02}", if min < 0 { '-' } else { '+' }, min.abs() / 60, min.abs() % 60)
}

fn hhmm(hour: f64) -> String {
    let m = (hour * 60.0).round() as u32 % (24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// Without a named zone the hour still follows the PC clock (the offset as of startup).
fn fallback_zone(os_offset_min: Option<i32>) -> TimeZone {
    os_offset_min.and_then(|m| jiff::tz::Offset::from_seconds(m * 60).ok()).map_or(TimeZone::UTC, TimeZone::fixed)
}

#[cfg(windows)]
fn os_offset_min() -> Option<i32> {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_ID_INVALID, TIME_ZONE_INFORMATION};
    let mut info = TIME_ZONE_INFORMATION::default();
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    // UTC = local + bias; id 2 is TIME_ZONE_ID_DAYLIGHT.
    let bias = match id {
        TIME_ZONE_ID_INVALID => return None,
        2 => info.Bias + info.DaylightBias,
        _ => info.Bias + info.StandardBias,
    };
    Some(-bias)
}

#[cfg(not(windows))]
fn os_offset_min() -> Option<i32> {
    None
}

/// The text of settings.txt; no file is fine, but an unreadable one (e.g. UTF-16) is an error to log.
pub fn read_settings(path: &std::path::Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e} (save it as UTF-8)", path.display())),
    }
}

impl SkyClock {
    /// `settings`: the text of %APPDATA%\pixel3d\settings.txt, if there is one.
    pub fn new(mode: Mode, date: Option<(u8, u8)>, settings: Option<&str>, fog: Fog) -> Self {
        let tz = TimeZone::try_system().unwrap_or_else(|e| {
            log::warn!("sky: no system time zone ({e}), using the OS's current UTC offset");
            fallback_zone(os_offset_min())
        });
        let now = today(&tz, date);
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
        Self { mode, tz, place, travel: None, shown: place, cycle_start, date, fog }
    }

    /// A city was chosen: the sky travels to it, from where it stands now, over TRAVEL_SECS from
    /// animation time `now`; the sun rises and sets there afterwards.
    #[cfg_attr(not(windows), allow(dead_code))] // chosen from the tray
    pub fn set_place(&mut self, place: (f64, f64), now: f64) {
        self.travel = Some(Travel { from: self.shown, start: now });
        self.place = place;
        let day = today(&self.tz, self.date);
        let today = sun::sun_times(day.date, day.offset_min, place);
        log::info!("sky: place {:.2}, {:.2}, rise {}, set {}", place.0, place.1, hhmm(today.rise), hhmm(today.set));
    }

    /// Where the sky is at animation time `t` (s): on the great circle from the old city to the
    /// chosen one, or at the chosen one.
    pub fn place_at(&self, t: f64) -> (f64, f64) {
        match self.travel {
            Some(Travel { from, start }) => {
                let f = smoothstep(((t - start) / TRAVEL_SECS).clamp(0.0, 1.0), 0.0, 1.0);
                sun::great_circle(from, self.place, f)
            }
            None => self.place,
        }
    }

    /// Notes that the sky is drawn at `t` and returns how far the clouds slide over the ground
    /// (world x, z) since the last frame: against the way the sky travelled, `CLOUD_PER_KM` a km.
    pub fn advance(&mut self, t: f64) -> [f64; 2] {
        let here = self.place_at(t);
        let (km, bearing) = sun::step(self.shown, here);
        self.shown = here;
        if self.travel.is_some_and(|trip| t - trip.start >= TRAVEL_SECS) {
            self.travel = None;
        }
        let slide = km * CLOUD_PER_KM;
        [-bearing.sin() * slide, bearing.cos() * slide]
    }

    /// The local clock, or (false) the web's 300 s day carrying on from the time shown now.
    pub fn set_clock(&mut self, on: bool, anim_time: f64) {
        if on {
            self.mode = Mode::Clock;
            return;
        }
        if !matches!(self.mode, Mode::Cycle(_)) {
            let now = today(&self.tz, self.date);
            let shown = day_t(self.mode, now.hour, sun::sun_times(now.date, now.offset_min, self.place_at(anim_time)), self.cycle_start, anim_time);
            self.cycle_start = carry_on(shown, anim_time);
            self.mode = Mode::Cycle(None);
        }
    }

    /// The post clock's (hour, minute) hands: always the local time, whatever the sky shows, less
    /// `behind` s (stopped, or still catching up).
    pub fn hands(&self, behind: f64) -> (f64, f64) {
        let (h, m, s) = at(&self.tz, Timestamp::now() - jiff::SignedDuration::from_secs_f64(behind)).hms;
        clock::hand_angles(h, m, s)
    }

    /// The sky for right now, the sun where it stands at the hour shown; `anim_time` (s) only
    /// drives the cycle mode. Real `weather` sets the automatic fog.
    pub fn state(&self, anim_time: f64, weather: Option<&Weather>) -> SkyState {
        let now = today(&self.tz, self.date);
        let place = self.place_at(anim_time);
        let sun = sun::sun_times(now.date, now.offset_min, place);
        let winter = sun::winterness(now.date, place.0);
        let phase = sky::phase(day_t(self.mode, now.hour, sun, self.cycle_start, anim_time));
        let (elevation, azimuth) = sun::sun_position(now.date, sky::phase_to_hour(phase, sun), now.offset_min, place);
        let mut state = sky::sky_at(phase, elevation, azimuth, winter);
        state.fog = match (self.fog, weather) {
            (Fog::Auto, Some(w)) => w.fog(),
            (mode, _) => sky::fog_amount(mode, state.elevation, winter),
        };
        state
    }
}

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
    fn switching_to_the_5_minute_day_carries_on_from_the_shown_time() {
        let shown = sky::clock_time(21.0, SUN);
        for anim in [0.0, 42.0, 1000.0] {
            let start = carry_on(shown, anim);
            assert!((day_t(Mode::Cycle(None), 13.0, SUN, start, anim) - shown).abs() < 1e-9, "at animation time {anim}");
            assert!(day_t(Mode::Cycle(None), 13.0, SUN, start, anim + 1.0) > shown, "and moves on");
        }
    }

    fn sky_at(place: &str) -> SkyClock {
        SkyClock::new(Mode::Hour(12.0), None, Some(place), Fog::Auto)
    }

    #[test]
    fn a_new_city_is_reached_along_the_great_circle_in_the_trip_time() {
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_place((0.0, 90.0), 100.0);
        let start = sky.place_at(100.0);
        assert!(start.0.abs() < 1e-6 && start.1.abs() < 1e-6, "starts where it was: {start:?}");
        let mid = sky.place_at(100.0 + TRAVEL_SECS / 2.0);
        assert!(mid.0.abs() < 1e-6 && (mid.1 - 45.0).abs() < 1e-6, "halfway at half the time: {mid:?}");
        let end = sky.place_at(100.0 + TRAVEL_SECS);
        assert!(end.0.abs() < 1e-6 && (end.1 - 90.0).abs() < 1e-6, "arrived: {end:?}");
    }

    #[test]
    fn a_second_city_chosen_mid_trip_starts_from_where_the_sky_was_drawn() {
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_place((0.0, 90.0), 0.0);
        sky.advance(TRAVEL_SECS / 2.0); // drawn at 45°E
        sky.set_place((0.0, -90.0), TRAVEL_SECS / 2.0);
        let p = sky.place_at(TRAVEL_SECS / 2.0);
        assert!((p.1 - 45.0).abs() < 1e-6, "no jump: {p:?}");
        let end = sky.place_at(TRAVEL_SECS / 2.0 + TRAVEL_SECS);
        assert!((end.1 + 90.0).abs() < 1e-6, "and ends at the second city: {end:?}");
    }

    #[test]
    fn clouds_slide_against_the_way_the_sky_travels_by_the_whole_distance() {
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_place((0.0, 90.0), 0.0); // due east
        let mut total = [0.0, 0.0];
        for k in 1..=60 {
            let s = sky.advance(k as f64 * TRAVEL_SECS / 60.0);
            total[0] += s[0];
            total[1] += s[1];
        }
        let (km, _) = sun::step((0.0, 0.0), (0.0, 90.0));
        assert!((total[0] + km * CLOUD_PER_KM).abs() < 1e-3, "west by the distance: {total:?}");
        assert!(total[1].abs() < 1e-6, "none north-south: {total:?}");
        assert_eq!(sky.advance(TRAVEL_SECS + 1.0), [0.0, 0.0], "arrived: nothing more");
    }

    #[test]
    fn a_frame_at_an_unchanged_time_slides_nothing() {
        // An occluded app doesn't advance its animation time: the trip waits, then resumes.
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_place((0.0, 90.0), 0.0);
        sky.advance(2.0);
        assert_eq!(sky.advance(2.0), [0.0, 0.0]);
    }

    #[test]
    fn choosing_the_same_city_again_changes_nothing() {
        let mut sky = sky_at("place = 10.8, 106.7\n");
        sky.set_place((10.8, 106.7), 5.0);
        assert_eq!(sky.advance(6.0), [0.0, 0.0]);
        assert_eq!(sky.place_at(6.0), (10.8, 106.7));
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

    #[test]
    fn an_unknown_zone_keeps_the_os_offset() {
        let tz = fallback_zone(Some(420));
        assert_eq!(tz.iana_name(), None, "logged as unknown, place from the offset");
        assert_eq!(tz.to_offset(Timestamp::now()).seconds(), 420 * 60);
        assert_eq!(fallback_zone(None).iana_name(), Some("UTC"));
    }

    #[test]
    fn only_a_missing_settings_file_is_silent() {
        let dir = std::env::temp_dir().join(format!("meadow-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.txt");
        assert_eq!(read_settings(&path), Ok(None));
        std::fs::write(&path, "place = 10.8, 106.7\n").unwrap();
        assert_eq!(read_settings(&path), Ok(Some("place = 10.8, 106.7\n".to_string())));
        // Windows PowerShell 5.1's `>` writes UTF-16LE: that must be a warning, not a silent zone guess.
        std::fs::write(&path, b"\xff\xfep\0l\0a\0c\0e\0").unwrap();
        assert!(read_settings(&path).unwrap_err().contains("UTF-8"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
