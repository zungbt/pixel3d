//! Fireflies, ported from the web's `src/fireflies.js`. Dusk to midnight, in a few loose swarms: males
//! drift low around their swarm, each giving a short rising ("J") flash every few seconds; females
//! wait on grass tips and answer a nearby male about 2 s later, and he turns toward her. A few lone
//! males wander the whole meadow instead. By day all of them are hidden in the grass.
//! One shared stream (seed 99) in the web's `rand()` order.
use core::f64::consts::PI;

use libm::{hypot, sin};

use crate::flight::{self, Obstacle, V3};
use crate::rng::Mulberry32;
use crate::world::{Layout, height_at, path_edge, pond_edge, pond_point};

const MALES: usize = 28;
const FEMALES: usize = 10;
const LONERS: usize = 20; // males that drift across the meadow on their own, away from the swarms
pub const COUNT: usize = MALES + FEMALES + LONERS;
pub const SIZE: f64 = 0.075; // about one art pixel
pub const GLOW: u32 = 0xd8ff6a;
const SPEED: f64 = 0.15;
const FLASH: f64 = 1.0; // seconds lit, fading in and out
const PERIOD: f64 = 5.5; // seconds between a male's flashes
const ANSWER_DELAY: f64 = 2.0;
const ANSWER_DIST: f64 = 2.0;
const CLUSTERS: usize = 6;
const CLUSTER_R: f64 = 3.0; // half-width of the square each swarm keeps to
const LONER_STEP: f64 = 3.0; // half-width of the square a loner picks its next spot in, around where it is
const LONER_GAP: f64 = 4.0; // loners start at least this far apart, so they cover the meadow
const SEED: u32 = 99;

struct Fly {
    male: bool,
    home: Option<V3>,
    pos: V3,
    vel: V3,
    target: V3,
    start: f64, // active window, in sky phase
    end: f64,
    active: bool,
    next: f64, // when the next flash starts (male) or the answer is due (female; 0 = none)
    lit: f64,  // when the current flash started
    suitor: Option<usize>,
}

/// One firefly for the renderer: where it is and how bright (0-1; hidden at 0.1 or less).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: V3,
    pub glow: f64,
}

pub struct Fireflies {
    rand: Mulberry32,
    flies: Vec<Fly>,
    poses: Vec<Pose>,
    obstacles: Vec<Obstacle>,
}

/// Grass-tip height over open ground: in view, off the water and the path, clear of trees and
/// rocks. Like the web, `out` is written as soon as the spot is off the water and path.
fn is_open(obstacles: &[Obstacle], out: &mut V3, x: f64, z: f64) -> bool {
    if !flight::in_roam(x, z) || pond_edge(x, z) < 0.3 || path_edge(x, z) < 0.2 {
        return false;
    }
    *out = V3::new(x, height_at(x, z) + 0.25, z);
    !obstacles.iter().any(|o| o.c.distance(*out) < o.r + 0.1)
}

fn open_spot(rand: &mut Mulberry32, obstacles: &[Obstacle], out: &mut V3, near_x: f64, near_z: f64, radius: f64) -> bool {
    for _ in 0..10 {
        let x = near_x + (rand.next() * 2.0 - 1.0) * radius;
        let z = near_z + (rand.next() * 2.0 - 1.0) * radius;
        if is_open(obstacles, out, x, z) {
            return true;
        }
    }
    false
}

impl Fireflies {
    pub fn new(layout: &Layout) -> Self {
        let obstacles = flight::obstacles(layout);
        let mut rand = Mulberry32::new(SEED);

        // Where they gather: damp grass by the pond, and meadow along forest edges.
        let mut centres: Vec<V3> = Vec::new();
        let mut spot = V3::ZERO;
        for _ in 0..500 {
            if centres.len() >= CLUSTERS {
                break;
            }
            let (x, z) = if centres.is_empty() {
                pond_point(rand.next() * PI * 2.0, 1.5)
            } else {
                let x = (rand.next() * 2.0 - 1.0) * 11.0;
                (x, (rand.next() * 2.0 - 1.0) * 11.0)
            };
            if !is_open(&obstacles, &mut spot, x, z) || centres.iter().any(|c| c.distance(spot) < 6.0) {
                continue;
            }
            let tree = layout.trees.iter().map(|t| hypot(t.x - x, t.z - z)).fold(f64::INFINITY, f64::min);
            if !centres.is_empty() && !(1.5..=3.0).contains(&tree) {
                continue;
            }
            centres.push(spot);
        }

        let mut flies: Vec<Fly> = Vec::with_capacity(COUNT);
        for i in 0..COUNT {
            let loner = i >= MALES + FEMALES;
            let home = if loner { None } else { Some(centres[i % centres.len()]) };
            let mut pos = V3::ZERO;
            if let Some(h) = home {
                if !open_spot(&mut rand, &obstacles, &mut pos, h.x, h.z, CLUSTER_R) {
                    pos = h;
                }
            } else {
                // Spaced out from the other loners if there's room; after 100 tries, any open spot.
                for _ in 0..100 {
                    while !open_spot(&mut rand, &obstacles, &mut pos, 0.0, 0.0, 11.0) {}
                    if !flies.iter().any(|o| o.home.is_none() && o.pos.distance(pos) < LONER_GAP) {
                        break;
                    }
                }
            }
            let start = 0.48 + (rand.next() - 0.5) * 0.06;
            let end = 0.75 + (rand.next() - 0.5) * 0.06;
            flies.push(Fly { male: i < MALES || loner, home, pos, vel: V3::ZERO, target: pos, start, end, active: false, next: 0.0, lit: -1.0, suitor: None });
        }
        let poses = flies.iter().map(|f| Pose { pos: f.pos, glow: 0.0 }).collect();
        Self { rand, flies, poses, obstacles }
    }

