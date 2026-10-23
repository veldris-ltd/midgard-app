using System.Buffers;
using System.Collections.Concurrent;
using System.Globalization;
using System.Net;
using System.Net.Sockets;
using System.Text;

namespace Midgard.Core;

/// <summary>The web server could not listen (the port is taken …).</summary>
public sealed class WebServerStartException(string message, Exception? inner = null) : Exception(message, inner);

/// <summary>
/// A small static file server for the home page and the four consoles' builds, on
/// 127.0.0.1 only. The port of apps/mac/Sources/MidgardShellCore/WebServer.swift. One
/// request per connection, GET and HEAD only: the consoles are plain files and the
/// WebView2 is the only client, so nothing more is needed.
/// <code>
///     /                      root\home\index.html
///     /home/…                root\home\…
///     /&lt;product&gt;/…           root\consoles\&lt;product&gt;\…   (midgard, heimdall, mimir, forge)
///     /media/…               root\recording\media\…      (the recording's camera stills and clips)
///     /radar/…               root\recording\radar\…      (the recording's rain radar frames)
///     /traffic/…             root\recording\traffic\…    (the recording's road traffic flow tiles)
///     /tiles/…               root\recording\tiles\…      (the recording's close-up globe terrain and imagery tiles)
/// </code>
/// </summary>
/// <remarks>
/// <para>The recording's folders (<see cref="RecordingFolders"/>) serve files only: no
/// index page, no redirect, no dot files, and nothing whose real path (links resolved)
/// leaves the folder.</para>
/// <para>A directory serves its index.html; without its trailing slash it is redirected
/// first, because the consoles load <c>./assets/…</c> relative to their own URL. Traversal
/// (<c>..</c>, NUL, backslash, also when percent-encoded) is refused with 403 before the file
/// system is touched, and a <c>Host</c> other than this server (DNS rebinding) gets 421. A
/// single <c>Range: bytes=…</c> is answered with 206; anything else gets the whole file.</para>
/// <para>Windows also refuses (403) any path segment Windows would read as something other
/// than a plain name: a drive letter or alternate data stream (<c>:</c>), the other
/// characters Windows forbids in names, a reserved device name (<c>CON</c>, <c>NUL</c>,
/// <c>COM1</c> …, with or without an extension) and a trailing dot or space, which Windows
/// strips (so <c>..%20</c> would otherwise be <c>..</c>). Symbolic links and junctions must
/// resolve inside their folder, checked by GetFinalPathNameByHandleW on the path and again
/// on the handle of the file actually opened (so a link swapped in between is caught too),
/// compared without regard to case. An 8.3 short name cannot reach a dot file either: the
/// real name is checked.</para>
/// </remarks>
public sealed class WebServer : IDisposable
{
    internal static readonly IReadOnlySet<string> Products =
        new HashSet<string>(StringComparer.Ordinal) { "midgard", "heimdall", "mimir", "forge" };

    /// <summary>URL prefix → the folder of the bundled recording it serves (files only).</summary>
    internal static readonly IReadOnlyDictionary<string, string> RecordingFolders =
        new Dictionary<string, string>(StringComparer.Ordinal)
        {
            ["media"] = Path.Join("recording", "media"),
            ["radar"] = Path.Join("recording", "radar"),
            ["traffic"] = Path.Join("recording", "traffic"),
            ["tiles"] = Path.Join("recording", "tiles"),
        };

    internal const int MaxHead = 16 * 1024;
    internal const int Chunk = 256 * 1024;
    const int ReadSize = 8192;
    /// <summary>
    /// After the response, what the client still sends (a body, the rest of an oversized
    /// head) is read and dropped, up to this much for up to <see cref="DrainTime"/>, so that
    /// closing never resets the connection under a response the client has not read yet.
    /// </summary>
    const int DrainLimit = 1024 * 1024;
    static readonly TimeSpan DrainTime = TimeSpan.FromSeconds(2);

    static readonly StringComparison PathComparison =
        OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;

