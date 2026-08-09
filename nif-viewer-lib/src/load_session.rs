//! Session-based streaming NIF load: parse the NIF up front to learn which
//! dependency files are needed, then accept file bytes in chunks.
//!
//! Platform-neutral state machine (unit testable natively); the wasm entry
//! points in `lib.rs` are thin wrappers around [`LoadSession`].

use std::collections::{BTreeSet, HashMap};

use crate::nif;
use crate::nif_model::{material_texture_hints, normalize_path};

/// Heuristic texture paths for a required `.mat` that has not been provided.
/// Speculative: never counted in [`LoadSession::pending`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureHint {
    pub mat_path: String,
    pub color: String,
    pub normal: String,
    pub rough: String,
    pub metal: String,
    pub ao: String,
    pub stem: String,
}

/// In-progress streaming load of a NIF plus its dependency files.
pub struct LoadSession {
    name: String,
    nif_bytes: Vec<u8>,
    /// All dependency paths discovered so far (normalized).
    required: BTreeSet<String>,
    /// Completed files, keyed by normalized path.
    files: HashMap<String, Vec<u8>>,
    /// Files currently being streamed in chunks.
    in_progress: HashMap<String, Vec<u8>>,
}

impl LoadSession {
    /// Parse `nif_bytes` and start a session. Returns the session plus the
    /// initial list of required dependency paths (external `.mesh` geometry,
    /// `.mat` materials, and direct `.dds` textures), deduplicated and
    /// normalized (lowercase, forward slashes).
    pub fn begin(name: &str, nif_bytes: &[u8]) -> Result<(Self, Vec<String>), String> {
        let scene = nif::parse_nif(nif_bytes).map_err(|e| e.to_string())?;

        let mut required = BTreeSet::new();
        for instance in &scene.meshes {
            if let nif::Geometry::External { path, .. } = &instance.geometry {
                required.insert(normalize_path(path));
            }
            let mat = &instance.material;
            if let Some(mat_path) = &mat.mat_path {
                required.insert(normalize_path(mat_path));
            }
            for tex in mat.diffuse.iter().chain(mat.normal.iter()) {
                required.insert(normalize_path(tex));
            }
        }

        let initial: Vec<String> = required.iter().cloned().collect();
        Ok((
            Self {
                name: name.to_string(),
                nif_bytes: nif_bytes.to_vec(),
                required,
                files: HashMap::new(),
                in_progress: HashMap::new(),
            },
            initial,
        ))
    }

    /// Begin streaming a file, pre-allocating `size` bytes.
    pub fn file_begin(&mut self, path: &str, size: usize) {
        let norm = normalize_path(path);
        self.in_progress.insert(norm, Vec::with_capacity(size));
    }

    /// Append a chunk to a file started with [`Self::file_begin`].
    pub fn file_chunk(&mut self, path: &str, chunk: &[u8]) -> Result<(), String> {
        let norm = normalize_path(path);
        self.in_progress
            .get_mut(&norm)
            .ok_or_else(|| format!("no file in progress for '{norm}'"))?
            .extend_from_slice(chunk);
        Ok(())
    }

    /// Mark a file complete. Completed `.mat` files are scanned immediately
    /// for `.dds` texture references, which become newly required paths.
    pub fn file_end(&mut self, path: &str) -> Result<(), String> {
        let norm = normalize_path(path);
        let bytes = self
            .in_progress
            .remove(&norm)
            .ok_or_else(|| format!("no file in progress for '{norm}'"))?;

        if norm.ends_with(".mat") {
            let set = nif::extract_mat_texture_set(&bytes);
            let all = set
                .albedo
                .into_iter()
                .chain(set.normal)
                .chain(set.rough)
                .chain(set.metal)
                .chain(set.ao);
            for dds in all {
                self.required.insert(normalize_path(&dds));
            }
        }

        self.files.insert(norm, bytes);
        Ok(())
    }

    /// Required paths not yet provided (sorted, deduplicated).
    pub fn pending(&self) -> Vec<String> {
        self.required
            .iter()
            .filter(|p| !self.files.contains_key(*p))
            .cloned()
            .collect()
    }

