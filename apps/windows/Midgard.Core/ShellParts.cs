using System.Globalization;
using System.Text;

namespace Midgard.Core;

// The window's logic that needs no window, ported from apps/mac/Sources/MidgardShellCore/Shell.swift:
// the shell's log, Heimdall's first view, page scripts, the services' names, the port-retry
// loop around Supervisor.StartAsync, the reload budget, the presenter's notes and the
// session timing. The offline rules are in OfflineRules.cs, the flow in ShellModel.cs.

/// <summary>
/// The shell's own steps, one timestamped line each, appended to <c>shell.log</c> (and to
/// stderr when <c>echo</c>). Safe from any thread; a line that cannot be written is dropped.
/// </summary>
public sealed class ShellLog
{
    public string File { get; }
    readonly Lock gate = new();
    readonly bool echo;

    public ShellLog(string file, bool echo = false)
    {
        File = file;
        this.echo = echo;
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(file))!);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    public void Write(string message)
    {
        var line = $"{DateTimeOffset.Now.ToString("yyyy-MM-dd'T'HH:mm:ss.fffzzz", CultureInfo.InvariantCulture)} {message}\n";
        lock (gate)
        {
            try
            {
                using var stream = new FileStream(File, FileMode.Append, FileAccess.Write,
                                                  FileShare.ReadWrite | FileShare.Delete);
                stream.Write(Encoding.UTF8.GetBytes(line));
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            {
            }
            if (echo) Console.Error.Write(line);
        }
    }
}

/// <summary>
/// Heimdall's operating picture remembers 2D or 3D in localStorage. A new presenter gets
/// 2D on Windows, as on an Intel Mac: the PCs this runs on draw the 3D globe on the CPU or
/// a modest integrated GPU. A choice they made since is kept.
/// </summary>
public static class HeimdallView
{
    /// <summary><c>VIEW_KEY</c> in products/heimdall/ui/src/screens/OperatingPictureScreen.tsx.</summary>
    public const string StorageKey = "heimdall.cop.view";

    public const string Preferred = "2d";

    /// <summary>
    /// Document-start script: sets the key to <paramref name="view"/> only if it is not set yet
    /// (or always, with <paramref name="force"/>, e.g. 2D after the 3D globe crashed the page),
    /// and only on the app's own origin.
    /// </summary>
    public static string Script(string origin, string view, bool force = false) =>
        "(function () {\n"
        + $"  if (typeof location === 'undefined' || location.origin !== {Session.Literal(origin)}) {{ return; }}\n"
        + "  try {\n"
        + $"    if ({(force ? "true" : "false")} || localStorage.getItem({Session.Literal(StorageKey)}) === null) {{\n"
        + $"      localStorage.setItem({Session.Literal(StorageKey)}, {Session.Literal(view)});\n"
        + "    }\n"
        + "  } catch (e) {}\n"
        + "})();";
}

/// <summary>JavaScript the shell runs in a page.</summary>
public static class PageScript
{
    /// <summary><c>window.&lt;function&gt;(&lt;text&gt;)</c> if the page defines it; <paramref name="text"/> as a safe literal.</summary>
    public static string Call(string function, string text) =>
        $"if (typeof window.{function} === 'function') {{ window.{function}({Session.Literal(text)}); }}";
}

/// <summary>The presenter-facing names of the services.</summary>
public static class ServiceNames
{
    public static string Friendly(string name) => name switch
    {
        "llama" => "Model",
        "mimir" => "Mimir",
        "gateway" => "Gateway",
        "heimdall" => "Heimdall",
        "forge" => "Forge",
        "replay" => "Replay",
        "analytics" => "Camera analytics",
        "" => "",
        _ => char.ToUpperInvariant(name[0]) + name[1..],
    };

    /// <summary>What the starting screen says beside a service, as StartingView's rows do.</summary>
    public static string StatusLabel(ServiceStatus? status) => status switch
    {
        ServiceStatus.Ready => "Ready",
        ServiceStatus.Starting => "Starting…",
        ServiceStatus.Restarting r => $"Restarting (attempt {r.Attempt})…",
        ServiceStatus.Failed f => f.Reason == "port in use" ? "Port in use, retrying…" : "Failed",
        _ => "Waiting",
    };
}

