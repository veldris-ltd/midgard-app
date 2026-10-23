using Midgard.Core;

namespace Midgard.Core.Tests;

// The cases of apps/mac/Tests/MidgardShellCoreTests/ModelServerTests.swift that apply to
// Windows (its Intel path: CPU only, one build), with the thread cap of 8 and MIDGARD_LLAMA
// for a development layout in place of Homebrew's server.
public sealed class ModelServerTests
{
    static readonly string Root = OperatingSystem.IsWindows() ? @"C:\M" : "/M";
    static readonly string Helpers = Path.Join(Root, "helpers");
    static readonly string Models = Path.Join(Root, "resources", "models");
    static readonly string Bundled = Path.Join(Helpers, "llama", "llama-server.exe");
    static readonly Dictionary<string, string> NoEnvironment = [];

    static Command Make(int threads) => ModelServer.Make(Bundled, Models, 4321, threads);

    static IReadOnlyList<string> Tail(Command command)
    {
        var i = command.Arguments.ToList().IndexOf("--n-gpu-layers");
        Assert.True(i >= 0);
        return command.Arguments.Skip(i).ToList();
    }

    static int Threads(int? cores, int logical = 99, Dictionary<string, string>? environment = null) =>
        ModelServer.Threads(cores, logical, environment ?? NoEnvironment);

    [Theory]
    [InlineData(4, 4)]
    [InlineData(8, 8)]
    [InlineData(16, 8)]
    [InlineData(2, 2)]
    [InlineData(6, 6)]
    public void ThreadsArePhysicalCoresUpToEight(int cores, int threads)
    {
        Assert.Equal(threads, Threads(cores));
        Assert.Equal(new[] { "--n-gpu-layers", "0", "--threads", $"{threads}" }, Tail(Make(Threads(cores))));
    }

    [Fact]
    public void AtLeastOneThread()
    {
        Assert.Equal(1, Threads(0));
        Assert.Equal(new[] { "--n-gpu-layers", "0", "--threads", "1" }, Tail(Make(0)));
    }

    [Fact]
    public void UnknownCoresFallBackToLogicalProcessors()
    {
        Assert.Equal(4, Threads(null, logical: 4));
        Assert.Equal(8, Threads(null, logical: 32));
    }

    [Fact]
    public void ModelThreadsOverride()
    {
        Assert.Equal(4, Threads(2, environment: new() { ["MIDGARD_MODEL_THREADS"] = "4" }));
        Assert.Equal(4, Threads(16, environment: new() { ["MIDGARD_MODEL_THREADS"] = "4" }));
        // Honoured as given, past the default cap too.
        Assert.Equal(12, Threads(16, environment: new() { ["MIDGARD_MODEL_THREADS"] = "12" }));
        // Not a positive number: ignored.
        foreach (var bad in new[] { "", "0", "-2", "four", " 4", "4.0", "99999999999" })
            Assert.Equal(2, Threads(2, environment: new() { ["MIDGARD_MODEL_THREADS"] = bad }));
    }

    [Fact]
    public void CommonArguments()
    {
        var command = Make(4);
        Assert.Equal(Bundled, command.Executable);
        Assert.Equal(new[] {
            "-m", Path.Join(Models, "granite", "granite-3.3-2b-instruct-Q4_K_M.gguf"),
            "--alias", "granite-3.3-2b-instruct-q4km",
            "--host", "127.0.0.1", "--port", "4321", "--ctx-size", "8192",
            "--parallel", "1", "--temp", "0.2", "--json-schema", "{\"type\":\"object\"}",
            "--n-gpu-layers", "0", "--threads", "4",
        }, command.Arguments);
        Assert.DoesNotContain("--cont-batching", command.Arguments);
    }

    [Fact]
    public void SuiteSpecsRunTheCommand()
    {
        var ports = new SuitePorts(1, 2, 3, 4, 5, 6, 7, 4321, 9);
        var specs = SuiteSpecs.Make(Helpers, Models, Path.Join(Root, "Home"), Path.Join(Root, "Logs"), ports,
                                    "http://127.0.0.1:2000", null, Make(4));
        var llama = Assert.Single(specs, s => s.Name == "llama");
        Assert.Equal(Bundled, llama.Executable);
        Assert.Equal(Make(4).Arguments, llama.Arguments);
        Assert.Equal(new Readiness.Health(new Uri("http://127.0.0.1:4321/health")), llama.Readiness);
    }

    // Locating the executable

    static string? Locate(bool devLayout, IEnumerable<string> present, Dictionary<string, string>? environment = null)
    {
        var set = present.ToHashSet();
        return ModelServer.Locate(Helpers, devLayout, environment ?? NoEnvironment, set.Contains);
    }

    static readonly string DevServer = Path.Join(Root, "dev", "llama-server.exe");
    static readonly string Flat = Path.Join(Helpers, "llama-server.exe");

    [Fact]
    public void PackageUsesOnlyTheBundledServer()
    {
        var all = new[] { Bundled, Flat, DevServer };
        Assert.Equal(Bundled, Locate(false, all));
        // Missing from an installed app: no fallback, and MIDGARD_LLAMA is not honoured.
        Assert.Null(Locate(false, [Flat, DevServer]));
        Assert.Equal(Bundled, Locate(false, all, new() { ["MIDGARD_LLAMA"] = DevServer }));
        Assert.Null(Locate(false, [DevServer], new() { ["MIDGARD_LLAMA"] = DevServer }));
    }

    [Fact]
    public void DevLayoutFallsBack()
    {
        Assert.Equal(Bundled, Locate(true, [Bundled, Flat]));
        Assert.Equal(Flat, Locate(true, [Flat]));
        Assert.Null(Locate(true, []));
    }

    [Fact]
    public void MidgardLlamaOverridesInADevLayout()
    {
        var env = new Dictionary<string, string> { ["MIDGARD_LLAMA"] = DevServer };
        Assert.Equal(DevServer, Locate(true, [Bundled, Flat, DevServer], env));
        // Set but missing: nothing else is run in its place.
        Assert.Null(Locate(true, [Bundled, Flat], env));
        // Empty counts as unset.
        Assert.Equal(Bundled, Locate(true, [Bundled], new() { ["MIDGARD_LLAMA"] = "" }));
    }

    [Fact]
    public void ThisPc()
    {
        var cores = ModelServer.PhysicalCores();
        if (OperatingSystem.IsWindows())
        {
            Assert.NotNull(cores);
            Assert.InRange(cores.Value, 1, Environment.ProcessorCount);
        }
        Assert.InRange(ModelServer.ThreadsForThisPc(NoEnvironment), 1, ModelServer.MaxThreads);
        Assert.Equal(3, ModelServer.ThreadsForThisPc(new Dictionary<string, string> { ["MIDGARD_MODEL_THREADS"] = "3" }));
    }
}
