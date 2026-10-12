//! The post clock turned off: it shatters into the grass beside the path and stops at the time it
//! broke, the pieces weathering as they lie there (a few hours, then a day, then a week). Turned
//! back on, the pieces fly back along their fall in clock order and the hands race round to now.
use core::f64::consts::TAU;

use crate::clock;
use crate::rng::Mulberry32;
use crate::world::{Layout, path_edge, pond_edge, smoothstep};

/// Post segments (bottom up), the hub with the hands, the face's outer wedges, the 12 hour ticks
/// (12 o'clock first).
pub const POST: usize = 3;
pub const HUB: usize = POST;
pub const WEDGE: usize = HUB + 1;
pub const WEDGES: usize = 6;
pub const TICK: usize = WEDGE + WEDGES;
pub const PIECES: usize = TICK + 12;
pub const HUB_R: f64 = 0.18; // the face's centre piece, in clock units (before SCALE)

const CRACK: f64 = 0.5; // s the clock shivers before the first piece falls
const FALL: f64 = 2.0; // s from standing to lying still
const RETURN: f64 = 1.5; // s for one piece to fly back
const HOUR: f64 = 3600.0;
const DAY: f64 = 24.0 * HOUR;
const SEED: u32 = 12;

// The heap: distances from the post, angle across (rad).
const NEAR: f64 = 0.4;
const FAR: f64 = 2.0;
const SPREAD: f64 = 4.4;

/// Where a piece comes to rest in the grass, and how it tumbles getting there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rest {
    pub x: f64,
    pub z: f64,
    pub yaw: f64,
    pub axis: [f64; 3], // unit tumble axis
    pub turns: f64,     // how far it tips over on the way down (turns), settling back as it lands
}

pub fn rest_spots(layout: &Layout) -> [Rest; PIECES] {
    let mut rand = Mulberry32::new(SEED);
    let mut rests: Vec<Rest> = Vec::with_capacity(PIECES);
    // One heap at the post's foot, on the side away from the path.
    let e = 0.01;
    let away = (path_edge(clock::X, clock::Z + e) - path_edge(clock::X, clock::Z - e)).atan2(path_edge(clock::X + e, clock::Z) - path_edge(clock::X - e, clock::Z));
    for _ in 0..PIECES {
        let mut spot = (clock::X, clock::Z);
        // Open grass, off the path and the water, clear of trunks, rocks and the other pieces (that
        // last one waived after 150 tries). No open spot in 200 leaves it at the post's foot.
        for attempt in 0..200 {
            let (a, d) = (away + (rand.next() - 0.5) * SPREAD, NEAR + rand.next() * (FAR - NEAR));
            let (x, z) = (clock::X + a.cos() * d, clock::Z + a.sin() * d);
            let open = path_edge(x, z) > 0.25
                && pond_edge(x, z) > 0.3
                && layout.trees.iter().all(|t| (t.x - x).hypot(t.z - z) > 0.45)
                && layout.rocks.iter().all(|k| (k.x - x).hypot(k.z - z) > k.r + 0.2);
            if open && (attempt >= 150 || rests.iter().all(|r| (r.x - x).hypot(r.z - z) > 0.15)) {
                spot = (x, z);
                break;
            }
        }
        let axis = [rand.next() - 0.5, rand.next() - 0.5, rand.next() - 0.5];
        let len = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt().max(1e-6);
        let yaw = rand.next() * TAU;
        let turns = 0.1 + rand.next() * 0.15;
        rests.push(Rest { x: spot.0, z: spot.1, yaw, axis: axis.map(|c| c / len), turns });
    }
    rests.try_into().unwrap_or_else(|_| unreachable!())
}

