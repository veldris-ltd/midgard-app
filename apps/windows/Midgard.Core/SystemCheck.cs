using System.Globalization;
using System.Runtime.InteropServices;

namespace Midgard.Core;

/// <summary>
/// What <see cref="SystemCheck"/> needs to know about the PC. The Windows-only readings
/// (installed memory, the WebView2 runtime) come from the window app, so this library
/// and its tests stay free of Windows-only APIs.
/// </summary>
/// <param name="OsVersion">Windows' version, e.g. 10.0.22631 for Windows 11 23H2.</param>
/// <param name="OsArchitecture">The processor Windows runs on.</param>
/// <param name="ProcessArchitecture">What this process runs as.</param>
/// <param name="MemoryBytes">The memory installed in the PC.</param>
/// <param name="Drive">The working copy's drive, e.g. <c>C:\</c>.</param>
/// <param name="FreeDiskBytes">Free space on that drive, or null when it cannot be read.</param>
/// <param name="WebView2Version">The WebView2 runtime's version, or null or empty when there is none.</param>
public sealed record SystemFacts(
    Version OsVersion,
    Architecture OsArchitecture,
    Architecture ProcessArchitecture,
    ulong MemoryBytes,
    string Drive,
    ulong? FreeDiskBytes,
    string? WebView2Version)
{
    /// <summary>
    /// The facts this library can read on any OS: Windows' version, the architectures, and
    /// the free space on <paramref name="home"/>'s drive. The memory and WebView2 version
    /// are supplied by the caller.
    /// </summary>
    public static SystemFacts Gather(string home, ulong memoryBytes, string? webView2Version)
    {
        var drive = Path.GetPathRoot(Path.GetFullPath(home)) ?? home;
        ulong? free = null;
        try
        {
            free = (ulong)new DriveInfo(drive).AvailableFreeSpace;
        }
        catch (Exception e) when (e is ArgumentException or IOException or UnauthorizedAccessException)
        {
            // A network path or a drive that cannot be read: not checked.
        }
        return new SystemFacts(Environment.OSVersion.Version, RuntimeInformation.OSArchitecture,
                               RuntimeInformation.ProcessArchitecture, memoryBytes, drive, free,
                               webView2Version);
    }
}

/// <summary>
/// The minimum PC the demo runs well on, checked before anything starts so the presenter
/// gets one clear sentence rather than services failing one by one. The port of
/// apps/mac/Sources/MidgardShellCore/SystemCheck.swift.
/// </summary>
public static class SystemCheck
{
    /// <summary>Windows 11's first build.</summary>
    internal const int MinimumBuild = 22000;
    /// <summary>8 GB PCs report 8 × 1024³ bytes installed.</summary>
    internal const ulong MinimumMemory = 8UL * 1024 * 1024 * 1024;
    /// <summary>Room for the working copy, the logs and the WebView2 profile.</summary>
    internal const ulong MinimumFreeDisk = 4UL * 1024 * 1024 * 1024;

    /// <summary>null when this PC is supported, else a short message for the presenter.</summary>
    public static string? Problem(SystemFacts facts)
    {
        var os = facts.OsVersion;
        if (os.Major < 10 || (os.Major == 10 && os.Build < MinimumBuild))
        {
            var name = os.Major == 10 ? "Windows 10" : $"Windows {os.Major}.{os.Minor}";
            return $"Midgard needs Windows 11 or later; this PC runs {name} (build {os.Build}).";
        }
        if (facts.OsArchitecture != Architecture.X64)
        {
            return "Midgard needs a PC with an Intel or AMD (x64) processor; "
                 + $"this PC has an {Name(facts.OsArchitecture)} processor.";
        }
        if (facts.ProcessArchitecture != Architecture.X64)
        {
            return $"Midgard must run as an x64 app; this copy runs as {Name(facts.ProcessArchitecture)}.";
        }
        if (facts.MemoryBytes < MinimumMemory)
        {
            return $"Midgard needs at least 8 GB of memory; this PC has {Gigabytes(facts.MemoryBytes, 8)} GB.";
        }
        if (facts.FreeDiskBytes is { } free && free < MinimumFreeDisk)
        {
            return $"Midgard needs at least 4 GB of free disk space on {DriveName(facts.Drive)}; "
                 + $"this PC has {Gigabytes(free, 4)} GB free.";
        }
        if (string.IsNullOrWhiteSpace(facts.WebView2Version) || facts.WebView2Version == "0.0.0.0")
        {
            return "Midgard needs the Microsoft Edge WebView2 Runtime, which this PC does not have. "
                 + "Windows Update installs it, or download it from Microsoft.";
        }
        return null;
    }

    /// <summary>
    /// <paramref name="bytes"/> in GB to the nearest tenth, but never shown as
    /// <paramref name="minimum"/> when it is below it ("7.9", "3", "0.5").
    /// </summary>
    static string Gigabytes(ulong bytes, int minimum)
    {
        var gb = bytes / (1024.0 * 1024 * 1024);
        var shown = Math.Min(Math.Round(gb * 10, MidpointRounding.AwayFromZero) / 10, minimum - 0.1);
        var text = shown.ToString("0.0", CultureInfo.InvariantCulture);
        return text.EndsWith(".0", StringComparison.Ordinal) ? text[..^2] : text;
    }

    static string Name(Architecture a) => a switch
    {
        Architecture.X86 => "x86",
        Architecture.Arm => "ARM",
        Architecture.Arm64 => "ARM64",
        _ => a.ToString(),
    };

    /// <summary>"drive C:" for <c>C:\</c>, else the root as given.</summary>
    static string DriveName(string root)
    {
        var trimmed = root.TrimEnd('\\', '/');
        return trimmed.Length == 2 && trimmed[1] == ':' ? $"drive {char.ToUpperInvariant(trimmed[0])}:" : root;
    }
}
