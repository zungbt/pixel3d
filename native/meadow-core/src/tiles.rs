//! Outer world tiles: 32-unit squares beyond the 64 inner ground, for the secondary screens. Each
//! tile has its own seeded trees, rocks, flowers and grass at the inner world's densities, and
//! depends only on its (i, j), so a tile looks the same whatever else is placed with it.
use libm::floor;

use crate::flowers::{self, Head};
use crate::grass::{self, Blade, CanopyMask};
use crate::rng::Mulberry32;
use crate::world::{self, Area, GROUND_SIZE, Rock, SIZE, Tree, height_at};

pub const TILE: f64 = 32.0;

// Per-stream salts: a tile's trees and rocks, flowers and grass come from unrelated seeds.
const SALT_LAYOUT: u32 = 0x1357_9bdf;
const SALT_FLOWERS: u32 = 0x2468_ace0;
const SALT_GRASS: u32 = 0x0f1e_2d3c;

/// The lower corner of tile (i, j): the tile covers [x0, x0 + TILE] × [z0, z0 + TILE].
pub fn tile_origin(i: i32, j: i32) -> (f64, f64) {
    (i as f64 * TILE, j as f64 * TILE)
}

fn tile_area(i: i32, j: i32) -> Area {
    let (x0, z0) = tile_origin(i, j);
    Area { x: x0 + TILE / 2.0, z: z0 + TILE / 2.0, size: TILE }
}

// Integer hash of the tile index and a salt, so each stream's seed depends on (i, j) alone.
fn tile_seed(i: i32, j: i32, salt: u32) -> u32 {
    let mut h = (i as u32).wrapping_mul(0x9e37_79b1) ^ (j as u32).wrapping_mul(0x85eb_ca77) ^ salt;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297a_2d39);
    h ^ (h >> 15)
}

// The inner count scaled by the tile's dart area over the inner's (2 * half)².
fn scaled(inner: u32, inner_side: f64) -> u32 {
    let r = TILE / inner_side;
    (inner as f64 * r * r).round() as u32
}

// Inner ground tiles (i, j ∈ {-1, 0}) are the mesh already drawn: never returned.
fn is_inner(i: i32, j: i32) -> bool {
    (-1..=0).contains(&i) && (-1..=0).contains(&j)
}

/// Outer tiles whose square, grown by `margin`, overlaps any footprint (a convex quad on the
/// ground, x/z corners in order). Sorted (j, then i) so the result is deterministic.
pub fn needed(footprints: &[[[f64; 2]; 4]], margin: f64) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for quad in footprints {
        let (mut x0, mut x1, mut z0, mut z1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in quad {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            z0 = z0.min(p[1]);
            z1 = z1.max(p[1]);
        }
        let range = |a: f64, b: f64| floor((a - margin) / TILE) as i32..=floor((b + margin) / TILE) as i32;
        for j in range(z0, z1) {
            for i in range(x0, x1) {
                if !is_inner(i, j) && overlaps(i, j, margin, quad) {
                    out.push((i, j));
                }
            }
        }
    }
    out.sort_by_key(|&(i, j)| (j, i));
    out.dedup();
    out
}

// Separating-axis test: the grown tile square against one convex quad. Axes are the x and z
// axes and the quad's edge normals; touching counts as overlapping.
fn overlaps(i: i32, j: i32, margin: f64, quad: &[[f64; 2]; 4]) -> bool {
    let (x0, z0) = tile_origin(i, j);
    let (x1, z1) = (x0 + TILE + margin, z0 + TILE + margin);
    let (x0, z0) = (x0 - margin, z0 - margin);
    let square = [[x0, z0], [x1, z0], [x1, z1], [x0, z1]];
    let mut axes = vec![[1.0, 0.0], [0.0, 1.0]];
    for k in 0..4 {
        let (a, b) = (quad[k], quad[(k + 1) % 4]);
        axes.push([a[1] - b[1], b[0] - a[0]]);
    }
    axes.iter().all(|ax| {
        let (s0, s1) = project(&square, ax);
        let (q0, q1) = project(quad, ax);
        s0 <= q1 && q0 <= s1
    })
}

fn project(pts: &[[f64; 2]], ax: &[f64; 2]) -> (f64, f64) {
    pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        let d = p[0] * ax[0] + p[1] * ax[1];
        (lo.min(d), hi.max(d))
    })
}

pub struct Outer {
    pub trees: Vec<Tree>,
    pub rocks: Vec<Rock>,
    pub flowers: Vec<Head>,
    pub blades: Vec<Blade>,
}

