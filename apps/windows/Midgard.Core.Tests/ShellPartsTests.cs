using Midgard.Core;

namespace Midgard.Core.Tests;

// The rest of apps/mac/Tests/MidgardShellCoreTests/ShellTests.swift (the offline rules and
// navigation are OfflineRulesTests, the instance lock InstanceLockTests).
public class ShellPartsTests
{
    [Fact]
    public void HeimdallScriptSetsOnlyAnUnsetKeyOnOwnOrigin()
    {
        Assert.Equal("2d", HeimdallView.Preferred);
        var script = HeimdallView.Script("http://127.0.0.1:5", "2d");
        Assert.Contains("location.origin !== \"http:\\/\\/127.0.0.1:5\"", script);
        Assert.Contains("if (false || localStorage.getItem(\"heimdall.cop.view\") === null)", script);
        Assert.Contains("if (true || localStorage", HeimdallView.Script("http://127.0.0.1:5", "2d", force: true));
        Assert.Contains("localStorage.setItem(\"heimdall.cop.view\", \"2d\")", script);
    }

    [Fact]
    public void PageScriptEscapesText() =>
        Assert.Equal("if (typeof window.midgardStatus === 'function') { window.midgardStatus("
                     + "\"a \\\"b\\\" \\u003c\\/script\\u003e\"); }",
                     PageScript.Call("midgardStatus", "a \"b\" </script>"));

    [Fact]
    public void FriendlyNames()
    {
        Assert.Equal(new[] { "Model", "Mimir", "Gateway", "Heimdall", "Forge", "Replay", "Camera analytics", "Other" },
                     new[] { "llama", "mimir", "gateway", "heimdall", "forge", "replay", "analytics", "other" }.Select(ServiceNames.Friendly));
    }

    [Fact]
    public void StartingScreenLabels()
    {
        Assert.Equal("Waiting", ServiceNames.StatusLabel(null));
        Assert.Equal("Waiting", ServiceNames.StatusLabel(new ServiceStatus.Stopped()));
        Assert.Equal("Starting…", ServiceNames.StatusLabel(new ServiceStatus.Starting()));
        Assert.Equal("Ready", ServiceNames.StatusLabel(new ServiceStatus.Ready()));
        Assert.Equal("Restarting (attempt 1)…", ServiceNames.StatusLabel(new ServiceStatus.Restarting(1, "x")));
        Assert.Equal("Port in use, retrying…", ServiceNames.StatusLabel(new ServiceStatus.Failed("port in use")));
        Assert.Equal("Failed", ServiceNames.StatusLabel(new ServiceStatus.Failed("exited")));
    }

    static SuitePorts PortsFrom(ushort first) =>
        SuitePorts.From(Enumerable.Range(first, SuitePorts.Count).Select(p => (ushort)p).ToArray())!;

    /// <summary>Records the calls in order and hands out ports 1010…, 1020…, ….</summary>
    sealed class Calls
    {
        readonly Lock gate = new();
        readonly List<string> list = [];
        ushort next = 1000;
        public void Add(string s) { lock (gate) list.Add(s); }
        public List<string> All { get { lock (gate) return [.. list]; } }
        public SuitePorts Ports() { lock (gate) { next += 10; return PortsFrom(next); } }
    }

    [Fact]
    public async Task SuiteStartRetriesWithFreshPortsAfterPortInUse()
    {
        var calls = new Calls();
        var ports = await SuiteStart.RunAsync(
            choosePorts: calls.Ports,
            start: (p, _) =>
            {
                calls.Add($"start {p.GatewayHttp}");
                return p.GatewayHttp < 1030 ? throw new PortInUseException("gateway") : Task.CompletedTask;
            },
            stopAll: () => { calls.Add("stopAll"); return Task.CompletedTask; },
            onRetry: (service, retry) => { calls.Add($"retry {retry} {service}"); return Task.CompletedTask; });
        Assert.Equal(1030, ports.GatewayHttp);
        Assert.Equal(new[] { "start 1010", "stopAll", "retry 1 gateway", "start 1020", "stopAll", "retry 2 gateway", "start 1030" },
                     calls.All);
    }

    [Fact]
    public async Task SuiteStartGivesUpAfterThreeRetries()
    {
        var calls = new Calls();
        var e = await Assert.ThrowsAsync<PortInUseException>(() => SuiteStart.RunAsync(
            choosePorts: calls.Ports,
            start: (_, _) => { calls.Add("start"); throw new PortInUseException("mimir"); },
            stopAll: () => { calls.Add("stopAll"); return Task.CompletedTask; }));
        Assert.Equal("mimir", e.Service);
        Assert.Equal(4, calls.All.Count(c => c == "start"));
        Assert.Equal(3, calls.All.Count(c => c == "stopAll"));
    }

