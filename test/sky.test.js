import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { buildSky, clockTime, fogAmount, hourToPhase, phaseToHour } from '../src/sky.js';
import { placeFor, sunTimes, winterness } from '../src/sun.js';

const EQUINOX = { rise: 6, set: 18 };
const WINTER = { rise: 6.6, set: 17.25 }; // a short day

function newSky() {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color();
  return { scene, sky: buildSky(scene) };
}

test('clockTime puts the sky at the local clock time', () => {
  const { sky } = newSky();
  for (const sun of [EQUINOX, WINTER]) {
    for (const [h, m] of [[0, 0], [5, 59], [6, 36], [12, 0], [17, 15], [18, 0], [23, 30]]) {
      sky.update(1234, clockTime(new Date(2026, 9, 7, h, m), sun));
      const hour = phaseToHour(sky.state.phase, sun);
      const off = Math.abs(hour - (h + m / 60));
      assert.ok(Math.min(off, 24 - off) < 1e-6, `${h}:${m} showed ${hour}`); // 23.999... is midnight
    }
  }
  sky.update(0, clockTime(new Date(2026, 9, 7, 12, 0), EQUINOX));
  assert.ok(sky.state.daylight > 0.99, 'noon is full daylight');
  sky.update(0, clockTime(new Date(2026, 9, 7, 0, 0), EQUINOX));
  assert.equal(sky.state.daylight, 0, 'midnight is dark');
});

test('sunrise and sunset sit at the horizon, whatever the day length', () => {
  for (const sun of [EQUINOX, WINTER, { rise: 4.7, set: 21.3 }]) {
    assert.equal(hourToPhase(sun.rise, sun), 0);
    assert.ok(Math.abs(hourToPhase(sun.set, sun) - 0.5) < 1e-9);
    for (let h = 0; h < 24; h += 0.7) assert.ok(Math.abs(phaseToHour(hourToPhase(h, sun), sun) - h) < 1e-9, `${h}`);
  }
});

test('twilight is never darker than midnight', () => {
  const { scene, sky } = newSky();
  const hemi = scene.children.find((c) => c.isHemisphereLight);
  const sun = scene.children.find((c) => c.isDirectionalLight);
  const light = (h) => {
    sky.update(0, clockTime(new Date(2026, 9, 7, Math.floor(h), (h % 1) * 60), EQUINOX));
    const lum = (c) => 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    return lum(hemi.color) * hemi.intensity + lum(sun.color) * sun.intensity;
  };
  const midnight = light(0);
  for (let h = 4; h <= 8; h += 1 / 12) assert.ok(light(h) >= midnight - 1e-9, `dawn ${h}`);
  for (let h = 16; h <= 20; h += 1 / 12) assert.ok(light(h) >= midnight - 1e-9, `dusk ${h}`);
});

test('auto fog: dusk to dawn, thicker in winter; off and always', () => {
  assert.equal(fogAmount('off', -1, 1), 0);
  assert.equal(fogAmount('always', 1, 0), 1);
  assert.equal(fogAmount('auto', 1, 1), 0, 'none at noon');
  assert.equal(fogAmount('auto', -0.5, 1), 1, 'full on a winter night');
  assert.ok(fogAmount('auto', -0.5, 0) < 0.5, 'thin on a summer night');
});

test('sun times follow the date and the place', () => {
  const hcmc = placeFor('Asia/Ho_Chi_Minh');
  const oct7 = new Date(2026, 9, 7);
  const rise = (((5.68 - (420 + oct7.getTimezoneOffset()) / 60) % 24) + 24) % 24; // 05:41 in Saigon, shifted to this machine's zone
  assert.ok(Math.abs(sunTimes(oct7, hcmc).rise - rise) < 0.1);
  const london = placeFor('Europe/London');
  const dayLength = ({ rise, set }) => (set - rise + 24) % 24; // wraps when this machine is far from London
  assert.ok(dayLength(sunTimes(new Date(2026, 5, 21), london)) > 16, 'long summer days');
  assert.ok(dayLength(sunTimes(new Date(2026, 11, 21), london)) < 8, 'short winter ones');
  assert.ok(Math.abs(dayLength(sunTimes(new Date(2026, 5, 21), placeFor('Mars/Olympus'))) - 12) < 0.2, 'unknown zone: the equator');
  assert.ok(winterness(new Date(2026, 0, 15), 21) > 0.99 && winterness(new Date(2026, 6, 15), 21) < 0.01);
  assert.ok(winterness(new Date(2026, 6, 15), -33) > 0.99, 'seasons flip south of the equator');
});
