//! Native dependency resolution from an extracted game data directory.

use crate::nif;

/// Map from normalized resource path to file bytes.
pub type ResourceFiles = std::collections::HashMap<String, Vec<u8>>;

/// Read a .nif from disk and resolve its dependencies (external `.mesh`
/// geometry, `.mat` materials and `.dds` textures) by walking up from the
/// nif's location to find a data root containing `geometries` / `textures` /
/// `materials` folders.
pub fn load_nif_with_dependencies(
    nif_path: &std::path::Path,
) -> anyhow::Result<(Vec<u8>, ResourceFiles)> {
    use anyhow::Context;
    use std::collections::HashMap;

    let nif_bytes =
        std::fs::read(nif_path).with_context(|| format!("reading {}", nif_path.display()))?;

    let scene = nif::parse_nif(&nif_bytes).map_err(|e| anyhow::anyhow!("{}", e))?;

    // Walk up from the nif to find the data root.
    let mut root = None;
    let mut dir = nif_path.parent();
    while let Some(d) = dir {
        if ["geometries", "textures", "materials"]
            .iter()
            .any(|sub| d.join(sub).is_dir())
        {
            root = Some(d.to_path_buf());
            break;
        }
        dir = d.parent();
    }
    let Some(root) = root else {
        log::warn!(
            "no data root (geometries/textures/materials) found above {}",
            nif_path.display()
        );
        return Ok((nif_bytes, HashMap::new()));
    };
    log::info!("using data root: {}", root.display());

    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    // Lazily built stem → color-dds-path index of <root>/textures.
    let mut stem_index: Option<HashMap<String, std::path::PathBuf>> = None;
    let read_into = |files: &mut HashMap<String, Vec<u8>>, rel: &str| -> bool {
        if files.contains_key(rel) {
            return true;
        }
        let full_path = root.join(rel);
        match std::fs::read(&full_path) {
            Ok(bytes) => {
                files.insert(rel.to_string(), bytes);
                true
            }
            Err(_) => {
                log::warn!("dependency not found on disk: {}", full_path.display());
                false
            }
        }
    };

    for instance in &scene.meshes {
        if let nif::Geometry::External { path, .. } = &instance.geometry {
            read_into(&mut files, path);
        }

        let mat = &instance.material;
        let mut dds_paths: Vec<String> = Vec::new();
        if let Some(mat_path) = &mat.mat_path {
            if read_into(&mut files, mat_path) {
                let set = nif::extract_mat_texture_set(&files[mat_path]);
                dds_paths.extend(set.albedo);
                dds_paths.extend(set.normal);
                dds_paths.extend(set.rough);
                dds_paths.extend(set.metal);
                dds_paths.extend(set.ao);
            } else {
                // No loose .mat (vanilla keeps them in a binary cdb): try
                // heuristic mirrored texture paths, then a stem search.
                resolve_heuristic_textures(&root, mat_path, &mut files, &mut stem_index);
            }
        }
        dds_paths.extend(mat.diffuse.clone());
        dds_paths.extend(mat.normal.clone());
        for dds in dds_paths {
            let norm = dds.to_lowercase().replace('\\', "/");
            read_into(&mut files, &norm);
        }
    }

    Ok((nif_bytes, files))
}

