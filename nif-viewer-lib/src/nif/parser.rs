//! NIF 20.2.0.7 block/scene parser (spec §1–§3, §5–§6).

use std::convert::TryFrom;

use bitflags::bitflags;
use log::{debug, info, warn};

use super::mesh::{parse_mesh_reader, MeshData};
use super::reader::{NifError, Reader, Result};
use super::{Geometry, MaterialInfo, NifMeshInstance, NifScene};

const NIF_VERSION: u32 = 0x1402_0007;

#[derive(Debug)]
pub(super) struct Header {
    pub header_string: String,
    pub version: u32,
    pub endian: u8,
    pub user_version: u32,
    pub bs_version: u32,
    pub num_blocks: usize,
    pub block_types: Vec<String>,
    pub block_type_index: Vec<u16>,
    pub block_sizes: Vec<u32>,
    pub strings: Vec<String>,
}

fn parse_header(r: &mut Reader<'_>) -> Result<Header> {
    let magic = r.line()?;
    if !magic.starts_with("Gamebryo File Format") && !magic.starts_with("NetImmerse File Format") {
        return Err(NifError::InvalidMagic(magic));
    }
    let version = r.u32()?;
    if version != NIF_VERSION {
        return Err(NifError::UnsupportedVersion(version));
    }
    let endian = r.u8()?;
    let user_version = r.u32()?;
    let num_blocks = r.u32()? as usize;

    // BSStreamHeader
    let bs_version = r.u32()?;
    let _author = r.export_string()?;
    if bs_version > 130 {
        let _unknown_int = r.u32()?;
    }
    if bs_version < 131 {
        let _process_script = r.export_string()?;
    }
    let _export_script = r.export_string()?;
    if (103..170).contains(&bs_version) {
        let _max_filepath = r.export_string()?;
    }
    if bs_version >= 170 {
        let len = r.u8()? as usize;
        r.skip(len)?;
    }

    let num_block_types = r.u16()? as usize;
    let mut block_types = Vec::with_capacity(num_block_types);
    for _ in 0..num_block_types {
        block_types.push(r.sized_string()?);
    }
    let mut block_type_index = Vec::with_capacity(num_blocks);
    for _ in 0..num_blocks {
        block_type_index.push(r.u16()? & 0x7FFF);
    }
    let mut block_sizes = Vec::with_capacity(num_blocks);
    for _ in 0..num_blocks {
        block_sizes.push(r.u32()?);
    }
    let num_strings = r.u32()? as usize;
    let _max_string_len = r.u32()?;
    let mut strings = Vec::with_capacity(num_strings);
    for _ in 0..num_strings {
        strings.push(r.sized_string()?);
    }
    let num_groups = r.u32()? as usize;
    r.skip(num_groups * 4)?;

    Ok(Header {
        header_string: magic,
        version,
        endian,
        user_version,
        bs_version,
        num_blocks,
        block_types,
        block_type_index,
        block_sizes,
        strings,
    })
}

/// NiObjectNET shared fields.
#[derive(Debug, Clone, Default)]
pub(super) struct ObjectNet {
    pub name: String,
    pub extra_data: Vec<i32>,
    pub controller: i32,
}

/// NiAVObject shared fields.
#[derive(Debug, Clone)]
pub(super) struct AvObject {
    pub net: ObjectNet,
    pub flags: AvFlags,
    pub translation: [f32; 3],
    /// Row-major 3x3 rotation (rows of basis vectors).
    pub rotation: [[f32; 3]; 3],
    pub scale: f32,
    /// Legacy rendering properties (`NiMaterialProperty`,
    /// `NiTexturingProperty`, shader properties, etc). Only present for
    /// `bs_version <= 34` (Oblivion/Fallout 3/NV and non-Bethesda streams);
    /// later Bethesda streams attach shader/alpha properties directly on
    /// the geometry block instead.
    pub properties: Vec<i32>,
    pub collision: i32,
}

impl AvObject {
    pub fn name(&self) -> &str {
        &self.net.name
    }
}

/// A single BSGeometry LOD entry (for structure introspection).
#[derive(Debug, Clone)]
pub(super) struct GeoLod {
    pub indices_count: u32,
    pub num_verts: u32,
    pub flags: u32,
    /// Normalized external path; `None` for embedded LODs.
    pub path: Option<String>,
}

