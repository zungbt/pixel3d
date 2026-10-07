import * as THREE from 'three';
import { heightAt, mulberry32, pathEdge, pondEdge, pondPoint } from './world.js';

const MALES = 28;
const FEMALES = 10;
const LONERS = 20; // males that drift across the meadow on their own, away from the swarms
const SIZE = 0.075; // about one art pixel
const SPEED = 0.15;
const FLASH = 1; // seconds lit, fading in and out
const PERIOD = 5.5; // seconds between a male's flashes
const ANSWER_DELAY = 2;
const ANSWER_DIST = 2;
const GLOW = new THREE.Color(0xd8ff6a);
const CLUSTERS = 6;
const CLUSTER_R = 3; // half-width of the square each swarm keeps to
const LONER_STEP = 3; // half-width of the square a loner picks its next spot in, around where it is
const LONER_GAP = 4; // loners start at least this far apart, so they cover the meadow

// Dusk to midnight, in a few loose swarms: males drift low around their swarm, each giving a short rising ("J") flash
// every few seconds; females wait on grass tips and answer a nearby male about 2 s later,
// and he turns toward her. A few lone males wander the whole meadow instead. By day all of
// them are hidden in the grass.
export function buildFireflies(scene, ctx) {
  const rand = mulberry32(99);

  // Grass-tip height over open ground: in view, off the water and the path, clear of trees and rocks.
  function isOpen(out, x, z) {
    if (!ctx.inRoam(x, z) || pondEdge(x, z) < 0.3 || pathEdge(x, z) < 0.2) return false;
    out.set(x, heightAt(x, z) + 0.25, z);
    return !ctx.obstacles.some((o) => o.c.distanceTo(out) < o.r + 0.1);
  }

  function openSpot(out, nearX, nearZ, radius) {
    for (let i = 0; i < 10; i++) {
      if (isOpen(out, nearX + (rand() * 2 - 1) * radius, nearZ + (rand() * 2 - 1) * radius)) return true;
    }
    return false;
  }

  // Where they gather: damp grass by the pond, and meadow along forest edges.
  const centres = [];
  const spot = new THREE.Vector3();
  for (let i = 0; i < 500 && centres.length < CLUSTERS; i++) {
    const [x, z] = centres.length ? [(rand() * 2 - 1) * 11, (rand() * 2 - 1) * 11] : pondPoint(rand() * Math.PI * 2, 1.5);
    if (!isOpen(spot, x, z) || centres.some((c) => c.distanceTo(spot) < 6)) continue;
    const tree = Math.min(...ctx.world.trees.map((t) => Math.hypot(t.position.x - x, t.position.z - z)));
    if (centres.length && (tree < 1.5 || tree > 3)) continue;
    centres.push(spot.clone());
  }

  const flies = [];
  for (let i = 0; i < MALES + FEMALES + LONERS; i++) {
    const loner = i >= MALES + FEMALES;
    const home = loner ? null : centres[i % centres.length];
    const pos = new THREE.Vector3();
    if (loner) {
      // Spaced out from the other loners if there's room; after 100 tries, any open spot.
      for (let k = 0; k < 100; k++) {
        while (!openSpot(pos, 0, 0, 11));
        if (!flies.some((o) => !o.home && o.pos.distanceTo(pos) < LONER_GAP)) break;
      }
    } else if (!openSpot(pos, home.x, home.z, CLUSTER_R)) pos.copy(home);
    flies.push({
      male: i < MALES || loner,
      home,
      pos,
      vel: new THREE.Vector3(),
      target: pos.clone(),
      start: 0.48 + (rand() - 0.5) * 0.06, // active window, in sky phase
      end: 0.75 + (rand() - 0.5) * 0.06,
      next: 0, // when the next flash starts (male) or the answer is due (female; 0 = none)
      lit: -1, // when the current flash started
    });
  }
  const females = flies.filter((f) => !f.male);

  // Transparent with no depth write: drawn after every opaque mesh but still hidden behind
  // trees and grass, and never darkened by the pixel pass's depth edges.
  const mesh = new THREE.InstancedMesh(
    new THREE.BoxGeometry(SIZE, SIZE, SIZE),
    new THREE.MeshBasicMaterial({ transparent: true, depthWrite: false }),
    flies.length,
  );
  mesh.name = 'fireflies';
  mesh.frustumCulled = false; // instances move, and start hidden at scale 0
  scene.add(mesh);
  const matrix = new THREE.Matrix4();
  const color = new THREE.Color();

  function wander(f) {
    const [x, z, r] = f.home ? [f.home.x, f.home.z, CLUSTER_R] : [f.pos.x, f.pos.z, LONER_STEP];
    if (openSpot(f.target, x, z, r)) f.target.y += rand() * 0.95; // 0.25-1.2 up
  }

  return {
    update(t, dt, sky) {
      for (const f of flies) {
        f.active = sky.phase > f.start && sky.phase < f.end;
        if (!f.active) {
          f.next = 0;
          f.lit = -1;
          continue;
        }
        if (!f.male) {
          if (f.next && t >= f.next) (f.lit = t), (f.next = 0);
          continue;
        }

        if (!f.next) f.next = t + rand() * PERIOD; // first flash after waking
        if (t >= f.next) {
          f.lit = t;
          f.next = t + PERIOD + (rand() - 0.5) * 1.5;
          // Females within range that aren't already answering someone reply after a pause.
          for (const g of females) {
            if (g.active && !g.next && g.pos.distanceTo(f.pos) < ANSWER_DIST) {
              g.next = t + ANSWER_DELAY + (rand() - 0.5) * 0.6;
              g.suitor = f;
            }
          }
        }

        if (f.pos.distanceTo(f.target) < 0.2) wander(f);
        ctx.steer(f.pos, f.vel, f.target, SPEED, 1, dt);
        if (t - f.lit < FLASH) f.vel.y += 0.4 * dt; // the rising stroke of the J
        ctx.avoid(f.pos, f.vel, dt, 0.2, null);
        f.pos.addScaledVector(f.vel, dt);
      }

      // A male that sees his answer heads for her.
      for (const g of females) {
        if (g.lit === t && g.suitor) g.suitor.target.copy(g.pos).setY(g.pos.y + 0.3);
      }

      flies.forEach((f, i) => {
        const u = (t - f.lit) / FLASH;
        const glow = f.active && f.lit >= 0 && u < 1 ? Math.sin(Math.PI * u) : 0;
        // Fades in and out through dim colours; hidden (scale 0) between flashes, since a dark
        // firefly is invisible against the night.
        const s = glow > 0.1 ? 1 : 0;
        matrix.makeScale(s, s, s).setPosition(f.pos);
        mesh.setMatrixAt(i, matrix);
        mesh.setColorAt(i, color.copy(GLOW).multiplyScalar(glow));
      });
      mesh.instanceMatrix.needsUpdate = true;
      mesh.instanceColor.needsUpdate = true;
    },
  };
}
