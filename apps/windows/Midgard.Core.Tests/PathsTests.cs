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
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Home"), p.Home);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Logs"), p.Logs);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "WebView"), p.WebView);
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
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "WebView"), p.WebView);
    }

    [Fact]
    public void HomeOverrideMovesOnlyHome()
    {
        var p = new Paths(new Dictionary<string, string> { ["MIDGARD_APP_HOME"] = @"D:\dev\home" },
                          Bundle, LocalAppData);
        Assert.Equal(@"D:\dev\home", p.Home);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Logs"), p.Logs);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "WebView"), p.WebView);
    }

    [Fact]
    public void WebViewOverrideMovesOnlyTheProfile()
    {
        var p = new Paths(new Dictionary<string, string> { ["MIDGARD_APP_WEBVIEW"] = @"D:\tmp\WebView" },
                          Bundle, LocalAppData);
        Assert.Equal(@"D:\tmp\WebView", p.WebView);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Home"), p.Home);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Logs"), p.Logs);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "settings.json"), p.Settings);
        Assert.Contains("MIDGARD_APP_WEBVIEW", Paths.EnvironmentKeys);
        // An empty value is no override.
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "WebView"),
                     new Paths(new Dictionary<string, string> { ["MIDGARD_APP_WEBVIEW"] = "" }, Bundle, LocalAppData).WebView);
    }

    [Fact]
    public void LogsOverrideMovesOnlyLogs()
    {
        var p = new Paths(new Dictionary<string, string> { ["MIDGARD_APP_LOGS"] = @"D:\dev\logs" },
                          Bundle, LocalAppData);
        Assert.Equal(@"D:\dev\logs", p.Logs);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Home"), p.Home);
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "WebView"), p.WebView);
    }

    [Fact]
    public void HomeLogsAndWebViewAreSeparateFolders()
    {
        // Reset demo wipes Home: the logs and the browser profile must not be inside it.
        var p = new Paths(NoEnv, Bundle, LocalAppData);
        var home = p.Home + Path.DirectorySeparatorChar;
        Assert.False(p.Logs.StartsWith(home, StringComparison.OrdinalIgnoreCase));
        Assert.False(p.WebView.StartsWith(home, StringComparison.OrdinalIgnoreCase));
        Assert.NotEqual(p.Home, p.Logs);
        Assert.NotEqual(p.Home, p.WebView);
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
        Assert.Equal(Path.Join(LocalAppData, "Midgard", "Home"), p.Home);
    }

    [Fact]
    public void CurrentUsesTheRealLocalAppDataAndTheExeFolder()
    {
        var p = Paths.Current(NoEnv);
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        Assert.Equal(Path.Join(local, "Midgard", "Home"), p.Home);
        Assert.Equal(Path.Join(AppContext.BaseDirectory, "resources"), p.Resources);
        Assert.Equal(Path.Join(AppContext.BaseDirectory, "helpers"), p.Helpers);
        Assert.False(p.DevLayout);
    }
}
