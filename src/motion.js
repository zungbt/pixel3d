import { mulberry32 } from './world.js';

// Small motion helpers shared by the insects (ideas from the nagomi koi pond, written afresh):
// per-individual randomness, frame-rate independent easing, and a damped turning spring.

// Individual i's own random stream, so adding or removing one insect doesn't reshuffle the rest.
export function rngFor(seed, i) {
  return mulberry32((seed ^ Math.imul(i + 1, 0x85ebca6b)) >>> 0);
}

// Moves cur toward target at rate k (per second); the same path at any frame rate.
export function ease(cur, target, k, dt) {
  return cur + (target - cur) * (1 - Math.exp(-k * dt));
}

// Into (-PI, PI].
export function wrapAngle(a) {
  a = (a + Math.PI) % (Math.PI * 2);
  if (a <= 0) a += Math.PI * 2;
  return a - Math.PI;
}

// Swings o.yaw toward targetYaw on a damped spring, its speed capped at maxRate (rad/s),
// so a heading never snaps round. Stable for dt up to the loop's 0.1 s clamp.
export function turn(o, targetYaw, dt, strength, damping, maxRate) {
  const err = wrapAngle(targetYaw - o.yaw);
  o.yawVel += (err * strength - o.yawVel * damping) * dt;
  o.yawVel = Math.max(-maxRate, Math.min(maxRate, o.yawVel));
  o.yaw = wrapAngle(o.yaw + o.yawVel * dt);
}

// One key of { name: weight }, chosen in proportion to its weight (zero never).
export function pick(rand, weights) {
  let total = 0;
  for (const k in weights) total += Math.max(0, weights[k]);
  let r = rand() * total;
  let last;
  for (const k in weights) {
    if (weights[k] <= 0) continue;
    last = k;
    if ((r -= weights[k]) < 0) return k;
  }
  return last;
}
