# Voxel Asset Spike 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Put cube voxels, chamfered voxels, real `.vox` packs and a low-poly GLB side by side in the meadow, so we can decide by eye which asset format the map editor should use.

**Architecture:** Every loader ends at the same output as spike 1, `Vec<(u32 /* sRGB hex */, MeshData)>` (one mesh per colour), which `render::voxel_draws` uploads through the existing toon pipeline. No shader changes. Tasks 1–3 each own one new file, so they can run in parallel worktrees. Task 4 wires them into `voxel_draws` and runs the visual and perf checks.

**Tech Stack:** Rust 2024, wgpu 30, glam 0.34, `dot_vox` 5.2, `gltf` 1.4.

**Spec:** No spec doc. The decisions come from the 2026-11-03/04 brainstorm (map editor, user-drawn assets, sharable). They are copied here:
- Assets are voxel (B). Hướng 2: the user paints voxels, and the app may mesh them with chamfers so they match the low-poly trees and rocks.
- Hướng 1: the app may also read low-poly GLB (Polyfork). Polyfork licence: bundling in the exe is OK; maps may not embed Polyfork files (reference them by ID).
- Free CC0 `.vox` packs exist (padadu Fantasy Bundle, Fuzzy Man Studios, theodd1in Voxel Trees, which still needs a licence check).
- The spike is throwaway, on branch `feat/voxel-spike` in `C:\Users\d\Documents\Github\pixel3d-voxel-spike`.

## Global Constraints

- Spike code only: every new item carries a `// SPIKE` or `//! SPIKE` marker. Never merge to `main`. No push.
- No changes to any `.wgsl` file, `Instance`, `Vertex`, `MeshData` or `upload`.
- Tasks 1–3 touch only their own file (listed per task). `main.rs`, `Cargo.toml`, `Cargo.lock` and `render.rs` belong to Task 0 and Task 4.
- Mesh units are voxels (GLB: file units), origin at the bottom centre of the bounding box, matching `voxel::meshes`. Triangles are counter-clockwise seen from outside.
- Colours are sRGB hex `u32` (`0xRRGGBB`), the same as `Instance::new`.
- Host: Windows with the MSVC toolchain (no `windres`; `build.rs` already skips it). Tests run from the repo root with `cargo test --manifest-path native/Cargo.toml -p meadow <module>` (agy matches allowed commands exactly and starts at the worktree root, so no `cd native`).

## Review Focus

1. **Thin features under the chamfer mesher** (1-voxel legs, bench slats, edge-only contact): they must not disappear or produce zero-area triangles. Owner: Task 1, `thin_features_keep_their_triangles`.
2. **MagicaVoxel is Z-up and right-handed; the meadow is Y-up.** A naive y↔z swap mirrors the model, so a chair would face backwards. Owner: Task 2, `vox_axes_are_y_up_and_not_mirrored`.
3. **Polyfork colours by vertex colour (`COLOR_0`), not by material or texture** (measured). Factor-only loading would draw the whole tree white. Owner: Task 3, `vertex_colours_split_the_mesh_by_colour`.
4. **glTF base colour is linear, our hex is sRGB.** Without conversion every GLB colour comes out washed out. Owner: Task 3, `material_colour_is_converted_to_srgb_hex`.
5. **A `.vox` file with several models or palette slots that are never used** must not panic, and unused colours must not create empty draws. Owner: Task 2, `unused_palette_entries_make_no_mesh`.

---

### Task 0: Prepare the spike branch (Claude, not agy)

**Files:**
- Modify: `native/meadow/src/voxel.rs` (`fn get` → `pub(crate) fn get`)
- Create: `native/meadow/src/voxel_nets.rs`, `native/meadow/src/vox_file.rs`, `native/meadow/src/glb.rs` (each only `//! SPIKE: <one line>`)
- Modify: `native/meadow/src/main.rs` (three `mod …; // SPIKE` lines under `mod voxel;`)
- Modify: `native/meadow/Cargo.toml` (`dot_vox = "5.2"`, `gltf = "1.4"`), `native/Cargo.lock`

- [ ] **Step 1:** Make the edits above, then run `cargo test -p meadow voxel` from `native/`. Expected: the 2 spike tests pass, and the build has no errors.
- [ ] **Step 2:** Commit spike 1, the stubs and this plan on `feat/voxel-spike`.

```bash
git add native docs/superpowers/plans/2026-11-04-voxel-asset-spike.md
git commit -m "feat(native): voxel spike scaffolding for chamfer, .vox and GLB loaders (spike, not for merge)"
```

