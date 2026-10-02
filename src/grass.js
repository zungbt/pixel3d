import * as THREE from 'three';
import { SIZE, fbm, heightAt, mulberry32, pathDist, pathHalfW, sandQ, toonGradient } from './world.js';

const ATTEMPTS = 135000; // ~58 blades per square unit before density thinning
const HALF = SIZE / 2 - 0.1; // just inside the ground
const BLADE_W = 0.1;
const BLADE_H = 0.3;
const CAMERA_YAW = Math.PI / 4; // blades face the camera, billboard-style

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
  return geo;
}

// Canopy mask: 4 cells per unit, so each blade does one lookup instead of a loop over all trees.
const MASK_RES = 4;
const MASK_N = SIZE * MASK_RES;

function canopyMask(trees) {
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

// Density: thin under tree canopies (shade) and in noisy patches, so it isn't uniform;
// none on the beach or the path, thickening back over a ragged edge just past them.
function grassDensity(x, z, mask) {
  const shore = THREE.MathUtils.smoothstep(sandQ(x, z), 1, 1.1);
  const path = THREE.MathUtils.smoothstep(pathDist(x, z) - pathHalfW(x, z), 0, 0.8);
  const density = shore * path * THREE.MathUtils.lerp(0.15, 1, THREE.MathUtils.smoothstep(fbm(x * 0.15 + 50, z * 0.15), 0.25, 0.5));
  const gx = Math.floor((x + SIZE / 2) * MASK_RES);
  const gz = Math.floor((z + SIZE / 2) * MASK_RES);
  return mask[gz * MASK_N + gx] ? density * 0.25 : density;
}

export function buildGrass(scene, trees) {
  const windUniforms = { uWindTime: { value: 0 } };
  const material = new THREE.MeshToonMaterial({ gradientMap: toonGradient(), side: THREE.DoubleSide });
  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, windUniforms);
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nuniform float uWindTime;\nvarying float vTip;')
      .replace('#include <begin_vertex>', /* glsl */ `
        #include <begin_vertex>
        // Travelling gust keyed on the blade's base; only the tip moves.
        vec2 base = instanceMatrix[3].xz;
        float sway = sin( uWindTime * 1.6 - ( base.x + base.y ) * 0.45 ) * 0.5 + 0.5;
        sway = sway * 0.12 + sin( uWindTime * 3.1 + base.x * 2.0 ) * 0.02;
        float tip = position.y / ${BLADE_H.toFixed(2)};
        transformed.xz += vec2( 0.7071, -0.7071 ) * sway * tip;
        vTip = tip;
      `);
    // Darker at the root, lighter at the tip.
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying float vTip;')
      .replace('#include <color_fragment>', '#include <color_fragment>\ndiffuseColor.rgb *= mix( 0.85, 1.12, vTip );');
  };
  // Distinct cache key so this program is never shared with plain toon materials.
  material.customProgramCacheKey = () => 'grass-wind';

  const mesh = new THREE.InstancedMesh(bladeGeometry(), material, ATTEMPTS);
  mesh.receiveShadow = true;

  const rand = mulberry32(99);
  const matrix = new THREE.Matrix4();
  const color = new THREE.Color();
  const mask = canopyMask(trees);
  let count = 0;
  for (let i = 0; i < ATTEMPTS; i++) {
    const x = (rand() * 2 - 1) * HALF;
    const z = (rand() * 2 - 1) * HALF;
    if (rand() > grassDensity(x, z, mask)) continue;
    const s = 0.7 + rand() * 0.6;
    matrix.makeScale(s, s, s).setPosition(x, heightAt(x, z), z);
    mesh.setMatrixAt(count, matrix);
    mesh.setColorAt(count, color.setHSL(0.25 + rand() * 0.04, 0.5, 0.71 + rand() * 0.1, THREE.SRGBColorSpace));
    count++;
  }
  mesh.count = count;

  scene.add(mesh);

  return {
    update(t) {
      windUniforms.uWindTime.value = t;
    },
  };
}
