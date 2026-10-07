import * as THREE from 'three';
import { EffectComposer } from 'three/addons/postprocessing/EffectComposer.js';
import { RenderPixelatedPass } from 'three/addons/postprocessing/RenderPixelatedPass.js';
import { OutputPass } from 'three/addons/postprocessing/OutputPass.js';
import { buildWorld } from './world.js';
import { addWind, buildGrass } from './grass.js';
import { buildSky } from './sky.js';
import { buildInsects } from './insects.js';

const VIEW_HEIGHT = 18; // world units visible vertically
const PIXEL_SIZE = 4; // screen px per art pixel (no setPixelRatio, so CSS px)

const renderer = new THREE.WebGLRenderer();
renderer.setSize(window.innerWidth, window.innerHeight);
renderer.shadowMap.enabled = true;
document.body.appendChild(renderer.domElement);

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x151729);
const world = buildWorld(scene);
const grass = buildGrass(scene, world);
const insects = buildInsects(scene, world);
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
}
updateFrustum();

const composer = new EffectComposer(renderer);
const pixelPass = new RenderPixelatedPass(PIXEL_SIZE, scene, camera, {
  normalEdgeStrength: 0.3,
  depthEdgeStrength: 0.4,
});
// The normal buffer is drawn with this override material (a private field of the pass);
// without the wind its outlines would sit at the blades' rest positions.
addWind(pixelPass._normalMaterial);
composer.addPass(pixelPass);
composer.addPass(new OutputPass());

window.addEventListener('resize', () => {
  renderer.setSize(window.innerWidth, window.innerHeight);
  composer.setSize(window.innerWidth, window.innerHeight);
  updateFrustum();
});

let lastT;
renderer.setAnimationLoop((ms) => {
  const t = ms / 1000;
  const dt = lastT === undefined ? 0 : Math.min(t - lastT, 0.1); // no jumps after a hidden tab
  lastT = t;
  world.update(t);
  grass.update(t);
  sky.update(t);
  insects.update(t, dt, sky.state); // after the sky, so its state is this frame's
  composer.render();
});
