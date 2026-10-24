using System.Diagnostics;
using System.Runtime.Versioning;
using Midgard.Core;

namespace Midgard.Core.Tests;

/// <summary>Every status the supervisor publishes, in order, read from its status stream.</summary>
sealed class StatusRecorder
{
    readonly Lock gate = new();
    readonly List<ServiceEvent> events = [];
    readonly CancellationTokenSource stop = new();

    public StatusRecorder(Supervisor supervisor) => _ = Task.Run(async () =>
    {
        try
        {
            await foreach (var e in supervisor.Statuses(stop.Token))
            {
                lock (gate) events.Add(e);
            }
        }
        catch (OperationCanceledException)
        {
        }
    });

    public void Cancel() => stop.Cancel();

    public List<ServiceStatus> Statuses(string name)
    {
        lock (gate) return events.Where(e => e.Service == name).Select(e => e.Status).ToList();
    }

    /// <summary>Statuses with Restarting shortened to "restarting N", for comparing sequences.</summary>
    public List<string> Labels(string name) => Statuses(name).Select(Label).ToList();

    public static string Label(ServiceStatus status) => status switch
    {
        ServiceStatus.Starting => "starting",
        ServiceStatus.Ready => "ready",
        ServiceStatus.Restarting r => $"restarting {r.Attempt}",
        ServiceStatus.Failed => "failed",
        ServiceStatus.Stopped => "stopped",
        _ => "?",
    };

    /// <summary>The services that reached a status with this label, in the order they reached it.</summary>
    public List<string> NamesReaching(string label)
    {
        lock (gate) return events.Where(e => Label(e.Status) == label).Select(e => e.Service).ToList();
    }

    /// <summary>Polls until <paramref name="condition"/> holds for the service's statuses, or the timeout passes.</summary>
    public async Task<bool> Wait(string name, Func<List<ServiceStatus>, bool> condition, double timeout = 10)
    {
        var deadline = DateTime.UtcNow.AddSeconds(timeout);
        while (DateTime.UtcNow < deadline)
        {
            if (condition(Statuses(name))) return true;
            await Task.Delay(20);
        }
        return condition(Statuses(name));
    }

    public async Task Expect(string name, string[] labels, double timeout = 5)
    {
        await Wait(name, s => s.Select(Label).SequenceEqual(labels), timeout);
        Assert.Equal(labels, Labels(name));
    }
}

static class Processes
{
    /// <summary>Whether the process with this pid has gone, polled for up to <paramref name="timeout"/> s.</summary>
    public static async Task<bool> Gone(int pid, double timeout = 5)
    {
        var deadline = DateTime.UtcNow.AddSeconds(timeout);
        while (true)
        {
            if (!Exists(pid)) return true;
            if (DateTime.UtcNow >= deadline) return false;
            await Task.Delay(20);
        }
    }

    public static bool Exists(int pid)
    {
        try
        {
            using var process = Process.GetProcessById(pid);
            return !process.HasExited;
        }
        catch (ArgumentException)
        {
            return false;
        }
        catch (InvalidOperationException)
        {
            return false;
        }
    }
}

// The cases of apps/mac/Tests/MidgardShellCoreTests/SupervisorTests.swift, with the fake
// services this assembly's exe plays (FakeService) in place of shell scripts, plus the
// retry before ready and the Job Object cases. Windows only: each test returns at once
// elsewhere.
[SupportedOSPlatform("windows")]
public sealed class SupervisorTests : IAsyncLifetime
{
    readonly string dir = Path.Join(Path.GetTempPath(), $"midgard-sup-{Guid.NewGuid():N}");
    Supervisor? supervisor;
    StatusRecorder? recorder;

    /// <summary>Short delays so restarts and stops finish in well under a second.</summary>
    static readonly Supervisor.Timing Fast = new()
    {
        BackoffBase = TimeSpan.FromSeconds(0.05),
        BackoffCap = TimeSpan.FromSeconds(0.2),
        StableAfter = TimeSpan.FromSeconds(300),
        HealthInterval = TimeSpan.FromSeconds(0.1),
        StopGrace = TimeSpan.FromSeconds(1),
    };

    Supervisor Sup => supervisor!;
    StatusRecorder Rec => recorder!;

