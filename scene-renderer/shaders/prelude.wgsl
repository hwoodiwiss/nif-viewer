// Material prelude, shader ABI 2. Prepended by the renderer to every
// material type's WGSL source; material shaders must not redeclare these.
//
// Bind groups:
//   0: material — declared by the material type. Texture slot i binds its
//      texture at @binding(2i) and sampler at @binding(2i+1); a uniform block,
//      if declared, follows at @binding(2 * slot count).
//   1: frame    (FrameUniform)
//   2: light    (Light)
// Vertex buffers:
//   0: position(0) uv(1) normal(2) tangent+handedness(3) colour(4)
//   1: instance model columns(5-8), normal matrix columns(9-11), det sign(12)
// Entry points: material shaders define `vertex_main` and `fragment_main`.
// Output: one colour target (location 0) holding linear scene colour; the
// compose pass applies output encoding.

struct FrameUniform {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(1) @binding(0)
var<uniform> frame: FrameUniform;

struct Light {
    // Direction the light travels.
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
    @location(3) tangent: vec4<f32>,
    @location(4) colour: vec4<f32>,
}

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
    @location(9) normal_matrix_0: vec3<f32>,
    @location(10) normal_matrix_1: vec3<f32>,
    @location(11) normal_matrix_2: vec3<f32>,
    @location(12) det_sign: f32,
}

// Standard varyings: tangent-space light/view vectors for normal mapping.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) tangent_position: vec3<f32>,
    // Direction towards the light, in tangent space.
    @location(2) tangent_light_dir: vec3<f32>,
    @location(3) tangent_view_position: vec3<f32>,
    @location(4) colour: vec4<f32>,
}

// Transforms a vertex by its instance and builds the tangent basis. Mirrored
// transforms flip the basis handedness via det_sign.
fn standard_vertex(model: VertexInput, instance: InstanceInput) -> VertexOutput {
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
    let linear = mat3x3<f32>(
        model_matrix[0].xyz,
        model_matrix[1].xyz,
        model_matrix[2].xyz,
    );

    let world_normal = normalize(normal_matrix * model.normal);
    let world_tangent = normalize(linear * model.tangent.xyz);
    let world_bitangent = cross(world_normal, world_tangent) * model.tangent.w * instance.det_sign;
    let tangent_matrix = transpose(mat3x3<f32>(
        world_tangent,
        world_bitangent,
        world_normal,
    ));

    let world_position = model_matrix * vec4<f32>(model.position, 1.0);

    var out: VertexOutput;
    out.clip_position = frame.view_proj * world_position;
    out.tex_coords = model.tex_coord;
    out.tangent_position = tangent_matrix * world_position.xyz;
    out.tangent_view_position = tangent_matrix * frame.view_pos.xyz;
    out.tangent_light_dir = tangent_matrix * (-light.direction);
    out.colour = model.colour;
    return out;
}

const PI: f32 = 3.14159265359;

// Reinhard tone map.
fn tone_map(colour: vec3<f32>) -> vec3<f32> {
    return colour / (colour + vec3<f32>(1.0));
}

// Decode a tangent-space normal map texel (+Y, n * 0.5 + 0.5).
fn decode_normal(texel: vec4<f32>) -> vec3<f32> {
    return normalize(texel.xyz * 2.0 - 1.0);
}
