//! Gnats, ported from the web's `src/gnats.js`. Dusk and dawn: a few columns of midges dance over the
//! pond rim (a mating swarm over a landmark). Each gnat circles, zips across or hangs, keeping clear
//! of its neighbours and inside the column. A dragonfly darting through scatters them, the nearest
//! first, and they drift back into the column. Same seeds and `rand()` order as the web.
use core::f64::consts::PI;

use libm::{atan2, cos, hypot, pow, round, sin};

use crate::flight::{self, Obstacle, V3};
use crate::motion::{Heading, ease, pick, rng_for, turn};
use crate::rng::Mulberry32;
use crate::world::{Layout, height_at, pond_point, smoothstep};

const SWARMS: usize = 2;
pub const PER_SWARM: usize = 10;
pub const SIZE: f64 = 0.067; // one art pixel
pub const COLOR: u32 = 0x4a4c3c; // in full light; dimmed with the sky so the specks don't glow at dusk
const RIM: f64 = 0.25; // swarm markers sit just off the shoreline
const COLUMN_Y: f64 = 1.3; // column centre above the ground
const COLUMN_R: f64 = 0.45; // horizontal radius of the column
const SPREAD_Y: f64 = 0.5; // each gnat keeps its own height within ±this of the centre
const SCARE_R: f64 = 0.6; // a darting dragonfly this close to the centre scatters the swarm
const COOLDOWN: f64 = 3.0;
const FLEE_SPEED: f64 = 1.2;
const SEED: u32 = 31;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Circle,
    Zip,
    Hang,
}

impl State {
    /// (speed, ease rate, min, max seconds)
    fn params(self) -> (f64, f64, f64, f64) {
        match self {
            State::Circle => (0.6, 3.0, 1.0, 3.0),
            State::Zip => (1.4, 10.0, 0.2, 0.5),
            State::Hang => (0.15, 5.0, 0.3, 1.2),
        }
    }
}

const NEXT: [(State, f64); 3] = [(State::Circle, 0.5), (State::Zip, 0.3), (State::Hang, 0.2)];

/// One column of midges, read by the dragonflies that hunt it.
pub struct Swarm {
    pub angle: f64, // where on the rim
    pub home: V3,
    pub centre: V3,
    pub visible: usize, // how many of its gnats show
    pub scared_at: f64,
    seed: f64,
}

struct Gnat {
    swarm: usize,
    k: usize,
    rand: Mulberry32,
    pos: V3,
    vel: V3,
    heading: Heading,
    speed: f64,
    state: State,
    timer: f64,
    spin: f64, // which way it circles
    height: f64,
    bob: f64, // up-and-down rate, rad/s
    phase: f64,
    pace: f64,
    reactivity: f64,
    flee_at: f64, // when it notices the dragonfly
    flee_until: f64,
    from: V3, // where the scare came from
}

pub struct Gnats {
    swarms: Vec<Swarm>,
    gnats: Vec<Gnat>,
    obstacles: Vec<Obstacle>,
    poses: Vec<Option<V3>>,
}

