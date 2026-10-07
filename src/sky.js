import * as THREE from 'three';
import { NOISE_GLSL } from './world.js';

export const DAY_LENGTH = 300; // seconds for a full day-night cycle
const DAY_START = 0.08; // fraction of the cycle at t = 0 (early morning)

// Clock hour <-> sky phase, for a day with sunrise and sunset at sun.rise and sun.set (sunTimes):
// the day maps onto phase 0 -> 0.5 and the night onto 0.5 -> 1, each at an even pace.
export function hourToPhase(hour, { rise, set }) {
  const day = (set - rise + 24) % 24;
  const since = (((hour - rise) % 24) + 24) % 24;
  return since < day ? (0.5 * since) / day : 0.5 + (0.5 * (since - day)) / (24 - day);
}

export function phaseToHour(phase, { rise, set }) {
  const day = (set - rise + 24) % 24;
  const hour = phase < 0.5 ? rise + 2 * phase * day : set + 2 * (phase - 0.5) * (24 - day);
  return ((hour % 24) + 24) % 24;
}

// The sky time that shows this local clock time.
export function clockTime(date, sun) {
  const hour = date.getHours() + date.getMinutes() / 60 + date.getSeconds() / 3600;
  return ((((hourToPhase(hour, sun) - DAY_START) % 1) + 1) % 1) * DAY_LENGTH;
}

// Ground fog: 'auto' rolls in at dusk and burns off after sunrise, thicker in winter
// (winterness, 0-1); 'always' is full fog day and night.
export function fogAmount(mode, elevation, winter) {
  if (mode === 'always') return 1;
  if (mode !== 'auto') return 0;
  return (1 - THREE.MathUtils.smoothstep(elevation, -0.05, 0.2)) * THREE.MathUtils.lerp(0.35, 1, winter);
}

const SUN_DAY = new THREE.Color(0xfff1d6);
const SUN_LOW = new THREE.Color(0xffa060);
const MOON = new THREE.Color(0x9fb4ff);
const SKY_DAY = new THREE.Color(0x7fb2e5);
const SKY_DUSK = new THREE.Color(0xe0875a);
const SKY_NIGHT = new THREE.Color(0x0b0d1a);
const HEMI_DAY = new THREE.Color(0xbbe4f4); // faint teal: shadows shift cool-green
const HEMI_NIGHT = new THREE.Color(0x2a3a66);
const FOG_DAY = new THREE.Color(0xdfe6ea);
const FOG_NIGHT = new THREE.Color(0x323c58);

const SUN_DIST = 38; // keeps every visible ground point in front of the shadow camera's near plane

const cloudUniforms = { uCloudTime: { value: 0 }, uFogAmount: { value: 0 }, uFogColor: { value: new THREE.Color() } };

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

// Low fog by absolute height, thinning upward: hollows and the pond fill first, the clock face
// and the crowns get a light haze; drifting patches.
const FOG_PARS = /* glsl */ `
  uniform float uFogAmount;
  uniform vec3 uFogColor;
  float groundFog( vec3 worldPos ) {
    float layer = exp( -0.5 * max( worldPos.y, 0.0 ) );
    vec2 p = worldPos.xz * 0.18 + vec2( uCloudTime * 0.04, uCloudTime * 0.015 );
    float n = valueNoise( p + 40.0 ) * 0.65 + valueNoise( p * 2.3 + 7.0 ) * 0.35;
    return uFogAmount * layer * mix( 0.45, 1.0, n ) * 0.5;
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
    shader.fragmentShader = shader.fragmentShader
      .replace(
        '#include <lights_toon_pars_fragment>',
        CLOUD_PARS +
          FOG_PARS +
          THREE.ShaderChunk.lights_toon_pars_fragment.replace(
            '* directLight.color;',
            '* directLight.color * cloudShade( vCloudPos );',
          ),
      )
      .replace(
        '#include <fog_fragment>',
        '#include <fog_fragment>\ngl_FragColor.rgb = mix( gl_FragColor.rgb, uFogColor, groundFog( vCloudPos ) );',
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
    // dayT sets the time of day; t alone drives the clouds, so they keep drifting when dayT is the clock.
    update(t, dayT = t, fog = { mode: 'off', winter: 0 }) {
      cloudUniforms.uCloudTime.value = t;

      state.phase = (dayT / DAY_LENGTH + DAY_START) % 1;
      const angle = (dayT / DAY_LENGTH + DAY_START) * Math.PI * 2;
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
        sun.intensity = 0.12 * fade; // faint, so moon shadows stay soft
      }

      const daylight = THREE.MathUtils.smoothstep(elevation, -0.1, 0.25);
      state.elevation = elevation;
      state.daylight = daylight;
      const dusk = 1 - THREE.MathUtils.smoothstep(Math.abs(elevation), 0, 0.35);
      // The ambient brightens from an hour before sunrise (and dims until an hour after sunset):
      // while the moon fades out and the sun fades in there's no direct light, and with the
      // night ambient that left twilight darker than midnight.
      const twilight = THREE.MathUtils.smoothstep(elevation, -0.25, 0.25);
      hemi.color.lerpColors(HEMI_NIGHT, HEMI_DAY, twilight);
      // The night ambient's blue is dim, hence the high intensity: with the faint moon it keeps
      // moon shadows at ~60% of the lit ground instead of near black.
      hemi.intensity = THREE.MathUtils.lerp(2.1, 1.2, twilight);
      scene.background.lerpColors(SKY_NIGHT, SKY_DAY, daylight).lerp(SKY_DUSK, dusk * 0.6);
      cloudUniforms.uFogAmount.value = fogAmount(fog.mode, elevation, fog.winter);
      cloudUniforms.uFogColor.value.lerpColors(FOG_NIGHT, FOG_DAY, daylight).lerp(SKY_DUSK, dusk * 0.3);
    },
  };
}
