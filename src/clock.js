import * as THREE from 'three';
import { heightAt } from './world.js';

// A park post clock beside the path, showing the viewer's local time (the browser's clock).
const X = -3.25;
const Z = -2.75; // open grass left of the path, no tree within 2.8 units
const POST_H = 1.6;
const R = 0.5; // face radius before SCALE
const SCALE = 1.3; // face ~40 art pixels across at pixel size 2 on 1080p

// Hand angles in radians, clockwise from 12.
export function handAngles(date) {
  const m = date.getMinutes() + date.getSeconds() / 60;
  return {
    hour: (((date.getHours() % 12) + m / 60) / 12) * Math.PI * 2,
    minute: (m / 60) * Math.PI * 2,
  };
}

// A box pivoting at its base, pointing up (12 o'clock) before rotation.
function hand(length, width, z, material) {
  const geo = new THREE.BoxGeometry(width, length, 0.02);
  geo.translate(0, length / 2 - 0.05, 0);
  const mesh = new THREE.Mesh(geo, material);
  mesh.position.set(0, POST_H, z);
  return mesh;
}

export function buildClock(scene, world) {
  const gradientMap = world.gradientMap;
  const iron = new THREE.MeshToonMaterial({ color: 0x3a4044, gradientMap });
  const ink = new THREE.MeshToonMaterial({ color: 0x23262b, gradientMap });
  // The face glows a warm cream, brighter after sunset like a lit street clock.
  const face = new THREE.MeshToonMaterial({ color: 0xf3ead2, emissive: 0xffd890, emissiveIntensity: 0, gradientMap });

  const clock = new THREE.Group();
  clock.position.set(X, heightAt(X, Z), Z);
  clock.rotation.y = Math.PI / 4; // face (+z) toward the camera
  clock.scale.setScalar(SCALE);

  const post = new THREE.Mesh(new THREE.CylinderGeometry(0.06, 0.09, POST_H, 6), iron);
  post.position.y = POST_H / 2;
  const rim = new THREE.Mesh(new THREE.CylinderGeometry(R + 0.07, R + 0.07, 0.14, 24), iron);
  rim.rotation.x = Math.PI / 2;
  rim.position.y = POST_H;
  const dial = new THREE.Mesh(new THREE.CircleGeometry(R, 24), face);
  dial.position.set(0, POST_H, 0.071);
  const parts = [post, rim, dial];

  // A tick per hour; 12, 3, 6 and 9 are bigger.
  const bigTick = new THREE.BoxGeometry(0.07, 0.12, 0.02);
  const smallTick = new THREE.BoxGeometry(0.045, 0.07, 0.02);
  for (let i = 0; i < 12; i++) {
    const a = (i * Math.PI) / 6;
    const tick = new THREE.Mesh(i % 3 ? smallTick : bigTick, ink);
    tick.position.set(Math.sin(a) * (R - 0.1), POST_H + Math.cos(a) * (R - 0.1), 0.08);
    tick.rotation.z = -a;
    parts.push(tick);
  }

  const hourHand = hand(0.3, 0.08, 0.09, ink);
  const minuteHand = hand(0.42, 0.06, 0.1, ink);
  parts.push(hourHand, minuteHand);

  for (const part of parts) {
    part.castShadow = true;
    part.receiveShadow = true;
    clock.add(part);
  }
  scene.add(clock);

  return {
    object: clock,
    update(date, daylight) {
      const { hour, minute } = handAngles(date);
      hourHand.rotation.z = -hour; // clockwise seen from the front
      minuteHand.rotation.z = -minute;
      face.emissiveIntensity = 0.25 + 0.25 * (1 - daylight); // faces away from the sun, so a little by day too
    },
  };
}
