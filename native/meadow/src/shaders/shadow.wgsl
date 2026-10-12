// Depth-only pass from the sun for the closed casters (trunks, crowns, rocks).

@vertex
fn vs_shadow(v: VertexIn) -> @builtin(position) vec4f {
    return g.light_view_proj * model_matrix(v) * vec4f(v.pos, 1.0);
}
