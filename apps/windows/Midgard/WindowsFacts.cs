using System.Runtime.InteropServices;
using Microsoft.Win32;
using Midgard.Core;

namespace Midgard;

/// <summary>The Windows-only readings SystemCheck needs, gathered for this PC.</summary>
static partial class WindowsFacts
{
    /// <summary>The facts for this PC, with the working copy at <paramref name="home"/>.</summary>
    public static SystemFacts ForThisPc(string home) =>
        SystemFacts.Gather(home, InstalledMemoryBytes(), WebView2Version());

    /// <summary>
    /// The memory installed in the PC, as the firmware reports it: an 8 GB PC reports
    /// 8 GB here even when the graphics reserve some, which the total Windows can use
    /// (GlobalMemoryStatusEx) would not. Falls back to the memory .NET sees.
    /// </summary>
    public static ulong InstalledMemoryBytes()
    {
        if (GetPhysicallyInstalledSystemMemory(out var kilobytes) && kilobytes > 0)
            return kilobytes * 1024;
        return (ulong)GC.GetGCMemoryInfo().TotalAvailableMemoryBytes;
    }

    [LibraryImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static partial bool GetPhysicallyInstalledSystemMemory(out ulong totalMemoryInKilobytes);

    /// <summary>The Evergreen WebView2 runtime's client id in EdgeUpdate.</summary>
    const string WebView2Client = @"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

    /// <summary>
    /// The installed WebView2 runtime's version, or null. Read from the registry keys
    /// Microsoft documents for detecting the runtime: per machine (under WOW6432Node on
    /// 64-bit Windows), then per user.
    /// </summary>
    public static string? WebView2Version()
    {
        string?[] found =
        [
            Read(Registry.LocalMachine, @"SOFTWARE\WOW6432Node\" + WebView2Client),
            Read(Registry.LocalMachine, @"SOFTWARE\" + WebView2Client),
            Read(Registry.CurrentUser, @"Software\" + WebView2Client),
        ];
        return found.FirstOrDefault(v => !string.IsNullOrWhiteSpace(v) && v != "0.0.0.0");

        static string? Read(RegistryKey root, string path)
        {
            try
            {
                using var key = root.OpenSubKey(path);
                return key?.GetValue("pv") as string;
            }
            catch (Exception e) when (e is System.Security.SecurityException or UnauthorizedAccessException or IOException)
            {
                return null;
            }
        }
    }
}
