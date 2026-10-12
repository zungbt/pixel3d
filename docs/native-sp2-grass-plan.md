# pixel3d native — sub-project 2: grass + wind (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The web's swaying grass in the native wallpaper, at the lowest RAM/CPU that still matches the look, then measured against `main`.

**Architecture:** Placement is pure Rust in `meadow-core` (tested on Linux): dart throwing over the 64-unit ground, thinned by the web's density map. The `meadow` crate packs each blade into a 20-byte instance, drops the rows that are never on screen, and sorts the rest by `|view x|` once. A resize then only changes how many get drawn. One triangle per blade is built in `vs_grass` from `vertex_index` (no vertex buffer), lit as flat ground but outlined with the ground's own normal.

**Tech Stack:** Rust 2024, wgpu 30, glam 0.34, bytemuck, libm (core maths), naga (shader validation test). Cross-compiled from WSL to `x86_64-pc-windows-gnu`.

**Spec:** `docs/native-sp2-grass.md` (read it first; this plan argues from it).

## Global Constraints

- Branch `feat/native-grass` (already exists, 2 doc commits on top of `main`). One idea per commit, subject style `native: …`, **no `Co-authored-by` or any other trailer** (user rule). No push and no PR without the user's OK.
- Every cargo command runs from `native/` with `export PATH=$HOME/.cargo/bin:$PATH` first.
- `meadow-core` uses `libm` for every transcendental (`sin`, `pow`, `sqrt`…), never `f64` std methods, so Linux tests and the Windows exe get bit-identical layouts. Squares are `a * a`, not `powi`.
- Grass palette is the user-approved baseline (web `522ea47`): HSL(`0.25 + r·0.04`, 0.5, `0.71 + r·0.1`) as sRGB, tip shading `mix(0.85, 1.12, tip)`. Don't change it without asking.
- Web constants carried over unchanged: `BLADE_W` 0.1, `BLADE_H` 0.3, seed 99, rand order x, z, accept, scale, hue, lightness, `VIEW_MARGIN` 0.6, canopy mask 4 cells per unit over the 48 area.
- Grass is not in the shadow pass.
- The code is not `rustfmt`-clean at default settings: **don't run `cargo fmt`**. Match the surrounding style by hand (long lines are fine, ~150 cols).
- Don't kill every `meadow.exe` (`run.sh` does `taskkill /IM meadow.exe`): the user's wallpaper may be one of them. Visual checks run a debug copy named `meadow-dev.exe`.
- Bench sparingly: T4's measurement round needs the user's OK first, with its duration stated (~4 min).

## Review Focus

1. **Window shapes the spec doesn't list** (32:9, portrait, a 1×1 art): the drawn count must stay ≤ kept, never panic, and grow with the aspect. Pinned by `drawn_count_follows_the_aspect` (Task 3).
2. **Blades on slopes:** if the blade's outline normal and the ground mesh's normal differ, every blade on a hill grows a bright outline. Pinned by `blade_normals_match_the_ground_mesh` (Task 2).
3. **Packing extremes:** scale 1.3 and colour 1.0 must give 255, not wrap to 0; negative normal components must keep their sign. Pinned by `instance_packs_into_20_bytes` (Task 2).
4. **Blades outside the 48 placement area:** the canopy mask must answer "uncovered" without an out-of-range index, and the grass must reach the ground's corners. Pinned by `mask_covers_exactly_the_cells_under_crowns` and `blades_sit_on_the_whole_ground` (Task 1).
5. **f32 rounding at the exclusion edges:** a blade accepted just outside a rock, the beach or a path must not be rejected by the test after its position is rounded to f32. The tests in Task 1 allow 1e-4.

---

## File structure

| File | Change | Responsibility |
|---|---|---|
| `native/meadow-core/src/world.rs` | modify | gains `pub const GROUND_SIZE`; `lerp` becomes `pub(crate)` |
| `native/meadow-core/src/grass.rs` | create | `Blade`, `CanopyMask`, `grass_density`, `place_grass`, `ground_normal`, `hsl_linear` + tests |
| `native/meadow-core/src/lib.rs` | modify | `pub mod grass;` |
| `native/meadow/src/mesh.rs` | modify | uses `world::GROUND_SIZE` |
| `native/meadow/src/grass.rs` | create | `GrassInstance` (20 B), vertex layout, `Grass` (buffer + count); in T3 `sort_for_view`, `drawn` + tests |
| `native/meadow/src/camera.rs` | modify (T3) | `pub VIEW_HEIGHT`, `half_width()` shared by the projection and the culling |
| `native/meadow/src/render.rs` | modify | prelude constants, `scene_pipeline` takes vs / buffers / cull, grass pipeline + draw, layout placed once |
| `native/meadow/src/shaders/scene.wgsl` | modify | `vs_grass`, `fs_grass` |
| `native/meadow/src/main.rs` | modify | `mod grass;` |
| `docs/perf-baseline.md` | modify (T4) | the A/B row |

---

### Task 1: core placement

**Files:**
- Modify: `native/meadow-core/src/world.rs` (const block near line 10; `fn lerp` near line 31)
- Modify: `native/meadow/src/mesh.rs:21` (drop the local `GROUND_SIZE`)
- Modify: `native/meadow-core/src/lib.rs`
- Create: `native/meadow-core/src/grass.rs`

**Interfaces:**
- Consumes: `world::{SIZE, Layout, Tree{x,z,canopy,..}, Rock{x,z,r,..}, height_at, sand_q, path_edge, fbm, smoothstep, lerp}`, `rng::Mulberry32`.
- Produces (used by Task 2/3):
  - `pub const world::GROUND_SIZE: f64 = 64.0`
  - `pub struct grass::Blade { pub x: f32, pub y: f32, pub z: f32, pub scale: f32, pub color: [f32; 3], pub normal: [f32; 3] }` (derives `Debug, PartialEq`)
  - `pub fn grass::place_grass(layout: &world::Layout) -> Vec<Blade>`
  - `pub fn grass::ground_normal(x: f64, z: f64) -> [f32; 3]`
  - `pub const grass::ATTEMPTS: u32`

