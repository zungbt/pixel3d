import * as THREE from 'three';
import { heightAt } from './world.js';
import { pick, rngFor, turn } from './motion.js';
import { windSway } from './grass.js';
import { HEAD_R, HEAD_Y } from './flowers.js';

const COUNT = 12;
const COLORS = [0xf3f0e6, 0xf0d860, 0xe8964a, 0x8fb4ee]; // cabbage white, brimstone, orange tip, blue
const WING_W = 0.12; // one wing: half the ~0.25 wingspan, so ~4 art pixels across
const WING_L = 0.1;
const ACCEL = 3;
const FOLDED = Math.PI / 2 - 0.08; // wings closed together over the back
const NEAR = 3; // usual hop to the next flower

// A flat wing hinged on the body axis (local z), extending to one side.
function wingGeometry(side) {
  const geo = new THREE.PlaneGeometry(WING_W, WING_L);
  geo.rotateX(-Math.PI / 2);
  geo.translate((side * WING_W) / 2, 0, 0);
  return geo;
}

// Day: erratic flower-to-flower flight, feeding, the odd bask on a warm rock.
// When the light fades each one settles on a flower with its wings closed until morning.
export function buildButterflies(scene, ctx) {
  const gradientMap = ctx.world.gradientMap;
  const heads = ctx.flowers.heads.filter((h) => ctx.inRoam(h.x, h.z));
  const rocks = ctx.world.rocks.filter((r) => ctx.inRoam(r.position.x, r.position.z));

  const bodyGeo = new THREE.BoxGeometry(0.03, 0.03, 0.12);
  const wingGeo = [wingGeometry(-1), wingGeometry(1)];
  const bodyMat = new THREE.MeshToonMaterial({ color: 0x3a3328, gradientMap });
  const wingMats = COLORS.map((color) => new THREE.MeshToonMaterial({ color, gradientMap, side: THREE.DoubleSide }));

  const flies = [];
  for (let i = 0; i < COUNT; i++) {
    const rand = rngFor(2024, i); // its own stream: one butterfly's choices never shift another's
    const group = new THREE.Group();
    group.name = 'butterfly';
    const left = new THREE.Mesh(wingGeo[0], wingMats[i % COLORS.length]);
    const right = new THREE.Mesh(wingGeo[1], wingMats[i % COLORS.length]);
    group.add(new THREE.Mesh(bodyGeo, bodyMat), left, right);
    scene.add(group);
    flies.push({
      group, left, right, rand,
      pos: new THREE.Vector3(),
      vel: new THREE.Vector3(),
      state: null, // fly | feed | bask | roost; toBask / toRoost while flying there
      perch: null, // the flower head or rock being flown to or sat on
      timer: 0,
      alt: 0.4 + rand() * 0.5, // cruising height above the ground
      threshold: 0.4 + rand() * 0.2, // daylight below this sends it to roost
      wander: rand() * Math.PI * 2,
      flap: rand() * 10,
      speed: 0.55 + rand() * 0.3,
      flapHz: 7.5 + rand() * 3,
      bask: 0.05 + rand() * 0.15, // chance of basking on a rock after feeding
      yaw: 0, // body heading, swung round by turn()
      yawVel: 0,
    });
  }

  const randomHead = (rand) => heads[Math.floor(rand() * heads.length)];
  function headNear(p, maxDist, rand) {
    const near = heads.filter((h) => Math.hypot(h.x - p.x, h.z - p.z) < maxDist);
    return near.length ? near[Math.floor(rand() * near.length)] : randomHead(rand);
  }
  function nearestHead(p) {
    let best = heads[0];
    for (const h of heads) if (Math.hypot(h.x - p.x, h.z - p.z) < Math.hypot(best.x - p.x, best.z - p.z)) best = h;
    return best;
  }

  // Where a perched butterfly sits: on the swaying flower head, or on top of the rock.
  const _perch = new THREE.Vector3();
  function perchPoint(perch, t) {
    if (perch.isObject3D) return ctx.rockTop(perch, _perch);
    const [sx, sz] = windSway(t, perch.x, perch.z, HEAD_Y);
    return _perch.set(perch.x + sx, perch.y + HEAD_R, perch.z + sz);
  }

  function flyTo(b, state, perch) {
    b.state = state;
    b.perch = perch;
    b.timer = 15; // give up and pick again if it can't get there
  }

  function nextAfterFeeding(b) {
    const warmRocks = rocks.filter((rock) => rock.position.distanceTo(b.pos) < 5);
    const next = pick(b.rand, { bask: warmRocks.length ? b.bask : 0, near: 0.73, far: 0.15 });
    if (next === 'bask') flyTo(b, 'toBask', warmRocks[Math.floor(b.rand() * warmRocks.length)]);
    else flyTo(b, 'fly', headNear(b.pos, next === 'near' ? NEAR : 8, b.rand));
  }

  const _aim = new THREE.Vector3();
  function fly(b, t, dt) {
    const target = perchPoint(b.perch, t);
    const dist = b.pos.distanceTo(target);
    // Cruise at its own height, drop to the target over the last stretch,
    // and wander side to side the whole way (the zig-zag that makes butterflies hard to catch).
    _aim.copy(target);
    if (dist > 0.8) _aim.y = Math.max(target.y, heightAt(b.pos.x, b.pos.z) + b.alt);
    b.wander += (b.rand() - 0.5) * 8 * dt;
    const jitter = 0.6 * Math.min(1, dist);
    _aim.x += Math.cos(b.wander) * jitter;
    _aim.z += Math.sin(b.wander) * jitter;
    ctx.steer(b.pos, b.vel, _aim, b.speed, ACCEL, dt);
    ctx.avoid(b.pos, b.vel, dt, 0.15, b.perch);
    b.pos.addScaledVector(b.vel, dt);
    b.timer -= dt;
    return dist < 0.06;
  }

  function land(b) {
    b.vel.set(0, 0, 0);
    b.yawVel = 0;
    if (b.state === 'toRoost') b.state = 'roost';
    else if (b.state === 'toBask') (b.state = 'bask'), (b.timer = 4 + b.rand() * 6);
    else (b.state = 'feed'), (b.timer = 2 + b.rand() * 6);
  }

  return {
    update(t, dt, sky) {
      for (const b of flies) {
        const dark = sky.daylight < b.threshold;
        if (b.state === null) {
          // First frame: start wherever this time of day would have left it.
          b.perch = randomHead(b.rand);
          b.pos.copy(perchPoint(b.perch, t));
          if (dark) b.state = 'roost';
          else flyTo(b, 'fly', headNear(b.pos, NEAR, b.rand));
        }

        if (b.state === 'roost') {
          if (sky.daylight > b.threshold + 0.05) flyTo(b, 'fly', headNear(b.pos, NEAR, b.rand));
        } else if (dark && b.state !== 'toRoost') {
          flyTo(b, 'toRoost', nearestHead(b.pos));
        }

        const flying = b.state === 'fly' || b.state === 'toBask' || b.state === 'toRoost';
        if (flying) {
          if (fly(b, t, dt)) land(b);
          else if (b.timer <= 0) flyTo(b, b.state === 'toRoost' ? 'toRoost' : 'fly', headNear(b.pos, 8, b.rand));
        } else {
          b.pos.copy(perchPoint(b.perch, t));
          if (b.state !== 'roost' && (b.timer -= dt) <= 0) nextAfterFeeding(b);
        }

        // Wings: a fast beat in flight (the body bobs with each stroke), a slow open-close
        // while feeding, flat open when basking, folded shut at roost.
        let wing;
        b.group.position.copy(b.pos);
        if (flying) {
          b.flap += dt * b.flapHz * Math.PI * 2;
          wing = 0.6 + 0.75 * Math.sin(b.flap);
          b.group.position.y += Math.sin(b.flap) * 0.03;
          // The body swings round toward its flight on a spring (at most 6 rad/s), never snapping.
          if (b.vel.lengthSq() > 0.0025) turn(b, Math.atan2(b.vel.x, b.vel.z), dt, 30, 10, 6);
        } else if (b.state === 'feed') {
          wing = 0.9 + 0.6 * Math.sin(t * 1.5 + b.wander);
        } else if (b.state === 'bask') {
          wing = 0.05;
        } else {
          wing = FOLDED;
        }
        b.group.rotation.y = b.yaw;
        b.left.rotation.z = -wing;
        b.right.rotation.z = wing;
      }
    },
  };
}