- [ ] **Step 3:** Download the test assets to `C:\Users\d\pixel3d-assets\` (outside every repo; not committed): the padadu Voxel Fantasy Bundle `.vox` files, one Polyfork tree GLB (a local test is within its licence), and the theodd1in Voxel Trees pack only if its page names a free licence. Done 2026-11-04: `polyfork/lineside-tree-97e02a.glb` and `ginkgo-tree-baa895.glb` (vertex colours, no texture). The itch.io downloads could not be scripted, so the user downloads those by hand. theodd1in is CC BY-SA 4.0 (attribution), which is fine for a local test.

### Task 1: Chamfered voxel mesher (agy, label `chamfer`)

**Files:**
- Create: `native/meadow/src/voxel_nets.rs` (code and its `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::voxel::{Model, tree, bench, meshes}`, `Model::get(&self, [i32; 3]) -> u8` (0 outside the grid), `Model.size`, `Model.palette`, `crate::mesh::{MeshData, Vertex}`.
- Produces: `pub fn chamfered(m: &Model) -> Vec<(u32, MeshData)>`, the same contract as `voxel::meshes`: one entry per palette colour that has triangles, in palette order.

- [ ] **Step 1: Write the failing tests** in `voxel_nets.rs`:

```rust
// helper: every triangle as 3 Vec3, across all colours
fn tris(out: &[(u32, MeshData)]) -> Vec<[Vec3; 3]>

#[test] fn a_single_voxel_makes_twelve_outward_triangles()
// Model [1,1,1], one cell set. chamfered → 1 mesh, 12 triangles.
// For each triangle, (b-a)×(c-a) · (centroid - (0,0.5,0)) > 0.

#[test] fn triangle_count_matches_the_culled_faces_per_colour()
// Box [4,3,5], cells x<2 → colour 1, else colour 2 (palette [0xff0000, 0x0000ff]).
// For each colour: chamfered index count == voxel::meshes(m, false) index count of that colour.
// (Each solid/empty edge crossing gives one quad, owned by the solid voxel.)

#[test] fn a_solid_box_is_closed_and_inside_its_bounds()
// Box [4,3,5] one colour. Every undirected edge (positions rounded to 1e-4) is used by exactly
// 2 triangles in opposite directions. Every vertex x∈[-2,2], y∈[0,3], z∈[-2.5,2.5].

