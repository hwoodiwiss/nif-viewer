# NifViewer.Blazor

Blazor WebAssembly component wrapping the wgpu-based NIF (Gamebryo/Starfield)
model viewer compiled to WebAssembly (WebGPU required — a recent Chromium or
Firefox with WebGPU enabled).

## Setup

Register the interop service in `Program.cs`:

```csharp
builder.Services.AddScoped<NifViewerInterop>();
```

## Usage

```razor
@using NifViewer.Blazor

<NifViewerCanvas @ref="_viewer" />

@code {
    private NifViewerCanvas _viewer = default!;

    private async Task Load()
    {
        var nifBytes = await Http.GetByteArrayAsync("models/gun.nif");
        var resolver = new HttpNifDependencyResolver(Http, "data"); // GETs data/{path}
        var result = await _viewer.LoadNifAsync("gun.nif", nifBytes, resolver);
        // result.Missing lists dependency paths that could not be resolved.
    }
}
```

Dependencies (external `.mesh` geometry, `.mat` materials, `.dds` textures)
are requested on demand through an `INifDependencyResolver`:

```csharp
public interface INifDependencyResolver
{
    Task<byte[]?> ResolveAsync(string path); // null = not found
}
```

`HttpNifDependencyResolver(HttpClient, baseUrl)` is provided; it GETs
`{baseUrl}/{path}` and returns `null` on any non-success status. Paths are
normalized lowercase with forward slashes (e.g. `textures/foo_color.dds`).

Camera speed and lighting:

```csharp
await _viewer.SetCameraSpeedAsync(2.0f);
await _viewer.SetLightAsync(new NifLightSettings { Intensity = 8f, Ambient = 0.25f, AutoOrbit = true });
```

Or drop in the ready-made control bar (camera speed, light colour,
azimuth/elevation, intensity, ambient, auto-orbit — defaults match the
renderer's):

```razor
<NifViewerCanvas @ref="_viewer" />
<NifViewerControls Viewer="_viewer" />
```

Moving the azimuth or elevation slider turns auto-orbit off, matching the
standalone webapp's behaviour.

## Readiness and load status

`AttachAsync` completes once the GPU device, shaders and pipelines are ready,
and throws if WebGPU or the shader assets are unavailable. `NifViewerCanvas`
calls it on first render and reports failures without throwing:

```razor
<NifViewerCanvas @ref="_viewer" OnInitializationFailed="m => _error = m" />
```

`LoadNifAsync` completes when the model is
converted and queued; observe upload and the first rendered frame with
`GetStatusAsync`:

```csharp
var result = await _viewer.LoadNifAsync("gun.nif", nifBytes, resolver);
var status = await _viewer.GetStatusAsync();
if (status.Load.Ticket == result.LoadTicket && status.Load.State == "failed")
{
    Console.WriteLine(status.Load.Error); // the previous model is still shown
}
foreach (var diagnostic in status.Load.Diagnostics)
{
    Console.WriteLine(diagnostic); // e.g. "texture not found: textures/foo_n.dds"
}
```

`State` values: `queued` → `uploaded` → `rendered` (first frame submitted —
not proof of correct visual output), or `failed` / `superseded`. Missing
dependencies are still reported through `NifLoadResult.Missing`.

See `CHANGELOG.md` for the full list of public API changes per version.

## Structure introspection

`NifStructureView` renders the raw block graph of a NIF (header summary plus
a collapsible tree of blocks with their parsed fields) without rendering the
model — useful for debugging. Parse the bytes with
`NifViewerInterop.ParseNifStructureAsync`, which returns a
`System.Text.Json.JsonElement` (kept schemaless on the C# side so new fields
from the Rust parser flow through without API changes):

```razor
@using System.Text.Json
@inject NifViewerInterop Interop

<NifStructureView Structure="_structure" />

@code {
    private JsonElement? _structure;

    private async Task Load()
    {
        var nifBytes = await Http.GetByteArrayAsync("models/gun.nif");
        _structure = await Interop.ParseNifStructureAsync(nifBytes);
    }
}
```

Blocks reachable from the file's roots form the tree (child nodes, extra
data, controller, shader/alpha/skin/texture-set refs); everything else is
listed under an "Unreferenced" section. Blocks the parser does not decode
still appear with their type and size (`fields: { "skipped": true }`).

## Asset serving note

The renderer loads `shaders/manifest.json` and the WGSL files it lists from
`_content/NifViewer.Blazor/shaders/`, using a URL derived from the interop
module's own location. No host configuration or global `fetch` patching is
needed, and the host's own `shaders/` or `resources/` paths are not affected.
The manifest declares a shader ABI version; a mismatched or missing manifest
makes `AttachAsync` throw with the offending URL.
