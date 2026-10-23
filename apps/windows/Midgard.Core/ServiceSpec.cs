namespace Midgard.Core;

/// <summary>How to tell that a service is ready.</summary>
public abstract record Readiness
{
    Readiness() { }

    /// <summary>Ready once this text appears in output written after the launch.</summary>
    public sealed record LogLine(string Text) : Readiness;

    /// <summary>Ready once a GET of this URL answers 200.</summary>
    public sealed record Health(Uri Url) : Readiness;
}

/// <summary>
/// How to run one native service and how to tell it is ready. The port of
/// <c>ServiceSpec</c> in apps/mac/Sources/MidgardShellCore/ServiceSpec.swift.
/// </summary>
/// <param name="Name">The service's name (also its log's name).</param>
/// <param name="Executable">The full path of the .exe.</param>
/// <param name="Arguments">Its arguments, quoted for Windows by the launcher.</param>
/// <param name="Readiness">When it counts as ready.</param>
/// <param name="LogFile">stdout and stderr are appended here (created if needed).</param>
public sealed record ServiceSpec(string Name, string Executable, IReadOnlyList<string> Arguments,
                                 Readiness Readiness, string LogFile)
{
    /// <summary>
    /// From launch to ready before the start counts as failed: 180 s, as run-native.sh
    /// allows (a first start on a slow PC can take a while).
    /// </summary>
    public TimeSpan ReadyTimeout { get; init; } = TimeSpan.FromSeconds(180);

    /// <summary>Variables set on top of the shell's own environment; null inherits it unchanged.</summary>
    public IReadOnlyDictionary<string, string>? Environment { get; init; }

    /// <summary>
    /// Not needed for the suite to start: <see cref="Supervisor.StartAsync"/> launches it and
    /// returns without waiting for it; if it never becomes ready it is published
    /// <see cref="ServiceStatus.Failed"/> and nothing else stops (the camera analytics, W3).
    /// </summary>
    public bool Optional { get; init; }

    // Records compare lists by reference; a spec is equal to another with the same arguments.
    public bool Equals(ServiceSpec? other) =>
        other is not null && Name == other.Name && Executable == other.Executable
        && Arguments.SequenceEqual(other.Arguments) && Readiness == other.Readiness
        && LogFile == other.LogFile && ReadyTimeout == other.ReadyTimeout && Optional == other.Optional
        && SameEnvironment(Environment, other.Environment);

    static bool SameEnvironment(IReadOnlyDictionary<string, string>? a, IReadOnlyDictionary<string, string>? b)
    {
        if (a is null || b is null) return a is null && b is null;
        return a.Count == b.Count && a.All(kv => b.TryGetValue(kv.Key, out var v) && v == kv.Value);
    }

    public override int GetHashCode() => HashCode.Combine(Name, Executable, Arguments.Count, Readiness, LogFile);
}

/// <summary>The loopback ports the shell chooses for the suite (the web server picks its own).</summary>
public sealed record SuitePorts(ushort GatewayHttp, ushort GatewayMesh, ushort MimirMesh, ushort MimirObserver,
                                ushort MimirHealth, ushort HeimdallMesh, ushort HeimdallHealth, ushort Llama,
                                ushort Analytics)
{
    /// <summary>How many ports the suite needs.</summary>
    public const int Count = 9;

    /// <summary>From exactly <see cref="Count"/> ports, in the order of the properties; null for any other count.</summary>
    public static SuitePorts? From(IReadOnlyList<ushort> ports) =>
        ports.Count != Count ? null
            : new SuitePorts(ports[0], ports[1], ports[2], ports[3], ports[4], ports[5], ports[6], ports[7], ports[8]);

    /// <summary>Fresh free ports (see <see cref="Ports.Free"/>).</summary>
    public static SuitePorts Free() => From(Ports.Free(Count))!;

    public IReadOnlyList<ushort> All =>
        [GatewayHttp, GatewayMesh, MimirMesh, MimirObserver, MimirHealth, HeimdallMesh, HeimdallHealth, Llama, Analytics];
}

/// <summary>An executable and its arguments (the model server's, from ModelServer in Task 4).</summary>
public sealed record Command(string Executable, IReadOnlyList<string> Arguments);

/// <summary>
/// The suite's services as tools/mac-app/run-native.sh and tools/win-app/run-suite.py run
/// them, with the shell's ports. The port of <c>SuiteSpecs</c> in ServiceSpec.swift:
/// llama-server is one of them, the model files come from <c>models</c> rather than the
/// home, and the gateway allows only the shell's web origin and issues 12-hour tokens.
/// The camera analytics (last and optional) are added in W3.
/// </summary>
public static class SuiteSpecs
{
    /// <summary>The service executables in <c>helpers\</c>, by service name.</summary>
    public static readonly IReadOnlyDictionary<string, string> Executables = new Dictionary<string, string>
    {
        ["mimir"] = "mimir-server.exe",
        ["gateway"] = "http-gateway.exe",
        ["heimdall"] = "heimdall-feed.exe",
        ["forge"] = "forge-worker.exe",
        ["replay"] = "midgard-replay.exe",
    };

