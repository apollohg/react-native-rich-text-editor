import Foundation

struct TableCellMeasurementKey: Hashable {
    let documentOwner: String
    let contentKey: String
    let innerWidthPixels: Int
    let themeDigest: String
    let fontEnvironmentRevision: Int
    let textScale: CGFloat
    let attachmentRevision: Int
}

final class TableCellMeasurementCache {
    private final class Node {
        let key: TableCellMeasurementKey
        let value: CGFloat
        let bytes: Int
        weak var previous: Node?
        var next: Node?

        init(key: TableCellMeasurementKey, value: CGFloat, bytes: Int) {
            self.key = key
            self.value = value
            self.bytes = bytes
        }
    }

    private var entries: [TableCellMeasurementKey: Node] = [:]
    private(set) var retainedBytes = 0
    private let byteLimit: Int
    private let capacity: Int
    private var mostRecent: Node?
    private var leastRecent: Node?

    var metadataCount: Int { entries.count }

    init(capacity: Int = 32_768, byteLimit: Int? = nil) {
        self.capacity = max(1, capacity)
        self.byteLimit = byteLimit ?? 32 * 1024 * 1024
    }

    func value(for key: TableCellMeasurementKey) -> CGFloat? {
        guard let entry = entries[key] else { return nil }
        moveToMostRecent(entry)
        return entry.value
    }

    func insert(_ value: CGFloat, for key: TableCellMeasurementKey) {
        guard value.isFinite && value >= 0 else { return }
        if let previous = entries.removeValue(forKey: key) {
            unlink(previous)
            retainedBytes -= previous.bytes
        }
        let bytes = max(64, key.documentOwner.utf8.count + key.contentKey.utf8.count + key.themeDigest.utf8.count + 48)
        let entry = Node(key: key, value: value, bytes: bytes)
        entries[key] = entry
        linkAsMostRecent(entry)
        retainedBytes += bytes
        while retainedBytes > byteLimit || entries.count > capacity {
            guard let oldest = leastRecent else { break }
            unlink(oldest)
            entries.removeValue(forKey: oldest.key)
            retainedBytes -= oldest.bytes
        }
    }

    private func moveToMostRecent(_ entry: Node) {
        guard entry !== mostRecent else { return }
        unlink(entry)
        linkAsMostRecent(entry)
    }

    private func linkAsMostRecent(_ entry: Node) {
        entry.previous = nil
        entry.next = mostRecent
        mostRecent?.previous = entry
        mostRecent = entry
        if leastRecent == nil { leastRecent = entry }
    }

    private func unlink(_ entry: Node) {
        let previous = entry.previous
        let next = entry.next
        previous?.next = next
        next?.previous = previous
        if mostRecent === entry { mostRecent = next }
        if leastRecent === entry { leastRecent = previous }
        entry.previous = nil
        entry.next = nil
    }
}
