//! Terrain, pond, paths and the seeded tree/rock layout, ported from the web's `src/world.js`.
//! Placement keeps the web's exact `rand()` call order, so seed 42 gives the same trees.
//! The hills are new: layered noise instead of the web's two sines (which read as washboard stripes).
use std::f64::consts::PI;

use libm::{atan2, cos, exp, floor, hypot, sin, sqrt};

use crate::rng::Mulberry32;

pub const SIZE: f64 = 48.0; // placement area; the ground mesh is larger so no edge shows
pub const GROUND_SIZE: f64 = 64.0; // the ground mesh: larger than the placement area, so no screen corner shows its edge
pub const GROUND_CELL: f64 = 0.5; // the ground mesh's grid; grass roots interpolate over the same triangles

// Pond: an irregular water polygon inside a wide sand beach, on terrain flattened around them.
pub const POND_X: f64 = 6.2;
pub const POND_Z: f64 = -2.3;
const POND_R_MAX: f64 = 3.7 * 1.22;
const SAND_R_MAX: f64 = POND_R_MAX * 1.1;
const FLAT_R: f64 = SAND_R_MAX + 1.5; // + one grid cell diagonal: every triangle under the sand is flat
const FLAT_BLEND: f64 = 3.0;

pub fn smoothstep(x: f64, min: f64, max: f64) -> f64 {
    if x <= min {
        return 0.0;
    }
    if x >= max {
        return 1.0;
    }
    let x = (x - min) / (max - min);
    x * x * (3.0 - 2.0 * x)
}

// three's MathUtils.lerp form, so t = 1 gives exactly b (flat ground is exactly SHORE_Y).
pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1.0 - t) * a + t * b
}

// Gentle rolling hills: two octaves of value noise, each in its own rotated domain so the noise
// lattice doesn't line up with the camera's 45° view.
fn hills(x: f64, z: f64) -> f64 {
    let (s1, c1) = (sin(0.47), cos(0.47));
    let (s2, c2) = (sin(-0.83), cos(-0.83));
    let a = value_noise((x * c1 - z * s1) * 0.085 + 3.7, (x * s1 + z * c1) * 0.085 - 11.3);
    let b = value_noise((x * c2 - z * s2) * 0.2 + 41.0, (x * s2 + z * c2) * 0.2 + 7.0);
    1.3 * (a * 0.75 + b * 0.25 - 0.5)
}

pub fn pond_radius(theta: f64) -> f64 {
    3.7 * (1.0 + 0.15 * sin(3.0 * theta + 1.0) + 0.07 * sin(5.0 * theta + 2.0))
}

// Beach width varies around the pond: wide on some sides, narrow on others.
pub fn sand_radius(theta: f64) -> f64 {
    pond_radius(theta) * (1.07 + 0.02 * sin(2.0 * theta + 0.5) + 0.01 * sin(3.0 * theta + 2.0))
}

fn pond_polar(x: f64, z: f64) -> (f64, f64) {
    let dx = x - POND_X;
    let dz = z - POND_Z;
    (hypot(dx, dz), atan2(dz, dx))
}

/// Distance past the shoreline in world units: < 0 is water.
pub fn pond_edge(x: f64, z: f64) -> f64 {
    let (d, theta) = pond_polar(x, z);
    d - pond_radius(theta)
}

/// The point `offset` units out from the shoreline in direction theta (< 0 is in the water).
pub fn pond_point(theta: f64, offset: f64) -> (f64, f64) {
    let r = pond_radius(theta) + offset;
    (POND_X + cos(theta) * r, POND_Z + sin(theta) * r)
}

/// Same, relative to the edge of the beach: < 1 is sand (or water).
pub fn sand_q(x: f64, z: f64) -> f64 {
    let (d, theta) = pond_polar(x, z);
    d / sand_radius(theta)
}

// Terrain from any base height, flattened to the pond centre's height around the pond.
fn height_with(base: &dyn Fn(f64, f64) -> f64, x: f64, z: f64) -> f64 {
    let d = hypot(x - POND_X, z - POND_Z);
    let flat = 1.0 - smoothstep(d, FLAT_R, FLAT_R + FLAT_BLEND);
    lerp(base(x, z), base(POND_X, POND_Z), flat)
}