    /// <summary>
    /// In start order: llama, mimir, gateway, heimdall, forge, then replay if
    /// <paramref name="recording"/>. <paramref name="llama"/> is the model server's command
    /// (ModelServer); the rest run from <paramref name="helpers"/>.
    /// </summary>
    public static IReadOnlyList<ServiceSpec> Make(string helpers, string models, string home, string logs,
                                                  SuitePorts ports, string webOrigin, string? recording,
                                                  Command llama)
    {
        static string Under(string root, string[] parts) => parts.Aggregate(root, (a, p) => Path.Join(a, p));
        string H(params string[] parts) => Under(home, parts);
        string M(params string[] parts) => Under(models, parts);
        static string Local(ushort port) => $"127.0.0.1:{port}";
        ServiceSpec Spec(string name, string executable, Readiness readiness, IReadOnlyList<string> arguments) =>
            new(name, executable, arguments, readiness, Path.Join(logs, $"{name}.log"));
        ServiceSpec Helper(string name, Readiness readiness, IReadOnlyList<string> arguments) =>
            Spec(name, Path.Join(helpers, Executables[name]), readiness, arguments);

        var specs = new List<ServiceSpec>
        {
            Spec("llama", llama.Executable, new Readiness.Health(new Uri($"http://{Local(ports.Llama)}/health")),
                 llama.Arguments),
            Helper("mimir", new Readiness.LogLine("\"serving\""), [
                "--identity-dir", H("airports-mesh", "mimir"), "--state-dir", H("mimir-state"),
                "--purposes", "investigation",
                "--bind", Local(ports.MimirMesh), "--health-bind", Local(ports.MimirHealth),
                "--observe-bind", Local(ports.MimirObserver),
                "--engine", "llama-cpp", "--model-dir", M("granite"),
                "--model-trust", M("trust", "model-signing.pub"), "--sidecar", Local(ports.Llama),
            ]),
            // W3 adds --analytics-addr and --analytics-token-file when the analytics are bundled.
            Helper("gateway", new Readiness.LogLine("listening on"), [
                "--state-dir", H("airports"), "--bind", Local(ports.GatewayHttp),
                "--allow-origin", webOrigin, "--token-ttl", "43200",
                "--mesh-bind", Local(ports.GatewayMesh), "--mesh-identity-dir", H("airports-mesh", "gateway"),
                "--heimdall-addr", Local(ports.HeimdallMesh), "--heimdall-identity-dir", H("airports-mesh", "feeds"),
                "--mimir-observe-addr", Local(ports.MimirObserver),
                "--mimir-identity-dir", H("airports-mesh", "mimir-observer"),
                "--forge-jobs", H("live-feeds", "data", "jobs.json"),
                "--forge-status", H("live-feeds", "worker-state", "status.json"),
                "--heimdall-feeds-status", H("heimdall-poc", "feeds", "state", "status.json"),
            ]),
            Helper("heimdall", new Readiness.LogLine("\"serving\""), [
                "--identity-dir", H("airports-mesh", "heimdall"), "--state-dir", H("heimdall-poc", "state"),
                "--bind", Local(ports.HeimdallMesh), "--health-bind", Local(ports.HeimdallHealth),
                "--track-ttl", "300",
                "--mimir-addr", Local(ports.MimirMesh), "--mimir-identity-dir", H("airports-mesh", "heimdall-advisor"),
                "--fabric-addr", Local(ports.GatewayMesh), "--fabric-identity-dir", H("airports-mesh", "heimdall-fabric"),
            ]),
            // Ready on the first successful job run ("job run failed" does not match).
            Helper("forge", new Readiness.LogLine("\"message\":\"job run\""), [
                "--identity-dir", H("airports-mesh", "forge"), "--state-dir", H("live-feeds", "worker-state"),
                "--jobs", H("live-feeds", "data", "jobs.json"), "--gateway", Local(ports.GatewayMesh),
            ]),
        };
        if (recording is not null)
        {
            // Replay runs heimdall-source.exe from its own folder (helpers\) by default.
            specs.Add(Helper("replay", new Readiness.LogLine("\"replay started\""), [
                "--recording", recording, "--identity-dir", H("airports-mesh", "feeds"),
                "--forge-dir", H("live-feeds", "data"),
                // Heimdall's mesh port: replay's default (7730) is run-native.sh's fixed port.
                "--addr", Local(ports.HeimdallMesh),
            ]));
        }
        return specs;
    }
}
