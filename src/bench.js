// Perf baseline, enabled with ?bench. For each hour: warm up, then time every frame's CPU stages,
// count draw calls across all passes, and time each renderer.render call on the GPU with
// EXT_disjoint_timer_query_webgl2 (gpu0..3 = colour + shadow map, normal, pixel composite, output).
// Results go to window.__bench and the console. Without the extension, gpu times are left out.
const HOURS = [12, 18.5, 2]; // day; dusk with fireflies + gnats; night
const WARMUP = 120;
const FRAMES = 600;

const pct = (xs, p) => {
  const s = [...xs].sort((a, b) => a - b);
  return +s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(3);
};

export function createBench(renderer, setHour) {
  const gl = renderer.getContext();
  const dbg = gl.getExtension('WEBGL_debug_renderer_info');
  const timer = gl.getExtension('EXT_disjoint_timer_query_webgl2');
  renderer.info.autoReset = false; // the composer renders several passes per frame; count them all

  const results = {
    gpu: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER),
    gpuTimer: !!timer,
    canvas: [gl.drawingBufferWidth, gl.drawingBufferHeight],
    css: [innerWidth, innerHeight],
    dpr: devicePixelRatio,
    runs: [],
  };
  let hourIndex = 0;
  let frame = 0;
  let samples;
  let t0;
  let last;
  let prevStart;
  let current; // this frame's GPU record: { samples, pending, sum }
  const queries = []; // [query, pass, record], oldest first
  setHour(HOURS[0]);

  if (timer) {
    const render = renderer.render.bind(renderer);
    renderer.render = (scene, camera) => {
      const q = gl.createQuery();
      gl.beginQuery(timer.TIME_ELAPSED_EXT, q);
      render(scene, camera);
      gl.endQuery(timer.TIME_ELAPSED_EXT);
      if (current) {
        queries.push([q, current.pending++, current]);
      } else gl.deleteQuery(q);
    };
  }

  // Results arrive a few frames late; a frame's sum is kept once all of its passes are in.
  function collect() {
    while (queries.length && gl.getQueryParameter(queries[0][0], gl.QUERY_RESULT_AVAILABLE)) {
      const [q, pass, rec] = queries.shift();
      const ms = gl.getQueryParameter(q, gl.QUERY_RESULT) / 1e6;
      gl.deleteQuery(q);
      if (gl.getParameter(timer.GPU_DISJOINT_EXT)) rec.bad = true; // timings unreliable (e.g. clock change)
      if (rec.bad || !rec.samples) continue;
      (rec.samples[`gpu${pass}`] ??= []).push(ms);
      rec.sum += ms;
      if (++rec.done === rec.pending) (rec.samples.gpu ??= []).push(rec.sum);
    }
  }

  return {
    begin() {
      renderer.info.reset();
      t0 = last = performance.now();
      if (frame === WARMUP) samples = { interval: [], cpu: [] };
      if (samples && prevStart !== undefined) samples.interval.push(t0 - prevStart);
      prevStart = t0;
      if (timer) {
        collect();
        current = { samples, pending: 0, done: 0, sum: 0 };
      }
    },
    mark(name) {
      const now = performance.now();
      if (samples) (samples[name] ??= []).push(now - last);
      last = now;
    },
    end() {
      if (samples) samples.cpu.push(performance.now() - t0);
      if (++frame < WARMUP + FRAMES) return;

      const { render, programs, memory } = renderer.info;
      const run = { hour: HOURS[hourIndex], calls: render.calls, triangles: render.triangles,
        points: render.points, lines: render.lines, programs: programs.length, ...memory, ms: {} };
      for (const [k, xs] of Object.entries(samples)) {
        if (xs.length) run.ms[k] = { p50: pct(xs, 0.5), p95: pct(xs, 0.95) };
      }
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
