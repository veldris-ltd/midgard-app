using System.Runtime.InteropServices;

namespace MidgardFrames;

/// <summary>
/// Clips through Media Foundation: a source reader on the file's first video stream, with its
/// video processing on (MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING) so it hands out RGB32 (B G R X
/// in memory, as the Mac's 32BGRA), one sample at a time, each copied and released at once.
/// The vtable slots below are the IDL order of mfobjects.idl and mfreadwrite.idl (IUnknown is
/// slots 0-2; IMFAttributes 3-32; IMFMediaType and IMFSample continue from 33).
/// </summary>
static unsafe partial class MediaFoundation
{
    const uint MF_VERSION = 0x0002_0070;  // MF_SDK_VERSION 2 << 16 | MF_API_VERSION 0x70
    const uint MFSTARTUP_FULL = 0;
    const uint MF_SOURCE_READER_FIRST_VIDEO_STREAM = 0xFFFF_FFFC;
    const uint MF_SOURCE_READER_ALL_STREAMS = 0xFFFF_FFFE;
    const uint MF_SOURCE_READERF_ERROR = 0x1;
    const uint MF_SOURCE_READERF_ENDOFSTREAM = 0x2;
    const uint MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED = 0x20;
    const uint MFT_ENUM_FLAG_ALL = 0x3F;
    const int MF_E_TOPO_CODEC_NOT_FOUND = unchecked((int)0xC00D5212);
    const long Timescale = 10_000_000;  // sample times are in 100 ns units

    static readonly Guid MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING = new("fb394f3d-ccf1-42ee-bbb3-f9b845d5681d");
    static readonly Guid MF_MT_MAJOR_TYPE = new("48eba18e-f8c9-4687-bf11-0a74c9f96a8f");
    static readonly Guid MF_MT_SUBTYPE = new("f7e34c9a-42e8-4714-b74b-cb29d72c35e5");
    static readonly Guid MF_MT_FRAME_SIZE = new("1652c33d-d6b2-4012-b834-72030849a37d");
    static readonly Guid MF_MT_MINIMUM_DISPLAY_APERTURE = new("d7388766-18fe-48c6-a177-ee894867c8c4");
    static readonly Guid MF_MT_DEFAULT_STRIDE = new("644b4e48-1e02-4516-b0eb-c01ca9d49ac6");
    static readonly Guid MFMediaType_Video = new("73646976-0000-0010-8000-00aa00389b71");
    static readonly Guid MFVideoFormat_RGB32 = new("00000016-0000-0010-8000-00aa00389b71");
    static readonly Guid MFVideoFormat_H264 = new("34363248-0000-0010-8000-00aa00389b71");
    static readonly Guid MFT_CATEGORY_VIDEO_DECODER = new("d6c02d4b-6833-45b4-971a-05a4b04bab91");
    static readonly Guid IID_IMF2DBuffer = new("7dc9d5f9-9ed9-44ec-9bbf-0600bb589fbb");

    [StructLayout(LayoutKind.Sequential)]
    struct MFT_REGISTER_TYPE_INFO
    {
        public Guid Major;
        public Guid Sub;
    }

    [LibraryImport("mfplat.dll")]
    private static partial int MFStartup(uint version, uint flags);

    [LibraryImport("mfplat.dll")]
    private static partial int MFShutdown();

    [LibraryImport("mfplat.dll")]
    private static partial int MFCreateAttributes(nint* attributes, uint initialSize);

    [LibraryImport("mfplat.dll")]
    private static partial int MFCreateMediaType(nint* type);

    // MFTEnumEx takes the category GUID by value, which the x64 calling convention (the only one
    // this helper is built for) passes as a pointer to a copy.
    [LibraryImport("mfplat.dll")]
    private static partial int MFTEnumEx(Guid* category, uint flags, MFT_REGISTER_TYPE_INFO* input,
                                         MFT_REGISTER_TYPE_INFO* output, nint* activates, uint* count);

