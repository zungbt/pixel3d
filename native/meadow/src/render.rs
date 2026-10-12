//! The three passes: sun shadow map → lit scene at art resolution (colour + view normals + depth)
//! → upscale to the window with pixel outlines.
use std::borrow::Cow;
use std::fmt::Write;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use meadow_core::world;
use wgpu::util::DeviceExt;

use crate::camera;
use crate::mesh::{self, MeshData, Vertex};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_SIZE: u32 = 2048;

// Web palette (sRGB hex), noon lights as the web's sky.js.
const BACKGROUND: u32 = 0x7fb2e5;
const SUN: (u32, f32) = (0xfff1d6, 2.5);
const HEMI_SKY: (u32, f32) = (0xbbe4f4, 1.2);
const HEMI_GROUND: (u32, f32) = (0x3a3326, 1.2);
const GROUND: u32 = 0xa4cc86;
const SAND: u32 = 0xecd8b8;
const TRUNK: u32 = 0x8d6d52;
const LEAVES: u32 = 0x589e63;
const ROCK: u32 = 0xaaafc2;
const SHADER_COLORS: [(&str, u32); 6] = [
    ("PATH_COLOR", 0xc3a284),
    ("WATER_DEEP", 0x3a8fc9),
    ("WATER_MID", 0x4fb0dc),
    ("WATER_SHALLOW", 0x6fcbe6),
    ("WATER_BANK", 0x2c5a7a),
    ("WATER_FOAM", 0xeef6ff),
];

fn linear(hex: u32) -> Vec3 {
    let c = |shift: u32| {
        let s = ((hex >> shift) & 0xff) as f32 / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    Vec3::new(c(16), c(8), c(0))
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    view: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    sun_dir: [f32; 4],
    sun_color: [f32; 4],
    hemi_sky: [f32; 4],
    hemi_ground: [f32; 4],
    water_tint: [f32; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    base: [f32; 4], // xyz: sway pivot, w: sway phase (< 0: no sway)
    color: [f32; 4],
}

impl Instance {
    fn new(model: Mat4, color: u32) -> Self {
        Self { model: model.to_cols_array_2d(), base: [0.0, 0.0, 0.0, -1.0], color: linear(color).extend(1.0).into() }
    }

    fn swaying(model: Mat4, color: u32, tree: &world::Tree) -> Self {
        Self { base: [tree.x as f32, tree.y as f32, tree.z as f32, tree.phase as f32], ..Self::new(model, color) }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PostParams {
    art: [u32; 2],
    pixel_size: u32,
    manual_srgb: u32,
}

#[derive(Clone, Copy)]
enum Shade {
    Toon,
    Ground,
    Water,
}

struct Draw {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    instances: wgpu::Buffer,
    instance_count: u32,
    shade: Shade,
    casts_shadow: bool,
}

// Art-resolution targets, rebuilt with the post bind group that reads them.
struct Targets {
    color: wgpu::TextureView,
    normal: wgpu::TextureView,
    depth: wgpu::TextureView,
    post: wgpu::BindGroup,
}

pub struct Renderer {
    globals: Globals,
    globals_buf: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    shadow_view: wgpu::TextureView,
    shadow_group: wgpu::BindGroup,
    shadow_pipeline: wgpu::RenderPipeline,
    toon_pipeline: wgpu::RenderPipeline,
    ground_pipeline: wgpu::RenderPipeline,
    water_pipeline: wgpu::RenderPipeline,
    post_pipeline: wgpu::RenderPipeline,
    post_layout: wgpu::BindGroupLayout,
    post_buf: wgpu::Buffer,
    manual_srgb: bool,
    draws: Vec<Draw>,
    targets: Targets,
    pixel_size: u32,
}

/// WGSL constants from meadow_core, so the shader's path and palette can't drift from the CPU side.
fn prelude() -> String {
    use world::path::*;
    let consts = [
        ("PATH_X0", X0),
        ("PATH_AMP", AMP),
        ("PATH_FREQ", FREQ),
        ("PATH_PHASE", PHASE),
        ("PATH_W_BASE", W_BASE),
        ("PATH_W_A1", W_A1),
        ("PATH_W_F1", W_F1),
        ("PATH_W_A2", W_A2),
        ("PATH_W_F2", W_F2),
        ("BRANCH_X0", branch_x0()),
        ("BRANCH_A", BRANCH_A),
        ("BRANCH_Z0", BRANCH_Z0),
        ("BRANCH_X1", BRANCH_X1),
        ("BRANCH_TAPER", BRANCH_TAPER),
        ("BRANCH_WIDTH", BRANCH_WIDTH),
    ];
    let mut s = String::new();
    for (name, v) in consts {
        writeln!(s, "const {name}: f32 = {:?};", v as f32).unwrap();
    }
    for (name, hex) in SHADER_COLORS {
        let c = linear(hex);
        writeln!(s, "const {name}: vec3f = vec3f({:?}, {:?}, {:?});", c.x, c.y, c.z).unwrap();
    }
    s
}

fn shader(device: &wgpu::Device, label: &str, parts: &[&str]) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(Cow::Owned(parts.concat())) })
}

fn texture(device: &wgpu::Device, label: &str, (w, h): (u32, u32), format: wgpu::TextureFormat) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn texture_entry(binding: u32, sample_type: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    }
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
        count: None,
    }
}

