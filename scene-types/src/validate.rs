use std::fmt;

use crate::{normal_matrix, AlphaMode, MaterialTypeDesc, SceneData};

/// A scene that cannot be uploaded as described.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneError {
    EmptyMesh {
        mesh: usize,
    },
    IndexCountNotTriangles {
        mesh: usize,
        count: usize,
    },
    IndexOutOfRange {
        mesh: usize,
        index: u32,
        vertices: usize,
    },
    AttributeLength {
        mesh: usize,
        attribute: &'static str,
        len: usize,
        vertices: usize,
    },
    NonFinite {
        what: String,
    },
    ImageSize {
        image: usize,
        expected: usize,
        actual: usize,
    },
    ZeroSizedImage {
        image: usize,
    },
    MissingReference {
        what: String,
        index: usize,
        len: usize,
    },
    SingularTransform {
        instance: usize,
    },
    InvalidAlphaCutoff {
        material: usize,
        cutoff: f32,
    },
    ColorSpace {
        material: usize,
        slot: String,
        expected: crate::ColorSpace,
    },
    UnknownMaterialType {
        material: usize,
        key: String,
    },
    MaterialBinding {
        material: usize,
        detail: String,
    },
    MaterialType {
        key: String,
        detail: String,
    },
}

impl fmt::Display for SceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SceneError::EmptyMesh { mesh } => {
                write!(f, "mesh {} has no positions or indices", mesh)
            }
            SceneError::IndexCountNotTriangles { mesh, count } => {
                write!(
                    f,
                    "mesh {} index count {} is not a multiple of 3",
                    mesh, count
                )
            }
            SceneError::IndexOutOfRange {
                mesh,
                index,
                vertices,
            } => write!(
                f,
                "mesh {} index {} out of range for {} vertices",
                mesh, index, vertices
            ),
            SceneError::AttributeLength {
                mesh,
                attribute,
                len,
                vertices,
            } => write!(
                f,
                "mesh {} {} has {} entries for {} vertices",
                mesh, attribute, len, vertices
            ),
            SceneError::NonFinite { what } => write!(f, "{} contains non-finite values", what),
            SceneError::ImageSize {
                image,
                expected,
                actual,
            } => write!(
                f,
                "image {} has {} bytes, expected {}",
                image, actual, expected
            ),
            SceneError::ZeroSizedImage { image } => write!(f, "image {} has zero size", image),
            SceneError::MissingReference { what, index, len } => {
                write!(
                    f,
                    "{} references index {} but only {} exist",
                    what, index, len
                )
            }
            SceneError::SingularTransform { instance } => {
                write!(f, "instance {} has a singular transform", instance)
            }
            SceneError::InvalidAlphaCutoff { material, cutoff } => {
                write!(
                    f,
                    "material {} alpha cutoff {} is outside 0..=1",
                    material, cutoff
                )
            }
            SceneError::ColorSpace {
                material,
                slot,
                expected,
            } => write!(
                f,
                "material {} '{}' texture must use colour space {:?}",
                material, slot, expected
            ),
            SceneError::UnknownMaterialType { material, key } => {
                write!(
                    f,
                    "material {} uses unregistered material type '{}'",
                    material, key
                )
            }
            SceneError::MaterialBinding { material, detail } => {
                write!(f, "material {}: {}", material, detail)
            }
            SceneError::MaterialType { key, detail } => {
                write!(f, "material type '{}': {}", key, detail)
            }
        }
    }
}

impl std::error::Error for SceneError {}

fn finite<const N: usize>(values: &[[f32; N]]) -> bool {
    values.iter().all(|v| v.iter().all(|c| c.is_finite()))
}

