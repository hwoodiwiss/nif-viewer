//! Bethesda NIF support for the scene renderer, independent of GPU, window
//! and browser code:
//!
//! * [`nif`] — NIF (20.2.0.7), Starfield `.mesh` and `.mat` parsing.
//! * [`load_session`] — staged dependency discovery and chunked file streaming.
//! * [`nif_model`] — geometry conversion (world transforms, Z-up → Y-up) and
//!   resource path helpers.
//! * [`nif_scene`] — NIF materials/textures → [`scene_types::SceneData`].
//! * [`textures`] — DDS decoding and ORM packing into plain RGBA8 pixels.
//! * [`materials`] — the `bethesda-pbr` material type (WGSL in `shaders/`).
//! * `disk` (native only) — resolve dependencies from an extracted data folder.

#[cfg(not(target_arch = "wasm32"))]
pub mod disk;
pub mod load_session;
pub mod materials;
pub mod nif;
pub mod nif_model;
pub mod nif_scene;
pub mod textures;

/// Opt-in local asset tests: `NIF_STARFIELD_DATA_ROOT` points at an extracted
/// Starfield data folder (`meshes/`, `geometries/`, `materials/`, `textures/`).
/// Tests using it are `#[ignore]`d; run them with `-- --ignored`.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn starfield_data_root() -> std::path::PathBuf {
    let root = std::env::var_os("NIF_STARFIELD_DATA_ROOT")
        .map(std::path::PathBuf::from)
        .expect("set NIF_STARFIELD_DATA_ROOT to an extracted Starfield data folder");
    assert!(root.is_dir(), "NIF_STARFIELD_DATA_ROOT is not a directory: {}", root.display());
    root
}

pub use nif_scene::{nif_to_scene, NifSceneData};
