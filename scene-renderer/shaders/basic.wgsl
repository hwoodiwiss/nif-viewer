// Stock material "basic": diffuse texture + tangent-space normal map,
// Lambert lighting from the directional light plus ambient, Reinhard tone map.
//
// Slots: 0 diffuse (sRGB, fallback white), 1 normal (linear, fallback flat).
// Uniform (binding 4): base_color_factor (linear RGBA multiplier),
// flags.x = 1.0 to multiply by vertex colour.

struct BasicMaterial {
    base_color_factor: vec4<f32>,
    flags: vec4<f32>,
}

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;
@group(0) @binding(2)
var t_normal: texture_2d<f32>;
@group(0) @binding(3)
var s_normal: sampler;
@group(0) @binding(4)
var<uniform> material: BasicMaterial;

@vertex
fn vertex_main(model: VertexInput, instance: InstanceInput) -> VertexOutput {
    return standard_vertex(model, instance);
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var base = textureSample(t_diffuse, s_diffuse, in.tex_coords) * material.base_color_factor;
    if (material.flags.x > 0.5) {
        base = base * in.colour;
    }
    let n = decode_normal(textureSample(t_normal, s_normal, in.tex_coords));
    let l = normalize(in.tangent_light_dir);
    let radiance = light.colour * light.intensity;
    let direct = base.rgb / PI * radiance * max(dot(n, l), 0.0);
    let ambient = light.colour * light.ambient * base.rgb;
    return vec4<f32>(tone_map(direct + ambient), base.a);
}
