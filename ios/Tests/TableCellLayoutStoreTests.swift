import XCTest

final class TableCellLayoutStoreTests: XCTestCase {
    private let cachedKeyBytes = MemoryLayout<Int>.stride
    private func layout(_ name: String, bytes: Int = 100) -> PreparedProseLayout {
        let key = ProseLayoutKey(semanticKey: name, widthPixels: 100, themeDigest: "store-test",
            nativeFontRevision: 0, fontEnvironmentRevision: 0, displayScale: 1,
            attachmentRevision: 0, generationIdentity: "store-test", semanticGenerationIdentity: "store-test")
        return PreparedProseLayout(key: key, size: CGSize(width: 100, height: 20), blocks: [], retainedBytes: bytes)
    }

    func testResidentAdmissionChargesCachedLookupKeyStorage() {
        let payloadBytes = 100
        let cachedHashBytes = MemoryLayout<Int>.stride
        let exactBudget = (payloadBytes + cachedHashBytes) * 2
        for (budget, expectedCount) in [(exactBudget, 2), (exactBudget - 1, 1)] {
            let store = TableCellLayoutStore(byteBudget: budget)
            store.insert(layout("first", bytes: payloadBytes))
            store.insert(layout("second", bytes: payloadBytes))
            XCTAssertEqual(store.count, expectedCount, "budget=\(budget) must charge the cached dictionary key")
            XCTAssertEqual(store.unmountedRetainedBytes, (payloadBytes + cachedHashBytes) * expectedCount)
        }
    }

    func testStoreLookupPreservesCanonicalStringEquality() {
        let store = TableCellLayoutStore()
        let composed = layout("caf\u{e9}")
        let decomposed = layout("cafe\u{301}", bytes: 200)
        store.insert(composed)
        XCTAssertTrue(store.peek(decomposed.key) === composed)
        store.insert(decomposed)
        XCTAssertEqual(store.count, 1, "Canonically equivalent keys must replace the same resident entry")
        XCTAssertTrue(store.peek(composed.key) === decomposed)
    }

    func testStoreEvictsByRetainedBytes() {
        let budget = (100 + cachedKeyBytes) * 2
        let store = TableCellLayoutStore(byteBudget: budget)
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
        XCTAssertEqual(store.unmountedRetainedBytes, budget)
    }

    func testIdenticalInsertionKeepsRetainedSnapshotAndRefreshesRecency() {
        let store = TableCellLayoutStore(capacity: 2)
        let first = layout("first")
        let second = layout("second")
        store.insert(first)
        store.insert(second)
        var visitedKeys = 0
        let keys = [first.key, second.key].map(TableCellLayoutStore.Key.init).lazy.map { key in
            visitedKeys += 1
            return key
        }
        let snapshot = TableCellLayoutStore.RetainedByteSnapshot(store: store)
        let initialBytes = store.retainedBytes(for: keys, snapshot: snapshot)
        XCTAssertEqual(visitedKeys, 2)
        store.insert(first)
        XCTAssertEqual(store.retainedBytes(for: keys, snapshot: snapshot), initialBytes)
        XCTAssertEqual(visitedKeys, 2, "An identical resident insertion must not rescan every cell key")
        let replacement = layout("first", bytes: first.retainedBytes * 2)
        store.insert(replacement)
        XCTAssertEqual(store.retainedBytes(for: keys, snapshot: snapshot), initialBytes + first.retainedBytes)
        XCTAssertEqual(visitedKeys, 4, "A different layout must invalidate the retained snapshot")
        store.insert(second)
        store.insert(layout("third"))
        XCTAssertNil(store.peek(first.key), "Identical reinsertion must still refresh LRU recency")
        XCTAssertTrue(store.peek(second.key) === second)
    }

