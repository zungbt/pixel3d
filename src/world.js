import * as THREE from 'three';

export const SIZE = 48; // large enough to fill a 21:9 screen at VIEW_HEIGHT 13

// Seeded PRNG so the layout is identical on every load.
export function mulberry32(seed) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function baseHeight(x, z) {
  return 0.35 * Math.sin(x * 0.6) * Math.cos(z * 0.5) + 0.15 * Math.sin((x + z) * 0.9);
}

// Pond: an irregular water polygon lying on terrain flattened around it, so the
// water never intersects sloped triangles (nothing is carved).
const POND_X = 3.5;
const POND_Z = -0.7;
export const SAND_SCALE = 1.18; // sand ring, relative to the shoreline
const POND_R_MAX = 2.6 * 1.22;
const FLAT_R = POND_R_MAX * SAND_SCALE + 1.5; // + one grid cell diagonal: every triangle under the sand is flat
const FLAT_BLEND = 3;
const SHORE_Y = baseHeight(POND_X, POND_Z);
const WATER = new THREE.Color(0x4a90c8);

function pondRadius(theta) {
  return 2.6 * (1 + 0.15 * Math.sin(3 * theta + 1) + 0.07 * Math.sin(5 * theta + 2));
}

// Distance from the pond centre relative to the shoreline: < 1 water, < SAND_SCALE sand.
export function pondQ(x, z) {
  const dx = x - POND_X;
  const dz = z - POND_Z;
  return Math.hypot(dx, dz) / pondRadius(Math.atan2(dz, dx));
}

// Shared by the terrain and by prop placement so nothing floats or sinks.
export function heightAt(x, z) {
  const d = Math.hypot(x - POND_X, z - POND_Z);
  const flat = 1 - THREE.MathUtils.smoothstep(d, FLAT_R, FLAT_R + FLAT_BLEND);
  return THREE.MathUtils.lerp(baseHeight(x, z), SHORE_Y, flat);
}

// Smooth 2D value noise in [0, 1], used as density maps for placement.
function hash2(x, z) {
  const s = Math.sin(x * 127.1 + z * 311.7) * 43758.5453;
  return s - Math.floor(s);
}

function valueNoise(x, z) {
  const ix = Math.floor(x);
  const iz = Math.floor(z);
  const fx = x - ix;
  const fz = z - iz;
  const ux = fx * fx * (3 - 2 * fx);
  const uz = fz * fz * (3 - 2 * fz);
  const a = THREE.MathUtils.lerp(hash2(ix, iz), hash2(ix + 1, iz), ux);
  const b = THREE.MathUtils.lerp(hash2(ix, iz + 1), hash2(ix + 1, iz + 1), ux);
  return THREE.MathUtils.lerp(a, b, uz);
}

export function fbm(x, z) {
  return valueNoise(x, z) * 0.65 + valueNoise(x * 2.1 + 17, z * 2.1 + 31) * 0.35;
}

function slopeAt(x, z) {
  const e = 0.1;
  return Math.hypot(heightAt(x + e, z) - heightAt(x - e, z), heightAt(x, z + e) - heightAt(x, z - e)) / (2 * e);
}

export function toonGradient() {
  const tex = new THREE.DataTexture(new Uint8Array([70, 160, 255]), 3, 1, THREE.RedFormat);
  tex.minFilter = THREE.NearestFilter;
  tex.magFilter = THREE.NearestFilter;
  tex.generateMipmaps = false;
  tex.needsUpdate = true;
  return tex;
}

function buildGround(gradientMap) {
  let geo = new THREE.PlaneGeometry(SIZE, SIZE, SIZE, SIZE);
  geo.rotateX(-Math.PI / 2);
  const pos = geo.attributes.position;
  for (let i = 0; i < pos.count; i++) {
    pos.setY(i, heightAt(pos.getX(i), pos.getZ(i)));
  }
  // Non-indexed so each face gets its own flat normal (the pixel pass reads geometry normals).
  geo = geo.toNonIndexed();
  geo.computeVertexNormals();

  const mesh = new THREE.Mesh(geo, new THREE.MeshToonMaterial({ color: 0x5e9e4a, gradientMap }));
  mesh.receiveShadow = true;
  return mesh;
}

