/// Directional light uniform. WGSL layout: vec3 fields are 16-byte aligned,
/// so the trailing f32s pack into the padding slots exactly.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Light {
    /// Normalized direction the light travels (towards the scene).
    pub direction: [f32; 3],
    pub intensity: f32,
    pub colour: [f32; 3],
    pub ambient: f32,
}

/// CPU-side light configuration, converted to [`Light`] each frame.
#[derive(Debug, Clone)]
pub struct LightSettings {
    pub direction: [f32; 3],
    pub colour: [f32; 3],
    pub intensity: f32,
    pub ambient: f32,
    /// Slowly orbit the direction about the Y axis.
    pub auto_orbit: bool,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self {
            // The BRDF divides diffuse by PI and Reinhard compresses highlights,
            // so a "1.0-feeling" key light needs intensity well above 1.
            direction: normalize([-0.4, -0.8, -0.45]),
            colour: [1.0, 1.0, 1.0],
            intensity: 8.0,
            ambient: 0.25,
            auto_orbit: true,
        }
    }
}

pub fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, -1.0, 0.0]
    }
}

impl LightSettings {
    pub fn to_uniform(&self) -> Light {
        Light {
            direction: normalize(self.direction),
            intensity: self.intensity,
            colour: self.colour,
            ambient: self.ambient,
        }
    }
}
