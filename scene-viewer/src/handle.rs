//! Per-viewer command queue and status events.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use scene_renderer::{CapturedImage, UploadReport};
use scene_types::{Camera, DirectionalLight, SceneData};

/// Identifies a [`ViewerHandle::replace_scene`] request.
pub type LoadTicket = u64;

const MAX_PENDING_CAPTURES: usize = 4;
const MAX_EVENTS: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum ViewerState {
    Initializing,
    Ready,
    Suspended,
    DeviceLost(String),
    Failed(String),
    Disposed,
}

impl ViewerState {
    pub fn name(&self) -> &'static str {
        match self {
            ViewerState::Initializing => "initializing",
            ViewerState::Ready => "ready",
            ViewerState::Suspended => "suspended",
            ViewerState::DeviceLost(_) => "device-lost",
            ViewerState::Failed(_) => "failed",
            ViewerState::Disposed => "disposed",
        }
    }
}

#[derive(Debug, Clone)]
pub enum ViewerEvent {
    StateChanged(ViewerState),
    /// A queued scene was replaced by a newer request before upload.
    SceneSuperseded {
        ticket: LoadTicket,
    },
    /// The scene is uploaded and now current; the previous one was released.
    SceneUploaded {
        ticket: LoadTicket,
        report: UploadReport,
    },
    /// Upload failed; the previous scene remains visible.
    SceneFailed {
        ticket: LoadTicket,
        error: String,
    },
    /// The first frame containing the scene was submitted (not proof of
    /// presentation or visual correctness).
    FirstFrameSubmitted {
        ticket: LoadTicket,
        frame: u64,
    },
    CaptureReady(CapturedImage),
    CaptureFailed(String),
}

/// Directional light plus viewer-side animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightSettings {
    pub light: DirectionalLight,
    /// Rotate the direction 0.5° per frame about +Y.
    pub auto_orbit: bool,
}

impl Default for LightSettings {
    fn default() -> Self {
        Self {
            light: DirectionalLight::default(),
            auto_orbit: true,
        }
    }
}

pub(crate) struct PendingScene {
    pub ticket: LoadTicket,
    pub scene: SceneData,
    pub frame_camera: bool,
}

#[derive(Default)]
pub(crate) struct Commands {
    pub scene: Option<PendingScene>,
    pub camera: Option<Camera>,
    pub camera_speed: Option<f32>,
    pub light: Option<LightSettings>,
    pub captures: VecDeque<String>,
}

struct Shared {
    state: ViewerState,
    commands: Commands,
    events: VecDeque<ViewerEvent>,
    next_ticket: LoadTicket,
    ready_waiters: Vec<futures::channel::oneshot::Sender<Result<(), String>>>,
}

/// Cloneable handle for issuing commands to one viewer and observing it.
///
/// Commands issued before the viewer is ready are queued: settings coalesce
/// to the latest value, at most one scene is pending (older requests are
/// superseded) and capture requests are bounded.
#[derive(Clone)]
pub struct ViewerHandle {
    shared: Rc<RefCell<Shared>>,
}

impl Default for ViewerHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ViewerHandle {
    pub fn new() -> Self {
        Self {
            shared: Rc::new(RefCell::new(Shared {
                state: ViewerState::Initializing,
                commands: Commands::default(),
                events: VecDeque::new(),
                next_ticket: 1,
                ready_waiters: Vec::new(),
            })),
        }
    }

    pub fn state(&self) -> ViewerState {
        self.shared.borrow().state.clone()
    }

    /// Queue a scene to replace the current one. With `frame_camera`, the
    /// camera and navigation speed are fitted to the scene bounds.
    pub fn replace_scene(&self, scene: SceneData, frame_camera: bool) -> LoadTicket {
        let mut shared = self.shared.borrow_mut();
        let ticket = shared.next_ticket;
        shared.next_ticket += 1;
        if let Some(old) = shared.commands.scene.replace(PendingScene {
            ticket,
            scene,
            frame_camera,
        }) {
            push_event(
                &mut shared.events,
                ViewerEvent::SceneSuperseded { ticket: old.ticket },
            );
        }
        ticket
    }

    pub fn set_camera(&self, camera: Camera) {
        self.shared.borrow_mut().commands.camera = Some(camera);
    }

