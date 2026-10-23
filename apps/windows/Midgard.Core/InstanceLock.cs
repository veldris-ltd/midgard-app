using System.Runtime.Versioning;
using System.Text;

namespace Midgard.Core;

/// <summary>
/// One app per working copy: a lock file (<see cref="Home.InstanceLockFile"/>) held open for
/// the life of the process, so a second launch knows the first is running, which process it
/// is (to bring its window forward) and whether it is quitting. The port of
/// <c>InstanceLock</c> in apps/mac/Sources/MidgardShellCore/Shell.swift, with Windows'
/// share modes in place of <c>flock</c>.
/// </summary>
/// <remarks>
/// <para>The holder opens the file for reading and writing and lets others only read it
/// (<see cref="FileShare.Read"/>): a second open for writing fails with a sharing violation,
/// and a reader can still see the holder's pid. Windows closes the handle when the process
/// ends, however it ends, so a lock left by a crash is free again at once: the file stays,
/// with the dead process's pid, and the next holder overwrites it.</para>
/// <para>.NET opens files with handles that child processes do not inherit, so the services
/// never hold the lock. The file's content is the holder's pid, then " stopping" once it is
/// quitting; only the first line counts.</para>
/// <para>Windows only: on Unix .NET maps share modes onto advisory <c>flock</c>, and a
/// <see cref="FileShare.Read"/> holder takes a shared lock that a second one also gets.</para>
/// </remarks>
[SupportedOSPlatform("windows")]
public sealed class InstanceLock : IDisposable
{
    /// <summary>What <see cref="Acquire(string)"/> found.</summary>
    public abstract record Outcome
    {
        Outcome() { }

        public sealed record Acquired(InstanceLock Lock) : Outcome;
        /// <summary>Another process holds it: its pid if it wrote one, and whether it is quitting.</summary>
        public sealed record HeldBy(int? Pid, bool Stopping) : Outcome;
        public sealed record Failed(string Reason) : Outcome;
    }

    public string File { get; }
    FileStream? stream;

    InstanceLock(string file, FileStream stream)
    {
        File = file;
        this.stream = stream;
    }

    /// <summary>Takes the lock at <paramref name="file"/> (creating it and its folder), writing this pid into it.</summary>
    public static Outcome Acquire(string file)
    {
        try
        {
            if (Path.GetDirectoryName(Path.GetFullPath(file)) is { } dir) Directory.CreateDirectory(dir);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            // The open below says what is wrong.
        }
        FileStream stream;
        try
        {
            stream = new FileStream(file, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.Read);
        }
        catch (IOException e) when (IsSharingViolation(e))
        {
            return ReadHolder(file);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return new Outcome.Failed($"cannot open {file}: {e.Message}");
        }
        var held = new InstanceLock(file, stream);
        held.Rewrite($"{Environment.ProcessId}\n");
        return new Outcome.Acquired(held);
    }

    /// <summary>Waits up to <paramref name="timeout"/> for the lock at <paramref name="file"/> to come free, then takes it.</summary>
    public static Outcome Acquire(string file, TimeSpan timeout)
    {
        var deadline = DateTime.UtcNow + timeout;
        while (true)
        {
            var outcome = Acquire(file);
            if (outcome is not Outcome.HeldBy || DateTime.UtcNow >= deadline) return outcome;
            Thread.Sleep(100);
        }
    }

    /// <summary>
    /// Tells an app launched meanwhile that this one is quitting (it waits for the lock
    /// instead of handing over to this one).
    /// </summary>
    public void MarkStopping() => Rewrite($"{Environment.ProcessId} stopping\n");

    public void Release()
    {
        stream?.Dispose();
        stream = null;
    }

    public void Dispose() => Release();

    /// <summary>
    /// Replaces the file's content: writes the new text first, then cuts the file to its
    /// length, so a reader never sees it empty. A write that fails (a full disk) leaves the
    /// file uncut: a mix of old and new bytes, which readers treat as an unknown holder. The
    /// open handle, not the content, is what holds the lock.
    /// </summary>
    void Rewrite(string text)
    {
        if (stream is null) return;
        var bytes = Encoding.UTF8.GetBytes(text);
        try
        {
            stream.Position = 0;
            stream.Write(bytes);
            stream.Flush();
            stream.SetLength(bytes.Length);
            stream.Flush();
        }
        catch (IOException)
        {
        }
    }

    /// <summary>For tests: what a reader could see between <see cref="Rewrite"/>'s write and its cut.</summary>
    internal void WriteWithoutCutting(string text)
    {
        if (stream is null) return;
        stream.Position = 0;
        stream.Write(Encoding.UTF8.GetBytes(text));
        stream.Flush();
    }

    static Outcome ReadHolder(string file)
    {
        var text = "";
        try
        {
            using var reader = new FileStream(file, FileMode.Open, FileAccess.Read,
                                              FileShare.ReadWrite | FileShare.Delete);
            var buffer = new byte[32];
            var count = reader.ReadAtLeast(buffer, buffer.Length, throwOnEndOfStream: false);
            text = Encoding.UTF8.GetString(buffer, 0, count);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            // Held, by whom unknown.
        }
        // Only the first line counts: a longer earlier content may follow it briefly.
        var firstLine = text.Split('\n')[0];
        var words = firstLine.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries);
        int? pid = words.Length > 0 && int.TryParse(words[0], out var p) ? p : null;
        return new Outcome.HeldBy(pid, words.Length > 1 && words[1] == "stopping");
    }

    /// <summary>ERROR_SHARING_VIOLATION or ERROR_LOCK_VIOLATION.</summary>
    static bool IsSharingViolation(IOException e) => (e.HResult & 0xFFFF) is 32 or 33;
}
