import * as THREE from 'three';
import { heightAt, mulberry32, pondPoint } from './world.js';
import { ease, pick, rngFor, turn } from './motion.js';

const SWARMS = 2;
const PER_SWARM = 10;
const SIZE = 0.067; // one art pixel
const COLOR = new THREE.Color(0x4a4c3c); // in full light; dimmed with the sky so the specks don't glow at dusk
const RIM = 0.25; // swarm markers sit just off the shoreline
const COLUMN_Y = 1.3; // column centre above the ground
const COLUMN_R = 0.45; // horizontal radius of the column
const SPREAD_Y = 0.5; // each gnat keeps its own height within ±this of the centre
const STATES = {
  circle: { speed: 0.6, k: 3, min: 1, max: 3 },
  zip: { speed: 1.4, k: 10, min: 0.2, max: 0.5 },
  hang: { speed: 0.15, k: 5, min: 0.3, max: 1.2 },
};
const NEXT = { circle: 0.5, zip: 0.3, hang: 0.2 };
const SCARE_R = 0.6; // a darting dragonfly this close to the centre scatters the swarm
const COOLDOWN = 3;
const FLEE_SPEED = 1.2;

// Dusk and dawn: a few columns of midges dance over the pond rim (a mating swarm over a
// landmark). Each gnat circles, zips across or hangs, keeping clear of its neighbours and
// inside the column. A dragonfly darting through scatters them, the nearest first, and they
// drift back into the column.
export function buildGnats(scene, ctx) {
  const rand = mulberry32(31);

  const swarms = [];
  for (let i = 0; i < 200 && swarms.length < SWARMS; i++) {
    const angle = rand() * Math.PI * 2;
    const [x, z] = pondPoint(angle, RIM);
    const home = new THREE.Vector3(x, heightAt(x, z) + COLUMN_Y, z);
    if (swarms.some((s) => s.home.distanceTo(home) < 2.5)) continue;
    // the column centre drifts up to ~0.35 off home, and the bobs add ~0.4 to the vertical spread
    const blocked = ctx.obstacles.some(
      (o) => Math.hypot(o.c.x - x, o.c.z - z) < o.r + COLUMN_R + 0.35 && Math.abs(o.c.y - home.y) < o.r + SPREAD_Y + 0.4,
    );
    if (blocked) continue;
    swarms.push({ angle, home, centre: home.clone(), visible: 0, scaredAt: -Infinity, seed: rand() * 100 });
  }
  ctx.swarms.push(...swarms);

  const gnats = [];
  swarms.forEach((swarm, s) => {
    for (let k = 0; k < PER_SWARM; k++) {
      const r = rngFor(31, s * PER_SWARM + k);
      const a = r() * Math.PI * 2;
      const d = r() * COLUMN_R;
      gnats.push({
        swarm, k, rand: r,
        pos: new THREE.Vector3(swarm.home.x + Math.cos(a) * d, swarm.home.y, swarm.home.z + Math.sin(a) * d),
        vel: new THREE.Vector3(),
        yaw: r() * Math.PI * 2 - Math.PI,
        yawVel: 0,
        speed: 0,
        state: 'circle',
        timer: r() * 3, // out of step from the start
        spin: r() < 0.5 ? 1 : -1, // which way it circles
        height: (r() * 2 - 1) * SPREAD_Y,
        bob: 1.5 + r() * 2, // up-and-down rate, rad/s
        phase: r() * Math.PI * 2,
        pace: 0.8 + r() * 0.4,
        reactivity: 0.35 + r() * 0.65,
        fleeAt: Infinity, // when it notices the dragonfly
        fleeUntil: 0,
        from: new THREE.Vector3(), // where the scare came from
      });
    }
  });

  // Transparent-free, unlit specks, darkened in update as the light fades.
  const mesh = new THREE.InstancedMesh(
    new THREE.BoxGeometry(SIZE, SIZE, SIZE),
    new THREE.MeshBasicMaterial(),
    gnats.length,
  );
  mesh.name = 'gnats';
  mesh.frustumCulled = false; // instances move, and start hidden at scale 0
  scene.add(mesh);
  const matrix = new THREE.Matrix4();

  function scatter(swarm, from, t) {
    swarm.scaredAt = t;
    for (const g of gnats) {
      if (g.swarm !== swarm) continue;
      // The nearest react first; the far side and the sluggish ones a moment later.
      const dist = Math.min(g.pos.distanceTo(from) / SCARE_R, 2);
      g.fleeAt = t + 0.03 + Math.pow(dist, 1.5) * 0.35 + g.rand() * 0.12 + (1 - g.reactivity) * 0.1;
      g.from.copy(from);
    }
  }

  // Flocking within the swarm: separation, alignment, a pull to the centre, a hard edge,
  // circling (in the circle state) and a slow wander. Returns the heading to aim for.
  function heading(g, t) {
    const c = g.swarm.centre;
    let fx = Math.sin(g.yaw); // keep going roughly the same way
    let fz = Math.cos(g.yaw);
    let ax = 0;
    let az = 0;
    let n = 0;
    for (const o of gnats) {
      if (o === g || o.swarm !== g.swarm) continue;
      const dx = g.pos.x - o.pos.x;
      const dz = g.pos.z - o.pos.z;
      const d = Math.hypot(dx, dz);
      if (d > 0.3 || d === 0) continue;
      if (d < 0.08) (fx += (dx / d) * 2.8), (fz += (dz / d) * 2.8);
      ax += Math.sin(o.yaw);
      az += Math.cos(o.yaw);
      n++;
    }
    if (n) (fx += (ax / n) * 0.42), (fz += (az / n) * 0.42);
    const rx = c.x - g.pos.x;
    const rz = c.z - g.pos.z;
    const r = Math.hypot(rx, rz) || 1;
    const pull = 0.6 * (r / COLUMN_R) + 4.8 * (Math.max(0, r - COLUMN_R) / 0.15);
    fx += (rx / r) * pull;
    fz += (rz / r) * pull;
    if (g.state === 'circle') (fx += (rz / r) * g.spin), (fz -= (rx / r) * g.spin);
    const wander = Math.sin(t * 2.9 + g.phase) * 0.7 + Math.sin(t * 1.13 + g.phase * 1.73) * 0.45;
    return Math.atan2(fx, fz) + wander * 0.6;
  }

  return {
    update(t, dt, sky) {
      const act = 1 - THREE.MathUtils.smoothstep(Math.abs(sky.elevation), 0.4, 0.5);
      // Unlit, so follow the hemisphere light's intensity curve (sky.js) to stay as dark as the scene.
      mesh.material.color.copy(COLOR).multiplyScalar(THREE.MathUtils.lerp(0.2, 1.2, sky.daylight) / 1.2);
      for (const swarm of swarms) {
        // The column drifts slowly about its marker and bobs.
        const drift = (f) =>
          (Math.sin(t * 0.29 + swarm.seed * f) * 0.7 + Math.sin(t * 0.113 + swarm.seed * f * 1.73) * 0.45) * 0.3;
        swarm.centre.set(swarm.home.x + drift(1), swarm.home.y + Math.sin(t * 0.4 + swarm.seed) * 0.2, swarm.home.z + drift(2));
        swarm.visible = Math.round(PER_SWARM * act);
        if (!swarm.visible || t - swarm.scaredAt < COOLDOWN) continue;
        const hunter = ctx.hunters.find((h) => h.state === 'dart' && h.pos.distanceTo(swarm.centre) < SCARE_R);
        if (hunter) scatter(swarm, hunter.pos, t);
      }

      // Always simulated, even when hidden, so a swarm fades in already in formation.
      for (const g of gnats) {
        if (t >= g.fleeAt) (g.fleeAt = Infinity), (g.fleeUntil = t + 0.5 + g.rand() * 0.5);
        let aim;
        let speed;
        let k;
        if (t < g.fleeUntil) {
          aim = Math.atan2(g.pos.x - g.from.x, g.pos.z - g.from.z);
          speed = FLEE_SPEED;
          k = 12;
        } else {
          if ((g.timer -= dt) <= 0) {
            g.state = pick(g.rand, NEXT);
            const st = STATES[g.state];
            g.timer = st.min + g.rand() * (st.max - st.min);
          }
          aim = heading(g, t);
          speed = STATES[g.state].speed * g.pace;
          k = STATES[g.state].k;
        }
        turn(g, aim, dt, 40, 8, 12);
        g.speed = ease(g.speed, speed, k, dt);
        g.vel.set(Math.sin(g.yaw) * g.speed, 0, Math.cos(g.yaw) * g.speed);
        g.pos.addScaledVector(g.vel, dt);
        g.pos.y = g.swarm.centre.y + g.height + Math.sin(t * g.bob + g.phase) * 0.2;
        ctx.avoid(g.pos, g.vel, dt, 0.5, null); // placement keeps the columns clear of obstacles; this is only a backstop
      }

      gnats.forEach((g, i) => {
        const s = g.k < g.swarm.visible ? 1 : 0;
        matrix.makeScale(s, s, s).setPosition(g.pos);
        mesh.setMatrixAt(i, matrix);
      });
      mesh.instanceMatrix.needsUpdate = true;
    },
  };
}
