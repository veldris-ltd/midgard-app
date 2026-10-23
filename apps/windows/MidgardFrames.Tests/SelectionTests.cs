namespace MidgardFrames.Tests;

/// <summary>
/// The frame selection, letterbox size and resize: FramesTests.swift's, case for case, so the
/// two helpers keep the same rules.
/// </summary>
public class SelectionTests
{
    /// 25 fps in, 4 fps out: ffmpeg 8 keeps input frames 3, 9, 15, 21, 28, … (measured on TfL
    /// clips by matching its output to every decoded frame).
    [Fact]
    public void SelectionMatchesFfmpegAt25To4()
    {
        var s = new FrameSelector(4, 40);
        var kept = new List<int>();
        for (int i = 0; i < 271 && !s.Done; i++)
            kept.AddRange(s.Push(i, s.Slot(i, 25)));
        kept.AddRange(s.Finish());
        Assert.Equal(40, kept.Count);
        Assert.Equal(new[] { 3, 9, 15, 21, 28, 34, 40, 46, 53, 59, 65, 71 }, kept.Take(12).ToArray());
        Assert.Equal(246, kept[^1]);
    }

    [Fact]
    public void SlotRoundsHalfUp()
    {
        var s = new FrameSelector(4, 1);
        Assert.Equal(0L, s.Slot(12, 100));    // 0.48
        Assert.Equal(1L, s.Slot(125, 1000));  // 0.5
        Assert.Equal(1L, s.Slot(16, 100));    // 0.64
        Assert.Equal(0L, s.Slot(0, 600));
        Assert.Equal(1L, s.Slot(1_250_000, 10_000_000));  // 0.125 s in Media Foundation's 100 ns
    }

    /// A slot with no frame repeats the frame before (a 2 fps input asked for 4 fps).
    [Fact]
    public void EmptySlotsRepeatTheFrameBefore()
    {
        var s = new FrameSelector(4, 10);
        var kept = new List<int>();
        for (int i = 0; i < 3; i++) kept.AddRange(s.Push(i, s.Slot(i, 2)));
        kept.AddRange(s.Finish());
        Assert.Equal(new[] { 0, 0, 1, 1, 2 }, kept.ToArray());
    }

    /// The clip's end bounds the last slots, as ffmpeg (4 s at 25 fps: 16 frames, slots 0-15;
    /// 0.32 s: 1 frame, measured with ffmpeg 8 on testsrc clips).
    [Fact]
    public void TheEndOfTheClipEndsTheSlots()
    {
        static List<int> Run(int frames)
        {
            var s = new FrameSelector(4, 40);
            var kept = new List<int>();
            for (int i = 0; i < frames; i++) kept.AddRange(s.Push(i, s.Slot(i, 25)));
            kept.AddRange(s.Finish(s.Slot(frames, 25)));
            return kept;
        }
        Assert.Equal(16, Run(100).Count);
        Assert.Equal(10, Run(63).Count);
        Assert.Equal(new[] { 3 }, Run(8).ToArray());
    }

    [Fact]
    public void FinishWithNothingGivesNothing()
    {
        Assert.Empty(new FrameSelector(4, 10).Finish());
    }

    [Fact]
    public void LetterboxSizeAsAnalyticsPy()
    {
        Assert.Equal((640, 523), Frames.LetterboxSize(352, 288, 640));
        Assert.Equal((640, 360), Frames.LetterboxSize(1920, 1080, 640));
        Assert.Equal((640, 640), Frames.LetterboxSize(640, 640, 640));
    }

    [Fact]
    public void ResizeKeepsAFlatColourAndDropsAlpha()
    {
        byte[] src = Enumerable.Repeat(new byte[] { 10, 20, 30, 255 }, 4 * 3).SelectMany(p => p).ToArray();
        var output = Frames.ResizeBgraToBgr(src, 4, 3, 16, 7, 5);
        Assert.Equal(7 * 5 * 3, output.Length);
        Assert.Equal(new byte[] { 10, 20, 30, 10, 20, 30 }, output.Take(6).ToArray());
        Assert.Equal(new byte[] { 10, 20, 30 }, output.Distinct().Order().ToArray());
    }

    /// Doubling a 2-pixel row: centres at 0.25 and 0.75 interpolate (src -0.25 clamps).
    [Fact]
    public void ResizeInterpolatesBetweenPixelCentres()
    {
        byte[] src = [0, 0, 0, 0, 200, 100, 40, 0];
        Assert.Equal(new byte[] { 0, 0, 0, 50, 25, 10, 150, 75, 30, 200, 100, 40 }, Frames.ResizeBgraToBgr(src, 2, 1, 8, 4, 1));
    }

    /// A bottom-up image (negative stride from its top row) resizes as the same image top-down.
    [Fact]
    public unsafe void ResizeTakesANegativeStride()
    {
        byte[] topDown = [1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0, 10, 11, 12, 0];  // 2 x 2
        byte[] bottomUp = [.. topDown[8..], .. topDown[..8]];
        byte[] expected = Frames.ResizeBgraToBgr(topDown, 2, 2, 8, 3, 3);
        fixed (byte* p = bottomUp)
            Assert.Equal(expected, Frames.ResizeBgraToBgr(p + 8, 2, 2, -8, 3, 3));
    }
}