/// (how far along the ground 0-1, height) at fall progress `p`, from height `y0` at the clock to
/// `y1` lying in the grass: a little toss and a fall, then one small bounce.
pub fn arc(p: f64, y0: f64, y1: f64) -> (f64, f64) {
    const LAND: f64 = 0.7; // of the time in the air
    const AIR: f64 = 0.88; // of the way covered before the bounce
    let hump = |s: f64, h: f64| h * 4.0 * s * (1.0 - s);
    if p < LAND {
        let s = p / LAND;
        (AIR * s, y0 + (y1 - y0) * s * s + hump(s, 0.15))
    } else {
        let s = ((p - LAND) / (1.0 - LAND)).min(1.0);
        (AIR + (1.0 - AIR) * s, y1 + hump(s, 0.05))
    }
}

/// How weathered a piece is after lying `age` s in the grass: a third over the first 3 hours,
/// two thirds after a day, fully after a week.
pub fn weathering(age: f64) -> f64 {
    (smoothstep(age, 0.0, 3.0 * HOUR) + smoothstep(age, DAY, DAY + HOUR) + smoothstep(age, 7.0 * DAY, 7.0 * DAY + HOUR)) / 3.0
}

/// Seconds after mending starts that piece `i` lifts off: the post from the ground up, the hub,
/// the rim, then the hours 1 to 12 round the face.
pub fn mend_delay(i: usize) -> f64 {
    match i {
        _ if i < HUB => 0.25 * i as f64,
        HUB => 0.9,
        _ if i < TICK => 1.3 + 0.15 * (i - WEDGE) as f64,
        _ => 2.4 + 0.2 * ((i - TICK + 11) % 12) as f64, // 1 o'clock first, 12 last
    }
}

/// Seconds after breaking that piece `i` starts to fall, after the crack: the hours loose first
/// (in a jumble), then the rim, the hub, and the post from the top down.
pub fn fall_delay(i: usize) -> f64 {
    CRACK
        + match i {
            _ if i < HUB => 1.3 + 0.2 * (HUB - 1 - i) as f64,
            HUB => 1.1,
            _ if i < TICK => 0.55 + 0.1 * ((i - WEDGE) * 5 % WEDGES) as f64,
            _ => 0.04 * ((i - TICK) * 7 % 12) as f64,
        }
}

// How long the hands take to make up `travel` s.
fn catch_up_time(travel: f64) -> f64 {
    1.5 + 4.5 * (travel / (2.0 * HALF_DAY)).min(1.0)
}

const HALF_DAY: f64 = 12.0 * HOUR;

#[derive(Clone)]
pub struct Wreck {
    progress: [f64; PIECES], // 0 standing, 1 lying in the grass
    broken: bool,
    since: f64,            // animation s since it last broke or started mending
    stopped: Option<f64>,  // unix s the hands show while stopped; None once they've caught up
    fell: f64,             // unix s it broke: the pieces' age
    catch_up: Option<(f64, f64)>, // (s the hands make up, s spent so far)
    glow: f64,
}

impl Wreck {
    /// Standing, or lying in the grass since unix time `broken_at`.
    pub fn new(broken_at: Option<f64>) -> Self {
        let broken = broken_at.is_some();
        Self {
            progress: [if broken { 1.0 } else { 0.0 }; PIECES],
            broken,
            since: 0.0,
            stopped: broken_at,
            fell: broken_at.unwrap_or(0.0),
            catch_up: None,
            glow: if broken { 0.0 } else { 1.0 },
        }
    }

    pub fn set(&mut self, broken: bool, now: f64) {
        if broken == self.broken {
            return;
        }
        if broken {
            self.stopped = Some(now - self.behind(now)); // where the hands are, even mid catch-up
            self.fell = now;
            self.catch_up = None;
        }
        self.broken = broken;
        self.since = 0.0;
    }

