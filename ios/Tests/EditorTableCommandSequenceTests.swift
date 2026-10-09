import XCTest

final class EditorTableCommandSequenceTests: XCTestCase {
    private enum Sequence {
        static let editorSize = CGSize(width: 600, height: 500)
        static let gridSize = 3
        static let middleOffset = 2
        static let originalWidth = 96
        static let resizedWidth = 132
        static let unsetColumnWidth = 0
        static let nodeBoundarySize = 2
        static let nodeContentOffset = 1
        static let cellContentOffset = 2
        static let unitSpan = 1
        static let mergedSpan = 3
        static let geometryAccuracy: CGFloat = 0.5
        static let nativeSelectionSyncTimeout: TimeInterval = 1
        static let before = "Before table"
        static let after = "After table"
        static let probe = "probe🙂"
        static let placeholder = "\u{200B}"
        static let tableClass = "command-sequence-grid"
        static let textSelection = "text"
        static let cellSelection = "cell"
        static let documentNode = "doc"
        static let paragraphNode = "paragraph"
        static let textNode = "text"
        static let tableNode = "table"
        static let rowNode = "table_row"
        static let cellNode = "table_cell"
        static let headerNode = "table_header"
        static let tab = "\t"
    }

    private struct Cell {
        var header = false
        var paragraphs: [String]
        var identity: String?
        var colspan = Sequence.unitSpan
        var rowspan = Sequence.unitSpan
        var widths: [Int]?

        var object: [String: Any] {
            var attrs: [String: Any] = [:]
            if let identity { attrs["class"] = identity }
            if colspan != Sequence.unitSpan { attrs["colspan"] = colspan }
            if rowspan != Sequence.unitSpan { attrs["rowspan"] = rowspan }
            if let widths { attrs["colwidth"] = widths }
            var node: [String: Any] = [
                "type": header ? Sequence.headerNode : Sequence.cellNode,
                "content": paragraphs.map(EditorTableCommandSequenceTests.paragraph)
            ]
            if !attrs.isEmpty { node["attrs"] = attrs }
            return node
        }

        var nodeSize: Int {
            Sequence.nodeBoundarySize + paragraphs.reduce(0) {
                $0 + Sequence.nodeBoundarySize + $1.unicodeScalars.count
            }
        }
    }

    private struct Document {
        var grid: [[Cell]]?
        var before = Sequence.before
        var after = Sequence.after

        init(headerRow: Bool, widths: [Int]? = [Sequence.originalWidth]) {
            grid = (0..<Sequence.gridSize).map { row in
                (0..<Sequence.gridSize).map { column in
                    Cell(header: headerRow && row == 0, paragraphs: ["r\(row)c\(column)"],
                        identity: "cell-\(row)-\(column)", widths: widths)
                }
            }
        }

        var object: [String: Any] {
            var blocks = [EditorTableCommandSequenceTests.paragraph(before)]
            if let grid {
                blocks.append([
                    "type": Sequence.tableNode, "attrs": ["class": Sequence.tableClass],
                    "content": grid.map { ["type": Sequence.rowNode, "content": $0.map(\.object)] }
                ])
            }
            blocks.append(EditorTableCommandSequenceTests.paragraph(after))
            return ["type": Sequence.documentNode, "content": blocks]
        }

        var tableSize: Int {
            guard let grid else { return 0 }
            return Sequence.nodeBoundarySize + grid.reduce(0) {
                $0 + Sequence.nodeBoundarySize + $1.reduce(0) { $0 + $1.nodeSize }
            }
        }

        func cellPosition(row: Int, column: Int) -> UInt32 {
            let grid = grid!
            let precedingRows = grid.prefix(row).reduce(0) {
                $0 + Sequence.nodeBoundarySize + $1.reduce(0) { $0 + $1.nodeSize }
            }
            let precedingCells = grid[row].prefix(column).reduce(0) { $0 + $1.nodeSize }
            return UInt32(before.unicodeScalars.count + Sequence.nodeBoundarySize
                + Sequence.nodeContentOffset + precedingRows + Sequence.nodeContentOffset + precedingCells)
        }

        func position(of caret: Caret) -> UInt32 {
            switch caret {
            case let .before(offset): return UInt32(Sequence.nodeContentOffset + offset)
            case let .cell(row, column, paragraph, offset):
                let preceding = grid![row][column].paragraphs.prefix(paragraph).reduce(0) {
                    $0 + Sequence.nodeBoundarySize + $1.unicodeScalars.count
                }
                return cellPosition(row: row, column: column)
                    + UInt32(Sequence.cellContentOffset + preceding + offset)
            case let .after(offset):
                return UInt32(before.unicodeScalars.count + Sequence.nodeBoundarySize
                    + tableSize + Sequence.nodeContentOffset + offset)
            }
        }

