using System.Diagnostics;
using System.Runtime.Versioning;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of apps/mac/Tests/MidgardShellCoreTests/ReapOrphansTests.swift: services a
// crashed shell left running, started here by plain Process.Start (outside any job of
// ours) from copies of the fake service in a temp helpers folder. Windows only.
[SupportedOSPlatform("windows")]
public sealed class OrphansTests : IDisposable
{
    readonly string dir = Path.Join(Path.GetTempPath(), $"midgard-reap-{Guid.NewGuid():N}");
    readonly List<Process> processes = [];

    string Helpers => Path.Join(dir, "helpers");

    public void Dispose()
    {
        foreach (var process in processes)
        {
            try
            {
                if (!process.HasExited) process.Kill(entireProcessTree: true);
                process.WaitForExit(5000);
            }
            catch (Exception e) when (e is InvalidOperationException or System.ComponentModel.Win32Exception)
            {
            }
            process.Dispose();
        }
        try
        {
            Directory.Delete(dir, recursive: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    /// <summary>
    /// The fake service under <paramref name="name"/> in <paramref name="folder"/>: its
    /// apphost renamed, beside the two assemblies and the runtimeconfig it loads (the apphost
    /// names its assembly itself, so the exe's name does not matter).
    /// </summary>
    static string Install(string folder, string name)
    {
        Directory.CreateDirectory(folder);
        var source = Path.GetDirectoryName(FakeService.Exe)!;
        foreach (var file in new[] { "Midgard.Core.Tests.dll", "Midgard.Core.Tests.runtimeconfig.json", "Midgard.Core.dll" })
        {
            var target = Path.Join(folder, file);
            if (!File.Exists(target)) File.Copy(Path.Join(source, file), target);
        }
        var exe = Path.Join(folder, name);
        File.Copy(FakeService.Exe, exe);
        return exe;
    }

    Process Launch(string exe, params string[] steps)
    {
        var start = new ProcessStartInfo(exe) { UseShellExecute = false, CreateNoWindow = true };
        start.ArgumentList.Add("fake");
        foreach (var step in steps) start.ArgumentList.Add(step);
        var process = Process.Start(start)!;
        processes.Add(process);
        return process;
    }

    static async Task WaitFor(string file)
    {
        var deadline = DateTime.UtcNow.AddSeconds(20);
        while (!File.Exists(file) && DateTime.UtcNow < deadline) await Task.Delay(50);
        Assert.True(File.Exists(file), $"{file} was not written");
    }

    [Fact]
    public async Task ReapsOnlyProcessesFromTheHelpersFolderReplayFirst()
    {
        if (!OperatingSystem.IsWindows()) return;
        var worker = Install(Helpers, "forge-worker.exe");
        var replay = Install(Helpers, "midgard-replay.exe");
        var outsiderExe = Install(Path.Join(dir, "elsewhere"), "forge-worker.exe");
        var childFile = Path.Join(dir, "child.pid");

        var w = Launch(worker, "wait");
        // Replay's heimdall-source runs are in helpers\ too: its child runs the same exe.
        var r = Launch(replay, $"child:{childFile}", "wait");
        var outsider = Launch(outsiderExe, "wait");
        var outsiderFromBin = Launch(FakeService.Exe, "wait");
        await WaitFor(childFile);
        var child = int.Parse(File.ReadAllText(childFile));

        var outcome = await Orphans.ReapAsync(Helpers, grace: TimeSpan.FromSeconds(5));
        var reaped = outcome.Reaped;

        Assert.Null(outcome.Skipped);
        Assert.Empty(outcome.NotEnded);
        Assert.Equal("midgard-replay.exe", Path.GetFileName(reaped[0].Executable));
        Assert.Equal(r.Id, reaped[0].Pid);
        Assert.Equal(new[] { r.Id, child, w.Id }.Order(), reaped.Select(p => p.Pid).Order());
        foreach (var pid in new[] { w.Id, r.Id, child }) Assert.True(await Processes.Gone(pid), $"pid {pid} left running");
        Assert.False(outsider.HasExited, "a process outside the helpers was ended");
        Assert.False(outsiderFromBin.HasExited, "a process outside the helpers was ended");
        Assert.DoesNotContain(reaped, p => p.Pid == Environment.ProcessId);
    }

    [Fact]
    public async Task ReapsUnderTheOtherFoldersToo()
    {
        if (!OperatingSystem.IsWindows()) return;
        var runtime = Path.Join(dir, "resources", "analytics", "runtime");
        var python = Launch(Install(runtime, "python.exe"), "wait");
        Directory.CreateDirectory(Helpers);
        var outcome = await Orphans.ReapAsync(Helpers, [runtime], TimeSpan.FromSeconds(5));
        Assert.Equal(python.Id, Assert.Single(outcome.Reaped).Pid);
        Assert.True(await Processes.Gone(python.Id));
    }

    /// <summary>The folder spelt another way (8.3 names, a trailing separator, other case) still matches.</summary>
    [Fact]
    public async Task MatchesTheFolderHoweverItIsSpelt()
    {
        if (!OperatingSystem.IsWindows()) return;
        var worker = Launch(Install(Helpers, "forge-worker.exe"), "wait");
        await Task.Delay(200);
        var outcome = await Orphans.ReapAsync(Helpers.ToUpperInvariant() + "\\", grace: TimeSpan.FromSeconds(5));
        Assert.Equal(worker.Id, Assert.Single(outcome.Reaped).Pid);
    }

    [Fact]
    public async Task FindsNothingWhenNothingIsLeft()
    {
        if (!OperatingSystem.IsWindows()) return;
        Directory.CreateDirectory(Helpers);
        var outcome = await Orphans.ReapAsync(Helpers);
        Assert.Empty(outcome.Reaped);
        Assert.Empty(outcome.NotEnded);
    }

    /// <summary>Without the instance lock another shell may run this working copy: no sweep.</summary>
    [Fact]
    public async Task SweepIsSkippedWithoutTheInstanceLock()
    {
        if (!OperatingSystem.IsWindows()) return;
        var worker = Launch(Install(Helpers, "forge-worker.exe"), "wait");
        await Task.Delay(200);
        var outcome = await Orphans.SweepAsync(Helpers, holdsInstanceLock: false, grace: TimeSpan.FromSeconds(5));
        Assert.Contains("instance lock", outcome.Skipped);
        Assert.Empty(outcome.Reaped);
        Assert.False(worker.HasExited);
    }

    /// <summary>
    /// Another Midgard.exe from the same install (a second working copy, MIDGARD_APP_HOME)
    /// runs its services from the same helpers: the sweep would end them, so it is skipped.
    /// </summary>
    [Fact]
    public async Task SweepIsSkippedWhileAnotherShellFromTheSameInstallRuns()
    {
        if (!OperatingSystem.IsWindows()) return;
        var worker = Launch(Install(Helpers, "forge-worker.exe"), "wait");
        var shell = Launch(Install(dir, "Midgard.exe"), "wait");
        await Task.Delay(300);
        var outcome = await Orphans.SweepAsync(Helpers, holdsInstanceLock: true, grace: TimeSpan.FromSeconds(5));
        Assert.Contains($"pid {shell.Id}", outcome.Skipped);
        Assert.False(worker.HasExited);

        // Once it has gone, the sweep runs.
        shell.Kill();
        await shell.WaitForExitAsync();
        outcome = await Orphans.SweepAsync(Helpers, holdsInstanceLock: true, grace: TimeSpan.FromSeconds(5));
        Assert.Null(outcome.Skipped);
        Assert.Equal(worker.Id, Assert.Single(outcome.Reaped).Pid);
    }

    /// <summary>A Midgard.exe elsewhere (another install) does not stop this install's sweep.</summary>
    [Fact]
    public async Task AShellFromAnotherInstallDoesNotStopTheSweep()
    {
        if (!OperatingSystem.IsWindows()) return;
        var worker = Launch(Install(Helpers, "forge-worker.exe"), "wait");
        var other = Launch(Install(Path.Join(dir, "other-install"), "Midgard.exe"), "wait");
        await Task.Delay(300);
        var outcome = await Orphans.SweepAsync(Helpers, holdsInstanceLock: true, grace: TimeSpan.FromSeconds(5));
        Assert.Null(outcome.Skipped);
        Assert.Equal(worker.Id, Assert.Single(outcome.Reaped).Pid);
        Assert.False(other.HasExited);
    }

    [Fact]
    public void ProcessFactsOfThisProcess()
    {
        if (!OperatingSystem.IsWindows()) return;
        Assert.Equal(WindowsProcesses.CurrentSession, WindowsProcesses.SessionOf(Environment.ProcessId));
        Assert.True(WindowsProcesses.Runs(Environment.ProcessId, Environment.ProcessPath!));
        Assert.False(WindowsProcesses.Runs(Environment.ProcessId, Path.Join(dir, "other.exe")));
    }
}
