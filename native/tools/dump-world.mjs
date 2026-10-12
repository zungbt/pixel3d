// Dumps the web layout (seed 42) for meadow-core's golden test: `node native/tools/dump-world.mjs`
// from pixel3d/. Numbers print as shortest round-trip decimals, which Rust parses back exactly.
import { writeFileSync } from 'node:fs';
import * as THREE from 'three';
import { buildWorld, mulberry32 } from '../../src/world.js';

const lines = [];
const rand = mulberry32(42);
for (let i = 0; i < 64; i++) lines.push(`rand ${rand()}`);

const scene = new THREE.Scene();
scene.background = new THREE.Color();
const { trees, rocks } = buildWorld(scene);
for (const t of trees) {
  const [trunk, crown] = t.children;
  lines.push(`tree ${t.position.x} ${t.position.y} ${t.position.z} ${trunk.scale.y} ${crown.scale.x}`);
}
for (const r of rocks) {
  lines.push(`rock ${r.position.x} ${r.position.y} ${r.position.z} ${r.scale.x} ${r.scale.y} ${r.rotation.y}`);
}
writeFileSync(new URL('../meadow-core/tests/fixtures/world42.txt', import.meta.url), lines.join('\n') + '\n');
console.log(`${trees.length} trees, ${rocks.length} rocks`);