    /// <summary>The folder served (home\, consoles\, recording\).</summary>
    public string Root { get; }
    /// <summary>The port asked for; 0 lets the system choose (see <see cref="BoundPort"/>).</summary>
    public ushort Port { get; }
    readonly TimeSpan headTimeout;
    readonly TimeSpan stallTimeout;
    readonly Action<Exception>? onFailure;

    readonly object gate = new();
    Socket? listener;
    Task? acceptLoop;
    volatile int bound;
    readonly ConcurrentDictionary<Session, byte> sessions = new();

    /// <param name="root">The folder to serve.</param>
    /// <param name="port">The port to listen on, 0 for any free one.</param>
    /// <param name="headTimeout">A connection that has not sent a whole request head by then is closed (15 s).</param>
    /// <param name="stallTimeout">A response the client stops reading for this long is abandoned (30 s).</param>
    /// <param name="onFailure">Called if the listener fails after <see cref="Start"/>.</param>
    public WebServer(string root, ushort port = 0, TimeSpan? headTimeout = null, TimeSpan? stallTimeout = null,
                     Action<Exception>? onFailure = null)
    {
        Root = Path.TrimEndingDirectorySeparator(Path.GetFullPath(root));
        Port = port;
        this.headTimeout = headTimeout ?? TimeSpan.FromSeconds(15);
        this.stallTimeout = stallTimeout ?? TimeSpan.FromSeconds(30);
        this.onFailure = onFailure;
    }

    /// <summary>The port actually listened on, once <see cref="Start"/> has returned (0 before and after <see cref="Stop"/>).</summary>
    public ushort BoundPort => (ushort)bound;

    /// <summary>The listening socket's address, null when not listening (for the tests).</summary>
    internal IPEndPoint? LocalEndPoint
    {
        get
        {
            lock (gate) return listener?.LocalEndPoint as IPEndPoint;
        }
    }

    /// <summary>Listens on 127.0.0.1:<see cref="Port"/>; throws if it cannot (port in use …).</summary>
    /// <exception cref="WebServerStartException">It could not listen.</exception>
    public void Start()
    {
        lock (gate)
        {
            if (listener is not null) throw new InvalidOperationException("The web server is already listening.");
            // The loopback address itself, so other machines cannot connect at all. No
            // SO_REUSEADDR (Swift's allowLocalEndpointReuse = false): a taken port fails.
            var socket = new Socket(AddressFamily.InterNetwork, SocketType.Stream, ProtocolType.Tcp);
            try
            {
                socket.Bind(new IPEndPoint(IPAddress.Loopback, Port));
                socket.Listen();
            }
            catch (SocketException e)
            {
                socket.Dispose();
                throw new WebServerStartException($"cannot listen on 127.0.0.1:{Port}: {e.Message}", e);
            }
            // Before any connection is accepted, so the Host check sees it.
            bound = ((IPEndPoint)socket.LocalEndPoint!).Port;
            listener = socket;
            acceptLoop = Task.Run(() => AcceptLoop(socket));
        }
    }

    /// <summary>
    /// Stops listening, drops any connection still being served, and returns once the port
    /// is released (so the same port can be started again straight away).
    /// </summary>
    public void Stop()
    {
        Socket? socket;
        Task? loop;
        lock (gate)
        {
            socket = listener;
            loop = acceptLoop;
            listener = null;
            acceptLoop = null;
            bound = 0;
        }
        socket?.Dispose();
        try
        {
            loop?.Wait(TimeSpan.FromSeconds(2));
        }
        catch (AggregateException)
        {
        }
        foreach (var session in sessions.Keys) session.Close();
    }

    public void Dispose() => Stop();

    bool IsCurrent(Socket socket)
    {
        lock (gate) return ReferenceEquals(listener, socket);
    }

    async Task AcceptLoop(Socket socket)
    {
        while (true)
        {
            Socket client;
            try
            {
                client = await socket.AcceptAsync().ConfigureAwait(false);
            }
            catch (SocketException e) when ((e.SocketErrorCode is SocketError.ConnectionReset or SocketError.ConnectionAborted)
                                             && IsCurrent(socket))
            {
                continue;  // a client that gave up while it waited to be accepted
            }
            catch (Exception e) when (e is SocketException or ObjectDisposedException)
            {
                if (IsCurrent(socket))
                {
                    onFailure?.Invoke(e);
                    socket.Dispose();
                }
                return;
            }
            _ = Serve(new Session(client));
        }
    }

