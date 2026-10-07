import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ease, pick, rngFor, turn, wrapAngle } from '../src/motion.js';

test('rngFor: the same individual always gets the same stream', () => {
  const a = rngFor(7, 3);
  const b = rngFor(7, 3);
  for (let i = 0; i < 5; i++) assert.equal(a(), b());
});

test('rngFor: neighbours get different streams, all in [0, 1)', () => {
  const firsts = [0, 1, 2, 3].map((i) => rngFor(7, i)());
  assert.equal(new Set(firsts).size, 4);
  for (const x of firsts) assert.ok(x >= 0 && x < 1);
});

test('ease: two half steps land where one full step does', () => {
  const once = ease(0, 1, 3, 0.1);
  const twice = ease(ease(0, 1, 3, 0.05), 1, 3, 0.05);
  assert.ok(Math.abs(once - twice) < 1e-12);
  assert.equal(ease(0.4, 1, 3, 0), 0.4);
});

test('wrapAngle: maps into (-PI, PI] and keeps the direction', () => {
  const cases = [[0, 0], [Math.PI, Math.PI], [-Math.PI, Math.PI], [1.5 * Math.PI, -0.5 * Math.PI], [-1.5 * Math.PI, 0.5 * Math.PI], [3.5 * Math.PI, -0.5 * Math.PI]];
  for (const [a, want] of cases) assert.ok(Math.abs(wrapAngle(a) - want) < 1e-9, `${a} -> ${wrapAngle(a)}`);
  const rand = rngFor(3, 0);
  for (let i = 0; i < 1000; i++) {
    const a = (rand() - 0.5) * 100;
    const w = wrapAngle(a);
    assert.ok(w > -Math.PI - 1e-12 && w <= Math.PI + 1e-12);
    assert.ok(Math.abs(Math.sin(w) - Math.sin(a)) < 1e-9 && Math.abs(Math.cos(w) - Math.cos(a)) < 1e-9);
  }
});

test('turn: settles on the target within the rate cap, even at dt 0.1', () => {
  for (const dt of [1 / 60, 0.05, 0.1]) {
    const o = { yaw: 0, yawVel: 0 };
    let maxRate = 0;
    for (let t = 0; t < 3; t += dt) {
      turn(o, 2, dt, 60, 14, 10);
      maxRate = Math.max(maxRate, Math.abs(o.yawVel));
    }
    assert.ok(Math.abs(o.yaw - 2) < 0.01, `dt ${dt}: yaw ${o.yaw}`);
    assert.ok(maxRate <= 10, `dt ${dt}: rate ${maxRate}`);
  }
});

test('turn: takes the short way across ±PI', () => {
  const o = { yaw: 3, yawVel: 0 };
  turn(o, -3, 1 / 60, 30, 10, 6);
  assert.ok(o.yawVel > 0);
});

test('pick: never picks a zero weight, otherwise in proportion', () => {
  const rand = rngFor(1, 0);
  const n = { a: 0, b: 0, c: 0 };
  for (let i = 0; i < 4000; i++) n[pick(rand, { a: 1, b: 3, c: 0 })]++;
  assert.equal(n.c, 0);
  assert.ok(Math.abs(n.b / 4000 - 0.75) < 0.03, `b share ${n.b / 4000}`);
});
