import UIKit
import XCTest

extension PositionBridgeTests {
    func testVirtualListMarkerPreservesExplicitContextsAtEveryUtf16Offset() {
        let bullet: [String: Any] = ["ordered": false]
        let ordered: [String: Any] = ["ordered": true, "index": NSNumber(value: 12)]
        var fixtures = [
            VirtualListMarkerFixture(name: "empty", text: NSAttributedString(string: ""), markers: [:]),
            VirtualListMarkerFixture(
                name: "ordinary Unicode prose",
                text: NSAttributedString(string: "A😀e\u{301}\nB👨‍👩‍👧‍👦\r\nC\u{2028}D\u{2029}"),
                markers: [:]
            )
        ]

        for contextCase in [
            ListMarkerContextCase(name: "bullet", context: bullet, expectedLength: 2),
            ListMarkerContextCase(name: "ordered", context: ordered, expectedLength: 4),
            ListMarkerContextCase(name: "empty context", context: [:], expectedLength: 2),
            ListMarkerContextCase(name: "missing ordered index", context: ["ordered": true], expectedLength: 3),
            ListMarkerContextCase(name: "negative ordered index",
                context: ["ordered": true, "index": NSNumber(value: -1)], expectedLength: 0),
            ListMarkerContextCase(name: "fractional ordered index",
                context: ["ordered": true, "index": NSNumber(value: 1.5)], expectedLength: 0),
            ListMarkerContextCase(name: "string ordered index", context: ["ordered": true, "index": "12"], expectedLength: 0),
            ListMarkerContextCase(name: "boolean ordered index", context: ["ordered": true, "index": true], expectedLength: 0),
            ListMarkerContextCase(name: "overflow ordered index",
                context: ["ordered": true, "index": NSNumber(value: UInt64(UInt32.max) + 1)], expectedLength: 0),
            ListMarkerContextCase(name: "checked task", context: ["kind": "task", "checked": true], expectedLength: 2),
            ListMarkerContextCase(name: "malformed task checked", context: ["kind": "task", "checked": "true"], expectedLength: 2)
        ] {
            fixtures.append(VirtualListMarkerFixture(name: contextCase.name, text: NSAttributedString(
                string: "😀Item\n",
                attributes: [RenderBridgeAttributes.listMarkerContext: contextCase.context]
            ), markers: [0: contextCase.expectedLength]))
        }

        for value in [NSNull(), "list", ["list"], NSNumber(value: 12)] as [Any] {
            fixtures.append(VirtualListMarkerFixture(name: "non-dictionary context \(value)", text: NSAttributedString(
                string: "A😀B",
                attributes: [RenderBridgeAttributes.listMarkerContext: value]
            ), markers: [:]))
        }

        let inherited = NSMutableAttributedString(
            string: "a\nbc",
            attributes: [RenderBridgeAttributes.listMarkerContext: bullet]
        )
        inherited.addAttribute(RenderBridgeAttributes.blockBoundary, value: true,
            range: NSRange(location: 1, length: inherited.length - 1))
        fixtures.append(VirtualListMarkerFixture(name: "context run starts before target paragraph",
            text: NSAttributedString(attributedString: inherited), markers: [0: 2]))
        inherited.addAttribute(.underlineStyle, value: NSUnderlineStyle.single.rawValue,
            range: NSRange(location: inherited.length - 1, length: 1))
        fixtures.append(VirtualListMarkerFixture(name: "later run starts inside target paragraph",
            text: NSAttributedString(attributedString: inherited), markers: [0: 2, 2: 2]))

        let malformedFirst = NSMutableAttributedString(
            string: "AB",
            attributes: [RenderBridgeAttributes.listMarkerContext: ["ordered": true, "index": "bad"]]
        )
        malformedFirst.addAttribute(RenderBridgeAttributes.listMarkerContext, value: ordered,
            range: NSRange(location: 1, length: 1))
        fixtures.append(VirtualListMarkerFixture(name: "first zero-length marker wins", text: malformedFirst, markers: [0: 0]))

        let missingFirst = NSMutableAttributedString(string: "AB")
        missingFirst.addAttribute(RenderBridgeAttributes.listMarkerContext, value: ordered,
            range: NSRange(location: 1, length: 1))
        fixtures.append(VirtualListMarkerFixture(name: "context begins inside paragraph", text: missingFirst, markers: [0: 4]))

        for fixture in fixtures {
            let view = UITextView(frame: .zero)
            view.textStorage.setAttributedString(fixture.text)
            for offset in [-1] + Array(0...view.textStorage.length) + [view.textStorage.length + 1, NSNotFound] {
                let actual = PositionBridge.virtualListMarker(atUtf16Offset: offset, in: view)
                let expectedLength = fixture.markers[offset]
                XCTAssertEqual(actual?.paragraphStartUtf16, expectedLength == nil ? nil : offset,
                    "\(fixture.name), UTF-16 offset \(offset): paragraph start changed")
                XCTAssertEqual(actual?.scalarLength, expectedLength,
                    "\(fixture.name), UTF-16 offset \(offset): marker scalar length changed")
            }
        }
    }

