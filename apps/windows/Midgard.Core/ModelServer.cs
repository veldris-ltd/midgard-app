using System.Globalization;
using System.Runtime.Versioning;

namespace Midgard.Core;

/// <summary>
/// Which llama-server the shell runs and how: the official llama.cpp Windows x64 CPU
/// build, <c>helpers\llama\llama-server.exe</c> (W4 fetches it), CPU only with one thread
/// per physical core up to 8. The port of apps/mac/Sources/MidgardShellCore/ModelServer.swift
/// (its Intel path: there is no GPU build on Windows). Kept pure, the hardware facts are
/// inputs; <see cref="PhysicalCores"/> reads the real ones.
/// </summary>
public static class ModelServer
{
    /// <summary>The model file, relative to the models folder.</summary>
    public const string Model = "granite/granite-3.3-2b-instruct-Q4_K_M.gguf";

    /// <summary>The name mimir-server asks llama-server for.</summary>
    public const string Alias = "granite-3.3-2b-instruct-q4km";

    /// <summary>
    /// The most threads given to the model by default: past 8 a 2B model gains next to
    /// nothing on the CPU and the rest of the suite loses cores. MIDGARD_MODEL_THREADS
    /// may ask for more.
    /// </summary>
    public const int MaxThreads = 8;

    /// <summary>The environment keys <see cref="ModelServer"/> reads.</summary>
    public static readonly IReadOnlyList<string> EnvironmentKeys = ["MIDGARD_LLAMA", "MIDGARD_MODEL_THREADS"];

    /// <summary>The bundled server: <c>helpers\llama\llama-server.exe</c> (its DLLs sit next to it).</summary>
    public static string BundledExecutable(string helpers) => Path.Join(helpers, "llama", "llama-server.exe");

    /// <summary>
    /// Where to find llama-server. In a package only the bundled one, null if it is
    /// missing, so a damaged install says so rather than running something else (as on the
    /// Mac). A development layout (<see cref="Paths.DevLayout"/>) runs MIDGARD_LLAMA when it
    /// is set (null if that file is missing: no quiet fallback), else the bundled one, else
    /// <c>helpers\llama-server.exe</c>.
    /// </summary>
    public static string? Locate(string helpers, bool devLayout, IReadOnlyDictionary<string, string> environment,
                                 Func<string, bool> exists)
    {
        var candidates = new List<string>();
        if (devLayout && environment.TryGetValue("MIDGARD_LLAMA", out var dev) && !string.IsNullOrEmpty(dev))
        {
            candidates.Add(Path.GetFullPath(dev));
        }
        else
        {
            candidates.Add(BundledExecutable(helpers));
            if (devLayout) candidates.Add(Path.Join(helpers, "llama-server.exe"));
        }
        return candidates.FirstOrDefault(exists);
    }

    /// <summary>
    /// The thread count: MIDGARD_MODEL_THREADS when it is a positive number (honoured as
    /// given, as on the Mac), else the physical cores (<paramref name="logicalProcessors"/>
    /// when they are unknown), at least 1 and at most <see cref="MaxThreads"/>.
    /// </summary>
    public static int Threads(int? physicalCores, int logicalProcessors, IReadOnlyDictionary<string, string> environment)
    {
        if (environment.TryGetValue("MIDGARD_MODEL_THREADS", out var value)
            && int.TryParse(value, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var n) && n > 0)
            return n;
        return Math.Clamp(physicalCores ?? logicalProcessors, 1, MaxThreads);
    }

    /// <summary>
    /// Every llama-server argument, the Mac's Intel ones: the model, its alias, loopback on
    /// <paramref name="port"/>, an 8192-token context, one slot (<c>--parallel 1</c>; the Mac
    /// passes no <c>--cont-batching</c>, llama-server's default), the Mac's sampling and
    /// JSON grammar, no GPU layers and <paramref name="threads"/> threads.
    /// </summary>
    public static Command Make(string executable, string models, ushort port, int threads) =>
        new(executable, [
            "-m", Path.Join(models, Model.Replace('/', Path.DirectorySeparatorChar)),
            "--alias", Alias,
            "--host", "127.0.0.1", "--port", port.ToString(CultureInfo.InvariantCulture), "--ctx-size", "8192",
            "--parallel", "1", "--temp", "0.2", "--json-schema", "{\"type\":\"object\"}",
            "--n-gpu-layers", "0", "--threads", Math.Max(1, threads).ToString(CultureInfo.InvariantCulture),
        ]);

    /// <summary>
    /// This PC's physical cores (one GetLogicalProcessorInformationEx record per core, so
    /// hyper-threads are not counted), null if Windows does not say or this is not Windows.
    /// </summary>
    public static int? PhysicalCores() => OperatingSystem.IsWindows() ? WindowsPhysicalCores() : null;

    /// <summary>The thread count for this PC and process environment.</summary>
    public static int ThreadsForThisPc(IReadOnlyDictionary<string, string> environment) =>
        Threads(PhysicalCores(), Environment.ProcessorCount, environment);

    [SupportedOSPlatform("windows")]
    static unsafe int? WindowsPhysicalCores()
    {
        uint length = 0;
        Kernel32.GetLogicalProcessorInformationEx(Kernel32.RelationProcessorCore, null, &length);
        if (length == 0) return null;
        var buffer = new byte[length];
        fixed (byte* start = buffer)
        {
            if (!Kernel32.GetLogicalProcessorInformationEx(Kernel32.RelationProcessorCore, start, &length)) return null;
            // SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX: Relationship (int), Size (uint), then the union.
            var cores = 0;
            for (uint offset = 0; offset + 8 <= length;)
            {
                var relationship = *(int*)(start + offset);
                var size = *(uint*)(start + offset + 4);
                if (size == 0) break;
                if (relationship == Kernel32.RelationProcessorCore) cores++;
                offset += size;
            }
            return cores > 0 ? cores : null;
        }
    }
}
