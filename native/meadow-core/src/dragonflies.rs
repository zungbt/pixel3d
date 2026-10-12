//! Dragonflies, ported from the web's `src/dragonflies.js`. Day: each one holds a stretch of the pond
//! rim, patrolling it in fast straight darts with hovers between, perching on a shore rock (or the
//! rim) now and then, and chasing off any rival that comes within a body length or so. Around dusk
//! and dawn it hawks through the midge swarms over its stretch. Before each straight flight it
//! pivots in the air, then shoots off. At dusk it drops into the grass near the shore.
//! A couple of roamers do the same over the open meadow instead: darts between clear spots near
//! where they are, rests on grass tips, and the night in the grass wherever they end up.
//! Same per-dragonfly streams and `rand()` order as the web.
use core::f64::consts::PI;

use libm::{atan2, ceil, floor, hypot};

use crate::flight::{self, Obstacle, V3};
use crate::gnats::Swarm;
use crate::motion::{Heading, pick, rng_for, turn, wrap_angle};
use crate::rng::Mulberry32;
use crate::world::{Layout, height_at, pond_edge, pond_point};

pub const COLORS: [u32; 3] = [0x3f7fd0, 0x4f9e5a, 0xc4533a]; // emperor blue, hawker green, common darter red
pub const ROAMERS: [u32; 2] = [0x4f9e5a, 0xc4533a]; // hawker and darter hunting over the meadow, away from the water
pub const WING_COLOR: u32 = 0xe4eef4;
const ROAM_STEP: f64 = 4.0; // half-width of the square a roamer picks its next spot in, around where it is
pub const BODY_L: f64 = 0.35; // ~5 art pixels
pub const WING_L: f64 = 0.16;
pub const WING_W: f64 = 0.06;
pub const WING_Z: [f64; 2] = [0.09, 0.02]; // front and hind wing along the body
const DART_ACCEL: f64 = 30.0; // reaches full speed in ~0.13 s
const SECTOR: f64 = 0.9; // half-width (radians) of each territory around the rim
const FLICK: f64 = 0.35; // wing angle, alternating sign every frame in flight
const FACING: f64 = 0.3; // radians: a straight flight only sets off once the body points this close to its target
const HUNT: f64 = 0.35; // chance, after a hover, of a pass through a midge swarm over its own stretch of rim
const SEED: u32 = 77;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Dart,
    Hover,
    ToPerch,
    Perch,
    ToSettle,
    Settle,
}

struct Fly {
    rand: Mulberry32,
    pos: V3,
    vel: V3,
    target: V3,
    state: Option<State>,
    rock: Option<usize>, // the rock (layout index) being flown to or perched on
    timer: f64,
    cooldown: f64, // after a rival chase, so the pair doesn't re-trigger every frame
    flick: bool,
    home: Option<f64>, // its stretch of rim (angle); None for a roamer
    rocks: Vec<usize>,
    threshold: f64, // daylight below this sends it into the grass
    dart_speed: f64,
    hover_scale: f64, // longer or shorter hovers than average
    perch_chance: f64,
    heading: Heading,
}

/// What the renderer draws for one dragonfly: its position, heading about +y (0 faces +z), the
/// wings' angle from flat and the body colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: V3,
    pub yaw: f64,
    pub wing: f64,
    pub color: u32,
}

pub struct Dragonflies {
    flies: Vec<Fly>,
    poses: Vec<Pose>,
    rock_tops: Vec<V3>, // every layout rock's top
    obstacles: Vec<Obstacle>,
}

