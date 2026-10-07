import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildScene, simulate } from './scene.js';
import { wrapAngle } from '../src/motion.js';
import { heightAt, pondEdge } from '../src/world.js';

for (const fps of [60, 10]) {
  test(`dragonflies pivot at most 10 rad/s and never fly sideways at ${fps} fps`, () => {
    const s = buildScene();
    const flies = s.scene.children.filter((c) => c.name === 'dragonfly');
    assert.equal(flies.length, 5); // 3 on the pond rim, then 2 roamers
    const prevYaw = flies.map((f) => f.rotation.y);
    const prevPos = flies.map((f) => f.position.clone());
    let maxRate = 0;
    let fast = 0;
    let sideways = 0;
    let nan = 0;
    let minAlt = Infinity;
    let roamerFrames = 0;
    let roamerFar = 0;
    let roamerInside = 0;
    const { obstacles } = s.insects.ctx;
    simulate(s, 300, (t, dt) => {
      flies.forEach((f, i) => {
        const p = f.position;
        if (!Number.isFinite(p.x + p.y + p.z)) nan++;
        if (i >= 3) {
          roamerFrames++;
          if (pondEdge(p.x, p.z) > 6) roamerFar++;
          if (obstacles.some((o) => o.c.distanceTo(p) < o.r - 0.01)) roamerInside++;
        }
        minAlt = Math.min(minAlt, p.y - heightAt(p.x, p.z));
        if (dt) {
          maxRate = Math.max(maxRate, Math.abs(wrapAngle(f.rotation.y - prevYaw[i])) / dt);
          const vx = (p.x - prevPos[i].x) / dt;
          const vz = (p.z - prevPos[i].z) / dt;
          if (Math.hypot(vx, vz) > 2) {
            fast++;
            if (Math.abs(wrapAngle(f.rotation.y - Math.atan2(vx, vz))) > 0.5) sideways++;
          }
        }
        prevYaw[i] = f.rotation.y;
        prevPos[i].copy(p);
      });
    }, fps);
    assert.equal(nan, 0);
    assert.ok(maxRate <= 10 + 1e-6, `max turn rate ${maxRate.toFixed(2)} rad/s`);
    assert.ok(fast > 0, 'no darts at all');
    assert.ok(sideways / fast < 0.02, `${sideways}/${fast} fast frames flying sideways`);
    assert.ok(minAlt > 0.1, `min altitude ${minAlt}`);
    assert.ok(roamerFar / roamerFrames > 0.3, `roamers far from the pond only ${roamerFar}/${roamerFrames} frames`);
    assert.equal(roamerInside, 0, 'a roamer inside a tree or rock');
  });
}
