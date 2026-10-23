using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of apps/mac/Tests/MidgardShellCoreTests/ServiceSpecTests.swift (the model
// server's own arguments are Task 4's), with Windows' .exe names and folders, and the
// ports wired as run-suite.py wires them.
public sealed class ServiceSpecTests
{
    static readonly string Root = OperatingSystem.IsWindows() ? @"C:\M" : "/M";
    static readonly string Helpers = Path.Join(Root, "helpers");
    static readonly string Models = Path.Join(Root, "resources", "models");
    static readonly string Home = Path.Join(Root, "Home");
    static readonly string Logs = Path.Join(Root, "Logs");
    static readonly string Recording = Path.Join(Root, "resources", "recording");
    static readonly SuitePorts Ports = new(1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009);
    const string Origin = "http://127.0.0.1:2000";
    static readonly Command Llama = new(Path.Join(Helpers, "llama", "llama-server.exe"), ["-m", "model.gguf"]);

    static string H(params string[] parts) => parts.Aggregate(Home, (a, p) => Path.Join(a, p));
    static string M(params string[] parts) => parts.Aggregate(Models, (a, p) => Path.Join(a, p));

    static IReadOnlyList<ServiceSpec> Specs(string? recording = null) =>
        SuiteSpecs.Make(Helpers, Models, Home, Logs, Ports, Origin, recording, Llama);

    static ServiceSpec Spec(string name, IReadOnlyList<ServiceSpec>? all = null) =>
        Assert.Single(all ?? Specs(Recording), s => s.Name == name);

    [Fact]
    public void StartOrderWithoutRecording() =>
        Assert.Equal(new[] { "llama", "mimir", "gateway", "heimdall", "forge" }, Specs().Select(s => s.Name));

    [Fact]
    public void ReplayOnlyWithRecording()
    {
        var all = Specs(Recording);
        Assert.Equal(new[] { "llama", "mimir", "gateway", "heimdall", "forge", "replay" }, all.Select(s => s.Name));
        var replay = Spec("replay", all);
        Assert.Equal(Path.Join(Helpers, "midgard-replay.exe"), replay.Executable);
        Assert.Equal(new[] {
            "--recording", Recording, "--identity-dir", H("airports-mesh", "feeds"),
            "--forge-dir", H("live-feeds", "data"), "--addr", "127.0.0.1:1006",
        }, replay.Arguments);
        Assert.Equal(new Readiness.LogLine("\"replay started\""), replay.Readiness);
        Assert.Equal(Path.Join(Logs, "replay.log"), replay.LogFile);
    }

    [Fact]
    public void LlamaIsTheModelServersCommandReadyByHealth()
    {
        var llama = Spec("llama");
        Assert.Equal(Llama.Executable, llama.Executable);
        Assert.Equal(Llama.Arguments, llama.Arguments);
        Assert.Equal(new Readiness.Health(new Uri("http://127.0.0.1:1008/health")), llama.Readiness);
        Assert.Equal(Path.Join(Logs, "llama.log"), llama.LogFile);
    }

    [Fact]
    public void Mimir()
    {
        var mimir = Spec("mimir");
        Assert.Equal(Path.Join(Helpers, "mimir-server.exe"), mimir.Executable);
        Assert.Equal(new[] {
            "--identity-dir", H("airports-mesh", "mimir"), "--state-dir", H("mimir-state"),
            "--purposes", "investigation",
            "--bind", "127.0.0.1:1003", "--health-bind", "127.0.0.1:1005", "--observe-bind", "127.0.0.1:1004",
            "--engine", "llama-cpp", "--model-dir", M("granite"),
            "--model-trust", M("trust", "model-signing.pub"), "--sidecar", "127.0.0.1:1008",
        }, mimir.Arguments);
        Assert.Equal(new Readiness.LogLine("\"serving\""), mimir.Readiness);
    }

    [Fact]
    public void Gateway()
    {
        var gateway = Spec("gateway");
        Assert.Equal(Path.Join(Helpers, "http-gateway.exe"), gateway.Executable);
        Assert.Equal(new[] {
            "--state-dir", H("airports"), "--bind", "127.0.0.1:1001",
            "--allow-origin", "http://127.0.0.1:2000", "--token-ttl", "43200",
            "--mesh-bind", "127.0.0.1:1002", "--mesh-identity-dir", H("airports-mesh", "gateway"),
            "--heimdall-addr", "127.0.0.1:1006", "--heimdall-identity-dir", H("airports-mesh", "feeds"),
            "--mimir-observe-addr", "127.0.0.1:1004", "--mimir-identity-dir", H("airports-mesh", "mimir-observer"),
            "--forge-jobs", H("live-feeds", "data", "jobs.json"),
            "--forge-status", H("live-feeds", "worker-state", "status.json"),
            "--heimdall-feeds-status", H("heimdall-poc", "feeds", "state", "status.json"),
        }, gateway.Arguments);
        Assert.Equal(new Readiness.LogLine("listening on"), gateway.Readiness);
    }

    [Fact]
    public void Heimdall()
    {
        var heimdall = Spec("heimdall");
        Assert.Equal(Path.Join(Helpers, "heimdall-feed.exe"), heimdall.Executable);
        Assert.Equal(new[] {
            "--identity-dir", H("airports-mesh", "heimdall"), "--state-dir", H("heimdall-poc", "state"),
            "--bind", "127.0.0.1:1006", "--health-bind", "127.0.0.1:1007", "--track-ttl", "300",
            "--mimir-addr", "127.0.0.1:1003", "--mimir-identity-dir", H("airports-mesh", "heimdall-advisor"),
            "--fabric-addr", "127.0.0.1:1002", "--fabric-identity-dir", H("airports-mesh", "heimdall-fabric"),
        }, heimdall.Arguments);
        Assert.Equal(new Readiness.LogLine("\"serving\""), heimdall.Readiness);
    }

    [Fact]
    public void Forge()
    {
        var forge = Spec("forge");
        Assert.Equal(Path.Join(Helpers, "forge-worker.exe"), forge.Executable);
        Assert.Equal(new[] {
            "--identity-dir", H("airports-mesh", "forge"), "--state-dir", H("live-feeds", "worker-state"),
            "--jobs", H("live-feeds", "data", "jobs.json"), "--gateway", "127.0.0.1:1002",
        }, forge.Arguments);
        Assert.Equal(new Readiness.LogLine("\"message\":\"job run\""), forge.Readiness);
    }

    /// <summary>
    /// Every listener and its clients agree: each service binds its own ports, and every
    /// address another service is given is one that service binds.
    /// </summary>
    [Fact]
    public void PortsAreWiredBetweenTheServices()
    {
        var all = Specs(Recording);
        string Value(string service, string flag)
        {
            var arguments = Spec(service, all).Arguments.ToList();
            var at = arguments.IndexOf(flag);
            Assert.True(at >= 0, $"{service} has no {flag}");
            return arguments[at + 1];
        }
        var llamaHealth = ((Readiness.Health)Spec("llama", all).Readiness).Url;
        Assert.Equal($"127.0.0.1:{llamaHealth.Port}", Value("mimir", "--sidecar"));
        Assert.Equal(Value("mimir", "--bind"), Value("heimdall", "--mimir-addr"));
        Assert.Equal(Value("mimir", "--observe-bind"), Value("gateway", "--mimir-observe-addr"));
        Assert.Equal(Value("gateway", "--mesh-bind"), Value("heimdall", "--fabric-addr"));
        Assert.Equal(Value("gateway", "--mesh-bind"), Value("forge", "--gateway"));
        Assert.Equal(Value("heimdall", "--bind"), Value("gateway", "--heimdall-addr"));
        Assert.Equal(Value("heimdall", "--bind"), Value("replay", "--addr"));
        Assert.Equal(Value("gateway", "--forge-jobs"), Value("forge", "--jobs"));
        Assert.Equal(Value("gateway", "--forge-status"), Path.Join(Value("forge", "--state-dir"), "status.json"));

        // Each port is bound by one service only.
        var binds = new[]
        {
            Value("gateway", "--bind"), Value("gateway", "--mesh-bind"), Value("mimir", "--bind"),
            Value("mimir", "--health-bind"), Value("mimir", "--observe-bind"), Value("heimdall", "--bind"),
            Value("heimdall", "--health-bind"), $"127.0.0.1:{llamaHealth.Port}",
        };
        Assert.Equal(binds.Length, binds.Distinct().Count());
        Assert.All(binds, b => Assert.StartsWith("127.0.0.1:", b));
        // The analytics' port (W3) is the only one not used yet.
        Assert.Equal(new[] { Ports.Analytics },
                     Ports.All.Where(p => !binds.Contains($"127.0.0.1:{p}")).ToArray());
    }

    [Fact]
    public void EveryServiceHas180SecondsToBeReadyAndIsRequired()
    {
        Assert.All(Specs(Recording), s => Assert.Equal(TimeSpan.FromSeconds(180), s.ReadyTimeout));
        Assert.All(Specs(Recording), s => Assert.False(s.Optional));
        Assert.All(Specs(Recording), s => Assert.Null(s.Environment));
        Assert.Equal(TimeSpan.FromSeconds(180),
                     new ServiceSpec("x", "x.exe", [], new Readiness.LogLine("x"), "x.log").ReadyTimeout);
    }

    [Fact]
    public void LogsAreOnePerServiceInTheLogsFolder() =>
        Assert.Equal(Specs(Recording).Select(s => Path.Join(Logs, s.Name + ".log")), Specs(Recording).Select(s => s.LogFile));

    [Fact]
    public void SpecsCompareByTheirArguments()
    {
        Assert.Equal(Specs(Recording), Specs(Recording));
        Assert.NotEqual(Spec("gateway"), Spec("gateway") with { Arguments = ["--other"] });
    }

    [Fact]
    public void SuitePortsFromAList()
    {
        Assert.Null(SuitePorts.From([1, 2, 3]));
        var ports = SuitePorts.From([1, 2, 3, 4, 5, 6, 7, 8, 9])!;
        Assert.Equal(new ushort[] { 1, 2, 3, 4, 5, 6, 7, 8, 9 }, ports.All);
        Assert.Equal(1, ports.GatewayHttp);
        Assert.Equal(8, ports.Llama);
        var free = SuitePorts.Free();
        Assert.Equal(SuitePorts.Count, free.All.Distinct().Count());
    }
}