    /// One step: `dt` animation s, `now` unix s.
    pub fn update(&mut self, dt: f64, now: f64) {
        self.since += dt;
        for (i, p) in self.progress.iter_mut().enumerate() {
            let moving = *p > 0.0 && *p < 1.0; // a piece in the air turns at once, never hangs
            if self.broken {
                if moving || self.since >= fall_delay(i) {
                    *p = (*p + dt / FALL).min(1.0);
                }
            } else if moving || self.since >= mend_delay(i) {
                *p = (*p - dt / RETURN).max(0.0);
            }
        }
        if !self.broken && self.progress.iter().all(|&p| p == 0.0) {
            if let Some(stopped) = self.stopped {
                match &mut self.catch_up {
                    None => {
                        // Whole turns of the dial look the same: make up at most a day.
                        let missed = (now - stopped).max(0.0);
                        let travel = if missed < HALF_DAY { missed } else { HALF_DAY + missed % HALF_DAY };
                        self.catch_up = Some((travel, 0.0));
                    }
                    Some((travel, spent)) => {
                        *spent += dt;
                        if *spent >= catch_up_time(*travel) {
                            self.catch_up = None;
                            self.stopped = None;
                        }
                    }
                }
            }
        }
        let lit = !self.broken && self.stopped.is_none();
        self.glow = if lit { (self.glow + dt / 0.6).min(1.0) } else { (self.glow - dt / 0.3).max(0.0) };
    }

    pub fn broken(&self) -> bool {
        self.broken
    }

    pub fn progress(&self) -> &[f64; PIECES] {
        &self.progress
    }

    /// The unix time the hands stopped at, while they're stopped.
    pub fn stopped(&self) -> Option<f64> {
        self.stopped
    }

    /// How far behind the local time the hands are (s).
    pub fn behind(&self, now: f64) -> f64 {
        match (self.catch_up, self.stopped) {
            (Some((travel, spent)), _) => travel * (1.0 - smoothstep(spent, 0.0, catch_up_time(travel))),
            (None, Some(stopped)) => (now - stopped).max(0.0),
            (None, None) => 0.0,
        }
    }

    /// The pieces' weathering lying in the grass (0-1).
    pub fn weathering(&self, now: f64) -> f64 {
        if self.stopped.is_some() { weathering(now - self.fell) } else { 0.0 }
    }

    /// 0-1: how hard the pieces still standing shiver as the clock cracks.
    pub fn shiver(&self) -> f64 {
        if self.broken { smoothstep(self.since, 0.0, CRACK) } else { 0.0 }
    }

