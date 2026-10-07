import * as THREE from 'three';
import { heightAt } from './world.js';
import { buildButterflies } from './butterflies.js';

const ROAM_R = 13; // the part of the map that is on screen

const _d = new THREE.Vector3();

// Spheres flyers must stay out of: trunk bases, tree crowns and rocks.
function buildObstacles(world) {
  const obstacles = [];
  for (const t of world.trees) {
    const { x, y, z } = t.position;
    obstacles.push({ c: new THREE.Vector3(x, y + 0.3, z), r: 0.3, src: t });
    obstacles.push({ c: new THREE.Vector3(x, y + t.userData.crown, z), r: t.userData.canopy + 0.1, src: t });
  }
  for (const rock of world.rocks) {
    obstacles.push({ c: rock.position.clone(), r: rock.userData.radius + 0.05, src: rock });
  }
  return obstacles;
}

// Shared flight helpers, handed to every species through ctx.
function flightHelpers(world) {
  const obstacles = buildObstacles(world);
  return {
    obstacles,
    inRoam: (x, z) => Math.hypot(x, z) < ROAM_R,

    // Accelerate toward target, slowing down on arrival.
    steer(pos, vel, target, maxSpeed, accel, dt) {
      _d.subVectors(target, pos);
      const dist = _d.length();
      _d.setLength(Math.min(maxSpeed, dist * 2));
      _d.sub(vel).clampLength(0, accel * dt);
      vel.add(_d);
    },

    // Push away from nearby obstacles (except `ignore`, e.g. the rock being landed on),
    // hard-project out of any that were entered, and keep `minAlt` above the ground.
    avoid(pos, vel, dt, minAlt, ignore) {
      for (const o of obstacles) {
        if (o.src === ignore) continue;
        _d.subVectors(pos, o.c);
        const dist = _d.length();
        const margin = o.r + 0.3;
        if (dist >= margin || dist === 0) continue;
        _d.divideScalar(dist);
        vel.addScaledVector(_d, ((margin - dist) / margin) * 12 * dt);
        if (dist < o.r) pos.copy(o.c).addScaledVector(_d, o.r);
      }
      const floor = heightAt(pos.x, pos.z) + minAlt;
      if (pos.y < floor) {
        pos.y = floor;
        if (vel.y < 0) vel.y = 0;
      }
    },
  };
}

// Every insect species; built before the sky so their toon materials get cloud shadows.
export function buildInsects(scene, world, flowers) {
  const ctx = { world, flowers, ...flightHelpers(world) };
  const species = [buildButterflies(scene, ctx)];
  return {
    update(t, dt, sky) {
      for (const s of species) s.update(t, dt, sky);
    },
  };
}
