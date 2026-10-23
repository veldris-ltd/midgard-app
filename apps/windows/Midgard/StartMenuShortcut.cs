using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;

namespace Midgard;

/// <summary>
/// The Start-menu shortcut the first run offers: <c>Midgard.lnk</c> in the user's
/// <c>%APPDATA%\Microsoft\Windows\Start Menu\Programs</c>, pointing at this Midgard.exe. Made
/// with the shell's own IShellLink COM object, so no script host is involved.
/// </summary>
static class StartMenuShortcut
{
    /// <summary>Where the shortcut goes.</summary>
    public static string File =>
        Path.Join(Environment.GetFolderPath(Environment.SpecialFolder.Programs), "Midgard.lnk");

    public static bool Exists() => System.IO.File.Exists(File);

    /// <summary>Makes (or replaces) the shortcut to <paramref name="target"/>; throws if Windows refuses.</summary>
    public static void Create(string target)
    {
        var link = (IShellLinkW)new ShellLink();
        try
        {
            link.SetPath(target);
            link.SetWorkingDirectory(Path.GetDirectoryName(target) ?? "");
            link.SetDescription("Midgard presentation app");
            link.SetIconLocation(target, 0);
            Directory.CreateDirectory(Path.GetDirectoryName(File)!);
            ((IPersistFile)link).Save(File, true);
        }
        finally
        {
            Marshal.FinalReleaseComObject(link);
        }
    }

    [ComImport]
    [Guid("00021401-0000-0000-C000-000000000046")]
    class ShellLink { }

    /// <summary>IShellLinkW, in its vtable order (ShObjIdl_core.h).</summary>
    [ComImport]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    [Guid("000214F9-0000-0000-C000-000000000046")]
    interface IShellLinkW
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder file, int size, nint findData, uint flags);
        void GetIDList(out nint idList);
        void SetIDList(nint idList);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder name, int size);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string name);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder dir, int size);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string dir);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder args, int size);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string args);
        void GetHotkey(out short hotkey);
        void SetHotkey(short hotkey);
        void GetShowCmd(out int showCmd);
        void SetShowCmd(int showCmd);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string path, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved);
        void Resolve(nint window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string file);
    }
}
