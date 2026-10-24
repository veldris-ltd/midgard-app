using System.Diagnostics;
using System.Runtime.Versioning;

namespace Midgard.Core;

/// <summary>A process the sweep ended.</summary>
public sealed record ReapedProcess(int Pid, string Executable);

/// <summary>A leftover the sweep found but could not end, and why (for shell.log).</summary>
public sealed record UnreapedProcess(int Pid, string Executable, string Reason);

/// <summary>What a sweep did: what it ended, what it could not, or why it did not look at all.</summary>
public sealed record SweepOutcome(IReadOnlyList<ReapedProcess> Reaped, IReadOnlyList<UnreapedProcess> NotEnded,
                                  string? Skipped = null);

/// <summary>
/// Services a previous shell left running. The port of <c>Supervisor.reapOrphans</c> in
/// apps/mac/Sources/MidgardShellCore/Supervisor.swift. On Windows the app's kill-on-close
/// Job Object normally leaves nothing behind, even after a crash; this is the sweep for
/// what it cannot cover (a shell killed before it made the job, a service started by hand
/// from the helpers folder), and the last check when quitting.
/// </summary>
/// <remarks>
/// <para>Only processes in this Windows session running as this user are candidates: another
/// account's Midgard on the same PC (fast user switching, Remote Desktop) is never touched.</para>
/// <para>One install can serve two working copies (MIDGARD_APP_HOME), and both run their
/// services from the same helpers folder, so a sweep by path from the second would end the
/// first's services. <see cref="SweepAsync"/> is the call for the shell: it sweeps only
/// when this shell holds its working copy's instance lock and no other Midgard.exe from the
/// same install runs in this session. <see cref="ReapAsync"/> is the sweep itself.</para>
/// </remarks>
[SupportedOSPlatform("windows")]
public static class Orphans
{
    /// <summary>
    /// The shell's sweep: <see cref="ReapAsync"/>, unless it could end another shell's
    /// services. Skipped (with the reason in <see cref="SweepOutcome.Skipped"/>) when
    /// <paramref name="holdsInstanceLock"/> is false (the single-instance check could not take
    /// the lock, so another shell may be running this working copy), or when another process
    /// runs <paramref name="shell"/> (default: Midgard.exe beside <paramref name="helpers"/>)
    /// in this session as this user.
    /// </summary>
    public static async Task<SweepOutcome> SweepAsync(string helpers, bool holdsInstanceLock,
                                                      IEnumerable<string>? alsoUnder = null, TimeSpan? grace = null,
                                                      string? shell = null)
    {
        if (!holdsInstanceLock)
            return new SweepOutcome([], [], "this Midgard does not hold its working copy's instance lock");
        shell ??= Path.Join(Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(Path.GetFullPath(helpers))),
                            "Midgard.exe");
        var me = Environment.ProcessId;
        foreach (var process in Process.GetProcesses())
        {
            using (process)
            {
                if (process.Id != me && process.Id > 4 && WindowsProcesses.Runs(process.Id, shell))
                    return new SweepOutcome([], [], $"another Midgard (pid {process.Id}) runs from {Path.GetDirectoryName(shell)}");
            }
        }
        return await ReapAsync(helpers, alsoUnder, grace).ConfigureAwait(false);
    }

    /// <summary>
    /// Ends every process in this session, running as this user, whose executable is inside
    /// <paramref name="helpers"/> or one of <paramref name="alsoUnder"/> (the camera
    /// analytics' python.exe is in helpers\analytics\, so the helpers cover it): replay first, so it starts no more heimdall-source
    /// runs, which are in helpers\ too. Each is ended at once (TerminateProcess; no service has
    /// a gentler stop on Windows), then waited for up to <paramref name="grace"/> in all. A
    /// process is reported reaped only if TerminateProcess succeeded and it exited; one that
    /// could not be opened or ended, or did not exit, is in <see cref="SweepOutcome.NotEnded"/>.
    /// Unlike the Mac, a llama-server outside the helpers is not matched by its model
    /// argument: Windows does not show another process's command line without reading its
    /// memory, and the bundled one is under helpers\llama\.
    /// </summary>
    /// <remarks>
    /// Run it only after the single-instance check (see <see cref="SweepAsync"/>): a second
    /// shell would otherwise end the services of the first. Also run after closing the
    /// supervisor when quitting, as a last sweep (it should find nothing).
    /// </remarks>
    public static async Task<SweepOutcome> ReapAsync(string helpers, IEnumerable<string>? alsoUnder = null,
                                                     TimeSpan? grace = null)
    {
        var roots = Roots(helpers, alsoUnder);
        var me = Environment.ProcessId;
        var found = new List<(ReapedProcess Process, Win32Handle Handle)>();
        var notEnded = new List<UnreapedProcess>();
        foreach (var process in Process.GetProcesses())
        {
            using (process)
            {
                if (process.Id == me || process.Id <= 4) continue;
                if (Match(process.Id, roots) is not { } path) continue;
                var handle = Kernel32.OpenProcess(Kernel32.PROCESS_TERMINATE | Kernel32.PROCESS_QUERY_LIMITED_INFORMATION
                                                  | Kernel32.SYNCHRONIZE, false, (uint)process.Id);
                if (handle.IsInvalid)
                {
                    notEnded.Add(new UnreapedProcess(process.Id, path, $"cannot open it: {Kernel32.LastError("OpenProcess").Message}"));
                    handle.Dispose();
                    continue;
                }
                found.Add((new ReapedProcess(process.Id, path), handle));
            }
        }
        var ordered = found.Where(f => IsReplay(f.Process.Executable))
            .Concat(found.Where(f => !IsReplay(f.Process.Executable))).ToList();
        var reaped = new List<ReapedProcess>();
        try
        {
            var ended = new List<(ReapedProcess Process, Win32Handle Handle)>();
            foreach (var entry in ordered)
            {
                if (Kernel32.TerminateProcess(entry.Handle.DangerousGetHandle(), 1)) ended.Add(entry);
                else if (Running(entry.Handle))
                    notEnded.Add(new UnreapedProcess(entry.Process.Pid, entry.Process.Executable,
                                                     $"cannot end it: {Kernel32.LastError("TerminateProcess").Message}"));
            }
            // The handles are kept open, so a pid cannot be reused meanwhile.
            var deadline = DateTime.UtcNow + (grace ?? TimeSpan.FromSeconds(10));
            await Task.Run(() =>
            {
                foreach (var (process, handle) in ended)
                {
                    var left = Math.Max(0, (deadline - DateTime.UtcNow).TotalMilliseconds);
                    if (Kernel32.WaitForSingleObject(handle.DangerousGetHandle(), (uint)left) == Kernel32.WAIT_OBJECT_0)
                        reaped.Add(process);
                    else
                        notEnded.Add(new UnreapedProcess(process.Pid, process.Executable, "it did not exit after being ended"));
                }
            }).ConfigureAwait(false);
        }
        finally
        {
            foreach (var (_, handle) in ordered) handle.Dispose();
        }
        return new SweepOutcome(reaped, notEnded);
    }

    /// <summary>
    /// What <see cref="ReapAsync"/> would end, ending nothing: every process in this session,
    /// running as this user, whose executable is inside <paramref name="helpers"/> or one of
    /// <paramref name="alsoUnder"/> (the headless check's last look after quitting).
    /// </summary>
    public static IReadOnlyList<ReapedProcess> Running(string helpers, IEnumerable<string>? alsoUnder = null)
    {
        var roots = Roots(helpers, alsoUnder);
        var me = Environment.ProcessId;
        var found = new List<ReapedProcess>();
        foreach (var process in Process.GetProcesses())
        {
            using (process)
            {
                if (process.Id != me && process.Id > 4 && Match(process.Id, roots) is { } path)
                    found.Add(new ReapedProcess(process.Id, path));
            }
        }
        return found;
    }

    /// <summary>Every spelling of the folders a sweep looks in (see <see cref="Forms"/>).</summary>
    static List<string> Roots(string helpers, IEnumerable<string>? alsoUnder) =>
        new[] { helpers }.Concat(alsoUnder ?? Array.Empty<string>())
            .SelectMany(root => Forms(root, folder: true)).Distinct(StringComparer.OrdinalIgnoreCase).ToList();

    /// <summary>
    /// The executable of process <paramref name="pid"/> when it is in this session, runs as
    /// this user and is inside one of <paramref name="roots"/>; else null.
    /// </summary>
    static string? Match(int pid, List<string> roots)
    {
        if (WindowsProcesses.SessionOf(pid) != WindowsProcesses.CurrentSession) return null;
        using var query = Kernel32.OpenProcess(Kernel32.PROCESS_QUERY_LIMITED_INFORMATION, false, (uint)pid);
        // Not ours to ask (a protected or elevated process): not a service of ours either.
        if (query.IsInvalid) return null;
        var path = WindowsProcesses.ImagePath(query);
        if (path is null) return null;
        var spellings = Forms(path, folder: false).ToList();
        if (!roots.Any(root => spellings.Any(p => p.StartsWith(root, StringComparison.OrdinalIgnoreCase)))) return null;
        return WindowsProcesses.UserOf(query) == WindowsProcesses.CurrentUser ? path : null;
    }

    static bool IsReplay(string executable) =>
        string.Equals(Path.GetFileName(executable), "midgard-replay.exe", StringComparison.OrdinalIgnoreCase);

    static unsafe bool Running(Win32Handle handle)
    {
        uint code;
        return Kernel32.GetExitCodeProcess(handle.DangerousGetHandle(), &code) && code == Kernel32.STILL_ACTIVE;
    }

    /// <summary>Every spelling of <paramref name="path"/> the sweep compares (see <see cref="Forms"/>).</summary>
    internal static List<string> Spellings(string path, bool folder) => Forms(path, folder).ToList();

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

    static string? FinalPath(string path) => RealPath.Windows(path);
}