    // MARK: Connections

    /// <summary>One connection and its deadline.</summary>
    sealed class Session(Socket socket)
    {
        public Socket Socket { get; } = socket;
        readonly object gate = new();
        Timer? timer;
        bool closed;

        /// <summary>(Re)starts the deadline; when it passes, the connection is dropped.</summary>
        public void Arm(TimeSpan due)
        {
            lock (gate)
            {
                if (closed) return;
                timer ??= new Timer(_ => Close());
                timer.Change(due, Timeout.InfiniteTimeSpan);
            }
        }

        /// <summary>Sends FIN and closes (once); any send or receive still waiting fails.</summary>
        public void Close()
        {
            lock (gate)
            {
                if (closed) return;
                closed = true;
                timer?.Dispose();
            }
            try
            {
                Socket.Shutdown(SocketShutdown.Send);
            }
            catch (Exception e) when (e is SocketException or ObjectDisposedException)
            {
            }
            Socket.Dispose();
        }
    }

    async Task Serve(Session session)
    {
        sessions[session] = 0;
        try
        {
            session.Socket.NoDelay = true;
            session.Arm(headTimeout);
            await ReadHead(session).ConfigureAwait(false);
        }
        catch (Exception)
        {
            // The client went away, a deadline passed, or the server stopped: just close.
        }
        finally
        {
            sessions.TryRemove(session, out _);
            session.Close();
        }
    }

    /// <summary>Reads until the blank line that ends the request head (the body, if any, is ignored).</summary>
    async Task ReadHead(Session session)
    {
        var buffer = new byte[MaxHead + ReadSize];
        var count = 0;
        while (true)
        {
            var n = await session.Socket.ReceiveAsync(buffer.AsMemory(count, ReadSize), SocketFlags.None)
                .ConfigureAwait(false);
            count += n;
            var end = buffer.AsSpan(0, count).IndexOf("\r\n\r\n"u8);
            if (end >= 0 && end <= MaxHead)
            {
                await Respond(session, buffer.AsMemory(0, end)).ConfigureAwait(false);
                return;
            }
            if (count > MaxHead)
            {
                await SendStatus(session, 431, "GET").ConfigureAwait(false);
                return;
            }
            if (n == 0) return;
        }
    }

    async Task Respond(Session session, ReadOnlyMemory<byte> head)
    {
        var lines = Encoding.UTF8.GetString(head.Span).Split("\r\n");
        var parts = lines[0].Split(' ');
        if (parts.Length != 3 || !parts[2].StartsWith("HTTP/", StringComparison.Ordinal))
        {
            await SendStatus(session, 400, "GET").ConfigureAwait(false);
            return;
        }
        var method = parts[0];
        var headers = lines.AsMemory(1);
        if (Header("host", headers.Span) is not { } host)
        {
            await SendStatus(session, 400, method).ConfigureAwait(false);
        }
        else if (!IsOwnHost(host, BoundPort))
        {
            await SendStatus(session, 421, method).ConfigureAwait(false);
        }
        else if (method is not ("GET" or "HEAD"))
        {
            await SendStatus(session, 405, method, new() { ["Allow"] = "GET, HEAD" }).ConfigureAwait(false);
        }
        else
        {
            switch (Resolve(parts[1], Root))
            {
                case Resolution.Failure failure:
                    await SendStatus(session, failure.Status, method).ConfigureAwait(false);
                    break;
                case Resolution.Redirect redirect:
                    await SendStatus(session, 301, method, new() { ["Location"] = redirect.Location }).ConfigureAwait(false);
                    break;
                case Resolution.Success success:
                    await SendFile(session, success, method, Header("range", headers.Span)).ConfigureAwait(false);
                    break;
            }
        }
    }