        mutating func insert(_ text: String, at caret: Caret) {
            func inserting(_ original: String, at offset: Int) -> String {
                let scalars = original.unicodeScalars
                return String(scalars.prefix(offset)) + text + String(scalars.dropFirst(offset))
            }
            switch caret {
            case let .before(offset):
                before = inserting(before, at: offset)
            case let .cell(row, column, paragraph, offset):
                grid![row][column].paragraphs[paragraph] = inserting(grid![row][column].paragraphs[paragraph], at: offset)
            case let .after(offset):
                after = inserting(after, at: offset)
            }
        }
    }

    private enum Caret {
        case before(offset: Int)
        case cell(row: Int, column: Int, paragraph: Int = 0, offset: Int)
        case after(offset: Int)

        func advanced(by distance: Int) -> Caret {
            switch self {
            case let .before(offset): return .before(offset: offset + distance)
            case let .cell(row, column, paragraph, offset):
                return .cell(row: row, column: column, paragraph: paragraph, offset: offset + distance)
            case let .after(offset): return .after(offset: offset + distance)
            }
        }
    }

    private enum Command: String {
        case rowBefore, rowAfter, columnBefore, columnAfter, deleteRow, deleteColumn
        case headerCell, headerRow, headerColumn, resize, deleteTable

        var payload: [String: String] {
            switch self {
            case .rowBefore: return ["type": "addTableRow", "side": "before"]
            case .rowAfter: return ["type": "addTableRow", "side": "after"]
            case .columnBefore: return ["type": "addTableColumn", "side": "before"]
            case .columnAfter: return ["type": "addTableColumn", "side": "after"]
            case .deleteRow: return ["type": "deleteTableRows"]
            case .deleteColumn: return ["type": "deleteTableColumns"]
            case .headerCell: return ["type": "toggleTableHeader", "target": "cell"]
            case .headerRow: return ["type": "toggleTableHeader", "target": "row"]
            case .headerColumn: return ["type": "toggleTableHeader", "target": "column"]
            case .resize: return ["type": "setTableColumnWidth"]
            case .deleteTable: return ["type": "deleteTable"]
            }
        }

        func expected(document: inout Document, caret: Caret) -> Caret {
            guard case let .cell(row, column, paragraph, offset) = caret, var grid = document.grid else {
                preconditionFailure("Structural matrix commands require a cell caret")
            }
            var result = caret
            switch self {
            case .rowBefore, .rowAfter:
                let insertion = row + (self == .rowAfter ? Sequence.unitSpan : 0)
                var reference: Int? = max(0, insertion - Sequence.unitSpan)
                if grid[reference!].allSatisfy(\.header) {
                    reference = insertion == 0 || insertion == grid.count ? nil : insertion
                }
                let fresh = grid[0].indices.map { columnIndex in
                    Cell(header: reference.map { grid[$0][columnIndex].header } ?? false, paragraphs: [""],
                        widths: grid.compactMap { $0[columnIndex].widths }.first)
                }
                grid.insert(fresh, at: insertion)
                if self == .rowBefore {
                    result = .cell(row: row + Sequence.unitSpan, column: column, paragraph: paragraph, offset: offset)
                }
            case .columnBefore, .columnAfter:
                let insertion = column + (self == .columnAfter ? Sequence.unitSpan : 0)
                let reference = max(0, insertion - Sequence.unitSpan)
                let referenceIsHeader = grid.allSatisfy { $0[reference].header }
                for index in grid.indices {
                    let header = !referenceIsHeader && grid[index][reference].header
                    grid[index].insert(Cell(header: header, paragraphs: [""]), at: insertion)
                }
                if self == .columnBefore {
                    result = .cell(row: row, column: column + Sequence.unitSpan, paragraph: paragraph, offset: offset)
                }
            case .deleteRow:
                grid.remove(at: row)
                result = row < grid.count ? .cell(row: row, column: 0, offset: 0) : .after(offset: 0)
            case .deleteColumn:
                for index in grid.indices { grid[index].remove(at: column) }
                if column < grid[row].count {
                    result = .cell(row: row, column: column, offset: 0)
                } else if row + Sequence.unitSpan < grid.count {
                    result = .cell(row: row + Sequence.unitSpan, column: 0, offset: 0)
                } else {
                    result = .after(offset: 0)
                }
            case .headerCell, .headerRow, .headerColumn:
                let rows = self == .headerColumn ? Array(grid.indices) : [row]
                let columns = self == .headerRow ? Array(grid[row].indices) : [column]
                let holdsHeader = rows.contains { rowIndex in columns.contains { grid[rowIndex][$0].header } }
                for rowIndex in rows {
                    for columnIndex in columns where !holdsHeader || grid[rowIndex][columnIndex].header {
                        grid[rowIndex][columnIndex].header = !holdsHeader
                    }
                }
            case .resize:
                for index in grid.indices { grid[index][column].widths = [Sequence.resizedWidth] }
            case .deleteTable:
                document.grid = nil
                return .before(offset: document.before.unicodeScalars.count)
            }
            document.grid = grid
            return result
        }
    }

