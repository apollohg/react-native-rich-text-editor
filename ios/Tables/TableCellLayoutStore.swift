import Foundation

final class TableCellLayoutStore {
    static let maximumResidentLayouts = 2_272

    struct Key: Hashable {
        let layoutKey: ProseLayoutKey
        private let cachedHash: Int
        static let additionalRetainedBytes = MemoryLayout<Self>.stride - MemoryLayout<ProseLayoutKey>.stride

        init(_ layoutKey: ProseLayoutKey) {
            self.layoutKey = layoutKey
            cachedHash = layoutKey.hashValue
        }

        func hash(into hasher: inout Hasher) { hasher.combine(cachedHash) }
        static func == (lhs: Self, rhs: Self) -> Bool { lhs.layoutKey == rhs.layoutKey }
    }

    // Each snapshot belongs to one immutable sequence of cell keys.
    final class RetainedByteSnapshot {
        // Includes the snapshot, its surface handle, and the store revision.
        static let estimatedRetainedBytes = 80
        fileprivate let store: TableCellLayoutStore
        fileprivate var revision: UInt64?
        fileprivate var bytes = 0

        init(store: TableCellLayoutStore) { self.store = store }
    }

    private final class Entry {
        let key: ProseLayoutKey
        let layout: PreparedProseLayout
        let retainedBytes: Int
        weak var previous: Entry?
        var next: Entry?

        init(_ layout: PreparedProseLayout, key: ProseLayoutKey, retainedBytes: Int) {
            self.key = key
            self.layout = layout
            self.retainedBytes = retainedBytes
        }
    }

    private let lock = NSRecursiveLock()
    private let byteBudget: Int
    private let capacity: Int
    private var entries: [Key: Entry] = [:]
    private var pins: [ProseLayoutKey: Int] = [:]
    private var newest: Entry?
    private var oldest: Entry?
    private var bytes = 0
    private var pinnedBytes = 0
    private var contentRevision: UInt64 = 0

    init(byteBudget: Int = PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget,
         capacity: Int = TableCellLayoutStore.maximumResidentLayouts) {
        self.byteBudget = max(0, byteBudget)
        self.capacity = max(0, capacity)
    }

    var residentLayouts: [PreparedProseLayout] { residentSnapshot.layouts }

    var residentSnapshot: (layouts: [PreparedProseLayout], keyBytes: Int) {
        lock.lock(); defer { lock.unlock() }
        return (entries.values.map(\.layout), entries.count * Key.additionalRetainedBytes)
    }

    var residentKeyRetainedBytes: Int {
        lock.lock(); defer { lock.unlock() }
        return entries.count * Key.additionalRetainedBytes
    }

    var count: Int { lock.lock(); defer { lock.unlock() }; return entries.count }
    var unmountedRetainedBytes: Int { lock.lock(); defer { lock.unlock() }; return bytes - pinnedBytes }

    func peek(_ key: ProseLayoutKey) -> PreparedProseLayout? {
        peek(Key(key))
    }

    func peek(_ key: Key) -> PreparedProseLayout? {
        lock.lock(); defer { lock.unlock() }
        return entries[key]?.layout
    }

    func retainedBytes<Keys: Sequence>(for keys: Keys, snapshot: RetainedByteSnapshot) -> Int
        where Keys.Element == Key {
        lock.lock(); defer { lock.unlock() }
        let canCache = snapshot.store === self && contentRevision != UInt64.max
        if canCache, snapshot.revision == contentRevision { return snapshot.bytes }
        let total = keys.reduce(entries.count * Key.additionalRetainedBytes) {
            $0 + (entries[$1]?.layout.retainedBytes ?? 0)
        }
        if canCache {
            snapshot.revision = contentRevision
            snapshot.bytes = total
        }
        return total
    }

    private func recordContentMutation() {
        if contentRevision != UInt64.max { contentRevision += 1 }
    }

    func value(for key: ProseLayoutKey, build: () -> PreparedProseLayout) -> PreparedProseLayout {
        value(for: Key(key), build: build)
    }

    func value(for key: Key, build: () -> PreparedProseLayout) -> PreparedProseLayout {
        lock.lock(); defer { lock.unlock() }
        if let entry = entries[key] {
            unlink(entry)
            link(entry)
            return entry.layout
        }
        let layout = build()
        insert(layout, for: key)
        return layout
    }

    func insert(_ layout: PreparedProseLayout, for key: ProseLayoutKey? = nil) {
        insert(layout, for: Key(key ?? layout.key))
    }

    func insert(_ layout: PreparedProseLayout, for key: Key) {
        lock.lock(); defer { lock.unlock() }
        let retainedBytes = layout.retainedBytes + layout.cellShapeCatalogRetainedBytes + Key.additionalRetainedBytes
        if let existing = entries[key], existing.layout === layout, existing.retainedBytes == retainedBytes {
            unlink(existing)
            link(existing)
            return
        }
        let entry = Entry(layout, key: key.layoutKey, retainedBytes: retainedBytes)
        if let existing = entries[key] { remove(existing) }
        recordContentMutation()
        entries[key] = entry
        bytes += entry.retainedBytes
        if pins[entry.key, default: 0] > 0 { pinnedBytes += entry.retainedBytes }
        link(entry)
        evict()
    }

    func pin(_ key: ProseLayoutKey) {
        lock.lock(); defer { lock.unlock() }
        if pins[key, default: 0] == 0 { pinnedBytes += entries[Key(key)]?.retainedBytes ?? 0 }
        pins[key, default: 0] += 1
    }

    func unpin(_ key: ProseLayoutKey) {
        lock.lock(); defer { lock.unlock() }
        guard let count = pins[key] else { return }
        if count > 1 { pins[key] = count - 1; return }
        pins.removeValue(forKey: key)
        pinnedBytes -= entries[Key(key)]?.retainedBytes ?? 0
        evict()
    }

    private func evict() {
        var candidate = oldest
        while bytes - pinnedBytes > byteBudget || entries.count > capacity {
            guard let entry = candidate else { break }
            candidate = entry.previous
            if pins[entry.key, default: 0] == 0 { remove(entry) }
        }
    }

    private func remove(_ entry: Entry) {
        recordContentMutation()
        unlink(entry)
        entries.removeValue(forKey: Key(entry.key))
        bytes -= entry.retainedBytes
        if pins[entry.key, default: 0] > 0 { pinnedBytes -= entry.retainedBytes }
    }

    private func link(_ entry: Entry) {
        entry.previous = nil
        entry.next = newest
        newest?.previous = entry
        newest = entry
        if oldest == nil { oldest = entry }
    }

    private func unlink(_ entry: Entry) {
        entry.previous?.next = entry.next
        entry.next?.previous = entry.previous
        if newest === entry { newest = entry.next }
        if oldest === entry { oldest = entry.previous }
        entry.previous = nil
        entry.next = nil
    }
}
