use cgmath::{EuclideanSpace, InnerSpace, Matrix4, Point3, Vector3};
use serde::{Deserialize, Serialize};

use crate::{Action, InputFrame};

#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: Matrix4<f32> = Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
);

#[derive(Clone, Debug)]
pub struct Camera {
    pub eye: Point3<f32>,
    pub target: Point3<f32>,
    pub up: Vector3<f32>,
    pub aspect: f32,
    pub fov_y: f32,
    pub z_near: f32,
    pub z_far: f32,
}

impl Camera {
    pub fn build_view_projection_matrix(&self) -> Matrix4<f32> {
        OPENGL_TO_WGPU_MATRIX
            * cgmath::perspective(
                cgmath::Deg(self.fov_y),
                self.aspect,
                self.z_near,
                self.z_far,
            )
            * Matrix4::look_at_rh(self.eye, self.target, self.up)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Fly,
    Orbit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NavigationSettings {
    pub mode: Mode,
    /// World units per second; retained across loads when automatic_speed is false.
    pub speed: f32,
    pub automatic_speed: bool,
    /// Radians per pointer displacement unit.
    pub mouse_sensitivity: f32,
    pub keyboard_look_speed: f32,
    pub stick_look_speed: f32,
    pub left_deadzone: f32,
    pub right_deadzone: f32,
    pub trigger_deadzone: f32,
    pub look_exponent: f32,
    pub invert_mouse_y: bool,
    pub invert_stick_y: bool,
    pub boost: f32,
    pub precision: f32,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Fly,
            speed: 12.0,
            automatic_speed: true,
            mouse_sensitivity: 0.003,
            keyboard_look_speed: 1.5,
            stick_look_speed: 2.5,
            left_deadzone: 0.15,
            right_deadzone: 0.15,
            trigger_deadzone: 0.05,
            look_exponent: 2.0,
            invert_mouse_y: false,
            invert_stick_y: false,
            boost: 4.0,
            precision: 0.2,
        }
    }
}

