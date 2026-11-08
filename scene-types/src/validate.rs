use std::fmt;

use crate::{normal_matrix, AlphaMode, SceneData, TextureRef};

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
        slot: &'static str,
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
            SceneError::ColorSpace { material, slot } => {
                write!(f, "material {} {} texture must be linear", material, slot)
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
        let check_tex = |m: usize, slot: &'static str, t: &Option<TextureRef>, linear: bool| {
            if let Some(t) = t {
                let image =
                    self.images
                        .get(t.image)
                        .ok_or_else(|| SceneError::MissingReference {
                            what: format!("material {} {} texture", m, slot),
                            index: t.image,
                            len: images,
                        })?;
                if linear && image.color_space != crate::ColorSpace::Linear {
                    return Err(SceneError::ColorSpace { material: m, slot });
                }
            }
            Ok(())
        };
        for (m, material) in self.materials.iter().enumerate() {
            check_tex(m, "base colour", &material.base_color_texture, false)?;
            check_tex(m, "normal", &material.normal_texture, true)?;
            check_tex(m, "orm", &material.orm_texture, true)?;
            if !material.base_color_factor.iter().all(|c| c.is_finite()) {
                return Err(SceneError::NonFinite {
                    what: format!("material {} base colour factor", m),
                });
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
        s.materials[0].base_color_texture = Some(TextureRef::new(0));
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

    #[test]
    fn rejects_bad_images_and_srgb_data_textures() {
        let mut s = triangle_scene();
        s.images
            .push(ImageData::rgba8("x", 2, 2, ColorSpace::Srgb, vec![0; 15]));
        assert!(matches!(
            s.validate(),
            Err(SceneError::ImageSize { expected: 16, .. })
        ));
        s.images[0].pixels.push(0);
        s.materials[0].normal_texture = Some(TextureRef::new(0));
        assert!(matches!(
            s.validate(),
            Err(SceneError::ColorSpace { slot: "normal", .. })
        ));
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
