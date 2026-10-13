//! Butterflies, ported from the web's `src/butterflies.js`. Day: erratic flower-to-flower flight,
//! feeding, the odd bask on a warm rock. When the light fades each one settles on a flower with its
//! wings closed until morning. Same per-butterfly streams and `rand()` order as the web.
use core::f64::consts::PI;

use libm::{atan2, cos, floor, hypot, sin, sqrt};

use crate::flight::{self, Obstacle, V3};
use crate::flowers::{HEAD_R, HEAD_Y, Head};
use crate::motion::{Heading, pick, rng_for, turn};
use crate::rng::Mulberry32;
use crate::wind::Wind;
use crate::world::{Layout, height_at, pond_edge};

pub const COUNT: usize = 12;
pub const COLORS: [u32; 4] = [0xf3f0e6, 0xf0d860, 0xe8964a, 0x8fb4ee]; // cabbage white, brimstone, orange tip, blue
pub const BODY: u32 = 0x3a3328;
pub const WING_W: f64 = 0.12; // one wing: half the ~0.25 wingspan, so ~4 art pixels across
pub const WING_L: f64 = 0.1;
const ACCEL: f64 = 3.0;
const FOLDED: f64 = PI / 2.0 - 0.08; // wings closed together over the back
const NEAR: f64 = 3.0; // usual hop to the next flower
const FAR: f64 = 10.0; // the longer hop, to another patch, or a wander over the meadow
const CROWD: f64 = 2.5; // a flower this close to another butterfly, or to where it's headed, is taken
const SEED: u32 = 2024;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Fly,
    Feed,
    Bask,
    Roost,
    ToBask,
    ToRoost,
}

/// The flower head (index into the roaming heads) or rock (index into the layout) being flown to
/// or sat on, or for a wander a point in the air.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Perch {
    Head(usize),
    Rock(usize),
    Air(V3),
}

struct Fly {
    rand: Mulberry32,
    pos: V3,
    vel: V3,
    state: Option<State>,
    perch: Option<Perch>,
    timer: f64,
    alt: f64,       // cruising height above the ground
    threshold: f64, // daylight below this sends it to roost
    wander: f64,
    flap: f64,
    speed: f64,
    flap_hz: f64,
    bask: f64, // chance of basking on a rock after feeding
    heading: Heading,
}

/// What the renderer draws for one butterfly: the body's position (with the wing-beat bob), its
/// heading about +y (0 faces +z) and the wings' angle from flat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: V3,
    pub yaw: f64,
    pub wing: f64,
    pub color: u32,
}

pub struct Butterflies {
    flies: Vec<Fly>,
    poses: Vec<Pose>,
    heads: Vec<Head>,   // the flowers on screen
    rocks: Vec<usize>,  // the rocks on screen, as layout indices
    rock_at: Vec<(V3, V3)>, // every layout rock's position and the top an insect sits on
    obstacles: Vec<Obstacle>,
}

impl Butterflies {
    pub fn new(layout: &Layout, flowers: &[Head]) -> Self {
        let flies = (0..COUNT as u32)
            .map(|i| {
                let mut rand = rng_for(SEED, i); // its own stream: one butterfly's choices never shift another's
                let mut r = || rand.next();
                let (alt, threshold, wander, flap) = (0.4 + r() * 0.5, 0.4 + r() * 0.2, r() * PI * 2.0, r() * 10.0);
                let (speed, flap_hz, bask) = (0.55 + r() * 0.3, 7.5 + r() * 3.0, 0.05 + r() * 0.15);
                Fly {
                    rand,
                    pos: V3::ZERO,
                    vel: V3::ZERO,
                    state: None,
                    perch: None,
                    timer: 0.0,
                    alt,
                    threshold,
                    wander,
                    flap,
                    speed,
                    flap_hz,
                    bask,
                    heading: Heading::default(),
                }
            })
            .collect();
        let poses = (0..COUNT).map(|i| Pose { pos: V3::ZERO, yaw: 0.0, wing: FOLDED, color: COLORS[i % COLORS.len()] }).collect();
        Self {
            flies,
            poses,
            heads: flowers.iter().copied().filter(|h| flight::in_roam(h.x, h.z)).collect(),
            rocks: (0..layout.rocks.len()).filter(|&i| flight::in_roam(layout.rocks[i].x, layout.rocks[i].z)).collect(),
            rock_at: layout.rocks.iter().map(|r| (V3::new(r.x, r.y, r.z), flight::rock_top(r))).collect(),
            obstacles: flight::obstacles(layout),
        }
    }

    pub fn poses(&self) -> &[Pose] {
        &self.poses
    }

    /// One step at animation time `t` (s) after `dt`, in this frame's daylight and wind.
    pub fn update(&mut self, t: f64, dt: f64, daylight: f64, wind: &Wind) {
        for i in 0..self.flies.len() {
            self.update_one(i, t, dt, daylight, wind);
        }
    }