const VERTEX_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32];
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 6] =
    wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32x4];
const BUFFERS: [Option<wgpu::VertexBufferLayout>; 2] = [
    Some(wgpu::VertexBufferLayout { array_stride: size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &VERTEX_ATTRS }),
    Some(wgpu::VertexBufferLayout { array_stride: size_of::<Instance>() as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &INSTANCE_ATTRS }),
];

fn depth_state(format: wgpu::TextureFormat) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::Less),
        stencil: Default::default(),
        bias: Default::default(),
    }
}

fn scene_pipeline(device: &wgpu::Device, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, fs: &str) -> wgpu::RenderPipeline {
    let target = |format| Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs_scene"), compilation_options: Default::default(), buffers: &BUFFERS },
        primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Back), ..Default::default() },
        depth_stencil: Some(depth_state(DEPTH_FORMAT)),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[target(COLOR_FORMAT), target(NORMAL_FORMAT)],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn upload(device: &wgpu::Device, mesh: MeshData, instances: &[Instance], shade: Shade, casts_shadow: bool) -> Draw {
    let init = |label, contents: &[u8], usage| device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage });
    Draw {
        vertices: init("vertices", bytemuck::cast_slice(&mesh.vertices), wgpu::BufferUsages::VERTEX),
        indices: init("indices", bytemuck::cast_slice(&mesh.indices), wgpu::BufferUsages::INDEX),
        index_count: mesh.indices.len() as u32,
        instances: init("instances", bytemuck::cast_slice(instances), wgpu::BufferUsages::VERTEX),
        instance_count: instances.len() as u32,
        shade,
        casts_shadow,
    }
}

// Every mesh in the meadow, as instanced draws.
fn build_draws(device: &wgpu::Device) -> Vec<Draw> {
    let layout = world::place(42);
    log::info!("layout: {} trees, {} rocks", layout.trees.len(), layout.rocks.len());
    let pond = |lift: f64| Mat4::from_translation(Vec3::new(world::POND_X as f32, (world::shore_y() + lift) as f32, world::POND_Z as f32));
    let at = |x: f64, y: f64, z: f64| Mat4::from_translation(Vec3::new(x as f32, y as f32, z as f32));
    let trunks: Vec<_> = layout
        .trees
        .iter()
        .map(|t| {
            let m = at(t.x, t.y + t.height / 2.0, t.z) * Mat4::from_scale(Vec3::new(1.0, t.height as f32, 1.0));
            Instance::swaying(m, TRUNK, t)
        })
        .collect();
    let crowns: Vec<_> = layout
        .trees
        .iter()
        .map(|t| Instance::swaying(at(t.x, t.y + t.height + 0.3, t.z) * Mat4::from_scale(Vec3::splat(t.canopy as f32)), LEAVES, t))
        .collect();
    let rocks: Vec<_> = layout
        .rocks
        .iter()
        .map(|r| {
            let m = at(r.x, r.y, r.z) * Mat4::from_rotation_y(r.rot_y as f32) * Mat4::from_scale(Vec3::new(r.r as f32, r.sy as f32, r.r as f32));
            Instance::new(m, ROCK)
        })
        .collect();
    vec![
        upload(device, mesh::ground(), &[Instance::new(Mat4::IDENTITY, GROUND)], Shade::Ground, false),
        upload(device, mesh::fan(world::sand_radius), &[Instance::new(pond(0.01), SAND)], Shade::Toon, false),
        upload(device, mesh::fan(world::pond_radius), &[Instance::new(pond(0.02), 0xffffff)], Shade::Water, false),
        upload(device, mesh::trunk(), &trunks, Shade::Toon, true),
        upload(device, mesh::icosahedron(), &crowns, Shade::Toon, true),
        upload(device, mesh::dodecahedron(), &rocks, Shade::Toon, true),
    ]
}

