using System.Runtime.CompilerServices;
using System.Threading.Channels;
using Midgard.Core;

namespace Midgard.Core.Tests;

/// <summary>The window's web view, recording what the shell asks of it.</summary>
sealed class RecordingView(Action<string> record) : IShellView
{
    public Exception? ConfigureFails;
    public readonly List<IReadOnlyList<string>> Scripts = [];
    public string? AppOrigin, GatewayOrigin;

    public Task ConfigureAsync(IReadOnlyList<string> scripts, string appOrigin, string? gatewayOrigin)
    {
        record($"view configure {scripts.Count} scripts, gateway {(gatewayOrigin is null ? "none" : "set")}");
        if (ConfigureFails is not null) throw ConfigureFails;
        lock (Scripts) Scripts.Add(scripts);
        AppOrigin = appOrigin;
        GatewayOrigin = gatewayOrigin;
        return Task.CompletedTask;
    }

    public void Load(string url) => record($"view load {url}");
    public void Reload() => record("view reload");
    public void Evaluate(string script) => record(script.Contains("midgardStatus") ? "view status" : "view evaluate");
    public Task ClearBrowsingDataAsync()
    {
        record("view clear data");
        return Task.CompletedTask;
    }
}

/// <summary>Fake steps for the shell: each records its name; failures and answers are set per test.</summary>
sealed class FakeShell
{
    readonly Lock gate = new();
    readonly List<string> calls = [];
    public readonly List<string> Log = [];
    public readonly Channel<ServiceEvent> Events = Channel.CreateUnbounded<ServiceEvent>();
    public InstanceClaim Claim = InstanceClaim.Held;
    public string? SystemProblem, FolderProblem;
    public Exception? PrepareFails, UsersFail, WebServerFails, StartFails, ResetFails;
    public Func<int, Exception?> SignInFails = _ => null;
    public bool ModelServerMissing;
    public TaskCompletionSource? StartGate;
    DateTimeOffset now = DateTimeOffset.FromUnixTimeSeconds(1_800_000_000);
    public DateTimeOffset Now
    {
        get { lock (gate) return now; }
        set { lock (gate) now = value; }
    }
    public TimeSpan SessionLife = TimeSpan.FromHours(12);
    public readonly RecordingView View;
    public readonly ShellModel Shell;
    int signIns;
    int statusCalls;
    ushort nextPorts = 2000;

    public FakeShell()
    {
        View = new RecordingView(Record);
        Shell = new ShellModel(Steps(), View);
    }

    /// <summary>Records a call; the home page's status line (sent whenever a status changes) is only counted.</summary>
    public void Record(string call)
    {
        if (call == "view status") Interlocked.Increment(ref statusCalls);
        else lock (gate) calls.Add(call);
    }

    public int StatusCalls => Volatile.Read(ref statusCalls);

    public List<string> Calls
    {
        get { lock (gate) return [.. calls]; }
    }

    public int SignIns => Volatile.Read(ref signIns);

