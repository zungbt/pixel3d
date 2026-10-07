import * as THREE from 'three';
import { SIZE, fbm, heightAt, mulberry32, pathEdge, sandQ } from './world.js';

const ATTEMPTS = 135000; // ~58 blades per square unit before density thinning
const HALF = SIZE / 2 - 0.1; // just inside the ground
const VIEW_MARGIN = 0.6; // world units kept past the screen edges: blade height, width and sway
const BLADE_W = 0.1;
export const BLADE_H = 0.3;
const CAMERA_YAW = Math.PI / 4; // blades face the camera, billboard-style

const windUniforms = { uWindTime: { value: 0 } };

// Marks a geometry as swaying in the wind (aWind = 1 on every vertex).
export function windAttribute(geo) {
  const n = geo.attributes.position.count;
  geo.setAttribute('aWind', new THREE.Float32BufferAttribute(new Float32Array(n).fill(1), 1));
}

// Travelling gust keyed on the blade's base; only the tip moves. Only instanced geometry with
// windAttribute() sways, so it can also patch the pixel pass's normal override for every mesh:
// the others (trees, rocks, insects) read the override's default aWind = 0.
export function addWind(material) {
  const previous = material.onBeforeCompile;
  // The wrapper's source is the same for every material, so keep the patched ones apart.
  const key = material.customProgramCacheKey();
  material.customProgramCacheKey = () => 'wind|' + key;
  material.onBeforeCompile = (shader, renderer) => {
    previous.call(material, shader, renderer);
    Object.assign(shader.uniforms, windUniforms);
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nuniform float uWindTime;\nattribute float aWind;')
      .replace('#include <begin_vertex>', /* glsl */ `
        #include <begin_vertex>
        #ifdef USE_INSTANCING
          vec2 base = instanceMatrix[3].xz;
          float sway = sin( uWindTime * 1.6 - ( base.x + base.y ) * 0.45 ) * 0.5 + 0.5;
          sway = sway * 0.12 + sin( uWindTime * 3.1 + base.x * 2.0 ) * 0.02;
          transformed.xz += vec2( 0.7071, -0.7071 ) * aWind * sway * ( position.y / ${BLADE_H.toFixed(2)} );
        #endif
      `);
  };
}

// JS copy of the shader sway above: the xz offset of a point at local height y.
export function windSway(t, x, z, y) {
  let sway = Math.sin(t * 1.6 - (x + z) * 0.45) * 0.5 + 0.5;
  sway = sway * 0.12 + Math.sin(t * 3.1 + x * 2.0) * 0.02;
  const k = sway * (y / BLADE_H);
  return [0.7071 * k, -0.7071 * k];
}

function bladeGeometry() {
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.Float32BufferAttribute([
    -BLADE_W / 2, 0, 0,
    BLADE_W / 2, 0, 0,
    0, BLADE_H, 0,
  ], 3));
  geo.rotateY(CAMERA_YAW);
  // Up-facing normals: lit like the ground and invisible to the pixel pass's normal edges.
  geo.setAttribute('normal', new THREE.Float32BufferAttribute([0, 1, 0, 0, 1, 0, 0, 1, 0], 3));
  windAttribute(geo);
  return geo;
}

// Canopy mask: 4 cells per unit, so each blade does one lookup instead of a loop over all trees.
const MASK_RES = 4;
const MASK_N = SIZE * MASK_RES;

export function canopyMask(trees) {
  const mask = new Uint8Array(MASK_N * MASK_N);
  for (const t of trees) {
    const r = t.userData.canopy;
    const x0 = Math.floor((t.position.x - r + SIZE / 2) * MASK_RES);
    const x1 = Math.ceil((t.position.x + r + SIZE / 2) * MASK_RES);
    const z0 = Math.floor((t.position.z - r + SIZE / 2) * MASK_RES);
    const z1 = Math.ceil((t.position.z + r + SIZE / 2) * MASK_RES);
    for (let gz = Math.max(z0, 0); gz < Math.min(z1, MASK_N); gz++) {
      for (let gx = Math.max(x0, 0); gx < Math.min(x1, MASK_N); gx++) {
        const cx = (gx + 0.5) / MASK_RES - SIZE / 2;
        const cz = (gz + 0.5) / MASK_RES - SIZE / 2;
        if (Math.hypot(cx - t.position.x, cz - t.position.z) < r) mask[gz * MASK_N + gx] = 1;
      }
    }
  }
  return mask;
}

