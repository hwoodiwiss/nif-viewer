//! Browser-only input lifecycle and low-frequency host APIs.
use crate::{Action, Bindings, Camera, GamepadSample, InputState, NavigationSettings};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    sync::Arc,
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{Event, EventTarget, HtmlCanvasElement};
use winit::{platform::web::WindowExtWebSys, window::Window};

#[derive(Default)]
pub struct Changes {
    pub clear: bool,
    pub locked: bool,
    pub mouse: [f32; 2],
    pub settings: Option<NavigationSettings>,
    pub bindings: Option<Bindings>,
    pub commands: Vec<Action>,
}
struct Shared {
    changes: Changes,
    attached: bool,
    generation: u32,
    active: bool,
    status: String,
    bindings: Bindings,
    gamepad_status: String,
}
thread_local! { static LIVE: RefCell<Weak<RefCell<Shared>>> = const { RefCell::new(Weak::new()) }; }

struct Listener {
    target: EventTarget,
    name: &'static str,
    callback: Closure<dyn FnMut(Event)>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self
            .target
            .remove_event_listener_with_callback(self.name, self.callback.as_ref().unchecked_ref());
    }
}
pub struct BrowserInput {
    canvas: HtmlCanvasElement,
    shared: Rc<RefCell<Shared>>,
    _listeners: Vec<Listener>,
}
impl BrowserInput {
    pub fn focus(&self) {
        let _ = self.canvas.focus();
    }
    pub fn new(window: &Arc<Window>) -> Self {
        let canvas = window.canvas().expect("winit canvas");
        let document = canvas.owner_document().expect("canvas document");
        let browser = web_sys::window().expect("browser window");
        let _ = canvas.set_attribute("tabindex", "0");
        let _ = canvas.set_attribute(
            "aria-label",
            "3D scene. Enter activates navigation; H shows controls; Escape releases navigation.",
        );
        window.set_prevent_default(false);
        let shared = Rc::new(RefCell::new(Shared {
            changes: Changes::default(),
            attached: true,
            generation: 1,
            active: false,
            status: "{}".into(),
            bindings: crate::input::default_bindings(),
            gamepad_status: "Press A on a controller to activate".into(),
        }));
        LIVE.with(|live| *live.borrow_mut() = Rc::downgrade(&shared));
        let mut listeners = vec![];
        let mut listen = |target: EventTarget, name, callback: Box<dyn FnMut(Event)>| {
            let callback = Closure::wrap(callback);
            target
                .add_event_listener_with_callback(name, callback.as_ref().unchecked_ref())
                .expect("input listener");
            listeners.push(Listener {
                target,
                name,
                callback,
            });
        };
        for (target, name) in [
            (browser.clone().unchecked_into::<EventTarget>(), "blur"),
            (document.clone().unchecked_into(), "visibilitychange"),
            (canvas.clone().unchecked_into(), "blur"),
        ] {
            let shared = shared.clone();
            listen(
                target,
                name,
                Box::new(move |_| {
                    shared.borrow_mut().changes.clear = true;
                }),
            );
        }
        {
            let shared = shared.clone();
            let canvas = canvas.clone();
            let document = document.clone();
            listen(
                document.clone().unchecked_into(),
                "pointerlockchange",
                Box::new(move |_| {
                    let locked = document
                        .pointer_lock_element()
                        .is_some_and(|e| e == canvas.clone().into());
                    let mut s = shared.borrow_mut();
                    s.changes.locked = locked;
                    if !locked {
                        s.changes.clear = true;
                    }
                }),
            );
        }
        {
            let shared = shared.clone();
            listen(
                document.clone().unchecked_into(),
                "pointerlockerror",
                Box::new(move |_| {
                    shared.borrow_mut().changes.locked = false;
                    log::warn!("Pointer lock denied; use right-button drag or keyboard look");
                }),
            );
        }
        {
            let shared = shared.clone();
            listen(
                document.clone().unchecked_into(),
                "mousemove",
                Box::new(move |event| {
                    let Some(event) = event.dyn_ref::<web_sys::MouseEvent>() else {
                        return;
                    };
                    let mut s = shared.borrow_mut();
                    if s.changes.locked && s.attached {
                        s.changes.mouse[0] += event.movement_x() as f32;
                        s.changes.mouse[1] -= event.movement_y() as f32;
                    }
                }),
            );
        }
        {
            let shared = shared.clone();
            let canvas = canvas.clone();
            let document = document.clone();
            listen(
                canvas.clone().unchecked_into(),
                "keydown",
                Box::new(move |event| {
                    let Some(event) = event.dyn_ref::<web_sys::KeyboardEvent>() else {
                        return;
                    };
                    if event.alt_key()
                        || event.meta_key()
                        || (event.ctrl_key() && event.code() != "ControlLeft")
                    {
                        return;
                    }
                    let mut s = shared.borrow_mut();
                    if !s.attached {
                        return;
                    }
                    let key: Option<winit::keyboard::KeyCode> =
                        serde_json::from_value(serde_json::Value::String(event.code())).ok();
                    let action = key.and_then(|key| s.bindings.get(&key).copied());
                    if action == Some(Action::Activate) {
                        s.active = true;
                    }
                    let active = s.active;
                    if event.code() == "Tab" {
                        return;
                    }
                    if active && action == Some(Action::Capture) && !event.repeat() {
                        if document.pointer_lock_element().is_some() {
                            document.exit_pointer_lock();
                        } else {
                            canvas.request_pointer_lock();
                        }
                    }
                    if active && action.is_some() {
                        event.prevent_default();
                    }
                }),
            );
        }
        {
            let canvas = canvas.clone();
            let shared = shared.clone();
            listen(
                canvas.clone().unchecked_into(),
                "mousedown",
                Box::new(move |_| {
                    shared.borrow_mut().active = true;
                    let _ = canvas.focus();
                }),
            );
        }
        for name in ["wheel", "contextmenu"] {
            let shared = shared.clone();
            listen(
                canvas.clone().unchecked_into(),
                name,
                Box::new(move |event| {
                    if shared.borrow().active {
                        event.prevent_default();
                    }
                }),
            );
        }
        Self {
            canvas,
            shared,
            _listeners: listeners,
        }
    }

