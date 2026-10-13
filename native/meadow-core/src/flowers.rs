//! Wildflower patches in open meadow, ported from the web's `src/flowers.js`: the grass's exclusions
//! (beach, paths, rocks) plus none under crowns, in patches of one colour. Same seed and `rand()` order
//! as the web; the native layout's trees and rocks differ, so not quite the same flowers.
use libm::floor;

use crate::grass::{CanopyMask, grass_density};
use crate::rng::Mulberry32;
use crate::world::{Area, Layout, Rock, SIZE, fbm, height_at, smoothstep};

pub const ATTEMPTS: u32 = 24_000; // ~1800 flowers, ~9 patches in view
const SEED: u32 = 314;
pub const HEAD_Y: f64 = 0.36; // over most grass tips (blades reach 0.39) so they don't flicker across it; also scales the sway
pub const HEAD_R: f64 = 0.1; // ~3 art pixels
const COLORS: [u32; 4] = [0xf4f1e8, 0xf2d35b, 0xa98be0, 0xee9fc0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Head {
    pub x: f64,
    pub y: f64, // the head's centre: ground + HEAD_Y
    pub z: f64,
    pub color: u32, // sRGB hex of its patch
    pub shade: f64, // × the linear colour: 0.92–1.08
}

pub fn place_flowers(layout: &Layout) -> Vec<Head> {
    let mask = CanopyMask::new(&layout.trees);
    place_in(&mut Mulberry32::new(SEED), &mask, &layout.rocks, Area { x: 0.0, z: 0.0, size: SIZE }, ATTEMPTS)
}

/// Heads over one square, darts from the stream `rng`; colours continue the same stream.
pub fn place_in(rng: &mut Mulberry32, mask: &CanopyMask, rocks: &[Rock], area: Area, attempts: u32) -> Vec<Head> {
    let mut rand = || rng.next();
    let half = area.size / 2.0 - 0.1; // just inside the square
    let mut heads = Vec::new();
    for _ in 0..attempts {
        let x = area.x + (rand() * 2.0 - 1.0) * half;
        let z = area.z + (rand() * 2.0 - 1.0) * half;
        let patch = smoothstep(fbm(x * 0.3 + 200.0, z * 0.3), 0.7, 0.74);
        if rand() > patch * grass_density(x, z, mask, rocks) || mask.covers(x, z) {
            continue;
        }
        heads.push(Head { x, y: height_at(x, z) + HEAD_Y, z, color: 0, shade: 0.0 });
    }
    // Colours after placement, continuing the same stream (as the web): one per patch, from
    // low-frequency noise, with a little per-flower variation.
    for h in &mut heads {
        h.color = COLORS[floor(fbm(h.x * 0.12 + 300.0, h.z * 0.12) * 8.0) as usize % COLORS.len()];
        h.shade = 0.92 + rand() * 0.16;
    }
    heads
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{self, path_edge, pond_edge};

    #[test]
    fn flowers_grow_in_open_meadow_only() {
        let layout = world::place(42);
        let heads = place_flowers(&layout);
        // The web places 1789 on its layout; the native trees and rocks shift that a little.
        assert!((1500..2100).contains(&heads.len()), "{} flowers", heads.len());
        let mask = CanopyMask::new(&layout.trees);
        for h in &heads {
            assert!(pond_edge(h.x, h.z) > 0.0 && path_edge(h.x, h.z) > 0.0, "{h:?} in the pond or on a path");
            assert!(!mask.covers(h.x, h.z), "{h:?} under a crown");
            assert!(layout.rocks.iter().all(|r| (r.x - h.x).hypot(r.z - h.z) >= r.r), "{h:?} in a rock");
            assert!((h.y - height_at(h.x, h.z) - HEAD_Y).abs() < 1e-12);
            assert!(COLORS.contains(&h.color) && (0.92..1.08).contains(&h.shade));
        }
        assert_eq!(place_flowers(&layout), heads, "seeded");
    }
}
