import Foundation

struct TableScalarExtent: Equatable {
    let scalarStart: UInt32
    let scalarEnd: UInt32
}

struct RootTablePositionMap {
    private struct Marker {
        let localStart: UInt32
        let extent: TableScalarExtent
        var localEnd: UInt32 { localStart + 1 }
        var delta: UInt32 { extent.scalarEnd - extent.scalarStart - 1 }
    }

    private let markers: [Marker]
    private let localLength: UInt32
    private let globalLength: UInt32
    let extents: [String: TableScalarExtent]
    var hasTables: Bool { !markers.isEmpty }

    static func fromRendered(_ text: NSAttributedString, extents: [String: TableScalarExtent],
                             scalarLength: UInt32) -> RootTablePositionMap? {
        var markers: [Marker] = []
        var observed = Set<String>()
        var delta: UInt64 = 0
        var valid = true
        let string = text.string as NSString
        text.enumerateAttribute(RenderBridgeAttributes.rootTableMarker,
                                in: NSRange(location: 0, length: text.length)) { value, range, stop in
            guard let value else { return }
            guard let key = value as? String, let extent = extents[key],
                  observed.insert(key).inserted, range.length == 1,
                  string.substring(with: range) == RenderBridge.rootTableAnchor,
                  extent.scalarStart < extent.scalarEnd else {
                valid = false
                stop.pointee = true
                return
            }
            let localStart = PositionBridge.utf16OffsetToScalar(range.location, in: text)
            guard localStart < UInt32.max, UInt64(extent.scalarStart) == UInt64(localStart) + delta,
                  markers.last.map({ $0.localEnd < localStart }) ?? true else {
                valid = false
                stop.pointee = true
                return
            }
            let marker = Marker(localStart: localStart, extent: extent)
            markers.append(marker)
            delta += UInt64(marker.delta)
        }
        let localLength = PositionBridge.utf16OffsetToScalar(text.length, in: text)
        guard valid, observed == Set(extents.keys), UInt64(localLength) + delta == UInt64(scalarLength) else { return nil }
        return RootTablePositionMap(markers: markers, localLength: localLength,
                                    globalLength: scalarLength, extents: extents)
    }

    func globalScalar(local: UInt32) -> UInt32? {
        guard local <= localLength,
              !markers.contains(where: { $0.localStart <= local && local <= $0.localEnd }) else { return nil }
        return globalBoundary(local: local)
    }

    func globalRange(from: UInt32, to: UInt32) -> Range<UInt32>? {
        guard from <= to,
              !markers.contains(where: { from <= $0.localEnd && $0.localStart <= to }),
              let start = globalScalar(local: from), let end = globalScalar(local: to) else { return nil }
        return start..<end
    }

    func localScalar(global: UInt32) -> UInt32? {
        guard global <= globalLength,
              !markers.contains(where: { $0.extent.scalarStart <= global && global <= $0.extent.scalarEnd }) else { return nil }
        return localBoundary(global: global)
    }

    func isImmediatelyAfterTable(local: UInt32) -> Bool {
        markers.contains { UInt64(local) == UInt64($0.localEnd) + 1 }
    }

    func globalBoundary(local: UInt32) -> UInt32 {
        let local = min(local, localLength)
        let delta = markers.prefix { $0.localEnd <= local }.reduce(UInt32.zero) { $0 + $1.delta }
        return local + delta
    }

    func localBoundary(global: UInt32) -> UInt32 {
        let global = min(global, globalLength)
        var delta: UInt32 = 0
        for marker in markers {
            if global <= marker.extent.scalarStart { break }
            if global <= marker.extent.scalarEnd { return marker.localEnd }
            delta += marker.delta
        }
        return global - delta
    }

    func isPositionRepresentable(_ scalar: UInt32) -> Bool {
        !markers.contains { $0.extent.scalarStart < scalar && scalar < $0.extent.scalarEnd }
    }

    func isRangeRepresentable(from: UInt32, to: UInt32) -> Bool {
        from <= to && !markers.contains { from < $0.extent.scalarEnd && $0.extent.scalarStart < to }
    }

    func isInputRangeSafe(from: UInt32, to: UInt32) -> Bool {
        from <= to && !markers.contains { from <= $0.extent.scalarEnd && $0.extent.scalarStart <= to }
    }
}
