//! The three passes: sun shadow map → lit scene at art resolution (colour + view normals + depth)
//! → upscale to the window with pixel outlines; once per screen, each looking at its own part of
//! one shared meadow.
use std::borrow::Cow;
use std::fmt::Write;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
use meadow_core::butterflies::{self, Butterflies};
use meadow_core::clock;
use meadow_core::clouds::{self, Clouds};
use meadow_core::wind::Wind;
use meadow_core::weather::Weather;
use meadow_core::dragonflies::{self, Dragonflies};
use meadow_core::fireflies::{self, Fireflies};
use meadow_core::frog::{self, Frog};
use meadow_core::gnats::{self, Gnats};
use meadow_core::flowers::{self, HEAD_R, HEAD_Y};
use meadow_core::flight::V3;
use meadow_core::sky::{self, SkyState};
use meadow_core::tiles::{self, Outer, TILE};
use meadow_core::world;
use meadow_core::grass::{self as blades, BLADE_H};
use meadow_core::wreck;
use wgpu::util::DeviceExt;

use crate::camera::{self, Pane};
use crate::grass::{self, Grass};
use crate::mesh::{self, MeshData, Vertex};

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SHADOW_SIZE: u32 = 2048;

// Web palette (sRGB hex). The time-of-day lights come from meadow_core::sky each frame; the ground
// half of the hemisphere light stays fixed, as the web's.
const HEMI_GROUND: (u32, f32) = (0x3a3326, 1.2);
const GROUND: u32 = 0xa4cc86;
const SAND: u32 = 0xecd8b8;
const TRUNK: u32 = 0x8d6d52;
const LEAVES: u32 = 0x589e63;
const ROCK: u32 = 0xaaafc2;
const IRON: u32 = 0x3a4044; // the post clock's post and rim
const INK: u32 = 0x23262b; // its ticks and hands
const CREAM: u32 = 0xf3ead2; // its face
const MOSS: u32 = 0x55663a; // its pieces, long fallen
const MOSSY_ROCK: u32 = 0x8c9a80; // the frog's stone
const LILY_PAD: u32 = 0x4f8a3a;
const FROG: u32 = 0x5a9a3c;
const FROG_LEGS: u32 = 0x3d6e2a;
const FROG_EYES: u32 = 0x8cc060;
const PUPIL: u32 = 0x1c1c1c;
const THROAT: u32 = 0xe6dca0;
const TONGUE: u32 = 0xd8667a;
const FROG_PARTS: usize = 12;
const SHADER_COLORS: [(&str, u32); 7] = [
    ("PATH_COLOR", 0xc3a284),
    ("WATER_DEEP", 0x3a8fc9),
    ("WATER_MID", 0x4fb0dc),
    ("WATER_SHALLOW", 0x6fcbe6),
    ("WATER_BANK", 0x2c5a7a),
    ("WATER_FOAM", 0xeef6ff),
    ("DIAL_GLOW", 0xffd890),
];

fn linear(hex: u32) -> Vec3 {
    vec3(sky::linear(hex))
}

