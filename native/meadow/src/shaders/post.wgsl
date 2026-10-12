// Upscales the art-resolution scene to the window, every art pixel exactly pixel_size² screen
// pixels, and darkens/brightens depth and normal edges into outlines (as three's RenderPixelatedPass).

struct Post {
    art: vec2u,
    pixel_size: u32,
    manual_srgb: u32, // 1 when the swapchain isn't *Srgb, so the shader encodes
}

@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var normal_tex: texture_2d<f32>;
@group(0) @binding(2) var depth_tex: texture_depth_2d;
@group(0) @binding(3) var<uniform> post: Post;

const NORMAL_EDGE: f32 = 0.3;
const DEPTH_EDGE: f32 = 0.4;

@vertex
fn vs_post(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    // One triangle covering the screen.
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
}

// Neighbour loads clamp to the art size by hand: out-of-range textureLoad is undefined.
fn texel(p: vec2i, dx: i32, dy: i32) -> vec2i {
    return clamp(p + vec2i(dx, dy), vec2i(0), vec2i(post.art) - 1);
}

fn depth_at(p: vec2i, dx: i32, dy: i32) -> f32 {
    return textureLoad(depth_tex, texel(p, dx, dy), 0);
}

fn normal_at(p: vec2i, dx: i32, dy: i32) -> vec3f {
    return textureLoad(normal_tex, texel(p, dx, dy), 0).rgb * 2.0 - 1.0;
}

fn depth_edge(p: vec2i, depth: f32) -> f32 {
    var diff = 0.0;
    diff += clamp(depth_at(p, 1, 0) - depth, 0.0, 1.0);
    diff += clamp(depth_at(p, -1, 0) - depth, 0.0, 1.0);
    diff += clamp(depth_at(p, 0, 1) - depth, 0.0, 1.0);
    diff += clamp(depth_at(p, 0, -1) - depth, 0.0, 1.0);
    return floor(smoothstep(0.01, 0.02, diff) * 2.0) / 2.0;
}

fn neighbour_normal_edge(p: vec2i, dx: i32, dy: i32, depth: f32, normal: vec3f) -> f32 {
    let depth_diff = depth_at(p, dx, dy) - depth;
    let neighbour = normal_at(p, dx, dy);
    // Edge pixels yield to faces whose normals are closer to the bias normal.
    let normal_diff = dot(normal - neighbour, vec3f(1.0));
    let normal_indicator = clamp(smoothstep(-0.01, 0.01, normal_diff), 0.0, 1.0);
    // Only the shallower pixel draws the normal edge.
    let depth_indicator = clamp(sign(depth_diff * 0.25 + 0.0025), 0.0, 1.0);
    return (1.0 - dot(normal, neighbour)) * depth_indicator * normal_indicator;
}

fn normal_edge(p: vec2i, depth: f32, normal: vec3f) -> f32 {
    var sum = 0.0;
    sum += neighbour_normal_edge(p, 0, -1, depth, normal);
    sum += neighbour_normal_edge(p, 0, 1, depth, normal);
    sum += neighbour_normal_edge(p, -1, 0, depth, normal);
    sum += neighbour_normal_edge(p, 1, 0, depth, normal);
    return step(0.1, sum);
}

fn srgb_encode(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs_post(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    let p = min(vec2i(floor(frag.xy / f32(post.pixel_size))), vec2i(post.art) - 1);
    let color = textureLoad(color_tex, p, 0).rgb;
    let depth = depth_at(p, 0, 0);
    let normal = normal_at(p, 0, 0);
    let dei = depth_edge(p, depth);
    let nei = normal_edge(p, depth, normal);
    let strength = select(1.0 + NORMAL_EDGE * nei, 1.0 - DEPTH_EDGE * dei, dei > 0.0);
    var out = clamp(color * strength, vec3f(0.0), vec3f(1.0));
    if post.manual_srgb == 1u {
        out = srgb_encode(out);
    }
    return vec4f(out, 1.0);
}
