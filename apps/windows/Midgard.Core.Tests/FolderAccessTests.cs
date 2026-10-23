using System.Security.AccessControl;
using System.Security.Principal;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The ACL check on the app's folders: the decision, then (Windows only) real folders.
public class FolderAccessTests
{
    const string Me = "S-1-5-21-1004336348-1177238915-682003330-1001";
    const string LocalSystem = "S-1-5-18";
    const string Administrators = "S-1-5-32-544";
    const uint FullControl = 0x1F01FF;
    const uint ReadAndExecute = 0x1200A9;
    const uint Modify = 0x1301BF;
    const string Folder = @"C:\Users\presenter\AppData\Local\Midgard";

    /// <summary>What %LOCALAPPDATA% has by default: the user, SYSTEM and Administrators.</summary>
    static readonly AccessEntry[] Default =
        [new(Me, true, FullControl), new(LocalSystem, true, FullControl), new(Administrators, true, FullControl)];

    [Fact]
    public void DefaultLocalAppDataPasses()
    {
        Assert.Null(FolderAccess.Problem(Folder, Default));
        Assert.Null(FolderAccess.Problem(Folder, []));
    }

    [Fact]
    public void EveryoneReadIsRefused()
    {
        var problem = FolderAccess.Problem(Folder, [.. Default, new(FolderAccess.Everyone, true, ReadAndExecute)]);
        Assert.NotNull(problem);
        Assert.StartsWith($"Other people who use this PC can read {Folder}: its permissions let Everyone read it.", problem);
        Assert.Contains("Remove Everyone from the folder's Properties > Security tab", problem);
    }

    [Fact]
    public void UsersReadIsRefused()
    {
        var problem = FolderAccess.Problem(Folder, [.. Default, new(FolderAccess.Users, true, ReadAndExecute)]);
        Assert.NotNull(problem);
        Assert.Contains("let Users read it", problem);
    }

    [Fact]
    public void AuthenticatedUsersModifyIsRefused()
    {
        var problem = FolderAccess.Problem(Folder, [.. Default, new(FolderAccess.AuthenticatedUsers, true, Modify)]);
        Assert.NotNull(problem);
        Assert.Contains("let Authenticated Users read it", problem);
    }

    [Fact]
    public void SeveralGroupsAreNamedTogether()
    {
        var problem = FolderAccess.Problem(Folder,
            [new(FolderAccess.AuthenticatedUsers, true, Modify), new(FolderAccess.Users, true, ReadAndExecute),
             new(FolderAccess.Everyone, true, FolderAccess.ReadData)]);
        Assert.NotNull(problem);
        Assert.Contains("let Everyone, Users and Authenticated Users read it", problem);
    }

    [Fact]
    public void GenericRightsCount()
    {
        // Inherit-only entries are often written with generic rights.
        Assert.NotNull(FolderAccess.Problem(Folder, [new(FolderAccess.Users, true, FolderAccess.GenericRead)]));
        Assert.NotNull(FolderAccess.Problem(Folder, [new(FolderAccess.Everyone, true, FolderAccess.GenericAll)]));
    }

    [Fact]
    public void RightsWithoutReadingAndDenyEntriesPass()
    {
        const uint ReadAttributesAndSynchronize = 0x100080;
        Assert.Null(FolderAccess.Problem(Folder, [.. Default, new(FolderAccess.Everyone, true, ReadAttributesAndSynchronize)]));
        Assert.Null(FolderAccess.Problem(Folder, [.. Default, new(FolderAccess.Everyone, false, FullControl)]));
        // Another named account is not one of the three groups.
        Assert.Null(FolderAccess.Problem(Folder, [.. Default, new("S-1-5-21-1-2-3-1002", true, ReadAndExecute)]));
    }

