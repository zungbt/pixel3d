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
use meadow_core::settings;
use meadow_core::world::{lerp, smoothstep};

// A trip goes straight there at the chosen speed (km/s), speeding up over RAMP and slowing down
// over RAMP at the end, the clouds sliding past at CLOUD_PER_KM: Da Nang to Buon Ma Thuot takes
// some 6 minutes at 1 km/s.
const RAMP: f64 = 2.0; // s
const CLOUD_PER_KM: f64 = 3.0; // world units (about 2.5 m)
const CLOCK_SECS: f64 = 3.0; // the post clock's hands take at least this long to slide to a new zone
const ETA_FADE: f64 = 1.0; // s the ETA takes to show at the start and to go at the end

/// A trip from where the sky was last drawn to the chosen place, flown at `speed` from `start`
/// on, `done` km of it behind it then. A trip whose speed changed goes on at once at the new one.
#[derive(Clone, Copy)]
struct Travel {
    from: (f64, f64),
    km: f64,
    start: f64, // animation time
    done: f64,
    speed: f64,
    ramp_in: f64, // s to get up to speed: RAMP from standing, none when the speed changed
    ramp_out: f64, // s to slow down at the end
}

impl Travel {
    fn new(from: (f64, f64), km: f64, start: f64, speed: f64) -> Self {
        let ramp = RAMP.min(km / speed); // a hop under RAMP's worth never gets up to speed
        Travel { from, km, start, done: 0.0, speed, ramp_in: ramp, ramp_out: ramp }
    }

    /// The rest of the trip from animation time `t` at `speed`, without stopping.
    fn at_speed(&self, t: f64, speed: f64) -> Self {
        let done = self.gone(t);
        let ramp_out = RAMP.min(2.0 * (self.km - done) / speed);
        Travel { done, start: t, speed, ramp_in: 0.0, ramp_out, ..*self }
    }

    /// Seconds from `start` to the end.
    fn secs(&self) -> f64 {
        (self.km - self.done) / self.speed + (self.ramp_in + self.ramp_out) / 2.0
    }

    /// km of the whole trip behind it at animation time `t`.
    fn gone(&self, t: f64) -> f64 {
        let (secs, rest, v) = (self.secs(), self.km - self.done, self.speed);
        let t = (t - self.start).clamp(0.0, secs);
        let leg = if t >= secs {
            rest
        } else if t < self.ramp_in {
            v * t * t / (2.0 * self.ramp_in)
        } else if t <= secs - self.ramp_out {
            v * (t - self.ramp_in / 2.0)
        } else {
            rest - v * (secs - t).powi(2) / (2.0 * self.ramp_out)
        };
        self.done + leg
    }

    fn arrived(&self, t: f64) -> bool {
        t - self.start >= self.secs()
    }
}