    func testIdenticalLayoutInsertionUpdatesChangedNestedShapeCharge() {
        let child = layout("nested-child")
        let nestedStore = TableCellLayoutStore(capacity: 1)
        let record = TableGridRecord(documentOwner: "nested-charge", columns: 1, rows: 1, columnWidths: [100],
            cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: child.key.semanticKey)])
        let surface = ViewerTableSurface(identity: record.documentOwner, record: record, viewportWidth: 100,
            style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: nestedStore) { _, _ in child }
        let parent = PreparedProseLayout(key: layout("nested-parent").key, size: surface.bounds.size,
            blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface)], retainedBytes: 100)
        let baseCharge = parent.retainedBytes + cachedKeyBytes
        let store = TableCellLayoutStore(byteBudget: baseCharge)
        store.insert(parent)
        store.pin(parent.key)
        let shapeKey = PreparedCellShapeKey(contentKey: child.key.semanticKey, widthPixels: child.key.widthPixels,
            scaleBits: child.key.displayScaleBits, styleDigest: "store-test", atomGeometryDigest: "", imageGeometryDigest: "")
        let shapedChild = child.withCellShape(PreparedCellShape(key: shapeKey, localLayout: child))
        nestedStore.insert(shapedChild)
        XCTAssertGreaterThan(parent.cellShapeCatalogRetainedBytes, 0)
        store.insert(parent)
        XCTAssertTrue(store.peek(parent.key) === parent)
        XCTAssertEqual(store.unmountedRetainedBytes, 0, "Nested growth remains charged to the pinned owner")
        nestedStore.insert(child)
        store.insert(parent)
        store.unpin(parent.key)
        XCTAssertTrue(store.peek(parent.key) === parent, "Same-object nested shrink must restore admission at the exact budget")
        XCTAssertEqual(store.unmountedRetainedBytes, baseCharge)
        store.pin(parent.key)
        nestedStore.insert(shapedChild)
        store.insert(parent)
        store.unpin(parent.key)
        XCTAssertNil(store.peek(parent.key), "The grown layout must be evicted when its final pin is released")
        XCTAssertEqual(store.unmountedRetainedBytes, 0)
    }

    func testSurfaceRetainedBytesTracksReplacementEvictionAndPinning() {
        let cellBytes = 100
        let store = TableCellLayoutStore(byteBudget: (cellBytes + cachedKeyBytes) * 3, capacity: 3)
        let names = ["first", "second", "third"]
        let cells = names.enumerated().map {
            TableGridCell(sourceIndex: $0.offset, row: 0, column: $0.offset, contentKey: $0.element)
        }
        let record = TableGridRecord(documentOwner: "live-charge", columns: names.count, rows: 1,
                                     columnWidths: names.map { _ in 100 }, cells: cells)
        var preparations = 0
        let surface = ViewerTableSurface(identity: "live-charge", record: record, viewportWidth: 300,
            style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { cell, _ in
                preparations += 1
                return self.layout(cell.contentKey, bytes: cellBytes)
            }
        let metadata = surface.metadataRetainedBytes
        XCTAssertEqual(surface.retainedBytes, metadata + cellBytes * 3 + cachedKeyBytes * 3)
        XCTAssertEqual(surface.retainedBytes, metadata + cellBytes * 3 + cachedKeyBytes * 3)
        store.insert(layout(names[0], bytes: cellBytes + cellBytes / 2))
        XCTAssertEqual(surface.retainedBytes, metadata + cellBytes * 2 + cellBytes / 2 + cachedKeyBytes * 2,
            "Replacing the same key changes its charge and evicts the least-recent sibling")
        let second = layout(names[1], bytes: cellBytes)
        store.pin(second.key)
        store.insert(second)
        XCTAssertEqual(surface.retainedBytes, metadata + cellBytes * 3 + cellBytes / 2 + cachedKeyBytes * 3)
        store.insert(layout("unmapped-first", bytes: cellBytes * 2))
        XCTAssertEqual(surface.retainedBytes, metadata + cellBytes + cachedKeyBytes * 2,
            "Only the pinned mapped cell remains resident")
        store.unpin(second.key)
        store.insert(layout("unmapped-second", bytes: cellBytes * 2))
        XCTAssertEqual(surface.retainedBytes, metadata + cachedKeyBytes,
            "Unpin-triggered eligibility and later eviction invalidate the old resident charge")
        XCTAssertEqual(preparations, names.count, "Reading memory charges must never rebuild evicted content")
    }

    func testSurfaceMemoryChargePreservesDuplicateKeysAndSeparateStores() {
        let cellBytes = 100
        let firstStore = TableCellLayoutStore()
        let secondStore = TableCellLayoutStore()
        let content = layout("shared", bytes: cellBytes)
        let cells = [firstStore, firstStore, secondStore].enumerated().map { index, store in
            PreparedViewerTableCell(sourceIndex: index, row: 0, column: index, rowspan: 1, colspan: 1,
                contentOrigin: .zero, content: content, isHeader: false, attributesKey: nil, layoutStore: store)
        }
        let record = TableGridRecord(documentOwner: "mixed-stores", columns: cells.count, rows: 1,
            columnWidths: cells.map { _ in 100 }, cells: cells.map {
                TableGridCell(sourceIndex: $0.sourceIndex, row: $0.row, column: $0.column, contentKey: "shared")
            })
        let grid = TableGridLayout(displayScale: 1).layout(record: record, viewportWidth: 300,
            style: TableStyle(), direction: .leftToRight) { _, _ in content.size.height }
        let surface = ViewerTableSurface(identity: "mixed-stores", hostViewportWidth: 300,
            style: TableStyle(), direction: .leftToRight, layout: grid, cells: cells, preparationError: nil)
        XCTAssertEqual(surface.retainedBytes, surface.metadataRetainedBytes + cellBytes * cells.count + cachedKeyBytes * 2)
        secondStore.insert(layout("shared", bytes: cellBytes * 2))
        XCTAssertEqual(surface.retainedBytes, surface.metadataRetainedBytes + cellBytes * 4 + cachedKeyBytes * 2,
            "A surface assembled from multiple stores must observe changes in each store")
        let shared = ViewerTableSurface(identity: "one-store", hostViewportWidth: 300,
            style: TableStyle(), direction: .leftToRight, layout: grid, cells: Array(cells.prefix(2)), preparationError: nil)
        DispatchQueue.concurrentPerform(iterations: 64) { _ in
            XCTAssertEqual(shared.retainedBytes, shared.metadataRetainedBytes + cellBytes * 2 + cachedKeyBytes,
                "Concurrent readers preserve the original per-cell multiplicity for a shared key")
        }
        firstStore.insert(layout("shared", bytes: cellBytes * 2))
        XCTAssertEqual(shared.retainedBytes, shared.metadataRetainedBytes + cellBytes * 4 + cachedKeyBytes)
        let parentBytes = 64
        let parent = PreparedProseLayout(key: layout("mixed-parent").key, size: surface.bounds.size,
            blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface)],
            retainedBytes: parentBytes + surface.retainedBytes)
        XCTAssertEqual(parent.currentRetainedBytes,
            parentBytes + surface.metadataRetainedBytes + cellBytes * 4 + MemoryLayout<Int>.stride * 2,
            "A mixed-store parent must count each resident layout and cached lookup key once")
    }

    func testCurrentParentMemoryFollowsCellEvictionAndRebuild() {
        let cellBytes = 100
        let parentBytes = 64
        let store = TableCellLayoutStore(byteBudget: cellBytes + cachedKeyBytes, capacity: 1)
        let record = TableGridRecord(documentOwner: "memory", columns: 1, rows: 1, columnWidths: [100],
            cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: "cell")])
        let surface = ViewerTableSurface(identity: "memory", record: record, viewportWidth: 100,
            style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store,
            transientCellIndices: [0]) { _, _ in
                self.layout("cell", bytes: cellBytes)
            }
        XCTAssertEqual(store.count, 1, "A generic callback cannot certify independent rebuilding; keep its content charged")
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
            parentBytes + first.metadataRetainedBytes + second.metadataRetainedBytes + cellBytes + cachedKeyBytes,
            "Aliased surfaces and a shared store must not multiply retained cell ownership")
        store.insert(layout("larger-unmapped", bytes: cellBytes * 2))
        XCTAssertEqual(parent.currentRetainedBytes,
            parentBytes + first.metadataRetainedBytes + second.metadataRetainedBytes + cellBytes * 2 + cachedKeyBytes,
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
        withExtendedLifetime(layouts) {
            let context = catalog.newBuildContext(reusing: layouts)
            defer { context.close() }
            XCTAssertLessThanOrEqual(catalog.prunePassesForTesting, 1,
                "Seeding multiple resident tables must not scan the catalog once per excess shape")
            XCTAssertEqual(catalog.countForTesting, capacity,
                "The build context seeds at most the existing resident-layout capacity")
        }
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

    func testSeededContextsShareLiveWinnerAndReleasePinsIndependently() {
        let catalog = PreparedCellShapeCatalog()
        let local = layout("seed-winner")
        let key = PreparedCellShapeKey(contentKey: "same-content", widthPixels: local.key.widthPixels,
            scaleBits: local.key.displayScaleBits, styleDigest: "store-test",
            atomGeometryDigest: "", imageGeometryDigest: "")
        var winner: PreparedCellShape? = PreparedCellShape(key: key, localLayout: local)
        weak var releasedWinner = winner
        var owner: PreparedProseLayout? = local.withCellShape(winner!)
        let first = catalog.newBuildContext(reusing: [owner!])
        let competingOwner = local.withCellShape(PreparedCellShape(key: key, localLayout: local))
        let second = catalog.newBuildContext(reusing: [competingOwner])
        if case let .cached(selected) = second.beginResolution(key) {
            XCTAssertTrue(selected === winner, "Seeding must preserve the catalog's live same-key winner")
        } else { XCTFail("The second context must reuse the already resident shape") }
        first.close()
        XCTAssertEqual(catalog.countForTesting, 1, "Closing one context must preserve the other's pin")
        if case let .cached(selected) = second.beginResolution(key) {
            XCTAssertTrue(selected === winner)
        } else { XCTFail("The remaining context lost its reusable shape") }
        owner = nil
        winner = nil
        XCTAssertNil(releasedWinner, "Open contexts and catalog pins must remain weak")
        XCTAssertEqual(catalog.countForTesting, 0, "Releasing the actual owner removes the weak entry")
        second.close()
        second.close()
        withExtendedLifetime(competingOwner) {}
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
        for budget in [PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget, 0] {
            let catalog = PreparedCellShapeCatalog()
            let context = catalog.newBuildContext()
            defer { context.close() }
            var rebuilds = 0
            weak var sourceStore: TableCellLayoutStore?
            var shape: PreparedCellShape!
            try autoreleasepool {
                let store = TableCellLayoutStore(byteBudget: budget)
                sourceStore = store
                let record = TableGridRecord(documentOwner: "nested-source", columns: 1, rows: 1, columnWidths: [100],
                    cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: "nested-cell")])
                let surface = ViewerTableSurface(identity: "nested-source", record: record, viewportWidth: 100,
                    style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { _, _ in
                        self.layout("nested-cell").withCellShape(nil, preparation: {
                            rebuilds += 1
                            return self.layout("nested-cell")
                        })
                    }
                let parent = PreparedProseLayout(key: layout("nested-parent").key, size: surface.bounds.size,
                    blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface)],
                    retainedBytes: surface.retainedBytes)
                let key = PreparedCellShapeKey(contentKey: "nested-parent", widthPixels: 100,
                    scaleBits: Double(1).bitPattern, styleDigest: "store-test", atomGeometryDigest: "", imageGeometryDigest: "")
                shape = try context.resolve(key, build: { parent }, bind: { _ in nil }).cellShape
            }
            XCTAssertEqual(rebuilds, 0, "neutralizing a parent must not materialize an evicted child")
            XCTAssertNil(sourceStore, "A neutral nested shape must not keep the source cell's shared store alive")
            let neutralTable = try XCTUnwrap(shape.localLayout.blocks.first?.tableSurface)
            let expectedSize = neutralTable.cells[0].contentSize
            neutralTable.layoutStore.insert(layout("evict-nested", bytes: PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget))
            XCTAssertNil(neutralTable.cells[0].cachedContent)
            let rebuilt = neutralTable.cells[0].content
            XCTAssertEqual(rebuilt.size, expectedSize,
                "Nested neutral content must still reconstruct after its own store evicts it")
            XCTAssertEqual(rebuilt.key, neutralTable.cells[0].contentKey)
            XCTAssertEqual(rebuilds, 1, "the neutral child rebuilds only when requested")
            XCTAssertTrue(neutralTable.cells[0].content === rebuilt,
                "A second access must reuse the reconstructed neutral layout")
            XCTAssertEqual(rebuilds, 1, "the reconstructed neutral key must hit the cache")
        }
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
        let store = TableCellLayoutStore(byteBudget: prepared.retainedBytes + cachedKeyBytes)
        store.insert(prepared)
        XCTAssertNil(store.peek(prepared.key), "The shape graph must also fit the unmounted budget")
    }

    func testPresentationPinsOnlyItsCurrentWindow() {
        let store = TableCellLayoutStore(byteBudget: 100 + cachedKeyBytes, capacity: 1)
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
        XCTAssertLessThanOrEqual(store.unmountedRetainedBytes, 100 + cachedKeyBytes)
    }

    func testProjectionUsesOneContentIdentityWhenCellExceedsUnmountedBudget() throws {
        let cellName = "oversized-projected-cell"
        let store = TableCellLayoutStore(byteBudget: 0)
        let record = TableGridRecord(documentOwner: "projection-identity", columns: 1, rows: 1,
            columnWidths: [100], cells: [TableGridCell(sourceIndex: 0, row: 0, column: 0, contentKey: cellName)])
        var preparations = 0
        let surface = ViewerTableSurface(identity: "projection-identity", record: record, viewportWidth: 100,
            style: TableStyle(), direction: .leftToRight, displayScale: 1, layoutStore: store) { _, _ in
                preparations += 1
                return self.layout(cellName)
            }
        let parent = PreparedProseLayout(key: layout("parent").key, size: surface.bounds.size,
            blocks: [PreparedProseBlock(fragments: [], bounds: surface.bounds, tableSurface: surface,
                                       tableBounds: surface.bounds)], retainedBytes: 100)
        preparations = 0
        let owner = ViewerTablePresentationOwner()
        let snapshot = ViewerTablePresentation.project(layout: parent, owner: owner, viewport: .known(surface.bounds))
        let cell = try XCTUnwrap(snapshot.cells.first)
        let content = try XCTUnwrap(snapshot.layouts.first { $0.layout.key.semanticKey == cellName })
        XCTAssertEqual(preparations, 1, "A projected cell must resolve its oversized content only once")
        XCTAssertTrue(cell.content === content.layout,
            "Cell painting joins projected layouts by identity, even when the unmounted cache cannot retain them")
        _ = ViewerTablePresentation.project(layout: parent, owner: owner, viewport: .known(.zero))
        XCTAssertEqual(store.unmountedRetainedBytes, 0, "Projection must preserve the unmounted memory budget")
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
