import XCTest

final class TableCellLayoutStoreTests: XCTestCase {
    private func layout(_ name: String, bytes: Int = 100) -> PreparedProseLayout {
        let key = ProseLayoutKey(semanticKey: name, widthPixels: 100, themeDigest: "store-test",
            nativeFontRevision: 0, fontEnvironmentRevision: 0, displayScale: 1,
            attachmentRevision: 0, generationIdentity: "store-test", semanticGenerationIdentity: "store-test")
        return PreparedProseLayout(key: key, size: CGSize(width: 100, height: 20), blocks: [], retainedBytes: bytes)
    }

    func testStoreEvictsByRetainedBytes() {
        let store = TableCellLayoutStore(byteBudget: 200)
        let first = layout("first")
        let second = layout("second")
        let third = layout("third")
        store.insert(first)
        store.insert(second)
        XCTAssertTrue(store.value(for: first.key) { XCTFail("Resident cell was rebuilt"); return first } === first)
        store.insert(third)
        XCTAssertNil(store.peek(second.key), "Least-recent cell must be released to respect the byte budget")
        XCTAssertTrue(store.peek(first.key) === first)
        XCTAssertTrue(store.peek(third.key) === third)
        XCTAssertEqual(store.unmountedRetainedBytes, 200)
    }

