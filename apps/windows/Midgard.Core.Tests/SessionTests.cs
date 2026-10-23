using System.Diagnostics;
using System.Net;
using System.Text;
using System.Text.Json;
using Midgard.Core;

namespace Midgard.Core.Tests;

/// <summary>Answers every request with a canned response (or failure) and records what it was sent.</summary>
sealed class StubHandler(HttpStatusCode status, string body, Exception? failure = null) : HttpMessageHandler
{
    public HttpMethod? Method;
    public Uri? Url;
    public string? ContentType;
    public string? Accept;
    public byte[] Body = [];

    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        Method = request.Method;
        Url = request.RequestUri;
        ContentType = request.Content?.Headers.ContentType?.MediaType;
        Accept = request.Headers.Accept.ToString();
        if (request.Content is not null) Body = await request.Content.ReadAsByteArrayAsync(cancellationToken);
        if (failure is not null) throw failure;
        return new HttpResponseMessage(status) { Content = new StringContent(body, Encoding.UTF8, "application/json") };
    }
}

// The port of apps/mac/Tests/MidgardShellCoreTests/SessionTests.swift.
public class SessionTests
{
    static readonly Uri Gateway = new("http://127.0.0.1:61234/api/v1");
    static readonly DemoUser User = new("analyst", "pa\"ss<word>", "Analyst");

    static (HttpClient Client, StubHandler Stub) Respond(HttpStatusCode status, string json, Exception? failure = null)
    {
        var stub = new StubHandler(status, json, failure);
        return (new HttpClient(stub), stub);
    }

    static async Task<SessionException> Fails(HttpClient client, DemoUser? user = null)
    {
        return await Assert.ThrowsAsync<SessionException>(() => Session.LoginAsync(Gateway, user ?? User, client));
    }

    // Demo users

    [Fact]
    public void LoadsDemoUsers()
    {
        var file = Path.Join(Path.GetTempPath(), $"demo-users-{Guid.NewGuid():N}.json");
        try
        {
            File.WriteAllText(file, """[{"username":"a","password":"p1","label":"Analyst"},{"username":"b","password":"p2","label":"Boss"}]""");
            Assert.Equal(new[] { new DemoUser("a", "p1", "Analyst"), new DemoUser("b", "p2", "Boss") }, DemoUsers.Load(file));
        }
        finally
        {
            File.Delete(file);
        }
    }

    [Theory]
    [InlineData("""[{"username":"a"}]""")]
    [InlineData("""[{"username":"a","password":1,"label":"x"}]""")]
    [InlineData("""{"username":"a"}""")]
    [InlineData("not json")]
    public void MalformedDemoUsersThrows(string text)
    {
        var file = Path.Join(Path.GetTempPath(), $"demo-users-{Guid.NewGuid():N}.json");
        try
        {
            File.WriteAllText(file, text);
            Assert.Throws<InvalidDataException>(() => DemoUsers.Load(file));
        }
        finally
        {
            File.Delete(file);
        }
    }

    [Fact]
    public void PasswordIsNeverDescribed()
    {
        const string Secret = "hunter2-secret";
        var u = new DemoUser("a", Secret, "Analyst");
        Assert.DoesNotContain(Secret, u.ToString());
        Assert.DoesNotContain(Secret, $"{u}");
        Assert.DoesNotContain(Secret, string.Join(",", new[] { u }));
        Assert.Contains("a", u.ToString());
    }

    // Login

    [Fact]
    public async Task SuccessfulLogin()
    {
        var (client, _) = Respond(HttpStatusCode.OK,
            """{"token":"abc123","subject":"analyst","clearance":"restricted","roles":["analyst","viewer"],"expiresAt":"1792000000"}""");
        var info = await Session.LoginAsync(Gateway, User, client);
        Assert.Equal("abc123", info.Token);
        Assert.Equal("analyst", info.Subject);
        Assert.Equal("restricted", info.Clearance);
        Assert.Equal(new[] { "analyst", "viewer" }, info.Roles);
        Assert.Equal(DateTimeOffset.FromUnixTimeSeconds(1_792_000_000), info.ExpiresAt);
    }

