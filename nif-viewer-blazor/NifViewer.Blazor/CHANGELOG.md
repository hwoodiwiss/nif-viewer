# NifViewer.Blazor changelog

## 0.1.0-preview.10

Adds keyboard, mouse and controller navigation via the shared
`scene-navigation` crate. Rendering output is unchanged.

### C# API

| Member | Kind | Details |
|---|---|---|
| `NifViewerCanvas` | Changed | Renders a host `<div id="{Id}-host">`; the canvas with `Id` is created inside it by JS. Implements `IAsyncDisposable` and detaches navigation input on disposal. |
| `NifViewerCanvas.SetNavigationSettingsAsync`, `GetNavigationStatusAsync`, `NavigationCommandAsync` | Added | Units/second navigation settings, status and commands such as `Frame`/`Reset`. |
| `NifViewerInterop.SetNavigationSettingsAsync`, `NavigationCommandAsync`, `GetNavigationStatusAsync`, `SetNavigationBindingsAsync` | Added | |
| `NifNavigationSettings`, `NifNavigationStatus` | Added | |
| `SetCameraSpeedAsync` | Changed | Legacy per-frame speed, converted to units/second at 60 Hz. |

### WASM exports

| Export | Kind |
|---|---|
| `set_navigation_settings`, `navigation_command`, `navigation_status`, `set_navigation_bindings`, `navigation_attach`, `navigation_detach` | Added |

## 0.1.0-preview.9

No C# API changes. Rendering output is unchanged (same regression baselines).

### Material system

The core renderer no longer contains NIF shading. It owns the frame
(passes, camera/light uniforms, vertex layout, output encoding) and a WGSL
prelude; material types supply their own shaders and bindings and are
registered by key. NIF materials use the `bethesda-pbr` type shipped by
`nif-scene`. The renderer also ships a stock `basic` diffuse/normal type.

### WASM exports

| Export | Kind | Details |
|---|---|---|
| `attach(canvasId, shaderBaseUrl?, materialBaseUrl?)` | Added parameter | `materialBaseUrl` locates the NIF material package (default `"materials/nif/"`, page-relative). Existing calls are unaffected. |

### Static web assets

| Asset | Kind |
|---|---|
| `shaders/manifest.json` | Changed — shader ABI 2; lists `prelude`, `compose` and stock material `basic`. |
| `shaders/prelude.wgsl`, `shaders/basic.wgsl` | Added |
| `shaders/geometry.wgsl` | **Removed** — moved to `materials/nif/bethesda_pbr.wgsl`. |
| `materials/nif/manifest.json`, `materials/nif/bethesda_pbr.wgsl` | Added — NIF material package. |

Hosts serving the WASM module themselves (not through this package) must
serve both directories; `attach` rejects with the missing URL otherwise.

## 0.1.0-preview.8

No public C#, JavaScript or WASM API changes since preview.6; rebuilt WASM.

- NIF parsing, dependency sessions and NIF → scene conversion moved from
  `nif-viewer-lib` into a separate `nif-scene` crate. DDS decoding moved out of
  the renderer into `nif-scene`; the renderer only accepts decoded RGBA8 images.
- Restored the browser console messages `loaded NIF model: <name>` (on
  successful upload) and `failed to load NIF model <name>: <error>`, which
  preview.6 and the local-only preview.7 build stopped emitting. Hosts that
  watch the console for load completion (such as EsmParser's browser smoke
  test) work again; `GetStatusAsync` remains the structured alternative.
- Rendering output is unchanged (same regression baselines).

## 0.1.0-preview.6

The renderer was extracted into reusable `scene-types`, `scene-renderer` and
`scene-viewer` crates. NIF loading now converts to renderer-neutral scene
data; GPU code no longer lives in `nif-viewer-lib`. Existing C# code compiles
unchanged. The changes below affect behaviour, assets and the JS/WASM layer.

### C# API

