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
pub use meadow_core::grass::BLADE_H;
const VIEW_MARGIN: f32 = 0.6; // world units kept past the screen edges: blade height, width and sway

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

    #[test]
    fn blade_roots_sit_on_the_ground_mesh() {
        // Height and normal at each root, interpolated over the mesh triangle the GPU draws there.
        let ground = crate::mesh::ground();
        let n = (ground.vertices.len() as f64).sqrt() as usize - 1; // cells per side
        let cell = world::GROUND_SIZE / n as f64;
        let blades = grass::place_grass(&world::place(42));
        for b in blades.iter().step_by(53) {
            let (gx, gz) = ((b.x as f64 + world::GROUND_SIZE / 2.0) / cell, (b.z as f64 + world::GROUND_SIZE / 2.0) / cell);
            let (i, j) = (gx.floor() as usize, gz.floor() as usize);
            let (u, v) = ((gx - i as f64) as f32, (gz - j as f64) as f32);
            let at = |di: usize, dj: usize| &ground.vertices[(j + dj) * (n + 1) + i + di];
            let tri = if u + v <= 1.0 { [(at(0, 0), 1.0 - u - v), (at(1, 0), u), (at(0, 1), v)] } else { [(at(1, 1), u + v - 1.0), (at(0, 1), 1.0 - u), (at(1, 0), 1.0 - v)] };
            let y: f32 = tri.iter().map(|(p, w)| p.pos[1] * w).sum();
            let normal = tri.iter().map(|(p, w)| Vec3::from(p.normal) * *w).sum::<Vec3>().normalize();
            assert!((b.y - y).abs() < 1e-5, "root at {}, {}: {} vs mesh {y}", b.x, b.z, b.y);
            assert!((Vec3::from(b.normal) - normal).abs().max_element() < 1e-5, "normal at {}, {}: {:?} vs mesh {normal}", b.x, b.z, b.normal);
        }
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