impl NavigationSettings {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value, min, max) in [
            ("speed", self.speed, 1e-6, 1e9),
            ("mouse_sensitivity", self.mouse_sensitivity, 1e-6, 1.0),
            ("keyboard_look_speed", self.keyboard_look_speed, 0.01, 20.0),
            ("stick_look_speed", self.stick_look_speed, 0.01, 20.0),
            ("left_deadzone", self.left_deadzone, 0.0, 0.95),
            ("right_deadzone", self.right_deadzone, 0.0, 0.95),
            ("trigger_deadzone", self.trigger_deadzone, 0.0, 0.95),
            ("look_exponent", self.look_exponent, 1.0, 4.0),
            ("boost", self.boost, 1.0, 100.0),
            ("precision", self.precision, 0.001, 1.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("{name} must be finite and in {min}..={max}"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub center: Point3<f32>,
    pub radius: f32,
}

impl Bounds {
    pub fn new(center: [f32; 3], radius: f32) -> Option<Self> {
        if center.iter().all(|v| v.is_finite()) && radius.is_finite() && radius > 0.0 {
            Some(Self {
                center: center.into(),
                radius: radius.clamp(1e-5, 1e8),
            })
        } else {
            None
        }
    }
}

pub struct Rig {
    pub settings: NavigationSettings,
    yaw: f32,
    pitch: f32,
    pivot: Point3<f32>,
    distance: f32,
    bounds: Bounds,
    reset: Camera,
}

impl Rig {
    pub fn new(camera: &Camera, speed: f32) -> Self {
        let mut rig = Self {
            settings: NavigationSettings {
                speed,
                ..Default::default()
            },
            yaw: 0.0,
            pitch: 0.0,
            pivot: camera.target,
            distance: 1.0,
            bounds: Bounds {
                center: camera.target,
                radius: 1.0,
            },
            reset: camera.clone(),
        };
        rig.sync_pose(camera);
        rig
    }

    fn sync_pose(&mut self, camera: &Camera) {
        let delta = camera.target - camera.eye;
        self.distance = delta.magnitude().max(1e-5);
        let f = if delta.magnitude2() > 1e-10 {
            delta.normalize()
        } else {
            -Vector3::unit_z()
        };
        self.yaw = f.x.atan2(-f.z);
        self.pitch = f.y.clamp(-1.0, 1.0).asin().clamp(-1.562, 1.562);
        self.pivot = camera.eye + self.forward() * self.distance;
    }

    fn forward(&self) -> Vector3<f32> {
        Vector3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        )
    }

    pub fn set_mode(&mut self, mode: Mode, camera: &Camera) {
        if mode != self.settings.mode {
            self.pivot = camera.eye + self.forward() * self.distance;
            self.settings.mode = mode;
        }
    }

    pub fn configure(
        &mut self,
        settings: NavigationSettings,
        camera: &Camera,
    ) -> Result<(), String> {
        settings.validate()?;
        self.set_mode(settings.mode, camera);
        self.settings = settings;
        Ok(())
    }

    pub fn set_bounds(&mut self, bounds: Bounds, camera: &mut Camera, frame: bool) {
        self.bounds = bounds;
        if self.settings.automatic_speed {
            self.settings.speed = (bounds.radius * 1.2).clamp(1e-6, 1e9);
        }
        if frame {
            self.frame(camera);
        }
        self.reset = camera.clone();
    }

    pub fn frame(&mut self, camera: &mut Camera) {
        let vfov = camera.fov_y.to_radians() * 0.5;
        let hfov = (vfov.tan() * camera.aspect.max(0.01)).atan();
        self.distance = self.bounds.radius * 1.15 / vfov.min(hfov).sin().max(0.001);
        self.pivot = self.bounds.center;
        camera.eye = self.pivot - self.forward() * self.distance;
        camera.target = self.pivot;
        self.clipping(camera);
    }

    fn clipping(&self, camera: &mut Camera) {
        let distance = (camera.eye - self.bounds.center).magnitude();
        camera.z_near = (self.bounds.radius * 0.0001).clamp(1e-6, 1e4);
        camera.z_far = (distance + self.bounds.radius * 4.0).max(camera.z_near * 100.0);
    }

    pub fn update(&mut self, camera: &mut Camera, input: &InputFrame, dt: f32) {
        let dt = finite(dt).clamp(0.0, 0.05);
        for command in &input.commands {
            match command {
                Action::Frame => self.frame(camera),
                Action::Reset => {
                    *camera = self.reset.clone();
                    self.sync_pose(camera);
                }
                Action::ToggleMode => self.set_mode(
                    if self.settings.mode == Mode::Fly {
                        Mode::Orbit
                    } else {
                        Mode::Fly
                    },
                    camera,
                ),
                Action::SpeedUp => self.settings.speed = (self.settings.speed * 2.0).min(1e9),
                Action::SpeedDown => self.settings.speed = (self.settings.speed * 0.5).max(1e-6),
                _ => {}
            }
        }
        let s = &self.settings;
        let factor = if input.precision {
            s.precision
        } else if input.boost {
            s.boost
        } else {
            1.0
        };
        self.yaw += finite(input.look[0]) * dt + finite(input.mouse[0]) * s.mouse_sensitivity;
        self.yaw = (self.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        self.pitch = (self.pitch
            + finite(input.look[1]) * dt
            + finite(input.mouse[1])
                * s.mouse_sensitivity
                * if s.invert_mouse_y { -1.0 } else { 1.0 })
        .clamp(-1.562, 1.562);
        let forward = self.forward();
        let right = Vector3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let up = right.cross(forward);
        let movement = Vector3::new(
            finite(input.movement[0]),
            finite(input.movement[1]),
            finite(input.movement[2]),
        );
        if self.settings.mode == Mode::Fly {
            let velocity =
                right * movement.x + Vector3::unit_y() * movement.y + forward * movement.z;
            camera.eye +=
                velocity / velocity.magnitude().max(1.0) * self.settings.speed * factor * dt;
            camera.eye +=
                forward * finite(input.wheel).clamp(-20.0, 20.0) * self.settings.speed * 0.1;
            camera.target = camera.eye + forward * self.distance;
            self.pivot = camera.target;
        } else {
            // Preserve distance-scaled inspection while honoring the shared speed control.
            let orbit_speed = (self.settings.speed / (self.bounds.radius * 1.2)).clamp(1e-4, 1e4);
            let pan = right * movement.x + up * movement.y;
            self.pivot +=
                pan / pan.magnitude().max(1.0) * self.distance * factor * orbit_speed * dt;
            let exponent = (-movement.z * factor * orbit_speed * dt - finite(input.wheel) * 0.15)
                .clamp(-10.0, 10.0);
            self.distance = (self.distance * exponent.exp())
                .clamp(self.bounds.radius * 0.0001, self.bounds.radius * 1e5);
            camera.eye = self.pivot - forward * self.distance;
            camera.target = self.pivot;
        }
        camera.up = Vector3::unit_y();
        // Bound accumulated travel to keep the view matrix finite after malformed input.
        camera.eye = Point3::from_vec(camera.eye.to_vec().map(|v| v.clamp(-1e12, 1e12)));
        self.clipping(camera);
    }
}

pub(crate) fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}
