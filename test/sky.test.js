import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { buildSky, clockTime } from '../src/sky.js';

test('clockTime puts the sky at the local clock time (sunrise 06:00, sunset 18:00)', () => {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color();
  const sky = buildSky(scene);
  for (const [h, m] of [[0, 0], [5, 59], [6, 0], [12, 0], [18, 0], [23, 30]]) {
    sky.update(1234, clockTime(new Date(2026, 9, 7, h, m)));
    const hour = (sky.state.phase * 24 + 6) % 24;
    const off = Math.abs(hour - (h + m / 60));
    assert.ok(Math.min(off, 24 - off) < 1e-6, `${h}:${m} showed ${hour}`); // 23.999... is midnight
  }
  sky.update(0, clockTime(new Date(2026, 9, 7, 12, 0)));
  assert.ok(sky.state.daylight > 0.99, 'noon is full daylight');
  sky.update(0, clockTime(new Date(2026, 9, 7, 0, 0)));
  assert.equal(sky.state.daylight, 0, 'midnight is dark');
});
