using System.Globalization;

namespace MidgardFrames;

/// <summary>
/// midgard-frames: the camera analytics' clip and image decoder in the Windows app, in place of
/// ffprobe and ffmpeg (heimdall/analytics/analytics.py, --frames-helper). The same command line
/// as apps/mac/Sources/midgard-frames/main.swift:
///
///   midgard-frames probe PATH
///       prints "W,H" (as `ffprobe -show_entries stream=width,height -of csv=p=0`)
///   midgard-frames decode PATH --width W --height H (--fps F --max-frames N | --still)
///       writes raw BGR frames of W x H to stdout (as `ffmpeg -vf fps=F,scale=W:H:flags=bilinear
///       -frames:v N -f rawvideo -pix_fmt bgr24 pipe:1`; --still: one image, scaled only)
///
/// Exit 0 on success; 1 with a one-line message on stderr otherwise; 2 for bad usage.
/// </summary>
public static class Cli
{
    sealed class UsageException : Exception;

    const string Usage =
        "usage: midgard-frames probe PATH | decode PATH --width W --height H (--fps F --max-frames N | --still)";

    public static int Run(string[] arguments, Stream stdout, TextWriter stderr)
    {
        string name = arguments.Length >= 2 ? Path.GetFileName(arguments[1]) : "";
        try
        {
            return Execute(arguments, stdout);
        }
        catch (UsageException)
        {
            stderr.Write($"midgard-frames: {Usage}\n");
            return 2;
        }
        catch (Exception e) when (e is FramesException or IOException or UnauthorizedAccessException)
        {
            stderr.Write($"midgard-frames: {name}: {OneLine(e.Message)}\n");
            return 1;
        }
    }

    static string OneLine(string s) => s.ReplaceLineEndings(" ").Trim();

    static int Execute(string[] arguments, Stream stdout)
    {
        var args = new Queue<string>(arguments);
        if (args.Count < 2) throw new UsageException();
        string command = args.Dequeue();
        string path = args.Dequeue();
        var options = new Dictionary<string, string>();
        bool still = false;
        while (args.Count > 0)
        {
            string a = args.Dequeue();
            switch (a)
            {
                case "--still":
                    still = true;
                    break;
                case "--width" or "--height" or "--fps" or "--max-frames":
                    if (args.Count == 0) throw new UsageException();
                    options[a] = args.Dequeue();
                    break;
                default:
                    throw new UsageException();
            }
        }

        int Number(string key, int limit)
        {
            // As Swift's Int(String): an optional sign, then digits only.
            if (!options.TryGetValue(key, out var s)
                || !int.TryParse(s, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out int v)
                || v <= 0 || v > limit)
                throw new UsageException();
            return v;
        }

        switch (command)
        {
            case "probe":
            {
                if (options.Count > 0 || still) throw new UsageException();
                var (w, h) = Frames.Probe(path);
                var line = System.Text.Encoding.ASCII.GetBytes($"{w},{h}\n");
                stdout.Write(line);
                stdout.Flush();
                return 0;
            }
            case "decode":
            {
                int w = Number("--width", 8192), h = Number("--height", 8192);
                if (still)
                {
                    if (options.ContainsKey("--fps") || options.ContainsKey("--max-frames")) throw new UsageException();
                    stdout.Write(Frames.DecodeStill(path, w, h));
                }
                else
                {
                    int fps = Number("--fps", 60), n = Number("--max-frames", 10_000);
                    Frames.DecodeClip(path, w, h, fps, n, frame => stdout.Write(frame));
                }
                stdout.Flush();
                return 0;
            }
            default:
                throw new UsageException();
        }
    }
}
