using System.Text.Json.Serialization;

namespace NifViewer.Blazor;

/// <summary>Shared Fly/Orbit controls. Speed is measured in world units per second.</summary>
public sealed record NifNavigationSettings
{
    [JsonPropertyName("mode")] public string Mode { get; init; } = "Fly";
    [JsonPropertyName("speed")] public float Speed { get; init; } = 12;
    [JsonPropertyName("automatic_speed")] public bool AutomaticSpeed { get; init; } = true;
    [JsonPropertyName("mouse_sensitivity")] public float MouseSensitivity { get; init; } = 0.003f;
    [JsonPropertyName("keyboard_look_speed")] public float KeyboardLookSpeed { get; init; } = 1.5f;
    [JsonPropertyName("stick_look_speed")] public float StickLookSpeed { get; init; } = 2.5f;
    [JsonPropertyName("left_deadzone")] public float LeftDeadzone { get; init; } = 0.15f;
    [JsonPropertyName("right_deadzone")] public float RightDeadzone { get; init; } = 0.15f;
    [JsonPropertyName("trigger_deadzone")] public float TriggerDeadzone { get; init; } = 0.05f;
    [JsonPropertyName("look_exponent")] public float LookExponent { get; init; } = 2;
    [JsonPropertyName("invert_mouse_y")] public bool InvertMouseY { get; init; }
    [JsonPropertyName("invert_stick_y")] public bool InvertStickY { get; init; }
    [JsonPropertyName("boost")] public float Boost { get; init; } = 4;
    [JsonPropertyName("precision")] public float Precision { get; init; } = 0.2f;
}

public sealed record NifNavigationStatus
{
    public NifNavigationSettings? Settings { get; init; }
    public bool Active { get; init; }
    public bool Captured { get; init; }
    public string Device { get; init; } = "";
    public string Help { get; init; } = "";
}
