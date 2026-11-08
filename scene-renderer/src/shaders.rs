use std::collections::{BTreeMap, HashMap};

use crate::RendererError;

/// Material ABI implemented by this renderer: the prelude's bind groups,
/// vertex/instance layout, entry points and output target, plus the
/// material binding convention in [`scene_types::MaterialTypeDesc`].
pub const SHADER_ABI_VERSION: u32 = 2;

/// Manifest file name inside a shader directory or base URL.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Stages a [`ShaderSet`] must provide to construct a [`crate::Renderer`].
pub const REQUIRED_STAGES: [&str; 2] = ["prelude", "compose"];

/// Directory of the renderer's own shaders (prelude, compose, stock
/// materials) in this crate's source package.
///
/// Intended for development and asset staging; distributed hosts should load
/// a staged copy rather than rely on a build-machine path.
pub fn packaged_shader_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders")
}

/// Parsed `manifest.json`. Renderer packages list `files` (stages); material
/// packages list `materials` (type key -> WGSL file); a package may have both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderManifest {
    pub abi_version: u32,
    /// Stage name -> file name relative to the manifest.
    pub files: BTreeMap<String, String>,
    /// Material type key -> file name relative to the manifest.
    pub materials: BTreeMap<String, String>,
}

impl ShaderManifest {
    pub fn parse(text: &str) -> Result<Self, RendererError> {
        let bad = |m: &str| RendererError::Shader(format!("invalid shader manifest: {}", m));
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
        let abi_version = json["abi_version"]
            .as_u64()
            .ok_or_else(|| bad("missing abi_version"))? as u32;
        let map = |name: &str| -> Result<BTreeMap<String, String>, RendererError> {
            match &json[name] {
                serde_json::Value::Null => Ok(BTreeMap::new()),
                serde_json::Value::Object(o) => o
                    .iter()
                    .map(|(k, v)| {
                        Ok((
                            k.clone(),
                            v.as_str()
                                .ok_or_else(|| bad("file names must be strings"))?
                                .to_string(),
                        ))
                    })
                    .collect(),
                _ => Err(bad(&format!("'{}' must be an object", name))),
            }
        };
        let manifest = Self {
            abi_version,
            files: map("files")?,
            materials: map("materials")?,
        };
        if manifest.abi_version != SHADER_ABI_VERSION {
            return Err(RendererError::Shader(format!(
                "shader ABI {} does not match renderer ABI {}",
                manifest.abi_version, SHADER_ABI_VERSION
            )));
        }
        Ok(manifest)
    }

    /// Every file the manifest references, for staging and fetching.
    pub fn all_files(&self) -> impl Iterator<Item = &String> {
        self.files.values().chain(self.materials.values())
    }
}

/// Already-loaded WGSL sources from one manifest. The renderer never reads
/// files or URLs itself; hosts load manifests and files with their own I/O.
#[derive(Debug, Clone, Default)]
pub struct ShaderSet {
    stages: HashMap<String, String>,
    materials: HashMap<String, String>,
}

impl ShaderSet {
    /// Load synchronously, resolving each manifest file through `read`.
    pub fn load_with(
        manifest_text: &str,
        mut read: impl FnMut(&str) -> Result<String, String>,
    ) -> Result<Self, RendererError> {
        let manifest = ShaderManifest::parse(manifest_text)?;
        let mut sources = HashMap::new();
        for file in manifest.all_files() {
            let text = read(file)
                .map_err(|e| RendererError::Shader(format!("loading {}: {}", file, e)))?;
            sources.insert(file.clone(), text);
        }
        Self::from_files(&manifest, &sources)
    }

    /// Build from a manifest and loaded file contents (file name -> WGSL).
    pub fn from_files(
        manifest: &ShaderManifest,
        files: &HashMap<String, String>,
    ) -> Result<Self, RendererError> {
        let resolve = |file: &String| {
            files
                .get(file)
                .cloned()
                .ok_or_else(|| RendererError::Shader(format!("missing shader file '{}'", file)))
        };
        Ok(Self {
            stages: manifest
                .files
                .iter()
                .map(|(k, f)| Ok((k.clone(), resolve(f)?)))
                .collect::<Result<_, RendererError>>()?,
            materials: manifest
                .materials
                .iter()
                .map(|(k, f)| Ok((k.clone(), resolve(f)?)))
                .collect::<Result<_, RendererError>>()?,
        })
    }

    pub fn source(&self, stage: &str) -> Option<&str> {
        self.stages.get(stage).map(String::as_str)
    }

    /// WGSL for a material type key, without the prelude.
    pub fn material_source(&self, key: &str) -> Option<&str> {
        self.materials.get(key).map(String::as_str)
    }

    pub(crate) fn require_stages(&self) -> Result<(), RendererError> {
        for stage in REQUIRED_STAGES {
            if !self.stages.contains_key(stage) {
                return Err(RendererError::Shader(format!(
                    "shader set lacks '{}'",
                    stage
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_abi_mismatch_and_missing_files() {
        let err = ShaderManifest::parse(r#"{"abi_version":99,"files":{}}"#);
        assert!(err.unwrap_err().to_string().contains("ABI 99"));
        let manifest =
            ShaderManifest::parse(r#"{"abi_version":2,"materials":{"x":"x.wgsl"}}"#).unwrap();
        assert!(ShaderSet::from_files(&manifest, &HashMap::new())
            .unwrap_err()
            .to_string()
            .contains("x.wgsl"));
    }

    #[test]
    fn material_only_sets_lack_renderer_stages() {
        let set = ShaderSet::load_with(r#"{"abi_version":2,"materials":{"m":"m.wgsl"}}"#, |_| {
            Ok("src".into())
        })
        .unwrap();
        assert_eq!(set.material_source("m"), Some("src"));
        assert!(set.require_stages().is_err());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn packaged_shaders_load() {
        let dir = packaged_shader_dir();
        let manifest = std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
        let set = ShaderSet::load_with(&manifest, |f| {
            std::fs::read_to_string(dir.join(f)).map_err(|e| e.to_string())
        })
        .unwrap();
        set.require_stages().unwrap();
        assert!(set
            .material_source(scene_types::BASIC_MATERIAL)
            .unwrap()
            .contains("fragment_main"));
    }
}
