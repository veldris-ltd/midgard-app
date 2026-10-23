namespace Midgard.Core;

/// <summary>One entry of a folder's access list, as <see cref="WindowsAcl"/> reads it.</summary>
/// <param name="Sid">Who it is for, as a SID string (e.g. S-1-1-0 for Everyone).</param>
/// <param name="Allow">An allow entry (true) or a deny entry.</param>
/// <param name="Rights">The access mask (FileSystemRights, generic rights included).</param>
public readonly record struct AccessEntry(string Sid, bool Allow, uint Rights);

/// <summary>
/// The once-at-start check that the app's folders are private to the presenter. The working
/// copy holds the demo's keys and data and the logs what the demo did; neither has an ACL
/// of its own (they inherit their folder's), so other accounts on the PC must not be able
/// to read them. The default <c>%LOCALAPPDATA%</c> ACL (the user, SYSTEM and
/// Administrators) passes.
/// </summary>
/// <remarks>
/// The decision is a pure function of the access entries, so it is tested on any OS;
/// reading a real folder's entries is <see cref="WindowsAcl.Entries"/>, Windows only.
/// </remarks>
public static class FolderAccess
{
    public const string Everyone = "S-1-1-0";
    public const string AuthenticatedUsers = "S-1-5-11";
    /// <summary>BUILTIN\Users.</summary>
    public const string Users = "S-1-5-32-545";

    static readonly (string Sid, string Name)[] Groups =
        [(Everyone, "Everyone"), (Users, "Users"), (AuthenticatedUsers, "Authenticated Users")];

    /// <summary>FILE_READ_DATA (FILE_LIST_DIRECTORY on a folder).</summary>
    internal const uint ReadData = 0x1;
    internal const uint GenericAll = 0x1000_0000;
    internal const uint GenericRead = 0x8000_0000;

    /// <summary>
    /// null when none of Everyone, Users or Authenticated Users is allowed to read
    /// <paramref name="folder"/>, else a short message for the presenter. Entries meant
    /// only for what the folder will hold (inherit-only) count too: they decide who can
    /// read the files.
    /// </summary>
    public static string? Problem(string folder, IEnumerable<AccessEntry> entries)
    {
        var readers = Groups
            .Where(g => entries.Any(e => e.Allow && e.Sid == g.Sid
                                         && (e.Rights & (ReadData | GenericRead | GenericAll)) != 0))
            .Select(g => g.Name)
            .ToList();
        if (readers.Count == 0) return null;
        var who = readers.Count == 1 ? readers[0] : string.Join(", ", readers.Take(readers.Count - 1)) + " and " + readers[^1];
        return $"Other people who use this PC can read {folder}: its permissions let {who} read it. "
             + "Midgard keeps the demo's working copy and logs there, so it will not start until only your "
             + $"account can read that folder. Remove {who} from the folder's Properties > Security tab "
             + "(or ask whoever looks after this PC), then open Midgard again.";
    }

    /// <summary>
    /// The folders to check: the working copy, the logs and the folder the WebView2 profile
    /// is in (<c>%LOCALAPPDATA%\Midgard</c> by default), each as it is or, if it does not
    /// exist yet, the nearest folder above it that does (a new folder inherits its ACL).
    /// </summary>
    public static IReadOnlyList<string> FoldersToCheck(Paths paths, Func<string, bool>? exists = null)
    {
        exists ??= Directory.Exists;
        var list = new List<string>();
        foreach (var folder in new[] { paths.Home, paths.Logs, Path.GetDirectoryName(paths.WebView) ?? paths.WebView })
        {
            var at = Path.TrimEndingDirectorySeparator(Path.GetFullPath(folder));
            while (!exists(at) && Path.GetDirectoryName(at) is { } up) at = up;
            if (exists(at) && !list.Contains(at, StringComparer.OrdinalIgnoreCase)) list.Add(at);
        }
        return list;
    }

    /// <summary>
    /// The first problem with the app's folders, or null. <paramref name="read"/> reads a
    /// folder's access list (<see cref="WindowsAcl.Entries"/>); a folder whose list cannot be
    /// read (e.g. on a FAT-formatted USB drive, which has none) is a problem too.
    /// </summary>
    public static string? Problem(Paths paths, Func<string, IReadOnlyList<AccessEntry>> read,
                                  Func<string, bool>? exists = null)
    {
        foreach (var folder in FoldersToCheck(paths, exists))
        {
            IReadOnlyList<AccessEntry> entries;
            try
            {
                entries = read(folder);
            }
            catch (Exception e) when (e is not OutOfMemoryException)
            {
                return $"Midgard could not check who can read {folder} ({e.Message}). It keeps the demo's "
                     + "working copy and logs there and needs that folder to be private to your account, "
                     + "on an NTFS drive.";
            }
            if (Problem(folder, entries) is { } problem) return problem;
        }
        return null;
    }
}