/// Everything on these outer tiles. Each tile is seeded from (i, j) alone, so a tile looks the
/// same whatever other tiles are placed with it.
pub fn place_outer(tiles: &[(i32, i32)]) -> Outer {
    let mut out = Outer { trees: vec![], rocks: vec![], flowers: vec![], blades: vec![] };
    for &(i, j) in tiles {
        let square = tile_area(i, j);
        // Trees and rocks stay 1.0 in from the edge, so canopies and neighbouring clusters stay on the tile.
        let inset = Area { size: TILE - 2.0, ..square };
        let mut layout = world::place_in(&mut Mulberry32::new(tile_seed(i, j, SALT_LAYOUT)), &height_at, inset, &world::scaled_limits(inset));
        // A stone at a cluster's rim can reach past the tile: drop it, so the tile holds only its own rocks.
        let (x0, z0) = tile_origin(i, j);
        layout.rocks.retain(|r| (x0..x0 + TILE).contains(&r.x) && (z0..z0 + TILE).contains(&r.z));
        let mask = CanopyMask::over(&layout.trees, square);
        let flowers = flowers::place_in(&mut Mulberry32::new(tile_seed(i, j, SALT_FLOWERS)), &mask, &layout.rocks, square, scaled(flowers::ATTEMPTS, SIZE - 0.2));
        let blades = grass::place_blades(&mut Mulberry32::new(tile_seed(i, j, SALT_GRASS)), &mask, &layout.rocks, square, scaled(grass::ATTEMPTS, GROUND_SIZE - 0.2));
        out.trees.extend(layout.trees);
        out.rocks.extend(layout.rocks);
        out.flowers.extend(flowers);
        out.blades.extend(blades);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDGE: f64 = 1e-4; // blades are f32

    fn in_tile(i: i32, j: i32, x: f64, z: f64, margin: f64) -> bool {
        let (x0, z0) = tile_origin(i, j);
        (x0 + margin..=x0 + TILE - margin).contains(&x) && (z0 + margin..=z0 + TILE - margin).contains(&z)
    }

    fn tree_key(t: &Tree) -> [f64; 6] {
        [t.x, t.y, t.z, t.height, t.canopy, t.phase]
    }

    fn rock_key(r: &Rock) -> [f64; 6] {
        [r.x, r.y, r.z, r.r, r.sy, r.rot_y]
    }

    #[test]
    fn needed_skips_the_inner_ground() {
        let inner = [[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];
        assert!(needed(&[inner], 0.0).is_empty());
        assert!(needed(&[inner], 3.0).is_empty()); // the margin reaches x = ±13 only: still inner
    }

    #[test]
    fn needed_finds_the_tiles_a_footprint_crosses() {
        let strip = [[-1.0, 20.0], [1.0, 20.0], [1.0, 60.0], [-1.0, 60.0]];
        assert_eq!(needed(&[strip], 0.0), vec![(-1, 1), (0, 1)]);
        let grown = needed(&[strip], 5.0);
        assert_eq!(grown, vec![(-1, 1), (0, 1), (-1, 2), (0, 2)]);
        assert_eq!(needed(&[strip, strip], 5.0), grown, "sorted and deduplicated");
    }

    #[test]
    fn tiles_depend_only_on_their_own_index() {
        let alone = place_outer(&[(0, 1)]);
        let pair = place_outer(&[(0, 1), (1, 1)]);
        assert_eq!(alone.blades, place_outer(&[(0, 1)]).blades, "deterministic");
        assert!(pair.trees.len() > alone.trees.len() && pair.blades.len() > alone.blades.len());
        assert_eq!(pair.flowers[..alone.flowers.len()], alone.flowers[..]);
        assert!(pair.trees.iter().map(tree_key).take(alone.trees.len()).eq(alone.trees.iter().map(tree_key)));
        assert!(pair.rocks.iter().map(rock_key).take(alone.rocks.len()).eq(alone.rocks.iter().map(rock_key)));
        assert_eq!(pair.blades[..alone.blades.len()], alone.blades[..]);
    }

    #[test]
    fn tile_contents_stay_on_their_tile() {
        for (i, j) in [(0, 1), (2, 2), (-1, -2)] {
            let o = place_outer(&[(i, j)]);
            assert!(!o.trees.is_empty() && !o.rocks.is_empty() && !o.flowers.is_empty() && !o.blades.is_empty());
            for t in &o.trees {
                assert!(in_tile(i, j, t.x, t.z, 1.0), "tree at {}, {}", t.x, t.z);
                assert!(t.y == height_at(t.x, t.z));
            }
            for r in &o.rocks {
                assert!(in_tile(i, j, r.x, r.z, 0.0), "rock at {}, {}", r.x, r.z);
            }
            for h in &o.flowers {
                assert!(in_tile(i, j, h.x, h.z, 0.0), "flower at {}, {}", h.x, h.z);
            }
            let grid = grass::GroundGrid::new(tile_area(i, j));
            for b in &o.blades {
                let (x, z) = (b.x as f64, b.z as f64);
                assert!(in_tile(i, j, x, z, -EDGE), "blade at {x}, {z}");
                // Roots on the tile's own mesh triangles, and close to the terrain.
                assert!((b.y - grid.at(x, z).0).abs() < 1e-5);
                assert!((b.y as f64 - height_at(x, z)).abs() < 0.02);
            }
        }
    }

    #[test]
    fn tile_mesh_vertices_match_the_terrain() {
        let (i, j) = (2, -2);
        let grid = grass::GroundGrid::new(tile_area(i, j));
        let (x0, z0) = tile_origin(i, j);
        // Interior vertices: the last row and column are never under a blade (darts stay 0.1 in).
        for k in 0..64 {
            for m in [0, 17, 63] {
                let (x, z) = (x0 + k as f64 * world::GROUND_CELL, z0 + m as f64 * world::GROUND_CELL);
                assert!((grid.at(x, z).0 as f64 - height_at(x, z)).abs() < 1e-5, "vertex {x}, {z}");
            }
        }
    }

    #[test]
    fn tile_grass_density_matches_the_inner_world() {
        let inner = grass::place_grass(&world::place(42)).len() as f64 / grass::ATTEMPTS as f64;
        let tile = place_outer(&[(2, 2)]).blades.len() as f64 / scaled(grass::ATTEMPTS, GROUND_SIZE - 0.2) as f64;
        assert!((0.75..=1.25).contains(&(tile / inner)), "tile {tile} vs inner {inner}");
    }
}
