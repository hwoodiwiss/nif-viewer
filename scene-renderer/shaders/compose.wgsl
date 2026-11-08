// Compose pass (shader ABI 1): resamples the linear scene-colour target into
// the output target and applies output encoding. A full-screen triangle is
// generated from the vertex index; no vertex buffers are bound.
//
// Bind group 0: 0 scene colour, 1 sampler, 2 ComposeUniform.

struct ComposeUniform {
    // x: 1.0 = gamma-encode manually (non-sRGB output format).
    params: vec4<f32>,
}

@group(0) @binding(0)
var scene_colour: texture_2d<f32>;
@group(0) @binding(1)
var scene_sampler: sampler;
@group(0) @binding(2)
var<uniform> compose: ComposeUniform;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coord: vec2<f32>,
}

@vertex
fn vertex_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    var out: VertexOutput;
    out.position = vec4<f32>(x, y, 1.0, 1.0);
    out.tex_coord = vec2<f32>(x * 0.5 + 0.5, 0.5 - y * 0.5);
    return out;
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var colour = textureSample(scene_colour, scene_sampler, in.tex_coord);
    if (compose.params.x > 0.5) {
        colour = vec4<f32>(pow(colour.rgb, vec3<f32>(1.0 / 2.2)), colour.a);
    }
    return colour;
}
