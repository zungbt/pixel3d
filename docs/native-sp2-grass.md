# pixel3d native — sub-project 2: grass + wind (spec)

## Context
Sub-project 1 (static scene) is merged. Measured: 142 MB working set / 103 MB private, 3.2 % CPU at 15 fps (`docs/perf-baseline.md`). The release exe now runs as the user's wallpaper.
Sub-project 2 adds the web's grass (`src/grass.js`): about 88k one-triangle blades, thinned by a density map, swaying in a travelling gust.

Decided with the user:
- **RAM/CPU first.** The look should match the web, but cheap beats exact.
- **Density: measure first, then decide.** Start with the web's density, measure, and let the user choose whether to cut. If it costs too much, the tricks below get applied in that order (no geometry change before the numbers):
  1. Fewer but wider blades: `ATTEMPTS` down, `BLADE_W` up. Two constants.
  2. Tint the ground under dense grass towards the blade colour, so thinning shows no bare ground.
  3. Use the blade's average colour for that tint, not the tip colour: a tapered blade's area centre sits ⅓ of the way up. A tip-coloured ground makes thin patches brighter than dense ones.
  4. Last resort: one alpha-cut quad per tuft of ~5 blades.
- **Flowers move to sub-project 4**, with the insects that perch on them.
- **Grass casts no shadow** (as the web: `receiveShadow` only), so it stays out of the shadow pass.
- **Not in this sub-project:** a user-set location (city or lat/lon) instead of the time-zone guess for sunrise and sunset. It goes to sub-projects 3 and 5. GPS was considered and dropped: it needs Windows Location on, WinRT and a permission prompt, all to fix ~20–30 min.

## Branch / commits
- `feat/native-grass` from `main`.
- One idea per commit, no co-author trailer (user rule), push and PR only on user OK.
- First commit: this spec, plus the roadmap update (flowers → SP4, location idea → SP3/SP5).

## Design

### Placement: `meadow-core/src/grass.rs` (pure Rust, tested on Linux)
- **`GROUND_SIZE` = 64 moves** from `meadow/src/mesh.rs` to `meadow_core::world`; the mesh uses it from there.
- **The grass area is the ground (64), not the placement area (48).** With a 48 area the blades would stop on a straight line that shows in the screen corners.
  - Half-extent `GROUND_SIZE / 2 − 0.1`.
  - Attempts keep the web's ~59 per square unit: `ATTEMPTS = 240_000`.
- **Ported from the web, same `rand()` order** (x, z, accept, scale, hue, lightness; seed 99):
  - `canopy_mask(trees)`: 4 cells per unit over the 48 area. Points outside it count as not under a canopy; the web never asks for them.
  - `grass_density(x, z, mask, rocks)`:
    - 0 inside a rock's `r` (exact test);
    - × `smoothstep(sand_q, 1, 1.1)`;
    - × `smoothstep(path_edge, 0, 0.8)`;
    - × `lerp(0.15, 1, smoothstep(fbm(x·0.15+50, z·0.15), 0.25, 0.5))`;
    - × 0.25 under a canopy.
  - Per blade:
    - scale `0.7 + rand·0.6`;
    - colour HSL(`0.25 + rand·0.04`, 0.5, `0.71 + rand·0.1`) taken as sRGB, then converted to linear. This is the user-approved palette (web `522ea47`, with the `mix(0.85, 1.12)` tip shading below): don't drift it without asking;
    - `y = height_at(x, z)`;
    - ground normal at the base: central differences of `height_at` with e = 0.25, exactly as `mesh::ground()` (used for outlines only, see below).
- `place_grass(layout) -> Vec<Blade { x, y, z, scale, color: [f32; 3], normal: [f32; 3] }>`, all f32, so the startup transient stays ~5 MB.
  - `Renderer::new` calls `world::place(42)` once and passes the layout to both `build_draws` and `place_grass`.
  - The result is dropped after the upload.
- The layout can't match the web blade for blade: rocks and hills already differ since SP1. Tests check rules instead.

