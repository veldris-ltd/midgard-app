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
/// <para>The decision is a pure function of the access entries, so it is tested on any OS;
/// reading a real folder's entries is <see cref="WindowsAcl.Entries"/>, Windows only.</para>
/// <para>Refused: an allow entry that lets Everyone, Users, Authenticated Users, INTERACTIVE,
/// Guests, a domain's Domain Users, or any other account or domain group (an S-1-5-21 SID
/// other than the presenter's) read the folder, write into it, or change its permissions or
/// owner. Deny entries are ignored: one could take some of those rights back, but working
/// that out (order, inheritance, group membership) is Windows' job, and ignoring them only
/// errs towards refusing. SYSTEM, Administrators and the creator-owner entries pass, as on
/// the default %LOCALAPPDATA%.</para>
/// </remarks>
public static class FolderAccess
{
    public const string Everyone = "S-1-1-0";
    public const string AuthenticatedUsers = "S-1-5-11";
    /// <summary>BUILTIN\Users.</summary>
    public const string Users = "S-1-5-32-545";
    /// <summary>INTERACTIVE: anyone signed in at the PC.</summary>
    public const string Interactive = "S-1-5-4";
    /// <summary>BUILTIN\Guests.</summary>
    public const string Guests = "S-1-5-32-546";

    static readonly (string Sid, string Name)[] Groups =
    [
        (Everyone, "Everyone"), (Users, "Users"), (AuthenticatedUsers, "Authenticated Users"),
        (Interactive, "INTERACTIVE"), (Guests, "Guests"),
    ];

    /// <summary>FILE_READ_DATA (FILE_LIST_DIRECTORY on a folder).</summary>
    internal const uint ReadData = 0x1;
    /// <summary>FILE_WRITE_DATA (FILE_ADD_FILE on a folder).</summary>
    internal const uint WriteData = 0x2;
    /// <summary>FILE_APPEND_DATA (FILE_ADD_SUBDIRECTORY on a folder).</summary>
    internal const uint AppendData = 0x4;
    internal const uint WriteDac = 0x4_0000;
    internal const uint WriteOwner = 0x8_0000;
    internal const uint GenericAll = 0x1000_0000;
    internal const uint GenericWrite = 0x4000_0000;
    internal const uint GenericRead = 0x8000_0000;

    const uint ReadRights = ReadData | GenericRead | GenericAll;
    const uint ChangeRights = WriteData | AppendData | WriteDac | WriteOwner | GenericWrite | GenericAll;

    /// <summary>
    /// Who <paramref name="sid"/> is, if it is someone the folder must not be open to: one of
    /// the groups, a domain's Domain Users (RID 513), or another account (an S-1-5-21 SID that
    /// is not <paramref name="me"/>; not judged when <paramref name="me"/> is null).
    /// </summary>
    static string? Outsider(string sid, string? me)
    {
        foreach (var (groupSid, name) in Groups)
        {
            if (sid == groupSid) return name;
        }
        if (!sid.StartsWith("S-1-5-21-", StringComparison.OrdinalIgnoreCase)) return null;
        if (sid.EndsWith("-513", StringComparison.Ordinal)) return "Domain Users";
        if (me is null || string.Equals(sid, me, StringComparison.OrdinalIgnoreCase)) return null;
        return $"another account ({sid})";
    }

    /// <summary>
    /// null when no one but the presenter (<paramref name="me"/>, their SID), SYSTEM and
    /// Administrators may read, write or re-permission <paramref name="folder"/>, else a short
    /// message for the presenter. Entries meant only for what the folder will hold
    /// (inherit-only) count too: they decide who can read the files.
    /// </summary>
    public static string? Problem(string folder, IEnumerable<AccessEntry> entries, string? me = null)
    {
        var allowed = entries.Where(e => e.Allow).ToList();
        List<string> Who(uint rights) => allowed
            .Where(e => (e.Rights & rights) != 0)
            .Select(e => Outsider(e.Sid, me))
            .OfType<string>()
            .Distinct()
            .OrderBy(name => Array.FindIndex(Groups, g => g.Name == name) is var i and >= 0 ? i : Groups.Length)
            .ToList();
        static string Names(List<string> names) =>
            names.Count == 1 ? names[0] : string.Join(", ", names.Take(names.Count - 1)) + " and " + names[^1];

        var readers = Who(ReadRights);
        var changers = Who(ChangeRights);
        if (readers.Count == 0 && changers.Count == 0) return null;
        var who = Names(readers.Count > 0 ? readers : changers);
        var all = Names(readers.Union(changers).ToList());
        var opening = readers.Count > 0
            ? $"Other people who use this PC can read {folder}: its permissions let {who} read it. "
            : $"Other people who use this PC can change {folder}: its permissions let {who} change it or its permissions. ";
        return opening
             + "Midgard keeps the demo's working copy and logs there, so it will not start until only your "
             + $"account can use that folder. Remove {all} from the folder's Properties > Security tab "
             + "(or ask whoever looks after this PC), then open Midgard again.";
    }

    /// <summary>
    /// The folders to check: the working copy, the logs, the folder the working copy is in
    /// (Reset demo makes its fresh copy there, and the instance lock is there; with
    /// MIDGARD_APP_HOME it is not the app's folder) and the folder the WebView2 profile is in
    /// (<c>%LOCALAPPDATA%\Midgard</c> by default), each as it is or, if it does not exist yet,
    /// the nearest folder above it that does (a new folder inherits its ACL).
    /// </summary>
    public static IReadOnlyList<string> FoldersToCheck(Paths paths, Func<string, bool>? exists = null)
    {
        exists ??= Directory.Exists;
        var list = new List<string>();
        var homeParent = Path.GetDirectoryName(Path.TrimEndingDirectorySeparator(Path.GetFullPath(paths.Home)));
        foreach (var folder in new[] { paths.Home, paths.Logs, homeParent ?? paths.Home,
                                       Path.GetDirectoryName(paths.WebView) ?? paths.WebView })
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
    /// <param name="me">The presenter's SID, so any other account's entry is refused too.</param>
    public static string? Problem(Paths paths, Func<string, IReadOnlyList<AccessEntry>> read,
                                  Func<string, bool>? exists = null, string? me = null)
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
            if (Problem(folder, entries, me) is { } problem) return problem;
        }
        return null;
    }
}