/// Shared by the terrain mesh and prop placement so nothing floats or sinks.
pub fn height_at(x: f64, z: f64) -> f64 {
    height_with(&hills, x, z)
}

/// Height of the pond and beach.
pub fn shore_y() -> f64 {
    hills(POND_X, POND_Z)
}

// Dirt path: a meandering centreline x = pathX(z), bottom-left to top-right on screen, left of the pond.
pub mod path {
    pub const X0: f64 = -2.8;
    pub const AMP: f64 = 2.0;
    pub const FREQ: f64 = 0.22;
    pub const PHASE: f64 = 1.0;
    // Ragged half-width.
    pub const W_BASE: f64 = 0.55;
    pub const W_A1: f64 = 0.1;
    pub const W_F1: f64 = 2.3;
    pub const W_A2: f64 = 0.06;
    pub const W_F2: f64 = 5.1;
    // Branch: leaves the main path, runs between the pond and the rocks along
    // z = z0 + a * (1 - exp(-(x - x0) / a)), and narrows into the grass at x1.
    pub const BRANCH_Z0: f64 = 1.4;
    pub const BRANCH_A: f64 = 5.5;
    pub const BRANCH_X1: f64 = 15.0;
    pub const BRANCH_TAPER: f64 = 3.0;
    pub const BRANCH_WIDTH: f64 = 0.82;

    pub fn branch_x0() -> f64 {
        X0 + AMP * libm::sin(BRANCH_Z0 * FREQ + PHASE)
    }
}

fn path_half_w(x: f64, z: f64) -> f64 {
    use path::*;
    W_BASE + W_A1 * sin(z * W_F1) + W_A2 * sin(z * W_F2 + x)
}

/// Distance from the main path's edge: < 0 is on it. Trees and rocks keep off this path only.
pub fn main_path_edge(x: f64, z: f64) -> f64 {
    use path::*;
    let a = z * FREQ + PHASE;
    let slope = AMP * FREQ * cos(a);
    let dist = (x - (X0 + AMP * sin(a))).abs() / sqrt(1.0 + slope * slope);
    dist - path_half_w(x, z)
}

fn branch_edge(x: f64, z: f64) -> f64 {
    use path::*;
    let x0 = branch_x0();
    if x < x0 {
        return f64::INFINITY;
    }
    let k = exp(-(x - x0) / BRANCH_A); // also the curve's slope
    let dist = (z - (BRANCH_Z0 + BRANCH_A * (1.0 - k))).abs() / sqrt(1.0 + k * k);
    let half_w = path_half_w(z, x) * BRANCH_WIDTH * (1.0 - smoothstep(x, BRANCH_X1 - BRANCH_TAPER, BRANCH_X1));
    dist - half_w
}

/// Distance from the edge of either path: < 0 is on a path.
pub fn path_edge(x: f64, z: f64) -> f64 {
    main_path_edge(x, z).min(branch_edge(x, z))
}

// Smooth 2D value noise in [0, 1], used as density maps for placement.
fn hash2(x: f64, z: f64) -> f64 {
    let s = sin(x * 127.1 + z * 311.7) * 43758.5453;
    s - floor(s)
}

fn value_noise(x: f64, z: f64) -> f64 {
    let ix = floor(x);
    let iz = floor(z);
    let fx = x - ix;
    let fz = z - iz;
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uz = fz * fz * (3.0 - 2.0 * fz);
    let a = lerp(hash2(ix, iz), hash2(ix + 1.0, iz), ux);
    let b = lerp(hash2(ix, iz + 1.0), hash2(ix + 1.0, iz + 1.0), ux);
    lerp(a, b, uz)
}

pub fn fbm(x: f64, z: f64) -> f64 {
    value_noise(x, z) * 0.65 + value_noise(x * 2.1 + 17.0, z * 2.1 + 31.0) * 0.35
}

