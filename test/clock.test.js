import { test } from 'node:test';
import assert from 'node:assert/strict';
import { handAngles } from '../src/clock.js';

const TURN = Math.PI * 2;

test('hands point at the local time, clockwise from 12', () => {
  for (const [h, m, hour, minute] of [
    [0, 0, 0, 0],
    [3, 0, 0.25, 0],
    [6, 30, 6.5 / 12, 0.5],
    [15, 0, 0.25, 0], // 12-hour dial
    [23, 45, 11.75 / 12, 0.75],
  ]) {
    const a = handAngles(new Date(2026, 9, 7, h, m));
    assert.ok(Math.abs(a.hour - hour * TURN) < 1e-9, `${h}:${m} hour hand ${a.hour}`);
    assert.ok(Math.abs(a.minute - minute * TURN) < 1e-9, `${h}:${m} minute hand ${a.minute}`);
  }
});