function buildRock(rand, r, gradientMap) {
  const mesh = new THREE.Mesh(
    new THREE.DodecahedronGeometry(r, 0),
    new THREE.MeshToonMaterial({ color: 0x8a8f99, gradientMap }),
  );
  mesh.scale.set(1, 0.6 + rand() * 0.4, 1);
  mesh.rotation.y = rand() * Math.PI;
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

function buildTree(rand, gradientMap) {
  const tree = new THREE.Group();
  const height = 0.8 + rand() * 0.6;

  const trunk = new THREE.Mesh(
    new THREE.CylinderGeometry(0.12, 0.16, height, 6),
    new THREE.MeshToonMaterial({ color: 0x6b4a2f, gradientMap }),
  );
  trunk.position.y = height / 2;

  const canopy = 0.6 + rand() * 0.3;
  tree.userData.canopy = canopy;
  const leaves = new THREE.Mesh(
    new THREE.IcosahedronGeometry(canopy, 0),
    new THREE.MeshToonMaterial({ color: 0x2f7a3c, gradientMap }),
  );
  leaves.position.y = height + 0.3;

  for (const part of [trunk, leaves]) {
    part.castShadow = true;
    part.receiveShadow = true;
    tree.add(part);
  }
  return tree;
}

function buildPondShape(scale, y, color, gradientMap) {
  const geo = new THREE.CircleGeometry(1, 72);
  geo.rotateX(-Math.PI / 2);
  const pos = geo.attributes.position;
  for (let i = 0; i < pos.count; i++) {
    const r = pondRadius(Math.atan2(pos.getZ(i), pos.getX(i))) * scale;
    pos.setXYZ(i, pos.getX(i) * r, 0, pos.getZ(i) * r);
  }
  const mesh = new THREE.Mesh(geo, new THREE.MeshToonMaterial({ color, gradientMap }));
  mesh.position.set(POND_X, y, POND_Z);
  mesh.receiveShadow = true;
  return mesh;
}

function placeOnGround(obj, x, z) {
  obj.position.set(x, heightAt(x, z), z);
}

export function buildWorld(scene) {
  const rand = mulberry32(42);
  const gradientMap = toonGradient();

  scene.add(buildGround(gradientMap));
  scene.add(buildPondShape(SAND_SCALE, SHORE_Y + 0.01, 0xc9b27c, gradientMap));
  const water = buildPondShape(1, SHORE_Y + 0.02, WATER, gradientMap);
  scene.add(water);

  const spread = () => (rand() - 0.5) * (SIZE - 3);
  const tooClose = (x, z, minDist) =>
    trees.some((t) => Math.hypot(t.position.x - x, t.position.z - z) < minDist);

  // Trees: groves where the forest mask is high, rare loners in clearings,
  // and a minimum spacing between trunks (dart throwing ≈ Poisson-disk).
  const trees = [];
  for (let attempt = 0; attempt < 4000 && trees.length < 120; attempt++) {
    const x = spread();
    const z = spread();
    const forest = THREE.MathUtils.smoothstep(fbm(x * 0.09, z * 0.09), 0.45, 0.65);
    if (rand() > Math.max(forest, 0.01) || tooClose(x, z, 1.6) || pondQ(x, z) < 1.6) continue;
    const tree = buildTree(rand, gradientMap);
    placeOnGround(tree, x, z);
    tree.userData.phase = trees.length * 2.4;
    trees.push(tree);
    scene.add(tree);
  }

  // Rocks: clusters of one boulder plus a few small stones, favouring slopes, never inside a tree.
  for (let clusters = 0, attempt = 0; clusters < 12 && attempt < 500; attempt++) {
    const cx = spread();
    const cz = spread();
    if (rand() > 0.3 + slopeAt(cx, cz) * 1.5 || tooClose(cx, cz, 1.5)) continue;
    clusters++;
    const stones = [[cx, cz, 0.6 + rand() * 0.3]];
    const small = 2 + Math.floor(rand() * 4);
    for (let i = 0; i < small; i++) {
      const a = rand() * Math.PI * 2;
      const d = 0.6 + rand() * 1.0;
      stones.push([cx + Math.cos(a) * d, cz + Math.sin(a) * d, 0.2 + rand() * 0.15]);
    }
    for (const [x, z, r] of stones) {
      if (tooClose(x, z, 0.7) || pondQ(x, z) < 1) continue;
      const rock = buildRock(rand, r, gradientMap);
      placeOnGround(rock, x, z);
      scene.add(rock);
    }
  }

  return {
    trees,
    update(t) {
      // Water picks up the sky colour (sky.update runs after this, so it lags one frame).
      water.material.color.copy(WATER).lerp(scene.background, 0.35);
      // Group origin sits on the ground, so the tree pivots at its base.
      for (const tree of trees) {
        const p = tree.userData.phase;
        tree.rotation.z = 0.035 * Math.sin(t * 1.3 + p);
        tree.rotation.x = 0.02 * Math.sin(t * 0.9 + p * 1.7);
      }
    },
  };
}
