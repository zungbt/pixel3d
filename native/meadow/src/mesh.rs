//! Hand-built meshes: the ground, unit-sized rock / trunk / crown / flower shapes, the butterfly's
//! body and wing, and the pond and beach fans.
use std::f64::consts::PI;

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use meadow_core::world;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub extra: f32, // ground: tint; fans: 0 at the centre, 1 on the rim; otherwise unused
}

pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// The terrain, with smooth normals (no faceted bands) and soft light/dark tint patches.
pub fn ground() -> MeshData {
    use world::{GROUND_CELL, GROUND_SIZE};
    let n = (GROUND_SIZE / GROUND_CELL) as u32;
    let mut vertices = Vec::with_capacity(((n + 1) * (n + 1)) as usize);
    for j in 0..=n {
        for i in 0..=n {
            let x = -GROUND_SIZE / 2.0 + i as f64 * GROUND_CELL;
            let z = -GROUND_SIZE / 2.0 + j as f64 * GROUND_CELL;
            let e = GROUND_CELL / 2.0;
            let dx = world::height_at(x + e, z) - world::height_at(x - e, z);
            let dz = world::height_at(x, z + e) - world::height_at(x, z - e);
            let normal = Vec3::new(-dx as f32, (2.0 * e) as f32, -dz as f32).normalize();
            let tint = 0.9 + 0.18 * world::fbm(x * 0.2 + 80.0, z * 0.2);
            vertices.push(Vertex { pos: [x as f32, world::height_at(x, z) as f32, z as f32], normal: normal.into(), extra: tint as f32 });
        }
    }
    let mut indices = Vec::with_capacity((n * n * 6) as usize);
    for j in 0..n {
        for i in 0..n {
            let a = j * (n + 1) + i;
            let b = a + n + 1;
            // Counter-clockwise seen from above (+y).
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    MeshData { vertices, indices }
}

// Faceted solid from three.js's polyhedron lists (detail 0): vertices pushed onto the unit sphere,
// one flat normal per triangle.
fn polyhedron(raw: &[f32], tris: &[u32]) -> MeshData {
    let corner = |i: u32| Vec3::from_slice(&raw[i as usize * 3..i as usize * 3 + 3]).normalize();
    let mut vertices = Vec::with_capacity(tris.len());
    for tri in tris.chunks(3) {
        let [a, b, c] = [corner(tri[0]), corner(tri[1]), corner(tri[2])];
        let normal = (b - a).cross(c - a).normalize();
        for p in [a, b, c] {
            vertices.push(Vertex { pos: p.into(), normal: normal.into(), extra: 0.0 });
        }
    }
    let indices = (0..vertices.len() as u32).collect();
    MeshData { vertices, indices }
}

const PHI: f32 = 1.618_034; // (1 + √5) / 2

/// Unit rock (three's DodecahedronGeometry(1, 0)).
pub fn dodecahedron() -> MeshData {
    let (t, r) = (PHI, 1.0 / PHI);
    #[rustfmt::skip]
    let raw = [
        -1.0, -1.0, -1.0,  -1.0, -1.0, 1.0,  -1.0, 1.0, -1.0,  -1.0, 1.0, 1.0,
        1.0, -1.0, -1.0,  1.0, -1.0, 1.0,  1.0, 1.0, -1.0,  1.0, 1.0, 1.0,
        0.0, -r, -t,  0.0, -r, t,  0.0, r, -t,  0.0, r, t,
        -r, -t, 0.0,  -r, t, 0.0,  r, -t, 0.0,  r, t, 0.0,
        -t, 0.0, -r,  t, 0.0, -r,  -t, 0.0, r,  t, 0.0, r,
    ];
    #[rustfmt::skip]
    let tris = [
        3, 11, 7, 3, 7, 15, 3, 15, 13,  7, 19, 17, 7, 17, 6, 7, 6, 15,
        17, 4, 8, 17, 8, 10, 17, 10, 6,  8, 0, 16, 8, 16, 2, 8, 2, 10,
        0, 12, 1, 0, 1, 18, 0, 18, 16,  6, 10, 2, 6, 2, 13, 6, 13, 15,
        2, 16, 18, 2, 18, 3, 2, 3, 13,  18, 1, 9, 18, 9, 11, 18, 11, 3,
        4, 14, 12, 4, 12, 0, 4, 0, 8,  11, 9, 5, 11, 5, 19, 11, 19, 7,
        19, 5, 14, 19, 14, 4, 19, 4, 17,  1, 12, 14, 1, 14, 5, 1, 5, 9,
    ];
    polyhedron(&raw, &tris)
}

/// Unit tree crown (three's IcosahedronGeometry(1, 0)).
pub fn icosahedron() -> MeshData {
    let t = PHI;
    #[rustfmt::skip]
    let raw = [
        -1.0, t, 0.0,  1.0, t, 0.0,  -1.0, -t, 0.0,  1.0, -t, 0.0,
        0.0, -1.0, t,  0.0, 1.0, t,  0.0, -1.0, -t,  0.0, 1.0, -t,
        t, 0.0, -1.0,  t, 0.0, 1.0,  -t, 0.0, -1.0,  -t, 0.0, 1.0,
    ];
    #[rustfmt::skip]
    let tris = [
        0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11,
        1, 5, 9, 5, 11, 4, 11, 10, 2, 10, 7, 6, 7, 1, 8,
        3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8, 3, 8, 9,
        4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9, 8, 1,
    ];
    polyhedron(&raw, &tris)
}

/// Unit flower head (three's OctahedronGeometry(1, 0)).
pub fn octahedron() -> MeshData {
    #[rustfmt::skip]
    let raw = [1.0, 0.0, 0.0,  -1.0, 0.0, 0.0,  0.0, 1.0, 0.0,  0.0, -1.0, 0.0,  0.0, 0.0, 1.0,  0.0, 0.0, -1.0];
    let tris = [0, 2, 4, 0, 4, 3, 0, 3, 5, 0, 5, 2, 1, 2, 5, 1, 5, 3, 1, 3, 4, 1, 4, 2];
    polyhedron(&raw, &tris)
}

/// Unit cube about the origin (three's BoxGeometry(1, 1, 1)): flat faces.
pub fn cube() -> MeshData {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for axis in 0..3 {
        for sign in [1.0_f32, -1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            // Two tangents with u × v = n, so the corners below wind counter-clockwise from outside.
            let u = Vec3::from_slice(&[[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]][axis]) * sign;
            let v = n.cross(u);
            let first = vertices.len() as u32;
            for (a, b) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
                vertices.push(Vertex { pos: (n * 0.5 + u * a + v * b).into(), normal: n.into(), extra: 0.0 });
            }
            indices.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        }
    }
    MeshData { vertices, indices }
}

/// A butterfly's right wing at unit size: flat, facing up, hinged on the z axis and reaching out
/// along +x (three's PlaneGeometry turned flat and shifted half its width).
pub fn wing() -> MeshData {
    let up = [0.0, 1.0, 0.0];
    let vertices = [[0.0, -0.5], [1.0, -0.5], [1.0, 0.5], [0.0, 0.5]]
        .map(|[x, z]| Vertex { pos: [x, 0.0, z], normal: up, extra: 0.0 })
        .to_vec();
    // Counter-clockwise seen from above.
    MeshData { vertices, indices: vec![0, 3, 2, 0, 2, 1] }
}

/// Unit-height trunk (three's CylinderGeometry(0.12, 0.16, 1, 6)): smooth sides, flat caps.
pub fn trunk() -> MeshData {
    const SEGMENTS: u32 = 6;
    let (top, bottom) = (0.12_f32, 0.16_f32);
    let slope = bottom - top; // over a height of 1
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (y, r) in [(0.5_f32, top), (-0.5, bottom)] {
        for s in 0..=SEGMENTS {
            let theta = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let (sin, cos) = theta.sin_cos();
            let normal = Vec3::new(sin, slope, cos).normalize();
            vertices.push(Vertex { pos: [r * sin, y, r * cos], normal: normal.into(), extra: 0.0 });
        }
    }
    for s in 0..SEGMENTS {
        let (a, b) = (s, s + SEGMENTS + 1); // top, bottom
        indices.extend_from_slice(&[a, b, a + 1, b, b + 1, a + 1]);
    }
    for (y, r, ny) in [(0.5_f32, top, 1.0_f32), (-0.5, bottom, -1.0)] {
        let centre = vertices.len() as u32;
        vertices.push(Vertex { pos: [0.0, y, 0.0], normal: [0.0, ny, 0.0], extra: 0.0 });
        for s in 0..=SEGMENTS {
            let theta = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let (sin, cos) = theta.sin_cos();
            vertices.push(Vertex { pos: [r * sin, y, r * cos], normal: [0.0, ny, 0.0], extra: 0.0 });
        }
        for s in 0..SEGMENTS {
            let (a, b) = (centre + 1 + s, centre + 2 + s);
            if ny > 0.0 { indices.extend_from_slice(&[centre, a, b]) } else { indices.extend_from_slice(&[centre, b, a]) }
        }
    }
    MeshData { vertices, indices }
}

/// Flat fan around the pond centre (local x/z), its rim at `radius(theta)`; `extra` is the
/// distance relative to the rim, so once interpolated it tells a fragment how far out it is.
pub fn fan(radius: fn(f64) -> f64) -> MeshData {
    const SEGMENTS: u32 = 72;
    let mut vertices = vec![Vertex { pos: [0.0; 3], normal: [0.0, 1.0, 0.0], extra: 0.0 }];
    for s in 0..=SEGMENTS {
        let theta = s as f64 / SEGMENTS as f64 * 2.0 * PI;
        let r = radius(theta);
        vertices.push(Vertex { pos: [(theta.cos() * r) as f32, 0.0, (theta.sin() * r) as f32], normal: [0.0, 1.0, 0.0], extra: 1.0 });
    }
    // theta runs from +x toward +z, which is clockwise seen from above: wind (centre, s+1, s).
    let indices = (0..SEGMENTS).flat_map(|s| [0, s + 2, s + 1]).collect();
    MeshData { vertices, indices }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri_normals(m: &MeshData) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        m.indices.chunks(3).map(|t| {
            let p = |i: u32| Vec3::from(m.vertices[i as usize].pos);
            let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
            ((b - a).cross(c - a), (a + b + c) / 3.0)
        })
    }

    #[test]
    fn closed_solids_wind_outward() {
        for m in [dodecahedron(), icosahedron(), octahedron(), cube(), trunk()] {
            for (n, centre) in tri_normals(&m) {
                assert!(n.dot(centre) > 0.0, "inward triangle at {centre}");
            }
        }
    }

    #[test]
    fn flat_meshes_face_up() {
        for m in [ground(), fan(world::pond_radius), fan(world::sand_radius), wing()] {
            assert!(tri_normals(&m).all(|(n, _)| n.y > 0.0));
            assert!(m.vertices.iter().all(|v| v.normal[1] > 0.0));
        }
    }

    #[test]
    fn trunk_side_normals_are_smooth() {
        // Shared ring vertices carry one normal, tilted up slightly by the taper.
        let m = trunk();
        let n = Vec3::from(m.vertices[0].normal);
        assert!(n.y > 0.0 && n.y < 0.1);
    }
}
