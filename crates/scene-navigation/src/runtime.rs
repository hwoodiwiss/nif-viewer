#[cfg(not(target_arch = "wasm32"))]
use crate::GamepadSample;
use crate::{Action, Bounds, Camera, InputState, NavigationSettings, Rig, HELP};
use std::sync::Arc;
use web_time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use winit::window::CursorGrabMode;
use winit::{
    event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{ModifiersState, PhysicalKey},
    window::Window,
};

pub struct CameraController {
    window: Arc<Window>,
    pub input: InputState,
    pub rig: Rig,
    clock: Instant,
    focused: bool,
    visible: bool,
    dragging: bool,
    captured: bool,
    cursor: Option<(f64, f64)>,
    title_clock: Instant,
    modifiers: ModifiersState,
    diagnostics: bool,
    #[cfg(not(target_arch = "wasm32"))]
    gamepads: Option<gilrs::Gilrs>,
    #[cfg(target_arch = "wasm32")]
    browser: crate::web::BrowserInput,
}

impl CameraController {
    pub fn new(window: Arc<Window>, camera: &Camera, legacy_speed: f32) -> Self {
        log::info!("{HELP}");
        #[cfg(not(target_arch = "wasm32"))]
        let gamepads = match gilrs::GilrsBuilder::new()
            .with_default_filters(false)
            .build()
        {
            Ok(pads) => Some(pads),
            Err(error) => {
                log::warn!("Controller backend unavailable: {error}");
                None
            }
        };
        let rig = Rig::new(camera, legacy_speed * 60.0);
        #[cfg(not(target_arch = "wasm32"))]
        let rig = {
            let mut rig = rig;
            if let Ok(json) = std::env::var("SCENE_NAVIGATION") {
                match serde_json::from_str::<NavigationSettings>(&json) {
                    Ok(settings) => {
                        if let Err(error) = rig.configure(settings, camera) {
                            log::warn!("SCENE_NAVIGATION: {error}");
                        }
                    }
                    Err(error) => log::warn!("SCENE_NAVIGATION: {error}"),
                }
            }
            if let Some(speed) = std::env::var("SCENE_NAV_SPEED")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
            {
                rig.settings.speed = speed.clamp(1e-6, 1e9);
                rig.settings.automatic_speed = false;
            }
            rig
        };
        #[cfg(not(target_arch = "wasm32"))]
        let diagnostics = std::env::var_os("SCENE_NAV_DIAGNOSTICS").is_some();
        #[cfg(target_arch = "wasm32")]
        let diagnostics = false;
        Self {
            #[cfg(target_arch = "wasm32")]
            browser: crate::web::BrowserInput::new(&window),
            focused: window.has_focus(),
            window,
            input: InputState::default(),
            rig,
            clock: Instant::now(),
            visible: true,
            dragging: false,
            captured: false,
            cursor: None,
            title_clock: Instant::now(),
            modifiers: ModifiersState::empty(),
            diagnostics,
            #[cfg(not(target_arch = "wasm32"))]
            gamepads,
        }
    }

    pub fn speed(&self) -> f32 {
        self.rig.settings.speed / 60.0
    }
    pub fn set_speed(&mut self, legacy_speed: f32) {
        if legacy_speed.is_finite() && legacy_speed > 0.0 {
            self.rig.settings.speed = (legacy_speed * 60.0).clamp(1e-6, 1e9);
        }
    }
    pub fn automatic_speed(&self) -> bool {
        self.rig.settings.automatic_speed
    }
    pub fn configure(
        &mut self,
        settings: NavigationSettings,
        camera: &Camera,
    ) -> Result<(), String> {
        self.rig.configure(settings, camera)
    }
    pub fn set_bounds(&mut self, bounds: Bounds, camera: &mut Camera, frame: bool) {
        self.rig.set_bounds(bounds, camera, frame);
    }

    pub fn deactivate(&mut self) {
        self.input.clear();
        self.dragging = false;
        self.cursor = None;
        self.clock = Instant::now();
        self.modifiers = ModifiersState::empty();
        self.release_capture();
    }

