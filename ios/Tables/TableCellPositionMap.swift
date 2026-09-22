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
        self.segments = segments.sorted { $0.localScalarRange.lowerBound < $1.localScalarRange.lowerBound }
    }

    func globalScalar(
        forLocalScalar localScalar: UInt32,
        currentRevision: UInt64? = nil,
        currentEpoch: UInt64? = nil
    ) -> UInt32? {
        if let currentRevision, currentRevision != binding.documentRevision { return nil }
        if let currentEpoch, currentEpoch != binding.positionEpoch { return nil }
        var resolved: UInt32?
        for segment in segments where segment.localScalarRange.contains(localScalar) {
            let offset = localScalar - segment.localScalarRange.lowerBound
            let (candidate, overflow) = segment.globalScalarStart.addingReportingOverflow(offset)
            guard !overflow, resolved == nil || resolved == candidate else { return nil }
            resolved = candidate
        }
        return resolved
    }

    func localScalar(
        forGlobalScalar globalScalar: UInt32,
        currentRevision: UInt64? = nil,
        currentEpoch: UInt64? = nil
    ) -> UInt32? {
        if let currentRevision, currentRevision != binding.documentRevision { return nil }
        if let currentEpoch, currentEpoch != binding.positionEpoch { return nil }
        var resolved: UInt32?
        for segment in segments where globalScalar >= segment.globalScalarStart {
            let width = segment.localScalarRange.upperBound - segment.localScalarRange.lowerBound
            let offset = globalScalar - segment.globalScalarStart
            guard offset < width else { continue }
            let candidate = segment.localScalarRange.lowerBound + offset
            guard resolved == nil || resolved == candidate else { return nil }
            resolved = candidate
        }
        return resolved
    }

    func globalScalarRange(fromLocalScalar start: UInt32, toLocalScalar end: UInt32) -> (from: UInt32, to: UInt32)? {
        guard start <= end,
              let globalStart = globalScalar(forLocalScalar: start),
              let globalEnd = globalScalar(forLocalScalar: end),
              globalEnd >= globalStart,
              UInt64(globalEnd) - UInt64(globalStart) == UInt64(end) - UInt64(start)
        else { return nil }
        let limit = UInt64(end) + 1
        var covered = UInt64(start)
        for segment in segments {
            let lower = max(UInt64(segment.localScalarRange.lowerBound), UInt64(start))
            let upper = min(UInt64(segment.localScalarRange.upperBound), limit)
            guard lower < upper else { continue }
            guard lower <= covered else { return nil }
            let actual = UInt64(segment.globalScalarStart) + lower - UInt64(segment.localScalarRange.lowerBound)
            let expected = UInt64(globalStart) + lower - UInt64(start)
            guard actual == expected else { return nil }
            covered = max(covered, upper)
        }
        guard covered == limit else { return nil }
        return (globalStart, globalEnd)
    }

    func globalScalar(forLocalUTF16 offset: Int, in text: String) -> UInt32? {
        globalScalar(forLocalScalar: PositionBridge.utf16OffsetToScalar(offset, in: text))
    }

    func globalScalarRange(forLocalUTF16 range: NSRange, in text: String) -> (UInt32, UInt32)? {
        guard range.location != NSNotFound,
              range.location >= 0,
              range.length >= 0,
              range.location <= text.utf16.count - range.length
        else { return nil }
        return globalScalarRange(
            fromLocalScalar: PositionBridge.utf16OffsetToScalar(range.location, in: text),
            toLocalScalar: PositionBridge.utf16OffsetToScalar(range.location + range.length, in: text)
        )
    }
}
