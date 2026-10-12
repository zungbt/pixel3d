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

## Grass: only the blades on screen

The grass covers the whole 48×48 ground (87,914 blades), but at 16:9 only about half of it is on screen. The camera never moves, so `fitView` sorts the blades by their position on screen once. After that, a resize only changes `mesh.count`. At 16:9, 46,103 blades are drawn (52 %); at 21:9, 66 %.

These numbers come from an A/B run at 1080p: main and this change took turns, three rounds each, giving 9 p50 values per side. The GPU was in its slow phase for these runs (p2 is about 0.3 ms instead of 0.13), so compare the two rows with each other, not with the table above.

| | gpu p50 median (range) | p0 | p1 | p2 | p3 |
|---|---|---|---|---|---|
| main | 6.18 (5.29–6.84) | 2.80 | 2.71 | 0.30 | 0.12 |
| grass culled | 4.89 (4.07–7.65) | 2.25 | 2.19 | 0.28 | 0.11 |

## Native (Rust + wgpu) RAM

Measured on Windows (Intel UHD Graphics, driver 101.6127, 60 Hz screen), release exe from `native/` (x86_64-pc-windows-gnu), window 1280×720. RAM from `Get-Process meadow` after 60 s; CPU is % of one core over 20 s. Web baseline to beat: Edge GPU process 270–300 MB + browser process 120–140 MB.

| commit | change | backend | fps cap | working set (MB) | private (MB) | CPU |
|---|---|---|---|---|---|---|
| spike | blank window + egui label | Vulkan (default) | 60 | 145 | 80 | ~16 % |
| spike | blank window + egui label | DX12 | 60 | 142 | 120 | ~17 % |
| spike | blank window + egui label | Vulkan | none (vsync) / 30 / 15 | – | – | 14.1 / 7.5 / 4.1 % |

CPU scales with frames drawn (~2.3 ms per frame), with or without the `WaitUntil` cap, so it's per-frame render/present cost, not the pacer. Fps cap holds within ±0.5 at 60/30/15.

### Static scene (sub-project 1, T9)

Release exe at `29a9cbc`, window 1280×720 at pixel size 2, noon scene. RAM after 60 s, CPU over the next 20 s (% of one core). "no layers" = `VK_LOADER_LAYERS_DISABLE=~implicit~`, which drops the OBS (`graphics-hook64.dll`) and RivaTuner (`RTSSVkLayer64.dll`) implicit layers this PC loads into every Vulkan app.

| backend | fps cap | fps | working set (MB) | private (MB) | CPU | acquire / encode / submit / present (ms per frame) |
|---|---|---|---|---|---|---|
| Vulkan | 60 | 56–57 | 148 | 103 | 11.4 % | 0.07 / 0.62–0.88 / 0.36–0.62 / 0.20–0.33 |
| Vulkan, no layers | 60 | 54–58 | 142 | 103 | 10.3 % | 0.07 / 0.56–0.86 / 0.40–0.45 / 0.37–0.56 |
| DX12 | 60 | 59–60 | 185 | 167 | 14.8 % | 0.12–0.14 / 1.50–1.73 / 0.51–0.71 / 0.67–1.01 |
| Vulkan | 15 | 15.0 | 142 | 103 | 3.2 % | 0.04–0.05 / 0.33–0.41 / 0.21–0.25 / 0.11–0.16 |
| Vulkan, no layers | 15 | 14.7–15.0 | 140 | 101 | 3.5 % | 0.06 / 0.45–0.47 / 0.26–0.33 / 0.12–0.29 |

Wallpaper mode (`--wallpaper`, 1920×1080, 15 fps, `a60f02e`: Vulkan-only instance + working-set trim on pause): ~147 MB working set / 97–101 MB private while drawing; 2–5 MB working set (same private) while the desktop is covered. Lively + WebView2 on the same PC: ~545 MB working set / ~437 MB private.

### Grass (sub-project 2, T4)

One A/B round, `--wallpaper` at 1920×1080, pixel size 2, 15 fps, Vulkan. It came out invalid: maximised windows covered the desktop, so both exes were paused and trimmed (2 MB working set, 0 % GPU, 0.4–0.6 % CPU). Only the private bytes count, and they didn't move. The user chose not to rerun it.

| exe | private (MB) | grass |
|---|---|---|
| main `557f191` | 101 | – |
| feat/native-grass `84aece6` | 100 | 171,786 placed, 112,210 kept (2.2 MB buffer) |

Drawn: 32,071 at 4:3, 40,155 at 16:10, 45,472 at 16:9, 61,849 at 21:9, the same at pixel sizes 2–4. Placement time on Windows (release): 856 ms with `height_at` per blade (`84aece6`); 204–241 ms (336 ms once, while Vite started alongside) once the roots interpolate the ground mesh's 129×129 vertex grid (`f332cc0`). That also runs again on each Explorer restart.

Findings:
- Within the SP1 target (≤ 200 MB working set, ≤ 130 MB private) on Vulkan; DX12 misses it (+63 MB private, +3.4 % CPU, encode twice as slow). Vulkan stays the default.
- ~2 ms CPU per drawn frame overall (3.2 % at 15 fps), close to the blank spike's 2.3 ms; the timed phases are ~0.8–1.5 ms of it, the rest is the event loop and driver threads. Encode (one shadow, one scene, one post pass) is the biggest phase.
- The OBS and RivaTuner layers cost ~3–6 MB working set and no CPU difference above noise: not worth disabling.
- Validation is off in release (`InstanceFlags::from_build_config()` only sets `VALIDATION_INDIRECT_CALL`; we draw nothing indirect). Debug builds — `run.sh` without `--release` — run full validation, so never measure them.
- At a 60 fps cap Vulkan delivers 54–58 fps while DX12 holds 59–60: the `WaitUntil` deadline sometimes lands just after a vsync under Fifo. Irrelevant for the wallpaper's 15 fps; worth a look only if a 60 fps mode matters.


### App polish (sub-project 5)

Release exe with `lto = true, strip = true`: 7,339,520 bytes (15,395,491 before). Quick reads ~8 s after start, window 1920×1080, Vulkan (not the 60 s measurement):

| exe | scene | working set (MB) | private (MB) |
|---|---|---|---|
| SP4b | noon, pixel size 2 | 188 | 113 |
| SP5 | noon, pixel size 2, post clock | 172–185 | 113–114 |
| SP5 | dusk cycle, pixel size 4, fog always, clock hidden | 155 | 96 |
