import XCTest

final class TableCellPositionMapTests: XCTestCase {
    private func attachmentInput(editorId: UInt64) throws -> EditorTextView {
        let view = EditorTextView(frame: .zero, textContainer: nil)
        view.bindEditor(id: editorId)
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let text = NSMutableAttributedString(string: "\u{FFFC}\n\u{200B}")
        text.addAttributes([
            .attachment: NSTextAttachment(),
            RenderBridgeAttributes.voidNodeType: "horizontal_rule"
        ], range: NSRange(location: 0, length: 1))
        _ = view.applyAttributedRender(text, usedPatch: false, positionCacheUpdate: .invalidate)
        view.tableCellPositionMap = TableCellPositionMap(
            binding: .init(cellSourcePosition: 2, documentRevision: adapter.baseDocumentRevision, positionEpoch: try XCTUnwrap(adapter.positionEpoch)),
            segments: [.init(localScalarRange: 0..<4, globalScalarStart: 10)]
        )
        return view
    }

    func testTrailingAttachmentDeletionUsesCellGlobalCoordinates() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = try attachmentInput(editorId: editorId)
        let range = try XCTUnwrap(view.trailingVoidBlockDeleteRangeForBackwardDelete(cursorUtf16Offset: 3))
        XCTAssertEqual(range.from, 10)
        XCTAssertEqual(range.to, 11)
        let adjacent = try XCTUnwrap(view.adjacentVoidBlockDeleteRangeForBackwardDelete(cursorUtf16Offset: 0, cursorScalar: 10))
        XCTAssertEqual(adjacent.from, 10)
        XCTAssertEqual(adjacent.to, 11)
        XCTAssertNil(view.adjacentVoidBlockDeleteRangeForBackwardDelete(cursorUtf16Offset: 0, cursorScalar: 11))
    }

    func testAttachmentDeletionRejectsUnmappedEndAndStaleEpoch() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = try attachmentInput(editorId: editorId)
        let binding = try XCTUnwrap(view.tableCellPositionMap?.binding)
        view.tableCellPositionMap = TableCellPositionMap(
            binding: binding,
            segments: [.init(localScalarRange: 0..<1, globalScalarStart: 10)]
        )
        XCTAssertNil(view.adjacentVoidBlockDeleteRangeForBackwardDelete(cursorUtf16Offset: 0, cursorScalar: 10))
        XCTAssertNil(view.trailingVoidBlockDeleteRangeForBackwardDelete(cursorUtf16Offset: 3))
        view.tableCellPositionMap = TableCellPositionMap(
            binding: .init(cellSourcePosition: binding.cellSourcePosition, documentRevision: binding.documentRevision, positionEpoch: binding.positionEpoch + 1),
            segments: [.init(localScalarRange: 0..<4, globalScalarStart: 10)]
        )
        XCTAssertNil(view.adjacentVoidBlockDeleteRangeForBackwardDelete(cursorUtf16Offset: 0, cursorScalar: 10))
        XCTAssertNil(view.trailingVoidBlockDeleteRangeForBackwardDelete(cursorUtf16Offset: 3))
    }

    private func map(_ segments: [TableCellPositionMap.Segment]) -> TableCellPositionMap {
        TableCellPositionMap(
            binding: .init(cellSourcePosition: 2, documentRevision: 4, positionEpoch: 9),
            segments: segments
        )
    }

    func testConflictingLocalCaretMappingsAreRejected() {
        let mapping = map([
            .init(localScalarRange: 0..<4, globalScalarStart: 10),
            .init(localScalarRange: 3..<7, globalScalarStart: 20)
        ])
        XCTAssertNil(mapping.globalScalar(forLocalScalar: 3))
    }

    func testAmbiguousInverseCaretMappingsAreRejected() {
        let mapping = map([
            .init(localScalarRange: 0..<4, globalScalarStart: 10),
            .init(localScalarRange: 5..<9, globalScalarStart: 13)
        ])
        XCTAssertNil(mapping.localScalar(forGlobalScalar: 13))
    }

    func testMatchingSharedCaretEndpointsRemainMappable() {
        let mapping = map([
            .init(localScalarRange: 0..<4, globalScalarStart: 10),
            .init(localScalarRange: 3..<7, globalScalarStart: 13)
        ])
        XCTAssertEqual(mapping.globalScalar(forLocalScalar: 3), 13)
        XCTAssertEqual(mapping.localScalar(forGlobalScalar: 13), 3)
    }

    func testRangeAcrossUnsortedContiguousSegmentsIncludesEndpoints() throws {
        let mapping = map([
            .init(localScalarRange: 4..<8, globalScalarStart: 14),
            .init(localScalarRange: 0..<4, globalScalarStart: 10)
        ])
        let range = try XCTUnwrap(mapping.globalScalarRange(fromLocalScalar: 1, toLocalScalar: 7))
        XCTAssertEqual(range.from, 11)
        XCTAssertEqual(range.to, 17)
    }

    func testRangeRejectsLocalHolesGlobalJumpsAndInteriorAmbiguity() {
        for segments: [TableCellPositionMap.Segment] in [
            [.init(localScalarRange: 0..<3, globalScalarStart: 10), .init(localScalarRange: 4..<8, globalScalarStart: 14)],
            [.init(localScalarRange: 0..<4, globalScalarStart: 10), .init(localScalarRange: 4..<8, globalScalarStart: 20)],
            [.init(localScalarRange: 0..<8, globalScalarStart: 10), .init(localScalarRange: 3..<4, globalScalarStart: 30)]
        ] {
            XCTAssertNil(map(segments).globalScalarRange(fromLocalScalar: 0, toLocalScalar: 7))
        }
    }

    func testLargeRangeDoesNotNeedPerScalarTraversal() throws {
        let mapping = map([.init(localScalarRange: 0..<1_000_000_001, globalScalarStart: 10)])
        let range = try XCTUnwrap(mapping.globalScalarRange(fromLocalScalar: 0, toLocalScalar: 1_000_000_000))
        XCTAssertEqual(range.from, 10)
        XCTAssertEqual(range.to, 1_000_000_010)
    }

    func testInverseNearUInt32LimitDoesNotOverflowIntermediateAddition() {
        let mapping = map([.init(localScalarRange: 100..<115, globalScalarStart: UInt32.max - 20)])
        XCTAssertEqual(mapping.localScalar(forGlobalScalar: UInt32.max - 10), 110)
        XCTAssertEqual(mapping.globalScalar(forLocalScalar: 110), UInt32.max - 10)
    }

    func testCollapsedRangeAndStaleEpoch() throws {
        let mapping = map([.init(localScalarRange: 0..<5, globalScalarStart: 10)])
        let range = try XCTUnwrap(mapping.globalScalarRange(fromLocalScalar: 4, toLocalScalar: 4))
        XCTAssertEqual(range.from, 14)
        XCTAssertEqual(range.to, 14)
        XCTAssertNil(mapping.globalScalarRange(fromLocalScalar: 4, toLocalScalar: 3))
        XCTAssertNil(mapping.globalScalarRange(fromLocalScalar: 4, toLocalScalar: 5))
        XCTAssertNil(mapping.globalScalar(forLocalScalar: 0, currentRevision: 4, currentEpoch: 10))
        XCTAssertNil(mapping.localScalar(forGlobalScalar: 10, currentRevision: 5, currentEpoch: 9))
    }
}