#[derive(Debug)]
pub(super) enum GeoSource {
    Embedded(MeshData),
    External {
        path: String,
        indices_count: u32,
        num_verts: u32,
    },
    None,
}

#[derive(Debug)]
pub(super) enum Block {
    Node {
        av: AvObject,
        children: Vec<i32>,
    },
    TriShape {
        av: AvObject,
        skin: i32,
        shader: i32,
        alpha: i32,
        vertex_desc: u64,
        num_triangles: u32,
        num_vertices: u32,
        mesh: MeshData,
    },
    TriStrips {
        av: AvObject,
        data: i32,
        skin: i32,
        shader: i32,
        alpha: i32,
    },
    TriStripsData {
        keep_flags: TriShapeFlags,
        compress_flags: u8,
        vertices: Vec<[f32; 3]>,
        normals: Vec<[f32; 3]>,
        vertex_colors: Vec<[f32; 4]>,
        uv_sets: Vec<Vec<[f32; 2]>>,
        /// One entry per strip; each entry is the list of point (vertex)
        /// indices making up that triangle strip.
        strips: Vec<Vec<u16>>,
    },
    Geometry {
        av: AvObject,
        skin: i32,
        shader: i32,
        alpha: i32,
        lods: Vec<GeoLod>,
        geo: GeoSource,
    },
    Shader {
        net: ObjectNet,
        texture_set: i32,
    },
    TextureSet {
        textures: Vec<String>,
    },
    Alpha {
        net: ObjectNet,
        flags: u16,
        threshold: u8,
    },
    IntExtra {
        name: String,
        value: u32,
    },
    Unknown,
}

bitflags! {
    #[derive(Debug, Clone)]
    pub(super) struct AvFlags: u32 {
        const VISIBLE = 0x1;
        const SELECTABLE = 0x2;
        const RENDERABLE = 0x4;
        const CAST_SHADOWS = 0x8;
        const RECEIVE_SHADOWS = 0x10;
        const BOUNDS_VALID = 0x20;
        const BOUNDS_AUTO_UPDATE = 0x40;
        const BOUNDS_AUTO_COMPUTE = 0x80;
        const BOUNDS_VISIBLE = 0x100;
        const BOUNDS_SELECTABLE = 0x200;
        const BOUNDS_RENDERABLE = 0x400;
    }
}

bitflags! {
    #[derive(Debug, Clone)]
    pub(super) struct TriShapeFlags: u8 {
        const HAS_VERTICES = 1;
        const HAS_NORMALS = 2;
        const HAS_BINORMALS = 3;
        const HAS_TANGENTS = 4;
        const HAS_UVS = 5;
        const HAS_COLORS = 8;
    }
}

fn string_idx(header: &Header, idx: u32) -> String {
    if idx == u32::MAX {
        return String::new();
    }
    header
        .strings
        .get(idx as usize)
        .cloned()
        .unwrap_or_default()
}

/// NiObjectNET fields: Name, extra data refs, controller.
fn parse_object_net(r: &mut Reader<'_>, header: &Header) -> Result<ObjectNet> {
    let name = string_idx(header, r.u32()?);
    let num_extra = r.u32()? as usize;
    let mut extra_data = Vec::with_capacity(num_extra.min(1024));
    for _ in 0..num_extra {
        extra_data.push(r.i32()?);
    }
    let controller = r.i32()?;
    Ok(ObjectNet {
        name,
        extra_data,
        controller,
    })
}

fn parse_av_object(r: &mut Reader<'_>, header: &Header) -> Result<AvObject> {
    let net = parse_object_net(r, header)?;
    let flags = AvFlags::from_bits_retain(r.u32()?);
    let translation = [r.f32()?, r.f32()?, r.f32()?];
    let mut rotation = [[0.0f32; 3]; 3];
    for row in rotation.iter_mut() {
        for v in row.iter_mut() {
            *v = r.f32()?;
        }
    }
    let scale = r.f32()?;
    // NiAVObject::Num Properties/Properties (vercond `#NI_BS_LTE_FO3#`:
    // bs_version <= 34, i.e. non-Bethesda streams, Oblivion, Fallout 3/NV).
    // Removed for Skyrim (83) and later, where properties live on the
    // geometry block (shader/alpha refs) instead.
    let mut properties = Vec::new();
    if header.bs_version <= 34 {
        let num_properties = r.u32()? as usize;
        properties.reserve(num_properties.min(1024));
        for _ in 0..num_properties {
            properties.push(r.i32()?);
        }
    }
    let collision = r.i32()?;
    Ok(AvObject {
        net,
        flags,
        translation,
        rotation,
        scale,
        properties,
        collision,
    })
}