#[test] fn thin_features_keep_their_triangles()
// voxel::bench(8) and voxel::tree(10, 1): every triangle has area > 1e-6, and every colour that
// voxel::meshes(m, true) returns is also returned by chamfered.
```

- [ ] **Step 2: Run** `cargo test --manifest-path native/Cargo.toml -p meadow voxel_nets`. Expected: FAIL, `chamfered` not found.
- [ ] **Step 3: Implement `chamfered`** as naive Surface Nets on the binary occupancy grid:
  - Pad the grid by one empty cell on every side.
  - A dual cell is centred on each voxel corner, and its 8 corners are the centres of the 8 voxels that share that corner. A dual cell with both solid and empty voxels among those 8 gets one vertex: the mean of the midpoints of its edges that cross solid/empty. No smoothing iterations. For example, a lone voxel becomes a cube of side 1/3 around its centre.
  - Never drop triangles to make a test pass. If `thin_features_keep_their_triangles` fails, leave it failing and put the failing model and triangle at the top of the reply.
  - For each pair of face-adjacent voxels (solid, empty), emit the quad of the 4 dual vertices around their shared face, in the mesh of the solid voxel's colour. Wind it counter-clockwise seen from the empty side.
  - Split each quad into 2 triangles, each with its own 3 vertices and its own face normal (flat shading, unshared vertices), `extra: 0.0`.
  - Shift positions to bottom-centre origin like `voxel::meshes`.
- [ ] **Step 4: Run** `cargo test --manifest-path native/Cargo.toml -p meadow voxel_nets`. Expected: 4 passed.
- [ ] **Step 5:** No commit (agy). Claude reviews the worktree diff and commits it on `feat/voxel-spike` with `feat(native): chamfered (surface nets) voxel mesher (spike)`.

### Task 2: `.vox` loader with palette snapping (agy, label `vox-load`)

**Files:**
- Create: `native/meadow/src/vox_file.rs` (code and tests)

**Interfaces:**
- Consumes: `crate::voxel::Model::{new, set}`, `Model.palette`, the `dot_vox` crate (`dot_vox::load_bytes`).
- Produces:
  - `pub fn load(bytes: &[u8]) -> Result<Vec<Model>, String>`: one `Model` per `.vox` model. Scene-graph transforms are ignored. Each model's palette holds only the colours it uses, as sRGB hex with alpha dropped. Cell value i means `palette[i-1]`.
  - `pub fn snap(m: &mut Model, to: &[u32])`: replaces each palette colour with the nearest entry of `to` (squared distance on sRGB bytes). Two palette entries may snap to the same colour.
  - `pub const MEADOW: [u32; 21]`: `0xa4cc86, 0xecd8b8, 0x8d6d52, 0x589e63, 0x46864f, 0x6db374, 0xaaafc2, 0x3a4044, 0x23262b, 0xf3ead2, 0x55663a, 0x8c9a80, 0x4f8a3a, 0x5a9a3c, 0x3d6e2a, 0x8cc060, 0xe6dca0, 0xd8667a, 0xc3a284, 0xa0703f, 0x6b4a2b`. A chosen subset of the render.rs constants plus spike 1.s tree and bench colours (no water, pupil or path-shadow tones).
- Axis mapping (MagicaVoxel Z-up → meadow Y-up, keeping handedness): vox `(x, y, z)` in a model of size `(sx, sy, sz)` → meadow cell `(x, z, sy - 1 - y)`, meadow size `[sx, sz, sy]`.

- [ ] **Step 1: Write the failing tests.** `fn tiny_vox(size: [u32; 3], voxels: &[[u8; 4]], palette: &[(u8, u32)]) -> Vec<u8>` hand-writes a valid `.vox`: `"VOX "`, version 150, then a `MAIN` chunk containing `SIZE`, `XYZI` and a 256-entry `RGBA` chunk, all other entries `0`. Each voxel is `[x, y, z, colour index 1..=255]`, and `palette` sets RGBA slot (index−1) to `0xRRGGBB` with alpha 255. Use `dot_vox` only to read, not to write.

```rust
#[test] fn vox_axes_are_y_up_and_not_mirrored()
// size [3,2,4]; voxels at vox (2,0,0) colour 1 and (0,1,3) colour 2.
// load → model size [3,4,2]; cell (2, 0, 1) is colour 0xff0000 and cell (0, 3, 0) is 0x00ff00;
// solid() == 2.

#[test] fn unused_palette_entries_make_no_mesh()
// One voxel using colour index 7 out of a 256-entry palette → model.palette.len() == 1, and
// voxel::meshes(&model, true).len() == 1.

#[test] fn snap_picks_the_nearest_meadow_colour()
// palette [0x5aa065, 0x8a6a50] snapped to MEADOW → [0x589e63, 0x8d6d52].

#[test] fn garbage_is_an_error_not_a_panic()
// load(b"not a vox") is Err.
```

- [ ] **Step 2: Run** `cargo test --manifest-path native/Cargo.toml -p meadow vox_file`. Expected: FAIL, `load` not found.
- [ ] **Step 3: Implement** `load`, `snap` and `MEADOW` as specified in Interfaces.
- [ ] **Step 4: Run** `cargo test --manifest-path native/Cargo.toml -p meadow vox_file`. Expected: 4 passed.
- [ ] **Step 5:** No commit (agy). Claude reviews and commits `feat(native): load MagicaVoxel .vox and snap it to the meadow palette (spike)`.

### Task 3: GLB / glTF loader (agy, label `glb-load`)

**Files:**
- Create: `native/meadow/src/glb.rs` (code and tests)

**Interfaces:**
- Consumes: the `gltf` crate (`gltf::import_slice`, default features), `crate::mesh::{MeshData, Vertex}`.
- Measured on real Polyfork files (`lineside-tree-97e02a.glb`, `ginkgo-tree-baa895.glb`): one material with no `baseColorFactor` and no texture. Colour comes from `COLOR_0` (VEC3 float), uniform within every triangle (0 of 153 triangles mixed). Parent nodes carry translations. Textures are out of scope: ignore them.
- Produces: `pub fn load(bytes: &[u8]) -> Result<Vec<(u32, MeshData)>, String>`, the same contract as `voxel::meshes`: one mesh per distinct sRGB colour, origin at the bottom centre of the whole scene's bounding box.
  - Traverse the default scene (or scene 0) and apply each node's world transform to positions and normals (normal matrix = inverse transpose).
  - Flat shading: unshared vertices, face normal per triangle (ignore file normals), `extra: 0.0`. Only `Mode::Triangles` primitives; skip others with a `log::warn!`.
  - Colour per triangle = `baseColorFactor` (linear; `[1,1,1,1]` when absent) × the mean of the triangle's 3 `COLOR_0` values (linear; `[1,1,1]` when the primitive has none). Read them with `reader.read_colors(0).map(|c| c.into_rgb_f32())`, which handles u8/u16/float and RGB/RGBA. Convert the product linear → sRGB per channel (IEC 61966-2-1), round to u8, pack as `0xRRGGBB`, drop alpha.

- [ ] **Step 1: Write the failing tests.** Fixtures are `.gltf` JSON strings built in the test, with the buffer as a base64 `data:` URI. `import_slice` accepts `.gltf` JSON as well as `.glb`.

```rust
#[test] fn a_triangle_round_trips_with_flat_normal_and_bottom_centre_origin()
// Triangle (0,0,0) (1,0,0) (0,1,0), base colour factor [1,1,1,1], node translation (5,5,5).
// → 1 mesh, colour 0xffffff, 3 vertices; positions shifted so the bbox bottom centre is the
// origin: (-0.5,0,0) (0.5,0,0) (-0.5,1,0); normal (0,0,1) on all three.

