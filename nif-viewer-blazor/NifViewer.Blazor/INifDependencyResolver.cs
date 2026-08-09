namespace NifViewer.Blazor;

/// <summary>
/// Resolves a NIF dependency path (e.g. <c>geometries/ab/cd1234.mesh</c>,
/// <c>textures/foo_color.dds</c>) to its file bytes, or <c>null</c> if unavailable.
/// Paths are normalized lowercase with forward slashes.
/// </summary>
public interface INifDependencyResolver
{
    Task<byte[]?> ResolveAsync(string path);
}
