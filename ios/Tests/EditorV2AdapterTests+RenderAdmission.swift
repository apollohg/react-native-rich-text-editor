import UIKit
import XCTest

extension EditorV2AdapterTests {
    func testTableInputMappingIsRetainedInAtomicAndViewSnapshots() throws {
        let adapter = makeAdapter()
        let snapshot = try tableInputMappingSnapshot()

        XCTAssertNotNil(adapter.adoptExternalRender(snapshot))
        XCTAssertEqual(adapter.cachedTableInputMappings?.tables["t0"]?.cells.count, 1)
        let atomic = try XCTUnwrap(adapter.atomicRenderJSON(matchingDocumentRevision: 1))
        XCTAssertNotNil(parseObject(atomic)["tableInputMappings"])
        XCTAssertNotNil(parseObject(try XCTUnwrap(adapter.cachedViewUpdateJSON))["tableInputMappings"])
    }

    func testTableInputMappingRejectsOrphansAndInvalidCoordinatesAtomically() throws {
        let adapter = makeAdapter()
        let valid = try tableInputMappingSnapshot()
        XCTAssertNotNil(adapter.adoptExternalRender(valid))
        let baseline = adapter.cacheStateForTesting

        for mutate in [
            { (mapping: inout [String: Any]) in mapping["tables"] = [:] },
            { (mapping: inout [String: Any]) in
                var tables = mapping["tables"] as! [String: Any]
                var table = tables["t0"] as! [String: Any]
                var cells = table["cells"] as! [[String: Any]]
                cells[0]["sourceEnd"] = 7
                table["cells"] = cells
                tables["t0"] = table
                mapping["tables"] = tables
            },
            { (mapping: inout [String: Any]) in
                var tables = mapping["tables"] as! [String: Any]
                var table = tables["t0"] as! [String: Any]
                var cells = table["cells"] as! [[String: Any]]
                var blocks = cells[0]["blocks"] as! [[String: Any]]
                blocks[0]["scalarEnd"] = 5
                cells[0]["blocks"] = blocks
                table["cells"] = cells
                tables["t0"] = table
                mapping["tables"] = tables
            },
            { (mapping: inout [String: Any]) in
                var tables = mapping["tables"] as! [String: Any]
                var table = tables["t0"] as! [String: Any]
                table["extent"] = ["scalarStart": -1, "scalarEnd": 4]
                tables["t0"] = table
                mapping["tables"] = tables
            }
        ] {
            let malformed = mutatedObjectJSON(valid) { object in
                var mapping = object["tableInputMappings"] as! [String: Any]
                mutate(&mapping)
                object["tableInputMappings"] = mapping
            }
            XCTAssertNil(adapter.adoptExternalRender(malformed))
            XCTAssertEqual(adapter.cacheStateForTesting, baseline)
        }
    }

    func testMissingTableInputMappingAndOwnerReleaseClearCachedMapping() throws {
        let adapter = makeAdapter()
        XCTAssertNotNil(adapter.adoptExternalRender(try tableInputMappingSnapshot()))
        XCTAssertNotNil(adapter.cachedTableInputMappings)

        let legacy = mutatedObjectJSON(try tableInputMappingSnapshot()) { $0.removeValue(forKey: "tableInputMappings") }
        XCTAssertNotNil(adapter.adoptExternalRender(legacy))
        XCTAssertNil(adapter.cachedTableInputMappings)

        let owner = UUID()
        adapter.claimNativeBindingIfUnowned(token: owner)
        XCTAssertNil(adapter.cachedTableInputMappings)
        let native = mutatedObjectJSON(try tableInputMappingSnapshot()) { $0["positionEpoch"] = "1" }
        XCTAssertNotNil(adapter.adoptExternalRender(native))
        adapter.releaseNativeBindingOwner(token: owner)
        XCTAssertNil(adapter.cachedTableInputMappings)
    }

