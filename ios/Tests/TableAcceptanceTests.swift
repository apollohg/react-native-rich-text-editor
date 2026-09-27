import XCTest

final class TableAcceptanceTests: XCTestCase {
    private enum Acceptance {
        static let config = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table"},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[{"name":"strong"},{"name":"em"}]},"initialization":{"type":"localHtml","html":"","snapshotScope":{"documentId":"table-acceptance","lineageId":"native-editor|table-acceptance"}}}"#
        static let introText = "Intro"
        static let trailingText = "Trailing"
        static let trailingParagraphs = 40
        static let editorSize = CGSize(width: 390, height: 480)
        static let tableRows = 3
        static let tableColumns = 3
        static let cellText = "abcdefghijkl"
        static let boldPrefixLength = 4
        static let secondParagraph = "second"
        static let pastedTSVRow = "\"abcdefghijkl\nsecond\"\tBody"
        static let bodyText = "Body"
        static let composedText = "Z"
        static let strongMark = "strong"
        static let paragraphBreak = "\n"
        static let resizeDelta: CGFloat = 60
        static let minimumResizedWidth = 100
        static let remoteRequestIdBase: UInt64 = 23_000_000
        static let exportDirectoryKey = "TABLE_ACCEPTANCE_EXPORT_DIR"
        static let exportFileName = "ios-table-acceptance.json"
        static let insertTable = "insertTable"
        static let deleteTableRows = "deleteTableRows"
        static let tableNode = "table"
        static let rowNode = "table_row"
        static let cellNode = "table_cell"
        static let headerNode = "table_header"
        static let paragraphNode = "paragraph"
        static let mergeLabel = "Merge cells"
        static let splitLabel = "Split cell"
        static let addRowAfterLabel = "Insert row below"
        static let deleteRowLabel = "Delete row"
        static let addColumnAfterLabel = "Insert column after"
        static let deleteColumnLabel = "Delete column"
        static let geometryAccuracy: CGFloat = 0.5
        static let mainQueueDrainTimeout: TimeInterval = 1
        static let parityTheme = ##"{"text":{"fontSize":17,"color":"#1b1f2aff"},"backgroundColor":"#ffffffff","contentInsets":{"top":12,"right":12,"bottom":12,"left":12},"table":{"minColumnWidth":72,"cellPadding":8,"borderWidth":1,"borderColor":"#a0acb7ff","headerBackgroundColor":"#e8f0f5ff"}}"##
        static let parityDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"Merged header across two columns"}]}]},{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"Status"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"A tall cell whose text wraps over several lines"}]},{"type":"paragraph","content":[{"type":"text","text":"second"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"abcdefghijkl"}]}]},{"type":"table_cell","attrs":{"colwidth":[140]},"content":[{"type":"paragraph","content":[{"type":"text","text":"Ready"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"x"}]}]},{"type":"table_cell","content":[{"type":"paragraph"}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"After"}]}]}"#
        static let irregularDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Raw"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","attrs":{"colspan":2},"content":[{"type":"paragraph","content":[{"type":"text","text":"Wide header"}]}]},{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"Status"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","attrs":{"rowspan":3},"content":[{"type":"paragraph","content":[{"type":"text","text":"Overhang"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Short row"}]}]}]},{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"One"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Two"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Three"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"Four"}]}]}]}]}]}"#
        static let irregularShortRowCell = 3
        static let parityLayoutKey = "table-acceptance-parity"
    }

    private struct Cell: Equatable, CustomStringConvertible {
        let type: String
        let paragraphs: [String]
        let colspan: Int
        let rowspan: Int
        let colwidth: [Int]?

        var description: String {
            "\(type)[\(paragraphs.joined(separator: "¶"))]\(colspan)x\(rowspan)\(colwidth.map { "w\($0)" } ?? "")"
        }
    }

    private final class Harness {
        let editorId: UInt64
        let adapter: EditorV2Adapter
        let window: UIWindow
        private(set) var expo: NativeEditorExpoView
        private var nextRemoteRequestId = Acceptance.remoteRequestIdBase

        init(editorId: UInt64, adapter: EditorV2Adapter) {
            self.editorId = editorId
            self.adapter = adapter
            window = UIWindow(frame: CGRect(origin: .zero, size: Acceptance.editorSize))
            expo = NativeEditorExpoView()
            expo.frame = window.bounds
            window.addSubview(expo)
            window.makeKeyAndVisible()
            expo.setEditorId(editorId)
        }

        var view: RichTextEditorView { expo.richTextView }
        var root: EditorTextView { view.textView }

        var surface: EditorTableSurface {
            get throws { try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first) }
        }

        var drawing: PreparedProseDrawingView {
            get throws { try XCTUnwrap(try surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first) }
        }

        var tableID: String {
            get throws {
                try XCTUnwrap(adapter.cachedTableRecords.first { $0.value["readOnlyDescendants"] as? Bool == false }?.key,
                              "no editable table is rendered")
            }
        }

        func positions() throws -> [UInt32] {
            let cells = try XCTUnwrap(adapter.cachedTableRecords[try tableID]?["cells"] as? [[String: Any]])
            return try cells.map { try XCTUnwrap(EditorV2Adapter.uint32Field($0, "sourcePos")) }
        }

        func activeCell() -> UInt32? {
            view.activeTextInput.tableCellPositionMap?.binding.cellSourcePosition
        }

        func selectedCells() throws -> Set<Int> {
            try drawing.selectedTableCellSourcePositions[try tableID] ?? []
        }

        func presentedCell(_ position: UInt32) throws -> ViewerTablePresentedCell {
            let id = try tableID
            return try XCTUnwrap(try drawing.mountedTablePresentation()?.cells.first {
                $0.surface.identity == id && $0.sourcePosition == Int(position) && $0.cell.sourceCellIndex != nil
            }, "cell \(position) is not presented")
        }

        func documentJSON() throws -> String {
            try XCTUnwrap(adapter.documentJson())
        }

        func blocks() throws -> [[String: Any]] {
            let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(try documentJSON().utf8)) as? [String: Any])
            return try XCTUnwrap(object["content"] as? [[String: Any]])
        }

        func grid() throws -> [[Cell]] {
            let table = try XCTUnwrap(try blocks().first { $0["type"] as? String == Acceptance.tableNode })
            let rows = try XCTUnwrap(table["content"] as? [[String: Any]])
            return try rows.map { row in
                XCTAssertEqual(row["type"] as? String, Acceptance.rowNode)
                return try (row["content"] as? [[String: Any]] ?? []).map(Self.cell)
            }
        }

        private static func cell(_ node: [String: Any]) throws -> Cell {
            let attrs = node["attrs"] as? [String: Any] ?? [:]
            let paragraphs = try (node["content"] as? [[String: Any]] ?? []).map { block -> String in
                XCTAssertEqual(block["type"] as? String, Acceptance.paragraphNode)
                return (block["content"] as? [[String: Any]] ?? []).compactMap { run in
                    guard let text = run["text"] as? String else { return nil }
                    let marks = (run["marks"] as? [[String: Any]] ?? []).compactMap { $0["type"] as? String }
                    return marks.isEmpty ? text : "<\(marks.joined(separator: ","))>\(text)</>"
                }.joined()
            }
            return try Cell(type: XCTUnwrap(node["type"] as? String), paragraphs: paragraphs,
                            colspan: attrs["colspan"] as? Int ?? 1, rowspan: attrs["rowspan"] as? Int ?? 1,
                            colwidth: attrs["colwidth"] as? [Int])
        }

        func applyLocalCommand(_ command: [String: Any]) throws {
            let result = adapter.callWithEnvelope(["command": command]) {
                editorV2ApplyCommand(editorId: self.adapter.editorId, requestJson: $0)
            }
            XCTAssertNil(result.error, "the local command \(command) was refused: \(String(describing: result.error))")
            XCTAssertTrue(root.applyUpdateJSON(try XCTUnwrap(adapter.refreshFromRustState(mirrorSelection: nil))))
            expo.layoutIfNeeded()
        }

        func applyRemote(_ payload: [String: Any], call: (String, String) -> FfiJsonResult) throws {
            nextRemoteRequestId += 1
            var envelope = payload
            envelope["version"] = 1
            envelope["requestId"] = String(nextRemoteRequestId)
            envelope["baseDocumentRevision"] = String(adapter.baseDocumentRevision)
            let data = try JSONSerialization.data(withJSONObject: envelope)
            let result = call(adapter.editorId, try XCTUnwrap(String(data: data, encoding: .utf8)))
            XCTAssertNil(result.error, "the remote peer's change was refused: \(String(describing: result.error))")
        }

        func deliverRemoteCommit() {
            NativeEditorViewRegistry.shared.applyRemoteCommitRefresh(editorId: editorId)
            expo.layoutIfNeeded()
        }

        func bind(cell position: UInt32) throws -> EditorTextView {
            let index = try XCTUnwrap(try positions().firstIndex(of: position))
            XCTAssertTrue(view.bindTableCell(tableID: try tableID, cellIndex: UInt32(index), contentRect: .zero))
            let input = view.activeTextInput
            XCTAssertFalse(input === root, "cell \(position) must own the reusable cell input")
            XCTAssertTrue(input.becomeFirstResponder())
            return input
        }

        func place(_ input: EditorTextView, range: NSRange) {
            input.selectedRange = range
            input.textViewDidChangeSelection(input)
        }

        func tableElement() throws -> TableAccessibilityTableElement {
            let drawing = try drawing
            return try XCTUnwrap((0..<drawing.accessibilityElementCount()).lazy.compactMap {
                drawing.accessibilityElement(at: $0) as? TableAccessibilityTableElement
            }.first, "the table exposes no accessibility data table")
        }

        func actionLabels(onCellAt position: UInt32) throws -> [String] {
            try cellElement(position).accessibilityCustomActions?.map(\.name) ?? []
        }

        func perform(_ label: String, onCellAt position: UInt32) throws {
            let element = try cellElement(position)
            let action = try XCTUnwrap(element.accessibilityCustomActions?.first { $0.name == label },
                                       "missing \(label) in \(element.accessibilityCustomActions?.map(\.name) ?? [])")
            XCTAssertTrue(action.actionHandler?(action) ?? false, "\(label) was refused")
            expo.layoutIfNeeded()
        }

        private func cellElement(_ position: UInt32) throws -> TableAccessibilityCellElement {
            try XCTUnwrap(try tableElement().cellElements.first { $0.cell.sourcePosition == Int(position) },
                          "cell \(position) has no accessibility element")
        }

        func remount() {
            expo.setEditorId(0)
            expo.removeFromSuperview()
            expo = NativeEditorExpoView()
            expo.frame = window.bounds
            window.addSubview(expo)
            expo.setEditorId(editorId)
            expo.layoutIfNeeded()
        }

        func close() {
            expo.setEditorId(0)
            window.isHidden = true
        }
    }

    func testIntegratedNativeTableWorkflowKeepsTheDocumentAndRealCellSelectionAtEveryStep() throws {
        UIPasteboard.general.items = []
        defer { UIPasteboard.general.items = [] }
        let editorId = makeV2Editor(configJson: Acceptance.config)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let harness = Harness(editorId: editorId, adapter: adapter)
        defer { harness.close() }
        XCTAssertTrue(harness.root.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(seedDocument()))))
        harness.expo.layoutIfNeeded()
        XCTAssertTrue(adapter.cachedTableRecords.isEmpty, "the seed holds no table yet")

        harness.root.selectedRange = NSRange(location: Acceptance.introText.count, length: 0)
        harness.root.syncSelectionImmediately()
        try harness.applyLocalCommand(["type": Acceptance.insertTable, "rows": Acceptance.tableRows,
                                       "columns": Acceptance.tableColumns, "withHeaderRow": true])
        var grid = try harness.grid()
        XCTAssertEqual(grid.map(\.count), Array(repeating: Acceptance.tableColumns, count: Acceptance.tableRows), "\(grid)")
        XCTAssertEqual(grid[0].map(\.type), Array(repeating: Acceptance.headerNode, count: Acceptance.tableColumns))
        XCTAssertEqual(Set(grid.dropFirst().flatMap { $0.map(\.type) }), [Acceptance.cellNode])
        XCTAssertEqual(try harness.blocks().first?["type"] as? String, Acceptance.paragraphNode, "the intro stays first")
        var positions = try harness.positions()
        XCTAssertEqual(positions.count, Acceptance.tableRows * Acceptance.tableColumns)
        XCTAssertEqual(try harness.selectedCells(), [], "an inserted table leaves a caret, not a cell rectangle")

        let bodyStart = positions[Acceptance.tableColumns]
        let input = try harness.bind(cell: bodyStart)
        input.insertText(Acceptance.cellText)
        harness.place(input, range: NSRange(location: 0, length: Acceptance.boldPrefixLength))
        input.performToolbarToggleMark(Acceptance.strongMark)
        harness.place(input, range: NSRange(location: Acceptance.cellText.count, length: 0))
        input.insertText(Acceptance.paragraphBreak)
        input.insertText(Acceptance.secondParagraph)
        harness.expo.layoutIfNeeded()
        grid = try harness.grid()
        let richCell = [
            "<\(Acceptance.strongMark)>\(Acceptance.cellText.prefix(Acceptance.boldPrefixLength))</>"
                + Acceptance.cellText.dropFirst(Acceptance.boldPrefixLength),
            Acceptance.secondParagraph
        ]
        XCTAssertEqual(grid[1][0].paragraphs, richCell, "\(grid)")
        XCTAssertEqual(harness.activeCell(), try harness.positions()[Acceptance.tableColumns], "typing keeps the cell bound")
        XCTAssertTrue(harness.view.activeTextInput === input, "one reusable cell input serves the whole session")

        positions = try harness.positions()
        let secondBody = try harness.bind(cell: positions[Acceptance.tableColumns + 1])
        XCTAssertTrue(secondBody === input, "rebinding reuses the single cell input")
        secondBody.insertText(Acceptance.bodyText)
        positions = try harness.positions()
        try EditorTableInputTests.selectCells(anchor: positions[Acceptance.tableColumns],
                                              head: positions[Acceptance.tableColumns + 1],
                                              adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        let bodyPair: Set<Int> = [Int(positions[Acceptance.tableColumns]), Int(positions[Acceptance.tableColumns + 1])]
        XCTAssertEqual(try harness.selectedCells(), bodyPair)

        harness.root.copy(nil)
        XCTAssertEqual(UIPasteboard.general.string, Acceptance.pastedTSVRow, "copy exports the real cell rectangle")
        let beforeMerge = try harness.grid()
        XCTAssertEqual(beforeMerge, grid.enumerated().map { index, row in
            index == 1 ? [row[0], Cell(type: Acceptance.cellNode, paragraphs: [Acceptance.bodyText], colspan: 1,
                                       rowspan: 1, colwidth: nil), row[2]] : row
        })

        try harness.perform(Acceptance.mergeLabel, onCellAt: positions[Acceptance.tableColumns])
        grid = try harness.grid()
        XCTAssertEqual(grid[1].count, Acceptance.tableColumns - 1, "\(grid)")
        XCTAssertEqual(grid[1][0].colspan, 2)
        XCTAssertEqual(grid[1][0].paragraphs, richCell + [Acceptance.bodyText])
        positions = try harness.positions()
        XCTAssertEqual(try harness.selectedCells(), [Int(positions[Acceptance.tableColumns])],
                       "the merged cell is the only selected real cell")

        try harness.perform(Acceptance.splitLabel, onCellAt: positions[Acceptance.tableColumns])
        grid = try harness.grid()
        XCTAssertEqual(grid.map(\.count), Array(repeating: Acceptance.tableColumns, count: Acceptance.tableRows), "\(grid)")
        XCTAssertEqual(grid[1][0].paragraphs, richCell + [Acceptance.bodyText], "split keeps the content in the anchor")
        XCTAssertEqual(grid[1][1].paragraphs, [""])
        positions = try harness.positions()
        XCTAssertEqual(try harness.selectedCells(), [Int(positions[Acceptance.tableColumns])],
                       "split keeps the content-holding top-left cell selected")

        try EditorTableInputTests.selectCells(anchor: positions[Acceptance.tableColumns * 2],
                                              head: positions[Acceptance.tableColumns * 2 + 1],
                                              adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        XCTAssertTrue(UIApplication.shared.sendAction(#selector(UIResponderStandardEditActions.paste(_:)),
                                                      to: harness.root, from: nil, for: nil))
        harness.expo.layoutIfNeeded()
        grid = try harness.grid()
        XCTAssertEqual(grid[2][0].paragraphs, richCell, "paste keeps the copied rich content: \(grid)")
        XCTAssertEqual(grid[2][1].paragraphs, [Acceptance.bodyText], "\(grid)")
        positions = try harness.positions()
        XCTAssertEqual(try harness.selectedCells(),
                       [Int(positions[Acceptance.tableColumns * 2]), Int(positions[Acceptance.tableColumns * 2 + 1])],
                       "paste selects the pasted rectangle")

        let rowAnchor = positions[Acceptance.tableColumns * 2]
        try harness.perform(Acceptance.addRowAfterLabel, onCellAt: rowAnchor)
        grid = try harness.grid()
        XCTAssertEqual(grid.count, Acceptance.tableRows + 1, "\(grid)")
        XCTAssertEqual(grid[3].map(\.paragraphs), Array(repeating: [""], count: Acceptance.tableColumns))
        positions = try harness.positions()
        try EditorTableInputTests.selectCells(anchor: positions[Acceptance.tableColumns * 3],
                                              head: positions[Acceptance.tableColumns * 3],
                                              adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        try harness.perform(Acceptance.deleteRowLabel, onCellAt: positions[Acceptance.tableColumns * 3])
        XCTAssertEqual(try harness.grid().count, Acceptance.tableRows)
        XCTAssertEqual(try harness.grid().prefix(3).map { $0.map(\.paragraphs) }, grid.prefix(3).map { $0.map(\.paragraphs) })

        positions = try harness.positions()
        try EditorTableInputTests.selectCells(anchor: positions[Acceptance.tableColumns - 1],
                                              head: positions[Acceptance.tableColumns - 1],
                                              adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        try harness.perform(Acceptance.addColumnAfterLabel, onCellAt: positions[Acceptance.tableColumns - 1])
        grid = try harness.grid()
        XCTAssertEqual(grid.map(\.count), Array(repeating: Acceptance.tableColumns + 1, count: Acceptance.tableRows), "\(grid)")
        XCTAssertEqual(grid[0][Acceptance.tableColumns].type, Acceptance.headerNode, "the header row stays a header row")
        positions = try harness.positions()
        let addedColumnHeader = positions[Acceptance.tableColumns]
        try EditorTableInputTests.selectCells(anchor: addedColumnHeader, head: addedColumnHeader,
                                              adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        try harness.perform(Acceptance.deleteColumnLabel, onCellAt: addedColumnHeader)
        grid = try harness.grid()
        XCTAssertEqual(grid.map(\.count), Array(repeating: Acceptance.tableColumns, count: Acceptance.tableRows), "\(grid)")
        let settled = grid

        positions = try harness.positions()
        let resizeTarget = positions[Acceptance.tableColumns * 2]
        try EditorTableInputTests.selectCells(anchor: resizeTarget, head: resizeTarget, adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        let firstColumn = try harness.presentedCell(positions[Acceptance.tableColumns])
        let edge = try harness.drawing.convert(CGPoint(x: firstColumn.bounds.maxX, y: firstColumn.bounds.midY),
                                               to: harness.view)
        let surface = try harness.surface
        XCTAssertTrue(surface.beginResizeDrag(at: edge))
        let dragged = CGPoint(x: edge.x + Acceptance.resizeDelta, y: edge.y)
        surface.updateResizeDrag(at: dragged)
        XCTAssertEqual(try harness.grid(), settled, "a resize preview never mutates the document")
        surface.endResizeDrag(at: dragged)
        grid = try harness.grid()
        let resizedWidth = try XCTUnwrap(grid[0][0].colwidth?.first, "\(grid)")
        XCTAssertGreaterThanOrEqual(resizedWidth, Acceptance.minimumResizedWidth)
        XCTAssertEqual(grid.map { $0[0].colwidth }, Array(repeating: [resizedWidth], count: Acceptance.tableRows),
                       "every cell of the column carries the width")
        XCTAssertEqual(try harness.presentedCell(try harness.positions()[0]).bounds.width, CGFloat(resizedWidth),
                       accuracy: Acceptance.geometryAccuracy)
        XCTAssertEqual(try harness.selectedCells(), [Int(try harness.positions()[Acceptance.tableColumns * 2])],
                       "a resize keeps the selected real cell")

        XCTAssertTrue(harness.root.applyUpdateJSON(try XCTUnwrap(adapter.undo())))
        harness.expo.layoutIfNeeded()
        XCTAssertEqual(try harness.grid(), settled, "one undo removes the whole resize")
        XCTAssertEqual(try harness.presentedCell(try harness.positions()[0]).bounds.width, firstColumn.bounds.width,
                       accuracy: Acceptance.geometryAccuracy, "undo restores the column geometry")
        XCTAssertTrue(harness.root.applyUpdateJSON(try XCTUnwrap(adapter.redo())))
        harness.expo.layoutIfNeeded()
        XCTAssertEqual(try harness.grid(), grid, "redo restores the resize")
        let resized = grid

        positions = try harness.positions()
        let lastCell = positions[positions.count - 1]
        let composing = try harness.bind(cell: lastCell)
        composing.setMarkedText(Acceptance.composedText, selectedRange: NSRange(location: 1, length: 0))
        XCTAssertTrue(composing.isComposing)
        let revisionBeforeScroll = adapter.baseDocumentRevision
        drainMainQueue()
        harness.root.contentOffset.y = max(0, harness.root.contentSize.height - harness.root.bounds.height)
        try harness.surface.updateGeometry(from: harness.root)
        harness.expo.layoutIfNeeded()
        let lastIndex = UInt32(positions.count - 1)
        let offscreenFrame = try XCTUnwrap(try harness.surface.cellFrame(tableID: try harness.tableID, cellIndex: lastIndex))
        XCTAssertFalse(offscreenFrame.intersects(try harness.surface.bounds),
                       "the active cell must have scrolled out of the viewport: \(offscreenFrame)")
        XCTAssertTrue(harness.view.activeTextInput === composing, "the offscreen active input stays pinned")
        XCTAssertTrue(composing.isComposing)
        XCTAssertTrue(composing.isFirstResponder)
        XCTAssertEqual(harness.activeCell(), lastCell)
        XCTAssertEqual(adapter.baseDocumentRevision, revisionBeforeScroll, "scrolling is not a mutation")
        composing.unmarkText()
        grid = try harness.grid()
        XCTAssertEqual(grid[2][2].paragraphs, [Acceptance.composedText], "the pinned composition lands in its cell")
        XCTAssertEqual(adapter.baseDocumentRevision, revisionBeforeScroll + 1)
        harness.root.contentOffset.y = 0
        try harness.surface.updateGeometry(from: harness.root)

        positions = try harness.positions()
        let activeBeforeRemote = try XCTUnwrap(harness.activeCell())
        try harness.applyRemote(["selection": [
            "type": "cell",
            "anchorCell": ["kind": "document", "offset": Int(activeBeforeRemote)],
            "headCell": ["kind": "document", "offset": Int(activeBeforeRemote)]
        ]]) { editorV2SetSelection(editorId: $0, requestJson: $1) }
        try harness.applyRemote(["command": ["type": Acceptance.deleteTableRows]]) {
            editorV2ApplyCommand(editorId: $0, requestJson: $1)
        }
        harness.deliverRemoteCommit()
        grid = try harness.grid()
        XCTAssertEqual(grid.count, Acceptance.tableRows - 1, "the remote peer removed the active cell's row: \(grid)")
        XCTAssertEqual(grid, Array(resized.prefix(Acceptance.tableRows - 1)))
        XCTAssertTrue(harness.view.activeTextInput === harness.root, "the dead cell releases the input")
        XCTAssertNil(harness.activeCell())
        let survivingSelection = try harness.selectedCells()
        XCTAssertTrue(survivingSelection.isSubset(of: Set(try harness.positions().map(Int.init))),
                      "only surviving real cells may stay selected: \(survivingSelection)")

        let tableBeforeDirection = try harness.documentJSON()
        positions = try harness.positions()
        let ltrFirst = try harness.presentedCell(positions[0]).bounds
        let ltrSecond = try harness.presentedCell(positions[1]).bounds
        XCTAssertLessThan(ltrFirst.minX, ltrSecond.minX)
        harness.view.tableDirection = .rightToLeft
        harness.expo.layoutIfNeeded()
        let rtlFirst = try harness.presentedCell(positions[0]).bounds
        let rtlSecond = try harness.presentedCell(positions[1]).bounds
        XCTAssertGreaterThan(rtlFirst.minX, rtlSecond.minX, "right-to-left mirrors the logical columns")
        XCTAssertEqual(rtlFirst.width, ltrFirst.width, accuracy: Acceptance.geometryAccuracy)
        XCTAssertEqual(try harness.documentJSON(), tableBeforeDirection, "direction is presentation only")

        let beforeRemount = try harness.documentJSON()
        harness.remount()
        XCTAssertEqual(try harness.documentJSON(), beforeRemount, "destroying the view never touches the document")
        positions = try harness.positions()
        XCTAssertEqual(positions.count, (Acceptance.tableRows - 1) * Acceptance.tableColumns)
        XCTAssertTrue(harness.view.activeTextInput === harness.root)
        try harness.presentedCell(positions[0])
        let rebound = try harness.bind(cell: positions[Acceptance.tableColumns])
        XCTAssertEqual(harness.activeCell(), positions[Acceptance.tableColumns])
        XCTAssertTrue(try harness.actionLabels(onCellAt: positions[Acceptance.tableColumns]).contains(Acceptance.addRowAfterLabel),
                      "table actions are available again after the rebind")
        rebound.insertText(Acceptance.composedText)
        try harness.perform(Acceptance.addRowAfterLabel, onCellAt: try harness.positions()[Acceptance.tableColumns])
        grid = try harness.grid()
        XCTAssertEqual(grid.count, Acceptance.tableRows, "\(grid)")
        XCTAssertEqual(grid[2].map(\.paragraphs), Array(repeating: [""], count: Acceptance.tableColumns))

        try export(adapter: adapter)
    }

    func testEditorAndViewerShareTableGeometryUnderOneThemeFontAndWidth() throws {
        let editorId = makeV2Editor(configJson: Acceptance.config)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let window = UIWindow(frame: CGRect(origin: .zero, size: Acceptance.editorSize))
        let view = RichTextEditorView(frame: window.bounds)
        window.addSubview(view)
        window.isHidden = false
        defer { window.isHidden = true }
        XCTAssertTrue(view.applyTheme(try XCTUnwrap(EditorTheme.from(json: Acceptance.parityTheme))))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(Acceptance.parityDocument))))
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let editorDrawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let viewerDocument = try compiledViewerDocument(Acceptance.parityDocument)

        for direction in [TableLayoutDirection.leftToRight, .rightToLeft] {
            view.tableDirection = direction
            view.layoutIfNeeded()
            let editorCells = try tableGeometry(editorDrawing)
            let viewerCells = try tableGeometry(try viewerDrawing(viewerDocument, direction: direction))
            XCTAssertEqual(editorCells.map(\.position), viewerCells.map(\.position), "\(direction)")
            for (editorCell, viewerCell) in zip(editorCells, viewerCells) {
                for (editorEdge, viewerEdge) in zip(editorCell.edges, viewerCell.edges) {
                    XCTAssertEqual(editorEdge, viewerEdge, accuracy: Acceptance.geometryAccuracy,
                                   "\(direction) cell \(editorCell.position): editor \(editorCell.edges) viewer \(viewerCell.edges)")
                }
            }
        }
    }

    func testRawIrregularImportProjectsWithoutRepairAndKeepsNativeActionsAcrossRebind() throws {
        let editorId = makeV2Editor(configJson: Acceptance.config)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let harness = Harness(editorId: editorId, adapter: adapter)
        defer { harness.close() }
        XCTAssertTrue(harness.root.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(Acceptance.irregularDocument))))
        harness.expo.layoutIfNeeded()
        let raw = try harness.grid()
        XCTAssertEqual(raw.map { $0.map(\.colspan).reduce(0, +) }, [3, 2, 4], "the import keeps its raw row widths: \(raw)")
        let record = try XCTUnwrap(adapter.cachedTableRecords[try harness.tableID])
        XCTAssertEqual(record["irregular"] as? Bool, true)
        XCTAssertFalse((record["syntheticRegions"] as? [Any] ?? []).isEmpty, "the projection fills the raw gaps")
        let revision = adapter.baseDocumentRevision

        let positions = try harness.positions()
        let shortRow = positions[Acceptance.irregularShortRowCell]
        try EditorTableInputTests.selectCells(anchor: shortRow, head: shortRow, adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        let published = try publishedActionLabels(adapter)
        XCTAssertEqual(try harness.actionLabels(onCellAt: shortRow), published,
                       "the native cell actions mirror the published toolbar state")
        XCTAssertEqual(try harness.grid(), raw, "rendering and selecting never repair the raw table")
        XCTAssertEqual(adapter.baseDocumentRevision, revision)

        harness.remount()
        XCTAssertEqual(try harness.grid(), raw, "rebinding never repairs the raw table")
        XCTAssertEqual(adapter.baseDocumentRevision, revision)
        try EditorTableInputTests.selectCells(anchor: shortRow, head: shortRow, adapter: adapter, view: harness.view)
        harness.expo.layoutIfNeeded()
        XCTAssertEqual(try harness.actionLabels(onCellAt: shortRow), published,
                       "the same actions are offered after the rebind")

        let input = try harness.bind(cell: shortRow)
        harness.place(input, range: NSRange(location: 0, length: 0))
        input.insertText(Acceptance.composedText)
        let typed = try harness.grid()
        XCTAssertEqual(typed[1][1].paragraphs, [Acceptance.composedText + "Short row"],
                       "a uniquely anchored real cell stays typeable: \(typed)")
        XCTAssertEqual(typed.map { $0.map(\.colspan).reduce(0, +) }, [3, 2, 4], "typing never normalizes the grid")
        XCTAssertEqual(adapter.baseDocumentRevision, revision + 1)
    }

    private struct CellGeometry {
        let position: Int
        let edges: [CGFloat]
    }

    private func tableGeometry(_ drawing: PreparedProseDrawingView) throws -> [CellGeometry] {
        let cells = try XCTUnwrap(drawing.mountedTablePresentation()?.cells.filter { $0.cell.sourceCellIndex != nil })
            .sorted { $0.sourcePosition < $1.sourcePosition }
        let originX = try XCTUnwrap(cells.map(\.bounds.minX).min())
        let originY = try XCTUnwrap(cells.map(\.bounds.minY).min())
        return cells.map { cell in
            CellGeometry(position: cell.sourcePosition, edges: [
                cell.bounds.minX - originX, cell.bounds.minY - originY, cell.bounds.width, cell.bounds.height
            ])
        }
    }

    private func compiledViewerDocument(_ source: String) throws -> ViewerDocument {
        var compiled = viewerCompile(request: FfiViewerCompileRequest(
            sourceKind: .json, source: source, configJson: Acceptance.config, imagesEnabled: true, mentionPrefix: nil
        ))
        XCTAssertNil(compiled.error, "the viewer refused the document: \(String(describing: compiled.error))")
        let document = try ViewerDocument(compiled: try XCTUnwrap(compiled.value))
        compiled.value = nil
        return document
    }

    private func viewerDrawing(_ document: ViewerDocument, direction: TableLayoutDirection) throws -> PreparedProseDrawingView {
        var theme = PreparedProseTheme.resolve(themeJSON: Acceptance.parityTheme)
        theme.tableDirection = direction
        let themed = document.withPreparedTheme(theme)
        let width = Acceptance.editorSize.width
        let displayScale = UIScreen.main.scale
        let key = ProseLayoutKey(
            semanticKey: themed.semanticKey, widthPixels: Int(width * displayScale),
            themeDigest: Acceptance.parityLayoutKey, nativeFontRevision: 0, fontEnvironmentRevision: 0,
            displayScale: displayScale, attachmentRevision: 0,
            generationIdentity: Acceptance.parityLayoutKey, semanticGenerationIdentity: Acceptance.parityLayoutKey
        )
        let layout = try CoreTextProseLayoutEngine().prepare(document: themed, key: key, widthPoints: width,
                                                             displayScale: displayScale)
        let drawing = PreparedProseDrawingView(frame: CGRect(origin: .zero, size: layout.size))
        drawing.install(layout: layout)
        return drawing
    }

    private func publishedActionLabels(_ adapter: EditorV2Adapter) throws -> [String] {
        let commands = try XCTUnwrap(adapter.cachedActiveState?["commands"] as? [String: Any])
        return TableAccessibilityAction.all.filter { commands[$0.applicability] as? Bool == true }.map(\.label)
    }

    private func drainMainQueue() {
        let drained = expectation(description: "drain main queue")
        DispatchQueue.main.async { drained.fulfill() }
        wait(for: [drained], timeout: Acceptance.mainQueueDrainTimeout)
    }

    private func seedDocument() throws -> String {
        let trailing = (0..<Acceptance.trailingParagraphs).map { index -> [String: Any] in
            ["type": Acceptance.paragraphNode, "content": [["type": "text", "text": "\(Acceptance.trailingText) \(index)"]]]
        }
        let document: [String: Any] = [
            "type": "doc",
            "content": [["type": Acceptance.paragraphNode, "content": [["type": "text", "text": Acceptance.introText]]]]
                + trailing
        ]
        return try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: document), encoding: .utf8))
    }

    private func export(adapter: EditorV2Adapter) throws {
        let exported = editorV2SnapshotExport(editorId: adapter.editorId)
        XCTAssertNil(exported.error, "snapshot export failed: \(String(describing: exported.error))")
        let snapshot = try XCTUnwrap(exported.value)
        let payload: [String: Any] = [
            "platform": "ios",
            "documentJson": try XCTUnwrap(JSONSerialization.jsonObject(with: Data(try XCTUnwrap(adapter.documentJson()).utf8))),
            "encodedStateBase64": snapshot.encodedState.base64EncodedString(),
            "metadata": try XCTUnwrap(JSONSerialization.jsonObject(with: Data(snapshot.metadataJson.utf8)))
        ]
        let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = Acceptance.exportFileName
        attachment.lifetime = .keepAlways
        add(attachment)
        let directory = ProcessInfo.processInfo.environment[Acceptance.exportDirectoryKey]
            .map { URL(fileURLWithPath: $0, isDirectory: true) } ?? FileManager.default.temporaryDirectory
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try data.write(to: directory.appendingPathComponent(Acceptance.exportFileName))
    }
}