impl Gnats {
    pub fn new(layout: &Layout) -> Self {
        let obstacles = flight::obstacles(layout);
        let mut rand = Mulberry32::new(SEED);
        let mut swarms: Vec<Swarm> = Vec::new();
        for _ in 0..200 {
            if swarms.len() >= SWARMS {
                break;
            }
            let angle = rand.next() * PI * 2.0;
            let (x, z) = pond_point(angle, RIM);
            let home = V3::new(x, height_at(x, z) + COLUMN_Y, z);
            if swarms.iter().any(|s| s.home.distance(home) < 2.5) {
                continue;
            }
            // the column centre drifts up to ~0.35 off home, and the bobs add ~0.4 to the vertical spread
            let blocked = obstacles
                .iter()
                .any(|o| hypot(o.c.x - x, o.c.z - z) < o.r + COLUMN_R + 0.35 && (o.c.y - home.y).abs() < o.r + SPREAD_Y + 0.4);
            if blocked {
                continue;
            }
            swarms.push(Swarm { angle, home, centre: home, visible: 0, scared_at: f64::NEG_INFINITY, seed: rand.next() * 100.0 });
        }

        let mut gnats = Vec::with_capacity(swarms.len() * PER_SWARM);
        for (s, swarm) in swarms.iter().enumerate() {
            for k in 0..PER_SWARM {
                let mut rand = rng_for(SEED, (s * PER_SWARM + k) as u32);
                let mut r = || rand.next();
                let a = r() * PI * 2.0;
                let d = r() * COLUMN_R;
                let pos = V3::new(swarm.home.x + cos(a) * d, swarm.home.y, swarm.home.z + sin(a) * d);
                let yaw = r() * PI * 2.0 - PI;
                let timer = r() * 3.0; // out of step from the start
                let spin = if r() < 0.5 { 1.0 } else { -1.0 };
                let height = (r() * 2.0 - 1.0) * SPREAD_Y;
                let bob = 1.5 + r() * 2.0;
                let phase = r() * PI * 2.0;
                let pace = 0.8 + r() * 0.4;
                let reactivity = 0.35 + r() * 0.65;
                gnats.push(Gnat {
                    swarm: s,
                    k,
                    rand,
                    pos,
                    vel: V3::ZERO,
                    heading: Heading { yaw, yaw_vel: 0.0 },
                    speed: 0.0,
                    state: State::Circle,
                    timer,
                    spin,
                    height,
                    bob,
                    phase,
                    pace,
                    reactivity,
                    flee_at: f64::INFINITY,
                    flee_until: 0.0,
                    from: V3::ZERO,
                });
            }
        }
        let poses = vec![None; gnats.len()];
        Self { swarms, gnats, obstacles, poses }
    }

    pub fn swarms(&self) -> &[Swarm] {
        &self.swarms
    }

    /// Each gnat's position, or None while hidden. Swarm k's are `k * PER_SWARM..`.
    pub fn poses(&self) -> &[Option<V3>] {
        &self.poses
    }

    fn scatter(&mut self, s: usize, from: V3, t: f64) {
        self.swarms[s].scared_at = t;
        for g in self.gnats.iter_mut().filter(|g| g.swarm == s) {
            // The nearest react first; the far side and the sluggish ones a moment later.
            let dist = (g.pos.distance(from) / SCARE_R).min(2.0);
            g.flee_at = t + 0.03 + pow(dist, 1.5) * 0.35 + g.rand.next() * 0.12 + (1.0 - g.reactivity) * 0.1;
            g.from = from;
        }
    }

    /// Flocking within the swarm: separation, alignment, a pull to the centre, a hard edge,
    /// circling (in the circle state) and a slow wander. Returns the heading to aim for.
    fn heading(&self, i: usize, t: f64) -> f64 {
        let g = &self.gnats[i];
        let c = self.swarms[g.swarm].centre;
        let mut fx = sin(g.heading.yaw); // keep going roughly the same way
        let mut fz = cos(g.heading.yaw);
        let (mut ax, mut az, mut n) = (0.0, 0.0, 0);
        for (j, o) in self.gnats.iter().enumerate() {
            if j == i || o.swarm != g.swarm {
                continue;
            }
            let (dx, dz) = (g.pos.x - o.pos.x, g.pos.z - o.pos.z);
            let d = hypot(dx, dz);
            if d > 0.3 || d == 0.0 {
                continue;
            }
            if d < 0.08 {
                fx += dx / d * 2.8;
                fz += dz / d * 2.8;
            }
            ax += sin(o.heading.yaw);
            az += cos(o.heading.yaw);
            n += 1;
        }
        if n > 0 {
            fx += ax / n as f64 * 0.42;
            fz += az / n as f64 * 0.42;
        }
        let (rx, rz) = (c.x - g.pos.x, c.z - g.pos.z);
        let r = hypot(rx, rz);
        let r = if r == 0.0 { 1.0 } else { r };
        let pull = 0.6 * (r / COLUMN_R) + 4.8 * ((r - COLUMN_R).max(0.0) / 0.15);
        fx += rx / r * pull;
        fz += rz / r * pull;
        if g.state == State::Circle {
            fx += rz / r * g.spin;
            fz -= rx / r * g.spin;
        }
        let wander = sin(t * 2.9 + g.phase) * 0.7 + sin(t * 1.13 + g.phase * 1.73) * 0.45;
        atan2(fx, fz) + wander * 0.6
    }

