using System.Diagnostics;
using System.Globalization;
using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;

namespace Midgard.Core;

/// <summary>
/// One of the presenter's demo accounts, from <c>demo-users.json</c>. Its password is never
/// shown: <see cref="ToString"/> and the debugger's display redact it.
/// </summary>
/// <param name="Label">What the window shows ("Analyst", "Supervisor" …).</param>
[DebuggerDisplay("{ToString(),nq}")]
public sealed record DemoUser(string Username, string Password, string Label)
{
    public override string ToString() => $"DemoUser({Username}, {Label}, password: <redacted>)";
}

public static class DemoUsers
{
    /// <summary>
    /// Reads <c>[{"username","password","label"}]</c>. Throws <see cref="InvalidDataException"/>
    /// when the file is not that shape (a missing or non-string field included).
    /// </summary>
    public static IReadOnlyList<DemoUser> Load(string file)
    {
        try
        {
            using var document = JsonDocument.Parse(File.ReadAllBytes(file));
            if (document.RootElement.ValueKind != JsonValueKind.Array)
                throw new InvalidDataException($"{file}: not a list of demo users");
            var users = new List<DemoUser>();
            foreach (var element in document.RootElement.EnumerateArray())
            {
                string Field(string name) =>
                    element.ValueKind == JsonValueKind.Object && element.TryGetProperty(name, out var value)
                    && value.ValueKind == JsonValueKind.String
                        ? value.GetString()!
                        : throw new InvalidDataException($"{file}: a demo user has no \"{name}\"");
                users.Add(new DemoUser(Field("username"), Field("password"), Field("label")));
            }
            return users;
        }
        catch (JsonException e)
        {
            throw new InvalidDataException($"{file}: not JSON ({e.Message})", e);
        }
    }
}

/// <summary>A signed-in presenter session, as the gateway's <c>/login</c> returns it.</summary>
/// <param name="Token">The hex bearer token.</param>
/// <param name="Clearance">The clearance band the token grants, for display; null if the gateway gave none.</param>
[DebuggerDisplay("{ToString(),nq}")]
public sealed record SessionInfo(string Token, string Subject, string? Clearance, IReadOnlyList<string> Roles,
                                 DateTimeOffset ExpiresAt)
{
    /// <summary>Without the token.</summary>
    public override string ToString() =>
        $"SessionInfo({Subject}, clearance: {Clearance ?? "none"}, roles: [{string.Join(", ", Roles)}], expires: {ExpiresAt:O})";

    public bool Equals(SessionInfo? other) =>
        other is not null && Token == other.Token && Subject == other.Subject && Clearance == other.Clearance
        && Roles.SequenceEqual(other.Roles) && ExpiresAt == other.ExpiresAt;

    public override int GetHashCode() => HashCode.Combine(Token, Subject, Clearance, Roles.Count, ExpiresAt);
}

/// <summary>Why a sign-in failed. The message never carries the password or the token.</summary>
public sealed class SessionException : Exception
{
    public enum Kinds
    {
        /// <summary>The gateway answered 401.</summary>
        InvalidCredentials,
        /// <summary>Any other answer than 200, with the gateway's <c>error</c> message (or the start of the body).</summary>
        Http,
        /// <summary>The gateway could not be reached.</summary>
        Transport,
        /// <summary>A 200 whose body was not a login result.</summary>
        BadResponse,
    }

    public Kinds Kind { get; }
    /// <summary>The HTTP status, for <see cref="Kinds.Http"/>.</summary>
    public int Status { get; }
    /// <summary>The gateway's message, the transport failure or what was wrong with the body.</summary>
    public string Detail { get; }

    SessionException(Kinds kind, string message, int status, string detail, Exception? inner = null)
        : base(message, inner)
    {
        Kind = kind;
        Status = status;
        Detail = detail;
    }

    public static SessionException InvalidCredentials() => new(Kinds.InvalidCredentials, "invalid credentials", 0, "");
    public static SessionException Http(int status, string message) =>
        new(Kinds.Http, $"login failed: HTTP {status}: {message}", status, message);
    public static SessionException Transport(string reason, Exception? inner = null) =>
        new(Kinds.Transport, $"gateway unreachable: {reason}", 0, reason, inner);
    public static SessionException BadResponse(string reason) =>
        new(Kinds.BadResponse, $"unexpected login response: {reason}", 0, reason);