    [LibraryImport("mfreadwrite.dll", StringMarshalling = StringMarshalling.Utf16)]
    private static partial int MFCreateSourceReaderFromURL(string url, nint attributes, nint* reader);

    // IMFAttributes (and so IMFMediaType, IMFSample)
    static int GetUINT32(nint p, Guid key, out uint v)
    {
        uint r = 0;
        int hr = ((delegate* unmanaged<nint, Guid*, uint*, int>)Com.Slot(p, 7))(p, &key, &r);
        v = r;
        return hr;
    }

    static int GetUINT64(nint p, Guid key, out ulong v)
    {
        ulong r = 0;
        int hr = ((delegate* unmanaged<nint, Guid*, ulong*, int>)Com.Slot(p, 8))(p, &key, &r);
        v = r;
        return hr;
    }

    static int GetGUID(nint p, Guid key, out Guid v)
    {
        Guid r = default;
        int hr = ((delegate* unmanaged<nint, Guid*, Guid*, int>)Com.Slot(p, 10))(p, &key, &r);
        v = r;
        return hr;
    }

    static int GetBlob(nint p, Guid key, byte* buffer, uint size, out uint used)
    {
        uint u = 0;
        int hr = ((delegate* unmanaged<nint, Guid*, byte*, uint, uint*, int>)Com.Slot(p, 15))(p, &key, buffer, size, &u);
        used = u;
        return hr;
    }

    static int SetUINT32(nint p, Guid key, uint v) =>
        ((delegate* unmanaged<nint, Guid*, uint, int>)Com.Slot(p, 21))(p, &key, v);

    static int SetGUID(nint p, Guid key, Guid v) =>
        ((delegate* unmanaged<nint, Guid*, Guid*, int>)Com.Slot(p, 24))(p, &key, &v);

    /// <summary>A video subtype's FourCC (its first four bytes), as "HEVC".</summary>
    static string FourCC(Guid subtype)
    {
        Span<byte> b = stackalloc byte[16];
        subtype.TryWriteBytes(b);
        return b[..4].ToArray().All(c => c is >= 0x20 and < 0x7F)
            ? System.Text.Encoding.ASCII.GetString(b[..4])
            : subtype.ToString();
    }

    /// <summary>Whether Media Foundation has a decoder for a compressed video subtype (unknown: yes).</summary>
    static bool HasDecoder(Guid subtype)
    {
        var input = new MFT_REGISTER_TYPE_INFO { Major = MFMediaType_Video, Sub = subtype };
        nint activates = 0;
        uint count = 0;
        Guid category = MFT_CATEGORY_VIDEO_DECODER;
        if (Com.Failed(MFTEnumEx(&category, MFT_ENUM_FLAG_ALL, &input, null, &activates, &count)))
            return true;
        for (uint i = 0; i < count; i++)
        {
            nint a = ((nint*)activates)[i];
            Com.Release(ref a);
        }
        Com.CoTaskMemFree(activates);
        return count > 0;
    }

    /// <summary>One decoded frame: its time and duration (100 ns; duration -1 when unknown) and size.</summary>
    internal readonly record struct Frame(long Time, long Duration, int Width, int Height);

    /// <summary>A source reader on a clip's first video stream, giving RGB32.</summary>
    sealed class Reader : IDisposable
    {
        readonly string name;
        Com.Apartment apartment;
        bool started;
        nint reader;
        // The current output type: the full frame, the display aperture in it, the default stride.
        int frameWidth, frameHeight, apertureX, apertureY, width, height, defaultStride;

        Reader(string name) => this.name = name;

        public static Reader Open(string path)
        {
            var r = new Reader(Path.GetFileName(path));
            try
            {
                r.Start(Path.GetFullPath(path));
                return r;
            }
            catch (Exception e) when (e is DllNotFoundException or EntryPointNotFoundException)
            {
                // N editions of Windows without the Media Feature Pack have no mfplat.dll at all.
                r.Dispose();
                throw new FramesException(FramesException.NoMediaFeaturePack);
            }
            catch
            {
                r.Dispose();
                throw;
            }
        }

