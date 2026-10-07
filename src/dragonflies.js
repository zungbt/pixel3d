import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { heightAt, mulberry32, pondEdge, pondPoint } from './world.js';

const COLORS = [0x3f7fd0, 0x4f9e5a, 0xc4533a]; // emperor blue, hawker green, common darter red
const BODY_L = 0.35; // ~5 art pixels
const WING_L = 0.16;
const WING_W = 0.06;
const DART_SPEED = 4;
const DART_ACCEL = 30; // reaches full speed in ~0.13 s
const SECTOR = 0.9; // half-width (radians) of each territory around the rim
const FLICK = 0.35; // wing angle, alternating sign every frame in flight

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
// rival that comes within a body length or so. At dusk it drops into the grass near the shore.
export function buildDragonflies(scene, ctx) {
  const rand = mulberry32(77);
  const gradientMap = ctx.world.gradientMap;
  const shoreRocks = ctx.world.rocks.filter((r) => pondEdge(r.position.x, r.position.z) < 2.5);

  const bodyGeo = new THREE.BoxGeometry(0.04, 0.04, BODY_L).translate(0, 0, -0.05);
  const wingGeo = [wingsGeometry(-1), wingsGeometry(1)];
  const wingMat = new THREE.MeshToonMaterial({ color: 0xe4eef4, gradientMap, side: THREE.DoubleSide });

  const flies = COLORS.map((color, i) => {
    const group = new THREE.Group();
    const left = new THREE.Mesh(wingGeo[0], wingMat);
    const right = new THREE.Mesh(wingGeo[1], wingMat);
    group.add(new THREE.Mesh(bodyGeo, new THREE.MeshToonMaterial({ color, gradientMap })), left, right);
    scene.add(group);
    const home = (i / COLORS.length) * Math.PI * 2 + 0.4;
    const [hx, hz] = pondPoint(home, 0);
    return {
      group, left, right,
      pos: new THREE.Vector3(),
      vel: new THREE.Vector3(),
      target: new THREE.Vector3(),
      state: null, // dart | hover | toPerch | perch | toSettle | settle
      rock: null, // the rock being flown to or perched on
      timer: 0,
      cooldown: 0, // after a rival chase, so the pair doesn't re-trigger every frame
      flick: false,
      home,
      rocks: shoreRocks.filter((r) => Math.hypot(r.position.x - hx, r.position.z - hz) < 3),
      threshold: 0.45 + rand() * 0.1, // daylight below this sends it into the grass
    };
  });

  const territoryAngle = (d) => d.home + (rand() * 2 - 1) * SECTOR;

  // Mostly over the water near the rim, a little over the bank.
  function dart(d) {
    const [x, z] = pondPoint(territoryAngle(d), -1.2 + rand() * 1.8);
    d.target.set(x, heightAt(x, z) + 0.4 + rand() * 0.4, z);
    d.state = 'dart';
    d.rock = null;
    d.timer = 6; // give up and hover if it can't get there
  }

  function toPerch(d) {
    d.rock = d.rocks.length ? d.rocks[Math.floor(rand() * d.rocks.length)] : null;
    if (d.rock) ctx.rockTop(d.rock, d.target);
    else {
      const [x, z] = pondPoint(territoryAngle(d), 0.1);
      d.target.set(x, heightAt(x, z) + 0.15, z);
    }
    d.state = 'toPerch';
    d.timer = 6;
  }

  // In the grass 1-3 units back from the shore, clear of trunks, crowns and rocks.
  function toSettle(d) {
    for (let i = 0; i < 10; i++) {
      const [x, z] = pondPoint(territoryAngle(d), 1 + rand() * 2);
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
    d.timer = 0.5 + rand() * 1.5;
  }

  // Straight-line flight to d.target; true on arrival.
  function flyTo(d, dt) {
    ctx.steer(d.pos, d.vel, d.target, DART_SPEED, DART_ACCEL, dt);
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
    _aim.set(d.target.x + (rand() - 0.5) * 0.1, d.target.y + (rand() - 0.5) * 0.06, d.target.z + (rand() - 0.5) * 0.1);
    ctx.steer(d.pos, d.vel, _aim, 0.4, 6, dt);
    ctx.avoid(d.pos, d.vel, dt, 0.15, null);
    d.pos.addScaledVector(d.vel, dt);
  }

  const flying = (d) => d.state !== 'perch' && d.state !== 'settle';

  return {
    update(t, dt, sky) {
      for (const d of flies) {
        const dark = sky.daylight < d.threshold;
        if (d.state === null) {
          // First frame: start wherever this time of day would have left it.
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
              if (rand() < 0.25) toPerch(d);
              else dart(d);
            }
            break;
          case 'toPerch':
            if (flyTo(d, dt)) (d.state = 'perch'), (d.timer = 3 + rand() * 7);
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
        if (d.vel.lengthSq() > 0.09) d.group.rotation.y = Math.atan2(d.vel.x, d.vel.z);
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
