struct Uniforms {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

struct Light {
    position: vec3<f32>,
    colour: vec3<f32>,
}
@group(1) @binding(0)
var<uniform> light: Light;

struct VertexInput {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(1) colour: vec3<f32>,
}

@vertex
fn vertex_main(
    model: VertexInput,
) -> VertexOutput {
    let scale = 0.25;
    var out: VertexOutput;
    out.clip_position = uniforms.view_proj * vec4<f32>(model.position * scale + light.position, 1.0);
    out.colour = light.colour;
    return out;
}

struct FragmentOutput {
    @location(0) diffuse: vec4<f32>,
    @location(1) normal: vec4<f32>,
}

@fragment
fn fragment_main(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    out.diffuse = vec4<f32>(in.colour, 1.0);
    out.normal = vec4<f32>(1.0);
    return out;
}