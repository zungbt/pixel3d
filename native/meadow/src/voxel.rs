//! SPIKE (throwaway, not for merge): voxel assets meshed into the existing toon pipeline, to judge
//! the look, the outlines and the memory before designing the map editor.
use glam::Vec3;

use crate::mesh::{MeshData, Vertex};

/// A voxel grid: 0 = empty, i = palette[i - 1].
pub struct Model {
    pub size: [usize; 3],
    cells: Vec<u8>,
    pub palette: Vec<u32>,
}

impl Model {
    pub fn new(size: [usize; 3], palette: Vec<u32>) -> Self {
        Self { size, cells: vec![0; size[0] * size[1] * size[2]], palette }
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, c: u8) {
        let [sx, sy, _] = self.size;
        self.cells[x + sx * (y + sy * z)] = c;
    }

    pub(crate) fn get(&self, p: [i32; 3]) -> u8 {
        let [sx, sy, sz] = self.size.map(|s| s as i32);
        if p[0] < 0 || p[1] < 0 || p[2] < 0 || p[0] >= sx || p[1] >= sy || p[2] >= sz {
            return 0;
        }
        self.cells[(p[0] + sx * (p[1] + sy * p[2])) as usize]
    }

    pub fn solid(&self) -> usize {
        self.cells.iter().filter(|&&c| c != 0).count()
    }
}

/// One mesh per palette colour (the scene pipeline colours per instance). A face is drawn only
/// where its neighbour is empty; `greedy` merges same-colour faces into rectangles. Units are
/// voxels, origin at the grid's bottom centre.
pub fn meshes(m: &Model, greedy: bool) -> Vec<(u32, MeshData)> {
    let mut out: Vec<MeshData> = m.palette.iter().map(|_| MeshData { vertices: vec![], indices: vec![] }).collect();
    let centre = Vec3::new(m.size[0] as f32 / 2.0, 0.0, m.size[2] as f32 / 2.0);
    for d in 0..3 {
        let (u, v) = ((d + 1) % 3, (d + 2) % 3);
        let (nu, nv) = (m.size[u], m.size[v]);
        for sign in [1i32, -1] {
            let mut mask = vec![0u8; nu * nv];
            for k in 0..m.size[d] {
                for j in 0..nv {
                    for i in 0..nu {
                        let mut p = [0i32; 3];
                        p[d] = k as i32;
                        p[u] = i as i32;
                        p[v] = j as i32;
                        let c = m.get(p);
                        let mut q = p;
                        q[d] += sign;
                        mask[i + nu * j] = if c != 0 && m.get(q) == 0 { c } else { 0 };
                    }
                }
                let plane = if sign > 0 { k as f32 + 1.0 } else { k as f32 };
                for j in 0..nv {
                    let mut i = 0;
                    while i < nu {
                        let c = mask[i + nu * j];
                        if c == 0 {
                            i += 1;
                            continue;
                        }
                        let mut w = 1;
                        let mut h = 1;
                        if greedy {
                            while i + w < nu && mask[i + w + nu * j] == c {
                                w += 1;
                            }
                            'grow: while j + h < nv {
                                for x in i..i + w {
                                    if mask[x + nu * (j + h)] != c {
                                        break 'grow;
                                    }
                                }
                                h += 1;
                            }
                        }
                        for y in j..j + h {
                            for x in i..i + w {
                                mask[x + nu * y] = 0;
                            }
                        }
                        let corner = |a: usize, b: usize| {
                            let mut p = Vec3::ZERO;
                            p[d] = plane;
                            p[u] = a as f32;
                            p[v] = b as f32;
                            p - centre
                        };
                        let mut n = Vec3::ZERO;
                        n[d] = sign as f32;
                        let mesh = &mut out[c as usize - 1];
                        let first = mesh.vertices.len() as u32;
                        for p in [corner(i, j), corner(i + w, j), corner(i + w, j + h), corner(i, j + h)] {
                            mesh.vertices.push(Vertex { pos: p.into(), normal: n.into(), extra: 0.0 });
                        }
                        // u × v = +d, so the corners above wind counter-clockwise seen from +d.
                        if sign > 0 {
                            mesh.indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
                        } else {
                            mesh.indices.extend_from_slice(&[first, first + 2, first + 1, first, first + 3, first + 2]);
                        }
                        i += w;
                    }
                }
            }
        }
    }
    m.palette.iter().copied().zip(out).filter(|(_, mesh)| !mesh.indices.is_empty()).collect()
}