    /// The face's glow, 0-1: out while broken, back once the hands have caught up.
    pub fn glow(&self) -> f64 {
        self.glow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world;

    const T0: f64 = 1.8e9;

    fn run(w: &mut Wreck, seconds: f64, now: &mut f64) {
        for _ in 0..(seconds * 15.0).round() as usize {
            *now += 1.0 / 15.0;
            w.update(1.0 / 15.0, *now);
        }
    }

    #[test]
    fn weathering_steps_over_hours_then_a_day_then_a_week() {
        assert_eq!(weathering(0.0), 0.0);
        assert!((weathering(3.0 * HOUR) - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(weathering(20.0 * HOUR), weathering(3.0 * HOUR), "steady through the first day");
        assert!((weathering(DAY + HOUR) - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(weathering(6.0 * DAY), weathering(DAY + HOUR), "steady through the week");
        assert_eq!(weathering(7.0 * DAY + HOUR), 1.0);
        assert_eq!(weathering(60.0 * DAY), 1.0, "a week is the most");
        let ages: Vec<f64> = (0..2000).map(|i| i as f64 * 0.005 * DAY).collect();
        assert!(ages.windows(2).all(|a| weathering(a[1]) >= weathering(a[0])));
    }

    #[test]
    fn pieces_mend_post_first_then_the_hub_the_rim_and_the_hours_in_order() {
        let order: Vec<usize> = (0..WEDGE).chain(WEDGE..TICK).chain((1..12).chain([0]).map(|h| TICK + h)).collect();
        assert_eq!(order.len(), PIECES);
        assert!(order.windows(2).all(|w| mend_delay(w[0]) < mend_delay(w[1])), "{:?}", order.iter().map(|&i| mend_delay(i)).collect::<Vec<_>>());
    }

    #[test]
    fn pieces_fall_after_the_crack_hours_first_then_the_rim_the_hub_and_the_post_top_down() {
        let last_tick = (TICK..PIECES).map(fall_delay).fold(0.0, f64::max);
        assert!((TICK..PIECES).all(|i| fall_delay(i) >= CRACK));
        assert!((WEDGE..TICK).all(|i| fall_delay(i) > last_tick));
        assert!((WEDGE..TICK).all(|i| fall_delay(i) < fall_delay(HUB)));
        let post: Vec<f64> = (0..HUB).rev().map(fall_delay).collect();
        assert!(fall_delay(HUB) < post[0] && post.windows(2).all(|w| w[0] < w[1]), "{post:?}");
    }

    #[test]
    fn the_pieces_land_on_open_grass_around_the_post() {
        let layout = world::place(42);
        let rests = rest_spots(&layout);
        assert_eq!(rests, rest_spots(&layout), "the same every time");
        for r in &rests {
            let d = (r.x - clock::X).hypot(r.z - clock::Z);
            assert!((NEAR..=FAR).contains(&d), "{d} from the post");
            assert!(path_edge(r.x, r.z) > 0.2, "on the path at {}, {}", r.x, r.z);
            assert!(pond_edge(r.x, r.z) > 0.3);
            assert!(layout.trees.iter().all(|t| (t.x - r.x).hypot(t.z - r.z) > 0.4));
            assert!(layout.rocks.iter().all(|k| (k.x - r.x).hypot(k.z - r.z) > k.r + 0.15));
            let axis = r.axis[0] * r.axis[0] + r.axis[1] * r.axis[1] + r.axis[2] * r.axis[2];
            assert!((axis - 1.0).abs() < 1e-9 && r.turns > 0.0 && r.turns <= 0.25, "tips over at most a quarter turn");
        }
        let apart = rests.iter().enumerate().all(|(i, a)| rests[..i].iter().all(|b| (a.x - b.x).hypot(a.z - b.z) > 0.1));
        assert!(apart, "pieces on top of each other");
        let mean = rests.iter().fold((0.0, 0.0), |m, r| (m.0 + r.x - clock::X, m.1 + r.z - clock::Z));
        let heap = (clock::X + mean.0 / PIECES as f64, clock::Z + mean.1 / PIECES as f64);
        assert!(path_edge(heap.0, heap.1) > path_edge(clock::X, clock::Z), "the heap leans away from the path");
    }

    #[test]
    fn the_fall_tosses_lands_and_settles_never_below_the_grass() {
        let (y0, y1) = (2.5, 0.1);
        assert_eq!(arc(0.0, y0, y1), (0.0, y0));
        assert_eq!(arc(1.0, y0, y1), (1.0, y1));
        let mut last = (0.0, y0);
        for i in 1..=1000 {
            let (along, y) = arc(i as f64 / 1000.0, y0, y1);
            assert!(y >= y1 - 1e-12 && along >= last.0, "at {i}");
            assert!((y - last.1).abs() < 0.02 && along - last.0 < 0.01, "a jump at {i}");
            last = (along, y);
        }
        assert!(arc(0.1, y0, y1).1 > y0, "tossed up first");
    }

    #[test]
    fn breaking_lays_every_piece_down_and_mending_brings_them_back_then_the_hands_catch_up() {
        let mut now = T0;
        let mut w = Wreck::new(None);
        assert!(w.progress().iter().all(|&p| p == 0.0));
        assert_eq!((w.behind(now), w.glow(), w.stopped()), (0.0, 1.0, None));

        w.set(true, now);
        assert_eq!(w.stopped(), Some(T0));
        run(&mut w, CRACK - 0.1, &mut now);
        assert!(w.progress().iter().all(|&p| p == 0.0) && w.shiver() > 0.5, "cracking, still standing");
        run(&mut w, 4.5 - CRACK + 0.1, &mut now);
        assert!(w.progress().iter().all(|&p| p == 1.0), "{:?}", w.progress());
        assert!((w.behind(now) - (now - T0)).abs() < 1e-3, "stopped at the time it broke");
        assert_eq!(w.glow(), 0.0);
        assert!((w.weathering(now) - weathering(now - T0)).abs() < 1e-6);

        now += 3.0 * HOUR + 20.0 * 60.0;
        assert!((w.weathering(now) - 1.0 / 3.0).abs() < 1e-6);
        w.set(false, now);
        run(&mut w, 0.3, &mut now);
        assert!(w.progress()[0] < 1.0 && w.progress()[TICK] == 1.0, "the post first");
        let missed = now - T0;
        let mut t = 0.0;
        while w.progress().iter().any(|&p| p > 0.0) {
            run(&mut w, 1.0 / 15.0, &mut now);
            t += 1.0 / 15.0;
            assert!(t < 7.0, "never mended");
        }
        let behind = w.behind(now);
        assert!((behind - (now - T0)).abs() < 0.2, "still stopped as the last piece lands: {behind} vs {missed}");
        let mut last = behind;
        while w.stopped().is_some() {
            run(&mut w, 1.0 / 15.0, &mut now);
            let b = w.behind(now);
            assert!(b <= last + 1.0 / 15.0 + 1e-6, "the hands go forward");
            last = b;
            t += 1.0 / 15.0;
            assert!(t < 14.0, "never caught up");
        }
        assert_eq!(w.behind(now), 0.0);
        run(&mut w, 1.0, &mut now);
        assert_eq!(w.glow(), 1.0);
        assert_eq!(w.weathering(now), 0.0);
    }

    #[test]
    fn mending_mid_fall_turns_the_pieces_straight_back() {
        let mut now = T0;
        let mut w = Wreck::new(None);
        w.set(true, now);
        run(&mut w, 2.0, &mut now);
        let before = *w.progress();
        assert!(before.iter().any(|&p| p > 0.0 && p < 1.0) && before[0] == 0.0, "{before:?}");
        w.set(false, now);
        assert_eq!(w.shiver(), 0.0);
        run(&mut w, 1.0 / 15.0, &mut now);
        let after = w.progress();
        assert!(after.iter().zip(&before).all(|(a, b)| a < b || *b == 0.0 && *a == 0.0), "a piece hung in the air");
    }

    #[test]
    fn days_broken_make_up_the_time_mod_12_hours_in_at_most_two_turns() {
        for missed in [5.0 * 60.0, 5.0 * HOUR, 13.0 * HOUR, 3.0 * DAY + 2.0 * HOUR] {
            let mut w = Wreck::new(Some(T0));
            let mut now = T0 + missed;
            w.set(false, now);
            while w.progress().iter().any(|&p| p > 0.0) {
                run(&mut w, 1.0 / 15.0, &mut now);
            }
            let travel = w.behind(now);
            let shown = (now - T0) % (12.0 * HOUR);
            assert!(travel <= 24.0 * HOUR, "{travel}");
            assert!(((travel % (12.0 * HOUR)) - shown).abs() < 1e-3, "the hands start where they stopped: {travel} vs {shown}");
        }
    }

    #[test]
    fn starting_broken_lies_still_with_the_hands_stopped() {
        let mut w = Wreck::new(Some(T0));
        let mut now = T0 + 100.0;
        run(&mut w, 1.0, &mut now);
        assert!(w.progress().iter().all(|&p| p == 1.0));
        assert_eq!(w.glow(), 0.0);
        assert!((w.behind(now) - 101.0).abs() < 1e-6);
    }

    #[test]
    fn breaking_again_while_catching_up_stops_the_hands_where_they_are() {
        let mut w = Wreck::new(Some(T0));
        let mut now = T0 + 5.0 * HOUR;
        w.set(false, now);
        while w.progress().iter().any(|&p| p > 0.0) {
            run(&mut w, 1.0 / 15.0, &mut now);
        }
        run(&mut w, 1.0, &mut now);
        let shown = w.behind(now);
        assert!(shown > 0.0 && shown < 5.0 * HOUR);
        w.set(true, now);
        assert!((w.behind(now) - shown).abs() < 1e-6);
        assert!(w.weathering(now) < 0.01, "a fresh break isn't weathered");
    }
}
