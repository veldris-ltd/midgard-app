using System.Diagnostics;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of InstanceLockTests in apps/mac/Tests/MidgardShellCoreTests/ShellTests.swift,
// plus a lock left by a killed process. Windows only (share modes are Windows').
public sealed class InstanceLockTests : IDisposable
{
    readonly string dir = Path.Join(Path.GetTempPath(), $"lock-{Guid.NewGuid():N}");
    string LockFile => Path.Join(dir, "Midgard.instance");

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

    /// <summary>Reads the file as another process can while it is held.</summary>
    static string ReadShared(string file)
    {
        using var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
        using var reader = new StreamReader(stream);
        return reader.ReadToEnd();
    }

    static string ContentOrEmpty(string file)
    {
        try
        {
            return ReadShared(file);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return "";
        }
    }

    [Fact]
    public void SecondAcquireSeesFirstPidUntilReleased()
    {
        if (!OperatingSystem.IsWindows()) return;
        var first = Assert.IsType<InstanceLock.Outcome.Acquired>(InstanceLock.Acquire(LockFile)).Lock;
        var held = Assert.IsType<InstanceLock.Outcome.HeldBy>(InstanceLock.Acquire(LockFile));
        Assert.Equal(Environment.ProcessId, held.Pid);
        Assert.False(held.Stopping);

        first.MarkStopping();
        var stopping = Assert.IsType<InstanceLock.Outcome.HeldBy>(InstanceLock.Acquire(LockFile));
        Assert.Equal(Environment.ProcessId, stopping.Pid);
        Assert.True(stopping.Stopping);

        using var release = new Timer(_ => first.Release(), null, 300, Timeout.Infinite);
        var waited = Assert.IsType<InstanceLock.Outcome.Acquired>(InstanceLock.Acquire(LockFile, TimeSpan.FromSeconds(5))).Lock;
        waited.Release();
        var again = Assert.IsType<InstanceLock.Outcome.Acquired>(InstanceLock.Acquire(LockFile)).Lock;
        again.Release();
    }

    [Fact]
    public void RewriteReplacesLongerContentWithoutLeftovers()
    {
        if (!OperatingSystem.IsWindows()) return;
        Directory.CreateDirectory(dir);
        File.WriteAllText(LockFile, "1234567890 stopping and some leftover text\n");
        using var held = Assert.IsType<InstanceLock.Outcome.Acquired>(InstanceLock.Acquire(LockFile)).Lock;
        Assert.Equal($"{Environment.ProcessId}\n", ReadShared(LockFile));
        held.MarkStopping();
        Assert.Equal($"{Environment.ProcessId} stopping\n", ReadShared(LockFile));
    }

    [Fact]
    public void OnlyTheFirstLineIsRead()
    {
        if (!OperatingSystem.IsWindows()) return;
        using var held = Assert.IsType<InstanceLock.Outcome.Acquired>(InstanceLock.Acquire(LockFile)).Lock;
        // As a reader could see it between Rewrite's write and its cut.
        held.WriteWithoutCutting($"{Environment.ProcessId}\n99 stopping\n");
        var seen = Assert.IsType<InstanceLock.Outcome.HeldBy>(InstanceLock.Acquire(LockFile));
        Assert.Equal(Environment.ProcessId, seen.Pid);
        Assert.False(seen.Stopping);
    }

    [Fact]
    public void AStaleLockFileIsReclaimed()
    {
        if (!OperatingSystem.IsWindows()) return;
        // Left by an app that ended without releasing it: nothing holds it open.
        Directory.CreateDirectory(dir);
        File.WriteAllText(LockFile, "4242 stopping\n");
        using var held = Assert.IsType<InstanceLock.Outcome.Acquired>(InstanceLock.Acquire(LockFile)).Lock;
        Assert.Equal($"{Environment.ProcessId}\n", ReadShared(LockFile));
    }

    [Fact]
    public void ALockHeldByAKilledProcessIsReclaimed()
    {
        if (!OperatingSystem.IsWindows()) return;
        Directory.CreateDirectory(dir);
        // Another process takes the lock as InstanceLock does (read-write, others may only read).
        var script = "$f = [System.IO.File]::Open('" + LockFile + "', 'OpenOrCreate', 'ReadWrite', 'Read'); "
                   + "$b = [System.Text.Encoding]::ASCII.GetBytes(\"$PID`n\"); $f.Write($b, 0, $b.Length); $f.Flush(); "
                   + "Start-Sleep -Seconds 120";
        var info = new ProcessStartInfo("powershell.exe") { UseShellExecute = false, CreateNoWindow = true };
        foreach (var arg in new[] { "-NoProfile", "-NonInteractive", "-Command", script }) info.ArgumentList.Add(arg);
        using var holder = Process.Start(info)!;
        try
        {
            // Wait for it to write its pid, reading as another process may (taking the lock
            // meanwhile could make its own open fail).
            var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(60);
            while (DateTime.UtcNow < deadline && !holder.HasExited && ContentOrEmpty(LockFile).Trim() != holder.Id.ToString())
                Thread.Sleep(100);
            var seen = InstanceLock.Acquire(LockFile) as InstanceLock.Outcome.HeldBy;
            Assert.NotNull(seen);
            Assert.Equal(holder.Id, seen.Pid);
            Assert.False(seen.Stopping);
        }
        finally
        {
            holder.Kill();
            holder.WaitForExit();
        }

        using var held = Assert.IsType<InstanceLock.Outcome.Acquired>(InstanceLock.Acquire(LockFile, TimeSpan.FromSeconds(5))).Lock;
        Assert.Equal($"{Environment.ProcessId}\n", ReadShared(LockFile));
    }

    [Fact]
    public void AnUnopenableFileFails()
    {
        if (!OperatingSystem.IsWindows()) return;
        // A folder where the lock file should be.
        Directory.CreateDirectory(LockFile);
        Assert.IsType<InstanceLock.Outcome.Failed>(InstanceLock.Acquire(LockFile));
    }
}