- [ ] **Step 1: Move `GROUND_SIZE` into `world` and expose `lerp` to the crate**

In `native/meadow-core/src/world.rs`, right after `pub const SIZE …`:

```rust
pub const SIZE: f64 = 48.0; // placement area; the ground mesh is larger so no edge shows
pub const GROUND_SIZE: f64 = 64.0; // the ground mesh: larger than the placement area, so no screen corner shows its edge
```

and change `fn lerp` to `pub(crate) fn lerp` (body unchanged).

In `native/meadow/src/mesh.rs` delete line 21 (`const GROUND_SIZE: f64 = 64.0; …`), and add at the top of `ground()`:

```rust
pub fn ground() -> MeshData {
    use world::GROUND_SIZE;
    let n = (GROUND_SIZE / GROUND_CELL) as u32;
```

- [ ] **Step 2: Check nothing moved**

Run: `cd native && export PATH=$HOME/.cargo/bin:$PATH && cargo test`
Expected: all existing tests PASS (meadow-core world tests, meadow `shaders_validate`, mesh tests).

- [ ] **Step 3: Write `grass.rs` with its tests (one step: every function is a straight port of the web, and the tests check its rules)**

Add `pub mod grass;` to `native/meadow-core/src/lib.rs` (keep alphabetical order: `frame_pacer`, `grass`, `rng`, `world`).

Create `native/meadow-core/src/grass.rs`:

