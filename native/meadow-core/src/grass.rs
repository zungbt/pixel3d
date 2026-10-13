//! Grass blades over the whole ground, ported from the web's `src/grass.js`: dart throwing, thinned
//! by a density map (none on the beach, the paths or under rocks; sparse in noisy patches and under
//! tree crowns). Same `rand()` order as the web, but not the same blades: the hills and rocks already
//! differ, and the area is the 64-unit ground instead of the 48 placement area.
use libm::{ceil, floor, hypot, pow, sqrt};

use crate::rng::Mulberry32;
use crate::world::{Area, GROUND_CELL, GROUND_SIZE, Layout, Rock, SIZE, Tree, fbm, height_at, lerp, path_edge, sand_q, smoothstep};

pub const BLADE_H: f64 = 0.3; // also the wind's height scale for the flowers and the perched insects
pub const ATTEMPTS: u32 = 240_000; // the web's ~59 per square unit, over the whole ground
const SEED: u32 = 99;
const NORMAL_E: f64 = GROUND_CELL / 2.0; // as the ground mesh's vertex normals

// Canopy mask: 4 cells per unit (no tree grows outside its area).
const MASK_RES: f64 = 4.0;

#[derive(Debug, PartialEq)]
pub struct Blade {
    pub x: f32,
    pub y: f32, // on the ground mesh
    pub z: f32,
    pub scale: f32,       // 0.7–1.3
    pub color: [f32; 3],  // linear rgb
    pub normal: [f32; 3], // the ground's normal at the base: outlines only, blades are lit as flat ground
}

/// The cells under a tree crown, so each blade does one lookup instead of a loop over all trees.
pub struct CanopyMask {
    n: usize,       // cells per side
    lo: (f64, f64), // the area's low corner
    cells: Vec<bool>,
}

impl CanopyMask {
    /// Over the placement area.
    pub fn new(trees: &[Tree]) -> Self {
        Self::over(trees, Area { x: 0.0, z: 0.0, size: SIZE })
    }

    pub fn over(trees: &[Tree], area: Area) -> Self {
        let n = (area.size * MASK_RES) as usize;
        let lo = (area.x - area.size / 2.0, area.z - area.size / 2.0);
        let mut cells = vec![false; n * n];
        let cell = |v: f64, l: f64| (v - l) * MASK_RES;
        for t in trees {
            let r = t.canopy;
            let from = |v: f64, l: f64| floor(cell(v, l)).max(0.0) as usize;
            let to = |v: f64, l: f64| (ceil(cell(v, l)).max(0.0) as usize).min(n);
            for gz in from(t.z - r, lo.1)..to(t.z + r, lo.1) {
                for gx in from(t.x - r, lo.0)..to(t.x + r, lo.0) {
                    let cx = (gx as f64 + 0.5) / MASK_RES + lo.0;
                    let cz = (gz as f64 + 0.5) / MASK_RES + lo.1;
                    if hypot(cx - t.x, cz - t.z) < r {
                        cells[gz * n + gx] = true;
                    }
                }
            }
        }
        Self { n, lo, cells }
    }

