namespace MidgardFrames;

/// <summary>
/// Stills (the JamCams' JPEGs, for stills-only cameras) through WIC: the first frame,
/// converted to 32bpp BGR (B G R X, as the Mac draws it), then resized as a clip's frames.
/// WIC is part of every Windows edition, N ones included. Vtable slots: wincodec.idl's order.
/// </summary>
static unsafe class Wic
{
    const uint CLSCTX_INPROC_SERVER = 0x1;
    const uint GENERIC_READ = 0x8000_0000;
    const int WICDecodeMetadataCacheOnDemand = 0;
    const int WICBitmapDitherTypeNone = 0;
    const int WICBitmapPaletteTypeCustom = 0;

    static readonly Guid CLSID_WICImagingFactory = new("cacaf262-9370-4615-a13b-9f5539da4c0a");
    static readonly Guid IID_IWICImagingFactory = new("ec5ec8a9-c395-4314-9c77-54d7a935ff70");
    static readonly Guid GUID_WICPixelFormat32bppBGR = new("6fddc324-4e03-4bfe-b185-3d77768dc90e");

    /// <summary>An image's first frame, open for reading; disposed releases everything.</summary>
    sealed class Image : IDisposable
    {
        Com.Apartment apartment;
        nint factory, decoder, frame;
        public int Width { get; private set; }
        public int Height { get; private set; }

        public static Image Open(string path)
        {
            var image = new Image();
            try
            {
                image.Start(path);
                return image;
            }
            catch
            {
                image.Dispose();
                throw;
            }
        }

        void Start(string path)
        {
            string name = Path.GetFileName(path);
            apartment = Com.Apartment.Enter();
            Guid clsid = CLSID_WICImagingFactory, iid = IID_IWICImagingFactory;
            nint f = 0;
            int hr = Com.CoCreateInstance(&clsid, 0, CLSCTX_INPROC_SERVER, &iid, &f);
            if (Com.Failed(hr)) throw new FramesException($"WIC did not start ({Com.Hex(hr)})");
            factory = f;

            nint d = 0;
            fixed (char* p = Path.GetFullPath(path))
                hr = ((delegate* unmanaged<nint, char*, Guid*, uint, int, nint*, int>)Com.Slot(factory, 3))(
                    factory, p, null, GENERIC_READ, WICDecodeMetadataCacheOnDemand, &d);
            if (Com.Failed(hr)) throw new FramesException($"not an image: {name} ({Com.Hex(hr)})");
            decoder = d;

            nint fr = 0;
            hr = ((delegate* unmanaged<nint, uint, nint*, int>)Com.Slot(decoder, 13))(decoder, 0, &fr);
            if (Com.Failed(hr)) throw new FramesException($"not an image: {name} ({Com.Hex(hr)})");
            frame = fr;

            uint w = 0, h = 0;
            hr = ((delegate* unmanaged<nint, uint*, uint*, int>)Com.Slot(frame, 3))(frame, &w, &h);
            if (Com.Failed(hr) || w == 0 || h == 0 || w > 65535 || h > 65535)
                throw new FramesException($"not an image: {name} (no size)");
            (Width, Height) = ((int)w, (int)h);
        }

        /// <summary>The frame as 32bpp BGR, top-down, Width x 4 bytes a row.</summary>
        public byte[] Pixels()
        {
            nint converter = 0;
            try
            {
                int hr = ((delegate* unmanaged<nint, nint*, int>)Com.Slot(factory, 10))(factory, &converter);
                if (Com.Failed(hr)) throw new FramesException($"CreateFormatConverter failed ({Com.Hex(hr)})");
                Guid format = GUID_WICPixelFormat32bppBGR;
                hr = ((delegate* unmanaged<nint, nint, Guid*, int, nint, double, int, int>)Com.Slot(converter, 8))(
                    converter, frame, &format, WICBitmapDitherTypeNone, 0, 0.0, WICBitmapPaletteTypeCustom);
                if (Com.Failed(hr)) throw new FramesException($"cannot convert the image to BGR ({Com.Hex(hr)})");
                int rowBytes = Width * 4;
                var pixels = new byte[(long)rowBytes * Height];
                fixed (byte* p = pixels)
                    hr = ((delegate* unmanaged<nint, void*, uint, uint, byte*, int>)Com.Slot(converter, 7))(
                        converter, null, (uint)rowBytes, (uint)pixels.Length, p);
                if (Com.Failed(hr)) throw new FramesException($"cannot read the image's pixels ({Com.Hex(hr)})");
                return pixels;
            }
            finally
            {
                Com.Release(ref converter);
            }
        }

        public void Dispose()
        {
            Com.Release(ref frame);
            Com.Release(ref decoder);
            Com.Release(ref factory);
            apartment.Dispose();
            apartment = default;
        }
    }

    public static (int Width, int Height) Probe(string path)
    {
        using var image = Image.Open(path);
        return (image.Width, image.Height);
    }

    public static byte[] DecodeStill(string path, int width, int height)
    {
        using var image = Image.Open(path);
        var pixels = image.Pixels();
        return Frames.ResizeBgraToBgr(pixels, image.Width, image.Height, image.Width * 4, width, height);
    }
}
