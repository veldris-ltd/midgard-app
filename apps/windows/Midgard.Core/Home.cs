using System.Globalization;

namespace Midgard.Core;

/// <summary>What went wrong with the working copy.</summary>
public enum HomeProblem
{
    /// <summary>
    /// The folder holds something that is not a working copy (no <c>.midgard-home</c>
    /// marker and not empty). It is never replaced: the presenter must move it away.
    /// </summary>
    NotAWorkingCopy,
    StartingStateMissing,
    /// <summary>The starting state must be plain files and folders: no symbolic links or junctions.</summary>
    LinkInStartingState,
    InsufficientSpace,
    /// <summary>The copy did not come out the same size as the starting state.</summary>
    CopyIncomplete,
    /// <summary>
    /// A fresh copy could not be moved into place and the previous one could not be put
    /// back either; it is at <see cref="HomeException.Path"/>.
    /// </summary>
    ReplaceFailed,
}

/// <summary>A working-copy error, with a message that can be shown to the presenter.</summary>
public sealed class HomeException : Exception
{
    public HomeProblem Problem { get; }
    /// <summary>The folder or file concerned (for ReplaceFailed, where the previous copy is).</summary>
    public string? Path { get; }
    public long Needed { get; }
    public long Available { get; }

    HomeException(HomeProblem problem, string message, string? path = null, long needed = 0,
                  long available = 0, Exception? inner = null) : base(message, inner)
    {
        Problem = problem;
        Path = path;
        Needed = needed;
        Available = available;
    }

    internal static HomeException NotAWorkingCopy(string home) =>
        new(HomeProblem.NotAWorkingCopy,
            $"{home} is not a Midgard working copy (no {Home.MarkerName} marker); move it away and try again",
            home);

    internal static HomeException StartingStateMissing(string starting) =>
        new(HomeProblem.StartingStateMissing, $"the starting state is missing at {starting}", starting);

    internal static HomeException LinkInStartingState(string path) =>
        new(HomeProblem.LinkInStartingState,
            $"the starting state contains a symbolic link or junction: {path}", path);

    internal static HomeException InsufficientSpace(long needed, long available) =>
        new(HomeProblem.InsufficientSpace,
            $"not enough free space for the working copy: needs {Bytes(needed)}, {Bytes(available)} available",
            needed: needed, available: available);

    internal static HomeException CopyIncomplete(string copy, long files, long bytes, long wantFiles, long wantBytes) =>
        new(HomeProblem.CopyIncomplete,
            $"the working copy came out incomplete ({files} files, {bytes} bytes; the starting state has "
            + $"{wantFiles} files, {wantBytes} bytes)", copy);

    internal static HomeException ReplaceFailed(Exception inner, string previousCopy) =>
        new(HomeProblem.ReplaceFailed,
            $"the working copy could not be replaced ({inner.Message}); the previous working copy is at {previousCopy}",
            previousCopy, inner: inner);

    /// <summary>Like macOS's ByteCountFormatter: decimal units, e.g. "1.2 GB".</summary>
    internal static string Bytes(long count)
    {
        string[] units = ["bytes", "KB", "MB", "GB", "TB"];
        double value = count;
        var unit = 0;
        while (Math.Abs(value) >= 1000 && unit < units.Length - 1)
        {
            value /= 1000;
            unit++;
        }
        return unit == 0
            ? $"{count} bytes"
            : value.ToString(value < 10 ? "0.#" : "0", CultureInfo.InvariantCulture) + " " + units[unit];
    }
}

