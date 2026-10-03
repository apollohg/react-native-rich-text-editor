import UIKit
import XCTest

final class EditorTableIndexTests: XCTestCase {
    private let rootKey = "root"
    private let attributeKey = "plain"
    private let revision: UInt64 = 1
    private let rootDocStart: UInt32 = 10
    private let rootScalarStart: UInt32 = 7

    func testVoidElementMetadataRejectsNonAtomsAndOutOfRangeIndicesAtomically() throws {
        let index = EditorTableIndex()
        _ = index.adopt(frame(), installedRevision: nil, frameRevision: revision)
        let original = index.record(tableKey: rootKey)
        for indices in [[UInt32(0)], [3], [1, 1]] {
            var corrupt = frame()
            if indices.count > 1 {
                corrupt.tables[0].cells[0].elements[1] = .inlineAtom(nodeType: "mention", docPos: 2, attrsJson: "{}", label: "a")
            }
            corrupt.tables[0].cells[0].voidElementIndices = indices
            guard case .failure(.inputBlockOutOfStride) = index.adopt(corrupt, installedRevision: revision, frameRevision: revision) else {
                return XCTFail("invalid void metadata was adopted: \(indices)")
            }
            XCTAssertEqual(index.record(tableKey: rootKey), original)
        }
    }

    private func cell(_ column: UInt32, stride: UInt32) -> FfiTableCellRecord {
        FfiTableCellRecord(
            sourceRow: 0,
            row: 0,
            column: column,
            rowspan: 1,
            colspan: 1,
            header: false,
            attrsKey: attributeKey,
            contentKey: "cell-\(column)",
            docSize: 5,
            scalarStride: stride,
            elements: [
                .blockStart(nodeType: "paragraph", language: nil, depth: 0, listContextJson: nil),
                .textRun(text: "a", marks: []),
                .blockEnd
            ],
            voidElementIndices: [],
            inputBlocks: [FfiCellInputBlock(
                elementIndex: 0,
                docStart: 2,
                docEnd: 3,
                scalarStart: 0,
                contentScalarStart: 0,
                scalarEnd: 1,
                breakScalarEnd: stride,
                void: false
            )],
            nestedTables: []
        )
    }

    private func frame() -> FfiTableFrame {
        let cells = [cell(0, stride: 2), cell(1, stride: 1)]
        let table = FfiTableRecord(
            tableKey: rootKey,
            host: nil,
            docSize: 14,
            rows: 1,
            columns: 2,
            columnWidths: [nil, nil],
            direction: nil,
            irregular: false,
            readOnlyDescendants: false,
            attrsKey: attributeKey,
            sourceRows: [.init(attrsKey: attributeKey, cellCount: 2)],
            cells: cells,
            syntheticRegions: [],
            failure: nil,
            compatibilityDiagnostic: nil
        )
        return FfiTableFrame(
            kind: .full,
            baseDocumentRevision: nil,
            attributes: [.init(key: attributeKey, json: "{}")],
            removedAttributeKeys: [],
            tables: [table],
            removedTableKeys: [],
            cellUpdates: [],
            extents: [.init(
                tableKey: rootKey,
                docStart: rootDocStart,
                docSize: table.docSize,
                scalarStart: rootScalarStart,
                scalarEnd: rootScalarStart + 3
            )]
        )
    }

    private func delta() -> FfiTableFrame {
        FfiTableFrame(
            kind: .delta,
            baseDocumentRevision: String(revision),
            attributes: [],
            removedAttributeKeys: [],
            tables: [],
            removedTableKeys: [],
            cellUpdates: [],
            extents: frame().extents
        )
    }

