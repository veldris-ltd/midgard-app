using System.Runtime.InteropServices;
using System.Runtime.Versioning;

namespace Midgard.Core;

/// <summary>
/// A path with every symbolic link and junction in it resolved, as the file system sees it
/// (on Windows also with 8.3 short names made long and the real letter case). Used by the
/// orphan sweep and by the web server's check that a file stays inside its folder.
/// </summary>
static partial class RealPath
{
    /// <summary>The resolved path of an existing file or folder, null if it cannot be resolved.</summary>
    public static string? Of(string path) => OperatingSystem.IsWindows() ? Windows(path) : Unix(path);

    /// <summary>Through a handle opened on the path (CreateFileW follows every reparse point), GetFinalPathNameByHandleW.</summary>
    [SupportedOSPlatform("windows")]
    public static unsafe string? Windows(string path)
    {
        using var handle = Kernel32.CreateFile(Path.GetFullPath(path), 0,
                                               Kernel32.FILE_SHARE_READ | Kernel32.FILE_SHARE_WRITE | Kernel32.FILE_SHARE_DELETE,
                                               null, Kernel32.OPEN_EXISTING, Kernel32.FILE_FLAG_BACKUP_SEMANTICS, 0);
        return handle.IsInvalid ? null : OfHandle(handle);
    }

    /// <summary>The path of the file an open handle is on: what was really opened, whatever the name said.</summary>
    [SupportedOSPlatform("windows")]
    public static unsafe string? OfHandle(SafeHandle handle)
    {
        var added = false;
        handle.DangerousAddRef(ref added);
        try
        {
            var buffer = new char[32768];
            fixed (char* result = buffer)
            {
                var length = Kernel32.GetFinalPathNameByHandle(handle.DangerousGetHandle(), result, (uint)buffer.Length, 0);
                if (length == 0 || length >= buffer.Length) return null;
                var final = new string(result, 0, (int)length);
                // \\?\C:\… → C:\…, \\?\UNC\server\… → \\server\…
                if (final.StartsWith(@"\\?\UNC\", StringComparison.Ordinal)) return @"\\" + final[8..];
                return final.StartsWith(@"\\?\", StringComparison.Ordinal) ? final[4..] : final;
            }
        }
        finally
        {
            if (added) handle.DangerousRelease();
        }
    }

    static string? Unix(string path)
    {
        var resolved = realpath(Path.GetFullPath(path), 0);
        if (resolved == 0) return null;
        try
        {
            return Marshal.PtrToStringUTF8(resolved);
        }
        finally
        {
            free(resolved);
        }
    }

    [LibraryImport("libc", EntryPoint = "realpath", StringMarshalling = StringMarshalling.Utf8)]
    private static partial nint realpath(string path, nint resolved);

    [LibraryImport("libc", EntryPoint = "free")]
    private static partial void free(nint pointer);
}