    ShellSteps Steps() => new()
    {
        ClaimInstance = () => { Record("claim"); return Claim; },
        SystemProblem = () => { Record("system check"); return SystemProblem; },
        FolderProblem = () => { Record("folder check"); return FolderProblem; },
        Sweep = held => { Record($"sweep (lock {(held ? "held" : "not held")})"); return Task.FromResult("no orphaned services"); },
        PrepareHome = _ => { Record("prepare home"); return PrepareFails is null ? Task.CompletedTask : Task.FromException(PrepareFails); },
        LoadUsers = () =>
        {
            Record("users");
            if (UsersFail is not null) throw UsersFail;
            return [new DemoUser("analyst", "pw", "Analyst"), new DemoUser("boss", "pw2", "Boss")];
        },
        StartWebServer = () => { Record("web server"); return WebServerFails is null ? Task.FromResult((ushort)61235) : Task.FromException<ushort>(WebServerFails); },
        ChoosePorts = () =>
        {
            ushort first;
            lock (gate) first = nextPorts += 100;
            Record($"ports {first}");
            return SuitePorts.From(Enumerable.Range(first, SuitePorts.Count).Select(p => (ushort)p).ToArray())!;
        },
        Specs = (ports, origin) => ModelServerMissing ? null :
        [
            new ServiceSpec("llama", "llama-server.exe", [], new Readiness.LogLine("x"), "llama.log"),
            new ServiceSpec("gateway", "http-gateway.exe", ["--allow-origin", origin, "--bind", $"127.0.0.1:{ports.GatewayHttp}"],
                            new Readiness.LogLine("listening on"), "gateway.log"),
        ],
        StartServices = async (specs, cancellation) =>
        {
            Record($"start services {string.Join(" ", specs.Select(s => s.Name))} ({specs[1].Arguments[1]}, {specs[1].Arguments[3]})");
            foreach (var spec in specs)
            {
                await Events.Writer.WriteAsync(new ServiceEvent(spec.Name, new ServiceStatus.Starting()), cancellation);
            }
            if (StartGate is not null) await StartGate.Task.WaitAsync(cancellation);
            if (StartFails is not null) throw StartFails;
            foreach (var spec in specs)
            {
                await Events.Writer.WriteAsync(new ServiceEvent(spec.Name, new ServiceStatus.Ready()), cancellation);
            }
        },
        StopServices = () => { Record("stop services"); return Task.CompletedTask; },
        CloseServices = () => { Record("close supervisor"); return Task.CompletedTask; },
        Statuses = Read,
        SignIn = (api, user, patient, _) =>
        {
            var n = Interlocked.Increment(ref signIns);
            Record($"sign in {user.Username} at {api} ({(patient ? "patient" : "normal")})");
            if (SignInFails(n) is { } failure) return Task.FromException<SessionInfo>(failure);
            return Task.FromResult(new SessionInfo($"token-{n}", user.Username, "restricted", ["analyst"], Now + SessionLife));
        },
        ResetHome = () =>
        {
            Record("reset home");
            if (ResetFails is not null) throw ResetFails;
            return new Home.ResetReport();
        },
        FinalSweep = () => { Record("final sweep"); return Task.FromResult("final sweep found nothing running"); },
        StopWebServer = () => { Record("stop web server"); return Task.CompletedTask; },
        MarkStopping = () => Record("mark stopping"),
        ReleaseInstance = () => Record("release lock"),
        PreparedAnswersUrl = origin => $"{origin}/home/prepared-answers.json",
        Now = () => Now,
        Log = line => { lock (Log) Log.Add(line); },
        RefreshPoll = TimeSpan.FromMilliseconds(20),
    };

    async IAsyncEnumerable<ServiceEvent> Read([EnumeratorCancellation] CancellationToken cancellation)
    {
        await foreach (var e in Events.Reader.ReadAllAsync(cancellation)) yield return e;
    }

    /// <summary>Waits until <paramref name="condition"/> holds for the shell's state (5 s at most).</summary>
    public async Task<ShellState> Until(Func<ShellState, bool> condition)
    {
        var deadline = DateTime.UtcNow.AddSeconds(5);
        while (!condition(Shell.State))
        {
            if (DateTime.UtcNow > deadline) Assert.Fail($"timed out; state {Shell.State}; calls: {string.Join(" | ", Calls)}");
            await Task.Delay(10);
        }
        return Shell.State;
    }

    /// <summary>Claims and launches; when it started, waits for the reader to take in every status.</summary>
    public async Task<ShellState> Launched()
    {
        Assert.True(Shell.Claim());
        await Shell.LaunchAsync();
        if (Shell.State.Phase == ShellPhase.Ready)
        {
            await Until(s => s.ReadyCount == s.ServiceOrder.Count);
            await Task.Delay(50);
        }
        return Shell.State;
    }
}

// The flow of apps/mac/Sources/Midgard/AppModel.swift, step by step.
public class ShellModelTests
{
    [Fact]
    public async Task LaunchRunsTheStepsInOrder()
    {
        var fake = new FakeShell();
        var state = await fake.Launched();
        Assert.Equal(ShellPhase.Ready, state.Phase);
        Assert.Equal(new[]
        {
            "claim", "system check", "folder check", "sweep (lock held)", "prepare home", "users", "web server",
            "view configure 0 scripts, gateway none",
            "ports 2100", "start services llama gateway (http://127.0.0.1:61235, 127.0.0.1:2100)",
            "sign in analyst at http://127.0.0.1:2100/api/v1 (patient)",
            "view configure 2 scripts, gateway set",
            "view load http://127.0.0.1:61235/",
        }, fake.Calls);
        Assert.Equal("Analyst", state.SignedInAs);
        Assert.Equal(new[] { "llama", "gateway" }, state.ServiceOrder);
        Assert.Equal(2, (await fake.Until(s => s.ReadyCount == 2)).ReadyCount);
        Assert.Equal("http://127.0.0.1:61235", fake.View.AppOrigin);
        Assert.Equal("http://127.0.0.1:2100", fake.View.GatewayOrigin);
    }

