using Midgard.Core;

namespace Midgard.Core.Tests;

// The quoting the C runtime (and CommandLineToArgvW, and Rust's std::env::args) reads back.
// SupervisorTests.ReadyByLogLineWithItsArgumentsAsGiven checks the round trip in a real process.
public sealed class CommandLineTests
{
    [Theory]
    [InlineData("plain", "plain")]
    [InlineData("", "\"\"")]
    [InlineData("two words", "\"two words\"")]
    [InlineData("C:\\Program Files\\x\\", "\"C:\\Program Files\\x\\\\\"")]
    [InlineData("C:\\no\\spaces\\", "C:\\no\\spaces\\")]
    [InlineData("say \"hi\"", "\"say \\\"hi\\\"\"")]
    [InlineData("a\\\"b", "\"a\\\\\\\"b\"")]
    [InlineData("{\"type\":\"object\"}", "\"{\\\"type\\\":\\\"object\\\"}\"")]
    [InlineData("tab\there", "\"tab\there\"")]
    public void QuotesEachArgument(string argument, string quoted) =>
        Assert.Equal($"\"C:\\x.exe\" {quoted}", CommandLine.Build("C:\\x.exe", [argument]));

    [Fact]
    public void ProgramNameIsQuotedWithoutEscapes() =>
        Assert.Equal("\"C:\\Program Files\\Midgard\\helpers\\http-gateway.exe\" --bind 127.0.0.1:1",
                     CommandLine.Build("C:\\Program Files\\Midgard\\helpers\\http-gateway.exe", ["--bind", "127.0.0.1:1"]));

    [Fact]
    public void EnvironmentBlockIsSortedWithExtrasOnTop()
    {
        var block = CommandLine.EnvironmentBlock(
            new Dictionary<string, string> { ["Path"] = "C:\\bin", ["b"] = "1", ["A"] = "0" },
            new Dictionary<string, string> { ["PATH"] = "C:\\other", ["c"] = "2" });
        Assert.Equal("A=0\0b=1\0c=2\0Path=C:\\other\0\0", block);
        Assert.Equal("\0\0", CommandLine.EnvironmentBlock(new Dictionary<string, string>(), new Dictionary<string, string>()));
    }
}
