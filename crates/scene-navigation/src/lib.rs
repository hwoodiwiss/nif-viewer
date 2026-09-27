//! Renderer-independent navigation shared by the native and WASM hosts.
mod input;
mod rig;
mod runtime;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub use input::{Action, Bindings, GamepadSample, InputFrame, InputState};
pub use rig::{Bounds, Camera, Mode, NavigationSettings, Rig};
pub use runtime::CameraController;

pub const HELP: &str = "WASD: move • E/Q or Space/Ctrl: up/down • arrows/RMB drag: look • L: mouse capture • Shift/C: fast/precise • [/]: speed • F/Y: frame • Home/X: reset • O/View: Fly/Orbit • Enter/A: activate • Escape/B: release • H/Menu: help. Xbox: left stick move, right stick look, RT/LT up/down, RB/LB fast/precise, D-pad left/right speed.";
