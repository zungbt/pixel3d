import * as THREE from 'three';
import { heightAt } from './world.js';
import { buildButterflies } from './butterflies.js';
import { buildDragonflies } from './dragonflies.js';
import { buildFireflies } from './fireflies.js';
import { buildGnats } from './gnats.js';

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
    // Where an insect sits on a rock (its squashed icosahedron's top face).
    rockTop: (rock, out) => out.copy(rock.position).setY(rock.position.y + rock.scale.y * 0.85),

    // Accelerate toward target, slowing down on arrival.
    steer(pos, vel, target, maxSpeed, accel, dt) {
      _d.subVectors(target, pos);
      const dist = _d.length();
      _d.setLength(Math.min(maxSpeed, dist * 2));
      _d.sub(vel).clampLength(0, accel * dt);
      vel.add(_d);
    },

    // Push away from nearby obstacles (except `ignore`, e.g. the rock being landed on),
    // hard-project out of any being flown into, and keep `minAlt` above the ground.
    // Something taking off from a rock top (inside its sphere) is just pushed out, without a jump.
    avoid(pos, vel, dt, minAlt, ignore) {
      for (const o of obstacles) {
        if (o.src === ignore) continue;
        _d.subVectors(pos, o.c);
        const dist = _d.length();
        const margin = o.r + 0.3;
        if (dist >= margin || dist === 0) continue;
        _d.divideScalar(dist);
        vel.addScaledVector(_d, ((margin - dist) / margin) * 12 * dt);
        if (dist < o.r && vel.dot(_d) < 0) pos.copy(o.c).addScaledVector(_d, o.r);
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
  // swarms (gnats) and hunters (dragonflies) are filled by those species and only read in update,
  // so build order doesn't matter.
  const ctx = { world, flowers, swarms: [], hunters: [], ...flightHelpers(world) };
  const species = [
    buildButterflies(scene, ctx),
    buildDragonflies(scene, ctx),
    buildFireflies(scene, ctx),
    buildGnats(scene, ctx),
  ];
  return {
    ctx, // read by the tests
    update(t, dt, sky) {
      for (const s of species) s.update(t, dt, sky);
    },
  };
}
