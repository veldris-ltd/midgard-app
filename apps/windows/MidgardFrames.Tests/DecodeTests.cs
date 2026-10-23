using System.Diagnostics;
using System.Globalization;
using System.Text;
using Xunit.Abstractions;

namespace MidgardFrames.Tests;

/// <summary>
/// Synthetic clips and a still made by ffmpeg (MIDGARD_FFMPEG; no camera footage in git), once
/// for the class, in a temporary folder:
///   grey.mp4      100 frames, 4 s at 25 fps, 352 x 288 (as the TfL JamCams), H.264 with B-frames;
///                 frame N one flat grey, Y = 16 + 2N, so every frame tells which it is
///   grey8.mp4     the first 8 of those (0.32 s: ffmpeg keeps one frame at 4 fps)
///   testsrc.mp4   ffmpeg's testsrc2 pattern, 10.4 s at 25 fps, 352 x 288: colour, edges, motion
///   hd.mp4        testsrc2 at 1920 x 1080, 1 s (coded as 1088 rows: the display aperture crops)
///   still.jpg     one testsrc2 frame, 640 x 480
/// Without MIDGARD_FFMPEG the tests pass vacuously off CI and fail on CI (CI is set there).
/// </summary>
public sealed class Fixtures : IDisposable
{
    public string? Ffmpeg { get; }
    public string Dir { get; } = Path.Combine(Path.GetTempPath(), "midgard-frames-" + Guid.NewGuid().ToString("N"));

    public Fixtures()
    {
        Ffmpeg = Environment.GetEnvironmentVariable("MIDGARD_FFMPEG");
        if (string.IsNullOrEmpty(Ffmpeg))
        {
            Ffmpeg = null;
            return;
        }
        Directory.CreateDirectory(Dir);
        const string x264 = "-c:v libx264 -preset veryfast -bf 2 -pix_fmt yuv420p";
        Make("grey.mp4", $"-f lavfi -i nullsrc=s=352x288:r=25:d=4,format=yuv420p,geq=lum=16+2*N:cb=128:cr=128 {x264} -crf 8 -g 25");
        Make("grey8.mp4", $"-f lavfi -i nullsrc=s=352x288:r=25:d=0.32,format=yuv420p,geq=lum=16+2*N:cb=128:cr=128 {x264} -crf 8 -g 25");
        Make("testsrc.mp4", $"-f lavfi -i testsrc2=s=352x288:r=25:d=10.4 {x264} -crf 23 -g 50");
        Make("hd.mp4", $"-f lavfi -i testsrc2=s=1920x1080:r=25:d=1 {x264} -crf 23 -g 25");
        Make("still.jpg", "-f lavfi -i testsrc2=s=640x480:r=1 -frames:v 1 -q:v 3");
        // HEVC, for a codec Windows may have no decoder for (the HEVC Video Extensions).
        HevcMade = TryMake("hevc.mp4", "-f lavfi -i testsrc2=s=352x288:r=25:d=1 -c:v libx265 -pix_fmt yuv420p -tag:v hvc1");
    }

    public bool HevcMade { get; }

    void Make(string name, string args)
    {
        if (!TryMake(name, args, out var err)) throw new InvalidOperationException($"ffmpeg could not make {name}: {err}");
    }

    bool TryMake(string name, string args) => TryMake(name, args, out _);

    bool TryMake(string name, string args, out string err)
    {
        var (code, _, e) = Tools.Run(Ffmpeg!, ["-v", "error", "-y", .. args.Split(' '), Path.Combine(Dir, name)]);
        err = e;
        return code == 0;
    }

    public string this[string name] => Path.Combine(Dir, name);

    public void Dispose()
    {
        try { Directory.Delete(Dir, recursive: true); } catch (IOException) { }
    }
}

static class Tools
{
    static readonly object ReportLock = new();