    [Fact]
    public async Task SuiteStartDoesNotRetryOtherFailures()
    {
        var calls = new Calls();
        var e = await Assert.ThrowsAsync<ServiceFailedException>(() => SuiteStart.RunAsync(
            choosePorts: calls.Ports,
            start: (_, _) => { calls.Add("start"); throw new ServiceFailedException("forge", "x"); },
            stopAll: () => { calls.Add("stopAll"); return Task.CompletedTask; }));
        Assert.Equal("forge", e.Service);
        Assert.Equal(new[] { "start" }, calls.All);
    }

    [Fact]
    public async Task SuiteStartCancellationStopsAll()
    {
        var calls = new Calls();
        using var cancel = new CancellationTokenSource();
        var run = SuiteStart.RunAsync(
            choosePorts: calls.Ports,
            start: async (_, c) => { calls.Add("start"); await Task.Delay(TimeSpan.FromSeconds(10), c); },
            stopAll: () => { calls.Add("stopAll"); return Task.CompletedTask; },
            cancellation: cancel.Token);
        await Task.Delay(100);
        await cancel.CancelAsync();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => run);
        Assert.Equal(new[] { "start", "stopAll" }, calls.All);
    }

    [Fact]
    public void OnlyTheLatestNavigationsOutcomeCounts()
    {
        Assert.Equal(PageLoads.Outcome.Loaded, PageLoads.Of(7, 7, success: true, cancelled: false));
        Assert.Equal(PageLoads.Outcome.Failed, PageLoads.Of(7, 7, success: false, cancelled: false));
        Assert.Equal(PageLoads.Outcome.Ignored, PageLoads.Of(7, 7, success: false, cancelled: true));
        // Superseded (the 72e55df e2e failure: ConnectionAborted for a load another one replaced).
        Assert.Equal(PageLoads.Outcome.Ignored, PageLoads.Of(6, 7, success: false, cancelled: false));
        Assert.Equal(PageLoads.Outcome.Ignored, PageLoads.Of(6, 7, success: true, cancelled: false));
    }

    [Fact]
    public void TwoReloadsPerMinute()
    {
        var budget = new ReloadBudget();
        var t0 = DateTimeOffset.FromUnixTimeSeconds(1000);
        Assert.True(budget.Take(t0));
        Assert.True(budget.Take(t0.AddSeconds(10)));
        Assert.False(budget.Take(t0.AddSeconds(20)));
        Assert.False(budget.Take(t0.AddSeconds(59)));
        Assert.True(budget.Take(t0.AddSeconds(60)));  // the first has aged out
        Assert.False(budget.Take(t0.AddSeconds(65)));
    }

    [Fact]
    public void OnlyRepeatedRestartsAndFailuresAlert()
    {
        Assert.Null(ServiceAlert.Message("heimdall", new ServiceStatus.Ready()));
        Assert.Null(ServiceAlert.Message("heimdall", new ServiceStatus.Starting()));
        Assert.Null(ServiceAlert.Message("heimdall", new ServiceStatus.Stopped()));
        Assert.Null(ServiceAlert.Message("heimdall", new ServiceStatus.Restarting(2, null)));
        Assert.Equal("Heimdall has stopped. Try Reset demo, or quit and reopen Midgard.",
                     ServiceAlert.Message("heimdall", new ServiceStatus.Restarting(3, "x")));
        Assert.Equal("Model has stopped. Try Reset demo, or quit and reopen Midgard.",
                     ServiceAlert.Message("llama", new ServiceStatus.Failed("boom")));
    }

    [Fact]
    public void WakeDecisionsByWallClock()
    {
        var refreshAt = DateTimeOffset.FromUnixTimeSeconds(1000);
        var expiresAt = DateTimeOffset.FromUnixTimeSeconds(1600);
        SessionTiming.Action At(long t) => SessionTiming.Decide(DateTimeOffset.FromUnixTimeSeconds(t), refreshAt, expiresAt);
        Assert.Equal(SessionTiming.Action.Wait, At(999));
        Assert.Equal(SessionTiming.Action.Refresh, At(1000));
        Assert.Equal(SessionTiming.Action.Refresh, At(1599));
        Assert.Equal(SessionTiming.Action.RefreshAndReload, At(1600));
        Assert.Equal(SessionTiming.Action.RefreshAndReload, At(99_999));
    }

    [Fact]
    public void HomeStatusIgnoresAnOptionalService()
    {
        string[] order = ["gateway", "heimdall", "analytics"];
        var optional = new HashSet<string> { "analytics" };
        string Line(ServiceStatus? analytics, ServiceStatus? gateway = null)
        {
            var st = new Dictionary<string, ServiceStatus>
            {
                ["gateway"] = gateway ?? new ServiceStatus.Ready(),
                ["heimdall"] = new ServiceStatus.Ready(),
            };
            if (analytics is not null) st["analytics"] = analytics;
            return HomeStatus.Line(order, st, optional, replaying: true);
        }
        Assert.Equal("All services running · replaying recorded hour", Line(new ServiceStatus.Starting()));
        Assert.Equal("All services running · replaying recorded hour", Line(new ServiceStatus.Ready()));
        Assert.Equal("All services running · replaying recorded hour · camera analytics unavailable",
                     Line(new ServiceStatus.Failed("x")));
        Assert.Equal("Gateway has stopped", Line(new ServiceStatus.Ready(), new ServiceStatus.Failed("x")));
        Assert.Equal("Starting…", Line(new ServiceStatus.Ready(), new ServiceStatus.Starting()));
        Assert.Equal("Gateway is restarting…", Line(new ServiceStatus.Ready(), new ServiceStatus.Restarting(1, null)));
        Assert.Equal("Camera analytics has stopped",
                     HomeStatus.Line(order, new Dictionary<string, ServiceStatus>
                     {
                         ["gateway"] = new ServiceStatus.Ready(), ["heimdall"] = new ServiceStatus.Ready(),
                         ["analytics"] = new ServiceStatus.Failed("x"),
                     }, new HashSet<string>(), replaying: false));
    }

    [Fact]
    public void OptionalServiceAlertIsNotFatal()
    {
        Assert.Equal("Camera analytics is unavailable; the rest of Midgard works.",
                     ServiceAlert.Message("analytics", new ServiceStatus.Failed("x"), optional: true));
        Assert.Equal("Gateway has stopped. Try Reset demo, or quit and reopen Midgard.",
                     ServiceAlert.Message("gateway", new ServiceStatus.Failed("x")));
        Assert.Null(ServiceAlert.Message("analytics", new ServiceStatus.Starting(), optional: true));
    }

    [Fact]
    public void FailureDetailsSplitAtTheLogLines()
    {
        Assert.Equal(("mimir exited.", (string?)"line 1\nline 2"),
                     new StartFailure("t", "mimir exited. Last log lines:\nline 1\nline 2").Split());
        Assert.Equal(("plain words", (string?)null), new StartFailure("t", "plain words").Split());
    }

    [Fact]
    public void WorkingCopyErrorsInPlainWords()
    {
        Assert.Equal("Something went wrong while copying the demo data: disk on fire",
                     Explain.WorkingCopy(new IOException("disk on fire")));
    }

    [Fact]
    public void StatusesForTheLog()
    {
        Assert.Equal("restarting (attempt 2): exited", Explain.Status(new ServiceStatus.Restarting(2, "exited\nmore")));
        Assert.Equal("failed: boom", Explain.Status(new ServiceStatus.Failed("boom\nLast log lines")));
        Assert.Equal("ready", Explain.Status(new ServiceStatus.Ready()));
    }

    [Fact]
    public void ShellLogAppendsTimestampedLines()
    {
        var dir = Path.Join(Path.GetTempPath(), $"shell-log-{Guid.NewGuid():N}");
        try
        {
            var log = new ShellLog(Path.Join(dir, "Logs", "shell.log"));
            log.Write("one");
            log.Write("two");
            var lines = File.ReadAllLines(log.File);
            Assert.Equal(2, lines.Length);
            Assert.EndsWith(" one", lines[0]);
            Assert.EndsWith(" two", lines[1]);
            Assert.True(DateTimeOffset.TryParse(lines[0].Split(' ')[0], System.Globalization.CultureInfo.InvariantCulture,
                                                System.Globalization.DateTimeStyles.None, out _), lines[0]);
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }

    [Fact]
    public void SettingsRememberTheShortcutAnswerAndKeepOtherKeys()
    {
        var dir = Path.Join(Path.GetTempPath(), $"settings-{Guid.NewGuid():N}");
        var file = Path.Join(dir, "settings.json");
        try
        {
            var fresh = AppSettings.Load(file);
            Assert.Null(fresh.StartMenuShortcut);
            Assert.True(AppSettings.ShouldOfferShortcut(fresh, devLayout: false, shortcutExists: false));
            Assert.False(AppSettings.ShouldOfferShortcut(fresh, devLayout: true, shortcutExists: false));
            Assert.False(AppSettings.ShouldOfferShortcut(fresh, devLayout: false, shortcutExists: true));

            Directory.CreateDirectory(dir);
            File.WriteAllText(file, """{"other": 5}""");
            var settings = AppSettings.Load(file);
            settings.StartMenuShortcut = "declined";
            settings.Save(file);
            var again = AppSettings.Load(file);
            Assert.Equal("declined", again.StartMenuShortcut);
            Assert.False(AppSettings.ShouldOfferShortcut(again, devLayout: false, shortcutExists: false));
            Assert.Contains("\"other\": 5", File.ReadAllText(file));
            Assert.False(File.Exists(file + ".tmp"));

            File.WriteAllText(file, "not json");
            Assert.Null(AppSettings.Load(file).StartMenuShortcut);
        }
        finally
        {
            if (Directory.Exists(dir)) Directory.Delete(dir, recursive: true);
        }
    }
}