    async Task SendFile(Session session, Resolution.Success file, string method, string? range)
    {
        FileStream stream;
        try
        {
            stream = new FileStream(file.File, new FileStreamOptions
            {
                Mode = FileMode.Open,
                Access = FileAccess.Read,
                Share = FileShare.Read | FileShare.Delete,
                Options = FileOptions.Asynchronous | FileOptions.SequentialScan,
                BufferSize = 0,
            });
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            await SendStatus(session, 404, method).ConfigureAwait(false);
            return;
        }
        await using (stream.ConfigureAwait(false))
        {
            // What was opened, not only what the name said a moment ago.
            if (OperatingSystem.IsWindows())
            {
                var where = Contain(RealPath.OfHandle(stream.SafeFileHandle), file.Within, file.FilesOnly);
                if (where != Containment.Inside)
                {
                    await SendStatus(session, where == Containment.Hidden ? 404 : 403, method).ConfigureAwait(false);
                    return;
                }
            }
            var total = stream.Length;
            var type = ContentType(file.File);
            var start = 0L;
            var length = total;
            var head = Head(200, type, total, new() { ["Accept-Ranges"] = "bytes" });
            var bytes = range is null ? ByteRange.Whole : ParseRange(range, total);
            if (bytes.Kind == ByteRangeKind.Unsatisfiable)
            {
                await SendStatus(session, 416, method, new() { ["Content-Range"] = $"bytes */{total}" }).ConfigureAwait(false);
                return;
            }
            if (bytes.Kind == ByteRangeKind.Part)
            {
                start = bytes.Start;
                length = bytes.End - bytes.Start;
                head = Head(206, type, length, new()
                {
                    ["Accept-Ranges"] = "bytes",
                    ["Content-Range"] = $"bytes {bytes.Start}-{bytes.End - 1}/{total}",
                });
            }
            if (method != "GET")
            {
                await Finish(session, head).ConfigureAwait(false);
                return;
            }
            stream.Seek(start, SeekOrigin.Begin);
            session.Arm(stallTimeout);
            await SendAll(session, head).ConfigureAwait(false);
            // A chunk at a time, so a large map tile set never sits in memory.
            var chunk = ArrayPool<byte>.Shared.Rent(Chunk);
            try
            {
                var remaining = length;
                while (remaining > 0)
                {
                    var n = await stream.ReadAsync(chunk.AsMemory(0, (int)Math.Min(Chunk, remaining))).ConfigureAwait(false);
                    if (n == 0) break;
                    session.Arm(stallTimeout);
                    await SendAll(session, chunk.AsMemory(0, n)).ConfigureAwait(false);
                    remaining -= n;
                }
            }
            finally
            {
                ArrayPool<byte>.Shared.Return(chunk);
            }
        }
        await Finish(session, null).ConfigureAwait(false);
    }

    Task SendStatus(Session session, int status, string method, Dictionary<string, string>? extra = null)
    {
        var body = Encoding.UTF8.GetBytes($"{status} {Reason(status)}\n");
        var head = Head(status, "text/plain; charset=utf-8", body.Length, extra);
        return Finish(session, method == "HEAD" ? head : head.Concat(body).ToArray());
    }

    static async Task SendAll(Session session, ReadOnlyMemory<byte> data)
    {
        while (!data.IsEmpty)
        {
            var n = await session.Socket.SendAsync(data, SocketFlags.None).ConfigureAwait(false);
            data = data[n..];
        }
    }

    /// <summary>
    /// Sends the last bytes, then FIN, reads and drops whatever the client still sends
    /// until it closes, and closes: one response per connection.
    /// </summary>
    async Task Finish(Session session, byte[]? data)
    {
        session.Arm(stallTimeout);
        if (data is not null) await SendAll(session, data).ConfigureAwait(false);
        session.Socket.Shutdown(SocketShutdown.Send);
        session.Arm(DrainTime < stallTimeout ? DrainTime : stallTimeout);
        var sink = new byte[ReadSize];
        for (var drained = 0; drained < DrainLimit;)
        {
            var n = await session.Socket.ReceiveAsync(sink, SocketFlags.None).ConfigureAwait(false);
            if (n == 0) break;
            drained += n;
        }
        session.Close();
    }