impl Dragonflies {
    pub fn new(layout: &Layout) -> Self {
        let shore_rocks: Vec<usize> = (0..layout.rocks.len()).filter(|&i| pond_edge(layout.rocks[i].x, layout.rocks[i].z) < 2.5).collect();
        let colors: Vec<u32> = COLORS.iter().chain(&ROAMERS).copied().collect();
        let flies = (0..colors.len())
            .map(|i| {
                let roamer = i >= COLORS.len();
                let mut rand = rng_for(SEED, i as u32); // its own stream: one dragonfly's choices never shift another's
                let home = i as f64 / COLORS.len() as f64 * PI * 2.0 + 0.4;
                let (hx, hz) = pond_point(home, 0.0);
                let rocks = if roamer {
                    Vec::new()
                } else {
                    shore_rocks.iter().copied().filter(|&r| hypot(layout.rocks[r].x - hx, layout.rocks[r].z - hz) < 3.0).collect()
                };
                let mut r = || rand.next();
                let (threshold, dart_speed, hover_scale, perch_chance) = (0.45 + r() * 0.1, 3.4 + r() * 1.2, 0.7 + r() * 0.7, 0.15 + r() * 0.2);
                Fly {
                    rand,
                    pos: V3::ZERO,
                    vel: V3::ZERO,
                    target: V3::ZERO,
                    state: None,
                    rock: None,
                    timer: 0.0,
                    cooldown: 0.0,
                    flick: false,
                    home: if roamer { None } else { Some(home) },
                    rocks,
                    threshold,
                    dart_speed,
                    hover_scale,
                    perch_chance,
                    heading: Heading::default(),
                }
            })
            .collect();
        let poses = colors.iter().map(|&color| Pose { pos: V3::ZERO, yaw: 0.0, wing: 0.0, color }).collect();
        Self { flies, poses, rock_tops: layout.rocks.iter().map(flight::rock_top).collect(), obstacles: flight::obstacles(layout) }
    }

    pub fn poses(&self) -> &[Pose] {
        &self.poses
    }

