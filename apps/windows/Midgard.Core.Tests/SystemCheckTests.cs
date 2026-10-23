using System.Runtime.InteropServices;
using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of apps/mac/Tests/MidgardShellCoreTests/SystemCheckTests.swift, plus the
// checks only Windows has: the processor, the free disk and the WebView2 runtime.
public class SystemCheckTests
{
    const ulong Gib = 1024UL * 1024 * 1024;

    static SystemFacts Pc(int build = 22631, ulong memory = 16 * Gib,
                          Architecture os = Architecture.X64, Architecture process = Architecture.X64,
                          ulong? freeDisk = 100 * Gib, string? webView2 = "130.0.2849.80",
                          string drive = @"C:\") =>
        new(new Version(10, 0, build), os, process, memory, drive, freeDisk, webView2);

    [Fact]
    public void SupportedPc()
    {
        Assert.Null(SystemCheck.Problem(Pc(build: 22000, memory: 8 * Gib)));
        Assert.Null(SystemCheck.Problem(Pc(build: 22631, memory: 8 * Gib)));
        Assert.Null(SystemCheck.Problem(Pc(build: 26100, memory: 8 * Gib)));
        Assert.Null(SystemCheck.Problem(Pc(build: 26200, memory: 16 * Gib, freeDisk: 4 * Gib)));
    }

    [Fact]
    public void OldWindows()
    {
        Assert.Equal("Midgard needs Windows 11 or later; this PC runs Windows 10 (build 19045).",
                     SystemCheck.Problem(Pc(build: 19045)));
        Assert.Equal("Midgard needs Windows 11 or later; this PC runs Windows 6.3 (build 9600).",
                     SystemCheck.Problem(Pc() with { OsVersion = new Version(6, 3, 9600) }));
    }

    [Fact]
    public void BuildNumberCounts()
    {
        // Windows 10 and 11 are both 10.0: the build tells them apart, and 11 starts at 22000.
        Assert.Equal("Midgard needs Windows 11 or later; this PC runs Windows 10 (build 21999).",
                     SystemCheck.Problem(Pc(build: 21999)));
        Assert.Equal("Midgard needs Windows 11 or later; this PC runs Windows 10 (build 20348).",
                     SystemCheck.Problem(Pc(build: 20348)));
    }

    [Fact]
    public void TooLittleMemory()
    {
        var bytes = (ulong)(7.9 * Gib);
        Assert.Equal("Midgard needs at least 8 GB of memory; this PC has 7.9 GB.",
                     SystemCheck.Problem(Pc(memory: bytes)));
        Assert.Equal("Midgard needs at least 8 GB of memory; this PC has 7.9 GB.",
                     SystemCheck.Problem(Pc(memory: 8 * Gib - 1)));
        Assert.Equal("Midgard needs at least 8 GB of memory; this PC has 4 GB.",
                     SystemCheck.Problem(Pc(memory: 4 * Gib)));
    }

    [Fact]
    public void OsReportedFirstWhenBothFail()
    {
        Assert.Equal("Midgard needs Windows 11 or later; this PC runs Windows 10 (build 19045).",
                     SystemCheck.Problem(Pc(build: 19045, memory: 4 * Gib, os: Architecture.Arm64,
                                            freeDisk: 1 * Gib, webView2: null)));
    }

    [Fact]
    public void ArmPcIsRefused()
    {
        // An x64 build runs emulated on an ARM PC, which is not supported.
        Assert.Equal("Midgard needs a PC with an Intel or AMD (x64) processor; this PC has an ARM64 processor.",
                     SystemCheck.Problem(Pc(os: Architecture.Arm64)));
        Assert.Equal("Midgard needs a PC with an Intel or AMD (x64) processor; this PC has an x86 processor.",
                     SystemCheck.Problem(Pc(os: Architecture.X86, process: Architecture.X86)));
    }

    [Fact]
    public void ProcessMustBeX64()
    {
        Assert.Equal("Midgard must run as an x64 app; this copy runs as x86.",
                     SystemCheck.Problem(Pc(process: Architecture.X86)));
    }

    [Fact]
    public void TooLittleFreeDisk()
    {
        Assert.Equal("Midgard needs at least 4 GB of free disk space on drive C:; this PC has 2.5 GB free.",
                     SystemCheck.Problem(Pc(freeDisk: (ulong)(2.5 * Gib))));
        Assert.Equal("Midgard needs at least 4 GB of free disk space on drive D:; this PC has 3.9 GB free.",
                     SystemCheck.Problem(Pc(freeDisk: 4 * Gib - 1, drive: @"d:\")));
        Assert.Equal(@"Midgard needs at least 4 GB of free disk space on \\server\share\; this PC has 0 GB free.",
                     SystemCheck.Problem(Pc(freeDisk: 0, drive: @"\\server\share\")));
    }

    [Fact]
    public void UnreadableDiskIsNotChecked()
    {
        Assert.Null(SystemCheck.Problem(Pc(freeDisk: null)));
    }

    [Fact]
    public void NoWebView2Runtime()
    {
        const string message = "Midgard needs the Microsoft Edge WebView2 Runtime, which this PC does not have. "
                             + "Windows Update installs it, or download it from Microsoft.";
        Assert.Equal(message, SystemCheck.Problem(Pc(webView2: null)));
        Assert.Equal(message, SystemCheck.Problem(Pc(webView2: "")));
        Assert.Equal(message, SystemCheck.Problem(Pc(webView2: "0.0.0.0")));
    }

    [Fact]
    public void GatherReadsThisMachine()
    {
        var home = Path.Join(Path.GetTempPath(), "midgard-not-made");
        var facts = SystemFacts.Gather(home, 16 * Gib, "1.0");
        Assert.Equal(Path.GetPathRoot(Path.GetFullPath(home)), facts.Drive);
        Assert.NotNull(facts.FreeDiskBytes);
        Assert.Equal(RuntimeInformation.ProcessArchitecture, facts.ProcessArchitecture);
        Assert.Equal(16 * Gib, facts.MemoryBytes);
        Assert.Equal("1.0", facts.WebView2Version);
    }

    [Fact]
    public void ThisPcPasses()
    {
        // Exercises the real readings (memory and WebView2 aside, which the window app reads);
        // meaningful only on a Windows x64 PC at or above the minimum, such as the CI runner.
        var facts = SystemFacts.Gather(Path.GetTempPath(), 16 * Gib, "1.0");
        if (!OperatingSystem.IsWindows() || facts.OsVersion.Build < 22000
            || facts.OsArchitecture != Architecture.X64 || facts.FreeDiskBytes < 4 * Gib)
            return;
        Assert.Null(SystemCheck.Problem(facts));
    }
}