    // MARK: Pure helpers

    /// <summary>What a request target maps to.</summary>
    internal abstract record Resolution
    {
        Resolution() { }

        /// <summary>
        /// Serve <paramref name="File"/>, which must stay inside <paramref name="Within"/>
        /// (and, if <paramref name="FilesOnly"/>, have no dot file in its real name).
        /// </summary>
        public sealed record Success(string File, string Within, bool FilesOnly) : Resolution;

        public sealed record Redirect(string Location) : Resolution;

        public sealed record Failure(int Status) : Resolution;
    }

    /// <summary>Maps a request target to a file under <paramref name="root"/>, a redirect, or the status to answer with.</summary>
    internal static Resolution Resolve(string target, string root)
    {
        root = Path.TrimEndingDirectorySeparator(Path.GetFullPath(root));
        var hash = target.IndexOf('#');
        var withoutFragment = hash < 0 ? target : target[..hash];
        var mark = withoutFragment.IndexOf('?');
        var raw = mark < 0 ? withoutFragment : withoutFragment[..mark];
        var query = mark < 0 ? "" : withoutFragment[mark..];
        if (!raw.StartsWith('/') || PercentDecode(raw) is not { } path) return new Resolution.Failure(400);
        if (path.Contains('\0') || path.Contains('\\')) return new Resolution.Failure(403);
        var segments = path.Split('/', StringSplitOptions.RemoveEmptyEntries);
        if (segments.Contains("..") || segments.Any(IsUnsafeOnWindows)) return new Resolution.Failure(403);

        string file;
        if (segments.Length == 0)
        {
            file = Path.Join(root, "home", "index.html");
        }
        else if (segments[0] == "home")
        {
            file = Under(Path.Join(root, "home"), segments[1..]);
        }
        else if (Products.Contains(segments[0]))
        {
            file = Under(Path.Join(root, "consoles"), segments);
        }
        else if (RecordingFolders.TryGetValue(segments[0], out var folder))
        {
            return ResolveFile(segments[1..], Path.Join(root, folder), root);
        }
        else
        {
            return new Resolution.Failure(404);
        }
        file = Path.GetFullPath(file);
        if (!file.StartsWith(root + Path.DirectorySeparatorChar, PathComparison)) return new Resolution.Failure(403);

        var isDirectory = Directory.Exists(file);
        if (segments.Length > 0 && !path.EndsWith('/') && isDirectory) return new Resolution.Redirect(raw + "/" + query);
        if ((segments.Length > 0 && path.EndsWith('/')) || isDirectory) file = Path.Join(file, "index.html");
        if (!File.Exists(file)) return new Resolution.Failure(404);
        return Contain(RealPath.Of(file), root, filesOnly: false) == Containment.Inside
            ? new Resolution.Success(file, root, false)
            : new Resolution.Failure(403);
    }

    /// <summary>
    /// A file (never a directory) at <paramref name="rest"/> under <paramref name="dir"/>:
    /// nothing hidden (dot files), still under <paramref name="root"/> once made full, and
    /// inside <paramref name="dir"/> with links resolved.
    /// </summary>
    static Resolution ResolveFile(string[] rest, string dir, string root)
    {
        if (rest.Length == 0 || rest.Any(s => s.StartsWith('.'))) return new Resolution.Failure(404);
        var file = Path.GetFullPath(Under(dir, rest));
        if (!file.StartsWith(root + Path.DirectorySeparatorChar, PathComparison)) return new Resolution.Failure(403);
        if (!File.Exists(file)) return new Resolution.Failure(404);
        return Contain(RealPath.Of(file), dir, filesOnly: true) switch
        {
            Containment.Inside => new Resolution.Success(file, dir, true),
            Containment.Hidden => new Resolution.Failure(404),
            _ => new Resolution.Failure(403),
        };
    }

    static string Under(string dir, IEnumerable<string> segments) => segments.Aggregate(dir, (a, s) => Path.Join(a, s));

    internal enum Containment { Inside, Outside, Hidden }

