import XCTest

final class RootTablePositionMapTests: XCTestCase {
    private func rendered(_ text: String, markers: [(String, NSRange)]) -> NSAttributedString {
        let result = NSMutableAttributedString(string: text)
        for (key, range) in markers {
            result.addAttribute(RenderBridgeAttributes.rootTableMarker, value: key, range: range)
        }
        return result
    }

    func testFollowingUnicodeProseMapsAndTableInputIsRejected() throws {
        let text = rendered("\u{200B}\n😀z", markers: [("table", NSRange(location: 0, length: 1))])
        let map = try XCTUnwrap(RootTablePositionMap.fromRendered(
            text, extents: ["table": TableScalarExtent(scalarStart: 0, scalarEnd: 4)], scalarLength: 7
        ))
        XCTAssertEqual(map.globalScalar(local: 2), 5)
        XCTAssertEqual(map.globalScalar(local: 3), 6)
        XCTAssertEqual(map.globalRange(from: 2, to: 3), 5..<6)
        XCTAssertNil(map.globalScalar(local: 0))
        XCTAssertNil(map.globalScalar(local: 1))
        XCTAssertNil(map.globalRange(from: 0, to: 2))
        XCTAssertNil(map.localScalar(global: 0))
        XCTAssertNil(map.localScalar(global: 4))
        XCTAssertEqual(map.localScalar(global: 5), 2)
        XCTAssertTrue(map.isImmediatelyAfterTable(local: 2))
        XCTAssertFalse(map.isImmediatelyAfterTable(local: 3))
        XCTAssertNil(map.globalScalar(local: 5))
        XCTAssertNil(map.localScalar(global: 8))
        XCTAssertNil(map.globalRange(from: 3, to: 2))
    }

    func testMultipleTablesAccumulateTheirExtents() throws {
        let text = rendered("a\n\u{200B}\n\u{200B}\nz", markers: [
            ("first", NSRange(location: 2, length: 1)), ("second", NSRange(location: 4, length: 1))
        ])
        let map = try XCTUnwrap(RootTablePositionMap.fromRendered(text, extents: [
            "first": TableScalarExtent(scalarStart: 2, scalarEnd: 6),
            "second": TableScalarExtent(scalarStart: 7, scalarEnd: 10)
        ], scalarLength: 12))
        XCTAssertEqual(map.globalScalar(local: 0), 0)
        XCTAssertEqual(map.globalScalar(local: 6), 11)
        XCTAssertEqual(map.localScalar(global: 11), 6)
        XCTAssertNil(map.globalRange(from: 0, to: 6))
        XCTAssertNil(map.localScalar(global: 8))
    }

