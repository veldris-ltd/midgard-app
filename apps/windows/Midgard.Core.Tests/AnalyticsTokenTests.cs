using System.Runtime.Versioning;
using System.Security.AccessControl;
using System.Security.Principal;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The token cases of apps/mac/Tests/MidgardShellCoreTests/ServiceSpecTests.swift, with the
// file's own DACL for mode 0600. Windows only: each test returns at once elsewhere.
[SupportedOSPlatform("windows")]
public sealed class AnalyticsTokenTests : IDisposable
{
    readonly string dir = Path.Join(Path.GetTempPath(), $"midgard-token-{Guid.NewGuid():N}");
    string Token => Path.Join(dir, "heimdall-poc", "analytics", "secret");

    public void Dispose()
    {
        try
        {
            Directory.Delete(dir, recursive: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    /// <summary>The file's DACL: protected, with one allow entry, this user's full control.</summary>
    static void AssertPrivate(string file)
    {
        var security = new FileInfo(file).GetAccessControl(AccessControlSections.Access);
        Assert.True(security.AreAccessRulesProtected, "the DACL still inherits");
        var rules = security.GetAccessRules(true, true, typeof(SecurityIdentifier)).Cast<FileSystemAccessRule>().ToList();
        var rule = Assert.Single(rules);
        Assert.Equal(WindowsIdentity.GetCurrent().User!.Value, ((SecurityIdentifier)rule.IdentityReference).Value);
        Assert.Equal(AccessControlType.Allow, rule.AccessControlType);
        Assert.Equal(FileSystemRights.FullControl, rule.FileSystemRights & FileSystemRights.FullControl);
    }

    [Fact]
    public void CreatesA48HexDigitTokenOnlyThisUserCanRead()
    {
        if (!OperatingSystem.IsWindows()) return;
        AnalyticsToken.Ensure(Token);
        var text = File.ReadAllText(Token);
        Assert.Matches("^[0-9a-f]{48}\n$", text);
        AssertPrivate(Token);
        // Another call keeps it.
        AnalyticsToken.Ensure(Token);
        Assert.Equal(text, File.ReadAllText(Token));
        // A fresh one differs.
        File.Delete(Token);
        AnalyticsToken.Ensure(Token);
        Assert.NotEqual(text, File.ReadAllText(Token));
    }

    [Fact]
    public void KeepsAnExistingTokenAndMakesItPrivate()
    {
        if (!OperatingSystem.IsWindows()) return;
        Directory.CreateDirectory(Path.GetDirectoryName(Token)!);
        File.WriteAllText(Token, "kept-token\n");
        Assert.False(new FileInfo(Token).GetAccessControl().AreAccessRulesProtected);
        AnalyticsToken.Ensure(Token);
        Assert.Equal("kept-token\n", File.ReadAllText(Token));
        AssertPrivate(Token);
    }

    [Fact]
    public void ReplacesAnEmptyFile()
    {
        if (!OperatingSystem.IsWindows()) return;
        Directory.CreateDirectory(Path.GetDirectoryName(Token)!);
        File.WriteAllText(Token, " \r\n");
        AnalyticsToken.Ensure(Token);
        Assert.Matches("^[0-9a-f]{48}\n$", File.ReadAllText(Token));
        AssertPrivate(Token);
    }

    /// <summary>A symbolic link is replaced, its target untouched (skipped where links cannot be made).</summary>
    [Fact]
    public void ReplacesALinkWithoutTouchingItsTarget()
    {
        if (!OperatingSystem.IsWindows()) return;
        Directory.CreateDirectory(Path.GetDirectoryName(Token)!);
        var target = Path.Join(dir, "elsewhere.txt");
        File.WriteAllText(target, "not a token of ours\n");
        try
        {
            File.CreateSymbolicLink(Token, target);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return;  // no SeCreateSymbolicLinkPrivilege and no developer mode
        }
        AnalyticsToken.Ensure(Token);
        Assert.Equal("not a token of ours\n", File.ReadAllText(target));
        Assert.False(File.GetAttributes(Token).HasFlag(FileAttributes.ReparsePoint));
        Assert.Matches("^[0-9a-f]{48}\n$", File.ReadAllText(Token));
        AssertPrivate(Token);
    }

    [Fact]
    public void RefusesAFolder()
    {
        if (!OperatingSystem.IsWindows()) return;
        Directory.CreateDirectory(Token);
        Assert.Throws<AnalyticsTokenException>(() => AnalyticsToken.Ensure(Token));
        Assert.True(Directory.Exists(Token));
    }
}