    [Fact]
    public async Task LoginToleratesNullClaimsAndNumericExpiry()
    {
        var (client, _) = Respond(HttpStatusCode.OK,
            """{"token":"t","subject":"s","clearance":null,"roles":null,"expiresAt":1792000000}""");
        var info = await Session.LoginAsync(Gateway, User, client);
        Assert.Null(info.Clearance);
        Assert.Empty(info.Roles);
        Assert.Equal(DateTimeOffset.FromUnixTimeSeconds(1_792_000_000), info.ExpiresAt);
    }

    [Fact]
    public async Task LoginPostsJsonCredentials()
    {
        var (client, stub) = Respond(HttpStatusCode.OK, """{"token":"t","subject":"s","clearance":"open","roles":[],"expiresAt":"1"}""");
        await Session.LoginAsync(Gateway, User, client);
        Assert.Equal(HttpMethod.Post, stub.Method);
        Assert.Equal("/api/v1/login", stub.Url!.AbsolutePath);
        Assert.Equal("application/json", stub.ContentType);
        Assert.Equal("application/json", stub.Accept);
        var body = JsonSerializer.Deserialize<Dictionary<string, string>>(stub.Body)!;
        Assert.Equal(new Dictionary<string, string> { ["username"] = "analyst", ["password"] = "pa\"ss<word>" }, body);
    }

    [Fact]
    public async Task TrailingSlashOnGatewayIsTolerated()
    {
        var (client, stub) = Respond(HttpStatusCode.OK, """{"token":"t","subject":"s","clearance":"open","roles":[],"expiresAt":"1"}""");
        await Session.LoginAsync(new Uri(Gateway.AbsoluteUri + "/"), User, client);
        Assert.Equal("/api/v1/login", stub.Url!.AbsolutePath);
    }

    [Fact]
    public async Task Status401IsInvalidCredentials()
    {
        var (client, _) = Respond(HttpStatusCode.Unauthorized, """{"error":"invalid credentials"}""");
        Assert.Equal(SessionException.Kinds.InvalidCredentials, (await Fails(client)).Kind);
    }

    [Fact]
    public async Task Status500IsHttpError()
    {
        var (client, _) = Respond(HttpStatusCode.InternalServerError, """{"error":"idp unavailable"}""");
        var e = await Fails(client);
        Assert.Equal(SessionException.Kinds.Http, e.Kind);
        Assert.Equal(500, e.Status);
        Assert.Equal("idp unavailable", e.Detail);
        Assert.Equal("login failed: HTTP 500: idp unavailable", e.Message);
    }

    [Fact]
    public async Task HttpErrorWithoutJsonQuotesTheStartOfTheBody()
    {
        var (client, _) = Respond(HttpStatusCode.BadGateway, new string('x', 300));
        var e = await Fails(client);
        Assert.Equal(new string('x', 200), e.Detail);
    }

    [Fact]
    public async Task TransportErrorIsWrapped()
    {
        var (client, _) = Respond(HttpStatusCode.OK, "", new HttpRequestException("No connection could be made"));
        Assert.Equal(SessionException.Kinds.Transport, (await Fails(client)).Kind);
    }

    [Fact]
    public async Task TimeoutIsATransportError()
    {
        var (client, _) = Respond(HttpStatusCode.OK, "", new TaskCanceledException("timeout"));
        var e = await Fails(client);
        Assert.Equal(SessionException.Kinds.Transport, e.Kind);
        Assert.Equal("timed out", e.Detail);
    }