    pub fn release(&self) {
        if let Some(document) = self.canvas.owner_document() {
            if document
                .pointer_lock_element()
                .is_some_and(|e| e == self.canvas.clone().into())
            {
                document.exit_pointer_lock();
            }
        }
    }

    pub fn drain(&self) -> Changes {
        let mut s = self.shared.borrow_mut();
        let locked = s.changes.locked;
        let mut changes = std::mem::take(&mut s.changes);
        s.changes.locked = locked;
        if !self.canvas.is_connected() || !s.attached {
            changes.clear = true;
        }
        changes
    }

    pub fn can_activate(&self) -> bool {
        let Some(document) = self.canvas.owner_document() else {
            return false;
        };
        self.shared.borrow().attached
            && self.canvas.is_connected()
            && !document.hidden()
            && document.has_focus().unwrap_or(false)
            && document
                .active_element()
                .is_some_and(|e| e == self.canvas.clone().into() || e.tag_name() == "BODY")
    }

    pub fn gamepads(&self) -> Vec<GamepadSample> {
        let Some(window) = web_sys::window() else {
            return vec![];
        };
        let Ok(pads) = window.navigator().get_gamepads() else {
            self.shared.borrow_mut().gamepad_status = "Gamepad API unavailable or blocked; use HTTPS/localhost and allow gamepad in embedded hosts".into();
            return vec![];
        };
        self.shared.borrow_mut().gamepad_status = "Press A on a controller to activate".into();
        pads.iter()
            .filter_map(|value| {
                let pad = value.dyn_into::<web_sys::Gamepad>().ok()?;
                if !pad.connected() || pad.mapping() != web_sys::GamepadMappingType::Standard {
                    self.shared.borrow_mut().gamepad_status =
                        format!("Unsupported controller mapping: {}", pad.id());
                    return None;
                }
                let axes = pad.axes();
                let buttons = pad.buttons();
                if axes.length() < 4 || buttons.length() < 16 {
                    self.shared.borrow_mut().gamepad_status =
                        format!("Incomplete controller mapping: {}", pad.id());
                    return None;
                }
                let axis = |i| axes.get(i).as_f64().unwrap_or(0.0) as f32;
                let button = |i| buttons.get(i).dyn_into::<web_sys::GamepadButton>().ok();
                let mut pressed = [false; 16];
                for (i, v) in pressed.iter_mut().enumerate() {
                    *v = button(i as u32).is_some_and(|b| b.pressed());
                }
                Some(GamepadSample {
                    id: pad.index() as usize,
                    name: pad.id(),
                    left: [axis(0), -axis(1)],
                    right: [axis(2), -axis(3)],
                    triggers: [
                        button(6).map_or(0.0, |b| b.value() as f32),
                        button(7).map_or(0.0, |b| b.value() as f32),
                    ],
                    buttons: pressed,
                })
            })
            .collect()
    }

