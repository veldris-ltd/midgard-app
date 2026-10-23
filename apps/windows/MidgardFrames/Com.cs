using System.Runtime.InteropServices;

namespace MidgardFrames;

/// <summary>
/// COM by hand: interface pointers as nint, methods called through their vtable slots
/// (the slot numbers are the IDL's order, counted after IUnknown's three: see
/// MediaFoundation.cs and Wic.cs), and every pointer released by its owner as soon as it is done
/// with, so a clip's decoder buffers go frame by frame, as the Mac helper's autorelease pools.
/// No runtime callable wrappers, so nothing waits for the garbage collector.
/// </summary>
static unsafe partial class Com
{
    public const int S_OK = 0;
    public const int S_FALSE = 1;
    public const int RPC_E_CHANGED_MODE = unchecked((int)0x80010106);
    public const int E_NOINTERFACE = unchecked((int)0x80004002);
    const uint COINIT_MULTITHREADED = 0;

    /// <summary>The function in slot of p's vtable.</summary>
    public static void* Slot(nint p, int slot) => (*(void***)p)[slot];

    public static void Release(ref nint p)
    {
        if (p == 0) return;
        nint q = p;
        p = 0;
        ((delegate* unmanaged<nint, uint>)Slot(q, 2))(q);
    }

    public static int QueryInterface(nint p, Guid iid, out nint result)
    {
        nint r = 0;
        int hr = ((delegate* unmanaged<nint, Guid*, nint*, int>)Slot(p, 0))(p, &iid, &r);
        result = r;
        return hr;
    }

    public static bool Failed(int hr) => hr < 0;

    public static string Hex(int hr) => $"0x{hr:X8}";

    [LibraryImport("ole32.dll")]
    private static partial int CoInitializeEx(nint reserved, uint coinit);

    [LibraryImport("ole32.dll")]
    private static partial void CoUninitialize();

    [LibraryImport("ole32.dll")]
    public static partial int CoCreateInstance(Guid* clsid, nint outer, uint context, Guid* iid, nint* result);

    [LibraryImport("ole32.dll")]
    public static partial void CoTaskMemFree(nint p);

    /// <summary>COM on this thread for a using block (multithreaded; an existing apartment is kept).</summary>
    public readonly struct Apartment : IDisposable
    {
        readonly bool owned;

        Apartment(bool owned) => this.owned = owned;

        public static Apartment Enter()
        {
            int hr = CoInitializeEx(0, COINIT_MULTITHREADED);
            if (hr == RPC_E_CHANGED_MODE) return new Apartment(false);  // already single-threaded: fine
            if (Failed(hr)) throw new FramesException($"COM did not start ({Hex(hr)})");
            return new Apartment(true);
        }

        public void Dispose()
        {
            if (owned) CoUninitialize();
        }
    }
}