    fn update_one(&mut self, i: usize, t: f64, dt: f64, daylight: f64, wind: &Wind) {
        let dark = daylight < self.flies[i].threshold;
        if self.flies[i].state.is_none() {
            // First frame: start wherever this time of day would have left it.
            let head = Perch::Head(self.random_head(i));
            self.flies[i].perch = Some(head);
            self.flies[i].pos = self.perch_point(head, wind);
            if dark {
                self.flies[i].state = Some(State::Roost);
            } else {
                let near = self.head_near(i, NEAR);
                self.fly_to(i, State::Fly, near);
            }
        }

        let state = self.flies[i].state.unwrap();
        if state == State::Roost {
            if daylight > self.flies[i].threshold + 0.05 {
                let near = self.head_near(i, NEAR);
                self.fly_to(i, State::Fly, near);
            }
        } else if dark && state != State::ToRoost {
            let nearest = self.nearest_head(self.flies[i].pos);
            self.fly_to(i, State::ToRoost, nearest);
        }

        let state = self.flies[i].state.unwrap();
        let flying = matches!(state, State::Fly | State::ToBask | State::ToRoost);
        if flying {
            if self.fly(i, dt, wind) {
                // A wander ends in the air; from there it looks for a flower close by.
                if matches!(self.flies[i].perch, Some(Perch::Air(_))) {
                    let near = self.head_near(i, NEAR);
                    self.fly_to(i, State::Fly, near);
                } else {
                    self.land(i);
                }
            } else if self.flies[i].timer <= 0.0 {
                let far = self.head_near(i, FAR);
                self.fly_to(i, if state == State::ToRoost { State::ToRoost } else { State::Fly }, far);
            }
        } else {
            self.flies[i].pos = self.perch_point(self.flies[i].perch.unwrap(), wind);
            if state != State::Roost {
                self.flies[i].timer -= dt;
                if self.flies[i].timer <= 0.0 {
                    self.next_after_feeding(i);
                }
            }
        }

        // Wings: a fast beat in flight (the body bobs with each stroke), a slow open-close while
        // feeding, flat open when basking, folded shut at roost.
        let b = &mut self.flies[i];
        let mut pos = b.pos;
        let wing = if flying {
            b.flap += dt * b.flap_hz * PI * 2.0;
            pos.y += sin(b.flap) * 0.03;
            // The body swings round toward its flight on a spring (at most 6 rad/s), never snapping.
            if b.vel.length_sq() > 0.0025 {
                turn(&mut b.heading, atan2(b.vel.x, b.vel.z), dt, 30.0, 10.0, 6.0);
            }
            0.6 + 0.75 * sin(b.flap)
        } else {
            match b.state.unwrap() {
                State::Feed => 0.9 + 0.6 * sin(t * 1.5 + b.wander),
                State::Bask => 0.05,
                _ => FOLDED,
            }
        };
        let pose = &mut self.poses[i];
        pose.pos = pos;
        pose.yaw = b.heading.yaw;
        pose.wing = wing;
    }

    fn random_head(&mut self, i: usize) -> usize {
        floor(self.flies[i].rand.next() * self.heads.len() as f64) as usize
    }

    /// Where a perch is: on top of the swaying flower head or the rock, or the wander's point.
    fn perch_point(&self, perch: Perch, wind: &Wind) -> V3 {
        match perch {
            Perch::Air(p) => p,
            Perch::Rock(r) => self.rock_at[r].1,
            Perch::Head(h) => {
                let h = self.heads[h];
                let (sx, sz) = wind.sway(h.x, h.z, HEAD_Y);
                V3::new(h.x + sx, h.y + HEAD_R, h.z + sz)
            }
        }
    }

    // The perch's resting spot, unswayed: what crowding and hop lengths measure from.
    fn perch_at(&self, perch: Perch) -> V3 {
        match perch {
            Perch::Air(p) => p,
            Perch::Rock(r) => self.rock_at[r].0,
            Perch::Head(h) => V3::new(self.heads[h].x, self.heads[h].y, self.heads[h].z),
        }
    }

    /// Free flowers: not near another butterfly or where it's headed (all of them if none are).
    fn free_heads(&self, i: usize) -> Vec<usize> {
        let crowded = |h: &Head| {
            self.flies.iter().enumerate().any(|(j, o)| {
                j != i && (hypot(h.x - o.pos.x, h.z - o.pos.z) < CROWD || o.perch.is_some_and(|p| {
                    let at = self.perch_at(p);
                    hypot(h.x - at.x, h.z - at.z) < CROWD
                }))
            })
        };
        let free: Vec<usize> = (0..self.heads.len()).filter(|&k| !crowded(&self.heads[k])).collect();
        if free.is_empty() { (0..self.heads.len()).collect() } else { free }
    }

