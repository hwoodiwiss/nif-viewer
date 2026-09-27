# Cross-platform scene navigation plan

Status: implementation plan, based on review of both repositories on 2026-09-27.
See [implementation results](input-navigation-results.md) for delivered behavior,
verification, and the remaining physical-hardware acceptance checks. The evidence
section below records the original planning pass.

Applies to `nif-viewer` and `D:\source\repos\wgpu-testbed`, including their native
apps, standalone WASM hosts, and NIF Viewer's Blazor wrapper.

## 1. Outcome and defaults

Every navigation operation must be available using keyboard alone, keyboard and
mouse, or an Xbox-style controller. Users must be able to move through the scene,
look in any direction, inspect an object, change speed, and recover a useful view.

Recommended defaults:

- **Fly** is the initial mode in both projects: unrestricted translation with
  yaw/pitch look and a stable Y-up horizon. Forward follows the view direction;
  strafing is camera-right; ascent/descent is world-up/down. There is no collision
  or gravity requirement. This is three translation axes plus yaw/pitch, not a
  rolling spacecraft camera.
- **Orbit** is an explicit mode for model inspection: rotate around a movable
  pivot, pan, and dolly. NIF loading frames the model without changing the selected
  mode. Both modes support keyboard and controller operation.
- Navigation requires an active viewer. Keyboard/controller navigation does not
  require mouse capture. Mouse look supports right-button drag and an optional
  persistent capture toggle for uninterrupted turning.
- Keyboard/controller controls remain usable when pointer lock is unavailable.
- Windows native and desktop Edge/Chrome on Windows are the initial acceptance
  platforms. Firefox is included where WebGPU is available; other desktop native
  platforms retain compile portability and receive hardware testing when available.

Controller-only means scene navigation after the application/page is open and a
scene is loaded. OS pairing, browser focus, and file dialogs are host operations.
An already visible, focused page must let a controller activate the viewer without
first clicking its canvas, once the browser exposes that controller.

## 2. Findings in the current code

| Area | Evidence and implication |
| --- | --- |
| Shared baseline | Both libraries use Rust edition 2018, `winit 0.30.13`, `wgpu 29`, and `cgmath 0.18`. Their `src/camera.rs` controllers are almost identical. A small shared library is practical without a renderer migration. |
| Movement | `CameraController::update_camera` adds a constant per redraw. W/S approach/retreat from a fixed target, A/D orbit it, and Space/Control change eye height. Forward motion stops near the target. This cannot provide general scene traversal. |
| Input | `process_inputs` uses logical lowercase character keys. There is no mouse or gamepad handling, keyboard look, or focus-loss reset. Shift/layout changes can affect character matching. |
| Event loop | Both `src/lib.rs` files call `state.input` before lifecycle handling, update on `RedrawRequested`, and request continuous redraw in `about_to_wait`. Escape exits the event loop, including on WASM. Capture release must replace this navigation behavior. |
| NIF framing/speed | `nif-viewer-lib/src/state.rs::load_nif_model` frames model bounds, sets far clipping, and sets speed to radius/50 per frame. `NIF_CAM_SPEED`, WASM `set_camera_speed`, and Blazor `SetCameraSpeedAsync` expose that legacy speed. |
| Browser lifetime | NIF `attach` rehomes a live canvas and reuses a page-lifetime event loop. `NifViewerCanvas.razor` has no input-detach lifecycle; disposing the JS module reference alone does not remove browser listeners. |
| Testbed host | Testbed's web entry calls `run_wasm`, which uses `run_app`; NIF already uses `spawn_app`. Testbed's resize path also lacks NIF's zero-size guard. Address these during browser/lifecycle integration. |
| Existing checks | Both workflows currently center on an Ubuntu web build. Neither web package has a test script. NIF already declares `puppeteer-core`; it can be reused for browser automation. |
| Build inputs | NIF's `NifViewerWasm.targets` watches library sources/manifests but will need to watch a new local navigation crate. Testbed's build script sets global WASM `RUSTFLAGS`; isolate these before adding native/WASM checks to one job. |