### GPU data, pipeline, shader
- **20 bytes per blade:**
  - `pos: Float32x3`;
  - `Unorm8x4`: linear rgb, `(scale − 0.7) / 0.6`;
  - `Snorm8x4`: ground normal xyz, 0.
  - One instance buffer, no vertex buffer: `vs_grass` builds the 3 corners from `vertex_index`.
  - The corners are (±BLADE_W/2, 0) and (0, BLADE_H), with BLADE_W 0.1 and BLADE_H 0.3, turned by the camera yaw π/4.
  - Draw with `draw(0..3, 0..count)`.
  - `BLADE_W` / `BLADE_H` are constants that `render.rs` fills into the shader, like the path constants.
- **`vs_grass` / `fs_grass` in `scene.wgsl`.** They share `Globals`, `shadow()`, `lighting()` and `view_normal()`.
  - **Wind** (web formula, time in `g.params.x`), with x, z the blade base and `tip = local y / BLADE_H`:
    ```
    sway = sin(t·1.6 − (x+z)·0.45)·0.5 + 0.5
    sway = sway·0.12 + sin(t·3.1 + x·2)·0.02
    local.xz += (0.7071, −0.7071) · sway · tip
    ```
    It is added in local space before the scale (as the web).
  - **Shading:** `color · mix(0.85, 1.12, tip)`, applied in `vs_grass`. `mix` is linear in `tip`, so interpolating it is exact. Lighting:
    - normal fixed at (0, 1, 0), as the web;
    - `toon()` ramp;
    - `shadow()` (receives tree shadows).
  - **Normal target:** `view_normal(ground normal at the base)`, **not** `up`.
    - Traced through `fs_post::normal_edge`: on the steepest hills (~17°) each ground neighbour adds `1 − cos θ` ≈ 0.044. A tip pixel with 3 ground neighbours sums 0.13, above the 0.1 threshold, so `up` would draw ×1.3 bright outlines there.
    - With the ground normal, the ground behind a blade (≤ 0.8 units away, < 5° apart) sums ≈ 0.
    - `fs_grass` is still separate from `fs_toon`, because the lighting normal ≠ the outline normal.
    - Making the lighting follow the ground too (shade blades like the ground beneath them) is then a one-line change, should it ever be wanted.
- **Pipeline `grass`:**
  - scene pass, after the ground;
  - `cull_mode: None`. The blade plane faces the eye (dot 0.87), and the wind moves the tip within that plane, above the base line, so the winding never flips. None vs Back makes no difference;
  - depth test + write;
  - not in the shadow pass.

### Only the blades on screen (as the web's `fitView`)
The camera never moves and the view is always 18 units high.
- **At `Renderer::new`:**
  - Project each blade base with `camera::view()`.
  - **Drop** blades with `|view y| ≥ 9 + VIEW_MARGIN` (0.6: blade height + sway). They can never be on screen at any window size.
  - Sort the rest by `|view x|` and upload in that order.
  - Keep `edges: Vec<f32>`, the sorted `|view x|` values (~0.4 MB).
- **In `set_camera`** (called by both `Renderer::new` and `resize`), from the same art `(w, h)` as the projection:
  - count the blades with `edges` below `9 · art_w / art_h + VIEW_MARGIN` (`partition_point`), and draw that many;
  - no GPU work.
- **Logs:** blades placed / kept / drawn, and the placement time.

Estimates (to check against the logs):
- ~160–170k placed: acceptance is higher outside the 48 area, with no pond, rocks or trees.
- ~112k kept (~2.2 MB buffer).
- Drawn: ~35k at 4:3, ~41k at 16:10, ~47k at 16:9, ~61k at 21:9.

### Resolution and aspect
- **Resolution:** a larger screen shows the same 18-unit view at a higher art resolution.
  - The blade count stays the same; each blade covers more art pixels (1080p ~3×9, 1440p ~4×12, 4K ~6×18 at pixel size 2).
  - Fill cost scales with art pixels for the whole scene: 4K at pixel size 2 is ×4 against 1080p.
  - 4K at `--pixel-size 4` looks and costs the same as 1080p at 2.
  - A per-resolution default pixel size belongs to the SP5 settings.
- **Aspect:** the blade count grows with the aspect.

