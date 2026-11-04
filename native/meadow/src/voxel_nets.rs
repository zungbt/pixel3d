//! SPIKE: chamfered (surface nets) voxel mesher. Plan: docs/superpowers/plans/2026-11-04-voxel-asset-spike.md, Task 1.
use glam::Vec3;

use crate::mesh::{MeshData, Vertex};
use crate::voxel::Model;

// SPIKE
pub fn chamfered(m: &Model) -> Vec<(u32, MeshData)> {
    let [sx, sy, sz] = m.size;
    if sx == 0 || sy == 0 || sz == 0 {
        return vec![];
    }

    let (nx, ny, nz) = (sx + 1, sy + 1, sz + 1);
    let mut dual_verts = vec![Vec3::ZERO; nx * ny * nz];
    let centre = Vec3::new(sx as f32 / 2.0, 0.0, sz as f32 / 2.0);

    let corner_idx = |cx: usize, cy: usize, cz: usize| cx + nx * (cy + ny * cz);

    for cz in 0..=sz {
        for cy in 0..=sy {
            for cx in 0..=sx {
                let mut solid = [[[false; 2]; 2]; 2];
                let mut has_solid = false;
                let mut has_empty = false;
                for dz in 0..2 {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let vx = cx as i32 - 1 + dx;
                            let vy = cy as i32 - 1 + dy;
                            let vz = cz as i32 - 1 + dz;
                            let s = m.get([vx, vy, vz]) != 0;
                            solid[dx as usize][dy as usize][dz as usize] = s;
                            if s {
                                has_solid = true;
                            } else {
                                has_empty = true;
                            }
                        }
                    }
                }

                if !has_solid || !has_empty {
                    continue;
                }

                let mut sum = Vec3::ZERO;
                let mut count = 0;

                // Edges along X
                for dy in 0..2 {
                    for dz in 0..2 {
                        if solid[0][dy][dz] != solid[1][dy][dz] {
                            sum += Vec3::new(
                                cx as f32,
                                cy as f32 - 0.5 + dy as f32,
                                cz as f32 - 0.5 + dz as f32,
                            );
                            count += 1;
                        }
                    }
                }

                // Edges along Y
                for dx in 0..2 {
                    for dz in 0..2 {
                        if solid[dx][0][dz] != solid[dx][1][dz] {
                            sum += Vec3::new(
                                cx as f32 - 0.5 + dx as f32,
                                cy as f32,
                                cz as f32 - 0.5 + dz as f32,
                            );
                            count += 1;
                        }
                    }
                }

                // Edges along Z
                for dx in 0..2 {
                    for dy in 0..2 {
                        if solid[dx][dy][0] != solid[dx][dy][1] {
                            sum += Vec3::new(
                                cx as f32 - 0.5 + dx as f32,
                                cy as f32 - 0.5 + dy as f32,
                                cz as f32,
                            );
                            count += 1;
                        }
                    }
                }

                let vert = sum / count as f32;
                dual_verts[corner_idx(cx, cy, cz)] = vert - centre;
            }
        }
    }

    let mut out: Vec<MeshData> = m
        .palette
        .iter()
        .map(|_| MeshData {
            vertices: vec![],
            indices: vec![],
        })
        .collect();

    for z in 0..sz {
        for y in 0..sy {
            for x in 0..sx {
                let p = [x as i32, y as i32, z as i32];
                let c = m.get(p);
                if c == 0 {
                    continue;
                }
                let mesh = &mut out[c as usize - 1];

                for d in 0..3 {
                    let u = (d + 1) % 3;
                    let v = (d + 2) % 3;
                    for sign in [1i32, -1] {
                        let mut q = p;
                        q[d] += sign;
                        if m.get(q) != 0 {
                            continue;
                        }

                        let plane = if sign > 0 {
                            p[d] as usize + 1
                        } else {
                            p[d] as usize
                        };

                        let make_corner = |u_off: usize, v_off: usize| {
                            let mut c_coord = [0usize; 3];
                            c_coord[d] = plane;
                            c_coord[u] = p[u] as usize + u_off;
                            c_coord[v] = p[v] as usize + v_off;
                            dual_verts[corner_idx(c_coord[0], c_coord[1], c_coord[2])]
                        };

                        let v0 = make_corner(0, 0);
                        let v1 = make_corner(1, 0);
                        let v2 = make_corner(1, 1);
                        let v3 = make_corner(0, 1);

                        let (tri1, tri2) = if sign > 0 {
                            ([v0, v1, v2], [v0, v2, v3])
                        } else {
                            ([v0, v3, v2], [v0, v2, v1])
                        };

                        for [a, b, c_pos] in [tri1, tri2] {
                            let n = (b - a).cross(c_pos - a).normalize_or_zero();
                            let base = mesh.vertices.len() as u32;
                            mesh.vertices.push(Vertex {
                                pos: a.into(),
                                normal: n.into(),
                                extra: 0.0,
                            });
                            mesh.vertices.push(Vertex {
                                pos: b.into(),
                                normal: n.into(),
                                extra: 0.0,
                            });
                            mesh.vertices.push(Vertex {
                                pos: c_pos.into(),
                                normal: n.into(),
                                extra: 0.0,
                            });
                            mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
                        }
                    }
                }
            }
        }
    }

    m.palette
        .iter()
        .copied()
        .zip(out)
        .filter(|(_, mesh)| !mesh.indices.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use glam::Vec3;
    use crate::mesh::MeshData;
    use crate::voxel::Model;
    use super::*;

    fn tris(out: &[(u32, MeshData)]) -> Vec<[Vec3; 3]> {
        let mut res = Vec::new();
        for (_, mesh) in out {
            for chunk in mesh.indices.chunks_exact(3) {
                let a = Vec3::from(mesh.vertices[chunk[0] as usize].pos);
                let b = Vec3::from(mesh.vertices[chunk[1] as usize].pos);
                let c = Vec3::from(mesh.vertices[chunk[2] as usize].pos);
                res.push([a, b, c]);
            }
        }
        res
    }

    #[test]
    fn a_single_voxel_makes_twelve_outward_triangles() {
        let mut m = Model::new([1, 1, 1], vec![0xffffff]);
        m.set(0, 0, 0, 1);
        let out = chamfered(&m);
        assert_eq!(out.len(), 1);
        let t = tris(&out);
        assert_eq!(t.len(), 12);
        for [a, b, c] in t {
            let n = (b - a).cross(c - a);
            let centroid = (a + b + c) / 3.0;
            assert!(n.dot(centroid - Vec3::new(0.0, 0.5, 0.0)) > 0.0);
        }
    }

    #[test]
    fn triangle_count_matches_the_culled_faces_per_colour() {
        let mut m = Model::new([4, 3, 5], vec![0xff0000, 0x0000ff]);
        for z in 0..5 {
            for y in 0..3 {
                for x in 0..4 {
                    m.set(x, y, z, if x < 2 { 1 } else { 2 });
                }
            }
        }
        let cham = chamfered(&m);
        let culled = crate::voxel::meshes(&m, false);
        assert_eq!(cham.len(), culled.len());
        for (c, (col_cham, m_cham)) in cham.iter().enumerate() {
            let (col_culled, m_culled) = &culled[c];
            assert_eq!(col_cham, col_culled);
            assert_eq!(m_cham.indices.len(), m_culled.indices.len());
        }
    }

    #[test]
    fn a_solid_box_is_closed_and_inside_its_bounds() {
        let mut m = Model::new([4, 3, 5], vec![0xffffff]);
        for z in 0..5 {
            for y in 0..3 {
                for x in 0..4 {
                    m.set(x, y, z, 1);
                }
            }
        }
        let out = chamfered(&m);
        let t = tris(&out);
        for tri in &t {
            for v in tri {
                assert!(v.x >= -2.0 - 1e-4 && v.x <= 2.0 + 1e-4, "v.x = {}", v.x);
                assert!(v.y >= 0.0 - 1e-4 && v.y <= 3.0 + 1e-4, "v.y = {}", v.y);
                assert!(v.z >= -2.5 - 1e-4 && v.z <= 2.5 + 1e-4, "v.z = {}", v.z);
            }
        }
        fn round_pt(p: Vec3) -> [i32; 3] {
            [
                (p.x * 10000.0).round() as i32,
                (p.y * 10000.0).round() as i32,
                (p.z * 10000.0).round() as i32,
            ]
        }
        let mut dir_edges: HashMap<([i32; 3], [i32; 3]), usize> = HashMap::new();
        let mut undir_edges: HashSet<([i32; 3], [i32; 3])> = HashSet::new();
        for &[a, b, c] in &t {
            let pts = [round_pt(a), round_pt(b), round_pt(c)];
            for i in 0..3 {
                let u = pts[i];
                let v = pts[(i + 1) % 3];
                *dir_edges.entry((u, v)).or_insert(0) += 1;
                let undir = if u <= v { (u, v) } else { (v, u) };
                undir_edges.insert(undir);
            }
        }
        for (u, v) in undir_edges {
            let fwd = dir_edges.get(&(u, v)).copied().unwrap_or(0);
            let rev = dir_edges.get(&(v, u)).copied().unwrap_or(0);
            assert_eq!((fwd, rev), (1, 1), "edge between {:?} and {:?}", u, v);
        }
    }

    #[test]
    fn thin_features_keep_their_triangles() {
        for m in [crate::voxel::bench(8), crate::voxel::tree(10, 1)] {
            let cham = chamfered(&m);
            let culled = crate::voxel::meshes(&m, true);
            let cham_cols: HashSet<u32> = cham.iter().map(|(c, _)| *c).collect();
            for (c, _) in &culled {
                assert!(cham_cols.contains(c), "missing colour 0x{:06x}", c);
            }
            let t = tris(&cham);
            for [a, b, c] in t {
                let area = (b - a).cross(c - a).length() * 0.5;
                assert!(area > 1e-6, "degenerate triangle with area {}", area);
            }
        }
    }
}