fn unpack_normbyte(b: u8) -> f32 {
    b as f32 / 255.0 * 2.0 - 1.0
}

/// Type name of the block a ref points to, resolved via the header's
/// block-type table (available for every block regardless of parse order).
fn ref_type_name(header: &Header, block_ref: i32) -> Option<&str> {
    let idx = usize::try_from(block_ref).ok()?;
    let type_idx = *header.block_type_index.get(idx)? as usize;
    header.block_types.get(type_idx).map(String::as_str)
}

/// Pre-Skyrim (`bs_version <= 34`) files attach shader/alpha properties to
/// the `NiAVObject`'s legacy `Properties` ref list instead of the dedicated
/// `Shader Property`/`Alpha Property` fields `NiGeometry` gained in Skyrim+.
/// Resolve them from that list so callers get a uniform `(shader, alpha)`
/// pair regardless of NIF era.
fn resolve_legacy_shader_alpha(header: &Header, properties: &[i32]) -> (i32, i32) {
    let mut shader = -1;
    let mut alpha = -1;
    for &prop in properties {
        match ref_type_name(header, prop) {
            Some(name) if name.contains("Shader") => shader = prop,
            Some("NiAlphaProperty") => alpha = prop,
            _ => {}
        }
    }
    (shader, alpha)
}

/// NiMain::MaterialData (version 20.2.0.7: no legacy `Has Shader` block,
/// since that field was removed after 20.1.0.3). Consumes the bytes and
/// returns the active material index (rarely useful, but kept for parity).
fn parse_material_data(r: &mut Reader<'_>) -> Result<i32> {
    let num_materials = r.u32()? as usize;
    for _ in 0..num_materials {
        let _material_name = r.u32()?; // NiFixedString ref
        let _material_extra_data = r.i32()?;
    }
    let active_material = r.i32()?;
    let _material_needs_update = r.u8()?;
    Ok(active_material)
}

/// NiTriStrips : NiTriBasedGeom : NiGeometry : NiAVObject.
fn parse_tri_strips(r: &mut Reader<'_>, header: &Header) -> Result<Block> {
    let bsver = header.bs_version;
    let av = parse_av_object(r, header)?;
    let data = r.i32()?;
    let skin = r.i32()?;
    let _active_material = parse_material_data(r)?;
    let (shader, alpha) = if bsver > 34 {
        (r.i32()?, r.i32()?)
    } else {
        resolve_legacy_shader_alpha(header, &av.properties)
    };

    let tri_strips = Block::TriStrips {
        av,
        data,
        skin,
        shader,
        alpha,
    };
    info!("NiTriStrips {:?}", tri_strips);
    Ok(tri_strips)
}

