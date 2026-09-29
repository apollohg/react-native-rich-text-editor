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
        let initial = parent.currentRetainedBytes
        store.insert(layout("evict", bytes: cellBytes))
        XCTAssertEqual(parent.currentRetainedBytes, initial,
            "The store still owns the replacement even when no current cell maps its key")
        _ = surface.cells[0].content
        XCTAssertEqual(parent.currentRetainedBytes, initial)
    }

    func testCurrentParentMemoryCountsSharedStoresAndLayoutsOnce() {
        let cellBytes = 100
        let parentBytes = 64
        let store = TableCellLayoutStore(capacity: 1)
        func surface(_ name: String) -> ViewerTableSurface {
            let record = TableGridRecord(documentOwner: name, columns: 1, rows: 1, columnWidths: [100],
                cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: name)])
            return ViewerTableSurface(identity: name, record: record, viewportWidth: 100,
                style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { _, _ in
                    self.layout(name, bytes: cellBytes)
                }
        }
        let first = surface("shared-first")
        let second = surface("shared-second")
        let surfaces = [first, second, first]
        let parent = PreparedProseLayout(key: layout("shared-parent").key, size: first.bounds.size,
            blocks: surfaces.map { PreparedProseBlock(fragments: [], bounds: $0.bounds, tableSurface: $0) },
            retainedBytes: parentBytes + surfaces.reduce(0) { $0 + $1.retainedBytes })
        XCTAssertEqual(parent.currentRetainedBytes,
            parentBytes + first.metadataRetainedBytes + second.metadataRetainedBytes + cellBytes,
            "Aliased surfaces and a shared store must not multiply retained cell ownership")
        store.insert(layout("larger-unmapped", bytes: cellBytes * 2))
        XCTAssertEqual(parent.currentRetainedBytes,
            parentBytes + first.metadataRetainedBytes + second.metadataRetainedBytes + cellBytes * 2,
            "Every resident entry counts even when its key is absent from both surfaces")
    }

    func testSeedingResidentShapesBoundsWorkBeforeStaging() {
        let capacity = TableCellLayoutStore.maximumResidentLayouts
        let layouts = (0..<(capacity * 2)).map { index -> PreparedProseLayout in
            let name = "seed-\(index)"
            let local = layout(name)
            let key = PreparedCellShapeKey(contentKey: name, widthPixels: local.key.widthPixels,
                scaleBits: local.key.displayScaleBits, styleDigest: "store-test",
                atomGeometryDigest: "", imageGeometryDigest: "")
            return local.withCellShape(PreparedCellShape(key: key, localLayout: local))
        }
        let catalog = PreparedCellShapeCatalog()
        let context = catalog.newBuildContext(reusing: layouts)
        defer { context.close() }
        XCTAssertLessThanOrEqual(catalog.prunePassesForTesting, 1,
            "Seeding multiple resident tables must not scan the catalog once per excess shape")
        XCTAssertEqual(catalog.countForTesting, capacity,
            "The build context seeds at most the existing resident-layout capacity")
    }

    func testParallelBuildContextsReleaseEvictedShapesBeforeClosing() {
        let catalog = PreparedCellShapeCatalog()
        let workerCount = CoreTextProseLayoutEngine.maxTablePreparationWorkers
        let capacity = 8
        let store = TableCellLayoutStore(capacity: capacity)
        let contexts = (0..<workerCount).map { _ in catalog.newBuildContext() }
        defer { contexts.forEach { $0.close() } }
        let uniqueCells = TableCellLayoutStore.maximumResidentLayouts + 1
        DispatchQueue.concurrentPerform(iterations: workerCount) { worker in
            for index in stride(from: worker, to: uniqueCells, by: workerCount) {
                autoreleasepool {
                    let key = PreparedCellShapeKey(contentKey: "parallel-\(index)", widthPixels: 100,
                        scaleBits: Double(1).bitPattern, styleDigest: "store-test",
                        atomGeometryDigest: "", imageGeometryDigest: "")
                    let prepared = try! contexts[worker].resolve(key,
                        build: { self.layout("parallel-\(index)") }, bind: { _ in nil })
                    store.insert(prepared)
                }
            }
        }
        XCTAssertEqual(store.count, capacity)
        XCTAssertEqual(catalog.countForTesting, capacity,
            "Only resident cells may keep shapes alive while every worker context remains open")
        XCTAssertEqual(catalog.retainedBytesForTesting,
            store.residentLayouts.reduce(0) { $0 + $1.cellShapeCatalogRetainedBytes })
    }

    func testParentCacheRechargesMutatedStoresBeforeReleasingMounts() throws {
        let heavyBytes = 4_096
        let lightBytes = 128
        let parentBytes = 64
        func parent(_ name: String) -> PreparedProseLayout {
            let store = TableCellLayoutStore(capacity: 1)
            let record = TableGridRecord(documentOwner: name, columns: 1, rows: 2, columnWidths: [100],
                cells: (0..<2).map { TableGridCell(sourceIndex: $0, row: $0, column: 0, contentKey: "\(name)-\($0)") })
            let surface = ViewerTableSurface(identity: name, record: record, viewportWidth: 100,
                style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { cell, _ in
                    self.layout("\(name)-\(cell.sourceIndex)", bytes: cell.sourceIndex == 0 ? heavyBytes : lightBytes)
                }
            return PreparedProseLayout(key: layout(name).key, size: surface.bounds.size,
                blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds,
                    tableSurface: surface, tableBounds: surface.bounds)], retainedBytes: parentBytes + surface.retainedBytes)
        }
        let first = parent("first")
        let second = parent("second")
        let budget = heavyBytes + first.retainedBytes
        let cache = PreparedProseLayoutCache(byteBudget: budget)
        for (name, layout) in [("first", first), ("second", second)] {
            _ = try cache.value(for: layout.key) { layout }
            cache.registerDirectMount(name, layout: layout)
            _ = layout.blocks[0].tableSurface!.cells[0].content
        }
        cache.releaseDirectMount("first")
        XCTAssertEqual(cache.unmountedRetainedBytesForTesting, first.currentRetainedBytes,
            "The released parent's charge must include its newly resident heavy cell")
        cache.releaseDirectMount("second")
        XCTAssertEqual(cache.countForTesting, 1, "Two individually admitted stores must still obey the aggregate parent budget")
        XCTAssertLessThanOrEqual(cache.unmountedRetainedBytesForTesting, budget)
        XCTAssertEqual(cache.unmountedRetainedBytesForTesting, second.currentRetainedBytes)
        cache.registerDirectMount("second", layout: second)
        _ = second.blocks[0].tableSurface!.cells[1].content
        cache.releaseDirectMount("second")
        XCTAssertEqual(cache.unmountedRetainedBytesForTesting, second.currentRetainedBytes,
            "Shrinking a resident store must remove its old charge")
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

    func testNestedNeutralShapeDoesNotRetainSourceCellStore() throws {
        let catalog = PreparedCellShapeCatalog()
        let context = catalog.newBuildContext()
        defer { context.close() }
        weak var sourceStore: TableCellLayoutStore?
        var shape: PreparedCellShape!
        try autoreleasepool {
            let store = TableCellLayoutStore()
            sourceStore = store
            let record = TableGridRecord(documentOwner: "nested-source", columns: 1, rows: 1, columnWidths: [100],
                cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: "nested-cell")])
            let surface = ViewerTableSurface(identity: "nested-source", record: record, viewportWidth: 100,
                style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { _, _ in
                    self.layout("nested-cell").withCellShape(nil, preparation: { self.layout("nested-cell") })
                }
            let parent = PreparedProseLayout(key: layout("nested-parent").key, size: surface.bounds.size,
                blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface)],
                retainedBytes: surface.retainedBytes)
            let key = PreparedCellShapeKey(contentKey: "nested-parent", widthPixels: 100,
                scaleBits: Double(1).bitPattern, styleDigest: "store-test", atomGeometryDigest: "", imageGeometryDigest: "")
            shape = try context.resolve(key, build: { parent }, bind: { _ in nil }).cellShape
        }
        XCTAssertNil(sourceStore, "A neutral nested shape must not keep the source cell's shared store alive")
        let neutralTable = try XCTUnwrap(shape.localLayout.blocks.first?.tableSurface)
        let expectedSize = neutralTable.cells[0].contentSize
        neutralTable.layoutStore.insert(layout("evict-nested", bytes: PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget))
        XCTAssertNil(neutralTable.cells[0].cachedContent)
        XCTAssertEqual(neutralTable.cells[0].content.size, expectedSize,
            "Nested neutral content must still reconstruct after its own store evicts it")
    }

    func testOpenBuildContextsDoNotRetainEvictedUniqueShapes() throws {
        let catalog = PreparedCellShapeCatalog()
        let context = catalog.newBuildContext()
        let worker = context.fork()
        defer { worker.close(); context.close() }
        let uniqueCells = TableCellLayoutStore.maximumResidentLayouts + 1
        for index in 0..<uniqueCells {
            weak var shape: PreparedCellShape?
            try autoreleasepool {
                let key = PreparedCellShapeKey(contentKey: "cold-\(index)", widthPixels: 100,
                    scaleBits: Double(1).bitPattern, styleDigest: "store-test",
                    atomGeometryDigest: "", imageGeometryDigest: "")
                let prepared = try (index.isMultiple(of: 2) ? context : worker).resolve(key,
                    build: { self.layout("cold-\(index)") }, bind: { _ in nil })
                shape = prepared.cellShape
                XCTAssertNotNil(shape)
            }
            XCTAssertNil(shape, "A completed cell without an owner must release before build contexts close: \(index)")
        }
        XCTAssertEqual(catalog.countForTesting, 0)
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
