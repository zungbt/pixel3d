//! Real weather from Open-Meteo (free, no key): the city search's places and a place's current
//! cloud cover and wind. Only the URLs and the parsing live here; the HTTP is the exe's.
use std::f64::consts::FRAC_PI_2;

pub const SEARCH_HOST: &str = "geocoding-api.open-meteo.com";
pub const FORECAST_HOST: &str = "api.open-meteo.com";

#[derive(Clone, Debug, PartialEq)]
pub struct City {
    pub name: String,
    pub detail: String, // the region and country, to tell same-named places apart
    pub lat: f64,
    pub lon: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weather {
    pub cover: f64,     // share of the sky in cloud, 0-1
    pub wind_from: f64, // degrees clockwise from north, where the wind comes from
    pub wind_kmh: f64,
}

impl Weather {
    /// The direction the wind blows toward in the scene's x, z: x east, z south (the sun rises
    /// at +x and stands at +z at noon), as an angle from +x.
    pub fn heading(&self) -> f64 {
        self.wind_from.to_radians() + FRAC_PI_2
    }
}

/// Path and query for a city search.
pub fn search_path(name: &str) -> String {
    format!("/v1/search?name={}&count=10&language=vi&format=json", encode(name.trim()))
}

/// Path and query for the current weather at `place` (lat, lon).
pub fn current_path((lat, lon): (f64, f64)) -> String {
    format!("/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=cloud_cover,wind_speed_10m,wind_direction_10m")
}

// Percent-encoding of everything but unreserved bytes, so a UTF-8 name like "Hà Nội" passes.
fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

pub fn parse_search(body: &str) -> Result<Vec<City>, String> {
    let root = Json::parse(body)?;
    // No match: no "results" at all.
    let Some(Json::Arr(results)) = root.get("results") else { return Ok(vec![]) };
    Ok(results
        .iter()
        .filter_map(|r| {
            let text = |k| match r.get(k) {
                Some(Json::Str(s)) => Some(s.as_str()),
                _ => None,
            };
            let name = text("name")?.to_string();
            let detail = [text("admin1"), text("country")].into_iter().flatten().filter(|s| *s != name).collect::<Vec<_>>().join(", ");
            Some(City { name, detail, lat: r.get("latitude")?.num()?, lon: r.get("longitude")?.num()? })
        })
        .collect())
}

pub fn parse_current(body: &str) -> Result<Weather, String> {
    let root = Json::parse(body)?;
    let current = root.get("current").ok_or("no \"current\" in the reply")?;
    let field = |k: &str| current.get(k).and_then(Json::num).ok_or(format!("no current {k}"));
    Ok(Weather {
        cover: (field("cloud_cover")? / 100.0).clamp(0.0, 1.0),
        wind_from: field("wind_direction_10m")?,
        wind_kmh: field("wind_speed_10m")?.max(0.0),
    })
}

// Just enough JSON for the two replies.
#[derive(Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { s: text.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(p.err());
        }
        Ok(v)
    }

    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self) -> String {
        format!("bad JSON at byte {}", self.i)
    }

    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        self.ws();
        let hit = self.s.get(self.i) == Some(&b);
        self.i += hit as usize;
        hit
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let rest = &self.s[self.i..];
        for (word, v) in [("null", Json::Null), ("true", Json::Bool(true)), ("false", Json::Bool(false))] {
            if rest.starts_with(word.as_bytes()) {
                self.i += word.len();
                return Ok(v);
            }
        }
        match rest.first() {
            Some(b'"') => self.string().map(Json::Str),
            Some(b'[') => {
                self.i += 1;
                let mut items = vec![];
                if !self.eat(b']') {
                    loop {
                        items.push(self.value()?);
                        if self.eat(b']') {
                            break;
                        }
                        if !self.eat(b',') {
                            return Err(self.err());
                        }
                    }
                }
                Ok(Json::Arr(items))
            }
            Some(b'{') => {
                self.i += 1;
                let mut fields = vec![];
                if !self.eat(b'}') {
                    loop {
                        self.ws();
                        let key = self.string()?;
                        if !self.eat(b':') {
                            return Err(self.err());
                        }
                        fields.push((key, self.value()?));
                        if self.eat(b'}') {
                            break;
                        }
                        if !self.eat(b',') {
                            return Err(self.err());
                        }
                    }
                }
                Ok(Json::Obj(fields))
            }
            _ => {
                let len = rest.iter().take_while(|b| b"+-.eE0123456789".contains(b)).count();
                let n = std::str::from_utf8(&rest[..len]).ok().and_then(|t| t.parse().ok()).ok_or_else(|| self.err())?;
                self.i += len;
                Ok(Json::Num(n))
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.s.get(self.i) != Some(&b'"') {
            return Err(self.err());
        }
        self.i += 1;
        let mut out: Vec<u16> = vec![]; // UTF-16, so \u surrogate pairs join up
        loop {
            let &b = self.s.get(self.i).ok_or_else(|| self.err())?;
            self.i += 1;
            match b {
                b'"' => return Ok(String::from_utf16_lossy(&out)),
                b'\\' => {
                    let &e = self.s.get(self.i).ok_or_else(|| self.err())?;
                    self.i += 1;
                    let c = match e {
                        b'u' => {
                            let hex = self.s.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u16::from_str_radix(h, 16).ok());
                            self.i += 4;
                            hex.ok_or_else(|| self.err())?
                        }
                        b'n' => b'\n' as u16,
                        b't' => b'\t' as u16,
                        b'r' => b'\r' as u16,
                        b'b' => 8,
                        b'f' => 12,
                        _ => e as u16, // \" \\ \/
                    };
                    out.push(c);
                }
                _ => {
                    // A whole UTF-8 character from here.
                    let start = self.i - 1;
                    let len = match b {
                        0..0x80 => 1,
                        0xe0..0xf0 => 3,
                        0xf0.. => 4,
                        _ => 2,
                    };
                    let ch = self.s.get(start..start + len).and_then(|c| std::str::from_utf8(c).ok()).ok_or_else(|| self.err())?;
                    out.extend(ch.encode_utf16());
                    self.i = start + len;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed real replies.
    const SEARCH: &str = r#"{"results":[{"id":1581130,"name":"Hà Nội","latitude":21.0245,"longitude":105.84117,"elevation":21.0,"feature_code":"PPLC","country_code":"VN","admin1_id":1581129,"timezone":"Asia/Bangkok","population":8053663,"postcodes":["100000"],"country_id":1562822,"country":"Việt Nam","admin1":"Hà Nội"},
        {"id":2,"name":"Hanoi","latitude":-1.5e1,"longitude":30,"country":"Testé \"x\" 🌧"}],"generationtime_ms":0.53}"#;
    const CURRENT: &str = r#"{"latitude":21.0,"longitude":105.875,"generationtime_ms":0.03,"utc_offset_seconds":0,"timezone":"GMT","timezone_abbreviation":"GMT","elevation":16.0,"current_units":{"time":"iso8601","interval":"seconds","cloud_cover":"%","wind_speed_10m":"km/h","wind_direction_10m":"°"},"current":{"time":"2026-10-13T08:00","interval":900,"cloud_cover":75,"wind_speed_10m":8.6,"wind_direction_10m":135}}"#;

    #[test]
    fn reads_the_search_results() {
        let cities = parse_search(SEARCH).unwrap();
        assert_eq!(cities[0], City { name: "Hà Nội".into(), detail: "Việt Nam".into(), lat: 21.0245, lon: 105.84117 });
        assert_eq!(cities[1], City { name: "Hanoi".into(), detail: "Testé \"x\" 🌧".into(), lat: -15.0, lon: 30.0 });
        assert_eq!(parse_search(r#"{"generationtime_ms":0.2}"#), Ok(vec![]), "no match");
        assert!(parse_search("<html>").is_err());
        assert!(parse_search(r#"{"results":[}"#).is_err());
    }

    #[test]
    fn reads_the_current_weather() {
        assert_eq!(parse_current(CURRENT), Ok(Weather { cover: 0.75, wind_from: 135.0, wind_kmh: 8.6 }));
        assert!(parse_current(r#"{"current":{"cloud_cover":10}}"#).is_err());
        assert!(parse_current(r#"{"error":true,"reason":"Latitude must be in range"}"#).is_err());
    }

    #[test]
    fn urls_are_encoded() {
        assert_eq!(search_path(" Hà Nội "), "/v1/search?name=H%C3%A0%20N%E1%BB%99i&count=10&language=vi&format=json");
        assert_eq!(current_path((21.0245, 105.84117)), "/v1/forecast?latitude=21.0245&longitude=105.8412&current=cloud_cover,wind_speed_10m,wind_direction_10m");
    }

    #[test]
    fn the_wind_blows_away_from_where_it_comes_from() {
        let toward = |from: f64| {
            let h = Weather { cover: 0.0, wind_from: from, wind_kmh: 10.0 }.heading();
            [h.cos(), h.sin()].map(|v| (v * 1e9).round() / 1e9 + 0.0)
        };
        assert_eq!(toward(0.0), [0.0, 1.0], "a north wind blows south (+z)");
        assert_eq!(toward(90.0), [-1.0, 0.0], "an east wind blows west (-x)");
        assert_eq!(toward(270.0), [1.0, 0.0], "a west wind blows east (+x)");
    }
}
