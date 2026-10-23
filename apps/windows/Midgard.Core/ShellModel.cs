using System.Diagnostics;

namespace Midgard.Core;

/// <summary>Where the shell is: what the window shows.</summary>
public enum ShellPhase
{
    /// <summary>The starting screen ("Starting Midgard…", "Resetting the demo…").</summary>
    Starting,
    /// <summary>What went wrong (<see cref="ShellState.Failure"/>), with Try again where it can help.</summary>
    Failed,
    /// <summary>The web view, with the home page or a console.</summary>
    Ready,
    /// <summary>Quitting: the services are being stopped.</summary>
    Stopping,
}

/// <summary>What the single-instance check found (Midgard\SingleInstance.cs).</summary>
public enum InstanceClaim
{
    /// <summary>This app holds its working copy's instance lock.</summary>
    Held,
    /// <summary>It runs without the lock (it could not be taken), so it does not sweep for orphans.</summary>
    NotHeld,
    /// <summary>Another Midgard runs this working copy and was brought forward: exit.</summary>
    OtherRunning,
}

/// <summary>
/// The window's web view as the shell drives it (WebView2 in Midgard\MainForm.cs). Called
/// from any thread; the window marshals to its own.
/// </summary>
public interface IShellView
{
    /// <summary>
    /// Replaces the document-start scripts and the origins <see cref="OfflineRules"/> lets
    /// through. The first call creates the web view; it throws if that fails.
    /// </summary>
    Task ConfigureAsync(IReadOnlyList<string> scripts, string appOrigin, string? gatewayOrigin);
    void Load(string url);
    void Reload();
    void Evaluate(string script);
    /// <summary>Every page's stored data (local and session storage, IndexedDB, caches, cookies).</summary>
    Task ClearBrowsingDataAsync();
}

/// <summary>
/// What the shell does at each step, as replaceable functions: Midgard\ShellWiring.cs gives
/// the real ones (instance lock, system check, ACL check, orphan sweep, Home, web server,
/// supervisor, gateway sign-in); the tests give recording fakes.
/// </summary>
public sealed class ShellSteps
{
    public required Func<InstanceClaim> ClaimInstance { get; init; }
    /// <summary>Why this PC cannot run Midgard (SystemCheck), or null.</summary>
    public required Func<string?> SystemProblem { get; init; }
    /// <summary>Why the app's folders are not private (FolderAccess), or null.</summary>
    public required Func<string?> FolderProblem { get; init; }
    /// <summary>The orphan sweep (<c>Orphans.SweepAsync</c>), given whether the instance lock is held; what it did, for shell.log.</summary>
    public required Func<bool, Task<string>> Sweep { get; init; }
    /// <summary><c>Home.Prepare</c>, off the caller's thread.</summary>
    public required Func<CancellationToken, Task> PrepareHome { get; init; }
    public required Func<IReadOnlyList<DemoUser>> LoadUsers { get; init; }
    /// <summary>Starts the loopback web server; its port.</summary>
    public required Func<Task<ushort>> StartWebServer { get; init; }
    public required Func<SuitePorts> ChoosePorts { get; init; }
    /// <summary>The services for these ports and the web origin, in start order; null when the model server is missing.</summary>
    public required Func<SuitePorts, string, IReadOnlyList<ServiceSpec>?> Specs { get; init; }
    /// <summary><c>Supervisor.StartAsync</c>.</summary>
    public required Func<IReadOnlyList<ServiceSpec>, CancellationToken, Task> StartServices { get; init; }
    /// <summary><c>Supervisor.StopAllAsync</c>.</summary>
    public required Func<Task> StopServices { get; init; }
    /// <summary><c>Supervisor.CloseAsync</c>: stops everything for good (quitting).</summary>
    public required Func<Task> CloseServices { get; init; }
    /// <summary><c>Supervisor.Statuses</c>.</summary>
    public required Func<CancellationToken, IAsyncEnumerable<ServiceEvent>> Statuses { get; init; }
    /// <summary><c>Session.LoginAsync</c> at the gateway API; patient (60 s) at startup.</summary>
    public required Func<Uri, DemoUser, bool, CancellationToken, Task<SessionInfo>> SignIn { get; init; }
    /// <summary><c>Home.Reset(…, servicesStopped: true)</c>; called only after <see cref="StopServices"/>.</summary>
    public required Func<Home.ResetReport> ResetHome { get; init; }
    /// <summary>The last sweep when quitting, after the supervisor is closed; what it found, for shell.log.</summary>
    public required Func<Task<string>> FinalSweep { get; init; }
    public required Func<Task> StopWebServer { get; init; }
    /// <summary><c>InstanceLock.MarkStopping</c>: a launch meanwhile waits rather than hands over.</summary>
    public required Action MarkStopping { get; init; }
    /// <summary><c>InstanceLock.Release</c>, the very last step of quitting.</summary>
    public required Action ReleaseInstance { get; init; }
    /// <summary><c>Session.PreparedAnswersUrl</c> for the origin.</summary>
    public Func<string, string?> PreparedAnswersUrl { get; init; } = _ => null;
    public Func<DateTimeOffset> Now { get; init; } = () => DateTimeOffset.Now;
    public Action<string> Log { get; init; } = _ => { };
    /// <summary>How often a waiting session refresh looks at the wall clock.</summary>
    public TimeSpan RefreshPoll { get; init; } = TimeSpan.FromSeconds(60);
}

