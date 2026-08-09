//! Raw NIF structure introspection: a JSON view of the block graph for
//! debugging / UI tree views. Reuses the block parser in `parser.rs`.

use serde_json::{json, Value};

use super::parser::{parse_blocks, AvObject, Block, GeoSource, ObjectNet};
use super::reader::NifError;

/// Parse a NIF and return a JSON description of its raw block structure.
///
/// Schema:
/// ```json
/// {
///   "headerString": "...", "version": "20.2.0.7", "bsVersion": 173,
///   "endian": 1, "userVersion": 12, "numBlocks": N,
///   "blocks": [ { "index", "type", "size", "name"?, "fields", "children" } ],
///   "roots": [0],
///   "strings": ["..."]
/// }
/// ```
/// Blocks the parser does not understand still appear with `index`/`type`/
/// `size` and `"fields": {"skipped": true}`. `children` lists block refs
/// (child nodes, extra data, controllers, shader/alpha/skin/texture-set refs)
/// so a UI can render the graph.
pub fn nif_structure(bytes: &[u8]) -> Result<Value, NifError> {
    let parsed = parse_blocks(bytes)?;
    let header = &parsed.header;

    let blocks: Vec<Value> = parsed
        .blocks
        .iter()
        .enumerate()
        .map(|(i, block)| {
            let type_name = header
                .block_types
                .get(header.block_type_index[i] as usize)
                .map(String::as_str)
                .unwrap_or("");
            let size = header.block_sizes.get(i).copied().unwrap_or(0);
            let (name, fields, children) = block_details(block);
            json!({
                "index": i,
                "type": type_name,
                "size": size,
                "name": name,
                "fields": fields,
                "children": children,
            })
        })
        .collect();

    let v = header.version;
    Ok(json!({
        "headerString": header.header_string,
        "version": format!(
            "{}.{}.{}.{}",
            (v >> 24) & 0xFF, (v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF
        ),
        "bsVersion": header.bs_version,
        "endian": header.endian,
        "userVersion": header.user_version,
        "numBlocks": header.num_blocks,
        "blocks": blocks,
        "roots": parsed.roots,
        "strings": header.strings,
    }))
}

/// Collect valid (>= 0) block refs into a children list.
fn push_refs(children: &mut Vec<i32>, refs: &[i32]) {
    children.extend(refs.iter().copied().filter(|&r| r >= 0));
}

fn net_children(net: &ObjectNet) -> Vec<i32> {
    let mut c = Vec::new();
    push_refs(&mut c, &net.extra_data);
    push_refs(&mut c, &[net.controller]);
    c
}

fn av_fields(av: &AvObject) -> Value {
    json!({
        "name": av.net.name,
        "flags": av.flags,
        "translation": av.translation,
        "rotation": av.rotation,
        "scale": av.scale,
        "collisionRef": av.collision,
        "controllerRef": av.net.controller,
        "extraDataRefs": av.net.extra_data,
    })
}

fn merge(base: Value, extra: Value) -> Value {
    let (Value::Object(mut a), Value::Object(b)) = (base, extra) else {
        unreachable!("merge called with non-objects");
    };
    a.extend(b);
    Value::Object(a)
}

/// Decode the BSTriShape vertex-desc attribute bits into human-readable flags.
fn decode_vertex_desc(desc: u64) -> Value {
    let attrs = (desc >> 44) & 0xFFF;
    json!({
        "hex": format!("0x{desc:016X}"),
        "strideDwords": desc & 0xF,
        "vertex": (attrs & 0x1) != 0,
        "uvs": (attrs & 0x2) != 0,
        "normals": (attrs & 0x8) != 0,
        "tangents": (attrs & 0x10) != 0,
        "colors": (attrs & 0x20) != 0,
        "skinned": (attrs & 0x40) != 0,
        "eyeData": (attrs & 0x100) != 0,
        "fullPrecision": (attrs & 0x400) != 0,
    })
}

/// Per-block (name, fields, children).
fn block_details(block: &Block) -> (Option<&str>, Value, Vec<i32>) {
    match block {
        Block::Node { av, children } => {
            let mut refs = net_children(&av.net);
            push_refs(&mut refs, children);
            (
                Some(av.name()),
                merge(av_fields(av), json!({ "childRefs": children })),
                refs,
            )
        }
        Block::TriShape {
            av,
            skin,
            shader,
            alpha,
            vertex_desc,
            num_triangles,
            num_vertices,
            ..
        } => {
            let mut refs = net_children(&av.net);
            push_refs(&mut refs, &[*skin, *shader, *alpha]);
            (
                Some(av.name()),
                merge(
                    av_fields(av),
                    json!({
                        "skinRef": skin,
                        "shaderRef": shader,
                        "alphaRef": alpha,
                        "vertexDesc": decode_vertex_desc(*vertex_desc),
                        "numTriangles": num_triangles,
                        "numVertices": num_vertices,
                    }),
                ),
                refs,
            )
        }
        Block::Geometry {
            av,
            skin,
            shader,
            alpha,
            lods,
            geo,
        } => {
            let mut refs = net_children(&av.net);
            push_refs(&mut refs, &[*skin, *shader, *alpha]);
            let lods: Vec<Value> = lods
                .iter()
                .map(|l| {
                    json!({
                        "indicesSize": l.indices_count,
                        "numVerts": l.num_verts,
                        "flags": l.flags,
                        "meshPath": l.path,
                        "embedded": l.path.is_none(),
                    })
                })
                .collect();
            let geo_kind = match geo {
                GeoSource::Embedded(_) => "embedded",
                GeoSource::External { .. } => "external",
                GeoSource::None => "none",
            };
            (
                Some(av.name()),
                merge(
                    av_fields(av),
                    json!({
                        "skinRef": skin,
                        "shaderRef": shader,
                        "alphaRef": alpha,
                        "lods": lods,
                        "geometrySource": geo_kind,
                    }),
                ),
                refs,
            )
        }
        Block::Shader { net, texture_set } => {
            let mut refs = net_children(net);
            push_refs(&mut refs, &[*texture_set]);
            (
                Some(net.name.as_str()),
                json!({
                    "name": net.name,
                    "textureSetRef": texture_set,
                    "controllerRef": net.controller,
                    "extraDataRefs": net.extra_data,
                }),
                refs,
            )
        }
        Block::TextureSet { textures } => (None, json!({ "textures": textures }), Vec::new()),
        Block::Alpha {
            net,
            flags,
            threshold,
        } => (
            Some(net.name.as_str()),
            json!({
                "name": net.name,
                "flags": flags,
                "threshold": threshold,
                "alphaBlend": (flags & 0x1) != 0,
                "srcBlend": (flags >> 1) & 0xF,
                "dstBlend": (flags >> 5) & 0xF,
                "alphaTest": (flags & 0x200) != 0,
                "testFunc": (flags >> 10) & 0x7,
                "noSorter": (flags & 0x2000) != 0,
            }),
            net_children(net),
        ),
        Block::IntExtra { name, value } => (
            Some(name.as_str()),
            json!({ "name": name, "value": value }),
            Vec::new(),
        ),
        Block::Unknown => (None, json!({ "skipped": true }), Vec::new()),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::nif_structure;

    const SAMPLE: &str =
        r"C:\Users\secro\Documents\StarfieldResources\meshes\weapons\ar99\ar99.nif";

    #[test]
    fn structure_of_sample_nif() {
        let Ok(bytes) = std::fs::read(SAMPLE) else {
            eprintln!("sample assets not present; skipping");
            return;
        };
        let v = nif_structure(&bytes).expect("nif_structure");

        let num_blocks = v["numBlocks"].as_u64().unwrap() as usize;
        let blocks = v["blocks"].as_array().unwrap();
        assert_eq!(num_blocks, blocks.len());

        // Root exists and is a valid block index.
        let roots = v["roots"].as_array().unwrap();
        assert!(!roots.is_empty());
        let root = roots[0].as_i64().unwrap();
        assert!(root >= 0 && (root as usize) < num_blocks);

        // At least one BSGeometry with an external meshPath.
        let has_geo_path = blocks.iter().any(|b| {
            b["type"] == "BSGeometry"
                && b["fields"]["lods"]
                    .as_array()
                    .is_some_and(|l| l.iter().any(|lod| lod["meshPath"].is_string()))
        });
        assert!(has_geo_path, "no BSGeometry block with a meshPath");

        // Skipped blocks still carry type + size.
        let skipped: Vec<_> = blocks
            .iter()
            .filter(|b| b["fields"]["skipped"] == true)
            .collect();
        assert!(!skipped.is_empty(), "expected some unparsed blocks");
        for b in skipped {
            assert!(b["type"].as_str().is_some_and(|t| !t.is_empty()));
            assert!(b["size"].as_u64().unwrap() > 0);
        }
    }
}
