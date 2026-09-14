# NIF Viewer

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
| `NIF_CAPTURE=<frame#>` | Automatically capture the deferred G-buffers on that frame number to `screenshot_NNNN_*.png` in the working directory — useful for headless/scripted rendering checks |
| `NIF_CAM_SPEED=<f32>` | Camera navigation speed; overrides the automatic scale-aware default (`bounding radius / 50`) applied when a model loads |
| `NIF_LIGHT_COLOR="r,g,b"` | Directional light colour as 0-1 floats (e.g. `"1,0.2,0.2"`) |
| `NIF_LIGHT_DIR="x,y,z"` | Direction the light travels (normalized internally); setting this disables the slow auto-orbit |
| `NIF_LIGHT_INTENSITY=<f32>` | Light intensity (default 3.0) |
| `NIF_LIGHT_AMBIENT=<f32>` | Constant ambient term (default 0.08) |
| `RUST_LOG` | Standard `env_logger` filter (e.g. `info`); dependency resolution and heuristic texture hits are logged at `info`/`warn` |

`Backspace` captures the G-buffers interactively at any time. `[` / `]` halve /
double the camera speed at runtime (the new value is logged).

### Rendering

Materials use Bethesda-style roughness/metalness PBR (Cook-Torrance GGX).
Alongside `_color` and `_normal`, the `_rough`, `_metal` and `_ao` greyscale
maps are resolved from `.mat` files (or heuristically) and packed CPU-side
into a single linear ORM texture (R=AO, G=roughness, B=metalness); missing
channels fall back to AO=1, roughness≈0.78, metalness=0. Lighting is a
single configurable directional light plus constant ambient, Reinhard
tone-mapped (with manual gamma encoding when the surface format is
non-sRGB).

## C# / Blazor NuGet package

`nif-viewer-blazor/NifViewer.Blazor` is a Razor class library (net10.0 + net11.0 preview) that
packages the wasm viewer for BlazorWasm consumers, including the interop JS,
shaders and fallback resources as static web assets. Build the wasm module and
pack it (or just run `./build.ps1`, which does both):

```pwsh
# from nif-viewer-lib:
$env:RUSTFLAGS = '--cfg=web_sys_unstable_apis --cfg getrandom_backend="wasm_js"'
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
$env:RUSTFLAGS = '--cfg=web_sys_unstable_apis --cfg getrandom_backend="wasm_js"'
wasm-pack build --release
```

This produces `nif-viewer-lib/pkg`, an ES module exporting:

- `attach(canvasId: string)` — attach the renderer to an existing `<canvas>` element by id and start the event loop.
- `run_wasm()` — legacy entry point that creates and appends its own canvas.
- `set_camera_speed(speed: number)` — set the navigation speed (applied on the next event-loop tick; loading a model resets it to a scale-aware default of `bounding radius / 50`).
- `set_light(r, g, b, x, y, z, intensity, ambient, autoOrbit: boolean)` — configure the directional light: colour (0-1 floats), travel direction (normalized internally), intensity, constant ambient, and whether the direction slowly auto-orbits about the Y axis.
- `parse_nif_structure(nifBytes: Uint8Array): object` — parse the raw block structure (header, block list with per-block fields and child refs, string table) without rendering; useful for introspection UIs.

### Recommended: streaming session API

Instead of reading every candidate dependency into memory up front, parse the
NIF first, then stream only the files it actually references, in chunks:

- `load_session_begin(name: string, nifBytes: Uint8Array): string[]` — parse the NIF and start a session; returns the required dependency paths (external `.mesh`, `.mat`, direct `.dds`), normalized lowercase forward slashes.
- `load_session_file_begin(path: string, size: number)` — start streaming one dependency file, pre-allocating `size` bytes in wasm memory.
- `load_session_file_chunk(path: string, chunk: Uint8Array)` — append a chunk (each chunk crosses the JS/wasm boundary once; no whole-file JS copy).
- `load_session_file_end(path: string)` — mark the file complete; completed `.mat` files immediately reveal the `.dds` textures they reference.
- `load_session_pending(): string[]` — paths still required but not yet provided (including `.dds` discovered from `.mat`s).
- `load_session_finish()` — queue the model for rendering and clear the session. Missing dependencies never error — meshes without resolvable files fall back to the default material.

The loop: `begin` → stream each `pending()` file (`file_begin` → `file_chunk`… → `file_end`) → call `pending()` again (`.mat`s reveal `.dds`) → repeat until empty or nothing matches → `finish()`.

### Simple path (small models)

- `load_nif(name: string, bytes: Uint8Array, files: object)` — queue a NIF for rendering in one call. `files` is a plain object mapping lowercase forward-slash relative paths (or bare filenames) to `Uint8Array`s. Fine for small models; for large folders prefer the session API above.

### BlazorWasm sketch

```js
// wwwroot/nifViewer.js
import init, { attach, load_nif } from "./pkg/nif_viewer_lib.js";
export async function start(canvasId) { await init(); attach(canvasId); }
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