    func testIncrementalCellsReleaseHistoricalIndexesAndRebuildAfterEviction() throws {
        final class IndexReference {
            weak var value: EditorTableIndex?
            init(_ value: EditorTableIndex) { self.value = value }
        }
        let cellCount = 128
        let width: CGFloat = 400
        var full = frame()
        var table = full.tables[0]
        table.columns = UInt32(cellCount)
        table.columnWidths = Array(repeating: nil, count: cellCount)
        table.cells = (0..<cellCount).map { cell(UInt32($0), stride: $0 == cellCount - 1 ? 1 : 2) }
        table.sourceRows[0].cellCount = UInt32(cellCount)
        table.docSize = UInt32(cellCount * 5 + 4)
        full.tables = [table]
        full.extents[0].docSize = table.docSize
        full.extents[0].scalarEnd = rootScalarStart + UInt32(cellCount * 2 - 1)
        var references: [IndexReference] = []
        var retained: PreparedProseLayout!
        try autoreleasepool {
            var index = EditorTableIndex()
            _ = try index.adopt(full, installedRevision: nil, frameRevision: revision).get()
            let engine = CoreTextProseLayoutEngine()
            engine.tablePreparationWorkerLimit = 1
            for step in 0...cellCount {
                if step > 0 {
                    index = index.copy()
                    var change = delta()
                    change.baseDocumentRevision = String(revision + UInt64(step - 1))
                    change.extents = full.extents
                    var changed = table.cells[step - 1]
                    changed.contentKey = "edited-\(step)"
                    changed.elements[1] = .textRun(text: "b", marks: [])
                    change.cellUpdates = [.init(tableKey: rootKey, cellIndex: UInt32(step - 1), cell: changed)]
                    _ = try index.adopt(
                        change,
                        installedRevision: revision + UInt64(step - 1),
                        frameRevision: revision + UInt64(step)
                    ).get()
                    let previous = try XCTUnwrap(retained.blocks.first?.tableSurface)
                    engine.incrementalTableSurface = { _ in (previous, IndexSet(integer: step - 1)) }
                }
                let record = try XCTUnwrap(index.record(tableKey: rootKey))
                let document = ViewerDocument(
                    semanticKey: "revision-\(step)",
                    blocks: [ViewerBlock(
                        nodeType: "table",
                        depth: 0,
                        inBlockquote: false,
                        listContext: nil,
                        listItemBoundary: nil,
                        inlines: [],
                        frameTable: record
                    )],
                    isEmpty: false,
                    retainedBytes: 0,
                    tableAttributes: index.attributeObjects,
                    frameIndex: index
                )
                let key = ProseLayoutKey(
                    semanticKey: document.semanticKey,
                    widthPixels: Int(width),
                    themeDigest: "retention",
                    nativeFontRevision: 0,
                    fontEnvironmentRevision: 0,
                    displayScale: 1,
                    attachmentRevision: 0,
                    generationIdentity: "retention",
                    semanticGenerationIdentity: "retention"
                )
                retained = try engine.prepare(document: document, key: key, widthPoints: width, displayScale: 1)
                engine.incrementalTableSurface = nil
                references.append(IndexReference(index))
            }
        }
        XCTAssertEqual(
            references.filter { $0.value != nil }.count,
            0,
            "Cell rebuild closures must not retain any whole historical frame index"
        )
        let surface = try XCTUnwrap(retained.blocks.first?.tableSurface)
        let eviction = PreparedProseLayout(
            key: retained.key,
            size: .zero,
            blocks: [],
            retainedBytes: PreparedProseLayoutCache.preparedLayoutUnmountedByteBudget
        )
        surface.layoutStore.insert(eviction)
        for cell in surface.cells {
            XCTAssertNil(cell.cachedContent)
            XCTAssertNil(cell.content.error, "Every evicted cell must rebuild from its own immutable input")
            XCTAssertEqual(cell.content.accessibilityNodes.map(\.label).joined(), "b")
        }
    }

