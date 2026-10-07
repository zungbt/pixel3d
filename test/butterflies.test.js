import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildScene, simulate } from './scene.js';
import { wrapAngle } from '../src/motion.js';
import { heightAt } from '../src/world.js';

for (const fps of [60, 10]) {
  test(`butterflies turn at most 6 rad/s and stay sane for a day at ${fps} fps`, () => {
    const s = buildScene();
    const flies = s.scene.children.filter((c) => c.name === 'butterfly');
    assert.equal(flies.length, 12);
    const prev = flies.map((f) => f.rotation.y);
    let maxRate = 0;
    let nan = 0;
    let minAlt = Infinity;
    simulate(s, 300, (t, dt) => {
      flies.forEach((f, i) => {
        if (dt) maxRate = Math.max(maxRate, Math.abs(wrapAngle(f.rotation.y - prev[i])) / dt);
        prev[i] = f.rotation.y;
        const p = f.position;
        if (!Number.isFinite(p.x + p.y + p.z)) nan++;
        minAlt = Math.min(minAlt, p.y - heightAt(p.x, p.z));
      });
    }, fps);
    assert.equal(nan, 0);
    assert.ok(maxRate <= 6 + 1e-6, `max turn rate ${maxRate.toFixed(2)} rad/s`);
    assert.ok(minAlt > 0, `min altitude ${minAlt}`);
  });
}