    public static (int Code, byte[] Stdout, string Stderr) Run(string exe, IEnumerable<string> args)
    {
        var info = new ProcessStartInfo(exe)
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
            CreateNoWindow = true,
        };
        foreach (var a in args) info.ArgumentList.Add(a);
        using var p = Process.Start(info)!;
        var err = p.StandardError.ReadToEndAsync();
        using var output = new MemoryStream();
        p.StandardOutput.BaseStream.CopyTo(output);
        p.WaitForExit();
        return (p.ExitCode, output.ToArray(), err.Result);
    }

    /// <summary>What analytics.py runs when it has no helper (decode(), the ffmpeg branch).</summary>
    /// every: all the frames, scaled only (still: true, every: true).
    public static byte[] FfmpegDecode(string ffmpeg, string path, int w, int h, int fps, int maxFrames, bool still,
                                      string? matrix = null, bool every = false)
    {
        string scale = $"scale={w}:{h}:flags=bilinear" + (matrix != null ? $":in_color_matrix={matrix}" : "");
        string vf = still ? scale : $"fps={fps},{scale}";
        var (code, output, err) = Run(ffmpeg, ["-v", "error", "-threads", "1", "-filter_threads", "1", "-i", path,
            "-vf", vf, "-frames:v", still && !every ? "1" : maxFrames.ToString(CultureInfo.InvariantCulture),
            "-f", "rawvideo", "-pix_fmt", "bgr24", "pipe:1"]);
        if (code != 0) throw new InvalidOperationException($"ffmpeg could not decode {path}: {err}");
        return output;
    }

    public static double MeanAbsDiff(ReadOnlySpan<byte> a, ReadOnlySpan<byte> b)
    {
        long sum = 0;
        for (int i = 0; i < a.Length; i++) sum += Math.Abs(a[i] - b[i]);
        return (double)sum / a.Length;
    }

    public static int MaxAbsDiff(ReadOnlySpan<byte> a, ReadOnlySpan<byte> b)
    {
        int max = 0;
        for (int i = 0; i < a.Length; i++) max = Math.Max(max, Math.Abs(a[i] - b[i]));
        return max;
    }

    /// <summary>A line for the comparison record (MIDGARD_FRAMES_REPORT), and the test output.</summary>
    public static void Record(ITestOutputHelper output, string line)
    {
        output.WriteLine(line);
        var report = Environment.GetEnvironmentVariable("MIDGARD_FRAMES_REPORT");
        if (string.IsNullOrEmpty(report)) return;
        lock (ReportLock)
            File.AppendAllText(report, line + Environment.NewLine);
    }
}

public class DecodeTests(Fixtures fixtures, ITestOutputHelper output) : IClassFixture<Fixtures>
{
    /// <summary>Whether the fixtures are here; on CI their absence is a failure.</summary>
    bool Ready()
    {
        if (fixtures.Ffmpeg != null) return true;
        Assert.True(string.IsNullOrEmpty(Environment.GetEnvironmentVariable("CI")),
            "MIDGARD_FFMPEG is not set: the decoding tests need ffmpeg on CI");
        output.WriteLine("MIDGARD_FFMPEG is not set: nothing decoded");
        return false;
    }

    List<byte[]> Decode(string path, int w, int h, int fps, int maxFrames)
    {
        var frames = new List<byte[]>();
        int n = Frames.DecodeClip(path, w, h, fps, maxFrames, f => frames.Add((byte[])f.Clone()));
        Assert.Equal(n, frames.Count);
        return frames;
    }

    static byte[] Frame(byte[] all, int i, int size) => all.AsSpan(i * size, size).ToArray();

    /// The probe is the stored-clip size, the one ffprobe reports, for an exact and a cropped size.
    [Fact]
    public void ProbeReportsTheClipSizeAsFfprobe()
    {
        if (!Ready()) return;
        Assert.Equal((352, 288), Frames.Probe(fixtures["grey.mp4"]));
        Assert.Equal((1920, 1080), Frames.Probe(fixtures["hd.mp4"]));
        Assert.Equal((640, 480), Frames.Probe(fixtures["still.jpg"]));

        using var stdout = new MemoryStream();
        using var stderr = new StringWriter();
        Assert.Equal(0, Cli.Run(["probe", fixtures["grey.mp4"]], stdout, stderr));
        Assert.Equal("352,288\n", Encoding.ASCII.GetString(stdout.ToArray()));
        Assert.Empty(stderr.ToString());
    }

