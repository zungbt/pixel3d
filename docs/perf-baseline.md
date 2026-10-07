# Perf baseline

Measured 2026-10-07 on `origin/main` @ 95b11cd, before any optimisation.

- **Machine:** WSL2, Intel UHD Graphics (iGPU) through ANGLE → D3D12 Mesa, headless Chrome.
- **Canvas:** 1920×1080, devicePixelRatio 1, FPS cap Native (vsync 60 Hz).
- **Method:** `?bench` (src/bench.js). For each hour: 120 warm-up frames, then 600 measured frames.
- **Run it:** `npm run dev`, then `node scripts/bench.mjs`.

Columns:
- **total:** CPU work plus waiting for the GPU (a 1-pixel `readPixels` forces the sync).
- **gpu:** the time spent waiting for that sync.
- **render:** JS time inside `composer.render()`.
- **calls / tris:** summed over all passes (shadow, pixel colour, pixel normal, output).

Runs 1, 3 and 4 agree. Run 2 came out about 1.8× slower across the board (total p50 about 22 ms). It was most likely iGPU clocking or load on the Windows side, so it is left out. To compare against this baseline, run the bench at least twice.

| hour | calls | tris | total p50 / p95 (ms) | gpu p50 / p95 | render p50 | insects p50 | world+grass+sky+hud |
|---|---|---|---|---|---|---|---|
| 12:00 day | 946 | 238k | 12.0–12.8 / 15–17.5 | 7.9–8.4 / 10 | 3.3–3.5 | 0.4 | < 0.3 |
| 18:30 dusk (fireflies, gnats) | 942 | 238k | 12.0–12.2 / 15.4–16.1 | 7.9 / 10 | 3.3–3.4 | 0.5 | < 0.3 |
| 02:00 night | 946–948 | 238k | 12.1–13.4 / 15–22 | 8.0–9.2 / 16 | 3.3–3.6 | 0.4 | < 0.3 |

## What it says

- The frame interval is pinned at 16.7 ms (vsync). Real work is about 12 ms p50. The p95 is 15–22 ms, so the slow frames already miss 60 fps, most often at night.
- **GPU is about 65% of the frame** (about 8 ms) and is the main target. Each frame draws the scene three times: shadow map 2048², pixel colour and pixel normal.
- **Draw-call submission is about 3.4 ms of CPU** for about 945 calls. Trees and rocks are separate meshes, not instanced.
- Simulation is cheap: insects about 0.5 ms, everything else under 0.3 ms.