/// Heuristic texture discovery for a missing `.mat`: try the exact mirrored
/// `textures/.../<stem>_{color,normal}.dds` paths on disk; if absent, search
/// a (lazily built) stem index of `<root>/textures` for the best fuzzy match
/// (exact stem, then prefix either direction, then `_`-boundary suffix — the
/// dumps often drop a model prefix, e.g. `ar99_receiver` → `receiver`). Found
/// files are inserted under the hint path keys so the material fallback in
/// `nif_scene` picks them up.
fn resolve_heuristic_textures(
    root: &std::path::Path,
    mat_path: &str,
    files: &mut ResourceFiles,
    stem_index: &mut Option<std::collections::HashMap<String, std::path::PathBuf>>,
) {
    let hints = crate::nif_model::material_texture_hints(mat_path);
    if files.contains_key(&hints.color) || files.contains_key(&hints.normal) {
        return;
    }

    // Given the on-disk _color.dds path, also pick up sibling _normal/_rough/
    // _metal/_ao maps and register them under the corresponding hint paths.
    let insert_set = |color_disk: &std::path::Path, files: &mut ResourceFiles| -> bool {
        let Ok(bytes) = std::fs::read(color_disk) else {
            return false;
        };
        files.insert(hints.color.clone(), bytes);
        let color_str = color_disk.to_string_lossy().to_lowercase();
        for (suffix, key) in [
            ("_normal.dds", &hints.normal),
            ("_rough.dds", &hints.rough),
            ("_metal.dds", &hints.metal),
            ("_ao.dds", &hints.ao),
        ] {
            let sibling = std::path::PathBuf::from(color_str.replace("_color.dds", suffix));
            if let Ok(bytes) = std::fs::read(&sibling) {
                files.insert(key.clone(), bytes);
            }
        }
        true
    };

    // Exact mirrored paths first.
    if insert_set(&root.join(&hints.color), files) {
        log::info!(
            "heuristic textures (exact) for {}: {}",
            mat_path,
            hints.color
        );
        return;
    }

    // Fall back to a fuzzy stem match against an index of all *_color.dds.
    let index = stem_index.get_or_insert_with(|| build_color_stem_index(root));
    let mut best: Option<(u64, &String, &std::path::PathBuf)> = None;
    for (cand, path) in index.iter() {
        let Some(score) = stem_match_score(&hints.stem, cand) else {
            continue;
        };
        if best.is_none_or(|(s, _, _)| score > s) {
            best = Some((score, cand, path));
        }
    }
    if let Some((_, cand, path)) = best {
        if insert_set(&path.clone(), files) {
            log::info!(
                "heuristic textures (stem '{}' ~ '{}') for {}: {}",
                hints.stem,
                cand,
                mat_path,
                path.display()
            );
            return;
        }
    }
    log::warn!("no heuristic textures found for {}", mat_path);
}

/// Score a candidate texture stem against a material stem. Higher is better;
/// `None` means no match. Tiers: exact (3), prefix either direction (2,
/// tie-break on common prefix length), `_`-boundary suffix (1, tie-break on
/// candidate length).
fn stem_match_score(hint_stem: &str, candidate: &str) -> Option<u64> {
    const TIER: u64 = 1 << 32;
    if candidate.is_empty() {
        return None;
    }
    if candidate == hint_stem {
        return Some(3 * TIER);
    }
    // Prefix either direction; require a meaningful shared prefix so tiny
    // candidates (e.g. a bare "_color.dds") never win.
    if (candidate.starts_with(hint_stem) || hint_stem.starts_with(candidate))
        && hint_stem.len().min(candidate.len()) >= 4
    {
        let common = hint_stem.len().min(candidate.len()) as u64;
        return Some(2 * TIER + common);
    }
    if hint_stem.ends_with(&format!("_{}", candidate)) {
        return Some(TIER + candidate.len() as u64);
    }
    None
}

/// Index `<root>/textures` recursively: `_color.dds` filename stem → path.
/// Capped at 30000 files to bound the walk on huge dumps.
fn build_color_stem_index(
    root: &std::path::Path,
) -> std::collections::HashMap<String, std::path::PathBuf> {
    let mut index = std::collections::HashMap::new();
    let mut seen = 0usize;
    let mut stack = vec![root.join("textures")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            seen += 1;
            if seen > 30000 {
                log::warn!("texture stem index capped at 30000 files; matches may be incomplete");
                return index;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if let Some(stem) = name.strip_suffix("_color.dds") {
                index.entry(stem.to_string()).or_insert(path);
            }
        }
    }
    log::info!("texture stem index: {} color textures", index.len());
    index
}

#[cfg(test)]
mod stem_tests {
    use super::stem_match_score;

    #[test]
    fn stem_scoring_prefers_exact_then_prefix_then_suffix() {
        let exact = stem_match_score("ar99_mag", "ar99_mag").unwrap();
        let prefix = stem_match_score("ar99_mag", "ar99_magsmall").unwrap();
        let suffix = stem_match_score("ar99_receiver", "receiver").unwrap();
        assert!(exact > prefix && prefix > suffix);

        // Empty / tiny candidates never match via the prefix rule.
        assert!(stem_match_score("ar99_receiver", "").is_none());
        assert!(stem_match_score("ar99_receiver", "ar9").is_none());
        assert!(stem_match_score("scope", "barrel").is_none());
    }
}
