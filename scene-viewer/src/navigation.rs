//! Keyboard fly/orbit navigation for a look-at [`Camera`].

use cgmath::{InnerSpace, Point3, Vector3};
use scene_types::Camera;
use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{Key, NamedKey};

/// WASD orbits/dollies around the target, Space/Ctrl move vertically,
/// `[`/`]` halve/double the speed (world units per update).
#[derive(Debug, Clone)]
pub struct CameraController {
    speed: f32,
    up: bool,
    down: bool,
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
}

impl CameraController {
    pub fn new(speed: f32) -> Self {
        Self {
            speed: speed.max(1e-6),
            up: false,
            down: false,
            forward: false,
            backward: false,
            left: false,
            right: false,
        }
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.max(1e-6);
    }

    /// Returns true when the event was consumed.
    pub fn process_event(&mut self, event: &WindowEvent) -> bool {
        let WindowEvent::KeyboardInput { event, .. } = event else {
            return false;
        };
        let pressed = event.state == ElementState::Pressed;
        match &event.logical_key {
            Key::Named(NamedKey::Space) => self.up = pressed,
            Key::Named(NamedKey::Control) => self.down = pressed,
            Key::Character(c) => match c.as_str() {
                "w" => self.forward = pressed,
                "s" => self.backward = pressed,
                "a" => self.left = pressed,
                "d" => self.right = pressed,
                "[" if pressed => {
                    self.set_speed(self.speed * 0.5);
                    log::info!("camera speed: {}", self.speed);
                }
                "]" if pressed => {
                    self.set_speed(self.speed * 2.0);
                    log::info!("camera speed: {}", self.speed);
                }
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    pub fn update_camera(&self, camera: &mut Camera) {
        let target = Point3::from(camera.target);
        let up = Vector3::from(camera.up);
        let mut eye = Point3::from(camera.eye);

        let forward = target - eye;
        let forward_norm = forward.normalize();
        if self.forward && forward.magnitude() > self.speed {
            eye += forward_norm * self.speed;
        }
        if self.backward {
            eye -= forward_norm * self.speed;
        }

        let right = forward_norm.cross(up);
        let forward = target - eye;
        let forward_mag = forward.magnitude();
        if self.right {
            eye = target - (forward + right * self.speed).normalize() * forward_mag;
        }
        if self.left {
            eye = target - (forward - right * self.speed).normalize() * forward_mag;
        }
        if self.up {
            eye += up * self.speed;
        }
        if self.down {
            eye -= up * self.speed;
        }
        camera.eye = eye.into();
    }
}
