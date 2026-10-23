using System.Globalization;
using Midgard.Core;

namespace Midgard;

/// <summary>
/// The shell's steps (<see cref="ShellSteps"/>) on this PC: the instance lock, the system and
/// ACL checks, the orphan sweep, the working copy, the web server, the supervisor and the
/// gateway sign-in. <see cref="ShellModel"/> runs them in order.
/// </summary>
sealed class ShellWiring(Paths paths, ShellLog log)
{
    readonly Supervisor supervisor = new();
    readonly Lock gate = new();
    InstanceLock? instance;
    WebServer? server;

    string Starting => Path.Join(paths.Resources, "starting-state");
    string Models => Path.Join(paths.Resources, "models");

    /// <summary>MIDGARD_LLAMA and MIDGARD_MODEL_THREADS, for the model server.</summary>
    static Dictionary<string, string> ModelEnvironment()
    {
        var env = new Dictionary<string, string>();
        foreach (var key in ModelServer.EnvironmentKeys)
        {
            if (Environment.GetEnvironmentVariable(key) is { } value) env[key] = value;
        }
        return env;
    }

    public ShellSteps Steps() => new()
    {
        ClaimInstance = () =>
        {
            if (!SingleInstance.Claim(paths.Home, out var held)) return InstanceClaim.OtherRunning;
            lock (gate) instance = held;
            return held is null ? InstanceClaim.NotHeld : InstanceClaim.Held;
        },
        SystemProblem = () => SystemCheck.Problem(WindowsFacts.ForThisPc(paths.Home)),
        FolderProblem = () => FolderAccess.Problem(paths, WindowsAcl.Entries),
        Sweep = async held => Describe("orphan sweep", await Orphans.SweepAsync(paths.Helpers, held).ConfigureAwait(false)),
        PrepareHome = cancellation => Task.Run(() => Home.Prepare(Starting, paths.Home), cancellation),
        LoadUsers = LoadUsers,
        StartWebServer = () => Task.Run(StartWebServer),
        ChoosePorts = SuitePorts.Free,
        Specs = Specs,
        StartServices = (specs, cancellation) => supervisor.StartAsync(specs, cancellation),
        StopServices = supervisor.StopAllAsync,
        CloseServices = supervisor.CloseAsync,
        Statuses = supervisor.Statuses,
        SignIn = (api, user, patient, cancellation) =>
            Session.LoginAsync(api, user, patient ? Session.PatientLoopback : Session.Loopback, cancellation),
        ResetHome = () => Home.Reset(Starting, paths.Home, servicesStopped: true),
        // Still holding the lock, so only this app's leftovers (never another install's).
        FinalSweep = async () => Describe("final sweep", await Orphans.SweepAsync(
            paths.Helpers, HoldsLock, grace: TimeSpan.FromSeconds(5)).ConfigureAwait(false)),
        StopWebServer = () => Task.Run(() =>
        {
            WebServer? running;
            lock (gate) running = server;
            running?.Stop();
        }),
        MarkStopping = () => Instance?.MarkStopping(),
        ReleaseInstance = () => Instance?.Release(),
        PreparedAnswersUrl = origin => Session.PreparedAnswersUrl(paths.Resources, origin),
        Log = log.Write,
    };

    InstanceLock? Instance
    {
        get { lock (gate) return instance; }
    }

    bool HoldsLock => Instance is not null;

    /// <summary>The working copy's demo-users.json; the starting state's as the fallback.</summary>
    IReadOnlyList<DemoUser> LoadUsers()
    {
        var inHome = Path.Join(paths.Home, "demo-users.json");
        return DemoUsers.Load(File.Exists(inHome) ? inHome : Path.Join(Starting, "demo-users.json"));
    }

    /// <summary>
    /// The previous launch's port first (remembered in <c>&lt;Home&gt;.webport</c>, beside the
    /// working copy), so the consoles' saved preferences (stored per origin) survive a
    /// relaunch; any free port if that one is taken.
    /// </summary>
    ushort StartWebServer()
    {
        var memory = Path.ChangeExtension(Home.InstanceLockFile(paths.Home), ".webport");
        ushort remembered = 0;
        try
        {
            if (!ushort.TryParse(File.ReadAllText(memory).Trim(), NumberStyles.None, CultureInfo.InvariantCulture, out remembered))
                remembered = 0;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
        WebServer? started = null;
        if (remembered > 0)
        {
            var candidate = new WebServer(paths.Resources, remembered);
            try
            {
                candidate.Start();
                started = candidate;
            }
            catch (WebServerStartException e)
            {
                log.Write($"web server: port {remembered} is taken ({e.Message}); using another");
                candidate.Dispose();
            }
        }
        if (started is null)
        {
            started = new WebServer(paths.Resources);
            started.Start();
        }
        lock (gate) server = started;
        try
        {
            File.WriteAllText(memory, $"{started.BoundPort}\n");
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            log.Write($"web server: cannot remember the port ({e.Message})");
        }
        return started.BoundPort;
    }

    /// <summary>The recording, when the package has one.</summary>
    string? Recording()
    {
        var dir = Path.Join(paths.Resources, "recording");
        return File.Exists(Path.Join(dir, "meta.json")) ? dir : null;
    }

    IReadOnlyList<ServiceSpec>? Specs(SuitePorts ports, string origin)
    {
        var env = ModelEnvironment();
        var llama = ModelServer.Locate(paths.Helpers, paths.DevLayout, env, File.Exists);
        if (llama is null)
        {
            log.Write($"llama-server not found: {ModelServer.BundledExecutable(paths.Helpers)}");
            return null;
        }
        var command = ModelServer.Make(llama, Models, ports.Llama, ModelServer.ThreadsForThisPc(env));
        return SuiteSpecs.Make(paths.Helpers, Models, paths.Home, paths.Logs, ports, origin, Recording(), command);
    }

    static string Describe(string what, SweepOutcome outcome)
    {
        if (outcome.Skipped is { } why) return $"{what} skipped: {why}";
        if (outcome.Reaped.Count == 0 && outcome.NotEnded.Count == 0) return $"{what}: nothing running";
        var lines = outcome.Reaped.Select(p => $"{what}: ended pid {p.Pid}: {p.Executable}")
            .Concat(outcome.NotEnded.Select(p => $"{what}: could not end pid {p.Pid} ({p.Executable}): {p.Reason}"));
        return string.Join("\n", lines);
    }
}
