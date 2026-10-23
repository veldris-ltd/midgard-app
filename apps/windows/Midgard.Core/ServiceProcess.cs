namespace Midgard.Core;

/// <summary>One running service as the <see cref="Supervisor"/> sees it.</summary>
public interface IServiceProcess : IDisposable
{
    int Pid { get; }

    /// <summary>Completes with the exit code when the process has exited.</summary>
    Task<int> Exited { get; }

    /// <summary>
    /// Ends the process and every process it started (a replay's heimdall-source runs), at
    /// once. Safe to call after it exited.
    /// </summary>
    void Kill();

    /// <summary>How many processes of this service (it and its descendants) are still running.</summary>
    int ActiveProcesses { get; }
}

/// <summary>Starts service processes.</summary>
public interface IProcessLauncher
{
    /// <summary>
    /// Starts <paramref name="executable"/> with <paramref name="arguments"/>, stdin from the
    /// null device and stdout and stderr appended to <paramref name="logFile"/>.
    /// </summary>
    /// <exception cref="IOException">It could not be started (the message says why).</exception>
    IServiceProcess Start(string executable, IReadOnlyList<string> arguments,
                          IReadOnlyDictionary<string, string>? environment, string logFile);
}

/// <summary>How a process's command line is written on Windows.</summary>
public static class CommandLine
{
    /// <summary>
    /// <paramref name="executable"/> then each argument, quoted so the C runtime's parser
    /// (and CommandLineToArgvW, and Rust's std::env::args) reads back exactly the same list.
    /// </summary>
    public static string Build(string executable, IEnumerable<string> arguments)
    {
        var line = new System.Text.StringBuilder();
        // The program name is parsed differently: no backslash escapes, quotes only.
        line.Append('"').Append(executable).Append('"');
        foreach (var argument in arguments)
        {
            line.Append(' ');
            Append(line, argument);
        }
        return line.ToString();
    }

    static void Append(System.Text.StringBuilder line, string argument)
    {
        if (argument.Length > 0 && argument.IndexOfAny([' ', '\t', '\n', '\v', '"']) < 0)
        {
            line.Append(argument);
            return;
        }
        line.Append('"');
        for (var i = 0; i < argument.Length; i++)
        {
            var backslashes = 0;
            while (i < argument.Length && argument[i] == '\\')
            {
                backslashes++;
                i++;
            }
            if (i == argument.Length)
            {
                // Before the closing quote every backslash is doubled.
                line.Append('\\', backslashes * 2);
                break;
            }
            if (argument[i] == '"')
            {
                line.Append('\\', backslashes * 2 + 1).Append('"');
            }
            else
            {
                line.Append('\\', backslashes).Append(argument[i]);
            }
        }
        line.Append('"');
    }

    /// <summary>
    /// A CreateProcess environment block: the current environment with
    /// <paramref name="extra"/> on top, sorted by name as Windows requires, NUL-separated and
    /// ending with two NULs.
    /// </summary>
    public static string EnvironmentBlock(IReadOnlyDictionary<string, string> current,
                                          IReadOnlyDictionary<string, string> extra)
    {
        var merged = new SortedDictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (var (key, value) in current) merged[key] = value;
        foreach (var (key, value) in extra) merged[key] = value;
        var block = new System.Text.StringBuilder();
        foreach (var (key, value) in merged) block.Append(key).Append('=').Append(value).Append('\0');
        if (merged.Count == 0) block.Append('\0');
        block.Append('\0');
        return block.ToString();
    }
}