/// <summary>A snapshot of what the window shows.</summary>
public sealed record ShellState(
    ShellPhase Phase, string Title, string? Step, StartFailure? Failure, string? Note,
    IReadOnlyList<string> ServiceOrder, IReadOnlyDictionary<string, ServiceStatus> Statuses,
    string? SignedInAs, bool CanReset)
{
    /// <summary>The services that are ready, for the starting screen's progress bar.</summary>
    public int ReadyCount => ServiceOrder.Count(n => Statuses.TryGetValue(n, out var s) && s is ServiceStatus.Ready);
}

/// <summary>
/// The shell's flow, the port of apps/mac/Sources/Midgard/AppModel.swift without the window:
/// launch, Try again, Reset demo, the session's renewal and quitting.
/// </summary>
/// <remarks>
/// <para>Launch, in order: the instance lock (<see cref="Claim"/>, before the window), the
/// system check, the ACL check, the orphan sweep (only when the lock is held), the working
/// copy, the demo users, the web server and the web view's offline rules, the ports and the
/// services (fresh ports when one is taken), the sign-in, then the home page. The web server
/// starts before the ports are chosen, as on the Mac: the gateway is told its origin, and a
/// port the server holds cannot be handed to a service.</para>
/// <para>Starting, Try again and Reset demo each run as one task tagged with a generation
/// number; a newer one cancels and waits for the older, and an older one never changes
/// what the window shows.</para>
/// <para>Quitting closes the supervisor (every service stopped, none can start) before the
/// final sweep, the web server and, last, the instance lock: a launch meanwhile waits for
/// this app rather than starting services beside its own.</para>
/// </remarks>
public sealed class ShellModel
{
    readonly ShellSteps steps;
    readonly IShellView view;
    readonly Lock gate = new();
    readonly ReloadBudget crashBudget = new();

    // Guarded by `gate`.
    ShellPhase phase = ShellPhase.Starting;
    string title = "Starting Midgard…";
    string? step;
    StartFailure? failure;
    string? note;
    List<string> order = [];
    HashSet<string> optional = [];
    Dictionary<string, ServiceStatus> statuses = [];
    DemoUser? user;
    SessionInfo? session;
    DateTimeOffset? refreshAt;
    SuitePorts? ports;
    string? origin;
    bool holdsLock;
    bool swept;
    bool prepared;
    bool viewReady;
    bool forceHeimdall2D;
    bool onHome = true;
    bool shuttingDown;
    int generation;
    Task current = Task.CompletedTask;
    CancellationTokenSource? currentCancel;
    CancellationTokenSource? refreshCancel;
    Task? shutdown;
    readonly CancellationTokenSource statusCancel = new();
    Task? statusTask;

    public ShellModel(ShellSteps steps, IShellView view)
    {
        this.steps = steps;
        this.view = view;
    }

    /// <summary>Raised (on any thread) whenever <see cref="State"/> changes.</summary>
    public event Action? Changed;

    public ShellState State
    {
        get
        {
            lock (gate)
            {
                return new ShellState(phase, title, step, failure, note, [.. order],
                                      new Dictionary<string, ServiceStatus>(statuses), user?.Label, CanResetLocked());
            }
        }
    }

    /// <summary>The app's web origin once its web server runs (<c>http://127.0.0.1:&lt;port&gt;</c>).</summary>
    public string? Origin
    {
        get { lock (gate) return origin; }
    }

    /// <summary>The gateway's origin once the services run.</summary>
    public string? GatewayOrigin
    {
        get { lock (gate) return ports is { } p ? $"http://127.0.0.1:{p.GatewayHttp}" : null; }
    }

