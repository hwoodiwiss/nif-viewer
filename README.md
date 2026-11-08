# NIF Viewer

## Repository layout

The renderer is split into reusable crates that know nothing about NIF files,
plus the NIF-specific application layer:

| Crate | Owns |
|---|---|
| `scene-types` | Renderer-neutral CPU scene descriptors (`SceneData`, `MeshData`, `ImageData`, `MaterialDesc`, `InstanceDesc`), camera/light values, validation, normal/tangent preparation. No GPU, windowing or I/O. |
| `scene-renderer` | `GpuContext`, `Renderer` (typed generational `SceneId` handles, transactional uploads, material types registered by key with per-type pipelines, draws batched by material), `RenderView`, frame encoding into caller-owned encoders, exact-frame capture/readback, and its shader package (`scene-renderer/shaders`: prelude defining the material ABI, compose pass, stock `basic` diffuse/normal material). No window, files or URLs. |
| `scene-viewer` | Optional host layer: `SurfacePresenter`, `Viewer`, `ViewerHandle` command/status queue, keyboard navigation, shader loading from a directory or base URL, and a winit `ViewerApp` runner. |
| `nif-scene` | NIF/`.mesh`/`.mat` parsing (`nif`), staged dependency sessions (`load_session`), DDS decoding and ORM packing (`textures`), the `bethesda-pbr` material type (`materials`, WGSL in `nif-scene/shaders`), NIF → `SceneData` conversion (`nif_scene::nif_to_scene`) and native data-folder resolution (`disk`). No GPU, window or browser code. |
| `nif-viewer-lib` | Entry points only: native `run()` and the wasm-bindgen exports, orchestrating `nif-scene` and `scene-viewer`. Re-exports `nif` for compatibility. |
| `nif-viewer-app` | Native executable (initialises logging, calls `nif_viewer_lib::run`). |

Dependency direction: `nif-viewer-lib → {nif-scene, scene-viewer}`,
`scene-viewer → scene-renderer → scene-types`, `nif-scene → scene-types`.
The renderer accepts only decoded in-memory images (`ImageData`: RGBA8 plus
colour space); game texture formats such as DDS are decoded in `nif-scene`.
See `scene-renderer/src/lib.rs` for the frame/capture contract and
`scene-types/src/lib.rs` for coordinate, matrix, UV, tangent and colour conventions.

### Materials

The renderer owns the frame: passes, camera and light uniforms, vertex and
instance layout, and output encoding. Its WGSL prelude
(`scene-renderer/shaders/prelude.wgsl`, shader ABI 2) defines that contract
and helpers such as `standard_vertex`, `tone_map` and `decode_normal`.
Material types own their shading: a `MaterialTypeDesc` declares a key, WGSL
source and bindings (texture slots with colour space and fallback texel, plus
an optional uniform block), and is registered with
`Renderer::register_material_type`. Materials refer to their type by string
key, so scenes stay serialisable and the renderer batches draws by type.

- `basic` (stock, `scene_renderer::stock::BasicMaterial`): diffuse + normal
  map, Lambert lighting.
- `bethesda-pbr` (`nif_scene::materials`): Cook-Torrance GGX with packed ORM,
  used for all NIF materials.

Shader and material packages share one manifest format: `files` for renderer
stages, `materials` for type key → WGSL file.

Minimal offscreen consumer (no window or NIF code):
`cargo run -p scene-renderer --example offscreen_triangle`.

## Prerequisites

### Build and release

- Rust

### Unpacking Resources

- Starfield
- Starfield Creation Kit

## Native viewer

Run the desktop app with a NIF path as the first CLI argument, or via the `NIF_PATH`
environment variable:

```powershell
cargo run -p nif-viewer-app -- "C:\path\to\data\meshes\weapons\ar99\ar99.nif"
# or
$env:NIF_PATH = 'C:\path\to\data\meshes\weapons\ar99\ar99.nif'; cargo run -p nif-viewer-app
```

