// Shared by the shadow and scene shaders (prepended in render.rs).

struct Globals {
    view_proj: mat4x4f,
    view: mat4x4f,
    light_view_proj: mat4x4f,
    sun_dir: vec4f,     // xyz: toward the sun
    sun_color: vec4f,   // rgb × intensity
    hemi_sky: vec4f,    // rgb × intensity
    hemi_ground: vec4f, // rgb × intensity
    water_tint: vec4f,  // white with a hint of the sky
    params: vec4f,      // x: time (s)
}

@group(0) @binding(0) var<uniform> g: Globals;

struct VertexIn {
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
    @location(2) extra: f32,
    @location(3) m0: vec4f,
    @location(4) m1: vec4f,
    @location(5) m2: vec4f,
    @location(6) m3: vec4f,
    @location(7) base: vec4f,  // xyz: tree base (sway pivot), w: sway phase, < 0 for no sway
    @location(8) color: vec4f, // linear rgb
}

fn rot_x(a: f32) -> mat4x4f {
    let c = cos(a);
    let s = sin(a);
    return mat4x4f(vec4f(1.0, 0.0, 0.0, 0.0), vec4f(0.0, c, s, 0.0), vec4f(0.0, -s, c, 0.0), vec4f(0.0, 0.0, 0.0, 1.0));
}

fn rot_z(a: f32) -> mat4x4f {
    let c = cos(a);
    let s = sin(a);
    return mat4x4f(vec4f(c, s, 0.0, 0.0), vec4f(-s, c, 0.0, 0.0), vec4f(0.0, 0.0, 1.0, 0.0), vec4f(0.0, 0.0, 0.0, 1.0));
}

fn translate(t: vec3f) -> mat4x4f {
    return mat4x4f(vec4f(1.0, 0.0, 0.0, 0.0), vec4f(0.0, 1.0, 0.0, 0.0), vec4f(0.0, 0.0, 1.0, 0.0), vec4f(t, 1.0));
}

// Instance matrix, with trees swaying about their base (as the web's world.update).
fn model_matrix(v: VertexIn) -> mat4x4f {
    let m = mat4x4f(v.m0, v.m1, v.m2, v.m3);
    let p = v.base.w;
    if p < 0.0 {
        return m;
    }
    let t = g.params.x;
    let sway = rot_x(0.02 * sin(t * 0.9 + p * 1.7)) * rot_z(0.035 * sin(t * 1.3 + p));
    return translate(v.base.xyz) * sway * translate(-v.base.xyz) * m;
}

// Normal matrix as the cofactor of the upper 3×3: right for non-uniform scale, no inverse.
fn normal_matrix(m: mat4x4f) -> mat3x3f {
    let c0 = m[0].xyz;
    let c1 = m[1].xyz;
    let c2 = m[2].xyz;
    return mat3x3f(cross(c1, c2), cross(c2, c0), cross(c0, c1));
}

fn noise_hash(p: vec2f) -> f32 {
    return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453);
}

fn value_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(noise_hash(i), noise_hash(i + vec2f(1.0, 0.0)), u.x),
        mix(noise_hash(i + vec2f(0.0, 1.0)), noise_hash(i + vec2f(1.0, 1.0)), u.x),
        u.y,
    );
}
