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
    params: vec4f,      // x: time (s), y: post clock face glow, z: cloud cover (0-1), w: the wind's flutter
    fog: vec4f,         // rgb: ground fog colour, a: amount (0: none)
    clouds: vec4f,      // xy: the big clouds' drift, zw: the wisps' (world units)
    wind: vec4f,        // xy: where the grass leans, z: the gusts' phase (s), w: their strength
    rain: vec4f,        // x: how hard it's raining, y: how wet the meadow is (0-1)
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
    @location(7) base: vec4f,  // xyz: tree base (sway pivot), w: sway phase, < 0 for no sway (-2: in the grass wind, scene.wgsl)
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
    let t = g.wind.z;
    let k = g.wind.w;
    let sway = rot_x(0.02 * k * sin(t * 0.9 + p * 1.7)) * rot_z(0.035 * k * sin(t * 1.3 + p));
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

// Integer hash (PCG 3D): three well-mixed u32s from a lattice point.
fn pcg3(c: vec3i) -> vec3u {
    var v = bitcast<vec3u>(c) * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v ^= v >> vec3u(16u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return v;
}

fn lattice_grad(c: vec3f, f: vec3f) -> f32 {
    let h = pcg3(vec3i(c));
    return dot(vec3f(h & vec3u(0xffffu)) / 32767.5 - 1.0, f);
}

// 3-D gradient noise, about -0.7 to 0.7: round blobs, no grid corners.
fn gradient_noise(p: vec3f) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let x00 = mix(lattice_grad(i, f), lattice_grad(i + vec3f(1.0, 0.0, 0.0), f - vec3f(1.0, 0.0, 0.0)), u.x);
    let x10 = mix(lattice_grad(i + vec3f(0.0, 1.0, 0.0), f - vec3f(0.0, 1.0, 0.0)), lattice_grad(i + vec3f(1.0, 1.0, 0.0), f - vec3f(1.0, 1.0, 0.0)), u.x);
    let x01 = mix(lattice_grad(i + vec3f(0.0, 0.0, 1.0), f - vec3f(0.0, 0.0, 1.0)), lattice_grad(i + vec3f(1.0, 0.0, 1.0), f - vec3f(1.0, 0.0, 1.0)), u.x);
    let x11 = mix(lattice_grad(i + vec3f(0.0, 1.0, 1.0), f - vec3f(0.0, 1.0, 1.0)), lattice_grad(i + vec3f(1.0), f - vec3f(1.0)), u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// Octaves of gradient noise, normalised: lumpy cloud edges. Its spread (sd) is about 0.13.
fn cloud_fbm(p: vec3f, octaves: i32) -> f32 {
    var sum = 0.0;
    var weight = 0.0;
    var amp = 0.5;
    var q = p;
    for (var k = 0; k < octaves; k++) {
        sum += amp * gradient_noise(q);
        weight += amp;
        q = q * 2.03 + 17.0;
        amp *= 0.5;
    }
    return sum / weight;
}

// Cloud shadows (grown from the web's sky.js): puffy clouds that drift with the upper wind (the
// CPU's Clouds), slowly change shape and come and go with the weather (cover), and faster thin
// wisps above. A low sun throws the tall clouds' shadows longer, away from it. They only dim
// direct light.
fn cloud_shade(world: vec3f) -> f32 {
    let cover = g.params.z;
    if cover <= 0.0 {
        return 1.0;
    }
    let morph = g.params.x / 300.0; // a cloud's shape turns over in a few minutes
    let flat = g.sun_dir.xz;
    let reach = 1.5 * min(length(flat) / max(g.sun_dir.y, 0.05), 3.0); // world units
    let away = -flat / max(length(flat), 1e-4) * reach;
    let p = world.xz - g.clouds.xy;
    // The cloud's base and its top, one reach further on: together a longer shadow.
    let big = max(cloud_fbm(vec3f(p * 0.08, morph), 3), cloud_fbm(vec3f((p - away) * 0.08, morph), 3));
    // The wisps streak along their wind (clouds.rs: HEADING + SHEAR, about 0.83 rad from +x).
    let w = world.xz - g.clouds.zw;
    let along = vec2f(0.67, 0.74);
    let streak = vec2f(dot(w, along) * 0.1, dot(w, vec2f(-along.y, along.x)) * 0.4);
    let wisps = cloud_fbm(vec3f(streak, morph * 1.7 + 50.0), 2);
    // Thresholds from the noise's spread: cover 0 is a clear sky, 1 about half of it in cloud.
    let edge = 0.3 - 0.3 * cover;
    let thick = smoothstep(edge, edge + 0.008, big);
    let thin = 0.5 * smoothstep(edge + 0.04, edge + 0.048, wisps);
    return mix(1.0, mix(0.55, 0.35, cover), max(thick, thin));
}

// Low fog by absolute height, thinning upward: hollows and the pond fill first, the crowns get a
// light haze; drifting patches.
fn ground_fog(world: vec3f) -> f32 {
    let t = g.params.x;
    let layer = exp(-0.5 * max(world.y, 0.0));
    let p = world.xz * 0.18 + vec2f(t * 0.04, t * 0.015);
    let n = value_noise(p + 40.0) * 0.65 + value_noise(p * 2.3 + 7.0) * 0.35;
    return g.fog.a * layer * mix(0.45, 1.0, n) * 0.5;
}