    private final class Session {
        let fixture: MountedEditorTestFixture
        var view: RichTextEditorView
        var adapter: EditorV2Adapter { fixture.adapter }

        init(document: Document) throws {
            fixture = try makeMountedEditorFixture(config: TableInputTestSchema.tableConfig,
                document: EditorTableCommandSequenceTests.json(document.object), size: Sequence.editorSize, windowed: true)
            view = fixture.view
            view.layoutIfNeeded()
        }

        func close() {
            _ = view.activeTextInput.resignFirstResponder()
            view.editorId = 0
            view.removeFromSuperview()
            fixture.close()
        }

        func bind(row: Int, column: Int, document: Document, offset: Int) throws {
            let index = document.grid!.prefix(row).reduce(0) { $0 + $1.count } + column
            let tableID = try adapter.editableTableID()
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: UInt32(index), contentRect: .zero))
            let input = view.activeTextInput
            XCTAssertTrue(input.becomeFirstResponder())
            input.selectedRange = NSRange(location: offset, length: 0)
            input.syncSelectionImmediately()
            view.layoutIfNeeded()
        }

        func apply(_ command: Command, caret: Caret, label: String) throws {
            let admission = try XCTUnwrap(adapter.tableMutationAdmission(tableID: adapter.editableTableID()), label)
            let update: String?
            switch command {
            case .resize:
                guard case let .cell(_, column, _, _) = caret else { preconditionFailure("Resize needs a cell") }
                update = adapter.resizeTableColumn(column: column, width: Sequence.resizedWidth, admission: admission)
            case .deleteTable: update = adapter.deleteTable(admission: admission)
            default: update = adapter.applyTableCommandAtSelection(command.payload, admission: admission)
            }
            XCTAssertTrue(view.activeTextInput.applyUpdateJSON(try XCTUnwrap(update, "\(label): command refused")), label)
        }

        func apply(_ command: [String: String], label: String) throws {
            let admission = try XCTUnwrap(adapter.tableMutationAdmission(tableID: adapter.editableTableID()), label)
            let update = try XCTUnwrap(adapter.applyTableCommandAtSelection(command, admission: admission), "\(label): command refused")
            XCTAssertTrue(view.activeTextInput.applyUpdateJSON(update), label)
        }

        func selectCells(anchor: UInt32, head: UInt32, label: String) throws {
            let tableID = try adapter.editableTableID()
            let index = try XCTUnwrap(try adapter.tableCellPositions().firstIndex(of: anchor), label)
            let mutation = try XCTUnwrap(adapter.tableMutationAdmission(tableID: tableID), label)
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.selectTableCell(cellIndex: index, admission: mutation))), label)
            let selection = try XCTUnwrap(adapter.tableCellSelectionAdmission(tableID: tableID), label)
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.selectExactTableCells(
                anchor: anchor, head: head, admission: selection))), label)
            XCTAssertTrue(view.textView.becomeFirstResponder())
        }

        func history(redo: Bool, label: String) throws {
            let update = redo ? adapter.redo() : adapter.undo()
            XCTAssertTrue(view.activeTextInput.applyUpdateJSON(try XCTUnwrap(update, label)), label)
        }

        func tab(backward: Bool, label: String) throws {
            let input = view.activeTextInput
            let modifiers: UIKeyModifierFlags = backward ? [.shift] : []
            let command = try XCTUnwrap(input.keyCommands?.first {
                $0.input == Sequence.tab && $0.modifierFlags == modifiers
            }, "\(label): tab unavailable")
            _ = input.perform(command.action)
        }

        @MainActor
        func tap(row: Int, column: Int, document: Document, label: String) async throws {
            view.layoutIfNeeded()
            let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first, label)
            let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first, label)
            let cell = try drawing.presentedRealCell(tableID: adapter.editableTableID(),
                position: document.cellPosition(row: row, column: column))
            let point = drawing.convert(CGPoint(x: cell.bounds.midX, y: cell.bounds.midY), to: view)
            XCTAssertTrue(view.activateTableCell(at: point), "\(label): native cell tap refused")
            let expectedCaret = document.position(of: .cell(row: row, column: column, offset: 0))
            let settled = XCTestExpectation(description: label + "/native-selection-synced")
            let input = view.activeTextInput
            let selectionSynced = input.onAuthoritativeTextSelectionSynced
            var didSettle = false
            input.onAuthoritativeTextSelectionSynced = { [weak adapter] in
                selectionSynced?()
                guard !didSettle, let selection = adapter?.cachedAtomicRenderSelection(),
                    selection["type"] as? String == Sequence.textSelection
                    && EditorV2Adapter.uint32Field(selection, "anchor") == expectedCaret
                    && EditorV2Adapter.uint32Field(selection, "head") == expectedCaret
                else { return }
                didSettle = true
                settled.fulfill()
            }
            defer { input.onAuthoritativeTextSelectionSynced = selectionSynced }
            let mainThread = Thread.isMainThread
            let runLoopMode = RunLoop.current.currentMode?.rawValue ?? "none"
            let mainQueueResumed = XCTestExpectation(description: label + "/main-queue-resumed")
            var queueResumed = false
            DispatchQueue.main.async {
                queueResumed = true
                mainQueueResumed.fulfill()
            }
            let result = await XCTWaiter.fulfillment(of: [settled, mainQueueResumed], timeout: Sequence.nativeSelectionSyncTimeout)
            XCTAssertEqual(result, .completed,
                "\(label): native tap selection did not settle; mainThread=\(mainThread), runLoopMode=\(runLoopMode), didSettle=\(didSettle), queueResumed=\(queueResumed), cachedSelection=\(String(describing: adapter.cachedAtomicRenderSelection()))")
        }

        func remount() throws {
            let old = view
            old.editorId = 0
            old.removeFromSuperview()
            view = RichTextEditorView(frame: old.frame)
            try XCTUnwrap(fixture.window).addSubview(view)
            view.bindEditor(id: fixture.editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
            XCTAssertTrue(view.textView.becomeFirstResponder())
        }
    }

    func testInsertRowsBeforeKeepsEveryCellCaretTypeable() throws { try runMatrix(.rowBefore) }
    func testInsertRowsAfterKeepsEveryCellCaretTypeable() throws { try runMatrix(.rowAfter) }
    func testInsertColumnsBeforeKeepsEveryCellCaretTypeable() throws { try runMatrix(.columnBefore) }
    func testInsertColumnsAfterKeepsEveryCellCaretTypeable() throws { try runMatrix(.columnAfter) }
    func testDeleteRowsMapsEveryCellCaretToTheNextEditablePosition() throws { try runMatrix(.deleteRow) }
    func testDeleteColumnsMapsEveryCellCaretToTheNextEditablePosition() throws { try runMatrix(.deleteColumn) }
    func testToggleHeaderCellKeepsEveryCellCaretTypeable() throws { try runMatrix(.headerCell) }
    func testToggleHeaderRowsKeepsEveryCellCaretTypeable() throws { try runMatrix(.headerRow) }
    func testToggleHeaderColumnsKeepsEveryCellCaretTypeable() throws { try runMatrix(.headerColumn) }
    func testResizeColumnsKeepsEveryCellCaretTypeable() throws { try runMatrix(.resize) }
    func testDeleteTableMapsEveryCellCaretToThePrecedingProseEnd() throws { try runMatrix(.deleteTable) }

    private func runMatrix(_ command: Command) throws {
        for headerRow in [false, true] {
            for row in 0..<Sequence.gridSize {
                for column in 0..<Sequence.gridSize {
                    let seed = Document(headerRow: headerRow)
                    let end = seed.grid![row][column].paragraphs[0].unicodeScalars.count
                    for offset in [0, Sequence.middleOffset, end] {
                        let label = "\(command.rawValue)/headerRow=\(headerRow)/r\(row)c\(column)/offset=\(offset)"
                        try XCTContext.runActivity(named: label) { _ in
                            try autoreleasepool {
                                let session = try Session(document: seed)
                                defer { session.close() }
                                try session.bind(row: row, column: column, document: seed, offset: offset)
                                let caret = Caret.cell(row: row, column: column, offset: offset)
                                try check(session, document: seed, caret: caret, label: label + "/before")
                                var expected = seed
                                let next = command.expected(document: &expected, caret: caret)
                                try session.apply(command, caret: caret, label: label)
                                try check(session, document: expected, caret: next, label: label + "/command")
                                try probe(session, document: expected, caret: next, label: label)
                            }
                        }
                    }
                }
            }
        }
    }

    @MainActor
    func testLongStructuralSelectionHistoryAndRemountTraceStaysEditable() async throws {
        let previousFixturesDrained = expectation(description: "previous fixture callbacks drained")
        DispatchQueue.main.async { previousFixturesDrained.fulfill() }
        let setup = await XCTWaiter.fulfillment(of: [previousFixturesDrained], timeout: MountedEditorTestFixture.releaseTimeout)
        XCTAssertEqual(setup, .completed, "closed fixture callbacks blocked the next native editing sequence")
        guard setup == .completed else { return }
        var document = Document(headerRow: true, widths: nil)
        let session = try Session(document: document)
        defer { session.close() }
        var caret = Caret.cell(row: 1, column: 1, offset: Sequence.middleOffset)
        try session.bind(row: 1, column: 1, document: document, offset: Sequence.middleOffset)
        for (step, command) in [Command.rowBefore, .columnAfter, .headerCell, .resize, .rowAfter, .columnBefore].enumerated() {
            let label = "trace/\(step)/\(command.rawValue)"
            try session.apply(command, caret: caret, label: label)
            caret = command.expected(document: &document, caret: caret)
            try check(session, document: document, caret: caret, label: label)
            try probe(session, document: document, caret: caret, label: label)
        }

        let mergeRow = 2
        let anchor = document.cellPosition(row: mergeRow, column: 0)
        let head = document.cellPosition(row: mergeRow, column: Sequence.mergedSpan - Sequence.unitSpan)
        try session.selectCells(anchor: anchor, head: head, label: "trace/select-merge")
        try checkCells(session, document: document, anchor: anchor, head: head,
            selected: Set((0..<Sequence.mergedSpan).map { document.cellPosition(row: mergeRow, column: $0) }),
            label: "trace/select-merge")
        let beforeMerge = document
        let sources = Array(document.grid![mergeRow].prefix(Sequence.mergedSpan))
        var merged = sources[0]
        merged.colspan = Sequence.mergedSpan
        merged.paragraphs = sources.flatMap(\.paragraphs).filter { !$0.isEmpty }
        let mergedWidths = sources.flatMap { $0.widths ?? Array(repeating: Sequence.unsetColumnWidth, count: $0.colspan) }
        merged.widths = mergedWidths.contains { $0 > Sequence.unsetColumnWidth } ? mergedWidths : nil
        document.grid![mergeRow].replaceSubrange(0..<Sequence.mergedSpan, with: [merged])
        try session.apply(["type": "mergeTableCells"], label: "trace/merge")
        try checkCells(session, document: document, anchor: anchor, head: anchor, selected: [anchor], label: "trace/merge")
        try session.history(redo: false, label: "trace/undo-merge")
        try checkCells(session, document: beforeMerge, anchor: anchor, head: head,
            selected: Set((0..<Sequence.mergedSpan).map { beforeMerge.cellPosition(row: mergeRow, column: $0) }),
            label: "trace/undo-merge")
        try session.history(redo: true, label: "trace/redo-merge")
        try checkCells(session, document: document, anchor: anchor, head: anchor, selected: [anchor], label: "trace/redo-merge")

        let beforeSplit = document
        merged.colspan = Sequence.unitSpan
        let splitCells = (0..<Sequence.mergedSpan).map { column -> Cell in
            var cell = merged
            if column > 0 { cell.paragraphs = [""] }
            cell.widths = mergedWidths[column] > Sequence.unsetColumnWidth ? [mergedWidths[column]] : nil
            return cell
        }
        document.grid![mergeRow].replaceSubrange(0..<Sequence.unitSpan,
            with: splitCells)
        let splitHead = document.cellPosition(row: mergeRow, column: Sequence.mergedSpan - Sequence.unitSpan)
        let splitSelection = Set((0..<Sequence.mergedSpan).map { document.cellPosition(row: mergeRow, column: $0) })
        try session.apply(["type": "splitTableCell"], label: "trace/split")
        try checkCells(session, document: document, anchor: anchor, head: splitHead,
            selected: splitSelection, label: "trace/split")
        try session.history(redo: false, label: "trace/undo-split")
        try checkCells(session, document: beforeSplit, anchor: anchor, head: anchor, selected: [anchor], label: "trace/undo-split")
        try session.history(redo: true, label: "trace/redo-split")
        try checkCells(session, document: document, anchor: anchor, head: splitHead,
            selected: splitSelection, label: "trace/redo-split")

        let beforeClear = document
        for column in 0..<Sequence.mergedSpan { document.grid![mergeRow][column].paragraphs = [""] }
        let clearHead = document.cellPosition(row: mergeRow, column: Sequence.mergedSpan - Sequence.unitSpan)
        let clearedSelection = Set((0..<Sequence.mergedSpan).map { document.cellPosition(row: mergeRow, column: $0) })
        try session.apply(["type": "clearTableCells"], label: "trace/clear")
        try checkCells(session, document: document, anchor: anchor, head: clearHead,
            selected: clearedSelection, label: "trace/clear")
        try session.history(redo: false, label: "trace/undo-clear")
        try checkCells(session, document: beforeClear, anchor: anchor, head: splitHead,
            selected: splitSelection, label: "trace/undo-clear")
        try session.history(redo: true, label: "trace/redo-clear")
        try checkCells(session, document: document, anchor: anchor, head: clearHead,
            selected: clearedSelection, label: "trace/redo-clear")

        try await session.tap(row: mergeRow, column: Sequence.mergedSpan, document: document, label: "trace/leave-rectangle")
        caret = .cell(row: mergeRow, column: Sequence.mergedSpan, offset: 0)
        try check(session, document: document, caret: caret, label: "trace/leave-rectangle")
        try probe(session, document: document, caret: caret, label: "trace/leave-rectangle")
        try session.tab(backward: false, label: "trace/tab")
        caret = .cell(row: mergeRow, column: Sequence.mergedSpan + Sequence.unitSpan, offset: 0)
        try check(session, document: document, caret: caret, label: "trace/tab")
        try probe(session, document: document, caret: caret, label: "trace/tab")
        try session.tab(backward: true, label: "trace/shift-tab")
        caret = .cell(row: mergeRow, column: Sequence.mergedSpan, offset: 0)
        try check(session, document: document, caret: caret, label: "trace/shift-tab")
        try probe(session, document: document, caret: caret, label: "trace/shift-tab")

        try session.apply(.deleteColumn, caret: caret, label: "trace/delete-column")
        caret = Command.deleteColumn.expected(document: &document, caret: caret)
        try check(session, document: document, caret: caret, label: "trace/delete-column")
        try probe(session, document: document, caret: caret, label: "trace/delete-column")
        try session.apply(.deleteRow, caret: caret, label: "trace/delete-row")
        caret = Command.deleteRow.expected(document: &document, caret: caret)
        try check(session, document: document, caret: caret, label: "trace/delete-row")
        try probe(session, document: document, caret: caret, label: "trace/delete-row")
        let beforeDelete = document
        let beforeDeleteCaret = caret
        try session.apply(.deleteTable, caret: caret, label: "trace/delete-table")
        caret = Command.deleteTable.expected(document: &document, caret: caret)
        try check(session, document: document, caret: caret, label: "trace/delete-table")
        try session.history(redo: false, label: "trace/undo-delete-table")
        try check(session, document: beforeDelete, caret: beforeDeleteCaret, label: "trace/undo-delete-table")
        try session.history(redo: true, label: "trace/redo-delete-table")
        try check(session, document: document, caret: caret, label: "trace/redo-delete-table")
        try probe(session, document: document, caret: caret, label: "trace/redo-delete-table")

        session.view.editorId = 0
        session.view.bindEditor(id: session.fixture.editorId, initialUpdateJSON: try XCTUnwrap(session.adapter.initialUpdateJSON()))
        XCTAssertTrue(session.view.textView.becomeFirstResponder())
        try check(session, document: document, caret: caret, label: "trace/rebind")
        try probe(session, document: document, caret: caret, label: "trace/rebind")
        try session.remount()
        try check(session, document: document, caret: caret, label: "trace/remount")
        try probe(session, document: document, caret: caret, label: "trace/remount")
    }

    private static func paragraph(_ text: String) -> [String: Any] {
        text.isEmpty ? ["type": Sequence.paragraphNode] : [
            "type": Sequence.paragraphNode, "content": [["type": Sequence.textNode, "text": text]]
        ]
    }

    private static func json(_ value: Any) throws -> String {
        try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: value, options: .sortedKeys), encoding: .utf8))
    }

    private func checkDocumentAndRendering(_ session: Session, document: Document, label: String) throws {
        session.view.layoutIfNeeded()
        let raw = try XCTUnwrap(session.adapter.documentJson(), label)
        let actual = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? NSDictionary, label)
        XCTAssertEqual(actual, document.object as NSDictionary, "\(label): full document differs: \(raw)")
        guard let grid = document.grid else {
            XCTAssertTrue(session.adapter.tableRecordsForTesting.isEmpty, "\(label): deleted table still indexed")
            return
        }
        let tableID = try session.adapter.editableTableID()
        let record = try XCTUnwrap(session.adapter.tableRecordsForTesting[tableID], label)
        XCTAssertEqual(record["rows"] as? Int, grid.count, "\(label): stale row count")
        XCTAssertEqual(record["columns"] as? Int, grid[0].reduce(0) { $0 + $1.colspan }, "\(label): stale column count")
        XCTAssertEqual(record["irregular"] as? Bool, false, "\(label): regular table became irregular")
        let surface = try XCTUnwrap(session.view.subviews.compactMap { $0 as? EditorTableSurface }.first, label)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first, label)
        let positions = try session.adapter.tableCellPositions(tableID: tableID)
        var sourceIndex = 0
        for row in grid.indices {
            var logicalColumn = 0
            for column in grid[row].indices {
                let expected = grid[row][column]
                let position = document.cellPosition(row: row, column: column)
                let indexedPosition = try XCTUnwrap(positions.indices.contains(sourceIndex) ? positions[sourceIndex] : nil,
                    "\(label): rendered cell r\(row)c\(column) is missing from the index")
                XCTAssertEqual(indexedPosition, position, "\(label): stale cell position r\(row)c\(column)")
                let cell = try drawing.presentedRealCell(tableID: tableID, position: position)
                XCTAssertEqual(cell.sourceIndex, sourceIndex, "\(label): stale rendered source index")
                XCTAssertEqual(cell.cell.row, row, "\(label): stale rendered row")
                XCTAssertEqual(cell.cell.column, logicalColumn, "\(label): stale rendered column")
                XCTAssertEqual(cell.cell.colspan, expected.colspan, "\(label): stale rendered span")
                XCTAssertEqual(cell.cell.isHeader, expected.header, "\(label): stale rendered header")
                XCTAssertEqual(cell.cell.accessibilitySummary.filter { $0.role == .text }.map(\.label),
                    expected.paragraphs.filter { !$0.isEmpty }, "\(label): stale rendered text r\(row)c\(column)")
                XCTAssertTrue([cell.bounds.minX, cell.bounds.minY, cell.bounds.width, cell.bounds.height].allSatisfy(\.isFinite),
                    "\(label): non-finite cell geometry")
                XCTAssertGreaterThan(cell.bounds.height, 0, "\(label): invisible rendered cell")
                if expected.colspan == Sequence.unitSpan, let width = expected.widths?.first {
                    XCTAssertEqual(cell.bounds.width, CGFloat(width), accuracy: Sequence.geometryAccuracy,
                        "\(label): stale rendered width r\(row)c\(column)")
                }
                logicalColumn += expected.colspan
                sourceIndex += Sequence.unitSpan
            }
        }
        XCTAssertEqual(positions.count, sourceIndex, "\(label): stale real cell count")
    }

    private func check(_ session: Session, document: Document, caret: Caret, label: String) throws {
        try checkDocumentAndRendering(session, document: document, label: label)
        let input = session.view.activeTextInput
        input.layoutIfNeeded()
        XCTAssertTrue(input.isFirstResponder, "\(label): input lost focus")
        XCTAssertTrue(input.isAuthorizedForTableCellInput(), "\(label): input authority is stale")
        XCTAssertEqual(input.selectedRange.length, 0, "\(label): native caret is a range")
        XCTAssertLessThanOrEqual(NSMaxRange(input.selectedRange), input.textStorage.length, "\(label): caret beyond native storage")
        let position = document.position(of: caret)
        let selection = try XCTUnwrap(session.adapter.cachedAtomicRenderSelection(), label)
        XCTAssertEqual(selection["type"] as? String, Sequence.textSelection, "\(label): engine caret became \(selection)")
        if document.grid != nil {
            let surface = try XCTUnwrap(session.view.subviews.compactMap { $0 as? EditorTableSurface }.first, label)
            let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first, label)
            XCTAssertTrue(drawing.selectedTableCellSourceIndices.values.allSatisfy(\.isEmpty),
                "\(label): a cell rectangle survived the text caret")
        }
        for endpoint in ["anchor", "head"] {
            XCTAssertEqual(EditorV2Adapter.uint32Field(selection, endpoint), position, "\(label): wrong engine \(endpoint): \(selection)")
        }
        let logical = try XCTUnwrap(input.currentLogicalScalarSelection(), "\(label): native caret cannot map")
        XCTAssertEqual(session.adapter.documentPosition(forScalar: logical.anchor), position, "\(label): native anchor differs")
        XCTAssertEqual(session.adapter.documentPosition(forScalar: logical.head), position, "\(label): native head differs")
        switch caret {
        case let .cell(row, column, _, _):
            XCTAssertFalse(input === session.view.textView, "\(label): root retained a cell caret")
            XCTAssertEqual(session.view.activeTableCellPosition, document.cellPosition(row: row, column: column),
                "\(label): wrong active cell")
            let binding = try XCTUnwrap(input.tableCellPositionMap?.binding, label)
            XCTAssertEqual(binding.documentRevision, session.adapter.baseDocumentRevision, "\(label): stale binding revision")
            XCTAssertEqual(binding.positionEpoch, session.adapter.positionEpoch, "\(label): stale binding epoch")
            XCTAssertEqual(input.textStorage.string.replacingOccurrences(of: Sequence.placeholder, with: ""),
                document.grid![row][column].paragraphs.joined(separator: "\n"), "\(label): stale native cell text")
        case .before, .after:
            XCTAssertTrue(input === session.view.textView, "\(label): retired cell kept the input")
            XCTAssertNil(session.view.activeTableCellPosition, "\(label): retired cell remained bound")
            XCTAssertTrue(input.textStorage.string.contains(document.before), "\(label): preceding prose disappeared")
            XCTAssertTrue(input.textStorage.string.contains(document.after), "\(label): following prose disappeared")
            if document.grid == nil {
                XCTAssertEqual(input.textStorage.string, document.before + "\n" + document.after,
                    "\(label): stale native table content survived deletion")
                XCTAssertFalse(PositionBridge.hasRootTableScalarExtents(in: input), "\(label): stale native table marker survived deletion")
            }
        }
        let rect = input.caretRect(for: try XCTUnwrap(input.selectedTextRange?.end, label))
        XCTAssertTrue([rect.minX, rect.minY, rect.width, rect.height].allSatisfy(\.isFinite), "\(label): non-finite caret \(rect)")
        XCTAssertGreaterThan(rect.height, 0, "\(label): collapsed caret")
        XCTAssertTrue(input.bounds.insetBy(dx: -rect.width, dy: -rect.height).intersects(rect), "\(label): caret outside input \(rect)")
        XCTAssertTrue(session.view.bounds.intersects(input.convert(rect, to: session.view)), "\(label): caret outside visible editor \(rect)")
    }

    private func checkCells(
        _ session: Session, document: Document, anchor: UInt32, head: UInt32, selected: Set<UInt32>, label: String
    ) throws {
        try checkDocumentAndRendering(session, document: document, label: label)
        let selection = try XCTUnwrap(session.adapter.cachedAtomicRenderSelection(), label)
        XCTAssertEqual(selection["type"] as? String, Sequence.cellSelection, "\(label): rectangle became \(selection)")
        let endpoints = try XCTUnwrap(EditorCellSelection.endpointPositions(selection), label)
        XCTAssertEqual(endpoints.anchor, anchor, "\(label): wrong rectangle anchor")
        XCTAssertEqual(endpoints.head, head, "\(label): wrong rectangle head")
        XCTAssertTrue(session.view.activeTextInput === session.view.textView, "\(label): a cell input retained the rectangle")
        XCTAssertTrue(session.view.textView.isFirstResponder, "\(label): rectangle lost focus")
        let surface = try XCTUnwrap(session.view.subviews.compactMap { $0 as? EditorTableSurface }.first, label)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first, label)
        let tableID = try session.adapter.editableTableID()
        let drawn = Set((drawing.selectedTableCellSourceIndices[tableID] ?? []).compactMap {
            drawing.tableCellDocumentPosition?(tableID, $0)
        })
        XCTAssertEqual(drawn, selected, "\(label): stale drawn rectangle")
    }

    private func probe(_ session: Session, document: Document, caret: Caret, label: String) throws {
        var pending = Sequence.probe
        var typed = document
        typed.insert(pending, at: caret)
        session.view.activeTextInput.insertText(pending)
        try check(session, document: typed, caret: caret.advanced(by: pending.unicodeScalars.count), label: label + "/type")
        while !pending.isEmpty {
            pending.removeLast()
            typed = document
            typed.insert(pending, at: caret)
            session.view.activeTextInput.deleteBackward()
            try check(session, document: typed, caret: caret.advanced(by: pending.unicodeScalars.count),
                label: label + "/backspace/remaining=\(pending.count)")
        }
    }
}