    /// Which input frames ffmpeg's fps output holds: each output frame found among all the
    /// clip's frames, decoded by ffmpeg at the same size (an exact match).
    int[] FfmpegSelection(string name, int w, int h, int maxFrames)
    {
        int size = w * h * 3;
        var selected = Tools.FfmpegDecode(fixtures.Ffmpeg!, fixtures[name], w, h, 4, maxFrames, still: false);
        var all = Tools.FfmpegDecode(fixtures.Ffmpeg!, fixtures[name], w, h, 0, 10_000, still: true, every: true);
        int n = all.Length / size;
        return Enumerable.Range(0, selected.Length / size).Select(k =>
        {
            var diffs = Enumerable.Range(0, n).Select(j => Tools.MeanAbsDiff(selected.AsSpan(k * size, size), all.AsSpan(j * size, size))).ToArray();
            return Array.IndexOf(diffs, diffs.Min());
        }).ToArray();
    }

    /// Which input frames ours are, from their grey: frame N is Y = 16 + 2N, so 2N x 255/219 in
    /// RGB, 2.3 levels from the next; the value must be within 1.1 of it.
    static int[] OurSelection(List<byte[]> frames)
    {
        return frames.Select(f =>
        {
            int v = f[f.Length / 2];
            int n = (int)Math.Round(v * 219.0 / 510);
            Assert.InRange(v, 2 * n * 255.0 / 219 - 1.1, 2 * n * 255.0 / 219 + 1.1);
            return n;
        }).ToArray();
    }

    /// 4 s at 25 fps, asked for 4 fps: the 16 frames ffmpeg gives (slots 0-15), each the input
    /// frame ffmpeg picks (frames 3, 9, 15, 21, 28, …), in the shape analytics.py reads.
    [Fact]
    public void SelectsTheFramesFfmpegSelects()
    {
        if (!Ready()) return;
        var (w, h) = Frames.LetterboxSize(352, 288, 640);
        Assert.Equal((640, 523), (w, h));
        int size = w * h * 3;
        var frames = Decode(fixtures["grey.mp4"], w, h, 4, 40);
        Assert.Equal(16, frames.Count);
        Assert.All(frames, f => Assert.Equal(size, f.Length));
        int[] expected = [3, 9, 15, 21, 28, 34, 40, 46, 53, 59, 65, 71, 78, 84, 90, 96];
        Assert.Equal(expected, OurSelection(frames));
        Assert.Equal(expected, FfmpegSelection("grey.mp4", 64, 52, 40));

        var reference = Tools.FfmpegDecode(fixtures.Ffmpeg!, fixtures["grey.mp4"], w, h, 4, 40, still: false);
        Assert.Equal(frames.Count * size, reference.Length);
        double worst = Enumerable.Range(0, frames.Count).Max(i => Tools.MeanAbsDiff(frames[i], reference.AsSpan(i * size, size)));
        Tools.Record(output, $"grey.mp4 352x288 -> 640x523 @4fps: input frames {string.Join(",", expected)}, as ffmpeg's; worst frame mean |diff| {worst:F3}");
    }

    [Fact]
    public void HonoursMaxFramesAndTheClipsEnd()
    {
        if (!Ready()) return;
        Assert.Equal(5, Decode(fixtures["grey.mp4"], 64, 52, 4, 5).Count);
        // 0.32 s: one frame, input frame 3, as ffmpeg.
        Assert.Equal(new[] { 3 }, OurSelection(Decode(fixtures["grey8.mp4"], 64, 52, 4, 40)));
        Assert.Equal(new[] { 3 }, FfmpegSelection("grey8.mp4", 64, 52, 40));

        // The command line writes the same frames to stdout.
        using var stdout = new MemoryStream();
        using var stderr = new StringWriter();
        Assert.Equal(0, Cli.Run(["decode", fixtures["grey.mp4"], "--width", "64", "--height", "52", "--fps", "4", "--max-frames", "5"],
            stdout, stderr));
        Assert.Equal(5 * 64 * 52 * 3, stdout.Length);
    }

