func verifyIntegers<T: FixedWidthInteger>(_ type: T.Type) throws {
    let values: [T] = [0, 1, T.min, T.max]
    let maximumPrefix = MemoryLayout<UInt64>.size
    for sliced in [false, true] {
        for prefix in 0..<maximumPrefix {
            let sliceOffset = sliced ? maximumPrefix : 0
            var data = Data(repeating: 0, count: sliceOffset + prefix)
            for value in values {
                var encoded = value.bigEndian
                withUnsafeBytes(of: &encoded) { data.append(contentsOf: $0) }
            }
            data.append(Data(repeating: 0, count: maximumPrefix * 2))
            if sliced { data.removeFirst(sliceOffset) }
            precondition(data.startIndex == sliceOffset)
            var reader = (data: data, offset: data.startIndex + prefix)
            for expected in values {
                let before = reader.offset
                let actual: T = try readInt(&reader)
                precondition(actual == expected,
                    "\(type), sliced=\(sliced), prefix=\(prefix): expected \(expected), got \(actual)")
                precondition(reader.offset == before + MemoryLayout<T>.size)
            }
        }
    }
    var encodedMaximum = T.max.bigEndian
    let exactData = withUnsafeBytes(of: &encodedMaximum) { Data($0) }
    var exactReader = (data: exactData, offset: 0)
    let exactValue: T = try readInt(&exactReader)
    precondition(exactValue == T.max && exactReader.offset == exactData.count)
    for remaining in 0..<MemoryLayout<T>.size {
        var reader = (data: Data(repeating: 0, count: remaining), offset: 0)
        do {
            let _: T = try readInt(&reader)
            preconditionFailure("\(type): truncated buffer with \(remaining) bytes was accepted")
        } catch UniffiInternalError.bufferOverflow {
            precondition(reader.offset == 0, "Failed reads must not advance")
        }
    }
}
try verifyIntegers(UInt8.self)
try verifyIntegers(Int8.self)
try verifyIntegers(UInt16.self)
try verifyIntegers(Int16.self)
try verifyIntegers(UInt32.self)
try verifyIntegers(Int32.self)
try verifyIntegers(UInt64.self)
try verifyIntegers(Int64.self)
print("Swift integer reader: signed/unsigned, unaligned, sliced and truncated controls passed")
