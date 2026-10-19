//! Viewer settings, ported from the web's `src/settings.js`: one `key = value` line each in
//! %APPDATA%\pixel3d\settings.txt (beside `place`). Every stored string is checked, so a missing
//! or unexpected value falls back to the default.
use crate::frame_pacer::parse_fps;
use crate::sky::Fog;
use crate::weather::City;

pub const PIXEL_SIZES: [u32; 3] = [2, 3, 4]; // screen px per art pixel

/// How fast the sky travels to a newly chosen city.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Speed {
    Plane,
    Supersonic,
    Rocket,
    Satellite,
}

impl Speed {
    pub const ALL: [Speed; 4] = [Speed::Plane, Speed::Supersonic, Speed::Rocket, Speed::Satellite];

    pub fn km_s(self) -> f64 {
        [0.25, 1.0, 5.0, 7.7][self as usize]
    }

    fn key(self) -> &'static str {
        ["plane", "supersonic", "rocket", "satellite"][self as usize]
    }
}

/// What distances and speeds are shown in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    M,
    Km,
    Mi,
}

impl Unit {
    pub const ALL: [Unit; 3] = [Unit::M, Unit::Km, Unit::Mi];

    /// `km` in this unit.
    pub fn of(self, km: f64) -> f64 {
        km * [1000.0, 1.0, 1.0 / 1.609_344][self as usize]
    }

    pub fn symbol(self) -> &'static str {
        ["m", "km", "mi"][self as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub pixel_size: u32,
    pub fps: Option<u32>, // None: no cap (vsync)
    pub clock: bool,      // the sky follows the local clock; false: the web's 300 s day
    pub fog: Fog,
    pub show_clock: bool, // the post clock beside the path
    pub broken_at: Option<i64>, // unix s the post clock was turned off (its pieces' age)
    pub all_screens: bool, // wallpaper: the meadow runs across every monitor; false: the primary only
    pub city_time: bool,  // the post clock shows the chosen city's local time; false: the PC's
    pub speed: Speed,
    pub unit: Unit,
}

/// One changed setting, as the tray menu sends it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Setting {
    PixelSize(u32),
    Fps(Option<u32>),
    Clock(bool),
    Fog(Fog),
    ShowClock(bool),
    BrokenAt(i64),
    AllScreens(bool),
    CityTime(bool),
    Speed(Speed),
    Unit(Unit),
}

// The value of `key` in settings.txt's text: its first line that isn't a comment.
fn stored<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.trim_start_matches('\u{feff}').lines().map(str::trim).filter(|l| !l.starts_with('#')).find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim())
    })
}

impl Settings {
    pub fn load(text: &str) -> Self {
        let get = |key| stored(text, key);
        Settings {
            pixel_size: get("pixelSize").and_then(|v| v.parse().ok()).filter(|n| PIXEL_SIZES.contains(n)).unwrap_or(3),
            fps: match get("fps") {
                Some("") => None, // no cap; unset means 15
                v => Some(v.and_then(parse_fps).unwrap_or(15)),
            },
            // Unlike the web (whose wallpaper mode forces it), the clock is the default.
            clock: get("clock") != Some("false"),
            fog: match get("fog") {
                Some("off") => Fog::Off,
                Some("always") => Fog::Always,
                _ => Fog::Auto,
            },
            show_clock: get("showClock") != Some("false"),
            broken_at: get("brokenAt").and_then(|v| v.parse().ok()),
            all_screens: get("screens") != Some("primary"),
            city_time: get("clockZone") == Some("city"),
            speed: Speed::ALL.into_iter().find(|s| get("travelSpeed") == Some(s.key())).unwrap_or(Speed::Supersonic),
            unit: Unit::ALL.into_iter().find(|u| get("unit") == Some(u.symbol())).unwrap_or(Unit::Km),
        }
    }

    pub fn set(&mut self, setting: Setting) {
        match setting {
            Setting::PixelSize(n) => self.pixel_size = n,
            Setting::Fps(fps) => self.fps = fps,
            Setting::Clock(on) => self.clock = on,
            Setting::Fog(fog) => self.fog = fog,
            Setting::ShowClock(on) => self.show_clock = on,
            Setting::BrokenAt(t) => self.broken_at = Some(t),
            Setting::AllScreens(on) => self.all_screens = on,
            Setting::CityTime(on) => self.city_time = on,
            Setting::Speed(speed) => self.speed = speed,
            Setting::Unit(unit) => self.unit = unit,
        }
    }
}

impl Setting {
    fn key_value(self) -> (&'static str, String) {
        match self {
            Setting::PixelSize(n) => ("pixelSize", n.to_string()),
            Setting::Fps(fps) => ("fps", fps.map_or(String::new(), |n| n.to_string())),
            Setting::Clock(on) => ("clock", on.to_string()),
            Setting::Fog(fog) => ("fog", ["off", "auto", "always"][fog as usize].to_string()),
            Setting::ShowClock(on) => ("showClock", on.to_string()),
            Setting::BrokenAt(t) => ("brokenAt", t.to_string()),
            Setting::AllScreens(on) => ("screens", if on { "all" } else { "primary" }.to_string()),
            Setting::CityTime(on) => ("clockZone", if on { "city" } else { "local" }.to_string()),
            Setting::Speed(speed) => ("travelSpeed", speed.key().to_string()),
            Setting::Unit(unit) => ("unit", unit.symbol().to_string()),
        }
    }
}

/// The chosen city's name (its place is `place`, read by sun.rs).
pub fn city(text: &str) -> Option<&str> {
    stored(text, "city").filter(|v| !v.is_empty())
}