    public Task InitializeAsync()
    {
        Directory.CreateDirectory(dir);
        if (OperatingSystem.IsWindows()) MakeSupervisor(Fast);
        return Task.CompletedTask;
    }

    public async Task DisposeAsync()
    {
        if (supervisor is not null) await supervisor.StopAllAsync();
        recorder?.Cancel();
        try
        {
            Directory.Delete(dir, recursive: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    void MakeSupervisor(Supervisor.Timing timing)
    {
        supervisor = new Supervisor(timing);
        recorder = new StatusRecorder(supervisor);
    }

    ServiceSpec Fake(string name, string[] steps, Readiness? readiness = null, double timeout = 10) =>
        new(name, FakeService.Exe, ["fake", .. steps], readiness ?? new Readiness.LogLine("listening on"),
            Path.Join(dir, "logs", $"{name}.log"))
        {
            ReadyTimeout = TimeSpan.FromSeconds(timeout),
        };

    string Log(string name)
    {
        try
        {
            using var stream = new FileStream(Path.Join(dir, "logs", $"{name}.log"), FileMode.Open, FileAccess.Read,
                                              FileShare.ReadWrite | FileShare.Delete);
            return new StreamReader(stream).ReadToEnd();
        }
        catch (IOException)
        {
            return "";
        }
    }

    static bool IsFailed(List<ServiceStatus> s) => s.Count > 0 && s[^1] is ServiceStatus.Failed;

    [Fact]
    public void FakeServiceIsThere()
    {
        if (!OperatingSystem.IsWindows()) return;
        Assert.True(File.Exists(FakeService.Exe), $"{FakeService.Exe} is missing");
    }

    [Fact]
    public async Task ReadyByLogLineWithItsArgumentsAsGiven()
    {
        if (!OperatingSystem.IsWindows()) return;
        var s = Fake("svc", ["args", "note:b c", "", "note:q\"uote", @"note:trail\", @"note:a\\""b", "sleep:200",
                             "say:listening on 1", "wait"]);
        await Sup.StartAsync([s]);
        await Rec.Expect("svc", ["starting", "ready"]);
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("svc"));
        Assert.Contains(@"args: fake | args | note:b c |  | note:q""uote | note:trail\ | note:a\\""b | sleep:200", Log("svc"));
        Assert.NotNull(Sup.PidOf("svc"));
    }

    [Fact]
    public async Task EnvironmentIsAddedToTheShells()
    {
        if (!OperatingSystem.IsWindows()) return;
        var s = Fake("svc", ["env:MIDGARD_FAKE_VALUE", "env:PATH", "say:listening on", "wait"]) with
        {
            Environment = new Dictionary<string, string> { ["MIDGARD_FAKE_VALUE"] = "one two" },
        };
        await Sup.StartAsync([s]);
        Assert.Contains("env MIDGARD_FAKE_VALUE=one two", Log("svc"));
        Assert.Contains($"env PATH={Environment.GetEnvironmentVariable("PATH")}", Log("svc"));
    }

    /// <summary>A ready line left in the log by an earlier run does not count.</summary>
    [Fact]
    public async Task OldLogLinesDoNotCount()
    {
        if (!OperatingSystem.IsWindows()) return;
        var s = Fake("svc", ["wait"], timeout: 0.5);
        Directory.CreateDirectory(Path.GetDirectoryName(s.LogFile)!);
        File.WriteAllText(s.LogFile, "listening on (old run)\n");
        var e = await Assert.ThrowsAsync<ServiceFailedException>(() => Sup.StartAsync([s]));
        Assert.Equal("svc", e.Service);
        Assert.StartsWith("listening on (old run)\n", Log("svc"));
    }

    [Fact]
    public async Task ReadyByHealthUrl()
    {
        if (!OperatingSystem.IsWindows()) return;
        var port = Ports.Free(1)[0];
        // The health URL starts answering only after a while.
        var s = Fake("llama", ["sleep:600", $"http:{port}", "wait"],
                     new Readiness.Health(new Uri($"http://127.0.0.1:{port}/health")));
        var started = DateTime.UtcNow;
        await Sup.StartAsync([s]);
        Assert.True(DateTime.UtcNow - started >= TimeSpan.FromSeconds(0.5));
        await Rec.Expect("llama", ["starting", "ready"]);
    }

    [Fact]
    public async Task NeverReadyFailsWithTheMacsMessageAndLogTail()
    {
        if (!OperatingSystem.IsWindows()) return;
        var never = Fake("never", ["lines:30", "wait"], timeout: 2);
        var marker = Path.Join(dir, "after-ran");
        var after = Fake("after", [$"marker:{marker}", "say:listening on", "wait"]);
        var e = await Assert.ThrowsAsync<ServiceFailedException>(() => Sup.StartAsync([never, after]));
        Assert.Equal("never", e.Service);
        Assert.StartsWith("never was not ready after 2 s. Last log lines:\n", e.Reason);
        Assert.Contains("line 30", e.Reason);
        Assert.Contains("line 11", e.Reason);
        Assert.DoesNotContain("line 10\n", e.Reason);
        Assert.Equal($"never: {e.Reason}", e.Message);
        Assert.True(await Rec.Wait("never", s => s.Count == 2));
        var failed = Assert.IsType<ServiceStatus.Failed>(Rec.Statuses("never")[^1]);
        Assert.Contains("line 30", failed.Reason);
        Assert.Empty(Rec.Statuses("after"));
        Assert.False(File.Exists(marker), "nothing after a failure is started");
        Assert.Null(Sup.PidOf("never"));
    }

    // Optional services (the camera analytics, W3)

    ServiceSpec OptionalFake(string name, string[] steps, double timeout = 10) =>
        Fake(name, steps, timeout: timeout) with { Optional = true };

    /// <summary>
    /// An optional service that crashes before it is ready (twice: the retry too): start does
    /// not throw, it is published failed with the reason, noted in its log, and the others
    /// keep running.
    /// </summary>
    [Fact]
    public async Task FailingOptionalServiceDoesNotFailTheStart()
    {
        if (!OperatingSystem.IsWindows()) return;
        var a = Fake("a", ["say:listening on", "wait"]);
        var opt = OptionalFake("analytics", ["say:boom", "exit:3"]);
        await Sup.StartAsync([a, opt]);
        Assert.True(await Rec.Wait("analytics", IsFailed), string.Join(", ", Rec.Labels("analytics")));
        var failed = Assert.IsType<ServiceStatus.Failed>(Rec.Statuses("analytics")[^1]);
        Assert.Contains("boom", failed.Reason);
        Assert.Equal(new[] { "starting", "restarting 1", "failed" }, Rec.Labels("analytics"));
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("a"));
        Assert.Contains("analytics (optional) did not start", Log("analytics"));
    }

    /// <summary>
    /// Start returns without waiting for an optional service (here never ready); when it
    /// times out it is failed and stopped, and nothing else is affected.
    /// </summary>
    [Fact]
    public async Task StartDoesNotWaitForAnOptionalService()
    {
        if (!OperatingSystem.IsWindows()) return;
        var opt = OptionalFake("analytics", ["wait"], timeout: 1.5);
        var a = Fake("a", ["say:listening on", "wait"]);
        var started = DateTime.UtcNow;
        await Sup.StartAsync([opt, a]);
        Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(1.2), "start waited for the optional service");
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("a"));
        Assert.IsType<ServiceStatus.Starting>(Sup.StatusOf("analytics"));
        Assert.True(await Rec.Wait("analytics", IsFailed), string.Join(", ", Rec.Labels("analytics")));
        Assert.Null(Sup.PidOf("analytics"));
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("a"));
    }

    [Fact]
    public async Task OptionalServiceBecomesReadyAndIsStopped()
    {
        if (!OperatingSystem.IsWindows()) return;
        await Sup.StartAsync([OptionalFake("analytics", ["sleep:300", "say:listening on", "wait"])]);
        await Rec.Expect("analytics", ["starting", "ready"]);
        var pid = Sup.PidOf("analytics")!.Value;
        await Sup.StopAllAsync();
        Assert.True(await Processes.Gone(pid));
    }

    /// <summary>StopAll while an optional service is still starting: it is stopped, never failed.</summary>
    [Fact]
    public async Task StopAllWhileAnOptionalServiceStarts()
    {
        if (!OperatingSystem.IsWindows()) return;
        await Sup.StartAsync([OptionalFake("analytics", ["wait"])]);
        await Task.Delay(200);
        var pid = Sup.PidOf("analytics")!.Value;
        await Sup.StopAllAsync();
        Assert.True(await Processes.Gone(pid));
        await Task.Delay(300);
        Assert.Equal(new[] { "starting", "stopped" }, Rec.Labels("analytics"));
    }

    /// <summary>
    /// The analytics' runtime missing (helpers\analytics\python.exe not there): the optional
    /// service is failed, the start goes on, and the required service after it starts.
    /// </summary>
    [Fact]
    public async Task OptionalServiceThatCannotLaunchDoesNotBlockTheStart()
    {
        if (!OperatingSystem.IsWindows()) return;
        var opt = OptionalFake("analytics", []) with { Executable = Path.Join(dir, "analytics", "python.exe") };
        var a = Fake("a", ["say:listening on", "wait"]);
        await Sup.StartAsync([opt, a]);
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("a"));
        Assert.True(await Rec.Wait("analytics", IsFailed), string.Join(", ", Rec.Labels("analytics")));
        Assert.Null(Sup.PidOf("analytics"));
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("a"));
    }

    // Exits before ready: one retry

    /// <summary>A required service that exits before it is ready is launched once more; then the start fails.</summary>
    [Fact]
    public async Task ExitBeforeReadyIsRetriedOnceThenFails()
    {
        if (!OperatingSystem.IsWindows()) return;
        var s = Fake("crash", ["say:boom", "exit:3"]);
        var e = await Assert.ThrowsAsync<ServiceFailedException>(() => Sup.StartAsync([s]));
        Assert.StartsWith("crash exited with status 3 before it was ready. Last log lines:\nboom", e.Reason);
        Assert.Equal(new[] { "starting", "restarting 1", "failed" }, Rec.Labels("crash"));
        var restarting = Assert.IsType<ServiceStatus.Restarting>(Rec.Statuses("crash")[1]);
        Assert.Contains("boom", restarting.Reason);
        Assert.Equal(2, Log("crash").Split("boom").Length - 1);
        Assert.Contains("Launching it again in 0.05 s (attempt 1)", Log("crash"));
    }

    /// <summary>A crash before ready, then a launch that works: the start goes on.</summary>
    [Fact]
    public async Task CrashBeforeReadyIsRetriedOnce()
    {
        if (!OperatingSystem.IsWindows()) return;
        var marker = Path.Join(dir, "ran-once");
        var s = Fake("flaky", [$"marker:{marker}", "say:listening on", "wait"]);
        var next = Fake("next", ["say:listening on", "wait"]);
        await Sup.StartAsync([s, next]);
        Assert.Equal(new[] { "starting", "restarting 1", "ready" }, Rec.Labels("flaky"));
        var restarting = Assert.IsType<ServiceStatus.Restarting>(Rec.Statuses("flaky")[1]);
        Assert.StartsWith("flaky exited with status 3 before it was ready.", restarting.Reason);
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("next"));
    }

    [Fact]
    public async Task PortInUseIsNotRetried()
    {
        if (!OperatingSystem.IsWindows()) return;
        foreach (var message in new[]
                 {
                     "Error: Only one usage of each socket address (protocol/network address/port) is normally permitted. (os error 10048)",
                     "Error: Address already in use (os error 48)", "bind: ADDRESS IN USE",
                     "error: Couldn't bind HTTP server socket, hostname: 127.0.0.1, port: 8080.",
                 })
        {
            var s = Fake("clash", [$"err:{message}", "exit:1"]);
            var e = await Assert.ThrowsAsync<PortInUseException>(() => Sup.StartAsync([s]));
            Assert.Equal("clash", e.Service);
            Assert.True(await Rec.Wait("clash", l => l.Count > 0 && l[^1] == new ServiceStatus.Failed("port in use")));
            await Sup.StopAllAsync();
            Assert.True(await Rec.Wait("clash", l => l.Count > 0 && l[^1] is ServiceStatus.Stopped));
            Assert.DoesNotContain("restarting 1", Rec.Labels("clash"));
        }
    }

    [Fact]
    public async Task CannotStartAMissingExecutable()
    {
        if (!OperatingSystem.IsWindows()) return;
        var s = Fake("missing", []) with { Executable = Path.Join(dir, "no-such.exe") };
        var e = await Assert.ThrowsAsync<ServiceFailedException>(() => Sup.StartAsync([s]));
        Assert.StartsWith($"cannot start {s.Executable}", e.Reason);
    }

    // Restarts after ready

    [Fact]
    public async Task RestartsWithBackoff()
    {
        if (!OperatingSystem.IsWindows()) return;
        var s = Fake("flaky", ["sleep:100", "say:listening on", "sleep:300", "exit:1"]);
        await Sup.StartAsync([s]);
        Assert.True(await Rec.Wait("flaky", _ => Rec.Labels("flaky").Contains("restarting 3")),
                    string.Join(", ", Rec.Labels("flaky")));
        Assert.Equal(new[] { "starting", "ready", "restarting 1", "ready", "restarting 2", "ready" },
                     Rec.Labels("flaky").Take(6));
        var first = Assert.IsType<ServiceStatus.Restarting>(Rec.Statuses("flaky")[2]);
        Assert.Equal("exited with status 1", first.Reason);
        Assert.Contains("restarting in", Log("flaky"));

        await Sup.StopAllAsync();
        Assert.True(await Rec.Wait("flaky", l => l.Count > 0 && l[^1] is ServiceStatus.Stopped));
        var count = Rec.Statuses("flaky").Count;
        await Task.Delay(800);
        Assert.Equal(count, Rec.Statuses("flaky").Count);
    }

    [Fact]
    public async Task BackoffResetsAfterStayingUp()
    {
        if (!OperatingSystem.IsWindows()) return;
        MakeSupervisor(Fast with { StableAfter = TimeSpan.FromSeconds(0.1) });
        await Sup.StartAsync([Fake("flaky", ["say:listening on", "sleep:300", "exit:1"])]);
        Assert.True(await Rec.Wait("flaky", _ => Rec.Labels("flaky").Count(l => l == "restarting 1") >= 3),
                    string.Join(", ", Rec.Labels("flaky")));
        Assert.DoesNotContain("restarting 2", Rec.Labels("flaky"));
    }

    /// <summary>A relaunch that fails is retried, and the next attempt says why it failed.</summary>
    [Fact]
    public async Task FailedRelaunchReasonIsPublished()
    {
        if (!OperatingSystem.IsWindows()) return;
        // The first run passes the marker (it is there), is ready and exits; the relaunch
        // finds it gone, says "first run" and exits before it is ready.
        var marker = Path.Join(dir, "marker");
        File.WriteAllText(marker, "");
        await Sup.StartAsync([Fake("flaky", [$"marker:{marker}", "say:listening on", "sleep:300", "exit:1"])]);
        File.Delete(marker);
        Assert.True(await Rec.Wait("flaky", _ => Rec.Labels("flaky").Contains("restarting 2")),
                    string.Join(", ", Rec.Labels("flaky")));
        var reason = Rec.Statuses("flaky").OfType<ServiceStatus.Restarting>().First(r => r.Attempt == 2).Reason;
        Assert.Contains("first run", reason);
    }

    [Fact]
    public async Task BigLogIsRotatedAtStart()
    {
        if (!OperatingSystem.IsWindows()) return;
        var s = Fake("svc", ["say:listening on", "wait"]);
        var logs = Path.GetDirectoryName(s.LogFile)!;
        Directory.CreateDirectory(logs);
        var old = Path.Join(logs, "svc.log.1");
        File.WriteAllText(old, "older");
        await using (var stream = new FileStream(s.LogFile, FileMode.Create))
        {
            stream.Write("first\n"u8);
            stream.SetLength(21L * 1024 * 1024);
        }
        await Sup.StartAsync([s]);
        Assert.Equal(21L * 1024 * 1024, new FileInfo(old).Length);
        Assert.StartsWith("listening on", Log("svc"));

        // A small log is appended to, not rotated.
        await Sup.StopAllAsync();
        await Sup.StartAsync([s]);
        Assert.Equal(3, Log("svc").Split("listening on").Length);
        Assert.Equal(21L * 1024 * 1024, new FileInfo(old).Length);
    }

    [Fact]
    public void BackoffDelays()
    {
        var timing = new Supervisor.Timing();
        Assert.Equal(new double[] { 2, 4, 8, 16, 32, 60, 60, 60 },
                     Enumerable.Range(1, 8).Select(a => timing.Backoff(a).TotalSeconds));
        Assert.Equal(TimeSpan.FromSeconds(300), timing.StableAfter);
        Assert.Equal(TimeSpan.FromSeconds(15), timing.StopGrace);
        Assert.Equal(TimeSpan.FromSeconds(0.5), timing.HealthInterval);
    }

    [Fact]
    public async Task CancellingStartStopsWaiting()
    {
        if (!OperatingSystem.IsWindows()) return;
        using var cancel = new CancellationTokenSource();
        var starting = Sup.StartAsync([Fake("never", ["wait"])], cancel.Token);
        await Rec.Wait("never", l => l.Count == 1);
        var began = DateTime.UtcNow;
        cancel.Cancel();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => starting);
        Assert.True(DateTime.UtcNow - began < TimeSpan.FromSeconds(2));
    }

    // Stopping

    [Fact]
    public async Task StopAllLeavesNoProcessesInReverseOrder()
    {
        if (!OperatingSystem.IsWindows()) return;
        var childFile = Path.Join(dir, "child.pid");
        var a = Fake("a", ["say:listening on", "wait"]);
        var b = Fake("b", ["say:listening on", "wait"]);
        // Replay's heimdall-source runs: a child of the service, ended with it.
        var replay = Fake("replay", [$"child:{childFile}", "say:\"replay started\"", "wait"],
                          new Readiness.LogLine("\"replay started\""));
        await Sup.StartAsync([a, b, replay]);
        var pids = new[] { "a", "b", "replay" }.Select(n => Sup.PidOf(n)!.Value).ToList();
        var child = int.Parse(File.ReadAllText(childFile));
        Assert.True(Processes.Exists(child));

        var started = DateTime.UtcNow;
        await Sup.StopAllAsync();
        Assert.True(DateTime.UtcNow - started < TimeSpan.FromSeconds(5));
        foreach (var pid in pids.Append(child)) Assert.True(await Processes.Gone(pid), $"pid {pid} still exists");
        foreach (var name in new[] { "a", "b", "replay" })
        {
            Assert.True(await Rec.Wait(name, l => l.Count > 0 && l[^1] is ServiceStatus.Stopped), name);
            Assert.Null(Sup.PidOf(name));
        }
        Assert.Equal(new[] { "replay", "b", "a" }, Rec.NamesReaching("stopped"));
        await Sup.StopAllAsync();
        await Sup.StopAllAsync();
        await Task.Delay(100);
        Assert.Single(Rec.Statuses("a").OfType<ServiceStatus.Stopped>());
    }

    [Fact]
    public async Task StopAllConcurrentlyAndAfterwardsStartAgain()
    {
        if (!OperatingSystem.IsWindows()) return;
        var a = Fake("a", ["say:listening on", "wait"]);
        await Sup.StartAsync([a]);
        var first = Sup.PidOf("a")!.Value;
        await Task.WhenAll(Sup.StopAllAsync(), Sup.StopAllAsync());
        Assert.True(await Processes.Gone(first, 0.5));
        await Sup.StartAsync([a]);
        Assert.IsType<ServiceStatus.Ready>(Sup.StatusOf("a"));
        Assert.NotEqual(first, Sup.PidOf("a"));
    }

    /// <summary>Reset demo then quit: the second start's processes must be the ones stopped.</summary>
    [Fact]
    public async Task StopAllAfterSecondStartStopsTheSecondGeneration()
    {
        if (!OperatingSystem.IsWindows()) return;
        var a = Fake("a", ["say:listening on", "wait"]);
        var b = Fake("b", ["say:listening on", "wait"]);
        await Sup.StartAsync([a, b]);
        var first = new[] { Sup.PidOf("a")!.Value, Sup.PidOf("b")!.Value };
        await Sup.StopAllAsync();
        await Sup.StartAsync([a, b]);
        var second = new[] { Sup.PidOf("a")!.Value, Sup.PidOf("b")!.Value };
        Assert.Empty(first.Intersect(second));
        await Sup.CloseAsync();
        foreach (var pid in first.Concat(second)) Assert.True(await Processes.Gone(pid, 2), $"pid {pid} still running");
    }

    [Fact]
    public async Task NoStartAfterClose()
    {
        if (!OperatingSystem.IsWindows()) return;
        var a = Fake("a", ["say:listening on", "wait"]);
        await Sup.StartAsync([a]);
        var first = Sup.PidOf("a")!.Value;
        await Sup.CloseAsync();
        Assert.True(Sup.IsClosed);
        Assert.True(await Processes.Gone(first, 0.5));
        await Assert.ThrowsAsync<SupervisorStoppedException>(() => Sup.StartAsync([a]));
        Assert.Null(Sup.PidOf("a"));
        Assert.IsType<ServiceStatus.Stopped>(Sup.StatusOf("a"));
    }

    [Fact]
    public async Task StopAllDuringStartStopsIt()
    {
        if (!OperatingSystem.IsWindows()) return;
        var slow = Fake("slow", ["sleep:1000", "say:listening on", "wait"]);
        var next = Fake("next", ["say:listening on", "wait"]);
        var starting = Sup.StartAsync([slow, next]);
        await Rec.Wait("slow", l => l.Count == 1);
        await Task.Delay(100);
        var pid = Sup.PidOf("slow")!.Value;
        await Sup.StopAllAsync();
        await Assert.ThrowsAsync<SupervisorStoppedException>(() => starting);
        Assert.True(await Processes.Gone(pid));
        Assert.True(await Rec.Wait("slow", l => l.Count > 0 && l[^1] is ServiceStatus.Stopped));
        Assert.Empty(Rec.Statuses("next"));
    }

    /// <summary>
    /// The shell crashes: a process (the fake, as a supervisor) starts two services, one with
    /// a child, and is killed outright. Windows closes its job handle, and the job ends them all.
    /// </summary>
    [Fact]
    public async Task KillingTheShellEndsEveryServiceAndTheirChildren()
    {
        if (!OperatingSystem.IsWindows()) return;
        var work = Path.Join(dir, "shell");
        var start = new ProcessStartInfo(FakeService.Exe) { UseShellExecute = false, CreateNoWindow = true };
        start.ArgumentList.Add("fake");
        start.ArgumentList.Add($"supervise:{work}");
        using var shell = Process.Start(start)!;
        var pidsFile = Path.Join(work, "pids.txt");
        var deadline = DateTime.UtcNow.AddSeconds(30);
        while (!File.Exists(pidsFile) && DateTime.UtcNow < deadline && !shell.HasExited) await Task.Delay(50);
        Assert.True(File.Exists(pidsFile), $"the fake shell did not start its services (exited: {shell.HasExited})");
        var pids = File.ReadAllLines(pidsFile).Select(int.Parse).ToList();
        Assert.Equal(3, pids.Count);
        Assert.All(pids, pid => Assert.True(Processes.Exists(pid), $"pid {pid} is not running"));

        shell.Kill(entireProcessTree: false);  // TerminateProcess of the shell alone
        await shell.WaitForExitAsync();
        foreach (var pid in pids) Assert.True(await Processes.Gone(pid), $"pid {pid} outlived the shell");
    }

    [Fact]
    public void ExitCodesReadAsTheMacs()
    {
        Assert.Equal("exited with status 3", Supervisor.Describe(3));
        Assert.Equal("exited with status 0xC0000005", Supervisor.Describe(unchecked((int)0xC0000005)));
    }

    [Fact]
    public void TailIsTheLastTwentyLines()
    {
        Assert.Equal(" It wrote nothing.", Supervisor.Tail(""));
        Assert.Equal(" Last log lines:\na\nb", Supervisor.Tail("a\r\nb\r\n"));
        var many = string.Join("\n", Enumerable.Range(1, 30).Select(i => $"line {i}"));
        var tail = Supervisor.Tail(many);
        Assert.StartsWith(" Last log lines:\nline 11\n", tail);
        Assert.EndsWith("line 30", tail);
    }

    [Fact]
    public void LogScannerFindsALineSplitAcrossReads()
    {
        var file = Path.Join(dir, "scan.log");
        File.WriteAllText(file, "old listening on\n");
        var scanner = new LogScanner(file, new FileInfo(file).Length);
        Assert.False(scanner.Contains("listening on"));
        File.AppendAllText(file, "liste");
        Assert.False(scanner.Contains("listening on"));
        File.AppendAllText(file, "ning on 1\n");
        Assert.True(scanner.Contains("listening on"));
    }
}