    [Fact]
    public async Task TheSessionScriptCarriesTheSignInAndThePreparedAnswers()
    {
        var fake = new FakeShell();
        await fake.Launched();
        var scripts = fake.View.Scripts[^1];
        Assert.Contains("'midgard.session.token', \"token-1\"", scripts[0]);
        Assert.Contains("window.__MIDGARD_GATEWAY__ = \"http:\\/\\/127.0.0.1:2100\\/api\\/v1\"", scripts[0]);
        Assert.Contains("window.__MIDGARD_CONSOLE__ = \"http:\\/\\/127.0.0.1:61235\\/midgard\\/index.html#\\/\"", scripts[0]);
        Assert.Contains("window.__MIDGARD_PREPARED__ = \"http:\\/\\/127.0.0.1:61235\\/home\\/prepared-answers.json\"", scripts[0]);
        Assert.Contains("location.origin !== \"http:\\/\\/127.0.0.1:61235\"", scripts[0]);
        Assert.Contains("heimdall.cop.view", scripts[1]);
    }

    [Fact]
    public async Task WithoutTheLockTheSweepIsSkippedByItsStep()
    {
        var fake = new FakeShell { Claim = InstanceClaim.NotHeld };
        await fake.Launched();
        Assert.Contains("sweep (lock not held)", fake.Calls);
    }

    [Fact]
    public void AnotherRunningAppMeansExit()
    {
        var fake = new FakeShell { Claim = InstanceClaim.OtherRunning };
        Assert.False(fake.Shell.Claim());
        Assert.Equal(new[] { "claim" }, fake.Calls);
    }

    [Fact]
    public async Task AnUnsupportedPcStopsBeforeAnythingStarts()
    {
        var fake = new FakeShell { SystemProblem = "Midgard needs Windows 11." };
        var state = await fake.Launched();
        Assert.Equal(ShellPhase.Failed, state.Phase);
        Assert.Equal(new StartFailure("Midgard cannot run on this PC", "Midgard needs Windows 11.", CanRetry: false), state.Failure);
        Assert.Equal(new[] { "claim", "system check" }, fake.Calls);
    }

    [Fact]
    public async Task AnOpenFolderStopsBeforeTheSweep()
    {
        var fake = new FakeShell { FolderProblem = "Other people who use this PC can read it." };
        var state = await fake.Launched();
        Assert.Equal("Midgard's folder is not private", state.Failure!.Title);
        Assert.False(state.Failure.CanRetry);
        Assert.Equal(new[] { "claim", "system check", "folder check" }, fake.Calls);
    }

    [Fact]
    public async Task APrepareFailureIsExplainedAndCanBeRetried()
    {
        var fake = new FakeShell { PrepareFails = new IOException("disk on fire") };
        var state = await fake.Launched();
        Assert.Equal("The demo data could not be prepared", state.Failure!.Title);
        Assert.Equal("Something went wrong while copying the demo data: disk on fire", state.Failure.Details);
        Assert.True(state.Failure.CanRetry);
        Assert.False(state.Failure.CanReset);  // no working copy yet
        Assert.DoesNotContain(fake.Calls, c => c.StartsWith("start services", StringComparison.Ordinal));

        fake.PrepareFails = null;
        await fake.Shell.RetryAsync();
        Assert.Equal(ShellPhase.Ready, fake.Shell.State.Phase);
        // Try again stops what may run, and sweeps only once.
        var calls = fake.Calls;
        Assert.Single(calls, c => c.StartsWith("sweep", StringComparison.Ordinal));
        Assert.Equal(2, calls.Count(c => c == "prepare home"));
        Assert.True(calls.IndexOf("stop services") < calls.LastIndexOf("prepare home"));
    }

