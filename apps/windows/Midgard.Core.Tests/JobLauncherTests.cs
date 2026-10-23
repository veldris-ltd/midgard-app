using System.Runtime.Versioning;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The Job Object launcher on its own: handles used while another thread closes them, and
// what it refuses to start. Windows only.
[SupportedOSPlatform("windows")]
public sealed class JobLauncherTests : IDisposable
{
    readonly string dir = Path.Join(Path.GetTempPath(), $"midgard-job-{Guid.NewGuid():N}");

    public JobLauncherTests() => Directory.CreateDirectory(dir);

    public void Dispose()
    {
        try
        {
            Directory.Delete(dir, recursive: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    IServiceProcess Start(params string[] steps) =>
        new JobLauncher().Start(FakeService.Exe, ["fake", .. steps], null, Path.Join(dir, $"{Guid.NewGuid():N}.log"));

    /// <summary>Kill, ActiveProcesses and Dispose from several threads at once: no exception, nothing left.</summary>
    [Fact]
    public async Task KillAndDisposeAtOnceAreSafe()
    {
        if (!OperatingSystem.IsWindows()) return;
        for (var round = 0; round < 20; round++)
        {
            var process = Start("wait");
            var pid = process.Pid;
            using var go = new ManualResetEventSlim();
            var workers = Enumerable.Range(0, 6).Select(i => Task.Run(() =>
            {
                go.Wait();
                for (var n = 0; n < 50; n++)
                {
                    switch ((i + n) % 3)
                    {
                        case 0: process.Kill(); break;
                        case 1: _ = process.ActiveProcesses; break;
                        default: process.Dispose(); break;
                    }
                }
            })).ToList();
            go.Set();
            await Task.WhenAll(workers);
            process.Dispose();
            Assert.Equal(0, process.ActiveProcesses);
            Assert.True(process.Exited.IsCompleted);
            Assert.True(await Processes.Gone(pid), $"pid {pid} left running");
        }
    }

    /// <summary>Disposing right as the process exits on its own: the exit wait never sees a closed handle.</summary>
    [Fact]
    public async Task DisposeWhileItExits()
    {
        if (!OperatingSystem.IsWindows()) return;
        for (var round = 0; round < 20; round++)
        {
            var process = Start("exit:0");
            await Task.Delay(round % 5 * 10);
            process.Dispose();
            await process.Exited.WaitAsync(TimeSpan.FromSeconds(10));
        }
    }

    [Fact]
    public void OnlyExeFilesAreStarted()
    {
        if (!OperatingSystem.IsWindows()) return;
        foreach (var name in new[] { "x.bat", "x.cmd", "x.ps1", "x" })
        {
            var file = Path.Join(dir, name);
            File.WriteAllText(file, "@echo off\r\n");
            var e = Assert.Throws<IOException>(() => new JobLauncher().Start(file, [], null, Path.Join(dir, "x.log")));
            Assert.Contains("only .exe programs", e.Message);
        }
    }

    /// <summary>StopAll while each service's process is also being disposed elsewhere (a restart's clean-up).</summary>
    [Fact]
    public async Task StopAllWhileTheProcessesAreDisposed()
    {
        if (!OperatingSystem.IsWindows()) return;
        var launcher = new Recording(new JobLauncher());
        await using var supervisor = new Supervisor(new Supervisor.Timing { StopGrace = TimeSpan.FromSeconds(1) }, launcher);
        var specs = Enumerable.Range(0, 4).Select(i => new ServiceSpec($"s{i}", FakeService.Exe,
            ["fake", "say:listening on", "wait"], new Readiness.LogLine("listening on"), Path.Join(dir, $"s{i}.log"))).ToList();
        await supervisor.StartAsync(specs);
        var pids = launcher.Started.Select(p => p.Pid).ToList();
        var disposing = Task.Run(() => Parallel.ForEach(launcher.Started, p => p.Dispose()));
        await Task.WhenAll(supervisor.StopAllAsync(), disposing);
        foreach (var pid in pids) Assert.True(await Processes.Gone(pid), $"pid {pid} left running");
        Assert.All(specs, s => Assert.IsType<ServiceStatus.Stopped>(supervisor.StatusOf(s.Name)));
    }

    sealed class Recording(IProcessLauncher inner) : IProcessLauncher
    {
        public List<IServiceProcess> Started { get; } = [];

        public IServiceProcess Start(string executable, IReadOnlyList<string> arguments,
                                     IReadOnlyDictionary<string, string>? environment, string logFile)
        {
            var process = inner.Start(executable, arguments, environment, logFile);
            lock (Started) Started.Add(process);
            return process;
        }
    }
}
