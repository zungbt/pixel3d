//! Flight helpers shared by every insect species, ported from the web's `src/insects.js`: the spheres
//! flyers keep out of, the part of the map that is on screen, steering, avoidance, and the grass's
//! wind for anything sitting on a flower.
use core::ops::{Add, AddAssign, Mul, Sub, SubAssign};

use libm::{hypot, sqrt};

use crate::world::{Layout, Rock, height_at};

const ROAM_R: f64 = 13.0; // the part of the map that is on screen

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl V3 {
    pub const ZERO: V3 = V3 { x: 0.0, y: 0.0, z: 0.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn length_sq(self) -> f64 {
        self.dot(self)
    }
    pub fn length(self) -> f64 {
        sqrt(self.length_sq())
    }
    pub fn distance(self, o: V3) -> f64 {
        (self - o).length()
    }
    /// three's `setLength`: a zero vector stays zero.
    pub fn with_length(self, l: f64) -> V3 {
        let len = self.length();
        if len == 0.0 { self } else { self * (l / len) }
    }
    /// three's `clampLength(min, max)`.
    pub fn clamp_length(self, min: f64, max: f64) -> V3 {
        let len = self.length();
        self * (len.clamp(min, max) / if len == 0.0 { 1.0 } else { len })
    }
    pub fn is_finite(self) -> bool {
        (self.x + self.y + self.z).is_finite()
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        V3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        V3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl SubAssign for V3 {
    fn sub_assign(&mut self, o: V3) {
        *self = *self - o;
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, k: f64) -> V3 {
        V3::new(self.x * k, self.y * k, self.z * k)
    }
}

/// A sphere flyers must stay out of. `rock`: its index in the layout, so a lander can ignore it.
pub struct Obstacle {
    pub c: V3,
    pub r: f64,
    pub rock: Option<usize>,
}

/// Trunk bases, tree crowns and rocks, in the web's order.
pub fn obstacles(layout: &Layout) -> Vec<Obstacle> {
    let mut out = Vec::with_capacity(layout.trees.len() * 2 + layout.rocks.len());
    for t in &layout.trees {
        out.push(Obstacle { c: V3::new(t.x, t.y + 0.3, t.z), r: 0.3, rock: None });
        out.push(Obstacle { c: V3::new(t.x, t.y + t.height + 0.3, t.z), r: t.canopy + 0.1, rock: None });
    }
    for (i, rock) in layout.rocks.iter().enumerate() {
        out.push(Obstacle { c: V3::new(rock.x, rock.y, rock.z), r: rock.r + 0.05, rock: Some(i) });
    }
    out
}

pub fn in_roam(x: f64, z: f64) -> bool {
    hypot(x, z) < ROAM_R
}

/// Where an insect sits on a rock: its squashed solid's top face.
pub fn rock_top(rock: &Rock) -> V3 {
    V3::new(rock.x, rock.y + rock.sy * 0.85, rock.z)
}

/// Accelerate toward target, slowing down on arrival.
pub fn steer(pos: V3, vel: &mut V3, target: V3, max_speed: f64, accel: f64, dt: f64) {
    let d = target - pos;
    let d = d.with_length(max_speed.min(d.length() * 2.0));
    *vel += (d - *vel).clamp_length(0.0, accel * dt);
}

/// Push away from nearby obstacles (except the rock `ignore`, e.g. the one being landed on),
/// hard-project out of any being flown into, and keep `min_alt` above the ground. Something taking
/// off from a rock top (inside its sphere) is just pushed out, without a jump.
pub fn avoid(obstacles: &[Obstacle], pos: &mut V3, vel: &mut V3, dt: f64, min_alt: f64, ignore: Option<usize>) {
    for o in obstacles {
        if ignore.is_some() && o.rock == ignore {
            continue;
        }
        let d = *pos - o.c;
        let dist = d.length();
        let margin = o.r + 0.3;
        if dist >= margin || dist == 0.0 {
            continue;
        }
        let d = d * (1.0 / dist);
        *vel += d * ((margin - dist) / margin * 12.0 * dt);
        if dist < o.r && vel.dot(d) < 0.0 {
            *pos = o.c + d * o.r;
        }
    }
    let floor = height_at(pos.x, pos.z) + min_alt;
    if pos.y < floor {
        pos.y = floor;
        if vel.y < 0.0 {
            vel.y = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world;

    #[test]
    fn steering_arrives_without_overshooting_the_speed() {
        let (mut pos, mut vel) = (V3::ZERO, V3::ZERO);
        let target = V3::new(3.0, 0.5, -2.0);
        let mut max_speed: f64 = 0.0;
        for _ in 0..600 {
            steer(pos, &mut vel, target, 0.8, 3.0, 1.0 / 60.0);
            pos += vel * (1.0 / 60.0);
            max_speed = max_speed.max(vel.length());
        }
        assert!(pos.distance(target) < 0.01, "{pos:?}");
        assert!(max_speed <= 0.8 + 1e-9, "{max_speed}");
        let mut still = V3::ZERO;
        steer(target, &mut still, target, 0.8, 3.0, 1.0 / 60.0);
        assert_eq!(still, V3::ZERO, "at the target: no NaN from a zero direction");
    }

    #[test]
    fn avoidance_keeps_flyers_out_of_rocks_and_off_the_ground() {
        let layout = world::place(42);
        let obs = obstacles(&layout);
        assert_eq!(obs.len(), layout.trees.len() * 2 + layout.rocks.len());
        let rock = obs.iter().find(|o| o.rock.is_some()).unwrap();
        // Flying straight into a rock: projected back onto its surface.
        let mut pos = rock.c + V3::new(rock.r * 0.5, 0.0, 0.0);
        let mut vel = V3::new(-1.0, 0.0, 0.0);
        avoid(&obs, &mut pos, &mut vel, 1.0 / 60.0, 0.15, None);
        assert!(pos.distance(rock.c) >= rock.r - 1e-9);
        // Ignored (landing on it): left alone.
        let inside = rock.c + V3::new(rock.r * 0.5, 0.0, 0.0);
        let (mut pos, mut vel) = (inside, V3::new(-1.0, 0.0, 0.0));
        avoid(&obs, &mut pos, &mut vel, 1.0 / 60.0, -10.0, rock.rock);
        assert_eq!((pos, vel), (inside, V3::new(-1.0, 0.0, 0.0)));
        // Below the floor: lifted, and no longer sinking.
        let mut pos = V3::new(0.0, -5.0, 0.0);
        let mut vel = V3::new(0.0, -1.0, 0.0);
        avoid(&[], &mut pos, &mut vel, 1.0 / 60.0, 0.15, None);
        assert!((pos.y - (height_at(0.0, 0.0) + 0.15)).abs() < 1e-12 && vel.y == 0.0);
    }
}
