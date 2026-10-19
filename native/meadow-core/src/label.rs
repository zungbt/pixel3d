//! The trip's ETA in the screen's bottom-left corner: two lines of a 5x7 pixel font (capitals,
//! digits, a little punctuation; accents dropped), outlined so they read over the grass.
use crate::settings::Unit;

pub const SIZE: (u32, u32) = (256, 18); // art px: the mask the label is drawn into
pub const FILL: u8 = 255;
pub const OUTLINE: u8 = 128;
const ADVANCE: u32 = 6; // a glyph and its gap
const LINE: u32 = 9; // a line and its gap

// Each row's 5 pixels, the leftmost in bit 4.
const GLYPHS: [(char, [u8; 7]); 48] = [
    ('A', [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('B', [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
    ('C', [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110]),
    ('D', [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
    ('E', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
    ('F', [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('G', [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111]),
    ('H', [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
    ('I', [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('J', [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
    ('K', [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
    ('L', [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
    ('M', [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
    ('N', [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001]),
    ('O', [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('P', [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
    ('Q', [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
    ('R', [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
    ('S', [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110]),
    ('T', [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('U', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
    ('V', [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
    ('W', [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010]),
    ('X', [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
    ('Y', [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100]),
    ('Z', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
    ('0', [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
    ('1', [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
    ('2', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111]),
    ('3', [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110]),
    ('4', [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
    ('5', [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
    ('6', [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110]),
    ('7', [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
    ('8', [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
    ('9', [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100]),
    (' ', [0; 7]),
    (':', [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000]),
    ('.', [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100]),
    (',', [0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b00100, 0b01000]),
    ('-', [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000]),
    ('\'', [0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000]),
    ('(', [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010]),
    (')', [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000]),
    ('/', [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000]),
    ('·', [0b00000, 0b00000, 0b00000, 0b01100, 0b01100, 0b00000, 0b00000]),
    ('&', [0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101]),
    ('?', [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100]),
];

// Accented letters and the capital they're drawn as.
const ACCENTS: [(&str, char); 9] = [
    ("àáảãạăằắẳẵặâầấẩẫậäå", 'A'),
    ("èéẻẽẹêềếểễệë", 'E'),
    ("ìíỉĩịîï", 'I'),
    ("òóỏõọôồốổỗộơờớởỡợöø", 'O'),
    ("ùúủũụưừứửữựûü", 'U'),
    ("ỳýỷỹỵÿ", 'Y'),
    ("đ", 'D'),
    ("ç", 'C'),
    ("ñ", 'N'),
];

/// The capital `c` is drawn as, if the font has one: accents dropped, lower case raised.
fn fold(c: char) -> Option<char> {
    let lower = c.to_lowercase().next()?;
    let c = ACCENTS.iter().find(|(set, _)| set.contains(lower)).map_or(c.to_ascii_uppercase(), |&(_, base)| base);
    GLYPHS.iter().any(|&(g, _)| g == c).then_some(c)
}

/// `name` in the font's letters, as many as fit a line.
pub fn letters(name: &str) -> String {
    let all: String = name.chars().filter_map(fold).collect();
    all.trim().chars().take(((SIZE.0 - 2) / ADVANCE) as usize).collect()
}

/// The ETA's two lines: the city, then the time and the distance left.
pub fn eta(name: &str, km_left: f64, secs_left: f64, unit: Unit) -> [String; 2] {
    let s = secs_left.max(0.0).ceil() as u64;
    let time = if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) };
    let d = unit.of(km_left.max(0.0));
    let dist = if d >= 10.0 { format!("{d:.0}") } else { format!("{d:.1}") };
    [letters(name), format!("{time} · {dist} {}", unit.symbol().to_uppercase())]
}

/// A speed for the tray menu, e.g. "250 m/s", "0.16 mi/s".
pub fn speed(km_s: f64, unit: Unit) -> String {
    let v = unit.of(km_s);
    let digits = if v >= 100.0 { 0 } else if v >= 10.0 { 1 } else { 2 };
    let text = format!("{v:.digits$}");
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.') } else { &text };
    format!("{text} {}/s", unit.symbol())
}

/// `lines` drawn into a SIZE mask, row by row from the top: FILL in the letters, OUTLINE around
/// them, 0 elsewhere. Characters the font lacks are left out.
pub fn draw(lines: &[String]) -> Vec<u8> {
    let (w, h) = SIZE;
    let mut mask = vec![0u8; (w * h) as usize];
    for (row, line) in lines.iter().enumerate() {
        let top = 1 + row as u32 * LINE;
        for (i, c) in line.chars().filter_map(fold).enumerate() {
            let rows = GLYPHS.iter().find(|&&(g, _)| g == c).map_or([0; 7], |&(_, r)| r);
            let left = 1 + i as u32 * ADVANCE;
            for (y, bits) in rows.iter().enumerate() {
                for x in 0..5 {
                    let (px, py) = (left + x, top + y as u32);
                    if bits >> (4 - x) & 1 == 1 && px < w - 1 && py < h - 1 {
                        mask[(py * w + px) as usize] = FILL;
                    }
                }
            }
        }
    }
    let lit = mask.clone();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let near = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (x + dx, y + dy)));
            let touches = near.filter(|&(nx, ny)| nx >= 0 && ny >= 0 && nx < w as i32 && ny < h as i32).any(|(nx, ny)| lit[(ny * w as i32 + nx) as usize] == FILL);
            let at = (y * w as i32 + x) as usize;
            if touches && lit[at] != FILL {
                mask[at] = OUTLINE;
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn city_names_lose_their_accents() {
        assert_eq!(letters("Buôn Ma Thuột"), "BUON MA THUOT");
        assert_eq!(letters("Đà Nẵng"), "DA NANG");
        assert_eq!(letters("Thừa Thiên Huế"), "THUA THIEN HUE");
        assert_eq!(letters("Zürich"), "ZURICH");
        assert_eq!(letters("東京 Tokyo"), "TOKYO", "unknown letters left out");
        assert_eq!(letters(&"A".repeat(60)).len(), 42, "as many as fit");
    }

    #[test]
    fn the_eta_shows_the_time_and_distance_left() {
        assert_eq!(eta("Buôn Ma Thuột", 252.4, 251.2, Unit::Km), ["BUON MA THUOT", "4:12 · 252 KM"]);
        assert_eq!(eta("London", 9500.0, 9502.0, Unit::Mi), ["LONDON", "2:38:22 · 5903 MI"]);
        assert_eq!(eta("Huế", 0.42, 1.4, Unit::M), ["HUE", "0:02 · 420 M"]);
        assert_eq!(eta("Huế", 3.24, 4.0, Unit::Km)[1], "0:04 · 3.2 KM");
    }

    #[test]
    fn tray_speeds_read_in_the_unit() {
        assert_eq!(speed(0.25, Unit::M), "250 m/s");
        assert_eq!(speed(1.0, Unit::Km), "1 km/s");
        assert_eq!(speed(7.7, Unit::Km), "7.7 km/s");
        assert_eq!(speed(0.25, Unit::Mi), "0.16 mi/s");
        assert_eq!(speed(7.7, Unit::M), "7700 m/s");
        assert_eq!(speed(299_792.458, Unit::Km), "299792 km/s");
    }

    #[test]
    fn every_glyph_is_in_the_font() {
        for (set, _) in ACCENTS {
            assert!(set.chars().all(|c| fold(c).is_some()), "{set}");
        }
        assert!(eta("x", 1.0, 1.0, Unit::Km).iter().all(|l| l.chars().all(|c| fold(c).is_some())));
    }

    #[test]
    fn letters_are_filled_and_outlined() {
        let mask = draw(&["I".into(), String::new()]);
        let at = |x: u32, y: u32| mask[(y * SIZE.0 + x) as usize];
        // I's top bar: x 2-4 of the glyph at 1, row 1.
        assert_eq!([at(1, 1), at(2, 1), at(3, 1), at(4, 1), at(5, 1)], [OUTLINE, FILL, FILL, FILL, OUTLINE]);
        assert_eq!(at(3, 0), OUTLINE, "above");
        assert_eq!(at(3, 4), FILL, "the stem");
        assert_eq!(at(1, 4), 0, "clear of the stem");
        assert_eq!(mask.iter().filter(|&&v| v != 0).count(), mask.iter().take((SIZE.0 * 10) as usize).filter(|&&v| v != 0).count(), "nothing on the empty line");
    }
}