/// <summary>
/// Starting the suite with ports chosen just before: if a service finds its port taken
/// meanwhile, stop everything and try again with fresh ports.
/// </summary>
public static class SuiteStart
{
    /// <summary>
    /// Calls <paramref name="start"/> with ports from <paramref name="choosePorts"/>; on
    /// <see cref="PortInUseException"/> calls <paramref name="stopAll"/>, reports the retry and
    /// tries again with new ports, at most <paramref name="retries"/> more times. Returns the
    /// ports that worked. Any other error is rethrown as it is (the caller decides whether to
    /// stop what is running); if <paramref name="cancellation"/> is cancelled,
    /// <paramref name="stopAll"/> runs and <see cref="OperationCanceledException"/> is thrown.
    /// </summary>
    public static async Task<SuitePorts> RunAsync(
        Func<SuitePorts> choosePorts,
        Func<SuitePorts, CancellationToken, Task> start,
        Func<Task> stopAll,
        Func<string, int, Task>? onRetry = null,
        int retries = 3,
        CancellationToken cancellation = default)
    {
        var retry = 0;
        while (true)
        {
            var ports = choosePorts();
            try
            {
                cancellation.ThrowIfCancellationRequested();
                await start(ports, cancellation).ConfigureAwait(false);
                cancellation.ThrowIfCancellationRequested();
                return ports;
            }
            catch (PortInUseException e) when (retry < retries && !cancellation.IsCancellationRequested)
            {
                retry++;
                await stopAll().ConfigureAwait(false);
                if (onRetry is not null) await onRetry(e.Service, retry).ConfigureAwait(false);
            }
            catch (Exception) when (cancellation.IsCancellationRequested)
            {
                await stopAll().ConfigureAwait(false);
                throw new OperationCanceledException(cancellation);
            }
        }
    }
}

/// <summary>
/// How often the web view may reload a page whose renderer crashed: at most
/// <see cref="Limit"/> times in <see cref="Window"/>; after that the shell gives up on that page.
/// </summary>
public sealed class ReloadBudget(int limit = 2, TimeSpan? window = null)
{
    public int Limit { get; } = limit;
    public TimeSpan Window { get; } = window ?? TimeSpan.FromSeconds(60);
    readonly List<DateTimeOffset> times = [];

    /// <summary>True (and counted) if another reload is allowed at <paramref name="now"/>.</summary>
    public bool Take(DateTimeOffset now)
    {
        times.RemoveAll(t => now - t >= Window);
        if (times.Count >= Limit) return false;
        times.Add(now);
        return true;
    }
}

/// <summary>The note shown when a service stops working after the demo started.</summary>
public static class ServiceAlert
{
    /// <summary>Restart attempts before the presenter is told.</summary>
    public const int Attempts = 3;

    /// <param name="optional">A service the suite runs without (the camera analytics), whose
    /// failure is said gently: the rest of Midgard works.</param>
    public static string? Message(string service, ServiceStatus status, bool optional = false)
    {
        switch (status)
        {
            case ServiceStatus.Restarting r when r.Attempt >= Attempts:
            case ServiceStatus.Failed:
                break;
            default:
                return null;
        }
        return optional
            ? $"{ServiceNames.Friendly(service)} is unavailable; the rest of Midgard works."
            : $"{ServiceNames.Friendly(service)} has stopped. Try Reset demo, or quit and reopen Midgard.";
    }
}

/// <summary>The home page's status line.</summary>
public static class HomeStatus
{
    /// <summary>
    /// The first required service that failed or is restarting, else "Starting…" until every
    /// required one is ready. Optional services never make it "Starting…" or "has stopped";
    /// one that failed is mentioned after the rest.
    /// </summary>
    public static string Line(IReadOnlyList<string> order, IReadOnlyDictionary<string, ServiceStatus> statuses,
                              IReadOnlySet<string> optional, bool replaying)
    {
        var required = order.Where(n => !optional.Contains(n)).ToList();
        bool Is<T>(string name) => statuses.TryGetValue(name, out var s) && s is T;
        if (required.FirstOrDefault(Is<ServiceStatus.Failed>) is { } bad)
            return $"{ServiceNames.Friendly(bad)} has stopped";
        if (required.FirstOrDefault(Is<ServiceStatus.Restarting>) is { } restarting)
            return $"{ServiceNames.Friendly(restarting)} is restarting…";
        if (!required.All(Is<ServiceStatus.Ready>)) return "Starting…";
        var missing = order.Where(n => optional.Contains(n) && (Is<ServiceStatus.Failed>(n) || Is<ServiceStatus.Restarting>(n)));
        return "All services running" + (replaying ? " · replaying recorded hour" : "")
             + string.Concat(missing.Select(n => $" · {ServiceNames.Friendly(n).ToLowerInvariant()} unavailable"));
    }
}