    /// Where the darting ones are: the gnats scatter from them.
    pub fn darting(&self) -> impl Iterator<Item = V3> + Clone + '_ {
        self.flies.iter().filter(|d| d.state == Some(State::Dart)).map(|d| d.pos)
    }

    #[cfg(test)]
    pub(crate) fn states(&self) -> impl Iterator<Item = Option<State>> + '_ {
        self.flies.iter().map(|d| d.state)
    }

    #[cfg(test)]
    pub(crate) fn obstacles(&self) -> &[Obstacle] {
        &self.obstacles
    }

    fn territory_angle(d: &mut Fly) -> f64 {
        d.home.unwrap() + (d.rand.next() * 2.0 - 1.0) * SECTOR
    }

    /// Open meadow at height y over the ground: in view, off the water, clear of trees and rocks.
    fn clear_at(&self, x: f64, z: f64, y: f64) -> bool {
        if !flight::in_roam(x, z) || pond_edge(x, z) < 0.5 {
            return false;
        }
        let p = V3::new(x, height_at(x, z) + y, z);
        !self.obstacles.iter().any(|o| o.c.distance(p) < o.r + 0.3)
    }

    /// A roamer's next spot near where it is, y over the ground, with a clear straight flight there.
    fn meadow_spot(&mut self, i: usize, y: f64) -> Option<V3> {
        for _ in 0..20 {
            let d = &mut self.flies[i];
            let p = d.pos;
            let x = p.x + (d.rand.next() * 2.0 - 1.0) * ROAM_STEP;
            let z = p.z + (d.rand.next() * 2.0 - 1.0) * ROAM_STEP;
            let steps = ceil(hypot(x - p.x, z - p.z) / 0.25).max(1.0) as u32;
            if (1..=steps).all(|j| {
                let f = j as f64 / steps as f64;
                self.clear_at(p.x + (x - p.x) * f, p.z + (z - p.z) * f, y)
            }) {
                return Some(V3::new(x, height_at(x, z) + y, z));
            }
        }
        None
    }

    /// Mostly over the water near the rim, a little over the bank; a roamer anywhere open nearby
    /// (or nowhere, so it just hovers again).
    fn dart(&mut self, i: usize) {
        if self.flies[i].home.is_none() {
            let y = 0.4 + self.flies[i].rand.next() * 0.4;
            let spot = self.meadow_spot(i, y);
            let d = &mut self.flies[i];
            d.target = spot.unwrap_or(d.pos);
        } else {
            let d = &mut self.flies[i];
            let angle = Self::territory_angle(d);
            let (x, z) = pond_point(angle, -1.2 + d.rand.next() * 1.8);
            d.target = V3::new(x, height_at(x, z) + 0.4 + d.rand.next() * 0.4, z);
        }
        let d = &mut self.flies[i];
        d.state = Some(State::Dart);
        d.rock = None;
        d.timer = 6.0; // give up and hover if it can't get there
    }

    /// An active midge swarm over its own stretch of rim, if there is one.
    fn prey_for(d: &Fly, swarms: &[Swarm]) -> Option<usize> {
        let home = d.home?;
        swarms.iter().position(|s| s.visible >= 5 && wrap_angle(s.angle - home).abs() < SECTOR + 0.3)
    }

    /// A dart straight through the swarm, coming out 0.8 beyond its centre (so it doesn't hover inside it).
    fn hunt(d: &mut Fly, swarm: &Swarm) {
        let dir = V3::new(swarm.centre.x - d.pos.x, 0.0, swarm.centre.z - d.pos.z).with_length(1.0);
        d.target = swarm.centre + dir * 0.8;
        d.state = Some(State::Dart);
        d.rock = None;
        d.timer = 6.0;
    }

    fn to_perch(&mut self, i: usize) {
        let d = &mut self.flies[i];
        d.rock = if d.rocks.is_empty() { None } else { Some(d.rocks[floor(d.rand.next() * d.rocks.len() as f64) as usize]) };
        if let Some(r) = d.rock {
            d.target = self.rock_tops[r];
        } else if d.home.is_none() {
            match self.meadow_spot(i, 0.45) {
                Some(spot) => self.flies[i].target = spot, // a grass tip
                None => return self.dart(i),
            }
        } else {
            let (x, z) = pond_point(Self::territory_angle(d), 0.1);
            d.target = V3::new(x, height_at(x, z) + 0.15, z);
        }
        let d = &mut self.flies[i];
        d.state = Some(State::ToPerch);
        d.timer = 6.0;
    }

    /// In the grass 1-3 units back from the shore, clear of trunks, crowns and rocks; a roamer
    /// nearby, or straight below if nothing nearby is clear.
    fn to_settle(&mut self, i: usize) {
        if self.flies[i].home.is_none() {
            let spot = self.meadow_spot(i, 0.2);
            let d = &mut self.flies[i];
            d.target = spot.unwrap_or(V3::new(d.pos.x, height_at(d.pos.x, d.pos.z) + 0.2, d.pos.z));
        } else {
            for _ in 0..10 {
                let d = &mut self.flies[i];
                let angle = Self::territory_angle(d);
                let (x, z) = pond_point(angle, 1.0 + d.rand.next() * 2.0);
                d.target = V3::new(x, height_at(x, z) + 0.2, z);
                let target = d.target;
                if !self.obstacles.iter().any(|o| o.c.distance(target) < o.r + 0.1) {
                    break;
                }
            }
        }
        let d = &mut self.flies[i];
        d.state = Some(State::ToSettle);
        d.rock = None;
        d.timer = 6.0;
    }

    fn hover(d: &mut Fly) {
        d.target = d.pos;
        d.state = Some(State::Hover);
        d.timer = (0.5 + d.rand.next() * 1.5) * d.hover_scale;
    }

    /// Straight-line flight to the target; true on arrival. Until the body faces the target it
    /// brakes instead (steering toward where it already is), so a re-dart never slides sideways.
    fn fly_to(obstacles: &[Obstacle], d: &mut Fly, dt: f64) -> bool {
        let (dx, dz) = (d.target.x - d.pos.x, d.target.z - d.pos.z);
        let facing = hypot(dx, dz) < 0.3 || wrap_angle(atan2(dx, dz) - d.heading.yaw).abs() < FACING;
        flight::steer(d.pos, &mut d.vel, if facing { d.target } else { d.pos }, d.dart_speed, DART_ACCEL, dt);
        flight::avoid(obstacles, &mut d.pos, &mut d.vel, dt, 0.15, d.rock);
        d.pos += d.vel * dt;
        d.timer -= dt;
        if d.pos.distance(d.target) > 0.1 {
            return false;
        }
        d.pos = d.target;
        d.vel = V3::ZERO;
        true
    }

    /// Holds station with tiny corrections.
    fn hover_step(obstacles: &[Obstacle], d: &mut Fly, dt: f64) {
        let mut r = || d.rand.next() - 0.5;
        let aim = V3::new(d.target.x + r() * 0.1, d.target.y + r() * 0.06, d.target.z + r() * 0.1);
        flight::steer(d.pos, &mut d.vel, aim, 0.4, 6.0, dt);
        flight::avoid(obstacles, &mut d.pos, &mut d.vel, dt, 0.15, None);
        d.pos += d.vel * dt;
    }

    /// One step of `dt` in this frame's daylight, with the midge swarms as they were left last frame.
    pub fn update(&mut self, dt: f64, daylight: f64, swarms: &[Swarm]) {
        for i in 0..self.flies.len() {
            let dark = daylight < self.flies[i].threshold;
            if self.flies[i].state.is_none() {
                // First frame: start wherever this time of day would have left it.
                if self.flies[i].home.is_none() {
                    loop {
                        let d = &mut self.flies[i];
                        d.pos = V3::new((d.rand.next() * 2.0 - 1.0) * 11.0, 0.0, (d.rand.next() * 2.0 - 1.0) * 11.0);
                        let p = d.pos;
                        if self.clear_at(p.x, p.z, 0.6) {
                            break;
                        }
                    }
                }
                if dark {
                    self.to_settle(i);
                    let d = &mut self.flies[i];
                    d.pos = d.target;
                    d.state = Some(State::Settle);
                } else {
                    self.dart(i);
                    let d = &mut self.flies[i];
                    d.pos = d.target;
                    Self::hover(d);
                }
            }

            let d = &mut self.flies[i];
            if d.state == Some(State::Settle) {
                if daylight > d.threshold + 0.05 {
                    self.dart(i);
                }
            } else if dark && d.state != Some(State::ToSettle) {
                self.to_settle(i);
            }

            let obstacles = &self.obstacles;
            let d = &mut self.flies[i];
            d.cooldown -= dt;
            match d.state.unwrap() {
                State::Dart => {
                    if Self::fly_to(obstacles, d, dt) || d.timer <= 0.0 {
                        Self::hover(d);
                    }
                }
                State::Hover => {
                    Self::hover_step(obstacles, d, dt);
                    d.timer -= dt;
                    if d.timer <= 0.0 {
                        #[derive(Clone, Copy, PartialEq)]
                        enum Next {
                            Dart,
                            Perch,
                            Hunt,
                        }
                        let prey = Self::prey_for(d, swarms);
                        let hunt = if prey.is_some() { HUNT } else { 0.0 };
                        let weights = [(Next::Dart, 1.0 - d.perch_chance - hunt), (Next::Perch, d.perch_chance), (Next::Hunt, hunt)];
                        match pick(&mut d.rand, &weights) {
                            Next::Perch => self.to_perch(i),
                            Next::Hunt => Self::hunt(d, &swarms[prey.unwrap()]),
                            Next::Dart => self.dart(i),
                        }
                    }
                }
                State::ToPerch => {
                    if Self::fly_to(obstacles, d, dt) {
                        d.state = Some(State::Perch);
                        d.timer = 3.0 + d.rand.next() * 7.0;
                    } else if d.timer <= 0.0 {
                        self.dart(i);
                    }
                }
                State::Perch => {
                    d.timer -= dt;
                    if d.timer <= 0.0 {
                        self.dart(i);
                    }
                }
                State::ToSettle => {
                    if Self::fly_to(obstacles, d, dt) {
                        d.state = Some(State::Settle);
                    } else if d.timer <= 0.0 {
                        self.to_settle(i);
                    }
                }
                State::Settle => {}
            }
        }

        // Rivals that meet in the air both dart off.
        let patrolling = |d: &Fly| matches!(d.state, Some(State::Dart | State::Hover));
        for i in 0..self.flies.len() {
            for j in i + 1..self.flies.len() {
                let (a, b) = (&self.flies[i], &self.flies[j]);
                if !patrolling(a) || !patrolling(b) || a.cooldown > 0.0 || b.cooldown > 0.0 || a.pos.distance(b.pos) > 1.0 {
                    continue;
                }
                for k in [i, j] {
                    self.dart(k);
                    self.flies[k].cooldown = 1.5;
                }
            }
        }

        for (d, pose) in self.flies.iter_mut().zip(&mut self.poses) {
            // The body swings round on a spring (at most 10 rad/s): toward the target on a straight
            // flight, along its drift in a hover, and stays put at rest.
            let state = d.state.unwrap();
            if matches!(state, State::Dart | State::ToPerch | State::ToSettle) {
                turn(&mut d.heading, atan2(d.target.x - d.pos.x, d.target.z - d.pos.z), dt, 60.0, 14.0, 10.0);
            } else if state == State::Hover && d.vel.length_sq() > 0.09 {
                turn(&mut d.heading, atan2(d.vel.x, d.vel.z), dt, 60.0, 14.0, 10.0);
            }
            // Wings beat too fast to see: alternate up/down every frame in flight, flat out at rest.
            let mut wing = 0.0;
            if !matches!(state, State::Perch | State::Settle) {
                d.flick = !d.flick;
                wing = if d.flick { FLICK } else { -FLICK };
            }
            pose.pos = d.pos;
            pose.yaw = d.heading.yaw;
            pose.wing = wing;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::gnats::Gnats;
    use crate::sky::sky_state;
    use crate::world;

    // The web's test/scene.js `simulate` for the two species that meet: a fixed fps, dt 0 on the
    // first frame, the manual sky day; dragonflies before gnats, as the web updates them.
    pub(crate) fn simulate(seconds: f64, fps: f64, mut on_frame: impl FnMut(&Dragonflies, &Gnats, f64, f64)) {
        let layout = world::place(42);
        let (mut d, mut g) = (Dragonflies::new(&layout), Gnats::new(&layout));
        let mut i = 0;
        while i as f64 / fps < seconds {
            let t = i as f64 / fps;
            let dt = if i == 0 { 0.0 } else { 1.0 / fps };
            let sky = sky_state(t, 0.0);
            d.update(dt, sky.daylight, g.swarms());
            g.update(t, dt, sky.elevation, d.darting());
            on_frame(&d, &g, t, dt);
            i += 1;
        }
    }

    #[test]
    fn dragonflies_pivot_at_most_10_rad_s_and_never_fly_sideways() {
        for fps in [60.0, 10.0] {
            let mut prev: Option<Vec<Pose>> = None;
            let (mut max_rate, mut fast, mut sideways, mut nan, mut min_alt) = (0.0_f64, 0, 0, 0, f64::INFINITY);
            let (mut roamer_frames, mut roamer_far, mut roamer_inside) = (0, 0, 0);
            simulate(300.0, fps, |d, _, _, dt| {
                assert_eq!(d.poses().len(), 5, "3 on the pond rim, then 2 roamers");
                for (i, p) in d.poses().iter().enumerate() {
                    let q = p.pos;
                    if !q.is_finite() {
                        nan += 1;
                    }
                    if i >= COLORS.len() {
                        roamer_frames += 1;
                        // The web counts > 6; native hills and rocks move where they spend the
                        // night, so 10 fps settles them 3-6 off the shore.
                        if pond_edge(q.x, q.z) > 3.0 {
                            roamer_far += 1;
                        }
                        if d.obstacles().iter().any(|o| o.c.distance(q) < o.r - 0.01) {
                            roamer_inside += 1;
                        }
                    }
                    min_alt = min_alt.min(q.y - height_at(q.x, q.z));
                    if let (Some(prev), true) = (&prev, dt > 0.0) {
                        let was = prev[i];
                        max_rate = max_rate.max(wrap_angle(p.yaw - was.yaw).abs() / dt);
                        let (vx, vz) = ((q.x - was.pos.x) / dt, (q.z - was.pos.z) / dt);
                        if hypot(vx, vz) > 2.0 {
                            fast += 1;
                            if wrap_angle(p.yaw - atan2(vx, vz)).abs() > 0.5 {
                                sideways += 1;
                            }
                        }
                    }
                }
                prev = Some(d.poses().to_vec());
            });
            assert_eq!(nan, 0, "{fps} fps");
            assert!(max_rate <= 10.0 + 1e-6, "{fps} fps: max turn rate {max_rate:.2} rad/s");
            assert!(fast > 0, "{fps} fps: no darts at all");
            assert!((sideways as f64) / (fast as f64) < 0.02, "{fps} fps: {sideways}/{fast} fast frames flying sideways");
            assert!(min_alt > 0.1, "{fps} fps: min altitude {min_alt}");
            assert!(roamer_far as f64 / roamer_frames as f64 > 0.3, "{fps} fps: roamers far from the pond only {roamer_far}/{roamer_frames} frames");
            assert_eq!(roamer_inside, 0, "{fps} fps: a roamer inside a tree or rock");
        }
    }
}
