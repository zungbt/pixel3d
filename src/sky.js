import * as THREE from 'three';
import { NOISE_GLSL } from './world.js';

export const DAY_LENGTH = 300; // seconds for a full day-night cycle
const DAY_START = 0.08; // fraction of the cycle at t = 0 (early morning)

const SUN_DAY = new THREE.Color(0xfff1d6);
const SUN_LOW = new THREE.Color(0xffa060);
const MOON = new THREE.Color(0x9fb4ff);
const SKY_DAY = new THREE.Color(0x7fb2e5);
const SKY_DUSK = new THREE.Color(0xe0875a);
const SKY_NIGHT = new THREE.Color(0x0b0d1a);
const HEMI_DAY = new THREE.Color(0xbbe4f4); // faint teal: shadows shift cool-green
const HEMI_NIGHT = new THREE.Color(0x2a3a66);

const SUN_DIST = 38; // keeps every visible ground point in front of the shadow camera's near plane

const cloudUniforms = { uCloudTime: { value: 0 } };

const CLOUD_VERTEX = /* glsl */ `
  #include <worldpos_vertex>
  vec4 cloudPos = vec4( transformed, 1.0 );
  #ifdef USE_INSTANCING
    cloudPos = instanceMatrix * cloudPos;
  #endif
  vCloudPos = ( modelMatrix * cloudPos ).xyz;
`;

// Hard-edged value-noise blobs drifting with the wind; they only dim direct sunlight.
const CLOUD_PARS = /* glsl */ `
  uniform float uCloudTime;
  varying vec3 vCloudPos;
  ${NOISE_GLSL}

  float cloudShade( vec3 worldPos ) {
    vec2 p = worldPos.xz * 0.15 + vec2( uCloudTime * 0.05, uCloudTime * 0.02 );
    float n = valueNoise( p ) * 0.65 + valueNoise( p * 2.3 ) * 0.35;
    return mix( 1.0, 0.35, smoothstep( 0.58, 0.6, n ) );
  }
`;

function addCloudShadow(material) {
  const previous = material.onBeforeCompile; // keep patches like the grass wind
  // The wrapper's source is the same for every material, so keep the patched ones apart.
  const key = material.customProgramCacheKey();
  material.customProgramCacheKey = () => 'cloud|' + key;
  material.onBeforeCompile = (shader, renderer) => {
    previous.call(material, shader, renderer);
    Object.assign(shader.uniforms, cloudUniforms);
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec3 vCloudPos;')
      .replace('#include <worldpos_vertex>', CLOUD_VERTEX);
    shader.fragmentShader = shader.fragmentShader.replace(
      '#include <lights_toon_pars_fragment>',
      CLOUD_PARS +
        THREE.ShaderChunk.lights_toon_pars_fragment.replace(
          '* directLight.color;',
          '* directLight.color * cloudShade( vCloudPos );',
        ),
    );
  };
}

// Call after everything that should receive cloud shadows is in the scene.
export function buildSky(scene) {
  // Collect first: a material shared by several meshes must be wrapped only once.
  const materials = new Set();
  scene.traverse((obj) => {
    if (obj.material?.isMeshToonMaterial) materials.add(obj.material);
  });
  materials.forEach(addCloudShadow);

  const hemi = new THREE.HemisphereLight(HEMI_DAY, 0x3a3326, 1.2);
  scene.add(hemi);

  // One directional light: the sun by day, the moon (mirrored across the horizon) by night.
  const sun = new THREE.DirectionalLight(SUN_DAY, 2.5);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  // ±22 covers the visible part of the ground (screen corners land ~20 units out on 21:9).
  sun.shadow.camera.left = -22;
  sun.shadow.camera.right = 22;
  sun.shadow.camera.top = 22;
  sun.shadow.camera.bottom = -22;
  sun.shadow.camera.near = 1;
  sun.shadow.camera.far = 80;
  sun.shadow.bias = -0.0005;
  sun.shadow.normalBias = 0.02;
  scene.add(sun);

  // Read by the insects each frame; phase 0 -> 0.5 is day, 0.5 is sunset.
  const state = { phase: 0, elevation: 0, daylight: 0 };

  return {
    state,
    update(t) {
      cloudUniforms.uCloudTime.value = t;

      state.phase = (t / DAY_LENGTH + DAY_START) % 1;
      const angle = (t / DAY_LENGTH + DAY_START) * Math.PI * 2;
      const elevation = Math.sin(angle); // >0 day, <0 night
      const isDay = elevation >= 0;
      const dir = isDay ? 1 : -1;
      sun.position.set(Math.cos(angle) * SUN_DIST * dir, Math.abs(elevation) * SUN_DIST, SUN_DIST * 0.36);

      // Light fades to 0 at the horizon, so swapping sun <-> moon there doesn't pop.
      const fade = THREE.MathUtils.smoothstep(Math.abs(elevation), 0, 0.2);
      if (isDay) {
        sun.color.lerpColors(SUN_LOW, SUN_DAY, THREE.MathUtils.smoothstep(elevation, 0, 0.5));
        sun.intensity = 2.5 * fade;
      } else {
        sun.color.copy(MOON);
        sun.intensity = 0.3 * fade;
      }

      const daylight = THREE.MathUtils.smoothstep(elevation, -0.1, 0.25);
      state.elevation = elevation;
      state.daylight = daylight;
      const dusk = 1 - THREE.MathUtils.smoothstep(Math.abs(elevation), 0, 0.35);
      hemi.color.lerpColors(HEMI_NIGHT, HEMI_DAY, daylight);
      hemi.intensity = THREE.MathUtils.lerp(0.2, 1.2, daylight);
      scene.background.lerpColors(SKY_NIGHT, SKY_DAY, daylight).lerp(SKY_DUSK, dusk * 0.6);
    },
  };
}