    func testTextReplacementProjectsTablesBeforeBetweenAndAfterTheEdit() throws {
        let text = rendered("a\n\u{200B}\nbc\n\u{200B}\nd", markers: [
            ("first", NSRange(location: 2, length: 1)), ("second", NSRange(location: 7, length: 1))
        ])
        let map = try XCTUnwrap(RootTablePositionMap.fromRendered(text, extents: [
            "first": TableScalarExtent(scalarStart: 2, scalarEnd: 6),
            "second": TableScalarExtent(scalarStart: 10, scalarEnd: 13)
        ], scalarLength: 15))
        func assertProjection(
            _ name: String, range: Range<UInt32>, length: UInt32,
            first: TableScalarExtent, second: TableScalarExtent, caret: (local: UInt32, global: UInt32)
        ) throws {
            let projected = try XCTUnwrap(map.applyingTextReplacement(
                fromLocal: range.lowerBound, toLocal: range.upperBound, newLocalLength: length
            ), name)
            XCTAssertEqual(projected.extents["first"], first, name)
            XCTAssertEqual(projected.extents["second"], second, name)
            XCTAssertEqual(projected.globalScalar(local: caret.local), caret.global, name)
            XCTAssertEqual(projected.localScalar(global: caret.global), caret.local, name)
            XCTAssertTrue(projected.isInputRangeSafe(from: caret.global, to: caret.global), name)
            XCTAssertFalse(projected.isInputRangeSafe(from: first.scalarStart, to: first.scalarEnd), name)
            let opaqueTableScalarDelta: UInt32 = 5
            XCTAssertEqual(projected.globalBoundary(local: length), length + opaqueTableScalarDelta, name)
        }
        try assertProjection("before", range: 0..<1, length: 13,
            first: TableScalarExtent(scalarStart: 5, scalarEnd: 9),
            second: TableScalarExtent(scalarStart: 13, scalarEnd: 16), caret: (4, 4))
        try assertProjection("between", range: 4..<6, length: 13,
            first: TableScalarExtent(scalarStart: 2, scalarEnd: 6),
            second: TableScalarExtent(scalarStart: 13, scalarEnd: 16), caret: (9, 12))
        try assertProjection("after", range: 9..<10, length: 15,
            first: TableScalarExtent(scalarStart: 2, scalarEnd: 6),
            second: TableScalarExtent(scalarStart: 10, scalarEnd: 13), caret: (15, 20))
        try assertProjection("shorter", range: 4..<6, length: 9,
            first: TableScalarExtent(scalarStart: 2, scalarEnd: 6),
            second: TableScalarExtent(scalarStart: 9, scalarEnd: 12), caret: (5, 8))
        XCTAssertEqual(map.globalBoundary(local: 10), 15, "projection must preserve the original ruler")
        XCTAssertNil(map.applyingTextReplacement(fromLocal: 2, toLocal: 3, newLocalLength: 10), "a native edit cannot replace a table marker")
        XCTAssertNil(map.applyingTextReplacement(fromLocal: 1, toLocal: 4, newLocalLength: 10), "a native edit cannot cross a table")
        XCTAssertNil(map.applyingTextReplacement(fromLocal: 6, toLocal: 4, newLocalLength: 10), "reversed ranges must be refused")
        XCTAssertNil(map.applyingTextReplacement(fromLocal: 9, toLocal: 11, newLocalLength: 10), "out-of-bounds ranges must be refused")
        XCTAssertNil(map.applyingTextReplacement(fromLocal: 4, toLocal: 6, newLocalLength: 7), "the new length cannot remove text outside the edit")
        XCTAssertNil(map.applyingTextReplacement(fromLocal: 4, toLocal: 6, newLocalLength: .max), "global positions must not overflow")
    }

    func testMalformedMarkersAndExtentsAreRejected() {
        let valid = rendered("\u{200B}\nz", markers: [("table", NSRange(location: 0, length: 1))])
        let extent = TableScalarExtent(scalarStart: 0, scalarEnd: 4)
        XCTAssertNil(RootTablePositionMap.fromRendered(valid, extents: [:], scalarLength: 3))
        XCTAssertNil(RootTablePositionMap.fromRendered(valid, extents: ["other": extent], scalarLength: 6))
        XCTAssertNil(RootTablePositionMap.fromRendered(valid, extents: ["table": extent], scalarLength: 5))
        XCTAssertNil(RootTablePositionMap.fromRendered(valid, extents: [
            "table": TableScalarExtent(scalarStart: 1, scalarEnd: 5)
        ], scalarLength: 6))
        XCTAssertNil(RootTablePositionMap.fromRendered(valid, extents: [
            "table": TableScalarExtent(scalarStart: 0, scalarEnd: 0)
        ], scalarLength: 2))
        for text in [
            rendered("x\nz", markers: [("table", NSRange(location: 0, length: 1))]),
            rendered("\u{200B}\nz", markers: [("table", NSRange(location: 0, length: 2))]),
            rendered("\u{200B}\n\u{200B}", markers: [
                ("table", NSRange(location: 0, length: 1)), ("table", NSRange(location: 2, length: 1))
            ])
        ] {
            XCTAssertNil(
                RootTablePositionMap.fromRendered(text, extents: ["table": extent], scalarLength: 6),
                "malformed markers must fail before installation: \(text)"
            )
        }
    }
}
