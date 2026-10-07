import { test } from 'node:test';
import assert from 'node:assert/strict';
import { loadSettings, saveSetting } from '../src/settings.js';

function memoryStorage(entries = {}) {
  const map = new Map(Object.entries(entries));
  return { getItem: (k) => map.get(k) ?? null, setItem: (k, v) => map.set(k, v) };
}

const DEFAULTS = { pixelSize: 2, fps: 15, clock: false, hideHud: false };

test('empty storage gives the defaults', () => assert.deepEqual(loadSettings(memoryStorage()), DEFAULTS));

test('no storage gives the defaults', () => assert.deepEqual(loadSettings(null), DEFAULTS));

test('storage that throws gives the defaults', () => {
  const storage = { getItem: () => { throw new Error('blocked'); }, setItem: () => { throw new Error('blocked'); } };
  assert.deepEqual(loadSettings(storage), DEFAULTS);
  saveSetting('fps', 30, storage); // doesn't throw
});

test('unexpected stored values fall back', () => {
  const storage = memoryStorage({
    'pixel3d.pixelSize': '5',
    'pixel3d.fps': '45',
    'pixel3d.clock': 'yes',
    'pixel3d.hideHud': '1',
  });
  assert.deepEqual(loadSettings(storage), DEFAULTS);
});

test('saved values load back', () => {
  const storage = memoryStorage();
  const settings = { pixelSize: 4, fps: 30, clock: true, hideHud: true };
  for (const [name, value] of Object.entries(settings)) saveSetting(name, value, storage);
  assert.deepEqual(loadSettings(storage), settings);
});

test('no fps cap round-trips', () => {
  const storage = memoryStorage({ 'pixel3d.fps': '30' });
  saveSetting('fps', null, storage);
  assert.equal(loadSettings(storage).fps, null);
});

test('reads the fps cap stored before settings existed', () => {
  assert.equal(loadSettings(memoryStorage({ 'pixel3d.fps': '20' })).fps, 20);
});
