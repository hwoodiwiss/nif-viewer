# Scene navigation

Both renderers use the same Y-up navigation implementation on desktop and WASM.
Start by clicking the canvas, focusing it with Tab then pressing Enter, or pressing
controller A with the page/window focused. Release sticks/buttons before pressing
A on a newly connected controller. The browser may require an initial controller
interaction before exposing it. Loading files/pairing controllers uses host UI.

| Operation | Keyboard / mouse | Xbox controller |
| --- | --- | --- |
| Move/strafe | WASD | Left stick |
| Ascend/descend | E/Q or Space/left Ctrl | RT/LT |
| Look | Arrows or right-button drag | Right stick |
| Persistent mouse capture | L; Escape releases | Not required |
| Fast/precise | Hold Shift/C | Hold RB/LB |
| Speed down/up | [ / ]; Shift+wheel | D-pad left/right |
| Frame scene / reset view | F / Home | Y / X |
| Fly/Orbit | O | View |
| Release navigation | Escape | B |
| Help | H | Menu |
| Screenshot | Backspace | — |

Fly moves through the scene without collision and keeps a stable horizon. Orbit
rotates around a movable pivot: WASD pans horizontally/dollies, E/Q pans vertically.
The wheel dollies in either mode. Precision takes priority over boost. Mode changes
preserve the current pose; framing/reset intentionally restores a useful view.
Tab and host form controls retain normal browser behavior. Escape releases input
rather than shutting down the viewer; close the native window to exit.

## Configuration

Native apps accept `SCENE_NAVIGATION` as JSON using `NavigationSettings` field
names, and `SCENE_NAV_SPEED` as a units/second override. For example:

```powershell
$env:SCENE_NAV_SPEED = '25'
$env:SCENE_NAVIGATION = '{"right_deadzone":0.18,"invert_stick_y":true}'
```

`SCENE_NAV_SPEED` takes precedence and persists across scene loads. Settings use
defaults for omitted fields and reject unknown/nonfinite/out-of-range values.
`automatic_speed: true` scales speed on scene load. In NIF Viewer, the legacy
`NIF_CAM_SPEED`, JS `set_camera_speed`, and C# `SetCameraSpeedAsync` still accept
legacy values, converted at a fixed 60 Hz reference: 0.2 means 12 units/second.

After `navigation_status()` returns settings, browser hosts can call:

```javascript
const status = JSON.parse(wasm.navigation_status());
wasm.set_navigation_settings(JSON.stringify({
  ...status.settings, speed: 25, automatic_speed: false, mode: "Fly"
}));
wasm.navigation_command("Frame");
// Physical KeyCode names, action names; replaces all keyboard bindings.
wasm.set_navigation_bindings(JSON.stringify({
  Enter: "Activate", Escape: "Release", KeyW: "Forward", KeyS: "Backward",
  KeyA: "Left", KeyD: "Right", ArrowLeft: "LookLeft", ArrowRight: "LookRight"
}));
```

Native library users can modify `CameraController.input.bindings` directly.
Tab is reserved in browser bindings. Browser gamepads require a `standard` mapping;
unsupported mappings are reported in `navigation_status().gamepadStatus`. Use
HTTPS/localhost and permit `gamepad` in iframe Permissions Policy when embedded.
Pointer lock requires a real local user gesture; right-drag works without it.

## Diagnostics and verification

Native: set `SCENE_NAV_DIAGNOSTICS=1` and `RUST_LOG=info` to log sampled controllers,
normalized actions, camera position, and capture state twice per second. Browser:
inspect `JSON.parse(wasm.navigation_status())` for controller samples, actual
settings, pose, activation/capture and mapping status. No .NET polling is required
for movement; Blazor status controls refresh twice per second.

Run `cargo test -p scene-navigation` for deterministic input/camera tests. From
either webapp after building `pkg-web`, `pkg`, and the webapp:

```powershell
$env:BROWSER_EXECUTABLE = 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe'
npm run test:navigation
$env:WASM_HOST = 'bundler'
npm run test:navigation
```

The browser suite exercises real WASM, generated NIF geometry/testbed geometry,
displayed pixel changes, keyboard/right-drag, focus/Tab, simulated standard
gamepads, disconnect and NIF attachment generations. Gamepad simulation does not
validate the physical driver. Run headed by setting `HEADED=1`.

Hardware acceptance still requires Xbox USB and Bluetooth checks: both sticks,
both triggers separately/together, all bindings, 60-second idle drift, hotplug
during motion, reconnect, sleep/wake, Alt+Tab, mouse capture/Escape, and browser
permission rejection. Record OS/browser/firmware/transport and tune deadzones.
Linux builds need `pkg-config` and `libudev-dev`.