    func testCurrentParentMemoryFollowsCellEvictionAndRebuild() {
        let cellBytes = 100
        let parentBytes = 64
        let store = TableCellLayoutStore(byteBudget: cellBytes, capacity: 1)
        let record = TableGridRecord(documentOwner: "memory", columns: 1, rows: 1, columnWidths: [100],
            cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: "cell")])
        let surface = ViewerTableSurface(identity: "memory", record: record, viewportWidth: 100,
            style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { _, _ in
                self.layout("cell", bytes: cellBytes)
            }
        let parent = PreparedProseLayout(key: layout("parent").key, size: surface.bounds.size,
            blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface,
                                       tableBounds: surface.bounds)], retainedBytes: parentBytes + surface.retainedBytes)
        let initial = parent.currentRetainedBytesForTesting
        store.insert(layout("evict", bytes: cellBytes))
        XCTAssertEqual(parent.currentRetainedBytesForTesting, initial - cellBytes)
        _ = surface.cells[0].content
        XCTAssertEqual(parent.currentRetainedBytesForTesting, initial)
    }

    func testActiveInputCellStaysPreparedOffscreen() {
        let store = TableCellLayoutStore(byteBudget: 100)
        let active = layout("active")
        store.pin(active.key)
        store.insert(active)
        for index in 0..<20 { store.insert(layout("scroll-\(index)")) }
        XCTAssertTrue(store.peek(active.key) === active, "Scrolling must not evict the input cell")
        XCTAssertLessThanOrEqual(store.unmountedRetainedBytes, 100)
        store.unpin(active.key)
        XCTAssertNil(store.peek(active.key), "Unbinding returns the old cell to normal eviction")
    }

    func testEvictionReleasesShapesEvenWhileParentRemainsMounted() throws {
        let retainedBudget = 1_024
        let store = TableCellLayoutStore(byteBudget: retainedBudget)
        let catalog = PreparedCellShapeCatalog()
        let context = catalog.newBuildContext()
        weak var releasedShape: PreparedCellShape?
        var parent: PreparedProseLayout!
        try autoreleasepool {
            let plain = layout("shaped")
            let shapeKey = PreparedCellShapeKey(contentKey: "shaped", widthPixels: 100,
                scaleBits: Double(1).bitPattern, styleDigest: "store-test", atomGeometryDigest: "", imageGeometryDigest: "")
            var prepared: PreparedProseLayout? = try context.resolve(shapeKey, build: { plain }, bind: { _ in nil })
            releasedShape = prepared?.cellShape
            let record = TableGridRecord(documentOwner: "store-test", columns: 1, rows: 1, columnWidths: [100],
                cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: "shaped")])
            let surface = ViewerTableSurface(identity: "store-test", record: record, viewportWidth: 100,
                style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { _, _ in
                    prepared ?? self.layout("shaped")
                }
            parent = PreparedProseLayout(key: layout("parent").key, size: surface.bounds.size,
                blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface,
                                           tableBounds: surface.bounds)], retainedBytes: 100)
            catalog.retainParent(parent)
            context.close()
            prepared = nil
        }
        defer { catalog.releaseParent(parent) }
        XCTAssertNotNil(releasedShape)
        store.insert(layout("replacement", bytes: retainedBudget))
        XCTAssertNil(releasedShape, "The catalogue must not independently retain an evicted cell shape")
        XCTAssertEqual(catalog.countForTesting, 0)
        XCTAssertEqual(parent.blocks.count, 1, "The parent remains mounted throughout cell eviction")
    }

    func testDistinctNeutralShapesHaveDistinctStoreKeys() throws {
        let catalog = PreparedCellShapeCatalog()
        let context = catalog.newBuildContext()
        defer { context.close() }
        let store = TableCellLayoutStore()
        for name in ["first", "second"] {
            let key = PreparedCellShapeKey(contentKey: name, widthPixels: 100,
                scaleBits: Double(1).bitPattern, styleDigest: "store-test", atomGeometryDigest: "", imageGeometryDigest: "")
            let prepared = try context.resolve(key, build: { self.layout(name) }, bind: { _ in nil })
            store.insert(try XCTUnwrap(prepared.cellShape).localLayout)
        }
        XCTAssertEqual(store.count, 2, "Different nested cell shapes must not replace one another in the store")
    }

    func testShapeStorageCountsAgainstUnmountedBudget() throws {
        let catalog = PreparedCellShapeCatalog()
        let context = catalog.newBuildContext()
        defer { context.close() }
        let plain = layout("shaped")
        let key = PreparedCellShapeKey(contentKey: "shaped", widthPixels: 100,
            scaleBits: Double(1).bitPattern, styleDigest: "store-test", atomGeometryDigest: "", imageGeometryDigest: "")
        let prepared = try context.resolve(key, build: { plain }, bind: { _ in nil })
        XCTAssertGreaterThan(prepared.cellShapeCatalogRetainedBytes, 0)
        let store = TableCellLayoutStore(byteBudget: prepared.retainedBytes)
        store.insert(prepared)
        XCTAssertNil(store.peek(prepared.key), "The shape graph must also fit the unmounted budget")
    }

    func testPresentationPinsOnlyItsCurrentWindow() {
        let store = TableCellLayoutStore(byteBudget: 100, capacity: 1)
        let record = TableGridRecord(documentOwner: "window", columns: 1, rows: 3, columnWidths: [100],
            cells: (0..<3).map { TableGridCell(sourceIndex: $0, row: $0, column: 0, contentKey: "cell-\($0)") })
        let surface = ViewerTableSurface(identity: "window", record: record, viewportWidth: 100,
            style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { cell, _ in
                self.layout("cell-\(cell.sourceIndex)")
            }
        let parent = PreparedProseLayout(key: layout("parent").key, size: surface.bounds.size,
            blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface,
                                       tableBounds: surface.bounds)], retainedBytes: 100)
        let owner = ViewerTablePresentationOwner()
        let shown = ViewerTablePresentation.project(layout: parent, owner: owner, viewport: .known(surface.bounds))
        XCTAssertEqual(shown.cells.count, 3)
        XCTAssertTrue(surface.cells.allSatisfy { $0.cachedContent != nil }, "Presented cells must survive LRU pressure")
        _ = ViewerTablePresentation.project(layout: parent, owner: owner, viewport: .known(.zero))
        XCTAssertEqual(store.count, 1, "Hidden cells must return to the bounded unmounted cache")
        XCTAssertLessThanOrEqual(store.unmountedRetainedBytes, 100)
    }

    func testRebuiltReusedCellKeepsItsStoreKey() {
        let store = TableCellLayoutStore()
        let retainedKey = layout("before-rebind").key
        let rebuilt = layout("after-rebind")
        XCTAssertTrue(store.value(for: retainedKey) { rebuilt } === rebuilt)
        XCTAssertTrue(store.peek(retainedKey) === rebuilt, "A reused cell keeps its lookup identity after eviction")
        XCTAssertTrue(store.value(for: retainedKey) {
            XCTFail("The rebuilt cell was lost under a different artifact key")
            return rebuilt
        } === rebuilt)
    }

    func testOversizedUnmountedLayoutIsReturnedWithoutRetention() {
        let store = TableCellLayoutStore(byteBudget: 100)
        let oversized = layout("oversized", bytes: 101)
        XCTAssertTrue(store.value(for: oversized.key) { oversized } === oversized)
        XCTAssertNil(store.peek(oversized.key))
        XCTAssertEqual(store.unmountedRetainedBytes, 0)
    }
}
