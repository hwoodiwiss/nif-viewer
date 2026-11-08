using System.Text.Json.Serialization;

namespace NifViewer.Blazor;

/// <summary>Viewer lifecycle and the latest load request.</summary>
/// <param name="State">
/// <c>initializing</c>, <c>ready</c>, <c>suspended</c> (zero-sized canvas),
/// <c>device-lost</c>, <c>failed</c> or <c>disposed</c>.
/// </param>
/// <param name="Error">Initialisation or device-loss message, if any.</param>
/// <param name="Load">The most recent load request.</param>
public sealed record NifViewerStatus(
    [property: JsonPropertyName("state")] string State,
    [property: JsonPropertyName("error")] string? Error,
    [property: JsonPropertyName("load")] NifLoadStatus Load)
{
    /// <summary>True once the renderer can draw.</summary>
    public bool IsReady => State is "ready" or "suspended";
}

/// <summary>Progress of a load request.</summary>
/// <param name="Ticket">Matches <see cref="NifLoadResult.LoadTicket"/>; zero before any load.</param>
/// <param name="Name">Name passed to the load call.</param>
/// <param name="State">
/// <c>queued</c>, <c>uploaded</c>, <c>rendered</c> (first frame submitted, which does
/// not prove correct visual output), <c>failed</c>, <c>superseded</c>, or empty.
/// The previous model stays visible when a load fails.
/// </param>
/// <param name="Error">Conversion or upload error for <c>failed</c> loads.</param>
/// <param name="Diagnostics">
/// Missing or undecodable textures/materials and features the renderer displays
/// approximately (for example alpha blending), with NIF resource paths.
/// </param>
public sealed record NifLoadStatus(
    [property: JsonPropertyName("ticket")] long Ticket,
    [property: JsonPropertyName("name")] string Name,
    [property: JsonPropertyName("state")] string State,
    [property: JsonPropertyName("error")] string? Error,
    [property: JsonPropertyName("diagnostics")] IReadOnlyList<string> Diagnostics);