    /// The pattern clip as analytics.py decodes a stored clip (40 frames at 4 fps, 640 x 523),
    /// against ffmpeg's frames: the pixel difference is recorded.
    [Fact]
    public void MatchesFfmpegOnAPatternClip()
    {
        if (!Ready()) return;
        Compare("testsrc.mp4", 352, 288, 40, maxMean: 4);
        // An untagged HD clip: Media Foundation takes it as BT.709 (its default for HD), where
        // ffmpeg's swscale takes BT.601; so it is compared with ffmpeg told BT.709 too (the
        // difference from ffmpeg's default is only recorded).
        Compare("hd.mp4", 1920, 1080, 4, maxMean: 1000);
        Compare("hd.mp4", 1920, 1080, 4, maxMean: 4, matrix: "bt709");
    }

    /// A clip whose codec Windows has no decoder for gets a one-line reason, not a crash; an
    /// H.264 clip without a decoder gets the Media Feature Pack message (not testable here).
    [Fact]
    public void ACodecWithoutADecoderIsNamed()
    {
        if (!Ready()) return;
        if (!fixtures.HevcMade)
        {
            Tools.Record(output, "hevc.mp4: this ffmpeg cannot make HEVC; not tried");
            return;
        }
        try
        {
            var size = Frames.Probe(fixtures["hevc.mp4"]);
            Tools.Record(output, $"hevc.mp4: this Windows decodes HEVC ({size.Width}x{size.Height})");
        }
        catch (FramesException e)
        {
            Tools.Record(output, $"hevc.mp4: {e.Message}");
            Assert.StartsWith("no Media Foundation decoder for hevc.mp4's video", e.Message);
        }
    }

    void Compare(string name, int width, int height, int count, double maxMean, string? matrix = null)
    {
        var (W, H) = Frames.Probe(fixtures[name]);
        Assert.Equal((width, height), (W, H));
        var (w, h) = Frames.LetterboxSize(W, H, 640);
        int size = w * h * 3;
        var frames = Decode(fixtures[name], w, h, 4, 40);
        var reference = Tools.FfmpegDecode(fixtures.Ffmpeg!, fixtures[name], w, h, 4, 40, still: false, matrix);
        Assert.Equal(count, frames.Count);
        Assert.Equal(frames.Count * size, reference.Length);
        double total = 0, worst = 0;
        int max = 0;
        for (int i = 0; i < frames.Count; i++)
        {
            var theirs = Frame(reference, i, size);
            double d = Tools.MeanAbsDiff(frames[i], theirs);
            total += d;
            worst = Math.Max(worst, d);
            max = Math.Max(max, Tools.MaxAbsDiff(frames[i], theirs));
        }
        double mean = total / frames.Count;
        Tools.Record(output, $"{name} {W}x{H} -> {w}x{h} @4fps{(matrix != null ? $", ffmpeg told {matrix}" : "")}: {frames.Count} frames as ffmpeg's; mean |diff| {mean:F3}, worst frame {worst:F3}, largest pixel diff {max}");
        Assert.True(mean < maxMean, $"{name}: mean |diff| {mean:F3} from ffmpeg");
    }

    [Fact]
    public void DecodesAStillAsFfmpeg()
    {
        if (!Ready()) return;
        var (W, H) = Frames.Probe(fixtures["still.jpg"]);
        var (w, h) = Frames.LetterboxSize(W, H, 640);
        var ours = Frames.DecodeStill(fixtures["still.jpg"], w, h);
        Assert.Equal(w * h * 3, ours.Length);
        var reference = Tools.FfmpegDecode(fixtures.Ffmpeg!, fixtures["still.jpg"], w, h, 0, 1, still: true);
        Assert.Equal(ours.Length, reference.Length);
        double mean = Tools.MeanAbsDiff(ours, reference);
        Tools.Record(output, $"still.jpg {W}x{H} -> {w}x{h}: mean |diff| {mean:F3}, largest pixel diff {Tools.MaxAbsDiff(ours, reference)}");
        Assert.True(mean < 4, $"still.jpg: mean |diff| {mean:F3} from ffmpeg");

        using var stdout = new MemoryStream();
        using var stderr = new StringWriter();
        Assert.Equal(0, Cli.Run(["decode", fixtures["still.jpg"], "--width", "64", "--height", "48", "--still"], stdout, stderr));
        Assert.Equal(64 * 48 * 3, stdout.Length);
    }
}