    /// One step at animation time `t` after `dt`, at this sun elevation, with the dragonflies that
    /// are darting this frame.
    pub fn update(&mut self, t: f64, dt: f64, elevation: f64, darting: impl Iterator<Item = V3> + Clone) {
        let act = 1.0 - smoothstep(elevation.abs(), 0.4, 0.5);
        for s in 0..self.swarms.len() {
            let swarm = &mut self.swarms[s];
            // The column drifts slowly about its marker and bobs.
            let seed = swarm.seed;
            let drift = |f: f64| (sin(t * 0.29 + seed * f) * 0.7 + sin(t * 0.113 + seed * f * 1.73) * 0.45) * 0.3;
            swarm.centre = V3::new(swarm.home.x + drift(1.0), swarm.home.y + sin(t * 0.4 + seed) * 0.2, swarm.home.z + drift(2.0));
            swarm.visible = round(PER_SWARM as f64 * act) as usize;
            if swarm.visible == 0 || t - swarm.scared_at < COOLDOWN {
                continue;
            }
            let centre = swarm.centre;
            if let Some(hunter) = darting.clone().find(|h| h.distance(centre) < SCARE_R) {
                self.scatter(s, hunter, t);
            }
        }

        // Always simulated, even when hidden, so a swarm fades in already in formation.
        for i in 0..self.gnats.len() {
            let g = &mut self.gnats[i];
            if t >= g.flee_at {
                g.flee_at = f64::INFINITY;
                g.flee_until = t + 0.5 + g.rand.next() * 0.5;
            }
            let (aim, speed, k) = if t < g.flee_until {
                (atan2(g.pos.x - g.from.x, g.pos.z - g.from.z), FLEE_SPEED, 12.0)
            } else {
                g.timer -= dt;
                if g.timer <= 0.0 {
                    g.state = pick(&mut g.rand, &NEXT);
                    let (_, _, min, max) = g.state.params();
                    g.timer = min + g.rand.next() * (max - min);
                }
                let (speed, k, _, _) = g.state.params();
                let speed = speed * g.pace;
                (self.heading(i, t), speed, k)
            };
            let centre_y = self.swarms[self.gnats[i].swarm].centre.y;
            let g = &mut self.gnats[i];
            turn(&mut g.heading, aim, dt, 40.0, 8.0, 12.0);
            g.speed = ease(g.speed, speed, k, dt);
            g.vel = V3::new(sin(g.heading.yaw) * g.speed, 0.0, cos(g.heading.yaw) * g.speed);
            g.pos += g.vel * dt;
            g.pos.y = centre_y + g.height + sin(t * g.bob + g.phase) * 0.2;
            flight::avoid(&self.obstacles, &mut g.pos, &mut g.vel, dt, 0.5, None); // placement keeps the columns clear of obstacles; this is only a backstop
        }

        for (g, pose) in self.gnats.iter().zip(&mut self.poses) {
            *pose = (g.k < self.swarms[g.swarm].visible).then_some(g.pos);
        }
    }

