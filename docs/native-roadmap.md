# pixel3d native — Rust + wgpu roadmap (approved 2026-10-12)

## Context
The web meadow (three.js, ~2,300 lines in `pixel3d/src/`) costs 400+ MB in the browser (Edge: GPU process 270–300 MB + browser 120–140 MB, flat, not a leak — Chromium baseline; the scene's own GPU data is ~25 MB). User wants a **standalone Windows window app** with much lower RAM. Tauri/Electron ruled out (same engine). Godot maybe later.

Decided with user:
- Rust + wgpu, Windows `.exe`, coded and built in WSL (Ubuntu 26.04, gcc 15 present; no Rust, no mingw yet), run on Windows (user `d`, interop via `powershell.exe` works).
- Lives in `pixel3d/native/`; web version stays as visual/behaviour reference.
- Fidelity: **same feel only** — layout, counts, details may change where native is easier.
- 5 sub-projects; each later gets its own short spec → plan → build, and ends runnable.

## Step 0 — toolchain + day-1 spike (retires most risk)
- Install: rustup (user-level), `rustup target add x86_64-pc-windows-gnu`, `sudo apt install mingw-w64` (**user runs the sudo step**).
- Target `x86_64-pc-windows-gnu` (no MSVC SDK/licence). `native/.cargo/config.toml` sets only the linker `x86_64-w64-mingw32-gcc` — **no default `build.target`**, so `cargo test -p meadow-core` stays a Linux host build.
- DX12 shaders compile with **FXC** (`d3dcompiler_47.dll`, ships with Windows); no `static-dxc` feature (it pulls MSVC-only bits). If FXC rejects our WGSL output, fall back to `WGPU_BACKEND=vulkan`.
- `native/run.sh`: `taskkill.exe /IM meadow.exe /F` (ignore "not found" — a running exe is locked and the copy would fail) → `cargo build --target x86_64-pc-windows-gnu` → `mkdir -p /mnt/c/Users/d/pixel3d` + copy exe → launch via interop. Debug builds keep the console so `log` output is visible.
- Spike (done 2026-10-12, commits on `feat/native-toolchain`): blank winit window + wgpu clear + egui-wgpu hello. Log adapter/backend; check the exe needs no mingw DLLs; measure RAM after 60 s on DX12 and Vulkan; measure frame-cap accuracy at 60/30/15 (Windows timer resolution); minimise/restore without panic. Pin the wgpu version that current egui-wgpu uses → wgpu 30 + winit 0.30 (egui 0.36). Passing env vars to the exe from WSL: `WSLENV=NAME` (not `NAME/u`).
- **Exit:** exe runs on Windows; baseline RAM + cap accuracy recorded → sets the real RAM target.

## Architecture (workspace `native/`)
- **`meadow-core`** (lib, no GPU deps, `cargo test` on Linux): `rng` (mulberry32/rngFor), `motion`, `world` (heightAt, path SDF, placements, obstacles), `grass`, `flowers`, `sky`, `sun`, `clock`, `settings`, `frame_pacer`, `insects/*`. Sim outputs plain structs; tests read those (fixes the JS sim/render coupling). Time-dependent functions take an explicit UTC offset / zone, never `Local`, so tests don't depend on the machine's TZ.
- **`meadow`** (bin): winit 0.30 `ApplicationHandler`; `gpu`, `camera`, `mesh` (hand-built flat primitives), `render`, `post`, `ui`, `shaders/*.wgsl`.
- Crates: wgpu, winit, glam, bytemuck, pollster, log + env_logger; egui + egui-wgpu + egui-winit (sub-project 5); jiff (sun/clock: system zone → IANA, local time; already in the lock via env_logger). Settings: `key=value` file in `%APPDATA%\pixel3d\`, no serde.
- Shaders: `common.wgsl` (noise, cloud shade, ground fog, 4-step toon, 5-tap PCF) prepended via `include_str!`. Everything drawn instanced (mat4 + colour + wind factor). Fixed pipelines: shadow, toon, water, unlit (opaque/alpha), post, egui. Tree sway moves to the vertex shader.

## Frame loop (traced — differs from the web on purpose)
- The web limiter (`src/frame-limiter.js`) *filters rAF ticks*. Natively there is no tick stream: skipping a frame under `Fifo` means no `get_current_texture` → no vsync block → the loop spins at 100 % CPU. So **don't port it 1:1**: `frame_pacer` returns the next deadline (`last + 1/fps`, same 1 ms early-tolerance and 250 ms stall reset, same "step along the schedule" rule) and the app sleeps with `ControlFlow::WaitUntil(deadline)` then `request_redraw`. Native (no cap) = `PresentMode::Fifo`, redraw every frame. `parseFps` ports 1:1; limiter tests are rewritten as deadline-sequence tests with the same expectations (≈ frame counts per second, stall reset).
- `dt` from `Instant`, clamped to 0.1 s, 0 on the first frame (as `main.js`).
- **Minimised** (size 0): stop requesting redraws; the restore `Resized` wakes the loop (dt clamp absorbs the gap). **Occluded**: winit never sends `WindowEvent::Occluded` on Windows, so rely on wgpu's `CurrentSurfaceTexture::Occluded` and retry every 250 ms (spike-verified). The browser did this for free; native must do it or it burns GPU in the background.
- **Surface errors** (this is the native answer to the web's "context lost → reload", the suspected Lively crash path): `Outdated`/`Lost` → reconfigure surface and skip the frame; `Timeout` → skip; `OutOfMemory` → log and exit cleanly. Never reload/rebuild the scene.
- **Resize**: ignore size 0 (configuring a 0×0 surface panics); art size = `max(1, floor(physical / pixelSize))` (winit physical size already includes DPR); rebuild art targets + grass view cull.

## Render pipeline (art resolution)
1. Shadow: Depth32 2048², ortho ±22, front-face culling for closed casters (trees, rocks, clock) as the web's shadowSide does; grass/flowers don't cast.
2. Scene pass, **MRT**: colour Rgba16F + view-normal Rgba8 + Depth32 (replaces the web's second full scene redraw for normals). Normal target is cleared to a fixed "up" normal (web clears it to the background colour; the ground fills the view, so only edge cases differ).
   - **Grass/flowers write the un-flipped normal** to the normal target. (Traced: grass normals are all (0,1,0); flipping back faces via `front_facing` would write (0,−1,0) and outline every blade. The web's normal pass is FrontSide-only, so back faces never reach it.) Lighting still flips for back faces, as three's DoubleSide does.
   - Fireflies (alpha, no depth write) use `write_mask = empty` on the normal target.
3. Post, one fullscreen triangle at swapchain res: art pixel = `min(floor(screen_px / pixelSize), art − 1)` (not `uv·art`, so every art pixel is exactly pixelSize² screen pixels; the `w % pixelSize` remainder at the right/bottom edge repeats the last pixel) + depth/normal edge (same thresholds; orthographic near/far 0.1/100 kept so depth stays linear in [0,1]) + output to an `*Srgb` swapchain format (fallback: apply sRGB OETF in the shader if none offered).
- Grass culled/sorted to view on resize only. RAM levers if needed: 1024² Depth16 shadow, Rgba8 colour.

## Sub-projects (branch each: `feat/native-<name>` from main)
1. **Static scene** — window, ortho camera (VIEW_HEIGHT 18, yaw π/4, pitch 30°), ground, sand, pond water shader, path, trees, rocks, noon toon lighting + shadows, MRT + outlines, pixel size 2/3/4 (CLI arg), fps cap, frame-loop rules above. Port rng/world/frame_pacer + tests. *Exit:* side-by-side screenshot vs web at 12:00 (grass/insects hidden), whole art pixels on resize, minimise/restore idles the GPU, RAM recorded, `cargo test` green.
2. **Grass + wind** — ~87k blades (count may drop), wind in the scene pass (grass casts no shadow, as the web), up-facing grass normals in MRT. Flowers moved to 4. Spec: `docs/native-sp2-grass.md`. *Exit:* screenshot vs web, wind checked live side by side (no per-blade outline noise), GPU/CPU/RAM delta vs main.
3. **Sky** — day–night colours, sun/moon swap, cloud shadows, ground fog, local-clock mode, NOAA sunrise/sunset from time zone (Windows zone → IANA via iana-time-zone, e.g. "SE Asia Standard Time" → Asia/Bangkok, which the zone table has). Port `sky.test.js` 1:1. Idea from the user: a user-set location (city or lat/lon in settings, picker in 5) overriding the time-zone guess, which can be ~20–30 min off (Hanoi on Asia/Bangkok); GPS dropped (Windows Location, WinRT, permission). *Exit:* screenshots at 06:00 / 12:00 / 18:30 / 02:00 vs web; startup logs zone + rise/set matching web on the same PC.
4. **Flowers + insects** — flowers (`src/flowers.js`: patches, octahedron heads, same wind as the grass; heads are perches), butterflies, dragonflies, fireflies, gnats as pure sim in core; port `test/scene.js` `simulate` (fixed fps, dt 0 on frame 0) + invariant tests against sim state. *Exit:* invariants pass at 60 and 10 fps; visual check at 12:00 and 18:30.
5. **App polish** — egui settings (time, pixel size, fps, fog, hide HUD, show clock), post clock, persisted settings (missing/corrupt file → defaults via ported `parseSetting`; dir created on first save; write failure ignored, as the web does), release exe (`windows_subsystem = "windows"`, lto, strip; release logs go to `%APPDATA%\pixel3d\log.txt` since there's no console). Port settings/clock tests. *Exit:* settings survive restart; final exe size + RAM recorded.

## RAM target & measurement
- Measure after 60 s at 12:00 and 18:30: `powershell.exe "Get-Process meadow | Select WorkingSet64,PrivateMemorySize64"` + Task Manager shared/dedicated GPU memory (iGPU allocs show as shared). One row per milestone in `docs/perf-baseline.md`.
- Baseline to beat: 400+ MB. Spike floor (blank window + egui, Vulkan): 145 MB working set / 80 MB private; DX12 is 142 / 120, so **Vulkan is the default backend**. Target: final ≤ 200 MB working set and ≤ 130 MB private (scene adds ~25–50 MB GPU data). See `docs/perf-baseline.md`.

## Risks → de-risk
- Cross-compiled exe / FXC / backend issues, egui–wgpu version drift, driver RAM floor, frame-cap timer accuracy → all in the day-1 spike.
- Outline/shadow look drift → sub-project 1 side-by-side exit; web dev server stays the reference.
- Windows time zone → IANA mapping → logged at startup (sub-project 3).
- Defender slowing launches → fixed exe path; folder exclusion if needed.

## Self-review (flow trace) — what changed vs the first draft
1. Frame limiter can't be ported 1:1 (tick filter → CPU spin under Fifo) → deadline pacer + `WaitUntil`.
2. MRT grass normals: flipping by `front_facing` would outline every blade → write un-flipped normals.
3. No handling for minimise/occlusion, 0×0 resize, or surface Lost/Outdated → added (the web's reload-on-context-loss is the path we suspected in the Lively crash; native reconfigures instead).
4. `run.sh` copy fails while the old exe runs (Windows file lock) → kill first, `mkdir -p`.
5. Default `build.target` would make `cargo test` build Windows test binaries → linker-only config.
6. DX12 shader compiler on the gnu target → FXC, Vulkan fallback.
7. Post-pass `uv·artSize` gives uneven art pixels when width isn't divisible → integer screen→art mapping.
8. Time-zone-dependent core functions take an explicit offset → tests deterministic on any machine.
9. Release exe has no console → file log.

## Process / verification
- Approval of this roadmap = go for Step 0 only. Each sub-project: short spec, approval, then build. Roadmap itself committed as `docs/native-roadmap.md` on `docs/native-roadmap`. Commits one idea each, `merge --no-ff`, push only on user OK, no co-author trailer (user rule).
- Every milestone verified with: `cargo test -p meadow-core` (WSL), exe launched on Windows via `run.sh`, screenshot vs `npm run dev` at the listed hours, RAM row added.