    fn release_capture(&mut self) {
        self.captured = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
        }
        #[cfg(target_arch = "wasm32")]
        self.browser.release();
    }

    fn toggle_capture(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.captured {
                self.release_capture();
                return;
            }
            let result = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
            if result.is_ok() {
                self.captured = true;
                self.window.set_cursor_visible(false);
            } else {
                log::warn!("Mouse capture unavailable; use right-button drag");
            }
        }
        // Browser capture is requested synchronously in the trusted DOM key handler.
    }

    pub fn device_event(&mut self, event: &DeviceEvent) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.focused && self.input.active && self.captured {
            if let DeviceEvent::MouseMotion { delta } = event {
                self.input.mouse(delta.0 as f32, -delta.1 as f32);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = event;
    }

    pub fn process_inputs(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                if !focused {
                    self.deactivate();
                }
            }
            WindowEvent::Occluded(hidden) => {
                self.visible = !hidden;
                if *hidden {
                    self.deactivate();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => self.cursor = None,
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    // Releases still reach held-state tracking when a system shortcut is used.
                    if event.state.is_pressed()
                        && (self.modifiers.alt_key() || self.modifiers.super_key())
                    {
                        return false;
                    }
                    if self.focused {
                        return self.input.key(
                            key,
                            event.state.is_pressed(),
                            event.repeat,
                            *is_synthetic,
                        );
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if self.focused && *state == ElementState::Pressed {
                    self.input.active = true;
                }
                if *button == MouseButton::Right {
                    self.dragging = *state == ElementState::Pressed;
                    self.cursor = None;
                    return self.input.active;
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let next = (position.x, position.y);
                if self.dragging && !self.captured && self.focused {
                    if let Some(old) = self.cursor {
                        let scale = self.window.scale_factor();
                        self.input.mouse(
                            ((next.0 - old.0) / scale) as f32,
                            ((old.1 - next.1) / scale) as f32,
                        );
                    }
                }
                self.cursor = Some(next);
            }
            WindowEvent::MouseWheel { delta, .. } if self.focused => {
                self.input.wheel(match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 100.0) as f32,
                });
                return self.input.active;
            }
            _ => {}
        }
        false
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn poll_gamepads(&mut self) -> Vec<GamepadSample> {
        use gilrs::{Axis, Button};
        let Some(pads) = &mut self.gamepads else {
            return vec![];
        };
        while pads.next_event().is_some() {}
        pads.gamepads()
            .map(|(id, pad)| {
                let mut buttons = [false; 16];
                for (i, button) in [
                    (0, Button::South),
                    (1, Button::East),
                    (2, Button::West),
                    (3, Button::North),
                    (4, Button::LeftTrigger),
                    (5, Button::RightTrigger),
                    (8, Button::Select),
                    (9, Button::Start),
                    (14, Button::DPadLeft),
                    (15, Button::DPadRight),
                ] {
                    buttons[i] = pad.is_pressed(button);
                }
                GamepadSample {
                    id: usize::from(id),
                    name: pad.name().into(),
                    left: [pad.value(Axis::LeftStickX), pad.value(Axis::LeftStickY)],
                    right: [pad.value(Axis::RightStickX), pad.value(Axis::RightStickY)],
                    triggers: [
                        pad.button_data(Button::LeftTrigger2)
                            .map_or(0.0, |b| b.value()),
                        pad.button_data(Button::RightTrigger2)
                            .map_or(0.0, |b| b.value()),
                    ],
                    buttons,
                }
            })
            .collect()
    }

    /// Returns true once when the screenshot action is pressed.
    pub fn update_camera(&mut self, camera: &mut Camera) -> bool {
        let now = Instant::now();
        let dt = now.duration_since(self.clock).as_secs_f32();
        self.clock = now;
        #[cfg(not(target_arch = "wasm32"))]
        let (pads, can_activate) = (self.poll_gamepads(), self.focused && self.visible);
        #[cfg(not(target_arch = "wasm32"))]
        if !can_activate {
            self.deactivate();
        }
        #[cfg(target_arch = "wasm32")]
        let (pads, can_activate) = {
            if !self.browser.can_activate() {
                self.deactivate();
            }
            let changes = self.browser.drain();
            if changes.clear {
                self.deactivate();
            }
            self.captured = changes.locked;
            if let Some(settings) = changes.settings {
                if let Err(e) = self.configure(settings, camera) {
                    log::warn!("{e}");
                }
            }
            if let Some(bindings) = changes.bindings {
                self.input.clear();
                self.input.bindings = bindings;
            }
            for action in changes.commands {
                self.input.command(action);
            }
            self.input.mouse(changes.mouse[0], changes.mouse[1]);
            (self.browser.gamepads(), self.browser.can_activate())
        };
        let input = self.input.frame(&pads, &self.rig.settings, can_activate);
        #[cfg(target_arch = "wasm32")]
        if self.input.selected.is_some() && self.input.active {
            self.browser.focus();
        }
        for action in &input.commands {
            match action {
                Action::Release => self.deactivate(),
                Action::Capture => self.toggle_capture(),
                Action::Help => log::info!("{HELP}"),
                _ => {}
            }
        }
        self.rig
            .update(camera, &input, if can_activate { dt } else { 0.0 });
        #[cfg(target_arch = "wasm32")]
        self.browser
            .status(&self.rig.settings, &self.input, camera, &pads);
        if now.duration_since(self.title_clock).as_secs_f32() >= 0.5 {
            self.title_clock = now;
            if self.diagnostics {
                log::info!(
                    "navigation pads={} input={input:?} eye={:?} captured={}",
                    serde_json::to_string(&pads).unwrap_or_default(),
                    camera.eye,
                    self.captured
                );
            }
            #[cfg(not(target_arch = "wasm32"))]
            self.window.set_title(&format!(
                "Scene viewer | {:?} | {:.3} units/s | {} | {} | H: help",
                self.rig.settings.mode,
                self.rig.settings.speed,
                self.input.device,
                if self.input.active {
                    "active"
                } else {
                    "Enter/A to activate"
                }
            ));
        }
        input.commands.contains(&Action::Screenshot)
    }
}

impl Drop for CameraController {
    fn drop(&mut self) {
        self.release_capture();
    }
}
