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
    private struct Entry { let value: CGFloat; let bytes: Int; var generation: UInt64 }
    private var entries: [TableCellMeasurementKey: Entry] = [:]
    private(set) var retainedBytes = 0
    private let byteLimit: Int
    private let capacity: Int
    private var nextGeneration: UInt64 = 0
    private var recency: [(TableCellMeasurementKey, UInt64)] = []
    private var recencyHead = 0

    init(capacity: Int = 32_768, byteLimit: Int? = nil) {
        self.capacity = max(1, capacity)
        self.byteLimit = byteLimit ?? 32 * 1024 * 1024
    }

    func value(for key: TableCellMeasurementKey) -> CGFloat? {
        guard var entry = entries[key] else { return nil }
        entry.generation = generation()
        entries[key] = entry
        recency.append((key, entry.generation))
        return entry.value
    }

    func insert(_ value: CGFloat, for key: TableCellMeasurementKey) {
        guard value.isFinite && value >= 0 else { return }
        if let previous = entries.removeValue(forKey: key) { retainedBytes -= previous.bytes }
        let bytes = max(64, key.documentOwner.utf8.count + key.contentKey.utf8.count + key.themeDigest.utf8.count + 48)
        entries[key] = Entry(value: value, bytes: bytes, generation: generation())
        recency.append((key, nextGeneration))
        retainedBytes += bytes
        while retainedBytes > byteLimit || entries.count > capacity {
            guard recencyHead < recency.count else { break }
            let (key, generation) = recency[recencyHead]
            recencyHead += 1
            guard let entry = entries[key], entry.generation == generation else { continue }
            entries.removeValue(forKey: key)
            retainedBytes -= entry.bytes
        }
        if recencyHead > capacity, recencyHead * 2 > recency.count {
            recency.removeFirst(recencyHead)
            recencyHead = 0
        }
    }

    private func generation() -> UInt64 {
        nextGeneration &+= 1
        return nextGeneration
    }
}
