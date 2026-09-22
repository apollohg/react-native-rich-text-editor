import Foundation
import XCTest

final class PreparedProseViewerFabricEventTests: XCTestCase {
    private let config = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"text","content":"","group":"inline","role":"text"},{"name":"card","content":"","group":"block","role":"block","isVoid":true},{"name":"table","content":"table_row+","group":"block","role":"block","tableRole":"table","attrs":{"class":{"default":null}}},{"name":"table_row","content":"(table_cell | table_header)*","role":"block","tableRole":"row"},{"name":"table_cell","content":"block+","role":"block","tableRole":"cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}},{"name":"table_header","content":"block+","role":"block","tableRole":"header_cell","attrs":{"class":{"default":null},"colspan":{"type":"number","default":1,"min":1},"rowspan":{"type":"number","default":1,"min":1},"colwidth":{"default":null}}}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
    private let source = #"{"type":"doc","content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"table","content":[{"type":"table_row","content":[{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"card"}]}]}]}]},{"type":"table_cell","attrs":{"colwidth":[600]},"content":[{"type":"paragraph","content":[{"type":"text","text":"right"}]}]}]}]}]}"#
    private let theme = #"{"viewerAtoms":{"generation":"fixture","revision":"r1","nodeTypes":["card"],"estimatedHeights":{"card":40}}}"#

    func testFabricAtomEventsUseMountedOwnerForOffsetViewportReplacementAndRecycle() throws {
        try requireCompiledFixture()
        let harness = PREPPreparedProseViewerFabricEventHarness(
            source: source, configJSON: config, themeJSON: theme,
            surfaceID: 73, componentTag: 701, leaseHandle: 97, width: 300, scale: 2
        )
        guard let initialValue = harness.events.first else {
            return XCTFail("Mounted exact owner must dispatch an atom event")
        }
        let initial = try event(initialValue)
        XCTAssertEqual(initial.generation, "fixture")
        XCTAssertEqual(initial.revision, "r1")
        XCTAssertEqual(initial.atoms.count, 1)
        let initialAtom = try XCTUnwrap(initial.atoms.first)
        XCTAssertEqual(initialAtom["docPos"] as? Int, 6)
        XCTAssertEqual(initialAtom["attrsJson"] as? String, "{}")
        XCTAssertEqual(initialAtom["width"] as? Double, 582)
        XCTAssertEqual(initialAtom["height"] as? Double, 40)
        let drawing = try XCTUnwrap(harness.drawingView as? PreparedProseDrawingView)
        let originalLayout = try XCTUnwrap(drawing.layout)
        XCTAssertNil(originalLayout.error)
        let originalTable = try XCTUnwrap(originalLayout.blocks.first?.tableSurface)
        let preparations = PreparedProseLayoutRegistry.shared.layoutPreparationCount

        harness.setTableLogicalOffset(10_000, sourceIdentity: "t0")
        guard harness.events.count == 2, let shiftedValue = harness.events.last else {
            return XCTFail("Offset geometry must dispatch exactly one changed projection")
        }
        let shifted = try event(shiftedValue)
        let shiftedAtom = try XCTUnwrap(shifted.atoms.first)
        XCTAssertGreaterThan(shifted.sequence, initial.sequence)
        XCTAssertNotEqual(shiftedAtom["x"] as? NSNumber, initialAtom["x"] as? NSNumber)
        XCTAssertEqual(shiftedAtom["width"] as? Double, 582)
        XCTAssertEqual(shiftedAtom["height"] as? Double, 40)
        XCTAssertEqual(shiftedAtom["docPos"] as? Int, 6)
        XCTAssertEqual(shiftedAtom["attrsJson"] as? String, "{}")
        let shiftedClip = try clip(shiftedAtom)
        XCTAssertTrue(shiftedClip.values.allSatisfy { $0.isFinite })
        XCTAssertEqual(shiftedClip["width"], 0)
        XCTAssertEqual(shiftedClip["height"], 0)
        XCTAssertTrue(drawing.layout === originalLayout)
        XCTAssertTrue(drawing.layout?.blocks.first?.tableSurface === originalTable)
        XCTAssertEqual(PreparedProseLayoutRegistry.shared.layoutPreparationCount, preparations)

        harness.setHostHidden(true)
        harness.setTableLogicalOffset(0, sourceIdentity: "t0")
        guard harness.events.count == 3, let hiddenValue = harness.events.last else {
            return XCTFail("Known-empty viewport geometry must dispatch exactly one changed projection")
        }
        let hidden = try event(hiddenValue)
        let hiddenAtom = try XCTUnwrap(hidden.atoms.first)
        XCTAssertEqual((hiddenAtom["presentation"] as? [String: Any])?["candidate"] as? Bool, false)
        XCTAssertEqual(hiddenAtom["docPos"] as? Int, 6)
        XCTAssertEqual(hiddenAtom["width"] as? Double, 582)
        XCTAssertTrue(drawing.layout === originalLayout)
        XCTAssertEqual(PreparedProseLayoutRegistry.shared.layoutPreparationCount, preparations)

        harness.setHostHidden(false)
        harness.beginPendingReplacement(withWidth: 280)
        let pendingCount = harness.events.count
        harness.setTableLogicalOffset(10, sourceIdentity: "t0")
        XCTAssertEqual(harness.events.count, pendingCount)
        harness.prepareReplacementAndInstall()
        XCTAssertEqual(harness.events.count, pendingCount + 1)
        XCTAssertFalse(drawing.layout === originalLayout)

        harness.expireLease()
        let expiredCount = harness.events.count
        harness.setTableLogicalOffset(40, sourceIdentity: "t0")
        XCTAssertEqual(harness.events.count, expiredCount)
        harness.replaceLease(99)
        harness.setTableLogicalOffset(60, sourceIdentity: "t0")
        XCTAssertEqual(harness.events.count, expiredCount + 1)
        let renewed = try event(XCTUnwrap(harness.events.last))
        XCTAssertGreaterThan(renewed.sequence, hidden.sequence)
        XCTAssertEqual(try XCTUnwrap(renewed.atoms.first)["docPos"] as? Int, 6)

        harness.recycle()
        let recycledCount = harness.events.count
        harness.setTableLogicalOffset(0, sourceIdentity: "t0")
        XCTAssertEqual(harness.events.count, recycledCount)
    }

    func testFabricAtomEventRejectsGeometryFromWidthOnlyPendingArtifact() throws {
        try requireCompiledFixture()
        let harness = PREPPreparedProseViewerFabricEventHarness(
            source: source, configJSON: config, themeJSON: theme,
            surfaceID: 74, componentTag: 702, leaseHandle: 98, width: 300, scale: 2
        )
        guard !harness.events.isEmpty else {
            return XCTFail("Mounted exact owner must dispatch an atom event")
        }
        let initial = try event(XCTUnwrap(harness.events.first))
        _ = try XCTUnwrap(initial.atoms.first)
        harness.setLayoutWidth(300)
        let settledCount = harness.events.count
        harness.setLayoutWidth(300)
        XCTAssertEqual(harness.events.count, settledCount)
        harness.setTableLogicalOffset(10, sourceIdentity: "t0")
        XCTAssertEqual(harness.events.count, settledCount + 1)
        let eventCount = harness.events.count
        harness.setLayoutWidth(280)
        XCTAssertEqual(harness.events.count, eventCount)
        harness.setTableLogicalOffset(20, sourceIdentity: "t0")
        XCTAssertEqual(harness.events.count, eventCount)
        harness.prepareReplacementAndInstall()
        XCTAssertEqual(harness.events.count, eventCount + 1)
        let replacementValue = try XCTUnwrap(harness.events.last)
        let replacement = try event(replacementValue)
        XCTAssertEqual(replacementValue["layoutWidth"] as? Double, 280)
        XCTAssertEqual(replacement.generation, initial.generation)
        XCTAssertEqual(replacement.revision, initial.revision)
        XCTAssertGreaterThan(replacement.sequence, initial.sequence)
        XCTAssertEqual(try XCTUnwrap(replacement.atoms.first)["docPos"] as? Int, 6)
        harness.setTableLogicalOffset(30, sourceIdentity: "t0")
        XCTAssertEqual(harness.events.count, eventCount + 2)
    }

    func testUnchangedMountedAtomProjectionHasStableSerializedBytes() throws {
        try requireCompiledFixture()
        let harness = PREPPreparedProseViewerFabricEventHarness(
            source: source, configJSON: config, themeJSON: theme,
            surfaceID: 75, componentTag: 703, leaseHandle: 100, width: 300, scale: 2
        )
        let drawing = try XCTUnwrap(harness.drawingView as? PreparedProseDrawingView)
        let projections = (0..<8).map { _ in drawing.atomLayoutsJSON(origin: .zero) }
        let first = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(projections[0].utf8)) as? NSArray)
        XCTAssertEqual(first.count, 1)
        for projection in projections {
            let parsed = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(projection.utf8)) as? NSArray)
            XCTAssertEqual(parsed, first)
        }
        XCTAssertEqual(Set(projections).count, 1)
    }

    private func requireCompiledFixture() throws {
        let compiled = viewerCompile(request: FfiViewerCompileRequest(
            sourceKind: .json, source: source, configJson: config,
            imagesEnabled: true, mentionPrefix: nil
        ))
        _ = try XCTUnwrap(compiled.value, String(describing: compiled.error))
    }

    private func event(_ value: [String: Any]) throws -> (generation: String, revision: String, sequence: UInt64, atoms: [[String: Any]]) {
        let generation = try XCTUnwrap(value["generation"] as? String)
        let revision = try XCTUnwrap(value["revision"] as? String)
        let atomsJSON = try XCTUnwrap(value["atomsJson"] as? String)
        let envelope = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(atomsJSON.utf8)) as? [String: Any])
        let sequenceText = try XCTUnwrap(envelope["presentationSequence"] as? String)
        let sequence = try XCTUnwrap(UInt64(sequenceText))
        let atoms = try XCTUnwrap(envelope["atoms"] as? [[String: Any]])
        return (generation, revision, sequence, atoms)
    }

    private func clip(_ atom: [String: Any]) throws -> [String: Double] {
        let presentation = try XCTUnwrap(atom["presentation"] as? [String: Any])
        let clip = try XCTUnwrap(presentation["clip"] as? [String: Any])
        return try ["x", "y", "width", "height"].reduce(into: [:]) { values, key in
            values[key] = try XCTUnwrap((clip[key] as? NSNumber)?.doubleValue)
        }
    }
}
