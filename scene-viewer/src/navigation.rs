//! Keyboard, mouse and controller navigation for a look-at [`Camera`],
//! backed by the renderer-independent `scene-navigation` crate.

use std::sync::Arc;

use scene_types::Camera;
use winit::event::{DeviceEvent, WindowEvent};
use winit::window::Window;

pub use scene_navigation::{Bounds, Mode, NavigationSettings, HELP};

/// Adapts [`scene_navigation::CameraController`] to [`scene_types::Camera`].
pub struct CameraController {
    inner: scene_navigation::CameraController,
}

fn to_nav(camera: &Camera, aspect: f32) -> scene_navigation::Camera {
    scene_navigation::Camera {
        eye: camera.eye.into(),
        target: camera.target.into(),
        up: camera.up.into(),
        aspect,
        fov_y: camera.fov_y_degrees,
        z_near: camera.z_near,
        z_far: camera.z_far,
    }
}

fn from_nav(nav: &scene_navigation::Camera, camera: &mut Camera) {
    camera.eye = nav.eye.into();
    camera.target = nav.target.into();
    camera.up = nav.up.into();
    camera.fov_y_degrees = nav.fov_y;
    camera.z_near = nav.z_near;
    camera.z_far = nav.z_far;
}

impl CameraController {
    /// `legacy_speed` is in world units per frame at a 60 Hz reference.
    pub fn new(window: Arc<Window>, camera: &Camera, legacy_speed: f32) -> Self {
        let aspect = aspect_of(&window);
        Self {
            inner: scene_navigation::CameraController::new(
                window,
                &to_nav(camera, aspect),
                legacy_speed,
            ),
        }
    }

    pub fn speed(&self) -> f32 {
        self.inner.speed()
    }

    /// Legacy speed: converted to world units/second at a fixed 60 Hz reference.
    pub fn set_speed(&mut self, legacy_speed: f32) {
        self.inner.set_speed(legacy_speed);
    }

    pub fn automatic_speed(&self) -> bool {
        self.inner.automatic_speed()
    }

    pub fn configure(
        &mut self,
        settings: NavigationSettings,
        camera: &Camera,
        aspect: f32,
    ) -> Result<(), String> {
        self.inner.configure(settings, &to_nav(camera, aspect))
    }

    /// Frame (optionally) and remember a recoverable view for `bounds`.
    pub fn set_bounds(&mut self, bounds: Bounds, camera: &mut Camera, aspect: f32, frame: bool) {
        let mut nav = to_nav(camera, aspect);
        self.inner.set_bounds(bounds, &mut nav, frame);
        from_nav(&nav, camera);
    }

    pub fn deactivate(&mut self) {
        self.inner.deactivate();
    }

    /// Adopt a camera pose set outside navigation, e.g. by a host command.
    pub fn sync_pose(&mut self, camera: &Camera, aspect: f32) {
        self.inner.sync_pose(&to_nav(camera, aspect));
    }

    pub fn device_event(&mut self, event: &DeviceEvent) {
        self.inner.device_event(event);
    }

    /// Returns true when the event was consumed.
    pub fn process_event(&mut self, event: &WindowEvent) -> bool {
        self.inner.process_inputs(event)
    }

    /// Advance navigation. Returns true once when the screenshot action is pressed.
    pub fn update_camera(&mut self, camera: &mut Camera, aspect: f32) -> bool {
        let mut nav = to_nav(camera, aspect);
        let screenshot = self.inner.update_camera(&mut nav);
        from_nav(&nav, camera);
        screenshot
    }
}

pub(crate) fn aspect_of(window: &Window) -> f32 {
    let size = window.inner_size();
    if size.width == 0 || size.height == 0 {
        1.0
    } else {
        size.width as f32 / size.height as f32
    }
}
