using System.Runtime.Versioning;
using System.Security.AccessControl;
using System.Security.Principal;

namespace Midgard.Core;

/// <summary>Reads a folder's access list for <see cref="FolderAccess"/>. Windows only.</summary>
[SupportedOSPlatform("windows")]
public static class WindowsAcl
{
    /// <summary>
    /// The allow and deny entries of <paramref name="folder"/>'s DACL, inherited ones
    /// included. A folder with no DACL at all (everyone has every right) reads as one entry
    /// giving Everyone full access.
    /// </summary>
    public static IReadOnlyList<AccessEntry> Entries(string folder)
    {
        var security = new DirectoryInfo(folder).GetAccessControl(AccessControlSections.Access);
        var sddl = security.GetSecurityDescriptorSddlForm(AccessControlSections.Access);
        if (sddl.Contains("NO_ACCESS_CONTROL", StringComparison.Ordinal))
            return [new AccessEntry(FolderAccess.Everyone, true, uint.MaxValue)];
        var list = new List<AccessEntry>();
        foreach (FileSystemAccessRule rule in security.GetAccessRules(true, true, typeof(SecurityIdentifier)))
        {
            list.Add(new AccessEntry(((SecurityIdentifier)rule.IdentityReference).Value,
                                     rule.AccessControlType == AccessControlType.Allow,
                                     unchecked((uint)rule.FileSystemRights)));
        }
        return list;
    }
}
