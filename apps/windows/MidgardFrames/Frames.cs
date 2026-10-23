namespace MidgardFrames;

// Decoding for the camera analytics (heimdall/analytics/analytics.py) without ffmpeg: what
// `ffprobe` and `ffmpeg -vf fps=F,scale=W:H:flags=bilinear -pix_fmt bgr24` give it, with Media
// Foundation (clips, MediaFoundation.cs) and WIC (stills, Wic.cs). The output is the same raw
// stream as the Mac's apps/mac/Sources/MidgardFrames/Frames.swift: frames of H x W x 3 bytes,
// B G R, row after row, no padding, frame after frame. This file is the part both share, line
// for line: the frame selection, the letterbox size and the bilinear resize.

/// <summary>A clip or image that cannot be probed or decoded; its message is the one-line reason.</summary>
public sealed class FramesException(string message) : Exception(message)
{
    /// <summary>What an N edition of Windows without the Media Feature Pack gets for a clip.</summary>
    public const string NoMediaFeaturePack =
        "This Windows edition lacks the Media Feature Pack; camera clips can't be decoded";
}

/// <summary>
/// Which decoded frames ffmpeg's <c>fps</c> filter (its default <c>round=near</c>) keeps: every
/// input frame belongs to output slot round(t x fps), and each slot shows the LAST frame of its
/// slot; a slot no frame falls in repeats the frame before. Slots count from the first frame's
/// and end before the slot of the clip's end (the last frame's time plus its duration), so the
/// last frame's own slot is often not shown. As FrameSelector in Frames.swift (checked there
/// against ffmpeg 8 on TfL JamCams clips, 25 fps in, 4 fps out: input frames 3, 9, 15, 21, 28, …).
/// </summary>
public sealed class FrameSelector
{
    public int Fps { get; }
    public int MaxFrames { get; }
    (long Slot, int Index)? held;
    long? nextSlot;
    public int Emitted { get; private set; }

    public FrameSelector(int fps, int maxFrames)
    {
        Fps = Math.Max(1, fps);
        MaxFrames = Math.Max(0, maxFrames);
    }

    /// <summary>The slot of a frame at value / timescale seconds: floor(t x fps + 1/2), exactly.</summary>
    public long Slot(long value, long timescale)
    {
        long num = 2 * value * Fps + timescale;
        long den = 2 * timescale;
        return num >= 0 ? num / den : -((-num + den - 1) / den);
    }

    public bool Done => Emitted >= MaxFrames;

    /// <summary>
    /// Feeds the next decoded frame (in presentation order) and holds it; returns the frames to
    /// output now, in order: the previously held one, repeated to fill any empty slots.
    /// </summary>
    public List<int> Push(int index, long slot)
    {
        var output = new List<int>();
        if (held is { } h && slot > h.Slot)
            output = Flush(slot - 1);
        held = (slot, index);
        nextSlot ??= slot;
        return output;
    }

    /// <summary>
    /// At the end of the input, whose end (the last frame's time plus its duration) is in slot
    /// endSlot: the held frame for the slots before it (null: for its own slot).
    /// </summary>
    public List<int> Finish(long? endSlot = null)
    {
        if (held is not { } h) return [];
        return Flush(endSlot is { } e ? e - 1 : h.Slot);
    }

    List<int> Flush(long last)
    {
        var output = new List<int>();
        if (held is not { } h || nextSlot is not { } s) return output;
        while (s <= last && Emitted < MaxFrames)
        {
            output.Add(h.Index);
            Emitted++;
            s++;
        }
        nextSlot = s;
        return output;
    }
}

public static partial class Frames
{
    /// <summary>The letterbox size analytics.py asks for: r = min(S/W, S/H), int(W r) x int(H r).</summary>
    public static (int Width, int Height) LetterboxSize(int width, int height, int target)
    {
        double r = Math.Min((double)target / width, (double)target / height);
        return (Math.Max(1, (int)(width * r)), Math.Max(1, (int)(height * r)));
    }

