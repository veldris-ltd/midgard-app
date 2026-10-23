using Midgard.Core;

namespace Midgard.Core.Tests;

public class HeadlessGuardTests
{
    static string? Refusal(params (string Key, string Value)[] env) =>
        HeadlessGuard.Refusal(env.ToDictionary(e => e.Key, e => e.Value));

    [Fact]
    public void RunsOnlyOptedInWithAThrowAwayHomeAndLogs()
    {
        Assert.Null(Refusal(("MIDGARD_HEADLESS_CHECK", "1"), ("MIDGARD_APP_HOME", @"D:\tmp\Home"), ("MIDGARD_APP_LOGS", @"D:\tmp\Logs"),
                            ("MIDGARD_APP_WEBVIEW", @"D:\tmp\WebView")));
    }

    [Fact]
    public void WithoutTheOptInItIsRefused()
    {
        var why = Refusal(("MIDGARD_APP_HOME", @"D:\tmp\Home"), ("MIDGARD_APP_LOGS", @"D:\tmp\Logs"), ("MIDGARD_APP_WEBVIEW", "w"));
        Assert.Contains("MIDGARD_HEADLESS_CHECK=1", why);
        Assert.NotNull(Refusal(("MIDGARD_HEADLESS_CHECK", "true"), ("MIDGARD_APP_HOME", "h"), ("MIDGARD_APP_LOGS", "l")));
        Assert.NotNull(Refusal());
    }

    [Theory]
    [InlineData("", "l", "w", "MIDGARD_APP_HOME")]
    [InlineData("h", " ", "w", "MIDGARD_APP_LOGS")]
    [InlineData("h", "l", null, "MIDGARD_APP_WEBVIEW")]
    [InlineData(null, null, "w", "MIDGARD_APP_HOME and MIDGARD_APP_LOGS")]
    [InlineData(null, null, null, "MIDGARD_APP_HOME, MIDGARD_APP_LOGS and MIDGARD_APP_WEBVIEW")]
    public void WithoutExplicitFoldersItIsRefused(string? home, string? logs, string? webView, string named)
    {
        var env = new Dictionary<string, string> { ["MIDGARD_HEADLESS_CHECK"] = "1" };
        if (home is not null) env["MIDGARD_APP_HOME"] = home;
        if (logs is not null) env["MIDGARD_APP_LOGS"] = logs;
        if (webView is not null) env["MIDGARD_APP_WEBVIEW"] = webView;
        var why = HeadlessGuard.Refusal(env);
        Assert.NotNull(why);
        Assert.Contains($"needs {named} set", why);
    }
}
