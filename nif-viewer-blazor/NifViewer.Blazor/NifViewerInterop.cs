using Microsoft.JSInterop;

namespace NifViewer.Blazor;

/// <summary>
/// JS interop wrapper around the wasm NIF viewer. Register with
/// <c>builder.Services.AddScoped&lt;NifViewerInterop&gt;()</c>.
/// </summary>
public sealed class NifViewerInterop(IJSRuntime jsRuntime) : IAsyncDisposable
{
    private const string ModulePath = "./_content/NifViewer.Blazor/nifviewer.interop.js";

    private IJSObjectReference? _module;

    private async ValueTask<IJSObjectReference> GetModuleAsync() =>
        _module ??= await jsRuntime.InvokeAsync<IJSObjectReference>("import", ModulePath);

    /// <summary>Loads the interop module and initializes the wasm runtime. Idempotent.</summary>
    public async Task InitializeAsync()
    {
        var module = await GetModuleAsync();
        await module.InvokeVoidAsync("init");
    }

    /// <summary>Starts the renderer attached to the canvas with the given element id.</summary>
    public async Task AttachAsync(string canvasId)
    {
        await InitializeAsync();
        var module = await GetModuleAsync();
        await module.InvokeVoidAsync("attach", canvasId);
    }

    /// <summary>
    /// Loads a NIF model, resolving external dependencies (meshes, materials,
    /// textures) through <paramref name="resolver"/> if provided.
    /// </summary>
    public async Task<NifLoadResult> LoadNifAsync(string name, byte[] nif, INifDependencyResolver? resolver = null)
    {
        var module = await GetModuleAsync();
        using var resolverRef = resolver is null ? null : DotNetObjectReference.Create(new ResolverBridge(resolver));
        var result = await module.InvokeAsync<LoadResultDto>("loadNif", name, nif, resolverRef);
        return new NifLoadResult(result.Missing ?? []);
    }

    public async Task SetCameraSpeedAsync(float speed)
    {
        var module = await GetModuleAsync();
        await module.InvokeVoidAsync("setCameraSpeed", speed);
    }

    /// <summary>
    /// Parses the raw block structure of a NIF (header, block list, refs,
    /// string table) without rendering it. Returned as a
    /// <see cref="System.Text.Json.JsonElement"/> so the C# surface stays
    /// stable as the Rust side adds fields; pass it to
    /// <c>&lt;NifStructureView Structure="..." /&gt;</c>.
    /// </summary>
    public async Task<System.Text.Json.JsonElement> ParseNifStructureAsync(byte[] nif)
    {
        await InitializeAsync();
        var module = await GetModuleAsync();
        return await module.InvokeAsync<System.Text.Json.JsonElement>("parseNifStructure", nif);
    }

    public async Task SetLightAsync(NifLightSettings settings)
    {
        var module = await GetModuleAsync();
        await module.InvokeVoidAsync(
            "setLight",
            settings.R, settings.G, settings.B,
            settings.DirectionX, settings.DirectionY, settings.DirectionZ,
            settings.Intensity, settings.Ambient, settings.AutoOrbit);
    }

    public async ValueTask DisposeAsync()
    {
        if (_module is not null)
        {
            try
            {
                await _module.DisposeAsync();
            }
            catch (JSDisconnectedException)
            {
                // Circuit/page already gone; nothing to clean up.
            }
        }
    }

    private sealed record LoadResultDto(string[]? Missing);

    /// <summary>Exposes an <see cref="INifDependencyResolver"/> to JS.</summary>
    private sealed class ResolverBridge(INifDependencyResolver resolver)
    {
        [JSInvokable]
        public Task<byte[]?> ResolveAsync(string path) => resolver.ResolveAsync(path);
    }
}
