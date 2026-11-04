//! SPIKE: low-poly GLB / glTF loader. Plan: docs/superpowers/plans/2026-11-04-voxel-asset-spike.md, Task 3.
use crate::mesh::{MeshData, Vertex};

// SPIKE
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// SPIKE
struct Triangle {
    p0: glam::Vec3,
    p1: glam::Vec3,
    p2: glam::Vec3,
    normal: glam::Vec3,
    color_srgb: u32,
}

// SPIKE
fn traverse_node(
    node: &gltf::Node,
    parent_transform: glam::Mat4,
    buffers: &[gltf::buffer::Data],
    triangles: &mut Vec<Triangle>,
) {
    let local_transform = glam::Mat4::from_cols_array_2d(&node.transform().matrix());
    let world_transform = parent_transform * local_transform;

    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                log::warn!("skipping non-triangles primitive: {:?}", primitive.mode());
                continue;
            }

            let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|d| &**d));
            let Some(positions_iter) = reader.read_positions() else {
                continue;
            };
            let positions: Vec<[f32; 3]> = positions_iter.collect();

            let indices: Vec<u32> = match reader.read_indices() {
                Some(iter) => iter.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };

            let colors: Option<Vec<[f32; 3]>> = reader.read_colors(0).map(|c| c.into_rgb_f32().collect());

            let base_color = primitive.material().pbr_metallic_roughness().base_color_factor();
            let base_rgb = [base_color[0], base_color[1], base_color[2]];

            let normal_matrix = glam::Mat3::from_mat4(world_transform).inverse().transpose();

            for chunk in indices.chunks_exact(3) {
                let (i0, i1, i2) = (chunk[0] as usize, chunk[1] as usize, chunk[2] as usize);
                if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
                    continue;
                }

                let p0_local = glam::Vec3::from(positions[i0]);
                let p1_local = glam::Vec3::from(positions[i1]);
                let p2_local = glam::Vec3::from(positions[i2]);

                let p0_world = world_transform.transform_point3(p0_local);
                let p1_world = world_transform.transform_point3(p1_local);
                let p2_world = world_transform.transform_point3(p2_local);

                let local_normal = (p1_local - p0_local).cross(p2_local - p0_local);
                let mut normal = normal_matrix * local_normal;
                if normal.length_squared() > 1e-12 {
                    normal = normal.normalize();
                } else {
                    let world_face = (p1_world - p0_world).cross(p2_world - p0_world);
                    if world_face.length_squared() > 1e-12 {
                        normal = world_face.normalize();
                    } else {
                        normal = glam::Vec3::Z;
                    }
                }

                let c0 = colors.as_ref().and_then(|c| c.get(i0).copied()).unwrap_or([1.0, 1.0, 1.0]);
                let c1 = colors.as_ref().and_then(|c| c.get(i1).copied()).unwrap_or([1.0, 1.0, 1.0]);
                let c2 = colors.as_ref().and_then(|c| c.get(i2).copied()).unwrap_or([1.0, 1.0, 1.0]);

                let mean_color = [
                    (c0[0] + c1[0] + c2[0]) / 3.0,
                    (c0[1] + c1[1] + c2[1]) / 3.0,
                    (c0[2] + c1[2] + c2[2]) / 3.0,
                ];

                let linear_rgb = [
                    base_rgb[0] * mean_color[0],
                    base_rgb[1] * mean_color[1],
                    base_rgb[2] * mean_color[2],
                ];

                let r = (linear_to_srgb(linear_rgb[0]) * 255.0).round().clamp(0.0, 255.0) as u32;
                let g = (linear_to_srgb(linear_rgb[1]) * 255.0).round().clamp(0.0, 255.0) as u32;
                let b = (linear_to_srgb(linear_rgb[2]) * 255.0).round().clamp(0.0, 255.0) as u32;
                let color_srgb = (r << 16) | (g << 8) | b;

                triangles.push(Triangle {
                    p0: p0_world,
                    p1: p1_world,
                    p2: p2_world,
                    normal,
                    color_srgb,
                });
            }
        }
    }

    for child in node.children() {
        traverse_node(&child, world_transform, buffers, triangles);
    }
}

