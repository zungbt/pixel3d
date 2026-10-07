# Perf baseline

Measured 2026-10-07 on WSL2, Intel UHD Graphics (iGPU) through ANGLE → D3D12 Mesa, in headless Chrome. Canvas 1920×1080, devicePixelRatio 1, FPS cap Native (vsync 60 Hz).

Method: `?bench` (src/bench.js). For each hour (12:00 day, 18:30 dusk with fireflies and gnats, 02:00 night) it runs 120 warm-up frames, then measures 600 frames. To run it: `npm run dev`, then `node scripts/bench.mjs`.

- **gpu**: GPU time of the whole frame, from `EXT_disjoint_timer_query_webgl2`, one query per `renderer.render` call. Split by pass:
  - **p0**: colour, plus the shadow map
  - **p1**: normal
  - **p2**: pixel composite
  - **p3**: output
- **cpu**: the loop's JS time (update + render submission).
- **render**: the submission part of cpu.
- **calls**: draw calls summed over all passes.

The iGPU is shared with the Windows desktop, so runs vary a lot. One run in three or four comes out 30–50 % slower. The values below are ranges of p50 over the runs, and every comparison needs at least two runs.

> The first version of this file timed the GPU by forcing a sync with a 1-pixel `readPixels`. On this setup that sync costs about 4 ms on its own: an empty scene at 480×270 still "took" 4.3 ms. Those "gpu" numbers were inflated and are dropped here.

| commit | change | calls | gpu p50 (ms) | p0 | p1 | p2 | p3 | cpu p50 | render p50 |
|---|---|---|---|---|---|---|---|---|---|
| 95b11cd | original | ~946 | 5.9–8.3 | 1.4–1.8 | 2.3–2.6 | 1.2–1.8 | 0.8–0.95 | 4.1–4.8 | 3.6–4.1 |
| 26f3afc | instanced trees and rocks | 94 | 5.6–6.5 (one run 9.4) | 1.3–1.5 | 1.7–2.4 | 1.2–1.6 | 0.75–0.83 | 1.7–3.1 | 0.9–1.9 |
| 7573afe | shadow map once per frame | 91 | 5.3–8.9 | 1.4–2.8 | 1.35–2.4 | 1.3–3.2 | 0.77–1.08 | 1.8–2.7 | 1.0–1.7 |
| step 3 | whole pipeline at art resolution | 91 | 3.0–4.9 | 1.27–2.05 | 1.37–1.94 | 0.13–0.22 | 0.05–0.09 | 1.7–2.2 | 0.9–1.2 |

From step 3 on, the drawing buffer is the art resolution (480×270 at 1080p), and the canvas is stretched to the window with `image-rendering: pixelated`. The bench's `canvas` field reports the drawing buffer, and `css` reports the window size.

At 960×540 (`W=960 H=540`), 7573afe compared with step 3:

| commit | gpu p50 | p2 | p3 |
|---|---|---|---|
| 7573afe | 3.85–5.0 | 0.33–0.63 | 0.12–0.23 |
| step 3 | 4.0–6.1 | 0.07–0.13 | 0.04–0.08 |

At 540p, p0 and p1 come out the same as at 1080p, or worse in noisy runs. They don't scale with art resolution, so they are bound by vertices and the fixed-size shadow map, not by fill rate. That is why the total gain shows up at large windows.

What the original numbers show:
- **Grass:** in a one-off A/B run, hiding the grass saved about 0.8 ms in p0 and about 0.5 ms in p1.
- **Shadow map:** it is redrawn by every `renderer.render` of the lit scene, and that includes the normal pass. This accounts for part of p1.
- **Composite and output:** p2 and p3 run at full screen resolution. At 960×540 they drop from about 2.5 ms to about 0.6 ms.
