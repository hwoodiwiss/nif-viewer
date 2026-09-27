use crate::{rig::finite, NavigationSettings};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Forward,
    Backward,
    Left,
    Right,
    Up,
    Down,
    LookLeft,
    LookRight,
    LookUp,
    LookDown,
    Boost,
    Precision,
    Frame,
    Reset,
    ToggleMode,
    SpeedUp,
    SpeedDown,
    Activate,
    Release,
    Capture,
    Help,
    Screenshot,
}

pub type Bindings = HashMap<KeyCode, Action>;

pub fn default_bindings() -> Bindings {
    use Action::*;
    [
        (KeyCode::KeyW, Forward),
        (KeyCode::KeyS, Backward),
        (KeyCode::KeyA, Left),
        (KeyCode::KeyD, Right),
        (KeyCode::KeyE, Up),
        (KeyCode::Space, Up),
        (KeyCode::KeyQ, Down),
        (KeyCode::ControlLeft, Down),
        (KeyCode::ArrowLeft, LookLeft),
        (KeyCode::ArrowRight, LookRight),
        (KeyCode::ArrowUp, LookUp),
        (KeyCode::ArrowDown, LookDown),
        (KeyCode::ShiftLeft, Boost),
        (KeyCode::ShiftRight, Boost),
        (KeyCode::KeyC, Precision),
        (KeyCode::KeyF, Frame),
        (KeyCode::Home, Reset),
        (KeyCode::KeyO, ToggleMode),
        (KeyCode::BracketLeft, SpeedDown),
        (KeyCode::BracketRight, SpeedUp),
        (KeyCode::Enter, Activate),
        (KeyCode::Escape, Release),
        (KeyCode::KeyL, Capture),
        (KeyCode::KeyH, Help),
        (KeyCode::Backspace, Screenshot),
    ]
    .iter()
    .copied()
    .collect()
}

#[derive(Clone, Debug, Default)]
pub struct InputFrame {
    /// Right, up, forward.
    pub movement: [f32; 3],
    /// Yaw right and pitch up, radians/second.
    pub look: [f32; 2],
    pub mouse: [f32; 2],
    pub wheel: f32,
    pub boost: bool,
    pub precision: bool,
    pub commands: Vec<Action>,
}

/// Canonical Xbox/standard layout. Both sticks use right/up positive.
#[derive(Clone, Debug, Default, Serialize)]
pub struct GamepadSample {
    pub id: usize,
    pub name: String,
    pub left: [f32; 2],
    pub right: [f32; 2],
    pub triggers: [f32; 2],
    pub buttons: [bool; 16],
}

pub fn deadzone(value: [f32; 2], threshold: f32, exponent: f32) -> [f32; 2] {
    let x = finite(value[0]).clamp(-1.0, 1.0);
    let y = finite(value[1]).clamp(-1.0, 1.0);
    let m = x.hypot(y);
    if m <= threshold || m == 0.0 {
        return [0.0; 2];
    }
    let scale = ((m - threshold) / (1.0 - threshold))
        .min(1.0)
        .powf(exponent)
        / m;
    [x * scale, y * scale]
}

pub struct InputState {
    pub bindings: Bindings,
    pub active: bool,
    pub selected: Option<usize>,
    pub device: String,
    held: HashSet<KeyCode>,
    commands: Vec<Action>,
    mouse: [f32; 2],
    wheel: f32,
    previous: HashMap<usize, [bool; 16]>,
    armed: HashSet<usize>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            bindings: default_bindings(),
            active: false,
            selected: None,
            device: "Keyboard/mouse".into(),
            held: HashSet::new(),
            commands: vec![],
            mouse: [0.0; 2],
            wheel: 0.0,
            previous: HashMap::new(),
            armed: HashSet::new(),
        }
    }
}

impl InputState {
    pub fn clear(&mut self) {
        self.active = false;
        self.selected = None;
        self.held.clear();
        self.commands.clear();
        self.mouse = [0.0; 2];
        self.wheel = 0.0;
        self.armed.clear();
        self.previous.clear();
    }

    pub fn key(&mut self, key: KeyCode, pressed: bool, repeat: bool, synthetic: bool) -> bool {
        let Some(&action) = self.bindings.get(&key) else {
            return false;
        };
        if !pressed {
            self.held.remove(&key);
            return self.active;
        }
        if repeat || synthetic {
            return self.active;
        }
        if action == Action::Activate {
            self.active = true;
        }
        if !self.active {
            return false;
        }
        if self.held.insert(key) {
            self.commands.push(action);
        }
        self.device = "Keyboard/mouse".into();
        true
    }