// SPIKE
pub fn load(bytes: &[u8]) -> Result<Vec<(u32, MeshData)>, String> {
    let (document, buffers, _images) = gltf::import_slice(bytes).map_err(|e| e.to_string())?;

    let scene = document.default_scene().or_else(|| document.scenes().next());
    let Some(scene) = scene else {
        return Ok(Vec::new());
    };

    let mut triangles = Vec::new();
    for node in scene.nodes() {
        traverse_node(&node, glam::Mat4::IDENTITY, &buffers, &mut triangles);
    }

    if triangles.is_empty() {
        return Ok(Vec::new());
    }

    let mut min = glam::Vec3::splat(f32::INFINITY);
    let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
    for tri in &triangles {
        for p in &[tri.p0, tri.p1, tri.p2] {
            min = min.min(*p);
            max = max.max(*p);
        }
    }

    let bottom_centre = glam::Vec3::new(
        (min.x + max.x) * 0.5,
        min.y,
        (min.z + max.z) * 0.5,
    );

    let mut result: Vec<(u32, MeshData)> = Vec::new();
    for tri in triangles {
        let v0 = Vertex {
            pos: (tri.p0 - bottom_centre).into(),
            normal: tri.normal.into(),
            extra: 0.0,
        };
        let v1 = Vertex {
            pos: (tri.p1 - bottom_centre).into(),
            normal: tri.normal.into(),
            extra: 0.0,
        };
        let v2 = Vertex {
            pos: (tri.p2 - bottom_centre).into(),
            normal: tri.normal.into(),
            extra: 0.0,
        };

        if let Some((_, mesh)) = result.iter_mut().find(|(c, _)| *c == tri.color_srgb) {
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend([v0, v1, v2]);
            mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
        } else {
            result.push((tri.color_srgb, MeshData {
                vertices: vec![v0, v1, v2],
                indices: vec![0, 1, 2],
            }));
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    // SPIKE
    use super::*;

    // SPIKE
    fn encode_base64(bytes: &[u8]) -> String {
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0];
            let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
            out.push(TABLE[(b0 >> 2) as usize] as char);
            out.push(TABLE[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
            } else {
                out.push('=');
            }
            if chunk.len() > 2 {
                out.push(TABLE[(b2 & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
        out
    }

    #[test]
    // SPIKE
    fn a_triangle_round_trips_with_flat_normal_and_bottom_centre_origin() {
        // Triangle (0,0,0) (1,0,0) (0,1,0), base colour factor [1,1,1,1], node translation (5,5,5).
        // → 1 mesh, colour 0xffffff, 3 vertices; positions shifted so the bbox bottom centre is the
        // origin: (-0.5,0,0) (0.5,0,0) (-0.5,1,0); normal (0,0,1) on all three.
        let mut buf = Vec::new();
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        for p in &positions {
            for &c in p {
                buf.extend_from_slice(&c.to_le_bytes());
            }
        }
        let b64 = encode_base64(&buf);
        let gltf_json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0,
  "nodes": [{{
    "mesh": 0,
    "translation": [5.0, 5.0, 5.0]
  }}],
  "meshes": [{{
    "primitives": [{{
      "attributes": {{
        "POSITION": 0
      }},
      "material": 0
    }}]
  }}],
  "materials": [{{
    "pbrMetallicRoughness": {{
      "baseColorFactor": [1.0, 1.0, 1.0, 1.0]
    }}
  }}],
  "accessors": [{{
    "bufferView": 0,
    "byteOffset": 0,
    "componentType": 5126,
    "count": 3,
    "type": "VEC3",
    "max": [1.0, 1.0, 0.0],
    "min": [0.0, 0.0, 0.0]
  }}],
  "bufferViews": [{{
    "buffer": 0,
    "byteOffset": 0,
    "byteLength": 36
  }}],
  "buffers": [{{
    "byteLength": 36,
    "uri": "data:application/octet-stream;base64,{}"
  }}]
}}"#,
            b64
        );

        let meshes = load(gltf_json.as_bytes()).expect("load should succeed");
        assert_eq!(meshes.len(), 1);
        let (color, ref mesh) = meshes[0];
        assert_eq!(color, 0xffffff);
        assert_eq!(mesh.vertices.len(), 3);

        let p0 = mesh.vertices[0].pos;
        let p1 = mesh.vertices[1].pos;
        let p2 = mesh.vertices[2].pos;
        assert!((p0[0] - (-0.5)).abs() < 1e-4 && (p0[1] - 0.0).abs() < 1e-4 && (p0[2] - 0.0).abs() < 1e-4, "p0: {:?}", p0);
        assert!((p1[0] - 0.5).abs() < 1e-4 && (p1[1] - 0.0).abs() < 1e-4 && (p1[2] - 0.0).abs() < 1e-4, "p1: {:?}", p1);
        assert!((p2[0] - (-0.5)).abs() < 1e-4 && (p2[1] - 1.0).abs() < 1e-4 && (p2[2] - 0.0).abs() < 1e-4, "p2: {:?}", p2);

        for v in &mesh.vertices {
            assert!((v.normal[0] - 0.0).abs() < 1e-4 && (v.normal[1] - 0.0).abs() < 1e-4 && (v.normal[2] - 1.0).abs() < 1e-4, "normal: {:?}", v.normal);
        }
    }

    #[test]
    // SPIKE
    fn material_colour_is_converted_to_srgb_hex() {
        // factor [0.2158605, 0.2158605, 0.2158605, 1] → colour 0x808080.
        let mut buf = Vec::new();
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        for p in &positions {
            for &c in p {
                buf.extend_from_slice(&c.to_le_bytes());
            }
        }
        let b64 = encode_base64(&buf);
        let gltf_json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0,
  "nodes": [{{
    "mesh": 0
  }}],
  "meshes": [{{
    "primitives": [{{
      "attributes": {{
        "POSITION": 0
      }},
      "material": 0
    }}]
  }}],
  "materials": [{{
    "pbrMetallicRoughness": {{
      "baseColorFactor": [0.2158605, 0.2158605, 0.2158605, 1.0]
    }}
  }}],
  "accessors": [{{
    "bufferView": 0,
    "byteOffset": 0,
    "componentType": 5126,
    "count": 3,
    "type": "VEC3",
    "max": [1.0, 1.0, 0.0],
    "min": [0.0, 0.0, 0.0]
  }}],
  "bufferViews": [{{
    "buffer": 0,
    "byteOffset": 0,
    "byteLength": 36
  }}],
  "buffers": [{{
    "byteLength": 36,
    "uri": "data:application/octet-stream;base64,{}"
  }}]
}}"#,
            b64
        );

        let meshes = load(gltf_json.as_bytes()).expect("load should succeed");
        assert_eq!(meshes.len(), 1);
        assert_eq!(meshes[0].0, 0x808080);
    }

    #[test]
    // SPIKE
    fn vertex_colours_split_the_mesh_by_colour() {
        // Two triangles in one primitive, no material: COLOR_0 (VEC3 float) is [1,0,0] on the first
        // triangle's 3 vertices and [0,0,1] on the second's → 2 meshes, colours {0xff0000, 0x0000ff},
        // 3 vertices each.
        let mut buf = Vec::new();
        let positions: [[f32; 3]; 6] = [
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0],
        ];
        for p in &positions {
            for &c in p {
                buf.extend_from_slice(&c.to_le_bytes());
            }
        }
        let colors: [[f32; 3]; 6] = [
            [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0],
        ];
        for col in &colors {
            for &c in col {
                buf.extend_from_slice(&c.to_le_bytes());
            }
        }
        let b64 = encode_base64(&buf);
        let gltf_json = format!(
            r#"{{
  "asset": {{ "version": "2.0" }},
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0,
  "nodes": [{{
    "mesh": 0
  }}],
  "meshes": [{{
    "primitives": [{{
      "attributes": {{
        "POSITION": 0,
        "COLOR_0": 1
      }}
    }}]
  }}],
  "accessors": [
    {{
      "bufferView": 0,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 6,
      "type": "VEC3",
      "max": [1.0, 1.0, 1.0],
      "min": [0.0, 0.0, 0.0]
    }},
    {{
      "bufferView": 1,
      "byteOffset": 0,
      "componentType": 5126,
      "count": 6,
      "type": "VEC3"
    }}
  ],
  "bufferViews": [
    {{
      "buffer": 0,
      "byteOffset": 0,
      "byteLength": 72
    }},
    {{
      "buffer": 0,
      "byteOffset": 72,
      "byteLength": 72
    }}
  ],
  "buffers": [{{
    "byteLength": 144,
    "uri": "data:application/octet-stream;base64,{}"
  }}]
}}"#,
            b64
        );

        let meshes = load(gltf_json.as_bytes()).expect("load should succeed");
        assert_eq!(meshes.len(), 2);
        let colors: std::collections::HashSet<u32> = meshes.iter().map(|(c, _)| *c).collect();
        assert!(colors.contains(&0xff0000));
        assert!(colors.contains(&0x0000ff));
        for (_, mesh) in &meshes {
            assert_eq!(mesh.vertices.len(), 3);
        }
    }

    #[test]
    // SPIKE
    fn the_real_polyfork_tree_loads_in_four_colours() {
        // If C:/Users/d/pixel3d-assets/polyfork/lineside-tree-97e02a.glb exists (otherwise eprintln and
        // return): load → 4 meshes (the file has 4 distinct COLOR_0 values over 2 primitives of 153 and 316
        // triangles), 469 triangles in total, and every normal has length 1 ± 1e-4.
        let path = std::path::Path::new("C:/Users/d/pixel3d-assets/polyfork/lineside-tree-97e02a.glb");
        if !path.exists() {
            eprintln!("Skipping test: {:?} does not exist", path);
            return;
        }
        let bytes = std::fs::read(path).expect("failed to read test glb");
        let meshes = load(&bytes).expect("failed to load glb");
        assert_eq!(meshes.len(), 4);
        let total_triangles: usize = meshes.iter().map(|(_, m)| m.indices.len() / 3).sum();
        assert_eq!(total_triangles, 469);
        for (_, mesh) in &meshes {
            for v in &mesh.vertices {
                let n = glam::Vec3::from(v.normal);
                let len = n.length();
                assert!((len - 1.0).abs() <= 1e-4, "normal length {len} not 1 ± 1e-4");
            }
        }
    }

    #[test]
    // SPIKE
    fn garbage_is_an_error_not_a_panic() {
        // load(b"nope") is Err.
        assert!(load(b"nope").is_err());
    }
}
