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
    clouds: vec4f,      // xy: the clouds' drift (world units), zw: unused
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

// Cloud shadows: separate cumulus some 600 m up (a world unit is about 2.5 m) that drift with the
// upper wind (the CPU's Clouds), each a few overlapping puffs (a lobed outline), 40-120 m across
// and many small, forming and melting away over 10-30 minutes; with more cover they swell and
// merge. Their soft edges (the sun's disc from that high) in pixel-art steps over some 4 m. They
// only dim direct light.
fn cloud_shade(world: vec3f) -> f32 {
    let cover = g.params.z;
    if cover <= 0.0 {
        return 1.0;
    }
    let t = g.params.x;
    let flat = g.sun_dir.xz;
    let reach = 240.0 * min(length(flat) / max(g.sun_dir.y, 0.05), 3.0); // world units
    let p = world.xz - g.clouds.xy + flat / max(length(flat), 1e-4) * reach;
    let m = t / 1200.0;
    var s = max(cloud_cells(p, t, 42.0, 0.6, 1.0, 0), cloud_cells(p, t, 18.0, 0.35, 0.3, 1));
    s += 3.6 * gradient_noise(vec3f(p * 0.1167, m)) + 1.5 * gradient_noise(vec3f(p.x * 0.3 + 3.0, p.y * 0.3, m));
    // Swollen with the cover so about that share of the ground is shaded (fitted to the field's spread).
    let c = max(cover, 0.01);
    s += 3.0 * (1.252 + 1.588 * c + 1.269 * log(c) + 1.623 * c * c * c * c);
    let dim = mix(0.55, 0.35, cover);
    return 1.0 - (1.0 - dim) * (0.35 * step(0.0, s) + 0.35 * step(0.8, s) + 0.3 * step(1.6, s));
}

// How far inside the nearest cloud of a layer `p` is (world units, negative outside): one cloud
// to a `size` cell (in a `share` of them), up to `reach` of the cell across.
fn cloud_cells(p: vec2f, t: f32, size: f32, reach: f32, share: f32, layer: i32) -> f32 {
    let base = vec2i(floor(p / size));
    var s = -1e3;
    for (var dx = -1; dx <= 1; dx++) {
        for (var dz = -1; dz <= 1; dz++) {
            let c = base + vec2i(dx, dz);
            let h = pcg3(vec3i(c, layer));
            let a = vec3f(h & vec3u(0xffffu)) / 65535.0;
            let b = vec3f(h >> vec3u(16u)) / 65535.0;
            if b.z >= share {
                continue;
            }
            let life = 600.0 + 1200.0 * a.z; // s: it forms, lingers and melts
            let grow = sqrt(sin(3.14159265 * fract(t / life + b.y * 7.0)));
            let r = size * reach * (0.3 + 0.7 * b.x * b.x) * grow;
            let centre = (vec2f(c) + 0.15 + 0.7 * a.xy) * size;
            let q = pcg3(vec3i(c, layer + 8));
            let lo = vec3f(q & vec3u(0xffffu)) / 65535.0;
            let hi = vec3f(q >> vec3u(16u)) / 65535.0;
            for (var j = 0; j < 3; j++) {
                let u = select(select(vec2f(hi.y, hi.z), vec2f(lo.z, hi.x), j == 1), lo.xy, j == 0);
                let k = fract((u.x + u.y) * 7.31);
                s = max(s, r * (0.55 + 0.35 * k) - length(p - centre - (u - 0.5) * r * 1.1));
            }
        }
    }
    return s;
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
