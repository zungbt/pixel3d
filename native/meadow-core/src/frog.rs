//! A frog on a mossy stone at the pond's edge. By day it puffs its throat, blinks, hops out to the
//! lily pads or plops into the water and swims back, and snaps its tongue at insects flying by (it
//! always misses). Round sunset it goes home to the stone and croaks (the throat sac balloons); at
//! night it sits there still, blinking slowly.
use core::f64::consts::PI;

use crate::flight::V3;
use crate::rng::Mulberry32;
use crate::world::{POND_X, POND_Z, pond_point, shore_y, smoothstep};

pub const SIZE: f64 = 0.3; // body length
pub const STONE_R: f64 = 0.45; // x/z radius
pub const STONE_H: f64 = 0.2; // its y scale: the top is this far above its centre
pub const PADS: usize = 3;
pub const PAD_R: f64 = 0.32;
const STONE_THETA: f64 = 2.2; // where round the pond (rad)
const REACH: f64 = 0.9; // the tongue's
const SNAP: f64 = 0.25; // s the tongue is out
const SNAP_REST: f64 = 3.0; // s between snaps
const SWIM_DEPTH: f64 = 0.5 * SIZE; // the body under, the eyes above the water
const SEED: u32 = 7;

/// The water's surface (the pond mesh sits 0.02 over the shore).
pub fn water_y() -> f64 {
    shore_y() + 0.02
}

/// The stone's centre; its top is STONE_H higher.
pub fn stone() -> V3 {
    let (x, z) = pond_point(STONE_THETA, -0.1);
    V3::new(x, shore_y(), z)
}

