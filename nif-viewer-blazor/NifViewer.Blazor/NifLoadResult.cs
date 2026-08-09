namespace NifViewer.Blazor;

/// <summary>Result of a NIF load: dependency paths that were required but never resolved.</summary>
public sealed record NifLoadResult(IReadOnlyList<string> Missing);
