//! Shader asset adapters: the renderer core only accepts loaded sources.

use scene_renderer::{RendererError, ShaderSet, MANIFEST_FILE};

/// Load a staged shader directory containing `manifest.json`.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_shader_dir(dir: &std::path::Path) -> Result<ShaderSet, RendererError> {
    let manifest_path = dir.join(MANIFEST_FILE);
    let manifest = std::fs::read_to_string(&manifest_path)
        .map_err(|e| RendererError::Shader(format!("{}: {}", manifest_path.display(), e)))?;
    ShaderSet::load_with(&manifest, |file| {
        let path = dir.join(file);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {}", path.display(), e))
    })
}

/// Locate the renderer's shaders: `env_var` (when set), `<exe dir>/shaders`,
/// `./shaders`, then the renderer package's own `shaders` directory
/// (development builds).
#[cfg(not(target_arch = "wasm32"))]
pub fn find_shader_dir(env_var: Option<&str>) -> Option<std::path::PathBuf> {
    find_asset_dir(env_var, "shaders", &scene_renderer::packaged_shader_dir())
}

/// Locate a staged asset directory containing `manifest.json`: `env_var`
/// (when set), `<exe dir>/<staged>`, `./<staged>`, then `packaged`.
#[cfg(not(target_arch = "wasm32"))]
pub fn find_asset_dir(
    env_var: Option<&str>,
    staged: &str,
    packaged: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = env_var.and_then(std::env::var_os) {
        candidates.push(std::path::PathBuf::from(dir));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
    {
        candidates.push(exe_dir.join(staged));
    }
    candidates.push(std::path::PathBuf::from(staged));
    candidates.push(packaged.to_path_buf());
    candidates
        .into_iter()
        .find(|d| d.join(MANIFEST_FILE).is_file())
}

/// Fetch `manifest.json` and its files relative to `base_url` (which should
/// end with `/`).
#[cfg(target_arch = "wasm32")]
pub async fn fetch_shader_set(base_url: &str) -> Result<ShaderSet, RendererError> {
    async fn fetch_text(url: &str) -> Result<String, String> {
        use wasm_bindgen::JsCast;
        let window = web_sys::window().ok_or("no window")?;
        let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
            .await
            .map_err(|e| format!("{:?}", e))?;
        let response: web_sys::Response = response
            .dyn_into()
            .map_err(|_| "not a Response".to_string())?;
        if !response.ok() {
            return Err(format!("HTTP {}", response.status()));
        }
        let text =
            wasm_bindgen_futures::JsFuture::from(response.text().map_err(|e| format!("{:?}", e))?)
                .await
                .map_err(|e| format!("{:?}", e))?;
        text.as_string()
            .ok_or_else(|| "body is not text".to_string())
    }

    let join = |file: &str| format!("{}{}", base_url, file);
    let manifest = fetch_text(&join(MANIFEST_FILE))
        .await
        .map_err(|e| RendererError::Shader(format!("{}: {}", join(MANIFEST_FILE), e)))?;
    let parsed = scene_renderer::ShaderManifest::parse(&manifest)?;
    let mut files = std::collections::HashMap::new();
    for file in parsed.all_files() {
        let url = join(file);
        let text = fetch_text(&url)
            .await
            .map_err(|e| RendererError::Shader(format!("{}: {}", url, e)))?;
        files.insert(file.clone(), text);
    }
    ShaderSet::from_files(&parsed, &files)
}
