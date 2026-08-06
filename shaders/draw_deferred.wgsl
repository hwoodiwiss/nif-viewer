struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coord: vec2<f32>,
}


struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(1) tex_coord: vec2<f32>,
}

@vertex
fn vertex_main(in: VertexInput) -> VertexOutput {
	var out: VertexOutput;
	out.position = vec4<f32>(in.position.x, in.position.y, 1.0, 1.0);
	out.tex_coord = in.tex_coord;
	return out;
}

@group(0) @binding(0)
var ss_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var ss_diffuse_sampler: sampler;

@group(0) @binding(2)
var ss_normal: texture_2d<f32>;
@group(0) @binding(3)
var ss_normal_sampler: sampler;

@fragment
fn fragment_main(in: VertexOutput) -> @location(0) vec4<f32> {
	let object_colour: vec4<f32> = textureSample(ss_diffuse, ss_diffuse_sampler, in.tex_coord);
	let object_specular: vec4<f32> = textureSample(ss_normal, ss_normal_sampler, in.tex_coord);
	return (object_colour * object_specular);
}