    func testFrameCellDocumentResolvesRelativeAtomPosition() throws {
        let config = TableInputTestSchema.tableConfig.replacingOccurrences(
            of: #"{"name":"text""#,
            with: #"{"name":"horizontal_rule","content":"","group":"block","role":"block","isVoid":true},{"name":"text""#
        )
        let editorId = makeV2Editor(configJson: config)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let source = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"horizontal_rule"},{"type":"paragraph"}]}]}]}]}"#
        XCTAssertNotNil(adapter.setContentJson(source))
        let key = try XCTUnwrap(adapter.tableIndex.tableKeys.first)
        let record = try XCTUnwrap(adapter.tableIndex.record(tableKey: key))
        let cell = try XCTUnwrap(TableSurfaceSource(frameRecord: record).cells.first)
        let doc = ViewerDocument(
            semanticKey: "frame-atom",
            blocks: [],
            isEmpty: false,
            retainedBytes: 0,
            frameIndex: adapter.tableIndex
        )
        let child = try doc.cellDocument(for: cell, in: key)
        guard case let .atom(_, actual, _, _) = child.blocks.first?.inlines.first else {
            return XCTFail("the real horizontal rule must remain an atom in the cell document")
        }
        guard case let .blockAtom(_, relative, _, _) = cell.elements.first else {
            return XCTFail("the frame must expose the cell-relative atom coordinate")
        }
        XCTAssertEqual(actual, adapter.tableIndex.absoluteDocPos(tableKey: key, cellIndex: 0, relative: relative))
    }

    func testEngineFramePositionsMatchEngineScalarConversions() throws {
        let editorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let result = editorV2RenderNativeFrame(
            editorId: adapter.editorId,
            ownerId: nil,
            mirrorScalarAnchor: nil,
            mirrorScalarHead: nil
        )
        XCTAssertNil(result.error)
        let native = try XCTUnwrap(result.frame)
        let table = try XCTUnwrap(native.tables.tables.first)
        let index = EditorTableIndex()
        _ = try index.adopt(native.tables, installedRevision: nil, frameRevision: adapter.baseDocumentRevision).get()
        let expectedCellStarts: [UInt32] = [2, 9]
        let cellTexts = ["one", "two"]
        let cellContentOffset: UInt32 = 2
        XCTAssertEqual(table.cells.count, expectedCellStarts.count)
        for (cellIndex, docStart) in expectedCellStarts.enumerated() {
            XCTAssertEqual(index.docStart(tableKey: table.tableKey, cellIndex: cellIndex), docStart)
            let contentStart = docStart + cellContentOffset
            let scalarStart = EditorV2Shadow.docToScalar(id: editorId, docPos: contentStart)
            let scalarEnd = EditorV2Shadow.docToScalar(id: editorId, docPos: contentStart + UInt32(cellTexts[cellIndex].unicodeScalars.count))
            XCTAssertEqual(index.scalarStart(tableKey: table.tableKey, cellIndex: cellIndex), scalarStart)
            let segments = try XCTUnwrap(index.inputSegments(tableKey: table.tableKey, cellIndex: cellIndex))
            XCTAssertEqual(segments.count, 1)
            let segment = try XCTUnwrap(segments.first)
            XCTAssertEqual(segment.globalScalarStart, scalarStart)
            XCTAssertEqual(segment.localScalarRange.count, Int(scalarEnd - scalarStart + 1))
            XCTAssertEqual(
                index.cellIndex(tableKey: table.tableKey, containingScalar: scalarEnd),
                cellIndex,
                "the terminal caret of each real input block belongs to its cell"
            )
        }
    }

    func testNestedInputSegmentsMatchRenderedCollapsedMarkers() throws {
        let source = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"nested text"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}]}]}]}"#
        let editorId = makeV2Editor(configJson: TableInputTestSchema.tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        XCTAssertNotNil(adapter.setContentJson(source))
        let result = editorV2RenderNativeFrame(
            editorId: adapter.editorId,
            ownerId: nil,
            mirrorScalarAnchor: nil,
            mirrorScalarHead: nil
        )
        XCTAssertNil(result.error)
        let native = try XCTUnwrap(result.frame)
        let root = try XCTUnwrap(native.tables.tables.first { $0.host == nil })
        let index = EditorTableIndex()
        _ = try index.adopt(native.tables, installedRevision: nil, frameRevision: adapter.baseDocumentRevision).get()
        let projection = try XCTUnwrap(EditorTableInputCoordinator.projection(
            cellIndex: 0, tableKey: root.tableKey, index: index,
            documentRevision: adapter.baseDocumentRevision, positionEpoch: adapter.positionEpoch ?? 1,
            baseFont: .systemFont(ofSize: 17), textColor: .black, theme: nil, atomConfiguration: nil
        ))
        XCTAssertEqual(projection.positionMap.segments.map(\.localScalarRange), [0..<7, 9..<15])
        XCTAssertEqual(
            index.inputSegments(tableKey: root.tableKey, cellIndex: 0),
            projection.positionMap.segments,
            "nested scalar widths must collapse to the one rendered marker before later input blocks"
        )
    }

    func testOneDeltaCanExchangeNestedTablesBetweenCells() throws {
        var full = frame()
        var children: [FfiTableRecord] = []
        for cellIndex in full.tables[0].cells.indices {
            var child = frame().tables[0]
            child.tableKey = "child-\(cellIndex)"
            child.host = .init(tableKey: rootKey, cellIndex: UInt32(cellIndex))
            children.append(child)
            full.tables[0].cells[cellIndex].docSize = child.docSize + 2
            full.tables[0].cells[cellIndex].scalarStride = cellIndex == 0 ? 4 : 3
            full.tables[0].cells[cellIndex].elements = [.table(tableId: child.tableKey)]
            full.tables[0].cells[cellIndex].inputBlocks = []
            full.tables[0].cells[cellIndex].nestedTables = [.init(
                elementIndex: 0,
                tableKey: child.tableKey,
                docOffset: 1,
                docSize: child.docSize,
                scalarStart: 0,
                scalarEnd: 3
            )]
        }
        full.tables[0].docSize = 36
        full.extents[0].docSize = 36
        full.extents[0].scalarEnd = rootScalarStart + 7
        full.tables += children
        let index = EditorTableIndex()
        _ = try index.adopt(full, installedRevision: nil, frameRevision: revision).get()
        var update = delta()
        update.extents = full.extents
        for cellIndex in full.tables[0].cells.indices {
            let other = 1 - cellIndex
            var cell = full.tables[0].cells[cellIndex]
            cell.elements = full.tables[0].cells[other].elements
            cell.nestedTables = full.tables[0].cells[other].nestedTables
            update.cellUpdates.append(.init(tableKey: rootKey, cellIndex: UInt32(cellIndex), cell: cell))
            children[other].host = .init(tableKey: rootKey, cellIndex: UInt32(cellIndex))
        }
        update.tables = children
        _ = try index.adopt(update, installedRevision: revision, frameRevision: revision + 1).get()
        XCTAssertEqual(index.docStart(tableKey: children[1].tableKey, cellIndex: 0), rootDocStart + 5)
        XCTAssertEqual(index.docStart(tableKey: children[0].tableKey, cellIndex: 0), rootDocStart + 21)
    }

    func testFullAdoptionBuildsPositionsAndInputSegments() throws {
        let index = EditorTableIndex()
        let full = frame()
        let changes = try index.adopt(full, installedRevision: nil, frameRevision: revision).get()
        XCTAssertTrue(changes.fullReset)
        XCTAssertEqual(changes.replacedTables, [rootKey])
        XCTAssertEqual(index.record(tableKey: rootKey), full.tables[0])
        XCTAssertEqual(index.docStart(tableKey: rootKey, cellIndex: 0), rootDocStart + 2)
        XCTAssertEqual(index.docStart(tableKey: rootKey, cellIndex: 1), rootDocStart + 7)
        XCTAssertEqual(index.scalarStart(tableKey: rootKey, cellIndex: 1), rootScalarStart + 2)
        XCTAssertEqual(index.cellIndex(tableKey: rootKey, containingDoc: rootDocStart + 8), 1)
        XCTAssertEqual(index.cellIndex(tableKey: rootKey, containingScalar: rootScalarStart + 2), 1)
        XCTAssertNil(index.cellIndex(tableKey: rootKey, containingDoc: rootDocStart))
        XCTAssertEqual(index.cellIndex(tableKey: rootKey, containingScalar: rootScalarStart + 3), 1)
        XCTAssertNil(index.cellIndex(tableKey: rootKey, containingScalar: rootScalarStart + 4))
        XCTAssertEqual(index.tableKey(containingDoc: rootDocStart + 8), rootKey)
        XCTAssertEqual(index.tableKey(containingScalar: rootScalarStart + 1), rootKey)
        XCTAssertEqual(index.absoluteDocPos(tableKey: rootKey, cellIndex: 1, relative: 2), rootDocStart + 9)
        XCTAssertEqual(index.inputSegments(tableKey: rootKey, cellIndex: 0), [
            .init(localScalarRange: 0..<2, globalScalarStart: rootScalarStart)
        ])
        XCTAssertNil(index.docStart(tableKey: rootKey, cellIndex: -1))
        XCTAssertNil(index.absoluteDocPos(tableKey: rootKey, cellIndex: 0, relative: 6))
    }

    func testOneCellDeltaRecomputesFollowingPrefixes() throws {
        let index = EditorTableIndex()
        let original = frame()
        _ = try index.adopt(original, installedRevision: nil, frameRevision: revision).get()
        var update = delta()
        var changed = original.tables[0].cells[0]
        let growth: UInt32 = 3
        changed.docSize += growth
        changed.scalarStride += growth
        changed.contentKey = "changed"
        changed.elements[1] = .textRun(text: "aaaa", marks: [])
        changed.inputBlocks[0].docEnd += growth
        changed.inputBlocks[0].scalarEnd += growth
        changed.inputBlocks[0].breakScalarEnd += growth
        update.cellUpdates = [.init(tableKey: rootKey, cellIndex: 0, cell: changed)]
        update.extents[0].docSize += growth
        update.extents[0].scalarEnd += growth
        let changes = try index.adopt(update, installedRevision: revision, frameRevision: revision + 1).get()
        XCTAssertFalse(changes.fullReset)
        XCTAssertTrue(changes.replacedTables.isEmpty)
        XCTAssertEqual(changes.changedCells, [rootKey: IndexSet(integer: 0)])
        XCTAssertEqual(index.docStart(tableKey: rootKey, cellIndex: 1), rootDocStart + 7 + growth)
        XCTAssertEqual(index.scalarStart(tableKey: rootKey, cellIndex: 1), rootScalarStart + 2 + growth)
        XCTAssertEqual(index.record(tableKey: rootKey)?.cells[1], original.tables[0].cells[1])
    }

    func testEveryRejectionLeavesInstalledRecordsAndPositionsUnchanged() throws {
        let original = frame()
        struct RejectionCase {
            let label: String
            let candidate: FfiTableFrame
            let expected: TableFrameRejection
        }
        var cases: [RejectionCase] = []
        var update = delta()
        update.baseDocumentRevision = "0"
        cases.append(RejectionCase(label: "base", candidate: update, expected: .baseRevisionMismatch(expected: revision, actual: 0)))
        update = delta(); update.removedTableKeys = ["missing"]
        cases.append(RejectionCase(label: "unknown table", candidate: update, expected: .unknownTable("missing")))
        update = delta(); update.cellUpdates = [.init(tableKey: rootKey, cellIndex: 2, cell: original.tables[0].cells[0])]
        cases.append(RejectionCase(label: "cell index", candidate: update, expected: .cellIndexOutOfRange(rootKey, 2)))
        var changed = original.tables[0].cells[0]; changed.header = true
        update = delta(); update.cellUpdates = [.init(tableKey: rootKey, cellIndex: 0, cell: changed)]
        cases.append(RejectionCase(label: "structural cell update", candidate: update, expected: .cellStructureChanged(rootKey, 0)))
        update = delta(); update.extents[0].docSize += 1
        cases.append(RejectionCase(label: "doc size", candidate: update, expected: .docSizeMismatch(rootKey, expected: 14, actual: 15)))
        update = delta(); update.extents[0].scalarEnd += 1
        cases.append(RejectionCase(label: "scalar size", candidate: update, expected: .scalarSizeMismatch(rootKey, expected: 3, actual: 4)))
        changed = original.tables[0].cells[0]; changed.inputBlocks[0].breakScalarEnd = changed.scalarStride + 1
        update = delta(); update.cellUpdates = [.init(tableKey: rootKey, cellIndex: 0, cell: changed)]
        cases.append(RejectionCase(label: "input stride", candidate: update, expected: .inputBlockOutOfStride(rootKey, 0)))
        update = delta(); update.removedAttributeKeys = [attributeKey]
        cases.append(RejectionCase(label: "attribute", candidate: update, expected: .missingAttribute(attributeKey)))
        update = delta(); update.tables = [original.tables[0], original.tables[0]]
        cases.append(RejectionCase(label: "duplicate table", candidate: update, expected: .duplicateTableKey(rootKey)))
        update = delta(); var orphan = original.tables[0]; orphan.host = .init(tableKey: "missing", cellIndex: 0)
        update.tables = [orphan]; update.extents = []
        cases.append(RejectionCase(label: "host", candidate: update, expected: .hostMissing(rootKey)))
        update = delta(); update.extents = []
        cases.append(RejectionCase(label: "extents", candidate: update, expected: .extentsIncomplete))
        let index = EditorTableIndex()
        _ = try index.adopt(original, installedRevision: nil, frameRevision: revision).get()
        for item in cases {
            let label = item.label
            let candidate = item.candidate
            let expected = item.expected
            let result = index.adopt(candidate, installedRevision: revision, frameRevision: revision + 1)
            guard case let .failure(actual) = result else { return XCTFail("accepted invalid \(label): \(result)") }
            XCTAssertEqual(actual, expected, label)
            XCTAssertEqual(index.record(tableKey: rootKey), original.tables[0], "\(label) mutated records")
            XCTAssertEqual(index.docStart(tableKey: rootKey, cellIndex: 1), rootDocStart + 7, "\(label) mutated doc prefix")
            XCTAssertEqual(index.scalarStart(tableKey: rootKey, cellIndex: 1), rootScalarStart + 2, "\(label) mutated scalar prefix")
        }
    }

    func testNestedPositionsUseHostCellAndRelativeExclusion() throws {
        var full = frame()
        let childKey = "child"
        var child = full.tables[0]
        child.tableKey = childKey
        child.host = .init(tableKey: rootKey, cellIndex: 0)
        child.readOnlyDescendants = true
        var parent = full.tables[0]
        parent.cells[0].docSize = child.docSize + 2
        parent.cells[0].scalarStride = 4
        parent.cells[0].elements = [.table(tableId: childKey)]
        parent.cells[0].inputBlocks = []
        parent.cells[0].nestedTables = [.init(
            elementIndex: 0,
            tableKey: childKey,
            docOffset: 1,
            docSize: child.docSize,
            scalarStart: 0,
            scalarEnd: 3
        )]
        parent.docSize = 25
        full.tables = [child, parent]
        full.extents[0].docSize = parent.docSize
        full.extents[0].scalarEnd = rootScalarStart + 5
        let index = EditorTableIndex()
        _ = try index.adopt(full, installedRevision: nil, frameRevision: revision).get()
        XCTAssertEqual(index.docStart(tableKey: childKey, cellIndex: 0), rootDocStart + 5)
        XCTAssertEqual(index.scalarStart(tableKey: childKey, cellIndex: 1), rootScalarStart + 2)
        XCTAssertEqual(index.tableKey(containingDoc: rootDocStart + 5), childKey)
        XCTAssertEqual(index.tableKey(containingScalar: rootScalarStart + 2), childKey)
        XCTAssertEqual(index.absoluteDocPos(tableKey: childKey, cellIndex: 1, relative: 2), rootDocStart + 12)
        var removal = delta()
        removal.extents = full.extents
        removal.removedTableKeys = [childKey]
        guard case .failure(.unknownTable(childKey)) = index.adopt(removal, installedRevision: revision, frameRevision: revision + 1) else {
            return XCTFail("removing a nested table that the installed parent still references must fail")
        }
        XCTAssertEqual(index.record(tableKey: childKey), child)
        XCTAssertEqual(index.record(tableKey: rootKey), parent)
        XCTAssertEqual(index.docStart(tableKey: childKey, cellIndex: 0), rootDocStart + 5)
    }

    func testSameRevisionEmptyDeltaPreservesExtentsAndFullResetRemovesTables() throws {
        let index = EditorTableIndex()
        let original = frame()
        _ = try index.adopt(original, installedRevision: nil, frameRevision: revision).get()
        var unchanged = delta()
        unchanged.extents = []
        let changes = try index.adopt(unchanged, installedRevision: revision, frameRevision: revision).get()
        XCTAssertEqual(changes, TableFrameChanges(fullReset: false, replacedTables: [], removedTables: [], changedCells: [:]))
        XCTAssertEqual(index.tableKey(containingScalar: rootScalarStart), rootKey)
        XCTAssertEqual(index.docStart(tableKey: rootKey, cellIndex: 1), rootDocStart + 7)
        var empty = original
        empty.tables = []
        empty.extents = []
        empty.attributes = []
        _ = try index.adopt(empty, installedRevision: revision, frameRevision: revision + 1).get()
        XCTAssertNil(index.record(tableKey: rootKey))
        XCTAssertNil(index.tableKey(containingScalar: rootScalarStart))
    }

    func testFailedTableAcceptsAnEmptyExtent() throws {
        var full = frame()
        full.tables[0].failure = .gridLimit
        full.tables[0].cells = []
        full.tables[0].sourceRows = []
        full.extents[0].scalarEnd = full.extents[0].scalarStart
        let index = EditorTableIndex()
        _ = try index.adopt(full, installedRevision: nil, frameRevision: revision).get()
        XCTAssertEqual(index.record(tableKey: rootKey), full.tables[0])
        XCTAssertNil(index.docStart(tableKey: rootKey, cellIndex: 0))
    }

    func testEmptySourceRowsContributeDocumentBoundaries() throws {
        var full = frame()
        full.tables[0].sourceRows = [
            .init(attrsKey: attributeKey, cellCount: 0), .init(attrsKey: attributeKey, cellCount: 2),
            .init(attrsKey: attributeKey, cellCount: 0)
        ]
        full.tables[0].rows = 3
        for cell in full.tables[0].cells.indices {
            full.tables[0].cells[cell].sourceRow = 1
            full.tables[0].cells[cell].row = 1
        }
        full.tables[0].docSize += 4
        full.extents[0].docSize += 4
        let index = EditorTableIndex()
        _ = try index.adopt(full, installedRevision: nil, frameRevision: revision).get()
        XCTAssertEqual(index.docStart(tableKey: rootKey, cellIndex: 0), rootDocStart + 4)
        XCTAssertEqual(index.docStart(tableKey: rootKey, cellIndex: 1), rootDocStart + 9)
        XCTAssertNil(index.cellIndex(tableKey: rootKey, containingDoc: rootDocStart + 2))
    }
}
