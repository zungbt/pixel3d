import * as THREE from 'three';
import { SIZE, fbm, heightAt, mulberry32 } from './world.js';
import { addWind, canopyMask, windAttribute, grassDensity, underCanopy } from './grass.js';

const ATTEMPTS = 24000; // ~1800 flowers, ~9 patches in view
const HALF = SIZE / 2 - 0.1;
export const HEAD_Y = 0.36; // over most grass tips (blades reach 0.39) so they don't flicker across it; also scales the sway
export const HEAD_R = 0.1; // ~3 art pixels
const COLORS = [0xf4f1e8, 0xf2d35b, 0xa98be0, 0xee9fc0].map((c) => new THREE.Color(c));

// Wildflower patches in open meadow: same exclusions as the grass, plus none under canopies.
// Unrotated, unit-scale instances, so windSway() in JS matches the shader for perched insects.
export function buildFlowers(scene, world) {
  const rand = mulberry32(314);
  const mask = canopyMask(world.trees);
  const heads = [];
  for (let i = 0; i < ATTEMPTS; i++) {
    const x = (rand() * 2 - 1) * HALF;
    const z = (rand() * 2 - 1) * HALF;
    const patch = THREE.MathUtils.smoothstep(fbm(x * 0.3 + 200, z * 0.3), 0.7, 0.74);
    if (rand() > patch * grassDensity(x, z, mask, world.rocks) || underCanopy(mask, x, z)) continue;
    heads.push({ x, z, y: heightAt(x, z) + HEAD_Y });
  }

  const geo = new THREE.OctahedronGeometry(HEAD_R, 0);
  geo.translate(0, HEAD_Y, 0);
  windAttribute(geo);
  const material = new THREE.MeshToonMaterial({ gradientMap: world.gradientMap });
  addWind(material);

  const mesh = new THREE.InstancedMesh(geo, material, heads.length);
  mesh.receiveShadow = true;
  const matrix = new THREE.Matrix4();
  const color = new THREE.Color();
  heads.forEach(({ x, z, y }, i) => {
    matrix.makeTranslation(x, y - HEAD_Y, z);
    mesh.setMatrixAt(i, matrix);
    // One colour per patch (low-frequency noise), with a little per-flower variation.
    const base = COLORS[Math.floor(fbm(x * 0.12 + 300, z * 0.12) * 8) % COLORS.length];
    mesh.setColorAt(i, color.copy(base).multiplyScalar(0.92 + rand() * 0.16));
  });
  scene.add(mesh);

  return { heads };
}
