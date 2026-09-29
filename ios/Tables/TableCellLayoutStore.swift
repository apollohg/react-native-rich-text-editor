import Foundation

final class TableCellLayoutStore {
    static let maximumResidentLayouts = 2_272

    private final class Entry {
        let key: ProseLayoutKey
        let layout: PreparedProseLayout
        let retainedBytes: Int
        weak var previous: Entry?
        var next: Entry?

        init(_ layout: PreparedProseLayout, key: ProseLayoutKey) {
            self.key = key
            self.layout = layout
            self.retainedBytes = layout.retainedBytes + layout.cellShapeCatalogRetainedBytes
        }
    }

    private let lock = NSRecursiveLock()
    private let byteBudget: Int
    private let capacity: Int
    private var entries: [ProseLayoutKey: Entry] = [:]
    private var pins: [ProseLayoutKey: Int] = [:]
    private var newest: Entry?
    private var oldest: Entry?
    private var bytes = 0
    private var pinnedBytes = 0

    init(byteBudget: Int = PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget,
         capacity: Int = TableCellLayoutStore.maximumResidentLayouts) {
        self.byteBudget = max(0, byteBudget)
        self.capacity = max(0, capacity)
    }

    var count: Int { lock.lock(); defer { lock.unlock() }; return entries.count }
    var unmountedRetainedBytes: Int { lock.lock(); defer { lock.unlock() }; return bytes - pinnedBytes }

    func peek(_ key: ProseLayoutKey) -> PreparedProseLayout? {
        lock.lock(); defer { lock.unlock() }
        return entries[key]?.layout
    }

    func value(for key: ProseLayoutKey, build: () -> PreparedProseLayout) -> PreparedProseLayout {
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
        lock.lock(); defer { lock.unlock() }
        let entry = Entry(layout, key: key ?? layout.key)
        if let existing = entries[entry.key] { remove(existing) }
        entries[entry.key] = entry
        bytes += entry.retainedBytes
        if pins[entry.key, default: 0] > 0 { pinnedBytes += entry.retainedBytes }
        link(entry)
        evict()
    }

    func pin(_ key: ProseLayoutKey) {
        lock.lock(); defer { lock.unlock() }
        if pins[key, default: 0] == 0 { pinnedBytes += entries[key]?.retainedBytes ?? 0 }
        pins[key, default: 0] += 1
    }

    func unpin(_ key: ProseLayoutKey) {
        lock.lock(); defer { lock.unlock() }
        guard let count = pins[key] else { return }
        if count > 1 { pins[key] = count - 1; return }
        pins.removeValue(forKey: key)
        pinnedBytes -= entries[key]?.retainedBytes ?? 0
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
        unlink(entry)
        entries.removeValue(forKey: entry.key)
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