    /// A random free flower within `max_dist` of the butterfly (then FAR, then anywhere).
    fn head_near(&mut self, i: usize, max_dist: f64) -> Perch {
        let pool = self.free_heads(i);
        let p = self.flies[i].pos;
        for r in [max_dist, FAR] {
            let near: Vec<usize> = pool.iter().copied().filter(|&k| hypot(self.heads[k].x - p.x, self.heads[k].z - p.z) < r).collect();
            if !near.is_empty() {
                return Perch::Head(near[floor(self.flies[i].rand.next() * near.len() as f64) as usize]);
            }
        }
        Perch::Head(pool[floor(self.flies[i].rand.next() * pool.len() as f64) as usize])
    }

    /// A random point within FAR of the butterfly, in view and off the water.
    fn point_near(&mut self, i: usize) -> (f64, f64) {
        let b = &mut self.flies[i];
        loop {
            let a = b.rand.next() * PI * 2.0;
            let r = FAR * sqrt(b.rand.next()); // uniform over the disc
            let (x, z) = (b.pos.x + cos(a) * r, b.pos.z + sin(a) * r);
            if flight::in_roam(x, z) && pond_edge(x, z) >= 0.0 {
                return (x, z);
            }
        }
    }

    /// The long hop: the free flower nearest a random point, so a lone flower in a bare stretch gets
    /// as many visits as the ground it stands for.
    fn head_by_area(&mut self, i: usize) -> Perch {
        let (px, pz) = self.point_near(i);
        let dist = |h: &Head| hypot(h.x - px, h.z - pz);
        let mut best: Option<usize> = None;
        for k in self.free_heads(i) {
            if best.is_none_or(|b| dist(&self.heads[k]) < dist(&self.heads[b])) {
                best = Some(k);
            }
        }
        Perch::Head(best.unwrap())
    }

    /// A wander: open air at its cruising height somewhere within FAR, clear of trees and rocks.
    fn air_near(&mut self, i: usize) -> Perch {
        for _ in 0..10 {
            let (x, z) = self.point_near(i);
            let spot = V3::new(x, height_at(x, z) + self.flies[i].alt, z);
            if !self.obstacles.iter().any(|o| o.c.distance(spot) < o.r + 0.3) {
                return Perch::Air(spot);
            }
        }
        self.head_by_area(i)
    }

    fn nearest_head(&self, p: V3) -> Perch {
        let dist = |h: &Head| hypot(h.x - p.x, h.z - p.z);
        let mut best = 0;
        for k in 0..self.heads.len() {
            if dist(&self.heads[k]) < dist(&self.heads[best]) {
                best = k;
            }
        }
        Perch::Head(best)
    }

    fn fly_to(&mut self, i: usize, state: State, perch: Perch) {
        let at = self.perch_at(perch);
        let b = &mut self.flies[i];
        b.state = Some(state);
        b.perch = Some(perch);
        // Give up and pick again if it can't get there; long hops get longer.
        b.timer = 15.0 + 1.5 * hypot(at.x - b.pos.x, at.z - b.pos.z) / b.speed;
    }

    fn next_after_feeding(&mut self, i: usize) {
        let pos = self.flies[i].pos;
        let warm: Vec<usize> = self
            .rocks
            .iter()
            .copied()
            .filter(|&r| self.rock_at[r].0.distance(pos) < 5.0)
            .collect();
        #[derive(Clone, Copy, PartialEq)]
        enum Next {
            Bask,
            Near,
            Far,
            Wander,
        }
        let bask = if warm.is_empty() { 0.0 } else { self.flies[i].bask };
        let next = pick(&mut self.flies[i].rand, &[(Next::Bask, bask), (Next::Near, 0.5), (Next::Far, 0.3), (Next::Wander, 0.15)]);
        match next {
            Next::Bask => {
                let r = warm[floor(self.flies[i].rand.next() * warm.len() as f64) as usize];
                self.fly_to(i, State::ToBask, Perch::Rock(r));
            }
            Next::Wander => {
                let air = self.air_near(i);
                self.fly_to(i, State::Fly, air);
            }
            Next::Near => {
                let near = self.head_near(i, NEAR);
                self.fly_to(i, State::Fly, near);
            }
            Next::Far => {
                let far = self.head_by_area(i);
                self.fly_to(i, State::Fly, far);
            }
        }
    }