    /// Swarm males and females first, then the loners (`COUNT - LONERS..`).
    pub fn poses(&self) -> &[Pose] {
        &self.poses
    }

    fn wander(&mut self, i: usize) {
        let f = &mut self.flies[i];
        let (x, z, r) = match f.home {
            Some(h) => (h.x, h.z, CLUSTER_R),
            None => (f.pos.x, f.pos.z, LONER_STEP),
        };
        if open_spot(&mut self.rand, &self.obstacles, &mut f.target, x, z, r) {
            f.target.y += self.rand.next() * 0.95; // 0.25-1.2 up
        }
    }

    /// One step at animation time `t` after `dt`, at this sky phase.
    pub fn update(&mut self, t: f64, dt: f64, phase: f64) {
        for i in 0..self.flies.len() {
            let f = &mut self.flies[i];
            f.active = phase > f.start && phase < f.end;
            if !f.active {
                f.next = 0.0;
                f.lit = -1.0;
                continue;
            }
            if !f.male {
                if f.next != 0.0 && t >= f.next {
                    f.lit = t;
                    f.next = 0.0;
                }
                continue;
            }

            if f.next == 0.0 {
                f.next = t + self.rand.next() * PERIOD; // first flash after waking
            }
            if t >= f.next {
                f.lit = t;
                f.next = t + PERIOD + (self.rand.next() - 0.5) * 1.5;
                // Females within range that aren't already answering someone reply after a pause.
                let from = f.pos;
                for g in MALES..MALES + FEMALES {
                    let g = &mut self.flies[g];
                    if g.active && g.next == 0.0 && g.pos.distance(from) < ANSWER_DIST {
                        g.next = t + ANSWER_DELAY + (self.rand.next() - 0.5) * 0.6;
                        g.suitor = Some(i);
                    }
                }
            }

            if self.flies[i].pos.distance(self.flies[i].target) < 0.2 {
                self.wander(i);
            }
            let f = &mut self.flies[i];
            flight::steer(f.pos, &mut f.vel, f.target, SPEED, 1.0, dt);
            if t - f.lit < FLASH {
                f.vel.y += 0.4 * dt; // the rising stroke of the J
            }
            flight::avoid(&self.obstacles, &mut f.pos, &mut f.vel, dt, 0.2, None);
            f.pos += f.vel * dt;
        }

        // A male that sees his answer heads for her.
        for g in MALES..MALES + FEMALES {
            let g = &self.flies[g];
            if let (true, Some(s)) = (g.lit == t, g.suitor) {
                let to = V3::new(g.pos.x, g.pos.y + 0.3, g.pos.z);
                self.flies[s].target = to;
            }
        }

        for (f, pose) in self.flies.iter().zip(&mut self.poses) {
            let u = (t - f.lit) / FLASH;
            pose.pos = f.pos;
            pose.glow = if f.active && f.lit >= 0.0 && u < 1.0 { sin(PI * u) } else { 0.0 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sky::sky_state;
    use crate::world;

    #[test]
    fn lone_fireflies_flash_out_across_the_meadow_away_from_the_pond_inside_the_roam_area() {
        let mut f = Fireflies::new(&world::place(42));
        assert_eq!(f.poses().len(), MALES + FEMALES + LONERS);
        let (mut flashes, mut far, mut nan, mut outside) = (0, 0, 0, 0);
        let fps = 60.0;
        let mut i = 0;
        while i as f64 / fps < 300.0 {
            let t = i as f64 / fps;
            f.update(t, if i == 0 { 0.0 } else { 1.0 / fps }, sky_state(t, 0.0).phase);
            for p in &f.poses()[COUNT - LONERS..] {
                if p.glow <= 0.1 {
                    continue; // hidden between flashes
                }
                let q = p.pos;
                if !q.is_finite() {
                    nan += 1;
                }
                if hypot(q.x, q.z) > 13.0 {
                    outside += 1;
                }
                flashes += 1;
                if pond_edge(q.x, q.z) > 6.0 {
                    far += 1;
                }
            }
            i += 1;
        }
        assert_eq!(nan, 0);
        assert_eq!(outside, 0, "a loner flashed outside the roam area");
        assert!(flashes > 0, "loners never flashed");
        assert!(far as f64 / flashes as f64 > 0.3, "only {far}/{flashes} loner flash frames far from the pond");
    }
}
