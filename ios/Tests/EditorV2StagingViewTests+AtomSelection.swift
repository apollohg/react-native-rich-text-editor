import ExpoModulesCore
import XCTest

extension EditorV2StagingViewTests {
    func testBackspaceDeletesLiteralPlaceholderTextBeforeSelectingCustomAtom() throws {
        let bound = makeTerminalAtomView()
        let view = bound.view
        let input = view.textView
        defer { view.removeFromSuperview(); bound.window.isHidden = true }
        let document = #"""
            {"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"Before"}]},
                {"type":"counterCard","attrs":{"count":7}},
                {"type":"paragraph","content":[{"type":"text","text":"\u200b"}]}
            ]}
            """#
        XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(bound.adapter.setContentJson(document))))
        setCollapsedCaret(in: input, utf16Offset: input.textStorage.length)
        input.textViewDidChangeSelection(input)
        flushMain()
        input.deleteBackward()
        XCTAssertEqual(v2DocumentText(bound.adapter), "Before", "a literal zero-width space is content and must be deleted first")
        XCTAssertEqual(try XCTUnwrap(bound.adapter.cachedAtomicRenderSelection())["type"] as? String, "text")
        input.deleteBackward()
        XCTAssertEqual(try XCTUnwrap(bound.adapter.cachedAtomicRenderSelection())["type"] as? String, "node")
    }

    func testBackspaceAtFollowingParagraphStartKeepsInterveningEmptyParagraphBoundary() throws {
        let bound = makeTerminalAtomView(
            html: #"<p>Before</p><div data-type="counter-card" data-count="7"></div><p></p><p>after</p>"#
        )
        let view = bound.view
        let input = view.textView
        defer { view.removeFromSuperview(); bound.window.isHidden = true }
        let followingRange = (input.textStorage.string as NSString).range(of: "after")
        guard followingRange.location != NSNotFound else { return XCTFail("fixture did not render its following paragraph") }
        setCollapsedCaret(in: input, utf16Offset: followingRange.location)
        input.textViewDidChangeSelection(input)
        flushMain()
        input.deleteBackward()
        let selection = try XCTUnwrap(bound.adapter.cachedAtomicRenderSelection())
        XCTAssertEqual(selection["type"] as? String, "text", "Backspace must remove the preceding empty paragraph before selecting the atom")
        let documentData = try XCTUnwrap(bound.adapter.documentJson()?.data(using: .utf8))
        let document = try XCTUnwrap(JSONSerialization.jsonObject(with: documentData) as? [String: Any])
        let blocks = try XCTUnwrap(document["content"] as? [[String: Any]])
        XCTAssertEqual(blocks.map { $0["type"] as? String }, ["paragraph", "counterCard", "paragraph"])
        XCTAssertEqual(v2DocumentText(bound.adapter), "Beforeafter", "the atom and both text neighbours must survive")
    }

    func testBackspaceThroughExampleContentAfterCustomAtomKeepsAtomDeletable() throws {
        let bound = makeTerminalAtomView()
        let view = bound.view
        let input = view.textView
        defer {
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            bound.window.isHidden = true
        }
        let exampleDocument = #"""
            {"type":"doc","content":[
                {"type":"paragraph","content":[{"type":"text","text":"Before"}]},
                {"type":"counterCard","attrs":{"count":2}},
                {"type":"ordered_list","content":[
                    {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"Insert another with the + button"}]}]},
                    {"type":"list_item","content":[{"type":"paragraph","content":[{"type":"text","text":"Tap a counter to select it, then delete it like any block"}]}]}
                ]},
                {"type":"horizontal_rule"},
                {"type":"paragraph"}
            ]}
            """#
        XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(bound.adapter.setContentJson(exampleDocument))))
        XCTAssertTrue(try XCTUnwrap(bound.adapter.documentJson()).contains("ordered_list"), "fixture must contain the example's following list")
        XCTAssertTrue(try XCTUnwrap(bound.adapter.documentJson()).contains("counterCard"), "fixture must contain the registered custom atom")
        XCTAssertTrue(input.becomeFirstResponder())
        setCollapsedCaret(in: input, utf16Offset: input.textStorage.length)
        input.textViewDidChangeSelection(input)
        flushMain()
        let maximumBackspaces = input.textStorage.length + Self.atomBoundaryBackspaceAllowance
        var atomSelected = false
        for step in 0..<maximumBackspaces {
            let before = try XCTUnwrap(bound.adapter.documentJson())
            let beforeSelection = input.selectedRange
            input.deleteBackward()
            let after = try XCTUnwrap(bound.adapter.documentJson())
            let selection = try XCTUnwrap(bound.adapter.cachedAtomicRenderSelection())
            if selection["type"] as? String == "node",
               let attachment = input.textStorage.attribute(.attachment, at: input.selectedRange.location, effectiveRange: nil) as? AtomBlockAttachment {
                atomSelected = attachment.nodeType == "counterCard"
                break
            }
            XCTAssertTrue(after != before || input.selectedRange != beforeSelection,
                "Backspace stuck after example CounterCard at step=\(step), native=\(String(reflecting: input.textStorage.string)), selection=\(selection)")
            if after == before, input.selectedRange == beforeSelection { break }
        }
        XCTAssertTrue(atomSelected, "deleting the example's ordered list and rule must reach a selectable CounterCard")
        input.deleteBackward()
        XCTAssertFalse(try XCTUnwrap(bound.adapter.documentJson()).contains("counterCard"))
        XCTAssertFalse(input.textStorage.string.contains(LayoutConstants.objectReplacementCharacter))
        input.insertText("again")
        XCTAssertEqual(v2DocumentText(bound.adapter), "Beforeagain")
        XCTAssertTrue(input.textStorage.string.contains("again"), "continued typing must render after removing the example atom")
        XCTAssertGreaterThan(input.caretRect(for: try XCTUnwrap(input.selectedTextRange?.start)).height, 0)
    }

    func testBackspaceRemovesSelectedCustomAtomAndPreservesSurroundingText() throws {
        let bound = makeTerminalAtomView(
            html: #"<p>Before</p><div data-type="counter-card" data-count="7"></div><p>after</p>"#
        )
        let view = bound.view
        let input = view.textView
        defer { view.removeFromSuperview(); bound.window.isHidden = true }
        let atomRange = (input.textStorage.string as NSString).range(of: LayoutConstants.objectReplacementCharacter)
        guard atomRange.location != NSNotFound else { return XCTFail("fixture did not render its custom atom") }
        let atom = try XCTUnwrap(input.textStorage.attribute(.attachment, at: atomRange.location, effectiveRange: nil) as? AtomBlockAttachment)
        let selection = try XCTUnwrap(EditorV2Shadow.setNodeSelection(id: view.editorId, docPos: atom.docPos))
        XCTAssertTrue(input.applyUpdateJSON(try XCTUnwrap(selection.refreshedUpdateJSON)))
        XCTAssertEqual(input.selectedRange, atomRange)
        input.deleteBackward()
        XCTAssertFalse(try XCTUnwrap(bound.adapter.documentJson()).contains("counterCard"))
        XCTAssertEqual(v2DocumentText(bound.adapter), "Beforeafter", "atom deletion must preserve both neighbours")
        XCTAssertFalse(input.textStorage.string.contains(LayoutConstants.objectReplacementCharacter))
    }

    func testBackspaceAfterCustomAtomRemovesFollowingTextThenAtom() throws {
        let followingText = "after"
        let bound = makeTerminalAtomView(
            html: #"<p>Before</p><div data-type="counter-card" data-count="7"></div><p>after</p>"#
        )
        let view = bound.view
        let input = view.textView
        defer {
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            bound.window.isHidden = true
        }
        XCTAssertTrue(input.becomeFirstResponder())
        setCollapsedCaret(in: input, utf16Offset: input.textStorage.length)
        input.textViewDidChangeSelection(input)
        flushMain()
        for remaining in stride(from: followingText.count - 1, through: 0, by: -1) {
            input.deleteBackward()
            XCTAssertEqual(v2DocumentText(bound.adapter), "Before" + String(followingText.prefix(remaining)),
                "Backspace must remove following content, remaining=\(remaining)")
        }
        let beforeAtomDeletion = try v2DocumentJSON(bound.adapter)
        input.deleteBackward()
        XCTAssertEqual(try v2DocumentJSON(bound.adapter), beforeAtomDeletion,
            "the first boundary Backspace selects the custom atom")
        flushMain()
        let selection = try XCTUnwrap(bound.adapter.cachedAtomicRenderSelection())
        XCTAssertEqual(selection["type"] as? String, "node", "custom atom must become deletable via native selection")
        XCTAssertEqual(input.selectedRange.length, 1, "native selection must cover the custom atom attachment")
        input.deleteBackward()
        XCTAssertFalse(try XCTUnwrap(bound.adapter.documentJson()).contains("counterCard"),
            "a second Backspace must remove the selected custom atom")
        XCTAssertFalse(input.textStorage.string.contains("\u{FFFC}"), "deleted atom must leave no stale native attachment")
        let deletedDocument = try v2DocumentJSON(bound.adapter)
        input.performToolbarUndo()
        XCTAssertEqual(try v2DocumentJSON(bound.adapter), beforeAtomDeletion, "Undo must restore the exact custom atom and its neighbours")
        XCTAssertEqual(input.selectedRange.length, 1, "Undo must restore the atom's native selection")
        input.performToolbarRedo()
        XCTAssertEqual(try v2DocumentJSON(bound.adapter), deletedDocument, "Redo must delete the same atom without changing its neighbours")
        input.insertText("again")
        XCTAssertEqual(v2DocumentText(bound.adapter), "Beforeagain", "typing must remain usable after removing the custom atom")
        XCTAssertTrue(input.textStorage.string.contains("again"), "continued typing must render after custom atom history")
        XCTAssertGreaterThan(input.caretRect(for: try XCTUnwrap(input.selectedTextRange?.start)).height, 0)
    }

    func testStagingBindRendersFromV2Session() {
        let bound = makeBoundView()
        let view = bound.view
        let adapter = bound.adapter
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }

        XCTAssertEqual(view.textView.textStorage.string, "Hello")
        XCTAssertEqual(v2DocumentText(adapter), "Hello")
        XCTAssertGreaterThan(adapter.baseDocumentRevision, 0)
    }

    func testStagingEditorOwnsTextDragAndDropDelegates() {
        let bound = makeBoundView()
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }

        XCTAssertTrue(view.textView.textDragDelegate === view.textView)
        XCTAssertTrue(view.textView.textDropDelegate === view.textView)
    }

    func testStagingTerminalAtomDoesNotExposeAdjacentCaretsOrExtraHeight() {
        let bound = makeTerminalAtomView()
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }
        let atomRect = terminalAtomRect(in: view.textView)
        for offset in [0, view.textView.textStorage.length] {
            setCollapsedCaret(in: view.textView, utf16Offset: offset)
            guard let position = view.textView.selectedTextRange?.start else {
                XCTFail("expected selected position")
                return
            }
            XCTAssertTrue(view.textView.caretRect(for: position).isEmpty)
        }
        let measuredHeight = view.textView.measuredAutoGrowHeightForTesting(
            width: view.textView.bounds.width
        )

        XCTAssertLessThanOrEqual(
            measuredHeight,
            ceil(atomRect.maxY + view.textView.textContainerInset.bottom) + 0.5
        )
    }

    func testStagingAtomBoundarySelectionRestoresLastParagraphCaret() {
        let bound = makeTerminalAtomView(
            html: #"<p>Before</p><div data-type="counter-card" data-count="7"></div>"#
        )
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }
        let validRange = NSRange(location: 3, length: 0)
        setCollapsedCaret(in: view.textView, utf16Offset: validRange.location)
        view.textView.textViewDidChangeSelection(view.textView)
        let atomOffset = view.textView.textStorage.length - 1

        for offset in [atomOffset, atomOffset + 1] {
            setCollapsedCaret(in: view.textView, utf16Offset: offset)
            view.textView.textViewDidChangeSelection(view.textView)
            XCTAssertEqual(view.textView.selectedRange, validRange)
        }
    }

    func testStagingTypingAtTerminalAtomBoundaryDoesNotChangeDocument() {
        let bound = makeTerminalAtomView()
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }
        let htmlBefore = EditorV2Shadow.getHtml(id: view.editorId)
        setCollapsedCaret(in: view.textView, utf16Offset: view.textView.textStorage.length)
        flushMain()

        view.textView.insertText("x")

        XCTAssertEqual(EditorV2Shadow.getHtml(id: view.editorId), htmlBefore)
    }

    func testStagingReturnAtTerminalAtomBoundaryDoesNotChangeDocument() {
        let bound = makeTerminalAtomView()
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }
        let htmlBefore = EditorV2Shadow.getHtml(id: view.editorId)
        setCollapsedCaret(in: view.textView, utf16Offset: view.textView.textStorage.length)
        flushMain()

        view.textView.insertText("\n")

        XCTAssertEqual(EditorV2Shadow.getHtml(id: view.editorId), htmlBefore)
    }

    func testStagingBackspaceInEmptyParagraphAfterTerminalAtomKeepsAtom() {
        let bound = makeTerminalAtomView()
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }
        let htmlBefore = EditorV2Shadow.getHtml(id: view.editorId)
        setCollapsedCaret(in: view.textView, utf16Offset: view.textView.textStorage.length)
        flushMain()

        view.textView.insertText("\n")
        view.textView.deleteBackward()

        XCTAssertEqual(EditorV2Shadow.getHtml(id: view.editorId), htmlBefore)
    }

    func testStagingAtomLineHitKeepsExistingParagraphPosition() throws {
        let bound = makeTerminalAtomView(
            html: #"<p>Before</p><div data-type="counter-card" data-count="7"></div>"#
        )
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }
        let textView = view.textView
        let atomRect = terminalAtomRect(in: textView)
        setCollapsedCaret(in: textView, utf16Offset: 3)
        textView.textViewDidChangeSelection(textView)

        let hitPosition = try XCTUnwrap(textView.closestPosition(
            to: CGPoint(x: atomRect.maxX - 1, y: atomRect.midY)
        ))
        let documentRange = try XCTUnwrap(textView.textRange(
            from: textView.beginningOfDocument,
            to: textView.endOfDocument
        ))
        let constrainedHitPosition = try XCTUnwrap(textView.closestPosition(
            to: CGPoint(x: atomRect.maxX - 1, y: atomRect.midY),
            within: documentRange
        ))

        XCTAssertEqual(
            textView.offset(from: textView.beginningOfDocument, to: hitPosition),
            3
        )
        XCTAssertEqual(
            textView.offset(from: textView.beginningOfDocument, to: constrainedHitPosition),
            3
        )
    }

    func testStagingFixedHeightParagraphHitBeforeOffscreenAtomMovesCaret() throws {
        let bound = makeTerminalAtomView(
            html: #"""
            <p>First line</p><p>Second line</p><p>Third line</p><p>Fourth line</p>
            <p>Fifth line</p><p>Sixth line</p><p>Seventh line</p><p>Eighth line</p>
            <p>Ninth line</p><p>Tenth line</p>
            <div data-type="counter-card" data-count="7"></div>
            """#,
            initialFrame: .zero,
            finalFrame: CGRect(x: 0, y: 0, width: 320, height: 480)
        )
        let view = bound.view
        let window = bound.window
        defer { view.removeFromSuperview(); window.isHidden = true }
        let textView = view.textView
        view.applyTheme(EditorTheme(dictionary: [
            "contentInsets": [
                "top": 28,
                "right": 20,
                "bottom": 336,
                "left": 20
            ]
        ]))
        view.layoutIfNeeded()
        XCTAssertTrue(textView.becomeFirstResponder())
        flushMain()
        XCTAssertEqual(textView.textContainer.size.height, CGFloat.greatestFiniteMagnitude)
        setCollapsedCaret(in: textView, utf16Offset: 3)
        textView.textViewDidChangeSelection(textView)
        let afterRange = (textView.textStorage.string as NSString).range(of: "Second")
        let glyphRange = textView.layoutManager.glyphRange(
            forCharacterRange: NSRange(location: afterRange.location + 2, length: 1),
            actualCharacterRange: nil
        )
        let glyphRect = textView.layoutManager.boundingRect(
            forGlyphRange: glyphRange,
            in: textView.textContainer
        )
        let point = CGPoint(
            x: glyphRect.midX + textView.textContainerInset.left,
            y: glyphRect.midY + textView.textContainerInset.top
        )

        let hitPosition = try XCTUnwrap(textView.closestPosition(to: point))
        let hitOffset = textView.offset(
            from: textView.beginningOfDocument,
            to: hitPosition
        )

        XCTAssertTrue(afterRange.location...NSMaxRange(afterRange) ~= hitOffset)
    }

}