## 3. Reusable architecture

### Independent repository ownership

Each repository owns a renderer-independent `scene-navigation` workspace crate
under `crates/scene-navigation`, shared by its native and WASM hosts. The initial
design applies to both projects, but each implementation and its tests are
maintained independently. There is no canonical cross-repository copy, synchronization
script, or requirement for matching sources or versions.

Each repository builds from a clean checkout, resolves only its local crate, and
runs its own contract tests in CI without needing the other repository.

Keep the shared API independent of NIF parsing, wgpu, shaders, filesystem access,
and Blazor. Use existing `cgmath` conventions. Preserve edition/toolchain versions.

### Layers

```text
winit keys/buttons/cursor + native gilrs OR browser Gamepad API
                            |
                   platform adapters
                            |
             per-source held state + transient events
                            |
                   normalized InputFrame
                            |
        NavigationRig::update(input, dt, scene context)
                            |
             camera pose + application commands
                            |
           existing Camera / uniforms / renderer
```

Suggested modules in the shared crate:

- `actions`: action identifiers, bindings, configuration, command edges.
- `input`: held controls per source, deltas, source selection, lifecycle reset.
- `navigation`: Fly/Orbit rig, framing, speed, camera pose conversion.
- `adapters/winit`: physical keys, buttons, drag deltas, wheel normalization.
- `adapters/native_gamepad`: native-only `gilrs` backend.
- `adapters/web`: WASM-only gamepad polling, pointer-lock/focus lifecycle hooks.

