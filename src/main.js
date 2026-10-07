import * as THREE from 'three';
import { EffectComposer } from 'three/addons/postprocessing/EffectComposer.js';
import { RenderPixelatedPass } from 'three/addons/postprocessing/RenderPixelatedPass.js';
import { OutputPass } from 'three/addons/postprocessing/OutputPass.js';
import { buildWorld } from './world.js';
import { addWind, buildGrass } from './grass.js';
import { buildSky, clockTime, DAY_LENGTH } from './sky.js';
import { buildFlowers } from './flowers.js';
import { buildInsects } from './insects.js';
import { buildClock } from './clock.js';
import { createFrameLimiter } from './frame-limiter.js';
import { createBench } from './bench.js';
import { loadSettings, parseSetting, saveSetting } from './settings.js';

// Wallpaper mode (?wallpaper): no HUD, the sky follows the local clock, and 15 fps, for this
// page only: the saved settings are left alone.
const params = new URLSearchParams(location.search);
const wallpaper = params.has('wallpaper');
const settings = loadSettings();
if (wallpaper) Object.assign(settings, { fps: 15, clock: true, hideHud: true });

const VIEW_HEIGHT = 18; // world units visible vertically
// Every pass renders at art resolution; the canvas is stretched to the window with
// image-rendering: pixelated (index.html), so composite and output run once per art pixel.
// Sized in device pixels, so browser zoom and display scaling keep every art pixel the same
// whole number of screen pixels instead of an uneven 4-or-5.
const artSize = () => [
  Math.floor((window.innerWidth * devicePixelRatio) / settings.pixelSize),
  Math.floor((window.innerHeight * devicePixelRatio) / settings.pixelSize),
];

const renderer = new THREE.WebGLRenderer();
renderer.setSize(...artSize(), false);
renderer.shadowMap.enabled = true;
// Every renderer.render of the lit scene would redraw the shadow map, the pixel pass's normal
// render included; the loop flags it once per frame so only the colour render draws it.
renderer.shadowMap.autoUpdate = false;
document.body.appendChild(renderer.domElement);
// The grass frees its CPU-side instance data after upload, so a lost context can't be restored.
renderer.domElement.addEventListener('webglcontextlost', () => location.reload());

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x151729);
const world = buildWorld(scene);
const grass = buildGrass(scene, world);
const flowers = buildFlowers(scene, world);
const insects = buildInsects(scene, world, flowers);
const clock = buildClock(scene, world);
const sky = buildSky(scene); // last: patches cloud shadows into every toon material above

const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0.1, 100);
const yaw = Math.PI / 4;
const pitch = THREE.MathUtils.degToRad(30);
const dist = 30;
camera.position.set(
  dist * Math.cos(pitch) * Math.sin(yaw),
  dist * Math.sin(pitch),
  dist * Math.cos(pitch) * Math.cos(yaw),
);
camera.lookAt(0, 0, 0);

function updateFrustum() {
  const aspect = window.innerWidth / window.innerHeight;
  camera.top = VIEW_HEIGHT / 2;
  camera.bottom = -VIEW_HEIGHT / 2;
  camera.left = (-VIEW_HEIGHT / 2) * aspect;
  camera.right = (VIEW_HEIGHT / 2) * aspect;
  camera.updateProjectionMatrix();
  grass.fitView(camera);
}
updateFrustum();

const composer = new EffectComposer(renderer);
const pixelPass = new RenderPixelatedPass(1, scene, camera, {
  normalEdgeStrength: 0.3,
  depthEdgeStrength: 0.4,
});
// The normal buffer is drawn with this override material (a private field of the pass);
// without the wind its outlines would sit at the blades' rest positions. Meshes without the
// aWind attribute (trees, rocks, insects) read this default and stay still.
addWind(pixelPass._normalMaterial);
pixelPass._normalMaterial.defaultAttributeValues = { aWind: [0] };
composer.addPass(pixelPass);
composer.addPass(new OutputPass());

function resize() {
  renderer.setSize(...artSize(), false);
  composer.setSize(...artSize());
  updateFrustum();
}
window.addEventListener('resize', resize);

// Time-of-day slider, 0-24 h (sunrise 06:00, sunset 18:00). It moves only the sky's clock;
// wind, water and wings keep real time. Always jumps forward, so the sky's time never goes negative.
const timeInput = document.querySelector('#time input');
const timeLabel = document.querySelector('#time span');
let skyOffset = 0;
timeInput.addEventListener('input', () => {
  const phase = (timeInput.valueAsNumber - 6) / 24;
  skyOffset += ((((phase - sky.state.phase) % 1) + 1) % 1) * DAY_LENGTH;
});

// Settings panel behind the gear button; every change applies at once and is remembered.
const hud = document.querySelector('#hud');
const form = document.querySelector('#settings');
form.pixelSize.value = settings.pixelSize;
form.fps.value = settings.fps ?? '';
form.clock.value = settings.clock;
form.hideHud.checked = settings.hideHud;
form.showClock.checked = settings.showClock;
function applyHud() {
  hud.classList.toggle('bare', settings.hideHud);
  timeInput.disabled = settings.clock; // the slider still shows the clock's time
  clock.object.visible = settings.showClock;
}
applyHud();
document.querySelector('#gear').addEventListener('click', () => (form.hidden = !form.hidden));
form.addEventListener('change', ({ target }) => {
  const value = parseSetting(target.name, target.type === 'checkbox' ? String(target.checked) : target.value);
  // Back to the slider: carry on from the clock's time of day, jumping forward as the slider does.
  if (target.name === 'clock' && !value) {
    skyOffset += (((clockTime() - (lastT ?? 0) - skyOffset) % DAY_LENGTH) + DAY_LENGTH) % DAY_LENGTH;
  }
  settings[target.name] = value;
  saveSetting(target.name, value);
  if (target.name === 'pixelSize') resize();
  applyHud();
});
if (wallpaper) hud.style.display = 'none';
const shouldRender = createFrameLimiter();

const bench = params.has('bench')
  ? createBench(renderer, (hour) => {
      timeInput.value = hour;
      timeInput.dispatchEvent(new Event('input'));
    })
  : null;

let lastT;
renderer.setAnimationLoop((ms) => {
  if (!shouldRender(ms, settings.fps)) return; // skipped ticks don't advance lastT, so dt spans the gap
  const t = ms / 1000;
  const dt = lastT === undefined ? 0 : Math.min(t - lastT, 0.1); // no jumps after a hidden tab
  lastT = t;
  bench?.begin();
  world.update(t);
  bench?.mark('world');
  grass.update(t);
  bench?.mark('grass');
  sky.update(t + skyOffset, settings.clock ? clockTime() : t + skyOffset);
  bench?.mark('sky');
  insects.update(t, dt, sky.state); // after the sky, so its state is this frame's
  bench?.mark('insects');
  clock.update(new Date(), sky.state.daylight);
  const hour = (sky.state.phase * 24 + 6) % 24;
  timeInput.value = hour;
  timeLabel.textContent = `${String(Math.floor(hour)).padStart(2, '0')}:${String(Math.floor((hour % 1) * 60)).padStart(2, '0')}`;
  bench?.mark('hud');
  renderer.shadowMap.needsUpdate = true;
  composer.render();
  bench?.mark('render');
  bench?.end();
});