fn slope_with(height: &dyn Fn(f64, f64) -> f64, x: f64, z: f64) -> f64 {
    let e = 0.1;
    hypot(height(x + e, z) - height(x - e, z), height(x, z + e) - height(x, z - e)) / (2.0 * e)
}

pub struct Tree {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub height: f64, // trunk height; the crown centre sits at height + 0.3
    pub canopy: f64, // crown radius
    pub phase: f64,  // sway phase
}

pub struct Rock {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub r: f64,       // x/z scale
    pub sy: f64,      // y scale
    pub rot_y: f64,
}

pub struct Layout {
    pub trees: Vec<Tree>,
    pub rocks: Vec<Rock>,
}

/// The seeded layout on this terrain.
pub fn place(seed: u32) -> Layout {
    place_with(seed, &height_at)
}

fn place_with(seed: u32, height: &dyn Fn(f64, f64) -> f64) -> Layout {
    let mut rng = Mulberry32::new(seed);
    let mut rand = move || rng.next();
    let spread = |rand: &mut dyn FnMut() -> f64| (rand() - 0.5) * (SIZE - 3.0);
    let too_close = |trees: &[Tree], x: f64, z: f64, min: f64| trees.iter().any(|t| hypot(t.x - x, t.z - z) < min);

    // Trees: groves where the forest mask is high, rare loners in clearings,
    // and a minimum spacing between trunks (dart throwing ≈ Poisson-disk).
    let mut trees: Vec<Tree> = Vec::new();
    let mut attempt = 0;
    while attempt < 4000 && trees.len() < 120 {
        attempt += 1;
        let x = spread(&mut rand);
        let z = spread(&mut rand);
        let forest = smoothstep(fbm(x * 0.09, z * 0.09), 0.45, 0.65);
        if rand() > forest.max(0.01) || too_close(&trees, x, z, 1.6) || sand_q(x, z) < 1.15 || main_path_edge(x, z) < 1.0 {
            continue;
        }
        let h = 0.8 + rand() * 0.6;
        let canopy = 0.6 + rand() * 0.3;
        let phase = trees.len() as f64 * 2.4;
        trees.push(Tree { x, y: height(x, z), z, height: h, canopy, phase });
    }

    // Rocks: clusters of one boulder plus a few small stones, favouring slopes, never inside a tree.
    // The centre check uses the largest boulder radius, so every counted cluster keeps its boulder.
    let mut rocks = Vec::new();
    let (mut clusters, mut attempt) = (0, 0);
    while clusters < 12 && attempt < 500 {
        attempt += 1;
        let cx = spread(&mut rand);
        let cz = spread(&mut rand);
        if rand() > 0.3 + slope_with(height, cx, cz) * 1.5
            || too_close(&trees, cx, cz, 1.5)
            || main_path_edge(cx, cz) < 0.9 + 0.15
            || pond_edge(cx, cz) < 0.9
        {
            continue;
        }
        clusters += 1;
        let mut stones = vec![(cx, cz, 0.6 + rand() * 0.3)];
        let small = 2 + floor(rand() * 4.0) as usize;
        for _ in 0..small {
            let a = rand() * PI * 2.0;
            let d = 0.6 + rand() * 1.0;
            stones.push((cx + cos(a) * d, cz + sin(a) * d, 0.2 + rand() * 0.15));
        }
        for (x, z, r) in stones {
            if too_close(&trees, x, z, 0.7) || pond_edge(x, z) < r || main_path_edge(x, z) < r + 0.15 {
                continue;
            }
            let sy = r * (0.6 + rand() * 0.4);
            let rot_y = rand() * PI;
            rocks.push(Rock { x, y: height(x, z), z, r, sy, rot_y });
        }
    }
    Layout { trees, rocks }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The web's terrain (two sines), only to check the placement port against its dump.
    fn web_base(x: f64, z: f64) -> f64 {
        0.35 * sin(x * 0.6) * cos(z * 0.5) + 0.15 * sin((x + z) * 0.9)
    }

    fn web_height(x: f64, z: f64) -> f64 {
        height_with(&web_base, x, z)
    }

    struct Golden {
        rand: Vec<f64>,
        trees: Vec<Vec<f64>>,
        rocks: Vec<Vec<f64>>,
    }

    // tools/dump-world.mjs output: `rand v`, `tree x y z height canopy`, `rock x y z r sy roty`.
    fn golden() -> Golden {
        let mut g = Golden { rand: vec![], trees: vec![], rocks: vec![] };
        for line in include_str!("../tests/fixtures/world42.txt").lines() {
            let mut parts = line.split_whitespace();
            let kind = parts.next().unwrap();
            let nums: Vec<f64> = parts.map(|p| p.parse().unwrap()).collect();
            match kind {
                "rand" => g.rand.push(nums[0]),
                "tree" => g.trees.push(nums),
                "rock" => g.rocks.push(nums),
                _ => panic!("unknown line {line}"),
            }
        }
        g
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9
    }

    #[test]
    fn mulberry32_matches_web() {
        let g = golden();
        let mut rng = Mulberry32::new(42);
        for (i, want) in g.rand.iter().enumerate() {
            assert_eq!(rng.next(), *want, "value {i}");
        }
    }

    #[test]
    fn placement_matches_web_on_web_terrain() {
        let g = golden();
        let layout = place_with(42, &web_height);
        assert_eq!(layout.trees.len(), g.trees.len());
        for (i, (t, w)) in layout.trees.iter().zip(&g.trees).enumerate() {
            let got = [t.x, t.y, t.z, t.height, t.canopy];
            assert!(got.iter().zip(w).all(|(a, b)| close(*a, *b)), "tree {i}: {got:?} vs {w:?}");
        }
        assert_eq!(layout.rocks.len(), g.rocks.len());
        for (i, (r, w)) in layout.rocks.iter().zip(&g.rocks).enumerate() {
            let got = [r.x, r.y, r.z, r.r, r.sy, r.rot_y];
            assert!(got.iter().zip(w).all(|(a, b)| close(*a, *b)), "rock {i}: {got:?} vs {w:?}");
        }
    }

    #[test]
    fn trees_keep_web_positions_on_new_hills() {
        // Tree acceptance never reads the height, so only y changes.
        let g = golden();
        let layout = place(42);
        assert_eq!(layout.trees.len(), g.trees.len());
        for (t, w) in layout.trees.iter().zip(&g.trees) {
            assert!(close(t.x, w[0]) && close(t.z, w[2]));
            assert_eq!(t.y, height_at(t.x, t.z));
        }
    }

    #[test]
    fn rocks_keep_clear_of_pond_path_and_trees() {
        let layout = place(42);
        assert!(!layout.rocks.is_empty());
        for r in &layout.rocks {
            assert!(pond_edge(r.x, r.z) >= r.r);
            assert!(main_path_edge(r.x, r.z) >= r.r + 0.15);
            assert!(layout.trees.iter().all(|t| hypot(t.x - r.x, t.z - r.z) >= 0.7));
            assert_eq!(r.y, height_at(r.x, r.z));
        }
    }

    #[test]
    fn ground_is_flat_under_the_beach() {
        let y = shore_y();
        for i in 0..72 {
            let theta = i as f64 / 72.0 * PI * 2.0;
            for k in [0.0, 0.5, 1.0] {
                let r = sand_radius(theta) * k;
                assert_eq!(height_at(POND_X + cos(theta) * r, POND_Z + sin(theta) * r), y);
            }
        }
    }

    #[test]
    fn hills_stay_gentle() {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        let mut steep: f64 = 0.0;
        for i in -32..=32 {
            for j in -32..=32 {
                let (x, z) = (i as f64, j as f64);
                let h = height_at(x, z);
                lo = lo.min(h);
                hi = hi.max(h);
                steep = steep.max(slope_with(&height_at, x, z));
            }
        }
        assert!(hi - lo > 0.4 && hi - lo < 1.6, "height range {lo}..{hi}");
        assert!(steep < 0.5, "max slope {steep}");
    }
}