    /// Heuristic texture hints for required `.mat` files not yet provided.
    /// Once a real `.mat` is streamed in, its hints disappear (the actual
    /// texture paths it references flow through [`Self::pending`] instead).
    /// Hint paths are speculative and never appear in [`Self::pending`], but
    /// files streamed under them are accepted and kept like any other file.
    pub fn texture_hints(&self) -> Vec<TextureHint> {
        self.required
            .iter()
            .filter(|p| p.ends_with(".mat") && !self.files.contains_key(*p))
            .map(|mat_path| {
                let h = material_texture_hints(mat_path);
                TextureHint {
                    mat_path: mat_path.clone(),
                    color: h.color,
                    normal: h.normal,
                    rough: h.rough,
                    metal: h.metal,
                    ao: h.ao,
                    stem: h.stem,
                }
            })
            .collect()
    }

    /// Consume the session, yielding (name, nif bytes, completed files).
    /// Missing dependencies are simply absent — rendering falls back to the
    /// default material.
    pub fn finish(self) -> (String, Vec<u8>, HashMap<String, Vec<u8>>) {
        (self.name, self.nif_bytes, self.files)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const SAMPLE_ROOT: &str = r"C:\Users\secro\Documents\StarfieldResources";

    fn sample_root() -> Option<PathBuf> {
        let p = PathBuf::from(SAMPLE_ROOT);
        p.exists().then_some(p)
    }

    #[test]
    fn session_streams_real_nif_dependencies() {
        let Some(root) = sample_root() else {
            eprintln!("sample dir missing, skipping");
            return;
        };
        let nif_bytes =
            std::fs::read(root.join(r"meshes\furniture\sit_chairleatherlowback01.nif")).unwrap();

        let (mut session, required) =
            LoadSession::begin("sit_chairleatherlowback01.nif", &nif_bytes).unwrap();
        assert!(!required.is_empty(), "expected required dependency paths");
        assert_eq!(session.pending(), required);

        // Stream every pending file that exists on disk, in chunks, looping
        // so .dds requirements discovered from .mat files get picked up.
        let mut streamed = 0usize;
        for _ in 0..4 {
            let pending = session.pending();
            let mut progressed = false;
            for path in pending {
                let disk = root.join(&path);
                let Ok(bytes) = std::fs::read(&disk) else {
                    continue;
                };
                session.file_begin(&path, bytes.len());
                for chunk in bytes.chunks(64 * 1024) {
                    session.file_chunk(&path, chunk).unwrap();
                }
                session.file_end(&path).unwrap();
                streamed += 1;
                progressed = true;
            }
            if !progressed {
                break;
            }
        }
        assert!(streamed > 0, "expected to stream at least one dependency");

        let mesh_paths: Vec<String> = required
            .iter()
            .filter(|p| p.ends_with(".mesh"))
            .cloned()
            .collect();

        let final_pending = session.pending();
        let (name, out_nif, files) = session.finish();
        assert_eq!(name, "sit_chairleatherlowback01.nif");
        assert_eq!(out_nif, nif_bytes);
        assert_eq!(files.len(), streamed);
        // Every streamed mesh made it into the map with the right bytes.
        for mesh in &mesh_paths {
            if final_pending.contains(mesh) {
                continue; // not on disk
            }
            let expected = std::fs::read(root.join(mesh)).unwrap();
            assert_eq!(files.get(mesh), Some(&expected), "mesh bytes mismatch");
        }
    }

    #[test]
    fn texture_hints_track_missing_mats_only() {
        let mut session = LoadSession {
            name: "t".into(),
            nif_bytes: vec![],
            required: BTreeSet::from([
                "materials/weapons/ar99/ar99_receiver.mat".to_string(),
                "geometries/ab/hash.mesh".to_string(),
            ]),
            files: HashMap::new(),
            in_progress: HashMap::new(),
        };

        let hints = session.texture_hints();
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0].mat_path, "materials/weapons/ar99/ar99_receiver.mat");
        assert_eq!(hints[0].color, "textures/weapons/ar99/ar99_receiver_color.dds");
        assert_eq!(hints[0].normal, "textures/weapons/ar99/ar99_receiver_normal.dds");
        assert_eq!(hints[0].stem, "ar99_receiver");

