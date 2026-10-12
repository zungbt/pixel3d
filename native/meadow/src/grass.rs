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
