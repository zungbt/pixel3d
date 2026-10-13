// Lit scene at art resolution: colour plus view-space normals for the outline pass.

@group(1) @binding(0) var shadow_map: texture_depth_2d;
@group(1) @binding(1) var shadow_sampler: sampler_comparison;

struct VertexOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) local_xz: vec2f, // mesh-local x/z: the pond fans' coordinates
    @location(3) extra: f32,
    @location(4) color: vec3f,
}

// The grass's travelling gust for something rooted at base: the tip's offset at BLADE_H
// (meadow_core::wind::Wind::sway, which the perched insects follow).
fn gust(base: vec2f) -> vec3f {
    let lean = g.wind.xy;
    let t = g.wind.z;
    let across = dot(base, vec2f(-lean.y, lean.x)) * 0.45 * 1.4142135;
    var sway = sin(t * 1.6 - across) * 0.5 + 0.5;
    sway = sway * 0.12 * g.wind.w + sin(t * 3.1 + base.x * 2.0) * 0.02 * g.params.w;
    return vec3f(lean.x, 0.0, lean.y) * sway;
}

// Instance base.w for meshes in the grass's wind (the flowers): unrotated, unit scale, keyed on
// their translation, sheared by mesh-local height. Normals stay put, as the web's.
const IN_WIND: f32 = -2.0;

@vertex
fn vs_scene(v: VertexIn) -> VertexOut {
    let m = model_matrix(v);
    var world = m * vec4f(v.pos, 1.0);
    if v.base.w == IN_WIND {
        world += vec4f(gust(m[3].xz) * (v.pos.y / BLADE_H), 0.0);
    }
    var out: VertexOut;
    out.clip = g.view_proj * world;
    out.world = world.xyz;
    out.normal = normal_matrix(m) * v.normal;
    out.local_xz = v.pos.xz;
    out.extra = v.extra;
    out.color = v.color.rgb;
    return out;
}

struct SceneOut {
    @location(0) color: vec4f,
    @location(1) normal: vec4f,
}

const PI: f32 = 3.14159265;
const TOON: array<f32, 4> = array<f32, 4>(60.0 / 255.0, 125.0 / 255.0, 195.0 / 255.0, 1.0);
// Ground band edges blend over this much of a band (in N·L/2 units × 4), so gentle hills read
// as soft tone patches rather than hard stripes.
const GROUND_SOFT: f32 = 0.3;

// 4-step toon ramp on N·L (as the web's gradient map: 60/125/195/255).
fn toon(n_dot_l: f32) -> f32 {
    let i = clamp(i32(floor((n_dot_l * 0.5 + 0.5) * 4.0)), 0, 3);
    return TOON[i];
}

// Same steps with soft edges: each band blends into the next over GROUND_SOFT.
fn toon_soft(n_dot_l: f32) -> f32 {
    let s = clamp((n_dot_l * 0.5 + 0.5) * 4.0, 0.0, 3.999);
    let i = i32(floor(s));
    let f = s - floor(s);
    let w = GROUND_SOFT + fwidth(s);
    let next = TOON[min(i + 1, 3)];
    return mix(TOON[i], next, smoothstep(1.0 - w, 1.0, f));
}

// 0 in shadow, 1 lit; 5 bilinear comparison taps. Off the shadow map counts as lit.
fn shadow(world: vec3f, normal: vec3f) -> f32 {
    let p = g.light_view_proj * vec4f(world + normal * 0.02, 1.0);
    let uv = p.xy * vec2f(0.5, -0.5) + 0.5;
    let depth = p.z - 0.0005;
    let texel = 1.0 / 2048.0;
    var lit = textureSampleCompareLevel(shadow_map, shadow_sampler, uv, depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2f(texel, 0.0), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv - vec2f(texel, 0.0), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2f(0.0, texel), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv - vec2f(0.0, texel), depth);
    let inside = all(uv >= vec2f(0.0)) && all(uv <= vec2f(1.0)) && depth <= 1.0;
    return select(1.0, lit / 5.0, inside);
}

