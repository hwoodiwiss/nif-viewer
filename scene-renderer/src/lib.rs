//! wgpu renderer for [`scene_types`] scenes.
//!
//! The renderer owns GPU resources and pipelines for one [`GpuContext`] and
//! encodes frames into caller-owned command encoders. It has no window,
//! event loop, file system or browser access: hosts supply a [`ShaderSet`]
//! and target texture views, then submit and present themselves.
//!
//! Frame sequence:
//!
//! 1. [`Renderer::encode`] writes per-view uniforms and encodes the geometry
//!    and compose passes into the caller's encoder.
//! 2. Optional [`Renderer::encode_capture`] copies are encoded into the same
//!    encoder, so they observe that frame's results.
//! 3. The host submits once, presents if applicable, then maps captures.
//!
//! Per-view uniform writes use `Queue::write_buffer`, which takes effect at
//! the next submission: submit after each `encode` of the same view.

mod capture;
mod context;
mod renderer;
mod shaders;

pub use capture::{CapturedImage, PendingCapture};
pub use context::{default_backends, Capabilities, ContextDesc, GpuContext};
pub use renderer::{
    CaptureSource, FrameReceipt, RenderView, Renderer, RendererDesc, SceneId, SceneStats,
    UploadReport, ViewDesc, SCENE_COLOR_FORMAT,
};
pub use shaders::{
    packaged_shader_dir, ShaderManifest, ShaderSet, MANIFEST_FILE, REQUIRED_STAGES,
    SHADER_ABI_VERSION,
};

pub use scene_types;
pub use wgpu;

#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("no compatible GPU adapter: {0}")]
    NoAdapter(String),
    #[error("device request failed: {0}")]
    Device(String),
    #[error("shader assets: {0}")]
    Shader(String),
    #[error("invalid scene: {0}")]
    Scene(#[from] scene_types::SceneError),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
    #[error("exceeds device limits: {0}")]
    Limits(String),
    #[error("stale or foreign scene handle {0:?}")]
    InvalidHandle(SceneId),
    #[error("render view is suspended (zero size)")]
    Suspended,
    #[error("capture failed: {0}")]
    Capture(String),
}
