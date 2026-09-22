import XCTest

final class EditorTableInputTests: XCTestCase {
    private let tableConfig = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
    private let listTableConfig = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"bulletList","content":"listItem+","group":"block","role":"list"},{"name":"listItem","content":"block+","role":"listItem"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#

    func testHostRoutesCellEditThroughRootAdapterUsingGeneratedSnapshotMapping() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let update = try XCTUnwrap(adapter.setContentJson(document))
        XCTAssertTrue(view.textView.applyUpdateJSON(update))

        XCTAssertNotNil(adapter.cachedTableInputMappings, "rebuilt engine must publish the snapshot sidecar")
        XCTAssertTrue(view.bindTableCell(tableID: "t8", cellIndex: 1, contentRect: CGRect(x: 8, y: 8, width: 140, height: 40)))
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, 13)
        view.activeTextInput.insertText("!")

        let documentJSON = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(documentJSON.contains(#""text":"!second""#), documentJSON)
        XCTAssertTrue(documentJSON.contains(#""text":"first""#), documentJSON)
        XCTAssertTrue(view.textView.ownsNativeBinding(adapter))
        XCTAssertFalse(view.activeTextInput.ownsNativeBinding(adapter))
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, 14)
        view.activeTextInput.insertText("😀")

        let secondDocumentJSON = try XCTUnwrap(adapter.documentJson())
        XCTAssertTrue(secondDocumentJSON.contains(#""text":"!😀second""#), secondDocumentJSON)
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, 15)
    }

    func testHostRejectsTableCellBindingWhenAnotherHostOwnsTheSession() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let owner = RichTextEditorView(frame: .zero)
        let stale = RichTextEditorView(frame: .zero)
        owner.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(owner.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))

        stale.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)

        XCTAssertTrue(owner.textView.ownsNativeBinding(adapter))
        XCTAssertFalse(stale.textView.ownsNativeBinding(adapter))
        XCTAssertFalse(stale.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
    }

    func testReturnedSelectionOutsideCellInvalidatesInput() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"outside"}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let cell = view.activeTextInput
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: 6, scalarHead: 6)
        XCTAssertTrue(cell.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(cell.editorId, 0)
        let before = try XCTUnwrap(adapter.documentJson())
        cell.insertText("!")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
    }

    func testInvalidationClearsUIKitCompositionAndResignsCellInput() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        window.rootViewController = UIViewController()
        window.makeKeyAndVisible()
        window.rootViewController?.view.addSubview(view)
        defer { window.isHidden = true }
        view.layoutIfNeeded()
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: CGRect(x: 0, y: 0, width: 140, height: 50)))
        let cell = view.activeTextInput
        XCTAssertTrue(cell.becomeFirstResponder())
        cell.setMarkedText("draft", selectedRange: NSRange(location: 5, length: 0))
        XCTAssertNotNil(cell.markedTextRange)
        let before = try XCTUnwrap(adapter.documentJson())

        view.invalidateTableCellBinding()

        XCTAssertNil(cell.markedTextRange)
        XCTAssertFalse(cell.isFirstResponder)
        XCTAssertFalse(cell.isComposing)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.activeTextInput === cell)
        XCTAssertNil(cell.markedTextRange)
    }

    func testHostRebindInvalidatesRetainedTableCellInput() throws {
        let firstEditorId = makeV2Editor(configJson: tableConfig)
        let secondEditorId = makeV2Editor(configJson: tableConfig)
        defer {
            destroyV2Editor(id: firstEditorId)
            destroyV2Editor(id: secondEditorId)
        }
        let firstAdapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: firstEditorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"old"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: firstEditorId, initialUpdateJSON: try XCTUnwrap(firstAdapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(firstAdapter.setContentJson(document))))
        let tableID = try XCTUnwrap(firstAdapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCellInput = view.activeTextInput
        XCTAssertNotNil(staleCellInput.tableCellPositionMap)
        XCTAssertNotNil(staleCellInput.onProjectedUpdate)

        view.bindEditor(id: 0, initialUpdateJSON: nil)
        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(staleCellInput.editorId, 0)
        XCTAssertNil(staleCellInput.tableCellPositionMap)
        XCTAssertNil(staleCellInput.onProjectedUpdate)

        view.bindEditor(id: secondEditorId, initialUpdateJSON: try XCTUnwrap(
            EditorV2Registry.adapter(forLegacyId: secondEditorId)?.initialUpdateJSON()
        ))
        staleCellInput.insertText("!")

        XCTAssertFalse(try XCTUnwrap(firstAdapter.documentJson()).contains("!old"))
        XCTAssertFalse(try XCTUnwrap(
            EditorV2Registry.adapter(forLegacyId: secondEditorId)?.documentJson()
        ).contains("!"))
    }

    func testRetainedComposingCellCannotCommitAfterExpoTakesNativeOwnership() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let originalHost = RichTextEditorView(frame: .zero)
        originalHost.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(originalHost.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(originalHost.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCellInput = originalHost.activeTextInput
        staleCellInput.setMarkedText("!", selectedRange: NSRange(location: 1, length: 0))
        XCTAssertTrue(staleCellInput.isComposing)
        XCTAssertNotNil(staleCellInput.markedTextReplacementScalarRange)
        let documentJSONBeforeTakeover = try XCTUnwrap(adapter.documentJson())

        let expoHost = NativeEditorExpoView()
        defer { expoHost.setEditorId(0) }
        expoHost.setEditorId(editorId)

        XCTAssertFalse(originalHost.textView.ownsNativeBinding(adapter))
        XCTAssertTrue(expoHost.ownsNativeBinding(editorId: editorId))
        XCTAssertTrue(expoHost.richTextView.bindTableCell(
            tableID: tableID,
            cellIndex: 0,
            contentRect: .zero
        ))
        staleCellInput.unmarkText()

        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), documentJSONBeforeTakeover)
    }

    func testExpoDestroyInvalidatesRetainedTableCellInput() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        var destroyed = false
        defer {
            if !destroyed {
                destroyV2Editor(id: editorId)
            }
        }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let expoHost = NativeEditorExpoView()
        expoHost.setEditorId(editorId)
        XCTAssertTrue(expoHost.richTextView.textView.applyUpdateJSON(
            try XCTUnwrap(adapter.setContentJson(document))
        ))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(expoHost.richTextView.bindTableCell(
            tableID: tableID,
            cellIndex: 0,
            contentRect: .zero
        ))
        let staleCellInput = expoHost.richTextView.activeTextInput

        NativeEditorViewRegistry.shared.invalidateDestroyedEditor(editorId: editorId)
        destroyV2Editor(id: editorId)
        destroyed = true

        XCTAssertEqual(expoHost.richTextView.editorId, 0)
        XCTAssertEqual(staleCellInput.editorId, 0)
        XCTAssertNil(staleCellInput.tableCellPositionMap)
        XCTAssertNil(staleCellInput.onProjectedUpdate)
    }

    func testStaleComposingCellCannotCommitStoredRangeAfterNativeRevisionChanges() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let initialDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let replacementDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"replacement"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(initialDocument))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCellInput = view.activeTextInput
        staleCellInput.setMarkedText("!", selectedRange: NSRange(location: 1, length: 0))
        XCTAssertTrue(staleCellInput.isComposing)
        XCTAssertNotNil(staleCellInput.markedTextReplacementScalarRange)

        _ = try XCTUnwrap(adapter.setContentJson(replacementDocument))
        let documentJSONAfterNativeRevision = try XCTUnwrap(adapter.documentJson())
        staleCellInput.unmarkText()

        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), documentJSONAfterNativeRevision)
    }

    func testFullContextProjectionMapsTwoParagraphsAndAnEmptyParagraph() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]},{"type":"paragraph"},{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let mapping = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID])
        let table = try XCTUnwrap(adapter.cachedTableRecords[tableID])
        let projection = try XCTUnwrap(EditorTableInputCoordinator.projection(
            cellIndex: 0,
            table: table,
            mapping: mapping,
            documentRevision: adapter.baseDocumentRevision,
            positionEpoch: try XCTUnwrap(adapter.positionEpoch),
            baseFont: view.textView.baseFont,
            textColor: view.textView.baseTextColor,
            theme: view.textView.theme,
            atomConfiguration: view.textView.atomRenderConfiguration
        ))

        XCTAssertEqual(projection.text.string.replacingOccurrences(of: "\u{200B}", with: ""), "one\n\ntwo")
        XCTAssertEqual(projection.positionMap.segments.count, 3)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertEqual(view.activeTextInput.inputScalarRange(fromLocal: 0, toLocal: 9)?.from, 0)
        XCTAssertEqual(view.activeTextInput.inputScalarRange(fromLocal: 0, toLocal: 9)?.to, 9)
    }

    func testCellAppliesAndPreservesBackwardGlobalSelection() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"alpha"}]}]}]}]}]}"#
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let map = try XCTUnwrap(view.activeTextInput.tableCellPositionMap)
        let cellStart = try XCTUnwrap(map.globalScalar(forLocalScalar: 0))
        let cellEnd = try XCTUnwrap(map.globalScalar(forLocalScalar: 5))

        _ = view.activeTextInput.applySelectionFromJSON([
            "type": "text",
            "anchor": NSNumber(value: cellEnd),
            "head": NSNumber(value: cellStart),
            "anchorScalar": NSNumber(value: cellEnd),
            "headScalar": NSNumber(value: cellStart)
        ])

        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.anchor, cellEnd)
        XCTAssertEqual(view.activeTextInput.currentLogicalScalarSelection()?.head, cellStart)
    }

    func testListCellMapsTextEndpointsAcrossTwoItems() throws {
        let editorId = makeV2Editor(configJson: listTableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}]}]}"#
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let mapping = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID])
        let table = try XCTUnwrap(adapter.cachedTableRecords[tableID])
        let projection = try XCTUnwrap(EditorTableInputCoordinator.projection(
            cellIndex: 0,
            table: table,
            mapping: mapping,
            documentRevision: adapter.baseDocumentRevision,
            positionEpoch: try XCTUnwrap(adapter.positionEpoch),
            baseFont: view.textView.baseFont,
            textColor: view.textView.baseTextColor,
            theme: view.textView.theme,
            atomConfiguration: view.textView.atomRenderConfiguration
        ))
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let start = PositionBridge.utf16OffsetToScalar(0, in: view.activeTextInput)
        let end = PositionBridge.utf16OffsetToScalar(view.activeTextInput.attributedText.length, in: view.activeTextInput)

        XCTAssertNotNil(
            view.activeTextInput.inputScalarRange(fromLocal: start, toLocal: end),
            "local=\(start)...\(end) segments=\(projection.positionMap.segments) text=\(projection.text.string.debugDescription)"
        )
    }

    func testPositionMapConvertsEmojiUtf16IntoCurrentGlobalScalarRange() throws {
        let map = TableCellPositionMap(
            binding: .init(cellSourcePosition: 10, documentRevision: 4, positionEpoch: 9),
            segments: [.init(localScalarRange: 0..<5, globalScalarStart: 40)]
        )

        XCTAssertEqual(map.globalScalar(forLocalUTF16: 3, in: "a😀bc"), 42)
        let range = try XCTUnwrap(
            map.globalScalarRange(forLocalUTF16: NSRange(location: 1, length: 2), in: "a😀bc")
        )
        XCTAssertEqual(range.0, 41)
        XCTAssertEqual(range.1, 42)
    }

    func testPositionMapRejectsStaleAndNestedOrSyntheticTargets() {
        let binding = TableCellPositionMap.Binding(
            cellSourcePosition: 10,
            documentRevision: 4,
            positionEpoch: 9
        )
        let map = TableCellPositionMap(
            binding: binding,
            segments: [.init(localScalarRange: 0..<2, globalScalarStart: 40)]
        )

        XCTAssertNil(map.globalScalar(forLocalScalar: 1, currentRevision: 5, currentEpoch: 9))
        XCTAssertFalse(EditorTableInputCoordinator.canBind(
            .init(binding: binding, isSynthetic: true, isNestedTarget: false)
        ))
        XCTAssertFalse(EditorTableInputCoordinator.canBind(
            .init(binding: binding, isSynthetic: false, isNestedTarget: true)
        ))
    }

    func testCoordinatorReusesOneInputAcrossThreeCellBindings() {
        let coordinator = EditorTableInputCoordinator()
        let input = coordinator.cellInput
        let target = { (position: UInt32) in
            EditorTableInputCoordinator.Target(
                binding: .init(cellSourcePosition: position, documentRevision: 4, positionEpoch: 9),
                isSynthetic: false,
                isNestedTarget: false
            )
        }

        XCTAssertTrue(coordinator.bind(target(10), text: NSAttributedString(string: "one"), positionMap: .init(binding: target(10).binding, segments: [.init(localScalarRange: 0..<4, globalScalarStart: 10)])))
        XCTAssertTrue(coordinator.bind(target(20), text: NSAttributedString(string: "two"), positionMap: .init(binding: target(20).binding, segments: [.init(localScalarRange: 0..<4, globalScalarStart: 20)])))
        XCTAssertTrue(coordinator.bind(target(30), text: NSAttributedString(string: "three"), positionMap: .init(binding: target(30).binding, segments: [.init(localScalarRange: 0..<6, globalScalarStart: 30)])))

        XCTAssertTrue(coordinator.cellInput === input)
        XCTAssertEqual(coordinator.inputInstanceCountForTesting, 1)
        XCTAssertEqual(coordinator.phase, .bound(cellSourcePos: 30, documentRevision: "4", positionEpoch: "9"))
    }
}