    /// One step of flight toward the perch; true on arrival.
    fn fly(&mut self, i: usize, dt: f64, wind: &Wind) -> bool {
        let perch = self.flies[i].perch.unwrap();
        let target = self.perch_point(perch, wind);
        let ignore = match perch {
            Perch::Rock(r) => Some(r),
            _ => None,
        };
        let b = &mut self.flies[i];
        let dist = b.pos.distance(target);
        // Cruise at its own height, drop to the target over the last stretch, and wander side to
        // side the whole way (the zig-zag that makes butterflies hard to catch).
        let mut aim = target;
        if dist > 0.8 {
            aim.y = target.y.max(height_at(b.pos.x, b.pos.z) + b.alt);
        }
        b.wander += (b.rand.next() - 0.5) * 8.0 * dt;
        let jitter = 0.6 * dist.min(1.0);
        aim.x += cos(b.wander) * jitter;
        aim.z += sin(b.wander) * jitter;
        flight::steer(b.pos, &mut b.vel, aim, b.speed, ACCEL, dt);
        flight::avoid(&self.obstacles, &mut b.pos, &mut b.vel, dt, 0.15, ignore);
        b.pos += b.vel * dt;
        b.timer -= dt;
        dist < 0.06
    }

    fn land(&mut self, i: usize) {
        let b = &mut self.flies[i];
        b.vel = V3::ZERO;
        b.heading.yaw_vel = 0.0;
        match b.state.unwrap() {
            State::ToRoost => b.state = Some(State::Roost),
            State::ToBask => {
                b.state = Some(State::Bask);
                b.timer = 4.0 + b.rand.next() * 6.0;
            }
            _ => {
                b.state = Some(State::Feed);
                b.timer = 2.0 + b.rand.next() * 6.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flowers::place_flowers;
    use crate::motion::wrap_angle;
    use crate::sky::{DAY_LENGTH, sky_state};
    use crate::world;

    fn butterflies() -> Butterflies {
        let layout = world::place(42);
        let flowers = place_flowers(&layout);
        Butterflies::new(&layout, &flowers)
    }

    // The web's test/scene.js `simulate`: a fixed fps, dt 0 on the first frame, the manual sky day.
    fn simulate(b: &mut Butterflies, seconds: f64, fps: f64, mut on_frame: impl FnMut(&Butterflies, f64)) {
        let mut i = 0;
        while i as f64 / fps < seconds {
            let t = i as f64 / fps;
            let dt = if i == 0 { 0.0 } else { 1.0 / fps };
            b.update(t, dt, sky_state(t, 0.0).daylight, &Wind::at(t));
            on_frame(b, dt);
            i += 1;
        }
    }

    #[test]
    fn butterflies_turn_at_most_6_rad_s_and_stay_sane_for_a_day() {
        for fps in [60.0, 10.0] {
            let mut b = butterflies();
            assert_eq!(b.poses().len(), 12);
            let mut prev: Vec<f64> = b.poses().iter().map(|p| p.yaw).collect();
            let (mut max_rate, mut nan, mut min_alt) = (0.0_f64, 0, f64::INFINITY);
            simulate(&mut b, DAY_LENGTH, fps, |b, dt| {
                for (k, p) in b.poses().iter().enumerate() {
                    if dt > 0.0 {
                        max_rate = max_rate.max(wrap_angle(p.yaw - prev[k]).abs() / dt);
                    }
                    prev[k] = p.yaw;
                    if !p.pos.is_finite() {
                        nan += 1;
                    }
                    min_alt = min_alt.min(p.pos.y - height_at(p.pos.x, p.pos.z));
                }
            });
            assert_eq!(nan, 0, "{fps} fps");
            assert!(max_rate <= 6.0 + 1e-6, "{fps} fps: max turn rate {max_rate:.2} rad/s");
            assert!(min_alt > 0.0, "{fps} fps: min altitude {min_alt}");
        }
    }

    #[test]
    fn by_day_they_visit_flowers_and_by_night_they_roost_on_one() {
        let mut b = butterflies();
        let (mut fed, mut flew) = (0, 0);
        simulate(&mut b, DAY_LENGTH, 30.0, |b, _| {
            for f in &b.flies {
                match f.state {
                    Some(State::Feed) => fed += 1,
                    Some(State::Fly) => flew += 1,
                    _ => {}
                }
            }
        });
        assert!(fed > 0 && flew > 0, "fed {fed}, flew {flew}");
        // Deep night (a third past sunset): every one sits folded on a flower's swaying head.
        let night = DAY_LENGTH * 0.75;
        assert!(sky_state(night, 0.0).daylight < 0.1);
        let mut b = butterflies();
        b.update(night, 0.0, sky_state(night, 0.0).daylight, &Wind::at(night));
        for (f, p) in b.flies.iter().zip(b.poses()) {
            assert_eq!(f.state, Some(State::Roost));
            assert_eq!(p.wing, FOLDED);
            let Some(Perch::Head(h)) = f.perch else { panic!("roosting off a flower") };
            assert!(p.pos.distance(b.perch_point(Perch::Head(h), &Wind::at(night))) < 1e-12);
        }
    }
}