fn vec3(c: sky::Rgb) -> Vec3 {
    Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32)
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
    fog: [f32; 4],    // rgb: ground fog colour, a: amount
    clouds: [f32; 4], // xy: the big clouds' drift, zw: the wisps'
    wind: [f32; 4],   // xy: where the grass leans, z: the gusts' phase (s), w: their strength
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

    /// In the grass's wind (IN_WIND in scene.wgsl); `color` already linear.
    fn in_wind(model: Mat4, color: Vec3) -> Self {
        Self { model: model.to_cols_array_2d(), base: [0.0, 0.0, 0.0, -2.0], color: color.extend(1.0).into() }
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
    Insect,
    Unlit,
    Dial,
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

// The post clock: the post, rim and face stand still; the ticks and hands share one cube draw,
// rewritten every frame with the hands (and the face, for its fog tint).
/// The post clock (meadow_core::clock) in the pieces it breaks into (meadow_core::wreck).
struct PostClock {
    pieces: Vec<Piece>,
    hub_shape: Mat4, // the hub's shape matrix's prefix: scale, then its centre to the origin
    draws: [Draw; 6], // post segments, rim hub, rim wedges, dial hub, dial wedges, ink (12 ticks, 2 hands)
}

const DIAL_DRAWS: [usize; 2] = [3, 4];
const ON_GRASS: f32 = 0.6 * BLADE_H as f32; // how high the blades hold up a fresh piece
const INK_DRAW: usize = 5;

// One piece: its rigid pose standing in the clock and lying in the grass (rotation, centre), how it
// tumbles between them, and its parts as (draw, colour, shape about the piece's centre).
struct Piece {
    stand: (Quat, Vec3),
    lie: (Quat, Vec3),
    rest: wreck::Rest,
    sink: f32, // how far it settles into the ground, fully weathered
    parts: Vec<(usize, u32, Mat4)>,
}

impl PostClock {
    fn new(device: &wgpu::Device, layout: &world::Layout) -> Self {
        use clock::{POST_H, R, SCALE, X, Z};
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, FRAC_PI_4, PI};
        let (post_h, r, scale, hub_r) = (POST_H as f32, R as f32, SCALE as f32, wreck::HUB_R as f32);
        let base = Vec3::new(X as f32, world::height_at(X, Z) as f32, Z as f32);
        let facing = Quat::from_rotation_y(FRAC_PI_4); // face toward the camera
        let frame = Mat4::from_translation(base) * Mat4::from_quat(facing) * Mat4::from_scale(Vec3::splat(scale));
        let upright = Mat4::from_rotation_x(FRAC_PI_2); // +y (a fan's or cylinder's axis) to +z
        let face_up = Quat::from_rotation_x(-FRAC_PI_2); // lying: the face (+z) to the sky
        let on_side = Quat::from_rotation_z(FRAC_PI_2); // lying: a post segment's axis level
        let at = |x: f32, y: f32, z: f32| Mat4::from_translation(Vec3::new(x, y, z));
        let rests = wreck::rest_spots(layout);
        // A piece centred at `centre` (clock units), lying with `lay` and its back `half` deep.
        let piece = |i: usize, centre: Vec3, lay: Quat, half: f32, parts: &[(usize, u32, Mat4)]| {
            let rest = rests[i];
            // Resting on the grass tops at first, sinking into the grass as it weathers.
            let ground = world::height_at(rest.x, rest.z) as f32 + ON_GRASS + half * scale;
            let shape = Mat4::from_scale(Vec3::splat(scale)) * Mat4::from_translation(-centre);
            Piece {
                stand: (facing, frame.transform_point3(centre)),
                lie: (Quat::from_rotation_y(rest.yaw as f32) * facing * lay, Vec3::new(rest.x as f32, ground, rest.z as f32)),
                rest,
                sink: ON_GRASS + half * scale * 0.7,
                parts: parts.iter().map(|&(draw, color, m)| (draw, color, shape * m)).collect(),
            }
        };
        let mut pieces = Vec::with_capacity(wreck::PIECES);
        for i in 0..wreck::POST {
            let h = post_h / wreck::POST as f32;
            let y = (i as f32 + 0.5) * h;
            let radius = 0.09 + (0.06 - 0.09) * y / post_h; // tapering up
            pieces.push(piece(i, Vec3::Y * y, on_side, radius, &[(0, IRON, at(0.0, y, 0.0) * Mat4::from_scale(Vec3::new(radius, h, radius)))]));
        }
        let rim = |turn: f32| at(0.0, post_h, 0.0) * upright * Mat4::from_rotation_y(turn) * Mat4::from_scale(Vec3::new(1.0, 0.14, 1.0));
        let dial = |turn: f32| at(0.0, post_h, 0.071) * upright * Mat4::from_rotation_y(turn);
        pieces.push(piece(wreck::HUB, Vec3::Y * post_h, face_up, 0.07, &[(1, IRON, rim(0.0)), (3, CREAM, dial(0.0))]));
        for j in 0..wreck::WEDGES {
            let turn = j as f32 * FRAC_PI_3;
            let mid = (hub_r + r + 0.07) / 2.0;
            let centre = rim(turn).transform_point3(Vec3::new(mid * (FRAC_PI_3 / 2.0).cos(), 0.0, mid * (FRAC_PI_3 / 2.0).sin()));
            pieces.push(piece(wreck::WEDGE + j, centre, face_up, 0.07, &[(2, IRON, rim(turn)), (4, CREAM, dial(turn))]));
        }
        // A tick per hour; 12, 3, 6 and 9 are bigger.
        for i in 0..12 {
            let a = i as f32 * PI / 6.0;
            let size = if i % 3 == 0 { Vec3::new(0.07, 0.12, 0.02) } else { Vec3::new(0.045, 0.07, 0.02) };
            let centre = Vec3::new(a.sin() * (r - 0.1), post_h + a.cos() * (r - 0.1), 0.08);
            let m = at(centre.x, centre.y, centre.z) * Mat4::from_rotation_z(-a) * Mat4::from_scale(size);
            pieces.push(piece(wreck::TICK + i, centre, face_up, 0.01, &[(INK_DRAW, INK, m)]));
        }
        let wedge = FRAC_PI_3;
        let blank = |n| vec![Instance::new(Mat4::IDENTITY, 0); n]; // posed before the first draw
        PostClock {
            pieces,
            hub_shape: Mat4::from_scale(Vec3::splat(scale)) * Mat4::from_translation(-Vec3::Y * post_h),
            draws: [
                upload(device, mesh::cylinder(1.0, 1.0, 6), &blank(wreck::POST), Shade::Toon, true),
                upload(device, mesh::cylinder(hub_r, hub_r, 24), &blank(1), Shade::Toon, true),
                upload(device, mesh::sector_prism(hub_r, r + 0.07, wedge, 4), &blank(wreck::WEDGES), Shade::Toon, true),
                upload(device, mesh::fan(|_| wreck::HUB_R), &blank(1), Shade::Dial, true),
                upload(device, mesh::sector(hub_r, r, wedge, 4), &blank(wreck::WEDGES), Shade::Dial, true),
                upload(device, mesh::cube(), &blank(14), Shade::Toon, true),
            ],
        }
    }

    /// Every piece at its place this frame, the hands riding on the hub, the face hazed by the fog.
    fn pose(&self, queue: &wgpu::Queue, clock: &ClockPose, sky: &SkyState, time: f64) {
        // The ground fog barely reaches the face, so haze it here: duller and tinted by the fog.
        let face = linear(CREAM).lerp(vec3(sky.fog_color), 0.35 * sky.fog as f32);
        let mut out: [Vec<Instance>; 6] = Default::default();
        let mut hub = Mat4::IDENTITY;
        for (i, piece) in self.pieces.iter().enumerate() {
            let p = clock.fallen[i];
            let (along, y) = wreck::arc(p, piece.stand.1.y as f64, piece.lie.1.y as f64);
            let along = along as f32;
            let weathered = (clock.weathering * p) as f32;
            let mut centre = piece.stand.1.lerp(piece.lie.1, along);
            centre.y = y as f32 - weathered * piece.sink;
            if p == 0.0 {
                // Cracking: each piece still standing jitters on its own.
                let t = time * 37.0 + i as f64 * 2.3;
                centre += Vec3::new(t.sin() as f32, (t * 1.3).cos() as f32, (t * 0.7).sin() as f32) * (0.012 * clock.shiver) as f32;
            }
            let axis = Vec3::from(piece.rest.axis.map(|c| c as f32));
            let tip = std::f32::consts::TAU * piece.rest.turns as f32 * (std::f32::consts::PI * along).sin(); // over and back
            let tumble = Quat::from_axis_angle(axis, tip);
            let rigid = Mat4::from_rotation_translation(piece.stand.0.slerp(piece.lie.0, along) * tumble, centre);
            if i == wreck::HUB {
                hub = rigid;
            }
            for &(draw, color, shape) in &piece.parts {
                let color = if DIAL_DRAWS.contains(&draw) { face } else { linear(color) };
                out[draw].push(Instance { color: weather(color, weathered).extend(1.0).into(), ..Instance::new(rigid * shape, 0) });
            }
        }
        // A box pivoting at its base, pointing up (12 o'clock) before turning clockwise.
        let weathered = (clock.weathering * clock.fallen[wreck::HUB]) as f32;
        let hand = |angle: f64, length: f32, width: f32, z: f32| {
            let m = hub
                * self.hub_shape
                * Mat4::from_translation(Vec3::new(0.0, clock::POST_H as f32, z))
                * Mat4::from_rotation_z(-angle as f32)
                * Mat4::from_translation(Vec3::new(0.0, length / 2.0 - 0.05, 0.0))
                * Mat4::from_scale(Vec3::new(width, length, 0.02));
            Instance { color: weather(linear(INK), weathered).extend(1.0).into(), ..Instance::new(m, 0) }
        };
        let (hour, minute) = clock.hands;
        out[INK_DRAW].extend([hand(hour, 0.3, 0.08, 0.09), hand(minute, 0.42, 0.06, 0.1)]);
        for (draw, instances) in self.draws.iter().zip(&out) {
            queue.write_buffer(&draw.instances, 0, bytemuck::cast_slice(instances));
        }
    }
}