/// NiTriStripsData : NiTriBasedGeomData : NiGeometryData : NiObject.
///
/// Note: unlike `NiTriShapeData`/`BSTriShape`, this is *not* an `NiAVObject`
/// and has no name/transform fields of its own — those live on the owning
/// `NiTriStrips` node.
fn parse_tri_strips_data(r: &mut Reader<'_>, header: &Header) -> Result<Block> {
    let bsver = header.bs_version;

    // Group ID (since 10.1.0.114; always present for our supported version).
    let _group_id = r.i32()?;
    let num_vertices = r.u16()? as usize;
    let keep_flags = TriShapeFlags::from_bits_retain(r.u8()?);
    let compress_flags = r.u8()?;

    let has_vertices = r.u8()? != 0;
    let mut vertices = Vec::with_capacity(if has_vertices { num_vertices } else { 0 });
    if has_vertices {
        for _ in 0..num_vertices {
            vertices.push([r.f32()?, r.f32()?, r.f32()?]);
        }
    }

    // BS Data Flags (version 20.2.0.7 + Bethesda stream always uses this
    // form rather than the generic `NiGeometryDataFlags`).
    let bs_data_flags = r.u16()?;
    let has_uv = (bs_data_flags & 0x1) != 0;
    let has_tangents = (bs_data_flags & 0x1000) != 0;

    if bsver > 34 {
        let _material_crc = r.u32()?;
    }

    let has_normals = r.u8()? != 0;
    let mut normals = Vec::with_capacity(if has_normals { num_vertices } else { 0 });
    if has_normals {
        for _ in 0..num_vertices {
            normals.push([r.f32()?, r.f32()?, r.f32()?]);
        }
    }
    if has_normals && has_tangents {
        for _ in 0..num_vertices {
            r.skip(12)?; // tangents (Vector3), unused for now
        }
        for _ in 0..num_vertices {
            r.skip(12)?; // bitangents (Vector3), unused for now
        }
    }

    r.skip(16)?; // Bounding Sphere (NiBound: Vector3 center + float radius)

    let has_vertex_colors = r.u8()? != 0;
    let mut vertex_colors = Vec::with_capacity(if has_vertex_colors { num_vertices } else { 0 });
    if has_vertex_colors {
        for _ in 0..num_vertices {
            vertex_colors.push([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
        }
    }

    let num_uv_sets = if has_uv { 1 } else { 0 };
    let mut uv_sets = Vec::with_capacity(num_uv_sets);
    for _ in 0..num_uv_sets {
        let mut set = Vec::with_capacity(num_vertices);
        for _ in 0..num_vertices {
            set.push([r.f32()?, r.f32()?]);
        }
        uv_sets.push(set);
    }

    let _consistency_flags = r.u16()?;
    let _additional_data = r.i32()?;

    // NiTriBasedGeomData
    let _num_triangles = r.u16()? as u32;

    // NiTriStripsData
    let num_strips = r.u16()? as usize;
    let mut strip_lengths = Vec::with_capacity(num_strips);
    for _ in 0..num_strips {
        strip_lengths.push(r.u16()? as usize);
    }
    let has_points = r.u8()? != 0;
    let mut strips = Vec::with_capacity(num_strips);
    if has_points {
        for &len in &strip_lengths {
            let mut points = Vec::with_capacity(len);
            for _ in 0..len {
                points.push(r.u16()?);
            }
            strips.push(points);
        }
    }

    Ok(Block::TriStripsData {
        keep_flags,
        compress_flags,
        vertices,
        normals,
        vertex_colors,
        uv_sets,
        strips,
    })
}

fn tri_strips_data_to_geometry(b: &Block) -> Option<Geometry> {
    match b {
        Block::TriStripsData {
            vertices,
            normals,
            vertex_colors,
            uv_sets,
            strips,
            ..
        } => {
            let mut mesh = MeshData {
                positions: vertices.clone(),
                normals: normals.clone(),
                colors: vertex_colors.clone(),
                ..Default::default()
            };
            if !uv_sets.is_empty() {
                mesh.uvs = uv_sets[0].clone();
            }
            // Convert triangle strips to triangle list indices.
            for strip in strips {
                if strip.len() < 3 {
                    continue;
                }
                for i in 0..(strip.len() - 2) {
                    let a = strip[i];
                    let b = strip[i + 1];
                    let c = strip[i + 2];
                    if i % 2 == 0 {
                        mesh.indices.push(a as u32);
                        mesh.indices.push(b as u32);
                        mesh.indices.push(c as u32);
                    } else {
                        mesh.indices.push(b as u32);
                        mesh.indices.push(a as u32);
                        mesh.indices.push(c as u32);
                    }
                }
            }
            Some(Geometry::Embedded(mesh))
        }
        _ => None,
    }
}

fn parse_tri_shape(r: &mut Reader<'_>, header: &Header) -> Result<Block> {
    let bsver = header.bs_version;
    let av = parse_av_object(r, header)?;
    r.skip(16)?; // bounding sphere
    if bsver >= 155 {
        r.skip(24)?; // bounding box
    }

    let skin = r.i32()?;
    let shader = r.i32()?;
    let alpha = r.i32()?;
    let desc = r.u64()?;
    let num_triangles = if bsver >= 130 {
        r.u32()?
    } else {
        r.u16()? as u32
    } as usize;
    let num_vertices = r.u16()? as usize;
    let data_size = r.u32()?;

    let mut mesh = MeshData::default();
    if data_size > 0 {
        let stride = ((desc & 0xF) as usize) * 4;
        let mut a = ((desc >> 44) & 0xFFF) as u32;
        if bsver == 100 {
            a |= 0x400; // SSE: always full-precision positions
        }
        let base = r.pos();
        for i in 0..num_vertices {
            r.seek(base + i * stride)?;
            let mut bitangent = [0.0f32; 3];
            let mut has_bitangent = false;
            if (a & 0x401) == 0x401 {
                mesh.positions.push([r.f32()?, r.f32()?, r.f32()?]);
                if (a & 0x10) != 0 {
                    bitangent[0] = r.f32()?;
                    has_bitangent = true;
                } else {
                    r.skip(4)?;
                }
            } else if (a & 0x401) == 0x001 {
                mesh.positions.push([r.f16()?, r.f16()?, r.f16()?]);
                if (a & 0x10) != 0 {
                    bitangent[0] = r.f16()?;
                    has_bitangent = true;
                } else {
                    r.skip(2)?;
                }
            }
            if (a & 0x2) != 0 {
                mesh.uvs.push([r.f16()?, r.f16()?]);
            }
            let mut normal = [0.0f32; 3];
            if (a & 0x8) != 0 {
                normal = [
                    unpack_normbyte(r.u8()?),
                    unpack_normbyte(r.u8()?),
                    unpack_normbyte(r.u8()?),
                ];
                mesh.normals.push(normal);
                bitangent[1] = unpack_normbyte(r.u8()?);
            }
            if (a & 0x18) == 0x18 {
                let tangent = [
                    unpack_normbyte(r.u8()?),
                    unpack_normbyte(r.u8()?),
                    unpack_normbyte(r.u8()?),
                ];
                bitangent[2] = unpack_normbyte(r.u8()?);
                // w = sign of dot(bitangent, cross(normal, tangent)), default 1.0
                let w = if has_bitangent {
                    let cx = normal[1] * tangent[2] - normal[2] * tangent[1];
                    let cy = normal[2] * tangent[0] - normal[0] * tangent[2];
                    let cz = normal[0] * tangent[1] - normal[1] * tangent[0];
                    let dot = bitangent[0] * cx + bitangent[1] * cy + bitangent[2] * cz;
                    if dot < 0.0 {
                        -1.0
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };
                mesh.tangents.push([tangent[0], tangent[1], tangent[2], w]);
            }
            if (a & 0x20) != 0 {
                let b = r.bytes(4)?;
                mesh.colors.push([
                    b[0] as f32 / 255.0,
                    b[1] as f32 / 255.0,
                    b[2] as f32 / 255.0,
                    b[3] as f32 / 255.0,
                ]);
            }
            // Skinning / eye data skipped via stride-based reseek.
        }
        r.seek(base + num_vertices * stride)?;
        mesh.indices.reserve(num_triangles * 3);
        for _ in 0..num_triangles * 3 {
            mesh.indices.push(r.u16()? as u32);
        }
    }
    // SSE particle data (if any) is skipped by the caller's block-size resync.
    Ok(Block::TriShape {
        av,
        skin,
        shader,
        alpha,
        vertex_desc: desc,
        num_triangles: num_triangles as u32,
        num_vertices: num_vertices as u32,
        mesh,
    })
}

fn normalize_mesh_path(path: &str) -> String {
    let p = path.to_lowercase().replace('\\', "/");
    format!("geometries/{}.mesh", p.trim_start_matches('/'))
}

fn parse_bs_geometry(r: &mut Reader<'_>, header: &Header) -> Result<Block> {
    let av = parse_av_object(r, header)?;
    r.skip(16)?; // bounding sphere
    r.skip(24)?; // bounding box
    let skin = r.i32()?;
    let shader = r.i32()?;
    let alpha = r.i32()?;

    let embedded = av.flags.contains(AvFlags::BOUNDS_SELECTABLE);
    let mut geo = GeoSource::None;
    let mut lods = Vec::new();
    for _lod in 0..4 {
        let has_mesh = r.u8()?;
        if has_mesh != 1 {
            continue;
        }
        let indices_count = r.u32()?;
        let num_verts = r.u32()?;
        let mesh_flags = r.u32()?;
        if embedded {
            let mesh = parse_mesh_reader(r)?;
            lods.push(GeoLod {
                indices_count,
                num_verts,
                flags: mesh_flags,
                path: None,
            });
            if matches!(geo, GeoSource::None) {
                geo = GeoSource::Embedded(mesh);
            }
        } else {
            let path = r.sized_string()?;
            let norm = normalize_mesh_path(&path);
            lods.push(GeoLod {
                indices_count,
                num_verts,
                flags: mesh_flags,
                path: Some(norm.clone()),
            });
            if matches!(geo, GeoSource::None) {
                geo = GeoSource::External {
                    path: norm,
                    indices_count,
                    num_verts,
                };
            }
        }
    }
    Ok(Block::Geometry {
        av,
        skin,
        shader,
        alpha,
        lods,
        geo,
    })
}

fn parse_shader_property(r: &mut Reader<'_>, header: &Header, is_lighting: bool) -> Result<Block> {
    let bsver = header.bs_version;
    if is_lighting && (83..=130).contains(&bsver) {
        let _shader_type = r.u32()?;
    }
    let net = parse_object_net(r, header)?;

    let mut texture_set = -1;

    if is_lighting && bsver == 34 {
        r.skip(22)?;
        texture_set = r.i32()?;
    }

    if is_lighting && (bsver == 100 || bsver == 130) {
        // Shader Flags 1/2, UV Offset, UV Scale, then Texture Set ref.
        r.skip(4 + 4 + 8 + 8)?;
        texture_set = r.i32()?;
    }
    // Remainder skipped via block-size resync.
    Ok(Block::Shader { net, texture_set })
}

fn parse_texture_set(r: &mut Reader<'_>) -> Result<Block> {
    let count = r.u32()? as usize;
    let mut textures = Vec::with_capacity(count);
    for _ in 0..count {
        textures.push(r.sized_string()?);
    }
    Ok(Block::TextureSet { textures })
}

fn parse_alpha_property(r: &mut Reader<'_>, header: &Header) -> Result<Block> {
    let net = parse_object_net(r, header)?;
    let flags = r.u16()?;
    let threshold = r.u8()?;
    Ok(Block::Alpha {
        net,
        flags,
        threshold,
    })
}

/// NiIntegerExtraData / BSXFlags: name string ref + u32 value.
fn parse_int_extra(r: &mut Reader<'_>, header: &Header) -> Result<Block> {
    let name = string_idx(header, r.u32()?);
    let value = r.u32()?;
    Ok(Block::IntExtra { name, value })
}

fn print_block_debug(
    r: &mut Reader<'_>,
    index: usize,
    offset: usize,
    size: usize,
    type_name: &str,
) {
    let bytes = r.bytes(size).unwrap_or_default();
    info!(
        "block {} ({}) at offset {:02X?}, size {}, content: {:02X?}",
        index, type_name, offset, size, bytes
    );
    r.seek(offset).expect("");
}

#[allow(dead_code)]
fn print_block_remaining(r: &mut Reader<'_>, offset: usize, size: usize) {
    let curr_offset = r.pos();
    let block_offset = curr_offset.saturating_sub(offset);
    let remaining_size = size.saturating_sub(block_offset);
    let bytes = r.bytes(remaining_size).unwrap_or_default();
    info!(
        "remaining block content at offset {:02X?}, size {}, content: {:02X?}",
        offset, remaining_size, bytes
    );
    r.seek(curr_offset).expect("");
}

fn handle_unknown_block(offset: usize, type_name: &str) -> Result<Block> {
    info!(
        "skipping unknown block type: {} at offset {:02X?}",
        type_name, offset
    );
    Ok(Block::Unknown)
}

// ---- transforms (column-major [[f32;4];4], m[col][row]) ----

const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

fn mat_mul(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for (col, out_col) in out.iter_mut().enumerate() {
        for (row, v) in out_col.iter_mut().enumerate() {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k][row] * b[col][k];
            }
            *v = sum;
        }
    }
    out
}

fn local_transform(av: &AvObject) -> [[f32; 4]; 4] {
    // v' = R * (v * scale) + t; column-major: m[col][row] = R[row][col] * scale
    let mut m = IDENTITY;
    for (col, m_col) in m.iter_mut().enumerate().take(3) {
        for (row, v) in m_col.iter_mut().enumerate().take(3) {
            *v = av.rotation[row][col] * av.scale;
        }
    }
    m[3] = [av.translation[0], av.translation[1], av.translation[2], 1.0];
    m
}

// ---- top level ----

/// Fully parsed file: header tables, per-block parse results, footer roots.
pub(super) struct ParsedNif {
    pub header: Header,
    pub blocks: Vec<Block>,
    pub roots: Vec<i32>,
}

pub(super) fn parse_blocks(bytes: &[u8]) -> Result<ParsedNif> {
    let mut r = Reader::new(bytes);
    let header = parse_header(&mut r)?;

    info!("parsed header: {:?}", header);
    let mut blocks = Vec::with_capacity(header.num_blocks);
    let mut offset = r.pos();
    for i in 0..header.num_blocks {
        let size = header.block_sizes[i] as usize;
        let type_name = header
            .block_types
            .get(header.block_type_index[i] as usize)
            .map(String::as_str)
            .unwrap_or("");
        r.seek(offset)?;
        let end = offset + size;
        let mut parse_checkpoint = r.pos();
        info!(
            "parsing block {} ({}) at offset {:02X?}, size {}",
            i, type_name, offset, size
        );
        let parsed: Result<Block> = match type_name {
            "NiNode" | "BSFadeNode" | "BSLeafAnimNode" | "BSOrderedNode" | "BSMultiBoundNode"
            | "BSTreeNode" => {
                let av = parse_av_object(&mut r, &header);
                av.and_then(|av| {
                    parse_checkpoint = r.pos();
                    let num_children = r.u32()? as usize;
                    let num_children: usize = if num_children == 0xFFFFFFFF {
                        0
                    } else {
                        num_children
                    };
                    let mut children = Vec::with_capacity(num_children);
                    for _ in 0..num_children {
                        children.push(r.i32()?);
                    }
                    // Num Effects (bsver < 130) skipped via resync.
                    let node = Block::Node { av, children };
                    Ok(node)
                })
            }
            "NiTriStrips" => parse_tri_strips(&mut r, &header),
            "NiTriStripsData" => parse_tri_strips_data(&mut r, &header),
            "BSTriShape" | "BSSubIndexTriShape" => parse_tri_shape(&mut r, &header),
            "BSGeometry" => parse_bs_geometry(&mut r, &header),
            "BSShaderPPLightingProperty" => parse_shader_property(&mut r, &header, true),
            "BSLightingShaderProperty" => parse_shader_property(&mut r, &header, true),
            "BSEffectShaderProperty" => parse_shader_property(&mut r, &header, false),
            "BSShaderTextureSet" => parse_texture_set(&mut r),
            "NiAlphaProperty" => parse_alpha_property(&mut r, &header),
            "NiIntegerExtraData" | "BSXFlags" => parse_int_extra(&mut r, &header),
            _ => handle_unknown_block(offset, type_name),
        };
        let block = match parsed {
            Ok(b) => {
                if !matches!(b, Block::Unknown) && r.pos() != end {
                    debug!(
                        "block {} ({}) consumed {} of {} bytes; resyncing",
                        i,
                        type_name,
                        r.pos() as isize - offset as isize,
                        size
                    );
                }
                b
            }
            Err(e) => {
                r.seek(parse_checkpoint)?;
                let bytes = r.bytes(size).unwrap_or_default();
                info!("offset {:02X?}, bytes: {:02X?}", parse_checkpoint, bytes);
                warn!(
                    "failed to parse block {} ({}): {}; skipping",
                    i, type_name, e
                );
                Block::Unknown
            }
        };
        blocks.push(block);
        offset = end;
        if offset > bytes.len() {
            return Err(NifError::UnexpectedEof {
                needed: offset - bytes.len(),
                at: bytes.len(),
            });
        }
    }

    // Footer: u32 num roots + i32 refs.
    let mut roots: Vec<i32> = Vec::new();
    if r.seek(offset).is_ok() {
        if let Ok(num_roots) = r.u32() {
            for _ in 0..num_roots.min(1024) {
                match r.i32() {
                    Ok(root) => roots.push(root),
                    Err(_) => break,
                }
            }
        }
    }
    if roots.is_empty() {
        roots.push(0);
    }

    Ok(ParsedNif {
        header,
        blocks,
        roots,
    })
}

pub(super) fn parse_nif(bytes: &[u8]) -> Result<NifScene> {
    let parsed = parse_blocks(bytes)?;
    let mut scene = NifScene { meshes: Vec::new() };
    let mut visited = vec![false; parsed.blocks.len()];
    for root in parsed.roots {
        traverse(&parsed.blocks, root, &IDENTITY, &mut visited, &mut scene);
    }
    Ok(scene)
}

fn material_info(blocks: &[Block], shader: i32, alpha: i32) -> MaterialInfo {
    let mut info = MaterialInfo::default();
    if let Some(Block::Shader { net, texture_set }) =
        usize::try_from(shader).ok().and_then(|i| blocks.get(i))
    {
        let norm = net.name.to_lowercase().replace('\\', "/");
        if norm.ends_with(".mat") {
            info.mat_path = Some(norm);
        }
        if let Some(Block::TextureSet { textures }) = usize::try_from(*texture_set)
            .ok()
            .and_then(|i| blocks.get(i))
        {
            let slot = |n: usize| -> Option<String> {
                textures
                    .get(n)
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_lowercase().replace('\\', "/"))
            };
            info.diffuse = slot(0);
            info.normal = slot(1);
        }
    }
    if let Some(Block::Alpha {
        flags, threshold, ..
    }) = usize::try_from(alpha).ok().and_then(|i| blocks.get(i))
    {
        info.alpha_blend = (flags & 0x1) != 0;
        info.alpha_test = (flags & 0x200) != 0;
        info.alpha_threshold = *threshold;
    }
    info
}

