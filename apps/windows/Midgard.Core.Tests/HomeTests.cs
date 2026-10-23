using System.Diagnostics;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of apps/mac/Tests/MidgardShellCoreTests/HomeTests.swift, then the ones only
// the Windows shell has: Logs and WebView beside Home, an interrupted reset, the services
// flag, read-only files, files in use, junctions and long paths.
public sealed class HomeTests : IDisposable
{
    readonly string root;
    readonly string starting;
    readonly string home;

    public HomeTests()
    {
        root = Path.Join(Path.GetTempPath(), $"home-tests-{Guid.NewGuid():N}");
        starting = Path.Join(root, "starting-state");
        home = Path.Join(root, "Support", "Midgard");
        Directory.CreateDirectory(Path.Join(starting, "gateway", "keys"));
        File.WriteAllText(Path.Join(starting, "VERSION"), "v1\n");
        File.WriteAllText(Path.Join(starting, "gateway", "config.toml"), "listen = 1");
        var key = Path.Join(starting, "gateway", "keys", "issuer.key");
        File.WriteAllText(key, "secret");
        if (!OperatingSystem.IsWindows())
            File.SetUnixFileMode(key, UnixFileMode.UserRead | UnixFileMode.UserWrite);
    }

    public void Dispose()
    {
        // Some tests make files read-only; DeleteTree clears that.
        try
        {
            Home.DeleteTree(root);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    string Read(string relative) => File.ReadAllText(Path.Join(home, relative));

    void Write(string text, string relative) => File.WriteAllText(Path.Join(home, relative), text);

    List<string> Siblings(string fragment)
    {
        var parent = Path.GetDirectoryName(home)!;
        if (!Directory.Exists(parent)) return [];
        return Directory.EnumerateFileSystemEntries(parent)
            .Select(p => Path.GetFileName(p))
            .Where(n => n.Contains(fragment, StringComparison.Ordinal))
            .ToList();
    }

    static Home.Hooks FailOnCopy(int nth)
    {
        var count = 0;
        return new Home.Hooks
        {
            BeforeCopy = _ =>
            {
                if (++count == nth) throw new IOException("simulated failure mid-copy");
            },
        };
    }

    /// <summary>Makes a symbolic link, or returns false where the account may not (no Developer Mode, not elevated).</summary>
    static bool TryLink(Func<FileSystemInfo> make)
    {
        try
        {
            make();
            return true;
        }
        catch (Exception e) when (e is UnauthorizedAccessException or IOException)
        {
            return false;
        }
    }

    [Fact]
    public void FirstPrepareCopiesAndMarks()
    {
        Home.Prepare(starting, home);
        Assert.Equal("listen = 1", Read("gateway/config.toml"));
        Assert.Equal("secret", Read("gateway/keys/issuer.key"));
        if (!OperatingSystem.IsWindows())
            Assert.Equal(UnixFileMode.UserRead | UnixFileMode.UserWrite,
                         File.GetUnixFileMode(Path.Join(home, "gateway", "keys", "issuer.key")));
        Assert.Equal("v1", Read(".midgard-home"));
        Assert.True(Home.IsPrepared(starting, home));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void VersionDefaultsToDev()
    {
        File.Delete(Path.Join(starting, "VERSION"));
        Home.Prepare(starting, home);
        Assert.Equal("dev", Read(".midgard-home"));
    }

    [Fact]
    public void VersionIsTheFirstLineWithoutBomOrCrlf()
    {
        // As Notepad may save it.
        File.WriteAllText(Path.Join(starting, "VERSION"), "\r\n  v7 \r\nrest\r\n", new System.Text.UTF8Encoding(true));
        Assert.Equal("v7", Home.Version(starting));
        File.WriteAllText(Path.Join(starting, "VERSION"), " \r\n");
        Assert.Equal("dev", Home.Version(starting));
    }

    [Fact]
    public void SecondPrepareIsANoOp()
    {
        Home.Prepare(starting, home);
        Write("edited", "gateway/config.toml");
        Home.Prepare(starting, home);
        Assert.Equal("edited", Read("gateway/config.toml"));
    }

    [Fact]
    public void InterruptedCopyIsCleanedUpAndRedone()
    {
        // A crash mid-copy leaves only a temporary sibling; home was never moved into place.
        var temp = Path.Join(Path.GetDirectoryName(home)!, "Midgard.midgard-copy-DEAD");
        Directory.CreateDirectory(temp);
        File.WriteAllText(Path.Join(temp, "partial"), "half");
        Assert.False(Home.IsPrepared(starting, home));

        Home.Prepare(starting, home);
        Assert.Equal("listen = 1", Read("gateway/config.toml"));
        Assert.False(File.Exists(Path.Join(home, "partial")));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void EmptyHomeIsReplaced()
    {
        Directory.CreateDirectory(home);
        File.WriteAllText(Path.Join(home, "desktop.ini"), "[.ShellClassInfo]");
        Home.Prepare(starting, home);
        Assert.True(Home.IsPrepared(starting, home));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void ForeignFolderIsNeverReplaced()
    {
        Directory.CreateDirectory(home);
        Write("mine", "notes.txt");
        foreach (var call in new Action[]
                 {
                     () => Home.Prepare(starting, home),
                     () => Home.Reset(starting, home, servicesStopped: true),
                 })
        {
            var error = Assert.Throws<HomeException>(call);
            Assert.Equal(HomeProblem.NotAWorkingCopy, error.Problem);
            Assert.Equal(home, error.Path);
            Assert.Contains(home, error.Message);
            Assert.Equal(new[] { "notes.txt" }, Directory.EnumerateFileSystemEntries(home).Select(p => Path.GetFileName(p)));
            Assert.Equal("mine", Read("notes.txt"));
            Assert.Empty(Siblings(".midgard-"));
        }
    }

    [Fact]
    public void FileInPlaceOfHomeIsNeverReplaced()
    {
        Directory.CreateDirectory(Path.GetDirectoryName(home)!);
        File.WriteAllText(home, "a file");
        var error = Assert.Throws<HomeException>(() => Home.Prepare(starting, home));
        Assert.Equal(HomeProblem.NotAWorkingCopy, error.Problem);
        Assert.Equal("a file", File.ReadAllText(home));
    }

    [Fact]
    public void SymlinkedStartingStateIsCopiedAsARealFolder()
    {
        var link = Path.Join(root, "starting-link");
        if (!TryLink(() => Directory.CreateSymbolicLink(link, starting))) return;
        Home.Prepare(link, home);
        Assert.Null(new DirectoryInfo(home).LinkTarget);
        Assert.True(Directory.Exists(home));
        Assert.Equal("listen = 1", Read("gateway/config.toml"));
        Assert.False(File.Exists(Path.Join(starting, ".midgard-home")));
        Assert.False(File.Exists(Path.Join(link, ".midgard-home")));
    }

    [Fact]
    public void SymlinkInsideStartingStateIsRejected()
    {
        var inner = Path.Join(starting, "gateway", "keys", "link.key");
        var target = Path.Join(starting, "gateway", "config.toml");
        if (!TryLink(() => File.CreateSymbolicLink(inner, target))) return;
        var error = Assert.Throws<HomeException>(() => Home.Prepare(starting, home));
        Assert.Equal(HomeProblem.LinkInStartingState, error.Problem);
        Assert.Equal("link.key", Path.GetFileName(error.Path));
        Assert.Contains("link.key", error.Message);
        Assert.False(Directory.Exists(home));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void JunctionInsideStartingStateIsRejected()
    {
        if (!OperatingSystem.IsWindows()) return;
        var junction = Path.Join(starting, "elsewhere");
        var info = new ProcessStartInfo("cmd.exe") { UseShellExecute = false, CreateNoWindow = true };
        foreach (var arg in new[] { "/c", "mklink", "/J", junction, Path.Join(starting, "gateway") })
            info.ArgumentList.Add(arg);
        using (var mklink = Process.Start(info)!)
        {
            mklink.WaitForExit();
            Assert.Equal(0, mklink.ExitCode);
        }
        var error = Assert.Throws<HomeException>(() => Home.Prepare(starting, home));
        Assert.Equal(HomeProblem.LinkInStartingState, error.Problem);
        Assert.Equal("elsewhere", Path.GetFileName(error.Path));
        Assert.False(Directory.Exists(home));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void CopyIsWritableAndUnquarantined()
    {
        var locked = Path.Join(starting, "locked");
        Directory.CreateDirectory(locked);
        var file = Path.Join(locked, "data.json");
        File.WriteAllText(file, "{}");
        if (OperatingSystem.IsWindows())
        {
            // Mark of the Web, as a ZIP extracted from a download carries it.
            File.WriteAllText(file + ":Zone.Identifier", "[ZoneTransfer]\r\nZoneId=3\r\n");
            Assert.True(File.Exists(file + ":Zone.Identifier"));
            File.SetAttributes(file, FileAttributes.ReadOnly);
            File.SetAttributes(locked, FileAttributes.Directory | FileAttributes.ReadOnly);
        }
        else
        {
            File.SetUnixFileMode(locked, UnixFileMode.UserRead | UnixFileMode.UserExecute
                                         | UnixFileMode.GroupRead | UnixFileMode.GroupExecute
                                         | UnixFileMode.OtherRead | UnixFileMode.OtherExecute);
        }

        Home.Prepare(starting, home);
        var copied = Path.Join(home, "locked");
        Assert.False(File.GetAttributes(copied).HasFlag(FileAttributes.ReadOnly));
        Assert.False(File.GetAttributes(Path.Join(copied, "data.json")).HasFlag(FileAttributes.ReadOnly));
        if (OperatingSystem.IsWindows())
            Assert.False(File.Exists(Path.Join(copied, "data.json") + ":Zone.Identifier"));
        else
            Assert.True(File.GetUnixFileMode(copied).HasFlag(UnixFileMode.UserWrite));
        // The demo can write there.
        Write("{\"x\":1}", "locked/data.json");
    }

    [Fact]
    public void RoomCheck()
    {
        Home.CheckRoom(needed: 10, available: 10);
        var error = Assert.Throws<HomeException>(() => Home.CheckRoom(needed: 11, available: 10));
        Assert.Equal(HomeProblem.InsufficientSpace, error.Problem);
        Assert.Equal(11, error.Needed);
        Assert.Equal(10, error.Available);
        Assert.Equal("not enough free space for the working copy: needs 4.2 GB, 900 MB available",
                     Assert.Throws<HomeException>(() => Home.CheckRoom(4_200_000_000, 900_000_000)).Message);
    }

    [Fact]
    public void ResetRestoresStartingState()
    {
        Home.Prepare(starting, home);
        Write("edited", "gateway/config.toml");
        Write("new", "added.txt");

        var report = Home.Reset(starting, home, servicesStopped: true);
        Assert.Null(report.Leftover);
        Assert.Equal("listen = 1", Read("gateway/config.toml"));
        Assert.False(File.Exists(Path.Join(home, "added.txt")));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void ResetWithoutAnyHomeStillPrepares()
    {
        Home.Reset(starting, home, servicesStopped: true);
        Assert.True(Home.IsPrepared(starting, home));
    }

    [Fact]
    public void ResetWhoseFreshCopyFailsPutsThePreviousCopyBack()
    {
        Home.Prepare(starting, home);
        Write("edited", "gateway/config.toml");
        Home.DeleteTree(starting);

        var error = Assert.Throws<HomeException>(() => Home.Reset(starting, home, servicesStopped: true));
        Assert.Equal(HomeProblem.StartingStateMissing, error.Problem);
        Assert.Equal("edited", Read("gateway/config.toml"));
        Assert.Equal("v1", Read(".midgard-home"));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void ResetReportsAnUndeletablePreviousCopy()
    {
        Home.Prepare(starting, home);
        Write("x", "stuck");
        var hooks = new Home.Hooks
        {
            BeforeDelete = path =>
            {
                if (Path.GetFileName(path).StartsWith("Midgard.midgard-old-", StringComparison.Ordinal))
                    throw new IOException("in use");
            },
        };

        var report = Home.Reset(starting, home, hooks);
        Assert.NotNull(report.Leftover);
        Assert.StartsWith("Midgard.midgard-old-", Path.GetFileName(report.Leftover));
        Assert.Equal("in use", report.Reason);
        Assert.True(Directory.Exists(report.Leftover));
        Assert.True(Home.IsPrepared(starting, home));
        Assert.False(File.Exists(Path.Join(home, "stuck")));
    }

    [Fact]
    public void SuccessfulPrepareSweepsOldCopies()
    {
        Directory.CreateDirectory(Path.Join(Path.GetDirectoryName(home)!, "Midgard.midgard-old-BEEF"));
        Home.Prepare(starting, home);
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void VersionChangeTriggersFreshCopy()
    {
        Home.Prepare(starting, home);
        Write("edited", "gateway/config.toml");
        File.WriteAllText(Path.Join(starting, "VERSION"), "v2");
        Assert.False(Home.IsPrepared(starting, home));

        Home.Prepare(starting, home);
        Assert.Equal("listen = 1", Read("gateway/config.toml"));
        Assert.Equal("v2", Read(".midgard-home"));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void MissingStartingStateFailsWithoutMarking()
    {
        var missing = Path.Join(root, "nope");
        var error = Assert.Throws<HomeException>(() => Home.Prepare(missing, home));
        Assert.Equal(HomeProblem.StartingStateMissing, error.Problem);
        Assert.Equal(missing, error.Path);
        Assert.False(Home.IsPrepared(missing, home));
        Assert.False(Directory.Exists(home));
    }

    // Windows shell only.

    [Fact]
    public void ResetLeavesLogsAndWebViewAlone()
    {
        var app = Path.Join(root, "Local", "Midgard");
        var appHome = Path.Join(app, "Home");
        var logs = Path.Join(app, "Logs");
        var webView = Path.Join(app, "WebView");
        Directory.CreateDirectory(logs);
        Directory.CreateDirectory(Path.Join(webView, "EBWebView"));
        File.WriteAllText(Path.Join(logs, "shell.log"), "started\n");
        File.WriteAllText(Path.Join(webView, "EBWebView", "Local State"), "{}");
        Home.Prepare(starting, appHome);
        File.WriteAllText(Path.Join(appHome, "added.txt"), "demo");

        Home.Reset(starting, appHome, servicesStopped: true);
        Assert.False(File.Exists(Path.Join(appHome, "added.txt")));
        Assert.Equal("started\n", File.ReadAllText(Path.Join(logs, "shell.log")));
        Assert.Equal("{}", File.ReadAllText(Path.Join(webView, "EBWebView", "Local State")));
        Assert.Equal(new[] { "Home", "Logs", "WebView" },
                     Directory.EnumerateFileSystemEntries(app).Select(p => Path.GetFileName(p)).Order());
    }

    [Fact]
    public void InterruptedResetLeavesTheOldHomeUsable()
    {
        File.WriteAllText(Path.Join(starting, "gateway", "second.toml"), "two");
        Home.Prepare(starting, home);
        Write("edited", "gateway/config.toml");

        var error = Assert.Throws<IOException>(() => Home.Reset(starting, home, FailOnCopy(2)));
        Assert.Equal("simulated failure mid-copy", error.Message);
        Assert.True(Home.IsPrepared(starting, home));
        Assert.Equal("edited", Read("gateway/config.toml"));
        Assert.Empty(Siblings(".midgard-"));

        // And the next reset works.
        Home.Reset(starting, home, servicesStopped: true);
        Assert.Equal("listen = 1", Read("gateway/config.toml"));
    }

    [Fact]
    public void ResetRequiresTheServicesStopped()
    {
        Home.Prepare(starting, home);
        Write("edited", "gateway/config.toml");
        var error = Assert.Throws<InvalidOperationException>(() => Home.Reset(starting, home, servicesStopped: false));
        Assert.Contains("services stopped", error.Message);
        Assert.Equal("edited", Read("gateway/config.toml"));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void ResetDeletesReadOnlyFiles()
    {
        Home.Prepare(starting, home);
        Directory.CreateDirectory(Path.Join(home, "sealed"));
        Write("x", "sealed/data.bin");
        File.SetAttributes(Path.Join(home, "sealed", "data.bin"), FileAttributes.ReadOnly);
        File.SetAttributes(Path.Join(home, "sealed"), FileAttributes.Directory | FileAttributes.ReadOnly);

        var report = Home.Reset(starting, home, servicesStopped: true);
        Assert.Null(report.Leftover);
        Assert.False(Directory.Exists(Path.Join(home, "sealed")));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void ResetWithAFileStillOpenChangesNothing()
    {
        // Windows will not move a folder while a file in it is open: had a service been
        // left running, Home stays as it was.
        if (!OperatingSystem.IsWindows()) return;
        Home.Prepare(starting, home);
        Write("edited", "gateway/config.toml");
        using (new FileStream(Path.Join(home, "gateway", "config.toml"), FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            var error = Record.Exception(() => Home.Reset(starting, home, servicesStopped: true));
            Assert.True(error is IOException or UnauthorizedAccessException, $"got {error}");
        }
        Assert.Equal("edited", Read("gateway/config.toml"));
        Assert.True(Home.IsPrepared(starting, home));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void LongPaths()
    {
        if (!OperatingSystem.IsWindows()) return;
        var deep = starting;
        for (var i = 0; i < 12; i++) deep = Path.Join(deep, $"a-rather-long-folder-name-{i:00}");
        Directory.CreateDirectory(deep);
        File.WriteAllText(Path.Join(deep, "far.txt"), "far");
        var relative = Path.GetRelativePath(starting, Path.Join(deep, "far.txt"));
        Assert.True(Path.Join(home, relative).Length > 300);

        Home.Prepare(starting, home);
        Assert.Equal("far", Read(relative));
        Write("changed", relative);
        Home.Reset(starting, home, servicesStopped: true);
        Assert.Equal("far", Read(relative));
        Assert.Empty(Siblings(".midgard-"));
    }

    [Fact]
    public void InstanceLockFileIsBesideHome()
    {
        Assert.Equal(Path.Join(Path.GetDirectoryName(home)!, "Midgard.instance"), Home.InstanceLockFile(home));
        Assert.Equal(Path.Join(Path.GetDirectoryName(home)!, "Midgard.instance"),
                     Home.InstanceLockFile(home + Path.DirectorySeparatorChar));
    }
}
