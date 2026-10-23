using System.Diagnostics;
using System.Runtime.Versioning;

namespace Midgard.Core;

/// <summary>A process <see cref="Orphans.ReapAsync"/> ended.</summary>
public sealed record ReapedProcess(int Pid, string Executable);

/// <summary>
/// Services a previous shell left running. The port of <c>Supervisor.reapOrphans</c> in
/// apps/mac/Sources/MidgardShellCore/Supervisor.swift. On Windows the app's kill-on-close
/// Job Object normally leaves nothing behind, even after a crash; this is the sweep for
/// what it cannot cover (a shell killed before it made the job, a service started by hand
/// from the helpers folder), and the last check when quitting.
/// </summary>
[SupportedOSPlatform("windows")]
public static class Orphans
{
    /// <summary>
    /// Ends every process whose executable is inside <paramref name="helpers"/> or one of
    /// <paramref name="alsoUnder"/> (the camera analytics' Python runtime in W3): replay
    /// first, so it starts no more heimdall-source runs, which are in helpers\ too. Each is
    /// ended at once (TerminateProcess; no service has a gentler stop on Windows), then
    /// waited for up to <paramref name="grace"/>. Returns the processes it ended, replay
    /// first. Unlike the Mac, a llama-server outside the helpers is not matched by its model
    /// argument: Windows does not show another process's command line without reading its
    /// memory, and the bundled one is under helpers\llama\.
    /// </summary>
    /// <remarks>
    /// Run it only after the single-instance check: a second shell would otherwise end the
    /// services of the first. Also run after closing the supervisor when quitting, as a
    /// last sweep (it should find nothing).
    /// </remarks>
    public static async Task<IReadOnlyList<ReapedProcess>> ReapAsync(string helpers, IEnumerable<string>? alsoUnder = null,
                                                                     TimeSpan? grace = null)
    {
        var roots = new[] { helpers }.Concat(alsoUnder ?? Array.Empty<string>())
            .SelectMany(root => Forms(root, folder: true)).Distinct(StringComparer.OrdinalIgnoreCase).ToList();
        var me = Environment.ProcessId;
        var found = new List<(ReapedProcess Process, Win32Handle Handle)>();
        foreach (var process in Process.GetProcesses())
        {
            using (process)
            {
                if (process.Id == me || process.Id <= 4) continue;
                var handle = Kernel32.OpenProcess(Kernel32.PROCESS_TERMINATE | Kernel32.PROCESS_QUERY_LIMITED_INFORMATION
                                                  | Kernel32.SYNCHRONIZE, false, (uint)process.Id);
                if (handle.IsInvalid)
                {
                    handle.Dispose();
                    continue;
                }
                var path = ImagePath(handle);
                var spellings = path is null ? new List<string>() : Forms(path, folder: false).ToList();
                if (path is not null
                    && roots.Any(root => spellings.Any(p => p.StartsWith(root, StringComparison.OrdinalIgnoreCase))))
                {
                    found.Add((new ReapedProcess(process.Id, path), handle));
                }
                else
                {
                    handle.Dispose();
                }
            }
        }
        var ordered = found.Where(f => IsReplay(f.Process.Executable))
            .Concat(found.Where(f => !IsReplay(f.Process.Executable))).ToList();
        try
        {
            foreach (var (_, handle) in ordered) Kernel32.TerminateProcess(handle.DangerousGetHandle(), 1);
            var deadline = DateTime.UtcNow + (grace ?? TimeSpan.FromSeconds(10));
            // The handles are kept open, so a pid cannot be reused meanwhile.
            while (ordered.Any(f => Running(f.Handle)) && DateTime.UtcNow < deadline)
            {
                await Task.Delay(50).ConfigureAwait(false);
            }
        }
        finally
        {
            foreach (var (_, handle) in ordered) handle.Dispose();
        }
        return ordered.Select(f => f.Process).ToList();
    }

    static bool IsReplay(string executable) =>
        string.Equals(Path.GetFileName(executable), "midgard-replay.exe", StringComparison.OrdinalIgnoreCase);

    static unsafe bool Running(Win32Handle handle)
    {
        uint code;
        return Kernel32.GetExitCodeProcess(handle.DangerousGetHandle(), &code) && code == Kernel32.STILL_ACTIVE;
    }

    /// <summary>The executable a process runs (QueryFullProcessImageNameW), null if it cannot be read.</summary>
    static unsafe string? ImagePath(Win32Handle process)
    {
        var buffer = new char[32768];
        fixed (char* name = buffer)
        {
            var size = (uint)buffer.Length;
            return Kernel32.QueryFullProcessImageName(process, 0, name, &size) ? new string(name, 0, (int)size) : null;
        }
    }

    /// <summary>
    /// The ways one path may be spelt, each ending in a separator when it is a folder: as
    /// given (made full), with 8.3 short names made long (the CI runner's temp folder is
    /// C:\Users\RUNNER~1\…), and with every junction and symbolic link resolved.
    /// </summary>
    static IEnumerable<string> Forms(string path, bool folder)
    {
        var full = Path.GetFullPath(path);
        var forms = new List<string> { full };
        if (LongPath(full) is { } longer) forms.Add(longer);
        if (FinalPath(full) is { } final) forms.Add(final);
        return folder ? forms.Select(f => Path.TrimEndingDirectorySeparator(f) + Path.DirectorySeparatorChar) : forms;
    }

    static unsafe string? LongPath(string path)
    {
        var buffer = new char[32768];
        fixed (char* result = buffer)
        {
            var length = Kernel32.GetLongPathName(path, result, (uint)buffer.Length);
            return length == 0 || length >= buffer.Length ? null : new string(result, 0, (int)length);
        }
    }

    static unsafe string? FinalPath(string path)
    {
        using var handle = Kernel32.CreateFile(path, 0, Kernel32.FILE_SHARE_READ | Kernel32.FILE_SHARE_WRITE | Kernel32.FILE_SHARE_DELETE,
                                               null, Kernel32.OPEN_EXISTING, Kernel32.FILE_FLAG_BACKUP_SEMANTICS, 0);
        if (handle.IsInvalid) return null;
        var buffer = new char[32768];
        fixed (char* result = buffer)
        {
            var length = Kernel32.GetFinalPathNameByHandle(handle, result, (uint)buffer.Length, 0);
            if (length == 0 || length >= buffer.Length) return null;
            var final = new string(result, 0, (int)length);
            // \\?\C:\… → C:\…, \\?\UNC\server\… → \\server\…
            if (final.StartsWith(@"\\?\UNC\", StringComparison.Ordinal)) return @"\\" + final[8..];
            return final.StartsWith(@"\\?\", StringComparison.Ordinal) ? final[4..] : final;
        }
    }
}
