import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { buildScene, simulate } from './scene.js';
import { pondEdge } from '../src/world.js';

const IN_SWARMS = 38; // instances before the loners
const LONERS = 20;
const m = new THREE.Matrix4();
const p = new THREE.Vector3();

test('lone fireflies flash out across the meadow, away from the pond, inside the roam area', () => {
  const s = buildScene();
  const mesh = s.scene.getObjectByName('fireflies');
  assert.equal(mesh.count, IN_SWARMS + LONERS);
  let flashes = 0;
  let farFlashes = 0;
  let nan = 0;
  let outside = 0;
  simulate(s, 300, () => {
    for (let i = IN_SWARMS; i < mesh.count; i++) {
      mesh.getMatrixAt(i, m);
      if (m.elements[0] === 0) continue; // hidden between flashes
      p.setFromMatrixPosition(m);
      if (!Number.isFinite(p.x + p.y + p.z)) nan++;
      if (Math.hypot(p.x, p.z) > 13) outside++;
      flashes++;
      if (pondEdge(p.x, p.z) > 6) farFlashes++;
    }
  });
  assert.equal(nan, 0);
  assert.equal(outside, 0, 'a loner flashed outside the roam area');
  assert.ok(flashes > 0, 'loners never flashed');
  assert.ok(farFlashes / flashes > 0.3, `only ${farFlashes}/${flashes} loner flash frames far from the pond`);
});