    /// <summary>
    /// Whether <paramref name="realFile"/> (a path with every link resolved) is inside
    /// <paramref name="dir"/> (also resolved): a link pointing out of the served tree is
    /// refused. With <paramref name="filesOnly"/>, a real name below the folder that starts
    /// with a dot counts as hidden (an 8.3 name or a link to a dot file).
    /// </summary>
    static Containment Contain(string? realFile, string dir, bool filesOnly)
    {
        var realDir = RealPath.Of(dir);
        if (realFile is null || realDir is null) return Containment.Outside;
        var prefix = Path.TrimEndingDirectorySeparator(realDir) + Path.DirectorySeparatorChar;
        if (!realFile.StartsWith(prefix, PathComparison)) return Containment.Outside;
        if (filesOnly && realFile[prefix.Length..].Split(Path.DirectorySeparatorChar).Any(s => s.StartsWith('.')))
            return Containment.Hidden;
        return Containment.Inside;
    }

    static readonly IReadOnlySet<string> ReservedNames = new HashSet<string>(
        new[] { "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM¹", "COM²", "COM³",
                "LPT¹", "LPT²", "LPT³" }
            .Concat(Enumerable.Range(0, 10).SelectMany(i => new[] { $"COM{i}", $"LPT{i}" })),
        StringComparer.OrdinalIgnoreCase);

    /// <summary>
    /// A segment Windows would not read as a plain name: a control character or one of
    /// <c>&lt;&gt;:"|?*</c> (<c>:</c> is a drive letter or an alternate data stream), a trailing
    /// dot or space (Windows strips them: <c>.. </c> is <c>..</c>), or a reserved device
    /// name with or without an extension (<c>NUL</c>, <c>con.html</c>, <c>COM1.jpg</c>). A
    /// lone <c>.</c> is the folder itself, as on the Mac.
    /// </summary>
    internal static bool IsUnsafeOnWindows(string segment)
    {
        if (segment == ".") return false;
        if (segment.Any(c => c < ' ' || c == '\x7f' || "<>:\"|?*".Contains(c))) return true;
        if (segment.EndsWith('.') || segment.EndsWith(' ')) return true;
        var stem = segment.Split('.')[0].TrimEnd(' ');
        return ReservedNames.Contains(stem);
    }

    /// <summary>
    /// Percent-decoding as Swift's <c>removingPercentEncoding</c>: null for a <c>%</c> not
    /// followed by two hex digits or bytes that are not UTF-8.
    /// </summary>
    internal static string? PercentDecode(string text)
    {
        var bytes = new List<byte>(text.Length);
        for (var i = 0; i < text.Length;)
        {
            if (text[i] == '%')
            {
                if (i + 2 >= text.Length) return null;
                if (!byte.TryParse(text.AsSpan(i + 1, 2), NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture,
                                   out var b)) return null;
                bytes.Add(b);
                i += 3;
            }
            else
            {
                var next = text.IndexOf('%', i);
                if (next < 0) next = text.Length;
                bytes.AddRange(Encoding.UTF8.GetBytes(text[i..next]));
                i = next;
            }
        }
        try
        {
            return new UTF8Encoding(false, throwOnInvalidBytes: true).GetString(bytes.ToArray());
        }
        catch (ArgumentException)
        {
            return null;
        }
    }

    internal enum ByteRangeKind { Whole, Part, Unsatisfiable }

    /// <summary>A <c>Range</c> header's meaning: the whole file, bytes [Start, End), or 416.</summary>
    internal readonly record struct ByteRange(ByteRangeKind Kind, long Start = 0, long End = 0)
    {
        public static ByteRange Whole => new(ByteRangeKind.Whole);
        public static ByteRange Unsatisfiable => new(ByteRangeKind.Unsatisfiable);
        public static ByteRange Part(long start, long end) => new(ByteRangeKind.Part, start, end);
    }

    static string TrimSpace(string s) => s.Trim(' ', '\t');

    static bool IsDigits(string s) => s.Length > 0 && s.All(char.IsAsciiDigit);

