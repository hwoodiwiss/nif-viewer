//! Pragmatic Starfield `.mat` JSON texture extraction (spec §6).

use serde_json::Value;

/// Texture paths extracted from a Starfield `.mat` JSON blob, keyed by the
/// conventional filename suffixes (`_color`, `_normal`, `_rough`, `_metal`,
/// `_ao`). All paths are normalized lowercase with forward slashes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MatTextureSet {
    pub albedo: Option<String>,
    pub normal: Option<String>,
    pub rough: Option<String>,
    pub metal: Option<String>,
    pub ao: Option<String>,
}

/// Scan a Starfield `.mat` JSON blob for `FileName` string values, returning
/// the first path found for each conventional suffix (case-insensitive).
pub fn extract_mat_texture_set(json_bytes: &[u8]) -> MatTextureSet {
    let mut set = MatTextureSet::default();
    let value: Value = match serde_json::from_slice(json_bytes) {
        Ok(v) => v,
        Err(_) => return set,
    };
    scan(&value, &mut set);
    set
}

/// Back-compat wrapper: `(albedo, normal)` only.
pub fn extract_mat_textures(json_bytes: &[u8]) -> (Option<String>, Option<String>) {
    let set = extract_mat_texture_set(json_bytes);
    (set.albedo, set.normal)
}

fn all_found(set: &MatTextureSet) -> bool {
    set.albedo.is_some()
        && set.normal.is_some()
        && set.rough.is_some()
        && set.metal.is_some()
        && set.ao.is_some()
}

fn scan(value: &Value, set: &mut MatTextureSet) {
    if all_found(set) {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, v) in map {
                if key == "FileName" {
                    if let Value::String(s) = v {
                        let norm = s.to_lowercase().replace('\\', "/");
                        let slot = if norm.ends_with("_color.dds") {
                            &mut set.albedo
                        } else if norm.ends_with("_normal.dds") {
                            &mut set.normal
                        } else if norm.ends_with("_rough.dds") {
                            &mut set.rough
                        } else if norm.ends_with("_metal.dds") {
                            &mut set.metal
                        } else if norm.ends_with("_ao.dds") {
                            &mut set.ao
                        } else {
                            continue;
                        };
                        if slot.is_none() {
                            *slot = Some(norm);
                        }
                    }
                } else {
                    scan(v, set);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr {
                scan(v, set);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_full_texture_set_from_json() {
        let json = br#"{
            "Layers": [
                {"Tex": {"FileName": "Textures\\W\\Gun_Color.DDS"}},
                {"Tex": {"FileName": "textures/w/gun_normal.dds"}},
                {"Tex": {"FileName": "textures/w/gun_rough.dds"}},
                {"Tex": {"FileName": "textures/w/gun_metal.dds"}},
                {"Tex": {"FileName": "textures/w/gun_ao.dds"}},
                {"Tex": {"FileName": "textures/w/gun_emissive.dds"}}
            ]
        }"#;
        let set = extract_mat_texture_set(json);
        assert_eq!(set.albedo.as_deref(), Some("textures/w/gun_color.dds"));
        assert_eq!(set.normal.as_deref(), Some("textures/w/gun_normal.dds"));
        assert_eq!(set.rough.as_deref(), Some("textures/w/gun_rough.dds"));
        assert_eq!(set.metal.as_deref(), Some("textures/w/gun_metal.dds"));
        assert_eq!(set.ao.as_deref(), Some("textures/w/gun_ao.dds"));
    }

    #[test]
    fn invalid_json_returns_empty_set() {
        assert_eq!(extract_mat_texture_set(b"not json"), MatTextureSet::default());
    }
}
