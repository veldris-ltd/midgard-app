namespace Midgard.Core;

/// <summary>
/// The offline guarantee for the window, as one pure decision: which URLs the web view may
/// load or navigate to. The Windows counterpart of <c>OfflineRules</c> and <c>Navigation</c>
/// in apps/mac/Sources/MidgardShellCore/Shell.swift. The window asks it for every request
/// (WebView2's WebResourceRequested, answered 403 when refused) and every navigation,
/// frames included (NavigationStarting, FrameNavigationStarting).
/// </summary>
/// <remarks>
/// <para>Allowed: the app's own web server and the gateway, each exactly as its origin
/// (<c>http://127.0.0.1:&lt;port&gt;</c>, no user name or password, the port written as it is,
/// followed by <c>/</c>, <c>?</c>, <c>#</c> or the end), <c>blob:</c> URLs of those origins,
/// <c>data:</c> and <c>about:blank</c>/<c>about:srcdoc</c>.</para>
/// <para>Refused: everything else, among them other ports, other loopback addresses
/// (<c>127.0.0.2</c>, <c>localhost</c>, <c>[::1]</c>), https, <c>ws:</c>/<c>wss:</c> (no console
/// uses WebSockets), <c>file:</c>, other <c>about:</c> pages and external schemes
/// (<c>mailto:</c>, <c>ms-settings:</c> …). Stricter than the Mac's content rules, which let
/// any loopback port through: here there is one decision for requests and navigations.</para>
/// <para>WebView2 hands over canonical URLs (scheme and host in lower case), but the check
/// does not rely on it: the scheme and host are compared without regard to case.</para>
/// </remarks>
public static class OfflineRules
{
    /// <summary>
    /// True if the web view may load or go to <paramref name="url"/>. <paramref name="appOrigin"/>
    /// is the app's web server (<c>http://127.0.0.1:61235</c>), <paramref name="gatewayOrigin"/>
    /// the gateway's (null before the services run).
    /// </summary>
    public static bool IsAllowed(string? url, string appOrigin, string? gatewayOrigin)
    {
        if (string.IsNullOrEmpty(url)) return false;
        var colon = url.IndexOf(':');
        if (colon <= 0) return false;
        var scheme = url[..colon].ToLowerInvariant();
        var rest = url[(colon + 1)..];
        switch (scheme)
        {
            case "data":
                return true;
            case "about":
                var page = rest.Split('?', '#')[0].ToLowerInvariant();
                return page is "blank" or "srcdoc";
            case "blob":
                // blob:<origin>/<uuid>: only blobs made by one of the allowed pages.
                return rest.StartsWith("http:", StringComparison.OrdinalIgnoreCase)
                       && IsAllowedHttp(rest, appOrigin, gatewayOrigin);
            case "http":
                return IsAllowedHttp(url, appOrigin, gatewayOrigin);
            default:
                return false;
        }
    }

    static bool IsAllowedHttp(string url, string appOrigin, string? gatewayOrigin) =>
        IsAt(url, appOrigin) || (gatewayOrigin is not null && IsAt(url, gatewayOrigin));

    /// <summary>
    /// True if <paramref name="url"/> is at exactly <paramref name="origin"/>: its text starts
    /// with the origin (scheme and host in any case, the port as written) and goes on with a
    /// path, query or fragment, or ends; and it parses to that same scheme, host and port with
    /// no user info. The text test rules out <c>http://127.0.0.1:1@example.com/</c>,
    /// <c>http://127.0.0.1:1.evil.example/</c> and backslash tricks; the parse is a second look.
    /// </summary>
    static bool IsAt(string url, string origin)
    {
        if (!Uri.TryCreate(origin, UriKind.Absolute, out var want) || want.Scheme != "http"
            || want.AbsolutePath != "/" || want.UserInfo.Length != 0)
            return false;
        if (!url.StartsWith(origin, StringComparison.OrdinalIgnoreCase)) return false;
        if (url.Length > origin.Length && url[origin.Length] is not ('/' or '?' or '#')) return false;
        if (!Uri.TryCreate(url, UriKind.Absolute, out var got)) return false;
        return got.Scheme == "http" && got.UserInfo.Length == 0 && got.HostNameType == want.HostNameType
               && string.Equals(got.IdnHost, want.IdnHost, StringComparison.OrdinalIgnoreCase)
               && got.Port == want.Port;
    }

    /// <summary>
    /// <c>http://host:port</c> of an http URL (host in lower case, no trailing slash; port 80
    /// when none is written), null for any other URL or one with a user name or password.
    /// What the window compares with the app's origin to know a page is its own.
    /// </summary>
    public static string? Origin(string? url)
    {
        if (url is null || !Uri.TryCreate(url, UriKind.Absolute, out var uri) || uri.Scheme != "http"
            || uri.UserInfo.Length != 0)
            return null;
        return $"http://{uri.Host.ToLowerInvariant()}:{uri.Port}";
    }

    /// <summary>
    /// The context-menu commands the window keeps (WebView2's item names): editing and
    /// copying. Everything else goes, above all what opens or saves things outside the window
    /// (open link in new window, save as, print, share, inspect, web capture …).
    /// </summary>
    public static readonly IReadOnlySet<string> ContextMenuCommands = new HashSet<string>(StringComparer.Ordinal)
    {
        "undo", "redo", "cut", "copy", "paste", "pasteAndMatchStyle", "selectAll",
        "copyLinkLocation", "copyImage", "copyImageLocation",
    };

    /// <summary>
    /// Which items of a context menu to keep, given each item's name (null for a separator):
    /// the allowed commands, with separators only between kept items and never two in a row.
    /// Returns the indexes kept, in order.
    /// </summary>
    public static IReadOnlyList<int> KeepInContextMenu(IReadOnlyList<string?> names)
    {
        var kept = new List<int>();
        var separatorPending = false;
        for (var i = 0; i < names.Count; i++)
        {
            if (names[i] is null)
            {
                separatorPending = kept.Count > 0;
                continue;
            }
            if (!ContextMenuCommands.Contains(names[i]!)) continue;
            if (separatorPending)
            {
                // The separator before this item: the last one seen.
                var separator = i - 1;
                while (separator >= 0 && names[separator] is not null) separator--;
                kept.Add(separator);
                separatorPending = false;
            }
            kept.Add(i);
        }
        return kept;
    }
}