#[test] fn material_colour_is_converted_to_srgb_hex()
// factor [0.2158605, 0.2158605, 0.2158605, 1] → colour 0x808080.

#[test] fn vertex_colours_split_the_mesh_by_colour()
// Two triangles in one primitive, no material: COLOR_0 (VEC3 float) is [1,0,0] on the first
// triangle's 3 vertices and [0,0,1] on the second's → 2 meshes, colours {0xff0000, 0x0000ff},
// 3 vertices each.

#[test] fn the_real_polyfork_tree_loads_in_four_colours()
// If C:/Users/d/pixel3d-assets/polyfork/lineside-tree-97e02a.glb exists (otherwise eprintln and
// return): load → 4 meshes (the file has 4 distinct COLOR_0 values over 2 primitives of 153 and 316
// triangles), 469 triangles in total, and every normal has length 1 ± 1e-4.

#[test] fn garbage_is_an_error_not_a_panic()
// load(b"nope") is Err.
```

- [ ] **Step 2: Run** `cargo test --manifest-path native/Cargo.toml -p meadow glb`. Expected: FAIL, `load` not found.
- [ ] **Step 3: Implement** `load` as specified in Interfaces.
- [ ] **Step 4: Run** `cargo test --manifest-path native/Cargo.toml -p meadow glb`. Expected: 5 passed.
- [ ] **Step 5:** No commit (agy). Claude reviews and commits `feat(native): load low-poly GLB per triangle colour (spike)`.

### Task 4: Wire it up, look, measure (Claude, not agy)

**Files:**
- Modify: `native/meadow/src/render.rs` (`voxel_draws` only)

**Interfaces:**
- Consumes: `voxel_nets::chamfered`, `vox_file::{load, snap, MEADOW}`, `glb::load`, all from Tasks 1–3.
- New `MEADOW_VOXELS` modes, alongside the existing `1` and `stress`:
  - `chamfer`: the same 6 models as `1`, meshed with `chamfered`.
  - `stress-chamfer`: `stress` meshed with `chamfered`.
  - `files:<path>;<path>;…`: `.vox` files go through `vox_file::load` and `snap` (raw colours if `MEADOW_VOX_RAW=1`), then either `meshes(…, true)` or `chamfered` (`MEADOW_VOX_CHAMFER=1`). `.glb`/`.gltf` files go through `glb::load`. Changed 2026-11-04 (user): `.vox` at a fixed 8 voxels per unit (keeps each pack's internal proportions, as the editor would) and GLB at 0.4 units per metre (a 6 m Polyfork tree ≈ the meadow tree), in rows of 8, 2.2 units apart. Nothing sways (`Instance::new`).

- [ ] **Step 1:** Implement the modes above, then run `cargo test -p meadow`. Expected: all pass, except the known flaky `sky_clock::an_unknown_city_zone_leaves_the_clock_on_the_pc`.
- [ ] **Step 2: Look.** Run `cargo build --release`, then the exe with `--window --hour 10` and `--hour 18`, at `--pixel-size 2` and `3`, for `1`, `chamfer`, and `files:` with 3 padadu models plus the Polyfork tree. Take screenshots into the brainstorm page next to spike 1's. The user judges: outlines, tone against the low-poly trees and rocks, snapped vs raw colours.
- [ ] **Step 3: Measure.** For `stress` and `stress-chamfer`: read the voxel log line (vertices, MB), the working set and private bytes after 60 s, and `--gpu-time` ms per pass with the window in front (not occluded). Run each mode 3 times. Report the numbers as measured, with their spread.
- [ ] **Step 4:** Commit `feat(native): MEADOW_VOXELS chamfer and files modes (spike)` on `feat/voxel-spike`. Do not merge.

---

## Execution: one Sonnet driver per agy run

Three Sonnet subagents run in parallel, one per Task 1–3. Each one drives its own agy run (agy writes the code) and checks the result. Claude does Task 0 and Task 4, and is the only one that commits.

**Preparation (Claude, after Task 0 is committed):**

- **P1 Toolchain inside agy.** `agy-run-win.sh` points `HOME` and `USERPROFILE` at a throwaway profile. With `CARGO_HOME` and `RUSTUP_HOME` unset (as on this machine), rustup would look for toolchains in the empty profile and `cargo test` would fail. Every run is therefore started as `CARGO_HOME=C:/Users/d/.cargo RUSTUP_HOME=C:/Users/d/.rustup ~/.claude/skills/agy/agy-run-win.sh …`.
- **P2 Smoke test P1 once** before the fan-out: a `low` run with `--allow "cargo test --manifest-path native/Cargo.toml -p meadow voxel"` that only runs that command and quotes its last 5 lines. Pass = `2 passed`. If it fails, stop and report to the user; do not widen the allowlist.
- **P3 Pre-warm the worktrees.** The runner reuses `/tmp/agy-wt/<label>` when it exists. For `chamfer`, `vox-load` and `glb-load`, run `git -C <spike> worktree add --detach /tmp/agy-wt/<label> HEAD` and copy `native/target/debug` from the spike worktree into each. Without that, each run spends its timeout building wgpu from scratch (target ≈ 1.8 GB; disk has 126 GB free).
- **P4 Timeout:** `AGY_TIMEOUT_MIN=20` (med defaults to 12; a cold meadow rebuild plus test iterations can exceed that).
- Do not use the Agent tool's `isolation: "worktree"`. It cuts from the main checkout (`main`, without the spike); the agy worktree is the isolation.

**Sonnet driver brief** (fill in `<N>`, `<label>`, `<file>`, `<filter>`):

> You drive one agy run for Task `<N>` of `C:\Users\d\Documents\Github\pixel3d-voxel-spike\docs\superpowers\plans\2026-11-04-voxel-asset-spike.md`. Read the Global Constraints, the Review Focus and Task `<N>`. Read only those parts.
> 1. Write an agy brief that:
>    - points to that plan path (relative: `docs/superpowers/plans/2026-11-04-voxel-asset-spike.md`) and Task `<N>`;
>    - says only `native/meadow/src/<file>` may change;
>    - gives the one allowed command `cargo test --manifest-path native/Cargo.toml -p meadow <filter>`, and says to use built-in tools for reading and searching;
>    - says no commit, no push, no staging;
>    - asks for conflicts and assumptions at the top, then the test output.
> 2. Run it in the foreground: `CARGO_HOME=C:/Users/d/.cargo RUSTUP_HOME=C:/Users/d/.rustup AGY_TIMEOUT_MIN=20 ~/.claude/skills/agy/agy-run-win.sh med C:/Users/d/Documents/Github/pixel3d-voxel-spike <label> --write --allow "cargo test --manifest-path native/Cargo.toml -p meadow <filter>"`. Read the reply by section (`head` first).
> 3. Review: `git -C /tmp/agy-wt/<label> status --short` must list only `<file>`. Read the diff against Task `<N>`: signatures, the test names and assertions as written in the plan, and no dropped triangles or loosened asserts. Trace each Review Focus line that Task `<N>` owns through the code.
> 4. Rerun the test yourself in `/tmp/agy-wt/<label>/native` (normal env). Trust your own output, not agy's.
> 5. If the tests fail or the review finds a defect, do one more agy run with the same label (the worktree is reused) and a brief that quotes the failure. At most 2 agy runs in total. After that, stop and report.
> 6. Don't commit, don't copy files into the spike worktree, don't run `--cleanup`.
> 7. Report, in this order: verdict (ready / not ready), test command and its last 10 lines from your own run, files changed, review findings with `file:line`, conflicts agy listed, the rc and usd of each agy run.

**After the drivers report (Claude):** read each worktree diff yourself and rerun its test. Then bring `<file>` over to `feat/voxel-spike`, one commit per task (message from the task's Step 5), and run `--cleanup <label>`. Then Task 4.