/// The post clock this frame (from meadow_core::wreck): its hands (radians clockwise from 12), how
/// far each piece has fallen (0-1), how hard the standing ones shiver as it cracks, how weathered
/// the fallen pieces are and the face's glow.
pub struct ClockPose<'a> {
    pub hands: (f64, f64),
    pub fallen: &'a [f64; wreck::PIECES],
    pub shiver: f64,
    pub weathering: f64,
    pub glow: f64,
}

// A piece's colour after lying in the grass: mossy and dull.
fn weather(color: Vec3, weathered: f32) -> Vec3 {
    color.lerp(linear(MOSS), 0.65 * weathered)
}

/// The face's glow: brighter after sunset like a lit street clock, and a little by day too (it
/// faces away from the sun); dimmed by the fog.
fn dial_glow(sky: &SkyState) -> f32 {
    ((0.25 + 0.25 * (1.0 - sky.daylight)) * (1.0 - 0.6 * sky.fog)) as f32
}

// Art-resolution targets, rebuilt with the post bind group that reads them.
struct Targets {
    color: wgpu::TextureView,
    normal: wgpu::TextureView,
    depth: wgpu::TextureView,
    post: wgpu::BindGroup,
}

/// A screen in physical px: its top-left corner relative to the primary's, and its size. The first
/// one is the primary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub offset: (i32, i32),
    pub size: (u32, u32),
}

// How far past a pane the world must exist: a crown standing just below the bottom edge still
// reaches into it.
const PANE_REACH: f32 = 2.5;

// One screen's camera, uniforms, targets and grass. The scene and the shadow map are shared.
struct View {
    offset: (i32, i32),
    art: (u32, u32),
    pane: Pane,
    globals_buf: wgpu::Buffer,
    globals_group: wgpu::BindGroup,
    post_buf: wgpu::Buffer,
    targets: Targets,
    grass: Grass,
}