    func testVirtualListMarkerPreservesRenderedListContinuationAndHardBreaks() {
        for (context, expectedLength) in [
            (["ordered": false], 2),
            (["ordered": true, "index": NSNumber(value: 12)], 4),
            (["kind": "task", "checked": true], 2)
        ] as [([String: Any], UInt32)] {
            let rendered = RenderBridge.renderElements(fromArray: [
                ["type": "blockStart", "nodeType": "listItem", "depth": 1, "listContext": context],
                ["type": "blockStart", "nodeType": "paragraph", "depth": 2],
                ["type": "textRun", "text": "A😀", "marks": []],
                ["type": "voidInline", "nodeType": "hardBreak", "docPos": 5],
                ["type": "textRun", "text": "B", "marks": [["type": "bold"]]],
                ["type": "blockEnd"],
                ["type": "blockStart", "nodeType": "paragraph", "depth": 2],
                ["type": "textRun", "text": "\u{200B}", "marks": []],
                ["type": "blockEnd"], ["type": "blockEnd"],
                ["type": "blockStart", "nodeType": "listItem", "depth": 1, "listContext": context],
                ["type": "blockStart", "nodeType": "paragraph", "depth": 2],
                ["type": "textRun", "text": "Last", "marks": []],
                ["type": "blockEnd"], ["type": "blockEnd"]
            ], baseFont: .systemFont(ofSize: 16), textColor: .label)
            let view = UITextView(frame: .zero)
            view.textStorage.setAttributedString(rendered)
            XCTAssertEqual(view.textStorage.string, "A😀\nB\n\u{200B}\nLast")
            let expectedMarkers: [Int: UInt32] = [0: expectedLength, 8: expectedLength]
            for offset in 0...view.textStorage.length {
                let actual = PositionBridge.virtualListMarker(atUtf16Offset: offset, in: view)
                let length = expectedMarkers[offset]
                XCTAssertEqual(actual?.paragraphStartUtf16, length == nil ? nil : offset,
                    "Rendered context \(context), UTF-16 offset \(offset): paragraph start changed")
                XCTAssertEqual(actual?.scalarLength, length,
                    "Rendered context \(context), UTF-16 offset \(offset): marker length changed")
            }
            let continuationOffset = (view.textStorage.string as NSString).range(of: "B").location
            XCTAssertTrue(RenderBridge.isListContinuationParagraph(continuationOffset, in: view.textStorage))
            XCTAssertNil(PositionBridge.virtualListMarker(atUtf16Offset: continuationOffset, in: view))
            let lastOffset = (view.textStorage.string as NSString).range(of: "Last").location
            XCTAssertEqual(PositionBridge.virtualListMarker(atUtf16Offset: lastOffset, in: view)?.paragraphStartUtf16,
                lastOffset, "The following real list item must retain its structural marker")
        }
    }

    func testOrdinaryMidParagraphMarkerLookupDoesNotVisitAttributeRuns() {
        let paragraphCount = 512
        let paragraphs = (0..<paragraphCount).map { "paragraph \($0): A😀B" }
        let storage = AttributeLookupTextStorage()
        storage.setAttributedString(NSAttributedString(string: paragraphs.joined(separator: "\n")))
        var paragraphStart = 0
        for (index, paragraph) in paragraphs.enumerated() {
            let length = paragraph.utf16.count
            storage.addAttribute(.foregroundColor, value: index.isMultiple(of: 2) ? UIColor.red : UIColor.blue,
                range: NSRange(location: paragraphStart, length: length))
            paragraphStart += length + 1
        }
        let manager = NSLayoutManager()
        let container = NSTextContainer(size: .zero)
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        let view = UITextView(frame: .zero, textContainer: container)
        let targetText = paragraphs[paragraphCount / 2]
        let targetParagraphStart = (storage.string as NSString).range(of: targetText).location
        let interiorOffset = targetParagraphStart + (targetText as NSString).range(of: "B").location
        storage.resetAttributeLookups()

        XCTAssertNil(PositionBridge.virtualListMarker(atUtf16Offset: interiorOffset, in: view))
        XCTAssertTrue(storage.attributeLookupLocations.isEmpty,
            "An ordinary interior backspace must reject before traversing storage attribute runs; visited \(storage.attributeLookupLocations)")
        let targetParagraph = (storage.string as NSString).paragraphRange(
            for: NSRange(location: targetParagraphStart, length: 0))
        storage.addAttribute(RenderBridgeAttributes.listMarkerContext, value: ["ordered": false], range: targetParagraph)
        storage.addAttribute(RenderBridgeAttributes.blockBoundary, value: true,
            range: NSRange(location: targetParagraphStart - 1, length: 1))
        storage.resetAttributeLookups()
        XCTAssertEqual(PositionBridge.virtualListMarker(atUtf16Offset: targetParagraphStart, in: view)?.scalarLength, 2)
        XCTAssertTrue(storage.attributeLookupLocations.allSatisfy {
            $0 >= targetParagraphStart - 1 && $0 < NSMaxRange(targetParagraph)
        }, "A structural marker query may read its preceding boundary and target paragraph, but visited \(storage.attributeLookupLocations)")
    }
}

private struct VirtualListMarkerFixture {
    let name: String
    let text: NSAttributedString
    let markers: [Int: UInt32]
}

private struct ListMarkerContextCase {
    let name: String
    let context: [String: Any]
    let expectedLength: UInt32
}
