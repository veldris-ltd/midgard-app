using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace Midgard.Core;

// The process calls the supervisor's review added (W2 Task 3): an owned copy of a process
// handle for the thread-pool wait, waits, and the session and user a process runs as.
// Kept apart from Kernel32.cs so the shell's other parts can add to that file meanwhile.
static unsafe partial class Kernel32
{
    public const uint DUPLICATE_SAME_ACCESS = 0x2;
    public const uint WAIT_OBJECT_0 = 0;
    public const uint TOKEN_QUERY = 0x0008;

    [LibraryImport(Dll)]
    public static partial nint GetCurrentProcess();

    [LibraryImport(Dll, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool DuplicateHandle(nint sourceProcess, nint source, nint targetProcess, nint* target,
                                               uint access, [MarshalAs(UnmanagedType.Bool)] bool inherit, uint options);

    [LibraryImport(Dll, SetLastError = true)]
    public static partial uint WaitForSingleObject(nint handle, uint milliseconds);

    [LibraryImport(Dll, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool ProcessIdToSessionId(uint pid, uint* session);

    [LibraryImport("advapi32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static partial bool OpenProcessToken(nint process, uint access, nint* token);
}