    pub fn mouse(&mut self, x: f32, y: f32) {
        if self.active {
            if x != 0.0 || y != 0.0 {
                self.device = "Keyboard/mouse".into();
            }
            self.mouse[0] += finite(x);
            self.mouse[1] += finite(y);
        }
    }

    pub fn wheel(&mut self, value: f32) {
        if !self.active {
            return;
        }
        if self
            .held
            .iter()
            .any(|k| self.bindings.get(k) == Some(&Action::Boost))
        {
            self.commands.push(if value > 0.0 {
                Action::SpeedUp
            } else {
                Action::SpeedDown
            });
        } else {
            self.wheel += finite(value);
        }
    }

    pub fn command(&mut self, action: Action) {
        self.commands.push(action);
    }

    pub fn frame(
        &mut self,
        pads: &[GamepadSample],
        settings: &NavigationSettings,
        can_activate: bool,
    ) -> InputFrame {
        let mut frame = InputFrame::default();
        self.previous
            .retain(|id, _| pads.iter().any(|p| p.id == *id));
        self.armed.retain(|id| pads.iter().any(|p| p.id == *id));
        if self
            .selected
            .is_some_and(|id| !pads.iter().any(|p| p.id == id))
        {
            self.clear();
        }
        for pad in pads {
            let left = deadzone(pad.left, settings.left_deadzone, 1.0);
            let right = deadzone(pad.right, settings.right_deadzone, settings.look_exponent);
            let trigger = |v: f32| {
                ((finite(v) - settings.trigger_deadzone) / (1.0 - settings.trigger_deadzone))
                    .clamp(0.0, 1.0)
            };
            let triggers = [trigger(pad.triggers[0]), trigger(pad.triggers[1])];
            if !pad.buttons.iter().any(|v| *v)
                && left == [0.0; 2]
                && right == [0.0; 2]
                && triggers == [0.0; 2]
            {
                self.armed.insert(pad.id);
            }
            let previous = self
                .previous
                .insert(pad.id, pad.buttons)
                .unwrap_or(pad.buttons);
            if !self.armed.contains(&pad.id) {
                continue;
            }
            if self.selected.is_none() && can_activate && pad.buttons[0] && !previous[0] {
                self.active = true;
                self.selected = Some(pad.id);
                self.device = pad.name.clone();
            }
            if self.selected != Some(pad.id) || !self.active {
                continue;
            }
            frame.movement = [left[0], triggers[1] - triggers[0], left[1]];
            frame.look = [
                right[0] * settings.stick_look_speed,
                right[1]
                    * settings.stick_look_speed
                    * if settings.invert_stick_y { -1.0 } else { 1.0 },
            ];
            frame.boost = pad.buttons[5];
            frame.precision = pad.buttons[4];
            if left != [0.0; 2]
                || right != [0.0; 2]
                || triggers != [0.0; 2]
                || pad.buttons.iter().any(|v| *v)
            {
                self.device = pad.name.clone();
            }
            for (index, action) in [
                (1, Action::Release),
                (2, Action::Reset),
                (3, Action::Frame),
                (8, Action::ToggleMode),
                (9, Action::Help),
                (14, Action::SpeedDown),
                (15, Action::SpeedUp),
            ] {
                if pad.buttons[index] && !previous[index] {
                    self.commands.push(action);
                }
            }
        }
        if !self.active {
            frame.commands = std::mem::take(&mut self.commands);
            return frame;
        }
        let has = |action| {
            self.held
                .iter()
                .any(|k| self.bindings.get(k) == Some(&action)) as u8 as f32
        };
        frame.movement[0] += has(Action::Right) - has(Action::Left);
        frame.movement[1] += has(Action::Up) - has(Action::Down);
        frame.movement[2] += has(Action::Forward) - has(Action::Backward);
        frame.look[0] +=
            (has(Action::LookRight) - has(Action::LookLeft)) * settings.keyboard_look_speed;
        frame.look[1] +=
            (has(Action::LookUp) - has(Action::LookDown)) * settings.keyboard_look_speed;
        for value in &mut frame.look {
            *value = value.clamp(-20.0, 20.0);
        }
        frame.boost |= has(Action::Boost) > 0.0;
        frame.precision |= has(Action::Precision) > 0.0;
        frame.mouse = std::mem::take(&mut self.mouse);
        frame.wheel = std::mem::take(&mut self.wheel);
        frame.commands = std::mem::take(&mut self.commands);
        if frame.commands.contains(&Action::Release) {
            self.clear();
            return InputFrame {
                commands: vec![Action::Release],
                ..Default::default()
            };
        }
        frame
    }
}