    [Fact]
    public void ChecksHomeLogsAndTheWebViewFolderOrTheirNearestExistingParent()
    {
        const string Local = @"C:\Users\presenter\AppData\Local";
        var paths = new Paths(new Dictionary<string, string> { ["MIDGARD_APP_LOGS"] = @"D:\logs\midgard" },
                              @"C:\Midgard\resources", Local);
        var existing = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
            { @"C:\", @"C:\Users", @"C:\Users\presenter", @"C:\Users\presenter\AppData", Local, @"D:\", @"D:\logs" };
        if (!OperatingSystem.IsWindows()) return;  // Windows paths
        Assert.Equal(new[] { Local, @"D:\logs" }, FolderAccess.FoldersToCheck(paths, existing.Contains));

        existing.Add(Local + @"\Midgard");
        existing.Add(Local + @"\Midgard\Home");
        Assert.Equal(new[] { Local + @"\Midgard\Home", @"D:\logs", Local + @"\Midgard" },
                     FolderAccess.FoldersToCheck(paths, existing.Contains));

        var read = new List<string>();
        Assert.Null(FolderAccess.Problem(paths, f => { read.Add(f); return Default; }, existing.Contains));
        Assert.Equal(3, read.Count);

        var problem = FolderAccess.Problem(paths,
            f => f.StartsWith(@"D:\", StringComparison.Ordinal)
                ? [new AccessEntry(FolderAccess.Users, true, ReadAndExecute)]
                : Default,
            existing.Contains);
        Assert.NotNull(problem);
        Assert.Contains(@"can read D:\logs", problem);
    }

    [Fact]
    public void AFolderWhoseAclCannotBeReadIsAProblem()
    {
        var paths = new Paths(new Dictionary<string, string>(), Path.Join(Path.GetTempPath(), "res"), Path.GetTempPath());
        var problem = FolderAccess.Problem(paths, _ => throw new UnauthorizedAccessException("denied"), _ => true);
        Assert.NotNull(problem);
        Assert.Contains("could not check who can read", problem);
        Assert.Contains("denied", problem);
    }

    // Real folders: Windows CI only.

    [Fact]
    public void RealDefaultFolderPassesAndEveryoneReadIsRefused()
    {
        if (!OperatingSystem.IsWindows()) return;
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        Assert.Null(FolderAccess.Problem(local, WindowsAcl.Entries(local)));

        var dir = Path.Join(local, $"midgard-acl-test-{Guid.NewGuid():N}");
        Directory.CreateDirectory(dir);
        try
        {
            // A new folder inherits %LOCALAPPDATA%'s ACL: it passes.
            Assert.Null(FolderAccess.Problem(dir, WindowsAcl.Entries(dir)));

            var info = new DirectoryInfo(dir);
            var security = info.GetAccessControl();
            security.AddAccessRule(new FileSystemAccessRule(
                new SecurityIdentifier(WellKnownSidType.WorldSid, null), FileSystemRights.ReadAndExecute,
                InheritanceFlags.ContainerInherit | InheritanceFlags.ObjectInherit, PropagationFlags.None,
                AccessControlType.Allow));
            info.SetAccessControl(security);

            var problem = FolderAccess.Problem(dir, WindowsAcl.Entries(dir));
            Assert.NotNull(problem);
            Assert.Contains("let Everyone read it", problem);

            // And a folder created inside it inherits the entry.
            var inner = Directory.CreateDirectory(Path.Join(dir, "Home")).FullName;
            Assert.NotNull(FolderAccess.Problem(inner, WindowsAcl.Entries(inner)));
        }
        finally
        {
            Directory.Delete(dir, recursive: true);
        }
    }

    [Fact]
    public void RealUsersReadIsRefused()
    {
        if (!OperatingSystem.IsWindows()) return;
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        var dir = Directory.CreateDirectory(Path.Join(local, $"midgard-acl-test-{Guid.NewGuid():N}"));
        try
        {
            var security = dir.GetAccessControl();
            security.AddAccessRule(new FileSystemAccessRule(
                new SecurityIdentifier(WellKnownSidType.BuiltinUsersSid, null), FileSystemRights.ReadData,
                AccessControlType.Allow));
            dir.SetAccessControl(security);
            Assert.Contains("let Users read it", FolderAccess.Problem(dir.FullName, WindowsAcl.Entries(dir.FullName)));
        }
        finally
        {
            dir.Delete(recursive: true);
        }
    }
}