| Change | Kind | Details |
|---|---|---|
| `NifViewerInterop.GetStatusAsync()` → `Task<NifViewerStatus>` | Added | Viewer lifecycle plus progress of the latest load. |
| `NifViewerCanvas.GetStatusAsync()` | Added | Forwards to the interop service. |
| `NifViewerCanvas.OnInitializationFailed` (`EventCallback<string>`) and `InitializationError` (`string?`) | Added | The canvas catches attach failures instead of throwing from `OnAfterRenderAsync`, so pages without WebGPU keep rendering; previously such failures were only logged to the browser console. |
| `NifViewerStatus(State, Error, Load)` record, `IsReady` | Added | `State`: `initializing`, `ready`, `suspended`, `device-lost`, `failed`, `disposed`. |
| `NifLoadStatus(Ticket, Name, State, Error, Diagnostics)` record | Added | `State`: `queued`, `uploaded`, `rendered`, `failed`, `superseded`. `Diagnostics` lists missing/undecodable textures and approximated features (e.g. alpha) with NIF paths. |
| `NifLoadResult.LoadTicket` (`long`, init-only) | Added | Correlates a load with `NifLoadStatus.Ticket`; `0` when conversion failed. The positional constructor `NifLoadResult(Missing)` is unchanged. |
| `NifViewerInterop.AttachAsync` | Behaviour | Now completes only when the GPU device, shaders and pipelines are ready, and throws `JSException` if initialisation fails (previously it returned immediately and failures were only logged to the console). Direct callers should handle the exception; `NifViewerCanvas` does so for you. |
| `NifViewerInterop.LoadNifAsync` / `NifViewerCanvas.LoadNifAsync` | Behaviour | Still return after the model is converted and queued (not after it is drawn). A load that fails to upload keeps the previous model visible; failure is reported via `GetStatusAsync()` rather than only the browser console. A newer load supersedes a queued older one. |

Unchanged: `INifDependencyResolver`, `HttpNifDependencyResolver`,
`NifLightSettings`, `SetCameraSpeedAsync`, `SetLightAsync`,
`ParseNifStructureAsync`, `NifStructureView`, `NifViewerControls`, target
frameworks (`net10.0`, `net11.0`).

### JavaScript module (`_content/NifViewer.Blazor/nifviewer.interop.js`)

| Change | Kind |
|---|---|
| `getStatus()` | Added |
| `attach(canvasId)` awaits readiness and rejects on failure | Behaviour |
| `loadNif(...)` returns `{ missing, ticket }` (was `{ missing }`) | Added field |
| Global `window.fetch` patch (`window.__nifViewerFetchPatched`) | **Removed** |

The fetch patch used to redirect any page request to top-level `shaders/…` or
`resources/…` paths into `_content/NifViewer.Blazor/`. Shaders are now fetched
from an explicit URL derived from the module's `import.meta.url`, so host apps
can serve their own `shaders/` and `resources/` paths again without them being
shadowed.

### WASM exports (`nif_viewer_lib.js`)

| Export | Kind | Details |
|---|---|---|
| `attach(canvasId, shaderBaseUrl?)` | **Changed** | Now `async` (returns a `Promise`) and accepts an optional shader base URL (default `"shaders/"`, page-relative). |
| `viewer_status()` | Added | `{ state, error, load: { ticket, name, state, error, diagnostics } }`. |
| `run_wasm()` | Behaviour | Now returns `Result` (throws on DOM failure); still creates and appends a canvas. |
| `load_nif`, `load_session_finish` | Behaviour | Convert the NIF immediately and queue the scene; return values unchanged. Conversion errors are reported through `viewer_status()` with load state `failed`. |
| `set_camera_speed`, `set_light`, `parse_nif_structure`, `load_session_*` | Unchanged | Commands sent before `attach` resolves are queued and applied when the viewer is ready. |

### Static web assets

| Asset | Kind |
|---|---|
| `shaders/manifest.json` | Added — declares shader ABI version 1, profile `nif-pbr-compat` and stage files. Loading fails with a clear error if it is missing or the ABI does not match. |
| `shaders/geometry.wgsl`, `shaders/compose.wgsl` | Added (replace the files below) |
| `shaders/shader.wgsl`, `shaders/draw_deferred.wgsl` | **Removed** |
| `shaders/compute_bitangents.wgsl` | **Removed** — tangents are now generated on the CPU. |

### Rendering

- Visual output is unchanged within ±1 per 8-bit channel on the regression
  fixtures. The scene-colour intermediate is now linear `Rgba16Float` instead of
  the swapchain format, and output gamma encoding happens once in the compose pass.
- Fixed: missing tangents were computed by a compute shader that dispatched
  64× too many invocations without bounds checks and divided by zero on
  degenerate UVs.
- Fixed: screenshot captures (Backspace) were submitted before the frame they
  were meant to capture. Captures are now encoded into the same frame after
  rendering; they produce `frame_scene.png` and, where the browser allows
  surface copies, `frame_output.png` (previously `ss_diffuse.png`/`ss_specular.png`).
- NIF alpha blend/test flags are preserved but still drawn opaque; each such
  load now lists a diagnostic in `NifLoadStatus.Diagnostics`.
