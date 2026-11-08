use cgmath::{Matrix4, Point3, Vector3};

#[rustfmt::skip]
const OPENGL_TO_WGPU_MATRIX: Matrix4<f32> = Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
);

/// Perspective look-at camera. The aspect ratio comes from the render view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_y_degrees: f32,
    pub z_near: f32,
    pub z_far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: [0.0, 1.0, 2.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_degrees: 45.0,
            z_near: 0.1,
            z_far: 1000.0,
        }
    }
}

impl Camera {
    /// Column-major view-projection with a 0..1 depth range.
    pub fn view_projection(&self, aspect: f32) -> [[f32; 4]; 4] {
        let view = Matrix4::look_at_rh(
            Point3::from(self.eye),
            Point3::from(self.target),
            Vector3::from(self.up),
        );
        let proj = cgmath::perspective(
            cgmath::Deg(self.fov_y_degrees),
            aspect,
            self.z_near,
            self.z_far,
        );
        (OPENGL_TO_WGPU_MATRIX * proj * view).into()
    }

    /// Camera looking at a bounding sphere from slightly above and in front (+Z).
    pub fn framing(center: [f32; 3], radius: f32) -> Self {
        let distance = radius * 2.5;
        Self {
            eye: [center[0], center[1] + distance * 0.35, center[2] + distance],
            target: center,
            z_far: (distance * 10.0).max(1000.0),
            ..Self::default()
        }
    }
}

/// Directional light. `direction` is the direction light travels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectionalLight {
    pub direction: [f32; 3],
    /// Linear RGB.
    pub colour: [f32; 3],
    pub intensity: f32,
    /// Ambient term, scaled by colour, albedo and occlusion.
    pub ambient: f32,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direction: normalize_or([-0.4, -0.8, -0.45], [0.0, -1.0, 0.0]),
            colour: [1.0, 1.0, 1.0],
            intensity: 8.0,
            ambient: 0.25,
        }
    }
}

impl DirectionalLight {
    pub fn normalized_direction(&self) -> [f32; 3] {
        normalize_or(self.direction, [0.0, -1.0, 0.0])
    }
}

pub(crate) fn normalize_or(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 && len.is_finite() {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        fallback
    }
}

/// Per-frame view inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewParams {
    pub camera: Camera,
    pub light: DirectionalLight,
    /// Linear RGBA clear colour.
    pub clear_color: [f64; 4],
}

impl Default for ViewParams {
    fn default() -> Self {
        Self {
            camera: Camera::default(),
            light: DirectionalLight::default(),
            clear_color: [0.0, 0.0, 0.0, 1.0],
        }
    }
}
