
struct ModelVertex {
	x: f32, y: f32, z: f32,
	uv: f32, uw: f32,
	nx: f32, ny: f32, nz: f32,
	tx: f32, ty: f32, tz: f32,
	bx: f32, by: f32, bz: f32,
	pad0_: u32, pad1_: u32,
}

struct ComputeInfo {
	num_vertices: u32,
	num_indicies: u32,
}

struct ModelVetexBuffer {
	verts: array<ModelVertex>
}

struct IndexBuffer {
	indicies: array<u32>
}

@group(0) @binding(0)
var<storage> src_verts: ModelVetexBuffer;

@group(0) @binding(1)
var<storage, read_write> dst_verts: ModelVetexBuffer;

@group(0) @binding(2)
var<storage> idx_buffer: IndexBuffer;

@group(0) @binding(3)
var<uniform> info: ComputeInfo;

fn getPos(vert: ModelVertex) -> vec3<f32> {
	return vec3<f32> (
		vert.x,
		vert.y,
		vert.z
	);
}

fn getUV(vert: ModelVertex) -> vec2<f32> {
	return vec2<f32> (
		vert.uv,
		vert.uw,
	);
}

fn calcTangentBitangent(vert_idx: u32) -> ModelVertex {
	var curr_vert = src_verts.verts[vert_idx];

	var tangent: vec3<f32>;
	var bitangent: vec3<f32>;
	var tris_included: f32;

	for(var i: u32 = 0u; i < info.num_indicies; i = i + 3u) {
		let idx0 = idx_buffer.indicies[i];
		let idx1 = idx_buffer.indicies[i + 1u];
		let idx2 = idx_buffer.indicies[i + 2u];

		if(idx0 == vert_idx || idx1 == vert_idx || idx2 == vert_idx) {
			let vert0 = src_verts.verts[idx0];
			let vert1 = src_verts.verts[idx1];
			let vert2 = src_verts.verts[idx2];

			let pos0 = getPos(vert0);
			let pos1 = getPos(vert1);
			let pos2 = getPos(vert2);

			let tex0 = getUV(vert0);
			let tex1 = getUV(vert1);
			let tex2 = getUV(vert2);

			let delta_pos1 = pos1 - pos0;
			let delta_pos2 = pos2 - pos0;

			let delta_uv1 = tex1 - tex0;
			let delta_uv2 = tex2 - tex0;

			let r: f32 = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
			tangent = tangent + (delta_pos1 * delta_uv2.y - delta_pos2 * delta_uv1.y) * r;
            bitangent = bitangent + (delta_pos2 * delta_uv1.x - delta_pos1 * delta_uv2.x) * r; 
            tris_included = tris_included + 1.0;
		}
	}

	if(tris_included > 0.0) {
		tangent = tangent / tris_included;
        bitangent = bitangent / tris_included;
        tangent = normalize(tangent);
        bitangent = normalize(bitangent);
	}

	curr_vert.tx = tangent.x;
	curr_vert.ty = tangent.y;
	curr_vert.tz = tangent.z;

	curr_vert.bx = bitangent.x;
    curr_vert.by = bitangent.y;
    curr_vert.bz = bitangent.z;

	return curr_vert;
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) global_ix: vec3<u32>) {
	let vert_idx = global_ix.x;
	let result = calcTangentBitangent(vert_idx);
	dst_verts.verts[vert_idx] = result;
}

