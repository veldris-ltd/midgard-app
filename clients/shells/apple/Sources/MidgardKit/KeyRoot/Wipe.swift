import Foundation

/// Wiping Swift's copies of the core's data key (Risks, 17). Swift cannot promise a
/// `Data` has no other copy, so these are best effort: keep every copy short-lived and
/// wipe what this module holds as soon as it has been handed over. [FR-C-9] [DD-6]
enum Wipe {
    /// Zero a buffer this code owns.
    static func wipe(_ data: inout Data) {
        data.withUnsafeMutableBytes { raw in
            guard let base = raw.baseAddress, raw.count > 0 else { return }
            _ = memset_s(base, raw.count, 0, raw.count)
        }
    }

    /// Zero the storage behind a `Data` this code was handed (the data key passed to
    /// `seal`), without copying it first: a mutable copy would be wiped instead of the
    /// original. The caller drops its reference right after the call returns.
    static func wipeInPlace(_ data: Data) {
        data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress, raw.count > 0 else { return }
            _ = memset_s(UnsafeMutableRawPointer(mutating: base), raw.count, 0, raw.count)
        }
    }

    /// A `Data` holding a copy of `bytes` that zeroes its buffer when it is released:
    /// what `unseal` hands back is lowered into a Rust buffer by the generated code and
    /// then released, so the Swift copy is wiped as soon as the core has it.
    static func selfWiping(_ bytes: UnsafeRawBufferPointer) -> Data {
        let count = bytes.count
        guard count > 0, let source = bytes.baseAddress else { return Data() }
        let buffer = UnsafeMutableRawPointer.allocate(byteCount: count, alignment: 1)
        buffer.copyMemory(from: source, byteCount: count)
        return Data(bytesNoCopy: buffer, count: count, deallocator: .custom { pointer, size in
            _ = memset_s(pointer, size, 0, size)
            pointer.deallocate()
        })
    }
}
