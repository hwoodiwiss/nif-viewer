namespace NifViewer.Blazor;

/// <summary>
/// Directional light settings. Defaults match the Rust renderer's defaults:
/// white light travelling down-forward, intensity 8, ambient 0.25, auto-orbit on.
/// </summary>
public sealed record NifLightSettings
{
    public float R { get; init; } = 1f;
    public float G { get; init; } = 1f;
    public float B { get; init; } = 1f;
    public float DirectionX { get; init; } = -0.4f;
    public float DirectionY { get; init; } = -0.8f;
    public float DirectionZ { get; init; } = -0.45f;
    public float Intensity { get; init; } = 8f;
    public float Ambient { get; init; } = 0.25f;
    public bool AutoOrbit { get; init; } = true;
}
