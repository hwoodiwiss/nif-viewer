use std::collections::HashMap;

use crate::RendererError;

/// Shader ABI implemented by this renderer: bind groups, vertex layouts,
/// entry points and render targets described in `shaders/*.wgsl`.
pub const SHADER_ABI_VERSION: u32 = 1;

/// Manifest file name inside a shader directory or base URL.
pub const MANIFEST_FILE: &str = "manifest.json";

/// Shader stages a [`ShaderSet`] must provide.
pub const REQUIRED_STAGES: [&str; 2] = ["geometry", "compose"];

/// Directory of the default shaders in this crate's source package.
///
/// Intended for development and asset staging; distributed hosts should load
/// a staged copy rather than rely on a build-machine path.
pub fn packaged_shader_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders")
}

/// Parsed `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderManifest {
    pub abi_version: u32,
    pub profile: String,
    /// Stage name -> file name relative to the manifest.
    pub files: HashMap<String, String>,
}

impl ShaderManifest {
    pub fn parse(text: &str) -> Result<Self, RendererError> {
        let bad = |m: &str| RendererError::Shader(format!("invalid shader manifest: {}", m));
        let json: serde_json::Value =
            serde_json::from_str(text).map_err(|e| bad(&e.to_string()))?;
        let abi_version = json["abi_version"]
            .as_u64()
            .ok_or_else(|| bad("missing abi_version"))? as u32;
        let profile = json["profile"].as_str().unwrap_or_default().to_string();
        let files = json["files"]
            .as_object()
            .ok_or_else(|| bad("missing files"))?
            .iter()
            .map(|(k, v)| {
                Ok((
                    k.clone(),
                    v.as_str()
                        .ok_or_else(|| bad("file names must be strings"))?
                        .to_string(),
                ))
            })
            .collect::<Result<_, RendererError>>()?;
        let manifest = Self {
            abi_version,
            profile,
            files,
        };
        manifest.check()?;
        Ok(manifest)
    }

    fn check(&self) -> Result<(), RendererError> {
        if self.abi_version != SHADER_ABI_VERSION {
            return Err(RendererError::Shader(format!(
                "shader ABI {} does not match renderer ABI {}",
                self.abi_version, SHADER_ABI_VERSION
            )));
        }
        for stage in REQUIRED_STAGES {
            if !self.files.contains_key(stage) {
                return Err(RendererError::Shader(format!(
                    "shader manifest lacks '{}'",
                    stage
                )));
            }
        }
        Ok(())
    }
}

/// Already-loaded WGSL sources. The renderer never reads files or URLs
/// itself; hosts load a manifest and its files with their own I/O.
#[derive(Debug, Clone)]
pub struct ShaderSet {
    pub profile: String,
    sources: HashMap<String, String>,
}

impl ShaderSet {
    /// Build from a manifest and stage sources (stage name -> WGSL).
    pub fn new(
        manifest: &ShaderManifest,
        sources: HashMap<String, String>,
    ) -> Result<Self, RendererError> {
        manifest.check()?;
        for stage in REQUIRED_STAGES {
            if !sources.contains_key(stage) {
                return Err(RendererError::Shader(format!(
                    "missing shader source for '{}'",
                    stage
                )));
            }
        }
        Ok(Self {
            profile: manifest.profile.clone(),
            sources,
        })
    }

    /// Load synchronously, resolving each manifest file through `read`.
    pub fn load_with(
        manifest_text: &str,
        mut read: impl FnMut(&str) -> Result<String, String>,
    ) -> Result<Self, RendererError> {
        let manifest = ShaderManifest::parse(manifest_text)?;
        let mut sources = HashMap::new();
        for (stage, file) in &manifest.files {
            let text = read(file)
                .map_err(|e| RendererError::Shader(format!("loading {}: {}", file, e)))?;
            sources.insert(stage.clone(), text);
        }
        Self::new(&manifest, sources)
    }

    pub fn source(&self, stage: &str) -> Option<&str> {
        self.sources.get(stage).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_abi_mismatch_and_missing_stage() {
        let err =
            ShaderManifest::parse(r#"{"abi_version":99,"files":{"geometry":"a","compose":"b"}}"#);
        assert!(err.unwrap_err().to_string().contains("ABI 99"));
        let err = ShaderManifest::parse(r#"{"abi_version":1,"files":{"geometry":"a"}}"#);
        assert!(err.unwrap_err().to_string().contains("compose"));
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
        assert_eq!(set.profile, "nif-pbr-compat");
        assert!(set.source("geometry").unwrap().contains("vertex_main"));
    }
}
