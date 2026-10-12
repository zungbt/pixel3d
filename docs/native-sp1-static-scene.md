# pixel3d native — sub-project 1: static scene (spec + plan)

## Context
The native Rust + wgpu port of the pixel3d meadow (roadmap `docs/native-roadmap.md`, branch `docs/native-roadmap`) finished its spike: a blank window cross-compiled in WSL runs on Windows at 145 MB WS / 80 MB private (Vulkan) against 400+ MB for the browser. RAM target approved: ≤ 200 MB WS, ≤ 130 MB private.
Sub-project 1 makes the first real scene: ground, paths, sand + pond, trees, rocks, noon toon lighting, shadows, pixel outlines.

Decided with the user:
- **Same feel, not a 1:1 copy** — native may be an improved version. Own toon lighting tuned by eye; web palette and layout kept.
- **Ground improved** (web ground has 3 problems): world edge shows on wide screens; washboard stripes from the two-sine heightmap; faceted/banded flat-shaded triangles. → **smooth gentle hills**: noise-shaped (non-repeating), smooth normals, soft toon band edges, ground larger than the view.
- **Motion included**: tree sway + pond foam/glint animation (GPU-side, cheap).
- **Web `?ref` mode** for side-by-side eye checks (hides grass/flowers/insects/clock/clouds, sun pinned at solar noon).
- egui dropped until sub-project 5. CLI only: `--fps N` (default 15 like web; 0 = vsync), `--pixel-size 2|3|4` (default 2).

## Branches / commits
- `feat/native-static-scene` from `feat/native-toolchain` (spike not on main yet).
- `feat/web-ref-mode` from `main`.
- One idea per commit, `merge --no-ff`, **no co-author trailer** (user rule), push only on user OK.
- First commit: this spec as `docs/native-sp1-static-scene.md`.

## Layout (workspace `native/`)
- `meadow-core/` (lib, no GPU deps, `cargo test` on Linux): `rng.rs`, `world.rs`, `frame_pacer.rs`. Dep: `libm` (all sin/cos/exp/atan2/hypot) so test host (glibc) and exe (mingw CRT) give identical layouts.
- `meadow/` (bin): `main.rs` (app + loop, from spike), `gpu.rs`, `camera.rs`, `mesh.rs`, `render.rs`, `shaders/{common,shadow,scene,post}.wgsl`. Remove egui deps.
- `tools/dump-world.mjs`: dumps web layout to `meadow-core/tests/fixtures/world42.txt`.

## Tasks (each ends runnable / verified)

