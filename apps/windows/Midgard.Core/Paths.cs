namespace Midgard.Core;

/// <summary>
/// Where the shell finds its bundled files and keeps its working copy and logs. Built
/// from an environment so tests and development runs can point it elsewhere. The port of
/// apps/mac/Sources/MidgardShellCore/Paths.swift.
/// </summary>
/// <remarks>
/// The extracted ZIP is one folder: <c>Midgard\Midgard.exe</c>, <c>Midgard\helpers\</c>
/// (the service executables) and <c>Midgard\resources\</c> (consoles, home,
/// starting-state, models …). Everything the shell writes is under
/// <c>%LOCALAPPDATA%\Midgard</c>, in three folders so Reset demo can wipe the working copy
/// without touching the others: <c>Home\</c> (the working copy), <c>Logs\</c> and
/// <c>WebView\</c> (the WebView2 user-data folder).
/// </remarks>
public sealed record Paths
{
    /// <summary>The resources folder (consoles\, home\, starting-state\, models\ …).</summary>
    public string Resources { get; }
    /// <summary>
    /// The service executables: <c>helpers\</c> beside Midgard.exe, or <c>resources\bin</c>
    /// in a development layout (MIDGARD_APP_RESOURCES set).
    /// </summary>
    public string Helpers { get; }
    /// <summary>
    /// True when MIDGARD_APP_RESOURCES points the shell at a development layout: only then
    /// may it fall back to a model server outside the helpers.
    /// </summary>
    public bool DevLayout { get; }
    /// <summary>The working copy of the starting state.</summary>
    public string Home { get; }
    /// <summary>One log per service plus shell.log.</summary>
    public string Logs { get; }
    /// <summary>The WebView2 user-data folder (the window's browser profile).</summary>
    public string WebView { get; }
    /// <summary>The app's few remembered answers (<c>%LOCALAPPDATA%\Midgard\settings.json</c>; see <see cref="AppSettings"/>).</summary>
    public string Settings { get; }

    /// <param name="environment">The process environment (only the MIDGARD_APP_* keys are read).</param>
    /// <param name="bundleResources">The package's resources folder.</param>
    /// <param name="localAppData">The user's %LOCALAPPDATA%.</param>
    /// <param name="bundleHelpers">The package's helpers folder, or null outside a package.</param>
    public Paths(IReadOnlyDictionary<string, string> environment, string bundleResources,
                 string localAppData, string? bundleHelpers = null)
    {
        string? Value(string key) =>
            environment.TryGetValue(key, out var value) && !string.IsNullOrEmpty(value) ? value : null;

        var devResources = Value("MIDGARD_APP_RESOURCES");
        Resources = devResources ?? bundleResources;
        if (bundleHelpers is not null && devResources is null)
        {
            Helpers = bundleHelpers;
            DevLayout = false;
        }
        else
        {
            Helpers = Path.Join(Resources, "bin");
            DevLayout = true;
        }
        var app = Path.Join(localAppData, "Midgard");
        // MIDGARD_APP_HOME moves the working copy only, MIDGARD_APP_LOGS the logs only.
        Home = Value("MIDGARD_APP_HOME") ?? Path.Join(app, "Home");
        Logs = Value("MIDGARD_APP_LOGS") ?? Path.Join(app, "Logs");
        WebView = Path.Join(app, "WebView");
        Settings = Path.Join(app, "settings.json");
    }

    /// <summary>The environment keys <see cref="Paths"/> reads.</summary>
    public static readonly IReadOnlyList<string> EnvironmentKeys =
        ["MIDGARD_APP_RESOURCES", "MIDGARD_APP_HOME", "MIDGARD_APP_LOGS"];

    /// <summary>
    /// The paths for this process: the folder Midgard.exe is in and the user's real
    /// %LOCALAPPDATA%. <paramref name="environment"/> defaults to the process environment.
    /// </summary>
    public static Paths Current(IReadOnlyDictionary<string, string>? environment = null)
    {
        environment ??= ProcessEnvironment();
        // A single-file app's base directory is the folder its exe is in.
        var exeDir = AppContext.BaseDirectory;
        return new Paths(environment, Path.Join(exeDir, "resources"), LocalAppData(),
                         Path.Join(exeDir, "helpers"));
    }

    static Dictionary<string, string> ProcessEnvironment()
    {
        var env = new Dictionary<string, string>();
        foreach (var key in EnvironmentKeys)
        {
            if (Environment.GetEnvironmentVariable(key) is { } value) env[key] = value;
        }
        return env;
    }

    static string LocalAppData()
    {
        var dir = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        if (string.IsNullOrEmpty(dir)) dir = Environment.GetEnvironmentVariable("LOCALAPPDATA") ?? "";
        if (string.IsNullOrEmpty(dir))
            throw new InvalidOperationException("Windows did not say where this user's local app data folder is (%LOCALAPPDATA%).");
        return dir;
    }
}