        void Start(string path)
        {
            apartment = Com.Apartment.Enter();
            int hr = MFStartup(MF_VERSION, MFSTARTUP_FULL);
            if (Com.Failed(hr)) throw new FramesException($"Media Foundation did not start ({Com.Hex(hr)})");
            started = true;

            nint attributes = 0;
            try
            {
                Check(MFCreateAttributes(&attributes, 1), "MFCreateAttributes");
                Check(SetUINT32(attributes, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1), "SetUINT32");
                nint r = 0;
                hr = MFCreateSourceReaderFromURL(path, attributes, &r);
                if (Com.Failed(hr)) throw new FramesException($"cannot read {name}: not a clip Media Foundation reads ({Com.Hex(hr)})");
                reader = r;
            }
            finally
            {
                Com.Release(ref attributes);
            }

            SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS, false);
            if (Com.Failed(SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM, true)))
                throw new FramesException("no video track");

            Guid native = Guid.Empty;
            nint nativeType = 0;
            if (!Com.Failed(GetNativeMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, 0, &nativeType)))
            {
                GetGUID(nativeType, MF_MT_SUBTYPE, out native);
                Com.Release(ref nativeType);
            }

            nint type = 0;
            try
            {
                Check(MFCreateMediaType(&type), "MFCreateMediaType");
                Check(SetGUID(type, MF_MT_MAJOR_TYPE, MFMediaType_Video), "SetGUID");
                Check(SetGUID(type, MF_MT_SUBTYPE, MFVideoFormat_RGB32), "SetGUID");
                hr = SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, type);
                if (Com.Failed(hr))
                {
                    // No decoder for the clip's video: for H.264 (the cameras' clips), an N edition
                    // of Windows without the Media Feature Pack; otherwise a codec Windows lacks.
                    if (hr == MF_E_TOPO_CODEC_NOT_FOUND || (native != Guid.Empty && !HasDecoder(native)))
                        throw new FramesException(native == MFVideoFormat_H264 || native == Guid.Empty
                            ? FramesException.NoMediaFeaturePack
                            : $"no Media Foundation decoder for {name}'s video ({FourCC(native)})");
                    throw new FramesException($"cannot decode {name} to RGB32 ({Com.Hex(hr)})");
                }
            }
            finally
            {
                Com.Release(ref type);
            }
            LoadFormat();
        }

        static void Check(int hr, string what)
        {
            if (Com.Failed(hr)) throw new FramesException($"{what} failed ({Com.Hex(hr)})");
        }

        int SetStreamSelection(uint stream, bool selected) =>
            ((delegate* unmanaged<nint, uint, int, int>)Com.Slot(reader, 4))(reader, stream, selected ? 1 : 0);

        int GetNativeMediaType(uint stream, uint index, nint* type) =>
            ((delegate* unmanaged<nint, uint, uint, nint*, int>)Com.Slot(reader, 5))(reader, stream, index, type);

        int GetCurrentMediaType(uint stream, nint* type) =>
            ((delegate* unmanaged<nint, uint, nint*, int>)Com.Slot(reader, 6))(reader, stream, type);

        int SetCurrentMediaType(uint stream, nint type) =>
            ((delegate* unmanaged<nint, uint, uint*, nint, int>)Com.Slot(reader, 7))(reader, stream, null, type);

        int ReadSample(uint stream, uint* actual, uint* flags, long* time, nint* sample) =>
            ((delegate* unmanaged<nint, uint, uint, uint*, uint*, long*, nint*, int>)Com.Slot(reader, 9))(
                reader, stream, 0, actual, flags, time, sample);

        /// <summary>The output frame's size, aperture and stride, from the reader's current type.</summary>
        void LoadFormat()
        {
            nint type = 0;
            Check(GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, &type), "GetCurrentMediaType");
            try
            {
                Check(GetUINT64(type, MF_MT_FRAME_SIZE, out ulong size), "MF_MT_FRAME_SIZE");
                frameWidth = (int)(size >> 32);
                frameHeight = (int)(size & 0xFFFF_FFFF);
                if (frameWidth <= 0 || frameHeight <= 0) throw new FramesException($"{name}: no frame size");
                (apertureX, apertureY, width, height) = (0, 0, frameWidth, frameHeight);
                // MFVideoArea: OffsetX, OffsetY (MFOffset: WORD fract, short value), SIZE (LONG cx, cy).
                byte* area = stackalloc byte[16];
                if (!Com.Failed(GetBlob(type, MF_MT_MINIMUM_DISPLAY_APERTURE, area, 16, out uint used)) && used == 16)
                {
                    int x = *(short*)(area + 2), y = *(short*)(area + 6), cx = *(int*)(area + 8), cy = *(int*)(area + 12);
                    if (x >= 0 && y >= 0 && cx > 0 && cy > 0 && x + cx <= frameWidth && y + cy <= frameHeight)
                        (apertureX, apertureY, width, height) = (x, y, cx, cy);
                }
                defaultStride = !Com.Failed(GetUINT32(type, MF_MT_DEFAULT_STRIDE, out uint stride)) && stride != 0
                    ? (int)stride
                    : frameWidth * 4;
            }
            finally
            {
                Com.Release(ref type);
            }
        }

        /// <summary>
        /// The next decoded frame, its displayed part copied top-down into pixels (resized to
        /// width x height x 4 as needed) unless pixels is null; null at the end of the clip.
        /// </summary>
        public Frame? Read(ref byte[]? pixels, bool copy)
        {
            for (int empty = 0; ; empty++)
            {
                uint actual = 0, flags = 0;
                long time = 0;
                nint sample = 0;
                int hr = ReadSample(MF_SOURCE_READER_FIRST_VIDEO_STREAM, &actual, &flags, &time, &sample);
                try
                {
                    if (Com.Failed(hr) || (flags & MF_SOURCE_READERF_ERROR) != 0)
                        throw new FramesException($"decoding {name} failed ({Com.Hex(hr)})");
                    if ((flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED) != 0) LoadFormat();
                    if (sample == 0)
                    {
                        if ((flags & MF_SOURCE_READERF_ENDOFSTREAM) != 0) return null;
                        if (empty > 100_000) throw new FramesException($"decoding {name}: no samples");
                        continue;  // a stream tick (a gap)
                    }
                    long t = time, d;
                    if (Com.Failed(((delegate* unmanaged<nint, long*, int>)Com.Slot(sample, 35))(sample, &t))) t = time;
                    if (Com.Failed(((delegate* unmanaged<nint, long*, int>)Com.Slot(sample, 37))(sample, &d))) d = -1;
                    if (copy)
                    {
                        if (pixels == null || pixels.Length != width * height * 4) pixels = new byte[width * height * 4];
                        CopyPixels(sample, pixels);
                    }
                    return new Frame(t, d, width, height);
                }
                finally
                {
                    Com.Release(ref sample);
                }
            }
        }

        /// <summary>The displayed part of a sample's RGB32 image, top-down, into pixels.</summary>
        void CopyPixels(nint sample, byte[] pixels)
        {
            nint buffer = 0, buffer2d = 0;
            try
            {
                Check(((delegate* unmanaged<nint, nint*, int>)Com.Slot(sample, 41))(sample, &buffer), "ConvertToContiguousBuffer");
                byte* scan0 = null;
                int pitch = 0;
                if (!Com.Failed(Com.QueryInterface(buffer, IID_IMF2DBuffer, out buffer2d)))
                {
                    // IMF2DBuffer.Lock2D: the top row and the pitch (negative for bottom-up).
                    Check(((delegate* unmanaged<nint, byte**, int*, int>)Com.Slot(buffer2d, 3))(buffer2d, &scan0, &pitch), "Lock2D");
                    try
                    {
                        Copy(scan0, pitch, pixels);
                    }
                    finally
                    {
                        ((delegate* unmanaged<nint, int>)Com.Slot(buffer2d, 4))(buffer2d);
                    }
                    return;
                }
                // IMFMediaBuffer.Lock: the bytes, laid out by the type's default stride.
                byte* data = null;
                uint max = 0, length = 0;
                Check(((delegate* unmanaged<nint, byte**, uint*, uint*, int>)Com.Slot(buffer, 3))(buffer, &data, &max, &length), "Lock");
                try
                {
                    long needed = (long)Math.Abs(defaultStride) * frameHeight;
                    if (length < needed) throw new FramesException($"decoding {name}: a short frame ({length} of {needed} bytes)");
                    scan0 = defaultStride < 0 ? data + (long)-defaultStride * (frameHeight - 1) : data;
                    Copy(scan0, defaultStride, pixels);
                }
                finally
                {
                    ((delegate* unmanaged<nint, int>)Com.Slot(buffer, 4))(buffer);
                }
            }
            finally
            {
                Com.Release(ref buffer2d);
                Com.Release(ref buffer);
            }
        }

        void Copy(byte* scan0, int pitch, byte[] pixels)
        {
            int rowBytes = width * 4;
            fixed (byte* dst = pixels)
            {
                for (int y = 0; y < height; y++)
                {
                    byte* src = scan0 + (long)(apertureY + y) * pitch + apertureX * 4;
                    Buffer.MemoryCopy(src, dst + (long)y * rowBytes, rowBytes, rowBytes);
                }
            }
        }

        public void Dispose()
        {
            Com.Release(ref reader);
            if (started)
            {
                started = false;
                MFShutdown();
            }
            apartment.Dispose();
            apartment = default;
        }
    }

    /// <summary>The first decoded frame's size (the displayed part).</summary>
    public static (int Width, int Height) Probe(string path)
    {
        using var reader = Reader.Open(path);
        byte[]? none = null;
        var frame = reader.Read(ref none, copy: false) ?? throw new FramesException("no frames decoded");
        return (frame.Width, frame.Height);
    }

    /// <summary>Frames.DecodeClip, as decodeClip in Frames.swift.</summary>
    public static int DecodeClip(string path, int width, int height, int fps, int maxFrames, Action<byte[]> write)
    {
        using var reader = Reader.Open(path);
        var selector = new FrameSelector(fps, maxFrames);
        // The frame the selector holds (the last of its slot so far), copied out of its sample;
        // resized only if output. The next frame is read into the spare buffer.
        byte[]? held = null, spare = null;
        int heldWidth = 0, heldHeight = 0;
        byte[]? heldBGR = null;
        int index = 0, written = 0;
        long? end = null;  // the last frame's time plus its duration
        long? lastTime = null;

        void Emit(int count)
        {
            if (count <= 0 || held == null) return;
            heldBGR ??= Frames.ResizeBgraToBgr(held, heldWidth, heldHeight, heldWidth * 4, width, height);
            for (int i = 0; i < count; i++)
            {
                write(heldBGR);
                written++;
            }
        }

        while (!selector.Done)
        {
            if (reader.Read(ref spare, copy: true) is not { } frame) break;
            long duration = frame.Duration;
            if (duration <= 0 && lastTime is { } last) duration = frame.Time - last;
            end = duration > 0 ? frame.Time + duration : null;
            lastTime = frame.Time;
            // What the selector releases is always the frame it held until now.
            Emit(selector.Push(index, selector.Slot(frame.Time, Timescale)).Count);
            (held, spare) = (spare, held);
            (heldWidth, heldHeight) = (frame.Width, frame.Height);
            heldBGR = null;
            index++;
        }
        if (!selector.Done)
        {
            long? endSlot = end is { } e ? selector.Slot(e, Timescale) : null;
            Emit(selector.Finish(endSlot).Count);
        }
        if (written == 0) throw new FramesException("no frames decoded");
        return written;
    }
}