fn traverse(
    blocks: &[Block],
    block_ref: i32,
    parent: &[[f32; 4]; 4],
    visited: &mut [bool],
    scene: &mut NifScene,
) {
    let idx = match usize::try_from(block_ref) {
        Ok(i) if i < blocks.len() => i,
        _ => return,
    };
    if visited[idx] {
        return;
    }
    visited[idx] = true;
    match &blocks[idx] {
        Block::Node { av, children } => {
            let world = mat_mul(parent, &local_transform(av));
            for &child in children {
                traverse(blocks, child, &world, visited, scene);
            }
        }
        Block::TriShape {
            av,
            shader,
            alpha,
            mesh,
            ..
        } => {
            let world = mat_mul(parent, &local_transform(av));
            scene.meshes.push(NifMeshInstance {
                name: av.name().to_string(),
                transform: world,
                geometry: Geometry::Embedded(mesh.clone()),
                material: material_info(blocks, *shader, *alpha),
            });
        }
        Block::Geometry {
            av,
            shader,
            alpha,
            geo,
            ..
        } => {
            let world = mat_mul(parent, &local_transform(av));
            let geometry = match geo {
                GeoSource::Embedded(mesh) => Geometry::Embedded(mesh.clone()),
                GeoSource::External {
                    path,
                    indices_count,
                    num_verts,
                } => Geometry::External {
                    path: path.clone(),
                    indices_count: *indices_count,
                    num_verts: *num_verts,
                },
                GeoSource::None => return,
            };
            scene.meshes.push(NifMeshInstance {
                name: av.name().to_string(),
                transform: world,
                geometry,
                material: material_info(blocks, *shader, *alpha),
            });
        }
        Block::TriStrips {
            av,
            data,
            skin: _,
            shader,
            alpha,
        } => {
            let world = mat_mul(parent, &local_transform(av));
            let tri_strips_geometry = tri_strips_data_to_geometry(&blocks[*data as usize]);
            if let Some(geometry) = tri_strips_geometry {
                scene.meshes.push(NifMeshInstance {
                    name: av.name().to_string(),
                    transform: world,
                    geometry,
                    material: material_info(blocks, *shader, *alpha),
                });
            }
        }
        _ => {}
    }
}