    [Fact]
    public async Task CancellationIsNotATransportError()
    {
        using var cancel = new CancellationTokenSource();
        await cancel.CancelAsync();
        var (client, _) = Respond(HttpStatusCode.OK, """{"token":"t","subject":"s","expiresAt":"1"}""");
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => Session.LoginAsync(Gateway, User, client, cancel.Token));
    }

    [Theory]
    [InlineData("""{"subject":"s"}""")]
    [InlineData("""{"token":"t","subject":"s","expiresAt":"soon"}""")]
    [InlineData("""{"token":"t","subject":"s","expiresAt":"1","roles":"admin"}""")]
    [InlineData("""{"token":"t","subject":"s","expiresAt":1e300}""")]
    [InlineData("[]")]
    [InlineData("not json")]
    public async Task MalformedBodyIsBadResponse(string body)
    {
        var (client, _) = Respond(HttpStatusCode.OK, body);
        Assert.Equal(SessionException.Kinds.BadResponse, (await Fails(client)).Kind);
    }

    [Fact]
    public async Task ErrorsNeverCarryThePassword()
    {
        var (client, _) = Respond(HttpStatusCode.InternalServerError, """{"error":"idp unavailable"}""");
        var e = await Fails(client);
        Assert.DoesNotContain(User.Password, e.ToString());
        Assert.DoesNotContain(User.Password, e.Message);
    }

    [Fact]
    public void LoopbackClientsHaveTheMacTimeouts()
    {
        Assert.Equal(TimeSpan.FromSeconds(10), Session.Loopback.Timeout);
        // Sign-in at startup waits up to a minute for a busy gateway, still loopback only.
        Assert.Equal(TimeSpan.FromSeconds(60), Session.PatientLoopback.Timeout);
    }

    /// <summary>The loopback clients go straight to the gateway: no proxy, whatever the PC's proxy settings.</summary>
    [Fact]
    public async Task LoopbackClientsBypassProxies()
    {
        var listener = new System.Net.Sockets.TcpListener(IPAddress.Loopback, 0);
        listener.Start();
        try
        {
            var port = ((IPEndPoint)listener.LocalEndpoint).Port;
            var served = Task.Run(async () =>
            {
                using var socket = await listener.AcceptSocketAsync();
                var buffer = new byte[8192];
                var read = await socket.ReceiveAsync(buffer.AsMemory());
                var head = Encoding.ASCII.GetString(buffer, 0, read);
                var reply = """{"token":"t","subject":"s","expiresAt":"1"}""";
                await socket.SendAsync(Encoding.ASCII.GetBytes(
                    $"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {reply.Length}\r\nConnection: close\r\n\r\n{reply}").AsMemory());
                return head;
            });
            var info = await Session.LoginAsync(new Uri($"http://127.0.0.1:{port}/api/v1"), User);
            Assert.Equal("t", info.Token);
            // Straight to the server: an origin-form request line, not a proxy's absolute URL.
            Assert.StartsWith("POST /api/v1/login HTTP/1.1", await served);
        }
        finally
        {
            listener.Stop();
        }
    }

    // Refresh

    static readonly DateTimeOffset Now = DateTimeOffset.FromUnixTimeSeconds(1_000_000);
    static SessionInfo Expiring(TimeSpan after) => new("t", "s", null, [], Now + after);

    [Fact]
    public void RefreshTenMinutesBeforeExpiry() =>
        Assert.Equal(Now + TimeSpan.FromHours(12) - TimeSpan.FromMinutes(10), Session.RefreshDate(Expiring(TimeSpan.FromHours(12)), Now));

    [Fact]
    public void RefreshAtHalfLifeWhenShort() =>
        Assert.Equal(Now + TimeSpan.FromSeconds(300), Session.RefreshDate(Expiring(TimeSpan.FromSeconds(600)), Now));

    [Fact]
    public void RefreshSoonButNotImmediatelyWhenExpired() =>
        Assert.Equal(Now + TimeSpan.FromSeconds(30), Session.RefreshDate(Expiring(TimeSpan.FromSeconds(-5)), Now));

    [Fact]
    public void RefreshNeverSoonerThanThirtySeconds() =>
        Assert.Equal(Now + TimeSpan.FromSeconds(30), Session.RefreshDate(Expiring(TimeSpan.FromSeconds(40)), Now));

    [Fact]
    public void SessionInfoNeverShowsToken()
    {
        var info = new SessionInfo("tok-SECRET-123", "s", "open", [], DateTimeOffset.Now);
        Assert.DoesNotContain("SECRET", info.ToString());
        Assert.DoesNotContain("SECRET", $"{info}");
    }

    // Injection, evaluated by Node.js (on the GitHub Windows runners) with a stand-in window,
    // location and sessionStorage, as the Swift tests do with JavaScriptCore.

    sealed record Injected(string? Gateway, string? Console, string? Token, JsonElement? Who, string? Prepared);

    const string Origin = "http://127.0.0.1:61235";

    static Injected? Evaluate(string script, string origin = Origin)
    {
        var node = Node.Find();
        if (node is null) return null;
        var harness = $$"""
            var window = {};
            var location = { origin: {{Session.Literal(origin)}} };
            var sessionStorage = { store: {}, setItem: function (k, v) { this.store[k] = String(v); } };
            {{script}}
            process.stdout.write(JSON.stringify({
              gateway: window.__MIDGARD_GATEWAY__, console: window.__MIDGARD_CONSOLE__,
              prepared: window.__MIDGARD_PREPARED__,
              token: sessionStorage.store['midgard.session.token'], who: sessionStorage.store['midgard.session.who'] }));
            """;
        var output = Node.Run(node, harness);
        using var document = JsonDocument.Parse(output);
        var root = document.RootElement;
        string? Text(string name) => root.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
        JsonElement? who = Text("who") is { } w ? JsonDocument.Parse(w).RootElement.Clone() : null;
        return new Injected(Text("gateway"), Text("console"), Text("token"), who, Text("prepared"));
    }

    static string[] Roles(JsonElement? who) => who!.Value.GetProperty("roles").EnumerateArray().Select(r => r.GetString()!).ToArray();

    [Fact]
    public void InjectionScriptRoundTrips()
    {
        var info = new SessionInfo("deadbeef", "analyst", "restricted", ["analyst", "viewer"], DateTimeOffset.Now);
        var script = Session.InjectionScript(Origin, "http://127.0.0.1:61234/api/v1", "http://127.0.0.1:61235/", info);
        if (Evaluate(script) is not { } got) return;
        Assert.Equal("http://127.0.0.1:61234/api/v1", got.Gateway);
        Assert.Equal("http://127.0.0.1:61235/", got.Console);
        Assert.Equal("deadbeef", got.Token);
        Assert.Equal("analyst", got.Who!.Value.GetProperty("subject").GetString());
        Assert.Equal("restricted", got.Who!.Value.GetProperty("clearance").GetString());
        Assert.Equal(new[] { "analyst", "viewer" }, Roles(got.Who));
    }

    /// <summary>The script's shape, as the Swift string has it (multi-line literal, no trailing newline).</summary>
    [Fact]
    public void InjectionScriptReadsAsTheMacs()
    {
        var info = new SessionInfo("t", "s", null, ["r"], DateTimeOffset.Now);
        Assert.Equal(
            "(function () {\n"
            + "  if (typeof location === 'undefined' || location.origin !== \"http:\\/\\/127.0.0.1:61235\") { return; }\n"
            + "  window.__MIDGARD_GATEWAY__ = \"g\";\n"
            + "  window.__MIDGARD_CONSOLE__ = \"c\";\n"
            + "  window.__MIDGARD_PREPARED__ = \"p\";\n"
            + "  try {\n"
            + "    sessionStorage.setItem('midgard.session.token', \"t\");\n"
            + "    sessionStorage.setItem('midgard.session.who', JSON.stringify({\"roles\":[\"r\"],\"subject\":\"s\"}));\n"
            + "  } catch (e) {}\n"
            + "})();",
            Session.InjectionScript(Origin, "g", "c", info, "p"));
    }

    const string Nasty = "a'b\"c\\d</script><script>alert(1)</script>\u2028e\u2029f\n\0g${x}`hé";

    [Fact]
    public void InjectionScriptEscapesHostileValues()
    {
        var info = new SessionInfo(Nasty, Nasty, Nasty, [Nasty, "plain"], DateTimeOffset.Now);
        var script = Session.InjectionScript(Origin, Nasty, Nasty, info);
        Assert.DoesNotContain("</script", script);
        Assert.DoesNotContain("<", script);
        Assert.DoesNotContain("\u2028", script);
        Assert.DoesNotContain("\u2029", script);
        Assert.True(script.All(c => c is '\n' or >= ' ' and <= '~'), "only printable ASCII and newlines");
        if (Evaluate(script) is not { } got) return;
        Assert.Equal(Nasty, got.Gateway);
        Assert.Equal(Nasty, got.Console);
        Assert.Equal(Nasty, got.Token);
        Assert.Equal(Nasty, got.Who!.Value.GetProperty("subject").GetString());
        Assert.Equal(Nasty, got.Who!.Value.GetProperty("clearance").GetString());
        Assert.Equal(new[] { Nasty, "plain" }, Roles(got.Who));
    }

    [Fact]
    public void InjectionOmitsMissingClearance()
    {
        var script = Session.InjectionScript(Origin, "g", "c", new SessionInfo("t", "s", null, [], DateTimeOffset.Now));
        Assert.DoesNotContain("clearance", script);
        if (Evaluate(script) is not { } got) return;
        Assert.False(got.Who!.Value.TryGetProperty("clearance", out _));
        Assert.Empty(Roles(got.Who));
    }

    [Fact]
    public void InjectionOnlyRunsOnItsOrigin()
    {
        var script = Session.InjectionScript(Origin, "g", "c", new SessionInfo("t", "s", null, [], DateTimeOffset.Now));
        if (Evaluate(script, Origin) is not { } matching) return;
        Assert.Equal("t", matching.Token);
        Assert.Equal("g", matching.Gateway);
        foreach (var other in new[] { "http://127.0.0.1:9999", "https://evil.example", "null", "", "http://127.0.0.1:61235/" })
        {
            var got = Evaluate(script, other)!;
            Assert.Null(got.Token);
            Assert.Null(got.Who);
            Assert.Null(got.Gateway);
            Assert.Null(got.Console);
        }
    }

    [Fact]
    public void InjectionOriginIsEscaped()
    {
        const string Odd = "http://a'b\"c</script>\u2028";
        var script = Session.InjectionScript(Odd, "g", "c", new SessionInfo("t", "s", null, [], DateTimeOffset.Now));
        Assert.DoesNotContain("</script", script);
        if (Evaluate(script, Odd) is not { } got) return;
        Assert.Equal("t", got.Token);
    }

    // Prepared answers

    [Fact]
    public void InjectionSetsPreparedUrlOnlyWhenGiven()
    {
        var info = new SessionInfo("t", "s", null, [], DateTimeOffset.Now);
        var without = Session.InjectionScript(Origin, "g", "c", info);
        Assert.DoesNotContain("__MIDGARD_PREPARED__", without);
        var url = $"{Origin}/home/prepared-answers.json";
        var script = Session.InjectionScript(Origin, "g", "c", info, url);
        if (Evaluate(without) is not { } none) return;
        Assert.Null(none.Prepared);
        var with = Evaluate(script)!;
        Assert.Equal(url, with.Prepared);
        Assert.Equal("t", with.Token);
        Assert.Null(Evaluate(script, "https://evil.example")!.Prepared);
    }

    [Fact]
    public void InjectionEscapesHostilePreparedUrl()
    {
        var script = Session.InjectionScript(Origin, "g", "c", new SessionInfo("t", "s", null, [], DateTimeOffset.Now), Nasty);
        Assert.DoesNotContain("</script", script);
        Assert.DoesNotContain("\u2028", script);
        if (Evaluate(script) is not { } got) return;
        Assert.Equal(Nasty, got.Prepared);
    }

    [Fact]
    public void PreparedAnswersUrlOnlyWhenTheFileExists()
    {
        var resources = Path.Join(Path.GetTempPath(), $"prepared-{Guid.NewGuid():N}");
        try
        {
            var home = Directory.CreateDirectory(Path.Join(resources, "home")).FullName;
            Assert.Null(Session.PreparedAnswersUrl(resources, Origin));
            var file = Path.Join(home, "prepared-answers.json");
            Directory.CreateDirectory(file);
            Assert.Null(Session.PreparedAnswersUrl(resources, Origin));  // a folder is not the file
            Directory.Delete(file);
            File.WriteAllText(file, "{}");
            Assert.Equal($"{Origin}/home/prepared-answers.json", Session.PreparedAnswersUrl(resources, Origin));
        }
        finally
        {
            Directory.Delete(resources, recursive: true);
        }
    }

    [Fact]
    public void LiteralWritesAsSwiftsJsonSerialization()
    {
        Assert.Equal("\"http:\\/\\/127.0.0.1:5\"", Session.Literal("http://127.0.0.1:5"));
        Assert.Equal("null", Session.Literal(null));
        Assert.Equal("[\"a\",\"b\"]", Session.Literal(new[] { "a", "b" }));
        Assert.Equal("{\"a\":\"1\",\"b\":[]}", Session.Literal(new Dictionary<string, object?> { ["b"] = Array.Empty<string>(), ["a"] = "1" }));
        Assert.Equal("\"\\u003c\\/script\\u003e \\u2028 \\n \\u0000 \\u00e9\"", Session.Literal("</script> \u2028 \n \0 é"));
        // A lone surrogate (not valid UTF-16 on its own) is escaped too, as JavaScript reads it.
        Assert.Equal("\"\\ud800x\"", Session.Literal("\ud800x"));
    }
}

