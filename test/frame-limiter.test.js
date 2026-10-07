import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createFrameLimiter, parseFps } from '../src/frame-limiter.js';
import { rngFor } from '../src/motion.js';

// Frames drawn when the screen ticks at hz for `seconds`, with ±jitter/2 ms timestamp noise.
function drawn(fps, hz, seconds, jitter = 0) {
  const shouldRender = createFrameLimiter();
  const rand = rngFor(5, 0);
  let n = 0;
  for (let i = 0; i < hz * seconds; i++) if (shouldRender((i * 1000) / hz + (rand() - 0.5) * jitter, fps)) n++;
  return n;
}

test('no cap draws every frame', () => assert.equal(drawn(null, 144, 2), 288));

test('caps a 120 Hz screen to 30 fps', () => {
  const n = drawn(30, 120, 10);
  assert.ok(Math.abs(n - 300) <= 1, `${n}`);
});

test('a 60 fps cap on a jittery 60 Hz screen keeps every frame', () => {
  const n = drawn(60, 60, 10, 1);
  assert.ok(n >= 598, `${n}`);
});

test('no drift: 144 Hz capped to 60 for 10 s', () => {
  const n = drawn(60, 144, 10);
  assert.ok(Math.abs(n - 600) <= 2, `${n}`);
});

test('after a long stall it restarts the schedule instead of bursting', () => {
  const shouldRender = createFrameLimiter();
  assert.equal(shouldRender(0, 30), true);
  assert.equal(shouldRender(5000, 30), true);
  assert.equal(shouldRender(5010, 30), false);
  assert.equal(shouldRender(5034, 30), true);
});

test('parseFps: only the offered caps, anything else is native (null)', () => {
  assert.equal(parseFps('60'), 60);
  assert.equal(parseFps('30'), 30);
  assert.equal(parseFps('20'), 20);
  assert.equal(parseFps('15'), 15);
  for (const bad of ['', null, undefined, 'abc', '45', '0', '-30']) assert.equal(parseFps(bad), null, String(bad));
});
