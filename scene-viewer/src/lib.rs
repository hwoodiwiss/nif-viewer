//! Optional interactive host for [`scene_renderer`].
//!
//! * [`Viewer`] owns a surface presenter, renderer, view and navigation for
//!   one window/canvas and can be driven by any winit event loop.
//! * [`ViewerHandle`] queues scene/camera/light/capture commands (also before
//!   the viewer is ready) and reports lifecycle and load events.
//! * [`ViewerApp`] is a convenience single-window winit runner.
//!
//! Nothing here initialises loggers, exits the process, loads a demo scene or
//! changes the DOM beyond the viewer's own canvas.

mod app;
pub mod assets;
mod handle;
pub mod navigation;
mod presenter;
mod viewer;

pub use app::{AppConfig, ViewerApp};
pub use handle::{LightSettings, LoadTicket, ViewerEvent, ViewerHandle, ViewerState};
pub use presenter::{Acquire, SurfacePresenter};
pub use viewer::{Viewer, ViewerConfig};

pub use scene_navigation;
pub use scene_renderer;
pub use scene_types;
pub use winit;