    /// <summary>
    /// A <c>Range</c> header against a file of <paramref name="size"/> bytes: one
    /// <c>bytes=a-b</c>, <c>a-</c> or <c>-n</c> range (clamped to the file); anything else
    /// (several ranges, other units, nonsense) is answered with the whole file, as HTTP
    /// allows. A range starting past the end is 416.
    /// </summary>
    internal static ByteRange ParseRange(string header, long size)
    {
        var value = TrimSpace(header);
        if (!value.StartsWith("bytes=", StringComparison.OrdinalIgnoreCase)) return ByteRange.Whole;
        var spec = TrimSpace(value[6..]);
        var dash = spec.IndexOf('-');
        if (spec.Contains(',') || dash < 0) return ByteRange.Whole;
        var first = TrimSpace(spec[..dash]);
        var last = TrimSpace(spec[(dash + 1)..]);
        static bool Number(string s, out long n) =>
            long.TryParse(s, NumberStyles.None, CultureInfo.InvariantCulture, out n);
        if (first.Length == 0)
        {
            if (!IsDigits(last) || !Number(last, out var n)) return ByteRange.Whole;
            if (n <= 0 || size <= 0) return ByteRange.Unsatisfiable;
            return ByteRange.Part(Math.Max(0, size - n), size);
        }
        if (!IsDigits(first) || !Number(first, out var a)) return ByteRange.Whole;
        if (a >= size) return ByteRange.Unsatisfiable;
        if (last.Length == 0) return ByteRange.Part(a, size);
        if (!IsDigits(last) || !Number(last, out var b)) return ByteRange.Whole;
        if (b < a) return ByteRange.Whole;
        return ByteRange.Part(a, Math.Min(b, size - 1) + 1);
    }

    /// <summary>The value of the first header called <paramref name="name"/> (lower case), if any.</summary>
    internal static string? Header(string name, ReadOnlySpan<string> lines)
    {
        foreach (var line in lines)
        {
            var colon = line.IndexOf(':');
            if (colon < 0 || line[..colon].ToLowerInvariant() != name) continue;
            return TrimSpace(line[(colon + 1)..]);
        }
        return null;
    }

    /// <summary>
    /// Only our own origin may be asked for, so a page on another site that rebinds its
    /// name to 127.0.0.1 cannot read the consoles.
    /// </summary>
    internal static bool IsOwnHost(string host, ushort port)
    {
        host = host.ToLowerInvariant();
        return host == $"127.0.0.1:{port}" || host == $"localhost:{port}";
    }

    internal static string ContentType(string file) =>
        Path.GetExtension(file).TrimStart('.').ToLowerInvariant() switch
        {
            "html" or "htm" => "text/html; charset=utf-8",
            "js" or "mjs" => "text/javascript",
            "css" => "text/css",
            "json" or "map" => "application/json",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "jpg" or "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "mp4" or "m4v" => "video/mp4",
            "webm" => "video/webm",
            "webp" => "image/webp",
            "ttf" => "font/ttf",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "wasm" => "application/wasm",
            "pbf" => "application/x-protobuf",
            "ico" => "image/x-icon",
            "txt" => "text/plain; charset=utf-8",
            _ => "application/octet-stream",
        };

    internal static byte[] Head(int status, string type, long length, Dictionary<string, string>? extra = null)
    {
        var lines = new List<string>
        {
            $"HTTP/1.1 {status} {Reason(status)}",
            $"Content-Type: {type}",
            $"Content-Length: {length}",
            "Cache-Control: no-store",
            "Connection: close",
            "X-Content-Type-Options: nosniff",
        };
        if (extra is not null)
            lines.AddRange(extra.OrderBy(kv => kv.Key, StringComparer.Ordinal).Select(kv => $"{kv.Key}: {kv.Value}"));
        return Encoding.UTF8.GetBytes(string.Join("\r\n", lines) + "\r\n\r\n");
    }

    internal static string Reason(int status) => status switch
    {
        200 => "OK",
        206 => "Partial Content",
        301 => "Moved Permanently",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        416 => "Range Not Satisfiable",
        421 => "Misdirected Request",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
}