    [Fact]
    public async Task UnreadableUsersAreAnIncompleteInstall()
    {
        var fake = new FakeShell { UsersFail = new InvalidDataException("no \"label\"") };
        var state = await fake.Launched();
        Assert.Equal(new StartFailure("Midgard is incomplete",
                                      "The list of demo users could not be read. Install Midgard again.", CanRetry: false),
                     state.Failure);
    }

    [Fact]
    public async Task AWebServerFailureIsSaidPlainly()
    {
        var fake = new FakeShell { WebServerFails = new WebServerStartException("address in use") };
        var state = await fake.Launched();
        Assert.Equal("Midgard could not start", state.Failure!.Title);
        Assert.Equal("Its built-in web server could not start (address in use).", state.Failure.Details);
        Assert.True(state.Failure.CanReset);
    }

    [Fact]
    public async Task AWebViewThatCannotStartStopsBeforeTheServices()
    {
        var fake = new FakeShell();
        fake.View.ConfigureFails = new InvalidOperationException("runtime not found");
        var state = await fake.Launched();
        Assert.Equal("Midgard could not start", state.Failure!.Title);
        Assert.Contains("Microsoft Edge WebView2 did not start: runtime not found", state.Failure.Details);
        Assert.DoesNotContain(fake.Calls, c => c.StartsWith("start services", StringComparison.Ordinal));
    }

    [Fact]
    public async Task AMissingModelServerIsAnIncompleteInstall()
    {
        var fake = new FakeShell { ModelServerMissing = true };
        var state = await fake.Launched();
        Assert.Equal(new StartFailure("Midgard is incomplete", "The model server is missing. Install Midgard again.", CanRetry: false),
                     state.Failure);
    }

    [Fact]
    public async Task AServiceFailureNamesTheServiceAndStopsTheRest()
    {
        var fake = new FakeShell { StartFails = new ServiceFailedException("gateway", "it exited (code 1). Last log lines:\nboom") };
        var state = await fake.Launched();
        Assert.Equal("Midgard could not start Gateway", state.Failure!.Title);
        Assert.Equal(("it exited (code 1).", (string?)"boom"), state.Failure.Split());
        Assert.True(state.Failure.CanReset);
        Assert.Equal("stop services", fake.Calls[^1]);
    }

    [Fact]
    public async Task APortTakenEachTimeIsRetriedWithFreshPortsThenExplained()
    {
        var fake = new FakeShell { StartFails = new PortInUseException("mimir") };
        var state = await fake.Launched();
        Assert.Equal("Midgard could not start Mimir", state.Failure!.Title);
        Assert.Equal("Its network port was taken by another program several times in a row.", state.Failure.Details);
        Assert.Equal(new[] { "ports 2100", "ports 2200", "ports 2300", "ports 2400" },
                     fake.Calls.Where(c => c.StartsWith("ports", StringComparison.Ordinal)).ToArray());
    }

    [Fact]
    public async Task ASignInTimeoutIsTriedOnceMore()
    {
        var fake = new FakeShell { SignInFails = n => n == 1 ? SessionException.Transport("timed out") : null };
        var state = await fake.Launched();
        Assert.Equal(ShellPhase.Ready, state.Phase);
        Assert.Equal(2, fake.SignIns);
    }

    [Fact]
    public async Task ARefusedSignInStopsTheServices()
    {
        var fake = new FakeShell { SignInFails = _ => SessionException.InvalidCredentials() };
        var state = await fake.Launched();
        Assert.Equal(new StartFailure("Midgard could not sign in", "The gateway refused the demo sign-in (invalid credentials).",
                                      CanRetry: true, CanReset: true), state.Failure);
        Assert.Equal(1, fake.SignIns);
        Assert.Equal("stop services", fake.Calls[^1]);
    }

    [Fact]
    public async Task QuitClosesTheSupervisorBeforeReleasingTheLock()
    {
        var fake = new FakeShell();
        await fake.Launched();
        var before = fake.Calls.Count;
        await fake.Shell.ShutdownAsync();
        Assert.Equal(new[] { "mark stopping", "close supervisor", "final sweep", "stop web server", "release lock" },
                     fake.Calls.Skip(before).ToArray());
        Assert.Equal(ShellPhase.Stopping, fake.Shell.State.Phase);
        Assert.Equal("Stopping Midgard…", fake.Shell.State.Title);
        // Again: nothing more.
        await fake.Shell.ShutdownAsync();
        Assert.Equal(before + 5, fake.Calls.Count);
    }