// Sun (or moon) through the toon ramp, shadow and cloud shade, plus the sky/ground hemisphere light
// (Lambert, /π); then the ground fog over it, as the web's patched toon material.
fn lighting(diffuse: vec3f, normal: vec3f, ramp: f32, lit: f32, world: vec3f) -> vec3f {
    return glowing(diffuse, normal, ramp, lit, world, vec3f(0.0));
}

// The same with an emissive glow added before the fog.
fn glowing(diffuse: vec3f, normal: vec3f, ramp: f32, lit: f32, world: vec3f, glow: vec3f) -> vec3f {
    let hemi = mix(g.hemi_ground.rgb, g.hemi_sky.rgb, normal.y * 0.5 + 0.5);
    let color = diffuse / PI * (g.sun_color.rgb * ramp * lit * cloud_shade(world) + hemi) + glow;
    return mix(color, g.fog.rgb, ground_fog(world));
}

fn view_normal(n: vec3f) -> vec4f {
    return vec4f(normalize((g.view * vec4f(n, 0.0)).xyz) * 0.5 + 0.5, 1.0);
}

@fragment
fn fs_toon(in: VertexOut) -> SceneOut {
    let n = normalize(in.normal);
    let lit = shadow(in.world, n);
    let color = lighting(in.color, n, toon(dot(n, g.sun_dir.xyz)), lit, in.world);
    return SceneOut(vec4f(color, 1.0), view_normal(n));
}

// Insects: two-sided (a wing's back face is lit with the normal flipped) and, as the web's, not
// shadowed.
@fragment
fn fs_insect(in: VertexOut, @builtin(front_facing) front: bool) -> SceneOut {
    let n = normalize(in.normal) * select(-1.0, 1.0, front);
    let color = lighting(in.color, n, toon(dot(n, g.sun_dir.xyz)), 1.0, in.world);
    return SceneOut(vec4f(color, 1.0), view_normal(n));
}

// The post clock's face: glowing a warm cream (params.y) and unshadowed, as the web's: at noon the
// sun grazes the upright face, and the shadow map can't tell the dial from the rim just behind it.
@fragment
fn fs_dial(in: VertexOut) -> SceneOut {
    let n = normalize(in.normal);
    let color = glowing(in.color, n, toon(dot(n, g.sun_dir.xyz)), 1.0, in.world, DIAL_GLOW * g.params.y);
    return SceneOut(vec4f(color, 1.0), view_normal(n));
}

// Unlit, as the web's MeshBasicMaterial: the gnats, and the fireflies (whose pipeline writes no
// depth or normals, so they never get outlines).
@fragment
fn fs_unlit(in: VertexOut) -> SceneOut {
    return SceneOut(vec4f(in.color, 1.0), view_normal(normalize(in.normal)));
}

// Path constants: meadow_core::world::path (filled in by render.rs).
fn on_path(x: f32, z: f32) -> bool {
    let a = z * PATH_FREQ + PATH_PHASE;
    let slope = PATH_AMP * PATH_FREQ * cos(a);
    let dist = abs(x - (PATH_X0 + PATH_AMP * sin(a))) / sqrt(1.0 + slope * slope);
    let half_w = PATH_W_BASE + PATH_W_A1 * sin(z * PATH_W_F1) + PATH_W_A2 * sin(z * PATH_W_F2 + x);
    if dist < half_w {
        return true;
    }
    let bx = x - BRANCH_X0;
    if bx <= 0.0 {
        return false;
    }
    let k = exp(-bx / BRANCH_A);
    let b_dist = abs(z - (BRANCH_Z0 + BRANCH_A * (1.0 - k))) / sqrt(1.0 + k * k);
    let b_half_w = (PATH_W_BASE + PATH_W_A1 * sin(x * PATH_W_F1) + PATH_W_A2 * sin(x * PATH_W_F2 + z))
        * BRANCH_WIDTH * (1.0 - smoothstep(BRANCH_X1 - BRANCH_TAPER, BRANCH_X1, x));
    return b_dist < b_half_w;
}

@fragment
fn fs_ground(in: VertexOut) -> SceneOut {
    let n = normalize(in.normal);
    var base = in.color;
    if on_path(in.world.x, in.world.z) {
        base = PATH_COLOR;
    }
    let lit = shadow(in.world, n);
    let color = lighting(base * in.extra, n, toon_soft(dot(n, g.sun_dir.xyz)), lit, in.world);
    return SceneOut(vec4f(color, 1.0), view_normal(n));
}