/// <summary>Deciding about the session by the wall clock (a PC that slept does not count the sleep in a timer).</summary>
public static class SessionTiming
{
    public enum Action
    {
        Wait,
        /// <summary>Sign in again; the open page can be given the new token in place.</summary>
        Refresh,
        /// <summary>Sign in again and reload the page: its token has expired.</summary>
        RefreshAndReload,
    }

    public static Action Decide(DateTimeOffset now, DateTimeOffset refreshAt, DateTimeOffset expiresAt)
    {
        if (now >= expiresAt) return Action.RefreshAndReload;
        return now >= refreshAt ? Action.Refresh : Action.Wait;
    }
}

/// <summary>What the starting screen shows when something went wrong.</summary>
/// <param name="Details">Plain-English explanation, then (for a service) its last log lines.</param>
/// <param name="CanRetry">False when trying again cannot help (an unsupported PC, a damaged install).</param>
/// <param name="CanReset">True when a working copy exists, so Reset demo is offered too.</param>
public sealed record StartFailure(string Title, string Details, bool CanRetry = true, bool CanReset = false)
{
    const string LogMarker = " Last log lines:\n";

    /// <summary>The supervisor's reasons end with " Last log lines:\n…": the summary, and the log lines if any.</summary>
    public (string Summary, string? LogLines) Split()
    {
        var at = Details.IndexOf(LogMarker, StringComparison.Ordinal);
        return at < 0 ? (Details, null) : (Details[..at], Details[(at + LogMarker.Length)..]);
    }
}

/// <summary>Errors in words a presenter can act on.</summary>
public static class Explain
{
    /// <summary>A working-copy error.</summary>
    public static string WorkingCopy(Exception error)
    {
        if (error is not HomeException home)
            return $"Something went wrong while copying the demo data: {error.Message}";
        return home.Problem switch
        {
            HomeProblem.NotAWorkingCopy =>
                $"The folder {home.Path} already holds other files, so Midgard will not put its demo data "
                + "there. Move that folder somewhere else, then try again.",
            HomeProblem.StartingStateMissing or HomeProblem.LinkInStartingState =>
                "This copy of Midgard is incomplete: its starting demo data is missing or damaged. "
                + "Install Midgard again.",
            HomeProblem.InsufficientSpace =>
                "There is not enough free disk space for the demo data. It needs "
                + $"{HomeException.Bytes(home.Needed)}, and {HomeException.Bytes(home.Available)} is free. "
                + "Free some space, then try again.",
            HomeProblem.CopyIncomplete =>
                "The demo data did not copy completely. Check that the disk has free space and that no other "
                + "program is using Midgard's folder, then try again.",
            HomeProblem.ReplaceFailed =>
                $"The demo could not be reset. The previous demo data was kept at {home.Path}.",
            _ => $"Something went wrong while copying the demo data: {home.Message}",
        };
    }

    /// <summary>How a service status reads in shell.log.</summary>
    public static string Status(ServiceStatus status)
    {
        static string FirstLine(string text) => text.Split('\n')[0];
        return status switch
        {
            ServiceStatus.Starting => "starting",
            ServiceStatus.Ready => "ready",
            ServiceStatus.Restarting r => $"restarting (attempt {r.Attempt})" + (r.Reason is { } why ? $": {FirstLine(why)}" : ""),
            ServiceStatus.Failed f => $"failed: {FirstLine(f.Reason)}",
            ServiceStatus.Stopped => "stopped",
            _ => status.ToString(),
        };
    }
}
