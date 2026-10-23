import UIKit
import XCTest

extension EditorV2AdapterTests {
    func testCommandsRouteThroughTypedV2Transactions() {
        let adapter = makeAdapter()
        let spy = ErrorSpy()
        adapter.onAutonomousError = spy.record
        _ = adapter.setContentHtml("<p>ab</p>")

        let bold = adapter.toggleMark("bold", anchor: 0, head: 2)
        XCTAssertNotNil(bold)
        let doc = documentJsonObject(adapter)
        let paragraph = (doc["content"] as? [[String: Any]])?.first
        let textNode = (paragraph?["content"] as? [[String: Any]])?.first
        XCTAssertEqual((textNode?["marks"] as? [[String: Any]])?.first?["type"] as? String, "bold")

        let heading = adapter.toggleHeading(level: 2, anchor: 1, head: 1)
        XCTAssertNotNil(heading)
        let headedDoc = documentJsonObject(adapter)
        let headingNode = (headedDoc["content"] as? [[String: Any]])?.first
        XCTAssertEqual(headingNode?["type"] as? String, "heading")
        XCTAssertEqual((headingNode?["attrs"] as? [String: Any])?["level"] as? Int, 2)

        // list_item's content expression rejects headings: wrap a fresh
        // paragraph instead.
        _ = adapter.setContentHtml("<p>ab</p>")
        let list = adapter.wrapInList(listType: "bullet_list", itemType: "list_item", anchor: 1, head: 1)
        XCTAssertNotNil(list)
        let listDoc = documentJsonObject(adapter)
        XCTAssertEqual((listDoc["content"] as? [[String: Any]])?.first?["type"] as? String, "bullet_list")

        let hardBreak = adapter.insertNode("hard_break", anchor: 1, head: 1)
        XCTAssertNotNil(hardBreak)

        _ = adapter.setContentHtml("<p>ab</p>")
        let quote = adapter.toggleBlockquote(anchor: 1, head: 1)
        XCTAssertNotNil(quote)
        let code = adapter.toggleCodeBlock(anchor: 1, head: 1)
        XCTAssertNotNil(code)
    }

