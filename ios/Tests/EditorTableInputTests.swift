import CoreText
import XCTest

final class EditorTableInputTests: XCTestCase {
    private let tableConfig = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
    private let listTableConfig = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"bulletList","content":"listItem+","group":"block","role":"list"},{"name":"listItem","content":"block+","role":"listItem"},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#

    func testEngineCellSelectionAdmitsAuthoritativeSnapshot() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"three"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 360, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let record = try XCTUnwrap(adapter.cachedTableRecords.values.first)
        let cells = try XCTUnwrap(record["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        let firstScalar = EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))
        let secondScalar = EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))
        let result = editorV2SetSelection(
            editorId: adapter.editorId,
            requestJson: #"{"version":1,"requestId":"991102","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(firstScalar),"kind":"scalar"},"headCell":{"offset":\#(secondScalar),"kind":"scalar"}}}"#
        )
        XCTAssertNil(result.error)
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: adapter.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
        let selection = try XCTUnwrap(object["selection"] as? [String: Any])
        XCTAssertEqual(selection["type"] as? String, "cell")
        XCTAssertEqual(selection["anchorCell"] as? Int, first)
        XCTAssertEqual(selection["headCell"] as? Int, second)
        XCTAssertFalse(raw.isEmpty)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        let snapshot = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        XCTAssertTrue(snapshot.contains(#""type":"cell""#))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: ["table": ["selectionColor": "#FF000080"]])))
        view.layoutIfNeeded()
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let block = try XCTUnwrap(drawing.layout?.blocks.first)
        let table = try XCTUnwrap(block.tableSurface)
        let origin = try XCTUnwrap(block.tableBounds).origin
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        let image = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        let cgImage = try XCTUnwrap(image.cgImage)
        var pixels = [UInt8](repeating: 0, count: cgImage.width * cgImage.height * 4)
        let bitmap = try XCTUnwrap(CGContext(data: &pixels, width: cgImage.width, height: cgImage.height,
                                              bitsPerComponent: 8, bytesPerRow: cgImage.width * 4,
                                              space: CGColorSpaceCreateDeviceRGB(),
                                              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        bitmap.draw(cgImage, in: CGRect(x: 0, y: 0, width: cgImage.width, height: cgImage.height))
        let selectedPositions = drawing.selectedTableCellSourcePositions
        drawing.selectedTableCellSourcePositions = [:]
        let baselineImage = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        drawing.selectedTableCellSourcePositions = selectedPositions
        let baselineCG = try XCTUnwrap(baselineImage.cgImage)
        var baselinePixels = [UInt8](repeating: 0, count: baselineCG.width * baselineCG.height * 4)
        let baselineBitmap = try XCTUnwrap(CGContext(data: &baselinePixels, width: baselineCG.width, height: baselineCG.height,
                                                      bitsPerComponent: 8, bytesPerRow: baselineCG.width * 4,
                                                      space: CGColorSpaceCreateDeviceRGB(),
                                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        baselineBitmap.draw(baselineCG, in: CGRect(x: 0, y: 0, width: baselineCG.width, height: baselineCG.height))
        let offset = { (cell: PreparedViewerTableCell) -> Int in
            let x = Int(origin.x + cell.frame.maxX - 6)
            let y = Int(origin.y + cell.frame.maxY - 6)
            return (y * cgImage.width + x) * 4
        }
        let alpha = { (cell: PreparedViewerTableCell) -> UInt8 in
            pixels[offset(cell) + 3]
        }
        XCTAssertGreaterThan(alpha(table.cells[0]), alpha(table.cells[2]))
        XCTAssertGreaterThan(alpha(table.cells[1]), alpha(table.cells[2]))
        for cell in table.cells.prefix(2) {
            let index = offset(cell)
            XCTAssertGreaterThan(pixels[index], pixels[index + 1])
            XCTAssertGreaterThan(pixels[index], pixels[index + 2])
            XCTAssertGreaterThan(pixels[index + 3], baselinePixels[index + 3])
        }
        let third = offset(table.cells[2])
        XCTAssertEqual(Array(pixels[third..<(third + 4)]), Array(baselinePixels[third..<(third + 4)]))
        let attachment = XCTAttachment(image: image)
        attachment.name = "Native-rendered iPhone 17 light cell selection"
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testCellSelectionPreservesFocusedCellInputAndRejectsStaleTyping() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 360, height: 220))
        let view = RichTextEditorView(frame: window.bounds)
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let cells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: CGRect(x: 0, y: 0, width: 100, height: 50)))
        let input = view.activeTextInput
        XCTAssertTrue(input.becomeFirstResponder())
        let request = #"{"version":1,"requestId":"991103","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: request).error)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertFalse(input.isFirstResponder)
        XCTAssertTrue(view.textView.isFirstResponder)
        XCTAssertTrue(view.activeTextInput === view.textView)
        let before = try XCTUnwrap(adapter.documentJson())
        input.insertText("unsafe")
        view.textView.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        let root = view.textView
        let afterRange = (root.text as NSString).range(of: "after")
        XCTAssertNotEqual(afterRange.location, NSNotFound)
        root.selectedRange = NSRange(location: afterRange.location, length: 0)
        root.textViewDidChangeSelection(root)
        XCTAssertTrue(root.authoritativeCellSelectionActive)
        XCTAssertTrue(root.rootTableSelectionInputBlocked)
        root.insertText("unsafe")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)

        root.layoutManager.ensureLayout(forCharacterRange: afterRange)
        let glyphs = root.layoutManager.glyphRange(forCharacterRange: afterRange, actualCharacterRange: nil)
        let rect = root.layoutManager.boundingRect(forGlyphRange: glyphs, in: root.textContainer)
        let point = CGPoint(x: rect.minX + root.textContainerInset.left + 2,
                            y: rect.midY + root.textContainerInset.top)
        XCTAssertTrue(root.placeCaret(at: point))
        flushMainQueue()
        XCTAssertFalse(root.authoritativeCellSelectionActive)
        XCTAssertFalse(root.rootTableSelectionInputBlocked)
        let surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        XCTAssertTrue(drawing.selectedTableCellSourcePositions.isEmpty)
        let proseSnapshot = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        let proseObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(proseSnapshot.utf8)) as? [String: Any])
        XCTAssertEqual((proseObject["selection"] as? [String: Any])?["type"] as? String, "text")
        root.insertText("!")
        XCTAssertTrue(try XCTUnwrap(adapter.documentJson()).contains(#""text":"!after""#))

        let nextRequest = #"{"version":1,"requestId":"991104","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: nextRequest).error)
        XCTAssertTrue(root.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        let cellFrame = try XCTUnwrap(surface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertTrue(view.activateTableCell(at: CGPoint(x: cellFrame.midX, y: cellFrame.midY)))
        let cellInput = view.activeTextInput
        flushMainQueue()
        XCTAssertTrue(drawing.selectedTableCellSourcePositions.isEmpty)
        XCTAssertFalse(root.authoritativeCellSelectionActive)
        XCTAssertEqual(cellInput.tableCellPositionMap?.binding.positionEpoch, adapter.positionEpoch)
        let cellSnapshot = try XCTUnwrap(adapter.cachedAtomicRenderJSON)
        let cellObject = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(cellSnapshot.utf8)) as? [String: Any])
        XCTAssertEqual((cellObject["selection"] as? [String: Any])?["type"] as? String, "text")
        let beforeCellTap = try XCTUnwrap(adapter.documentJson())
        cellInput.insertText("?")
        let editedJSON = try XCTUnwrap(adapter.documentJson())
        XCTAssertNotEqual(editedJSON, beforeCellTap)
        let edited = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(editedJSON.utf8)) as? [String: Any])
        let topLevel = try XCTUnwrap(edited["content"] as? [[String: Any]])
        let rows = try XCTUnwrap(topLevel[0]["content"] as? [[String: Any]])
        let editedCells = try XCTUnwrap(rows[0]["content"] as? [[String: Any]])
        let firstCellJSON = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: editedCells[0]), encoding: .utf8))
        XCTAssertTrue(firstCellJSON.contains("?"))
        XCTAssertFalse(try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: editedCells[1]), encoding: .utf8)).contains("?"))
        XCTAssertFalse(try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: topLevel[1]), encoding: .utf8)).contains("?"))
    }

    func testCellSelectionResolverUsesRealSourceCellsAndClosesMergedSpans() {
        func cell(_ pos: Int, _ row: Int, _ column: Int, colspan: Int = 1) -> [String: Any] {
            ["sourcePos": pos, "row": row, "column": column, "rowspan": 1, "colspan": colspan]
        }
        let records: [String: [String: Any]] = [
            "t1": ["tablePos": 1, "sourceEnd": 40, "rows": 2, "columns": 3,
                   "direction": "rtl", "failure": NSNull(),
                   "cells": [cell(3, 0, 0), cell(7, 0, 1, colspan: 2),
                             cell(13, 1, 0), cell(17, 1, 1), cell(21, 1, 2)],
                   "syntheticRegions": [["row": 0, "column": 2]]],
            "t40": ["tablePos": 40, "sourceEnd": 70, "rows": 1, "columns": 1,
                    "failure": NSNull(), "cells": [cell(42, 0, 0)]]
        ]
        let selection: [String: Any] = ["type": "cell", "anchorCell": 3, "headCell": 17]
        XCTAssertEqual(EditorCellSelection.resolve(selection, records: records),
                       .drawable(tableID: "t1", sourcePositions: Set([3, 7, 13, 17, 21])))
        XCTAssertNil(EditorCellSelection.resolve(selection.merging(["anchorScalar": 0]) { _, new in new }, records: records))
        XCTAssertNil(EditorCellSelection.resolve(["type": "cell", "anchorCell": 3.5, "headCell": 17], records: records))
        XCTAssertNil(EditorCellSelection.resolve(["type": "cell", "anchorCell": 3, "headCell": 42], records: records))
        let failed: [String: [String: Any]] = ["t1": ["tablePos": 1, "sourceEnd": 40,
                                                   "failure": "gridLimit", "cells": []]]
        XCTAssertEqual(EditorCellSelection.resolve(selection, records: failed), .unavailable(tableID: "t1"))
        let chained: [String: [String: Any]] = ["t1": ["tablePos": 1, "sourceEnd": 40,
            "rows": 3, "columns": 3, "failure": NSNull(), "direction": "rtl",
            "cells": [
                ["sourcePos": 3, "row": 0, "column": 0, "rowspan": 2, "colspan": 1],
                ["sourcePos": 7, "row": 2, "column": 0, "rowspan": 1, "colspan": 2],
                ["sourcePos": 11, "row": 1, "column": 1, "rowspan": 1, "colspan": 1],
                ["sourcePos": 15, "row": 1, "column": 2, "rowspan": 2, "colspan": 1]
            ]]]
        let forward: [String: Any] = ["type": "cell", "anchorCell": 11, "headCell": 15]
        let backward: [String: Any] = ["type": "cell", "anchorCell": 15, "headCell": 11]
        let closed = EditorCellSelection.drawable(tableID: "t1", sourcePositions: Set([3, 7, 11, 15]))
        XCTAssertEqual(EditorCellSelection.resolve(forward, records: chained), closed)
        XCTAssertEqual(EditorCellSelection.resolve(backward, records: chained), closed)
    }

    func testExpoOwnerPublishesAuthoritativeCellSelectionShape() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let expoHost = NativeEditorExpoView()
        defer { expoHost.setEditorId(0) }
        expoHost.setEditorId(editorId)
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
        XCTAssertTrue(expoHost.richTextView.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let cells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        XCTAssertTrue(expoHost.richTextView.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let staleCell = expoHost.richTextView.activeTextInput
        let request = #"{"version":1,"requestId":"991105","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: request).error)
        let update = EditorV2Shadow.getCurrentState(id: editorId)
        XCTAssertTrue(expoHost.richTextView.textView.applyUpdateJSON(update))
        XCTAssertTrue(expoHost.ownsNativeBinding(editorId: editorId))
        XCTAssertTrue(expoHost.richTextView.activeTextInput === expoHost.richTextView.textView)
        XCTAssertEqual(staleCell.editorId, 0)
        let event = try XCTUnwrap(NativeEditorExpoView.nativeCommitEventPayload(
            originatingEditorId: adapter.editorId, updateJSON: update
        ))
        XCTAssertEqual(Set(event.keys), ["editorId", "documentRevision", "updateJson"])
        XCTAssertEqual(event["editorId"] as? String, adapter.editorId)
        let atomic = try XCTUnwrap(event["updateJson"] as? String)
        let payload = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(atomic.utf8)) as? [String: Any])
        let selection = try XCTUnwrap(payload["selection"] as? [String: Any])
        XCTAssertEqual(Set(selection.keys), ["type", "anchorCell", "headCell"])
        XCTAssertEqual(selection["anchorCell"] as? Int, first)
        XCTAssertEqual(selection["headCell"] as? Int, second)
    }

    func testSyntheticPreservedFailureFrameAdmitsCellSelectionWithoutGeometry() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"one"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"two"}]}]}]}]}]}"#
        _ = try XCTUnwrap(adapter.setContentJson(document))
        let tableID = try XCTUnwrap(adapter.cachedTableRecords.keys.first)
        let cells = try XCTUnwrap(adapter.cachedTableRecords[tableID]?["cells"] as? [[String: Any]])
        let first = try XCTUnwrap(cells[0]["sourcePos"] as? Int)
        let second = try XCTUnwrap(cells[1]["sourcePos"] as? Int)
        let request = #"{"version":1,"requestId":"991106","baseDocumentRevision":"\#(adapter.baseDocumentRevision)","selection":{"type":"cell","anchorCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(first + 2))),"kind":"scalar"},"headCell":{"offset":\#(EditorV2Shadow.docToScalar(id: editorId, docPos: UInt32(second + 2))),"kind":"scalar"}}}"#
        XCTAssertNil(editorV2SetSelection(editorId: adapter.editorId, requestJson: request).error)
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: adapter.editorId, mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        var snapshot = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
        var records = try XCTUnwrap(snapshot["tableRecords"] as? [String: [String: Any]])
        var record = try XCTUnwrap(records[tableID])
        record["rows"] = 0
        record["columns"] = 0
        record["columnWidths"] = []
        record["sourceRows"] = []
        record["cells"] = []
        record["syntheticRegions"] = []
        record["failure"] = "gridLimit"
        record["compatibilityDiagnostic"] = NSNull()
        records[tableID] = record
        snapshot["tableRecords"] = records
        let attributes = try XCTUnwrap(snapshot["tableAttributes"] as? [String: String])
        let tableAttrsKey = try XCTUnwrap(record["attrsKey"] as? String)
        snapshot["tableAttributes"] = attributes.filter { $0.key == tableAttrsKey }
        snapshot.removeValue(forKey: "tableInputMappings")
        let selection = try XCTUnwrap(snapshot["selection"] as? [String: Any])
        XCTAssertEqual(EditorCellSelection.resolve(selection, records: records), .unavailable(tableID: tableID))
        let failureJSON = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: snapshot), encoding: .utf8))
        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(failureJSON))

        var malformed = snapshot
        malformed["selection"] = selection.merging(["anchorScalar": 0]) { _, new in new }
        let malformedJSON = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: malformed), encoding: .utf8))
        XCTAssertNil(EditorV2Adapter.parseAtomicRenderSnapshot(malformedJSON))
    }

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

    func testRootTableUsesPreparedViewerPresentation() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let layout = try XCTUnwrap(drawing.layout)
        XCTAssertFalse(drawing.isOpaque)
        XCTAssertEqual(layout.blocks.compactMap(\.tableSurface).count, 1)
        XCTAssertEqual(layout.blocks.compactMap(\.tableSurface).first?.cells.count, 1)

        var paintedCells = 0
        var paintedText = 0
        drawing.onTableChromeDrawnForTesting = { _ in paintedCells += 1 }
        drawing.onTableRichFragmentDrawnForTesting = { paintedText += 1 }
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        _ = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
            drawing.draw(drawing.bounds)
        }
        XCTAssertGreaterThan(paintedCells, 0)
        XCTAssertGreaterThan(paintedText, 0)
    }

    func testRootTableKeepsScalarMarkerAndTracksItsRealCellAfterScroll() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 100))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let extent = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.extent)
        let marker = (view.textView.text as NSString).range(of: "\u{200B}")
        XCTAssertNotEqual(marker.location, NSNotFound)
        XCTAssertEqual(PositionBridge.utf16OffsetToScalar(marker.location, in: view.textView), extent.scalarStart)
        XCTAssertEqual(PositionBridge.utf16OffsetToScalar(NSMaxRange(marker), in: view.textView), extent.scalarEnd)

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let contentLayout = try XCTUnwrap(drawing.layout)
        let tableBounds = try XCTUnwrap(drawing.layout?.blocks.first?.tableBounds)
        let after = (view.textView.text as NSString).range(of: "after")
        XCTAssertNotEqual(after.location, NSNotFound)
        view.textView.layoutManager.ensureLayout(forCharacterRange: after)
        let afterGlyphs = view.textView.layoutManager.glyphRange(forCharacterRange: after, actualCharacterRange: nil)
        let afterLine = view.textView.layoutManager.lineFragmentRect(forGlyphAt: afterGlyphs.location, effectiveRange: nil)
        XCTAssertGreaterThanOrEqual(
            view.textView.textContainerInset.top + afterLine.minY,
            tableBounds.maxY - 0.5
        )
        let firstFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        let firstCenter = CGPoint(x: firstFrame.midX, y: firstFrame.midY)
        XCTAssertEqual(tableSurface.cellHit(at: firstCenter)?.tableID, tableID)
        XCTAssertEqual(tableSurface.cellHit(at: firstCenter)?.cellIndex, 0)

        let renderCalls = adapter.renderUpdateCallCountForTesting
        view.textView.contentOffset.y += 12
        tableSurface.updateGeometry(from: view.textView)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, renderCalls)
        XCTAssertTrue(drawing.layout === contentLayout)
        let scrolledFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertEqual(scrolledFrame.minY, firstFrame.minY - 12, accuracy: 0.5)
        XCTAssertNil(tableSurface.cellHit(at: CGPoint(x: tableBounds.maxX + 1, y: scrolledFrame.midY)))
    }

    func testRootTableRepreparesForAppearanceAndHostWidthWithoutDocumentUpdate() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let initialLayout = try XCTUnwrap(drawing.layout)
        let initialWidth = try XCTUnwrap(initialLayout.blocks.first?.tableSurface?.hostViewportWidth)
        let renderCalls = adapter.renderUpdateCallCountForTesting

        view.configure(font: .systemFont(ofSize: 22))
        let appearanceLayout = try XCTUnwrap(drawing.layout)
        XCTAssertFalse(initialLayout === appearanceLayout)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, renderCalls)

        view.frame.size.width = 220
        view.layoutIfNeeded()

        let resizedWidth = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.hostViewportWidth)
        XCTAssertLessThan(resizedWidth, initialWidth)
        XCTAssertEqual(adapter.renderUpdateCallCountForTesting, renderCalls)
    }

    func testRootTableProjectsEditorThemeIntoInactiveCells() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
            "text": ["fontSize": 22, "color": "#FF0000"],
            "table": ["cellPadding": 14]
        ])))

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let cell = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.cells.first)
        let line = try XCTUnwrap(cell.content.blocks.flatMap(\.fragments).first(where: { $0.kind == .text })?.line)
        let run = try XCTUnwrap((CTLineGetGlyphRuns(line) as? [CTRun])?.first)
        let attributes = CTRunGetAttributes(run) as NSDictionary
        let color = try unwrapCoreTextAttribute(attributes[kCTForegroundColorAttributeName], as: CGColor.self)

        XCTAssertEqual(UIColor(cgColor: color), UIColor.red)
        XCTAssertEqual(drawing.layout?.blocks.first?.tableSurface?.style.cellPadding, 14)
    }

    func testRootTableUsesHostInsetWidthOnce() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
            "version": 1,
            "styles": ["content": ["paddingLeft": 20, "paddingRight": 13]]
        ])))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let prepared = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)
        XCTAssertEqual(view.textView.textContainerInset.left, 20)
        XCTAssertEqual(view.textView.textContainerInset.right, 13)
        XCTAssertEqual(prepared.hostViewportWidth, 287, accuracy: 0.5)
    }

    func testActiveHeaderCellSuppressesOnlyItsPreparedContent() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"active"}]}]},{"type":"table_header","content":[{"type":"paragraph","content":[{"type":"text","text":"inactive"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let block = try XCTUnwrap(drawing.layout?.blocks.first)
        let surface = try XCTUnwrap(block.tableSurface)
        let tableBounds = try XCTUnwrap(block.tableBounds)
        let activeHeader = try XCTUnwrap(surface.cells.first { $0.sourceCellIndex == 0 })
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        func paint() -> (rich: Int, chrome: Int, image: CGImage?) {
            var rich = 0
            var chrome = 0
            drawing.onTableRichFragmentDrawnForTesting = { rich += 1 }
            drawing.onTableChromeDrawnForTesting = { _ in chrome += 1 }
            let image = UIGraphicsImageRenderer(size: drawing.bounds.size, format: format).image { _ in
                drawing.draw(drawing.bounds)
            }.cgImage
            return (rich, chrome, image)
        }

        let unbound = paint()
        XCTAssertGreaterThanOrEqual(unbound.rich, 2)
        XCTAssertEqual(unbound.chrome, 2)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let bound = paint()
        XCTAssertGreaterThan(bound.rich, 0)
        XCTAssertLessThan(bound.rich, unbound.rich)
        XCTAssertEqual(bound.chrome, 2)

        let image = try XCTUnwrap(bound.image)
        let point = CGPoint(x: tableBounds.minX + activeHeader.frame.minX + 3, y: tableBounds.minY + activeHeader.frame.minY + 3)
        var pixels = [UInt8](repeating: 0, count: image.width * image.height * 4)
        let context = try XCTUnwrap(CGContext(data: &pixels, width: image.width, height: image.height, bitsPerComponent: 8, bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
        context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
        let pixelIndex = Int(point.y) * image.width * 4 + Int(point.x) * 4
        var red: CGFloat = 0
        var green: CGFloat = 0
        var blue: CGFloat = 0
        var alpha: CGFloat = 0
        XCTAssertTrue(surface.style.headerBackgroundColor.getRed(&red, green: &green, blue: &blue, alpha: &alpha))
        XCTAssertEqual(Array(pixels[pixelIndex..<(pixelIndex + 4)]), [red, green, blue, alpha].map { UInt8(($0 * 255).rounded()) })

        tableSurface.invalidateAppearance()
        tableSurface.updateGeometry(from: view.textView)
        let refreshedCell = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.cells.first { $0.sourceCellIndex == 0 })
        XCTAssertFalse(refreshedCell.content === activeHeader.content)
        XCTAssertEqual(paint().rich, bound.rich)

        view.invalidateTableCellBinding()
        let restored = paint()
        XCTAssertEqual(restored.rich, unbound.rich)
        XCTAssertEqual(restored.chrome, 2)
    }

    func testActiveCellInputStaysTransparentAcrossRootBackgrounds() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)

        for color in [UIColor.red, UIColor.clear] {
            view.textView.baseBackgroundColor = color
            view.textView.backgroundColor = color
            XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
            XCTAssertEqual(view.activeTextInput.backgroundColor, .clear)
            XCTAssertFalse(view.activeTextInput.isOpaque)
        }
    }

    func testRootTableMarginsOffsetPaintAndReserveFollowingProse() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
            "version": 1,
            "styles": ["table": ["marginTop": 17, "marginBottom": 23, "marginLeft": 11, "marginRight": 7]]
        ])))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let tableBounds = try XCTUnwrap(drawing.layout?.blocks.first?.tableBounds)
        let marker = (view.textView.text as NSString).range(of: "\u{200B}")
        XCTAssertNotEqual(marker.location, NSNotFound)
        let markerGlyph = view.textView.layoutManager.glyphRange(forCharacterRange: marker, actualCharacterRange: nil)
        let markerLine = view.textView.layoutManager.lineFragmentRect(forGlyphAt: markerGlyph.location, effectiveRange: nil)
        let anchorX = view.textView.textContainerInset.left + markerLine.minX
        let anchorY = view.textView.textContainerInset.top + markerLine.minY
        XCTAssertEqual(tableBounds.minX, anchorX + 11, accuracy: 0.5)
        XCTAssertEqual(tableBounds.minY, anchorY + 17, accuracy: 0.5)
        let markerStyle = try XCTUnwrap(view.textView.textStorage.attribute(.paragraphStyle, at: marker.location, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertGreaterThanOrEqual(markerStyle.minimumLineHeight, tableBounds.maxY + 22 - anchorY)
        let after = (view.textView.text as NSString).range(of: "after")
        XCTAssertNotEqual(after.location, NSNotFound)
        view.textView.layoutManager.ensureLayout(forCharacterRange: after)
        let afterGlyph = view.textView.layoutManager.glyphRange(forCharacterRange: after, actualCharacterRange: nil)
        let afterLine = view.textView.layoutManager.lineFragmentRect(forGlyphAt: afterGlyph.location, effectiveRange: nil)
        let newlineStyle = try XCTUnwrap(view.textView.textStorage.attribute(.paragraphStyle, at: marker.location + 1, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(newlineStyle.minimumLineHeight, markerStyle.minimumLineHeight, accuracy: 0.5)
        XCTAssertGreaterThanOrEqual(
            view.textView.textContainerInset.top + afterLine.minY,
            tableBounds.maxY + 22
        )
    }

    func testRootTableDeepScrollKeepsDrawingViewportMounted() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let before: [[String: Any]] = (0..<20).map { index in
            ["type": "paragraph", "content": [["type": "text", "text": "before \(index)"]]]
        }
        let document = try XCTUnwrap(String(
            data: JSONSerialization.data(withJSONObject: [
                "type": "doc",
                "content": before + [[
                    "type": "table",
                    "content": [[
                        "type": "table_row",
                        "content": [[
                            "type": "table_cell",
                            "content": [[
                                "type": "paragraph",
                                "content": [["type": "text", "text": "cell"]]
                            ]]
                        ]]
                    ]]
                ]]
            ]),
            encoding: .utf8
        ))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 80))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let initialFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertGreaterThan(initialFrame.minY, view.bounds.height)
        let revision = adapter.baseDocumentRevision
        let history = adapter.cachedHistoryState
        let documentBeforeScroll = try XCTUnwrap(adapter.documentJson())

        let window = UIWindow(frame: view.bounds)
        window.addSubview(view)
        window.isHidden = false
        defer { window.isHidden = true }

        view.textView.contentOffset.y = initialFrame.minY - 10
        tableSurface.updateGeometry(from: view.textView)

        let visibleFrame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertEqual(visibleFrame.minY, 10, accuracy: 0.5)
        XCTAssertTrue(drawing.frame.intersects(tableSurface.bounds))
        XCTAssertLessThanOrEqual(drawing.bounds.height, view.bounds.height)
        var paintedCells = 0
        var paintedText = 0
        drawing.onTableChromeDrawnForTesting = { _ in paintedCells += 1 }
        drawing.onTableRichFragmentDrawnForTesting = { paintedText += 1 }
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        _ = UIGraphicsImageRenderer(size: view.bounds.size, format: format).image { context in
            context.cgContext.translateBy(x: -drawing.bounds.minX, y: -drawing.bounds.minY)
            drawing.draw(drawing.bounds)
        }
        XCTAssertGreaterThan(paintedCells, 0)
        XCTAssertGreaterThan(paintedText, 0)
        XCTAssertEqual(adapter.baseDocumentRevision, revision)
        XCTAssertEqual(adapter.cachedHistoryState?.canUndo, history?.canUndo)
        XCTAssertEqual(adapter.cachedHistoryState?.canRedo, history?.canRedo)
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), documentBeforeScroll)
    }

    func testInactiveCellTouchBelongsToRootScrollView() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let frame = try XCTUnwrap(tableSurface.cellFrame(tableID: tableID, cellIndex: 0))
        XCTAssertTrue(view.hitTest(CGPoint(x: frame.midX, y: frame.midY), with: nil) === view.textView)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.hitTest(CGPoint(x: frame.midX, y: frame.midY), with: nil) === view.activeTextInput)
    }

    func testActiveCellInputUsesPreparedContentInsets() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: ["contentInsets": ["top": 12, "left": 20]])))
        view.layoutIfNeeded()

        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let cell = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface?.cells.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        let input = view.activeTextInput
        let origin = try XCTUnwrap(drawing.layout?.blocks.first?.tableBounds?.origin)
        XCTAssertEqual(input.frame.minX, (origin.x + cell.frame.minX + cell.contentOrigin.x).rounded(), accuracy: 1)
        XCTAssertEqual(input.textContainerInset, .zero)
        XCTAssertEqual(input.textContainer.lineFragmentPadding, 0)
    }

    func testRootTableRebindWithSameRevisionReplacesPreparedContent() throws {
        let firstID = makeV2Editor(configJson: tableConfig)
        let secondID = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: firstID); destroyV2Editor(id: secondID) }
        let first = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: firstID))
        let second = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: secondID))
        let firstDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"first"}]}]}]}]}]}"#
        let secondDocument = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"second"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: firstID, initialUpdateJSON: try XCTUnwrap(first.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(first.setContentJson(firstDocument))))
        view.layoutIfNeeded()
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        let firstSurface = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)

        _ = try XCTUnwrap(second.initialUpdateJSON())
        _ = try XCTUnwrap(second.setContentJson(secondDocument))
        XCTAssertEqual(first.baseDocumentRevision, second.baseDocumentRevision)
        view.bindEditor(id: secondID, initialUpdateJSON: try XCTUnwrap(second.initialUpdateJSON()))
        view.layoutIfNeeded()

        XCTAssertEqual(first.baseDocumentRevision, second.baseDocumentRevision)
        let secondSurface = try XCTUnwrap(drawing.layout?.blocks.first?.tableSurface)
        XCTAssertFalse(firstSurface === secondSurface)
        XCTAssertTrue(String(describing: secondSurface.sourceTable).contains("second"))
        let secondTableID = try XCTUnwrap(second.cachedTableInputMappings?.tables.keys.first)
        XCTAssertNotNil(second.positionEpoch)
        XCTAssertTrue(view.bindTableCell(tableID: secondTableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.textView.ownsNativeBinding(second))
    }

    func testPrepopulatedInitialBindRetainsTablePresentationAndCellAuthority() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"preloaded"}]}]}]}]}]}"#
        _ = try XCTUnwrap(adapter.setContentJson(document))
        let revision = adapter.baseDocumentRevision
        let history = adapter.cachedHistoryState
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        view.layoutIfNeeded()

        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        XCTAssertTrue(String(describing: drawing.layout?.blocks.first?.tableSurface?.sourceTable).contains("preloaded"))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertNotNil(adapter.positionEpoch)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 0, contentRect: .zero))
        XCTAssertTrue(view.textView.ownsNativeBinding(adapter))
        XCTAssertEqual(adapter.baseDocumentRevision, revision)
        XCTAssertEqual(adapter.cachedHistoryState?.canUndo, history?.canUndo)
        XCTAssertEqual(adapter.cachedHistoryState?.canRedo, history?.canRedo)
    }

    func testRootTableRecoversAfterZeroWidthLayout() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        view.layoutIfNeeded()
        let tableSurface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
        let drawing = try XCTUnwrap(tableSurface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        XCTAssertNotNil(drawing.layout)

        view.frame.size.width = 0
        view.layoutIfNeeded()
        XCTAssertNil(drawing.layout)
        view.frame.size.width = 320
        view.layoutIfNeeded()
        XCTAssertNotNil(drawing.layout)
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

    func testProjectedUpdateCannotRetargetInputAfterCellSourceMoves() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let initial = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"a"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"b"}]}]}]}]}]}"#
        let replacement = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"longer"}]}]},{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"replacement"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(initial))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        XCTAssertTrue(view.bindTableCell(tableID: tableID, cellIndex: 1, contentRect: .zero))
        let oldInput = view.activeTextInput
        let oldSource = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.cells[1].sourcePos)

        _ = try XCTUnwrap(adapter.setContentJson(replacement))
        let movedCell = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID]?.cells[1])
        XCTAssertNotEqual(movedCell.sourcePos, oldSource)
        let newSelection = try XCTUnwrap(movedCell.blocks.first?.contentScalarStart)
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: newSelection, scalarHead: newSelection)
        XCTAssertTrue(oldInput.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))

        XCTAssertTrue(view.activeTextInput === view.textView)
        XCTAssertEqual(oldInput.editorId, 0)
        let settled = try XCTUnwrap(adapter.documentJson())
        oldInput.insertText("!")
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), settled)
    }

    func testRootInputBlocksStaleCaretWhenAuthoritativeSelectionMovesInsideTable() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let tableID = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.keys.first)
        let table = try XCTUnwrap(adapter.cachedTableInputMappings?.tables[tableID])
        let interior = try XCTUnwrap(table.cells.first?.blocks.first?.contentScalarStart) + 1
        let extent = try XCTUnwrap(table.extent)
        XCTAssertTrue(extent.scalarStart < interior && interior < extent.scalarEnd)

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: interior, scalarHead: interior)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        let before = try XCTUnwrap(adapter.documentJson())
        view.textView.insertText("!")
        view.textView.deleteBackward()
        XCTAssertFalse(view.textView.pasteHTML("<strong>unsafe</strong>", detectContentChange: true))
        XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)

        let afterStart = extent.scalarEnd + 1
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: afterStart, scalarHead: afterStart)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertFalse(view.textView.rootTableSelectionInputBlocked)
        view.textView.insertText("!")
        XCTAssertTrue(try XCTUnwrap(adapter.documentJson()).contains(#""text":"!after""#))

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: interior, scalarHead: interior)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        let afterOffset = (view.textView.text as NSString).range(of: "!after").location
        XCTAssertNotEqual(afterOffset, NSNotFound)
        view.textView.selectedRange = NSRange(location: afterOffset, length: 0)
        view.textView.textViewDidChangeSelection(view.textView)
        XCTAssertFalse(view.textView.rootTableSelectionInputBlocked)
        XCTAssertEqual(view.textView.currentLogicalScalarSelection()?.head, afterStart)

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: interior, scalarHead: interior)
        XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
        XCTAssertTrue(view.textView.rootTableSelectionInputBlocked)
        view.bindEditor(id: 0, initialUpdateJSON: nil)
        XCTAssertFalse(view.textView.rootTableSelectionInputBlocked)
    }

    func testRootTableAnchorEndpointsCannotEditACellWithoutCellBinding() throws {
        let editorId = makeV2Editor(configJson: tableConfig)
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let document = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]}]}"#
        let view = RichTextEditorView(frame: .zero)
        view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
        XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
        let extent = try XCTUnwrap(adapter.cachedTableInputMappings?.tables.values.first?.extent)
        let before = try XCTUnwrap(adapter.documentJson())

        for scalar in [extent.scalarStart, extent.scalarEnd] {
            EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: scalar, scalarHead: scalar)
            XCTAssertTrue(view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId)))
            view.textView.insertText("!")
            XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        }
    }

    func testRootTextDragRejectsTableSourceAndDestination() throws {
        try MainActor.assumeIsolated {
            let editorId = makeV2Editor(configJson: tableConfig)
            defer { destroyV2Editor(id: editorId) }
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
            let document = #"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"before"}]},{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","content":[{"type":"paragraph","content":[{"type":"text","text":"cell"}]}]}]}]},{"type":"paragraph","content":[{"type":"text","text":"after"}]}]}"#
            let view = RichTextEditorView(frame: .zero)
            view.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
            XCTAssertTrue(view.textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(document))))
            let textView = view.textView
            let before = try XCTUnwrap(adapter.documentJson())
            let tableOffset = (textView.text as NSString).range(of: "\u{200B}").location
            XCTAssertNotEqual(tableOffset, NSNotFound)
            let item = UIDragItem(itemProvider: NSItemProvider(object: "be" as NSString))

            @MainActor func position(_ offset: Int) throws -> UITextPosition {
                try XCTUnwrap(textView.position(from: textView.beginningOfDocument, offset: offset))
            }
            @MainActor func drag(_ start: Int, _ end: Int) throws -> TestTextDragSession {
                let session = TestTextDragSession(items: [item])
                let range = try XCTUnwrap(textView.textRange(from: position(start), to: position(end)))
                _ = textView.textDraggableView(textView, itemsForDrag: TestTextDragRequest(
                    dragRange: range,
                    suggestedItems: [item],
                    isSelected: true,
                    dragSession: session
                ))
                return session
            }
            @MainActor func proposal(_ destination: Int, session: TestTextDragSession) throws -> UITextDropProposal {
                let request = TestTextDropRequest(
                    dropPosition: try position(destination),
                    isSameView: true,
                    dropSession: TestTextDropSession(dragSession: session)
                )
                let result = textView.textDroppableView(textView, proposalForDrop: request)
                textView.textDroppableView(textView, willPerformDrop: request)
                return result
            }

            XCTAssertEqual(try proposal(tableOffset, session: drag(0, 2)).operation, .forbidden)
            XCTAssertEqual(try proposal(tableOffset + 1, session: drag(0, 2)).operation, .forbidden)
            XCTAssertEqual(try proposal(0, session: drag(tableOffset, tableOffset + 1)).operation, .forbidden)
            XCTAssertEqual(try XCTUnwrap(adapter.documentJson()), before)
        }
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