    func testFailedNativeRecoveryClearsTableInputMapping() throws {
        let adapter = makeAdapter()
        adapter.claimNativeBindingIfUnowned(token: UUID())
        let native = mutatedObjectJSON(try tableInputMappingSnapshot()) { $0["positionEpoch"] = "1" }
        XCTAssertNotNil(adapter.adoptExternalRender(native))
        XCTAssertNotNil(adapter.cachedTableInputMappings)
        XCTAssertNil(editorV2Destroy(editorId: adapter.editorId).error)

        XCTAssertNil(adapter.recoverNativeRender())
        XCTAssertNil(adapter.cachedTableInputMappings)
    }

    func testFailedExternalPositionPinClearsTableInputMapping() throws {
        let adapter = makeAdapter()
        adapter.claimNativeBindingIfUnowned(token: UUID())
        let stale = mutatedObjectJSON(try tableInputMappingSnapshot()) { $0["documentVersion"] = "999" }

        XCTAssertNil(adapter.adoptExternalRender(stale))
        XCTAssertNil(adapter.cachedTableInputMappings)
    }

    func testTableInputMappingAllowsSparseListBlockMappingWithZeroLeafSibling() throws {
        let adapter = makeAdapter()
        let snapshot = try tableInputMappingSnapshot()
        let valid = mutatedObjectJSON(snapshot) { object in
            var records = object["tableRecords"] as! [String: Any]
            var table = records["t0"] as! [String: Any]
            var cells = table["cells"] as! [[String: Any]]
            var first = cells[0]
            first["sourceEnd"] = 14
            first["elements"] = [
                ["type": "blockStart", "nodeType": "customList", "depth": 0],
                ["type": "blockStart", "nodeType": "item", "depth": 1,
                 "listContext": ["ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true]],
                ["type": "blockStart", "nodeType": "paragraph", "depth": 2],
                ["type": "textRun", "text": "base", "marks": []],
                ["type": "blockEnd"], ["type": "blockEnd"], ["type": "blockEnd"]
            ]
            cells[0] = first
            var zeroLeaf = first
            zeroLeaf["sourcePos"] = 14
            zeroLeaf["sourceEnd"] = 16
            zeroLeaf["column"] = 1
            zeroLeaf["contentKey"] = "zero-leaf"
            zeroLeaf["elements"] = []
            cells.append(zeroLeaf)
            table["sourceEnd"] = 18
            table["columns"] = 2
            table["columnWidths"] = [NSNull(), NSNull()]
            table["sourceRows"] = [["sourcePos": 1, "sourceEnd": 17, "attrsKey": String(repeating: "a", count: 64)]]
            table["cells"] = cells
            records["t0"] = table
            object["tableRecords"] = records

            var mapping = object["tableInputMappings"] as! [String: Any]
            var tables = mapping["tables"] as! [String: Any]
            var mappedTable = tables["t0"] as! [String: Any]
            var mappedCells = mappedTable["cells"] as! [[String: Any]]
            mappedTable["extent"] = ["scalarStart": 0, "scalarEnd": 6]
            mappedCells[0]["sourceEnd"] = 14
            mappedCells[0]["blocks"] = [["elementIndex": 2, "docStart": 6, "docEnd": 10,
                "scalarStart": 0, "contentScalarStart": 2, "scalarEnd": 6,
                "breakScalarEnd": 6, "void": false]]
            mappedCells.append(["cellIndex": 1, "sourcePos": 14, "sourceEnd": 16, "blocks": [], "excluded": []])
            mappedTable["cells"] = mappedCells
            tables["t0"] = mappedTable
            mapping["tables"] = tables
            object["tableInputMappings"] = mapping
            object["scalarLength"] = 6
        }
        XCTAssertNotNil(adapter.adoptExternalRender(valid))
    }

    func testTableInputMappingRequiresNestedTableExclusionWithMatchingExtent() throws {
        let adapter = makeAdapter()
        let snapshot = try tableInputMappingSnapshot()
        let valid = mutatedObjectJSON(snapshot) { object in
            let attrsKey = String(repeating: "a", count: 64)
            var records = object["tableRecords"] as! [String: Any]
            var outer = records["t0"] as! [String: Any]
            var cells = outer["cells"] as! [[String: Any]]
            var cell = cells[0]
            var elements = cell["elements"] as! [[String: Any]]
            cell["sourceEnd"] = 12
            elements.append(["type": "table", "tableId": "t9"])
            cell["elements"] = elements
            cells[0] = cell
            outer["cells"] = cells
            outer["sourceEnd"] = 14
            outer["sourceRows"] = [["sourcePos": 1, "sourceEnd": 13, "attrsKey": attrsKey]]
            records["t0"] = outer
            records["t9"] = [
                "tablePos": 9, "sourceEnd": 11, "rows": 0, "columns": 0, "columnWidths": [],
                "direction": NSNull(), "irregular": false, "readOnlyDescendants": true, "attrsKey": attrsKey,
                "sourceRows": [], "cells": [], "syntheticRegions": [], "failure": "invalidStructure", "compatibilityDiagnostic": NSNull()
            ]
            object["tableRecords"] = records
            var mapping = object["tableInputMappings"] as! [String: Any]
            var tables = mapping["tables"] as! [String: Any]
            var outerMapping = tables["t0"] as! [String: Any]
            var mappedCells = outerMapping["cells"] as! [[String: Any]]
            mappedCells[0]["sourceEnd"] = 12
            mappedCells[0]["excluded"] = [["elementIndex": 3, "tableId": "t9", "extent": NSNull()]]
            outerMapping["cells"] = mappedCells
            tables["t0"] = outerMapping
            tables["t9"] = ["extent": NSNull(), "cells": []]
            mapping["tables"] = tables
            object["tableInputMappings"] = mapping
        }
        XCTAssertNotNil(adapter.adoptExternalRender(valid))
        let missingExclusion = mutatedObjectJSON(valid) { object in
            var mapping = object["tableInputMappings"] as! [String: Any]
            var tables = mapping["tables"] as! [String: Any]
            var outer = tables["t0"] as! [String: Any]
            var cells = outer["cells"] as! [[String: Any]]
            cells[0]["excluded"] = []
            outer["cells"] = cells
            tables["t0"] = outer
            mapping["tables"] = tables
            object["tableInputMappings"] = mapping
        }
        XCTAssertNil(adapter.adoptExternalRender(missingExclusion))
    }

    private func tableInputMappingSnapshot() throws -> String {
        let adapter = makeAdapter()
        _ = adapter.setContentHtml("<p>base</p>")
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: adapter.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        let attrsKey = String(repeating: "a", count: 64)
        let table: [String: Any] = [
            "tablePos": 0, "sourceEnd": 12, "rows": 1, "columns": 1,
            "columnWidths": [NSNull()], "direction": NSNull(), "irregular": false,
            "readOnlyDescendants": false, "attrsKey": attrsKey,
            "sourceRows": [["sourcePos": 1, "sourceEnd": 11, "attrsKey": attrsKey]],
            "syntheticRegions": [], "failure": NSNull(), "compatibilityDiagnostic": NSNull(),
            "cells": [["sourcePos": 2, "sourceEnd": 10, "row": 0, "column": 0,
                "rowspan": 1, "colspan": 1, "header": false, "attrsKey": attrsKey,
                "contentKey": "cell", "elements": [["type": "blockStart", "nodeType": "paragraph", "depth": 0], ["type": "textRun", "text": "base", "marks": []], ["type": "blockEnd"]]]]
        ]
        return mutatedObjectJSON(raw) { object in
            object["renderBlocks"] = [[ ["type": "table", "tableId": "t0"] ]]
            object["tableAttributes"] = [attrsKey: "{}"]
            object["tableRecords"] = ["t0": table]
            object["scalarLength"] = 4
            object["tableInputMappings"] = ["version": 1, "tables": ["t0": [
                "extent": ["scalarStart": 0, "scalarEnd": 4],
                "cells": [["cellIndex": 0, "sourcePos": 2, "sourceEnd": 10,
                    "blocks": [["elementIndex": 0, "docStart": 4, "docEnd": 8,
                        "scalarStart": 0, "contentScalarStart": 0, "scalarEnd": 4,
                        "breakScalarEnd": 4, "void": false]], "excluded": []]]
            ]]]
        }
    }

    func testSemanticTableAdmissionRetainsSnapshotOnMalformedPatch() throws {
        let adapter = makeAdapter()
        let attrsKey = String(repeating: "a", count: 64)
        _ = adapter.setContentHtml("<p>base</p>")
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: adapter.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        let table: [String: Any] = [
            "tablePos": 0, "sourceEnd": 10, "rows": 1, "columns": 1,
            "columnWidths": [NSNull()], "direction": NSNull(), "irregular": false,
            "readOnlyDescendants": false, "attrsKey": attrsKey,
            "sourceRows": [["sourcePos": 1, "sourceEnd": 9, "attrsKey": attrsKey]],
            "syntheticRegions": [], "failure": NSNull(), "compatibilityDiagnostic": NSNull(),
            "cells": [["sourcePos": 2, "sourceEnd": 8, "row": 0, "column": 0,
                       "rowspan": 1, "colspan": 1, "header": false, "attrsKey": attrsKey,
                       "contentKey": "same", "elements": [["type": "textRun", "text": "base", "marks": []]]]]
        ]
        let valid = mutatedObjectJSON(raw) {
            $0["renderBlocks"] = [[ ["type": "table", "tableId": "t0"] ]]
            $0["tableAttributes"] = [attrsKey: "{}"]
            $0["tableRecords"] = ["t0": table]
        }
        let pool = try XCTUnwrap(EditorV2Adapter.parseTableAttributes([attrsKey: "{}"]))
        XCTAssertTrue(EditorV2Adapter.validSemanticRenderElements([["type": "table", "tableId": "t0"]], tableAttributes: pool, tableRecords: ["t0": table]))
        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(valid))
        XCTAssertNotNil(adapter.adoptExternalRender(valid))
        let retainedNoop = mutatedObjectJSON(valid) {
            $0["renderBlocks"] = NSNull()
            $0["renderPatch"] = ["baseDocumentVersion": $0["documentVersion"]!, "startIndex": 0,
                "deleteCount": 0, "renderBlocks": []]
        }
        XCTAssertNotNil(adapter.adoptExternalRender(retainedNoop))
        let baseline = adapter.cacheStateForTesting
        let missingRetainedReference = mutatedObjectJSON(valid) {
            $0["renderBlocks"] = NSNull()
            $0["tableAttributes"] = [String: String]()
            $0["renderPatch"] = ["baseDocumentVersion": $0["documentVersion"]!, "startIndex": 0,
                "deleteCount": 0, "renderBlocks": []]
        }
        XCTAssertNil(adapter.adoptExternalRender(missingRetainedReference))
        XCTAssertEqual(adapter.cacheStateForTesting, baseline)
        for invalid in ["failure", "compatibilityDiagnostic", "columns", "attrsJson"] {
            var changed = table
            changed[invalid] = invalid == "columns" ? 0 : invalid == "attrsJson" ? "{\"width\":1e309}" : "unknown"
            let patch = mutatedObjectJSON(raw) { object in
                object["renderBlocks"] = NSNull()
                object["tableAttributes"] = [attrsKey: "{}"]
                object["tableRecords"] = ["t0": changed]
                object["renderPatch"] = ["baseDocumentVersion": object["documentVersion"]!, "startIndex": 0,
                    "deleteCount": 1, "renderBlocks": [[ ["type": "table", "tableId": "t0"] ]]]
            }
            XCTAssertNil(adapter.adoptExternalRender(patch), invalid)
            XCTAssertEqual(adapter.cacheStateForTesting, baseline, invalid)
        }
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

        XCTAssertNotNil(adapter.adoptExternalRender(withPatch))
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

        XCTAssertNotNil(adapter.adoptExternalRender(withMention))
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
            return adapter.adoptExternalRender(snapshot)
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
        XCTAssertNotNil(adapter.adoptExternalRender(raw))
        let baseline = adapter.cacheStateForTesting
        let baselineDebugNotes = adapter.debugNotes

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
            XCTAssertNil(adapter.adoptExternalRender(malformed), name)
            XCTAssertEqual(adapter.cacheStateForTesting, baseline, name)
            XCTAssertEqual(adapter.debugNotes, baselineDebugNotes, name)
            XCTAssertEqual(spy.errors.count, errorsBefore + 1, name)
            XCTAssertEqual(spy.last?.domain, "boundary", name)
            XCTAssertEqual(spy.last?.code, "FFI_RESULT_INVALID", name)
        }
    }

}