    func testPasteRoutesThroughV2() {
        let adapter = makeAdapter()
        let spy = ErrorSpy()
        adapter.onAutonomousError = spy.record
        _ = adapter.setContentHtml("<p>ab</p>")

        let html = adapter.insertContentHtml("<strong>CD</strong>", anchor: 2, head: 2)
        XCTAssertEqual(renderedText(html), "abCD")

        // Plain-text paste with newlines goes through insertContentJson on the
        // composed fragment (the view builds the fragment; the adapter just
        // routes it typed).
        let fragment = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"X"}]},{"type":"paragraph","content":[{"type":"text","text":"Y"}]}]}"#
        let json = adapter.insertContentJson(fragment, anchor: 4, head: 4)
        XCTAssertNotNil(json)
        XCTAssertEqual(documentText(adapter), "abCDXY")
    }

    func testVoidAndListMarkerPositionMapping() {
        let adapter = makeAdapter()
        let spy = ErrorSpy()
        adapter.onAutonomousError = spy.record
        let doc = #"{"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.com/a.png","alt":null,"title":null,"width":null,"height":null}},{"type":"bullet_list","content":[{"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"item"}]}]}]}]}"#
        _ = adapter.setContentJson(doc)
        XCTAssertEqual(documentText(adapter), "item")

        // Type inside the list item paragraph: the image void is scalar 0,
        // the block separator is scalar 1, "item" text starts at scalar 2.
        let update = adapter.insertText("X", atScalar: 2)
        XCTAssertEqual(renderedText(update), "Xitem")

        // Atom boundaries cannot host a caret, so typing there is a no-op.
        let boundaryInsert = adapter.insertText("Z", atScalar: 1)
        XCTAssertEqual(renderedText(boundaryInsert), "Xitem")
        XCTAssertNil(spy.last)
        XCTAssertEqual(documentText(adapter), "Xitem")
        let after = documentJsonObject(adapter)
        XCTAssertEqual((after["content"] as? [[String: Any]])?.first?["type"] as? String, "image")
        XCTAssertEqual((after["content"] as? [[String: Any]])?.last?["type"] as? String, "bullet_list")

        // Resize the void image by its document position.
        let resized = adapter.resizeImage(atDocPos: 0, width: 120, height: 80)
        XCTAssertNotNil(resized)
        let resizedDoc = documentJsonObject(adapter)
        let image = (resizedDoc["content"] as? [[String: Any]])?.first
        let attrs = image?["attrs"] as? [String: Any]
        XCTAssertEqual(attrs?["width"] as? Int, 120)
        XCTAssertEqual(attrs?["height"] as? Int, 80)
    }

    func testLocalOnlyMutationRequiresNoTransportOwner() {
        let adapter = makeAdapter()
        XCTAssertNotNil(adapter.insertText("x", atScalar: 0))
        XCTAssertEqual(documentText(adapter), "x")
    }

    func testSynthesizedUpdateCarriesRustStateNotFabricatedState() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>ab</p>")
        let update = parseObject(adapter.insertText("c", atScalar: 2))

        XCTAssertEqual(update["documentVersion"] as? String, "2")
        let history = historyState(adapter.insertText("d", atScalar: 3))
        XCTAssertEqual(history["canUndo"] as? Bool, true)
        XCTAssertEqual(history["canRedo"] as? Bool, false)
        // Toolbar state derives from the authoritative document+selection.
        let state = parseObject(adapter.currentStateJSON())
        XCTAssertNotNil(state["activeState"])
        XCTAssertEqual(state["documentVersion"] as? String, "3")
    }

    func testStructuredErrorEnvelopeFields() {
        let adapter = makeAdapter(
            configJson: #"{"initialization":{"type":"localEmpty"},"policy":{"readOnly":true}}"#
        )
        _ = adapter.setContentHtml("<p>seed</p>")
        let spy = ErrorSpy()
        adapter.onAutonomousError = spy.record
        XCTAssertNil(adapter.insertText("x", atScalar: 1))
        let error = spy.last
        XCTAssertEqual(error?.domain, "boundary")
        XCTAssertEqual(error?.code, "MUTATION_REJECTED")
        XCTAssertFalse(error?.message.isEmpty ?? true)
        XCTAssertNotNil(error?.requestId)
        XCTAssertNotNil(UInt64(error?.requestId ?? ""), "requestId is a decimal string")
        XCTAssertNil(error?.operationIndex)
        XCTAssertNil(error?.limit)
        XCTAssertNil(error?.actual)
    }

    /// Golden fixture assertions against the accessor alone (these survive
    /// the probe removal): rendered text, scalar extent, active state at a
    /// mirrored mark selection, and backward-selection doc resolution.
    func testRenderAccessorFixtureMatrixGoldenContent() {
        // Empty doc: one empty block, a one-scalar extent (the empty
        // paragraph's synthetic placeholder), and the authoritative initial
        // text selection carried by the locked render snapshot.
        let empty = makeFixtureAdapter("")
        let emptyUpdate = parseObject(editorV2RenderUpdate(editorId: empty.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        XCTAssertNotNil(emptyUpdate["renderBlocks"] as? NSArray)
        XCTAssertEqual((emptyUpdate["scalarLength"] as? NSNumber)?.uint32Value, 1)
        let emptySelection = emptyUpdate["selection"] as? [String: Any]
        XCTAssertEqual(emptySelection?["type"] as? String, "text")
        XCTAssertEqual((emptySelection?["anchor"] as? NSNumber)?.uint32Value, 1)
        XCTAssertEqual((emptySelection?["head"] as? NSNumber)?.uint32Value, 1)
        XCTAssertNotNil(emptyUpdate["activeState"] as? NSDictionary)

        // Emoji: the scalar extent counts every Unicode scalar (a, emoji,
        // e + combining acute, 7-scalar ZWJ family, b = 12).
        let emoji = makeFixtureAdapter(Self.accessorFixtures.first { $0.0 == "emoji" }!.1)
        let emojiUpdate = parseObject(editorV2RenderUpdate(editorId: emoji.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        XCTAssertEqual((emojiUpdate["scalarLength"] as? NSNumber)?.uint32Value, 12)

        // Marks: mirroring the bold run activates the bold toolbar mark.
        let marks = makeFixtureAdapter(Self.accessorFixtures.first { $0.0 == "marks" }!.1)
        let marksUpdate = parseObject(editorV2RenderUpdate(editorId: marks.editorId, mirrorScalarAnchor: 6, mirrorScalarHead: 10).value)
        let activeState = marksUpdate["activeState"] as? [String: Any]
        let activeMarks = activeState?["marks"] as? [String: Any]
        XCTAssertEqual(activeMarks?["bold"] as? Bool, true, "mirror over the bold run activates bold")

        // Multi-block: extent counts the block separator as one scalar; a
        // backward mirror resolves each side to its doc position.
        let multi = makeFixtureAdapter(Self.accessorFixtures.first { $0.0 == "multi-block" }!.1)
        let multiUpdate = parseObject(editorV2RenderUpdate(editorId: multi.editorId, mirrorScalarAnchor: 4, mirrorScalarHead: 1).value)
        XCTAssertEqual((multiUpdate["scalarLength"] as? NSNumber)?.uint32Value, 5)
        let selection = multiUpdate["selection"] as? [String: Any]
        XCTAssertEqual((selection?["anchorScalar"] as? NSNumber)?.uint32Value, 4)
        XCTAssertEqual((selection?["headScalar"] as? NSNumber)?.uint32Value, 1)
        XCTAssertEqual((selection?["anchor"] as? NSNumber)?.uint32Value, 6)
        XCTAssertEqual((selection?["head"] as? NSNumber)?.uint32Value, 2)

        // Void nodes: the image void occupies one scalar; text starts after
        // the block separator.
        let void = makeFixtureAdapter(Self.accessorFixtures.first { $0.0 == "void-nodes" }!.1)
        let voidUpdate = parseObject(editorV2RenderUpdate(editorId: void.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        XCTAssertEqual((voidUpdate["scalarLength"] as? NSNumber)?.uint32Value, 7)
        let voidBlocks = voidUpdate["renderBlocks"] as? [[[String: Any]]]
        let voidText = (voidBlocks ?? []).flatMap { $0 }
            .compactMap { ($0["type"] as? String) == "textRun" ? ($0["text"] as? String) : nil }
            .joined()
        XCTAssertEqual(voidText, "after")
    }

    /// Wrapping a line in a list must not drag the caret backwards.
    ///
    /// `commandAtSelection` mirrors the post-command render update at the
    /// caret's *pre-command* scalar offsets. That is right for a mark toggle,
    /// which moves no text, but a structural wrap inserts the list, item, and
    /// paragraph opening tokens in front of the line — every offset in it
    /// shifts by two. Mirroring the old offsets pins the caret two characters
    /// short of where the user left it.
    func testWrappingALineInAListKeepsTheCaretAtTheEndOfTheText() {
        let adapter = makeAdapter()
        _ = adapter.insertText("one", atScalar: 0)

        let beforeUpdate = parseObject(
            editorV2RenderUpdate(
                editorId: adapter.editorId,
                mirrorScalarAnchor: nil,
                mirrorScalarHead: nil
            ).value
        )
        let beforeSelection = beforeUpdate["selection"] as? [String: Any]
        XCTAssertEqual(
            (beforeSelection?["anchorScalar"] as? NSNumber)?.uint32Value,
            3,
            "precondition: the caret sits at the end of the bare line"
        )

        let updateJSON = adapter.wrapInList(
            listType: "bullet_list",
            itemType: "list_item",
            anchor: 3,
            head: 3
        )
        let update = parseObject(updateJSON)
        let selection = update["selection"] as? [String: Any]

        XCTAssertEqual(
            (selection?["anchorScalar"] as? NSNumber)?.uint32Value,
            5,
            "the caret must land at the end of the wrapped line, not at the "
                + "offset it held before the list tokens were inserted"
        )
        XCTAssertEqual(
            (selection?["headScalar"] as? NSNumber)?.uint32Value,
            5,
            "the caret must stay collapsed at the end of the wrapped line"
        )
    }

    /// The same shift applies to a caret parked mid-word, so a fix cannot get
    /// away with pinning the caret to the end of the line.
    func testWrappingALineInAListKeepsAMidWordCaretOnTheSameCharacter() {
        let adapter = makeAdapter()
        _ = adapter.insertText("one", atScalar: 0)

        let updateJSON = adapter.wrapInList(
            listType: "bullet_list",
            itemType: "list_item",
            anchor: 1,
            head: 1
        )
        let update = parseObject(updateJSON)
        let selection = update["selection"] as? [String: Any]

        XCTAssertEqual(
            (selection?["anchorScalar"] as? NSNumber)?.uint32Value,
            3,
            "a caret one character into the line must still be one character "
                + "in after the wrap"
        )
    }

    /// A render update that fails at the engine used to return nil in silence,
    /// so no caller could name the cause. The adapter reports it like any
    /// other boundary failure.
    func testRefreshReportsAFailedEngineRenderUpdate() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        let spy = ErrorSpy()
        adapter.onAutonomousError = spy.record

        // Destroy the engine session behind the adapter's back: the next
        // render update reaches a handle the registry no longer knows.
        _ = editorV2Destroy(editorId: adapter.editorId)

        XCTAssertNil(adapter.refreshFromRustState(mirrorSelection: nil))
        XCTAssertEqual(spy.errors.count, 1, "the failure is reported exactly once")
        XCTAssertEqual(spy.last?.domain, "lifecycle")
    }

    private struct SelectionRenderObservation {
        let update: NSDictionary
        let atomicRender: NSDictionary
        let cacheState: NSDictionary
        let presentationRevision: UInt64?
        let presentationBaseRevision: UInt64?
        let presentationChanges: TableFrameChanges?
        let lastSyncedSelection: [UInt32]?
    }

    private func observeSelectionRender(_ adapter: EditorV2Adapter, update: String) -> SelectionRenderObservation {
        SelectionRenderObservation(
            update: parseObject(update) as NSDictionary,
            atomicRender: parseObject(adapter.cachedAtomicRenderJSON) as NSDictionary,
            cacheState: parseObject(adapter.cacheStateForTesting).filter { $0.key != "viewUpdateJSON" } as NSDictionary,
            presentationRevision: adapter.cachedTablePresentation?.documentRevision,
            presentationBaseRevision: adapter.cachedTablePresentation?.baseDocumentRevision,
            presentationChanges: adapter.cachedTablePresentation?.changes,
            lastSyncedSelection: adapter.lastSyncedScalarSelection.map { [$0.anchor, $0.head] }
        )
    }

    private func assertSameSelectionRender(
        _ fast: SelectionRenderObservation,
        _ full: SelectionRenderObservation,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        XCTAssertEqual(fast.update, full.update, "view update\nfast: \(fast.update)\nfull: \(full.update)", file: file, line: line)
        XCTAssertEqual(fast.atomicRender, full.atomicRender,
            "cached atomic render\nfast: \(fast.atomicRender)\nfull: \(full.atomicRender)", file: file, line: line)
        XCTAssertEqual(fast.cacheState, full.cacheState,
            "adapter caches\nfast: \(fast.cacheState)\nfull: \(full.cacheState)", file: file, line: line)
        XCTAssertEqual(fast.presentationRevision, full.presentationRevision, file: file, line: line)
        XCTAssertEqual(fast.presentationBaseRevision, full.presentationBaseRevision, file: file, line: line)
        XCTAssertEqual(fast.presentationChanges, full.presentationChanges, file: file, line: line)
        XCTAssertEqual(fast.lastSyncedSelection, full.lastSyncedSelection, file: file, line: line)
    }

    private func authoritativeSelectionState(_ adapter: EditorV2Adapter) throws -> [String: Any] {
        let result = editorV2RenderNativeFrame(
            editorId: adapter.editorId, ownerId: nil, mirrorScalarAnchor: nil, mirrorScalarHead: nil
        )
        XCTAssertNil(result.error)
        let snapshot = parseObject(try XCTUnwrap(result.frame).snapshotJson)
        return snapshot.filter { EditorV2Adapter.renderStateKeys.contains($0.key) }
    }

    private func assertSelectionStateAdoptionMatchesFullRender(
        _ adapter: EditorV2Adapter,
        outcome: EditorV2Adapter.NativeIntentOutcome,
        file: StaticString = #filePath,
        line: UInt = #line
    ) throws {
        let selectionState = try authoritativeSelectionState(adapter)
        let fastOutcome = EditorV2Adapter.NativeIntentOutcome(
            changed: outcome.changed, documentChanged: outcome.documentChanged, selectionState: selectionState,
            unappliedSelection: nil
        )
        let fullOutcome = EditorV2Adapter.NativeIntentOutcome(
            changed: outcome.changed, documentChanged: outcome.documentChanged, selectionState: nil,
            unappliedSelection: nil
        )
        let tableIndex = adapter.tableIndex
        let retainedBlocks = adapter.cachedSemanticRenderBlocks.map { $0 as NSArray }
        let fullAdoptions = adapter.fullFrameAdoptionCountForTesting
        let deltaAdoptions = adapter.deltaFrameAdoptionCountForTesting
        let positionEpoch = adapter.positionEpoch
        let frameFetches = adapter.renderUpdateCallCountForTesting
        let fast = try XCTUnwrap(adapter.renderNativeIntentOutcome(fastOutcome), "selection state adoption", file: file, line: line)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, frameFetches,
            "a valid selection state must not fetch a native frame", file: file, line: line)
        XCTAssertTrue(adapter.tableIndex === tableIndex, "selection state must keep the installed table index", file: file, line: line)
        XCTAssertEqual(adapter.cachedSemanticRenderBlocks.map { $0 as NSArray }, retainedBlocks, file: file, line: line)
        XCTAssertEqual(adapter.fullFrameAdoptionCountForTesting, fullAdoptions, file: file, line: line)
        XCTAssertEqual(adapter.deltaFrameAdoptionCountForTesting, deltaAdoptions, file: file, line: line)
        XCTAssertEqual(adapter.positionEpoch, positionEpoch, "selection state must keep the position epoch", file: file, line: line)
        XCTAssertEqual(adapter.cachedTablePresentation?.positionEpoch, positionEpoch, file: file, line: line)
        let fastObservation = observeSelectionRender(adapter, update: fast.updateJSON)

        let full = try XCTUnwrap(adapter.renderNativeIntentOutcome(fullOutcome), "full render", file: file, line: line)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, frameFetches + 1,
            "the outcome without selection state must take the full frame path", file: file, line: line)
        XCTAssertEqual(fast.changed, full.changed, file: file, line: line)
        XCTAssertEqual(fast.documentChanged, full.documentChanged, file: file, line: line)
        assertSameSelectionRender(fastObservation, observeSelectionRender(adapter, update: full.updateJSON), file: file, line: line)
    }

    func testSelectionStateAdoptionMatchesTheFullFramePathForATextSelection() throws {
        let adapter = makeAdapter()
        XCTAssertNotNil(adapter.setContentHtml("<p><strong>bold 👩‍💻</strong> plain</p><blockquote><p>quoted</p></blockquote>"))
        let owner = UUID()
        adapter.claimNativeBindingIfUnowned(token: owner)
        defer { adapter.releaseNativeBindingOwner(token: owner) }
        XCTAssertNotNil(adapter.initialUpdateJSON())
        let revision = adapter.baseDocumentRevision

        let outcome = try XCTUnwrap(adapter.submitNativeIntent(adapter.nativeIntent("setSelection", anchor: 2, head: 9)))
        XCTAssertFalse(outcome.documentChanged)
        let deliveredSelectionState = try XCTUnwrap(outcome.selectionState,
            "a selection-only intent against the owner's current render carries its selection state")
        XCTAssertEqual(deliveredSelectionState as NSDictionary, try authoritativeSelectionState(adapter) as NSDictionary)
        try assertSelectionStateAdoptionMatchesFullRender(adapter, outcome: outcome)
        XCTAssertEqual(adapter.baseDocumentRevision, revision)
        XCTAssertEqual(adapter.cachedAuthoritativeScalarSelection.map { [$0.anchor, $0.head] }, [2, 9])
    }

    func testSelectionStateAdoptionResolvesCellSelectionsAgainstTheInstalledTableIndex() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let owner = UUID()
        adapter.claimNativeBindingIfUnowned(token: owner)
        defer { adapter.releaseNativeBindingOwner(token: owner) }
        XCTAssertNotNil(adapter.initialUpdateJSON())
        let key = try XCTUnwrap(adapter.cachedTablePresentation?.index.tableKeys.first)
        let admission = try XCTUnwrap(adapter.tableMutationAdmission(tableID: key))
        XCTAssertNotNil(adapter.selectTableCell(cellIndex: 1, admission: admission))
        XCTAssertEqual(adapter.cachedAtomicRenderSelection()?["type"] as? String, "cell")

        try assertSelectionStateAdoptionMatchesFullRender(
            adapter, outcome: EditorV2Adapter.NativeIntentOutcome(
                changed: false, documentChanged: false, selectionState: nil, unappliedSelection: nil
            )
        )
        XCTAssertEqual(adapter.cachedAtomicRenderSelection()?["type"] as? String, "cell")
    }

    func testInvalidSelectionStateFallsBackToTheFullFramePath() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let owner = UUID()
        adapter.claimNativeBindingIfUnowned(token: owner)
        defer { adapter.releaseNativeBindingOwner(token: owner) }
        XCTAssertNotNil(adapter.initialUpdateJSON())
        let key = try XCTUnwrap(adapter.cachedTablePresentation?.index.tableKeys.first)
        let admission = try XCTUnwrap(adapter.tableMutationAdmission(tableID: key))
        XCTAssertNotNil(adapter.selectTableCell(cellIndex: 0, admission: admission))
        let valid = try authoritativeSelectionState(adapter)
        let revision = try XCTUnwrap(valid["documentVersion"] as? String)
        var unresolvedCell = valid
        unresolvedCell["selection"] = ["type": "cell", "anchorCell": 0, "headCell": 0]
        XCTAssertNotNil(EditorV2Adapter.atomicRenderSnapshot(validatingStateOf: unresolvedCell.merging(
            ["renderBlocks": NSNull(), "renderPatch": NSNull()]) { current, _ in current }),
            "the unresolved cell case must pass state validation so only cell resolution rejects it")
        var staleRevision = valid
        staleRevision["documentVersion"] = String(try XCTUnwrap(UInt64(revision)) + 1)
        var extraKey = valid
        extraKey["positionEpoch"] = "1"
        var missingKey = valid
        missingKey.removeValue(forKey: "documentIsEmpty")
        var malformedHistory = valid
        malformedHistory["historyState"] = ["canUndo": 1, "canRedo": false]
        let cases: [(String, [String: Any]?)] = [
            ("missing", nil), ("unresolved cell", unresolvedCell), ("stale document version", staleRevision),
            ("extra key", extraKey), ("missing key", missingKey), ("malformed history", malformedHistory)
        ]
        for (label, selectionState) in cases {
            let frameFetches = adapter.renderUpdateCallCountForTesting
            let fallback = try XCTUnwrap(adapter.renderNativeIntentOutcome(EditorV2Adapter.NativeIntentOutcome(
                changed: false, documentChanged: false, selectionState: selectionState,
                unappliedSelection: nil
            )), label)
            XCTAssertEqual(adapter.renderUpdateCallCountForTesting, frameFetches + 1, "\(label) must take the full frame path")
            let fallbackObservation = observeSelectionRender(adapter, update: fallback.updateJSON)
            let full = try XCTUnwrap(adapter.renderNativeIntentOutcome(EditorV2Adapter.NativeIntentOutcome(
                changed: false, documentChanged: false, selectionState: nil,
                unappliedSelection: nil
            )), label)
            assertSameSelectionRender(fallbackObservation, observeSelectionRender(adapter, update: full.updateJSON))
            XCTAssertEqual(adapter.cachedAtomicRenderSelection() as NSDictionary?, valid["selection"] as? NSDictionary,
                "\(label) must leave the authoritative cell selection installed")
        }
    }

}


extension EditorV2AdapterTests {
    func testAlignmentCommandKeepsTextAndRejectsUnsupportedPhysicalAlignment() throws {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>שלום</p>")
        let updated = try XCTUnwrap(adapter.setTextAlign("right", anchor: 1, head: 1))
        let update = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(updated.utf8)) as? [String: Any])
        XCTAssertEqual((update["activeState"] as? [String: Any])?["textAlign"] as? String, "right")
        XCTAssertEqual(documentText(adapter), "שלום")
        let original = documentJsonObject(adapter)
        XCTAssertNil(adapter.setTextAlign("justify", anchor: 1, head: 1))
        XCTAssertEqual(documentJsonObject(adapter) as NSDictionary, original as NSDictionary)
        XCTAssertNotNil(adapter.setTextAlign(nil, anchor: 1, head: 1))
        let paragraph = try XCTUnwrap((documentJsonObject(adapter)["content"] as? [[String: Any]])?.first)
        XCTAssertTrue((paragraph["attrs"] as? [String: Any])?["textAlign"] == nil || (paragraph["attrs"] as? [String: Any])?["textAlign"] is NSNull)
    }
}
