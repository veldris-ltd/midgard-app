using System.Diagnostics;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using Midgard.Core;

namespace Midgard.Core.Tests;

/// <summary>A parsed HTTP response from the raw test client.</summary>
sealed record RawResponse(int Status, Dictionary<string, string> Headers, byte[] Body)
{
    public string Text => Encoding.UTF8.GetString(Body);

    public string? this[string header] => Headers.GetValueOrDefault(header);
}

// The cases of apps/mac/Tests/MidgardShellCoreTests/WebServerTests.swift, one for one and
// in its order, then the ones only Windows needs: path segments Windows reads as more than
// a name, junctions, 8.3 short names, letter case, and the listener's address.
public sealed class WebServerTests : IDisposable
{
    readonly string @base;
    readonly string root;
    WebServer server;
    ushort port;
    readonly byte[] big;

    public WebServerTests()
    {
        @base = Path.Join(Path.GetTempPath(), $"midgard-web-{Guid.NewGuid():N}");
        root = Path.Join(@base, "root");
        Put("home/index.html", "<h1>home</h1>"u8.ToArray());
        Put("home/logo.svg", "<svg/>"u8.ToArray());
        Put("consoles/midgard/index.html", "<h1>midgard</h1>"u8.ToArray());
        Put("consoles/heimdall/assets/app.js", "console.log(1)"u8.ToArray());
        Put("consoles/heimdall/assets/app.css", "body{}"u8.ToArray());
        Put("consoles/forge/docs/index.html", "<h1>forge docs</h1>"u8.ToArray());
        big = RandomNumberGenerator.GetBytes(5 * 1024 * 1024);
        Put("consoles/mimir/data/big.i16", big);
        Put("recording/media/JamCams_1.jpg", [0xFF, 0xD8, 0xFF, 0xE0, .. "jpeg"u8.ToArray()]);
        Put("recording/media/JamCams_1.mp4", Clip);
        Put("recording/media/x.webm", "webm"u8.ToArray());
        Put("recording/media/.hidden.jpg", "hidden"u8.ToArray());
        Put("recording/media/sub/a.png", "png"u8.ToArray());
        Put("recording/heimdall.jsonl.gz", "recording"u8.ToArray());
        Put("recording/radar/manifest.json", """{"version":1}"""u8.ToArray());
        Put("recording/radar/frames/0/31/19.png", [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        Put("recording/radar/.hidden.json", "hidden"u8.ToArray());
        Put("recording/traffic/manifest.json", """{"version":1}"""u8.ToArray());
        Put("recording/traffic/7/63/42.pbf", [0x1A, 0x09, 0x0A, 0x05, 0x65, 0x6D, 0x70, 0x74, 0x79, 0x78, 0x02]);
        Put("recording/tiles/manifest.json", """{"version":1}"""u8.ToArray());
        Put("recording/tiles/terrarium/10/511/340.png", [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        Put("recording/tiles/s2cloudless/11/681/1023.jpg", [0xFF, 0xD8, 0xFF, 0xE0]);
        // Outside root: traversal must never reach it.
        File.WriteAllText(Path.Join(@base, "secret.txt"), "secret");

        // Port 0: the system picks, so parallel test runs never race for a port.
        server = new WebServer(root);
        server.Start();
        port = server.BoundPort;
    }

    public void Dispose()
    {
        server.Stop();
        try
        {
            Directory.Delete(@base, recursive: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
        }
    }

    static readonly byte[] Clip = Enumerable.Range(0, 1000).Select(i => (byte)(i % 251)).ToArray();

    void Put(string relative, byte[] data)
    {
        var path = Path.Join(root, relative);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllBytes(path, data);
    }

    // Made full, so the separators are Windows' (mklink reads a / as a switch).
    string R(string relative) => Path.GetFullPath(Path.Join(root, relative));

    // The raw client: sends the request verbatim (HttpClient would normalise `..` away) and reads to EOF.

    static Socket Connect(string host, int port)
    {
        var address = IPAddress.Parse(host);
        var socket = new Socket(address.AddressFamily, SocketType.Stream, ProtocolType.Tcp);
        try
        {
            socket.Connect(address, port);
            return socket;
        }
        catch
        {
            socket.Dispose();
            throw;
        }
    }

    static RawResponse RawRequest(string request, int port, string host = "127.0.0.1")
    {
        using var socket = Connect(host, port);
        socket.ReceiveTimeout = 5000;
        socket.Send(Encoding.UTF8.GetBytes(request));
        var data = new MemoryStream();
        var buffer = new byte[65536];
        while (true)
        {
            int n;
            try
            {
                n = socket.Receive(buffer);
            }
            catch (SocketException e) when (e.SocketErrorCode == SocketError.TimedOut)
            {
                throw new TimeoutException($"timed out waiting for the response to {request.Split('\r')[0]}");
            }
            if (n <= 0) break;
            data.Write(buffer, 0, n);
        }
        var bytes = data.ToArray();
        var split = bytes.AsSpan().IndexOf("\r\n\r\n"u8);
        if (split < 0) throw new InvalidDataException($"no header end in the response to {request.Split('\r')[0]}");
        var head = Encoding.UTF8.GetString(bytes, 0, split).Split("\r\n");
        var status = int.Parse(head[0].Split(' ')[1], System.Globalization.CultureInfo.InvariantCulture);
        var headers = new Dictionary<string, string>();
        foreach (var line in head.Skip(1))
        {
            var colon = line.IndexOf(':');
            if (colon < 0) continue;
            headers[line[..colon].ToLowerInvariant()] = line[(colon + 1)..].Trim();
        }
        return new RawResponse(status, headers, bytes[(split + 4)..]);
    }

    /// <summary>The first non-loopback IPv4 address of this PC, if it has one.</summary>
    static IPAddress? NonLoopbackIPv4() =>
        NetworkInterface.GetAllNetworkInterfaces()
            .Where(i => i.OperationalStatus == OperationalStatus.Up && i.NetworkInterfaceType != NetworkInterfaceType.Loopback)
            .SelectMany(i => i.GetIPProperties().UnicastAddresses)
            .Select(a => a.Address)
            .FirstOrDefault(a => a.AddressFamily == AddressFamily.InterNetwork && !IPAddress.IsLoopback(a));

    RawResponse Get(string path, string method = "GET") =>
        RawRequest($"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n", port);

    RawResponse Ranged(string path, string range, string method = "GET") =>
        RawRequest($"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nRange: {range}\r\n\r\n", port);

    static void StatusIs(int expected, RawResponse response, string what)
    {
        if (response.Status != expected) Assert.Fail($"{what}: {response.Status}, expected {expected}");
    }

    /// <summary>A refusal that shows nothing of what it refused.</summary>
    static void Refused(int expected, RawResponse response, string what)
    {
        StatusIs(expected, response, what);
        foreach (var word in new[] { "secret", "recording", "hidden", "jpeg" })
        {
            if (response.Text.Contains(word, StringComparison.Ordinal)) Assert.Fail($"{what}: the body shows {word}");
        }
    }

    [Fact]
    public void ServesHtmlAndJsWithTypes()
    {
        var html = Get("/midgard/index.html");
        Assert.Equal(200, html.Status);
        Assert.Equal("text/html; charset=utf-8", html["content-type"]);
        Assert.Equal("<h1>midgard</h1>", html.Text);
        Assert.Equal("no-store", html["cache-control"]);
        Assert.Equal("close", html["connection"]);
        Assert.Equal("nosniff", html["x-content-type-options"]);
        Assert.Equal("16", html["content-length"]);

        var js = Get("/heimdall/assets/app.js?v=3#x");
        Assert.Equal(200, js.Status);
        Assert.Equal("text/javascript", js["content-type"]);
        Assert.Equal("console.log(1)", js.Text);

        Assert.Equal("text/css", Get("/heimdall/assets/app.css")["content-type"]);
        Assert.Equal("image/svg+xml", Get("/home/logo.svg")["content-type"]);
        Assert.Equal("application/octet-stream", Get("/mimir/data/big.i16", "HEAD")["content-type"]);
    }

    [Fact]
    public void RootServesHome()
    {
        var r = Get("/");
        Assert.Equal(200, r.Status);
        Assert.Equal("<h1>home</h1>", r.Text);
    }

    [Fact]
    public void DirectoryServesIndex()
    {
        Assert.Equal("<h1>forge docs</h1>", Get("/forge/docs/").Text);
        Assert.Equal("<h1>midgard</h1>", Get("/midgard/").Text);
    }

    /// <summary>The consoles load <c>./assets/…</c>, which only resolves under the directory's own URL.</summary>
    [Fact]
    public void DirectoryWithoutSlashRedirects()
    {
        var r = Get("/forge/docs?tab=2");
        Assert.Equal(301, r.Status);
        Assert.Equal("/forge/docs/?tab=2", r["location"]);
        var m = Get("/midgard");
        Assert.Equal(301, m.Status);
        Assert.Equal("/midgard/", m["location"]);
    }

    [Fact]
    public void MissingIs404()
    {
        Assert.Equal(404, Get("/midgard/nope.js").Status);
        Assert.Equal(404, Get("/elsewhere/index.html").Status);
        Assert.Equal(404, Get("/consoles/midgard/index.html").Status);
        Assert.Equal(404, Get("/heimdall/").Status);  // a directory without index.html
    }

    [Fact]
    public void TraversalIs403()
    {
        foreach (var path in new[] { "/../secret.txt", "/midgard/../../secret.txt", "/midgard/%2e%2e/%2E%2E/secret.txt",
                                     "/midgard/..%5c..%5csecret.txt", "/midgard/a%5cb", "/midgard/index.html%00.js" })
            Refused(403, Get(path), path);
    }

    [Fact]
    public void HeadHasHeadersButNoBody()
    {
        var r = Get("/midgard/index.html", "HEAD");
        Assert.Equal(200, r.Status);
        Assert.Equal("16", r["content-length"]);
        Assert.Empty(r.Body);
    }

    [Fact]
    public async Task LargeFileRoundTrips()
    {
        using var http = new HttpClient(new SocketsHttpHandler { UseProxy = false });
        using var response = await http.GetAsync(new Uri($"http://127.0.0.1:{port}/mimir/data/big.i16"));
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var data = await response.Content.ReadAsByteArrayAsync();
        Assert.Equal(big.Length, data.Length);
        Assert.True(data.AsSpan().SequenceEqual(big));
    }

    [Fact]
    public void OtherMethodsAre405()
    {
        var r = RawRequest($"POST /midgard/index.html HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\n\r\n", port);
        Assert.Equal(405, r.Status);
        Assert.Equal("GET, HEAD", r["allow"]);
    }

    [Fact]
    public void OversizedHeadIsRefused()
    {
        var filler = new string('a', 20 * 1024);
        Assert.Equal(431, RawRequest($"GET / HTTP/1.1\r\nX-Filler: {filler}\r\n\r\n", port).Status);
    }

    [Fact]
    public void MalformedRequestLineIs400()
    {
        Assert.Equal(400, RawRequest("NONSENSE\r\n\r\n", port).Status);
        Assert.Equal(400, RawRequest("GET midgard HTTP/1.1\r\n\r\n", port).Status);
    }

    [Fact]
    public void ListensOnLoopbackOnly()
    {
        if (NonLoopbackIPv4() is not { } address) return;  // no non-loopback IPv4 address
        Assert.ThrowsAny<SocketException>(() => Connect(address.ToString(), port).Dispose());
    }

    [Fact]
    public void StartFailsWhenPortTaken()
    {
        var second = new WebServer(root, port);
        var error = Assert.Throws<WebServerStartException>(second.Start);
        Assert.Contains($"127.0.0.1:{port}", error.Message);
        second.Stop();
    }

    [Fact]
    public void HostMustBeThisServer()
    {
        int Status(string? host)
        {
            var header = host is null ? "" : $"Host: {host}\r\n";
            return RawRequest($"GET / HTTP/1.1\r\n{header}\r\n", port).Status;
        }
        Assert.Equal(200, Status($"127.0.0.1:{port}"));
        Assert.Equal(200, Status($"LocalHost:{port}"));
        Assert.Equal(421, Status($"evil.example:{port}"));
        Assert.Equal(421, Status($"127.0.0.1:{(ushort)(port + 1)}"));
        Assert.Equal(421, Status("127.0.0.1"));
        Assert.Equal(400, Status(null));
    }

    [Fact]
    public void PortZeroBindsAFreePort()
    {
        using var any = new WebServer(root);
        any.Start();
        Assert.NotEqual(0, any.BoundPort);
        Assert.NotEqual(port, any.BoundPort);
        var r = RawRequest($"GET / HTTP/1.1\r\nHost: localhost:{any.BoundPort}\r\n\r\n", any.BoundPort);
        Assert.Equal(200, r.Status);
    }

    [Fact]
    public void RestartOnTheSamePort()
    {
        for (var i = 0; i < 3; i++)
        {
            server.Stop();
            server = new WebServer(root, port);
            server.Start();
            Assert.Equal(200, Get("/").Status);
        }
    }

    /// <summary>The redirect is built from the path's segments: "//midgard" is not sent on to another host.</summary>
    [Fact]
    public void RedirectNeverLeavesThisServer()
    {
        var r = Get("//midgard");
        Assert.Equal(301, r.Status);
        Assert.Equal("/midgard/", r["location"]);
        Assert.Equal("/forge/docs/?x=1", Get("//forge//docs?x=1")["location"]);
        Assert.Equal("/forge/docs/", Get("/forge/./docs")["location"]);
        Assert.IsType<WebServer.Resolution.Redirect>(WebServer.Resolve("//midgard", root));
        Assert.Equal("/midgard/", ((WebServer.Resolution.Redirect)WebServer.Resolve("//midgard", root)).Location);
    }

    /// <summary>Beyond MaxConnections, a connection waits until one ends.</summary>
    [Fact]
    public void ConnectionsBeyondTheCapWait()
    {
        using var capped = new WebServer(root, maxConnections: 2);
        capped.Start();
        Assert.Equal(2, capped.MaxConnections);
        Assert.Equal(64, server.MaxConnections);
        using var idle1 = Connect("127.0.0.1", capped.BoundPort);
        var idle2 = Connect("127.0.0.1", capped.BoundPort);
        Thread.Sleep(300);  // both accepted, each waiting for its head
        using var third = Connect("127.0.0.1", capped.BoundPort);
        third.ReceiveTimeout = 1000;
        third.Send(Encoding.UTF8.GetBytes($"GET / HTTP/1.1\r\nHost: 127.0.0.1:{capped.BoundPort}\r\n\r\n"));
        var buffer = new byte[4096];
        var e = Assert.Throws<SocketException>(() => third.Receive(buffer));
        Assert.Equal(SocketError.TimedOut, e.SocketErrorCode);
        idle2.Dispose();  // a slot comes free
        third.ReceiveTimeout = 5000;
        var n = third.Receive(buffer);
        Assert.StartsWith("HTTP/1.1 200", Encoding.ASCII.GetString(buffer, 0, n));
    }

    [Fact]
    public void IdleConnectionIsClosed()
    {
        using var quick = new WebServer(root, headTimeout: TimeSpan.FromSeconds(0.3));
        quick.Start();
        using var socket = Connect("127.0.0.1", quick.BoundPort);
        socket.ReceiveTimeout = 3000;
        socket.Send("GET / HT"u8.ToArray());  // a head that never completes
        var started = Stopwatch.StartNew();
        var n = socket.Receive(new byte[1]);
        Assert.Equal(0, n);  // the server closed the connection
        Assert.True(started.Elapsed < TimeSpan.FromSeconds(2.5), $"closed after {started.Elapsed}");
    }

    [Fact]
    public void ServesRecordingMediaWithTypes()
    {
        var jpg = Get("/media/JamCams_1.jpg?t=12345");
        Assert.Equal(200, jpg.Status);
        Assert.Equal("image/jpeg", jpg["content-type"]);
        Assert.Equal("bytes", jpg["accept-ranges"]);
        Assert.Equal(new byte[] { 0xFF, 0xD8, 0xFF }, jpg.Body.Take(3));
        var mp4 = Get("/media/JamCams_1.mp4");
        Assert.Equal(200, mp4.Status);
        Assert.Equal("video/mp4", mp4["content-type"]);
        Assert.Equal(1000, mp4.Body.Length);
        Assert.Equal("video/webm", Get("/media/x.webm", "HEAD")["content-type"]);
        Assert.Equal("image/png", Get("/media/sub/a.png")["content-type"]);
    }

    [Fact]
    public void MediaRefusesEverythingButItsFiles()
    {
        Assert.Equal(404, Get("/media/").Status);
        Assert.Equal(404, Get("/media").Status);
        Assert.Equal(404, Get("/media/sub").Status);  // a directory: no redirect, no index
        Assert.Equal(404, Get("/media/nope.jpg").Status);
        Assert.Equal(404, Get("/media/.hidden.jpg").Status);
        Assert.Equal(404, Get("/recording/heimdall.jsonl.gz").Status);
        foreach (var path in new[] { "/media/../heimdall.jsonl.gz", "/media/%2e%2e/heimdall.jsonl.gz",
                                     "/media/..%2f..%2f..%2fsecret.txt", "/media/a%5c..%5cb.jpg", "/media/x.jpg%00.mp4" })
            Refused(403, Get(path), path);
        var post = RawRequest($"POST /media/JamCams_1.jpg HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 0\r\n\r\n", port);
        Assert.Equal(405, post.Status);
        var rebound = RawRequest($"GET /media/JamCams_1.jpg HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n", port);
        Assert.Equal(421, rebound.Status);
    }

    [Fact]
    public void ServesRecordingRadarWithTheSameRules()
    {
        var manifest = Get("/radar/manifest.json");
        Assert.Equal(200, manifest.Status);
        Assert.Equal("application/json", manifest["content-type"]);
        Assert.Equal("""{"version":1}""", manifest.Text);
        var tile = Get("/radar/frames/0/31/19.png");
        Assert.Equal(200, tile.Status);
        Assert.Equal("image/png", tile["content-type"]);
        Assert.Equal(new byte[] { 0x89, 0x50, 0x4E, 0x47 }, tile.Body.Take(4));
        Assert.Equal(404, Get("/radar/").Status);
        Assert.Equal(404, Get("/radar/frames/0").Status);  // a directory
        Assert.Equal(404, Get("/radar/.hidden.json").Status);
        Assert.Equal(404, Get("/radar/nope.png").Status);
        foreach (var path in new[] { "/radar/../heimdall.jsonl.gz", "/radar/%2e%2e/media/JamCams_1.jpg",
                                     "/radar/..%2f..%2f..%2fsecret.txt" })
            StatusIs(403, Get(path), path);
        // A link from radar into media stays in root but leaves radar: refused.
        File.CreateSymbolicLink(R("recording/radar/cam.jpg"), R("recording/media/JamCams_1.jpg"));
        Refused(403, Get("/radar/cam.jpg"), "/radar/cam.jpg");
    }

    [Fact]
    public void ServesRecordingTrafficWithTheSameRules()
    {
        var manifest = Get("/traffic/manifest.json");
        Assert.Equal(200, manifest.Status);
        Assert.Equal("application/json", manifest["content-type"]);
        var tile = Get("/traffic/7/63/42.pbf");
        Assert.Equal(200, tile.Status);
        Assert.Equal("application/x-protobuf", tile["content-type"]);
        Assert.Equal(11, tile.Body.Length);
        Assert.Equal(404, Get("/traffic/").Status);
        Assert.Equal(404, Get("/traffic/7/63").Status);  // a directory
        Assert.Equal(404, Get("/traffic/7/63/43.pbf").Status);
        foreach (var path in new[] { "/traffic/../heimdall.jsonl.gz", "/traffic/%2e%2e/radar/manifest.json" })
            StatusIs(403, Get(path), path);
    }

    [Fact]
    public void ServesRecordingTilesWithTheSameRules()
    {
        var manifest = Get("/tiles/manifest.json");
        Assert.Equal(200, manifest.Status);
        Assert.Equal("application/json", manifest["content-type"]);
        var terrain = Get("/tiles/terrarium/10/511/340.png");
        Assert.Equal(200, terrain.Status);
        Assert.Equal("image/png", terrain["content-type"]);
        Assert.Equal(8, terrain.Body.Length);
        var imagery = Get("/tiles/s2cloudless/11/681/1023.jpg");
        Assert.Equal(200, imagery.Status);
        Assert.Equal("image/jpeg", imagery["content-type"]);
        Assert.Equal(404, Get("/tiles/").Status);
        Assert.Equal(404, Get("/tiles/terrarium/10").Status);  // a directory
        Assert.Equal(404, Get("/tiles/terrarium/10/511/341.png").Status);  // sea: not recorded
        foreach (var path in new[] { "/tiles/../heimdall.jsonl.gz", "/tiles/%2e%2e/traffic/manifest.json" })
            StatusIs(403, Get(path), path);
    }

    [Fact]
    public void SymlinksOutOfTheTreeAreRefused()
    {
        var media = R("recording/media");
        // A link to a file outside root, one to the recording (inside root, outside media),
        // a link to a directory outside, and a link that stays inside media (allowed).
        File.CreateSymbolicLink(Path.Join(media, "out.jpg"), Path.Join(@base, "secret.txt"));
        File.CreateSymbolicLink(Path.Join(media, "rec.jpg"), R("recording/heimdall.jsonl.gz"));
        Directory.CreateSymbolicLink(Path.Join(media, "up"), @base);
        File.CreateSymbolicLink(Path.Join(media, "same.jpg"), Path.Join(media, "JamCams_1.jpg"));
        File.CreateSymbolicLink(R("consoles/heimdall/assets/out.js"), Path.Join(@base, "secret.txt"));
        foreach (var path in new[] { "/media/out.jpg", "/media/rec.jpg", "/media/up/secret.txt", "/heimdall/assets/out.js" })
            Refused(403, Get(path), path);
        Assert.Equal(200, Get("/media/same.jpg").Status);
    }

    /// <summary>WebView2, like WebKit, asks for <c>bytes=0-1</c> first and plays a clip only if it gets 206.</summary>
    [Fact]
    public void RangeRequestsForClips()
    {
        var probe = Ranged("/media/JamCams_1.mp4", "bytes=0-1");
        Assert.Equal(206, probe.Status);
        Assert.Equal("bytes 0-1/1000", probe["content-range"]);
        Assert.Equal("2", probe["content-length"]);
        Assert.Equal("video/mp4", probe["content-type"]);
        Assert.Equal(Clip[0..2], probe.Body);
        var middle = Ranged("/media/JamCams_1.mp4", "bytes=500-");
        Assert.Equal("bytes 500-999/1000", middle["content-range"]);
        Assert.Equal(Clip[500..], middle.Body);
        var tail = Ranged("/media/JamCams_1.mp4", "bytes=-10");
        Assert.Equal(Clip[990..], tail.Body);
        var clamped = Ranged("/media/JamCams_1.mp4", "bytes=990-5000");
        Assert.Equal("bytes 990-999/1000", clamped["content-range"]);
        var head = Ranged("/media/JamCams_1.mp4", "bytes=0-99", "HEAD");
        Assert.Equal(206, head.Status);
        Assert.Empty(head.Body);
        var past = Ranged("/media/JamCams_1.mp4", "bytes=1000-");
        Assert.Equal(416, past.Status);
        Assert.Equal("bytes */1000", past["content-range"]);
        foreach (var odd in new[] { "bytes=0-1,5-6", "items=0-1", "bytes=5-2", "bytes=x-y", "bytes=99999999999999999999-" })
        {
            var r = Ranged("/media/JamCams_1.mp4", odd);
            StatusIs(200, r, odd);
            Assert.Equal(1000, r.Body.Length);
        }
        // The big file through a range: the stream stops at the range's end.
        var part = Ranged("/mimir/data/big.i16", "bytes=1000000-1299999");
        Assert.Equal(206, part.Status);
        Assert.True(part.Body.AsSpan().SequenceEqual(big.AsSpan(1_000_000, 300_000)));
    }

    [Fact]
    public void ByteRangeParsing()
    {
        Assert.Equal(WebServer.ByteRange.Part(0, 2), WebServer.ParseRange("bytes=0-1", 10));
        Assert.Equal(WebServer.ByteRange.Part(3, 10), WebServer.ParseRange("bytes=3-", 10));
        Assert.Equal(WebServer.ByteRange.Part(7, 10), WebServer.ParseRange("bytes=-3", 10));
        Assert.Equal(WebServer.ByteRange.Part(0, 10), WebServer.ParseRange("bytes=-30", 10));
        Assert.Equal(WebServer.ByteRange.Unsatisfiable, WebServer.ParseRange("bytes=-0", 10));
        Assert.Equal(WebServer.ByteRange.Unsatisfiable, WebServer.ParseRange("bytes=10-", 10));
        Assert.Equal(WebServer.ByteRange.Unsatisfiable, WebServer.ParseRange("bytes=0-", 0));
        Assert.Equal(WebServer.ByteRange.Whole, WebServer.ParseRange("bytes=+1-2", 10));
        Assert.Equal(WebServer.ByteRange.Whole, WebServer.ParseRange("bytes=-", 10));
    }

    [Fact]
    public void StopClosesTheListener()
    {
        server.Stop();
        Assert.ThrowsAny<SocketException>(() => Connect("127.0.0.1", port).Dispose());
    }

    // Windows only needs these.

    [Fact]
    public void SegmentsWindowsReadAsMoreThanANameAre403()
    {
        foreach (var path in new[] {
            // Drive letters and alternate data streams.
            "/midgard/C:/Windows/win.ini", "/midgard/C:%5cWindows%5cwin.ini", "/media/c:", "/media/C%3a",
            "/media/JamCams_1.jpg:secret", "/media/JamCams_1.jpg::$DATA", "/media/JamCams_1.jpg%3a%3a$DATA",
            "/media/sub:$I30:$INDEX_ALLOCATION/a.png",
            // Reserved device names, with or without an extension, in any case.
            "/media/NUL", "/media/nul.jpg", "/midgard/CON", "/midgard/con.html", "/media/COM1.jpg", "/media/lpt9.png",
            "/media/Aux", "/media/PRN.txt", "/media/CONIN$", "/media/conout$.jpg", "/media/COM%C2%B9.jpg",
            "/media/aux%20.jpg", "/midgard/COM0",
            // Trailing dots and spaces, which Windows strips.
            "/media/JamCams_1.jpg.", "/media/JamCams_1.jpg%20", "/midgard/index.html...", "/midgard/..%20/..%20/secret.txt",
            "/midgard/.../secret.txt", "/midgard/..%2e/secret.txt", "/media/sub./a.png", "/media/sub%20/a.png",
            // Backslashes and slashes, encoded in either case.
            "/media/..%5C..%5Csecret.txt", "/media/..%2F..%2F..%2Fsecret.txt", "/midgard/..%2F..%2Fsecret.txt",
            // The other characters Windows forbids in names.
            "/media/a%3fb.jpg", "/media/a*.jpg", "/media/a%7cb.jpg", "/media/a%3cb.jpg", "/media/a%3eb.jpg",
            "/media/a%22b.jpg", "/media/a%01b.jpg", "/media/a%09b.jpg",
        })
            Refused(403, Get(path), path);
        // A lone "." is the folder itself, as on the Mac.
        Assert.Equal("<h1>midgard</h1>", Get("/midgard/./index.html").Text);
        // The recording folders refuse it with the dot files, as on the Mac.
        Assert.Equal(404, Get("/media/./JamCams_1.jpg").Status);
    }

    [Fact]
    public void MalformedEscapesAre400()
    {
        foreach (var path in new[] { "/midgard/%zz", "/midgard/%", "/midgard/a%2", "/midgard/%e9.html", "/media/%c0%af" })
            StatusIs(400, Get(path), path);
    }

    [Fact]
    public void SegmentRules()
    {
        foreach (var bad in new[] { "C:", "a:b", "CON", "con", "Nul.txt", "COM9", "LPT1.tar.gz", "x.", "x ", "...", ".. ",
                                    "a?b", "a*b", "a|b", "a<b", "a>b", "a\"b", "a\u0001b", "a\u007fb", "CONIN$" })
            Assert.True(WebServer.IsUnsafeOnWindows(bad), bad);
        foreach (var good in new[] { ".", "index.html", ".hidden.jpg", "CONSOLE.js", "nul-ish.txt", "COM10", "LPT",
                                     "assets", "JamCams_1.jpg", "a b.png", "café.png", "~1" })
            Assert.False(WebServer.IsUnsafeOnWindows(good), good);
    }

    static void Junction(string link, string target)
    {
        var info = new ProcessStartInfo("cmd.exe") { UseShellExecute = false, CreateNoWindow = true };
        foreach (var arg in new[] { "/c", "mklink", "/J", Path.GetFullPath(link), Path.GetFullPath(target) })
            info.ArgumentList.Add(arg);
        using var mklink = Process.Start(info)!;
        mklink.WaitForExit();
        Assert.Equal(0, mklink.ExitCode);
    }

    [Fact]
    public void JunctionsOutOfTheirFolderAreRefused()
    {
        if (!OperatingSystem.IsWindows()) return;
        var media = R("recording/media");
        Junction(Path.Join(media, "up"), @base);                       // out of root
        Junction(Path.Join(media, "radar"), R("recording/radar"));     // in root, out of media
        Junction(R("consoles/heimdall/assets/up"), @base);             // out of root from a console
        Junction(Path.Join(media, "inner"), Path.Join(media, "sub"));  // stays in media: allowed
        foreach (var path in new[] { "/media/up/secret.txt", "/media/radar/manifest.json", "/heimdall/assets/up/secret.txt" })
            Refused(403, Get(path), path);
        Assert.Equal("png", Get("/media/inner/a.png").Text);
    }

    [Fact]
    public void ShortNamesCannotReachDotFiles()
    {
        if (!OperatingSystem.IsWindows()) return;
        var buffer = new char[32768];
        var length = Kernel32.GetShortPathName(R("recording/media/.hidden.jpg"), buffer, (uint)buffer.Length);
        Assert.NotEqual(0u, length);
        var name = Path.GetFileName(new string(buffer, 0, (int)length));
        if (name == ".hidden.jpg") return;  // 8.3 names are off on this volume
        Refused(404, Get($"/media/{name}"), name);
        Refused(404, Get($"/media/{name.ToLowerInvariant()}"), name);
    }

    [Fact]
    public void LettersCaseOnlyMattersInTheRoutes()
    {
        if (!OperatingSystem.IsWindows()) return;
        // The file system ignores case and so does the check that a file stays in its folder.
        Assert.Equal(200, Get("/media/JAMCAMS_1.JPG").Status);
        Assert.Equal("<h1>midgard</h1>", Get("/midgard/INDEX.HTML").Text);
        Assert.Equal(404, Get("/MEDIA/JamCams_1.jpg").Status);
        Assert.Equal(404, Get("/Midgard/index.html").Status);
        Assert.Equal(404, Get("/media/.HIDDEN.JPG").Status);
    }

    [Fact]
    public void NeverListensOnAnythingBut127001()
    {
        Assert.Equal(new IPEndPoint(IPAddress.Loopback, port), server.LocalEndPoint);
        if (OperatingSystem.IsWindows())
        {
            var listeners = IPGlobalProperties.GetIPGlobalProperties().GetActiveTcpListeners()
                .Where(e => e.Port == port).ToList();
            Assert.NotEmpty(listeners);
            Assert.All(listeners, e => Assert.Equal(IPAddress.Loopback, e.Address));
        }
        Assert.ThrowsAny<SocketException>(() => Connect("::1", port).Dispose());
        server.Stop();
        Assert.Null(server.LocalEndPoint);
        Assert.Equal(0, server.BoundPort);
    }
}
