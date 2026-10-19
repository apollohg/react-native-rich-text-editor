import ExpoModulesCore
import XCTest

private enum ExampleDocumentFixture {
    static let codeText = "const greet"
    static let typedCharacters = ["a", "b", "c"]
    static let highlightTimeout: TimeInterval = 3
    static let runLoopSlice: TimeInterval = 0.005
    static let settleInterval: TimeInterval = 0.2
    static let html = "<h1>Field notes</h1>" +
        String(repeating: "<p>A native editor with a Rust core. Everything below is editable: headings, emphasis, underline, strikethrough, and links.</p>", count: 5) +
        "<pre><code>const greet = (name) =&gt; {\n    return name;\n};</code></pre>" +
        "<img src='test://image' width='400' height='267'><p>Counters</p><img src='test://counter' width='400' height='120'>" +
        "<ol><li><p>Insert another with the + button</p></li><li><p>Tap a counter to select it, then delete it like any block</p></li></ol><hr><p></p>"

    static var theme: EditorTheme {
        EditorTheme(dictionary: ["version": 1, "styles": [
            "content": ["paddingTop": 28, "paddingLeft": 20, "paddingRight": 20, "paddingBottom": 36],
            "text": ["fontSize": 17],
            "paragraph": ["lineHeight": 27, "marginBottom": 12],
            "orderedList": ["indent": 20, "baseIndentMultiplier": 1, "marginBottom": 12],
            "listItem": ["marginBottom": 4],
            "codeBlock": ["paddingTop": 12, "paddingBottom": 12, "marginTop": 12, "marginBottom": 12]
        ]])
    }
}