    [Fact]
    public async Task QuitWhileStartingCancelsTheStartFirst()
    {
        var fake = new FakeShell { StartGate = new TaskCompletionSource() };
        Assert.True(fake.Shell.Claim());
        var launch = fake.Shell.LaunchAsync();
        await fake.Until(_ => fake.Calls.Any(c => c.StartsWith("start services", StringComparison.Ordinal)));
        await fake.Shell.ShutdownAsync();
        await launch;
        var calls = fake.Calls;
        Assert.True(calls.IndexOf("stop services") < calls.IndexOf("close supervisor"), string.Join(" | ", calls));
        Assert.Equal("release lock", calls[^1]);
        Assert.DoesNotContain(calls, c => c.StartsWith("sign in", StringComparison.Ordinal));
        Assert.Equal(ShellPhase.Stopping, fake.Shell.State.Phase);
    }

    [Fact]
    public async Task ResetDemoStopsResetsClearsAndStartsAgain()
    {
        var fake = new FakeShell();
        await fake.Launched();
        var before = fake.Calls.Count;
        Assert.True(fake.Shell.CanReset);
        Assert.True(fake.Shell.ResetDemo(() => { fake.Record("confirm"); return true; }, out var done));
        await done;
        Assert.Equal(ShellPhase.Ready, fake.Shell.State.Phase);
        Assert.Equal(new[]
        {
            "confirm", "view load about:blank", "stop services", "reset home", "view clear data", "users",
            "ports 2200", "start services llama gateway (http://127.0.0.1:61235, 127.0.0.1:2200)",
            "sign in analyst at http://127.0.0.1:2200/api/v1 (patient)",
            "view configure 2 scripts, gateway set", "view load http://127.0.0.1:61235/",
        }, fake.Calls.Skip(before).ToArray());
    }

    [Fact]
    public async Task ResetDemoNeedsTheConfirmation()
    {
        var fake = new FakeShell();
        await fake.Launched();
        var before = fake.Calls.Count;
        Assert.False(fake.Shell.ResetDemo(() => false, out var done));
        await done;
        Assert.Equal(before, fake.Calls.Count);
        Assert.Equal(ShellPhase.Ready, fake.Shell.State.Phase);
    }

    [Fact]
    public async Task ResetDemoIsNotOfferedWhereItCannotHelp()
    {
        var fake = new FakeShell { SystemProblem = "too old" };
        await fake.Launched();
        Assert.False(fake.Shell.CanReset);
        Assert.False(fake.Shell.ResetDemo(() => throw new InvalidOperationException("not asked"), out _));
    }

    [Fact]
    public async Task AFailedResetIsExplained()
    {
        var fake = new FakeShell();
        await fake.Launched();
        fake.ResetFails = new IOException("in use");
        Assert.True(fake.Shell.ResetDemo(() => true, out var done));
        await done;
        var state = fake.Shell.State;
        Assert.Equal("The demo could not be reset", state.Failure!.Title);
        Assert.False(state.Failure.CanReset);  // the working copy is in doubt: prepare it again
        fake.ResetFails = null;
        await fake.Shell.RetryAsync();
        Assert.Equal(ShellPhase.Ready, fake.Shell.State.Phase);
        Assert.Equal(2, fake.Calls.Count(c => c == "prepare home"));
    }

    [Fact]
    public async Task TheSessionIsRenewedBeforeItExpires()
    {
        var fake = new FakeShell { SessionLife = TimeSpan.FromSeconds(600) };
        await fake.Launched();
        Assert.Equal(1, fake.SignIns);
        // Refresh at half life (300 s): not yet.
        fake.Now += TimeSpan.FromSeconds(299);
        await Task.Delay(100);
        Assert.Equal(1, fake.SignIns);
        fake.Now += TimeSpan.FromSeconds(2);
        await fake.Until(_ => fake.SignIns == 2 && fake.Calls.Contains("view evaluate"));
        Assert.Contains("sign in analyst at http://127.0.0.1:2100/api/v1 (normal)", fake.Calls);
        Assert.Contains("'midgard.session.token', \"token-2\"", fake.View.Scripts[^1][0]);
        Assert.DoesNotContain("view reload", fake.Calls);
    }

