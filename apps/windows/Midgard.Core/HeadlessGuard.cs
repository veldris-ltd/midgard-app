namespace Midgard.Core;

/// <summary>
/// The guard on <c>Midgard.exe --headless-check</c>, the end-to-end check CI runs. It is in
/// the presenter build (so CI tests the real exe), and it resets the working copy and can
/// run a stub in place of the model server, so it must never run against a presenter's own
/// demo by accident.
/// </summary>
public static class HeadlessGuard
{
    /// <summary>The opt-in: the check runs only with this set to 1.</summary>
    public const string OptIn = "MIDGARD_HEADLESS_CHECK";

    /// <summary>The environment keys the guard reads.</summary>
    public static readonly IReadOnlyList<string> EnvironmentKeys = [OptIn, "MIDGARD_APP_HOME", "MIDGARD_APP_LOGS", "MIDGARD_APP_WEBVIEW"];

    /// <summary>
    /// Why the check must not run, or null: it needs <see cref="OptIn"/>=1 and
    /// MIDGARD_APP_HOME, MIDGARD_APP_LOGS and MIDGARD_APP_WEBVIEW all set explicitly (to a
    /// throw-away working copy, logs and browser profile, as the windows-shell-e2e job does),
    /// so the presenter's <c>%LOCALAPPDATA%\Midgard</c> (the working copy it resets, the
    /// profile whose data it clears) is never touched by it.
    /// </summary>
    public static string? Refusal(IReadOnlyDictionary<string, string> environment)
    {
        string? Value(string key) =>
            environment.TryGetValue(key, out var v) && !string.IsNullOrWhiteSpace(v) ? v : null;
        if (Value(OptIn) != "1")
            return $"--headless-check is for tests only: it resets the working copy. Set {OptIn}=1 to run it.";
        var missing = new[] { "MIDGARD_APP_HOME", "MIDGARD_APP_LOGS", "MIDGARD_APP_WEBVIEW" }.Where(k => Value(k) is null).ToList();
        if (missing.Count > 0)
            return $"--headless-check needs {Names(missing)} set to throw-away folders: "
                 + "it resets the working copy, clears the browser profile, and must never touch the presenter's own.";
        return null;

        static string Names(List<string> names) =>
            names.Count == 1 ? names[0] : string.Join(", ", names.Take(names.Count - 1)) + " and " + names[^1];
    }

    /// <summary>The guard's keys from the process environment.</summary>
    public static Dictionary<string, string> ProcessEnvironment()
    {
        var env = new Dictionary<string, string>();
        foreach (var key in EnvironmentKeys)
        {
            if (Environment.GetEnvironmentVariable(key) is { } value) env[key] = value;
        }
        return env;
    }
}