/// <summary>Node.js for evaluating page scripts: required on CI (GitHub's runners carry it), optional elsewhere.</summary>
static class Node
{
    public static string? Find()
    {
        var names = OperatingSystem.IsWindows() ? new[] { "node.exe" } : ["node"];
        var found = (Environment.GetEnvironmentVariable("PATH") ?? "")
            .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries)
            .SelectMany(dir => names.Select(n => Path.Join(dir, n)))
            .FirstOrDefault(File.Exists);
        if (found is null && Environment.GetEnvironmentVariable("CI") is { Length: > 0 })
            Assert.Fail("Node.js is needed on CI to evaluate the page scripts (not found on PATH)");
        return found;
    }

    public static string Run(string node, string script)
    {
        var file = Path.Join(Path.GetTempPath(), $"midgard-script-{Guid.NewGuid():N}.js");
        File.WriteAllText(file, script, new UTF8Encoding(false));
        try
        {
            var start = new ProcessStartInfo(node)
            {
                RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false,
                StandardOutputEncoding = Encoding.UTF8, StandardErrorEncoding = Encoding.UTF8,
            };
            start.ArgumentList.Add(file);
            using var process = Process.Start(start)!;
            var output = process.StandardOutput.ReadToEndAsync();
            var errors = process.StandardError.ReadToEndAsync();
            if (!process.WaitForExit(30_000)) process.Kill();
            Assert.True(process.ExitCode == 0, $"node failed: {errors.Result}");
            return output.Result;
        }
        finally
        {
            File.Delete(file);
        }
    }
}