pub struct Renderer {
    globals: Globals, // the shared part; each view adds its cameras
    sun: Vec3,
    background: Vec3, // this frame's sky: the scene pass's clear colour
    shadow_view: wgpu::TextureView,
    shadow_group: wgpu::BindGroup,
    shadow_pipeline: wgpu::RenderPipeline,
    toon_pipeline: wgpu::RenderPipeline,
    ground_pipeline: wgpu::RenderPipeline,
    water_pipeline: wgpu::RenderPipeline,
    insect_pipeline: wgpu::RenderPipeline,
    grass_pipeline: wgpu::RenderPipeline,
    post_pipeline: wgpu::RenderPipeline,
    post_layout: wgpu::BindGroupLayout,
    manual_srgb: bool,
    draws: Vec<Draw>,
    views: Vec<View>,
    primary: (u32, u32), // the primary screen's size, which every pane is measured from
    butterflies: Butterflies,
    butterfly_body: Draw,  // 12 instances, rewritten every frame
    butterfly_wings: Draw, // 24: left and right of each
    dragonflies: Dragonflies,
    dragonfly_body: Draw,
    dragonfly_wings: Draw, // front and hind, left and right: 4 each
    gnats: Gnats,
    gnat_draw: Draw,
    fireflies: Fireflies,
    firefly_draw: Draw, // after everything opaque, with no depth or normal writes
    glow_pipeline: wgpu::RenderPipeline,
    unlit_pipeline: wgpu::RenderPipeline,
    dial_pipeline: wgpu::RenderPipeline,
    post_clock: PostClock,
    frog: Frog,
    clouds: Clouds,
    wind: Wind,
    frog_draw: Draw,
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
        ("BLADE_W", grass::BLADE_W),
        ("BLADE_H", grass::BLADE_H),
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

fn scene_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    (vs, fs): (&str, &str),
    buffers: &[Option<wgpu::VertexBufferLayout>],
    cull_mode: Option<wgpu::Face>,
) -> wgpu::RenderPipeline {
    overlay_pipeline(device, layout, module, (vs, fs), buffers, cull_mode, false)
}

/// `overlay`: colour only, no depth or normal writes (the web's transparent, depthWrite-false
/// materials), so the outline pass never sees it.
fn overlay_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    (vs, fs): (&str, &str),
    buffers: &[Option<wgpu::VertexBufferLayout>],
    cull_mode: Option<wgpu::Face>,
    overlay: bool,
) -> wgpu::RenderPipeline {
    let target = |format, write_mask| Some(wgpu::ColorTargetState { format, blend: None, write_mask });
    let normal_mask = if overlay { wgpu::ColorWrites::empty() } else { wgpu::ColorWrites::ALL };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fs),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some(vs), compilation_options: Default::default(), buffers },
        primitive: wgpu::PrimitiveState { cull_mode, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState { depth_write_enabled: Some(!overlay), ..depth_state(DEPTH_FORMAT) }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fs),
            compilation_options: Default::default(),
            targets: &[target(COLOR_FORMAT, wgpu::ColorWrites::ALL), target(NORMAL_FORMAT, normal_mask)],
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
        instances: init("instances", bytemuck::cast_slice(instances), wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST),
        instance_count: instances.len() as u32,
        shade,
        casts_shadow,
    }
}

// Every mesh in the meadow, as instanced draws: the inner world, then the outer tiles' (other
// screens').
fn build_draws(device: &wgpu::Device, layout: &world::Layout, flowers: &[flowers::Head], outer: &Outer, tiles: &[(i32, i32)]) -> Vec<Draw> {
    log::info!("layout: {} trees, {} rocks, {} flowers", layout.trees.len(), layout.rocks.len(), flowers.len());
    if !tiles.is_empty() {
        log::info!("outer tiles {tiles:?}: {} trees, {} rocks, {} flowers", outer.trees.len(), outer.rocks.len(), outer.flowers.len());
    }
    let mut ground = mesh::ground();
    for &(i, j) in tiles {
        ground.append(mesh::ground_square(i as f64 * TILE, j as f64 * TILE, TILE));
    }
    let pond = |lift: f64| Mat4::from_translation(Vec3::new(world::POND_X as f32, (world::shore_y() + lift) as f32, world::POND_Z as f32));
    let at = |x: f64, y: f64, z: f64| Mat4::from_translation(Vec3::new(x as f32, y as f32, z as f32));
    let trunks: Vec<_> = layout
        .trees
        .iter()
        .chain(&outer.trees)
        .map(|t| {
            let m = at(t.x, t.y + t.height / 2.0, t.z) * Mat4::from_scale(Vec3::new(1.0, t.height as f32, 1.0));
            Instance::swaying(m, TRUNK, t)
        })
        .collect();
    let crowns: Vec<_> = layout
        .trees
        .iter()
        .chain(&outer.trees)
        .map(|t| Instance::swaying(at(t.x, t.y + t.height + 0.3, t.z) * Mat4::from_scale(Vec3::splat(t.canopy as f32)), LEAVES, t))
        .collect();
    let rocks: Vec<_> = layout
        .rocks
        .iter()
        .chain(&outer.rocks)
        .map(|r| {
            let m = at(r.x, r.y, r.z) * Mat4::from_rotation_y(r.rot_y as f32) * Mat4::from_scale(Vec3::new(r.r as f32, r.sy as f32, r.r as f32));
            Instance::new(m, ROCK)
        })
        .chain([{
            let s = frog::stone();
            Instance::new(at(s.x, s.y, s.z) * Mat4::from_scale(Vec3::new(frog::STONE_R as f32, frog::STONE_H as f32, frog::STONE_R as f32)), MOSSY_ROCK)
        }])
        .collect();
    // Each pad's notch turned a different way.
    let pads: Vec<_> = frog::pads()
        .iter()
        .enumerate()
        .map(|(i, p)| Instance::new(at(p.x, p.y, p.z) * Mat4::from_rotation_y(i as f32 * 2.1), LILY_PAD))
        .collect();
    // Heads in the wind, so the mesh sits HEAD_Y up in its own space (the sway grows with local y).
    let mut head = mesh::octahedron();
    for v in &mut head.vertices {
        v.pos = (Vec3::from(v.pos) * HEAD_R as f32 + Vec3::Y * HEAD_Y as f32).into();
    }
    let heads: Vec<_> = flowers.iter().chain(&outer.flowers).map(|h| Instance::in_wind(at(h.x, h.y - HEAD_Y, h.z), linear(h.color) * h.shade as f32)).collect();
    vec![
        upload(device, ground, &[Instance::new(Mat4::IDENTITY, GROUND)], Shade::Ground, false),
        upload(device, mesh::fan(world::sand_radius), &[Instance::new(pond(0.01), SAND)], Shade::Toon, false),
        upload(device, mesh::fan(world::pond_radius), &[Instance::new(pond(0.02), 0xffffff)], Shade::Water, false),
        upload(device, mesh::trunk(), &trunks, Shade::Toon, true),
        upload(device, mesh::icosahedron(), &crowns, Shade::Toon, true),
        upload(device, mesh::dodecahedron(), &rocks, Shade::Toon, true),
        upload(device, mesh::sector(0.0, frog::PAD_R as f32, std::f32::consts::TAU - 0.5, 12), &pads, Shade::Toon, false),
        upload(device, head, &heads, Shade::Toon, false),
    ]
}

