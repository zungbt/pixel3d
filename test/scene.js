import * as THREE from 'three';
import { buildWorld } from '../src/world.js';
import { buildGrass } from '../src/grass.js';
import { buildFlowers } from '../src/flowers.js';
import { buildInsects } from '../src/insects.js';
import { buildSky } from '../src/sky.js';

// The whole scene without a renderer, built in the same order as main.js.
export function buildScene() {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(); // the sky lerps it
  const world = buildWorld(scene);
  buildGrass(scene, world);
  const flowers = buildFlowers(scene, world);
  const insects = buildInsects(scene, world, flowers);
  const sky = buildSky(scene);
  return { scene, world, insects, sky };
}

// Steps the scene at a fixed fps for `seconds` (300 = one full day), calling onFrame(t, dt) after each update.
export function simulate({ insects, sky }, seconds, onFrame, fps = 60) {
  for (let i = 0; i * (1 / fps) < seconds; i++) {
    const t = i / fps;
    const dt = i ? 1 / fps : 0;
    sky.update(t);
    insects.update(t, dt, sky.state);
    onFrame(t, dt);
  }
}