fn hash(x: usize, y: usize, z: usize, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ (z as u32).wrapping_mul(2_147_483_647) ^ seed.wrapping_mul(0x9e37_79b9);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// A tree the size of the meadow's (trunk ~1.1 units, crown radius ~0.75) at `n` voxels per unit,
/// so the same tree can be compared at several voxel sizes. `seed` varies the crown's speckle.
pub fn tree(n: usize, seed: u32) -> Model {
    let u = |units: f32| (units * n as f32).round() as usize;
    let (w, top) = (u(1.7), u(2.3));
    let mut m = Model::new([w, top, w], vec![0x8d6d52, 0x589e63, 0x46864f, 0x6db374]);
    let c = w as f32 / 2.0;
    let trunk_r = (0.12 * n as f32).max(0.75);
    let crown = Vec3::new(c, 1.45 * n as f32, c);
    let crown_r = 0.75 * n as f32;
    for z in 0..w {
        for y in 0..top {
            for x in 0..w {
                let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                let wobble = 1.0 + 0.12 * (hash(x / 2, y / 2, z / 2, seed) - 0.5);
                if (p - crown).length() < crown_r * wobble {
                    let r = hash(x, y, z, seed);
                    m.set(x, y, z, if r < 0.18 { 3 } else if r > 0.9 { 4 } else { 2 });
                } else if (p.x - c).hypot(p.z - c) < trunk_r && p.y < crown.y {
                    m.set(x, y, z, 1);
                }
            }
        }
    }
    m
}

/// A park bench ~1.2 units long at `n` voxels per unit.
pub fn bench(n: usize) -> Model {
    let u = |units: f32| ((units * n as f32).round() as usize).max(1);
    let (len, depth, height) = (u(1.2), u(0.45), u(0.8));
    let mut m = Model::new([len, height, depth], vec![0xa0703f, 0x6b4a2b, 0x3c3c44]);
    let (seat_y, seat_t, leg) = (u(0.35), u(0.08), u(0.1));
    for z in 0..depth {
        for y in 0..height {
            for x in 0..len {
                let is_leg = (x < leg || x >= len - leg) && (z < leg || z >= depth - leg) && y < seat_y;
                let is_seat = y >= seat_y && y < seat_y + seat_t;
                let is_back = z < u(0.08) && y >= seat_y + seat_t + u(0.08);
                if is_leg {
                    m.set(x, y, z, 3);
                } else if is_seat {
                    m.set(x, y, z, if (x / u(0.2)) % 2 == 0 { 1 } else { 2 });
                } else if is_back && y % (u(0.12) + 1) != 0 {
                    m.set(x, y, z, 1);
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_voxel_is_a_closed_cube() {
        let mut m = Model::new([1, 1, 1], vec![0xffffff]);
        m.set(0, 0, 0, 1);
        let meshes = meshes(&m, true);
        assert_eq!(meshes.len(), 1);
        let mesh = &meshes[0].1;
        assert_eq!((mesh.vertices.len(), mesh.indices.len()), (24, 36));
        // Every triangle faces away from the centre (counter-clockwise from outside).
        for t in mesh.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(mesh.vertices[i as usize].pos));
            let n = (b - a).cross(c - a);
            let mid = (a + b + c) / 3.0 - Vec3::new(0.0, 0.5, 0.0);
            assert!(n.dot(mid) > 0.0, "inward triangle {a} {b} {c}");
        }
    }

    #[test]
    fn greedy_merges_a_solid_box_to_six_quads() {
        let mut m = Model::new([4, 3, 5], vec![0xffffff]);
        for z in 0..5 {
            for y in 0..3 {
                for x in 0..4 {
                    m.set(x, y, z, 1);
                }
            }
        }
        assert_eq!(meshes(&m, true)[0].1.vertices.len(), 24);
        assert_eq!(meshes(&m, false)[0].1.vertices.len(), 4 * 2 * (4 * 3 + 3 * 5 + 4 * 5));
    }
}