export function underCanopy(mask, x, z) {
  const gx = Math.floor((x + SIZE / 2) * MASK_RES);
  const gz = Math.floor((z + SIZE / 2) * MASK_RES);
  return mask[gz * MASK_N + gx] === 1;
}

// Density: thin under tree canopies (shade) and in noisy patches, so it isn't uniform;
// none on the beach, the path or under rocks, thickening back over a ragged edge past the first two.
export function grassDensity(x, z, mask, rocks) {
  // Exact test, not the mask: a 0.25 cell would still let blades through small stones.
  if (rocks.some((r) => (r.position.x - x) ** 2 + (r.position.z - z) ** 2 < r.userData.radius ** 2)) return 0;
  const shore = THREE.MathUtils.smoothstep(sandQ(x, z), 1, 1.1);
  const path = THREE.MathUtils.smoothstep(pathEdge(x, z), 0, 0.8);
  const density = shore * path * THREE.MathUtils.lerp(0.15, 1, THREE.MathUtils.smoothstep(fbm(x * 0.15 + 50, z * 0.15), 0.25, 0.5));
  return underCanopy(mask, x, z) ? density * 0.25 : density;
}

export function buildGrass(scene, world) {
  const material = new THREE.MeshToonMaterial({ gradientMap: world.gradientMap, side: THREE.DoubleSide });
  material.onBeforeCompile = (shader) => {
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nvarying float vTip;')
      .replace('#include <begin_vertex>', `#include <begin_vertex>\nvTip = position.y / ${BLADE_H.toFixed(2)};`);
    // Darker at the root, lighter at the tip.
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying float vTip;')
      .replace('#include <color_fragment>', '#include <color_fragment>\ndiffuseColor.rgb *= mix( 0.85, 1.12, vTip );');
  };
  addWind(material);

  // Place first, then size the instance buffers to the blades actually kept.
  const rand = mulberry32(99);
  const mask = canopyMask(world.trees);
  const placed = [];
  for (let i = 0; i < ATTEMPTS; i++) {
    const x = (rand() * 2 - 1) * HALF;
    const z = (rand() * 2 - 1) * HALF;
    if (rand() > grassDensity(x, z, mask, world.rocks)) continue;
    const s = 0.7 + rand() * 0.6;
    const h = 0.25 + rand() * 0.04;
    placed.push([x, z, s, h, 0.71 + rand() * 0.1]);
  }

  const mesh = new THREE.InstancedMesh(bladeGeometry(), material, placed.length);
  mesh.receiveShadow = true;
  const matrix = new THREE.Matrix4();
  const color = new THREE.Color();
  placed.forEach(([x, z, s, h, l], i) => {
    matrix.makeScale(s, s, s).setPosition(x, heightAt(x, z), z);
    mesh.setMatrixAt(i, matrix);
    mesh.setColorAt(i, color.setHSL(h, 0.5, l, THREE.SRGBColorSpace));
  });

  scene.add(mesh);

  // The camera never moves and the view is always the same height, so the first call drops the
  // blades above and below the screen and sorts the rest by distance from the centre line; after
  // that a resize only changes how many of them are drawn.
  let edges; // |view-space x| of the kept blades, ascending
  return {
    update(t) {
      windUniforms.uWindTime.value = t;
    },
    fitView(camera) {
      if (!edges) {
        camera.updateMatrixWorld();
        const p = new THREE.Vector3();
        const kept = [];
        for (let i = 0; i < placed.length; i++) {
          p.fromArray(mesh.instanceMatrix.array, i * 16 + 12).applyMatrix4(camera.matrixWorldInverse);
          if (Math.abs(p.y) < camera.top + VIEW_MARGIN) kept.push([Math.abs(p.x), i]);
        }
        kept.sort((a, b) => a[0] - b[0]);
        const matrices = mesh.instanceMatrix.array.slice();
        const colors = mesh.instanceColor.array.slice();
        kept.forEach(([, i], j) => {
          mesh.instanceMatrix.array.set(matrices.subarray(i * 16, i * 16 + 16), j * 16);
          mesh.instanceColor.array.set(colors.subarray(i * 3, i * 3 + 3), j * 3);
        });
        mesh.instanceMatrix.needsUpdate = true;
        mesh.instanceColor.needsUpdate = true;
        edges = kept.map(([x]) => x);
      }
      let n = 0; // first blade past the side edges
      for (let hi = edges.length; n < hi; ) {
        const mid = (n + hi) >> 1;
        if (edges[mid] < camera.right + VIEW_MARGIN) n = mid + 1;
        else hi = mid;
      }
      mesh.count = n;
    },
  };
}