    void Log(string message) => steps.Log(message);
    void Notify() => Changed?.Invoke();

    // Launch

    /// <summary>
    /// Before any window: true if this is the only app for its working copy (with or without
    /// the lock), false if another one runs it and was brought forward (exit).
    /// </summary>
    public bool Claim()
    {
        var claim = steps.ClaimInstance();
        Log(claim switch
        {
            InstanceClaim.Held => "instance lock held",
            InstanceClaim.NotHeld => "instance lock unavailable; running without it (no orphan sweep)",
            _ => "another Midgard runs this working copy; handing over to it",
        });
        lock (gate) holdsLock = claim == InstanceClaim.Held;
        return claim != InstanceClaim.OtherRunning;
    }

    /// <summary>The system and ACL checks, then the start (returned; it reports through <see cref="State"/>).</summary>
    public Task LaunchAsync()
    {
        if (steps.SystemProblem() is { } problem)
        {
            Log($"system check: {problem}");
            Fail(0, "Midgard cannot run on this PC", problem, canRetry: false);
            return Task.CompletedTask;
        }
        if (steps.FolderProblem() is { } access)
        {
            Log($"folder check: {access}");
            Fail(0, "Midgard's folder is not private", access, canRetry: false);
            return Task.CompletedTask;
        }
        ListenToStatuses();
        return Run(Bootstrap);
    }

    /// <summary>The failure screen's Try again.</summary>
    public Task RetryAsync()
    {
        lock (gate)
        {
            if (shuttingDown || phase != ShellPhase.Failed || failure is not { CanRetry: true }) return Task.CompletedTask;
        }
        Log("try again");
        ShowStarting("Starting Midgard…", "Stopping the services…");
        return Run(async (gen, cancellation) =>
        {
            await steps.StopServices().ConfigureAwait(false);
            if (!IsCurrent(gen, cancellation))
            {
                Interrupted(gen);
                return;
            }
            await Bootstrap(gen, cancellation).ConfigureAwait(false);
        });
    }