Dependencies (`geometries\*.mesh`, `materials\*.mat`, `textures\*.dds`) are resolved
automatically by walking up from the NIF to find the data root. When `.mat` files are
absent (vanilla Starfield keeps materials in a binary cdb), textures are matched
heuristically from the material path stem against the `textures\` tree.

Native environment variables:

| Variable | Effect |
|---|---|
| `NIF_PATH` | NIF file to load at startup (CLI arg takes precedence) |
| `NIF_VIEWER_SHADER_DIR` | Directory containing the renderer's shader `manifest.json`. Otherwise `<exe dir>/shaders`, `./shaders`, then (development builds) `scene-renderer/shaders` are searched |
| `NIF_VIEWER_MATERIAL_DIR` | Directory containing the NIF material package's `manifest.json`. Otherwise `<exe dir>/materials/nif`, `./materials/nif`, then (development builds) `nif-scene/shaders` are searched |
| `NIF_CAPTURE=<frame#>` | Automatically capture that frame number to `screenshot_NNNN_frame_{scene,output}.png` in the working directory (linear scene colour converted to sRGB, and the presented output when the surface allows copies) — useful for headless/scripted rendering checks |
| `NIF_CAM_SPEED=<f32>` | Camera navigation speed; overrides the automatic scale-aware default (`bounding radius / 50`) applied when a model loads |
| `NIF_LIGHT_COLOR="r,g,b"` | Directional light colour as 0-1 floats (e.g. `"1,0.2,0.2"`) |
| `NIF_LIGHT_DIR="x,y,z"` | Direction the light travels (normalized internally); setting this disables the slow auto-orbit |
| `NIF_LIGHT_INTENSITY=<f32>` | Light intensity (default 8.0) |
| `NIF_LIGHT_AMBIENT=<f32>` | Constant ambient term (default 0.25) |
| `RUST_LOG` | Standard `env_logger` filter (e.g. `info`); dependency resolution and heuristic texture hits are logged at `info`/`warn` |

`Backspace` captures the next frame interactively at any time. `[` / `]` halve /
double the camera speed at runtime (the new value is logged).

### Rendering

Materials use Bethesda-style roughness/metalness PBR (Cook-Torrance GGX),
the `bethesda-pbr` material type from `nif-scene`.
Alongside `_color` and `_normal`, the `_rough`, `_metal` and `_ao` greyscale
maps are resolved from `.mat` files (or heuristically) and packed CPU-side
into a single linear ORM texture (R=AO, G=roughness, B=metalness); missing
channels fall back to AO=1, roughness≈0.78, metalness=0. Lighting is a
single configurable directional light plus constant ambient, Reinhard
tone-mapped into a linear `Rgba16Float` scene-colour target (rendered at 2×
resolution); a compose pass resamples it to the output and gamma-encodes
manually when the output format is non-sRGB. Missing tangents are generated
on the CPU from UVs, with handedness preserved.

Known limitations: NIF alpha blend/test flags are carried into the scene as
`AlphaMode::Blend`/`Mask` but drawn opaque (each upload reports a diagnostic);
NIF vertex colours are preserved but not applied; NIF two-sided flags are not
interpreted (all materials draw both faces).

### Regression tests

`cargo test -p nif-viewer-lib --test regression` renders a generated NIF
offscreen and compares conversion values, pixel samples and coverage against
baselines recorded before the renderer was extracted into the `scene-*`
crates. `cargo test -p scene-renderer --test offscreen` covers handles,
rollback, captures, colour spaces and transforms. GPU tests skip without an
adapter unless `NIF_VIEWER_REQUIRE_GPU=1`.

For the browser path, build the webapp and run
`node nif-viewer-webapp/scripts/webgpu-smoke.mjs <path to msedge/chrome>`; it
serves `dist/` under a nested URL, loads a generated NIF and samples the
presented WebGPU canvas.

Tests against real Starfield assets are opt-in: set `NIF_STARFIELD_DATA_ROOT`
to an extracted data folder and run `cargo test -p nif-scene -- --ignored`.

## C# / Blazor NuGet package

`nif-viewer-blazor/NifViewer.Blazor` is a Razor class library (net10.0 + net11.0 preview) that
packages the wasm viewer for BlazorWasm consumers, including the interop JS and
the renderer's shader manifest and WGSL files as static web assets. Build the wasm module and
pack it (or just run `./build.ps1`, which does both):

```pwsh
# from nif-viewer-lib:
wasm-pack build --release --target web --out-dir pkg-web
# from the repo root:
dotnet pack nif-viewer-blazor/NifViewer.Blazor -c Release -o artifacts/
```

Consumer sketch:

```csharp
builder.Services.AddScoped<NifViewerInterop>();           // Program.cs
// <NifViewerCanvas @ref="_viewer" />                     // page markup
await _viewer.LoadNifAsync("gun.nif", nifBytes, new HttpNifDependencyResolver(Http, "data"));
```

The package also ships `NifStructureView`, a collapsible tree view of a NIF's
raw block structure (header, block graph, per-block fields) fed by
`NifViewerInterop.ParseNifStructureAsync(nifBytes)` — no renderer needed.

See `nif-viewer-blazor/NifViewer.Blazor/README.md` for the full API.

## Using the wasm package from Blazor / JS


Build the package (from `nif-viewer-lib`):

```pwsh
wasm-pack build --release
```

This produces `nif-viewer-lib/pkg`, an ES module exporting:

- `attach(canvasId: string, shaderBaseUrl?: string, materialBaseUrl?: string): Promise<void>` — attach the renderer to an existing `<canvas>` element by id. Fetches the renderer's `manifest.json` and WGSL from `shaderBaseUrl` (default `"shaders/"`) and the NIF material package from `materialBaseUrl` (default `"materials/nif/"`), both relative to the page, and resolves once the viewer is ready; rejects with the initialisation error (e.g. no WebGPU adapter, missing or ABI-mismatched shaders). Calling it again re-homes the live canvas.
- `run_wasm()` — legacy entry point that creates and appends its own canvas.
- `viewer_status(): object` — `{ state, error, load: { ticket, name, state, error, diagnostics } }`. Viewer `state`: `initializing` / `ready` / `suspended` / `device-lost` / `failed` / `disposed`. Load `state`: `queued` / `uploaded` / `rendered` (first frame submitted) / `failed` / `superseded`.
- `set_camera_speed(speed: number)` — set the navigation speed (applied on the next frame; loading a model resets it to a scale-aware default of `bounding radius / 50`).
- `set_light(r, g, b, x, y, z, intensity, ambient, autoOrbit: boolean)` — configure the directional light: colour (0-1 floats), travel direction (normalized internally), intensity, constant ambient, and whether the direction slowly auto-orbits about the Y axis.
- `parse_nif_structure(nifBytes: Uint8Array): object` — parse the raw block structure (header, block list with per-block fields and child refs, string table) without rendering; useful for introspection UIs.

Commands issued before `attach` resolves are queued and applied once the
viewer is ready. Serve the contents of `scene-renderer/shaders` at the shader
base URL and `nif-scene/shaders` at the material base URL (each includes its
`manifest.json`); the webapp's webpack config copies them to `dist/shaders`
and `dist/materials/nif`.

### Recommended: streaming session API

Instead of reading every candidate dependency into memory up front, parse the
NIF first, then stream only the files it actually references, in chunks:

- `load_session_begin(name: string, nifBytes: Uint8Array): string[]` — parse the NIF and start a session; returns the required dependency paths (external `.mesh`, `.mat`, direct `.dds`), normalized lowercase forward slashes.
- `load_session_file_begin(path: string, size: number)` — start streaming one dependency file, pre-allocating `size` bytes in wasm memory.
- `load_session_file_chunk(path: string, chunk: Uint8Array)` — append a chunk (each chunk crosses the JS/wasm boundary once; no whole-file JS copy).
- `load_session_file_end(path: string)` — mark the file complete; completed `.mat` files immediately reveal the `.dds` textures they reference.
- `load_session_pending(): string[]` — paths still required but not yet provided (including `.dds` discovered from `.mat`s).
- `load_session_finish(): string[]` — convert the model, queue it for upload and clear the session; returns paths that were required but never provided. Missing dependencies never error — meshes without resolvable files fall back to the default material. Upload and first frame are reported later through `viewer_status()`; the previous model stays visible if the new one fails.

The loop: `begin` → stream each `pending()` file (`file_begin` → `file_chunk`… → `file_end`) → call `pending()` again (`.mat`s reveal `.dds`) → repeat until empty or nothing matches → `finish()`.

### Simple path (small models)

- `load_nif(name: string, bytes: Uint8Array, files: object)` — queue a NIF for rendering in one call. `files` is a plain object mapping lowercase forward-slash relative paths (or bare filenames) to `Uint8Array`s. Fine for small models; for large folders prefer the session API above.

### BlazorWasm sketch

```js
// wwwroot/nifViewer.js
import init, { attach, load_nif } from "./pkg/nif_viewer_lib.js";
export async function start(canvasId) {
  await init();
  await attach(
    canvasId,
    new URL("./shaders/", import.meta.url).href,
    new URL("./materials/nif/", import.meta.url).href,
  );
}
export function load(name, bytes, files) { load_nif(name, bytes, files); }
```

```csharp
var module = await JS.InvokeAsync<IJSObjectReference>("import", "./nifViewer.js");
await module.InvokeVoidAsync("start", "nif-canvas"); // <canvas id="nif-canvas"> in the .razor markup
await module.InvokeVoidAsync("load", "model.nif", nifBytes, filesDict); // byte[] marshals as Uint8Array
```

`filesDict` can be a `Dictionary<string, byte[]>` — it marshals to a plain JS object of `Uint8Array`s.

For large models, use the session API from C#: call `load_session_begin` with
the nif `byte[]`, then for each pending path stream chunks read from a
`Stream` (or slices of a `byte[]`) through `load_session_file_begin` /
`load_session_file_chunk` / `load_session_file_end` via JS interop — each
chunk marshals as a `Uint8Array` directly into wasm memory, so no whole file
is ever materialized on the JS side. Loop on `load_session_pending()` (`.mat`
files reveal `.dds` requirements) and call `load_session_finish()` when done.