/// <summary>The command line's usage and errors, as main.swift's: 2 for usage, 1 with a message.</summary>
public sealed class CliTests : IDisposable
{
    readonly string dir = Directory.CreateTempSubdirectory("midgard-frames-cli-").FullName;

    public void Dispose()
    {
        try { Directory.Delete(dir, recursive: true); } catch (IOException) { }
    }

    static (int Code, string Stderr, long Out) Run(params string[] args)
    {
        using var stdout = new MemoryStream();
        using var stderr = new StringWriter();
        int code = Cli.Run(args, stdout, stderr);
        return (code, stderr.ToString(), stdout.Length);
    }

    [Theory]
    [InlineData("")]
    [InlineData("probe")]
    [InlineData("probe x.mp4 --still")]
    [InlineData("probe x.mp4 --width 10")]
    [InlineData("decode x.mp4 --width 10 --height 10")]
    [InlineData("decode x.mp4 --width 10 --height 10 --fps 4")]
    [InlineData("decode x.mp4 --width 0 --height 10 --fps 4 --max-frames 4")]
    [InlineData("decode x.mp4 --width 8193 --height 10 --fps 4 --max-frames 4")]
    [InlineData("decode x.mp4 --width 10 --height 10 --fps 61 --max-frames 4")]
    [InlineData("decode x.mp4 --width 10 --height 10 --fps 4 --max-frames 10001")]
    [InlineData("decode x.mp4 --width 1.5 --height 10 --fps 4 --max-frames 4")]
    [InlineData("decode x.mp4 --width 10 --height 10 --still --fps 4")]
    [InlineData("decode x.mp4 --width 10 --height 10 --fps")]
    [InlineData("decode x.mp4 --size 10")]
    [InlineData("frames x.mp4")]
    public void BadUsageExitsTwo(string line)
    {
        var (code, stderr, written) = Run(line.Split(' ', StringSplitOptions.RemoveEmptyEntries));
        Assert.Equal(2, code);
        Assert.StartsWith("midgard-frames: usage: midgard-frames probe PATH", stderr);
        Assert.Equal(0, written);
    }

    [Fact]
    public void AMissingFileExitsOneWithItsName()
    {
        var (code, stderr, _) = Run("probe", Path.Combine(dir, "gone.mp4"));
        Assert.Equal(1, code);
        Assert.Equal("midgard-frames: gone.mp4: no such file: gone.mp4\n", stderr);
    }

    /// Something that is not media: refused by the probe, the clip decoder and the still decoder.
    [Fact]
    public void RefusesSomethingThatIsNotMedia()
    {
        string path = Path.Combine(dir, "x.mp4");
        File.WriteAllText(path, "not a clip");
        Assert.Throws<FramesException>(() => Frames.Probe(path));
        Assert.Throws<FramesException>(() => Frames.DecodeClip(path, 10, 10, 4, 4, _ => { }));
        Assert.Throws<FramesException>(() => Frames.DecodeStill(path, 10, 10));
        var (code, stderr, written) = Run("decode", path, "--width", "10", "--height", "10", "--fps", "4", "--max-frames", "4");
        Assert.Equal(1, code);
        Assert.StartsWith("midgard-frames: x.mp4: ", stderr);
        Assert.EndsWith("\n", stderr);
        Assert.Single(stderr.Split('\n', StringSplitOptions.RemoveEmptyEntries));
        Assert.Equal(0, written);
    }
}
