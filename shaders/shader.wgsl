// Geometry pass: PBR (Cook-Torrance GGX) lighting with a directional light.
// Writes WHITE to target0 and the full tone-mapped lit colour to target1;
// draw_deferred.wgsl multiplies the two targets, so target0=1 makes the
// compose a pass-through of target1 (minimal-churn fix for the old
// albedo-double-multiply bug).

struct Uniforms {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
    // x: 1.0 = apply manual gamma encode (non-srgb surface), 0.0 = srgb surface.
    params: vec4<f32>,
}
@group(1) @binding(0)
var<uniform> uniforms: Uniforms;

struct Light {
    direction: vec3<f32>,
    intensity: f32,
    colour: vec3<f32>,
    ambient: f32,
}
@group(2) @binding(0)
var<uniform> light: Light;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coord: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) tangent: vec3<f32>,
    @location(4) bitangent: vec3<f32>,
}

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
    @location(9) normal_matrix_0: vec3<f32>,
    @location(10) normal_matrix_1: vec3<f32>,
    @location(11) normal_matrix_2: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) tangent_position: vec3<f32>,
    @location(2) tangent_light_dir: vec3<f32>,
    @location(3) tangent_view_position: vec3<f32>,
}

@vertex
fn vertex_main(model: VertexInput, instance: InstanceInput) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    let normal_matrix = mat3x3<f32>(
        instance.normal_matrix_0,
        instance.normal_matrix_1,
        instance.normal_matrix_2,
    );

    var out: VertexOutput;

    let world_normal = normalize(normal_matrix * model.normal);
    let world_tangent = normalize(normal_matrix * model.tangent);
    let world_bitangent = normalize(normal_matrix * model.bitangent);
    let tangent_matrix = transpose(mat3x3<f32>(
        world_tangent,
        world_bitangent,
        world_normal,
    ));

    var world_position: vec4<f32> = model_matrix * vec4<f32>(model.position, 1.0);

    out.clip_position = uniforms.view_proj * world_position;
    out.tex_coords = model.tex_coord;
    out.tangent_position = tangent_matrix * world_position.xyz;
    out.tangent_view_position = tangent_matrix * uniforms.view_pos.xyz;
    // Direction TOWARDS the light (light.direction is the travel direction).
    out.tangent_light_dir = tangent_matrix * (-light.direction);
    return out;
}

// Fragment shader

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;

@group(0) @binding(2)
var t_normal: texture_2d<f32>;
@group(0) @binding(3)
var s_normal: sampler;

// ORM: R = ambient occlusion, G = roughness, B = metalness (linear).
@group(0) @binding(4)
var t_orm: texture_2d<f32>;
@group(0) @binding(5)
var s_orm: sampler;

struct FragmentOutput {
    @location(0) diffuse: vec4<f32>,
    @location(1) normal: vec4<f32>,
}

const PI: f32 = 3.14159265359;

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

// Reinhard tone map.
fn tone_map(colour: vec3<f32>) -> vec3<f32> {
    return colour / (colour + vec3<f32>(1.0));
}

@fragment
fn fragment_main(in: VertexOutput) -> FragmentOutput {
    var out: FragmentOutput;
    let albedo_sample: vec4<f32> = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    let object_normal: vec4<f32> = textureSample(t_normal, s_normal, in.tex_coords);
    let orm: vec4<f32> = textureSample(t_orm, s_orm, in.tex_coords);

    let albedo = albedo_sample.rgb;
    let ao = orm.r;
    let roughness = clamp(orm.g, 0.04, 1.0);
    let metalness = orm.b;

    let n = normalize(object_normal.xyz * 2.0 - 1.0);
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

    var colour = tone_map(direct + ambient);
    if (uniforms.params.x > 0.5) {
        // Non-srgb surface: gamma-encode manually.
        colour = pow(colour, vec3<f32>(1.0 / 2.2));
    }

    // target0 = white so draw_deferred's multiply passes target1 through.
    out.diffuse = vec4<f32>(1.0, 1.0, 1.0, albedo_sample.a);
    out.normal = vec4<f32>(colour, 1.0);
    return out;
}
