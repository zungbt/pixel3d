// Frame-rate cap: decides which animation-loop ticks get drawn. A lower cap halves (or more)
// the pixel pass's work, which suits a pixel-art scene that doesn't need 144 Hz.
export const FPS_CAPS = [60, 30, 20, 15];

const EARLY = 1; // ms: tick timestamps jitter, so a frame this early still counts as on time
const STALL = 250; // ms: a gap this long (hidden tab, hitch) restarts the schedule

// The cap stored in the selector or localStorage; anything unexpected means no cap (null).
export function parseFps(value) {
  const n = Number(value);
  return FPS_CAPS.includes(n) ? n : null;
}

export function createFrameLimiter() {
  let last = null; // when the last drawn frame was due, ms
  return (now, fps) => {
    if (!fps || last === null) {
      last = now;
      return true;
    }
    const interval = 1000 / fps;
    const elapsed = now - last;
    if (elapsed < interval - EARLY) return false;
    // Step along the ideal schedule rather than jumping to now, so rounding never adds up to drift.
    last = elapsed > STALL ? now : last + Math.max(1, Math.floor((elapsed + EARLY) / interval)) * interval;
    return true;
  };
}
