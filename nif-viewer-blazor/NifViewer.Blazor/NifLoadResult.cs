namespace NifViewer.Blazor;

/// <summary>Result of a NIF load: dependency paths that were required but never resolved.</summary>
/// <remarks>
/// Returning from a load means the model was converted and queued, not that it is
/// on screen. Use <see cref="LoadTicket"/> with <see cref="NifViewerStatus.Load"/>
/// to observe upload and first-frame milestones.
/// </remarks>
public sealed record NifLoadResult(IReadOnlyList<string> Missing)
{
    /// <summary>
    /// Identifies this load in <see cref="NifViewerStatus.Load"/>. Zero when the NIF
    /// could not be converted (see <see cref="NifLoadStatus.Error"/>).
    /// </summary>
    public long LoadTicket { get; init; }
}
