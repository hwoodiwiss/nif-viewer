# Navigation implementation and verification

Implemented on `feature/scene-navigation` in both `nif-viewer` and `wgpu-testbed`.

## Delivered

- Independently owned `scene-navigation` crate in each repository, shared locally
  by native/WASM hosts: Fly/Orbit rig, time-based movement,
  physical-key bindings, keyboard look, drag/captured mouse look, wheel dolly,
  speed modifiers, framing/reset, clip planes, and settings validation.
- Native gilrs/WGI and WASM standard Gamepad API adapters, stick/trigger deadzones,
  controller ownership, neutral-before-activation and disconnect reset.
- Native and browser activation/focus handling, Escape release, Tab escape,
  browser trusted-gesture pointer lock and explicit listener ownership.
- Web settings/status/help, native diagnostic logging, browser gamepad diagnostics,
  native JSON settings and units/second override, and legacy NIF speed conversion.
- Blazor settings/commands/status, generation-safe disposal, and a JS-owned canvas
  inside a component-owned container. Fixed the missing Razor web event import.
- Testbed bounds across transformed instances, async startup resize recovery and
  browser `spawn_app`; build scripts/CI include native controller prerequisites.
- Shared browser regression suite, generated NIF fixture, and a local-package
  Blazor WASM smoke host. No proprietary test resources were added.

## Local checks

Environment: Windows ARM64, Rust 1.98.1, Edge 153.0.4234.32 with WebGPU.

| Check | Result |
| --- | --- |
| `cargo test -p scene-navigation` in both repositories | 13 passed each |
| `cargo test -p nif-viewer-lib` | Passed; one explicitly ignored local-asset test. Existing asset-optional early returns are not evidence of asset validation. |
| `cargo test -p wgpu-testbed-lib` | Passed (no existing library tests); shared tests cover navigation |
| `cargo clippy --workspace --all-targets -- -D warnings` in both | Passed |
| WASM Clippy for `scene-navigation` | Passed |
| `wasm-pack build --release --target web --out-dir pkg-web` in both | Passed |
| `wasm-pack build --release --target bundler --out-dir pkg` in both | Passed |
| `npm run build` in both webapps | Passed |
| `npm run test:navigation`, web and bundler in both | Passed in Edge; rendered geometry, keyboard/drag, focus/Tab, rebinding, simulated controller/disconnect, unsupported mapping, and attachment checks |
| `dotnet pack ... -c Release -o artifacts -m:1` | Produced `NifViewer.Blazor.0.1.0-preview.6.nupkg` for net10.0/net11.0 |
| Local-package `NavigationSmoke` restore/build | Passed |
| `npm run test:blazor` | Passed: three disposal/reattach cycles while moving, host-field isolation, settings |

The .NET launcher selected **11.0.100-rc.1.26425.128**, although `global.json`
requests **11.0.100-preview.6.26359.118**. This is a successful build with the
available SDK, not verification on that exact requested preview SDK. Temporary
smoke-host package caches under `obj` avoided stale NuGet package content while
iterating on the new preview.6 package.

The visual browser check copies the final displayed WebGPU canvas, requires
multiple distinct colors and a changed rendered image after movement/turning.
It uses generated NIF landmarks and testbed's existing cube scene. This caught
and fixed testbed's 1x1 surface initialization bug. It is a rendered smoke check,
not an exact cross-driver image comparison or a substitute for visual review.

## Hardware acceptance still required

The user confirmed native Xbox controller navigation works after reconnecting a
disconnected controller. The transport and individual controls were not recorded;
this confirms basic native operation, not the full acceptance matrix below.

- Xbox Series Controller over USB and Bluetooth, including native WGI trigger
  values/signs, drift, hotplug/reconnect, and sleep/wake.
- Native captured mouse behavior/Confined fallback, browser real pointer lock,
  rejection, OS release gestures, multi-monitor DPI, and sustained controller-only
  usability. Automation exercises unlocked right-drag and simulated gamepads.
- Linux/macOS, Firefox, Chrome, and Windows x64 hardware runs. CI jobs are added,
  but have not been run remotely in this work session.

The planned initial hardware spike is represented by the live diagnostic hooks;
the full hardware matrix remains pending beyond the user's basic native check.
Defaults are initial values pending that test. See `crates/scene-navigation/CONTROLS.md` for commands and the
original plan for the full acceptance matrix.

## Implementation choices versus the plan

- The shared crate uses target-specific device dependencies but currently includes
  winit for its key-binding types; a no-platform-feature build is not introduced.
- Browser adapter testing uses Puppeteer against the real WASM module rather than
  adding a second wasm-bindgen-test harness. Native deterministic tests and both
  packaging formats run each repository's local navigation implementation.

- Settings/rebinding APIs and controller diagnostics are available; a visual
  rebinding editor, settings persistence, non-standard gamepad remapping, roll,
  collision and rumble remain outside this delivery.

Each repository builds, tests, and evolves independently. The original synchronization
script was removed; neither repository depends on the other's source or version.
