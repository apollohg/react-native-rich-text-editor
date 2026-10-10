#!/usr/bin/env python3
"""Apply the checked integer-reader optimization to UniFFI Swift output."""
import pathlib
import sys

ORIGINAL_READER = """fileprivate func readInt<T: FixedWidthInteger>(_ reader: inout (data: Data, offset: Data.Index)) throws -> T {
    let range = reader.offset..<reader.offset + MemoryLayout<T>.size
    guard reader.data.count >= range.upperBound else {
        throw UniffiInternalError.bufferOverflow
    }
    if T.self == UInt8.self {
        let value = reader.data[reader.offset]
        reader.offset += 1
        return value as! T
    }
    var value: T = 0
    let _ = withUnsafeMutableBytes(of: &value, { reader.data.copyBytes(to: $0, from: range)})
    reader.offset = range.upperBound
    return value.bigEndian
}"""

OPTIMIZED_READER = """fileprivate func readInt<T: FixedWidthInteger>(_ reader: inout (data: Data, offset: Data.Index)) throws -> T {
    let range = reader.offset..<reader.offset + MemoryLayout<T>.size
    guard reader.data.count >= range.upperBound else {
        throw UniffiInternalError.bufferOverflow
    }
    if T.self == UInt8.self {
        let value = reader.data[reader.offset]
        reader.offset += 1
        return value as! T
    }
    let value: T = reader.data.withUnsafeBytes { bytes in
        bytes.loadUnaligned(fromByteOffset: reader.offset - reader.data.startIndex, as: T.self)
    }
    reader.offset = range.upperBound
    return value.bigEndian
}"""


def main():
    path = pathlib.Path(sys.argv[1])
    source = path.read_text()
    if source.count(ORIGINAL_READER) != 1 or source.count("fileprivate func readInt<") != 1:
        raise SystemExit("Unexpected UniFFI Swift integer reader; review the binding optimization before regenerating")
    path.write_text(source.replace(ORIGINAL_READER, OPTIMIZED_READER))


if __name__ == "__main__":
    main()
