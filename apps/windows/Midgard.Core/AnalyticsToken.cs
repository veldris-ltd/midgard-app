using System.Runtime.Versioning;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;

namespace Midgard.Core;

/// <summary>The token file could not be made safe (a folder in its place).</summary>
public sealed class AnalyticsTokenException(string message) : Exception(message);

/// <summary>
/// The bearer token the gateway sends the camera analytics
/// (<see cref="CameraAnalytics.TokenFile"/>). The port of <c>CameraAnalytics.ensureToken</c>
/// in apps/mac/Sources/MidgardShellCore/ServiceSpec.swift, with Windows' equivalents: the
/// file's own protected DACL (inheritance off, one entry: this user, full control) for mode
/// 0600, FileMode.CreateNew for O_CREAT|O_EXCL, and a reparse point (a symbolic link or
/// junction) removed, never followed, for O_NOFOLLOW. The token is never logged.
/// </summary>
[SupportedOSPlatform("windows")]
public static class AnalyticsToken
{
    /// <summary>
    /// Makes sure <paramref name="file"/> holds a token readable by this user only; the
    /// gateway reads it when it starts, so this runs before the services. A regular file with
    /// a token is kept (its DACL made private); a reparse point or an empty file is replaced;
    /// a folder is an error. A new file is created exclusively, with the private DACL from
    /// the start, holding 48 hex digits from the system's random source.
    /// </summary>
    /// <exception cref="AnalyticsTokenException">A folder is where the file should be.</exception>
    /// <exception cref="IOException">The file could not be read, removed or written.</exception>
    /// <exception cref="UnauthorizedAccessException">The same, for want of access.</exception>
    public static void Ensure(string file)
    {
        file = Path.GetFullPath(file);
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        FileAttributes? attributes;
        try
        {
            // GetFileAttributesEx: the attributes of a link itself, not of its target.
            attributes = File.GetAttributes(file);
        }
        catch (Exception e) when (e is FileNotFoundException or DirectoryNotFoundException)
        {
            attributes = null;
        }
        if (attributes is { } a)
        {
            if (a.HasFlag(FileAttributes.ReparsePoint))
            {
                // Removes the link only (a folder link with Directory.Delete, not recursive).
                if (a.HasFlag(FileAttributes.Directory)) Directory.Delete(file);
                else File.Delete(file);
            }
            else if (a.HasFlag(FileAttributes.Directory))
            {
                throw new AnalyticsTokenException($"{file} is not a regular file");
            }
            else
            {
                string text;
                using (var stream = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.Read))
                {
                    var buffer = new byte[256];
                    var n = stream.Read(buffer, 0, buffer.Length);
                    text = Encoding.UTF8.GetString(buffer, 0, n);
                }
                if (text.Trim().Length > 0)
                {
                    new FileInfo(file).SetAccessControl(Private());
                    return;
                }
                File.Delete(file);
            }
        }
        var token = Encoding.ASCII.GetBytes(Convert.ToHexStringLower(RandomNumberGenerator.GetBytes(24)) + "\n");
        using var created = new FileInfo(file).Create(FileMode.CreateNew, FileSystemRights.Write | FileSystemRights.Synchronize,
                                                      FileShare.None, 4096, FileOptions.None, Private());
        created.Write(token);
    }

    /// <summary>Inheritance off, one entry: this user, full control.</summary>
    internal static FileSecurity Private()
    {
        var security = new FileSecurity();
        security.SetAccessRuleProtection(isProtected: true, preserveInheritance: false);
        var me = WindowsIdentity.GetCurrent().User
                 ?? throw new UnauthorizedAccessException("Windows did not say who this user is");
        security.AddAccessRule(new FileSystemAccessRule(me, FileSystemRights.FullControl, AccessControlType.Allow));
        return security;
    }
}