/// The post clock's offset from UTC sliding from where it stood to the zone it now shows.
#[derive(Clone, Copy)]
struct Glide {
    from_min: f64,
    start: f64, // animation time
    secs: f64,
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
    speed: f64,             // km/s the next trip goes at
    shown: (f64, f64),      // where the sky was last drawn: a new trip starts here
    city_tz: Option<TimeZone>, // the chosen city's own zone, if known
    pub city_time: bool,       // the post clock shows that zone's time; false: the PC's
    glide: Option<Glide>,
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
        let city_tz = settings.and_then(settings::city_timezone).and_then(|name| TimeZone::get(name).ok());
        Self { mode, tz, place, travel: None, speed: 1.0, shown: place, city_tz, city_time: false, glide: None, cycle_start, date, fog }
    }

    /// Where the sky was last drawn: a trip to a new city starts there.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn shown(&self) -> (f64, f64) {
        self.shown
    }

    /// A city was chosen: the sky travels to it at the chosen speed, from where it stands now, from
    /// animation time `now`; the sun rises and sets there afterwards. `timezone` is its IANA zone
    /// name ("" if unknown), for the post clock, whose hands slide there over the trip.
    #[cfg_attr(not(windows), allow(dead_code))] // chosen from the tray
    pub fn set_place(&mut self, place: (f64, f64), timezone: &str, now: f64) {
        let (km, _) = sun::step(self.shown, place);
        let trip = Travel::new(self.shown, km, now, self.speed);
        self.glide_clock(now, trip.secs().max(CLOCK_SECS), |sky| sky.city_tz = TimeZone::get(timezone).ok());
        self.travel = (km > 0.0).then_some(trip);
        self.place = place;
        let day = today(&self.tz, self.date);
        let today = sun::sun_times(day.date, day.offset_min, place);
        log::info!("sky: place {:.2}, {:.2}, rise {}, set {}", place.0, place.1, hhmm(today.rise), hhmm(today.set));
    }

    /// Where the sky is at animation time `t` (s): on the great circle from the old city to the
    /// chosen one, or at the chosen one.
    pub fn place_at(&self, t: f64) -> (f64, f64) {
        match (self.travel, self.progress(t)) {
            (Some(Travel { from, .. }), Some(f)) => sun::great_circle(from, self.place, f),
            _ => self.place,
        }
    }

    /// How much of the trip under way is behind the sky at animation time `t`, 0-1; None if arrived.
    pub fn progress(&self, t: f64) -> Option<f64> {
        self.travel.map(|trip| trip.gone(t) / trip.km)
    }

    /// The speed (km/s) trips go at from animation time `now`, the one under way included; the
    /// post clock's hands then take the rest of it to get there.
    pub fn set_speed(&mut self, speed: f64, now: f64) {
        self.speed = speed;
        if let Some(trip) = self.travel.filter(|trip| !trip.arrived(now)) {
            let trip = trip.at_speed(now, speed);
            self.travel = Some(trip);
            if self.glide.is_some() {
                self.glide_clock(now, trip.secs().max(CLOCK_SECS), |_| {});
            }
        }
    }

    /// The trip under way at animation time `t`: (km, s) still to go, and how much the ETA shows, 0-1.
    pub fn eta(&self, t: f64) -> Option<(f64, f64, f64)> {
        let trip = self.travel.filter(|trip| !trip.arrived(t))?;
        let left = trip.secs() - (t - trip.start);
        let shown = (t - (trip.start - trip.done / trip.speed)).min(left) / ETA_FADE;
        Some((trip.km - trip.gone(t), left, shown.clamp(0.0, 1.0)))
    }

    /// Notes that the sky is drawn at `t` and returns how far the clouds slide over the ground
    /// (world x, z) since the last frame: against the way the sky travelled.
    pub fn advance(&mut self, t: f64) -> [f64; 2] {
        let here = self.place_at(t);
        let (km, bearing) = sun::step(self.shown, here);
        self.shown = here;
        let per_km = if self.travel.is_some() { CLOUD_PER_KM } else { 0.0 };
        if self.travel.is_some_and(|trip| trip.arrived(t)) {
            self.travel = None;
        }
        // Against the way travelled, on the compass the sun is placed by (east right, north up).
        let (east, north) = (-bearing.sin() * km * per_km, -bearing.cos() * km * per_km);
        [east * sky::EAST[0] + north * sky::NORTH[0], east * sky::EAST[1] + north * sky::NORTH[1]]
    }

    // Runs `change` and has the post clock's offset slide from where it stood to where it ends up.
    fn glide_clock(&mut self, now: f64, secs: f64, change: impl FnOnce(&mut Self)) {
        let from_min = self.hands_offset(Timestamp::now(), now);
        change(self);
        self.glide = Some(Glide { from_min, start: now, secs });
    }

    /// The post clock shows the chosen city's time (or the PC's): its hands slide there.
    pub fn set_city_time(&mut self, on: bool, now: f64) {
        self.glide_clock(now, CLOCK_SECS, |sky| sky.city_time = on);
    }

    // The offset (minutes east of UTC) the post clock's hands show for `instant` at animation time `t`.
    fn hands_offset(&self, instant: Timestamp, t: f64) -> f64 {
        let zone = match (&self.city_tz, self.city_time) {
            (Some(city), true) => city,
            _ => &self.tz,
        };
        let live = zone.to_offset(instant).seconds() as f64 / 60.0;
        match self.glide {
            Some(Glide { from_min, start, secs }) => lerp(from_min, live, smoothstep(((t - start) / secs).clamp(0.0, 1.0), 0.0, 1.0)),
            None => live,
        }
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

    /// The post clock's (hour, minute) hands: the PC's local time, or the chosen city's, whatever
    /// the sky shows, less `behind` s (stopped, or still catching up); at animation time `t`.
    pub fn hands(&self, behind: f64, t: f64) -> (f64, f64) {
        let instant = Timestamp::now() - jiff::SignedDuration::from_secs_f64(behind);
        let (h, m, s) = at(&TimeZone::UTC, instant).hms;
        let day = (h as f64 * 3600.0 + m as f64 * 60.0 + s + self.hands_offset(instant, t) * 60.0).rem_euclid(86_400.0);
        clock::hand_angles((day / 3600.0) as u32, (day % 3600.0 / 60.0) as u32, day % 60.0)
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

    // A fresh trip of `km` at 1 km/s, the default: how long it takes, and how much is behind it `t` s in.
    fn trip_secs(km: f64) -> f64 {
        Travel::new((0.0, 0.0), km, 0.0, 1.0).secs()
    }

    fn trip_share(km: f64, t: f64) -> f64 {
        Travel::new((0.0, 0.0), km, 0.0, 1.0).gone(t) / km
    }

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
        sky.set_place((0.0, 90.0), "", 100.0);
        let secs = trip_secs(sun::step((0.0, 0.0), (0.0, 90.0)).0);
        let start = sky.place_at(100.0);
        assert!(start.0.abs() < 1e-6 && start.1.abs() < 1e-6, "starts where it was: {start:?}");
        let mid = sky.place_at(100.0 + secs / 2.0);
        assert!(mid.0.abs() < 1e-6 && (mid.1 - 45.0).abs() < 1e-6, "halfway at half the time: {mid:?}");
        let end = sky.place_at(100.0 + secs);
        assert!(end.0.abs() < 1e-6 && (end.1 - 90.0).abs() < 1e-6, "arrived: {end:?}");
    }

    #[test]
    fn a_trip_goes_at_1_km_a_second_after_speeding_up_gently() {
        let km = sun::step((16.0544, 108.2022), (12.6667, 108.05)).0; // Da Nang to Buon Ma Thuot
        assert!((trip_secs(km) - (km + RAMP)).abs() < 1e-9 && (370.0..390.0).contains(&km), "{km} km");
        let share = |t: f64| trip_share(km, t);
        assert_eq!((share(-1.0), share(0.0), share(trip_secs(km) + 1.0)), (0.0, 0.0, 1.0));
        assert!(((share(100.0) - share(99.0)) * km - 1.0).abs() < 1e-9, "cruising at 1 km/s");
        assert!((share(0.2) - share(0.1)) * km < 0.2, "slow off the mark");
        let steps: Vec<f64> = (0..=400).map(|k| share(k as f64)).collect();
        assert!(steps.windows(2).all(|w| w[1] >= w[0]), "never back");
        // A hop of 1 km never gets up to speed: 2 s, halfway at 1 s.
        assert_eq!(trip_secs(1.0), 2.0);
        assert!((trip_share(1.0, 1.0) - 0.5).abs() < 1e-12);
        assert_eq!(trip_secs(0.0), 0.0);
    }

    #[test]
    fn a_new_speed_mid_trip_goes_on_from_where_it_is() {
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_place((0.0, 90.0), "", 0.0);
        let km = sun::step((0.0, 0.0), (0.0, 90.0)).0;
        let before = sky.progress(100.0).unwrap();
        sky.set_speed(5.0, 100.0);
        assert!((sky.progress(100.0).unwrap() - before).abs() < 1e-12, "no jump");
        assert!(((sky.progress(101.0).unwrap() - before) * km - 5.0).abs() < 1e-9, "on at 5 km/s at once");
        let (left, secs, _) = sky.eta(100.0).unwrap();
        assert!((secs - (left / 5.0 + RAMP / 2.0)).abs() < 1e-9, "slowing down at the end: {left} km in {secs} s");
        assert!(sky.progress(100.0 + secs - 0.01).unwrap() < 1.0 && sky.eta(100.0 + secs + 1e-9).is_none(), "arrives then");
        // The next trip starts at the new speed.
        sky.advance(100.0 + secs + 1e-9);
        sky.set_place((0.0, 0.0), "", 1e5);
        assert!((sky.eta(1e5).unwrap().1 - (km / 5.0 + RAMP)).abs() < 1e-9);
    }

    #[test]
    fn the_eta_counts_down_and_fades_in_and_out() {
        let mut sky = sky_at("place = 0, 0\n");
        assert!(sky.eta(0.0).is_none(), "not travelling");
        sky.set_place((0.0, 1.0), "", 10.0); // 111 km
        let km = sun::step((0.0, 0.0), (0.0, 1.0)).0;
        let secs = trip_secs(km);
        assert_eq!(sky.eta(10.0), Some((km, secs, 0.0)), "the whole way, not yet shown");
        let (left, to_go, shown) = sky.eta(60.0).unwrap();
        assert!((left - (km - 49.0)).abs() < 1e-9 && (to_go - (secs - 50.0)).abs() < 1e-9 && shown == 1.0);
        assert!((sky.eta(10.0 + 0.5).unwrap().2 - 0.5).abs() < 1e-9 && (sky.eta(10.0 + secs - 0.25).unwrap().2 - 0.25).abs() < 1e-9);
        assert!(sky.eta(10.0 + secs).is_none(), "arrived");
    }

    #[test]
    fn a_second_city_chosen_mid_trip_starts_from_where_the_sky_was_drawn() {
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_place((0.0, 90.0), "", 0.0);
        let half = trip_secs(sun::step((0.0, 0.0), (0.0, 90.0)).0) / 2.0;
        sky.advance(half); // drawn at 45°E
        sky.set_place((0.0, -90.0), "", half);
        let p = sky.place_at(half);
        assert!((p.1 - 45.0).abs() < 1e-6, "no jump: {p:?}");
        let end = sky.place_at(half + trip_secs(sun::step((0.0, 45.0), (0.0, -90.0)).0));
        assert!((end.1 + 90.0).abs() < 1e-6, "and ends at the second city: {end:?}");
    }

    #[test]
    fn clouds_slide_against_the_way_the_sky_travels_by_the_whole_distance() {
        // The sun's own compass: north is the top of the screen, east its right (sky.rs).
        let slid = |to: (f64, f64)| {
            let mut sky = sky_at("place = 0, 0\n");
            sky.set_place(to, "", 0.0);
            let mut total = [0.0, 0.0];
            let secs = trip_secs(sun::step((0.0, 0.0), to).0);
            for k in 1..=60 {
                let s = sky.advance(k as f64 * secs / 60.0);
                total[0] += s[0];
                total[1] += s[1];
            }
            assert_eq!(sky.advance(secs + 1.0), [0.0, 0.0], "arrived: nothing more");
            (total, sun::step((0.0, 0.0), to).0 * CLOUD_PER_KM)
        };
        // Due east: the clouds go to the screen's left, i.e. against EAST.
        let (total, by) = slid((0.0, 90.0));
        let want = [-sky::EAST[0] * by, -sky::EAST[1] * by];
        assert!((total[0] - want[0]).abs() < 1e-6 * by && (total[1] - want[1]).abs() < 1e-6 * by, "east: {total:?} not {want:?}");
        // Due north: down the screen, against NORTH.
        let (total, by) = slid((10.0, 0.0));
        let want = [-sky::NORTH[0] * by, -sky::NORTH[1] * by];
        assert!((total[0] - want[0]).abs() < 1e-6 * by && (total[1] - want[1]).abs() < 1e-6 * by, "north: {total:?} not {want:?}");
    }

    #[test]
    fn a_frame_at_an_unchanged_time_slides_nothing() {
        // An occluded app doesn't advance its animation time: the trip waits, then resumes.
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_place((0.0, 90.0), "", 0.0);
        sky.advance(2.0);
        assert_eq!(sky.advance(2.0), [0.0, 0.0]);
    }

    // The offset (minutes east of UTC) the post clock's hands show at 2026-10-15 09:30 UTC.
    fn offset_at(sky: &SkyClock, t: f64) -> f64 {
        sky.hands_offset("2026-10-15T09:30:00Z".parse().unwrap(), t)
    }

    #[test]
    fn the_hands_follow_the_pc_zone_or_slide_to_the_chosen_citys() {
        let mut sky = sky_at("place = 0, 0\n");
        let pc = offset_at(&sky, 0.0);
        sky.set_place((35.7, 139.7), "Asia/Tokyo", 10.0);
        assert_eq!(offset_at(&sky, 10.0), pc, "the PC's time until asked for the city's");
        sky.set_city_time(true, 20.0);
        assert!((offset_at(&sky, 20.0) - pc).abs() < 1e-9, "starts where the hands were");
        assert!((offset_at(&sky, 20.0 + CLOCK_SECS / 2.0) - (pc + 540.0) / 2.0).abs() < 1e-9, "halfway at half the time");
        assert_eq!(offset_at(&sky, 20.0 + CLOCK_SECS), 540.0, "Tokyo is UTC+9");
        sky.set_city_time(false, 100.0);
        assert!((offset_at(&sky, 100.0) - 540.0).abs() < 1e-9, "and back from where they stood");
        assert_eq!(offset_at(&sky, 100.0 + CLOCK_SECS), pc);
    }

    #[test]
    fn on_the_citys_time_the_hands_slide_over_the_whole_trip() {
        let mut sky = sky_at("place = 0, 0\n");
        sky.set_city_time(true, 0.0);
        let pc = offset_at(&sky, 10.0);
        sky.set_place((35.7, 139.7), "Asia/Tokyo", 10.0);
        let secs = trip_secs(sun::step((0.0, 0.0), (35.7, 139.7)).0);
        assert!((offset_at(&sky, 10.0 + secs / 2.0) - (pc + 540.0) / 2.0).abs() < 1e-9, "halfway there, half the way round");
        assert_eq!(offset_at(&sky, 10.0 + secs), 540.0);
    }

    #[test]
    fn an_unknown_city_zone_leaves_the_clock_on_the_pc() {
        let mut sky = sky_at("place = 0, 0\n");
        let pc = offset_at(&sky, 0.0);
        sky.set_city_time(true, 0.0);
        sky.set_place((35.7, 139.7), "", 1.0);
        assert_eq!(offset_at(&sky, 1000.0), pc);
        sky.set_place((35.7, 139.7), "Not/AZone", 2.0);
        assert_eq!(offset_at(&sky, 1000.0), pc);
    }

    #[test]
    fn choosing_the_same_city_again_changes_nothing() {
        let mut sky = sky_at("place = 10.8, 106.7\n");
        sky.set_place((10.8, 106.7), "", 5.0);
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