/// Art pixels covering the window; the last row/column may be cut off by the window edge.
fn art_size(width: u32, height: u32, pixel_size: u32) -> (u32, u32) {
    (width.div_ceil(pixel_size), height.div_ceil(pixel_size))
}

impl Renderer {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, width: u32, height: u32, pixel_size: u32) -> Self {
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow map"),
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Depth),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                texture_entry(0, wgpu::TextureSampleType::Float { filterable: false }),
                texture_entry(1, wgpu::TextureSampleType::Float { filterable: false }),
                texture_entry(2, wgpu::TextureSampleType::Depth),
                uniform_entry(3, wgpu::ShaderStages::FRAGMENT),
            ],
        });

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals_buf.as_entire_binding() }],
        });
        // The shadow pass binds globals only: the shadow map can't be sampled while it's the target.
        let shadow_view = texture(device, "shadow map", (SHADOW_SIZE, SHADOW_SIZE), DEPTH_FORMAT);
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow map"),
            layout: &shadow_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&shadow_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&shadow_sampler) },
            ],
        });

        let common = include_str!("shaders/common.wgsl");
        let prelude = prelude();
        let shadow_module = shader(device, "shadow", &[common, include_str!("shaders/shadow.wgsl")]);
        let scene_module = shader(device, "scene", &[common, &prelude, include_str!("shaders/scene.wgsl")]);
        let post_module = shader(device, "post", &[include_str!("shaders/post.wgsl")]);

        let pipeline_layout = |label, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: groups, immediate_size: 0 })
        };
        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow"),
            layout: Some(&pipeline_layout("shadow", &[Some(&globals_layout)])),
            vertex: wgpu::VertexState { module: &shadow_module, entry_point: Some("vs_shadow"), compilation_options: Default::default(), buffers: &BUFFERS },
            // Back faces into the map: acne lands on faces the sun doesn't light anyway.
            primitive: wgpu::PrimitiveState { cull_mode: Some(wgpu::Face::Front), ..Default::default() },
            depth_stencil: Some(depth_state(DEPTH_FORMAT)),
            multisample: Default::default(),
            fragment: None,
            multiview_mask: None,
            cache: None,
        });
        let scene_layout = pipeline_layout("scene", &[Some(&globals_layout), Some(&shadow_layout)]);
        let toon_pipeline = scene_pipeline(device, &scene_layout, &scene_module, "fs_toon");
        let ground_pipeline = scene_pipeline(device, &scene_layout, &scene_module, "fs_ground");
        let water_pipeline = scene_pipeline(device, &scene_layout, &scene_module, "fs_water");
        let post_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("post"),
            layout: Some(&pipeline_layout("post", &[Some(&post_layout)])),
            vertex: wgpu::VertexState { module: &post_module, entry_point: Some("vs_post"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &post_module,
                entry_point: Some("fs_post"),
                compilation_options: Default::default(),
                targets: &[Some(surface_format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let post_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post"),
            size: size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (sun, sky, ground) = (linear(SUN.0) * SUN.1, linear(HEMI_SKY.0) * HEMI_SKY.1, linear(HEMI_GROUND.0) * HEMI_GROUND.1);
        let globals = Globals {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(), // set by resize
            view: camera::view().to_cols_array_2d(),
            light_view_proj: camera::light_view_proj().to_cols_array_2d(),
            sun_dir: camera::SUN_POS.normalize().extend(0.0).into(),
            sun_color: sun.extend(0.0).into(),
            hemi_sky: sky.extend(0.0).into(),
            hemi_ground: ground.extend(0.0).into(),
            // White lerped 10% toward the background, as the web's water material.
            water_tint: Vec3::ONE.lerp(linear(BACKGROUND), 0.1).extend(0.0).into(),
            params: [0.0; 4],
        };
        let targets = Self::targets(device, &post_layout, &post_buf, art_size(width, height, pixel_size));
        let mut renderer = Self {
            globals,
            globals_buf,
            globals_group,
            shadow_view,
            shadow_group,
            shadow_pipeline,
            toon_pipeline,
            ground_pipeline,
            water_pipeline,
            post_pipeline,
            post_layout,
            post_buf,
            manual_srgb: !surface_format.is_srgb(),
            draws: build_draws(device),
            targets,
            pixel_size,
        };
        renderer.set_camera(art_size(width, height, pixel_size));
        renderer
    }

    fn targets(device: &wgpu::Device, post_layout: &wgpu::BindGroupLayout, post_buf: &wgpu::Buffer, art: (u32, u32)) -> Targets {
        let color = texture(device, "colour", art, COLOR_FORMAT);
        let normal = texture(device, "normals", art, NORMAL_FORMAT);
        let depth = texture(device, "depth", art, DEPTH_FORMAT);
        let post = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post"),
            layout: post_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&color) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&normal) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&depth) },
                wgpu::BindGroupEntry { binding: 3, resource: post_buf.as_entire_binding() },
            ],
        });
        Targets { color, normal, depth, post }
    }

    fn set_camera(&mut self, (w, h): (u32, u32)) {
        self.globals.view_proj = (camera::projection(w, h) * camera::view()).to_cols_array_2d();
    }

    /// New window size (never 0): rebuilds the art targets, the post bind group and the camera together.
    pub fn resize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) {
        let art = art_size(width, height, self.pixel_size);
        self.targets = Self::targets(device, &self.post_layout, &self.post_buf, art);
        self.set_camera(art);
        let params = PostParams { art: [art.0, art.1], pixel_size: self.pixel_size, manual_srgb: self.manual_srgb as u32 };
        queue.write_buffer(&self.post_buf, 0, bytemuck::bytes_of(&params));
        log::info!("art {}x{} at pixel size {}", art.0, art.1, self.pixel_size);
    }

    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, out: &wgpu::TextureView, time: f32) {
        self.globals.params[0] = time;
        queue.write_buffer(&self.globals_buf, 0, bytemuck::bytes_of(&self.globals));
        let mut encoder = device.create_command_encoder(&Default::default());
        let depth_attachment = |view| {
            Some(wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            })
        };
        let color_attachment = |view, clear: Vec4| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: clear.x as f64, g: clear.y as f64, b: clear.z as f64, a: clear.w as f64 }),
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        let draw = |pass: &mut wgpu::RenderPass, d: &Draw| {
            pass.set_vertex_buffer(0, d.vertices.slice(..));
            pass.set_vertex_buffer(1, d.instances.slice(..));
            pass.set_index_buffer(d.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..d.index_count, 0, 0..d.instance_count);
        };

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow"),
                color_attachments: &[],
                depth_stencil_attachment: depth_attachment(&self.shadow_view),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.globals_group, &[]);
            for d in self.draws.iter().filter(|d| d.casts_shadow) {
                draw(&mut pass, d);
            }
        }
        {
            // Background pixels get the sky colour and a view-space "up" normal.
            let up = (camera::view() * Vec4::Y).truncate().normalize() * 0.5 + 0.5;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[
                    color_attachment(&self.targets.color, linear(BACKGROUND).extend(1.0)),
                    color_attachment(&self.targets.normal, up.extend(1.0)),
                ],
                depth_stencil_attachment: depth_attachment(&self.targets.depth),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.globals_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            for d in &self.draws {
                pass.set_pipeline(match d.shade {
                    Shade::Toon => &self.toon_pipeline,
                    Shade::Ground => &self.ground_pipeline,
                    Shade::Water => &self.water_pipeline,
                });
                draw(&mut pass, d);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[color_attachment(out, Vec4::ZERO)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.post_pipeline);
            pass.set_bind_group(0, &self.targets.post, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaders_validate() {
        let common = include_str!("shaders/common.wgsl");
        let prelude = prelude();
        for (name, src) in [
            ("shadow", [common, include_str!("shaders/shadow.wgsl")].concat()),
            ("scene", [common, &prelude, include_str!("shaders/scene.wgsl")].concat()),
            ("post", include_str!("shaders/post.wgsl").to_string()),
        ] {
            let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(&src)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        }
    }

    #[test]
    fn art_pixels_cover_the_window() {
        assert_eq!(art_size(1280, 720, 2), (640, 360));
        assert_eq!(art_size(1281, 719, 3), (427, 240));
    }

    #[test]
    fn linear_matches_srgb_decode() {
        assert!((linear(0xffffff) - Vec3::ONE).length() < 1e-6);
        assert!((linear(0x808080).x - 0.2158605).abs() < 1e-5);
    }
}
