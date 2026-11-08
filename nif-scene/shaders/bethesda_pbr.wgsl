// Material type "bethesda-pbr" (shader ABI 2): Bethesda-style metal/rough
// Cook-Torrance GGX with a directional light, packed ORM texture and
// Reinhard tone map. Requires the renderer prelude.
//
// Slots: 0 diffuse (sRGB, fallback white), 1 normal (linear, fallback flat),
// 2 ORM (linear: R=AO, G=roughness, B=metalness; fallback 255,200,0).
// Uniform (binding 6): base_color_factor, flags.x = 1.0 to multiply by
// vertex colour.

struct MaterialUniform {
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
var t_orm: texture_2d<f32>;
@group(0) @binding(5)
var s_orm: sampler;
@group(0) @binding(6)
var<uniform> material: MaterialUniform;

@vertex
fn vertex_main(model: VertexInput, instance: InstanceInput) -> VertexOutput {
    return standard_vertex(model, instance);
}

// Trowbridge-Reitz GGX normal distribution.
fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / max(PI * d * d, 1e-6);
}

// Smith-Schlick geometry term.
fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    let g_v = n_dot_v / (n_dot_v * (1.0 - k) + k);
    let g_l = n_dot_l / (n_dot_l * (1.0 - k) + k);
    return g_v * g_l;
}

// Schlick Fresnel.
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo_sample = textureSample(t_diffuse, s_diffuse, in.tex_coords) * material.base_color_factor;
    if (material.flags.x > 0.5) {
        albedo_sample = albedo_sample * in.colour;
    }
    let object_normal = textureSample(t_normal, s_normal, in.tex_coords);
    let orm = textureSample(t_orm, s_orm, in.tex_coords);

    let albedo = albedo_sample.rgb;
    let ao = orm.r;
    let roughness = clamp(orm.g, 0.04, 1.0);
    let metalness = orm.b;

    let n = decode_normal(object_normal);
    let l = normalize(in.tangent_light_dir);
    let v = normalize(in.tangent_view_position - in.tangent_position);
    let h = normalize(v + l);

    let n_dot_l = max(dot(n, l), 0.0);
    let n_dot_v = max(dot(n, v), 1e-4);
    let n_dot_h = max(dot(n, h), 0.0);
    let h_dot_v = max(dot(h, v), 0.0);

    let f0 = mix(vec3<f32>(0.04), albedo, metalness);

    let d = distribution_ggx(n_dot_h, roughness);
    let g = geometry_smith(n_dot_v, n_dot_l, roughness);
    let f = fresnel_schlick(h_dot_v, f0);

    let specular = (d * g * f) / max(4.0 * n_dot_v * n_dot_l, 1e-4);

    // Lambert diffuse, energy-conserved against specular and metalness.
    let k_d = (vec3<f32>(1.0) - f) * (1.0 - metalness);
    let radiance = light.colour * light.intensity;
    let direct = (k_d * albedo / PI + specular) * radiance * n_dot_l;

    let ambient = light.colour * light.ambient * albedo * ao;

    return vec4<f32>(tone_map(direct + ambient), albedo_sample.a);
}
