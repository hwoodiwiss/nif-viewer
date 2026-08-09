# NIF 20.2.0.7 / Starfield Format Spec (for Rust parser)

All facts sourced from:
- `R:\source\repos\nifskope-fo76\build\nif.xml` (nifskope-fo76's authoritative XML — the upstream `R:\source\repos\nifxml\nif.xml` does **not** contain Starfield BSGeometry/BSMeshData; use the fo76 fork's copy)
- `R:\source\repos\nifskope-fo76\src\io\MeshFile.cpp` (.mesh reader)
- `R:\source\repos\nifskope-fo76\lib\libfo76utils\src\fp32vec4_base.hpp` (unpack formulas)
- `R:\source\repos\nifskope-fo76\lib\libfo76utils\src\material.hpp`, `mat_json.cpp` (Starfield .mat)

**Everything is little-endian.** All Bethesda files of interest are NIF version `20.2.0.7` = u32 `0x14020007`.

Primitive shorthand: `u8/u16/u32/i16/i32/f32/f16` (f16 = IEEE half). `SizedString` = `u32 length` + `length` bytes (no NUL). `ExportString` = `u8 length` + `length` bytes **including** a NUL terminator. `Triangle` = 3 × `u16` vertex indices.

---

## 1. Header (nif.xml `Header` struct, build/nif.xml:2502)

Read in this exact order:

| # | Field | Type | Notes |
|---|-------|------|-------|
| 1 | Header String | line terminated by `0x0A` | `"Gamebryo File Format, Version 20.2.0.7\n"` (NetImmerse for <= 10.0.1.0; see nifmodel.cpp:407). Parse version out of it or just validate. |
| 2 | Version | u32 | `0x14020007` for all games here |
| 3 | Endian Type | u8 | 1 = little-endian (always, for these games) |
| 4 | User Version | u32 | 12 for Skyrim/SSE/FO4; 0x0C for Starfield too |
| 5 | Num Blocks | u32 | |
| 6 | BS Header | BSStreamHeader | present because this is a Bethesda stream (`#BSSTREAMHEADER#`) — see below |
| 7 | Num Block Types | u16 | |
| 8 | Block Types | SizedString × Num Block Types | e.g. `"NiNode"`, `"BSGeometry"` |
| 9 | Block Type Index | u16 × Num Blocks | index into Block Types. **Upper bit (0x8000) is a flag** (used for PhysX block types) — mask with `0x7FFF` (nif.xml:308 `BlockTypeIndex`) |
| 10 | Block Size | u32 × Num Blocks | byte size of each block (since 20.2.0.5). Use it to skip unknown blocks. |
| 11 | Num Strings | u32 | |
| 12 | Max String Length | u32 | informational only |
| 13 | Strings | SizedString × Num Strings | the string table; `string` fields in blocks are u32 indices into this (`0xFFFFFFFF` = none) |
| 14 | Num Groups | u32 | usually 0 |
| 15 | Groups | u32 × Num Groups | |

