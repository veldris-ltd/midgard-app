using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of apps/mac/Tests/MidgardShellCoreTests/PathsTests.swift.
public class PathsTests
{
    const string Exe = @"C:\Users\presenter\Downloads\Midgard";
    static readonly string Bundle = Path.Join(Exe, "resources");
    static readonly string BundleHelpers = Path.Join(Exe, "helpers");
    const string LocalAppData = @"C:\Users\presenter\AppData\Local";
    static readonly Dictionary<string, string> NoEnv = new();

    [Fact]
    public void Defaults()
    {
        var p = new Paths(NoEnv, Bundle, LocalAppData);
        Assert.Equal(Bundle, p.Resources);
        Assert.Equal(Path.Join(LocalAppData, "Midgard"), p.Home);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Logs"), p.Logs);
    }

    [Fact]
    public void Overrides()
    {
        var env = new Dictionary<string, string>
        {
            ["MIDGARD_APP_RESOURCES"] = @"D:\dev\res",
            ["MIDGARD_APP_HOME"] = @"D:\dev\home",
            ["MIDGARD_APP_LOGS"] = @"D:\dev\logs",
        };
        var p = new Paths(env, Bundle, LocalAppData);
        Assert.Equal(@"D:\dev\res", p.Resources);
        Assert.Equal(@"D:\dev\home", p.Home);
        Assert.Equal(@"D:\dev\logs", p.Logs);
    }

    [Fact]
    public void HomeOverrideKeepsDefaultLogs()
    {
        var p = new Paths(new Dictionary<string, string> { ["MIDGARD_APP_HOME"] = @"D:\dev\home" },
                          Bundle, LocalAppData);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Logs"), p.Logs);
    }

    [Fact]
    public void HelpersOutsideAPackage()
    {
        var p = new Paths(NoEnv, Bundle, LocalAppData);
        Assert.Equal(Path.Join(Bundle, "bin"), p.Helpers);
        Assert.True(p.DevLayout);
    }

    [Fact]
    public void HelpersInAPackage()
    {
        var p = new Paths(NoEnv, Bundle, LocalAppData, BundleHelpers);
        Assert.Equal(BundleHelpers, p.Helpers);
        Assert.False(p.DevLayout);
    }

    [Fact]
    public void DevelopmentResourcesKeepBin()
    {
        var p = new Paths(new Dictionary<string, string> { ["MIDGARD_APP_RESOURCES"] = @"D:\dev\res" },
                          Bundle, LocalAppData, BundleHelpers);
        Assert.Equal(Path.Join(@"D:\dev\res", "bin"), p.Helpers);
        Assert.True(p.DevLayout);
    }

    [Fact]
    public void EmptyOverrideIsIgnored()
    {
        var p = new Paths(new Dictionary<string, string> { ["MIDGARD_APP_HOME"] = "" }, Bundle, LocalAppData);
        Assert.Equal(Path.Join(LocalAppData, "Midgard"), p.Home);
    }

    [Fact]
    public void CurrentUsesTheRealLocalAppDataAndTheExeFolder()
    {
        var p = Paths.Current(NoEnv);
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        Assert.Equal(Path.Join(local, "Midgard"), p.Home);
        Assert.Equal(Path.Join(AppContext.BaseDirectory, "resources"), p.Resources);
        Assert.Equal(Path.Join(AppContext.BaseDirectory, "helpers"), p.Helpers);
        Assert.False(p.DevLayout);
    }
}