    pub fn status(
        &self,
        settings: &NavigationSettings,
        input: &InputState,
        camera: &Camera,
        pads: &[GamepadSample],
    ) {
        let mut s = self.shared.borrow_mut();
        s.active = input.active;
        s.status = serde_json::json!({ "settings": settings, "active": input.active, "device": input.device, "captured": s.changes.locked, "eye": [camera.eye.x, camera.eye.y, camera.eye.z], "target": [camera.target.x, camera.target.y, camera.target.z], "help": crate::HELP, "gamepads": pads, "gamepadStatus": s.gamepad_status }).to_string();
    }
}

fn with_live<T>(f: impl FnOnce(&mut Shared) -> T) -> Result<T, JsValue> {
    LIVE.with(|live| {
        let shared = live
            .borrow()
            .upgrade()
            .ok_or_else(|| JsValue::from_str("Navigation is not ready"))?;
        let result = f(&mut shared.borrow_mut());
        Ok(result)
    })
}
pub fn settings(json: &str) -> Result<(), JsValue> {
    let settings: NavigationSettings =
        serde_json::from_str(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    settings.validate().map_err(|e| JsValue::from_str(&e))?;
    with_live(|s| s.changes.settings = Some(settings))
}
pub fn command(action: &str) -> Result<(), JsValue> {
    let action: Action = serde_json::from_value(serde_json::Value::String(action.into()))
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    with_live(|s| {
        s.changes.commands.push(action);
    })
}
pub fn bindings(json: &str) -> Result<(), JsValue> {
    let bindings: Bindings =
        serde_json::from_str(json).map_err(|e| JsValue::from_str(&e.to_string()))?;
    if bindings.contains_key(&winit::keyboard::KeyCode::Tab) {
        return Err(JsValue::from_str("Tab is reserved for leaving the canvas"));
    }
    with_live(|s| {
        s.bindings = bindings.clone();
        s.changes.bindings = Some(bindings);
    })
}
pub fn status() -> String {
    with_live(|s| s.status.clone()).unwrap_or_else(|_| "{}".into())
}
pub fn attach() -> Result<u32, JsValue> {
    with_live(|s| {
        s.attached = true;
        s.changes.clear = true;
        s.generation = s.generation.wrapping_add(1);
        s.generation
    })
}
pub fn detach(generation: u32) -> Result<(), JsValue> {
    with_live(|s| {
        if s.generation == generation {
            s.attached = false;
            s.changes.clear = true;
        }
    })
}