```rust
//! Grass blades over the whole ground, ported from the web's `src/grass.js`: dart throwing, thinned
//! by a density map (none on the beach, the paths or under rocks; sparse in noisy patches and under
//! tree crowns). Same `rand()` order as the web, but not the same blades: the hills and rocks already
//! differ, and the area is the 64-unit ground instead of the 48 placement area.
use libm::{ceil, floor, hypot, pow, sqrt};

use crate::rng::Mulberry32;
use crate::world::{GROUND_SIZE, Layout, Rock, SIZE, Tree, fbm, height_at, lerp, path_edge, sand_q, smoothstep};

pub const ATTEMPTS: u32 = 240_000; // the web's ~59 per square unit, over the whole ground
const HALF: f64 = GROUND_SIZE / 2.0 - 0.1; // just inside the ground
const SEED: u32 = 99;
const NORMAL_E: f64 = 0.25; // as the ground mesh's vertex normals: half its 0.5 cell

// Canopy mask: 4 cells per unit over the placement area (no tree grows outside it).
const MASK_RES: f64 = 4.0;
const MASK_N: usize = (SIZE * MASK_RES) as usize;

#[derive(Debug, PartialEq)]
pub struct Blade {
    pub x: f32,
    pub y: f32, // height_at(x, z)
    pub z: f32,
    pub scale: f32,       // 0.7–1.3
    pub color: [f32; 3],  // linear rgb
    pub normal: [f32; 3], // the ground's normal at the base: outlines only, blades are lit as flat ground
}

/// The cells under a tree crown, so each blade does one lookup instead of a loop over all trees.
pub struct CanopyMask(Vec<bool>);

impl CanopyMask {
    pub fn new(trees: &[Tree]) -> Self {
        let mut mask = vec![false; MASK_N * MASK_N];
        let cell = |v: f64| (v + SIZE / 2.0) * MASK_RES;
        for t in trees {
            let r = t.canopy;
            let lo = |v: f64| floor(cell(v)).max(0.0) as usize;
            let hi = |v: f64| (ceil(cell(v)).max(0.0) as usize).min(MASK_N);
            for gz in lo(t.z - r)..hi(t.z + r) {
                for gx in lo(t.x - r)..hi(t.x + r) {
                    let cx = (gx as f64 + 0.5) / MASK_RES - SIZE / 2.0;
                    let cz = (gz as f64 + 0.5) / MASK_RES - SIZE / 2.0;
                    if hypot(cx - t.x, cz - t.z) < r {
                        mask[gz * MASK_N + gx] = true;
                    }
                }
            }
        }
        Self(mask)
    }

    /// Outside the placement area counts as uncovered.
    pub fn covers(&self, x: f64, z: f64) -> bool {
        let cell = |v: f64| floor((v + SIZE / 2.0) * MASK_RES);
        let inside = |g: f64| (0.0..MASK_N as f64).contains(&g);
        let (gx, gz) = (cell(x), cell(z));
        inside(gx) && inside(gz) && self.0[gz as usize * MASK_N + gx as usize]
    }
}

/// The chance a dart at (x, z) becomes a blade: thin under crowns and in noisy patches; none on the
/// beach, the paths or under rocks, thickening back over a ragged edge past the first two.
pub fn grass_density(x: f64, z: f64, mask: &CanopyMask, rocks: &[Rock]) -> f64 {
    // Exact test, not the mask: a 0.25 cell would still let blades through small stones.
    if rocks.iter().any(|r| {
        let (dx, dz) = (r.x - x, r.z - z);
        dx * dx + dz * dz < r.r * r.r
    }) {
        return 0.0;
    }
    let shore = smoothstep(sand_q(x, z), 1.0, 1.1);
    let path = smoothstep(path_edge(x, z), 0.0, 0.8);
    let density = shore * path * lerp(0.15, 1.0, smoothstep(fbm(x * 0.15 + 50.0, z * 0.15), 0.25, 0.5));
    if mask.covers(x, z) { density * 0.25 } else { density }
}

/// Every blade on the ground for this layout (~160k). Seeded: the same layout gives the same blades.
pub fn place_grass(layout: &Layout) -> Vec<Blade> {
    let mut rng = Mulberry32::new(SEED);
    let mut rand = move || rng.next();
    let mask = CanopyMask::new(&layout.trees);
    let mut blades = Vec::new();
    for _ in 0..ATTEMPTS {
        let x = (rand() * 2.0 - 1.0) * HALF;
        let z = (rand() * 2.0 - 1.0) * HALF;
        if rand() > grass_density(x, z, &mask, &layout.rocks) {
            continue;
        }
        let scale = 0.7 + rand() * 0.6;
        let hue = 0.25 + rand() * 0.04;
        let lightness = 0.71 + rand() * 0.1;
        blades.push(Blade {
            x: x as f32,
            y: height_at(x, z) as f32,
            z: z as f32,
            scale: scale as f32,
            color: hsl_linear(hue, 0.5, lightness),
            normal: ground_normal(x, z),
        });
    }
    blades
}

/// The terrain normal as the ground mesh computes it (central differences of `height_at`), so the
/// outline pass sees no edge between a blade and the ground behind it.
pub fn ground_normal(x: f64, z: f64) -> [f32; 3] {
    let e = NORMAL_E;
    let dx = height_at(x + e, z) - height_at(x - e, z);
    let dz = height_at(x, z + e) - height_at(x, z - e);
    let len = sqrt(dx * dx + 4.0 * e * e + dz * dz);
    [(-dx / len) as f32, (2.0 * e / len) as f32, (-dz / len) as f32]
}

// three's Color.setHSL(h, s, l, SRGBColorSpace): HSL taken as sRGB, then decoded to linear.
fn hsl_linear(h: f64, s: f64, l: f64) -> [f32; 3] {
    let hi = if l <= 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let lo = 2.0 * l - hi;
    let channel = |t: f64| {
        let t = if t < 0.0 { t + 1.0 } else if t > 1.0 { t - 1.0 } else { t };
        let c = if t < 1.0 / 6.0 {
            lo + (hi - lo) * 6.0 * t
        } else if t < 0.5 {
            hi
        } else if t < 2.0 / 3.0 {
            lo + (hi - lo) * 6.0 * (2.0 / 3.0 - t)
        } else {
            lo
        };
        (if c < 0.04045 { c / 12.92 } else { pow((c + 0.055) / 1.055, 2.4) }) as f32
    };
    [channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use crate::world::place;

    // Placement takes a while in debug builds: the tests share one run.
    fn meadow() -> &'static (Layout, Vec<Blade>) {
        static MEADOW: OnceLock<(Layout, Vec<Blade>)> = OnceLock::new();
        MEADOW.get_or_init(|| {
            let layout = place(42);
            let blades = place_grass(&layout);
            (layout, blades)
        })
    }

    #[test]
    fn same_layout_gives_the_same_blades() {
        let (layout, blades) = meadow();
        assert_eq!(&place_grass(layout), blades);
    }

    #[test]
    fn acceptance_is_near_the_webs() {
        let ratio = meadow().1.len() as f64 / ATTEMPTS as f64;
        assert!((0.5..=0.85).contains(&ratio), "{ratio}");
    }

    #[test]
    fn blades_keep_off_rocks_beach_and_paths() {
        let tol = 1e-4; // positions are f32: ~4e-6 rounding at 32 units
        let (layout, blades) = meadow();
        for b in blades {
            let (x, z) = (b.x as f64, b.z as f64);
            for r in &layout.rocks {
                assert!(hypot(r.x - x, r.z - z) >= r.r - tol, "blade in a rock at {x}, {z}");
            }
            assert!(sand_q(x, z) >= 1.0 - tol, "blade on the beach at {x}, {z}");
            assert!(path_edge(x, z) >= -tol, "blade on a path at {x}, {z}");
        }
    }

    #[test]
    fn blades_sit_on_the_whole_ground() {
        let blades = &meadow().1;
        for b in blades {
            let (x, z) = (b.x as f64, b.z as f64);
            assert!(x.abs() <= HALF && z.abs() <= HALF);
            assert!((b.y as f64 - height_at(x, z)).abs() < 1e-4);
            assert!((0.7..=1.3).contains(&b.scale));
            let [nx, ny, nz] = b.normal;
            assert!(((nx * nx + ny * ny + nz * nz).sqrt() - 1.0).abs() < 1e-5 && ny > 0.7, "{:?}", b.normal);
        }
        // Out to the ground's edges, not just the 48 placement area.
        let far = |f: fn(&Blade) -> f32| blades.iter().map(|b| f(b).abs()).fold(0.0, f32::max);
        assert!(far(|b| b.x) > 31.0 && far(|b| b.z) > 31.0);
    }

    #[test]
    fn mask_covers_exactly_the_cells_under_crowns() {
        let trees = &meadow().0.trees;
        let mask = CanopyMask::new(trees);
        assert!(trees.iter().all(|t| mask.covers(t.x, t.z)));
        for gz in 0..MASK_N {
            for gx in 0..MASK_N {
                let cx = (gx as f64 + 0.5) / MASK_RES - SIZE / 2.0;
                let cz = (gz as f64 + 0.5) / MASK_RES - SIZE / 2.0;
                let under = trees.iter().any(|t| hypot(cx - t.x, cz - t.z) < t.canopy);
                assert_eq!(mask.covers(cx, cz), under, "cell {gx}, {gz}");
            }
        }
        // Outside the placement area: uncovered, with no out-of-range lookup.
        for (x, z) in [(-24.01, 0.0), (24.0, 0.0), (0.0, 31.9), (-31.9, -31.9)] {
            assert!(!mask.covers(x, z));
        }
    }

    #[test]
    fn crowns_thin_the_grass_to_a_quarter() {
        let layout = &meadow().0;
        let mask = CanopyMask::new(&layout.trees);
        let mut under = 0;
        for t in &layout.trees {
            for (dx, dz) in [(0.0, 0.0), (0.2, 0.1), (-0.1, -0.25)] {
                let (x, z) = (t.x + dx, t.z + dz);
                if mask.covers(x, z) {
                    under += 1;
                    assert!(grass_density(x, z, &mask, &layout.rocks) <= 0.25);
                }
            }
        }
        assert!(under > 0);
    }

    #[test]
    fn hsl_matches_threes_set_hsl() {
        let linear = |c: f64| pow((c + 0.055) / 1.055, 2.4) as f32; // every value here is above 0.04045
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(close(hsl_linear(0.0, 1.0, 0.5), [1.0, 0.0, 0.0]));
        assert!(close(hsl_linear(1.0 / 3.0, 1.0, 0.5), [0.0, 1.0, 0.0]));
        // A grass colour: yellow-green, lightness above 0.5 → sRGB (0.75, 0.875, 0.625).
        assert!(close(hsl_linear(0.25, 0.5, 0.75), [linear(0.75), linear(0.875), linear(0.625)]));
    }
}
```

