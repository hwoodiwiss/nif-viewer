use crate::camera::normalize_or;
use crate::MeshData;

/// A mesh with every attribute present, ready for GPU upload.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub tangents: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// Fill missing attributes: smooth normals, `[0,0]` UVs, white colours and
/// UV-derived tangents. Supplied attributes are preserved. Assumes the mesh
/// passed [`crate::SceneData::validate`].
pub fn prepare_mesh(mesh: &MeshData) -> PreparedMesh {
    let n = mesh.positions.len();
    let normals = match &mesh.normals {
        Some(normals) => normals
            .iter()
            .map(|&v| normalize_or(v, [0.0, 1.0, 0.0]))
            .collect(),
        None => compute_normals(&mesh.positions, &mesh.indices),
    };
    let uvs = mesh.uv0.clone().unwrap_or_else(|| vec![[0.0, 0.0]; n]);
    let tangents = match &mesh.tangents {
        Some(t) => t.clone(),
        None => generate_tangents(&mesh.positions, &normals, &uvs, &mesh.indices),
    };
    PreparedMesh {
        positions: mesh.positions.clone(),
        normals,
        uvs,
        tangents,
        colors: mesh.colors.clone().unwrap_or_else(|| vec![[1.0; 4]; n]),
        indices: mesh.indices.clone(),
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn add_assign(a: &mut [f32; 3], b: [f32; 3]) {
    a[0] += b[0];
    a[1] += b[1];
    a[2] += b[2];
}

fn triangles(indices: &[u32], n: usize) -> impl Iterator<Item = [usize; 3]> + '_ {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
        .filter(move |t| t.iter().all(|&i| i < n))
}

/// Area-weighted smooth normals. Vertices without non-degenerate triangles get +Y.
pub fn compute_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; positions.len()];
    for [a, b, c] in triangles(indices, positions.len()) {
        let face = cross(
            sub(positions[b], positions[a]),
            sub(positions[c], positions[a]),
        );
        for i in [a, b, c] {
            add_assign(&mut normals[i], face);
        }
    }
    normals
        .into_iter()
        .map(|v| normalize_or(v, [0.0, 1.0, 0.0]))
        .collect()
}

/// Per-vertex tangents with handedness, accumulated per triangle (linear in
/// triangles + vertices) and Gram-Schmidt orthogonalised against the normal.
/// Triangles with degenerate UVs are skipped; vertices with no usable
/// triangle receive an arbitrary tangent perpendicular to the normal.
pub fn generate_tangents(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u32],
) -> Vec<[f32; 4]> {
    let n = positions.len();
    let mut tan = vec![[0.0f32; 3]; n];
    let mut bitan = vec![[0.0f32; 3]; n];
    for [a, b, c] in triangles(indices, n) {
        let e1 = sub(positions[b], positions[a]);
        let e2 = sub(positions[c], positions[a]);
        let du1 = uvs[b][0] - uvs[a][0];
        let dv1 = uvs[b][1] - uvs[a][1];
        let du2 = uvs[c][0] - uvs[a][0];
        let dv2 = uvs[c][1] - uvs[a][1];
        let det = du1 * dv2 - dv1 * du2;
        if !det.is_finite() || det.abs() < 1e-12 {
            continue;
        }
        let r = 1.0 / det;
        let t = [
            (e1[0] * dv2 - e2[0] * dv1) * r,
            (e1[1] * dv2 - e2[1] * dv1) * r,
            (e1[2] * dv2 - e2[2] * dv1) * r,
        ];
        let bt = [
            (e2[0] * du1 - e1[0] * du2) * r,
            (e2[1] * du1 - e1[1] * du2) * r,
            (e2[2] * du1 - e1[2] * du2) * r,
        ];
        if !t.iter().chain(bt.iter()).all(|v| v.is_finite()) {
            continue;
        }
        for i in [a, b, c] {
            add_assign(&mut tan[i], t);
            add_assign(&mut bitan[i], bt);
        }
    }
    (0..n)
        .map(|i| {
            let normal = normals[i];
            let t = tan[i];
            let d = dot(normal, t);
            let ortho = [
                t[0] - normal[0] * d,
                t[1] - normal[1] * d,
                t[2] - normal[2] * d,
            ];
            let len = dot(ortho, ortho).sqrt();
            let tangent = if len > 1e-8 && len.is_finite() {
                [ortho[0] / len, ortho[1] / len, ortho[2] / len]
            } else {
                any_perpendicular(normal)
            };
            let w = if dot(cross(normal, tangent), bitan[i]) < 0.0 {
                -1.0
            } else {
                1.0
            };
            [tangent[0], tangent[1], tangent[2], w]
        })
        .collect()
}

fn any_perpendicular(n: [f32; 3]) -> [f32; 3] {
    let axis = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let d = dot(n, axis);
    normalize_or(
        [axis[0] - n[0] * d, axis[1] - n[1] * d, axis[2] - n[2] * d],
        [1.0, 0.0, 0.0],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
        (
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0],
            ],
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            vec![0, 1, 2, 2, 1, 3],
        )
    }

    #[test]
    fn normals_follow_ccw_winding() {
        let (p, _, i) = quad();
        for n in compute_normals(&p, &i) {
            assert_eq!(n, [0.0, 0.0, 1.0]);
        }
    }

    #[test]
    fn tangents_follow_u_with_handedness() {
        let (p, uv, i) = quad();
        let n = compute_normals(&p, &i);
        for t in generate_tangents(&p, &n, &uv, &i) {
            assert_eq!(t, [1.0, 0.0, 0.0, 1.0]);
        }
        // Mirroring V flips handedness.
        let mirrored: Vec<_> = uv.iter().map(|u| [u[0], 1.0 - u[1]]).collect();
        for t in generate_tangents(&p, &n, &mirrored, &i) {
            assert_eq!(t, [1.0, 0.0, 0.0, -1.0]);
        }
    }

    #[test]
    fn degenerate_uvs_produce_finite_perpendicular_tangents() {
        let (p, _, i) = quad();
        let n = compute_normals(&p, &i);
        let flat = vec![[0.5, 0.5]; 4];
        for (t, n) in generate_tangents(&p, &n, &flat, &i).iter().zip(&n) {
            assert!(t.iter().all(|v| v.is_finite()));
            assert!(dot([t[0], t[1], t[2]], *n).abs() < 1e-6);
            assert_eq!(t[3].abs(), 1.0);
        }
    }

    #[test]
    fn supplied_attributes_are_preserved() {
        let (p, uv, i) = quad();
        let mesh = MeshData {
            positions: p,
            indices: i,
            uv0: Some(uv),
            tangents: Some(vec![[0.0, 1.0, 0.0, -1.0]; 4]),
            colors: Some(vec![[0.5, 0.25, 1.0, 1.0]; 4]),
            ..MeshData::default()
        };
        let prepared = prepare_mesh(&mesh);
        assert_eq!(prepared.tangents, vec![[0.0, 1.0, 0.0, -1.0]; 4]);
        assert_eq!(prepared.colors, vec![[0.5, 0.25, 1.0, 1.0]; 4]);
    }
}