    pub fn set_camera_speed(&self, speed: f32) {
        self.shared.borrow_mut().commands.camera_speed = Some(speed);
    }

    pub fn set_light(&self, light: LightSettings) {
        self.shared.borrow_mut().commands.light = Some(light);
    }

    /// Capture the next presented frame. Returns false when too many
    /// captures are already pending.
    pub fn request_capture(&self, label: &str) -> bool {
        let mut shared = self.shared.borrow_mut();
        if shared.commands.captures.len() >= MAX_PENDING_CAPTURES {
            push_event(
                &mut shared.events,
                ViewerEvent::CaptureFailed("too many pending captures".into()),
            );
            return false;
        }
        shared.commands.captures.push_back(label.to_string());
        true
    }

    /// Take queued events (oldest first). At most 256 are retained.
    pub fn drain_events(&self) -> Vec<ViewerEvent> {
        self.shared.borrow_mut().events.drain(..).collect()
    }

    /// Resolves when the viewer is ready, or with an error if it fails.
    pub fn wait_ready(&self) -> impl std::future::Future<Output = Result<(), String>> {
        let (sender, receiver) = futures::channel::oneshot::channel();
        {
            let mut shared = self.shared.borrow_mut();
            match &shared.state {
                ViewerState::Ready | ViewerState::Suspended => {
                    let _ = sender.send(Ok(()));
                }
                ViewerState::Failed(e) | ViewerState::DeviceLost(e) => {
                    let _ = sender.send(Err(e.clone()));
                }
                ViewerState::Disposed => {
                    let _ = sender.send(Err("viewer disposed".into()));
                }
                ViewerState::Initializing => shared.ready_waiters.push(sender),
            }
        }
        async move {
            receiver
                .await
                .unwrap_or_else(|_| Err("viewer dropped".into()))
        }
    }

    pub(crate) fn set_state(&self, state: ViewerState) {
        let mut shared = self.shared.borrow_mut();
        if shared.state == state {
            return;
        }
        shared.state = state.clone();
        let result = match &state {
            ViewerState::Ready | ViewerState::Suspended => Some(Ok(())),
            ViewerState::Failed(e) | ViewerState::DeviceLost(e) => Some(Err(e.clone())),
            ViewerState::Disposed => Some(Err("viewer disposed".into())),
            ViewerState::Initializing => None,
        };
        if let Some(result) = result {
            for waiter in shared.ready_waiters.drain(..) {
                let _ = waiter.send(result.clone());
            }
        }
        push_event(&mut shared.events, ViewerEvent::StateChanged(state));
    }

    pub(crate) fn take_commands(&self) -> Commands {
        std::mem::take(&mut self.shared.borrow_mut().commands)
    }

    pub(crate) fn emit(&self, event: ViewerEvent) {
        push_event(&mut self.shared.borrow_mut().events, event);
    }
}

fn push_event(events: &mut VecDeque<ViewerEvent>, event: ViewerEvent) {
    if events.len() >= MAX_EVENTS {
        events.pop_front();
    }
    events.push_back(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_scene_supersedes_pending_one() {
        let handle = ViewerHandle::new();
        let first = handle.replace_scene(SceneData::default(), true);
        let second = handle.replace_scene(SceneData::default(), true);
        assert!(second > first);
        let events = handle.drain_events();
        assert!(
            matches!(events.as_slice(), [ViewerEvent::SceneSuperseded { ticket }] if *ticket == first)
        );
        assert_eq!(handle.take_commands().scene.unwrap().ticket, second);
    }

    #[test]
    fn captures_are_bounded() {
        let handle = ViewerHandle::new();
        for _ in 0..MAX_PENDING_CAPTURES {
            assert!(handle.request_capture("x"));
        }
        assert!(!handle.request_capture("x"));
    }

    #[test]
    fn ready_waiters_resolve_on_state_change() {
        let handle = ViewerHandle::new();
        let wait = handle.wait_ready();
        handle.set_state(ViewerState::Ready);
        assert_eq!(futures::executor::block_on(wait), Ok(()));
        let failed = ViewerHandle::new();
        let wait = failed.wait_ready();
        failed.set_state(ViewerState::Failed("boom".into()));
        assert_eq!(futures::executor::block_on(wait), Err("boom".into()));
    }
}
