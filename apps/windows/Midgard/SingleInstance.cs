using System.Diagnostics;
using System.Runtime.InteropServices;
using Midgard.Core;

namespace Midgard;

/// <summary>
/// One app per working copy, as the Mac app's <c>claimSingleInstance</c>: a second launch
/// brings the running app's window forward and exits; an app that is quitting is waited for.
/// </summary>
static partial class SingleInstance
{
    /// <summary>
    /// True if this is the only app for <paramref name="home"/> (with the lock it holds in
    /// <paramref name="held"/>, or null when the lock could not be taken: better to run
    /// than to refuse), else false after bringing the other one forward.
    /// </summary>
    /// <remarks>
    /// The lock counts as another app's only if the pid it names is a running Midgard.exe.
    /// Something else can hold the file for a moment (an antivirus or backup scan), and a
    /// holder that has just quit can leave its pid behind; those are retried for a couple
    /// of seconds, and after that this app runs without the lock (held is null, so the
    /// orphan sweep is skipped: <see cref="Orphans.SweepAsync"/>).
    /// </remarks>
    public static bool Claim(string home, out InstanceLock? held)
    {
        held = null;
        var file = Home.InstanceLockFile(home);
        var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(2);
        while (true)
        {
            switch (InstanceLock.Acquire(file))
            {
                case InstanceLock.Outcome.Acquired a:
                    held = a.Lock;
                    return true;
                case InstanceLock.Outcome.HeldBy { Pid: { } pid } h when IsMidgard(pid):
                    if (h.Stopping && InstanceLock.Acquire(file, TimeSpan.FromSeconds(30)) is InstanceLock.Outcome.Acquired later)
                    {
                        held = later.Lock;
                        return true;
                    }
                    Focus(pid);
                    return false;
                case InstanceLock.Outcome.HeldBy:
                    // Held, but not by a running Midgard: a scan, or a holder on its way out.
                    if (DateTime.UtcNow >= deadline) return true;
                    Thread.Sleep(100);
                    break;
                default:
                    return true;
            }
        }
    }

    /// <summary>Whether process <paramref name="pid"/> runs an exe named as this one (Midgard.exe).</summary>
    static bool IsMidgard(int pid)
    {
        if (pid == Environment.ProcessId) return false;
        var image = WindowsProcesses.ImagePath(pid);
        var mine = Path.GetFileName(Environment.ProcessPath);
        return image is not null && mine is not null
               && string.Equals(Path.GetFileName(image), mine, StringComparison.OrdinalIgnoreCase);
    }

    /// <summary>
    /// Brings process <paramref name="pid"/>'s main window forward. This process was just
    /// launched by the presenter, so Windows lets it hand over the foreground.
    /// </summary>
    static void Focus(int pid)
    {
        try
        {
            using var process = Process.GetProcessById(pid);
            var window = process.MainWindowHandle;
            if (window == 0) return;
            if (IsIconic(window)) ShowWindow(window, SwRestore);
            SetForegroundWindow(window);
        }
        catch (Exception e) when (e is ArgumentException or InvalidOperationException or System.ComponentModel.Win32Exception)
        {
            // It has just quit.
        }
    }

    const int SwRestore = 9;

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool SetForegroundWindow(nint window);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool ShowWindow(nint window, int command);

    [LibraryImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool IsIconic(nint window);
}
