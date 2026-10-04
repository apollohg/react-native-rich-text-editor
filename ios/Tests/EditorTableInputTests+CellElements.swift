import XCTest

extension EditorTableInputTests {
    func testHorizontalRuleInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("horizontalRule")
    }

    func testBulletListInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("bulletList")
    }

    func testOrderedListInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("orderedList")
    }

    func testTaskListInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("taskList")
    }

    func testHeadingInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("h2")
    }

    func testBlockquoteInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("blockquote")
    }

    func testCodeBlockInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("codeBlock")
    }

    func testHardBreakInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("hardBreak")
    }

    func testMentionInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("mention")
    }

    func testImageInCellSupportsSurroundingEditsAndRemoval() throws {
        try exerciseCellElement("image")
    }

    private func exerciseCellElement(_ nodeType: String) throws {
        for header in [false, true] {
            for backspace in [false, true] {
                try exerciseCellElement(nodeType, header: header, backspace: backspace)
            }
        }
    }

    private func exerciseCellElement(_ nodeType: String, header: Bool, backspace: Bool) throws {
        let left = "left"
        let right = "right"
        let inside = "middle"
        let before = "before"
        let after = "after😀"
        let listTypes: Set<String> = ["bulletList", "orderedList", "taskList"]
        let textElements = listTypes.union(["h2", "blockquote", "codeBlock"])
        func node(_ type: String, _ children: [[String: Any]] = []) -> [String: Any] {
            var value: [String: Any] = ["type": type]
            if !children.isEmpty { value["content"] = children }
            return value
        }
        func text(_ value: String) -> [String: Any] { ["type": "text", "text": value] }
        func json(_ value: Any) throws -> String {
            try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: value, options: .sortedKeys), encoding: .utf8))
        }
        let paragraph = node("paragraph", [text(inside)])
        let element: [String: Any]
        switch nodeType {
        case let type where listTypes.contains(type): element = node(type, [node("listItem", [paragraph])])
        case "blockquote": element = node(nodeType, [paragraph])
        case "h2": element = ["type": nodeType, "attrs": ["level": 2], "content": [text(inside)]]
        case "codeBlock": element = node(nodeType, [text(inside)])
        case "mention": element = ["type": nodeType, "attrs": ["label": "Ada"]]
        case "image": element = ["type": nodeType, "attrs": ["src": "https://example.test/image.png"]]
        default: element = node(nodeType)
        }
        var config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(TableInputTestSchema.tableConfig.utf8)) as? [String: Any])
        var schema = try XCTUnwrap(config["schema"] as? [String: Any])
        let baseNodes = try XCTUnwrap(schema["nodes"] as? [[String: Any]])
        let additions = #"""
        [
        {"name":"horizontalRule","group":"block","role":"block","isVoid":true,"htmlTag":"hr"},
        {"name":"hardBreak","group":"inline","role":"hardBreak","isVoid":true,"htmlTag":"br"},
        {"name":"bulletList","content":"listItem+","group":"block","role":"list","htmlTag":"ul"},
        {"name":"orderedList","content":"listItem+","group":"block","role":"list","htmlTag":"ol","attrs":{"start":{"default":1}}},
        {"name":"taskList","content":"listItem+","group":"block","role":"list"},
        {"name":"listItem","content":"paragraph block*","role":"listItem","attrs":{"checked":{"default":false}}},
        {"name":"h2","content":"inline*","group":"block","role":"textBlock","htmlTag":"h2","attrs":{"level":{"default":2}}},
        {"name":"blockquote","content":"block+","group":"block","role":"block","htmlTag":"blockquote"},
        {"name":"codeBlock","content":"text*","group":"block","role":"textBlock","htmlTag":"pre"},
        {"name":"mention","group":"inline","role":"inline","isVoid":true,"attrs":{"label":{"default":""}}},
        {"name":"image","group":"block","role":"block","isVoid":true,"deletableOnBackspace":false,"htmlTag":"img","attrs":{"src":{}}}
        ]
        """#
        schema["nodes"] = baseNodes + (try XCTUnwrap(JSONSerialization.jsonObject(with: Data(additions.utf8)) as? [[String: Any]]))
        config["schema"] = schema
        let initialParagraphs = ["hardBreak", "mention"].contains(nodeType)
            ? [node("paragraph", [text(left + right)])]
            : [node("paragraph", [text(left)]), node("paragraph", [text(right)])]
        let document = node("doc", [node("table", [node("table_row", [
            node(header ? "table_header" : "table_cell", initialParagraphs),
            node("table_cell", [node("paragraph", [text("untouched")])])
        ])])])
        try withMountedTable(document: json(document), configJSON: json(config), cellSelection: nil) { fixture in
            XCTAssertTrue(fixture.view.bindTableCell(tableID: fixture.tableID, cellIndex: 0, contentRect: .zero))
            func input() -> EditorTextView {
                XCTAssertFalse(fixture.view.activeTextInput === fixture.view.textView, "\(nodeType) lost the cell input")
                return fixture.view.activeTextInput
            }
            func cells() throws -> [[String: Any]] {
                let document = try fixture.documentObject()
                let content = try XCTUnwrap(document["content"] as? [[String: Any]])
                let table = try XCTUnwrap(content.first { $0["type"] as? String == "table" })
                let rows = try XCTUnwrap(table["content"] as? [[String: Any]])
                XCTAssertEqual(rows.count, 1, "\(nodeType) changed table rows")
                let cells = try XCTUnwrap(rows.first?["content"] as? [[String: Any]])
                XCTAssertEqual(cells.count, 2, "\(nodeType) changed table columns")
                return cells
            }
            func descendants(_ node: [String: Any]) -> [[String: Any]] {
                [node] + (node["content"] as? [[String: Any]] ?? []).flatMap(descendants)
            }
            let originalNeighbour = try json(cells()[1])
            func check(_ expected: String, _ stage: String) throws {
                fixture.view.layoutIfNeeded()
                let cell = try cells()[0]
                let actual = descendants(cell).compactMap { $0["text"] as? String }.joined()
                XCTAssertEqual(actual, expected, "\(nodeType) header=\(header) backspace=\(backspace) \(stage) document=\(cell) input=\(input().textStorage.string)")
                XCTAssertEqual(try json(cells()[1]), originalNeighbour, "\(nodeType) \(stage) touched neighbour")
                let rendered = input().textStorage.string
                if expected.isEmpty || expected == "again" {
                    XCTAssertEqual(rendered.replacingOccurrences(of: "\u{200B}", with: ""), expected, "\(nodeType) \(stage) stale native text")
                    for key in [RenderBridgeAttributes.voidNodeType, RenderBridgeAttributes.listMarkerContext, RenderBridgeAttributes.blockquoteBorderWidth, RenderBridgeAttributes.codeBlockBackgroundColor] {
                        input().textStorage.enumerateAttribute(key, in: NSRange(location: 0, length: input().textStorage.length)) { value, _, _ in
                            XCTAssertNil(value, "\(nodeType) \(stage) stale native decoration")
                        }
                    }
                }
                for token in [before, left, inside, after, right, "typed", "again"] where expected.contains(token) {
                    let range = (rendered as NSString).range(of: token)
                    XCTAssertNotEqual(range.location, NSNotFound, "\(nodeType) \(stage) missing visible \(token) in \(rendered)")
                    if range.location != NSNotFound {
                        input().layoutManager.ensureLayout(for: input().textContainer)
                        let glyphs = input().layoutManager.glyphRange(forCharacterRange: range, actualCharacterRange: nil)
                        let bounds = input().layoutManager.boundingRect(forGlyphRange: glyphs, in: input().textContainer)
                        XCTAssertGreaterThan(bounds.width, 0, "\(nodeType) \(stage) invisible \(token)")
                        XCTAssertGreaterThan(bounds.height, 0, "\(nodeType) \(stage) collapsed \(token)")
                    }
                }
            }
            func select(_ token: String, end: Bool = false) throws {
                let range = (input().textStorage.string as NSString).range(of: token)
                XCTAssertNotEqual(range.location, NSNotFound, "missing selection target \(token)")
                guard range.location != NSNotFound else { return }
                input().selectedRange = NSRange(location: end ? NSMaxRange(range) : range.location, length: 0)
            }
            input().selectedRange = NSRange(location: left.utf16.count, length: 0)
            let local = try XCTUnwrap(input().currentScalarSelection())
            let selection = try XCTUnwrap(input().inputScalarRange(fromLocal: local.anchor, toLocal: local.head))
            let update = try XCTUnwrap(fixture.adapter.insertContentJson(json(node("doc", [element])), anchor: selection.from, head: selection.to))
            XCTAssertTrue(input().applyUpdateJSON(update))
            _ = try XCTUnwrap(input().tableCellPositionMap, "\(nodeType) lost projection after insertion: \(update)")
            let middle = textElements.contains(nodeType) ? inside : ""
            try check(left + middle + right, "insert")
            XCTAssertTrue(try descendants(cells()[0]).contains { $0["type"] as? String == nodeType }, "\(nodeType) was not inserted")
            let insertedText = "typed"
            input().insertText(insertedText)
            try check(left + middle + insertedText + right, "type at insertion caret")
            for _ in insertedText { input().deleteBackward() }
            try check(left + middle + right, "delete at insertion caret")
            let decoration: NSAttributedString.Key?
            switch nodeType {
            case "horizontalRule", "hardBreak", "mention", "image": decoration = RenderBridgeAttributes.voidNodeType
            case "bulletList", "orderedList", "taskList": decoration = RenderBridgeAttributes.listMarkerContext
            case "blockquote": decoration = RenderBridgeAttributes.blockquoteBorderWidth
            case "codeBlock": decoration = RenderBridgeAttributes.codeBlockBackgroundColor
            default: decoration = nil
            }
            if let decoration {
                var found = false
                input().textStorage.enumerateAttribute(decoration, in: NSRange(location: 0, length: input().textStorage.length)) { value, _, _ in
                    if value != nil { found = true }
                }
                XCTAssertTrue(found, "\(nodeType) decoration missing from native text")
            }
            try select(right)
            input().insertText(after)
            try check(left + middle + after + right, "type after")
            try select(left)
            input().insertText(before)
            try check(before + left + middle + after + right, "type before")
            try select(before, end: true)
            for _ in before { input().deleteBackward() }
            try check(left + middle + after + right, "delete before")
            try select(after, end: true)
            for _ in after { input().deleteBackward() }
            try check(left + middle + right, "backspace after")
            XCTAssertTrue(try descendants(cells()[0]).contains { $0["type"] as? String == nodeType }, "text deletion removed \(nodeType) prematurely")
            if backspace {
                input().selectedRange = NSRange(location: input().textStorage.length, length: 0)
                let maximumBackspaces = input().textStorage.length + (try descendants(cells()[0]).count) * 2
                var presses = 0
                while try descendants(cells()[0]).contains(where: { $0["type"] as? String == "text" || $0["type"] as? String == nodeType }) && presses < maximumBackspaces {
                    input().deleteBackward()
                    presses += 1
                }
            } else {
                input().selectedRange = NSRange(location: 0, length: input().textStorage.length)
                input().deleteBackward()
            }
            try check("", "delete across element")
            XCTAssertFalse(try descendants(cells()[0]).contains { $0["type"] as? String == nodeType }, "selected \(nodeType) survived deletion")
            input().insertText("again")
            try check("again", "type after removal")
        }
    }
}