    [Fact]
    public async Task AnExpiredSessionAfterSleepReloadsThePage()
    {
        var fake = new FakeShell { SessionLife = TimeSpan.FromHours(12) };
        await fake.Launched();
        fake.Now += TimeSpan.FromHours(13);
        fake.Shell.DidWake();
        await fake.Until(_ => fake.Calls.Contains("view reload"));
        // The wake, or the wall-clock check that may have seen the time first.
        Assert.InRange(fake.SignIns, 2, 3);
    }

    [Fact]
    public async Task AFailedRefreshIsTriedAgainInAMinute()
    {
        var fake = new FakeShell { SessionLife = TimeSpan.FromSeconds(600) };
        fake.SignInFails = n => n == 2 ? SessionException.Transport("down") : null;
        await fake.Launched();
        fake.Now += TimeSpan.FromSeconds(301);
        await fake.Until(_ => fake.SignIns == 2);
        await Task.Delay(100);
        Assert.Equal(2, fake.SignIns);
        fake.Now += TimeSpan.FromSeconds(61);
        await fake.Until(_ => fake.SignIns == 3);
        Assert.Equal(ShellPhase.Ready, fake.Shell.State.Phase);
    }

    [Fact]
    public async Task AServiceThatStopsAfterTheStartIsNotedAndClearedWhenBack()
    {
        var fake = new FakeShell();
        await fake.Launched();
        await fake.Events.Writer.WriteAsync(new ServiceEvent("gateway", new ServiceStatus.Failed("exited")));
        var state = await fake.Until(s => s.Note is not null);
        Assert.Equal("Gateway has stopped. Try Reset demo, or quit and reopen Midgard.", state.Note);
        Assert.Equal("Gateway has stopped", fake.Shell.StatusLine);
        await fake.Events.Writer.WriteAsync(new ServiceEvent("gateway", new ServiceStatus.Ready()));
        await fake.Until(s => s.Note is null);
        Assert.Equal("All services running", fake.Shell.StatusLine);
    }

    [Fact]
    public async Task ACrashedPageIsReloadedTwiceAMinuteThenHome()
    {
        var fake = new FakeShell();
        await fake.Launched();
        const string Heimdall = "http://127.0.0.1:61235/heimdall/index.html#/";
        await fake.Shell.ContentCrashedAsync(Heimdall);
        Assert.Equal($"view load {Heimdall}", fake.Calls[^1]);
        // Heimdall comes back in 2D.
        Assert.Contains("if (true || localStorage", fake.View.Scripts[^1][1]);
        fake.Shell.PageLoaded(Heimdall);
        await fake.Until(_ => fake.View.Scripts[^1][1].Contains("if (false || localStorage"));
        await fake.Shell.ContentCrashedAsync(Heimdall);
        await fake.Shell.ContentCrashedAsync(Heimdall);
        Assert.Equal("view load http://127.0.0.1:61235/", fake.Calls[^1]);
        Assert.Equal("The Heimdall page stopped working, so Midgard went back to the home screen.", fake.Shell.State.Note);
    }

    [Fact]
    public async Task TheHomePageGetsTheStatusLine()
    {
        var fake = new FakeShell();
        await fake.Launched();
        var before = fake.StatusCalls;
        fake.Shell.PageLoaded("http://127.0.0.1:61235/");
        Assert.Equal(before + 1, fake.StatusCalls);
        fake.Shell.PageLoaded("http://127.0.0.1:61235/forge/index.html");
        fake.Shell.PageLoaded("https://example.com/");
        Assert.Equal(before + 1, fake.StatusCalls);
    }

    [Theory]
    [InlineData("http://127.0.0.1:61235/", "home")]
    [InlineData("http://127.0.0.1:61235/home/guide.html", "home")]
    [InlineData("http://127.0.0.1:61235/heimdall/index.html#/", "heimdall")]
    [InlineData("http://127.0.0.1:61235/midgard/", "midgard")]
    [InlineData("http://127.0.0.1:61235/media/x.mp4", "home")]
    [InlineData("not a url", "home")]
    public void SectionsOfThePages(string url, string section) => Assert.Equal(section, ShellModel.Section(url));
}
