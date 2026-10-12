//! Small motion helpers shared by the insects, ported from the web's `src/motion.js`: per-individual
//! randomness, frame-rate independent easing, a damped turning spring and weighted choice.
use core::f64::consts::PI;

use libm::exp;

use crate::rng::Mulberry32;

/// Individual i's own random stream, so adding or removing one insect doesn't reshuffle the rest.
pub fn rng_for(seed: u32, i: u32) -> Mulberry32 {
    Mulberry32::new(seed ^ (i + 1).wrapping_mul(0x85eb_ca6b))
}

/// Moves cur toward target at rate k (per second); the same path at any frame rate.
pub fn ease(cur: f64, target: f64, k: f64, dt: f64) -> f64 {
    cur + (target - cur) * (1.0 - exp(-k * dt))
}

/// Into (-PI, PI].
pub fn wrap_angle(a: f64) -> f64 {
    let mut a = (a + PI) % (PI * 2.0);
    if a <= 0.0 {
        a += PI * 2.0;
    }
    a - PI
}

/// A heading on a spring: `yaw` and its rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Heading {
    pub yaw: f64,
    pub yaw_vel: f64,
}

/// Swings the heading toward `target` on a damped spring, its speed capped at `max_rate` (rad/s),
/// so it never snaps round. Stable for dt up to the loop's 0.1 s clamp.
pub fn turn(o: &mut Heading, target: f64, dt: f64, strength: f64, damping: f64, max_rate: f64) {
    let err = wrap_angle(target - o.yaw);
    o.yaw_vel += (err * strength - o.yaw_vel * damping) * dt;
    o.yaw_vel = o.yaw_vel.clamp(-max_rate, max_rate);
    o.yaw = wrap_angle(o.yaw + o.yaw_vel * dt);
}

/// One key of `(key, weight)`, chosen in proportion to its weight (zero never). One `rand()` call.
pub fn pick<K: Copy>(rand: &mut Mulberry32, weights: &[(K, f64)]) -> K {
    let total: f64 = weights.iter().map(|&(_, w)| w.max(0.0)).sum();
    let mut r = rand.next() * total;
    let mut last = None;
    for &(k, w) in weights {
        if w <= 0.0 {
            continue;
        }
        last = Some(k);
        r -= w;
        if r < 0.0 {
            return k;
        }
    }
    last.expect("pick: at least one positive weight")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_for_gives_each_individual_the_webs_stream() {
        let mut r = rng_for(2024, 0);
        assert_eq!([r.next(), r.next(), r.next()], [0.8391879298724234, 0.48736495221965015, 0.20849094609729946]);
        assert_eq!(rng_for(7, 3).next(), 0.6777639728970826);
        let firsts: Vec<f64> = (0..4).map(|i| rng_for(7, i).next()).collect();
        for (i, a) in firsts.iter().enumerate() {
            assert!((0.0..1.0).contains(a));
            assert!(firsts[i + 1..].iter().all(|b| b != a), "neighbours get different streams");
        }
    }

    #[test]
    fn ease_takes_the_same_path_at_any_step() {
        let once = ease(0.0, 1.0, 3.0, 0.1);
        let twice = ease(ease(0.0, 1.0, 3.0, 0.05), 1.0, 3.0, 0.05);
        assert!((once - twice).abs() < 1e-12);
        assert_eq!(ease(0.4, 1.0, 3.0, 0.0), 0.4);
    }

    #[test]
    fn wrap_angle_maps_into_minus_pi_pi_and_keeps_the_direction() {
        for (a, want) in [(0.0, 0.0), (PI, PI), (-PI, PI), (1.5 * PI, -0.5 * PI), (-1.5 * PI, 0.5 * PI), (3.5 * PI, -0.5 * PI)] {
            assert!((wrap_angle(a) - want).abs() < 1e-9, "{a} -> {}", wrap_angle(a));
        }
        let mut rand = rng_for(3, 0);
        for _ in 0..1000 {
            let a = (rand.next() - 0.5) * 100.0;
            let w = wrap_angle(a);
            assert!(w > -PI - 1e-12 && w <= PI + 1e-12);
            assert!((w.sin() - a.sin()).abs() < 1e-9 && (w.cos() - a.cos()).abs() < 1e-9);
        }
    }

    #[test]
    fn turn_settles_within_the_rate_cap_even_at_dt_0_1() {
        for dt in [1.0 / 60.0, 0.05, 0.1] {
            let mut o = Heading::default();
            let mut max_rate: f64 = 0.0;
            let mut t = 0.0;
            while t < 3.0 {
                turn(&mut o, 2.0, dt, 60.0, 14.0, 10.0);
                max_rate = max_rate.max(o.yaw_vel.abs());
                t += dt;
            }
            assert!((o.yaw - 2.0).abs() < 0.01, "dt {dt}: yaw {}", o.yaw);
            assert!(max_rate <= 10.0, "dt {dt}: rate {max_rate}");
        }
        let mut o = Heading { yaw: 3.0, yaw_vel: 0.0 };
        turn(&mut o, -3.0, 1.0 / 60.0, 30.0, 10.0, 6.0);
        assert!(o.yaw_vel > 0.0, "the short way across ±PI");
    }

    #[test]
    fn pick_never_picks_a_zero_weight_otherwise_in_proportion() {
        let mut rand = rng_for(1, 0);
        let mut n = [0; 3];
        for _ in 0..4000 {
            n[pick(&mut rand, &[(0, 1.0), (1, 3.0), (2, 0.0)])] += 1;
        }
        assert_eq!(n[2], 0);
        assert!((n[1] as f64 / 4000.0 - 0.75).abs() < 0.03, "b share {}", n[1] as f64 / 4000.0);
    }
}
