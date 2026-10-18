import UIKit
import XCTest

extension EditorV2AdapterTests {
    func testReplacingNativeOwnerDoesNotRefreshBeforeNewErrorCallbackIsInstalled() {
        let adapter = makeAdapter()
        let firstOwner = UUID()
        let secondOwner = UUID()
        var firstErrors: [FfiError] = []
        var secondErrors: [FfiError] = []
        adapter.bindAutonomousErrorOwner(token: firstOwner) { firstErrors.append($0) }
        let renderCalls = adapter.renderUpdateCallCountForTesting

        adapter.bindAutonomousErrorOwner(token: secondOwner) { secondErrors.append($0) }

        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, renderCalls)
        XCTAssertTrue(firstErrors.isEmpty)
        XCTAssertTrue(secondErrors.isEmpty)
        XCTAssertTrue(adapter.isAutonomousErrorOwner(token: secondOwner))
    }

    func testDestroyReleasesTheRetainedFrameIndex() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        XCTAssertFalse(adapter.tableIndex.tableKeys.isEmpty)
        XCTAssertNil(adapter.destroy())
        XCTAssertTrue(adapter.tableIndex.tableKeys.isEmpty)
        XCTAssertNil(adapter.installedFrameRevision)
        XCTAssertNil(adapter.cachedTablePresentation)
        XCTAssertNil(adapter.cachedSemanticRenderBlocks)
    }

    func testClaimWithAStaleCachedFrameDefersToTheNextFullRefresh() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        var oldErrors: [FfiError] = []
        var newErrors: [FfiError] = []
        adapter.bindAutonomousErrorOwner(token: UUID()) { oldErrors.append($0) }
        let mutation = adapter.callWithEnvelope(["text": "X"]) { editorV2ApplyInput(editorId: adapter.editorId, requestJson: $0) }
        XCTAssertNil(mutation.error)
        let owner = UUID()
        adapter.bindAutonomousErrorOwner(token: owner) { newErrors.append($0) }
        defer { adapter.releaseNativeBindingOwner(token: owner) }
        XCTAssertTrue(oldErrors.isEmpty)
        XCTAssertTrue(newErrors.isEmpty)
        let calls = adapter.renderUpdateCallCountForTesting
        XCTAssertNotNil(adapter.initialUpdateJSON())
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, calls + 1)
        XCTAssertTrue(oldErrors.isEmpty)
        XCTAssertTrue(newErrors.isEmpty)
    }

    func testMatchingExternalResetAdoptsTheSingleFetchedFrame() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let owner = UUID()
        adapter.claimNativeBindingIfUnowned(token: owner)
        defer { adapter.releaseNativeBindingOwner(token: owner) }
        let reset = "{\"history\":\"resetAndClear\",\"documentRevision\":\"\(adapter.baseDocumentRevision)\",\"setJson\":\(TableInputTestSchema.twoCellDocument)}"
        let notice = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        let calls = adapter.renderUpdateCallCountForTesting
        XCTAssertNotNil(adapter.externalRenderNotice(notice))
        XCTAssertNotNil(adapter.adoptExternalReset(resetJSON: reset))
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, calls + 1)
    }

    func testStaleFrameBaseRecoversWithExactlyOneFullFrame() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let owner = UUID()
        adapter.claimNativeBindingIfUnowned(token: owner)
        defer { adapter.releaseNativeBindingOwner(token: owner) }
        var observed: [FfiTableFrameKind] = []
        adapter.transformNativeFrameForTesting = { incoming in
            var frame = incoming
            observed.append(frame.tables.kind)
            if observed.count == 1 { frame.tables.baseDocumentRevision = "0" }
            return frame
        }
        let before = adapter.fullFrameAdoptionCountForTesting
        XCTAssertNotNil(adapter.initialUpdateJSON())
        XCTAssertEqual(observed, [.delta, .full])
        XCTAssertEqual(adapter.fullFrameAdoptionCountForTesting, before + 1)
    }

    func testCorruptFullSnapshotKeepsRootAndIndexWithOneError() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let key = try XCTUnwrap(adapter.tableIndex.tableKeys.first)
        let original = adapter.tableIndex.record(tableKey: key)
        let root = adapter.cachedViewUpdate?.updateJSON
        let spy = ErrorSpy()
        adapter.onAutonomousError = spy.record
        var calls = 0
        adapter.transformNativeFrameForTesting = { incoming in
            calls += 1
            var frame = incoming
            frame.snapshotJson = "{"
            frame.tables.tables[0].cells[0].contentKey = "must-not-install"
            return frame
        }
        XCTAssertNil(adapter.initialUpdateJSON())
        XCTAssertEqual(calls, 1)
        XCTAssertEqual(spy.errors.count, 1)
        XCTAssertEqual(spy.last?.message, "native table frame violates the frozen shape")
        XCTAssertEqual(adapter.cachedViewUpdate?.updateJSON, root)
        XCTAssertEqual(adapter.tableIndex.record(tableKey: key), original)
    }

    func testReclaimedOwnerAndRefusedCellBackspaceUseDeltaFrames() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let first = UUID()
        adapter.claimNativeBindingIfUnowned(token: first)
        adapter.releaseNativeBindingOwner(token: first)
        let second = UUID()
        adapter.claimNativeBindingIfUnowned(token: second)
        defer { adapter.releaseNativeBindingOwner(token: second) }
        let fullBefore = adapter.fullFrameAdoptionCountForTesting
        XCTAssertNotNil(adapter.initialUpdateJSON())
        let key = try XCTUnwrap(adapter.tableIndex.tableKeys.first)
        let start = try XCTUnwrap(adapter.tableIndex.scalarStart(tableKey: key, cellIndex: 0))
        XCTAssertNotNil(adapter.syncSelection(anchor: start, head: start))
        let before = adapter.documentJson()
        _ = adapter.deleteBackward(anchor: start, head: start)
        XCTAssertEqual(adapter.documentJson(), before)
        XCTAssertEqual(adapter.fullFrameAdoptionCountForTesting, fullBefore)
    }

    func testExternalRenderNoticeNeverInstallsJavascriptTableContent() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        let key = try XCTUnwrap(adapter.tableIndex.tableKeys.first)
        let original = adapter.tableIndex.record(tableKey: key)
        let notice = "{\"documentVersion\":\"\(adapter.baseDocumentRevision)\",\"tableRecords\":{\"forged\":false},\"renderBlocks\":[[{}]]}"
        XCTAssertNotNil(adapter.externalRenderNotice(notice))
        XCTAssertNotNil(adapter.adoptExternalRender())
        XCTAssertEqual(adapter.tableIndex.record(tableKey: key), original)
        XCTAssertEqual(adapter.tableIndex.tableKeys, [key])
        XCTAssertNil(parseObject(adapter.cachedAtomicRenderJSON)["tableRecords"])
    }

    func testFailedTableFullFrameAdoptsWithoutCells() throws {
        let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
        XCTAssertNotNil(adapter.setContentJson(TableInputTestSchema.twoCellDocument))
        adapter.transformNativeFrameForTesting = { incoming in
            var frame = incoming
            frame.tables.tables[0].failure = .gridLimit
            frame.tables.tables[0].cells = []
            frame.tables.tables[0].sourceRows = []
            return frame
        }
        XCTAssertNotNil(adapter.initialUpdateJSON())
        let key = try XCTUnwrap(adapter.tableIndex.tableKeys.first)
        XCTAssertEqual(adapter.tableIndex.record(tableKey: key)?.failure, .gridLimit)
        XCTAssertTrue(try XCTUnwrap(adapter.tableIndex.record(tableKey: key)).cells.isEmpty)
    }

    func testRevisionMismatchRefusesSelectionRelativeInputWithoutReplay() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")

        // Externally advance the same v2 session so the adapter's tracked
        // base revision goes stale.
        let external = editorV2ApplyCommand(
            editorId: adapter.editorId,
            requestJson: #"{"version":1,"requestId":"990001","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","command":{"type":"insertText","text":"EXT"}}"#
        )
        XCTAssertNil(external.error, "external mutation failed: \(String(describing: external.error))")
        XCTAssertEqual(documentText(adapter), "EXTbase")

        let callsBefore = adapter.backendEnvelopeCallCountForTesting
        let update = adapter.insertText("REBASED", atScalar: 0)
        XCTAssertNotNil(update)
        XCTAssertEqual(documentText(adapter), "EXTbase")
        XCTAssertEqual(renderedText(update), "EXTbase")
        XCTAssertEqual(adapter.backendEnvelopeCallCountForTesting, callsBefore + 1)

        let recovered = adapter.insertText("ok", atScalar: 0)
        XCTAssertEqual(renderedText(recovered), "okEXTbase")
        XCTAssertEqual(documentText(adapter), "okEXTbase")
    }

    func testPreSyncMismatchRefusesSelectionRelativeInputWithoutReplay() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        _ = adapter.syncSelection(anchor: 0, head: 0)

        let external = editorV2ApplyCommand(
            editorId: adapter.editorId,
            requestJson: #"{"version":1,"requestId":"990002","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","command":{"type":"insertText","text":"EXT"}}"#
        )
        XCTAssertNil(external.error)
        let callsBefore = adapter.backendEnvelopeCallCountForTesting

        let update = adapter.insertText("X", atScalar: 2)

        XCTAssertEqual(documentText(adapter), "EXTbase")
        XCTAssertEqual(renderedText(update), "EXTbase")
        XCTAssertEqual(adapter.backendEnvelopeCallCountForTesting, callsBefore + 1)
    }

    func testPreSyncMismatchRefusesDeleteBackwardWithoutReplay() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        _ = adapter.syncSelection(anchor: 4, head: 4)
        let external = editorV2ApplyCommand(
            editorId: adapter.editorId,
            requestJson: #"{"version":1,"requestId":"990003","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","command":{"type":"insertText","text":"R"}}"#
        )
        XCTAssertNil(external.error)

        let update = adapter.deleteBackward(anchor: 2, head: 2)

        XCTAssertNotNil(update)
        XCTAssertEqual(documentText(adapter), "baseR")
    }

    func testMismatchRefreshDoesNotInvokeCompatibilityRecovery() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        _ = adapter.syncSelection(anchor: 0, head: 0)
        let first = editorV2ApplyCommand(
            editorId: adapter.editorId,
            requestJson: #"{"version":1,"requestId":"990004","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","command":{"type":"insertText","text":"EXT"}}"#
        )
        XCTAssertNil(first.error)
        var recovered = false
        adapter.onRemoteRecoveryForTesting = { recovered = true }
        let callsBefore = adapter.backendEnvelopeCallCountForTesting

        let update = adapter.insertText("X", atScalar: 2)

        XCTAssertNotNil(update)
        XCTAssertEqual(documentText(adapter), "EXTbase")
        XCTAssertEqual(adapter.backendEnvelopeCallCountForTesting, callsBefore + 1)
        XCTAssertFalse(recovered)
    }

    func testRevisionMismatchNeverReplaysExplicitlyPositionedMutation() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        let external = editorV2ApplyCommand(
            editorId: adapter.editorId,
            requestJson: #"{"version":1,"requestId":"990006","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","command":{"type":"insertText","text":"EXT"}}"#
        )
        XCTAssertNil(external.error)
        let callsBefore = adapter.backendEnvelopeCallCountForTesting

        let update = adapter.deleteScalarRange(from: 0, to: 4)

        XCTAssertNotNil(update)
        XCTAssertEqual(documentText(adapter), "EXTbase")
        XCTAssertEqual(adapter.backendEnvelopeCallCountForTesting, callsBefore + 1)
    }

    func testAtomicRenderValidationAcceptsAValidRenderPatch() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        let raw = editorV2RenderUpdate(
            editorId: adapter.editorId,
            mirrorScalarAnchor: nil,
            mirrorScalarHead: nil
        ).value!
        let withPatch = mutatedObjectJSON(raw) { object in
            let renderBlocks = object["renderBlocks"]!
            object["renderBlocks"] = NSNull()
            object["renderPatch"] = [
                "baseDocumentVersion": object["documentVersion"]!,
                "startIndex": 0,
                "deleteCount": 0,
                "renderBlocks": renderBlocks
            ]
        }

        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(withPatch))
    }

    /// Rust emits `attrs` on every void/opaque element, so an inserted mention
    /// must survive external-render validation on its way back to the view.
    func testAtomicRenderValidationAcceptsAnInsertedMentionCarryingNodeAttrs() {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        let raw = editorV2RenderUpdate(
            editorId: adapter.editorId,
            mirrorScalarAnchor: nil,
            mirrorScalarHead: nil
        ).value!
        let withMention = mutatedObjectJSON(raw) { object in
            object["renderBlocks"] = [[
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                [
                    "type": "opaqueInlineAtom",
                    "nodeType": "mention",
                    "label": "@Alice Chen",
                    "docPos": 1,
                    "attrs": [
                        "id": "user-alice",
                        "label": "Alice Chen",
                        "mentionSuggestionChar": "@",
                        "type": "user"
                    ],
                    "mentionTheme": ["node": ["textColor": "#336EC1"]]
                ],
                ["type": "blockEnd"]
            ]]
        }

        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(withMention))
    }

    func testAtomicRenderValidationAcceptsAtomIdOnlyOnVoidBlock() {
        func adopt(_ element: [String: Any]) -> String? {
            let adapter = makeAdapter()
            _ = adapter.setContentHtml("<p>base</p>")
            let raw = editorV2RenderUpdate(
                editorId: adapter.editorId,
                mirrorScalarAnchor: nil,
                mirrorScalarHead: nil
            ).value!
            let snapshot = mutatedObjectJSON(raw) { object in
                object["renderBlocks"] = [[element]]
            }
            return EditorV2Adapter.parseAtomicRenderSnapshot(snapshot)?.viewUpdateJSON
        }

        XCTAssertNotNil(adopt([
            "type": "voidBlock",
            "nodeType": "counterCard",
            "docPos": 1,
            "atomId": "y1-2"
        ]))
        XCTAssertNil(adopt([
            "type": "voidBlock",
            "nodeType": "counterCard",
            "docPos": 1,
            "atomId": 7
        ]))
        XCTAssertNil(adopt([
            "type": "voidInline",
            "nodeType": "hardBreak",
            "docPos": 1,
            "atomId": "y1-2"
        ]))
    }

    func testMalformedAtomicRenderNestedVariantsLeaveEveryCacheUnchanged() throws {
        let adapter = makeAdapter()
        let spy = ErrorSpy()
        adapter.onAutonomousError = spy.record
        _ = adapter.setContentHtml("<p>base</p>")
        let raw = editorV2RenderUpdate(
            editorId: adapter.editorId,
            mirrorScalarAnchor: nil,
            mirrorScalarHead: nil
        ).value!
        XCTAssertNotNil(adapter.externalRenderNotice(raw))
        XCTAssertNotNil(adapter.adoptExternalRender())
        let baseline = adapter.cacheStateForTesting

        let variants: [(String, (inout [String: Any]) throws -> Void)] = [
            ("extra top-level field", { $0["legacyRevision"] = 1 }),
            ("null selection", { $0["selection"] = NSNull() }),
            ("selection extra field", { object in
                var selection = try XCTUnwrap(object["selection"] as? [String: Any])
                selection["legacyAnchor"] = 0
                object["selection"] = selection
            }),
            ("selection scalar above u32", { object in
                var selection = try XCTUnwrap(object["selection"] as? [String: Any])
                selection["anchorScalar"] = NSNumber(value: UInt64(UInt32.max) + 1)
                object["selection"] = selection
            }),
            ("node selection fractional scalar", {
                $0["selection"] = ["type": "node", "pos": 1, "posScalar": 0.5]
            }),
            ("all selection extra field", {
                $0["selection"] = ["type": "all", "anchor": 0]
            }),
            ("invalid text mark", { object in
                var blocks = try XCTUnwrap(object["renderBlocks"] as? [[[String: Any]]])
                let index = blocks[0].firstIndex { $0["type"] as? String == "textRun" }!
                var textRun = blocks[0][index]
                textRun["marks"] = [["type": 7]]
                blocks[0][index] = textRun
                object["renderBlocks"] = blocks
            }),
            ("text run extra field", { object in
                var blocks = try XCTUnwrap(object["renderBlocks"] as? [[[String: Any]]])
                let index = blocks[0].firstIndex { $0["type"] as? String == "textRun" }!
                blocks[0][index]["legacyText"] = "base"
                object["renderBlocks"] = blocks
            }),
            ("block start list u32 above range", { object in
                var blocks = try XCTUnwrap(object["renderBlocks"] as? [[[String: Any]]])
                let index = blocks[0].firstIndex { $0["type"] as? String == "blockStart" }!
                var blockStart = blocks[0][index]
                blockStart["listContext"] = [
                    "ordered": false,
                    "index": NSNumber(value: UInt64(UInt32.max) + 1),
                    "total": 1,
                    "start": 1,
                    "isFirst": true,
                    "isLast": true
                ]
                blocks[0][index] = blockStart
                object["renderBlocks"] = blocks
            }),
            ("block start invalid list boolean", { object in
                var blocks = try XCTUnwrap(object["renderBlocks"] as? [[[String: Any]]])
                let index = blocks[0].firstIndex { $0["type"] as? String == "blockStart" }!
                var blockStart = blocks[0][index]
                blockStart["listContext"] = [
                    "ordered": 1,
                    "index": 1,
                    "total": 1,
                    "start": 1,
                    "isFirst": true,
                    "isLast": true
                ]
                blocks[0][index] = blockStart
                object["renderBlocks"] = blocks
            }),
            ("block end extra field", {
                $0["renderBlocks"] = [["type": "blockEnd", "legacy": true]]
            }),
            ("void inline array attrs", {
                $0["renderBlocks"] = [[
                    "type": "voidInline",
                    "nodeType": "image",
                    "docPos": 0,
                    "attrs": []
                ]]
            }),
            ("void block fractional doc position", {
                $0["renderBlocks"] = [[
                    "type": "voidBlock",
                    "nodeType": "image",
                    "docPos": 0.5
                ]]
            }),
            ("opaque inline invalid mention theme", {
                $0["renderBlocks"] = [[
                    "type": "opaqueInlineAtom",
                    "nodeType": "mention",
                    "label": "Ada",
                    "docPos": 0,
                    "mentionTheme": ["node": ["borderWidth": true]]
                ]]
            }),
            ("opaque block extra field", {
                $0["renderBlocks"] = [[
                    "type": "opaqueBlockAtom",
                    "nodeType": "unknown",
                    "label": "Unknown",
                    "docPos": 0,
                    "legacy": true
                ]]
            }),
            ("opaque inline array attrs", {
                $0["renderBlocks"] = [[
                    "type": "opaqueInlineAtom",
                    "nodeType": "mention",
                    "label": "Ada",
                    "docPos": 0,
                    "attrs": []
                ]]
            }),
            ("render patch invalid nested element", { object in
                object["renderPatch"] = [
                    "baseDocumentVersion": object["documentVersion"]!,
                    "startIndex": 0,
                    "deleteCount": 0,
                    "renderBlocks": [["type": "unknownElement"]]
                ]
            }),
            ("render patch fractional start", { object in
                object["renderPatch"] = [
                    "baseDocumentVersion": object["documentVersion"]!,
                    "startIndex": 0.5,
                    "deleteCount": 0,
                    "renderBlocks": object["renderBlocks"]!
                ]
            }),
            ("active-state extra field", { object in
                var active = try XCTUnwrap(object["activeState"] as? [String: Any])
                active["legacy"] = false
                object["activeState"] = active
            }),
            ("active-state non-boolean map value", { object in
                var active = try XCTUnwrap(object["activeState"] as? [String: Any])
                active["marks"] = ["bold": "yes"]
                object["activeState"] = active
            }),
            ("active-state non-record mark attrs", { object in
                var active = try XCTUnwrap(object["activeState"] as? [String: Any])
                active["markAttrs"] = ["link": "https://example.com"]
                object["activeState"] = active
            }),
            ("active-state non-string insertion", { object in
                var active = try XCTUnwrap(object["activeState"] as? [String: Any])
                active["insertableNodes"] = ["image", 1]
                object["activeState"] = active
            }),
            ("history numeric boolean", { object in
                object["historyState"] = ["canUndo": 1, "canRedo": false]
            }),
            ("non-canonical revision", { $0["documentVersion"] = "01" }),
            ("state revision numeric", { $0["stateRevision"] = 1 }),
            ("scalar length above u32", {
                $0["scalarLength"] = NSNumber(value: UInt64(UInt32.max) + 1)
            }),
            ("scalar length fractional", { $0["scalarLength"] = 0.5 })
        ]

        for (name, mutate) in variants {
            let errorsBefore = spy.errors.count
            let malformed = try mutatedObjectJSON(raw, mutate)
            adapter.transformNativeFrameForTesting = { incoming in
                var frame = incoming
                frame.snapshotJson = malformed
                return frame
            }
            XCTAssertNil(adapter.initialUpdateJSON(), name)
            XCTAssertEqual(adapter.cacheStateForTesting, baseline, name)
            XCTAssertEqual(spy.errors.count, errorsBefore + 1, name)
            XCTAssertEqual(spy.last?.domain, "boundary", name)
            XCTAssertEqual(spy.last?.code, "FFI_RESULT_INVALID", name)
        }
    }

    private enum RetainedSemanticAdmissionFixture {
        static let markedText = "bold 👩‍💻 e\u{301}"
        static let proseHTML = "<p><strong>\(markedText)</strong> plain</p><p>middle</p><p>tail</p>"
        static let plainDocument = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"lead"}]},{"type":"paragraph","content":[{"type":"text","text":"tail"}]}]}"#
        static let trailingText = "tail"
        static let editedTrailingText = "taXil"
        static let insertion = "X"
        static let caret: UInt32 = 1
        static let trailingCaretDistance: UInt32 = 2
        static let singleFrameCount = 1
        static let recoveryFrameCount = 2
        static let invalidMarkType = 7
        static let invalidBoolean = 1
        static let staleBase = "0"
        static let unknownTableKey = "retained-semantic-unknown-table"
        static let positionEpochKey = "positionEpoch"
        static let nonCanonicalEpochPrefix = "0"
    }

    private enum MalformedSemanticDelta: String, CaseIterable {
        case malformedMark = "malformed incoming delta mark"
        case malformedHistory = "malformed incoming delta history"
        case staleRenderBase = "stale semantic patch base"
        case outOfBoundsReplacement = "semantic replacement outside retained blocks"
    }

    private enum MalformedPositionEpoch: String, CaseIterable {
        case nonCanonicalFrameField = "non-canonical frame position epoch"
        case snapshotField = "position epoch inside the atomic snapshot"
    }

    private enum RetainedTableDelta: String, CaseIterable {
        case duplicateReference = "duplicate table reference across retained and inserted blocks"
        case unknownReference = "unknown inserted table reference"
        case removedIndexRoot = "retained table reference after a valid index removal"
    }

    private func withRetainedSemanticAdapter(
        documentJson: String? = nil,
        configJson: String? = nil,
        _ body: (EditorV2Adapter) throws -> Void
    ) throws {
        let adapter = configJson.map { makeAdapter(configJson: $0) } ?? makeAdapter()
        let initialUpdate: String?
        if let documentJson {
            initialUpdate = adapter.setContentJson(documentJson)
        } else {
            initialUpdate = adapter.setContentHtml(RetainedSemanticAdmissionFixture.proseHTML)
        }
        _ = try XCTUnwrap(initialUpdate)
        let owner = UUID()
        adapter.claimNativeBindingIfUnowned(token: owner)
        defer {
            adapter.transformNativeFrameForTesting = nil
            adapter.releaseNativeBindingOwner(token: owner)
        }
        _ = try XCTUnwrap(adapter.initialUpdateJSON())
        try body(adapter)
    }

    private func completedSemanticSnapshot(_ adapter: EditorV2Adapter) throws -> NSDictionary {
        var object = parseObject(try XCTUnwrap(adapter.cachedAtomicRenderJSON))
        XCTAssertEqual(Set(object.keys), EditorV2Adapter.atomicRenderSnapshotKeys)
        object["renderBlocks"] = try XCTUnwrap(adapter.cachedSemanticRenderBlocks)
        object["renderPatch"] = NSNull()
        return object as NSDictionary
    }

    private func assertMatchedTableFreeSemanticDelta(
        _ adapter: EditorV2Adapter,
        phase: String,
        _ operation: () -> String?
    ) throws {
        let retainedRevision = try XCTUnwrap(adapter.cachedAtomicRenderDocumentRevision, phase)
        let retainedBlocks = try XCTUnwrap(adapter.cachedSemanticRenderBlocks, phase)
        XCTAssertEqual(adapter.installedFrameRevision, retainedRevision, phase)
        XCTAssertTrue(adapter.tableIndex.tableKeys.isEmpty, phase)
        XCTAssertTrue(adapter.tableIndex.rootExtents.isEmpty, phase)
        let callsBefore = adapter.renderUpdateCallCountForTesting
        let fullBefore = adapter.fullFrameAdoptionCountForTesting
        let deltaBefore = adapter.deltaFrameAdoptionCountForTesting
        var observed: [FfiNativeRenderFrame] = []
        let spy = ErrorSpy()
        let previousError = adapter.onAutonomousError
        let previousTransform = adapter.transformNativeFrameForTesting
        adapter.onAutonomousError = spy.record
        adapter.transformNativeFrameForTesting = { incoming in
            observed.append(incoming)
            return incoming
        }
        defer {
            adapter.transformNativeFrameForTesting = previousTransform
            adapter.onAutonomousError = previousError
        }

        _ = try XCTUnwrap(operation(),
            "\(phase): observed \(observed.count) frames, error \(String(describing: spy.last)), document \(documentJsonObject(adapter))")

        XCTAssertTrue(spy.errors.isEmpty, phase)
        XCTAssertEqual(observed.count, RetainedSemanticAdmissionFixture.singleFrameCount, phase)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting,
            callsBefore + RetainedSemanticAdmissionFixture.singleFrameCount, phase)
        XCTAssertEqual(adapter.fullFrameAdoptionCountForTesting, fullBefore, phase)
        XCTAssertEqual(adapter.deltaFrameAdoptionCountForTesting,
            deltaBefore + RetainedSemanticAdmissionFixture.singleFrameCount, phase)
        let incoming = try XCTUnwrap(observed.first, phase)
        XCTAssertEqual(incoming.tables.kind, .delta, phase)
        let snapshot = try XCTUnwrap(EditorV2Adapter.parseAtomicRenderSnapshot(incoming.snapshotJson), phase)
        XCTAssertTrue(snapshot.renderObject["renderBlocks"] is NSNull,
            "\(phase): eligibility requires the real frame to carry a semantic delta")
        let patch = try XCTUnwrap(snapshot.renderObject["renderPatch"] as? [String: Any], phase)
        XCTAssertEqual(EditorV2Adapter.uint64Field(patch, "baseDocumentVersion"), retainedRevision, phase)
        let start = try XCTUnwrap(EditorV2Adapter.uint32Field(patch, "startIndex"), phase)
        let delete = try XCTUnwrap(EditorV2Adapter.uint32Field(patch, "deleteCount"), phase)
        let inserted = try XCTUnwrap(patch["renderBlocks"] as? [[[String: Any]]], phase)
        XCTAssertLessThanOrEqual(UInt64(start) + UInt64(delete), UInt64(retainedBlocks.count), phase)
        XCTAssertGreaterThan(retainedBlocks.count, Int(delete),
            "\(phase): the fixture must retain already admitted blocks")
        XCTAssertFalse(inserted.joined().contains { $0["type"] as? String == "table" }, phase)
        XCTAssertTrue(adapter.tableIndex.tableKeys.isEmpty, phase)
        XCTAssertTrue(adapter.tableIndex.rootExtents.isEmpty, phase)
        XCTAssertFalse(try XCTUnwrap(adapter.cachedSemanticRenderBlocks, phase).joined().contains {
            $0["type"] as? String == "table"
        }, phase)
        XCTAssertEqual(adapter.installedFrameRevision, snapshot.documentRevision, phase)
        XCTAssertEqual(adapter.cachedAtomicRenderDocumentRevision, snapshot.documentRevision, phase)
        XCTAssertEqual(adapter.cachedScalarLength, snapshot.scalarLength, phase)
    }

    private func assertFreshFullSemanticSnapshotMatches(
        _ adapter: EditorV2Adapter,
        phase: String
    ) throws {
        let completeBefore = try completedSemanticSnapshot(adapter)
        let documentBefore = try XCTUnwrap(adapter.documentJson(), phase)
        let keysBefore = adapter.tableIndex.tableKeys
        let extentsBefore = adapter.tableIndex.rootExtents
        let recordsBefore = keysBefore.compactMap(adapter.tableIndex.record(tableKey:))
        let callsBefore = adapter.renderUpdateCallCountForTesting
        let fullBefore = adapter.fullFrameAdoptionCountForTesting
        let deltaBefore = adapter.deltaFrameAdoptionCountForTesting
        var observed: [FfiNativeRenderFrame] = []
        let previousTransform = adapter.transformNativeFrameForTesting
        adapter.transformNativeFrameForTesting = { incoming in
            observed.append(incoming)
            return incoming
        }
        defer { adapter.transformNativeFrameForTesting = previousTransform }

        _ = try XCTUnwrap(adapter.recoverNativeRender(), phase)

        XCTAssertEqual(observed.map(\.tables.kind), [.full], phase)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting,
            callsBefore + RetainedSemanticAdmissionFixture.singleFrameCount, phase)
        XCTAssertEqual(adapter.fullFrameAdoptionCountForTesting,
            fullBefore + RetainedSemanticAdmissionFixture.singleFrameCount, phase)
        XCTAssertEqual(adapter.deltaFrameAdoptionCountForTesting, deltaBefore, phase)
        XCTAssertEqual(try completedSemanticSnapshot(adapter), completeBefore,
            "\(phase): only render representation is normalized; marks, selection, active/history state, scalar extent and revisions must match")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson(), phase), documentBefore, phase)
        XCTAssertEqual(adapter.tableIndex.tableKeys, keysBefore, phase)
        XCTAssertEqual(adapter.tableIndex.rootExtents, extentsBefore, phase)
        for record in recordsBefore {
            XCTAssertEqual(adapter.tableIndex.record(tableKey: record.tableKey), record, phase)
        }
        let rawFull = parseObject(try XCTUnwrap(adapter.cachedAtomicRenderJSON, phase))
        let fullSnapshot = try XCTUnwrap(EditorV2Adapter.parseAtomicRenderSnapshot(
            try XCTUnwrap(adapter.cachedAtomicRenderJSON, phase)), phase)
        XCTAssertNotNil(fullSnapshot.renderObject["renderBlocks"] as? [[[String: Any]]], phase)
        XCTAssertTrue(fullSnapshot.renderObject["renderPatch"] is NSNull, "\(phase): the comparison oracle must be a genuine full semantic frame")
        let fullFrame = try XCTUnwrap(observed.first, phase)
        XCTAssertEqual(adapter.cachedAtomicRenderJSON, fullFrame.snapshotJson, phase)
        XCTAssertNotNil(fullFrame.positionEpoch, phase)
        XCTAssertEqual(fullFrame.positionEpoch, adapter.positionEpoch.map(String.init), phase)
        XCTAssertNil(rawFull["positionEpoch"],
            "\(phase): a renewed binding epoch is private and must not enter the canonical commit")
    }

    private func assertRejectedSemanticDeltaRecoversWithoutPublishing(
        _ adapter: EditorV2Adapter,
        atScalar caret: UInt32,
        phase: String,
        mutate: @escaping (inout FfiNativeRenderFrame) -> Void
    ) throws {
        let installedIndex = adapter.tableIndex
        let atomicBefore = adapter.cachedAtomicRenderJSON
        let callsBefore = adapter.renderUpdateCallCountForTesting
        let fullBefore = adapter.fullFrameAdoptionCountForTesting
        let deltaBefore = adapter.deltaFrameAdoptionCountForTesting
        let spy = ErrorSpy()
        let previousError = adapter.onAutonomousError
        let previousTransform = adapter.transformNativeFrameForTesting
        var observed: [FfiTableFrameKind] = []
        var cacheBeforeRejectedAdoption: String?
        adapter.onAutonomousError = spy.record
        adapter.transformNativeFrameForTesting = { incoming in
            let firstFrame = observed.isEmpty
            observed.append(incoming.tables.kind)
            if firstFrame {
                XCTAssertEqual(incoming.tables.kind, .delta, phase)
                XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(incoming.snapshotJson), phase)
                cacheBeforeRejectedAdoption = adapter.cacheStateForTesting
                var frame = incoming
                mutate(&frame)
                return frame
            }
            XCTAssertEqual(observed, [.delta, .full], phase)
            XCTAssertEqual(adapter.cacheStateForTesting, cacheBeforeRejectedAdoption,
                "\(phase): rejected adoption must not publish any retained cache")
            XCTAssertTrue(adapter.tableIndex === installedIndex, phase)
            XCTAssertEqual(adapter.cachedAtomicRenderJSON, atomicBefore, phase)
            return incoming
        }
        defer {
            adapter.transformNativeFrameForTesting = previousTransform
            adapter.onAutonomousError = previousError
        }

        _ = try XCTUnwrap(adapter.insertText(RetainedSemanticAdmissionFixture.insertion, atScalar: caret), phase)

        XCTAssertEqual(observed, [.delta, .full], phase)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting,
            callsBefore + RetainedSemanticAdmissionFixture.recoveryFrameCount, phase)
        XCTAssertEqual(adapter.fullFrameAdoptionCountForTesting,
            fullBefore + RetainedSemanticAdmissionFixture.singleFrameCount, phase)
        XCTAssertEqual(adapter.deltaFrameAdoptionCountForTesting, deltaBefore, phase)
        XCTAssertTrue(spy.errors.isEmpty, "\(phase): a successful full recovery must not report a contract failure")
    }

    func testMatchedTableFreeNativeInsertAndDeleteAgreeWithFreshFullSemanticSnapshots() throws {
        try withRetainedSemanticAdapter { adapter in
            let caret = RetainedSemanticAdmissionFixture.caret
            _ = try XCTUnwrap(adapter.syncSelection(anchor: caret, head: caret))
            let originalDocument = try XCTUnwrap(adapter.documentJson())
            try assertMatchedTableFreeSemanticDelta(adapter, phase: "native insertion") {
                adapter.insertText(RetainedSemanticAdmissionFixture.insertion, atScalar: caret)
            }
            let activeMarks = try XCTUnwrap(adapter.cachedActiveState?["marks"] as? [String: Any])
            XCTAssertEqual(activeMarks["bold"] as? Bool, true,
                "The complete comparison must include genuine retained and inserted formatting")
            XCTAssertNotEqual(try XCTUnwrap(adapter.documentJson()), originalDocument)
            try assertFreshFullSemanticSnapshotMatches(adapter, phase: "insertion against the same engine state")

            let insertedLength = try XCTUnwrap(UInt32(exactly: RetainedSemanticAdmissionFixture.insertion.unicodeScalars.count))
            let postCaret = caret + insertedLength
            try assertMatchedTableFreeSemanticDelta(adapter, phase: "native deletion") {
                adapter.deleteBackward(anchor: postCaret, head: postCaret)
            }
            XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), originalDocument,
                "The real delete must restore every original Unicode scalar and mark")
            try assertFreshFullSemanticSnapshotMatches(adapter, phase: "deletion against the same engine state")
        }
    }

    func testMalformedActualProseDeltasRecoverWithoutPublishingRetainedCache() throws {
        for variant in MalformedSemanticDelta.allCases {
            try withRetainedSemanticAdapter { adapter in
                let caret = RetainedSemanticAdmissionFixture.caret
                _ = try XCTUnwrap(adapter.syncSelection(anchor: caret, head: caret))
                let retainedCount = try XCTUnwrap(adapter.cachedSemanticRenderBlocks).count
                try assertRejectedSemanticDeltaRecoversWithoutPublishing(
                    adapter, atScalar: caret, phase: variant.rawValue
                ) { frame in
                    frame.snapshotJson = self.mutatedObjectJSON(frame.snapshotJson) { object in
                        XCTAssertTrue(object["renderBlocks"] is NSNull, variant.rawValue)
                        guard var patch = object["renderPatch"] as? [String: Any],
                              var blocks = patch["renderBlocks"] as? [[[String: Any]]]
                        else {
                            return XCTFail("Expected a real semantic delta for \(variant.rawValue)")
                        }
                        switch variant {
                        case .malformedMark:
                            var changed = false
                            for blockIndex in blocks.indices {
                                if let elementIndex = blocks[blockIndex].firstIndex(where: { $0["type"] as? String == "textRun" }) {
                                    blocks[blockIndex][elementIndex]["marks"] = [["type": RetainedSemanticAdmissionFixture.invalidMarkType]]
                                    changed = true
                                    break
                                }
                            }
                            XCTAssertTrue(changed, "The actual replacement must contain the mark being corrupted")
                            patch["renderBlocks"] = blocks
                        case .malformedHistory:
                            object["historyState"] = ["canUndo": RetainedSemanticAdmissionFixture.invalidBoolean, "canRedo": false]
                        case .staleRenderBase:
                            XCTAssertNotEqual(patch["baseDocumentVersion"] as? String, RetainedSemanticAdmissionFixture.staleBase)
                            patch["baseDocumentVersion"] = RetainedSemanticAdmissionFixture.staleBase
                        case .outOfBoundsReplacement:
                            patch["startIndex"] = retainedCount + RetainedSemanticAdmissionFixture.singleFrameCount
                        }
                        object["renderPatch"] = patch
                    }
                    switch variant {
                    case .malformedMark, .malformedHistory:
                        XCTAssertNil(EditorV2Adapter.parseAtomicRenderSnapshot(frame.snapshotJson), variant.rawValue)
                    case .staleRenderBase, .outOfBoundsReplacement:
                        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(frame.snapshotJson),
                            "\(variant.rawValue): rejection must reach the retained-base/bounds guard, not an unrelated shape failure")
                    }
                }
                XCTAssertTrue(adapter.tableIndex.tableKeys.isEmpty, variant.rawValue)
                XCTAssertTrue(adapter.tableIndex.rootExtents.isEmpty, variant.rawValue)
            }
        }
    }

    func testMalformedFramePositionEpochsRecoverWithoutPublishingRetainedCache() throws {
        for variant in MalformedPositionEpoch.allCases {
            try withRetainedSemanticAdapter { adapter in
                let caret = RetainedSemanticAdmissionFixture.caret
                _ = try XCTUnwrap(adapter.syncSelection(anchor: caret, head: caret))
                try assertRejectedSemanticDeltaRecoversWithoutPublishing(
                    adapter, atScalar: caret, phase: variant.rawValue
                ) { frame in
                    guard let epoch = frame.positionEpoch else {
                        return XCTFail("Expected a bound native frame epoch for \(variant.rawValue)")
                    }
                    switch variant {
                    case .nonCanonicalFrameField:
                        frame.positionEpoch = RetainedSemanticAdmissionFixture.nonCanonicalEpochPrefix + epoch
                        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(frame.snapshotJson),
                            "\(variant.rawValue): rejection must come from the frame epoch, not the snapshot shape")
                    case .snapshotField:
                        frame.snapshotJson = self.mutatedObjectJSON(frame.snapshotJson) { object in
                            object[RetainedSemanticAdmissionFixture.positionEpochKey] = epoch
                        }
                        XCTAssertNil(EditorV2Adapter.parseAtomicRenderSnapshot(frame.snapshotJson), variant.rawValue)
                    }
                }
            }
        }
    }

    func testActualProseDeltaRejectsCrossRetainedTableReferencesAndRemovedIndexRoot() throws {
        let document = try mutatedObjectJSON(TableInputTestSchema.twoCellDocument) { object in
            var content = try XCTUnwrap(object["content"] as? [[String: Any]])
            content.append(["type": "paragraph", "content": [["type": "text", "text": RetainedSemanticAdmissionFixture.trailingText]]])
            object["content"] = content
        }
        for variant in RetainedTableDelta.allCases {
            let adapter = makeAdapter(configJson: TableInputTestSchema.tableConfig)
            _ = try XCTUnwrap(adapter.setContentJson(document))
            let owner = UUID()
            adapter.claimNativeBindingIfUnowned(token: owner)
            defer {
                adapter.transformNativeFrameForTesting = nil
                adapter.releaseNativeBindingOwner(token: owner)
            }
            _ = try XCTUnwrap(adapter.initialUpdateJSON())
            let rootKey = try XCTUnwrap(adapter.tableIndex.rootExtents.keys.first)
            let originalRecord = try XCTUnwrap(adapter.tableIndex.record(tableKey: rootKey))
            let originalExtents = adapter.tableIndex.rootExtents
            let retained = try XCTUnwrap(adapter.cachedSemanticRenderBlocks)
            let rootBlock = try XCTUnwrap(retained.firstIndex { block in
                block.contains { $0["type"] as? String == "table" && $0["tableId"] as? String == rootKey }
            })
            let scalarLength = try XCTUnwrap(adapter.cachedScalarLength)
            XCTAssertGreaterThan(scalarLength, RetainedSemanticAdmissionFixture.trailingCaretDistance)
            let caret = scalarLength - RetainedSemanticAdmissionFixture.trailingCaretDistance
            _ = try XCTUnwrap(adapter.syncSelection(anchor: caret, head: caret))
            try assertRejectedSemanticDeltaRecoversWithoutPublishing(
                adapter, atScalar: caret, phase: variant.rawValue
            ) { frame in
                let incoming = self.parseObject(frame.snapshotJson)
                guard let patch = incoming["renderPatch"] as? [String: Any],
                      let start = EditorV2Adapter.uint32Field(patch, "startIndex"),
                      let delete = EditorV2Adapter.uint32Field(patch, "deleteCount"),
                      let inserted = patch["renderBlocks"] as? [[[String: Any]]]
                else {
                    return XCTFail("Expected the genuine trailing-prose delta for \(variant.rawValue)")
                }
                XCTAssertFalse((Int(start)..<(Int(start) + Int(delete))).contains(rootBlock),
                    "\(variant.rawValue): the real table reference must remain in retained blocks")
                XCTAssertFalse(inserted.joined().contains { $0["type"] as? String == "table" }, variant.rawValue)
                switch variant {
                case .duplicateReference, .unknownReference:
                    let key = variant == .duplicateReference ? rootKey : RetainedSemanticAdmissionFixture.unknownTableKey
                    frame.snapshotJson = self.mutatedObjectJSON(frame.snapshotJson) { object in
                        var replacement = patch
                        var blocks = inserted
                        blocks.append([["type": "table", "tableId": key]])
                        replacement["renderBlocks"] = blocks
                        object["renderPatch"] = replacement
                    }
                    XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(frame.snapshotJson),
                        "\(variant.rawValue): the inserted reference must pass parser admission so the merged reference guard is exercised")
                case .removedIndexRoot:
                    XCTAssertFalse(frame.tables.removedTableKeys.contains(rootKey), variant.rawValue)
                    frame.tables.removedTableKeys.append(rootKey)
                    frame.tables.tables.removeAll { $0.tableKey == rootKey }
                    frame.tables.cellUpdates.removeAll { $0.tableKey == rootKey }
                    frame.tables.extents.removeAll { $0.tableKey == rootKey }
                    let staged = adapter.tableIndex.copy()
                    guard let revision = EditorV2Adapter.uint64Field(incoming, "documentVersion"),
                          case .success = staged.adopt(frame.tables,
                            installedRevision: adapter.installedFrameRevision, frameRevision: revision)
                    else {
                        return XCTFail("The real index removal must stage successfully before semantic/root consistency rejects it")
                    }
                    XCTAssertTrue(staged.tableKeys.isEmpty, variant.rawValue)
                    XCTAssertTrue(staged.rootExtents.isEmpty, variant.rawValue)
                }
            }
            XCTAssertEqual(adapter.tableIndex.tableKeys, [rootKey], variant.rawValue)
            XCTAssertEqual(adapter.tableIndex.record(tableKey: rootKey), originalRecord, variant.rawValue)
            XCTAssertEqual(adapter.tableIndex.rootExtents, originalExtents, variant.rawValue)
            XCTAssertTrue(try XCTUnwrap(adapter.cachedSemanticRenderBlocks).joined().contains {
                $0["type"] as? String == "table" && $0["tableId"] as? String == rootKey
            }, variant.rawValue)
        }
    }

    func testRealTableInsertionAndRemovalRestoreMatchedTableFreeDeltaAdmission() throws {
        try withRetainedSemanticAdapter(
            documentJson: RetainedSemanticAdmissionFixture.plainDocument,
            configJson: TableInputTestSchema.tableConfig
        ) { adapter in
            let originalDocument = documentJsonObject(adapter)
            let originalContent = try XCTUnwrap(originalDocument["content"] as? [[String: Any]])
            XCTAssertEqual(originalContent.compactMap { $0["type"] as? String }, ["paragraph", "paragraph"])
            let originalText = documentText(adapter)
            XCTAssertTrue(adapter.tableIndex.tableKeys.isEmpty)
            let caret = RetainedSemanticAdmissionFixture.caret
            _ = try XCTUnwrap(adapter.insertContentJson(TableInputTestSchema.twoCellDocument, anchor: caret, head: caret))
            let rootKey = try XCTUnwrap(adapter.tableIndex.rootExtents.keys.first)
            XCTAssertTrue(try XCTUnwrap(adapter.cachedSemanticRenderBlocks).joined().contains {
                $0["type"] as? String == "table" && $0["tableId"] as? String == rootKey
            })
            try assertFreshFullSemanticSnapshotMatches(adapter, phase: "prose to actual table")

            let admission = try XCTUnwrap(adapter.tableMutationAdmission(tableID: rootKey))
            _ = try XCTUnwrap(adapter.deleteTable(admission: admission))
            XCTAssertTrue(adapter.tableIndex.tableKeys.isEmpty)
            XCTAssertTrue(adapter.tableIndex.rootExtents.isEmpty)
            XCTAssertFalse(try XCTUnwrap(adapter.cachedSemanticRenderBlocks).joined().contains {
                $0["type"] as? String == "table"
            })
            XCTAssertEqual(documentText(adapter), originalText)
            let restoredContent = try XCTUnwrap(documentJsonObject(adapter)["content"] as? [[String: Any]])
            XCTAssertTrue(restoredContent.allSatisfy { $0["type"] as? String == "paragraph" })
            XCTAssertEqual(try XCTUnwrap(restoredContent.last) as NSDictionary,
                try XCTUnwrap(originalContent.last) as NSDictionary)
            try assertFreshFullSemanticSnapshotMatches(adapter, phase: "actual table back to prose")
            let scalarLength = try XCTUnwrap(adapter.cachedScalarLength)
            XCTAssertGreaterThan(scalarLength, RetainedSemanticAdmissionFixture.trailingCaretDistance)
            let trailingCaret = scalarLength - RetainedSemanticAdmissionFixture.trailingCaretDistance
            _ = try XCTUnwrap(adapter.syncSelection(anchor: trailingCaret, head: trailingCaret))
            XCTAssertEqual(adapter.cachedAuthoritativeScalarSelection?.anchor, trailingCaret)
            XCTAssertEqual(adapter.cachedAuthoritativeScalarSelection?.head, trailingCaret)
            try assertMatchedTableFreeSemanticDelta(adapter, phase: "native input after table removal") {
                adapter.insertText(RetainedSemanticAdmissionFixture.insertion, atScalar: trailingCaret)
            }
            let editedContent = try XCTUnwrap(documentJsonObject(adapter)["content"] as? [[String: Any]])
            let editedTrailingContent = try XCTUnwrap(editedContent.last?["content"] as? [[String: Any]])
            XCTAssertEqual(editedTrailingContent.compactMap { $0["text"] as? String }.joined(),
                RetainedSemanticAdmissionFixture.editedTrailingText)
            try assertFreshFullSemanticSnapshotMatches(adapter, phase: "post-transition delta against the same engine state")
        }
    }
}