After the header, the `Num Blocks` block bodies follow back-to-back (sizes per array #10), then a footer (u32 num roots + root refs) which you can ignore.

### BSStreamHeader (build/nif.xml:2490)

| Field | Type | Condition |
|-------|------|-----------|
| BS Version | u32 | always. **100 = Skyrim SE, 130 = FO4, 155 = FO76, 172/173+ = Starfield** (xml uses thresholds `>=170` for STF, `>=175` for late-SF fields) |
| Author | ExportString | always |
| Unknown Int | u32 | if `BS Version > 130` |
| Process Script | ExportString | if `BS Version < 131` |
| Export Script | ExportString | always |
| Max Filepath | ExportString | if `BS Version >= 103 && BS Version < 170` |
| Unknown Data | u8 length + `length` bytes | if `BS Version >= 170` (Starfield, `ExportDataSF`) |

---

## 2. BSTriShape (SSE BSVER=100 / FO4 BSVER=130) — build/nif.xml:9482

### NiObject inheritance chain fields (read first, in order)

**NiObjectNET** (nif.xml:3908) — for 20.2.0.7:
- `Name`: u32 string-table index
- `Num Extra Data List`: u32
- `Extra Data List`: i32 ref × count
- `Controller`: i32 ref

(The `Shader Type` u32 prefix in NiObjectNET applies **only** to `BSLightingShaderProperty` blocks with BSVER 83..130, see §6.)

**NiAVObject** (nif.xml:3989):
- `Flags`: u32 (because BSVER > 26; plain Gamebryo uses u16)
- `Translation`: 3 × f32
- `Rotation`: 9 × f32 (Matrix33, **row-major rows of basis vectors**; nifskope treats it as 3 rows of 3)
- `Scale`: f32 (uniform)
- `Collision Object`: i32 ref

### BSTriShape body

| Field | Type | Condition |
|-------|------|-----------|
| Bounding Sphere | Vector3 center + f32 radius (`NiBound`) | always |
| Bounding Box | Vector3 center + Vector3 dimensions (`BSBoundingBox`) | **only BSVER >= 155 (FO76)** — absent in SSE/FO4 |
| Skin | i32 ref | always |
| Shader Property | i32 ref (→ BSLightingShaderProperty etc.) | always |
| Alpha Property | i32 ref (→ NiAlphaProperty) | always |
| Vertex Desc | u64 (`BSVertexDesc` bitfield) | always |
| Num Triangles | **u32** if BSVER >= 130 (FO4/76); **u16** if BSVER < 130 (SSE) | |
| Num Vertices | u16 | always |
| Data Size | u32 | = `(VertexDesc & 0xF) * NumVertices * 4 + NumTriangles * 6` |
| Vertex Data | NumVertices × vertex records | only if `Data Size > 0`. Layout per `VertexDesc >> 44` attribute flags (below) |
| Triangles | NumTriangles × Triangle (3×u16) | only if `Data Size > 0` |
| Particle Data Size | u32 | **SSE only** (BSVER == 100) |
| Particle Vertices | NumVertices × 3×f16 | SSE only, if Particle Data Size > 0 |
| Particle Normals | NumVertices × 3×f16 | SSE only, if Particle Data Size > 0 |
| Particle Triangles | NumTriangles × Triangle | SSE only, if Particle Data Size > 0 |

### BSVertexDesc u64 bitfield (nif.xml:2631)

| Bits | Meaning |
|------|---------|
| 0–3 | Vertex Data Size (in **u32 units**: bytes = value × 4) |
| 4–7 | Dynamic Vertex Size |
| 8–11 | UV1 Offset |
| 12–15 | UV2 Offset |
| 16–19 | Normal Offset |
| 20–23 | Tangent Offset |
| 24–27 | Color Offset |
| 28–31 | Skinning Data Offset |
| 32–35 | Landscape Data Offset |
| 36–39 | Eye Data Offset |
| 44–55 | **Vertex Attributes flags** (`VertexAttribute`, nif.xml:2616) |

Attribute flags (`let a = (desc >> 44) & 0xFFF`):

| Bit | Flag |
|-----|------|
| 0 (0x1) | Vertex (positions) |
| 1 (0x2) | UVs |
| 2 (0x4) | UVs_2 |
| 3 (0x8) | Normals |
| 4 (0x10) | Tangents |
| 5 (0x20) | Vertex_Colors |
| 6 (0x40) | Skinned |
| 7 (0x80) | Land_Data |
| 8 (0x100) | Eye_Data |
| 10 (0x400) | Full_Precision |

### Per-vertex record layout, in field order

FO4/76 (`BSVertexData`, nif.xml:2646) — note in FO4 half-precision positions are the default; SSE (`BSVertexDataSSE`, nif.xml:2667) **always** uses full-precision f32 positions (treat as if 0x400 always set):

| Field | Type | Condition (a = attr flags; for SSE substitute `a|0x400` for a) |
|-------|------|------------|
| Vertex | 3×f32 | `(a & 0x401) == 0x401` |
| Bitangent X | f32 | `(a & 0x411) == 0x411` |
| Unused W | u32 | `(a & 0x411) == 0x401` (padding when no tangents) |
| Vertex | 3×f16 | `(a & 0x401) == 0x001` (FO4 half precision) |
| Bitangent X | f16 | `(a & 0x411) == 0x011` |
| Unused W | u16 | `(a & 0x411) == 0x001` |
| UV | 2×f16 | `a & 0x2` |
| Normal | 3×u8 | `a & 0x8` — unpack: `n = b/255*2 - 1` (byte maps [0,255]→[-1,1]) |
| Bitangent Y | u8 (normbyte) | `a & 0x8` — same unpack |
| Tangent | 3×u8 | `(a & 0x18) == 0x18` — same unpack |
| Bitangent Z | u8 | `(a & 0x18) == 0x18` |
| Vertex Colors | 4×u8 RGBA | `a & 0x20` — `c/255` |
| Bone Weights | 4×f16 | `a & 0x40` |
| Bone Indices | 4×u8 | `a & 0x40` |
| Eye Data | f32 | `a & 0x100` |

Bitangent = (Bitangent X, Y, Z) reassembled from the three scattered fields. Record stride must equal `(desc & 0xF) * 4`.

---

## 3. Starfield BSGeometry (BSVER >= 170) — build/nif.xml:9863

Inherits NiAVObject (same fields as §2, incl. u32 Flags). Body:

| Field | Type |
|-------|------|
| Bounding Sphere | Vector3 + f32 (`NiBound`) |
| Bounding Box | Vector3 center + Vector3 dimensions (`BSBoundingBox`) |
| Skin | i32 ref |
| Shader Property | i32 ref |
| Alpha Property | i32 ref |
| Meshes | `BSMeshArray` × **4** (fixed-length LOD array), with `arg = NiAVObject Flags` |

### BSMeshArray (nif.xml:9858)
- `Has Mesh`: u8 — if `== 1`, a `BSMesh` follows (arg passed down = `Flags & 512`)

### BSMesh (nif.xml:9850)
| Field | Type | Condition |
|-------|------|-----------|
| Indices Size | u32 | always (triangle index count = 3 × tri count) |
| Num Verts | u32 | always |
| Flags | u32 | always |
| Mesh Path | SizedString | if `(NiAVObject.Flags & 512) == 0` — path **without** `geometries\` prefix or `.mesh` extension; resolve as `geometries\<path>.mesh` (MeshFile.cpp:213) |
| Mesh Data | `BSMeshData` (inline, same layout as external .mesh file, §4) | if `(NiAVObject.Flags & 512) != 0` (embedded geometry) |

So: **NiAVObject Flags bit 9 (0x200) selects embedded vs external geometry.**

---

## 4. Starfield external `.mesh` file format

Source: `src/io/MeshFile.cpp:49-203` (`MeshFile::update`) and nif.xml `BSMeshData` (build/nif.xml:9822). Read sequentially, little-endian:

| Field | Type | Notes |
|-------|------|-------|
| Version | u32 | must be `<= 2` |
| Indices Size | u32 | count of u16 indices |
| Triangles | (Indices Size / 3) × Triangle (3×u16) | |
| Scale | f32 | must be `> 0` else invalid |
| Weights Per Vertex | u32 | |
| Num Verts | u32 | must be > 0 |
| Vertices | Num Verts × (3 × i16) | read as u32 `xy` + u16 `z` (6 bytes). **Dequantize: `pos = f32(i16) / 32767.0 * scale`** (MeshFile.cpp:106-108; convertInt16 in fp32vec4_base.hpp:48 is plain `float(int16)`) |
| Num UVs | u32 | |
| UVs | Num UVs × (2 × f16) | UV set 1 |
| Num UVs 2 | u32 | |
| UVs 2 | Num UVs 2 × (2 × f16) | present only if count > 0 (array is simply empty otherwise) |
| Num Vertex Colors | u32 | |
| Vertex Colors | count × u32 **BGRA** | `r = byte2/255, g = byte1/255, b = byte0/255, a = byte3/255` (shuffle 2,1,0,3 — MeshFile.cpp:147) |
| Num Normals | u32 | |
| Normals | count × u32 packed X10Y10Z10W2 | unpack below |
| Num Tangents | u32 | |
| Tangents | count × u32 packed X10Y10Z10W2 | xyz = tangent, **w = bitangent sign/basis**: `bitangent = cross(normal, tangent * w)` (MeshFile.h:36) |
| Num Weights | u32 | total = NumVerts × WeightsPerVertex |
| Weights | count × (u16 bone, u16 weight) | weight normalized: `w / 65535.0` |
| Num LODs | u32 | **only if Version >= 1** |
| LODs | Num LODs × { u32 indicesSize; (indicesSize/3) × Triangle } | alternate index buffers (BSMeshTriangles, nif.xml:9798) |
| Num Meshlets | u32 | per nif.xml (nifskope's raw reader stops before these — safe to stop reading here for rendering) |
| Meshlets | count × { u32 vertCount, u32 vertOffset, u32 triCount, u32 triOffset } | `BSMeshlet`, nif.xml:9803 — skip: 16 bytes each |
| Num Cull Data | u32 | |
| Cull Data | count × `BSCullData` | if Version < 2: NiBound (16 B) + 4×u8 normal cone + f32 apex offset = 24 B each; if Version >= 2: BSBoundingBox (24 B) each (nif.xml BSCullData, arg=Version) |

### X10Y10Z10W2 unpack (fp32vec4_base.hpp `convertX10Y10Z10W2`)

```text
x = f32(n & 0x3FF)          * (2/1023)       - 1
y = f32(n & 0xFFC00)        * (2/1047552)    - 1     // == ((n>>10)&0x3FF)/1023*2 - 1
z = f32(n & 0x3FF00000)     * (2/1072693248) - 1     // == ((n>>20)&0x3FF)/1023*2 - 1
w = f32(n >> 30)            * (2/3)          - 1
```

(Unsigned-normalized 10:10:10:2 remapped from [0,1] to [-1,1].)

---

## 5. Scene graph

- **Refs** are `i32`; `-1` = null. They index blocks 0..NumBlocks-1.
- **NiNode** (nif.xml:4933) = NiAVObject fields (§2) + :
  - `Num Children`: u32, `Children`: i32 ref × count
  - `Num Effects`: u32 + refs — **only if BSVER < 130** (absent in FO4/76/SF: `#NI_BS_LT_FO4#`)
- **Transform composition**: `world = parent_world * local`, where local applies `v' = rotation * (v * scale) + translation`. Traverse from root (block 0 typically, or footer roots), multiplying down.
- Skippable for static rendering (use header Block Size array to skip unknown types blindly):
  - `bhk*` (Havok collision) — complex, skip via block size
  - `BSXFlags` (nif.xml:4847): inherits NiIntegerExtraData
  - `NiIntegerExtraData` (nif.xml:4842): `Name` (u32 string idx) + `Integer Data` (u32)
  - `NiExtraData` base: just `Name` (u32 string idx)
  - `NiAlphaProperty` (useful, tiny): NiObjectNET+NiAVObject? No — inherits NiProperty (= NiObjectNET only), then `Flags` u16 + `Threshold` u8. Flags bit 0 = alpha blend enable, bit 9 = alpha test enable, bits 10-12 test func, default 4844.

---

## 6. Materials / textures

### BSLightingShaderProperty (SSE / FO4) — build/nif.xml:7766

Inherits `BSShaderProperty` → `NiShadeProperty` → `NiProperty` → NiObjectNET. For BSVER >= 83 none of the old BSShaderProperty fields are written; the effective stream for SSE (BSVER 100) is:

1. `Shader Type`: u32 (**written before Name** — NiObjectNET quirk, only for BSLightingShaderProperty at BSVER 83..130; nif.xml:3910). 0=default, 1=envmap, 5=skin, 6=hair...
2. `Name`: u32 string idx (for FO4 may be a `.bgsm` material path; for Starfield a `.mat` path)
3. `Num Extra Data List` u32 + refs, `Controller` ref
4. Then, **SSE (BSVER < 130)**: `Shader Flags 1` u32, `Shader Flags 2` u32, `UV Offset` 2×f32, `UV Scale` 2×f32, **`Texture Set` i32 ref**, `Emissive Color` 3×f32, `Emissive Multiple` f32, `Texture Clamp Mode` u32, `Alpha` f32, `Refraction Strength` f32, `Glossiness` f32, `Specular Color` 3×f32, `Specular Strength` f32, `Lighting Effect 1` f32, `Lighting Effect 2` f32, then shader-type-conditional extras (env map scale if type==1, skin tint if 5, hair tint if 6, ...).
5. **FO4 (BSVER == 130)** differs: FO4 flags, then `Num SF1`/`SF1` only for BSVER 132–139 (skip for 130), `UV Offset/Scale`, `Texture Set` ref, `Emissive Color/Multiple`, `Root Material` (u32 string idx), `Texture Clamp Mode`, `Alpha`, `Refraction Strength`, `Smoothness`, `Specular Color`, `Specular Strength`, `Subsurface Rolloff`, `Rimlight Power`, conditional `Backlight Power`, `Grayscale to Palette Scale`, `Fresnel Power`, `Wetness` params (7×f32 in FO4), plus type-conditional extras. (See xml block for exact FO4 list if you need full FO4 fidelity.)

### BSShaderTextureSet (build/nif.xml:6860)

- `Num Textures`: u32 (default 6; FO4 uses 10)
- `Textures`: SizedString × count. Slots:
  - **0 = Diffuse**, **1 = Normal/Gloss**, 2 = Glow/Skin/Hair, 3 = Height/Parallax, 4 = Environment, 5 = Env Mask, 6 = Subsurface (FO4: 7 = specular/smoothness in practice), 7 = Backlight, 9/10 = FO76 reflectivity/lighting

### Starfield material linkage

- The BSGeometry's `Shader Property` ref points to a **`BSLightingShaderProperty`** block whose **`Name` string is the `.mat` path** (e.g. `materials\...\foo.mat`). nif.xml:7768: `Material type="BSLayeredMaterial" vercond="#BS_GTE_STF#" cond="$Name"` — i.e. everything comes from the external material; the block itself has essentially no further parseable payload for SF.
- `.mat` files are JSON (libfo76utils `mat_json.cpp:538-560`): top-level `{"Version": 1, "Objects": [...]}`. Each object: `{"Parent": "<res-id or path>", "ID": "<this>"|res-id, "Components": [ ... ]}`. Components are typed by reflection name; texture paths live in components of type **`BSMaterial::TextureFile`** / **`BSMaterial::MRTextureFile`** (bsrefl.cpp:717/698) containing a **`FileName`** field (bsrefl.cpp:1003) with a `.dds` path. Slot semantics come from the owning `TextureSet` object's channel index (material.hpp:69-84):
  - index 0 = **albedo** (`_color.dds`), 1 = **normal** (`_normal.dds`), 2 = opacity, 3 = roughness, 4 = metalness, 5 = AO, 6 = height, 7 = emissive, 8 = transmissive, 20 = id.
  - Full fidelity requires resolving the object inheritance chain against the game's `materialsbeta.cdb`; for a simple viewer, scanning the JSON for `FileName` entries ending in `_color.dds` / `_normal.dds` is a pragmatic shortcut.

### Common DDS formats (brief)

- BC1 (DXT1): opaque/1-bit-alpha diffuse
- BC3 (DXT5): diffuse w/ alpha (older assets)
- BC5: 2-channel normal maps (X,Y; reconstruct Z)
- BC7: high-quality diffuse/normal/packed maps (FO4+, standard in Starfield)
- DX10 header extension present when DXGI format is used (BC5/BC7 always).

---

## Quick reference: BS version gates used above

| Symbol | Condition |
|--------|-----------|
| SSE | BSVER == 100 |
| FO4 | BSVER == 130 |
| FO76 | BSVER == 155 |
| Starfield | BSVER >= 170 (172 launch, 173+ updates) |
| `#BS_GTE_130#` | BSVER >= 130 |
| `#BS_GTE_F76#` | BSVER >= 155 |
| `#BS_GTE_STF#` / `#STF#` | BSVER >= 170 |
