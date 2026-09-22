import XCTest

final class EditorTableInputTests: XCTestCase {
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

        XCTAssertTrue(coordinator.bind(target(10), text: NSAttributedString(string: "one")))
        XCTAssertTrue(coordinator.bind(target(20), text: NSAttributedString(string: "two")))
        XCTAssertTrue(coordinator.bind(target(30), text: NSAttributedString(string: "three")))

        XCTAssertTrue(coordinator.cellInput === input)
        XCTAssertEqual(coordinator.inputInstanceCountForTesting, 1)
        XCTAssertEqual(coordinator.phase, .bound(cellSourcePos: 30, documentRevision: "4", positionEpoch: "9"))
    }
}