    /// <summary>
    /// Bilinear resize of a BGRA (or BGRX) image to packed BGR, sampling at pixel centres
    /// (src = (dst + 0.5) x scale - 0.5, clamped at the edges), as ffmpeg's bilinear scaler and
    /// resizeBGRAtoBGR in Frames.swift (the same doubles, the same rounding: half away from zero).
    /// rowBytes may be negative (a bottom-up image with src at its top row).
    /// </summary>
    public static unsafe byte[] ResizeBgraToBgr(byte* src, int width, int height, int rowBytes, int toWidth, int toHeight)
    {
        int ow = toWidth, oh = toHeight;
        var output = new byte[ow * oh * 3];
        double sx = (double)width / ow, sy = (double)height / oh;
        // Per column: left index and weight (shared by every row).
        var x0 = new int[ow];
        var x1 = new int[ow];
        var wx = new double[ow];
        for (int x = 0; x < ow; x++)
        {
            double fx = Math.Min(Math.Max((x + 0.5) * sx - 0.5, 0), width - 1);
            int i = (int)fx;
            x0[x] = i * 4;
            x1[x] = Math.Min(i + 1, width - 1) * 4;
            wx[x] = fx - i;
        }
        fixed (byte* o = output)
        {
            for (int y = 0; y < oh; y++)
            {
                double fy = Math.Min(Math.Max((y + 0.5) * sy - 0.5, 0), height - 1);
                int j = (int)fy;
                double wy = fy - j;
                byte* r0 = src + (long)j * rowBytes;
                byte* r1 = src + (long)Math.Min(j + 1, height - 1) * rowBytes;
                int p = y * ow * 3;
                for (int x = 0; x < ow; x++)
                {
                    int a = x0[x], b = x1[x];
                    double w = wx[x];
                    for (int c = 0; c < 3; c++)
                    {
                        double top = r0[a + c] * (1 - w) + r0[b + c] * w;
                        double bottom = r1[a + c] * (1 - w) + r1[b + c] * w;
                        double v = Math.Round(top * (1 - wy) + bottom * wy, MidpointRounding.AwayFromZero);
                        o[p + c] = (byte)Math.Min(255, Math.Max(0, v));
                    }
                    p += 3;
                }
            }
        }
        return output;
    }

    /// <summary>ResizeBgraToBgr of a top-down array (tests, and the frames this helper copies).</summary>
    public static unsafe byte[] ResizeBgraToBgr(byte[] src, int width, int height, int rowBytes, int toWidth, int toHeight)
    {
        fixed (byte* s = src)
            return ResizeBgraToBgr(s, width, height, rowBytes, toWidth, toHeight);
    }

    /// <summary>Whether a file starts like a still image WIC reads (JPEG, PNG, BMP, GIF, TIFF).</summary>
    static bool LooksLikeImage(string path)
    {
        Span<byte> head = stackalloc byte[8];
        int n;
        try
        {
            using var f = File.OpenRead(path);
            n = f.ReadAtLeast(head, head.Length, throwOnEndOfStream: false);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return false;
        }
        ReadOnlySpan<byte> h = head[..n];
        return h.StartsWith(Jpeg) || h.StartsWith(Png) || h.StartsWith("BM"u8) || h.StartsWith("GIF8"u8)
            || h.StartsWith("II*\0"u8) || h.StartsWith("MM\0*"u8);
    }

    static ReadOnlySpan<byte> Jpeg => [0xFF, 0xD8, 0xFF];
    static ReadOnlySpan<byte> Png => [0x89, 0x50, 0x4E, 0x47];

    /// <summary>
    /// A clip's or image's size, the one its frames are resized from (and so the one the
    /// letterbox ratio must come from): for a clip, its first decoded frame's (the display
    /// aperture, as ffprobe reports it for a cropped H.264 clip), for an image its pixel size.
    /// No rotation is applied, as on the Mac.
    /// </summary>
    public static (int Width, int Height) Probe(string path)
    {
        if (!File.Exists(path)) throw new FramesException($"no such file: {Path.GetFileName(path)}");
        if (LooksLikeImage(path)) return Wic.Probe(path);
        FramesException clipError;
        try
        {
            return MediaFoundation.Probe(path);
        }
        catch (FramesException e) when (e.Message != FramesException.NoMediaFeaturePack)
        {
            clipError = e;
        }
        // Not a clip Media Foundation reads: perhaps an image WIC knows that the sniff missed.
        try
        {
            return Wic.Probe(path);
        }
        catch (FramesException)
        {
            throw clipError;
        }
    }

    /// <summary>
    /// Decodes a clip at fps (ffmpeg's fps filter selection), at most maxFrames frames, each
    /// resized from its own frame (see Probe) to width x height BGR and handed to write as it is
    /// ready. Returns how many frames were written.
    /// </summary>
    public static int DecodeClip(string path, int width, int height, int fps, int maxFrames, Action<byte[]> write)
    {
        if (!File.Exists(path)) throw new FramesException($"no such file: {Path.GetFileName(path)}");
        return MediaFoundation.DecodeClip(path, width, height, fps, maxFrames, write);
    }

    /// <summary>Decodes one image (JPEG, PNG …) resized to width x height BGR.</summary>
    public static byte[] DecodeStill(string path, int width, int height)
    {
        if (!File.Exists(path)) throw new FramesException($"no such file: {Path.GetFileName(path)}");
        return Wic.DecodeStill(path, width, height);
    }
}
