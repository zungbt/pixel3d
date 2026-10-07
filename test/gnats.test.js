import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { buildScene, simulate } from './scene.js';
import { heightAt } from '../src/world.js';
import { DAY_LENGTH } from '../src/sky.js';

const PER_SWARM = 10;
const m = new THREE.Matrix4();
const p = new THREE.Vector3();

// Mean horizontal distance of swarm k's visible members from its centre.
function spreadOf(mesh, swarms, k) {
  const w = swarms[k];
  let sum = 0;
  for (let j = 0; j < w.visible; j++) {
    mesh.getMatrixAt(k * PER_SWARM + j, m);
    p.setFromMatrixPosition(m);
    sum += Math.hypot(p.x - w.centre.x, p.z - w.centre.z);
  }
  return sum / w.visible;
}

for (const fps of [60, 10]) {
  test(`gnats: dusk and dawn only, clear of everything, scattered by dragonflies and re-formed (${fps} fps)`, () => {
    const s = buildScene();
    const mesh = s.scene.getObjectByName('gnats');
    assert.ok(mesh, 'no gnats mesh');
    assert.equal(mesh.count, 20);
    const { swarms, obstacles, hunters } = s.insects.ctx;
    assert.equal(swarms.length, 2);
    const lastScare = swarms.map((w) => w.scaredAt);
    const checks = [];
    const spreads = [];
    let nan = 0;
    let outOfWindow = 0;
    let maxVisible = 0;
    let minAlt = Infinity;
    let inside = 0;
    let scatters = 0;
    let unsettledAtNight = 0;
    simulate(s, DAY_LENGTH, (t) => {
      let visible = 0;
      for (let i = 0; i < mesh.count; i++) {
        mesh.getMatrixAt(i, m);
        if (m.elements[0] === 0) continue; // decompose() can't see a zero scale in r186
        visible++;
        p.setFromMatrixPosition(m);
        if (!Number.isFinite(p.x + p.y + p.z)) nan++;
        minAlt = Math.min(minAlt, p.y - heightAt(p.x, p.z));
        if (obstacles.some((o) => o.c.distanceTo(p) < o.r - 0.01)) inside++;
      }
      if (visible && Math.abs(s.sky.state.elevation) >= 0.5) outOfWindow++;
      maxVisible = Math.max(maxVisible, visible);
      swarms.forEach((w, k) => {
        if (w.scaredAt === lastScare[k]) return;
        lastScare[k] = w.scaredAt;
        scatters++;
        for (const c of checks) if (c.k === k && !c.done) c.done = true; // re-scattered: that check restarts
        checks.push({ k, at: t + 4 });
      });
      for (const c of checks) {
        if (c.done || t < c.at) continue;
        c.done = true;
        if (swarms[c.k].visible) spreads.push(spreadOf(mesh, swarms, c.k));
      }
      // Midnight (phase 0.75): every dragonfly is down in the grass, hunts or not.
      if (Math.abs(s.sky.state.phase - 0.75) < 0.001) unsettledAtNight += hunters.filter((h) => h.state !== 'settle').length;
    }, fps);
    assert.equal(nan, 0);
    assert.equal(outOfWindow, 0, 'gnats seen outside dusk/dawn');
    assert.equal(maxVisible, 20);
    assert.ok(minAlt >= 0.45, `min altitude ${minAlt}`);
    assert.equal(inside, 0, 'gnats inside a tree or rock');
    assert.ok(scatters > 0, 'no dragonfly ever scattered a swarm');
    assert.ok(spreads.length > 0 && spreads.every((d) => d < 0.5), `spread 4 s after scatters: ${spreads.map((d) => d.toFixed(2))}`);
    assert.equal(unsettledAtNight, 0, 'a dragonfly was still flying at night');
  });
}

test('gnats: jumping the clock to dusk finds them already in formation', () => {
  const s = buildScene();
  const mesh = s.scene.getObjectByName('gnats');
  const { swarms } = s.insects.ctx;
  const noon = (0.25 - 0.08) * DAY_LENGTH; // sky seconds at phase 0.25 (DAY_START is 0.08)
  const dusk = (0.46 - 0.08) * DAY_LENGTH; // phase 0.46, about 17:00
  let checked = false;
  for (let i = 0; i < 20 * 60; i++) {
    const t = i / 60;
    s.sky.update(t < 10 ? noon + t : dusk + t); // what the time slider does
    s.insects.update(t, i ? 1 / 60 : 0, s.sky.state);
    if (t >= 10 && !checked) {
      checked = true;
      swarms.forEach((w, k) => {
        assert.equal(w.visible, PER_SWARM);
        assert.ok(spreadOf(mesh, swarms, k) < 0.5, `swarm ${k} spread ${spreadOf(mesh, swarms, k)}`);
      });
    }
  }
  assert.ok(checked);
});
