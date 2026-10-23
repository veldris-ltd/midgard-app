using System.Text.Json;
using System.Text.Json.Nodes;

namespace Midgard.Core;

/// <summary>
/// The few answers the app remembers, in <see cref="Paths.Settings"/>
/// (<c>%LOCALAPPDATA%\Midgard\settings.json</c>, outside the working copy so Reset demo keeps
/// them). Today only the first run's Start-menu question. A missing or unreadable file
/// reads as nothing answered; unknown keys are kept when it is written back.
/// </summary>
public sealed class AppSettings
{
    const string ShortcutKey = "startMenuShortcut";

    static readonly JsonSerializerOptions Indented = new() { WriteIndented = true };
    readonly JsonObject values;

    AppSettings(JsonObject values) => this.values = values;

    /// <summary>The answer to "Add Midgard to the Start menu?": "added", "declined", or null if never asked.</summary>
    public string? StartMenuShortcut
    {
        get => values[ShortcutKey] is JsonValue v && v.TryGetValue<string>(out var s) ? s : null;
        set
        {
            if (value is null) values.Remove(ShortcutKey);
            else values[ShortcutKey] = value;
        }
    }

    public static AppSettings Load(string file)
    {
        try
        {
            if (JsonNode.Parse(File.ReadAllText(file)) is JsonObject found) return new AppSettings(found);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        {
        }
        return new AppSettings(new JsonObject());
    }

    /// <summary>Writes the file (through a temporary file, so a crash never leaves half of it).</summary>
    public void Save(string file)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(file))!);
        var temporary = file + ".tmp";
        File.WriteAllText(temporary, values.ToJsonString(Indented));
        File.Move(temporary, file, overwrite: true);
    }

    /// <summary>
    /// Whether the first run offers a Start-menu shortcut: in an installed copy (not a
    /// development layout), never asked before, and with no shortcut there already.
    /// </summary>
    public static bool ShouldOfferShortcut(AppSettings settings, bool devLayout, bool shortcutExists) =>
        !devLayout && settings.StartMenuShortcut is null && !shortcutExists;
}