    /// <summary>The message alone (no stack, no inner exception): what shell.log gets.</summary>
    public override string ToString() => Message;
}

/// <summary>
/// Signing the presenter in at the gateway and handing the session to the pages. The port of
/// apps/mac/Sources/MidgardShellCore/Session.swift.
/// </summary>
public static class Session
{
    static HttpClient LoopbackClient(TimeSpan timeout) =>
        new(new SocketsHttpHandler
        {
            UseProxy = false,
            UseCookies = false,
            AllowAutoRedirect = false,
            ConnectTimeout = timeout,
        })
        { Timeout = timeout };

    /// <summary>Loopback only: no proxy, no cookies kept, no redirects, 10 s timeout.</summary>
    public static readonly HttpClient Loopback = LoopbackClient(TimeSpan.FromSeconds(10));

    /// <summary>
    /// Like <see cref="Loopback"/>, but a request may take 60 s: the gateway can be slow to
    /// answer while it is busy storing the services' first syncs, and a slower PC must still
    /// sign in.
    /// </summary>
    public static readonly HttpClient PatientLoopback = LoopbackClient(TimeSpan.FromSeconds(60));

    /// <summary>
    /// <c>POST &lt;gateway&gt;/login {username, password}</c>. <paramref name="gateway"/> is the
    /// API base (<c>http://127.0.0.1:&lt;port&gt;/api/v1</c>; a trailing slash is fine).
    /// </summary>
    /// <exception cref="SessionException"/>
    /// <exception cref="OperationCanceledException"><paramref name="cancellation"/> was cancelled.</exception>
    public static async Task<SessionInfo> LoginAsync(Uri gateway, DemoUser user, HttpClient? client = null,
                                                     CancellationToken cancellation = default)
    {
        cancellation.ThrowIfCancellationRequested();
        client ??= Loopback;
        var url = new Uri(gateway.AbsoluteUri.TrimEnd('/') + "/login");
        var body = JsonSerializer.SerializeToUtf8Bytes(
            new Dictionary<string, string> { ["username"] = user.Username, ["password"] = user.Password });
        using var request = new HttpRequestMessage(HttpMethod.Post, url)
        {
            Content = new ByteArrayContent(body) { Headers = { ContentType = new MediaTypeHeaderValue("application/json") } },
        };
        request.Headers.Accept.Add(new MediaTypeWithQualityHeaderValue("application/json"));

        HttpResponseMessage response;
        byte[] data;
        try
        {
            response = await client.SendAsync(request, cancellation).ConfigureAwait(false);
            data = await response.Content.ReadAsByteArrayAsync(cancellation).ConfigureAwait(false);
        }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested)
        {
            throw;
        }
        catch (Exception e) when (e is HttpRequestException or TaskCanceledException or IOException)
        {
            // TaskCanceledException without our cancellation: the client's timeout.
            throw SessionException.Transport(e is TaskCanceledException ? "timed out" : e.Message, e);
        }
        using (response)
        {
            return response.StatusCode switch
            {
                HttpStatusCode.OK => Parse(data),
                HttpStatusCode.Unauthorized => throw SessionException.InvalidCredentials(),
                _ => throw SessionException.Http((int)response.StatusCode, MessageIn(data)),
            };
        }
    }

    /// <summary>
    /// The soonest a refresh is scheduled, so a short-lived or already-expired token (a
    /// misconfigured TTL, clock skew) cannot drive a tight sign-in loop.
    /// </summary>
    public static readonly TimeSpan MinimumRefreshDelay = TimeSpan.FromSeconds(30);

    /// <summary>
    /// When to sign in again: ten minutes before expiry, or halfway through the remaining
    /// life if that is sooner, but never sooner than <see cref="MinimumRefreshDelay"/> from now.
    /// </summary>
    public static DateTimeOffset RefreshDate(SessionInfo info, DateTimeOffset now)
    {
        var floor = now + MinimumRefreshDelay;
        var remaining = info.ExpiresAt - now;
        if (remaining <= TimeSpan.Zero) return floor;
        var early = TimeSpan.FromTicks(Math.Min(TimeSpan.FromMinutes(10).Ticks, remaining.Ticks / 2));
        var at = info.ExpiresAt - early;
        return at > floor ? at : floor;
    }

    /// <summary>
    /// JavaScript to run at document start in every page: where the gateway and the Midgard
    /// console are, and the signed-in session as <c>AuthGate</c> reads it. It does nothing
    /// unless the page's <c>location.origin</c> is exactly <paramref name="origin"/> (the app's
    /// own web server, e.g. <c>http://127.0.0.1:61235</c>), so the token never reaches another
    /// page. <paramref name="preparedUrl"/>, when given, is where the shipped prepared Mimir
    /// answers are (<c>window.__MIDGARD_PREPARED__</c>, read by Heimdall's console; see
    /// <see cref="PreparedAnswersUrl"/>). Every value is embedded as an escaped JSON literal.
    /// </summary>
    public static string InjectionScript(string origin, string gatewayUrl, string midgardConsoleUrl,
                                         SessionInfo session, string? preparedUrl = null)
    {
        var who = new SortedDictionary<string, object?>(StringComparer.Ordinal)
        {
            ["subject"] = session.Subject,
            ["roles"] = session.Roles,
        };
        if (session.Clearance is { } clearance) who["clearance"] = clearance;
        var prepared = preparedUrl is null ? "" : $"\n  window.__MIDGARD_PREPARED__ = {Literal(preparedUrl)};";
        return "(function () {\n"
             + $"  if (typeof location === 'undefined' || location.origin !== {Literal(origin)}) {{ return; }}\n"
             + $"  window.__MIDGARD_GATEWAY__ = {Literal(gatewayUrl)};\n"
             + $"  window.__MIDGARD_CONSOLE__ = {Literal(midgardConsoleUrl)};{prepared}\n"
             + "  try {\n"
             + $"    sessionStorage.setItem('midgard.session.token', {Literal(session.Token)});\n"
             + $"    sessionStorage.setItem('midgard.session.who', JSON.stringify({Literal(who)}));\n"
             + "  } catch (e) {}\n"
             + "})();";
    }

    /// <summary>
    /// The shipped prepared answers' URL on the app's web server
    /// (<c>&lt;origin&gt;/home/prepared-answers.json</c>) when
    /// <c>resources\home\prepared-answers.json</c> exists as a file, else null (the console
    /// then asks Mimir live).
    /// </summary>
    public static string? PreparedAnswersUrl(string resources, string origin) =>
        File.Exists(Path.Join(resources, "home", "prepared-answers.json")) ? $"{origin}/home/prepared-answers.json" : null;

    /// <summary>
    /// <paramref name="value"/> (a string, a list of strings, a dictionary, a bool or null) as
    /// JSON text that is also a safe JavaScript literal inside a page, as Swift's
    /// JSONSerialization writes it (keys sorted, <c>/</c> as <c>\/</c>), with <c>&lt;</c> and
    /// <c>&gt;</c> escaped (no <c>&lt;/script&gt;</c> or <c>&lt;!--</c>) and every character
    /// outside printable ASCII as <c>\uXXXX</c>: U+2028/U+2029, which older JavaScript does not
    /// allow raw, and lone surrogates included. Only the inside of strings can hold these.
    /// </summary>
    public static string Literal(object? value)
    {
        var text = new StringBuilder();
        Write(text, value);
        return text.ToString();

        static void Write(StringBuilder text, object? value)
        {
            switch (value)
            {
                case null:
                    text.Append("null");
                    break;
                case bool b:
                    text.Append(b ? "true" : "false");
                    break;
                case string s:
                    WriteString(text, s);
                    break;
                case IEnumerable<KeyValuePair<string, object?>> map:
                    text.Append('{');
                    var first = true;
                    foreach (var (key, item) in map.OrderBy(p => p.Key, StringComparer.Ordinal))
                    {
                        if (!first) text.Append(',');
                        first = false;
                        WriteString(text, key);
                        text.Append(':');
                        Write(text, item);
                    }
                    text.Append('}');
                    break;
                case IEnumerable<string> list:
                    text.Append('[');
                    var firstItem = true;
                    foreach (var item in list)
                    {
                        if (!firstItem) text.Append(',');
                        firstItem = false;
                        WriteString(text, item);
                    }
                    text.Append(']');
                    break;
                default:
                    throw new ArgumentException($"cannot write {value.GetType().Name} as a literal", nameof(value));
            }
        }

        static void WriteString(StringBuilder text, string s)
        {
            text.Append('"');
            foreach (var c in s)
            {
                switch (c)
                {
                    case '"': text.Append("\\\""); break;
                    case '\\': text.Append("\\\\"); break;
                    case '/': text.Append("\\/"); break;
                    case '\n': text.Append("\\n"); break;
                    case '\r': text.Append("\\r"); break;
                    case '\t': text.Append("\\t"); break;
                    case '\b': text.Append("\\b"); break;
                    case '\f': text.Append("\\f"); break;
                    case '<': text.Append("\\u003c"); break;
                    case '>': text.Append("\\u003e"); break;
                    default:
                        if (c < 0x20 || c > 0x7E)
                            text.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        else
                            text.Append(c);
                        break;
                }
            }
            text.Append('"');
        }
    }

    static SessionInfo Parse(byte[] data)
    {
        try
        {
            using var document = JsonDocument.Parse(data);
            var root = document.RootElement;
            if (root.ValueKind != JsonValueKind.Object) throw new InvalidDataException("not an object");
            string Required(string name) =>
                root.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String
                    ? v.GetString()!
                    : throw new InvalidDataException($"no \"{name}\"");
            var token = Required("token");
            var subject = Required("subject");
            string? clearance = null;
            if (root.TryGetProperty("clearance", out var c) && c.ValueKind != JsonValueKind.Null)
                clearance = c.ValueKind == JsonValueKind.String ? c.GetString() : throw new InvalidDataException("\"clearance\" is not a string");
            var roles = new List<string>();
            if (root.TryGetProperty("roles", out var r) && r.ValueKind != JsonValueKind.Null)
            {
                if (r.ValueKind != JsonValueKind.Array) throw new InvalidDataException("\"roles\" is not a list");
                foreach (var role in r.EnumerateArray())
                    roles.Add(role.ValueKind == JsonValueKind.String ? role.GetString()! : throw new InvalidDataException("a role is not a string"));
            }
            // The gateway sends unix seconds as a string; a number is accepted too.
            if (!root.TryGetProperty("expiresAt", out var e)) throw new InvalidDataException("no \"expiresAt\"");
            double seconds = e.ValueKind switch
            {
                JsonValueKind.Number => e.GetDouble(),
                JsonValueKind.String when double.TryParse(e.GetString(), NumberStyles.Float, CultureInfo.InvariantCulture, out var n) => n,
                _ => throw new InvalidDataException("expiresAt is not unix seconds"),
            };
            if (double.IsNaN(seconds) || double.IsInfinity(seconds) || Math.Abs(seconds) > 253402300799)
                throw new InvalidDataException("expiresAt is out of range");
            var expires = DateTimeOffset.UnixEpoch + TimeSpan.FromSeconds(seconds);
            return new SessionInfo(token, subject, clearance, roles, expires);
        }
        catch (Exception e) when (e is JsonException or InvalidDataException or ArgumentException or OverflowException)
        {
            throw SessionException.BadResponse(e.Message);
        }
    }

    static string MessageIn(byte[] data)
    {
        try
        {
            using var document = JsonDocument.Parse(data);
            if (document.RootElement.ValueKind == JsonValueKind.Object
                && document.RootElement.TryGetProperty("error", out var error) && error.ValueKind == JsonValueKind.String)
                return error.GetString()!;
        }
        catch (JsonException)
        {
        }
        var text = Encoding.UTF8.GetString(data, 0, Math.Min(200, data.Length));
        return text;
    }
}