**T1 — core: rng + world + pacer**
- `mulberry32` with `wrapping_*` u32 ops; `lerp` as `(1-t)*a + t*b` (three's form).
- Port from `src/world.js`: `pondRadius/sandRadius/pondEdge/pondPoint/sandQ`, `mainPathEdge/branchEdge/pathEdge`, `hash2/valueNoise/fbm`, `slopeAt`, placement (`place(42) -> {trees, rocks}`) keeping the exact `rand()` call order.
- **New `heightAt`**: replace `baseHeight` (two sines) with low-frequency multi-octave value noise (~±0.5 amplitude, wavelength ~8–15 units, tuned by eye); keep the flat ring around the pond (`FLAT_R`, `FLAT_BLEND`, `SHORE_Y = base(pond centre)`).
- `frame_pacer` in f64 ms: `next_deadline`, same EARLY 1 ms, STALL 250 ms, and **skip missed slots** (`last + max(1, floor((elapsed+EARLY)/interval))·interval`) — the spike's one-step advance bursts after a hitch. `parse_fps` (60/30/20/15).
- Verify: ported limiter tests as deadline sequences (same counts) + a burst test (80 ms hitch at 60 fps → 1 frame); world tests below.

**T2 — golden layout test**
- `dump-world.mjs` imports `src/world.js` with a stub scene (as `test/scene.js`), writes first 64 rng values, then tree `x z height canopy`.
- Rust test: rng exact; tree count + x/z/height/canopy equal (|Δ| ≤ 1e-9 — libm ≠ V8 bit-for-bit). Tree acceptance doesn't read `heightAt`, so trees match the web exactly.
- Rocks favour slopes, so the new hills change them: own tests instead — 12 clusters, none within `r` of pond, `r+0.15` of main path, 0.7 of a tree; deterministic for seed 42.

**T3 — meshes (`mesh.rs`)**
Vertex = `pos vec3, normal vec3, extra f32`.
- Ground: **64×64 world units, 0.5-unit cells** (~33k tris), indexed, **smooth normals from `heightAt` central differences**; `extra` = tint `0.9 + 0.18·fbm(x·0.2+80, z·0.2)`. Placement area stays `SIZE = 48`.
  - Check: screen corner on ground needs |sx| + 2|sy| ≤ half-diagonal; 64 → 45.3 ≥ 21:9's 39 ✓.
- Dodecahedron / icosahedron: three's vertex/index lists verbatim, flat normals (faceted like web).
- Trunk: 6-segment cylinder r 0.12→0.16, **smooth side normals** (web CylinderGeometry is smooth), flat caps.
- Pond / sand: 72-segment fans, `extra` = edge 0→1, at `SHORE_Y + 0.01 / + 0.02`.
- Verify: unit test that face normals point outward / up.

**T4 — instancing**
- Per instance: static `mat4` + `vec4(base.xyz, phase | -1)` + `vec4 colour`.
- Sway in vertex shader (shared fn in `common.wgsl`, used by shadow and scene passes): `T(base)·Rx(0.02 sin(t·0.9+1.7p))·Rz(0.035 sin(t·1.3+p))·T(-base)·M`, phase = index·2.4.
- Normals via cofactor of the 3×3 (handles non-uniform rock/trunk scale).

**T5 — rendering (`gpu.rs`, `render.rs`, shaders)**
- `Globals` uniform: view_proj, view, light_view_proj, sun dir/colour, hemi sky/ground, time, water tint. Web hex colours → linear.
- Lighting (own, starts from web values):
  - sun 0xfff1d6 ×2.5 at noon pos (0, 38, 13.68);
  - hemi 0xbbe4f4 / 0x3a3326 ×1.2;
  - 4-step toon 60/125/195/255 on N·L·0.5+0.5;
  - **ground uses soft band edges** (step smoothed over ~1 art pixel via `fwidth`), so smooth hills read as gentle tone patches, not stripes;
  - trees/rocks keep hard steps.
- **Shadow pass**: Depth32 2048², ortho ±22, near 1 far 80, `cull_mode: Front`, casters = trunks, crowns, rocks (ground/fans don't cast). Comparison sampler, 5-tap PCF, normal offset 0.02 + bias −0.0005, outside frustum = lit; tune by eye.
- **Scene pass (MRT)** at art res:
  - colour `Rgba16Float` cleared to 0x7fb2e5;
  - normal `Rgba8Unorm` cleared to encoded view-space up;
  - `Depth32Float` with `TEXTURE_BINDING`.
  - Pipelines: `fs_toon`, `fs_ground` (path + branch SDF in f32, from world.js GLSL patch), `fs_water` (bands/bank/foam/glints port; water colour lerped 10% to sky).
- **Post pass**: fullscreen triangle at swapchain res, `textureLoad` only; art px = `min(floor(frag/px), art−1)`; neighbours clamped manually; depth+normal edges with web thresholds (normal 0.3, depth 0.4, algorithm from three `RenderPixelatedPass.js`); write to `*Srgb` swapchain (OETF fallback).

**T6 — camera + resize**
- Ortho, eye `30·(cos30°·sin45°, sin30°, cos30°·cos45°)` → origin, top/bottom ±9, near/far 0.1/100.
- **Aspect = art_w/art_h** (keeps art pixels square).
- `Resized` stores size + dirty flag; `render()` applies once before acquire: configure surface, rebuild art targets + post bind group + camera. Size 0 → minimised, touch nothing.

**T7 — frame loop / surface robustness** (from spike, fixed)
- Pacer deadlines with `WaitUntil`; no cap → `request_redraw` each frame under Fifo.
- `Suboptimal` → draw then reconfigure; `Outdated` → reconfigure; **`Lost` → recreate surface from the kept `Instance`**; `Occluded` → retry every 250 ms; `Timeout`/`Validation` → skip.
- Device-lost callback / uncaptured error → log + exit cleanly (wgpu 30 has no `OutOfMemory` variant).
- `dt` from `Instant`, clamp 0.1 s, 0 on first frame.

**T8 — web `?ref` mode** (`feat/web-ref-mode`, `src/main.js`, `src/sky.js`)
- Skip building grass/flowers/insects (guard their updates); hide clock + HUD.
- `buildSky(scene, { clouds: false })`; fog off; sky time pinned at solar noon `(0.25 − DAY_START)·DAY_LENGTH`.
- `&px=N` overrides pixel size without saving. `npm test` still green.

**T9 — measure + CPU look (time-box 1 h)**
- RAM after 60 s at noon, Vulkan + DX12 → row in `docs/perf-baseline.md` (expect +20–30 MB: shadow map 16 MB, art targets).
- CPU per frame: time acquire / encode / submit / present with `Instant`. Check:
  - egui removal (free);
  - Vulkan implicit layers (`HKLM\SOFTWARE\Khronos\Vulkan\ImplicitLayers`);
  - `with_env()` validation;
  - DX12 vs Vulkan.
- Report findings, no fixes beyond the time-box without asking.

## Verification (exit criteria)
1. `cargo test -p meadow-core` green in WSL (pacer, golden trees, rock rules, mesh normals).
2. `native/run.sh -- --fps 60` on Windows: scene renders, sway + water animate, no panics; log shows backend.
3. Side by side with `npm run dev` → `http://127.0.0.1:5173/?ref`: same layout of trees/pond/paths, same feel; differences expected and listed (new hills, rocks, soft ground bands, no cloud shadows). Screenshots to the user.
4. Resize drag at pixel size 2/3/4: art pixels stay whole squares, no stretching; 21:9 window shows no background corners.
5. Minimise → GPU idles (Task Manager), restore resumes; covered window retries at 250 ms.
6. RAM within ≤ 200 MB WS / ≤ 130 MB private; numbers recorded. CPU findings reported.

## Risks (flow-traced)
- **Stale post bind group after resize** → rebuilt on the same dirty path (T6).
- **Pond/sand z-fight or false outlines**: +0.01 lift ≫ Depth32 precision; normals all up → none expected; check visually.
- **Shadow acne**: only on tree/rock back faces (ground doesn't cast) → bias tuning.
- **Water noise hash** differs per GPU → glint pattern not pixel-equal to web (fine for "same feel").
- **New hills shift rocks**: slopeAt drives rock clusters; accepted (same feel).
