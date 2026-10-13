//! Rain from the real weather: how hard it's falling (the streaks, the pond's rings), how wet the
//! meadow is (darker grass and path, drying slowly after), the gloom it brings (dimmer sun, greyer
//! light, insects taking cover) and, in a thunderstorm, the lightning.
use libm::exp;

use crate::rng::Mulberry32;
use crate::sky::{Rgb, SkyState};
use crate::weather::Weather;
use crate::world::lerp;

const EASE: f64 = 15.0; // s: the rain sets in and stops over this long (1/e)
const SOAK: f64 = 600.0; // s: steady rain soaks the meadow over some ten minutes...
const DRY: f64 = 3600.0; // s: ...and it dries over an hour
const FLASH_GAP: (f64, f64) = (4.0, 20.0); // s between flashes in a storm
const SECOND: f64 = 0.22; // s: the flash's flicker after the first

pub struct Rain {
    pub falling: f64, // 0 dry to 1 a downpour
    pub wet: f64,     // 0 dry to 1 soaked
    since: f64,       // s since the last flash
    next: f64,        // s until the next, in a storm
    rng: Mulberry32,
}

impl Default for Rain {
    fn default() -> Self {
        Rain { falling: 0.0, wet: 0.0, since: f64::INFINITY, next: FLASH_GAP.0, rng: Mulberry32::new(0x5eed_f1a5) }
    }
}

impl Rain {
    /// Steps `dt` on toward `weather`'s rain (none without).
    pub fn update(&mut self, dt: f64, weather: Option<&Weather>) {
        let target = weather.map_or(0.0, Weather::rain);
        self.falling += (target - self.falling) * (1.0 - exp(-dt / EASE));
        self.wet = if self.falling > 0.05 { (self.wet + dt * self.falling / SOAK).min(1.0) } else { (self.wet - dt / DRY).max(0.0) };
        self.since += dt;
        if weather.is_some_and(Weather::storm) {
            self.next -= dt;
            if self.next <= 0.0 {
                self.since = -self.next;
                self.next = lerp(FLASH_GAP.0, FLASH_GAP.1, self.rng.next());
            }
        }
    }

    /// The lightning's brightness, 0-1: a flash and a fainter flicker after it.
    pub fn flash(&self) -> f64 {
        let s = self.since;
        let first = exp(-s / 0.08);
        let second = if s >= SECOND { 0.7 * exp(-(s - SECOND) / 0.1) } else { 0.0 };
        (first + second).min(1.0)
    }

    /// `sky` under this rain: a dimmer sun, greyer light and, as far as the insects can tell,
    /// less daylight; lit up by the lightning.
    pub fn sky(&self, sky: &SkyState) -> SkyState {
        let r = self.falling;
        let flash = self.flash();
        let grey = |c: Rgb, dim: f64| {
            let l = (c[0] + c[1] + c[2]) / 3.0;
            c.map(|v| lerp(v, l, 0.6 * r) * (1.0 - dim * r))
        };
        let lit = |c: Rgb, k: f64| [c[0] + 0.85 * k * flash, c[1] + 0.9 * k * flash, c[2] + k * flash];
        SkyState {
            daylight: sky.daylight * (1.0 - r),
            sun_color: grey(sky.sun_color, 0.6),
            hemi_sky: lit(grey(sky.hemi_sky, 0.25), 2.5),
            background: lit(grey(sky.background, 0.3), 1.0),
            fog_color: grey(sky.fog_color, 0.2),
            ..*sky
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky::sky_state;
    use crate::sky::NOON;
    use crate::weather::tests::CLEAR;

    fn run(rain: &mut Rain, seconds: f64, weather: Option<&Weather>) {
        for _ in 0..(seconds * 15.0) as usize {
            rain.update(1.0 / 15.0, weather);
        }
    }

    #[test]
    fn rain_sets_in_soaks_and_dries() {
        let downpour = Weather { code: 65, ..CLEAR };
        let mut rain = Rain::default();
        run(&mut rain, 120.0, Some(&downpour));
        assert!(rain.falling > 0.99, "{}", rain.falling);
        assert!((0.15..0.25).contains(&rain.wet), "a couple of minutes: damp, {}", rain.wet);
        run(&mut rain, 900.0, Some(&downpour));
        assert_eq!(rain.wet, 1.0);
        // Stops: the falling eases off at once, the wet lingers.
        run(&mut rain, 120.0, Some(&CLEAR));
        assert!(rain.falling < 0.001 && rain.wet > 0.95, "{} {}", rain.falling, rain.wet);
        run(&mut rain, 3600.0, None);
        assert_eq!(rain.wet, 0.0);
    }

    #[test]
    fn rain_glooms_the_sky_and_lightning_lights_it() {
        let sky = sky_state(NOON, 0.0);
        let mut rain = Rain::default();
        assert_eq!(format!("{:?}", rain.sky(&sky)), format!("{sky:?}"), "dry: unchanged");
        run(&mut rain, 120.0, Some(&Weather { code: 63, ..CLEAR }));
        let wet = rain.sky(&sky);
        assert!(wet.sun_color[0] < sky.sun_color[0] * 0.7 && wet.daylight < sky.daylight * 0.5);
        assert_eq!(rain.flash(), 0.0, "no lightning without thunder");
        // A storm: flashes every 4-20 s, each lighting the sky up and gone in a second.
        let storm = Weather { code: 95, ..CLEAR };
        let (mut flashes, mut lit, mut was) = (0, 0, 0.0);
        for _ in 0..15 * 600 {
            rain.update(1.0 / 15.0, Some(&storm));
            let f = rain.flash();
            if f > 0.5 && was <= 0.5 {
                flashes += 1;
            }
            if f > 0.05 {
                lit += 1;
            }
            was = f;
        }
        assert!((600.0 / 20.0..=600.0 / 4.0 * 2.0).contains(&(flashes as f64)), "{flashes} flashes in 10 min");
        assert!((lit as f64) < 15.0 * 600.0 * 0.15, "lit {lit} frames");
        rain.since = 0.0;
        assert!(rain.sky(&sky).hemi_sky[2] > wet.hemi_sky[2] + 2.0);
    }
}
