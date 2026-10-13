//! Viewer settings, ported from the web's `src/settings.js`: one `key = value` line each in
//! %APPDATA%\pixel3d\settings.txt (beside `place`). Every stored string is checked, so a missing
//! or unexpected value falls back to the default.
use crate::frame_pacer::parse_fps;
use crate::sky::Fog;

pub const PIXEL_SIZES: [u32; 3] = [2, 3, 4]; // screen px per art pixel

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub pixel_size: u32,
    pub fps: Option<u32>, // None: no cap (vsync)
    pub clock: bool,      // the sky follows the local clock; false: the web's 300 s day
    pub fog: Fog,
    pub show_clock: bool, // the post clock beside the path
    pub broken_at: Option<i64>, // unix s the post clock was turned off (its pieces' age)
    pub all_screens: bool, // wallpaper: the meadow runs across every monitor; false: the primary only
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
        }
    }
}

/// `text` with the setting's line replaced, or added at the end; line endings kept.
pub fn save(text: &str, setting: Setting) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let (key, value) = setting.key_value();
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

    const DEFAULTS: Settings = Settings { pixel_size: 3, fps: Some(15), clock: true, fog: Fog::Auto, show_clock: true, broken_at: None, all_screens: true };

    #[test]
    fn an_empty_file_gives_the_defaults() {
        assert_eq!(Settings::load(""), DEFAULTS);
        assert_eq!(Settings::load("place = 10.8, 106.7\n# a note\n"), DEFAULTS);
    }

    #[test]
    fn unexpected_stored_values_fall_back() {
        let text = "pixelSize = 5\nfps = 45\nclock = yes\nshowClock = 0\nfog = thick\nbrokenAt = soon\nscreens = 2\n";
        assert_eq!(Settings::load(text), DEFAULTS);
    }

    #[test]
    fn saved_values_load_back() {
        let changes = [Setting::PixelSize(4), Setting::Fps(Some(30)), Setting::Clock(false), Setting::Fog(Fog::Off), Setting::ShowClock(false), Setting::BrokenAt(1_800_000_000), Setting::AllScreens(false)];
        let text = changes.iter().fold(String::new(), |text, &s| save(&text, s));
        assert_eq!(Settings::load(&text), Settings { pixel_size: 4, fps: Some(30), clock: false, fog: Fog::Off, show_clock: false, broken_at: Some(1_800_000_000), all_screens: false });
        let mut set = DEFAULTS;
        changes.iter().for_each(|&s| set.set(s));
        assert_eq!(set, Settings::load(&text), "set agrees with save + load");
        assert_eq!(Settings::load(&save(&text, Setting::Fog(Fog::Always))).fog, Fog::Always);
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
}