        // Hint paths are speculative: never part of pending().
        let pending = session.pending();
        assert!(!pending.contains(&hints[0].color));
        assert!(!pending.contains(&hints[0].normal));

        // Streaming a file under a hint path (not in `required`) is accepted,
        // lands in the final map, and leaves pending() unaffected.
        session.file_begin(&hints[0].color, 2);
        session.file_chunk(&hints[0].color, &[9, 9]).unwrap();
        session.file_end(&hints[0].color).unwrap();
        assert_eq!(session.pending(), pending);

        // Providing the real .mat removes its hint.
        session.file_begin("materials/weapons/ar99/ar99_receiver.mat", 0);
        session
            .file_end("materials/weapons/ar99/ar99_receiver.mat")
            .unwrap();
        assert!(session.texture_hints().is_empty());

        let (_, _, files) = session.finish();
        assert_eq!(
            files.get("textures/weapons/ar99/ar99_receiver_color.dds"),
            Some(&vec![9, 9])
        );
    }

    #[test]
    fn ar99_sample_generates_texture_hints() {
        let Some(root) = sample_root() else {
            eprintln!("sample dir missing, skipping");
            return;
        };
        let nif_bytes = std::fs::read(root.join(r"meshes\weapons\ar99\ar99.nif")).unwrap();
        let (session, _) = LoadSession::begin("ar99.nif", &nif_bytes).unwrap();
        let hints = session.texture_hints();
        assert!(!hints.is_empty(), "expected texture hints for ar99");
        let receiver = hints
            .iter()
            .find(|h| h.mat_path == "materials/weapons/ar99/ar99_receiver.mat")
            .expect("expected ar99_receiver hint");
        assert_eq!(receiver.stem, "ar99_receiver");
        // The dump drops the model prefix on texture stems: the exact hint
        // path is absent, but a suffix stem match ("receiver") exists.
        assert!(
            root.join(r"textures\weapons\ar99\receiver_color.dds").exists(),
            "expected suffix stem-match color texture on disk"
        );
    }

    #[test]
    fn chunk_and_end_require_begin() {
        let mut session = LoadSession {
            name: "t".into(),
            nif_bytes: vec![],
            required: BTreeSet::new(),
            files: HashMap::new(),
            in_progress: HashMap::new(),
        };
        assert!(session.file_chunk("a.mesh", &[1]).is_err());
        assert!(session.file_end("a.mesh").is_err());

        session.file_begin("Geometries\\AB\\Hash.MESH", 3);
        session.file_chunk("geometries/ab/hash.mesh", &[1, 2]).unwrap();
        session.file_chunk("geometries/ab/hash.mesh", &[3]).unwrap();
        session.file_end("geometries/ab/hash.mesh").unwrap();

        let (_, _, files) = session.finish();
        assert_eq!(files.get("geometries/ab/hash.mesh"), Some(&vec![1, 2, 3]));
    }

    #[test]
    fn completed_mat_reveals_dds_requirements() {
        let Some(root) = sample_root() else {
            eprintln!("sample dir missing, skipping");
            return;
        };
        // Find any .mat on disk that references at least one texture.
        let mut stack = vec![root.join("materials")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mat")) {
                    let bytes = std::fs::read(&path).unwrap();
                    let (albedo, normal) = nif::extract_mat_textures(&bytes);
                    if albedo.is_none() && normal.is_none() {
                        continue;
                    }
                    let mut session = LoadSession {
                        name: "t".into(),
                        nif_bytes: vec![],
                        required: BTreeSet::from(["materials/test.mat".to_string()]),
                        files: HashMap::new(),
                        in_progress: HashMap::new(),
                    };
                    session.file_begin("materials/test.mat", bytes.len());
                    session.file_chunk("materials/test.mat", &bytes).unwrap();
                    session.file_end("materials/test.mat").unwrap();
                    let pending = session.pending();
                    assert!(
                        pending.iter().any(|p| p.ends_with(".dds")),
                        "expected .dds requirement from .mat, got {:?}",
                        pending
                    );
                    return;
                }
            }
        }
        eprintln!("no texture-referencing .mat found, skipping");
    }
}
