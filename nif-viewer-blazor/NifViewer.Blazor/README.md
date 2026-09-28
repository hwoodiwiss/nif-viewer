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

The wasm renderer fetches `shaders/*.wgsl` and `resources/cube/*` with
relative URLs at startup. These files ship inside this package as static web
assets under `_content/NifViewer.Blazor/`, and the interop module installs a
small `window.fetch` patch that transparently redirects `shaders/...` and
`resources/...` requests there — no host configuration is needed. If your app
serves its own files under top-level `shaders/` or `resources/` paths, those
will be shadowed by the redirect; rename them or serve them elsewhere.

## Navigation (preview.6)

`NifViewerCanvas` supports keyboard, mouse and standard-mapped controllers. Its
input attachment is automatically deactivated on component disposal, with a
generation token protecting a newer canvas attachment from stale disposal.

Use `SetNavigationSettingsAsync(new NifNavigationSettings { Speed = 25,
AutomaticSpeed = false })` for units/second configuration. Existing
`SetCameraSpeedAsync(0.2f)` remains a legacy adapter equivalent to 12 units/second.
`NavigationCommandAsync("Frame")` and `NavigationCommandAsync("Reset")` recover
a useful view. `GetNavigationStatusAsync()` returns actual settings/device status.
`NifViewerControls` shows mode, speed, help and navigation status.

Mouse capture is requested by L in the canvas's browser event handler. Escape/B
releases navigation; Tab leaves the canvas. Pairing/file selection remains host UI.
