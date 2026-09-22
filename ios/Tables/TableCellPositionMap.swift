import Foundation

struct TableCellPositionMap {
    struct Binding: Equatable {
        let cellSourcePosition: UInt32
        let documentRevision: UInt64
        let positionEpoch: UInt64
    }

    struct Segment: Equatable {
        let localScalarRange: Range<UInt32>
        let globalScalarStart: UInt32
    }

    let binding: Binding
    let segments: [Segment]

    init(binding: Binding, segments: [Segment]) {
        self.binding = binding
        self.segments = segments
    }

    func globalScalar(
        forLocalScalar localScalar: UInt32,
        currentRevision: UInt64? = nil,
        currentEpoch: UInt64? = nil
    ) -> UInt32? {
        if let currentRevision, currentRevision != binding.documentRevision { return nil }
        if let currentEpoch, currentEpoch != binding.positionEpoch { return nil }
        guard let segment = segments.first(where: { $0.localScalarRange.contains(localScalar) }) else {
            return nil
        }
        let offset = localScalar - segment.localScalarRange.lowerBound
        let (globalScalar, overflow) = segment.globalScalarStart.addingReportingOverflow(offset)
        return overflow ? nil : globalScalar
    }

    func globalScalar(forLocalUTF16 offset: Int, in text: String) -> UInt32? {
        globalScalar(forLocalScalar: PositionBridge.utf16OffsetToScalar(offset, in: text))
    }

    func globalScalarRange(forLocalUTF16 range: NSRange, in text: String) -> (UInt32, UInt32)? {
        guard range.location != NSNotFound,
              range.location >= 0,
              range.length >= 0,
              range.location <= text.utf16.count - range.length,
              let start = globalScalar(forLocalUTF16: range.location, in: text),
              let end = globalScalar(forLocalUTF16: range.location + range.length, in: text),
              hasContiguousMapping(from: start, to: end, localRange: range, text: text)
        else { return nil }
        return (min(start, end), max(start, end))
    }

    private func hasContiguousMapping(
        from start: UInt32,
        to end: UInt32,
        localRange: NSRange,
        text: String
    ) -> Bool {
        let localStart = PositionBridge.utf16OffsetToScalar(localRange.location, in: text)
        let localEnd = PositionBridge.utf16OffsetToScalar(localRange.location + localRange.length, in: text)
        guard localStart <= localEnd else { return false }
        var local = localStart
        var global = start
        while local < localEnd {
            guard let next = globalScalar(forLocalScalar: local), next == global else { return false }
            guard local < UInt32.max, global < UInt32.max else { return false }
            local += 1
            global += 1
        }
        return global == end
    }
}