The mathematical core can compile/test without platform features. Put winit and
device adapters behind features and target-specific dependencies. Platform handles
belong to the host/runtime adapter, not the camera pose. The app owns a monotonic
frame clock; use `web-time::Instant` (already in winit's dependency graph) or the
equivalent target-specific clocks, passing seconds into the pure update function.

### Input contract

`InputFrame` distinguishes:

- Continuous rates: forward/right/up translation, yaw/pitch rotation, boost and
  precision modifiers. These are multiplied by elapsed seconds.
- Accumulated deltas: pointer look/pan and wheel dolly. Consume once per update;
  do **not** multiply mouse displacement by elapsed time.
- Edge commands: frame scene, restore initial view, toggle mode, change speed,
  activate/deactivate navigation, toggle capture, help, and capture screenshot.

Track held physical controls, not one boolean per action. Two keys bound to the
same action must remain active when only one is released. Opposing directions
cancel; ignore OS repeats for edge commands, and suppress command edges caused by
synthetic focus restoration. Preserve down/up command edges between redraws.

Combine keyboard and the selected controller's translation, then limit the final
world-space velocity to the configured maximum; preserve analog magnitudes below
that limit. This also handles view-forward and world-up becoming nearly parallel.
Clamp combined rate-based look, then add mouse angular displacement. Only one
controller owns input at a time, so idle noise cannot steal ownership. Last-device
help hints change only on meaningful activity after deadzones.

### Camera and time semantics

- Store position, yaw/pitch, orbit pivot/distance, and a remembered focus distance.
  Derive orthonormal forward/right vectors; wrap yaw and clamp pitch below +/-90
  degrees (initially +/-89.5). Do not normalize zero-length vectors.
- Fly updates eye and derives target from eye + forward; it can pass through the
  former target. Orbit updates eye from pivot, angles, and positive distance.
- On Fly -> Orbit, place the pivot along the current forward direction at the
  remembered focus distance. Orbit -> Fly preserves pose. Toggling never snaps to
  the scene origin; framing is a separate intentional command.
- Orbit A/D pans camera-right, ascent/descent pans camera-up, W/S dollies; look
  rotates about the pivot. Keyboard/controller dolly uses a per-second exponential
  distance factor; wheel dolly uses a per-notch factor. Scale panning by distance
  and FOV for controllable close-up inspection.
- Start with a maximum `dt` of 50 ms and discard excess elapsed time. Reset the
  clock when inactive, suspended, hidden, or reattached; there is no catch-up jump.
  Clamp nonfinite/negative inputs and bound speed, distance, sensitivity, and dt.
- Framing uses host-supplied world-space bounds, vertical and horizontal FOV, and
  a margin. Handle missing/degenerate bounds with a known fallback view. Store the
  framed view as the reset view on scene load. Testbed must compute bounds over
  its transformed instances; NIF supplies already converted Y-up bounds.
- Choose near/far clipping from scene scale and camera distance with positive
  limits, updating as needed during navigation. Test both close inspection and
  flight around large scenes; an improved controller alone cannot fix clipping.

### Default controls

Bindings are configurable data, with separate mouse/stick sensitivity, stick
deadzone, invert-Y settings, boost/precision factors, and keyboard look speed.
The first release needs API-level rebinding and documented defaults; a visual
rebinding editor and settings persistence can follow.

| Operation | Keyboard alone / keyboard + mouse | Xbox Series Controller |
| --- | --- | --- |
| Fly forward/back; Orbit dolly | W / S | Left stick up/down |
| Fly strafe; Orbit horizontal pan | A / D | Left stick left/right |
| Fly ascend/descend; Orbit vertical pan | E / Q; Space / left Control aliases | RT / LT (difference of independent analog values) |
| Look / Orbit rotate | Arrow keys; right-button drag; captured mouse movement | Right stick |
| Fast / precision movement | Hold Shift / C | Hold RB / LB; precision wins if both held |
| Base speed down/up | [ / ] | D-pad left/right |
| Frame current scene | F | Y |
| Restore initial/framed view | Home | X |
| Toggle Fly/Orbit | O | View button |
| Activate navigation | Focus canvas with Tab, then Enter; click canvas | A, when page/window is active and no host control owns focus |
| Release capture and deactivate | Escape | B |
| Persistent mouse capture toggle | L while viewer active; canvas-local browser gesture | Not needed for controller navigation |
| Dolly with mouse | Wheel; Shift+wheel changes speed | W/S equivalent above |
| Controls help | H | Menu button |

Escape always releases/deactivates, rather than terminating the browser renderer.
Native window close/Alt+F4 remains the normal exit path. Keep Backspace screenshot
capture as an app command, activated once per press and only in the viewer context.
Gamepad Guide/Share buttons are not required bindings because OS/browser exposure
varies. Provide a visible mode/speed/active-device/capture status and a help legend
in web hosts; native can start with window-title status and a logged help legend.

## 4. Platform integration

### Native

- Continue using `winit::ApplicationHandler`; handle focus/suspend/resize before
  dispatching consumable navigation events. Add `device_event` for relative mouse
  motion, restricted to the active focused viewer and confirmed capture/drag mode.
- Prefer `CursorGrabMode::Locked`, with `Confined` where needed (notably Windows)
  and drag-only fallback on failure. Restore the cursor on release, blur, and exit.
  For drag-only mode use cursor-position differences; never count both raw motion
  and cursor motion for the same interaction. Rebase on drag start and DPI changes.
- Use native-target-only `gilrs` (0.11.x candidate) with its default Windows Gaming
  Input backend first. Initialize after window creation and poll/drain before each
  update without blocking. A backend error produces a device-status message and
  leaves keyboard/mouse usable. XInput is a targeted fallback if the hardware
  spike establishes a WGI problem, not a second simultaneously active backend.
- Drain device events while inactive as appropriate, but discard actions and
  rearm after neutral input. Clear removed-device state immediately on disconnect.
- Linux CI needs `pkg-config` and `libudev-dev` once gilrs is introduced. Keep
  native gamepad dependencies out of the WASM dependency graph.

### Browser/WASM

- Gilrs also supports WASM. Prefer a small direct browser adapter here because
  mapping availability, permission failures, canvas activation, and page lifecycle
  need explicit handling and diagnostics in the embedded Blazor host. Keep the
  provider interface narrow so this choice can be revisited after the spike.
- Use `web-sys` `Navigator::get_gamepads` once per visible update. Re-read current
  objects rather than retaining connection-event snapshots. Add required features
  explicitly, including `Navigator`, `Gamepad`, `GamepadButton`, and mapping types.
  Polling stays in WASM; there is no per-frame .NET interop.
- Map `mapping == "standard"` by position: axes 0/1 left, 2/3 right; buttons 0..3
  A/B/X/Y, 4/5 LB/RB, 6/7 LT/RT, 8 View, 9 Menu, 12..15 D-pad. Check array lengths,
  sparse/null slots, connection state, and reused indices. Unknown mappings get an
  explicit unsupported/remapping status rather than guessing from a name string.
- Handle unavailable/blocked APIs and document HTTPS/localhost, browser activation,
  and iframe `gamepad` Permissions Policy requirements. Show "press A to activate"
  once available; gamepad polling does not itself grant pointer-lock activation.
- Use winit for keys/buttons/unlocked cursor and wheel. Verify its relative
  `DeviceEvent::MouseMotion` behavior against the pinned version in the spike.
  If a browser-specific relative-motion listener is needed, it replaces that
  source during lock; it must not add duplicate movement.
- Request pointer lock directly from a trusted canvas-local click/key handler.
  Do not defer through asynchronous Blazor initialization. Confirm actual state
  using `pointerlockchange`/`document.pointerLockElement`, handle failure, and
  never automatically recapture after Escape. Raw/unadjusted movement is optional.
- Explicitly configure canvas focusability, a visible focus outline, and accessible
  instructions. Preserve Tab/Shift+Tab and browser/system shortcuts. Winit defaults
  to preventing browser defaults on canvas events: configure this deliberately and
  suppress only consumed navigation keys, wheel, and context menu interactions.
  Form fields/sliders/contenteditable and external controls must retain their keys.
- A focus state machine distinguishes inactive, active-unlocked, drag, and locked
  states. Page/window blur, canvas focus leaving navigation, hidden document,
  suspension, detach, and lock loss clear held state/deltas and reset time. Require
  a fresh activation and controller neutral state to avoid resuming a held stick.
  Allow controller A activation from a neutral page background, but never steal
  focus from a host field, dialog, or another active viewer.
- Keep one polling/update path driven by winit redraw. Listeners have explicit
  ownership and removable handles. Detach disables input/releases capture; attach
  rebinds to the actual live canvas exactly once. Use an attachment generation/token
  so disposal of an old Blazor component cannot deactivate a newer attachment.
- Bring testbed's web startup to `spawn_app` and add zero-size/suspension guards.
  Preserve NIF's existing page-lifetime canvas model.

### Controller normalization

Normalize native and browser inputs into the same signed convention (right/up
positive in the core), including their potentially different stick-Y signs and
trigger representations. Use semantic gilrs controls, not its raw numeric codes.

Start with radial stick deadzones of 0.15, rescaled continuously to full range:
for magnitude `m <= d`, output zero; otherwise use direction times
`min(1, (m-d)/(1-d))`. Keep this configurable per stick. Apply a configurable
response curve after deadzone (linear movement, initially quadratic look) and a
small independent trigger deadzone before computing RT-LT. Check gilrs default
filter behavior and configure it so the application does not unknowingly apply
two deadzones. Poll button edges without generating repeated toggles.

Select a controller on deliberate A activation, retain it until release or
disconnect, and require neutral -> press to transfer ownership. Startup/reconnect
must not replay a held command. No input-device name or GUID should be assumed
stable across USB and Bluetooth.

## 5. NIF speed compatibility and host APIs

Use world units/second internally. Preserve old entry points as documented legacy
adapters using a fixed **60 Hz reference**, not actual frame rate:

- `set_camera_speed(s)`, `SetCameraSpeedAsync(s)`, and `NIF_CAM_SPEED=s` set the new
  speed to `s * 60`. Existing 0.2 becomes 12 units/second. Validate finite positive
  values before conversion and clamp the result.
- Automatic radius/50 per-frame speed becomes `radius * 1.2` units/second.
- Add unambiguous `set_navigation_settings` / `SetNavigationSettingsAsync`, with
  units/second speed and the mode/sensitivity/deadzone/binding settings. Native
  startup options use the same Rust configuration; a new explicit units/second
  override takes precedence over the legacy environment variable.
- Keep the old model-load reset behavior for callers using only the legacy API.
  New settings distinguish automatic scale-aware speed from an explicit override
  which persists across loads. Commands adjust the current base speed; report the
  actual value back to the host so its control display cannot become stale.
- Add frame-scene/reset-view actions and a low-frequency navigation-status API.
  Synchronize Rust exports, JS interop, C# settings types, and controls together.
  Update the standalone web and Blazor speed labels/defaults to units/second.
- Add attach/deactivate/detach lifecycle support for Blazor. Pointer-lock requests
  originate in browser handlers even when the hosting component is C#.

## 6. Delivery sequence and completion gates

1. **Device/lifecycle spike.** Add a small opt-in diagnostic example/page using
   the proposed adapters: raw/normalized axes, trigger values, button edges,
   connected/active device, focus, and pointer-lock state. Validate Windows native
   WGI and browser standard mapping with the Xbox Series Controller over USB and
   Bluetooth. Verify pinned winit relative motion, capture fallbacks, Tab behavior,
   and lock rejection. Gate: no unresolved axis/trigger or duplicate-motion issue;
   record browser/OS/transport and actual outcomes. The diagnostic becomes the
   reusable manual-test harness rather than a separate input implementation.
2. **Shared core + keyboard parity.** Add the workspace crate, data bindings, dt
   integration, Fly/Orbit math, reset/framing, and deterministic tests. Integrate
   both renderers and speed migration. Gate: every scene navigation operation is
   available with keyboard alone; local replay tests pass.
3. **Mouse and focus lifecycle.** Implement drag/capture, wheel behavior, state
   clearing, browser focus/shortcut policy, and testbed startup/resize changes.
   Gate: native and browser mouse+keyboard scenarios pass, including Escape,
   Alt+Tab, capture denial, resizing, and returning from a hidden tab.
4. **Controller integration.** Connect verified native/browser adapters, deadzones,
   ownership, hotplug, and status. Gate: all navigation tasks can be completed
   without keyboard/mouse after activation; USB/Bluetooth and reconnect checks pass.
5. **Host UX and package integration.** Add help/status/settings to both web hosts
   and Blazor, update API documentation, add generation-safe detach/reattach, and
   extend `NifViewerWasm.targets` inputs for the shared crate/configuration. Gate:
   both WASM packaging formats and a local Blazor consumer smoke test pass.
6. **Acceptance and CI.** Add platform checks and replay/browser tests, tune values
   from hardware feedback, run the matrix below, and record limitations by tested
   platform. Gate: no drift, stuck movement, double input, camera singularity, or
   input leaking into host controls; rendered landmarks visibly track navigation.

Each repository implements the stages independently and runs its affected tests.
Scene bounds, screenshot commands, renderer startup, and host UI
remain repository integration points. Use scoped changes in `src/camera.rs`,
`src/state.rs`, `src/lib.rs`, manifests, web entrypoints, and the Blazor files named
above; no parser or shader redesign is needed.

## 7. Test strategy

### A. Deterministic core tests (no window, GPU, or physical controller)

- Known poses: forward/strafe/up directions, movement beyond an old target, pitch
  limits, yaw wrap, finite matrices at extreme pitch, and invalid settings.
- Time: pure translation and pure rate-based rotation over one second at 30/60/144
  Hz agree within numerical tolerance. Mixed turning/movement paths have a stated
  integration tolerance (initial target <=1% of path length at >=30 Hz). Mouse
  displacement has the same angular result when split across event/frame batches.
- Opposite keys, diagonal speed limits, near-vertical mixed translation, partial
  analog values, keyboard+controller combination, modifier precedence, rebinding,
  aliases released independently, and edge commands across rapid down/up events.
- Orbit pivot/distance/pan, pose-preserving mode switches, reset and framing with
  wide/tall aspect ratios, tiny/large/degenerate bounds, and clipping limits.
- Stick deadzone continuity and full-range behavior, response curves, independent
  triggers/both triggers held, sign normalization, missing axes/buttons, NaN/Inf,
  held-on-connect buttons, disconnect/reused ID, controller selection and handoff.
- Blur/hide/detach/lock-loss state resets; no deltas or command edges replay after
  reactivation. Long pauses do not produce catch-up travel.

Use independent numeric expectations and action traces, not snapshots of the
implementation. Keep versioned traces in each repository for local regression tests.

### B. Adapter and browser automation

Use a fake gamepad provider and fake clock below platform polling for portable
adapter tests. Also run `wasm-bindgen-test` browser tests for the actual web adapter
with a controlled `navigator.getGamepads` shim installed before initialization.
Model sparse arrays, changed objects, blocked API errors, reconnects, mappings,
held buttons, and page lifecycle changes. These tests establish adapter behavior,
not physical Xbox compatibility.

Reuse NIF's `puppeteer-core` for a browser smoke suite and add equivalent dev/test
wiring to testbed. Explicitly configure a browser executable in local/CI runs.
Test real canvas focus and browser keyboard/mouse delivery, navigation controls,
form-field isolation, Tab escape, resize/DPI changes, and navigation after attach.
Expose a test-only camera/status readback and renderer-ready/frame-complete signal;
avoid assertions based on arbitrary sleeps or console output alone.

Run browser tests with real WASM in both bundler and `--target web` hosts. Test
Blazor navigation away/back repeatedly, including stale disposal after reattach,
and verify one input delivery per event. Separate pure input tests from GPU tests
so lack of WebGPU produces an explicit GPU-test skip, not a false rendering pass.
Pointer-lock success/raw motion still needs a headed/manual check; synthetic DOM
events do not establish trusted browser activation or OS pointer behavior.

### C. Rendered acceptance scene

Use a generated scene with colored/labelled axis landmarks, near/far objects, an
off-origin object, and an asymmetric arrangement that makes reversed movement
obvious. Include small and large scale variants. Supply a generated NIF fixture
and an equivalent testbed scene without proprietary assets.

After deterministic movement, assert camera pose and visibly rendered landmark
changes. Where GPU screenshot comparison is used, compare robust regions/landmark
locations with tolerances rather than exact cross-driver pixel hashes. Disable
animated lighting for captures. A parsed scene or updated camera matrix is not
evidence that the geometry rendered correctly. Existing G-buffer captures are
diagnostics; final-color frame capture/readback is needed for visual acceptance.

### D. Manual acceptance matrix

Run each applicable row in **both repositories**, including NIF's Blazor host.

| Environment | Keyboard | Mouse + keyboard | Xbox USB | Xbox Bluetooth |
| --- | --- | --- | --- | --- |
| Windows native | Required | Required | Required | Required |
| Windows Edge, secure/localhost WASM | Required | Required | Required | Required |
| Windows Chrome, secure/localhost WASM | Required | Required | Required | Required |
| Firefox with available WebGPU | Required when available | Required when available | Test and record | Test and record |
| Linux/macOS native and other browser hosts | Compile checks where available; record hardware coverage separately | | | |

For each device setup: approach and pass a landmark, strafe, ascend/descend, turn
360 degrees, look up/down, orbit and pan an off-origin model, inspect it closely,
change speed, frame/reset, and switch modes. Confirm keyboard-only usability with
Shift/Caps Lock and a non-US layout, and controller-only recovery using B then A.

For controllers: start connected, connect after startup, leave untouched for 60
seconds (no drift), test partial/full sticks, each trigger and both together,
hold buttons (no repeated toggles), disconnect during movement, reconnect/sleep/
wake, and select between two devices if available. Record hardware firmware,
transport, backend/mapping, OS/browser version, and tuned deadzones. Testbed and NIF
must agree for the same normalized input; OS/browser sensitivity differences are
measured and addressed in adapters/settings rather than hidden in camera math.

For lifecycle: release a held key outside the viewer, Alt+Tab, minimize, hide the
tab for several seconds, Escape from capture, deny capture, drag out of bounds,
move between DPI scales, focus speed/light/file controls, and detach/reattach.
Movement must stop by the next active update after loss/disconnect and must not
resume from stale input. Hidden tabs must be inert until explicitly reactivated.

### E. Build and CI checks

Run appropriate checks per stage, preserving target-specific `.cargo/config.toml`
flags and checking global `RUSTFLAGS` / `CARGO_ENCODED_RUSTFLAGS` if targets disagree.

From NIF Viewer root:

```shell
cargo test -p scene-navigation
cargo test -p nif-viewer-lib
cargo clippy --workspace --all-targets -- -D warnings
```

From testbed root:

```shell
cargo test -p scene-navigation
cargo test -p wgpu-testbed-lib
cargo clippy --workspace --all-targets -- -D warnings
```

From each Rust library directory, test both browser consumers:

```shell
wasm-pack build --release --target web --out-dir pkg-web
wasm-pack build --release --target bundler --out-dir pkg
```

From each webapp directory run `npm ci`, `npm run build`, and the browser test
script introduced in the implementation. Run native checks on Windows and Linux;
install gilrs system prerequisites on Linux. Run pure core/browser adapter tests
without a GPU and a separate Chromium WebGPU integration job where supported.
Use a headed hardware run for final controller/pointer acceptance.

For Blazor changes, use the SDK in `global.json`, build matching WASM assets, then
from NIF Viewer root:

```shell
dotnet pack nif-viewer-blazor/NifViewer.Blazor -c Release -o artifacts -m:1
```

Use a new local package version for changed assets and a small downstream smoke
host to exercise exported settings, navigation, and lifecycle. Keep packing serial.
Report missing SDKs, skipped hardware cases, and unverified rendering explicitly.

## 8. Evidence and remaining validation

This planning pass inspected source, manifests, build scripts, CI, and Blazor
interop in both repositories, plus the upstream references below. It did not run
a PoC, test suite, browser session, or controller test. The architecture can be
planned from those sources; the phase-one spike is specifically the gate for
behavior that source inspection cannot establish on the user's hardware.

The main remaining empirical questions are Xbox USB/Bluetooth mapping parity,
gilrs filter/trigger behavior, winit relative-motion delivery across browsers,
capture fallback behavior on Windows, and comfortable scale/sensitivity defaults.

References:

- [winit 0.30.13 web integration](https://docs.rs/winit/0.30.13/winit/platform/web/index.html)
- [winit 0.30.13 web window implementation](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/web/window.rs)
- [gilrs 0.11.2 API and platform notes](https://docs.rs/gilrs/0.11.2/gilrs/)
- [MDN: Using the Gamepad API](https://developer.mozilla.org/en-US/docs/Web/API/Gamepad_API/Using_the_Gamepad_API)
- [MDN: Pointer Lock API](https://developer.mozilla.org/en-US/docs/Web/API/Pointer_Lock_API)
