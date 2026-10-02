import * as THREE from 'three';
import { buildWorld } from './world.js';

const VIEW_HEIGHT = 13; // world units visible vertically

const renderer = new THREE.WebGLRenderer();
renderer.setSize(window.innerWidth, window.innerHeight);
renderer.shadowMap.enabled = true;
document.body.appendChild(renderer.domElement);

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x151729);
buildWorld(scene);

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

window.addEventListener('resize', () => {
  renderer.setSize(window.innerWidth, window.innerHeight);
  updateFrustum();
});

renderer.setAnimationLoop(() => {
  renderer.render(scene, camera);
});
