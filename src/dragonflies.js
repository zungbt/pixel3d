import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { heightAt, pondEdge, pondPoint } from './world.js';
import { pick, rngFor, turn, wrapAngle } from './motion.js';

const COLORS = [0x3f7fd0, 0x4f9e5a, 0xc4533a]; // emperor blue, hawker green, common darter red
const ROAMERS = [0x4f9e5a, 0xc4533a]; // hawker and darter hunting over the meadow, away from the water
const ROAM_STEP = 4; // half-width of the square a roamer picks its next spot in, around where it is
const BODY_L = 0.35; // ~5 art pixels
const WING_L = 0.16;
const WING_W = 0.06;
const DART_ACCEL = 30; // reaches full speed in ~0.13 s
const SECTOR = 0.9; // half-width (radians) of each territory around the rim
const FLICK = 0.35; // wing angle, alternating sign every frame in flight
const FACING = 0.3; // radians: a straight flight only sets off once the body points this close to its target
const HUNT = 0.35; // chance, after a hover, of a pass through a midge swarm over its own stretch of rim

// Front and hind wing on one side, flat, hinged on the body axis (local z).
function wingsGeometry(side) {
  const wings = [0.09, 0.02].map((z) => {
    const geo = new THREE.PlaneGeometry(WING_L, WING_W);
    geo.rotateX(-Math.PI / 2);
    geo.translate((side * WING_L) / 2, 0, z);
    return geo;
  });
  return mergeGeometries(wings);
}

