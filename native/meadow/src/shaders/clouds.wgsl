// The cloud map: cloud_depth over a view's ground (Renderer's cloud_map), one fullscreen triangle.

struct CloudOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_clouds(@builtin(vertex_index) i: u32) -> CloudOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return CloudOut(vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

@fragment
fn fs_clouds(in: CloudOut) -> @location(0) vec4f {
    let xz = g.cloud_origin.xy + in.uv.x * g.cloud_origin.zw + in.uv.y * g.cloud_axis_v.xy;
    return vec4f(cloud_depth(xz), 0.0, 0.0, 1.0);
}