impl SceneData {
    /// Check index ranges, attribute lengths, finiteness, image sizes,
    /// references and transforms. Returns the first problem found.
    pub fn validate(&self) -> Result<(), SceneError> {
        for (m, mesh) in self.meshes.iter().enumerate() {
            let n = mesh.positions.len();
            if n == 0 || mesh.indices.is_empty() {
                return Err(SceneError::EmptyMesh { mesh: m });
            }
            if mesh.indices.len() % 3 != 0 {
                return Err(SceneError::IndexCountNotTriangles {
                    mesh: m,
                    count: mesh.indices.len(),
                });
            }
            if let Some(&index) = mesh.indices.iter().find(|&&i| i as usize >= n) {
                return Err(SceneError::IndexOutOfRange {
                    mesh: m,
                    index,
                    vertices: n,
                });
            }
            let lengths = [
                ("normals", mesh.normals.as_ref().map(Vec::len)),
                ("uv0", mesh.uv0.as_ref().map(Vec::len)),
                ("colors", mesh.colors.as_ref().map(Vec::len)),
                ("tangents", mesh.tangents.as_ref().map(Vec::len)),
            ];
            for (attribute, len) in lengths {
                if let Some(len) = len.filter(|&len| len != n) {
                    return Err(SceneError::AttributeLength {
                        mesh: m,
                        attribute,
                        len,
                        vertices: n,
                    });
                }
            }
            let ok = finite(&mesh.positions)
                && mesh.normals.as_deref().is_none_or(finite)
                && mesh.uv0.as_deref().is_none_or(finite)
                && mesh.colors.as_deref().is_none_or(finite)
                && mesh.tangents.as_deref().is_none_or(finite);
            if !ok {
                return Err(SceneError::NonFinite {
                    what: format!("mesh {}", m),
                });
            }
        }

        for (i, image) in self.images.iter().enumerate() {
            if image.width == 0 || image.height == 0 {
                return Err(SceneError::ZeroSizedImage { image: i });
            }
            let expected =
                image.width as usize * image.height as usize * image.format.bytes_per_pixel();
            if image.pixels.len() != expected {
                return Err(SceneError::ImageSize {
                    image: i,
                    expected,
                    actual: image.pixels.len(),
                });
            }
        }

        let images = self.images.len();
        for (m, material) in self.materials.iter().enumerate() {
            for (slot, t) in material.textures.iter().enumerate() {
                if let Some(t) = t {
                    if t.image >= images {
                        return Err(SceneError::MissingReference {
                            what: format!("material {} texture slot {}", m, slot),
                            index: t.image,
                            len: images,
                        });
                    }
                }
            }
            if let AlphaMode::Mask { cutoff } = material.alpha_mode {
                if !(0.0..=1.0).contains(&cutoff) {
                    return Err(SceneError::InvalidAlphaCutoff {
                        material: m,
                        cutoff,
                    });
                }
            }
        }

        for (i, instance) in self.instances.iter().enumerate() {
            if instance.mesh >= self.meshes.len() {
                return Err(SceneError::MissingReference {
                    what: format!("instance {} mesh", i),
                    index: instance.mesh,
                    len: self.meshes.len(),
                });
            }
            if instance.material >= self.materials.len() {
                return Err(SceneError::MissingReference {
                    what: format!("instance {} material", i),
                    index: instance.material,
                    len: self.materials.len(),
                });
            }
            if !finite(&instance.transform) {
                return Err(SceneError::NonFinite {
                    what: format!("instance {} transform", i),
                });
            }
            if normal_matrix(&instance.transform).is_none() {
                return Err(SceneError::SingularTransform { instance: i });
            }
        }
        Ok(())
    }