/// The ground under a pane and PANE_REACH past it: where the world must have something.
fn footprint(pane: Pane, art: (u32, u32)) -> [[f64; 2]; 4] {
    let half = pane.half(art) + Vec2::splat(PANE_REACH);
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sy)| {
        let p = camera::ground_at(pane.centre + half * Vec2::new(sx, sy));
        [p.x as f64, p.z as f64]
    })
}

/// The frog in cubes (FROG_PARTS of them), facing +z from between its feet; hidden parts get a
/// zero matrix.
fn frog_parts(f: &frog::Pose) -> Vec<Instance> {
    let frame = Mat4::from_translation(Vec3::new(f.pos.x as f32, f.pos.y as f32, f.pos.z as f32)) * Mat4::from_rotation_y(f.yaw as f32);
    let (stretch, puff) = (f.stretch as f32, f.puff as f32);
    let cube = |centre: Vec3, size: Vec3, color: u32| Instance::new(frame * Mat4::from_translation(centre) * Mat4::from_scale(size), color);
    let eye_h = if f.blink { 0.01 } else { 0.06 };
    let mut parts = vec![
        // Nose up a little; the back legs kick out mid hop.
        Instance::new(frame * Mat4::from_translation(Vec3::new(0.0, 0.09, -0.02)) * Mat4::from_rotation_x(-0.25) * Mat4::from_scale(Vec3::new(0.22, 0.13, 0.26)), FROG),
        cube(Vec3::new(0.0, 0.15, 0.12), Vec3::new(0.2, 0.1, 0.13), FROG),
        cube(Vec3::new(0.0, 0.09, 0.15 + 0.03 * puff), Vec3::new(0.12, 0.05, 0.08) + Vec3::splat(0.11 * puff), THROAT),
    ];
    for side in [-1.0_f32, 1.0] {
        parts.push(cube(Vec3::new(0.07 * side, 0.21, 0.12), Vec3::new(0.06, eye_h, 0.06), FROG_EYES));
        let pupil = Vec3::new(0.07 * side, 0.215, 0.152);
        parts.push(if f.blink { Instance::new(Mat4::ZERO, PUPIL) } else { cube(pupil, Vec3::new(0.03, 0.035, 0.01), PUPIL) });
        let back = 0.16 + 0.14 * stretch;
        parts.push(cube(Vec3::new(0.11 * side, 0.035 + 0.03 * stretch, -0.06 - back / 2.0 + 0.08), Vec3::new(0.07, 0.06, back), FROG_LEGS));
        parts.push(cube(Vec3::new(0.08 * side, 0.04, 0.11 + 0.05 * stretch), Vec3::new(0.04, 0.08, 0.04), FROG_LEGS));
    }
    // The tongue: from the mouth out toward what it snaps at.
    parts.push(match f.tongue {
        Some((at, out)) if out > 0.0 => {
            let mouth = frame.transform_point3(Vec3::new(0.0, 0.13, 0.19));
            let to = Vec3::new(at.x as f32, at.y as f32, at.z as f32) - mouth;
            let reach = to.length() * out as f32;
            let m = Mat4::from_translation(mouth + to.normalize() * reach / 2.0) * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Z, to.normalize())) * Mat4::from_scale(Vec3::new(0.025, 0.02, reach));
            Instance::new(m, TONGUE)
        }
        _ => Instance::new(Mat4::ZERO, TONGUE),
    });
    debug_assert_eq!(parts.len(), FROG_PARTS);
    parts
}

/// Art pixels covering the window; the last row/column may be cut off by the window edge.
fn art_size(width: u32, height: u32, pixel_size: u32) -> (u32, u32) {
    (width.div_ceil(pixel_size), height.div_ceil(pixel_size))
}