/// <summary>
/// The working copy of the starting state: the services' configuration and initial data,
/// copied from <c>resources\starting-state</c> into <see cref="Paths.Home"/> so a demo can
/// change it and Reset demo can put it back. The port of
/// apps/mac/Sources/MidgardShellCore/Home.swift.
/// </summary>
/// <remarks>
/// <para>A copy counts as prepared only when <c>Home\.midgard-home</c> exists and holds the
/// starting state's version (<c>starting\VERSION</c>, or "dev" without one). The copy is
/// made in a temporary sibling (<c>Home.midgard-copy-…</c>), checked against the starting
/// state (same number of files and bytes) and marked before it is moved into place, so a
/// half-finished copy never counts as prepared.</para>
/// <para><c>Home</c> is replaced only when it is missing, empty, or a working copy (marked,
/// of any version). Anything else throws <see cref="HomeProblem.NotAWorkingCopy"/> and is
/// left untouched.</para>
/// <para>When the app is upgraded and the starting state's version changes, the old working
/// copy is discarded and a fresh one made: any changes made during demos are lost.</para>
/// <para>Windows, unlike the Mac code: the room check is always made (NTFS has no clones,
/// so a copy takes its full size even on the same drive); copied files lose the read-only
/// attribute and the Zone.Identifier stream (Mark of the Web, which a ZIP extracted from a
/// download carries, as macOS's quarantine flag); the copy's permissions are the ones it
/// inherits from Home's folder. Paths longer than 260 characters need nothing special:
/// .NET adds the <c>\\?\</c> prefix itself where Windows needs it.</para>
/// </remarks>
public static class Home
{
    public const string MarkerName = ".midgard-home";
    internal const string CopyInfix = ".midgard-copy";
    internal const string OldInfix = ".midgard-old";

    /// <summary>What an otherwise empty folder may hold and still count as empty.</summary>
    static readonly string[] Ignorable = [".DS_Store", "desktop.ini", "Thumbs.db"];

    static readonly EnumerationOptions AllEntries = new()
    {
        // The default skips hidden and system files; the copy must not.
        AttributesToSkip = 0,
        IgnoreInaccessible = false,
        RecurseSubdirectories = false,
        ReturnSpecialDirectories = false,
    };

    /// <summary>What a reset left behind.</summary>
    /// <param name="Leftover">The previous working copy, if it could not be deleted (it can be removed by hand).</param>
    /// <param name="Reason">Why it could not be deleted.</param>
    public sealed record ResetReport(string? Leftover = null, string? Reason = null);

    /// <summary>Fault injection for tests: each runs before the step it names and may throw.</summary>
    internal sealed class Hooks
    {
        /// <summary>Before each file is copied, with its path relative to the starting state.</summary>
        public Action<string>? BeforeCopy;
        /// <summary>Before a folder is deleted, with its full path.</summary>
        public Action<string>? BeforeDelete;
    }

    /// <summary>The lock file one running app holds for this working copy: <c>&lt;Home&gt;.instance</c> beside it.</summary>
    public static string InstanceLockFile(string home)
    {
        var full = Normalize(home);
        return System.IO.Path.Join(Parent(full), System.IO.Path.GetFileName(full) + ".instance");
    }

    /// <summary>The starting state's version: the first line of <c>starting\VERSION</c>, else "dev".</summary>
    public static string Version(string starting)
    {
        string text;
        try
        {
            text = File.ReadAllText(System.IO.Path.Join(starting, "VERSION"));
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return "dev";
        }
        var line = text.Split(['\r', '\n'], StringSplitOptions.RemoveEmptyEntries).FirstOrDefault() ?? "";
        var trimmed = line.Trim();
        return trimmed.Length == 0 ? "dev" : trimmed;
    }

    /// <summary>Whether <paramref name="home"/> is a complete working copy of this version of <paramref name="starting"/>.</summary>
    public static bool IsPrepared(string starting, string home) =>
        MarkerVersion(home) is { } marked && marked == Version(starting);

    /// <summary>Makes <paramref name="home"/> a working copy of <paramref name="starting"/> unless it already is one.</summary>
    public static void Prepare(string starting, string home) => Prepare(starting, home, null);

    internal static void Prepare(string starting, string home, Hooks? hooks)
    {
        home = Normalize(home);
        if (IsPrepared(starting, home)) return;
        CheckReplaceable(home);
        var fresh = MakeCopy(starting, home, hooks);
        Swap(fresh, home);
        // The replaced copy, and any left by an earlier reset whose delete failed.
        RemoveSiblings(home, OldInfix, hooks);
    }