    /// <summary>Runs <paramref name="work"/> as the one start or reset in flight, after cancelling and waiting for the previous one.</summary>
    Task Run(Func<int, CancellationToken, Task> work)
    {
        lock (gate)
        {
            currentCancel?.Cancel();
            var previous = current;
            var gen = ++generation;
            var cancel = currentCancel = new CancellationTokenSource();
            current = Task.Run(async () =>
            {
                try
                {
                    await previous.ConfigureAwait(false);
                }
                catch (Exception e)
                {
                    Log($"previous start ended with {e.GetType().Name}: {e.Message}");
                }
                if (!IsCurrent(gen, cancel.Token)) return;
                try
                {
                    await work(gen, cancel.Token).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (cancel.IsCancellationRequested)
                {
                    Interrupted(gen);
                }
                catch (Exception e)
                {
                    Log($"start failed unexpectedly: {e}");
                    Fail(gen, "Midgard could not start", e.Message);
                }
            });
            return current;
        }
    }

    /// <summary>Whether the task of generation <paramref name="gen"/> may still change what the window shows.</summary>
    bool IsCurrent(int gen, CancellationToken cancellation)
    {
        lock (gate) return gen == generation && !shuttingDown && !cancellation.IsCancellationRequested;
    }

    /// <summary>A start that stopped part-way: unless something newer took over or Midgard is quitting, say so.</summary>
    void Interrupted(int gen)
    {
        lock (gate)
        {
            if (gen != generation || shuttingDown) return;
        }
        Log("start interrupted");
        Fail(gen, "Midgard stopped starting", "Starting was interrupted before it finished.");
    }

    void ShowStarting(string newTitle, string? newStep)
    {
        lock (gate)
        {
            phase = ShellPhase.Starting;
            title = newTitle;
            step = newStep;
            failure = null;
        }
        Notify();
    }

    void SetStep(string? newStep)
    {
        lock (gate) step = newStep;
        Notify();
    }

    void Fail(int gen, string failureTitle, string details, bool canRetry = true)
    {
        lock (gate)
        {
            if (gen != generation || shuttingDown) return;
            step = null;
            phase = ShellPhase.Failed;
            failure = new StartFailure(failureTitle, details, canRetry, canRetry && prepared);
        }
        Log($"failed: {failureTitle}");
        Notify();
    }

    void ListenToStatuses()
    {
        lock (gate)
        {
            if (statusTask is not null) return;
            // Started before any start, so no status is missed; the supervisor's only reader.
            statusTask = Task.Run(async () =>
            {
                try
                {
                    await foreach (var e in steps.Statuses(statusCancel.Token).ConfigureAwait(false))
                        StatusChanged(e.Service, e.Status);
                }
                catch (OperationCanceledException)
                {
                }
            });
        }
    }

    void StatusChanged(string name, ServiceStatus status)
    {
        bool ready;
        bool isOptional;
        ServiceStatus? old;
        lock (gate)
        {
            old = statuses.GetValueOrDefault(name);
            statuses[name] = status;
            if (!order.Contains(name)) order.Add(name);
            ready = phase == ShellPhase.Ready;
            isOptional = optional.Contains(name);
        }
        if (old != status) Log($"service {name}: {Explain.Status(status)}");
        if (ready)
        {
            UpdateHomeStatus();
            var message = ServiceAlert.Message(name, status, isOptional);
            lock (gate)
            {
                if (message is not null && note != message)
                {
                    note = message;
                }
                else if (status is ServiceStatus.Ready && note is not null
                         && note == ServiceAlert.Message(name, new ServiceStatus.Failed(""), isOptional))
                {
                    // It came back: the warning no longer holds.
                    note = null;
                    message = $"note cleared: {ServiceNames.Friendly(name)} is running again";
                }
                else
                {
                    message = null;
                }
            }
            if (message is not null) Log(message.StartsWith("note cleared", StringComparison.Ordinal) ? message : $"note: {message}");
        }
        Notify();
    }

    async Task Bootstrap(int gen, CancellationToken cancellation)
    {
        ShowStarting("Starting Midgard…", null);
        bool needSweep, needPrepare, lockHeld;
        lock (gate)
        {
            needSweep = !swept;
            needPrepare = !prepared;
            lockHeld = holdsLock;
        }
        if (needSweep)
        {
            var outcome = await steps.Sweep(lockHeld).ConfigureAwait(false);
            lock (gate) swept = true;
            Log(outcome);
            if (!IsCurrent(gen, cancellation))
            {
                Interrupted(gen);
                return;
            }
        }

        if (needPrepare)
        {
            SetStep("Preparing the demo data…");
            var started = Stopwatch.StartNew();
            try
            {
                await steps.PrepareHome(cancellation).ConfigureAwait(false);
                lock (gate) prepared = true;
                Log($"working copy ready ({Seconds(started)})");
            }
            catch (OperationCanceledException) when (cancellation.IsCancellationRequested)
            {
                Interrupted(gen);
                return;
            }
            catch (Exception e)
            {
                Log($"working copy failed: {e.Message}");
                Fail(gen, "The demo data could not be prepared", Explain.WorkingCopy(e));
                return;
            }
            if (!IsCurrent(gen, cancellation))
            {
                Interrupted(gen);
                return;
            }
        }

        try
        {
            var users = steps.LoadUsers();
            if (users.Count == 0) throw new InvalidDataException("no demo users");
            lock (gate)
            {
                // The presenter account, as on the Mac: the first one listed.
                if (user is null || !users.Contains(user)) user = users[0];
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or InvalidDataException)
        {
            Log($"demo users: {e.Message}");
            Fail(gen, "Midgard is incomplete", "The list of demo users could not be read. Install Midgard again.", canRetry: false);
            return;
        }

        string? appOrigin;
        lock (gate) appOrigin = origin;
        if (appOrigin is null)
        {
            SetStep("Starting the web server…");
            try
            {
                var port = await steps.StartWebServer().ConfigureAwait(false);
                appOrigin = $"http://127.0.0.1:{port}";
                lock (gate) origin = appOrigin;
                Log($"web server on {appOrigin}");
            }
            catch (Exception e) when (e is not OperationCanceledException)
            {
                Log($"web server failed: {e.Message}");
                Fail(gen, "Midgard could not start", $"Its built-in web server could not start ({e.Message}).");
                return;
            }
            if (!IsCurrent(gen, cancellation))
            {
                Interrupted(gen);
                return;
            }
        }

        bool needView;
        lock (gate) needView = !viewReady;
        if (needView)
        {
            try
            {
                // Nothing is loaded yet: this sets up the web view and its offline rules first.
                await view.ConfigureAsync([], appOrigin, null).ConfigureAwait(false);
                lock (gate) viewReady = true;
                Log("web view ready, offline rules in place");
            }
            catch (Exception e)
            {
                Log($"web view failed: {e}");
                Fail(gen, "Midgard could not start",
                     $"Its window could not show web pages (Microsoft Edge WebView2 did not start: {e.Message}). "
                     + "Restart the PC, then open Midgard again.");
                return;
            }
            if (!IsCurrent(gen, cancellation))
            {
                Interrupted(gen);
                return;
            }
        }
        await StartServices(gen, appOrigin, cancellation).ConfigureAwait(false);
    }

    async Task StartServices(int gen, string appOrigin, CancellationToken cancellation)
    {
        ShowStarting(State.Title, null);
        var planned = steps.Specs(SuitePorts.From(new ushort[SuitePorts.Count])!, appOrigin);
        if (planned is null)
        {
            Log("llama-server not found");
            Fail(gen, "Midgard is incomplete", "The model server is missing. Install Midgard again.", canRetry: false);
            return;
        }
        lock (gate)
        {
            statuses = [];
            order = planned.Select(s => s.Name).ToList();
            optional = planned.Where(s => s.Optional).Select(s => s.Name).ToHashSet();
        }
        Notify();
        Log($"starting services {string.Join(", ", planned.Select(s => s.Name))}");
        var started = Stopwatch.StartNew();
        SuitePorts chosen;
        try
        {
            chosen = await SuiteStart.RunAsync(
                choosePorts: () =>
                {
                    var p = steps.ChoosePorts();
                    Log($"ports: gateway {p.GatewayHttp}, llama {p.Llama}, all {string.Join(" ", p.All)}");
                    return p;
                },
                start: (p, c) => steps.StartServices(steps.Specs(p, appOrigin)!, c),
                stopAll: steps.StopServices,
                onRetry: (service, retry) =>
                {
                    Log($"{service}: port in use; retry {retry} with fresh ports");
                    return Task.CompletedTask;
                },
                cancellation: cancellation).ConfigureAwait(false);
        }
        catch (OperationCanceledException)
        {
            Log("start cancelled");
            Interrupted(gen);
            return;
        }
        catch (SupervisorStoppedException)
        {
            Log("start stopped");
            Interrupted(gen);
            return;
        }
        catch (Exception e)
        {
            Log($"start failed: {e.Message}");
            await steps.StopServices().ConfigureAwait(false);
            switch (e)
            {
                case ServiceFailedException f:
                    Fail(gen, $"Midgard could not start {ServiceNames.Friendly(f.Service)}", f.Reason);
                    break;
                case PortInUseException p:
                    Fail(gen, $"Midgard could not start {ServiceNames.Friendly(p.Service)}",
                         "Its network port was taken by another program several times in a row.");
                    break;
                default:
                    Fail(gen, "Midgard could not start", e.Message);
                    break;
            }
            return;
        }
        lock (gate) ports = chosen;
        Log($"all services ready ({Seconds(started)})");
        if (!IsCurrent(gen, cancellation))
        {
            Interrupted(gen);
            return;
        }

        DemoUser presenter;
        lock (gate) presenter = user!;
        SetStep("Signing in…");
        SessionInfo info;
        try
        {
            info = await StartupLogin(presenter, cancellation).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested)
        {
            Interrupted(gen);
            return;
        }
        catch (Exception e)
        {
            Log($"sign-in as {presenter.Username} failed: {e.Message}");
            await steps.StopServices().ConfigureAwait(false);
            Fail(gen, "Midgard could not sign in", $"The gateway refused the demo sign-in ({e.Message}).");
            return;
        }
        if (!IsCurrent(gen, cancellation))
        {
            Interrupted(gen);
            return;
        }
        try
        {
            await Adopt(info, presenter).ConfigureAwait(false);
        }
        catch (Exception e)
        {
            Log($"web view scripts failed: {e}");
            await steps.StopServices().ConfigureAwait(false);
            Fail(gen, "Midgard could not start", $"Its window could not take the sign-in ({e.Message}).");
            return;
        }
        lock (gate)
        {
            if (gen != generation || shuttingDown) return;
            step = null;
            phase = ShellPhase.Ready;
            onHome = true;
        }
        Log("ready");
        Notify();
        view.Load(appOrigin + "/");
    }

    // Session

    Uri GatewayApi()
    {
        lock (gate)
        {
            return ports is { } p ? new Uri($"http://127.0.0.1:{p.GatewayHttp}/api/v1")
                : throw SessionException.Transport("no gateway");
        }
    }

    /// <summary>
    /// The sign-in at startup: up to 60 s per attempt and one more attempt after a timeout
    /// or a dropped connection, because the gateway may still be busy storing the services'
    /// first syncs.
    /// </summary>
    async Task<SessionInfo> StartupLogin(DemoUser presenter, CancellationToken cancellation)
    {
        var api = GatewayApi();
        var started = Stopwatch.StartNew();
        try
        {
            var info = await steps.SignIn(api, presenter, true, cancellation).ConfigureAwait(false);
            if (started.Elapsed > TimeSpan.FromSeconds(5)) Log($"sign-in took {Seconds(started)}");
            return info;
        }
        catch (SessionException e) when (e.Kind == SessionException.Kinds.Transport)
        {
            Log($"sign-in attempt failed after {Seconds(started)} ({e.Detail}); trying once more");
            cancellation.ThrowIfCancellationRequested();
            var info = await steps.SignIn(api, presenter, true, cancellation).ConfigureAwait(false);
            Log($"sign-in succeeded on the second attempt, {Seconds(started)} in all");
            return info;
        }
    }

    /// <summary>Makes <paramref name="info"/> the session for every later page load and schedules its renewal.</summary>
    async Task Adopt(SessionInfo info, DemoUser presenter)
    {
        lock (gate)
        {
            session = info;
            user = presenter;
        }
        Log($"signed in as {info.Subject} (clearance {info.Clearance ?? "none"}, expires {info.ExpiresAt:O})");
        await InstallScripts().ConfigureAwait(false);
        ScheduleRefresh(Session.RefreshDate(info, steps.Now()));
    }

    /// <summary>The injection script for <paramref name="info"/>, or null before the web server and the gateway run.</summary>
    public string? Injection(SessionInfo info)
    {
        string? appOrigin;
        SuitePorts? p;
        lock (gate)
        {
            appOrigin = origin;
            p = ports;
        }
        if (appOrigin is null || p is null) return null;
        return Session.InjectionScript(appOrigin, $"http://127.0.0.1:{p.GatewayHttp}/api/v1",
                                       $"{appOrigin}/midgard/index.html#/", info, steps.PreparedAnswersUrl(appOrigin));
    }

    Task InstallScripts()
    {
        SessionInfo? info;
        string? appOrigin;
        bool force;
        lock (gate)
        {
            info = session;
            appOrigin = origin;
            force = forceHeimdall2D;
        }
        if (info is null || appOrigin is null || Injection(info) is not { } injection) return Task.CompletedTask;
        var heimdall = HeimdallView.Script(appOrigin, force ? "2d" : HeimdallView.Preferred, force);
        return view.ConfigureAsync([injection, heimdall], appOrigin, GatewayOrigin);
    }

    /// <summary>Renews the session at <paramref name="date"/> by the wall clock, looked at at least every <see cref="ShellSteps.RefreshPoll"/>.</summary>
    void ScheduleRefresh(DateTimeOffset date)
    {
        CancellationTokenSource cancel;
        lock (gate)
        {
            refreshCancel?.Cancel();
            cancel = refreshCancel = new CancellationTokenSource();
            refreshAt = date;
        }
        Log($"next sign-in refresh at {date:O}");
        _ = Task.Run(async () =>
        {
            try
            {
                while (!cancel.IsCancellationRequested)
                {
                    var remaining = date - steps.Now();
                    if (remaining <= TimeSpan.Zero)
                    {
                        await RefreshSession(cancel.Token).ConfigureAwait(false);
                        return;
                    }
                    await Task.Delay(remaining < steps.RefreshPoll ? remaining : steps.RefreshPoll, cancel.Token).ConfigureAwait(false);
                }
            }
            catch (OperationCanceledException)
            {
            }
        });
    }

    /// <summary>The PC woke from sleep: renew the session now if it is due or expired.</summary>
    public void DidWake()
    {
        SessionTiming.Action action;
        CancellationTokenSource cancel;
        lock (gate)
        {
            if (phase != ShellPhase.Ready || session is null || refreshAt is null) return;
            action = SessionTiming.Decide(steps.Now(), refreshAt.Value, session.ExpiresAt);
            if (action == SessionTiming.Action.Wait) return;
            refreshCancel?.Cancel();
            cancel = refreshCancel = new CancellationTokenSource();
        }
        Log($"woke from sleep: session {action}");
        _ = Task.Run(() => RefreshSession(cancel.Token));
    }

    async Task RefreshSession(CancellationToken cancellation)
    {
        DemoUser? presenter;
        SessionInfo? old;
        DateTimeOffset at;
        lock (gate)
        {
            if (shuttingDown || phase != ShellPhase.Ready || user is null || session is null) return;
            presenter = user;
            old = session;
            at = refreshAt ?? DateTimeOffset.MinValue;
        }
        var action = SessionTiming.Decide(steps.Now(), at, old.ExpiresAt);
        try
        {
            var info = await steps.SignIn(GatewayApi(), presenter, false, cancellation).ConfigureAwait(false);
            lock (gate)
            {
                if (cancellation.IsCancellationRequested || shuttingDown || phase != ShellPhase.Ready) return;
                session = info;
            }
            await InstallScripts().ConfigureAwait(false);
            if (action == SessionTiming.Action.RefreshAndReload)
                view.Reload();  // The page's token has expired: it must load again with the new one.
            else if (Injection(info) is { } injection)
                view.Evaluate(injection);  // The open page keeps its token until it reloads: give it the new one now.
            Log($"session refreshed for {info.Subject}, expires {info.ExpiresAt:O}"
                + (action == SessionTiming.Action.RefreshAndReload ? "; page reloaded" : ""));
            ScheduleRefresh(Session.RefreshDate(info, steps.Now()));
        }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested)
        {
        }
        catch (Exception e)
        {
            lock (gate)
            {
                if (cancellation.IsCancellationRequested || shuttingDown || phase != ShellPhase.Ready) return;
            }
            Log($"session refresh failed: {e.Message}; trying again in 60 s");
            ScheduleRefresh(steps.Now() + TimeSpan.FromSeconds(60));
        }
    }

    // Presenter actions

    /// <summary>The home page.</summary>
    public void GoHome()
    {
        string? appOrigin;
        lock (gate)
        {
            if (shuttingDown || phase != ShellPhase.Ready) return;
            appOrigin = origin;
            onHome = true;
        }
        Log("show home");
        view.Load(appOrigin + "/");
    }

    public void DismissNote()
    {
        lock (gate) note = null;
        Notify();
    }

    bool CanResetLocked() => !shuttingDown && (phase == ShellPhase.Ready || (phase == ShellPhase.Failed && failure is { CanReset: true }));

    /// <summary>Whether Reset demo may be offered now.</summary>
    public bool CanReset
    {
        get { lock (gate) return CanResetLocked(); }
    }

    /// <summary>
    /// Reset demo: <paramref name="confirm"/> (the dialog, on the caller's thread), then stop
    /// every service, put the working copy back (<c>Home.Reset</c> with the services stopped),
    /// clear the web view's stored data and start again. False if it was not offered or not confirmed.
    /// </summary>
    public bool ResetDemo(Func<bool> confirm, out Task done)
    {
        done = Task.CompletedTask;
        if (!CanReset) return false;
        if (!confirm())
        {
            Log("reset demo cancelled");
            return false;
        }
        lock (gate)
        {
            if (!CanResetLocked()) return false;
            refreshCancel?.Cancel();
            note = null;
            onHome = true;
        }
        Log("reset demo");
        ShowStarting("Resetting the demo…", "Stopping the services…");
        view.Load("about:blank");
        done = Run(async (gen, cancellation) =>
        {
            await steps.StopServices().ConfigureAwait(false);
            if (!IsCurrent(gen, cancellation))
            {
                Interrupted(gen);
                return;
            }
            SetStep("Restoring the starting data…");
            var started = Stopwatch.StartNew();
            try
            {
                var report = await Task.Run(steps.ResetHome, CancellationToken.None).ConfigureAwait(false);
                Log($"working copy reset ({Seconds(started)})");
                if (report.Leftover is { } leftover)
                {
                    Log($"reset left {leftover}: {report.Reason}");
                    lock (gate)
                    {
                        note = "The demo was reset, but the previous demo data could not be removed. "
                             + $"It is at {leftover} and can be deleted by hand.";
                    }
                }
            }
            catch (Exception e)
            {
                Log($"reset failed: {e.Message}");
                lock (gate) prepared = false;
                Fail(gen, "The demo could not be reset", Explain.WorkingCopy(e));
                return;
            }
            try
            {
                // The consoles' stored state belongs to the old data too; Heimdall's view
                // default is seeded again by its script.
                await view.ClearBrowsingDataAsync().ConfigureAwait(false);
                Log("web view data cleared");
            }
            catch (Exception e)
            {
                Log($"web view data not cleared: {e.Message}");
            }
            if (!IsCurrent(gen, cancellation))
            {
                Interrupted(gen);
                return;
            }
            lock (gate) title = "Starting Midgard…";
            await Bootstrap(gen, cancellation).ConfigureAwait(false);
        });
        return true;
    }

    // Quitting

    /// <summary>At once when quitting is asked for: "Stopping Midgard…", and no other action can start.</summary>
    public void BeginShutdown()
    {
        lock (gate)
        {
            if (shuttingDown) return;
            shuttingDown = true;
            phase = ShellPhase.Stopping;
            title = "Stopping Midgard…";
            step = null;
        }
        steps.MarkStopping();
        Log("quit: stopping services");
        Notify();
    }

    /// <summary>
    /// Stops every service (the supervisor closed, so none can start again), sweeps, stops the
    /// web server and releases the instance lock, in that order. Safe to call more than once.
    /// </summary>
    public Task ShutdownAsync()
    {
        BeginShutdown();
        lock (gate) return shutdown ??= Task.Run(Shutdown);
    }

    async Task Shutdown()
    {
        var started = Stopwatch.StartNew();
        Task start;
        lock (gate)
        {
            currentCancel?.Cancel();
            refreshCancel?.Cancel();
            start = current;
        }
        try
        {
            await start.ConfigureAwait(false);
        }
        catch (Exception e)
        {
            Log($"quit: the start ended with {e.Message}");
        }
        try
        {
            await steps.CloseServices().ConfigureAwait(false);
            // A last sweep: anything still running from the helpers should not outlive the app.
            Log($"quit: {await steps.FinalSweep().ConfigureAwait(false)}");
            await steps.StopWebServer().ConfigureAwait(false);
        }
        catch (Exception e)
        {
            Log($"quit: {e}");
        }
        await statusCancel.CancelAsync().ConfigureAwait(false);
        Log($"quit: stopped ({Seconds(started)})");
        // Last: a launch meanwhile has waited for this app to finish.
        steps.ReleaseInstance();
    }

    // Web view events

    /// <summary>A page finished loading.</summary>
    public void PageLoaded(string? url)
    {
        bool reinstall;
        lock (gate)
        {
            if (origin is null || OfflineRules.Origin(url) != origin) return;
            onHome = Section(url!) == "home";
            reinstall = forceHeimdall2D;
            forceHeimdall2D = false;
        }
        if (reinstall) _ = InstallScripts();
        UpdateHomeStatus();
    }

    /// <summary>The page's renderer ended (a crash, or memory pressure on an 8 GB PC).</summary>
    public async Task ContentCrashedAsync(string? url)
    {
        string appOrigin;
        string target;
        bool allowed;
        lock (gate)
        {
            if (phase != ShellPhase.Ready || shuttingDown || origin is null) return;
            appOrigin = origin;
            target = url is not null && OfflineRules.Origin(url) == appOrigin ? url : appOrigin + "/";
            allowed = crashBudget.Take(steps.Now());
        }
        var page = Section(target);
        Log($"web content process ended on {target}");
        if (!allowed)
        {
            Log("page crashed again; going home");
            lock (gate)
            {
                note = $"The {char.ToUpperInvariant(page[0]) + page[1..]} page stopped working, so Midgard went back to the home screen.";
                onHome = true;
            }
            Notify();
            view.Load(appOrigin + "/");
            return;
        }
        if (page == "heimdall")
        {
            // The 3D globe is the heaviest page: reopen Heimdall in 2D.
            lock (gate) forceHeimdall2D = true;
            await InstallScripts().ConfigureAwait(false);
            Log("reopening Heimdall in 2D");
        }
        view.Load(target);
    }

    /// <summary>A page of the app's own could not be opened (not a refusal or a cancelled load).</summary>
    public void PageFailed(string? url, string reason)
    {
        Log($"page load failed for {url ?? "?"}: {reason}");
        lock (gate)
        {
            if (phase != ShellPhase.Ready || shuttingDown) return;
            note = $"The page could not be opened ({reason}).";
        }
        Notify();
    }

    /// <summary>The section a page of the app's own web server belongs to: a console's name, else "home".</summary>
    public static string Section(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return "home";
        var first = uri.AbsolutePath.Split('/', StringSplitOptions.RemoveEmptyEntries).FirstOrDefault() ?? "";
        return first is "midgard" or "heimdall" or "mimir" or "forge" ? first : "home";
    }

    /// <summary>The home page's status line.</summary>
    public string StatusLine
    {
        get
        {
            lock (gate)
                return HomeStatus.Line(order, statuses, optional, order.Contains("replay"));
        }
    }

    void UpdateHomeStatus()
    {
        lock (gate)
        {
            if (!onHome || phase != ShellPhase.Ready) return;
        }
        view.Evaluate(PageScript.Call("midgardStatus", StatusLine));
    }

    static string Seconds(Stopwatch watch) =>
        watch.Elapsed.TotalSeconds.ToString("0.0", System.Globalization.CultureInfo.InvariantCulture) + " s";
}
