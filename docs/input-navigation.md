# Scene navigation

Native, standalone WASM, and Blazor hosts support keyboard-only, mouse/keyboard,
and Xbox-style controller navigation. This document describes the implementation
and its current verification status.

NIF Viewer and wgpu-testbed each own a local `crates/scene-navigation` workspace
crate. Native and WASM hosts share that crate within their repository. The two
repositories build, test, and evolve independently; neither requires a sibling
checkout or matching source versions.

## Navigation behavior

- **Fly** is the default mode. Forward follows the view direction, strafing follows
  camera-right, and ascent/descent follows world Y. Movement is unrestricted,
  with yaw/pitch look and a stable horizon.
- **Orbit** rotates around a movable pivot. Horizontal movement pans, vertical
  movement pans camera-up/down, and forward/backward dollies. Panning scales with
  distance and the speed setting; dolly distance changes exponentially.
- Switching modes preserves the camera pose. Framing and resetting are separate
  commands for recovering a useful view.
- NIF loading frames the model's Y-up bounds and records the reset view. Testbed
  computes scene bounds across transformed instances and retains its initial view
  until the user frames the scene.
- Translation and keyboard/controller look use elapsed seconds. Pointer movement
  is accumulated displacement, consumed once without multiplying by frame time.
  Updates clamp elapsed time to 50 ms; deactivation resets the clock and input.
- Fly velocity is limited after combining inputs, avoiding faster diagonal
  movement while preserving partial analog input. Pitch is clamped below the
  vertical singularity; near/far clipping tracks scene scale and camera distance.

### Controls and activation

Activate by clicking the viewer, focusing its canvas and pressing Enter, or
pressing controller A while the window/page is focused. A controller must first
be observed with its buttons and axes at rest. Only one controller owns navigation
at a time; disconnecting or releasing navigation clears that selection.

| Operation | Keyboard / mouse | Xbox controller |
| --- | --- | --- |
| Move / strafe | WASD | Left stick |
| Ascend / descend; Orbit vertical pan | E/Q or Space/left Ctrl | RT/LT |
| Look / Orbit rotate | Arrow keys, right-button drag, captured mouse | Right stick |
| Fast / precision | Hold Shift/C | Hold RB/LB |
| Speed down/up | `[` / `]`; Shift+wheel | D-pad left/right |
| Frame / reset view | F/Home | Y/X |
| Fly/Orbit | O | View |
| Release navigation | Escape | B |
| Persistent mouse capture | L | Not required |
| Help | H | Menu |

The wheel dollies; Backspace captures a screenshot. Precision takes priority over
boost. Escape releases input rather than exiting the app; close the native window
to exit. Browser Tab leaves the canvas, and host fields retain normal text input.
Controller navigation does not require pointer capture. OS pairing, file dialogs,
and loading a scene remain host operations.

See [controls and configuration](../crates/scene-navigation/CONTROLS.md) for the
complete usage reference and diagnostic commands.

## Implementation

```text
winit events + native gilrs / browser Gamepad API
                         |
             per-source input state
                         |
                    InputFrame
                         |
                 Rig::update(dt)
                         |
           Camera -> uniforms -> renderer
```

The local navigation crate contains:

| Module | Responsibility |
| --- | --- |
| `input.rs` | Actions, physical-key bindings, held controls, command edges, controller selection and normalization |
| `rig.rs` | Camera, Fly/Orbit math, bounds, framing, reset, clipping and validated settings |
| `runtime.rs` | winit event integration, frame clock, native gamepad polling, capture and diagnostics |
| `web.rs` | Browser gamepad polling, DOM listeners, pointer lock, attachment generations and host APIs |

Camera math is independent of the GPU and NIF parser. The crate currently depends
on winit for key-binding types; it does not provide a platform-free feature build.
Device dependencies remain target-specific.

### Native input

`gilrs` uses Windows Gaming Input on Windows. Events are drained before sampling
cached controller state. Application deadzones are applied after normalization;
gilrs default filters are disabled. Default stick deadzones are 0.15, with linear
movement and quadratic look response. Independent trigger values use a 0.05
deadzone before computing RT-LT.

Mouse capture tries `Locked`, then `Confined`, with right-drag available if capture
fails. Captured relative motion and uncaptured cursor-position differences are
separate input paths. Focus loss, suspension and deactivation clear navigation
state and release capture. Linux builds require `pkg-config` and `libudev-dev`.

### Browser input and lifetime

The WASM adapter polls `navigator.getGamepads()` for current snapshots. It accepts
the browser's `standard` mapping, handles sparse slots/disconnects, and reports
unsupported or unavailable mappings through navigation status. Browser gamepad
access may require user interaction, HTTPS/localhost, and iframe permissions.

winit handles keys, buttons, wheel and uncaptured cursor events. A DOM mouse
listener handles locked relative motion. Pointer lock is requested synchronously
from a trusted canvas key event; actual lock state comes from browser events.
Focus loss, hidden/detached canvases and lock loss deactivate navigation.
Listeners have explicit owners and removable handles.

The NIF browser renderer reuses one live canvas and event loop. Blazor owns an
empty container; JS/winit own the canvas inside it. Attachment generations prevent
disposal of an old component from deactivating a newer attachment. Both web hosts
use `spawn_app` and recover canvas size after asynchronous GPU initialization.

