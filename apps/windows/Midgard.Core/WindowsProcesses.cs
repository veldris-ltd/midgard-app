using System.Runtime.Versioning;
using System.Security.Principal;

namespace Midgard.Core;

/// <summary>Facts about another process: its executable, its session and the user it runs as. Windows only.</summary>
[SupportedOSPlatform("windows")]
public static unsafe class WindowsProcesses
{
    /// <summary>The Windows session this process runs in (each signed-in user's desktop has its own).</summary>
    public static uint CurrentSession { get; } = SessionOf(Environment.ProcessId) ?? uint.MaxValue;

    /// <summary>The account this process runs as.</summary>
    public static SecurityIdentifier CurrentUser { get; } = WindowsIdentity.GetCurrent().User!;

    /// <summary>The session process <paramref name="pid"/> runs in, null if it is gone.</summary>
    public static uint? SessionOf(int pid)
    {
        uint session;
        return Kernel32.ProcessIdToSessionId((uint)pid, &session) ? session : null;
    }

    /// <summary>The executable process <paramref name="pid"/> runs, null if it is gone or cannot be asked.</summary>
    public static string? ImagePath(int pid)
    {
        using var handle = Kernel32.OpenProcess(Kernel32.PROCESS_QUERY_LIMITED_INFORMATION, false, (uint)pid);
        return handle.IsInvalid ? null : ImagePath(handle);
    }

    internal static string? ImagePath(Win32Handle process)
    {
        var buffer = new char[32768];
        fixed (char* name = buffer)
        {
            var size = (uint)buffer.Length;
            return Kernel32.QueryFullProcessImageName(process, 0, name, &size) ? new string(name, 0, (int)size) : null;
        }
    }

    /// <summary>The account the process behind <paramref name="process"/> runs as, null if its token cannot be read.</summary>
    internal static SecurityIdentifier? UserOf(Win32Handle process)
    {
        nint token = 0;
        var added = false;
        try
        {
            process.DangerousAddRef(ref added);
            if (!Kernel32.OpenProcessToken(process.DangerousGetHandle(), Kernel32.TOKEN_QUERY, &token)) return null;
            using var identity = new WindowsIdentity(token);
            return identity.User;
        }
        catch (Exception e) when (e is UnauthorizedAccessException or ArgumentException
                                      or System.Security.SecurityException)
        {
            return null;
        }
        finally
        {
            if (token != 0) Kernel32.CloseHandle(token);
            if (added) process.DangerousRelease();
        }
    }

    /// <summary>
    /// Whether process <paramref name="pid"/> runs <paramref name="executable"/> (however
    /// either is spelt: 8.3 names, case, links) in this session as this user.
    /// </summary>
    public static bool Runs(int pid, string executable)
    {
        if (SessionOf(pid) != CurrentSession) return false;
        using var handle = Kernel32.OpenProcess(Kernel32.PROCESS_QUERY_LIMITED_INFORMATION, false, (uint)pid);
        if (handle.IsInvalid) return false;
        if (ImagePath(handle) is not { } image || UserOf(handle) != CurrentUser) return false;
        var wanted = Orphans.Spellings(executable, folder: false);
        return Orphans.Spellings(image, folder: false).Any(s => wanted.Contains(s, StringComparer.OrdinalIgnoreCase));
    }
}