## Tasks (each ends runnable / verified)

**T1 — core placement**
- Move `GROUND_SIZE` to `meadow_core::world`.
- Add `grass.rs`: `canopy_mask`, `grass_density`, `place_grass`.
- Verify (`cargo test -p meadow-core`):
  - same seed → same blades;
  - no blade within any rock's `r`, where `sand_q < 1`, or where `path_edge ≤ 0`;
  - the density never exceeds 0.25 under a canopy;
  - the mask flags every cell centre inside a canopy;
  - acceptance (placed / attempts) lies in 0.5–0.85.

**T2 — grass pipeline**
- Instance buffer, `vs_grass` / `fs_grass`, pipeline, draw in the scene pass. Everything placed is drawn for now.
- Verify: `native/run.sh --release -- --fps 60`: grass on screen, sways, no outline noise, tree shadows fall on it, no panics.

**T3 — view culling**
- Startup sort + `edges`; draw count set in `set_camera`; the logs above.
- Verify by resizing the window to 4:3, 16:10, 16:9 and 21:9 at pixel size 2, 3 and 4:
  - the side edges never run out of grass;
  - the drawn counts in the log follow the aspect.

**T4 — measure + look**
- Ask the user before running: one A/B round, ~4 min. A second round only on the user's OK.
- A/B the release exe from `main` and the branch exe, each in `--wallpaper` at 1920×1080, pixel size 2, 15 fps:
  - RAM after 60 s;
  - CPU over 20 s;
  - GPU 3D utilisation from `Get-Counter '\GPU Engine(pid_<pid>*engtype_3D)\Utilization Percentage'`. At 15 fps it reads a few % and is noisy: a difference under ~1 point is reported as within noise.
- Add a row to `docs/perf-baseline.md`.
- Screenshot via `PrintWindow` next to the web at 12:00. Use the normal web mode with the slider at 12:00, not `?ref`, which hides the grass.
- Zoomed crop of the blade tips next to the web's (see Risks: depth edges).
- The user watches the wind live, side by side.
- The user then decides whether to apply the density tricks.

## Verification (exit criteria)
1. `cargo test -p meadow-core` green in WSL.
2. Grass sways like the web, with no per-blade outlines, and receives tree shadows. The screenshot is shown to the user.
3. No missing grass at the side edges for 4:3 to 21:9 and pixel sizes 2 to 4.
4. RAM, CPU and GPU deltas are recorded against `main`, and the user has decided on the density.

## Risks (flow-traced)
- **Explorer restart:** reattaching runs `Renderer::new` again, so the placement and sort run again (estimated tens of ms). The time is logged; if it's slow, cache the sorted buffer in `App`.
- **Normal outlines on hills:** fixed by design (the ground normal goes in the normal target, see the shader section).
- **Depth edges darken tall blade tips.**
  - 0.4 is the darkening strength. The threshold is `smoothstep(0.01, 0.02, Σ deeper-neighbour Δdepth)`, with depth linear over 99.9 units.
  - The ground behind a tip is `0.6·s` units deeper (0.006·s in depth), and a tip pixel has up to 3 deeper neighbours.
  - s = 0.7 sums 0.0126: no edge. s = 1.3 sums 0.023: `dei` = 1, so the tip is up to 40 % darker.
  - The web has the same math and near/far, so this is part of its look. Compare the zoomed tips in T4, and act only if native differs.
- Note — blade roots: they sit on `height_at`, while the mesh interpolates over 0.5 cells. The error is ≈ curvature·cell²/8 ≈ 0.01 units ≈ 0.3 art px, which is invisible.
- **The shadow map covers ±22:** grass further out is always lit, the same as the ground there.
- **Aspect beyond 3:1 (32:9):** the 64 ground already shows its edge in the corners (SP1 limit), and the grass stops with it. Fix later by growing `GROUND_SIZE` (placement attempts grow with the area; the drawn count at 16:9 doesn't change).
- **Several monitors:** the wallpaper covers the primary screen only (`SM_CXSCREEN`), out of scope.
- **4K at pixel size 2:** ×4 fill for the whole scene; estimated only, since this PC has no 4K screen.