// Day: each one holds a stretch of the pond rim, patrolling it in fast straight darts with
// hovers between, perching on a shore rock (or the rim) now and then, and chasing off any
// rival that comes within a body length or so. Around dusk and dawn it hawks through the midge
// swarms over its stretch. Before each straight flight it pivots in the air, then shoots off.
// At dusk it drops into the grass near the shore.
// A couple of roamers do the same over the open meadow instead: darts between clear spots near
// where they are, rests on grass tips, and the night in the grass wherever they end up.
export function buildDragonflies(scene, ctx) {
  const gradientMap = ctx.world.gradientMap;
  const shoreRocks = ctx.world.rocks.filter((r) => pondEdge(r.position.x, r.position.z) < 2.5);

  const bodyGeo = new THREE.BoxGeometry(0.04, 0.04, BODY_L).translate(0, 0, -0.05);
  const wingGeo = [wingsGeometry(-1), wingsGeometry(1)];
  const wingMat = new THREE.MeshToonMaterial({ color: 0xe4eef4, gradientMap, side: THREE.DoubleSide });

  const flies = [...COLORS, ...ROAMERS].map((color, i) => {
    const roamer = i >= COLORS.length;
    const rand = rngFor(77, i); // its own stream: one dragonfly's choices never shift another's
    const group = new THREE.Group();
    group.name = 'dragonfly';
    const left = new THREE.Mesh(wingGeo[0], wingMat);
    const right = new THREE.Mesh(wingGeo[1], wingMat);
    group.add(new THREE.Mesh(bodyGeo, new THREE.MeshToonMaterial({ color, gradientMap })), left, right);
    scene.add(group);
    const home = (i / COLORS.length) * Math.PI * 2 + 0.4;
    const [hx, hz] = pondPoint(home, 0);
    return {
      group, left, right, rand,
      pos: new THREE.Vector3(),
      vel: new THREE.Vector3(),
      target: new THREE.Vector3(),
      state: null, // dart | hover | toPerch | perch | toSettle | settle
      rock: null, // the rock being flown to or perched on
      timer: 0,
      cooldown: 0, // after a rival chase, so the pair doesn't re-trigger every frame
      flick: false,
      home: roamer ? null : home,
      rocks: roamer ? [] : shoreRocks.filter((r) => Math.hypot(r.position.x - hx, r.position.z - hz) < 3),
      threshold: 0.45 + rand() * 0.1, // daylight below this sends it into the grass
      dartSpeed: 3.4 + rand() * 1.2,
      hoverScale: 0.7 + rand() * 0.7, // longer or shorter hovers than average
      perchChance: 0.15 + rand() * 0.2,
      yaw: 0, // body heading, swung round by turn()
      yawVel: 0,
    };
  });
  ctx.hunters.push(...flies);

  const territoryAngle = (d) => d.home + (d.rand() * 2 - 1) * SECTOR;

  // Open meadow at height y over the ground: in view, off the water, clear of trees and rocks.
  const _p = new THREE.Vector3();
  function clearAt(x, z, y) {
    if (!ctx.inRoam(x, z) || pondEdge(x, z) < 0.5) return false;
    _p.set(x, heightAt(x, z) + y, z);
    return !ctx.obstacles.some((o) => o.c.distanceTo(_p) < o.r + 0.3);
  }
  // A roamer's next spot near where it is, y over the ground, with a clear straight flight there.
  // `out` is only written when one is found.
  function meadowSpot(d, out, y) {
    for (let i = 0; i < 20; i++) {
      const x = d.pos.x + (d.rand() * 2 - 1) * ROAM_STEP;
      const z = d.pos.z + (d.rand() * 2 - 1) * ROAM_STEP;
      const steps = Math.max(1, Math.ceil(Math.hypot(x - d.pos.x, z - d.pos.z) / 0.25));
      let clear = true;
      for (let j = 1; j <= steps && clear; j++) clear = clearAt(d.pos.x + ((x - d.pos.x) * j) / steps, d.pos.z + ((z - d.pos.z) * j) / steps, y);
      if (!clear) continue;
      out.set(x, heightAt(x, z) + y, z);
      return true;
    }
    return false;
  }

  // Mostly over the water near the rim, a little over the bank; a roamer anywhere open nearby
  // (or nowhere, so it just hovers again).
  function dart(d) {
    if (d.home === null) {
      if (!meadowSpot(d, d.target, 0.4 + d.rand() * 0.4)) d.target.copy(d.pos);
    } else {
      const [x, z] = pondPoint(territoryAngle(d), -1.2 + d.rand() * 1.8);
      d.target.set(x, heightAt(x, z) + 0.4 + d.rand() * 0.4, z);
    }
    d.state = 'dart';
    d.rock = null;
    d.timer = 6; // give up and hover if it can't get there
  }

  // An active midge swarm over its own stretch of rim, if there is one.
  const preyFor = (d) =>
    d.home !== null &&
    ctx.swarms.find((s) => s.visible >= 5 && Math.abs(wrapAngle(s.angle - d.home)) < SECTOR + 0.3);

  // A dart straight through the swarm, coming out 0.8 beyond its centre (so it doesn't hover inside it).
  const _dir = new THREE.Vector3();
  function hunt(d, swarm) {
    _dir.set(swarm.centre.x - d.pos.x, 0, swarm.centre.z - d.pos.z).normalize();
    d.target.copy(swarm.centre).addScaledVector(_dir, 0.8);
    d.state = 'dart';
    d.rock = null;
    d.timer = 6;
  }

  function toPerch(d) {
    d.rock = d.rocks.length ? d.rocks[Math.floor(d.rand() * d.rocks.length)] : null;
    if (d.rock) ctx.rockTop(d.rock, d.target);
    else if (d.home === null) {
      if (!meadowSpot(d, d.target, 0.45)) return dart(d); // a grass tip
    } else {
      const [x, z] = pondPoint(territoryAngle(d), 0.1);
      d.target.set(x, heightAt(x, z) + 0.15, z);
    }
    d.state = 'toPerch';
    d.timer = 6;
  }

  // In the grass 1-3 units back from the shore, clear of trunks, crowns and rocks; a roamer
  // nearby, or straight below if nothing nearby is clear.
  function toSettle(d) {
    if (d.home === null && !meadowSpot(d, d.target, 0.2)) d.target.set(d.pos.x, heightAt(d.pos.x, d.pos.z) + 0.2, d.pos.z);
    for (let i = 0; i < 10 && d.home !== null; i++) {
      const [x, z] = pondPoint(territoryAngle(d), 1 + d.rand() * 2);
      d.target.set(x, heightAt(x, z) + 0.2, z);
      if (!ctx.obstacles.some((o) => o.c.distanceTo(d.target) < o.r + 0.1)) break;
    }
    d.state = 'toSettle';
    d.rock = null;
    d.timer = 6;
  }

  function hover(d) {
    d.target.copy(d.pos);
    d.state = 'hover';
    d.timer = (0.5 + d.rand() * 1.5) * d.hoverScale;
  }

  // Straight-line flight to d.target; true on arrival. Until the body faces the target it
  // brakes instead (steering toward where it already is), so a re-dart never slides sideways.
  function flyTo(d, dt) {
    const dx = d.target.x - d.pos.x;
    const dz = d.target.z - d.pos.z;
    const facing = Math.hypot(dx, dz) < 0.3 || Math.abs(wrapAngle(Math.atan2(dx, dz) - d.yaw)) < FACING;
    ctx.steer(d.pos, d.vel, facing ? d.target : d.pos, d.dartSpeed, DART_ACCEL, dt);
    ctx.avoid(d.pos, d.vel, dt, 0.15, d.rock);
    d.pos.addScaledVector(d.vel, dt);
    d.timer -= dt;
    if (d.pos.distanceTo(d.target) > 0.1) return false;
    d.pos.copy(d.target);
    d.vel.set(0, 0, 0);
    return true;
  }

  const _aim = new THREE.Vector3();
  function hoverStep(d, dt) {
    // Holds station with tiny corrections.
    _aim.set(
      d.target.x + (d.rand() - 0.5) * 0.1,
      d.target.y + (d.rand() - 0.5) * 0.06,
      d.target.z + (d.rand() - 0.5) * 0.1,
    );
    ctx.steer(d.pos, d.vel, _aim, 0.4, 6, dt);
    ctx.avoid(d.pos, d.vel, dt, 0.15, null);
    d.pos.addScaledVector(d.vel, dt);
  }

  const flying = (d) => d.state !== 'perch' && d.state !== 'settle';
  const straight = (d) => d.state === 'dart' || d.state === 'toPerch' || d.state === 'toSettle';

  return {
    update(t, dt, sky) {
      for (const d of flies) {
        const dark = sky.daylight < d.threshold;
        if (d.state === null) {
          // First frame: start wherever this time of day would have left it.
          if (d.home === null) {
            do d.pos.set((d.rand() * 2 - 1) * 11, 0, (d.rand() * 2 - 1) * 11);
            while (!clearAt(d.pos.x, d.pos.z, 0.6));
          }
          if (dark) {
            toSettle(d);
            d.pos.copy(d.target);
            d.state = 'settle';
          } else {
            dart(d);
            d.pos.copy(d.target);
            hover(d);
          }
        }

        if (d.state === 'settle') {
          if (sky.daylight > d.threshold + 0.05) dart(d);
        } else if (dark && d.state !== 'toSettle') {
          toSettle(d);
        }

        d.cooldown -= dt;
        switch (d.state) {
          case 'dart':
            if (flyTo(d, dt) || d.timer <= 0) hover(d);
            break;
          case 'hover':
            hoverStep(d, dt);
            if ((d.timer -= dt) <= 0) {
              const prey = preyFor(d);
              const huntChance = prey ? HUNT : 0;
              const next = pick(d.rand, { dart: 1 - d.perchChance - huntChance, perch: d.perchChance, hunt: huntChance });
              if (next === 'perch') toPerch(d);
              else if (next === 'hunt') hunt(d, prey);
              else dart(d);
            }
            break;
          case 'toPerch':
            if (flyTo(d, dt)) (d.state = 'perch'), (d.timer = 3 + d.rand() * 7);
            else if (d.timer <= 0) dart(d);
            break;
          case 'perch':
            if ((d.timer -= dt) <= 0) dart(d);
            break;
          case 'toSettle':
            if (flyTo(d, dt)) d.state = 'settle';
            else if (d.timer <= 0) toSettle(d);
            break;
        }
      }

      // Rivals that meet in the air both dart off.
      for (let i = 0; i < flies.length; i++) {
        for (let j = i + 1; j < flies.length; j++) {
          const a = flies[i];
          const b = flies[j];
          const patrolling = (d) => d.state === 'dart' || d.state === 'hover';
          if (!patrolling(a) || !patrolling(b) || a.cooldown > 0 || b.cooldown > 0) continue;
          if (a.pos.distanceTo(b.pos) > 1) continue;
          for (const d of [a, b]) dart(d), (d.cooldown = 1.5);
        }
      }

      for (const d of flies) {
        d.group.position.copy(d.pos);
        // The body swings round on a spring (at most 10 rad/s): toward the target on a straight
        // flight, along its drift in a hover, and stays put at rest.
        if (straight(d)) turn(d, Math.atan2(d.target.x - d.pos.x, d.target.z - d.pos.z), dt, 60, 14, 10);
        else if (d.state === 'hover' && d.vel.lengthSq() > 0.09) turn(d, Math.atan2(d.vel.x, d.vel.z), dt, 60, 14, 10);
        d.group.rotation.y = d.yaw;
        // Wings beat too fast to see: alternate up/down every frame in flight, flat out at rest.
        let wing = 0;
        if (flying(d)) {
          d.flick = !d.flick;
          wing = d.flick ? FLICK : -FLICK;
        }
        d.left.rotation.z = -wing;
        d.right.rotation.z = wing;
      }
    },
  };
}