## Settings and host integration

`NavigationSettings` exposes mode, world-units-per-second speed, automatic
scale-aware speed, mouse/keyboard/stick look sensitivity, deadzones, invert-Y,
response curve, boost and precision. Keyboard bindings are replaceable through
the native input state or the browser bindings API. Settings reject invalid,
nonfinite and out-of-range values.

- Native configuration: `SCENE_NAVIGATION` JSON and `SCENE_NAV_SPEED` override.
- Browser configuration: `set_navigation_settings`, `set_navigation_bindings`,
  `navigation_command` and `navigation_status`.
- Blazor: `NifNavigationSettings`, settings/command/status methods, and
  `NifViewerControls` for mode, speed, help and status. Movement stays in WASM;
  it does not require per-frame .NET interop.
- Diagnostics: `SCENE_NAV_DIAGNOSTICS=1` logs native samples and normalized input;
  browser status includes controller samples, pose, settings and capture state.

Legacy NIF speed APIs (`NIF_CAM_SPEED`, `set_camera_speed`, and
`SetCameraSpeedAsync`) convert values using a fixed 60 Hz reference: 0.2 means
12 units/second. Automatic speed is bounding radius × 1.2 units/second. Explicit
new settings can retain a speed across model loads.

The Blazor package version is `0.1.0-preview.6`, targeting net10.0 and net11.0.
Its incremental WASM build watches navigation sources and configuration. Packing
runs serially; the local-package `NavigationSmoke` host exercises the packaged
component rather than a project reference.

## Automated verification

Rust tests exercise navigation without a window, GPU or controller. Playwright
tests run the actual WASM renderer with an isolated browser context per test.
Each webapp owns its tests and a read-only asset server, managed by Playwright.
The `web` project tests browser-native modules; `bundler` tests the built webapp.

Browser cases are grouped into navigation, gamepad and rendering specs, plus NIF
attachment/interop and separately configured Blazor specs. Assertions wait for
observable state and gamepad polling rather than fixed delays or frame counts.
Rendering checks poll compositor screenshots for contrasting interior geometry
and meaningful image change after an observed camera turn. NIF uses generated
landmarks; testbed uses its cube scene. These are rendered smoke checks, not
pixel-perfect cross-driver comparisons. Missing WebGPU fails explicitly.

CI configures native checks on Windows/Linux and browser checks on Windows. The
browser job installs Chromium matched to pinned Playwright 1.63.0, runs both
packaging projects, and uploads reports, traces, screenshots, console/request logs
and navigation state. Reports and test outputs are gitignored.

### Recorded local results

| Check | Status |
| --- | --- |
| Navigation Rust tests | 13 passed in each repository |
| NIF library tests | Passed; one explicitly ignored local-asset test |
| Testbed library tests | Passed; no separate library test cases |
| Workspace Clippy, all targets, warnings denied | Passed in both repositories |
| WASM Clippy for the navigation crate | Passed |
| WASM release builds, `web` and `bundler` | Passed in both repositories |
| Webapp builds | Passed in both repositories |
| Playwright NIF navigation | 23 passed across both packaging projects |
| Playwright testbed navigation | 20 passed across both packaging projects |
| Playwright packaged Blazor | 2 passed, including repeated disposal/reattachment and settings |
| Blazor pack and downstream smoke-host build | Passed |

Rust/build checks ran on Windows ARM64 with Rust 1.98.1. The latest browser checks
used Playwright 1.63.0 and matched Chromium 153.0.8010.12. The Blazor tests used
the existing built package/host. The .NET builds used
`11.0.100-rc.1.26425.128`, which is installed locally and pinned in `global.json`.

The revised Playwright workflow has not yet run on GitHub Actions. One simultaneous
local testbed run timed out during runner shutdown after completing its cases;
the standalone run completed successfully. The optional local-asset inspection
script was syntax-checked, but its proprietary-asset flow was not rerun. Asset-
optional Rust tests returning early do not establish real-asset verification.

Setup and commands:

- [NIF browser tests](../nif-viewer-webapp/tests/README.md)
- [Packaged Blazor smoke host](../nif-viewer-blazor/NavigationSmoke/README.md)
- Testbed: `wgpu-testbed-webapp/tests/README.md` in its own checkout

## Remaining validation and limitations

The user confirmed native Xbox controller navigation works. Transport and
individual controls were not recorded, so full hardware acceptance remains open:

- USB and Bluetooth: partial/full sticks, separate/simultaneous triggers, all
  bindings, idle drift, controller selection, hotplug, reconnect and sleep/wake.
- Native capture fallback and browser real pointer lock: permission rejection,
  Escape/OS release gestures, Alt+Tab and multi-monitor DPI changes.
- Sustained keyboard/controller-only navigation and sensitivity tuning at small
  and large scene scales.
- Additional platform/browser hardware runs, including Linux/macOS and Firefox.

Simulated browser gamepads verify the adapter contract, not physical driver
behavior. Successful parsing or camera-state changes alone do not prove rendering.

There is no visual rebinding editor, settings persistence, non-standard gamepad
remapping, roll, collision, gravity or rumble support.
