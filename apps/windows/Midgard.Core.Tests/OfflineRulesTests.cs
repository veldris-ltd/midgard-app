using Midgard.Core;

namespace Midgard.Core.Tests;

// The request filter and navigation decision, with the Swift OfflineRules and Navigation cases.
public class OfflineRulesTests
{
    const string App = "http://127.0.0.1:61235";
    const string Gateway = "http://127.0.0.1:7710";

    static bool Allowed(string url) => OfflineRules.IsAllowed(url, App, Gateway);

    [Theory]
    [InlineData("http://127.0.0.1:61235/")]
    [InlineData("http://127.0.0.1:61235")]
    [InlineData("http://127.0.0.1:61235/heimdall/index.html")]
    [InlineData("http://127.0.0.1:61235/midgard/index.html#/objects")]
    [InlineData("http://127.0.0.1:61235?x=1")]
    [InlineData("http://127.0.0.1:61235#top")]
    [InlineData("http://127.0.0.1:7710/api/v1/health")]
    [InlineData("HTTP://127.0.0.1:61235/home/")]
    [InlineData("about:blank")]
    [InlineData("about:srcdoc")]
    [InlineData("ABOUT:BLANK")]
    [InlineData("blob:http://127.0.0.1:61235/0b4e7a8c-1d2e-4f00-9a1b-2c3d4e5f6a7b")]
    [InlineData("data:image/png;base64,AAAA")]
    [InlineData("data:text/html,<p>hi</p>")]
    public void OwnOriginGatewayAndInPageSchemesAreAllowed(string url) => Assert.True(Allowed(url), url);

    [Theory]
    // Elsewhere.
    [InlineData("https://example.com/")]
    [InlineData("http://example.com/")]
    [InlineData("http://example.com/?http://127.0.0.1:61235/")]
    [InlineData("http://example.com/http://127.0.0.1:61235/")]
    // Other ports and loopback addresses, and https on the right port.
    [InlineData("http://127.0.0.1:9999/")]
    [InlineData("http://127.0.0.1/")]
    [InlineData("http://127.0.0.1:80/")]
    [InlineData("http://127.0.0.1:612350/")]
    [InlineData("http://127.0.0.1:061235/")]
    [InlineData("https://127.0.0.1:61235/")]
    [InlineData("http://127.0.0.2:61235/")]
    [InlineData("http://127.0.0.10:61235/")]
    [InlineData("http://localhost:61235/")]
    [InlineData("http://LOCALHOST:61235/")]
    [InlineData("http://[::1]:61235/")]
    [InlineData("http://[::ffff:127.0.0.1]:61235/")]
    [InlineData("http://0.0.0.0:61235/")]
    [InlineData("http://2130706433:61235/")]
    [InlineData("http://0x7f.0.0.1:61235/")]
    // A host that starts like the address.
    [InlineData("http://127.0.0.1.evil.example/")]
    [InlineData("http://127.0.0.1:61235.evil.example/")]
    // User info: the real host is after the @.
    [InlineData("http://127.0.0.1:61235@example.com/")]
    [InlineData("http://127.0.0.1:@example.com/")]
    [InlineData("http://user:pass@127.0.0.1:61235/")]
    [InlineData("http://user@127.0.0.1:61235/")]
    // Backslashes, which browsers read as slashes in http URLs.
    [InlineData("http://127.0.0.1:61235\\@example.com/")]
    [InlineData("http:\\\\127.0.0.1:61235/")]
    // Other schemes.
    [InlineData("ws://127.0.0.1:61235/")]
    [InlineData("wss://127.0.0.1:61235/")]
    [InlineData("ws://example.com/")]
    [InlineData("file:///C:/Windows/win.ini")]
    [InlineData("file://server/share/x")]
    [InlineData("ftp://127.0.0.1:61235/")]
    [InlineData("mailto:someone@example.com")]
    [InlineData("ms-settings:privacy")]
    [InlineData("javascript:alert(1)")]
    [InlineData("edge://settings")]
    [InlineData("about:settings")]
    [InlineData("about:version")]
    // Blobs of other origins.
    [InlineData("blob:https://example.com/0b4e7a8c")]
    [InlineData("blob:http://127.0.0.1:9999/0b4e7a8c")]
    [InlineData("blob:null/0b4e7a8c")]
    // Not URLs.
    [InlineData("")]
    [InlineData("127.0.0.1:61235/")]
    [InlineData(":61235/")]
    [InlineData("http:")]
    [InlineData("http://")]
    public void EverythingElseIsRefused(string url) => Assert.False(Allowed(url), url);

    [Fact]
    public void NullIsRefused() => Assert.False(OfflineRules.IsAllowed(null, App, Gateway));

    [Fact]
    public void TheGatewayIsAllowedOnlyOnceItRuns()
    {
        Assert.True(OfflineRules.IsAllowed("http://127.0.0.1:7710/api/v1/health", App, Gateway));
        Assert.False(OfflineRules.IsAllowed("http://127.0.0.1:7710/api/v1/health", App, null));
        Assert.True(OfflineRules.IsAllowed("http://127.0.0.1:61235/", App, null));
    }

    [Fact]
    public void AnOriginThatIsNotLoopbackHttpAllowsNothing()
    {
        Assert.False(OfflineRules.IsAllowed("https://127.0.0.1:61235/", "https://127.0.0.1:61235", null));
        Assert.False(OfflineRules.IsAllowed("http://127.0.0.1:61235/x/", "http://127.0.0.1:61235/x", null));
    }

    [Theory]
    [InlineData("http://127.0.0.1:61235/midgard/index.html#/", "http://127.0.0.1:61235")]
    [InlineData("HTTP://127.0.0.1:61235/", "http://127.0.0.1:61235")]
    [InlineData("http://LOCALHOST:5/", "http://localhost:5")]
    [InlineData("http://127.0.0.1/", "http://127.0.0.1:80")]
    [InlineData("http://[::1]:5/", "http://[::1]:5")]
    [InlineData("http://127.0.0.1:61235@example.com/", null)]
    [InlineData("https://127.0.0.1:61235/", null)]
    [InlineData("about:blank", null)]
    [InlineData(null, null)]
    public void OriginOfAUrl(string? url, string? origin) => Assert.Equal(origin, OfflineRules.Origin(url));

    [Fact]
    public void ContextMenusKeepEditingAndCopyingOnly()
    {
        string?[] menu = ["back", "forward", "reload", null, "saveAs", "print", null, "cut", "copy", "paste", null,
                          "openLinkInNewWindow", "saveLinkAs", "copyLinkLocation", null, "share", "webCapture", "inspectElement"];
        var kept = OfflineRules.KeepInContextMenu(menu).Select(i => menu[i] ?? "|").ToArray();
        Assert.Equal(new[] { "cut", "copy", "paste", "|", "copyLinkLocation" }, kept);
        Assert.Empty(OfflineRules.KeepInContextMenu(["saveImageAs", null, "inspectElement", null]));
        Assert.Equal(new[] { 1, 2 }, OfflineRules.KeepInContextMenu([null, "copy", "selectAll", null]));
    }
}