    /// Check every material against its registered type: the type exists,
    /// texture slots and uniform size match, and bound images use the slot's
    /// colour space. `lookup` resolves a type key.
    pub fn validate_materials<'a>(
        &self,
        lookup: impl Fn(&str) -> Option<&'a MaterialTypeDesc>,
    ) -> Result<(), SceneError> {
        for (m, material) in self.materials.iter().enumerate() {
            let ty =
                lookup(&material.material_type).ok_or_else(|| SceneError::UnknownMaterialType {
                    material: m,
                    key: material.material_type.clone(),
                })?;
            if material.textures.len() > ty.textures.len() {
                return Err(SceneError::MaterialBinding {
                    material: m,
                    detail: format!(
                        "{} texture slots supplied, type '{}' declares {}",
                        material.textures.len(),
                        ty.key,
                        ty.textures.len()
                    ),
                });
            }
            if !material.uniforms.is_empty() && material.uniforms.len() != ty.uniform_size {
                return Err(SceneError::MaterialBinding {
                    material: m,
                    detail: format!(
                        "{} uniform bytes supplied, type '{}' declares {}",
                        material.uniforms.len(),
                        ty.key,
                        ty.uniform_size
                    ),
                });
            }
            for (slot, t) in material.textures.iter().enumerate() {
                let (Some(t), Some(decl)) = (t, ty.textures.get(slot)) else {
                    continue;
                };
                if let Some(image) = self.images.get(t.image) {
                    if image.color_space != decl.color_space {
                        return Err(SceneError::ColorSpace {
                            material: m,
                            slot: decl.name.clone(),
                            expected: decl.color_space,
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

impl MaterialTypeDesc {
    /// Check the declaration itself: key, uniform size and default bytes.
    pub fn validate(&self) -> Result<(), SceneError> {
        let bad = |detail: String| SceneError::MaterialType {
            key: self.key.clone(),
            detail,
        };
        if self.key.is_empty() {
            return Err(bad("empty key".into()));
        }
        if !self.uniform_size.is_multiple_of(16) {
            return Err(bad(format!(
                "uniform size {} is not a multiple of 16",
                self.uniform_size
            )));
        }
        if self.default_uniforms.len() != self.uniform_size {
            return Err(bad(format!(
                "default uniforms are {} bytes, expected {}",
                self.default_uniforms.len(),
                self.uniform_size
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::*;

    fn triangle_scene() -> SceneData {
        SceneData {
            meshes: vec![MeshData {
                positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
                ..MeshData::default()
            }],
            images: vec![],
            materials: vec![MaterialDesc::default()],
            instances: vec![InstanceDesc::new(0, 0)],
        }
    }

    #[test]
    fn valid_scene_passes() {
        triangle_scene().validate().unwrap();
    }

    #[test]
    fn rejects_out_of_range_index() {
        let mut s = triangle_scene();
        s.meshes[0].indices[2] = 3;
        assert!(matches!(
            s.validate(),
            Err(SceneError::IndexOutOfRange { index: 3, .. })
        ));
    }

    #[test]
    fn rejects_attribute_length_mismatch_and_nan() {
        let mut s = triangle_scene();
        s.meshes[0].uv0 = Some(vec![[0.0; 2]; 2]);
        assert!(matches!(
            s.validate(),
            Err(SceneError::AttributeLength {
                attribute: "uv0",
                ..
            })
        ));
        let mut s = triangle_scene();
        s.meshes[0].positions[1][0] = f32::NAN;
        assert!(matches!(s.validate(), Err(SceneError::NonFinite { .. })));
    }

    #[test]
    fn rejects_bad_references_and_singular_transforms() {
        let mut s = triangle_scene();
        s.instances[0].material = 4;
        assert!(matches!(
            s.validate(),
            Err(SceneError::MissingReference { .. })
        ));
        let mut s = triangle_scene();
        s.materials[0].textures = vec![Some(TextureRef::new(0))];
        assert!(matches!(
            s.validate(),
            Err(SceneError::MissingReference { .. })
        ));
        let mut s = triangle_scene();
        s.instances[0].transform[2] = [0.0; 4];
        assert!(matches!(
            s.validate(),
            Err(SceneError::SingularTransform { .. })
        ));
    }

    fn two_slot_type() -> MaterialTypeDesc {
        MaterialTypeDesc {
            key: "test".into(),
            source: String::new(),
            textures: vec![
                TextureSlot {
                    name: "colour".into(),
                    color_space: ColorSpace::Srgb,
                    fallback: [255; 4],
                },
                TextureSlot {
                    name: "normal".into(),
                    color_space: ColorSpace::Linear,
                    fallback: [128, 128, 255, 255],
                },
            ],
            uniform_size: 16,
            default_uniforms: vec![0; 16],
        }
    }

    #[test]
    fn rejects_bad_images() {
        let mut s = triangle_scene();
        s.images
            .push(ImageData::rgba8("x", 2, 2, ColorSpace::Srgb, vec![0; 15]));
        assert!(matches!(
            s.validate(),
            Err(SceneError::ImageSize { expected: 16, .. })
        ));
    }

    #[test]
    fn materials_are_checked_against_their_type() {
        let ty = two_slot_type();
        let lookup = |k: &str| (k == "test").then_some(&ty);
        let mut s = triangle_scene();
        s.images
            .push(ImageData::rgba8("x", 1, 1, ColorSpace::Srgb, vec![0; 4]));
        s.materials[0].material_type = "test".into();
        s.validate_materials(lookup).unwrap();

        s.materials[0].textures = vec![None, Some(TextureRef::new(0))];
        assert!(matches!(
            s.validate_materials(lookup),
            Err(SceneError::ColorSpace { ref slot, .. }) if slot == "normal"
        ));
        s.materials[0].textures = vec![None; 3];
        assert!(matches!(
            s.validate_materials(lookup),
            Err(SceneError::MaterialBinding { .. })
        ));
        s.materials[0].textures.clear();
        s.materials[0].uniforms = vec![0; 8];
        assert!(matches!(
            s.validate_materials(lookup),
            Err(SceneError::MaterialBinding { .. })
        ));
        s.materials[0].material_type = "missing".into();
        assert!(matches!(
            s.validate_materials(lookup),
            Err(SceneError::UnknownMaterialType { .. })
        ));
    }

    #[test]
    fn material_type_declarations_are_checked() {
        let mut ty = two_slot_type();
        ty.validate().unwrap();
        ty.uniform_size = 12;
        assert!(ty.validate().is_err());
        ty.uniform_size = 16;
        ty.default_uniforms.pop();
        assert!(ty.validate().is_err());
    }

    #[test]
    fn negative_scale_is_valid_and_flips_normal_matrix_sign() {
        let mut s = triangle_scene();
        s.instances[0].transform[0][0] = -2.0;
        s.validate().unwrap();
        let (m, sign) = normal_matrix(&s.instances[0].transform).unwrap();
        assert_eq!(sign, -1.0);
        assert!((m[0][0] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn bounds_use_instance_transforms() {
        let mut s = triangle_scene();
        s.instances[0].transform[3] = [10.0, 0.0, 0.0, 1.0];
        let b = s.bounds().unwrap();
        assert_eq!(b.min, [10.0, 0.0, 0.0]);
        assert_eq!(b.max, [11.0, 1.0, 0.0]);
    }
}