    #[cfg(test)]
    pub(crate) fn obstacles(&self) -> &[Obstacle] {
        &self.obstacles
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dragonflies::{Dragonflies, State as Hunter, tests::simulate};
    use crate::sky::{DAY_LENGTH, sky_state};
    use crate::world;

    // Mean horizontal distance of swarm k's visible members from its centre.
    fn spread_of(g: &Gnats, k: usize) -> f64 {
        let w = &g.swarms()[k];
        let members = &g.poses()[k * PER_SWARM..k * PER_SWARM + w.visible];
        let sum: f64 = members.iter().flatten().map(|p| hypot(p.x - w.centre.x, p.z - w.centre.z)).sum();
        sum / w.visible as f64
    }

    #[test]
    fn gnats_dance_at_dusk_and_dawn_clear_of_everything_and_reform_after_a_scatter() {
        for fps in [60.0, 10.0] {
            struct Check {
                k: usize,
                at: f64,
                done: bool,
            }
            let mut last_scare = vec![f64::NEG_INFINITY; SWARMS];
            let mut checks: Vec<Check> = Vec::new();
            let mut spreads = Vec::new();
            let (mut nan, mut out_of_window, mut max_visible, mut min_alt, mut inside, mut scatters, mut unsettled) =
                (0, 0, 0, f64::INFINITY, 0, 0, 0);
            simulate(DAY_LENGTH, fps, |d, g, t, _| {
                assert_eq!((g.poses().len(), g.swarms().len()), (20, 2));
                let sky = sky_state(t, 0.0);
                let mut visible = 0;
                for p in g.poses().iter().flatten() {
                    visible += 1;
                    if !p.is_finite() {
                        nan += 1;
                    }
                    min_alt = min_alt.min(p.y - height_at(p.x, p.z));
                    if g.obstacles().iter().any(|o| o.c.distance(*p) < o.r - 0.01) {
                        inside += 1;
                    }
                }
                if visible > 0 && sky.elevation.abs() >= 0.5 {
                    out_of_window += 1;
                }
                max_visible = max_visible.max(visible);
                for (k, w) in g.swarms().iter().enumerate() {
                    if w.scared_at == last_scare[k] {
                        continue;
                    }
                    last_scare[k] = w.scared_at;
                    scatters += 1;
                    for c in checks.iter_mut().filter(|c| c.k == k) {
                        c.done = true; // re-scattered: that check restarts
                    }
                    checks.push(Check { k, at: t + 4.0, done: false });
                }
                for c in checks.iter_mut().filter(|c| !c.done && t >= c.at) {
                    c.done = true;
                    if g.swarms()[c.k].visible > 0 {
                        spreads.push(spread_of(g, c.k));
                    }
                }
                // Midnight (phase 0.75): every dragonfly is down in the grass, hunts or not.
                if (sky.phase - 0.75).abs() < 0.001 {
                    unsettled += d.states().filter(|&s| s != Some(Hunter::Settle)).count();
                }
            });
            assert_eq!(nan, 0, "{fps} fps");
            assert_eq!(out_of_window, 0, "{fps} fps: gnats seen outside dusk/dawn");
            assert_eq!(max_visible, 20, "{fps} fps");
            assert!(min_alt >= 0.45, "{fps} fps: min altitude {min_alt}");
            assert_eq!(inside, 0, "{fps} fps: gnats inside a tree or rock");
            assert!(scatters > 0, "{fps} fps: no dragonfly ever scattered a swarm");
            assert!(!spreads.is_empty() && spreads.iter().all(|&d| d < 0.5), "{fps} fps: spread 4 s after scatters: {spreads:?}");
            assert_eq!(unsettled, 0, "{fps} fps: a dragonfly was still flying at night");
        }
    }

    #[test]
    fn jumping_the_clock_to_dusk_finds_them_already_in_formation() {
        let layout = world::place(42);
        let (mut d, mut g) = (Dragonflies::new(&layout), Gnats::new(&layout));
        let noon = (0.25 - 0.08) * DAY_LENGTH; // sky seconds at phase 0.25 (DAY_START is 0.08)
        let dusk = (0.46 - 0.08) * DAY_LENGTH; // phase 0.46, about 17:00
        let mut checked = false;
        for i in 0..20 * 60 {
            let t = i as f64 / 60.0;
            let sky = sky_state(if t < 10.0 { noon + t } else { dusk + t }, 0.0); // what the time slider does
            let dt = if i == 0 { 0.0 } else { 1.0 / 60.0 };
            d.update(dt, sky.daylight, g.swarms());
            g.update(t, dt, sky.elevation, d.darting());
            if t >= 10.0 && !checked {
                checked = true;
                for k in 0..SWARMS {
                    assert_eq!(g.swarms()[k].visible, PER_SWARM);
                    assert!(spread_of(&g, k) < 0.5, "swarm {k} spread {}", spread_of(&g, k));
                }
            }
        }
        assert!(checked);
    }
}