// Pond: stepped contour bands, a dark far bank (fakes water sitting below the ground), broken
// foam along the rim and twinkling glints (as the web's water material).
@fragment
fn fs_water(in: VertexOut) -> SceneOut {
    let t = g.params.x;
    let p = in.local_xz;
    let edge = in.extra;
    let e = edge + (value_noise(p * 1.3) - 0.5) * 0.12; // wobbly, not concentric
    var water = select(select(WATER_SHALLOW, WATER_MID, e < 0.75), WATER_DEEP, e < 0.45);
    // Far side from the camera (world -x,-z): the inner bank wall shows as a dark band.
    let far = dot(p, vec2f(-0.7071)) / max(length(p), 1e-4);
    let bank = smoothstep(0.2, 1.0, far) * 0.16;
    let is_bank = edge > 1.0 - bank;
    let foam_edge = 0.95 - value_noise(p * 4.0 + vec2f(t * 0.3, 0.0)) * 0.1;
    let is_foam = !is_bank && (edge > foam_edge || (edge > 0.8 && noise_hash(floor(p * 12.0)) > 0.97));
    // Glints: short dashes along the screen horizontal (world x - z), drifting slowly.
    let sp = vec2f((p.x - p.y) * 0.7071 + t * 0.08, (p.x + p.y) * 0.7071);
    let h = noise_hash(floor(sp * vec2f(4.0, 12.0)));
    let is_glint = edge < 0.85 && h > 0.97 && sin(t * 2.0 + h * 60.0) > 0.5;
    if is_bank {
        water = WATER_BANK;
    }
    if is_foam || is_glint {
        water = WATER_FOAM;
    }
    let n = vec3f(0.0, 1.0, 0.0);
    let lit = shadow(in.world, n);
    let color = lighting(g.water_tint.rgb * water, n, toon(dot(n, g.sun_dir.xyz)), lit, in.world);
    return SceneOut(vec4f(color, 1.0), view_normal(n));
}

// Grass: one triangle per blade, built from the vertex index (no vertex buffer). BLADE_W and
// BLADE_H: filled in by render.rs.
struct GrassIn {
    @builtin(vertex_index) corner: u32,
    @location(0) pos: vec3f,         // base, on the ground
    @location(1) color_scale: vec4f, // linear rgb, (scale - 0.7) / 0.6
    @location(2) normal: vec4f,      // the ground's normal at the base
}

struct GrassOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) color: vec3f,
    @location(2) ground_normal: vec3f,
}

@vertex
fn vs_grass(v: GrassIn) -> GrassOut {
    // Corners (±w/2, 0) and (0, h) in the blade's plane, turned to face the camera (yaw π/4).
    var local = vec3f(0.0, BLADE_H, 0.0);
    if v.corner < 2u {
        let x = select(-0.5, 0.5, v.corner == 1u) * BLADE_W;
        local = vec3f(x * 0.7071, 0.0, -x * 0.7071);
    }
    // Travelling gust keyed on the base, tip only, in local space before the scale (as the web).
    let tip = local.y / BLADE_H;
    local += gust(v.pos.xz) * tip;
    let world = v.pos + local * (0.7 + 0.6 * v.color_scale.a);
    var out: GrassOut;
    out.clip = g.view_proj * vec4f(world, 1.0);
    out.world = world;
    out.color = v.color_scale.rgb * mix(0.85, 1.12, tip); // darker at the root (linear in tip: exact per vertex)
    out.ground_normal = v.normal.xyz;
    return out;
}

// Lit as flat ground (normal up, as the web), but outlined with the ground's own normal, so a blade
// on a slope doesn't read as an edge against the ground behind it.
@fragment
fn fs_grass(in: GrassOut) -> SceneOut {
    let up = vec3f(0.0, 1.0, 0.0);
    let lit = shadow(in.world, up);
    let color = lighting(in.color, up, toon(dot(up, g.sun_dir.xyz)), lit, in.world);
    return SceneOut(vec4f(color, 1.0), view_normal(in.ground_normal));
}
