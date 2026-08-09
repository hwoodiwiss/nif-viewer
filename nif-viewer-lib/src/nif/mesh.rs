//! Starfield external `.mesh` file parser (spec §4).

use log::debug;

use super::reader::{NifError, Reader, Result};

/// Parsed geometry from a Starfield `.mesh` file or an inline `BSMeshData` blob.
#[derive(Debug, Default, Clone)]
pub struct MeshData {
    pub indices: Vec<u32>,
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub uvs2: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub normals: Vec<[f32; 3]>,
    /// w = bitangent sign.
    pub tangents: Vec<[f32; 4]>,
}

/// Unpack unsigned-normalized 10:10:10:2 remapped from [0,1] to [-1,1].
pub fn unpack_x10y10z10w2(n: u32) -> [f32; 4] {
    [
        (n & 0x3FF) as f32 * (2.0 / 1023.0) - 1.0,
        (n & 0xFFC00) as f32 * (2.0 / 1_047_552.0) - 1.0,
        (n & 0x3FF0_0000) as f32 * (2.0 / 1_072_693_248.0) - 1.0,
        (n >> 30) as f32 * (2.0 / 3.0) - 1.0,
    ]
}

/// Parse a Starfield `.mesh` byte stream (also used for inline `BSMeshData`).
/// Consumes through cull data when present, but tolerates EOF after weights.
pub fn parse_mesh_reader(r: &mut Reader<'_>) -> Result<MeshData> {
    let version = r.u32()?;
    if version > 2 {
        return Err(NifError::Invalid(format!(".mesh version {} > 2", version)));
    }

    let indices_size = r.u32()? as usize;
    let mut indices = Vec::with_capacity(indices_size);
    for _ in 0..indices_size {
        indices.push(r.u16()? as u32);
    }

    let scale = r.f32()?;
    if scale.is_nan() || scale <= 0.0 {
        return Err(NifError::Invalid(format!(".mesh scale {} <= 0", scale)));
    }

    let weights_per_vertex = r.u32()?;
    let num_verts = r.u32()? as usize;
    if num_verts == 0 {
        return Err(NifError::Invalid(".mesh has 0 vertices".to_string()));
    }

    let mut positions = Vec::with_capacity(num_verts);
    for _ in 0..num_verts {
        let x = r.i16()?;
        let y = r.i16()?;
        let z = r.i16()?;
        positions.push([
            x as f32 / 32767.0 * scale,
            y as f32 / 32767.0 * scale,
            z as f32 / 32767.0 * scale,
        ]);
    }

    let num_uvs = r.u32()? as usize;
    let mut uvs = Vec::with_capacity(num_uvs);
    for _ in 0..num_uvs {
        uvs.push([r.f16()?, r.f16()?]);
    }

    let num_uvs2 = r.u32()? as usize;
    let mut uvs2 = Vec::with_capacity(num_uvs2);
    for _ in 0..num_uvs2 {
        uvs2.push([r.f16()?, r.f16()?]);
    }

    let num_colors = r.u32()? as usize;
    let mut colors = Vec::with_capacity(num_colors);
    for _ in 0..num_colors {
        // stored BGRA; shuffle 2,1,0,3
        let b = r.bytes(4)?;
        colors.push([
            b[2] as f32 / 255.0,
            b[1] as f32 / 255.0,
            b[0] as f32 / 255.0,
            b[3] as f32 / 255.0,
        ]);
    }

    let num_normals = r.u32()? as usize;
    let mut normals = Vec::with_capacity(num_normals);
    for _ in 0..num_normals {
        let v = unpack_x10y10z10w2(r.u32()?);
        normals.push([v[0], v[1], v[2]]);
    }

    let num_tangents = r.u32()? as usize;
    let mut tangents = Vec::with_capacity(num_tangents);
    for _ in 0..num_tangents {
        tangents.push(unpack_x10y10z10w2(r.u32()?));
    }

    let mesh = MeshData {
        indices,
        positions,
        uvs,
        uvs2,
        colors,
        normals,
        tangents,
    };

    // Everything past here is optional for rendering — tolerate EOF.
    if let Err(e) = skip_trailer(r, version, weights_per_vertex) {
        debug!(".mesh trailer not fully read (ok): {}", e);
    }

    Ok(mesh)
}

fn skip_trailer(r: &mut Reader<'_>, version: u32, _weights_per_vertex: u32) -> Result<()> {
    let num_weights = r.u32()? as usize;
    r.skip(num_weights * 4)?; // u16 bone + u16 weight

    if version >= 1 {
        let num_lods = r.u32()? as usize;
        for _ in 0..num_lods {
            let lod_indices = r.u32()? as usize;
            r.skip(lod_indices * 2)?;
        }
    }

    if r.is_eof() {
        return Ok(());
    }

    let num_meshlets = r.u32()? as usize;
    r.skip(num_meshlets * 16)?;

    let num_cull = r.u32()? as usize;
    // version < 2: NiBound (16) + 4x u8 cone + f32 apex = 24 B; version >= 2: BSBoundingBox = 24 B
    r.skip(num_cull * 24)?;
    Ok(())
}

/// Parse a standalone Starfield `.mesh` file.
pub fn parse_mesh(bytes: &[u8]) -> Result<MeshData> {
    let mut r = Reader::new(bytes);
    parse_mesh_reader(&mut r)
}