    /// Outside the area counts as uncovered.
    pub fn covers(&self, x: f64, z: f64) -> bool {
        let cell = |v: f64, l: f64| floor((v - l) * MASK_RES);
        let inside = |g: f64| (0.0..self.n as f64).contains(&g);
        let (gx, gz) = (cell(x, self.lo.0), cell(z, self.lo.1));
        inside(gx) && inside(gz) && self.cells[gz as usize * self.n + gx as usize]
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
    let mask = CanopyMask::new(&layout.trees);
    let ground = Area { x: 0.0, z: 0.0, size: GROUND_SIZE };
    place_blades(&mut Mulberry32::new(SEED), &mask, &layout.rocks, ground, ATTEMPTS)
}

/// Blades over one square of ground, darts from the stream `rng`, thinned by `grass_density`.
/// Roots sit on the square's own ground mesh (`GroundGrid`).
pub fn place_blades(rng: &mut Mulberry32, mask: &CanopyMask, rocks: &[Rock], area: Area, attempts: u32) -> Vec<Blade> {
    let mut rand = || rng.next();
    let half = area.size / 2.0 - 0.1; // just inside the square
    let ground = GroundGrid::new(area);
    let mut blades = Vec::new();
    for _ in 0..attempts {
        let x = area.x + (rand() * 2.0 - 1.0) * half;
        let z = area.z + (rand() * 2.0 - 1.0) * half;
        if rand() > grass_density(x, z, mask, rocks) {
            continue;
        }
        let scale = 0.7 + rand() * 0.6;
        let hue = 0.25 + rand() * 0.04;
        let lightness = 0.71 + rand() * 0.1;
        let (y, normal) = ground.at(x, z);
        blades.push(Blade { x: x as f32, y, z: z as f32, scale: scale as f32, color: hsl_linear(hue, 0.5, lightness), normal });
    }
    blades
}

/// Heights and normals at a square's mesh vertices (every GROUND_CELL, as the ground mesh draws it).
/// Each blade interpolates them over the triangle the GPU draws under it, so its root sits on the
/// drawn ground, at a fifth of the `height_at` calls.
pub(crate) struct GroundGrid {
    x0: f64, // the low corner
    z0: f64,
    n: usize, // cells per side
    height: Vec<f64>,
    normal: Vec<[f32; 3]>,
}

impl GroundGrid {
    pub(crate) fn new(area: Area) -> Self {
        let n = (area.size / GROUND_CELL) as usize;
        let (x0, z0) = (area.x - area.size / 2.0, area.z - area.size / 2.0);
        let at = |o: f64, k: usize| o + k as f64 * GROUND_CELL;
        let (mut height, mut normal) = (Vec::with_capacity((n + 1) * (n + 1)), Vec::with_capacity((n + 1) * (n + 1)));
        for j in 0..=n {
            for i in 0..=n {
                height.push(height_at(at(x0, i), at(z0, j)));
                normal.push(ground_normal(at(x0, i), at(z0, j)));
            }
        }
        Self { x0, z0, n, height, normal }
    }

    /// Height and normal at (x, z) inside the square, as the mesh's triangles interpolate them: each
    /// cell is split along its (i + 1, j)–(i, j + 1) diagonal.
    pub(crate) fn at(&self, x: f64, z: f64) -> (f32, [f32; 3]) {
        let (gx, gz) = ((x - self.x0) / GROUND_CELL, (z - self.z0) / GROUND_CELL);
        let (i, j) = (floor(gx) as usize, floor(gz) as usize);
        let (u, v) = (gx - i as f64, gz - j as f64);
        let k = |di: usize, dj: usize| (j + dj) * (self.n + 1) + i + di;
        let corners = if u + v <= 1.0 { [(k(0, 0), 1.0 - u - v), (k(1, 0), u), (k(0, 1), v)] } else { [(k(1, 1), u + v - 1.0), (k(0, 1), 1.0 - u), (k(1, 0), 1.0 - v)] };
        let (mut y, mut n) = (0.0, [0.0; 3]);
        for (c, w) in corners {
            y += self.height[c] * w;
            for (n, &m) in n.iter_mut().zip(&self.normal[c]) {
                *n += m as f64 * w;
            }
        }
        let len = sqrt(n.iter().map(|c| c * c).sum());
        (y as f32, n.map(|c| (c / len) as f32))
    }
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
        let mut off = 0.0f64; // farthest root from the terrain: the mesh's flat triangles cut its curves
        for b in blades {
            let (x, z) = (b.x as f64, b.z as f64);
            let half = GROUND_SIZE / 2.0 - 0.1;
            assert!(x.abs() <= half && z.abs() <= half);
            off = off.max((b.y as f64 - height_at(x, z)).abs());
            assert!((0.7..=1.3).contains(&b.scale));
            let [nx, ny, nz] = b.normal;
            assert!(((nx * nx + ny * ny + nz * nz).sqrt() - 1.0).abs() < 1e-5 && ny > 0.7, "{:?}", b.normal);
        }
        assert!(off < 0.02, "{off}"); // ≈ curvature · cell² / 8, under half an art pixel
        // Out to the ground's edges, not just the 48 placement area.
        let far = |f: fn(&Blade) -> f32| blades.iter().map(|b| f(b).abs()).fold(0.0, f32::max);
        assert!(far(|b| b.x) > 31.0 && far(|b| b.z) > 31.0);
    }

    #[test]
    fn mask_covers_exactly_the_cells_under_crowns() {
        let trees = &meadow().0.trees;
        let mask = CanopyMask::new(trees);
        assert!(trees.iter().all(|t| mask.covers(t.x, t.z)));
        let n = (SIZE * MASK_RES) as usize;
        for gz in 0..n {
            for gx in 0..n {
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
