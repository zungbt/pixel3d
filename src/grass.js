import * as THREE from 'three';
import { heightAt, mulberry32, toonGradient } from './world.js';

const COUNT = 15000;
const HALF = 7.9; // just inside the 16×16 ground
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

export function buildGrass(scene) {
  const windUniforms = { uWindTime: { value: 0 } };
  const material = new THREE.MeshToonMaterial({ gradientMap: toonGradient(), side: THREE.DoubleSide });
  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, windUniforms);
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nuniform float uWindTime;')
      .replace('#include <begin_vertex>', /* glsl */ `
        #include <begin_vertex>
        // Travelling gust keyed on the blade's base; only the tip moves.
        vec2 base = instanceMatrix[3].xz;
        float sway = sin( uWindTime * 1.6 - ( base.x + base.y ) * 0.45 ) * 0.5 + 0.5;
        sway = sway * 0.12 + sin( uWindTime * 3.1 + base.x * 2.0 ) * 0.02;
        float tip = position.y / ${BLADE_H.toFixed(2)};
        transformed.xz += vec2( 0.7071, -0.7071 ) * sway * tip;
      `);
  };
  // Distinct cache key so this program is never shared with plain toon materials.
  material.customProgramCacheKey = () => 'grass-wind';

  const mesh = new THREE.InstancedMesh(bladeGeometry(), material, COUNT);
  mesh.receiveShadow = true;

  const rand = mulberry32(99);
  const matrix = new THREE.Matrix4();
  const color = new THREE.Color();
  for (let i = 0; i < COUNT; i++) {
    const x = (rand() * 2 - 1) * HALF;
    const z = (rand() * 2 - 1) * HALF;
    const s = 0.7 + rand() * 0.6;
    matrix.makeScale(s, s, s).setPosition(x, heightAt(x, z), z);
    mesh.setMatrixAt(i, matrix);
    mesh.setColorAt(i, color.setHSL(0.27 + rand() * 0.04, 0.42, 0.4 + rand() * 0.12, THREE.SRGBColorSpace));
  }

  scene.add(mesh);

  return {
    update(t) {
      windUniforms.uWindTime.value = t;
    },
  };
}