    /// <summary>
    /// Reset demo: puts <paramref name="home"/> back to <paramref name="starting"/>. Only the
    /// working copy changes; the logs and the WebView2 profile are separate folders.
    /// </summary>
    /// <remarks>
    /// The services must be stopped first (<paramref name="servicesStopped"/>): on Windows a
    /// file a service holds open stops the working copy being moved or deleted. A fresh copy
    /// is made beside <paramref name="home"/> while it is still in place; only once that copy
    /// is complete is the old one moved aside and the fresh one moved in. If the copy fails,
    /// <paramref name="home"/> is untouched and the error rethrown; if the swap fails, the
    /// previous copy is put back (or, if even that fails, <see cref="HomeProblem.ReplaceFailed"/>
    /// says where it is). A previous copy that cannot be deleted is left beside
    /// <paramref name="home"/> and reported.
    /// </remarks>
    /// <exception cref="InvalidOperationException">The services were not stopped.</exception>
    public static ResetReport Reset(string starting, string home, bool servicesStopped)
    {
        if (!servicesStopped)
            throw new InvalidOperationException(
                "Reset demo needs the services stopped first: they hold files in the working copy open.");
        return Reset(starting, home, (Hooks?)null);
    }

    internal static ResetReport Reset(string starting, string home, Hooks? hooks)
    {
        home = Normalize(home);
        CheckReplaceable(home);
        var fresh = MakeCopy(starting, home, hooks);
        var old = Swap(fresh, home);
        var report = new ResetReport();
        if (old is not null)
        {
            try
            {
                DeleteTree(old, hooks);
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            {
                report = new ResetReport(old, e.Message);
            }
        }
        // Copies left by earlier resets whose delete failed.
        RemoveSiblings(home, OldInfix, hooks, except: report.Leftover);
        return report;
    }

    internal static void CheckRoom(long needed, long available)
    {
        if (needed > available) throw HomeException.InsufficientSpace(needed, available);
    }

    // Helpers

    static string Normalize(string path) =>
        System.IO.Path.TrimEndingDirectorySeparator(System.IO.Path.GetFullPath(path));

    static string Parent(string full) =>
        System.IO.Path.GetDirectoryName(full)
        ?? throw new ArgumentException($"{full} is a drive's root, not a folder for the working copy");

    static string? MarkerVersion(string home)
    {
        try
        {
            return File.ReadAllText(System.IO.Path.Join(home, MarkerName)).Trim();
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return null;
        }
    }

    /// <summary><paramref name="home"/> may be replaced only if it is missing, an empty folder, or a marked copy.</summary>
    static void CheckReplaceable(string home)
    {
        if (!Exists(home)) return;
        if (IsLink(home) || !Directory.Exists(home)) throw HomeException.NotAWorkingCopy(home);
        if (MarkerVersion(home) is not null) return;
        string[] names;
        try
        {
            names = new DirectoryInfo(home).EnumerateFileSystemInfos("*", AllEntries).Select(e => e.Name).ToArray();
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            names = ["?"];
        }
        if (names.All(n => Ignorable.Contains(n, StringComparer.OrdinalIgnoreCase))) return;
        throw HomeException.NotAWorkingCopy(home);
    }

    /// <summary>
    /// Makes a complete, marked copy of the starting state in a new sibling of
    /// <paramref name="home"/> and returns its path; <paramref name="home"/> is not touched.
    /// </summary>
    static string MakeCopy(string starting, string home, Hooks? hooks)
    {
        var source = ResolveRoot(starting);
        if (!Directory.Exists(source)) throw HomeException.StartingStateMissing(starting);
        var (files, bytes) = Inspect(source);
        var parent = Parent(home);
        Directory.CreateDirectory(parent);
        if (AvailableBytes(parent) is { } available) CheckRoom(bytes, available);
        // Copies a crash interrupted.
        RemoveSiblings(home, CopyInfix, hooks);

        var version = Version(source);
        var temp = Sibling(home, CopyInfix);
        try
        {
            CopyTree(source, temp, "", hooks);
            var (copiedFiles, copiedBytes) = Inspect(temp);
            if (copiedFiles != files || copiedBytes != bytes)
                throw HomeException.CopyIncomplete(temp, copiedFiles, copiedBytes, files, bytes);
            File.WriteAllText(System.IO.Path.Join(temp, MarkerName), version);
        }
        catch
        {
            TryDelete(temp);
            throw;
        }
        return temp;
    }

    /// <summary>
    /// Moves <paramref name="home"/> (if any) aside and <paramref name="fresh"/> into its
    /// place; returns where the previous copy went. On failure the previous copy is put back
    /// and <paramref name="fresh"/> deleted.
    /// </summary>
    static string? Swap(string fresh, string home)
    {
        string? aside = null;
        if (Exists(home))
        {
            var old = Sibling(home, OldInfix);
            try
            {
                // Fails while a file inside is open (a service still running): nothing changes.
                Directory.Move(home, old);
            }
            catch
            {
                TryDelete(fresh);
                throw;
            }
            aside = old;
        }
        try
        {
            Directory.Move(fresh, home);
        }
        catch (Exception e)
        {
            if (aside is not null)
            {
                try
                {
                    Directory.Move(aside, home);
                }
                catch
                {
                    TryDelete(fresh);
                    throw HomeException.ReplaceFailed(e, aside);
                }
            }
            TryDelete(fresh);
            throw;
        }
        return aside;
    }

    /// <summary>The starting state's real folder, if <paramref name="starting"/> is a link to one.</summary>
    static string ResolveRoot(string starting)
    {
        var full = Normalize(starting);
        try
        {
            return Directory.ResolveLinkTarget(full, returnFinalTarget: true)?.FullName ?? full;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return full;
        }
    }

    /// <summary>Checks a folder is plain files and folders; returns how many files and bytes it holds.</summary>
    static (long Files, long Bytes) Inspect(string dir)
    {
        long files = 0, bytes = 0;
        foreach (var entry in new DirectoryInfo(dir).EnumerateFileSystemInfos("*", AllEntries))
        {
            if (entry.LinkTarget is not null) throw HomeException.LinkInStartingState(entry.FullName);
            if (entry is DirectoryInfo sub)
            {
                var (f, b) = Inspect(sub.FullName);
                files += f;
                bytes += b;
            }
            else if (entry is FileInfo file)
            {
                files++;
                bytes += file.Length;
            }
        }
        return (files, bytes);
    }

    /// <summary>Copies plain files and folders (never following a link), making each copy the demo's own.</summary>
    static void CopyTree(string source, string target, string relative, Hooks? hooks)
    {
        Directory.CreateDirectory(target);
        foreach (var entry in new DirectoryInfo(source).EnumerateFileSystemInfos("*", AllEntries))
        {
            if (entry.LinkTarget is not null) throw HomeException.LinkInStartingState(entry.FullName);
            var to = System.IO.Path.Join(target, entry.Name);
            var rel = relative.Length == 0 ? entry.Name : System.IO.Path.Join(relative, entry.Name);
            if (entry is DirectoryInfo)
            {
                CopyTree(entry.FullName, to, rel, hooks);
            }
            else
            {
                hooks?.BeforeCopy?.Invoke(rel);
                // Keeps the attributes (read-only among them) and, on Unix, the file's mode.
                File.Copy(entry.FullName, to, overwrite: false);
                MakeOwnAndWritable(to, isFile: true);
            }
        }
        MakeOwnAndWritable(target, isFile: false);
    }

    /// <summary>
    /// The copy belongs to the demo: no read-only attribute, so services can write and a
    /// later reset can delete it, and no Mark of the Web; on Unix, every folder is u+rwx.
    /// </summary>
    static void MakeOwnAndWritable(string path, bool isFile)
    {
        try
        {
            var attributes = File.GetAttributes(path);
            if (attributes.HasFlag(FileAttributes.ReadOnly))
                File.SetAttributes(path, attributes & ~FileAttributes.ReadOnly);
            if (!isFile && !OperatingSystem.IsWindows())
                File.SetUnixFileMode(path, File.GetUnixFileMode(path) | UnixFileMode.UserRead
                                           | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
            if (isFile && OperatingSystem.IsWindows())
                File.Delete(path + ":Zone.Identifier");
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            // Best effort, as the Mac code's.
        }
    }

    static long? AvailableBytes(string folder)
    {
        try
        {
            var root = System.IO.Path.GetPathRoot(folder);
            return string.IsNullOrEmpty(root) ? null : new DriveInfo(root).AvailableFreeSpace;
        }
        catch (Exception e) when (e is ArgumentException or IOException or UnauthorizedAccessException)
        {
            // A network share: not checked.
            return null;
        }
    }

    internal static string Sibling(string home, string infix) =>
        System.IO.Path.Join(Parent(home), $"{System.IO.Path.GetFileName(home)}{infix}-{Guid.NewGuid():N}");

    /// <summary>Deletes, as far as it can, every <c>&lt;home&gt;.midgard-…</c> sibling with this infix.</summary>
    static void RemoveSiblings(string home, string infix, Hooks? hooks, string? except = null)
    {
        var parent = Parent(home);
        var prefix = System.IO.Path.GetFileName(home) + infix;
        IEnumerable<FileSystemInfo> entries;
        try
        {
            entries = new DirectoryInfo(parent).EnumerateFileSystemInfos(prefix + "*", AllEntries).ToList();
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return;
        }
        foreach (var entry in entries)
        {
            if (!entry.Name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) continue;
            if (except is not null && string.Equals(entry.FullName, except, StringComparison.OrdinalIgnoreCase)) continue;
            TryDelete(entry.FullName, hooks);
        }
    }

    static void TryDelete(string path, Hooks? hooks = null)
    {
        try
        {
            DeleteTree(path, hooks);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    /// <summary>
    /// Deletes a folder (or file, or link) and everything in it, read-only files included.
    /// Links are deleted, never followed. A delete that fails is retried briefly, as an
    /// antivirus or indexer scan can hold a file for a moment.
    /// </summary>
    internal static void DeleteTree(string path, Hooks? hooks = null)
    {
        hooks?.BeforeDelete?.Invoke(path);
        for (var attempt = 1; ; attempt++)
        {
            try
            {
                if (IsLink(path) || File.Exists(path))
                {
                    ClearReadOnly(path);
                    if (Directory.Exists(path)) Directory.Delete(path);
                    else File.Delete(path);
                }
                else if (Directory.Exists(path))
                {
                    ClearReadOnlyTree(path);
                    Directory.Delete(path, recursive: true);
                }
                return;
            }
            catch (Exception e) when (attempt < 3 && e is IOException or UnauthorizedAccessException)
            {
                Thread.Sleep(100 * attempt);
            }
        }
    }

    static void ClearReadOnlyTree(string dir)
    {
        ClearReadOnly(dir);
        foreach (var entry in new DirectoryInfo(dir).EnumerateFileSystemInfos("*", AllEntries))
        {
            if (entry is DirectoryInfo && entry.LinkTarget is null) ClearReadOnlyTree(entry.FullName);
            else ClearReadOnly(entry.FullName);
        }
    }

    static void ClearReadOnly(string path)
    {
        try
        {
            var attributes = File.GetAttributes(path);
            if (attributes.HasFlag(FileAttributes.ReadOnly))
                File.SetAttributes(path, attributes & ~FileAttributes.ReadOnly);
            if (!OperatingSystem.IsWindows() && attributes.HasFlag(FileAttributes.Directory) && !IsLink(path))
                File.SetUnixFileMode(path, File.GetUnixFileMode(path) | UnixFileMode.UserRead
                                           | UnixFileMode.UserWrite | UnixFileMode.UserExecute);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    static bool Exists(string path) => File.Exists(path) || Directory.Exists(path) || IsLink(path);

    static bool IsLink(string path)
    {
        try
        {
            return new FileInfo(path).LinkTarget is not null;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }
}
