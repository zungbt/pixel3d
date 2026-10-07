// Perf baseline, enabled with ?bench. For each hour: warm up, then time every frame's stages,
// count draw calls across all passes, and force a GPU sync (1-pixel readPixels) so "gpu" is the
// wait for the frame to finish. Results go to window.__bench and the console.
const HOURS = [12, 18.5, 2]; // day; dusk with fireflies + gnats; night
const WARMUP = 120;
const FRAMES = 600;

const pct = (xs, p) => {
  const s = [...xs].sort((a, b) => a - b);
  return +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(3);
};

export function createBench(renderer, setHour) {
  const gl = renderer.getContext();
  const px = new Uint8Array(4);
  const dbg = gl.getExtension('WEBGL_debug_renderer_info');
  renderer.info.autoReset = false; // the composer renders several passes per frame; count them all

  const results = {
    gpu: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER),
    canvas: [gl.drawingBufferWidth, gl.drawingBufferHeight],
    dpr: devicePixelRatio,
    runs: [],
  };
  let hourIndex = 0;
  let frame = 0;
  let samples;
  let t0;
  let last;
  let prevStart;
  setHour(HOURS[0]);

  return {
    begin() {
      renderer.info.reset();
      t0 = last = performance.now();
      if (frame === WARMUP) samples = { interval: [], gpu: [], total: [] };
      if (samples && prevStart !== undefined) samples.interval.push(t0 - prevStart);
      prevStart = t0;
    },
    mark(name) {
      const now = performance.now();
      if (samples) (samples[name] ??= []).push(now - last);
      last = now;
    },
    end() {
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
      this.mark('gpu');
      if (samples) samples.total.push(performance.now() - t0);
      if (++frame < WARMUP + FRAMES) return;

      const { render, programs, memory } = renderer.info;
      const run = { hour: HOURS[hourIndex], calls: render.calls, triangles: render.triangles,
        points: render.points, lines: render.lines, programs: programs.length, ...memory, ms: {} };
      for (const [k, xs] of Object.entries(samples)) run.ms[k] = { p50: pct(xs, 0.5), p95: pct(xs, 0.95) };
      results.runs.push(run);
      console.log('[bench]', JSON.stringify(run));

      frame = 0;
      samples = undefined;
      prevStart = undefined;
      if (++hourIndex < HOURS.length) setHour(HOURS[hourIndex]);
      else {
        window.__bench = results;
        console.log('[bench] done', JSON.stringify(results));
      }
    },
  };
}