- [ ] **Step 4: Run the core tests**

Run: `cd native && export PATH=$HOME/.cargo/bin:$PATH && cargo test -p meadow-core`
Expected: all PASS, including the 7 new `grass::tests::*`.
If `acceptance_is_near_the_webs` fails, print the ratio and **stop and report** (it checks the spec's 160–170k estimate; a far-off ratio means a port bug, not a test to loosen). Same for a failing exclusion test: report the position, don't widen `tol`.

- [ ] **Step 5: Record placement time in release (for the spec's "tens of ms" estimate)**

Run: `cd native && cargo test -p meadow-core --release grass -- --nocapture 2>&1 | tail -5` and note the wall time of the test run. This is only a rough number (two placements run); the real number comes from Task 2's log. Report it in the task summary.

- [ ] **Step 6: Commit**

```bash
git add native/meadow-core/src/world.rs native/meadow-core/src/lib.rs native/meadow-core/src/grass.rs native/meadow/src/mesh.rs
git commit -m "native: grass placement in meadow-core (web density map over the 64 ground, ground normals, sRGB HSL palette)"
```

---

### Task 2: grass pipeline (everything placed is drawn)

**Files:**
- Create: `native/meadow/src/grass.rs`
- Modify: `native/meadow/src/main.rs` (module list)
- Modify: `native/meadow/src/render.rs` (imports, `prelude`, `scene_pipeline`, `build_draws`, `Renderer`, `Renderer::new`, `render`)
- Modify: `native/meadow/src/shaders/scene.wgsl` (append)

**Interfaces:**
- Consumes: `meadow_core::grass::{Blade, place_grass, ground_normal}`, `meadow_core::world::Layout`.
- Produces (used by Task 3):
  - `pub struct GrassInstance { pos: [f32; 3], color_scale: [u8; 4], normal: [i8; 4] }` with `fn new(&Blade) -> Self`
  - `pub const grass::BUFFERS: [Option<wgpu::VertexBufferLayout>; 1]`, `pub const BLADE_W: f64`, `pub const BLADE_H: f64`
  - `pub struct Grass { pub instances: wgpu::Buffer, pub count: u32 }` with `pub fn new(device, &Layout) -> Self`
  - `scene_pipeline(device, layout, module, (vs, fs): (&str, &str), buffers, cull_mode)`

- [ ] **Step 1: Write the GPU-side module with its tests**

Create `native/meadow/src/grass.rs`:

```rust
//! The grass on the GPU: one 20-byte instance per blade, drawn as one triangle that `vs_grass`
//! builds from the vertex index (no vertex buffer).
use bytemuck::{Pod, Zeroable};
use meadow_core::grass::{self, Blade};
use meadow_core::world;
use wgpu::util::DeviceExt;

// Filled into the shader by render.rs.
pub const BLADE_W: f64 = 0.1;
pub const BLADE_H: f64 = 0.3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct GrassInstance {
    pos: [f32; 3],        // base, on the ground
    color_scale: [u8; 4], // linear rgb, (scale − 0.7) / 0.6
    normal: [i8; 4],      // ground normal xyz, 0
}

impl GrassInstance {
    fn new(blade: &Blade) -> Self {
        let unorm = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let snorm = |v: f32| (v.clamp(-1.0, 1.0) * 127.0).round() as i8;
        let ([r, g, b], [nx, ny, nz]) = (blade.color, blade.normal);
        Self {
            pos: [blade.x, blade.y, blade.z],
            color_scale: [unorm(r), unorm(g), unorm(b), unorm((blade.scale - 0.7) / 0.6)],
            normal: [snorm(nx), snorm(ny), snorm(nz), 0],
        }
    }
}

const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4, 2 => Snorm8x4];
pub const BUFFERS: [Option<wgpu::VertexBufferLayout>; 1] =
    [Some(wgpu::VertexBufferLayout { array_stride: size_of::<GrassInstance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &ATTRS })];

pub struct Grass {
    pub instances: wgpu::Buffer,
    pub count: u32, // blades drawn
}

impl Grass {
    pub fn new(device: &wgpu::Device, layout: &world::Layout) -> Self {
        let start = std::time::Instant::now();
        let blades = grass::place_grass(layout);
        let instances: Vec<_> = blades.iter().map(GrassInstance::new).collect();
        log::info!("grass: {} blades placed in {:.0} ms", blades.len(), start.elapsed().as_secs_f64() * 1000.0);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grass"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self { instances: buffer, count: instances.len() as u32 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_packs_into_20_bytes() {
        assert_eq!(size_of::<GrassInstance>(), 20);
        let blade = Blade { x: 1.5, y: -0.25, z: 30.0, scale: 1.3, color: [0.0, 0.5, 1.0], normal: [-0.6, 0.8, 0.0] };
        let i = GrassInstance::new(&blade);
        assert_eq!(i.pos, [1.5, -0.25, 30.0]);
        assert_eq!(i.color_scale, [0, 128, 255, 255]);
        assert_eq!(i.normal, [-76, 102, 0, 0]);
        assert_eq!(GrassInstance::new(&Blade { scale: 0.7, ..blade }).color_scale[3], 0);
    }

    #[test]
    fn blade_normals_match_the_ground_mesh() {
        // The outline pass compares a blade's normal with the ground behind it: they must agree.
        let ground = crate::mesh::ground();
        for v in ground.vertices.iter().step_by(97) {
            let n = grass::ground_normal(v.pos[0] as f64, v.pos[2] as f64);
            assert!(n.iter().zip(v.normal).all(|(a, b)| (a - b).abs() < 1e-5), "{n:?} vs {:?}", v.normal);
        }
    }
}
```

Add `mod grass;` to `native/meadow/src/main.rs` between `mod gpu;` and `mod mesh;`.

- [ ] **Step 2: Run the new tests**

Run: `cd native && export PATH=$HOME/.cargo/bin:$PATH && cargo test -p meadow grass`
Expected: both PASS. (`Grass` is unused until Step 5; a dead-code warning is fine here.)

- [ ] **Step 3: Add the shaders**

Append to `native/meadow/src/shaders/scene.wgsl`:

```wgsl
// Grass: one triangle per blade, built from the vertex index (no vertex buffer). BLADE_W and
// BLADE_H: filled in by render.rs.
struct GrassIn {
    @builtin(vertex_index) corner: u32,
    @location(0) pos: vec3f,         // base, on the ground
    @location(1) color_scale: vec4f, // linear rgb, (scale - 0.7) / 0.6
    @location(2) normal: vec4f,      // the ground's normal at the base
}

struct GrassOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) color: vec3f,
    @location(2) ground_normal: vec3f,
}

@vertex
fn vs_grass(v: GrassIn) -> GrassOut {
    // Corners (±w/2, 0) and (0, h) in the blade's plane, turned to face the camera (yaw π/4).
    var local = vec3f(0.0, BLADE_H, 0.0);
    if v.corner < 2u {
        let x = select(-0.5, 0.5, v.corner == 1u) * BLADE_W;
        local = vec3f(x * 0.7071, 0.0, -x * 0.7071);
    }
    // Travelling gust keyed on the base, tip only, in local space before the scale (as the web).
    let tip = local.y / BLADE_H;
    let t = g.params.x;
    var sway = sin(t * 1.6 - (v.pos.x + v.pos.z) * 0.45) * 0.5 + 0.5;
    sway = sway * 0.12 + sin(t * 3.1 + v.pos.x * 2.0) * 0.02;
    local += vec3f(0.7071, 0.0, -0.7071) * sway * tip;
    let world = v.pos + local * (0.7 + 0.6 * v.color_scale.a);
    var out: GrassOut;
    out.clip = g.view_proj * vec4f(world, 1.0);
    out.world = world;
    out.color = v.color_scale.rgb * mix(0.85, 1.12, tip); // darker at the root (linear in tip: exact per vertex)
    out.ground_normal = v.normal.xyz;
    return out;
}

// Lit as flat ground (normal up, as the web), but outlined with the ground's own normal, so a blade
// on a slope doesn't read as an edge against the ground behind it.
@fragment
fn fs_grass(in: GrassOut) -> SceneOut {
    let up = vec3f(0.0, 1.0, 0.0);
    let lit = shadow(in.world, up);
    let color = lighting(in.color, up, toon(dot(up, g.sun_dir.xyz)), lit);
    return SceneOut(vec4f(color, 1.0), view_normal(in.ground_normal));
}
```

In `native/meadow/src/render.rs`, add the import and the two prelude constants:

```rust
use crate::camera;
use crate::grass::{self, Grass};
use crate::mesh::{self, MeshData, Vertex};
```

```rust
        ("BRANCH_WIDTH", BRANCH_WIDTH),
        ("BLADE_W", grass::BLADE_W),
        ("BLADE_H", grass::BLADE_H),
    ];
```

- [ ] **Step 4: Validate the shader**

Run: `cd native && cargo test -p meadow shaders_validate`
Expected: PASS (naga parses and validates `vs_grass` / `fs_grass` with the prelude's `BLADE_*`).

- [ ] **Step 5: Pipeline, buffer and draw**

In `render.rs`, generalise `scene_pipeline` (only the signature and two fields change):

```rust
fn scene_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    (vs, fs): (&str, &str),
    buffers: &[Option<wgpu::VertexBufferLayout>],
    cull_mode: Option<wgpu::Face>,
) -> wgpu::RenderPipeline {
    let target = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
        primitive: wgpu::PrimitiveState { cull_mode, ..Default::default() },
```

(the rest of the function is unchanged).

`build_draws` takes the layout instead of placing it:

```rust
// Every mesh in the meadow, as instanced draws.
fn build_draws(device: &wgpu::Device, layout: &world::Layout) -> Vec<Draw> {
    log::info!("layout: {} trees, {} rocks", layout.trees.len(), layout.rocks.len());
```

(delete the `let layout = world::place(42);` line; the body is otherwise unchanged).

`Renderer` gains two fields, after `water_pipeline` and after `draws`:

```rust
    water_pipeline: wgpu::RenderPipeline,
    grass_pipeline: wgpu::RenderPipeline,
```

```rust
    draws: Vec<Draw>,
    grass: Grass,
```

In `Renderer::new`, the pipeline calls become:

```rust
        let scene_layout = pipeline_layout("scene", &[Some(&globals_layout), Some(&shadow_layout)]);
        let toon_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_toon"), &BUFFERS, Some(wgpu::Face::Back));
        let ground_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_ground"), &BUFFERS, Some(wgpu::Face::Back));
        let water_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_water"), &BUFFERS, Some(wgpu::Face::Back));
        // No culling: a blade faces the camera, and the wind keeps its tip in that plane.
        let grass_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_grass", "fs_grass"), &grass::BUFFERS, None);
```

and the layout is placed once, before `let targets = …`:

```rust
        // Placed once: the trees and rocks also thin the grass.
        let world_layout = world::place(42);
        let targets = Self::targets(device, &post_layout, &post_buf, art_size(width, height, pixel_size));
```

with the struct literal gaining:

```rust
            water_pipeline,
            grass_pipeline,
```

```rust
            draws: build_draws(device, &world_layout),
            grass: Grass::new(device, &world_layout),
```

In `render`, after the `for d in &self.draws { … }` loop inside the scene pass block:

```rust
            pass.set_pipeline(&self.grass_pipeline);
            pass.set_vertex_buffer(0, self.grass.instances.slice(..));
            pass.draw(0..3, 0..self.grass.count);
```

- [ ] **Step 6: Build and test**

Run: `cd native && cargo test && cargo build --target x86_64-pc-windows-gnu`
Expected: tests PASS; the Windows build has no warnings from our code.

- [ ] **Step 7: Look at it on Windows (debug copy, the wallpaper untouched)**

Patch the scratch screenshot script once (outside the repo) so it can target the debug copy:

```bash
sed -i 's/^param(\[string\]\$out, \[int\]\$w, \[int\]\$h, \[switch\]\$minimise)/param([string]$out, [int]$w, [int]$h, [switch]$minimise, [string]$name = "meadow")/; s/^\$p = Get-Process meadow /$p = Get-Process $name /' /mnt/c/Users/d/pixel3d/check.ps1
head -12 /mnt/c/Users/d/pixel3d/check.ps1   # confirm both lines changed
```

Launch the debug copy (console logs go to a file):

```bash
cp target/x86_64-pc-windows-gnu/debug/meadow.exe /mnt/c/Users/d/pixel3d/meadow-dev.exe
powershell.exe -NoProfile -c "Start-Process C:\Users\d\pixel3d\meadow-dev.exe -ArgumentList '--fps','60' -WorkingDirectory C:\Users\d\pixel3d -RedirectStandardError C:\Users\d\pixel3d\dev-log.txt"
sleep 20   # debug placement is slow
cat /mnt/c/Users/d/pixel3d/dev-log.txt | grep -E "grass|layout|art|panic|ERROR"
powershell.exe -NoProfile -File 'C:\Users\d\pixel3d\check.ps1' -out 'C:\Users\d\pixel3d\grass-t2.png' -w 1296 -h 759 -name meadow-dev
```

Read `/mnt/c/Users/d/pixel3d/grass-t2.png` and check:
- grass covers the ground except the beach, paths and rocks, thinner under trees;
- no bright single-blade outlines on the hills;
- tree shadows darken the grass;
- no panic or validation error in `dev-log.txt`; the `grass: N blades placed` line shows N ≈ 160–170k.

Then show the user the screenshot and ask them to watch the live window for the sway. Stop the copy afterwards:

```bash
powershell.exe -NoProfile -c "Get-Process meadow-dev -ErrorAction SilentlyContinue | Stop-Process -Force"
```

- [ ] **Step 8: Commit**

```bash
git add native/meadow/src/grass.rs native/meadow/src/main.rs native/meadow/src/render.rs native/meadow/src/shaders/scene.wgsl
git commit -m "native: grass pipeline — 20-byte instances, one triangle per blade from vertex_index, web wind, ground-normal outlines"
```

---

### Task 3: draw only the blades on screen

**Files:**
- Modify: `native/meadow/src/camera.rs` (`VIEW_HEIGHT`, `projection`)
- Modify: `native/meadow/src/grass.rs` (`Grass`, new `sort_for_view`, `drawn`, tests)
- Modify: `native/meadow/src/render.rs` (`set_camera`, `resize` log)

**Interfaces:**
- Consumes: Task 2's `GrassInstance::new`, `Grass`.
- Produces: `pub const camera::VIEW_HEIGHT: f32`, `pub fn camera::half_width(art_w: u32, art_h: u32) -> f32`, `pub fn Grass::set_view(&mut self, art: (u32, u32))`.

- [ ] **Step 1: One half-width for the projection and the culling**

In `native/meadow/src/camera.rs`:

```rust
pub const VIEW_HEIGHT: f32 = 18.0; // world units visible vertically
```

```rust
/// Half the visible width in world units. Aspect from the art size, not the window, so every art
/// pixel is square.
pub fn half_width(art_w: u32, art_h: u32) -> f32 {
    VIEW_HEIGHT / 2.0 * art_w as f32 / art_h as f32
}

pub fn projection(art_w: u32, art_h: u32) -> Mat4 {
    let half_h = VIEW_HEIGHT / 2.0;
    let half_w = half_width(art_w, art_h);
```

(the old `/// Aspect from the art size…` doc line moves to `half_width`; the rest of `projection` is unchanged).

- [ ] **Step 2: Write the failing tests**

Add to the `tests` module in `native/meadow/src/grass.rs` (`Vec3` and `camera` come in through `use super::*` once Step 3 imports them):

```rust
    #[test]
    fn view_sort_keeps_the_screen_rows_in_order() {
        // A grid over the whole ground, 0.5 apart.
        let blades: Vec<Blade> = (-63..=63)
            .flat_map(|i| (-63..=63).map(move |j| Blade { x: i as f32 * 0.5, y: 0.0, z: j as f32 * 0.5, scale: 1.0, color: [0.5; 3], normal: [0.0, 1.0, 0.0] }))
            .collect();
        let view = camera::view();
        let (instances, edges) = sort_for_view(&blades, view);
        assert_eq!(instances.len(), edges.len());
        assert!(edges.len() < blades.len(), "rows above and below the screen are dropped");
        assert!(edges.windows(2).all(|w| w[0] <= w[1]));
        for (inst, &edge) in instances.iter().zip(&edges) {
            let p = view.transform_point3(Vec3::from(inst.pos));
            assert!(p.y.abs() < camera::VIEW_HEIGHT / 2.0 + VIEW_MARGIN);
            assert_eq!(p.x.abs(), edge);
        }
    }

    #[test]
    fn drawn_count_follows_the_aspect() {
        let edges: Vec<f32> = (0..4000).map(|i| i as f32 * 0.01).collect(); // |view x| 0..40
        let count = |w, h| drawn(&edges, (w, h));
        // Wider draws more (4:3 < 16:10 < 16:9 < 21:9); only the aspect matters, not the size.
        assert!(count(640, 480) < count(640, 400) && count(640, 400) < count(640, 360) && count(640, 360) < count(840, 360));
        assert_eq!(count(640, 360), count(960, 540));
        // 16:9: half width 16 plus the 0.6 margin.
        assert!((1655..=1665).contains(&count(640, 360)), "{}", count(640, 360));
        // Never more than kept, even far wider than the ground; portrait and 1×1 still draw.
        assert_eq!(count(5000, 100), edges.len() as u32);
        assert!(count(1, 1) > 0 && count(270, 480) > 0);
    }
```

Run: `cd native && export PATH=$HOME/.cargo/bin:$PATH && cargo test -p meadow grass`
Expected: FAIL to compile — `sort_for_view`, `drawn` and `VIEW_MARGIN` don't exist yet.

- [ ] **Step 3: Implement the sort and the count**

In `native/meadow/src/grass.rs`, update the module doc and imports:

```rust
//! The grass on the GPU: one 20-byte instance per blade, drawn as one triangle that `vs_grass`
//! builds from the vertex index (no vertex buffer). The camera never moves and the view is always
//! VIEW_HEIGHT high, so the blades are sorted once (as the web's fitView) and a resize only
//! changes how many are drawn.
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use meadow_core::grass::{self, Blade};
use meadow_core::world;
use wgpu::util::DeviceExt;

use crate::camera;

// Filled into the shader by render.rs.
pub const BLADE_W: f64 = 0.1;
pub const BLADE_H: f64 = 0.3;
const VIEW_MARGIN: f32 = 0.6; // world units kept past the screen edges: blade height, width and sway
```

Replace `Grass` and its `impl`:

```rust
pub struct Grass {
    pub instances: wgpu::Buffer,
    edges: Vec<f32>, // |view x| of the uploaded blades, ascending
    pub count: u32,  // blades drawn: the first `count`, those inside the screen's sides
}

impl Grass {
    pub fn new(device: &wgpu::Device, layout: &world::Layout) -> Self {
        let start = std::time::Instant::now();
        let blades = grass::place_grass(layout);
        let (instances, edges) = sort_for_view(&blades, camera::view());
        log::info!("grass: {} blades placed, {} kept in the screen's rows, in {:.0} ms", blades.len(), edges.len(), start.elapsed().as_secs_f64() * 1000.0);
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grass"),
            contents: bytemuck::cast_slice(&instances),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self { instances: buffer, edges, count: 0 } // set_view sets the count
    }

    /// New art size: draw the blades inside the screen's sides.
    pub fn set_view(&mut self, art: (u32, u32)) {
        self.count = drawn(&self.edges, art);
    }
}

/// Drops the blades above or below the screen (at any window size) and sorts the rest by distance
/// from the screen's vertical centre line. Returns the instances and their |view x|, ascending.
fn sort_for_view(blades: &[Blade], view: Mat4) -> (Vec<GrassInstance>, Vec<f32>) {
    let mut kept: Vec<(f32, GrassInstance)> = blades
        .iter()
        .filter_map(|b| {
            let p = view.transform_point3(Vec3::new(b.x, b.y, b.z));
            (p.y.abs() < camera::VIEW_HEIGHT / 2.0 + VIEW_MARGIN).then(|| (p.x.abs(), GrassInstance::new(b)))
        })
        .collect();
    kept.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let (edges, instances) = kept.into_iter().unzip();
    (instances, edges)
}

// Uses the projection's own half width, so the culling and the camera can't disagree.
fn drawn(edges: &[f32], (w, h): (u32, u32)) -> u32 {
    let half_w = camera::half_width(w, h);
    edges.partition_point(|&x| x < half_w + VIEW_MARGIN) as u32
}
```

In `native/meadow/src/render.rs`:

```rust
    fn set_camera(&mut self, (w, h): (u32, u32)) {
        self.globals.view_proj = (camera::projection(w, h) * camera::view()).to_cols_array_2d();
        self.grass.set_view((w, h));
    }
```

and in `resize` the log line becomes:

```rust
        log::info!("art {}x{} at pixel size {}, {} grass blades drawn", art.0, art.1, self.pixel_size, self.grass.count);
```

- [ ] **Step 4: Run all tests and the Windows build**

Run: `cd native && cargo test && cargo build --target x86_64-pc-windows-gnu`
Expected: all PASS, no warnings from our code.

- [ ] **Step 5: Check the sides at every aspect and pixel size**

Rebuild and copy the debug exe as in Task 2 Step 7, then for each pixel size launch it and take four shots (`-w/-h` are the outer window size; the log gives the exact art size):

```bash
for px in 2 3 4; do
  powershell.exe -NoProfile -c "Start-Process C:\Users\d\pixel3d\meadow-dev.exe -ArgumentList '--fps','60','--pixel-size','$px' -WorkingDirectory C:\Users\d\pixel3d -RedirectStandardError C:\Users\d\pixel3d\dev-log.txt"
  sleep 20
  for wh in "1296 999:43" "1296 839:1610" "1296 759:169" "1696 759:219"; do
    size=${wh%%:*}; tag=${wh##*:}
    powershell.exe -NoProfile -File 'C:\Users\d\pixel3d\check.ps1' -out "C:\Users\d\pixel3d\grass-px$px-$tag.png" -w ${size% *} -h ${size#* } -name meadow-dev
  done
  grep -E "grass|art " /mnt/c/Users/d/pixel3d/dev-log.txt
  powershell.exe -NoProfile -c "Get-Process meadow-dev -ErrorAction SilentlyContinue | Stop-Process -Force"
done
```

Check every screenshot: grass reaches both side edges of the window (no bare strip at the left or right). Check the log: the drawn count rises with the aspect, about 35k (4:3), 41k (16:10), 47k (16:9), 61k (21:9), and is the same across pixel sizes for the same aspect. Report the counts and the kept count.

- [ ] **Step 6: Commit**

```bash
git add native/meadow/src/camera.rs native/meadow/src/grass.rs native/meadow/src/render.rs
git commit -m "native: draw only the grass on screen — rows culled and sorted by |view x| once, count set with the camera"
```

---

### Task 4: measure against `main` and compare with the web

No code. **Ask the user before Step 2**: one A/B round takes ~4 min, their wallpaper is stopped meanwhile and restarted afterwards, and the desktop must stay visible (no maximised window) or the wallpaper pauses.

**Files:**
- Modify: `docs/perf-baseline.md` (a new subsection under "Native (Rust + wgpu) RAM")

- [ ] **Step 1: Build both release exes**

```bash
cd /home/dung/Study/pixel3d
git worktree add ../pixel3d-main main
(cd ../pixel3d-main/native && CARGO_TARGET_DIR=/home/dung/Study/pixel3d/native/target cargo build --release --target x86_64-pc-windows-gnu)
cp native/target/x86_64-pc-windows-gnu/release/meadow.exe /mnt/c/Users/d/pixel3d/meadow-main.exe
(cd native && cargo build --release --target x86_64-pc-windows-gnu)
cp native/target/x86_64-pc-windows-gnu/release/meadow.exe /mnt/c/Users/d/pixel3d/meadow-grass.exe
git worktree remove ../pixel3d-main
```

Then write the measuring script to `/mnt/c/Users/d/pixel3d/ab.ps1`:

```powershell
param([string]$exe)
$p = Start-Process $exe -ArgumentList '--wallpaper' -WorkingDirectory C:\Users\d\pixel3d -PassThru
Start-Sleep 60
$p.Refresh(); $ws = [int]($p.WorkingSet64 / 1MB); $priv = [int]($p.PrivateMemorySize64 / 1MB)
$c0 = $p.TotalProcessorTime.TotalMilliseconds; $t0 = Get-Date
$gpu = Get-Counter "\GPU Engine(pid_$($p.Id)*engtype_3D)\Utilization Percentage" -SampleInterval 1 -MaxSamples 20
$p.Refresh(); $cpu = ($p.TotalProcessorTime.TotalMilliseconds - $c0) / ((Get-Date) - $t0).TotalMilliseconds * 100
$g = ($gpu.CounterSamples | Measure-Object CookedValue -Sum).Sum / 20
"$(Split-Path $exe -Leaf): ws $ws MB, private $priv MB, cpu $([math]::Round($cpu, 1)) %, gpu 3D $([math]::Round($g, 1)) %"
Stop-Process $p -Force
```

- [ ] **Step 2: Run one A/B round (after the user's OK)**

```bash
powershell.exe -NoProfile -c "Get-CimInstance Win32_Process -Filter \"Name like 'meadow%'\" | Select-Object ProcessId, CommandLine | Format-List"
```

Note the user's wallpaper command line, stop that process by its id (`Stop-Process -Id <id>`), then:

```bash
powershell.exe -NoProfile -File 'C:\Users\d\pixel3d\ab.ps1' -exe 'C:\Users\d\pixel3d\meadow-main.exe'
powershell.exe -NoProfile -File 'C:\Users\d\pixel3d\ab.ps1' -exe 'C:\Users\d\pixel3d\meadow-grass.exe'
grep -E "grass|art " /mnt/c/Users/d/AppData/Roaming/pixel3d/log.txt   # the grass run's log, before the wallpaper truncates it
```

Restart the user's wallpaper with the noted command line (`Start-Process <exe> -ArgumentList '--wallpaper'`) and confirm it runs. A GPU difference under ~1 point is reported as within noise. A second round only on the user's OK.

- [ ] **Step 3: Record**

Append to `docs/perf-baseline.md`, after the "Wallpaper mode" paragraph of the static-scene section:

```markdown
### Grass (sub-project 2, T4)

`--wallpaper`, 1920×1080, pixel size 2, 15 fps, Vulkan, one A/B round. RAM after 60 s; CPU (% of one core) and GPU 3D utilisation over the next 20 s.

| exe | working set (MB) | private (MB) | CPU | GPU 3D | grass |
|---|---|---|---|---|---|
| main `<sha>` | … | … | … % | … % | – |
| feat/native-grass `<sha>` | … | … | … % | … % | <placed> placed, <kept> kept, <drawn> drawn, placed in <ms> ms |
```

with the measured numbers, plus one line of findings (RAM/CPU/GPU deltas, which are within noise).

- [ ] **Step 4: Screenshots next to the web**

Native: launch the debug copy as in Task 2 Step 7 and take `-w 1296 -h 759`; note the client size it prints.
Web (dev server from `pixel3d/`: `npm run dev`, in the background), with the same client size and pixel size 2, the slider at 12:00:

```bash
cat > /tmp/web-shot.mjs <<'EOF'
const { chromium } = await import('/home/dung/.npm/_npx/e41f203b7505f1fb/node_modules/playwright-core/index.mjs');
const [w, h] = [+process.argv[2], +process.argv[3]];
const browser = await chromium.launch({
  executablePath: process.env.HOME + '/.cache/ms-playwright/chromium_headless_shell-1234/chrome-headless-shell-linux64/chrome-headless-shell',
  args: ['--use-angle=gl', '--ignore-gpu-blocklist', '--enable-gpu', '--no-sandbox'],
  env: { ...process.env, GALLIUM_DRIVER: 'd3d12', MESA_LOADER_DRIVER_OVERRIDE: 'd3d12', LD_LIBRARY_PATH: '/usr/lib/wsl/lib' },
});
const page = await browser.newPage({ viewport: { width: w, height: h } });
await page.goto('http://127.0.0.1:5173/?px=2');
await page.evaluate(() => { const s = document.querySelector('#time input'); s.value = 12; s.dispatchEvent(new Event('input')); });
await page.waitForTimeout(3000);
await page.screenshot({ path: '/mnt/c/Users/d/pixel3d/web-grass.png' });
await browser.close();
EOF
node /tmp/web-shot.mjs <client w> <client h>
```

Show the user both, side by side, plus a 4× zoomed crop of the same patch of tall blades from each (the spec's depth-edge risk: tall tips darkened by up to 40 % on both is expected; only a difference matters). Ask the user to watch the native wind live next to the web.

- [ ] **Step 5: The user decides on density**

Present the numbers and the screenshots, and ask whether to keep the web density or apply the spec's tricks (in order: fewer/wider blades → ground tint → average colour → tuft quads). Any trick is a new task after this plan.

- [ ] **Step 6: Commit the measurement**

```bash
git add docs/perf-baseline.md
git commit -m "docs: native grass RAM/CPU/GPU against main (SP2 T4)"
```

---

## Notes for the executor

- Placement cost: ~240k density calls plus ~5 `height_at` calls per blade, all in software `libm`. Expect tens to a few hundred ms in release, seconds in debug. The log line in Task 2 gives the real number. Over ~300 ms in release, report it: the spec's fallback is caching in `App` across an Explorer restart, which is a separate decision.
- Startup transient: the `Vec<Blade>` grows by doubling to ~262k × 40 B, so ~16 MB peak for a moment (the spec's "~5 MB" assumed an exact-size vector). Large allocations go straight back to the OS, so this doesn't show in the steady-state RAM. Report it; don't optimise unless T4's numbers ask for it.
- `cargo test` builds `meadow` for Linux: the Windows-only modules are `cfg`-gated, so the grass tests run in WSL.
