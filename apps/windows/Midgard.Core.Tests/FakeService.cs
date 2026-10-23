using System.Diagnostics;
using System.Net;
using System.Net.Sockets;
using System.Text;
using Midgard.Core;

namespace Midgard.Core.Tests;

/// <summary>
/// The fake services the supervisor tests start: this test assembly's own exe (the project
/// sets GenerateProgramFile=false, and this is its Main), run as
/// <c>Midgard.Core.Tests.exe fake STEP…</c>. vstest never calls it. Only Midgard.Core and
/// the BCL are used here, so a copy of the exe with Midgard.Core.Tests.dll,
/// Midgard.Core.dll and the runtimeconfig beside it runs anywhere (the reaping tests).
/// </summary>
/// <remarks>
/// Steps, in order:
/// <list type="bullet">
/// <item><c>say:TEXT</c> / <c>err:TEXT</c>: a line to stdout / stderr.</item>
/// <item><c>args</c>: "args: " and every argument, joined by " | ".</item>
/// <item><c>env:NAME</c>: "env NAME=value".</item>
/// <item><c>lines:N</c>: "line 1" … "line N".</item>
/// <item><c>sleep:MS</c>, <c>wait</c> (for ever), <c>exit:CODE</c>.</item>
/// <item><c>http:PORT</c>: answer 200 to every request on 127.0.0.1:PORT from now on.</item>
/// <item><c>child:FILE</c>: start <c>fake wait</c> from this exe and write its pid to FILE.</item>
/// <item><c>marker:FILE</c>: the first time (FILE missing) create FILE, say "first run" and exit 3.</item>
/// <item><c>supervise:DIR</c>: a <see cref="Supervisor"/> starting two fakes, one with a child,
/// then the pids of all three in DIR\pids.txt, then wait (the crash test kills this process).</item>
/// <item>anything else (<c>note:…</c>, empty): nothing.</item>
/// </list>
/// </remarks>
public static class FakeService
{
    /// <summary>The fake's exe: this assembly's apphost.</summary>
    public static string Exe => Path.ChangeExtension(typeof(FakeService).Assembly.Location, ".exe");

    public static int Main(string[] args)
    {
        if (args.Length == 0 || args[0] != "fake")
        {
            Console.Error.WriteLine("usage: Midgard.Core.Tests fake STEP…");
            return 64;
        }
        foreach (var step in args.Skip(1))
        {
            var colon = step.IndexOf(':');
            var (verb, value) = colon < 0 ? (step, "") : (step[..colon], step[(colon + 1)..]);
            switch (verb)
            {
                case "say":
                    Console.Out.WriteLine(value);
                    Console.Out.Flush();
                    break;
                case "err":
                    Console.Error.WriteLine(value);
                    Console.Error.Flush();
                    break;
                case "args":
                    Console.Out.WriteLine("args: " + string.Join(" | ", args));
                    break;
                case "env":
                    Console.Out.WriteLine($"env {value}={Environment.GetEnvironmentVariable(value)}");
                    break;
                case "lines":
                    for (var i = 1; i <= int.Parse(value); i++) Console.Out.WriteLine($"line {i}");
                    break;
                case "sleep":
                    Thread.Sleep(int.Parse(value));
                    break;
                case "wait":
                    Thread.Sleep(Timeout.Infinite);
                    break;
                case "exit":
                    return int.Parse(value);
                case "http":
                    Serve(int.Parse(value));
                    break;
                case "child":
                    var start = new ProcessStartInfo(Environment.ProcessPath!) { UseShellExecute = false };
                    start.ArgumentList.Add("fake");
                    start.ArgumentList.Add("wait");
                    var child = Process.Start(start)!;
                    File.WriteAllText(value, child.Id.ToString());
                    Console.Out.WriteLine($"child {child.Id}");
                    break;
                case "marker":
                    if (!File.Exists(value))
                    {
                        File.WriteAllText(value, "");
                        Console.Out.WriteLine("first run");
                        return 3;
                    }
                    break;
                case "supervise":
                    return Supervise(value);
            }
        }
        return 0;
    }

    static void Serve(int port)
    {
        var listener = new TcpListener(IPAddress.Loopback, port);
        listener.Start();
        var thread = new Thread(() =>
        {
            while (true)
            {
                using var client = listener.AcceptTcpClient();
                using var stream = client.GetStream();
                var buffer = new byte[4096];
                if (stream.Read(buffer) <= 0) continue;
                stream.Write("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"u8);
            }
        }) { IsBackground = true };
        thread.Start();
    }

    static int Supervise(string dir)
    {
        if (!OperatingSystem.IsWindows()) return 65;
        Directory.CreateDirectory(dir);
        var supervisor = new Supervisor(new Supervisor.Timing { StopGrace = TimeSpan.FromSeconds(1) });
        ServiceSpec Fake(string name, params string[] steps) =>
            new(name, Self(), ["fake", .. steps], new Readiness.LogLine("listening on"),
                Path.Join(dir, $"{name}.log"));
        var grandchild = Path.Join(dir, "grandchild.pid");
        supervisor.StartAsync([
            Fake("a", "say:listening on", "wait"),
            Fake("b", $"child:{grandchild}", "say:listening on", "wait"),
        ]).GetAwaiter().GetResult();
        var pids = new[] { supervisor.PidOf("a")!.Value, supervisor.PidOf("b")!.Value, int.Parse(File.ReadAllText(grandchild)) };
        var temp = Path.Join(dir, "pids.tmp");
        File.WriteAllText(temp, string.Join("\n", pids), Encoding.ASCII);
        File.Move(temp, Path.Join(dir, "pids.txt"));
        Thread.Sleep(Timeout.Infinite);
        return 0;
    }

    /// <summary>The running exe (a copy in a reaping test is not at <see cref="Exe"/>).</summary>
    static string Self() => Environment.ProcessPath!;
}