extension RichTextEditorViewTests {
    func testCaretAfterInvalidatingEarlierLayoutKeepsItsPosition() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 440, height: 600))
        textView.theme = ExampleDocumentFixture.theme
        textView.bindEditor(id: editorId, initialHTML: ExampleDocumentFixture.html)
        textView.layoutIfNeeded()
        let range = (textView.textStorage.string as NSString).range(of: "block")
        setCollapsedSelection(in: textView, utf16Offset: NSMaxRange(range))
        textView.layoutManager.ensureLayout(for: textView.textContainer)
        for edit in 0..<4 {
            if edit == 1 || edit == 2 { textView.insertText("\n") }
            if edit == 3 { textView.deleteBackward() }
            for offset in [0, 10, 100, range.location - 20] {
                textView.layoutManager.invalidateLayout(
                    forCharacterRange: NSRange(location: offset, length: 1),
                    actualCharacterRange: nil
                )
                let position = try XCTUnwrap(textView.selectedTextRange?.start)
                let first = textView.caretRect(for: position)
                textView.layoutManager.ensureLayout(for: textView.textContainer)
                let settled = textView.caretRect(for: position)
                XCTAssertEqual(first.minY, settled.minY, accuracy: 1, "edit \(edit), offset \(offset)")
                XCTAssertEqual(first.minX, settled.minX, accuracy: 1, "edit \(edit), offset \(offset)")
            }
        }
    }

    func testCaretNearDocumentStartLeavesDistantLayoutLazy() throws {
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        textView.attributedText = NSAttributedString(
            string: String(repeating: "Paragraph with enough text to wrap onto another line.\n", count: 2_000),
            attributes: [.font: UIFont.systemFont(ofSize: 17)]
        )
        textView.layoutManager.invalidateLayout(
            forCharacterRange: NSRange(location: 0, length: textView.textStorage.length),
            actualCharacterRange: nil
        )
        let position = try XCTUnwrap(textView.position(from: textView.beginningOfDocument, offset: 4))
        XCTAssertGreaterThan(textView.caretRect(for: position).height, 0)
        XCTAssertLessThan(textView.layoutManager.firstUnlaidCharacterIndex(), textView.textStorage.length / 2)
    }

    func testCaretRectInTallLineHeightListItemUsesResolvedGlyphBaseline() {
        let theme = EditorTheme(dictionary: [
            "paragraph": [
                "lineHeight": 32
            ],
            "list": [
                "markerScale": 2
            ]
        ])
        let json = """
        [
            {"type": "blockStart", "nodeType": "listItem", "depth": 1,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 2},
            {"type": "textRun", "text": "Bullet item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"}
        ]
        """

        let attributed = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: theme
        )

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        let plainTextView = UITextView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        textView.attributedText = attributed
        plainTextView.attributedText = attributed
        textView.layoutIfNeeded()
        plainTextView.layoutIfNeeded()

        let position = textView.position(from: textView.beginningOfDocument, offset: 0)
        let plainPosition = plainTextView.position(from: plainTextView.beginningOfDocument, offset: 0)
        XCTAssertNotNil(position)
        XCTAssertNotNil(plainPosition)

        guard let caretPosition = position, let plainCaretPosition = plainPosition else { return }
        let caretRect = textView.caretRect(for: caretPosition)
        let plainCaretRect = plainTextView.caretRect(for: plainCaretPosition)
        let expected = expectedCaretRect(
            in: plainTextView,
            offset: 0,
            referenceRect: plainCaretRect,
            font: UIFont.systemFont(ofSize: 16)
        )

        XCTAssertEqual(caretRect.origin.y, expected.origin.y, accuracy: 1.0)
        XCTAssertEqual(caretRect.height, expected.height, accuracy: 1.0)
    }

    func testCaretRectUsesResolvedGlyphBaselineAcrossWrappedParagraphLines() {
        let theme = EditorTheme(dictionary: [
            "paragraph": [
                "lineHeight": 32
            ]
        ])
        let json = """
        [
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "This is a wrapped paragraph for caret alignment checks across multiple lines.", "marks": []},
            {"type": "blockEnd"}
        ]
        """

        let attributed = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: theme
        )

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 120, height: 240))
        let plainTextView = UITextView(frame: CGRect(x: 0, y: 0, width: 120, height: 240))
        textView.attributedText = attributed
        plainTextView.attributedText = attributed
        textView.layoutIfNeeded()
        plainTextView.layoutIfNeeded()

        let offsets = [0, 20, attributed.length - 1]
        for offset in offsets {
            guard let position = textView.position(from: textView.beginningOfDocument, offset: offset) else {
                XCTFail("expected position for offset \(offset)")
                continue
            }
            guard let plainPosition = plainTextView.position(from: plainTextView.beginningOfDocument, offset: offset) else {
                XCTFail("expected plain position for offset \(offset)")
                continue
            }

            let caretRect = textView.caretRect(for: position)
            let plainCaretRect = plainTextView.caretRect(for: plainPosition)
            let expected = expectedCaretRect(
                in: plainTextView,
                offset: offset,
                referenceRect: plainCaretRect,
                font: UIFont.systemFont(ofSize: 16)
            )

            XCTAssertEqual(caretRect.origin.y, expected.origin.y, accuracy: 1.0, "offset \(offset)")
            XCTAssertEqual(caretRect.height, expected.height, accuracy: 1.0, "offset \(offset)")
        }
    }

    func testCaretRectUsesCorrectVisualLineAtWrappedParagraphBoundaries() {
        let theme = EditorTheme(dictionary: [
            "paragraph": [
                "lineHeight": 32
            ]
        ])
        let json = """
        [
            {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
            {"type": "textRun", "text": "This is a wrapped paragraph for caret alignment checks across multiple lines.", "marks": []},
            {"type": "blockEnd"}
        ]
        """

        let attributed = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: theme
        )

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 120, height: 240))
        let plainTextView = UITextView(frame: CGRect(x: 0, y: 0, width: 120, height: 240))
        textView.attributedText = attributed
        plainTextView.attributedText = attributed
        textView.layoutIfNeeded()
        plainTextView.layoutIfNeeded()

        let offsets = [0, 20, attributed.length - 1]
        for offset in offsets {
            guard let position = textView.position(from: textView.beginningOfDocument, offset: offset) else {
                XCTFail("expected position for offset \(offset)")
                continue
            }
            guard let plainPosition = plainTextView.position(from: plainTextView.beginningOfDocument, offset: offset) else {
                XCTFail("expected plain position for offset \(offset)")
                continue
            }

            let caretRect = textView.caretRect(for: position)
            let plainCaretRect = plainTextView.caretRect(for: plainPosition)
            let expected = expectedCaretRect(
                in: plainTextView,
                offset: offset,
                referenceRect: plainCaretRect,
                font: UIFont.systemFont(ofSize: 16)
            )

            XCTAssertEqual(caretRect.origin.y, expected.origin.y, accuracy: 1.0, "offset \(offset)")
        }
    }

    func testCaretRectAfterBlockquoteMatchesPlainTextViewHorizontalPosition() {
        let attributed = RenderBridge.renderElements(
            fromJSON: """
            [
                {"type": "blockStart", "nodeType": "blockquote", "depth": 0},
                {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
                {"type": "textRun", "text": "Hello", "marks": []},
                {"type": "blockEnd"},
                {"type": "blockEnd"},
                {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
                {"type": "textRun", "text": "World", "marks": []},
                {"type": "blockEnd"}
            ]
            """,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: EditorTheme(dictionary: [
                "blockquote": [
                    "indent": 20,
                    "borderWidth": 4,
                    "markerGap": 10
                ]
            ])
        )

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 220, height: 200))
        let plainTextView = UITextView(frame: CGRect(x: 0, y: 0, width: 220, height: 200))
        textView.attributedText = attributed
        plainTextView.attributedText = attributed
        textView.layoutIfNeeded()
        plainTextView.layoutIfNeeded()

        let offset = (attributed.string as NSString).range(of: "World").location + 4
        guard let position = textView.position(from: textView.beginningOfDocument, offset: offset) else {
            XCTFail("expected editor caret position after blockquote")
            return
        }
        guard let plainPosition = plainTextView.position(from: plainTextView.beginningOfDocument, offset: offset) else {
            XCTFail("expected plain caret position after blockquote")
            return
        }

        let caretRect = textView.caretRect(for: position)
        let plainCaretRect = plainTextView.caretRect(for: plainPosition)
        let expected = expectedCaretRect(
            in: plainTextView,
            offset: offset,
            referenceRect: plainCaretRect,
            font: UIFont.systemFont(ofSize: 16)
        )

        XCTAssertEqual(caretRect.minX, expected.minX, accuracy: 1.0)
        XCTAssertEqual(caretRect.minY, expected.minY, accuracy: 1.0)
        XCTAssertEqual(caretRect.height, expected.height, accuracy: 1.0)
    }

    func testCaretRectAfterBlockquoteAlignsToNextCharacterEdge() {
        let attributed = RenderBridge.renderElements(
            fromJSON: """
            [
                {"type": "blockStart", "nodeType": "blockquote", "depth": 0},
                {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
                {"type": "textRun", "text": "Hello", "marks": []},
                {"type": "blockEnd"},
                {"type": "blockEnd"},
                {"type": "blockStart", "nodeType": "paragraph", "depth": 0},
                {"type": "textRun", "text": "World", "marks": []},
                {"type": "blockEnd"}
            ]
            """,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: EditorTheme(dictionary: [
                "blockquote": [
                    "indent": 20,
                    "borderWidth": 4,
                    "markerGap": 10
                ]
            ])
        )

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 220, height: 200))
        textView.attributedText = attributed
        textView.layoutIfNeeded()

        let offset = (attributed.string as NSString).range(of: "World").location + 4
        guard let position = textView.position(from: textView.beginningOfDocument, offset: offset),
            let nextPosition = textView.position(from: position, offset: 1),
            let range = textView.textRange(from: position, to: nextPosition)
        else {
            XCTFail("expected caret and next character positions after blockquote")
            return
        }

        let expectedX = textView.selectionRects(for: range)
            .map(\.rect)
            .first(where: { !$0.isEmpty && $0.width > 0 })?.minX
        XCTAssertNotNil(expectedX)

        let caretRect = textView.caretRect(for: position)
        XCTAssertEqual(caretRect.minX, expectedX ?? caretRect.minX, accuracy: 1.0)
    }

    func testBoundEditorCaretRectAfterBlockquoteMatchesPlainTextViewHorizontalPosition() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 220, height: 200))
        textView.bindEditor(id: editorId, initialHTML: "<blockquote><p>Hello</p></blockquote><p>World</p>")

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: 10, scalarHead: 10)
        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        textView.layoutIfNeeded()

        let plainTextView = UITextView(frame: CGRect(x: 0, y: 0, width: 220, height: 200))
        plainTextView.attributedText = textView.attributedText
        plainTextView.layoutIfNeeded()

        let offset = textView.offset(
            from: textView.beginningOfDocument,
            to: textView.selectedTextRange?.start ?? textView.endOfDocument
        )

        guard let position = textView.position(from: textView.beginningOfDocument, offset: offset) else {
            XCTFail("expected editor caret position after blockquote in bound editor")
            return
        }
        guard let plainPosition = plainTextView.position(from: plainTextView.beginningOfDocument, offset: offset) else {
            XCTFail("expected plain caret position after blockquote in bound editor")
            return
        }

        let caretRect = textView.caretRect(for: position)
        let plainCaretRect = plainTextView.caretRect(for: plainPosition)
        let expected = expectedCaretRect(
            in: plainTextView,
            offset: offset,
            referenceRect: plainCaretRect,
            font: UIFont.systemFont(ofSize: 16)
        )

        XCTAssertEqual(caretRect.minX, expected.minX, accuracy: 1.0)
        XCTAssertEqual(caretRect.minY, expected.minY, accuracy: 1.0)
        XCTAssertEqual(caretRect.height, expected.height, accuracy: 1.0)
    }

    func testTypingAtParagraphEndAfterBlockquoteKeepsCaretAtRenderedEnd() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 220, height: 200))
        textView.bindEditor(id: editorId, initialHTML: "<blockquote><p>Hello</p></blockquote><p>World</p>")

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: 11, scalarHead: 11)
        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        textView.layoutIfNeeded()

        textView.insertText("!")
        textView.layoutIfNeeded()

        let html = EditorV2Shadow.getHtml(id: editorId)
        XCTAssertEqual(html, "<blockquote><p>Hello</p></blockquote><p>World!</p>")

        let caretOffset = textView.offset(
            from: textView.beginningOfDocument,
            to: textView.selectedTextRange?.start ?? textView.endOfDocument
        )
        XCTAssertEqual(caretOffset, textView.text.count, "logical selection should remain at rendered end")

        let plainTextView = UITextView(frame: CGRect(x: 0, y: 0, width: 220, height: 200))
        plainTextView.attributedText = textView.attributedText
        plainTextView.layoutIfNeeded()

        guard let position = textView.position(from: textView.beginningOfDocument, offset: caretOffset),
            let plainPosition = plainTextView.position(from: plainTextView.beginningOfDocument, offset: caretOffset)
        else {
            XCTFail("expected caret positions after typing at paragraph end")
            return
        }

        let caretRect = textView.caretRect(for: position)
        let plainCaretRect = plainTextView.caretRect(for: plainPosition)
        let expected = expectedCaretRect(
            in: plainTextView,
            offset: caretOffset,
            referenceRect: plainCaretRect,
            font: UIFont.systemFont(ofSize: 16)
        )

        XCTAssertEqual(caretRect.minX, expected.minX, accuracy: 1.0)
        XCTAssertEqual(caretRect.minY, expected.minY, accuracy: 1.0)
        XCTAssertEqual(caretRect.height, expected.height, accuracy: 1.0)
    }

    func testTypingAtTheScrolledDocumentEndKeepsTheScrollPositionThroughCodeHighlighting() throws {
        try assertTypingAtDocumentEndKeepsHighlightingAndScrollPosition()
    }

    func testTypingAtTheScrolledDocumentEndWithATableKeepsCodeHighlighting() throws {
        try assertTypingAtDocumentEndKeepsHighlightingAndScrollPosition(withTable: true)
    }

    private func assertTypingAtDocumentEndKeepsHighlightingAndScrollPosition(withTable: Bool = false) throws {
        let provider = EditorHighlightingFixtureProvider()
        try NativeCodeHighlightingRegistry.register(provider: provider)
        let editorId: UInt64
        if withTable {
            var configuration = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(TableInputTestSchema.tableConfig.utf8)) as? [String: Any])
            var schema = try XCTUnwrap(configuration["schema"] as? [String: Any])
            var nodes = try XCTUnwrap(schema["nodes"] as? [[String: Any]])
            nodes.append(["name": "codeBlock", "content": "text*", "group": "block", "role": "textBlock", "htmlTag": "pre"])
            schema["nodes"] = nodes
            configuration["schema"] = schema
            editorId = makeV2Editor(configJson: try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: configuration), encoding: .utf8)))
        } else {
            editorId = makeV2Editor()
        }
        defer { destroyV2Editor(id: editorId) }
        let window = makeTestWindow(frame: CGRect(x: 0, y: 0, width: 440, height: 600))
        let textView = EditorTextView(frame: window.bounds)
        window.addSubview(textView)
        window.makeKeyAndVisible()
        defer {
            _ = textView.resignFirstResponder()
            window.isHidden = true
        }
        textView.theme = ExampleDocumentFixture.theme
        if withTable {
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
            textView.bindEditor(id: editorId, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
            var document = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(TableInputTestSchema.twoCellDocument.utf8)) as? [String: Any])
            var blocks = try XCTUnwrap(document["content"] as? [[String: Any]])
            let paragraph: [String: Any] = ["type": "paragraph", "content": [["type": "text", "text": "A native editor with a Rust core. Everything below is editable: headings, emphasis, underline, strikethrough, and links."]]]
            blocks += Array(repeating: paragraph, count: 5)
            blocks.append(["type": "codeBlock", "content": [["type": "text", "text": "const greet = (name) => {\n    return name;\n};"]]])
            blocks += Array(repeating: paragraph, count: 5)
            blocks.append(["type": "paragraph"])
            document["content"] = blocks
            let json = try XCTUnwrap(String(data: JSONSerialization.data(withJSONObject: document), encoding: .utf8))
            XCTAssertTrue(textView.applyUpdateJSON(try XCTUnwrap(adapter.setContentJson(json))))
        } else {
            textView.bindEditor(id: editorId, initialHTML: ExampleDocumentFixture.html)
        }
        XCTAssertEqual(PositionBridge.hasRootTableScalarExtents(in: textView), withTable, "the table variant must exercise a real collapsed root table")
        let codeLocation = (textView.textStorage.string as NSString).range(of: ExampleDocumentFixture.codeText).location
        guard codeLocation != NSNotFound else { return XCTFail("fixture must contain a rendered code block") }
        func isCodeHighlighted() -> Bool {
            (textView.textStorage.attribute(.foregroundColor, at: codeLocation, effectiveRange: nil) as? UIColor) == UIColor.red
        }
        func waitForHighlightedCode(_ context: String) {
            let deadline = Date().addingTimeInterval(ExampleDocumentFixture.highlightTimeout)
            while !isCodeHighlighted(), Date() < deadline {
                RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(ExampleDocumentFixture.runLoopSlice))
            }
            RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(ExampleDocumentFixture.settleInterval))
            XCTAssertTrue(isCodeHighlighted(), "\(context): the code block keeps its highlighting")
        }
        textView.codeHighlighting = NativeCodeHighlightConfiguration(provider: provider.id, theme: "fixture")
        waitForHighlightedCode("initial")
        XCTAssertTrue(textView.becomeFirstResponder())
        setCollapsedSelection(in: textView, utf16Offset: textView.textStorage.length)
        textView.layoutManager.ensureLayout(for: textView.textContainer)
        _ = textView.layoutManager.usedRect(for: textView.textContainer)
        textView.layoutIfNeeded()
        func maximumOffset() -> CGFloat {
            textView.contentSize.height - textView.bounds.height + textView.adjustedContentInset.bottom
        }
        XCTAssertGreaterThan(maximumOffset(), 0, "the fixture must scroll")
        textView.setContentOffset(CGPoint(x: 0, y: maximumOffset()), animated: false)
        waitForHighlightedCode("scrolled")
        let bottomOffset = textView.contentOffset.y
        XCTAssertEqual(bottomOffset, maximumOffset(), accuracy: 1, "scrolled to the end, contentSize=\(textView.contentSize)")

        var committedOffsets: [CGPoint] = []
        let commitObserver = CFRunLoopObserverCreateWithHandler(
            nil, CFRunLoopActivity.beforeWaiting.union(.exit).rawValue, true, CFIndex.max
        ) { _, _ in
            committedOffsets.append(CGPoint(x: textView.contentSize.height, y: textView.contentOffset.y))
        }
        CFRunLoopAddObserver(CFRunLoopGetMain(), commitObserver, .commonModes)
        defer { CFRunLoopRemoveObserver(CFRunLoopGetMain(), commitObserver, .commonModes) }
        let highlightCountBeforeTyping = provider.highlightCount
        for character in ExampleDocumentFixture.typedCharacters {
            textView.insertText(character)
            XCTAssertTrue(isCodeHighlighted(), "typing outside code must retain its highlighting immediately, including full renders")
            waitForHighlightedCode("after typing \(character)")
        }

        XCTAssertTrue(textView.textStorage.string.hasSuffix(ExampleDocumentFixture.typedCharacters.joined()),
            textView.textStorage.string.debugDescription)
        XCTAssertFalse(committedOffsets.isEmpty)
        XCTAssertTrue(committedOffsets.allSatisfy { abs($0.y - bottomOffset) < 1 },
            "no committed frame may scroll away from y=\(bottomOffset) while typing at the end (contentHeight, offset): \(committedOffsets)")
        XCTAssertEqual(provider.highlightCount, highlightCountBeforeTyping,
            "typing outside the code block must leave its highlighting untouched")

        setCollapsedSelection(in: textView, utf16Offset: codeLocation)
        textView.insertText(ExampleDocumentFixture.typedCharacters[0])
        waitForHighlightedCode("after typing inside the code block")
        XCTAssertGreaterThan(provider.highlightCount, highlightCountBeforeTyping, "an edited code block is highlighted again")
    }
}