/// The chosen city's IANA time zone name, if one was saved with it.
pub fn city_timezone(text: &str) -> Option<&str> {
    stored(text, "timezone").filter(|v| !v.is_empty())
}

/// `text` with the city's name, time zone and place saved.
pub fn save_city(text: &str, city: &City) -> String {
    let text = put(text, "city", &city.name);
    let text = put(&text, "timezone", &city.timezone);
    put(&text, "place", &format!("{:.4}, {:.4}", city.lat, city.lon))
}

/// `text` with the setting's line replaced, or added at the end; line endings kept.
pub fn save(text: &str, setting: Setting) -> String {
    let (key, value) = setting.key_value();
    put(text, key, &value)
}

fn put(text: &str, key: &str, value: &str) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let line = format!("{key} = {value}").trim_end().to_string(); // no cap: `fps =`
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let ours = |l: &String| {
        let l = l.trim_start_matches('\u{feff}').trim();
        !l.starts_with('#') && l.split_once('=').is_some_and(|(k, _)| k.trim() == key)
    };
    match lines.iter().position(ours) {
        Some(i) => {
            let bom = if lines[i].starts_with('\u{feff}') { "\u{feff}" } else { "" };
            lines[i] = format!("{bom}{line}");
        }
        None => lines.push(line),
    }
    lines.iter().map(|l| format!("{l}{eol}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: Settings = Settings { pixel_size: 3, fps: Some(15), clock: true, fog: Fog::Auto, show_clock: true, broken_at: None, all_screens: true, city_time: false, speed: Speed::Supersonic, unit: Unit::Km };

    #[test]
    fn an_empty_file_gives_the_defaults() {
        assert_eq!(Settings::load(""), DEFAULTS);
        assert_eq!(Settings::load("place = 10.8, 106.7\n# a note\n"), DEFAULTS);
    }

    #[test]
    fn unexpected_stored_values_fall_back() {
        let text = "pixelSize = 5\nfps = 45\nclock = yes\nshowClock = 0\nfog = thick\nbrokenAt = soon\nscreens = 2\nclockZone = utc\ntravelSpeed = warp\nunit = ft\n";
        assert_eq!(Settings::load(text), DEFAULTS);
    }

    #[test]
    fn saved_values_load_back() {
        let changes = [Setting::PixelSize(4), Setting::Fps(Some(30)), Setting::Clock(false), Setting::Fog(Fog::Off), Setting::ShowClock(false), Setting::BrokenAt(1_800_000_000), Setting::AllScreens(false), Setting::CityTime(true), Setting::Speed(Speed::Rocket), Setting::Unit(Unit::Mi)];
        let text = changes.iter().fold(String::new(), |text, &s| save(&text, s));
        assert_eq!(Settings::load(&text), Settings { pixel_size: 4, fps: Some(30), clock: false, fog: Fog::Off, show_clock: false, broken_at: Some(1_800_000_000), all_screens: false, city_time: true, speed: Speed::Rocket, unit: Unit::Mi });
        let mut set = DEFAULTS;
        changes.iter().for_each(|&s| set.set(s));
        assert_eq!(set, Settings::load(&text), "set agrees with save + load");
        assert_eq!(Settings::load(&save(&text, Setting::Fog(Fog::Always))).fog, Fog::Always);
        for speed in Speed::ALL {
            assert_eq!(Settings::load(&save(&text, Setting::Speed(speed))).speed, speed);
        }
        for unit in Unit::ALL {
            assert_eq!(Settings::load(&save(&text, Setting::Unit(unit))).unit, unit);
        }
    }

    #[test]
    fn units_convert_from_km() {
        assert_eq!(Unit::M.of(0.25), 250.0);
        assert_eq!(Unit::Km.of(380.0), 380.0);
        assert!((Unit::Mi.of(1.609_344) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn no_fps_cap_round_trips() {
        assert_eq!(Settings::load(&save("fps = 30\n", Setting::Fps(None))).fps, None);
    }

    #[test]
    fn reads_a_hand_written_fps_cap() {
        assert_eq!(Settings::load("fps=20").fps, Some(20));
    }

    #[test]
    fn saving_replaces_its_own_line_and_keeps_the_rest() {
        let text = "\u{feff}# Hanoi\r\nplace = 21.03, 105.85\r\n# fps = 60\r\nfps = 20\r\n";
        let saved = save(text, Setting::Fps(Some(30)));
        assert_eq!(saved, "\u{feff}# Hanoi\r\nplace = 21.03, 105.85\r\n# fps = 60\r\nfps = 30\r\n", "Notepad's BOM and CRLF, comments untouched");
        assert_eq!(save("place = 1, 2", Setting::ShowClock(false)), "place = 1, 2\nshowClock = false\n");
    }

    #[test]
    fn the_city_saves_with_its_place() {
        let hanoi = City { name: "Hà Nội".into(), detail: "Việt Nam".into(), lat: 21.0245, lon: 105.84117, timezone: "Asia/Bangkok".into() };
        let text = save_city("fps = 20\nplace = 1, 2\n", &hanoi);
        assert_eq!(text, "fps = 20\nplace = 21.0245, 105.8412\ncity = Hà Nội\ntimezone = Asia/Bangkok\n");
        assert_eq!(city(&text), Some("Hà Nội"));
        assert_eq!(city_timezone(&text), Some("Asia/Bangkok"));
        assert_eq!(city_timezone("timezone =\n"), None);
        assert_eq!(city_timezone("fps = 20\n"), None, "an older settings file");
        assert_eq!(crate::sun::parse_place(&text), Ok(Some((21.0245, 105.8412))));
        assert_eq!(city("city =\n"), None);
    }
}