impl Renderer {
    /// The meadow for these screens (the first is the primary); `resize` each view before drawing.
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat, screens: &[Screen], pixel_size: u32) -> Self {
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
        let toon_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_toon"), &BUFFERS, Some(wgpu::Face::Back));
        let ground_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_ground"), &BUFFERS, Some(wgpu::Face::Back));
        let water_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_water"), &BUFFERS, Some(wgpu::Face::Back));
        let insect_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_insect"), &BUFFERS, None);
        let unlit_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_unlit"), &BUFFERS, Some(wgpu::Face::Back));
        let dial_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_dial"), &BUFFERS, Some(wgpu::Face::Back));
        let glow_pipeline = overlay_pipeline(device, &scene_layout, &scene_module, ("vs_scene", "fs_unlit"), &BUFFERS, Some(wgpu::Face::Back), true);
        // No culling: a blade faces the camera, and the wind keeps its tip in that plane.
        let grass_pipeline = scene_pipeline(device, &scene_layout, &scene_module, ("vs_grass", "fs_grass"), &grass::BUFFERS, None);
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

        let ground = linear(HEMI_GROUND.0) * HEMI_GROUND.1;
        // The time-of-day fields are set by set_sky, before every upload.
        let globals = Globals {
            view_proj: Mat4::IDENTITY.to_cols_array_2d(), // each view's
            view: camera::view().to_cols_array_2d(),
            hemi_ground: ground.extend(0.0).into(),
            ..Zeroable::zeroed()
        };
        // Placed once: the trees and rocks also thin the grass.
        let world_layout = world::place(42);
        let flowers = flowers::place_flowers(&world_layout);
        let butterflies = Butterflies::new(&world_layout, &flowers);
        let (dragonflies, gnats, fireflies) = (Dragonflies::new(&world_layout), Gnats::new(&world_layout), Fireflies::new(&world_layout));
        let n_dragonflies = dragonflies.poses().len();
        let n_gnats = gnats.poses().len();
        let blank = |n| vec![Instance::new(Mat4::IDENTITY, 0); n]; // posed before the first draw
        // The other screens look past the inner ground: the world grows tiles under them.
        let primary_art = art_size(screens[0].size.0, screens[0].size.1, pixel_size);
        let panes: Vec<(Pane, (u32, u32))> = screens
            .iter()
            .map(|sc| {
                let art = art_size(sc.size.0, sc.size.1, pixel_size);
                (Pane::new(art, sc.offset, primary_art, pixel_size), art)
            })
            .collect();
        let footprints: Vec<[[f64; 2]; 4]> = panes.iter().map(|(pane, art)| footprint(*pane, *art)).collect();
        let tiles = tiles::needed(&footprints, 0.0);
        let start = std::time::Instant::now();
        let mut outer = tiles::place_outer(&tiles);
        let mut all_blades = blades::place_grass(&world_layout);
        log::info!("{} grass blades, {} more on {} outer tiles, placed in {:.0} ms", all_blades.len(), outer.blades.len(), tiles.len(), start.elapsed().as_secs_f64() * 1000.0);
        all_blades.append(&mut outer.blades);
        let views = screens
            .iter()
            .zip(&panes)
            .map(|(sc, &(pane, art))| {
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
                let post_buf = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("post"),
                    size: size_of::<PostParams>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                // A little taller than now, so a new pixel size (which moves the art grid by less
                // than an art pixel) still finds its rows.
                let grass = Grass::new(device, &all_blades, pane, pane.half(art).y + 4.0 * pane.unit);
                View { offset: sc.offset, art, pane, globals_group, targets: Self::targets(device, &post_layout, &post_buf, art), globals_buf, post_buf, grass }
            })
            .collect();
        drop(all_blades);
        Self {
            globals,
            sun: Vec3::Y,
            background: Vec3::ZERO,
            shadow_view,
            shadow_group,
            shadow_pipeline,
            toon_pipeline,
            ground_pipeline,
            water_pipeline,
            insect_pipeline,
            grass_pipeline,
            post_pipeline,
            post_layout,
            manual_srgb: !surface_format.is_srgb(),
            draws: build_draws(device, &world_layout, &flowers, &outer, &tiles),
            views,
            primary: screens[0].size,
            butterfly_body: upload(device, mesh::cube(), &blank(butterflies::COUNT), Shade::Insect, false),
            butterfly_wings: upload(device, mesh::wing(), &blank(butterflies::COUNT * 2), Shade::Insect, false),
            butterflies,
            dragonfly_body: upload(device, mesh::cube(), &blank(n_dragonflies), Shade::Insect, false),
            dragonfly_wings: upload(device, mesh::wing(), &blank(n_dragonflies * 4), Shade::Insect, false),
            dragonflies,
            gnat_draw: upload(device, mesh::cube(), &blank(n_gnats), Shade::Unlit, false),
            gnats,
            firefly_draw: upload(device, mesh::cube(), &blank(fireflies::COUNT), Shade::Unlit, false),
            fireflies,
            glow_pipeline,
            unlit_pipeline,
            dial_pipeline,
            post_clock: PostClock::new(device, &world_layout),
            frog: Frog::new(),
            clouds: Clouds::default(),
            wind: Wind::default(),
            frog_draw: upload(device, mesh::cube(), &blank(FROG_PARTS), Shade::Toon, true),
            pixel_size,
        }
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

    /// This frame's time of day: lights, shadow camera, water tint, ground fog and the sky colour.
    fn set_sky(&mut self, sky: &SkyState) {
        let sun = vec3(sky.sun_pos);
        self.sun = sun;
        self.globals.sun_dir = sun.normalize().extend(0.0).into();
        self.globals.sun_color = vec3(sky.sun_color).extend(0.0).into();
        self.globals.hemi_sky = vec3(sky.hemi_sky).extend(0.0).into();
        self.background = vec3(sky.background);
        // White lerped 10% toward the sky, as the web's water material.
        self.globals.water_tint = Vec3::ONE.lerp(self.background, 0.1).extend(0.0).into();
        self.globals.fog = vec3(sky.fog_color).extend(sky.fog as f32).into();
    }

    /// Takes effect with the next resize of each view.
    pub fn set_pixel_size(&mut self, pixel_size: u32) {
        self.pixel_size = pixel_size;
    }

    /// New window size for view `i` (never 0): rebuilds its art targets, post bind group and camera
    /// together. The primary's (view 0's) size sets the others' scale: resize it first.
    pub fn resize(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, i: usize, width: u32, height: u32) {
        if i == 0 {
            self.primary = (width, height);
        }
        let primary_art = art_size(self.primary.0, self.primary.1, self.pixel_size);
        let view = &mut self.views[i];
        view.art = art_size(width, height, self.pixel_size);
        view.pane = Pane::new(view.art, view.offset, primary_art, self.pixel_size);
        view.targets = Self::targets(device, &self.post_layout, &view.post_buf, view.art);
        view.grass.set_view(view.pane, view.art);
        let params = PostParams { art: [view.art.0, view.art.1], pixel_size: self.pixel_size, manual_srgb: self.manual_srgb as u32 };
        queue.write_buffer(&view.post_buf, 0, bytemuck::bytes_of(&params));
        log::info!("view {i}: art {}x{} at pixel size {}, centre {:?}, {} grass blades drawn", view.art.0, view.art.1, self.pixel_size, view.pane.centre, view.grass.count);
    }

    /// Every insect one step on (in the web's species order) and at its pose for this frame.
    fn pose_insects(&mut self, queue: &wgpu::Queue, time: f64, dt: f64, sky: &SkyState) {
        self.butterflies.update(time, dt, sky.daylight, &self.wind);
        self.dragonflies.update(dt, sky.daylight, self.gnats.swarms());
        self.fireflies.update(time, dt, sky.phase);
        self.gnats.update(time, dt, sky.elevation, self.dragonflies.darting());

        let at = |p: V3| Mat4::from_translation(Vec3::new(p.x as f32, p.y as f32, p.z as f32));
        let wing_scale = Mat4::from_scale(Vec3::new(butterflies::WING_W as f32, 1.0, butterflies::WING_L as f32));
        let (mut body, mut wings) = (Vec::with_capacity(butterflies::COUNT), Vec::with_capacity(butterflies::COUNT * 2));
        for p in self.butterflies.poses() {
            let frame = at(p.pos) * Mat4::from_rotation_y(p.yaw as f32);
            let wing = p.wing as f32;
            body.push(Instance::new(frame * Mat4::from_scale(Vec3::new(0.03, 0.03, 0.12)), butterflies::BODY));
            wings.push(Instance::new(frame * Mat4::from_rotation_z(wing) * wing_scale, p.color));
            // The left wing: the right one turned half round (it's symmetric in z), raised the other way.
            wings.push(Instance::new(frame * Mat4::from_rotation_z(-wing) * Mat4::from_rotation_y(std::f32::consts::PI) * wing_scale, p.color));
        }
        queue.write_buffer(&self.butterfly_body.instances, 0, bytemuck::cast_slice(&body));
        queue.write_buffer(&self.butterfly_wings.instances, 0, bytemuck::cast_slice(&wings));

        let body_scale = Mat4::from_translation(Vec3::new(0.0, 0.0, -0.05)) * Mat4::from_scale(Vec3::new(0.04, 0.04, dragonflies::BODY_L as f32));
        let wing_scale = Mat4::from_scale(Vec3::new(dragonflies::WING_L as f32, 1.0, dragonflies::WING_W as f32));
        let (mut body, mut wings) = (Vec::new(), Vec::new());
        for p in self.dragonflies.poses() {
            let frame = at(p.pos) * Mat4::from_rotation_y(p.yaw as f32);
            let wing = p.wing as f32;
            body.push(Instance::new(frame * body_scale, p.color));
            for z in dragonflies::WING_Z {
                let along = Mat4::from_translation(Vec3::new(0.0, 0.0, z as f32));
                wings.push(Instance::new(frame * Mat4::from_rotation_z(wing) * along * wing_scale, dragonflies::WING_COLOR));
                wings.push(Instance::new(frame * Mat4::from_rotation_z(-wing) * along * Mat4::from_rotation_y(std::f32::consts::PI) * wing_scale, dragonflies::WING_COLOR));
            }
        }
        queue.write_buffer(&self.dragonfly_body.instances, 0, bytemuck::cast_slice(&body));
        queue.write_buffer(&self.dragonfly_wings.instances, 0, bytemuck::cast_slice(&wings));

        // Unlit, so dimmed along the hemisphere light's curve (as the web) to stay as dark as the scene.
        let speck = |p: Option<V3>, size: f64, color: Vec3| {
            let model = p.map_or(Mat4::ZERO, |p| at(p) * Mat4::from_scale(Vec3::splat(size as f32)));
            Instance { color: color.extend(1.0).into(), ..Instance::new(model, 0) }
        };
        let dim = linear(gnats::COLOR) * (world::lerp(0.2, 1.2, sky.daylight) / 1.2) as f32;
        let specks: Vec<_> = self.gnats.poses().iter().map(|&p| speck(p, gnats::SIZE, dim)).collect();
        queue.write_buffer(&self.gnat_draw.instances, 0, bytemuck::cast_slice(&specks));
        // Hidden between flashes, since a dark firefly is invisible against the night.
        let glow = linear(fireflies::GLOW);
        let specks: Vec<_> =
            self.fireflies.poses().iter().map(|p| speck((p.glow > 0.1).then_some(p.pos), fireflies::SIZE, glow * p.glow as f32)).collect();
        queue.write_buffer(&self.firefly_draw.instances, 0, bytemuck::cast_slice(&specks));

        let insects = self.gnats.poses().iter().flatten().copied().chain(self.dragonflies.poses().iter().map(|p| p.pos));
        self.frog.update(dt, sky.daylight, sky.phase, insects);
        let parts = frog_parts(&self.frog.pose(sky.daylight, sky.phase));
        queue.write_buffer(&self.frog_draw.instances, 0, bytemuck::cast_slice(&parts));
    }

    /// Moves the meadow on to animation time `time` (s), `dt` after the last drawn frame, at unix
    /// time `unix` (s, the weather's), with the post clock as `clock`. Once per frame, before
    /// rendering the views.
    pub fn update(&mut self, queue: &wgpu::Queue, time: f64, dt: f64, unix: f64, sky: &SkyState, clock: &ClockPose, weather: Option<&Weather>) {
        self.set_sky(sky);
        self.wind.update(dt, weather);
        self.pose_insects(queue, time, dt, sky);
        self.post_clock.pose(queue, clock, sky, time);
        self.globals.params[0] = time as f32;
        self.globals.params[1] = dial_glow(sky) * clock.glow as f32;
        let [lx, lz] = self.wind.lean();
        self.globals.wind = [lx, lz, self.wind.phase, self.wind.strength].map(|v| v as f32);
        self.globals.params[3] = self.wind.flutter as f32;
        self.clouds.update(time, dt, weather, &self.wind);
        self.globals.params[2] = clouds::cover(unix, sky.elevation, weather) as f32;
        let (big, wisps) = (self.clouds.big, self.clouds.wisps);
        self.globals.clouds = [big[0], big[1], wisps[0], wisps[1]].map(|v| v as f32);
    }

    /// View `i`'s frame into `out`: its own shadow pass (the shared map, centred on its pane),
    /// scene and post.
    pub fn render(&self, device: &wgpu::Device, queue: &wgpu::Queue, i: usize, out: &wgpu::TextureView) {
        let view = &self.views[i];
        let globals = Globals {
            view_proj: (camera::projection(view.art, view.pane) * camera::view()).to_cols_array_2d(),
            light_view_proj: camera::light_view_proj(self.sun, view.pane.ground_centre()).to_cols_array_2d(),
            ..self.globals
        };
        queue.write_buffer(&view.globals_buf, 0, bytemuck::bytes_of(&globals));
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
            pass.set_bind_group(0, &view.globals_group, &[]);
            for d in self.draws.iter().chain(&self.post_clock.draws).chain([&self.frog_draw]).filter(|d| d.casts_shadow) {
                draw(&mut pass, d);
            }
        }
        {
            // Background pixels get the sky colour and a view-space "up" normal.
            let up = (camera::view() * Vec4::Y).truncate().normalize() * 0.5 + 0.5;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[
                    color_attachment(&view.targets.color, self.background.extend(1.0)),
                    color_attachment(&view.targets.normal, up.extend(1.0)),
                ],
                depth_stencil_attachment: depth_attachment(&view.targets.depth),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &view.globals_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            let insects = [&self.frog_draw, &self.butterfly_body, &self.butterfly_wings, &self.dragonfly_body, &self.dragonfly_wings, &self.gnat_draw];
            for d in self.draws.iter().chain(&self.post_clock.draws).chain(insects) {
                pass.set_pipeline(match d.shade {
                    Shade::Toon => &self.toon_pipeline,
                    Shade::Ground => &self.ground_pipeline,
                    Shade::Water => &self.water_pipeline,
                    Shade::Insect => &self.insect_pipeline,
                    Shade::Unlit => &self.unlit_pipeline,
                    Shade::Dial => &self.dial_pipeline,
                });
                draw(&mut pass, d);
            }
            pass.set_pipeline(&self.grass_pipeline);
            pass.set_vertex_buffer(0, view.grass.instances.slice(..));
            pass.draw(0..3, 0..view.grass.count);
            pass.set_pipeline(&self.glow_pipeline);
            draw(&mut pass, &self.firefly_draw);
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
            pass.set_bind_group(0, &view.targets.post, &[]);
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

    #[test]
    fn globals_match_the_wgsl_struct() {
        let src = include_str!("shaders/common.wgsl");
        let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
        let span = module
            .types
            .iter()
            .find_map(|(_, t)| match t.inner {
                naga::TypeInner::Struct { span, .. } if t.name.as_deref() == Some("Globals") => Some(span),
                _ => None,
            })
            .expect("Globals in common.wgsl");
        assert_eq!(span as usize, size_of::<Globals>());
        assert_eq!(size_of::<Globals>(), 336, "three mat4 and nine vec4");
    }
}
