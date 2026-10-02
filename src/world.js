import * as THREE from 'three';

const SIZE = 16;

// Seeded PRNG so the layout is identical on every load.
function mulberry32(seed) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Shared by the terrain and by prop placement so nothing floats or sinks.
export function heightAt(x, z) {
  return 0.35 * Math.sin(x * 0.6) * Math.cos(z * 0.5) + 0.15 * Math.sin((x + z) * 0.9);
}

function toonGradient() {
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

function buildRock(rand, gradientMap) {
  const r = 0.3 + rand() * 0.4;
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

  const leaves = new THREE.Mesh(
    new THREE.IcosahedronGeometry(0.6 + rand() * 0.3, 0),
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

function placeOnGround(obj, x, z) {
  obj.position.set(x, heightAt(x, z), z);
}

export function buildWorld(scene) {
  const rand = mulberry32(42);
  const gradientMap = toonGradient();

  scene.add(buildGround(gradientMap));

  const spread = () => (rand() - 0.5) * (SIZE - 3);
  const trees = [];
  for (let i = 0; i < 10; i++) {
    const rock = buildRock(rand, gradientMap);
    placeOnGround(rock, spread(), spread());
    scene.add(rock);
  }
  for (let i = 0; i < 8; i++) {
    const tree = buildTree(rand, gradientMap);
    placeOnGround(tree, spread(), spread());
    tree.userData.phase = i * 2.4; // not from rand(), so the layout stays unchanged
    trees.push(tree);
    scene.add(tree);
  }

  scene.add(new THREE.HemisphereLight(0xbfd8ff, 0x3a3326, 1.2));

  const sun = new THREE.DirectionalLight(0xfff1d6, 2.5);
  sun.position.set(8, 12, 4);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  sun.shadow.camera.left = -10;
  sun.shadow.camera.right = 10;
  sun.shadow.camera.top = 10;
  sun.shadow.camera.bottom = -10;
  sun.shadow.camera.near = 1;
  sun.shadow.camera.far = 40;
  sun.shadow.bias = -0.0005;
  sun.shadow.normalBias = 0.02;
  scene.add(sun);

  return {
    update(t) {
      // Group origin sits on the ground, so the tree pivots at its base.
      for (const tree of trees) {
        const p = tree.userData.phase;
        tree.rotation.z = 0.035 * Math.sin(t * 1.3 + p);
        tree.rotation.x = 0.02 * Math.sin(t * 0.9 + p * 1.7);
      }
    },
  };
}
