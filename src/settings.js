import { parseFps } from './frame-limiter.js';

// Viewer settings, one localStorage key each (pixel3d.<name>). Every stored string is checked,
// so a missing or unexpected value falls back to the default.
export const PIXEL_SIZES = [2, 3, 4]; // device px per art pixel

const PARSE = {
  pixelSize: (v) => (PIXEL_SIZES.includes(Number(v)) ? Number(v) : 2),
  fps: (v) => (v === '' ? null : (parseFps(v) ?? 15)), // '' is Native (null: no cap); unset means 15
  clock: (v) => v === 'true', // the sky follows the local clock instead of the slider
  hideHud: (v) => v === 'true',
  showClock: (v) => v !== 'false', // the post clock beside the path
};

export function parseSetting(name, value) {
  return PARSE[name](value);
}

// Storage access throws when the browser blocks it: then settings just aren't remembered.
function defaultStorage() {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export function loadSettings(storage = defaultStorage()) {
  const settings = {};
  for (const name of Object.keys(PARSE)) {
    let value = null;
    try {
      value = storage?.getItem(`pixel3d.${name}`) ?? null;
    } catch {}
    settings[name] = parseSetting(name, value);
  }
  return settings;
}

export function saveSetting(name, value, storage = defaultStorage()) {
  try {
    storage?.setItem(`pixel3d.${name}`, String(value ?? ''));
  } catch {}
}
