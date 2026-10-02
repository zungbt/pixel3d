import * as THREE from 'three';

const renderer = new THREE.WebGLRenderer();
renderer.setSize(window.innerWidth, window.innerHeight);
document.body.appendChild(renderer.domElement);

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x151729);

const camera = new THREE.PerspectiveCamera(50, window.innerWidth / window.innerHeight, 0.1, 100);
camera.position.set(3, 3, 3);
camera.lookAt(0, 0, 0);

const cube = new THREE.Mesh(new THREE.BoxGeometry(), new THREE.MeshNormalMaterial());
scene.add(cube);

renderer.setAnimationLoop(() => {
  cube.rotation.y += 0.01;
  renderer.render(scene, camera);
});
