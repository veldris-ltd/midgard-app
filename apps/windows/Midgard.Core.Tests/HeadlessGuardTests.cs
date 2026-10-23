using Midgard.Core;

namespace Midgard.Core.Tests;

public class HeadlessGuardTests
{
    static string? Refusal(params (string Key, string Value)[] env) =>
        HeadlessGuard.Refusal(env.ToDictionary(e => e.Key, e => e.Value));

    [Fact]
    public void RunsOnlyOptedInWithAThrowAwayHomeAndLogs()
    {
        Assert.Null(Refusal(("MIDGARD_HEADLESS_CHECK", "1"), ("MIDGARD_APP_HOME", @"D:\tmp\Home"), ("MIDGARD_APP_LOGS", @"D:\tmp\Logs")));
    }

    [Fact]
    public void WithoutTheOptInItIsRefused()
    {
        var why = Refusal(("MIDGARD_APP_HOME", @"D:\tmp\Home"), ("MIDGARD_APP_LOGS", @"D:\tmp\Logs"));
        Assert.Contains("MIDGARD_HEADLESS_CHECK=1", why);
        Assert.NotNull(Refusal(("MIDGARD_HEADLESS_CHECK", "true"), ("MIDGARD_APP_HOME", "h"), ("MIDGARD_APP_LOGS", "l")));
        Assert.NotNull(Refusal());
    }

    [Theory]
    [InlineData("", "l", "MIDGARD_APP_HOME")]
    [InlineData("h", " ", "MIDGARD_APP_LOGS")]
    [InlineData(null, null, "MIDGARD_APP_HOME and MIDGARD_APP_LOGS")]
    public void WithoutExplicitHomeAndLogsItIsRefused(string? home, string? logs, string named)
    {
        var env = new Dictionary<string, string> { ["MIDGARD_HEADLESS_CHECK"] = "1" };
        if (home is not null) env["MIDGARD_APP_HOME"] = home;
        if (logs is not null) env["MIDGARD_APP_LOGS"] = logs;
        var why = HeadlessGuard.Refusal(env);
        Assert.NotNull(why);
        Assert.Contains($"needs {named} set", why);
    }
}
