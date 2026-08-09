use crate::camera::Camera;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniforms {
	view_position: [f32; 4],
	view_proj: [[f32; 4]; 4],
	/// x: 1.0 = apply manual gamma encode in the shader (non-srgb surface),
	/// 0.0 = surface/attachment formats are srgb and encode on write.
	/// y/z/w: unused padding.
	params: [f32; 4],
}

impl Uniforms {
	pub fn new() -> Self {
		use cgmath::SquareMatrix;
		Self {
			view_position: [0.0; 4],
			view_proj: cgmath::Matrix4::identity().into(),
			params: [0.0; 4],
		}
	}

	pub fn set_manual_gamma(&mut self, manual: bool) {
		self.params[0] = if manual { 1.0 } else { 0.0 };
	}

	pub fn update_view_proj(&mut self, camera: &Camera) {
		self.view_position = camera.eye.to_homogeneous().into();
		self.view_proj = camera.build_view_projection_matrix().into();
	}
}