/// The lily pads' centres, out on the water within a hop of the stone.
pub fn pads() -> [V3; PADS] {
    let s = stone();
    let inward = (V3::new(POND_X, s.y, POND_Z) - s).with_length(1.0);
    let side = V3::new(-inward.z, 0.0, inward.x);
    [(0.95, -0.55), (1.35, 0.25), (0.8, 0.75)].map(|(along, across)| {
        let p = s + inward * along + side * across;
        V3::new(p.x, water_y() + 0.005, p.z)
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spot {
    Stone,
    Pad(usize),
    Water,
}

#[derive(Clone, Copy, Debug)]
enum Motion {
    Still,
    Hop { from: V3, to: V3, t: f64, dur: f64, dest: Spot },
    Swim { from: V3, to: V3, t: f64, dur: f64 },
}

/// The frog this frame, for the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub pos: V3,  // between its feet
    pub yaw: f64, // facing (sin yaw, 0, cos yaw)
    pub stretch: f64, // legs out, 0-1 (mid hop)
    pub puff: f64,    // throat sac, 0-1
    pub blink: bool,
    pub tongue: Option<(V3, f64)>, // aimed at, out 0-1
}

pub struct Frog {
    rand: Mulberry32,
    spot: Spot,
    pos: V3,
    yaw: f64,
    motion: Motion,
    wait: f64,  // s until the next move
    t: f64,     // own clock, s
    blink_at: f64,
    tongue: Option<(V3, f64)>, // target, s out
    snap_rest: f64,
}

impl Frog {
    pub fn new() -> Self {
        let mut rand = Mulberry32::new(SEED);
        let wait = 6.0 + rand.next() * 8.0;
        Self {
            rand,
            spot: Spot::Stone,
            pos: seat(Spot::Stone),
            yaw: PI / 4.0, // toward the camera
            motion: Motion::Still,
            wait,
            t: 0.0,
            blink_at: 2.0,
            tongue: None,
            snap_rest: 0.0,
        }
    }

    pub fn spot(&self) -> Spot {
        self.spot
    }

    /// One step: `dt` s, the sky's `daylight` (0-1) and `phase` (0.5 sunset), and the insects
    /// flying about.
    pub fn update(&mut self, dt: f64, daylight: f64, phase: f64, insects: impl Iterator<Item = V3>) {
        self.t += dt;
        let active = daylight > 0.5 && !croaking(phase);
        self.snap_rest = (self.snap_rest - dt).max(0.0);
        if let Some((_, out)) = &mut self.tongue {
            *out += dt;
            if *out >= SNAP {
                self.tongue = None;
                self.snap_rest = SNAP_REST;
            }
        }
        match self.motion {
            Motion::Still => {
                self.wait -= dt;
                if !active && self.spot != Spot::Stone {
                    self.wait = self.wait.min(0.5); // home before dark
                }
                if self.wait <= 0.0 && self.tongue.is_none() && (active || self.spot != Spot::Stone) {
                    self.next_move(active);
                } else if active && self.spot != Spot::Water && self.tongue.is_none() && self.snap_rest == 0.0 {
                    let mouth = self.pos + V3::new(0.0, 0.4 * SIZE, 0.0);
                    if let Some(target) = insects.into_iter().find(|p| p.y > mouth.y && p.distance(mouth) < REACH) {
                        self.yaw = (target.x - self.pos.x).atan2(target.z - self.pos.z);
                        self.tongue = Some((target, 0.0));
                    }
                }
            }
            Motion::Hop { from, to, t, dur, dest } => {
                let t = t + dt;
                if t >= dur {
                    self.arrive(dest, to);
                } else {
                    let s = t / dur;
                    self.pos = from + (to - from) * s + V3::new(0.0, 0.35 * 4.0 * s * (1.0 - s), 0.0);
                    self.motion = Motion::Hop { from, to, t, dur, dest };
                }
            }
            Motion::Swim { from, to, t, dur } => {
                let t = t + dt;
                if t >= dur {
                    self.pos = to;
                    self.hop(Spot::Stone);
                } else {
                    self.pos = from + (to - from) * smoothstep(t, 0.0, dur);
                    self.motion = Motion::Swim { from, to, t, dur };
                }
            }
        }
    }

    // From sitting: out to a pad or the water by day, home to the stone otherwise.
    fn next_move(&mut self, active: bool) {
        let r = self.rand.next();
        let pad = |r: f64| Spot::Pad(((r * 7.0) as usize) % PADS);
        let dest = match self.spot {
            _ if !active => Spot::Stone,
            Spot::Stone if r < 0.6 => pad(r),
            Spot::Stone => Spot::Water,
            Spot::Pad(i) if r < 0.4 => match pad(r) {
                Spot::Pad(j) if j == i => Spot::Pad((i + 1) % PADS),
                other => other,
            },
            Spot::Pad(_) => Spot::Stone,
            Spot::Water => Spot::Stone,
        };
        if self.spot == Spot::Water {
            // Swim to the foot of the stone, then climb up.
            let s = stone();
            let to = s + (self.pos - s).with_length(STONE_R + 0.15);
            let to = V3::new(to.x, self.pos.y, to.z);
            self.yaw = (to.x - self.pos.x).atan2(to.z - self.pos.z);
            self.motion = Motion::Swim { from: self.pos, to, t: 0.0, dur: 1.0 + self.pos.distance(to) * 2.5 };
        } else {
            self.hop(dest);
        }
    }

    fn hop(&mut self, dest: Spot) {
        let to = match dest {
            Spot::Water => {
                // Off the stone's far side, between the pads.
                let s = stone();
                let out = s + (V3::new(POND_X, s.y, POND_Z) - s).with_length(0.9) + V3::new(0.0, 0.0, 0.3 * (self.rand.next() - 0.5));
                V3::new(out.x, water_y() - SWIM_DEPTH, out.z)
            }
            _ => seat(dest),
        };
        self.yaw = (to.x - self.pos.x).atan2(to.z - self.pos.z);
        let dur = 0.45 + 0.15 * self.pos.distance(to);
        self.motion = Motion::Hop { from: self.pos, to, t: 0.0, dur, dest };
    }

    fn arrive(&mut self, dest: Spot, at: V3) {
        self.spot = dest;
        self.pos = at;
        self.motion = Motion::Still;
        self.wait = match dest {
            Spot::Water => 1.5 + self.rand.next() * 2.0,
            _ => 10.0 + self.rand.next() * 15.0,
        };
        if dest == Spot::Stone {
            self.yaw = PI / 4.0 + (self.rand.next() - 0.5) * 1.2; // round to face out again
        }
    }

    /// The frog this frame: `phase` as for update.
    pub fn pose(&self, daylight: f64, phase: f64) -> Pose {
        let stretch = match self.motion {
            Motion::Hop { t, dur, .. } => (PI * t / dur).sin(),
            _ => 0.0,
        };
        let puff = if croaking(phase) && self.motion_still() {
            // A croak every 3 s: the sac balloons and empties.
            let c = self.t.rem_euclid(3.0);
            if c < 0.8 { (PI * c / 0.8).sin() } else { 0.0 }
        } else {
            let rate = if daylight > 0.5 { 5.0 } else { 2.0 };
            0.12 * (0.5 + 0.5 * (self.t * rate).sin())
        };
        let gap = if daylight > 0.5 { 4.0 } else { 7.0 };
        let blink = (self.t + self.blink_at).rem_euclid(gap) < 0.15;
        Pose { pos: self.pos, yaw: self.yaw, stretch, puff, blink, tongue: self.tongue.map(|(at, out)| (at, (PI * out / SNAP).sin())) }
    }

    fn motion_still(&self) -> bool {
        matches!(self.motion, Motion::Still)
    }
}

impl Default for Frog {
    fn default() -> Self {
        Self::new()
    }
}

// Round sunset: home on the stone, croaking.
fn croaking(phase: f64) -> bool {
    (0.46..0.6).contains(&phase)
}

// Where the frog sits at `spot`.
fn seat(spot: Spot) -> V3 {
    match spot {
        Spot::Stone => {
            let s = stone();
            V3::new(s.x, s.y + STONE_H * 0.8, s.z)
        }
        Spot::Pad(i) => pads()[i],
        Spot::Water => {
            let s = stone();
            V3::new(s.x, water_y() - SWIM_DEPTH, s.z)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{pond_edge, sand_q};

    const DAY: (f64, f64) = (1.0, 0.25);
    const DUSK: (f64, f64) = (0.3, 0.52);
    const NIGHT: (f64, f64) = (0.0, 0.75);

    fn run(f: &mut Frog, (daylight, phase): (f64, f64), seconds: f64, insects: &[V3], mut each: impl FnMut(&Frog)) {
        for _ in 0..(seconds * 15.0) as usize {
            f.update(1.0 / 15.0, daylight, phase, insects.iter().copied());
            each(f);
        }
    }

    #[test]
    fn the_stone_is_at_the_shore_and_the_pads_on_the_water_within_a_hop() {
        let s = stone();
        assert!(pond_edge(s.x, s.z).abs() < STONE_R, "the stone straddles the shoreline");
        assert!(sand_q(s.x, s.z) < 1.0);
        let pads = pads();
        for (i, p) in pads.iter().enumerate() {
            assert!(pond_edge(p.x, p.z) < -PAD_R, "pad {i} sticks out of the water");
            let d = (p.x - s.x).hypot(p.z - s.z);
            assert!(d > STONE_R + PAD_R && d < 1.8, "pad {i} {d} from the stone");
            assert!(pads[..i].iter().all(|q| (q.x - p.x).hypot(q.z - p.z) > 2.0 * PAD_R), "pads overlap");
        }
    }

    #[test]
    fn by_day_it_hops_about_and_only_ever_sits_on_the_stone_a_pad_or_in_the_pond() {
        let mut f = Frog::new();
        let mut spots = Vec::new();
        run(&mut f, DAY, 1800.0, &[], |f| {
            assert!(f.pos.is_finite());
            assert!(pond_edge(f.pos.x, f.pos.z) < STONE_R + 0.05, "wandered off: {:?}", f.pos);
            if f.motion_still() {
                assert_eq!(f.pos, if f.spot == Spot::Water { f.pos } else { seat(f.spot) });
                if f.spot == Spot::Water {
                    assert!(pond_edge(f.pos.x, f.pos.z) < 0.0 && (f.pos.y - (water_y() - SWIM_DEPTH)).abs() < 1e-9);
                }
                if spots.last() != Some(&f.spot) {
                    spots.push(f.spot);
                }
            }
        });
        assert!(spots.len() > 40, "{} moves in half an hour", spots.len());
        assert!(spots.contains(&Spot::Water) && (0..PADS).all(|i| spots.contains(&Spot::Pad(i))), "{spots:?}");
        assert!(spots.windows(2).all(|w| w[0] != Spot::Water || w[1] == Spot::Stone), "out of the water onto the stone");
    }

    #[test]
    fn at_dusk_it_goes_home_and_croaks_and_at_night_it_sits_still() {
        let mut f = Frog::new();
        run(&mut f, DAY, 1800.0, &[], |_| {});
        let mut f2 = Frog::new();
        // Wherever it is, it's home within seconds.
        let mut home_at = None;
        let mut t = 0.0;
        run(&mut f2, DAY, 9.0, &[], |_| {});
        run(&mut f, DUSK, 20.0, &[], |f| {
            t += 1.0 / 15.0;
            if home_at.is_none() && f.spot == Spot::Stone && f.motion_still() {
                home_at = Some(t);
            }
        });
        assert!(home_at.is_some_and(|t| t < 12.0), "{home_at:?}");
        let mut max_puff: f64 = 0.0;
        run(&mut f, DUSK, 6.0, &[], |f| max_puff = max_puff.max(f.pose(DUSK.0, DUSK.1).puff));
        assert!(max_puff > 0.9, "croaks: {max_puff}");
        let at = f.pos;
        run(&mut f, NIGHT, 1800.0, &[], |f| {
            assert_eq!(f.pos, at, "moved at night");
            assert!(f.pose(NIGHT.0, NIGHT.1).puff < 0.2);
        });
        let mut day_puff: f64 = 0.0;
        run(&mut f, DAY, 5.0, &[], |f| day_puff = day_puff.max(f.pose(DAY.0, DAY.1).puff));
        assert!(day_puff < 0.2, "only breathing by day: {day_puff}");
    }

    #[test]
    fn it_snaps_at_insects_in_reach_above_it_by_day_only() {
        let near = seat(Spot::Stone) + V3::new(0.3, 0.5, 0.2);
        let far = seat(Spot::Stone) + V3::new(2.0, 0.5, 0.0);
        let below = seat(Spot::Stone) + V3::new(0.3, -0.3, 0.0);
        let mut f = Frog::new();
        run(&mut f, DAY, 1.0 / 15.0, &[near], |_| {});
        let tongue = f.pose(DAY.0, DAY.1).tongue;
        assert!(tongue.is_some_and(|(at, _)| at == near), "{tongue:?}");
        let mut snaps = 0;
        let mut was = true;
        run(&mut f, DAY, 5.0, &[near], |f| {
            let out = f.pose(DAY.0, DAY.1).tongue.is_some();
            snaps += (out && !was) as u32;
            was = out;
        });
        assert!((1..=2).contains(&snaps), "rests between snaps: {snaps}");
        for (insects, sky) in [(far, DAY), (below, DAY), (near, NIGHT)] {
            let mut f = Frog::new();
            run(&mut f, sky, 5.0, &[insects], |f| assert!(f.pose(sky.0, sky.1).tongue.is_none(), "snapped at {insects:?}"));
        }
    }

    #[test]
    fn the_same_every_time() {
        let (mut a, mut b) = (Frog::new(), Frog::new());
        run(&mut a, DAY, 300.0, &[], |_| {});
        run(&mut b, DAY, 300.0, &[], |_| {});
        assert_eq!(a.pose(1.0, 0.25), b.pose(1.0, 0.25));
    }
}
