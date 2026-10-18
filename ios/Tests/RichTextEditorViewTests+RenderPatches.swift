import ExpoModulesCore
import XCTest

extension RichTextEditorViewTests {
    private static let retainedMentionLabels = ["@Alice", "@Bob"]
    private static let plainTypingHTML = #"<p><strong>Before after</strong><em> italic</em></p><p><a href="https://example.com">Tail</a></p>"#
    private static let publicStyledTypingHTML = #"<p><strong>Before 😀 after</strong><em> italic</em></p><p><a href="https://example.com">Tail</a></p>"#
    private static let plainTypingPrefix = "Before "
    private static let textKitOriginalFontAttribute = NSAttributedString.Key("NSOriginalFont")

    private enum OriginalFontControl: CaseIterable {
        case absent, differentFont
    }
    private static let overlappingSiblingMarginHTML = #"""
    <h2>Heading 東京</h2><p><strong>Before café é</strong> <em>italic</em></p><blockquote><p>Quote café <a href="https://example.com/margins">link</a></p><blockquote><p><strong>Nested 👩‍💻 text</strong></p></blockquote><p>Quote tail</p></blockquote><pre><code>Code first
    Code second</code></pre><p>After 😀</p>
    """#
    private static let siblingMarginHTML = overlappingSiblingMarginHTML.replacingOccurrences(
        of: "</blockquote><pre>", with: "</blockquote><p>Between decorations</p><pre>")
    private static let overlappingSiblingMarginText = "Heading 東京\nBefore café é italic\nQuote café link\nNested 👩‍💻 text\nQuote tail\nCode first\nCode second\nAfter 😀"
    private static let siblingMarginText = overlappingSiblingMarginText.replacingOccurrences(
        of: "\nCode first", with: "\nBetween decorations\nCode first")
    private static let siblingMarginThemeRoot: [String: Any] = ["version": 1, "styles": [
        "text": ["color": "#0000ffff", "lineHeight": 26],
        "paragraph": ["marginTop": 7, "marginBottom": -3, "paddingTop": 2],
        "h2": ["fontSize": 20, "marginTop": -5, "marginBottom": 12],
        "blockquote": ["marginTop": 13, "marginBottom": -5, "paddingTop": 4,
            "paddingBottom": 6, "paddingLeft": 8, "borderLeftWidth": 3,
            "borderLeftColor": "#ff0000ff"],
        "codeBlock": ["marginTop": -11, "marginBottom": -4,
            "paddingTop": 6, "paddingBottom": 8, "backgroundColor": "#eeeeeeff"]
    ]]

    private func siblingMarginTheme(styleOverrides: [String: [String: Any]] = [:]) throws -> EditorTheme {
        var root = Self.siblingMarginThemeRoot
        var styles = try XCTUnwrap(root["styles"] as? [String: [String: Any]])
        for (nodeType, overrides) in styleOverrides {
            var style = try XCTUnwrap(styles[nodeType], "The control must override an existing fixture style")
            style.merge(overrides) { _, next in next }
            styles[nodeType] = style
        }
        root["styles"] = styles
        return EditorTheme(dictionary: root)
    }

    private func hostSiblingMarginEditor(editorId: UInt64, html: String? = nil) throws -> (RichTextEditorView, UIWindow) {
        let theme = try siblingMarginTheme()
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        let window = hostEditorView(view)
        XCTAssertTrue(view.applyTheme(theme))
        view.editorId = editorId
        view.setContent(html: html ?? Self.siblingMarginHTML)
        let expectedText = html == nil ? Self.siblingMarginText : Self.overlappingSiblingMarginText
        XCTAssertEqual(view.textView.textStorage.string as NSString, expectedText as NSString,
            "The complete mixed Unicode/nested/code fixture must be mounted")
        XCTAssertEqual(try siblingQuoteChildTypes(editorId: editorId), ["paragraph", "blockquote", "paragraph"])
        XCTAssertTrue(view.textView.becomeFirstResponder())
        flushMainQueue()
        view.textView.layoutManager.ensureLayout(for: view.textView.textContainer)
        return (view, window)
    }

    private func siblingQuoteChildTypes(editorId: UInt64) throws -> [String] {
        let document = parseJSONObject(EditorV2Shadow.getJson(id: editorId))
        let roots = try XCTUnwrap(document["content"] as? [[String: Any]])
        let quoteIndex = 2
        let quote = try XCTUnwrap(roots.indices.contains(quoteIndex) ? roots[quoteIndex] : nil)
        XCTAssertEqual(quote["type"] as? String, "blockquote")
        return try XCTUnwrap(quote["content"] as? [[String: Any]]).map {
            try XCTUnwrap($0["type"] as? String)
        }
    }

    private struct StyledMarginGeometry: Equatable {
        let usedRect: CGRect
        let lineRects: [CGRect]
        let caretRect: CGRect
        let bitmap: Data
    }

    private func styledMarginGeometry(in input: EditorTextView) throws -> StyledMarginGeometry {
        input.layoutManager.ensureLayout(for: input.textContainer)
        let used = input.layoutManager.usedRect(for: input.textContainer)
        let glyphRange = NSRange(location: 0, length: input.layoutManager.numberOfGlyphs)
        var lines: [CGRect] = []
        input.layoutManager.enumerateLineFragments(forGlyphRange: glyphRange) { _, rect, _, _, _ in
            lines.append(rect)
        }
        let position = try XCTUnwrap(input.position(from: input.beginningOfDocument,
            offset: input.selectedRange.location))
        let size = CGSize(width: input.bounds.width,
            height: ceil(used.maxY + input.textContainerInset.top + input.textContainerInset.bottom))
        let bitmap = try renderGlyphBitmap(input.layoutManager, glyphRange: glyphRange,
            origin: CGPoint(x: input.textContainerInset.left, y: input.textContainerInset.top), size: size)
        XCTAssertTrue(bitmap.contains { $0 != 0 }, "Margin equality must compare actual painted rich text and decorations")
        return StyledMarginGeometry(usedRect: used, lineRects: lines,
            caretRect: input.caretRect(for: position), bitmap: bitmap)
    }

    private func comparableStyledAttributes(in text: NSAttributedString, at offset: Int) -> [NSAttributedString.Key: Any] {
        func boxValues(_ descriptor: EditorRenderedBox) -> [String: Any] {
            ["box": descriptor.box.values, "depth": descriptor.depth,
                "leading": descriptor.leading, "trailing": descriptor.trailing,
                "topInset": descriptor.topInset, "bottomInset": descriptor.bottomInset]
        }
        var attributes = text.attributes(at: offset, effectiveRange: nil)
        if let boxes = renderedStyleBoxes(from: attributes[editorStyleBoxesAttribute]) {
            attributes[editorStyleBoxesAttribute] = boxes.map(boxValues)
        }
        if let box = attributes[editorBlockSpacingBoxAttribute] as? EditorRenderedBox {
            attributes[editorBlockSpacingBoxAttribute] = boxValues(box)
        }
        if let code = attributes[editorCodeBlockAttribute] as? EditorCodeBlockPresentation {
            attributes[editorCodeBlockAttribute] = ["language": code.language as Any? ?? NSNull()]
        }
        if let rule = attributes[.attachment] as? HorizontalRuleAttachment {
            attributes[.attachment] = [
                "type": String(describing: type(of: rule)), "bounds": NSValue(cgRect: rule.bounds),
                "lineColor": rule.lineColor, "lineHeight": rule.lineHeight,
                "verticalPadding": rule.verticalPadding, "styleBox": rule.styleBox?.values as Any? ?? NSNull()
            ] as [String: Any]
        }
        return attributes
    }

    private func freshSiblingMarginReference(
        for input: EditorTextView, blocks: [[NSDictionary]], theme: EditorTheme
    ) -> EditorTextView {
        styledDrawingReference(for: input, storage: RenderBridge.renderBlocks(fromArray: blocks,
            baseFont: input.baseFont, textColor: input.baseTextColor, theme: theme), theme: theme)
    }

    private func styledDrawingReference(
        for input: EditorTextView, storage: NSAttributedString, theme: EditorTheme?
    ) -> EditorTextView {
        let reference = EditorTextView(frame: input.bounds)
        reference.theme = theme
        reference.baseFont = input.baseFont
        reference.baseTextColor = input.baseTextColor
        reference.textContainerInset = input.textContainerInset
        reference.textContainer.lineFragmentPadding = input.textContainer.lineFragmentPadding
        reference.textStorage.setAttributedString(storage)
        reference.selectedRange = input.selectedRange
        return reference
    }

    private func assertStyledDrawingMatchesFreshStorage(
        in input: EditorTextView, context: String
    ) throws -> StyledMarginGeometry {
        let reference = styledDrawingReference(for: input,
            storage: NSAttributedString(attributedString: input.textStorage), theme: input.theme)
        let actual = try styledMarginGeometry(in: input)
        let expected = try styledMarginGeometry(in: reference)
        XCTAssertEqual(actual, expected, "\(context): live drawing must match a fresh TextKit manager")
        XCTAssertEqual(input.textStorage.string, reference.textStorage.string, context)
        XCTAssertEqual(input.selectedRange, reference.selectedRange, context)
        guard input.textStorage.length == reference.textStorage.length else {
            XCTFail("\(context): live/reference UTF16 lengths differ")
            return actual
        }
        for offset in 0..<input.textStorage.length {
            XCTAssertTrue(NSDictionary(dictionary: comparableStyledAttributes(in: input.textStorage, at: offset))
                .isEqual(to: comparableStyledAttributes(in: reference.textStorage, at: offset)),
                "\(context): all attributed values must remain exact at UTF16 \(offset)")
        }
        return actual
    }

    func testRepeatedStyledDrawsAvoidRebuildingUnchangedContainerGroups() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let (view, window) = try hostSiblingMarginEditor(editorId: editorId, html: Self.overlappingSiblingMarginHTML)
        let input = view.textView
        defer {
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        let selection = input.selectedRange
        let before = NSAttributedString(attributedString: input.textStorage)
        let expected = try assertStyledDrawingMatchesFreshStorage(in: input, context: "first actual paint")
        let previousObserver = PreparedProseInstrumentation.tableStageObserverForTesting
        defer { PreparedProseInstrumentation.tableStageObserverForTesting = previousObserver }
        var groupingDurations: [UInt64] = []
        PreparedProseInstrumentation.tableStageObserverForTesting = { stage, start, end in
            if stage == .editorStyledBoxGrouping { groupingDurations.append(end - start) }
        }
        let paintProbe = PREPEditorPaintProbe(layoutManager: input.layoutManager)
        defer { paintProbe.close() }
        var actualDrawCount = 0
        paintProbe.onDraw = { _, _ in actualDrawCount += 1 }
        let repeatedPaintCount = 20
        for iteration in 0..<repeatedPaintCount {
            XCTAssertEqual(try styledMarginGeometry(in: input), expected,
                "Repeated real glyph paint \(iteration) must preserve exact geometry and RGBA pixels")
            XCTAssertEqual(input.selectedRange, selection)
            XCTAssertTrue(input.textStorage.isEqual(to: before))
        }
        XCTAssertEqual(actualDrawCount, repeatedPaintCount, "The performance regression requires every real glyph draw")
        XCTAssertEqual(groupingDurations.count, 0,
            "Unchanged immutable styled containers must reuse their initial grouping; actual repeated work=\(groupingDurations.count), durationsNanos=\(groupingDurations)")
    }

    func testStyledDrawingKeepsMutableInsetsWidthAndOriginLive() throws {
        let input = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        input.theme = try siblingMarginTheme()
        let elements: [[String: Any]] = [
            ["type": "blockStart", "nodeType": "blockquote", "depth": 0],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 1],
            ["type": "textRun", "text": "Wrapped café é 東京 😀 styled text that changes lines when the real container becomes narrow.", "marks": ["bold"]],
            ["type": "blockEnd"], ["type": "blockEnd"]
        ]
        input.textStorage.setAttributedString(RenderBridge.renderElements(fromArray: elements,
            baseFont: input.baseFont, textColor: input.baseTextColor, theme: input.theme))
        let initial = try assertStyledDrawingMatchesFreshStorage(in: input, context: "warm original geometry")
        let boxes = try XCTUnwrap(renderedStyleBoxes(from: input.textStorage.attribute(editorStyleBoxesAttribute,
            at: 0, effectiveRange: nil)))
        let quote = try XCTUnwrap(boxes.first { $0.depth == 0 })
        let storedAttributes = NSAttributedString(attributedString: input.textStorage)
        quote.topInset += 9
        quote.bottomInset += 11
        XCTAssertTrue(input.textStorage.isEqual(to: storedAttributes), "Inset mutation keeps the same immutable stack and descriptor identities")
        let insetGeometry = try assertStyledDrawingMatchesFreshStorage(in: input, context: "mutable descriptor insets")
        XCTAssertNotEqual(insetGeometry.bitmap, initial.bitmap, "The live border must actually move/resize")
        input.bounds.size.width = 180
        input.layoutIfNeeded()
        let narrow = try assertStyledDrawingMatchesFreshStorage(in: input, context: "real container resize")
        XCTAssertGreaterThan(narrow.lineRects.count, insetGeometry.lineRects.count, "The width control must change actual wrapping")
        input.textContainerInset = UIEdgeInsets(top: 19, left: 23, bottom: 17, right: 7)
        input.layoutIfNeeded()
        let shifted = try assertStyledDrawingMatchesFreshStorage(in: input, context: "changed drawing origin")
        XCTAssertNotEqual(shifted.bitmap, narrow.bitmap, "The origin control must change actual painted positions")
    }

    func testStyledDrawingObservesDirtyAttributesAndNilDelegateStorageReplacement() throws {
        let input = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let originalDelegate = input.textStorage.delegate
        input.textStorage.delegate = nil
        defer { input.textStorage.delegate = originalDelegate }
        let text = "Styled storage café"
        let red = EditorRenderedBox(box: EditorStyleBox(["backgroundColor": "#ff0000ff"]),
            depth: 0, leading: 0, trailing: 0)
        let gray = EditorRenderedBox(box: EditorStyleBox(["backgroundColor": "#eeeeeeff"]),
            depth: 0, leading: 0, trailing: 0)
        input.textStorage.setAttributedString(NSAttributedString(string: text, attributes: [
            .font: input.baseFont, .foregroundColor: UIColor.blue,
            editorStyleBoxesAttribute: EditorRenderedBoxStack(boxes: [red])
        ]))
        let original = try assertStyledDrawingMatchesFreshStorage(in: input, context: "initial immutable red stack")
        let glyphs = NSRange(location: 0, length: input.layoutManager.numberOfGlyphs)
        let canvas = CGSize(width: input.bounds.width,
            height: CGFloat(original.bitmap.count / (Int(input.bounds.width) * 4)))
        let origin = CGPoint(x: input.textContainerInset.left, y: input.textContainerInset.top)
        let range = NSRange(location: 0, length: input.textStorage.length)
        // Only box attributes change; characters, fonts and laid-out glyph ranges remain identical.
        do {
            input.textStorage.beginEditing()
            defer { input.textStorage.endEditing() }
            input.textStorage.addAttribute(editorStyleBoxesAttribute,
                value: EditorRenderedBoxStack(boxes: [gray]), range: range)
            XCTAssertTrue(input.textStorage.editedMask.contains(.editedAttributes))
            let grayReference = styledDrawingReference(for: input,
                storage: NSAttributedString(attributedString: input.textStorage), theme: input.theme)
            let expectedGray = try styledMarginGeometry(in: grayReference)
            let actualGray = try renderGlyphBitmap(input.layoutManager, glyphRange: glyphs, origin: origin, size: canvas)
            XCTAssertEqual(actualGray, expectedGray.bitmap, "The first pending attribute read must paint the current gray stack")
            XCTAssertNotEqual(actualGray, original.bitmap)
            input.textStorage.removeAttribute(editorStyleBoxesAttribute, range: range)
            let plainReference = styledDrawingReference(for: input,
                storage: NSAttributedString(attributedString: input.textStorage), theme: input.theme)
            XCTAssertEqual(try renderGlyphBitmap(input.layoutManager, glyphRange: glyphs, origin: origin, size: canvas),
                try styledMarginGeometry(in: plainReference).bitmap,
                "A second read in the same open batch must observe removed containers")
            input.textStorage.addAttribute(editorStyleBoxesAttribute,
                value: EditorRenderedBoxStack(boxes: [red]), range: range)
            XCTAssertEqual(try renderGlyphBitmap(input.layoutManager, glyphRange: glyphs, origin: origin, size: canvas), original.bitmap,
                "A third pending read must observe restored identities before didProcessEditing")
        }
        _ = try assertStyledDrawingMatchesFreshStorage(in: input, context: "closed attribute batch with nil delegate")
        let prefix = "Prefix 👩‍💻\n"
        input.textStorage.replaceCharacters(in: NSRange(location: 0, length: 0),
            with: NSAttributedString(string: prefix, attributes: [.font: input.baseFont, .foregroundColor: UIColor.blue]))
        XCTAssertEqual(input.textStorage.string, prefix + text)
        _ = try assertStyledDrawingMatchesFreshStorage(in: input, context: "UTF16-changing prefix with nil delegate")
        input.textStorage.setAttributedString(NSAttributedString(string: "No containers", attributes: [
            .font: input.baseFont, .foregroundColor: UIColor.blue
        ]))
        input.selectedRange = NSRange(location: 0, length: 0)
        let plain = try assertStyledDrawingMatchesFreshStorage(in: input, context: "whole replacement with no containers")
        input.textStorage.addAttribute(editorStyleBoxesAttribute, value: EditorRenderedBoxStack(boxes: [gray]),
            range: NSRange(location: 0, length: input.textStorage.length))
        let restored = try assertStyledDrawingMatchesFreshStorage(in: input, context: "containers after verified absence")
        XCTAssertNotEqual(restored.bitmap, plain.bitmap)
        input.textStorage.setAttributedString(NSAttributedString(string: ""))
        input.layoutManager.ensureLayout(for: input.textContainer)
        XCTAssertEqual(input.layoutManager.numberOfGlyphs, 0)
        input.theme = try siblingMarginTheme(styleOverrides: ["blockquote": ["borderLeftColor": "#00ff00ff"]])
        input.textStorage.setAttributedString(RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "blockquote", "depth": 0],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 1],
            ["type": "textRun", "text": "New themed 東京", "marks": ["italic"]],
            ["type": "blockEnd"], ["type": "blockEnd"]
        ], baseFont: input.baseFont, textColor: input.baseTextColor, theme: input.theme))
        _ = try assertStyledDrawingMatchesFreshStorage(in: input, context: "new themed document after empty replacement")
    }

    func testStyledDrawingReadsLegacyMutableStackChangesWithoutStorageEdits() throws {
        let input = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let red = EditorRenderedBox(box: EditorStyleBox(["backgroundColor": "#ff0000ff"]),
            depth: 0, leading: 0, trailing: 0)
        let gray = EditorRenderedBox(box: EditorStyleBox(["backgroundColor": "#eeeeeeff"]),
            depth: 0, leading: 0, trailing: 0)
        let mutableStack = NSMutableArray(array: [red, gray])
        input.textStorage.setAttributedString(NSAttributedString(string: "Legacy mutable café", attributes: [
            .font: input.baseFont, .foregroundColor: UIColor.blue, editorStyleBoxesAttribute: mutableStack
        ]))
        let grayOnTop = try assertStyledDrawingMatchesFreshStorage(in: input, context: "raw mutable gray-last stack")
        mutableStack.exchangeObject(at: 0, withObjectAt: 1)
        XCTAssertTrue(input.textStorage.editedMask.isEmpty, "In-place raw-array mutation sends no storage edit")
        let redOnTop = try assertStyledDrawingMatchesFreshStorage(in: input, context: "raw mutable red-last stack")
        XCTAssertEqual(redOnTop.usedRect, grayOnTop.usedRect)
        XCTAssertEqual(redOnTop.lineRects, grayOnTop.lineRects)
        XCTAssertEqual(redOnTop.caretRect, grayOnTop.caretRect)
        XCTAssertNotEqual(redOnTop.bitmap, grayOnTop.bitmap, "Legacy paint order must actually follow the changed raw array")
        mutableStack.removeObject(at: 1)
        _ = try assertStyledDrawingMatchesFreshStorage(in: input, context: "raw mutable stack removes a descriptor")
    }

    func testStyledDrawingRebindsTheSameLayoutManagerToAnotherStorage() throws {
        let viewport = CGSize(width: 320, height: 240)
        let reference = EditorTextView(frame: CGRect(origin: .zero, size: viewport))
        reference.textContainerInset = .zero
        let red = EditorRenderedBox(box: EditorStyleBox(["backgroundColor": "#ff0000ff"]),
            depth: 0, leading: 0, trailing: 0)
        let gray = EditorRenderedBox(box: EditorStyleBox(["backgroundColor": "#eeeeeeff"]),
            depth: 0, leading: 0, trailing: 0)
        let makeText = { (label: String, box: EditorRenderedBox) in
            NSAttributedString(string: label, attributes: [.font: reference.baseFont,
                .foregroundColor: UIColor.blue, editorStyleBoxesAttribute: EditorRenderedBoxStack(boxes: [box])])
        }
        let first = NSTextStorage(attributedString: makeText("First café", red))
        let second = NSTextStorage(attributedString: makeText("Second 東京 with another UTF16 range", gray))
        let manager = EditorLayoutManager()
        let container = NSTextContainer(size: viewport)
        manager.addTextContainer(container)
        first.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        let firstPaint = try renderGlyphBitmap(manager,
            glyphRange: NSRange(location: 0, length: manager.numberOfGlyphs), origin: .zero, size: viewport)
        first.removeLayoutManager(manager)
        second.addLayoutManager(manager)
        XCTAssertTrue(manager.textStorage === second)
        manager.ensureLayout(for: container)
        let actual = try renderGlyphBitmap(manager,
            glyphRange: NSRange(location: 0, length: manager.numberOfGlyphs), origin: .zero, size: viewport)
        reference.textStorage.setAttributedString(second)
        reference.layoutManager.ensureLayout(for: reference.textContainer)
        let expected = try renderGlyphBitmap(reference.layoutManager,
            glyphRange: NSRange(location: 0, length: reference.layoutManager.numberOfGlyphs), origin: .zero, size: viewport)
        XCTAssertEqual(actual, expected, "Same-manager storage rebind must paint fresh current ranges and identities")
        XCTAssertEqual(manager.usedRect(for: container), reference.layoutManager.usedRect(for: reference.textContainer))
        XCTAssertNotEqual(actual, firstPaint, "The rebound content and background must actually differ")
    }

    private func siblingDecorationOverlapPixels(
        in input: EditorTextView, blocks: [[NSDictionary]], geometry: StyledMarginGeometry, action: String
    ) throws -> [Int] {
        let borderTransparent = try siblingMarginTheme(styleOverrides: [
            "blockquote": ["borderLeftColor": "#00000000"]
        ])
        let backgroundTransparent = try siblingMarginTheme(styleOverrides: [
            "codeBlock": ["backgroundColor": "#00000000"]
        ])
        var controls: [StyledMarginGeometry] = []
        for theme in [borderTransparent, backgroundTransparent] {
            let reference = freshSiblingMarginReference(for: input, blocks: blocks, theme: theme)
            XCTAssertEqual(reference.textStorage.string as NSString, input.textStorage.string as NSString, action)
            let control = try styledMarginGeometry(in: reference)
            XCTAssertEqual(control.usedRect, geometry.usedRect, "\(action) changing paint color must retain the full layout")
            XCTAssertEqual(control.lineRects, geometry.lineRects, "\(action) color masks require identical glyph lines")
            XCTAssertEqual(control.caretRect, geometry.caretRect, "\(action) color masks must retain the exact caret")
            XCTAssertEqual(control.bitmap.count, geometry.bitmap.count, action)
            controls.append(control)
        }
        guard controls.allSatisfy({ $0.bitmap.count == geometry.bitmap.count }) else { return [] }
        let overlap = paintOverlapPixels(controls[0].bitmap, controls[1].bitmap)
        XCTAssertFalse(overlap.isEmpty,
            "\(action) independent color masks must prove actual quote-border/code-background overlap")
        return overlap
    }

    private func paintOverlapPixels(_ grayControl: Data, _ redControl: Data) -> [Int] {
        XCTAssertEqual(grayControl.count, redControl.count, "Paint masks must share the exact native canvas")
        guard grayControl.count == redControl.count else { return [] }
        let grayPixel: [UInt8] = [238, 238, 238, 255]
        let redPixel: [UInt8] = [255, 0, 0, 255]
        let bytesPerPixel = grayPixel.count
        return stride(from: 0, to: grayControl.count, by: bytesPerPixel).filter { offset in
            Array(grayControl[offset..<(offset + bytesPerPixel)]) == grayPixel
                && Array(redControl[offset..<(offset + bytesPerPixel)]) == redPixel
        }
    }

    func testLegacyStyledBoxPaintKeepsEqualRangeStackOrderAndNestedDepth() throws {
        let redPixel: [UInt8] = [255, 0, 0, 255]
        let grayPixel: [UInt8] = [238, 238, 238, 255]
        let freshRenderCount = 8
        for nested in [false, true] {
            func render(firstHidden: Bool = false, secondHidden: Bool = false) throws -> StyledMarginGeometry {
                let input = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
                let firstColor = firstHidden ? "#00000000" : "#ff0000ff"
                let firstValues: [String: Any] = nested
                    ? ["borderLeftWidth": 3, "borderLeftColor": firstColor]
                    : ["backgroundColor": firstColor]
                let first = EditorRenderedBox(box: EditorStyleBox(firstValues),
                    depth: nested ? 1 : 0, leading: 0, trailing: 0)
                let second = EditorRenderedBox(box: EditorStyleBox([
                    "backgroundColor": secondHidden ? "#00000000" : "#eeeeeeff"
                ]), depth: 0, leading: 0, trailing: 0)
                input.textStorage.setAttributedString(NSAttributedString(string: "Legacy café 東京", attributes: [
                    .font: input.baseFont, .foregroundColor: UIColor.blue,
                    editorStyleBoxesAttribute: [first, second]
                ]))
                return try styledMarginGeometry(in: input)
            }
            let grayControl = try render(firstHidden: true)
            let redControl = try render(secondHidden: true)
            XCTAssertEqual(grayControl.usedRect, redControl.usedRect)
            XCTAssertEqual(grayControl.lineRects, redControl.lineRects)
            XCTAssertEqual(grayControl.caretRect, redControl.caretRect)
            let overlap = paintOverlapPixels(grayControl.bitmap, redControl.bitmap)
            XCTAssertFalse(overlap.isEmpty, "The legacy controls must prove actual equal-range paint overlap")
            let expected = nested ? redPixel : grayPixel
            for iteration in 0..<freshRenderCount {
                let actual = try render()
                XCTAssertEqual(actual.usedRect, grayControl.usedRect)
                XCTAssertEqual(actual.lineRects, grayControl.lineRects)
                XCTAssertEqual(actual.caretRect, grayControl.caretRect)
                XCTAssertEqual(actual.bitmap.count, grayControl.bitmap.count)
                guard actual.bitmap.count == grayControl.bitmap.count else { continue }
                let wrong = overlap.first { Array(actual.bitmap[$0..<($0 + expected.count)]) != expected }
                XCTAssertNil(wrong.map { "RGBA byte \($0): \(Array(actual.bitmap[$0..<($0 + expected.count)]))" },
                    "nested=\(nested) fresh=\(iteration): depth must win first; equal-depth/equal-range boxes must retain legacy stack order")
            }
        }
    }

    func testVersionedSiblingMarginCollapseDoesNotRewriteCurrentSeparators() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let (view, window) = try hostSiblingMarginEditor(editorId: editorId)
        let input = view.textView
        defer {
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        let caret = (input.textStorage.string as NSString).range(of: "Nested").location + "Nest".utf16.count
        setCollapsedSelection(in: input, utf16Offset: caret)
        let selection = input.selectedRange
        let beforeGeometry = try styledMarginGeometry(in: input)
        let before = NSAttributedString(attributedString: input.textStorage)
        let beforeAttributes = (0..<before.length).map { comparableStyledAttributes(in: before, at: $0) }
        var edits: [NSRange] = []
        let observer = NotificationCenter.default.addObserver(
            forName: NSTextStorage.didProcessEditingNotification, object: input.textStorage, queue: nil
        ) { notification in
            guard let storage = notification.object as? NSTextStorage else { return }
            XCTAssertEqual(storage.editedMask, .editedAttributes, "Margin collapse must never edit document characters")
            edits.append(storage.editedRange)
        }
        defer { NotificationCenter.default.removeObserver(observer) }
        input.textStorage.beginEditing()
        RenderBridge.collapseStyledSiblingMargins(in: input.textStorage)
        input.textStorage.endEditing()
        let idempotentEdits = edits
        XCTAssertTrue(input.textStorage.isEqual(to: before), "Repeated collapse must preserve every attributed value")
        for offset in 0..<before.length {
            XCTAssertTrue(NSDictionary(dictionary: comparableStyledAttributes(in: input.textStorage, at: offset))
                .isEqual(to: beforeAttributes[offset]), "Descriptor geometry changed at UTF16 \(offset)")
        }
        XCTAssertEqual(input.selectedRange, selection)
        let afterGeometry = try styledMarginGeometry(in: input)
        XCTAssertEqual(afterGeometry.usedRect, beforeGeometry.usedRect)
        XCTAssertEqual(afterGeometry.lineRects, beforeGeometry.lineRects)
        XCTAssertEqual(afterGeometry.caretRect, beforeGeometry.caretRect)
        XCTAssertEqual(afterGeometry.bitmap, beforeGeometry.bitmap, "Repeated collapse must retain text, quote border and code background pixels")

        let separatorOffset = (before.string as NSString).range(of: "Quote tail").location - 1
        XCTAssertNotNil(before.attribute(RenderBridgeAttributes.blockBoundary, at: separatorOffset, effectiveRange: nil))
        let separator = NSRange(location: separatorOffset, length: 1)
        let staleStyle = try XCTUnwrap((before.attribute(.paragraphStyle, at: separatorOffset,
            effectiveRange: nil) as? NSParagraphStyle)?.mutableCopy() as? NSMutableParagraphStyle)
        staleStyle.alignment = .right
        let staleSpacingAdjustment: CGFloat = 37
        staleStyle.paragraphSpacingBefore += staleSpacingAdjustment
        edits.removeAll()
        do {
            input.textStorage.beginEditing()
            defer { input.textStorage.endEditing() }
            input.textStorage.addAttribute(.paragraphStyle, value: staleStyle, range: separator)
            let currentStale = try XCTUnwrap(input.textStorage.attribute(.paragraphStyle,
                at: separatorOffset, effectiveRange: nil) as? NSParagraphStyle)
            XCTAssertEqual(currentStale.alignment, .right, "The open batch must expose the genuinely stale target")
            XCTAssertEqual(currentStale.paragraphSpacingBefore, staleStyle.paragraphSpacingBefore)
            RenderBridge.collapseStyledSiblingMargins(in: input.textStorage)
            XCTAssertTrue((input.textStorage.attribute(.paragraphStyle, at: separatorOffset,
                effectiveRange: nil) as? NSObject)?.isEqual(before.attribute(.paragraphStyle,
                    at: separatorOffset, effectiveRange: nil)) == true,
                "The helper must repair all target fields before NSTextStorage can auto-fix the batch")
        }
        XCTAssertFalse(edits.isEmpty, "A genuinely stale separator must still produce a real attribute edit")
        XCTAssertTrue(input.textStorage.isEqual(to: before), "Repair must restore the full preceding paragraph style, not only its spacing")
        XCTAssertEqual(input.selectedRange, selection)
        let repairedGeometry = try styledMarginGeometry(in: input)
        XCTAssertEqual(repairedGeometry.usedRect, beforeGeometry.usedRect)
        XCTAssertEqual(repairedGeometry.lineRects, beforeGeometry.lineRects)
        XCTAssertEqual(repairedGeometry.caretRect, beforeGeometry.caretRect)
        XCTAssertEqual(repairedGeometry.bitmap, beforeGeometry.bitmap)
        edits.removeAll()
        input.textStorage.beginEditing()
        input.textStorage.removeAttribute(.paragraphStyle, range: separator)
        XCTAssertNil(input.textStorage.attribute(.paragraphStyle, at: separatorOffset, effectiveRange: nil),
            "The open batch must expose the genuinely missing target style")
        RenderBridge.collapseStyledSiblingMargins(in: input.textStorage)
        XCTAssertTrue((input.textStorage.attribute(.paragraphStyle, at: separatorOffset,
            effectiveRange: nil) as? NSObject)?.isEqual(before.attribute(.paragraphStyle,
                at: separatorOffset, effectiveRange: nil)) == true,
            "The helper must supply the missing style before NSTextStorage's automatic fixing")
        input.textStorage.endEditing()
        XCTAssertFalse(edits.isEmpty, "A missing separator style must still be repaired")
        XCTAssertTrue(input.textStorage.isEqual(to: before), "Missing targets must receive the full preceding paragraph style")
        XCTAssertTrue(idempotentEdits.isEmpty,
            "Already collapsed live separators must not invalidate TextKit across the document: \(idempotentEdits)")
    }

    func testVersionedMarginCollapseMatchesFreshRenderAfterRichEdits() throws {
        try assertSiblingMarginEditsMatchFreshRendering()
    }

    func testOverlappingSiblingDecorationsPaintInDocumentOrderAfterRichEdits() throws {
        try assertSiblingMarginEditsMatchFreshRendering(html: Self.overlappingSiblingMarginHTML,
            verifyDecorationOverlap: true)
    }

    private func assertSiblingMarginEditsMatchFreshRendering(
        html: String? = nil, verifyDecorationOverlap: Bool = false
    ) throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let (view, window) = try hostSiblingMarginEditor(editorId: editorId, html: html)
        let input = view.textView
        defer {
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        let quoteOffset = (input.textStorage.string as NSString).range(of: "Quote café").location + "Quote ".utf16.count
        setCollapsedSelection(in: input, utf16Offset: quoteOffset)
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        for action in ["insert", "delete", "split", "merge"] {
            let beforeText = input.textStorage.string as NSString
            let beforeCaret = input.selectedRange
            XCTAssertEqual(beforeCaret.length, 0, "\(action) requires an actual collapsed native caret")
            let beforeRevision = adapter.baseDocumentRevision
            let beforeStateRevision = adapter.stateRevision
            var expectedQuoteTypes = try siblingQuoteChildTypes(editorId: editorId)
            let insertsText = action == "insert" || action == "split"
            let replacement = action == "insert" ? "!" : action == "split" ? "\n" : ""
            let editRange = insertsText ? beforeCaret : NSRange(location: beforeCaret.location - 1, length: 1)
            if !insertsText {
                XCTAssertEqual(beforeText.substring(with: editRange), action == "delete" ? "!" : "\n")
            }
            let expectedText = beforeText.replacingCharacters(in: editRange, with: replacement)
            let expectedCaret = editRange.location + replacement.utf16.count
            switch action {
            case "insert": input.insertText("!")
            case "delete", "merge": input.deleteBackward()
            case "split": input.insertText("\n")
            default: XCTFail("Unknown rich edit: \(action)")
            }
            XCTAssertEqual(input.textStorage.string, expectedText, "\(action) must actually mutate accepted text")
            XCTAssertEqual(input.selectedRange, NSRange(location: expectedCaret, length: 0), action)
            XCTAssertGreaterThan(adapter.baseDocumentRevision, beforeRevision, "\(action) must commit a document revision")
            XCTAssertGreaterThan(adapter.stateRevision, beforeStateRevision, "\(action) must commit authoritative state")
            let splitParagraphIndex = 1
            if action == "split" { expectedQuoteTypes.insert("paragraph", at: splitParagraphIndex) }
            if action == "merge" { expectedQuoteTypes.remove(at: splitParagraphIndex) }
            XCTAssertEqual(try siblingQuoteChildTypes(editorId: editorId), expectedQuoteTypes,
                "\(action) must change actual paragraph structure without flattening the nested quote")
            XCTAssertTrue(input.lastRenderAppliedPatch(), "\(action) must exercise the real styled render patch")
            let blocks = try XCTUnwrap(input.currentRenderBlocks, action)
            let theme = try XCTUnwrap(input.theme)
            let reference = freshSiblingMarginReference(for: input, blocks: blocks, theme: theme)
            let actualGeometry = try styledMarginGeometry(in: input)
            let freshGeometry = try styledMarginGeometry(in: reference)
            let repeatedActual = try styledMarginGeometry(in: input)
            let repeatedFresh = try styledMarginGeometry(in: reference)
            let secondReference = freshSiblingMarginReference(for: input, blocks: blocks, theme: theme)
            let independentFresh = try styledMarginGeometry(in: secondReference)
            if verifyDecorationOverlap {
                let overlap = try siblingDecorationOverlapPixels(in: input, blocks: blocks,
                    geometry: actualGeometry, action: action)
                let codeBackgroundPixel: [UInt8] = [238, 238, 238, 255]
                let bytesPerPixel = codeBackgroundPixel.count
                for (name, geometry) in [("actual", actualGeometry), ("actual-repeat", repeatedActual),
                    ("fresh", freshGeometry), ("fresh-repeat", repeatedFresh), ("fresh-independent", independentFresh)] {
                    let wrongPixel = overlap.first { offset in
                        Array(geometry.bitmap[offset..<(offset + bytesPerPixel)]) != codeBackgroundPixel
                    }
                    XCTAssertNil(wrongPixel.map { offset in
                        "RGBA byte \(offset): \(Array(geometry.bitmap[offset..<(offset + bytesPerPixel)]))"
                    }, "\(action) \(name): the later code background must cover the earlier sibling quote border")
                }
            }
            if actualGeometry.bitmap != freshGeometry.bitmap
                || actualGeometry.bitmap != repeatedActual.bitmap
                || freshGeometry.bitmap != repeatedFresh.bitmap
                || freshGeometry.bitmap != independentFresh.bitmap {
                for (name, geometry) in [("actual", actualGeometry), ("actual-repeat", repeatedActual),
                    ("fresh", freshGeometry), ("fresh-repeat", repeatedFresh), ("fresh-independent", independentFresh)] {
                    let attachment = XCTAttachment(data: geometry.bitmap, uniformTypeIdentifier: "public.data")
                    attachment.name = "\(action)-\(name)-\(Int(input.bounds.width))x\(geometry.bitmap.count / (Int(input.bounds.width) * 4)).rgba"
                    attachment.lifetime = .keepAlways
                    add(attachment)
                }
            }
            XCTAssertEqual(actualGeometry.bitmap, repeatedActual.bitmap, "\(action) repeated live draw must be deterministic")
            XCTAssertEqual(freshGeometry.bitmap, repeatedFresh.bitmap, "\(action) repeated fresh draw must be deterministic")
            XCTAssertEqual(freshGeometry.bitmap, independentFresh.bitmap, "\(action) independent fresh render must paint the same pixels")
            XCTAssertEqual(input.textStorage.string, reference.textStorage.string, action)
            XCTAssertEqual(input.textStorage.length, reference.textStorage.length, action)
            guard input.textStorage.length == reference.textStorage.length else { continue }
            for offset in 0..<input.textStorage.length {
                let actual = comparableStyledAttributes(in: input.textStorage, at: offset)
                let expected = comparableStyledAttributes(in: reference.textStorage, at: offset)
                XCTAssertTrue(NSDictionary(dictionary: actual).isEqual(to: expected),
                    "\(action) UTF16 \(offset): actual=\(actual) fresh=\(expected)")
            }
            XCTAssertEqual(actualGeometry.usedRect, freshGeometry.usedRect, action)
            XCTAssertEqual(actualGeometry.lineRects, freshGeometry.lineRects, action)
            XCTAssertEqual(actualGeometry.caretRect, freshGeometry.caretRect, action)
            XCTAssertEqual(actualGeometry.bitmap, freshGeometry.bitmap,
                "\(action) must retain fresh-render rich glyph, nested quote border and code background pixels")
        }
    }

    private func assertRetainedSnapshotMetadata(
        _ snapshot: NSAttributedString,
        matches rendered: NSAttributedString,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        let metadataKeys: Set<NSAttributedString.Key> = [
            RenderBridgeAttributes.docPos, RenderBridgeAttributes.topLevelChildIndex,
            RenderBridgeAttributes.voidNodeType, RenderBridgeAttributes.blockBoundary
        ]
        XCTAssertEqual(snapshot.string, rendered.string, file: file, line: line)
        guard snapshot.length == rendered.length else { return }
        let firstMismatch = (0..<rendered.length).first { offset in
            let snapshotMetadata = snapshot.attributes(at: offset, effectiveRange: nil)
                .filter { metadataKeys.contains($0.key) }
            let renderedMetadata = rendered.attributes(at: offset, effectiveRange: nil)
                .filter { metadataKeys.contains($0.key) }
            return !NSDictionary(dictionary: snapshotMetadata).isEqual(to: renderedMetadata)
        }
        XCTAssertNil(firstMismatch.map { offset in
            "offset=\(offset) snapshot=\(snapshot.attributes(at: offset, effectiveRange: nil)) rendered=\(rendered.attributes(at: offset, effectiveRange: nil))"
        }, "The snapshot must retain current positional metadata", file: file, line: line)
    }

    private func assertTextKitAttributedStorage(
        in textView: UITextView,
        matches expected: NSAttributedString,
        compareStyledValues: Bool = false,
        file: StaticString = #filePath,
        line: UInt = #line
    ) {
        // Emoji font fallback happens during glyph layout, not fixAttributes alone.
        let normalizedActual = textKitLaidOutStorage(textView.textStorage, in: textView)
        let normalizedExpected = textKitLaidOutStorage(expected, in: textView)
        XCTAssertEqual(normalizedActual.string, normalizedExpected.string, file: file, line: line)
        guard normalizedActual.length == normalizedExpected.length else { return }
        let mismatch = (0..<normalizedActual.length).first { offset in
            let actual = compareStyledValues
                ? comparableStyledAttributes(in: normalizedActual, at: offset)
                : normalizedActual.attributes(at: offset, effectiveRange: nil)
            let expected = compareStyledValues
                ? comparableStyledAttributes(in: normalizedExpected, at: offset)
                : normalizedExpected.attributes(at: offset, effectiveRange: nil)
            return !NSDictionary(dictionary: actual).isEqual(to: expected)
        }
        XCTAssertNil(mismatch.map { offset in
            "offset=\(offset) live=\(normalizedActual.attributes(at: offset, effectiveRange: nil)) expected=\(normalizedExpected.attributes(at: offset, effectiveRange: nil))"
        }, "Every attributed value must match after the same TextKit font fixing", file: file, line: line)
    }

    private func textKitLaidOutStorage(_ source: NSAttributedString, in textView: UITextView) -> NSAttributedString {
        let manager = NSLayoutManager()
        let container = NSTextContainer(size: textView.textContainer.size)
        manager.addTextContainer(container)
        let storage = NSTextStorage()
        storage.addLayoutManager(manager)
        let oracle = UITextView(frame: textView.bounds, textContainer: container)
        oracle.textContainerInset = textView.textContainerInset
        container.lineFragmentPadding = textView.textContainer.lineFragmentPadding
        storage.setAttributedString(source)
        manager.ensureLayout(for: container)
        return NSAttributedString(attributedString: storage)
    }

    private func retainedAtomRenderBlocks(prefix: [String], positionShift: Int = 0) -> [[[String: Any]]] {
        let initialMentionDocumentPosition = 14
        let initialSecondMentionDocumentPosition = 15
        let initialCardDocumentPosition = 17
        let initialRuleDocumentPosition = 18
        func paragraph(_ text: String) -> [[String: Any]] {
            [
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                ["type": "textRun", "text": text, "marks": []],
                ["type": "blockEnd"]
            ]
        }
        return prefix.map(paragraph) + [
            [
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                ["type": "textRun", "text": "Hello ", "marks": []],
                ["type": "opaqueInlineAtom", "nodeType": "mention", "label": Self.retainedMentionLabels[0], "docPos": initialMentionDocumentPosition + positionShift],
                ["type": "opaqueInlineAtom", "nodeType": "mention", "label": Self.retainedMentionLabels[1], "docPos": initialSecondMentionDocumentPosition + positionShift],
                ["type": "blockEnd"]
            ],
            [["type": "opaqueBlockAtom", "nodeType": "card", "label": "Card", "docPos": initialCardDocumentPosition + positionShift]],
            [["type": "voidBlock", "nodeType": "horizontal_rule", "docPos": initialRuleDocumentPosition + positionShift]],
            paragraph("Tail")
        ]
    }

    func testFocusedAuthoritativeTextPatchAdvancesCaretBeforeSelectionReapply() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        defer {
            view.textView.onApplyingRustTextForTesting = nil
            view.textView.captureApplyUpdateTraceForTesting = false
            _ = view.textView.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        let initialHTML = #"<p><strong>A😀B</strong><em>C</em></p><p><a href="https://example.com">Tail</a></p>"#
        let prefix = "A😀"
        view.editorId = editorId
        view.setContent(html: initialHTML)
        let initialStorage = NSAttributedString(attributedString: view.textView.textStorage)
        XCTAssertTrue(view.textView.becomeFirstResponder())
        setCollapsedSelection(in: view.textView, utf16Offset: prefix.utf16.count)
        flushMainQueue()
        view.textView.captureApplyUpdateTraceForTesting = true
        let previousStageObserver = PreparedProseInstrumentation.tableStageObserverForTesting
        var collaborationPublicationCount = 0
        var authorizedTextCanonicalComparisonCount = 0
        PreparedProseInstrumentation.tableStageObserverForTesting = { stage, start, end in
            if stage == .collaborationSelectionPublication { collaborationPublicationCount += 1 }
            if stage == .authorizedTextCanonicalComparison { authorizedTextCanonicalComparisonCount += 1 }
            previousStageObserver?(stage, start, end)
        }
        defer { PreparedProseInstrumentation.tableStageObserverForTesting = previousStageObserver }
        XCTAssertFalse(try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId)).roomBound)

        func assertExplicitPatchDoesNotDeriveAgain(line: UInt = #line) throws {
            let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
            let updateJSON = try XCTUnwrap(adapter.cachedViewUpdate?.updateJSON)
            let update = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(updateJSON.utf8)) as? [String: Any])
            XCTAssertNotNil(view.textView.parseRenderPatch(update["renderPatch"]),
                "The real native frame must supply the authoritative patch", line: line)
            let trace = try XCTUnwrap(view.textView.lastApplyUpdateTrace())
            XCTAssertTrue(trace.usedPatch, line: line)
            XCTAssertEqual(trace.deriveRenderPatchNanos, 0,
                "An accepted explicit native patch must not trigger a second whole-document diff", line: line)
            XCTAssertEqual(collaborationPublicationCount, 0,
                "Local native typing must not resolve and publish collaboration selections without a room", line: line)
            XCTAssertEqual(authorizedTextCanonicalComparisonCount, 0,
                "Exact UTF16-authorized storage must not scan the complete Unicode string for canonical equivalence during typing", line: line)
        }

        for inserted in ["!", "👩‍💻"] {
            var appliedCarets: [NSRange] = []
            view.textView.onApplyingRustTextForTesting = {
                appliedCarets.append(view.textView.selectedRange)
            }
            view.textView.insertText(inserted)
            try assertExplicitPatchDoesNotDeriveAgain()
            let insertedCaret = NSRange(location: (prefix + inserted).utf16.count, length: 0)
            XCTAssertEqual(appliedCarets, [insertedCaret], "\(inserted): accepted UIKit text replacement must advance the caret before Rust selection is applied")
            XCTAssertEqual(view.textView.selectedRange, insertedCaret)
            XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), #"<p><strong>A😀\#(inserted)B</strong><em>C</em></p><p><a href="https://example.com">Tail</a></p>"#)
            assertRetainedSnapshotMetadata(view.textView.lastAuthorizedAttributedTextStorage, matches: view.textView.textStorage)
            assertTextKitAttributedStorage(in: view.textView, matches: view.textView.lastAuthorizedAttributedTextStorage)
            flushMainQueue()

            appliedCarets.removeAll()
            view.textView.deleteBackward()
            try assertExplicitPatchDoesNotDeriveAgain()
            let restoredCaret = NSRange(location: prefix.utf16.count, length: 0)
            XCTAssertEqual(appliedCarets, [restoredCaret], "\(inserted): accepted grapheme deletion must move the caret before Rust selection is applied")
            XCTAssertEqual(view.textView.selectedRange, restoredCaret)
            XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHTML)
            assertTextKitAttributedStorage(in: view.textView, matches: initialStorage)
            assertTextKitAttributedStorage(in: view.textView, matches: view.textView.lastAuthorizedAttributedTextStorage)
            flushMainQueue()
        }
        XCTAssertEqual(view.textView.reconciliationCount, 0)
    }

    func testAcceptedPlainTypingDoesNotInvalidateUnchangedAttributesTwice() throws {
        try assertAcceptedPlainTypingKeepsInheritedAttributes()
    }

    func testAcceptedPublicStyledTypingDoesNotInvalidateUnchangedAttributesTwice() throws {
        let fixtures = try EditorPublicThemeFixtures.load()
        let theme = try XCTUnwrap(EditorTheme.from(json: fixtures.publicThemeJSON))
        XCTAssertNotNil(theme.styleSheet, "The regression must use the exact public stylesheet")
        try assertAcceptedPlainTypingKeepsInheritedAttributes(theme: theme)
    }

    func testPublicStyledTypingRetainsDistinctSiblingsAndAppliesRealStyleChanges() throws {
        let fixtures = try EditorPublicThemeFixtures.load()
        let theme = try XCTUnwrap(EditorTheme.from(json: fixtures.publicThemeJSON))
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        let input = view.textView
        defer {
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        XCTAssertTrue(view.applyTheme(theme))
        view.editorId = editorId
        let initialHTML = Self.publicStyledTypingHTML
        view.setContent(html: initialHTML)
        XCTAssertTrue(input.becomeFirstResponder())
        let prefix = Self.plainTypingPrefix
        let initialParagraphCount = 2
        setCollapsedSelection(in: input, utf16Offset: prefix.utf16.count)
        flushMainQueue()
        try assertStyledPatchMatchesFreshRender(in: input, context: "initial")

        input.insertText("\n")
        let splitHTML = #"<p><strong>Before </strong></p><p><strong>😀 after</strong><em> italic</em></p><p><a href="https://example.com">Tail</a></p>"#
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), splitHTML,
            "Actual Return must create a new paragraph rather than share the old container")
        XCTAssertEqual(input.selectedRange, NSRange(location: (prefix + "\n").utf16.count, length: 0))
        XCTAssertEqual(try styledParagraphBoxes(in: input).count, initialParagraphCount + 1)
        try assertStyledPatchMatchesFreshRender(in: input, context: "split")
        input.deleteBackward()
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHTML)
        XCTAssertEqual(input.selectedRange, NSRange(location: prefix.utf16.count, length: 0))
        XCTAssertEqual(try styledParagraphBoxes(in: input).count, initialParagraphCount)
        try assertStyledPatchMatchesFreshRender(in: input, context: "merge")

        let markedText = "😀 after"
        let markedRange = (input.textStorage.string as NSString).range(of: markedText)
        setSelection(in: input, utf16Range: markedRange)
        input.performToolbarToggleMark("bold")
        let unmarkedHTML = #"<p><strong>Before </strong>😀 after<em> italic</em></p><p><a href="https://example.com">Tail</a></p>"#
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), unmarkedHTML)
        let unmarkedFont = try XCTUnwrap(input.textStorage.attribute(.font,
            at: (input.textStorage.string as NSString).range(of: "after").location,
            effectiveRange: nil) as? UIFont)
        XCTAssertFalse(unmarkedFont.fontDescriptor.symbolicTraits.contains(.traitBold),
            "A genuine mark change must not retain the previous run's font")
        try assertStyledPatchMatchesFreshRender(in: input, context: "mark-change")

        var changedRoot = try XCTUnwrap(JSONSerialization.jsonObject(with:
            Data(fixtures.publicThemeJSON.utf8)) as? [String: Any])
        var changedStyles = try XCTUnwrap(changedRoot["styles"] as? [String: [String: Any]])
        var paragraph = try XCTUnwrap(changedStyles["paragraph"])
        var textStyle = try XCTUnwrap(changedStyles["text"])
        let changedMargin: CGFloat = 9
        let changedBottomMargin: CGFloat = 11
        let changedBorderWidth: CGFloat = 3
        let changedBorderColor = "#ff0000ff"
        let changedFontSize: CGFloat = 21
        paragraph["marginTop"] = changedMargin
        paragraph["marginBottom"] = changedBottomMargin
        paragraph["borderLeftWidth"] = changedBorderWidth
        paragraph["borderLeftColor"] = changedBorderColor
        changedStyles["paragraph"] = paragraph
        textStyle["fontSize"] = changedFontSize
        changedStyles["text"] = textStyle
        changedRoot["styles"] = changedStyles
        let beforeGeometry = try styledMarginGeometry(in: input)
        let beforeSelection = input.selectedRange
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: changedRoot)))
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), unmarkedHTML,
            "A theme mutation must preserve authoritative content")
        XCTAssertEqual(input.selectedRange, beforeSelection)
        let changedFont = try XCTUnwrap(input.textStorage.attribute(.font,
            at: (input.textStorage.string as NSString).range(of: "after").location,
            effectiveRange: nil) as? UIFont)
        XCTAssertEqual(changedFont.pointSize, changedFontSize,
            "An actual theme font change must not retain the old nominal or fallback font")
        let changedBoxes = try styledParagraphBoxes(in: input)
        for box in changedBoxes {
            XCTAssertEqual(box.box.margin.top, changedMargin)
            XCTAssertEqual(box.box.borders.left, changedBorderWidth)
            XCTAssertEqual(box.box.color("borderLeftColor"), EditorTheme.color(from: changedBorderColor))
        }
        let afterGeometry = try styledMarginGeometry(in: input)
        XCTAssertNotEqual(afterGeometry.bitmap, beforeGeometry.bitmap,
            "The border control must change actual painted pixels")
        XCTAssertNotEqual(afterGeometry.usedRect, beforeGeometry.usedRect,
            "The margin control must change actual layout geometry")
        try assertStyledPatchMatchesFreshRender(in: input, context: "theme-change")

        let linkedPrefix = "Ta"
        let linkedRange = (input.textStorage.string as NSString).range(of: "Tail")
        XCTAssertNotEqual(linkedRange.location, NSNotFound)
        let linkedCaret = linkedRange.location + linkedPrefix.utf16.count
        setCollapsedSelection(in: input, utf16Offset: linkedCaret)
        flushMainQueue()
        var appliedCarets: [NSRange] = []
        input.onApplyingRustTextForTesting = { appliedCarets.append(input.selectedRange) }
        defer { input.onApplyingRustTextForTesting = nil }
        let linkedInserted = "!"
        input.insertText(linkedInserted)
        let linkedInsertedCaret = NSRange(location: linkedCaret + linkedInserted.utf16.count, length: 0)
        XCTAssertEqual(appliedCarets, [linkedInsertedCaret], "Non-first paragraph insertion must retain UIKit caret adoption")
        XCTAssertEqual(input.selectedRange, linkedInsertedCaret)
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId),
            unmarkedHTML.replacingOccurrences(of: "Tail", with: "Ta!il"))
        try assertStyledPatchMatchesFreshRender(in: input, context: "non-first-margin-insert")
        appliedCarets.removeAll()
        input.deleteBackward()
        let restoredLinkedCaret = NSRange(location: linkedCaret, length: 0)
        XCTAssertEqual(appliedCarets, [restoredLinkedCaret], "Non-first paragraph deletion must retain UIKit caret adoption")
        XCTAssertEqual(input.selectedRange, restoredLinkedCaret)
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), unmarkedHTML)
        try assertStyledPatchMatchesFreshRender(in: input, context: "non-first-margin-delete")
        input.onApplyingRustTextForTesting = nil

        let authorized = NSAttributedString(attributedString: input.lastAuthorizedAttributedTextStorage)
        let authorizedGeometry = try styledMarginGeometry(in: input)
        let rollbackSuffix = " transient"
        input.performTransientTextMutation {
            input.textStorage.replaceCharacters(in: NSRange(location: input.textStorage.length, length: 0),
                with: rollbackSuffix)
        }
        XCTAssertEqual(input.textStorage.string, authorized.string + rollbackSuffix,
            "The rollback control must actually change mounted storage")
        input.restoreAuthorizedTextSnapshot()
        XCTAssertEqual(input.selectedRange, restoredLinkedCaret)
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), unmarkedHTML,
            "Transient storage and rollback must not mutate the authoritative document")
        assertTextKitAttributedStorage(in: input, matches: authorized, compareStyledValues: true)
        XCTAssertEqual(try styledMarginGeometry(in: input), authorizedGeometry,
            "Rollback must retain current sibling margins, glyphs, decorations and caret")
        try assertStyledPatchMatchesFreshRender(in: input, context: "non-first-margin-rollback")

        for originalFontControl in OriginalFontControl.allCases {
            XCTAssertTrue(view.applyTheme(theme))
            view.setContent(html: initialHTML)
            setCollapsedSelection(in: input, utf16Offset: prefix.utf16.count)
            flushMainQueue()
            input.layoutManager.ensureLayout(for: input.textContainer)
            let emojiRange = (input.textStorage.string as NSString).range(of: "😀")
            XCTAssertNotEqual(emojiRange.location, NSNotFound)
            let originalFont = try XCTUnwrap(input.textStorage.attribute(Self.textKitOriginalFontAttribute,
                at: emojiRange.location, effectiveRange: nil) as? UIFont,
                "The negative control must begin with actual UIKit font fallback metadata")
            let fallbackFont = try XCTUnwrap(input.textStorage.attribute(.font,
                at: emojiRange.location, effectiveRange: nil) as? UIFont)
            XCTAssertFalse(fallbackFont.isEqual(originalFont))
            let inconsistentFontSizeAdjustment: CGFloat = 5
            let inconsistentFont = UIFont.italicSystemFont(ofSize: originalFont.pointSize + inconsistentFontSizeAdjustment)
            input.performTransientTextMutation {
                switch originalFontControl {
                case .absent:
                    input.textStorage.removeAttribute(Self.textKitOriginalFontAttribute, range: emojiRange)
                case .differentFont:
                    input.textStorage.addAttribute(Self.textKitOriginalFontAttribute,
                        value: inconsistentFont, range: emojiRange)
                }
            }
            let actualOriginal = input.textStorage.attribute(Self.textKitOriginalFontAttribute,
                at: emojiRange.location, effectiveRange: nil)
            switch originalFontControl {
            case .absent:
                XCTAssertNil(actualOriginal, "The absent-marker control must be real before native input")
            case .differentFont:
                let actualFont = try XCTUnwrap(actualOriginal as? UIFont)
                XCTAssertTrue(actualFont.isEqual(inconsistentFont),
                    "The inconsistent-marker control must be real before native input")
                XCTAssertFalse(actualFont.isEqual(originalFont))
            }
            print("EDITOR_PUBLIC_STYLED_FONT_CONTROL case=\(originalFontControl) original=\(String(describing: actualOriginal))")
            let inserted = "!"
            input.insertText(inserted)
            XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId),
                initialHTML.replacingOccurrences(of: prefix, with: prefix + inserted))
            XCTAssertEqual(input.selectedRange, NSRange(location: (prefix + inserted).utf16.count, length: 0))
            try assertStyledPatchMatchesFreshRender(in: input, context: "\(originalFontControl)-insert")
            input.deleteBackward()
            XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHTML)
            XCTAssertEqual(input.selectedRange, NSRange(location: prefix.utf16.count, length: 0))
            try assertStyledPatchMatchesFreshRender(in: input, context: "\(originalFontControl)-delete")
        }
    }

    private func styledParagraphBoxes(in input: EditorTextView) throws -> [EditorRenderedBox] {
        let text = input.textStorage.string as NSString
        var boxes: [EditorRenderedBox] = []
        var start = 0
        while start < text.length {
            let range = text.paragraphRange(for: NSRange(location: start, length: 0))
            let stack = try XCTUnwrap(renderedStyleBoxes(from:
                input.textStorage.attribute(editorStyleBoxesAttribute, at: start, effectiveRange: nil)))
            XCTAssertEqual(stack.count, 1, "Each plain top-level paragraph must retain one container")
            boxes.append(try XCTUnwrap(stack.first))
            start = NSMaxRange(range)
        }
        XCTAssertEqual(Set(boxes.map(ObjectIdentifier.init)).count, boxes.count,
            "Visually equal sibling paragraphs must never share a container identity")
        if let first = boxes.first {
            for box in boxes.dropFirst() {
                XCTAssertTrue(NSDictionary(dictionary: first.box.values).isEqual(to: box.box.values),
                    "The sibling identity control must actually use equal resolved box values")
            }
        }
        return boxes
    }

    @discardableResult
    private func assertStyledPatchMatchesFreshRender(in input: EditorTextView, context: String) throws -> EditorTextView {
        let blocks = try XCTUnwrap(input.currentRenderBlocks, context)
        let theme = try XCTUnwrap(input.theme, context)
        let reference = freshSiblingMarginReference(for: input, blocks: blocks, theme: theme)
        XCTAssertEqual(input.textStorage.string, reference.textStorage.string, context)
        try preserveValidatedRetainedChildIndexCoverage(from: input.textStorage, in: reference.textStorage,
            context: context)
        assertTextKitAttributedStorage(in: input, matches: reference.textStorage, compareStyledValues: true)
        assertRetainedSnapshotMetadata(input.lastAuthorizedAttributedTextStorage, matches: input.textStorage)
        XCTAssertEqual(try styledMarginGeometry(in: input), try styledMarginGeometry(in: reference),
            "\(context): current attributes, glyphs, decorations, layout and caret must match fresh rendering")
        _ = try styledParagraphBoxes(in: input)
        return reference
    }

    private func preserveValidatedRetainedChildIndexCoverage(
        from actual: NSAttributedString, in fresh: NSMutableAttributedString, context: String
    ) throws {
        XCTAssertEqual(actual.string, fresh.string, context)
        guard actual.length == fresh.length else { return }
        let key = RenderBridgeAttributes.topLevelChildIndex
        var owner: NSNumber?
        var extraCoverage: [(offset: Int, value: NSNumber)] = []
        var rawActual: [String] = []
        var rawFresh: [String] = []
        for offset in 0..<fresh.length {
            let canonicalValue = fresh.attribute(key, at: offset, effectiveRange: nil)
            let actualValue = actual.attribute(key, at: offset, effectiveRange: nil)
            if let canonicalValue {
                let marker = try XCTUnwrap(canonicalValue as? NSNumber, context)
                XCTAssertNotNil(v2ExactUInt32(marker), "\(context): fresh marker must be an exact unsigned index")
                owner = marker
                rawFresh.append("\(offset):\(marker):\(String(cString: marker.objCType))")
                XCTAssertNotNil(actualValue, "\(context): canonical boundary marker at \(offset) must remain mounted")
            }
            guard let actualValue else { continue }
            let marker = try XCTUnwrap(actualValue as? NSNumber, context)
            rawActual.append("\(offset):\(marker):\(String(cString: marker.objCType))")
            let expectedOwner = try XCTUnwrap(owner, "\(context): actual marker has no independent fresh owner at \(offset)")
            guard let actualIndex = v2ExactUInt32(marker),
                  let expectedIndex = v2ExactUInt32(expectedOwner) else {
                XCTFail("\(context): child index must preserve its exact unsigned numeric type at \(offset)")
                continue
            }
            XCTAssertEqual(actualIndex, expectedIndex,
                "\(context): retained marker at \(offset) must match independent fresh ownership")
            if actualIndex == expectedIndex, canonicalValue == nil {
                extraCoverage.append((offset, marker))
            }
        }
        print("EDITOR_PUBLIC_STYLED_INDEX_COVERAGE context=\(context) actual=\(rawActual) canonical=\(rawFresh)")
        for entry in extraCoverage {
            fresh.addAttribute(key, value: entry.value, range: NSRange(location: entry.offset, length: 1))
        }
    }

    private struct AcceptedAttributeEdit {
        let range: NSRange
        let before: NSAttributedString
        let diagnostic: String
    }

    private func assertAcceptedAttributeEditsAreNecessary(
        _ edits: [AcceptedAttributeEdit], expected: NSAttributedString, input: EditorTextView, context: String
    ) {
        let normalizedExpected = textKitLaidOutStorage(expected, in: input)
        var redundantOffsets: [Int] = []
        for edit in edits {
            let normalizedBefore = textKitLaidOutStorage(edit.before, in: input)
            XCTAssertEqual(normalizedBefore.string, normalizedExpected.string, context)
            guard normalizedBefore.length == normalizedExpected.length else { continue }
            let edited = NSIntersectionRange(edit.range, NSRange(location: 0, length: normalizedBefore.length))
            var necessaryOffsets: [Int] = []
            for offset in edited.location..<NSMaxRange(edited) {
                let before = comparableStyledAttributes(in: normalizedBefore, at: offset)
                let expected = comparableStyledAttributes(in: normalizedExpected, at: offset)
                if NSDictionary(dictionary: before).isEqual(to: expected) {
                    redundantOffsets.append(offset)
                } else {
                    necessaryOffsets.append(offset)
                }
            }
            print("EDITOR_PUBLIC_STYLED_ATTRIBUTE_EDIT context=\(context) \(edit.diagnostic) necessaryOffsets=\(necessaryOffsets)")
        }
        XCTAssertTrue(redundantOffsets.isEmpty,
            "\(context): attribute edits must repair genuinely different fresh-normalized values, not rewrite unchanged UTF16 offsets \(redundantOffsets)")
    }

    private func assertAcceptedPlainTypingKeepsInheritedAttributes(theme: EditorTheme? = nil) throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        let input = view.textView
        defer {
            input.onApplyingRustTextForTesting = nil
            _ = input.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        let initialHTML = theme == nil ? Self.plainTypingHTML : Self.publicStyledTypingHTML
        let prefix = Self.plainTypingPrefix
        let inserted = "!"
        if let theme { XCTAssertTrue(view.applyTheme(theme)) }
        view.editorId = editorId
        view.setContent(html: initialHTML)
        XCTAssertTrue(input.becomeFirstResponder())
        setCollapsedSelection(in: input, utf16Offset: prefix.utf16.count)
        flushMainQueue()
        input.layoutManager.ensureLayout(for: input.textContainer)
        var attributeOnlyEdits: [AcceptedAttributeEdit] = []
        var characterEditCount = 0
        var capturingInput = false
        var previousStorage = NSAttributedString(attributedString: input.textStorage)
        var acceptedCarets: [NSRange] = []
        if theme != nil {
            input.onApplyingRustTextForTesting = { acceptedCarets.append(input.selectedRange) }
        }
        let observer = NotificationCenter.default.addObserver(
            forName: NSTextStorage.didProcessEditingNotification, object: input.textStorage, queue: nil
        ) { notification in
            guard let storage = notification.object as? NSTextStorage else { return }
            guard capturingInput else { return }
            if storage.editedMask.contains(.editedCharacters) { characterEditCount += 1 }
            if storage.editedMask == .editedAttributes {
                var changedKeys: Set<String> = []
                var changedValueKeys: Set<String> = []
                if previousStorage.string == storage.string, previousStorage.length == storage.length {
                    let edited = NSIntersectionRange(storage.editedRange,
                        NSRange(location: 0, length: storage.length))
                    for offset in edited.location..<NSMaxRange(edited) {
                        let before = previousStorage.attributes(at: offset, effectiveRange: nil)
                        let after = storage.attributes(at: offset, effectiveRange: nil)
                        let beforeValues = self.comparableStyledAttributes(in: previousStorage, at: offset)
                        let afterValues = self.comparableStyledAttributes(in: storage, at: offset)
                        for key in Set(before.keys).union(after.keys) {
                            if !NSDictionary(dictionary: before.filter { $0.key == key })
                                .isEqual(to: after.filter { $0.key == key }) {
                                changedKeys.insert(key.rawValue)
                            }
                            if !NSDictionary(dictionary: beforeValues.filter { $0.key == key })
                                .isEqual(to: afterValues.filter { $0.key == key }) {
                                changedValueKeys.insert(key.rawValue)
                            }
                        }
                    }
                }
                attributeOnlyEdits.append(AcceptedAttributeEdit(range: storage.editedRange,
                    before: previousStorage,
                    diagnostic: "mask=\(storage.editedMask.rawValue) range=\(storage.editedRange) keys=\(changedKeys.sorted()) valueKeys=\(changedValueKeys.sorted())"))
            }
            previousStorage = NSAttributedString(attributedString: storage)
        }
        defer { NotificationCenter.default.removeObserver(observer) }

        capturingInput = true
        input.insertText(inserted)
        capturingInput = false
        let insertedHTML = initialHTML.replacingOccurrences(of: prefix, with: prefix + inserted)
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), insertedHTML)
        let insertedCaret = NSRange(location: (prefix + inserted).utf16.count, length: 0)
        if theme != nil {
            XCTAssertEqual(acceptedCarets, [insertedCaret],
                "UIKit must advance the accepted caret before Rust selection is reapplied")
        }
        XCTAssertEqual(input.selectedRange, insertedCaret)
        assertTextKitAttributedStorage(in: input, matches: input.lastAuthorizedAttributedTextStorage)
        assertRetainedSnapshotMetadata(input.lastAuthorizedAttributedTextStorage, matches: input.textStorage)
        XCTAssertGreaterThan(characterEditCount, 0, "The observer must see the actual UIKit character mutation")
        if theme != nil {
            let reference = try assertStyledPatchMatchesFreshRender(in: input, context: "insert")
            assertAcceptedAttributeEditsAreNecessary(attributeOnlyEdits, expected: reference.textStorage,
                input: input, context: "insert")
        } else {
            XCTAssertTrue(attributeOnlyEdits.isEmpty,
                "UIKit already supplied the exact authorized rich attributes; typing must not trigger a second attribute-only layout invalidation: \(attributeOnlyEdits.map(\.diagnostic))")
        }

        attributeOnlyEdits.removeAll()
        characterEditCount = 0
        acceptedCarets.removeAll()
        previousStorage = NSAttributedString(attributedString: input.textStorage)
        capturingInput = true
        input.deleteBackward()
        capturingInput = false
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHTML)
        let restoredCaret = NSRange(location: prefix.utf16.count, length: 0)
        if theme != nil {
            XCTAssertEqual(acceptedCarets, [restoredCaret],
                "UIKit must restore the accepted caret before Rust selection is reapplied")
        }
        XCTAssertEqual(input.selectedRange, restoredCaret)
        assertTextKitAttributedStorage(in: input, matches: input.lastAuthorizedAttributedTextStorage)
        assertRetainedSnapshotMetadata(input.lastAuthorizedAttributedTextStorage, matches: input.textStorage)
        XCTAssertGreaterThan(characterEditCount, 0, "The observer must see the actual UIKit deletion")
        if theme != nil {
            let reference = try assertStyledPatchMatchesFreshRender(in: input, context: "delete")
            assertAcceptedAttributeEditsAreNecessary(attributeOnlyEdits, expected: reference.textStorage,
                input: input, context: "delete")
        } else {
            XCTAssertTrue(attributeOnlyEdits.isEmpty,
                "Deleting an inherited styled character must not rewrite unchanged attributes: \(attributeOnlyEdits.map(\.diagnostic))")
        }
        XCTAssertEqual(input.reconciliationCount, 0)
    }

    func testRejectedFocusedInsertKeepsAuthorizedTextCaretAndAttributes() {
        let editorId = makeV2Editor(configJson: #"{"initialization":{"type":"localEmpty"},"policy":{"maxLength":3}}"#)
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        let window = hostEditorView(view)
        defer {
            view.textView.onApplyingRustTextForTesting = nil
            _ = view.textView.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.editorId = editorId
        let initialHTML = "<p><strong>ab</strong></p>"
        view.setContent(html: initialHTML)
        XCTAssertTrue(view.textView.becomeFirstResponder())
        let authorizedCaret = NSRange(location: view.textView.textStorage.length, length: 0)
        setSelection(in: view.textView, utf16Range: authorizedCaret)
        flushMainQueue()
        let authorized = NSAttributedString(attributedString: view.textView.textStorage)
        var renderApplications = 0
        view.textView.onApplyingRustTextForTesting = { renderApplications += 1 }

        view.textView.insertText("rejected")
        flushMainQueue()

        XCTAssertEqual(renderApplications, 0, "A rejected engine intent must never enter a native text replacement")
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHTML)
        XCTAssertEqual(view.textView.selectedRange, authorizedCaret)
        XCTAssertTrue(view.textView.textStorage.isEqual(to: authorized))
        XCTAssertTrue(view.textView.lastAuthorizedAttributedTextStorage.isEqual(to: authorized))
        XCTAssertEqual(view.textView.reconciliationCount, 0)
    }

    private func applyRenderBlockFixture(_ blocks: [[[String: Any]]], to textView: EditorTextView) throws {
        let data = try JSONSerialization.data(withJSONObject: ["renderBlocks": blocks])
        XCTAssertTrue(textView.applyUpdateJSON(try XCTUnwrap(String(data: data, encoding: .utf8)), notifyDelegate: false))
    }

    func testRetainedOpaqueAtomsKeepMetadataThroughTextChangesAndRollback() throws {
        let initial = retainedAtomRenderBlocks(prefix: ["Alpha"])
        let updates = [
            retainedAtomRenderBlocks(prefix: ["Alpha!"], positionShift: 1),
            retainedAtomRenderBlocks(prefix: ["Al", "pha"], positionShift: 2)
        ]
        for updated in updates {
            let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
            try applyRenderBlockFixture(initial, to: textView)
            let styled = NSMutableAttributedString(attributedString: textView.textStorage)
            let splitRunLength = 2
            let splitFont = UIFont.italicSystemFont(ofSize: textView.baseFont.pointSize)
            for label in Self.retainedMentionLabels {
                let labelRange = (styled.string as NSString).range(of: label)
                XCTAssertNotEqual(labelRange.location, NSNotFound)
                let splitRun = NSRange(location: labelRange.location, length: splitRunLength)
                styled.addAttribute(.font, value: splitFont, range: splitRun)
                styled.addAttribute(.foregroundColor, value: UIColor.systemRed, range: splitRun)
            }
            _ = textView.applyAttributedRender(styled, usedPatch: false, positionCacheUpdate: .invalidate)
            try applyRenderBlockFixture(updated, to: textView)
            XCTAssertTrue(textView.lastRenderAppliedPatch(), "The unchanged opaque atom suffix must be retained")
            XCTAssertEqual(textView.currentTopLevelChildMetadata?.count, updated.count, "Opaque atoms must pass metadata preflight in the current frame")
            let expected = RenderBridge.renderBlocks(fromArray: updated as [[NSDictionary]], baseFont: textView.baseFont, textColor: textView.baseTextColor)
            XCTAssertEqual(textView.textStorage.string, expected.string)
            for label in Self.retainedMentionLabels {
                let labelRange = (textView.textStorage.string as NSString).range(of: label)
                XCTAssertEqual(textView.textStorage.attribute(.font, at: labelRange.location, effectiveRange: nil) as? UIFont, splitFont)
                XCTAssertEqual(textView.textStorage.attribute(.foregroundColor, at: labelRange.location, effectiveRange: nil) as? UIColor, UIColor.systemRed)
            }
            expected.enumerateAttribute(RenderBridgeAttributes.voidNodeType, in: NSRange(location: 0, length: expected.length)) { value, range, _ in
                guard value != nil else { return }
                for offset in range.location..<NSMaxRange(range) {
                    XCTAssertEqual(
                        (textView.textStorage.attribute(RenderBridgeAttributes.docPos, at: offset, effectiveRange: nil) as? NSNumber)?.uint32Value,
                        (expected.attribute(RenderBridgeAttributes.docPos, at: offset, effectiveRange: nil) as? NSNumber)?.uint32Value,
                        "Retained opaque atom position at \(offset)"
                    )
                }
            }
            let authorized = NSAttributedString(attributedString: textView.textStorage)
            assertRetainedSnapshotMetadata(textView.lastAuthorizedAttributedTextStorage, matches: authorized)
            textView.performTransientTextMutation {
                textView.textStorage.replaceCharacters(in: NSRange(location: 0, length: 0), with: "optimistic ")
            }
            textView.restoreAuthorizedTextSnapshot()
            XCTAssertTrue(textView.textStorage.isEqual(to: authorized), "Optimistic rollback must restore current atom positions and child indexes immediately")
        }
    }

    func testRenderPatchRecoversCurrentFrameWhenRetainedAtomMetadataIsUnavailable() throws {
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let initial = retainedAtomRenderBlocks(prefix: ["Alpha"])
        let updated = retainedAtomRenderBlocks(prefix: ["Alpha!"], positionShift: 1)
        try applyRenderBlockFixture(initial, to: textView)
        let mentionRange = (textView.textStorage.string as NSString).range(of: Self.retainedMentionLabels[0])
        textView.textStorage.removeAttribute(RenderBridgeAttributes.voidNodeType, range: mentionRange)
        try applyRenderBlockFixture(updated, to: textView)
        XCTAssertFalse(textView.lastRenderAppliedPatch(), "Unsupported retained metadata must trigger a full render in the current frame")
        XCTAssertEqual(textView.currentTopLevelChildMetadata?.count, updated.count)
        let expected = RenderBridge.renderBlocks(fromArray: updated as [[NSDictionary]], baseFont: textView.baseFont, textColor: textView.baseTextColor)
        assertRetainedSnapshotMetadata(textView.textStorage, matches: expected)
        assertRetainedSnapshotMetadata(textView.lastAuthorizedAttributedTextStorage, matches: expected)
    }

    func testRetainedMetadataUpdatesCoalesceAcrossStructuralEdits() throws {
        let suffixCount = 24
        let maximumStorageTransactions = 2
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let suffix = (0..<suffixCount).map { index in
            "<p><strong>Sibling \(index)</strong> <em>styled tail</em></p>"
        }.joined()
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p>" + suffix)
        setSelection(in: textView, utf16Range: NSRange(location: 2, length: 0))

        var storageTransactions = 0
        let observer = NotificationCenter.default.addObserver(
            forName: NSTextStorage.didProcessEditingNotification,
            object: textView.textStorage,
            queue: nil
        ) { _ in storageTransactions += 1 }
        defer { NotificationCenter.default.removeObserver(observer) }

        let edits: [(name: String, apply: () -> Void)] = [
            ("split", { textView.insertText("\n") }),
            ("merge", { textView.deleteBackward() }),
            ("prepend", {
                self.setSelection(in: textView, utf16Range: NSRange(location: 0, length: 0))
                textView.insertText("\n")
            }),
            ("type before retained siblings", { textView.insertText("x") })
        ]
        for edit in edits {
            storageTransactions = 0
            edit.apply()
            XCTAssertTrue(textView.lastRenderAppliedPatch(), edit.name)
            let blocks = try XCTUnwrap(textView.currentRenderBlocks, edit.name)
            let expected = RenderBridge.renderBlocks(
                fromArray: blocks,
                baseFont: textView.baseFont,
                textColor: textView.baseTextColor
            )
            XCTAssertEqual(textView.textStorage.string, expected.string, edit.name)
            let firstRetainedSibling = (expected.string as NSString).range(of: "Sibling 0")
            XCTAssertNotEqual(firstRetainedSibling.location, NSNotFound, edit.name)
            let retainedAttributesStart = max(0, firstRetainedSibling.location - 1)
            var firstAttributeMismatch: String?
            var expectedChildIndex: Int?
            for offset in 0..<expected.length {
                var actualAttributes = textView.textStorage.attributes(at: offset, effectiveRange: nil)
                var expectedAttributes = expected.attributes(at: offset, effectiveRange: nil)
                let expectedIndex = expectedAttributes.removeValue(forKey: RenderBridgeAttributes.topLevelChildIndex) as? NSNumber
                let actualIndex = actualAttributes.removeValue(forKey: RenderBridgeAttributes.topLevelChildIndex) as? NSNumber
                if let expectedIndex {
                    expectedChildIndex = expectedIndex.intValue
                    XCTAssertEqual(actualIndex?.intValue, expectedChildIndex, "\(edit.name) required child marker at \(offset)")
                }
                if let actualIndex {
                    XCTAssertEqual(actualIndex.intValue, expectedChildIndex, "\(edit.name) retained child index at \(offset)")
                }
                let actualAttachment = actualAttributes.removeValue(forKey: .attachment) as? NSTextAttachment
                let expectedAttachment = expectedAttributes.removeValue(forKey: .attachment) as? NSTextAttachment
                XCTAssertEqual(actualAttachment?.bounds, expectedAttachment?.bounds, "\(edit.name) attachment at \(offset)")
                XCTAssertEqual(
                    actualAttachment.map { String(describing: type(of: $0)) },
                    expectedAttachment.map { String(describing: type(of: $0)) },
                    "\(edit.name) attachment type at \(offset)"
                )
                if offset >= retainedAttributesStart,
                   firstAttributeMismatch == nil,
                   !NSDictionary(dictionary: actualAttributes).isEqual(to: expectedAttributes) {
                    firstAttributeMismatch = "offset=\(offset) actual=\(actualAttributes) expected=\(expectedAttributes)"
                }
            }
            XCTAssertNil(firstAttributeMismatch, "\(edit.name): rich attributes and child indexes must match a fresh render")
            XCTAssertLessThanOrEqual(
                storageTransactions,
                maximumStorageTransactions,
                "\(edit.name): one patch transaction and one retained metadata transaction must cover \(suffixCount) rich siblings"
            )
        }

        storageTransactions = 0
        XCTAssertTrue(textView.refreshRetainedPositionalMetadata(
            startingAt: 0,
            updatedRenderBlocks: try XCTUnwrap(textView.currentRenderBlocks)
        ))
        XCTAssertEqual(storageTransactions, 0, "Already current metadata must not invalidate text layout")
    }

    func testVersionedParagraphSplitKeepsCollapsedSiblingMargins() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        textView.captureApplyUpdateTraceForTesting = true
        textView.theme = try XCTUnwrap(EditorTheme.from(json: #"{"version":1,"styles":{"paragraph":{"marginTop":20,"marginBottom":12}}}"#))
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p><p>Beta</p><p>Gamma</p>")
        let beta = (textView.text as NSString).range(of: "Beta")
        let splitOffset = UInt32(NSMaxRange(beta))
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: splitOffset, scalarHead: splitOffset)
        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        textView.insertText("\n")
        XCTAssertTrue(textView.lastRenderAppliedPatch())
        let text = textView.textStorage.string as NSString
        var start = 0
        while start < text.length {
            let range = text.paragraphRange(for: NSRange(location: start, length: 0))
            let style = try XCTUnwrap(textView.textStorage.attribute(.paragraphStyle, at: start, effectiveRange: nil) as? NSParagraphStyle)
            XCTAssertEqual(style.paragraphSpacingBefore, start == 0 ? 20 : 8, accuracy: 0.01, "paragraph at \(start)")
            start = NSMaxRange(range)
        }
    }

    func testParagraphSplitAppliesTopLevelRenderPatch() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.captureApplyUpdateTraceForTesting = true
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p><p>Beta</p><p>Gamma</p>")

        let betaRange = (textView.text as NSString).range(of: "Beta")
        XCTAssertNotEqual(betaRange.location, NSNotFound)
        let splitOffset = UInt32(betaRange.location + betaRange.length)
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: splitOffset, scalarHead: splitOffset)
        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        XCTAssertEqual(textView.currentLogicalScalarSelection()?.head, splitOffset)

        textView.insertText("\n")

        XCTAssertTrue(
            textView.lastRenderAppliedPatch(),
            "splitting a middle paragraph should use the native top-level patch path"
        )
        XCTAssertEqual(
            textView.textStorage.string,
            "Alpha\nBeta\n\u{200B}\nGamma",
            "split patches must replace the full structural block region so the new paragraph separator renders correctly"
        )
        let selectedOffset = textView.offset(
            from: textView.beginningOfDocument,
            to: textView.selectedTextRange?.start ?? textView.endOfDocument
        )
        let gammaRange = (textView.text as NSString).range(of: "Gamma")
        XCTAssertGreaterThanOrEqual(
            selectedOffset,
            betaRange.location + betaRange.length + 1,
            "after splitting at the end of a paragraph, the caret should land inside the inserted empty paragraph"
        )
        XCTAssertLessThan(
            selectedOffset,
            gammaRange.location,
            "after splitting at the end of a paragraph, the caret must stay before the following paragraph"
        )
        XCTAssertEqual(
            EditorV2Shadow.getHtml(id: editorId),
            "<p>Alpha</p><p>Beta</p><p></p><p>Gamma</p>"
        )
    }

    func testSequentialParagraphSplitsKeepUsingTopLevelRenderPatch() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        textView.captureApplyUpdateTraceForTesting = true
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p><p>Beta</p><p>Gamma</p>")

        let betaRange = (textView.text as NSString).range(of: "Beta")
        XCTAssertNotEqual(betaRange.location, NSNotFound)
        let firstSplitOffset = UInt32(betaRange.location + betaRange.length)
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: firstSplitOffset, scalarHead: firstSplitOffset)
        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        textView.insertText("\n")

        XCTAssertTrue(textView.lastRenderAppliedPatch())

        let gammaRange = (textView.text as NSString).range(of: "Gamma")
        XCTAssertNotEqual(gammaRange.location, NSNotFound)
        let secondSplitOffset = UInt32(gammaRange.location + gammaRange.length)
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: secondSplitOffset, scalarHead: secondSplitOffset)
        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        textView.insertText("\n")

        XCTAssertTrue(
            textView.lastRenderAppliedPatch(),
            "top-level metadata cache should remain valid across consecutive structural edits"
        )
        XCTAssertEqual(
            textView.textStorage.string,
            "Alpha\nBeta\n\u{200B}\nGamma\n\u{200B}"
        )
        XCTAssertEqual(
            EditorV2Shadow.getHtml(id: editorId),
            "<p>Alpha</p><p>Beta</p><p></p><p>Gamma</p><p></p>"
        )
    }

    func testFullAtomicRenderRefreshesShiftedImageDocPosBeforeResizeAction() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.editorId = editorId
        view.setContent(html: """
        <p>A</p><img src="https://example.com/target.png" width="140" height="80"><img src="https://example.com/control.png" width="90" height="60"><p></p>
        """)
        view.layoutIfNeeded()

        guard let originalTargetRange = firstImageRange(in: view.textView),
            let originalDocPos = (view.textView.textStorage.attributes(
                at: originalTargetRange.location,
                effectiveRange: nil
            )[RenderBridgeAttributes.docPos] as? NSNumber)?.uint32Value
        else {
            XCTFail("expected the target image and its document position")
            return
        }

        let fullAtomicRender = EditorV2Shadow.replaceHtml(
            id: editorId,
            html: """
            <p>Preceding text is now longer</p><img src="https://example.com/target.png" width="140" height="80"><img src="https://example.com/control.png" width="90" height="60"><p></p>
            """
        )
        view.textView.applyUpdateJSON(fullAtomicRender, notifyDelegate: false)
        view.layoutIfNeeded()

        guard let targetRange = firstImageRange(in: view.textView),
            let refreshedDocPos = (view.textView.textStorage.attributes(
                at: targetRange.location,
                effectiveRange: nil
            )[RenderBridgeAttributes.docPos] as? NSNumber)?.uint32Value
        else {
            XCTFail("expected the target image after the atomic render")
            return
        }

        XCTAssertNotEqual(
            refreshedDocPos,
            originalDocPos,
            "a preceding extent change must refresh retained atom document positions"
        )

        XCTAssertTrue(view.textView.becomeFirstResponder())
        setSelection(in: view.textView, utf16Range: targetRange)
        flushMainQueue()
        view.layoutIfNeeded()
        view.resizeSelectedImageForTesting(width: 200, height: 100)
        flushMainQueue()

        let html = EditorV2Shadow.getHtml(id: editorId)
        XCTAssertTrue(
            html.contains("target.png\" width=\"200\""),
            "the selected target image must receive the resize action, got: \(html)"
        )
        XCTAssertTrue(
            html.contains("control.png\" width=\"90\""),
            "resizing the target must not affect the adjacent control image, got: \(html)"
        )
    }

    func testPrependingTopLevelChildRefreshesRetainedChildIndexes() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.bindEditor(id: editorId, initialHTML: "<hr><hr>")

        let fullAtomicRender = EditorV2Shadow.replaceHtml(
            id: editorId,
            html: "<p>Prelude</p><hr><hr>"
        )
        textView.applyUpdateJSON(fullAtomicRender, notifyDelegate: false)

        let horizontalRuleRanges = (0..<textView.textStorage.length).compactMap { index -> NSRange? in
            let attrs = textView.textStorage.attributes(at: index, effectiveRange: nil)
            return (attrs[RenderBridgeAttributes.voidNodeType] as? String)
                .map(EditorNodeTypes.isHorizontalRule) == true
                ? NSRange(location: index, length: 1)
                : nil
        }
        XCTAssertEqual(horizontalRuleRanges.count, 2)
        guard horizontalRuleRanges.count == 2 else { return }
        XCTAssertEqual(
            (textView.textStorage.attributes(
                at: horizontalRuleRanges[0].location,
                effectiveRange: nil
            )[RenderBridgeAttributes.topLevelChildIndex] as? NSNumber)?.intValue,
            1,
            "the first retained atom must receive its shifted top-level child index"
        )
        XCTAssertEqual(
            (textView.textStorage.attributes(
                at: horizontalRuleRanges[1].location,
                effectiveRange: nil
            )[RenderBridgeAttributes.topLevelChildIndex] as? NSNumber)?.intValue,
            2,
            "every retained sibling after a prepend must receive its shifted index"
        )
    }

    func testExplicitPrependRenderPatchRefreshesRetainedAtomMetadata() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.captureApplyUpdateTraceForTesting = true
        textView.bindEditor(id: editorId, initialHTML: "<hr><hr>")

        let finalUpdateJSON = EditorV2Shadow.replaceHtml(
            id: editorId,
            html: "<p>Prelude</p><hr><hr>"
        )
        var explicitPatchUpdate = parseJSONObject(finalUpdateJSON)
        let finalRenderBlocks = try XCTUnwrap(
            (explicitPatchUpdate["renderPatch"] as? [String: Any])?["renderBlocks"]
                as? [[NSDictionary]]
        )
        explicitPatchUpdate["renderBlocks"] = finalRenderBlocks
        explicitPatchUpdate["renderPatch"] = [
            "startIndex": 0,
            "deleteCount": 0,
            "renderBlocks": [finalRenderBlocks[0]]
        ]
        let explicitPatchData = try JSONSerialization.data(withJSONObject: explicitPatchUpdate)
        let explicitPatchJSON = try XCTUnwrap(String(data: explicitPatchData, encoding: .utf8))

        var storageTransactions = 0
        let observer = NotificationCenter.default.addObserver(
            forName: NSTextStorage.didProcessEditingNotification,
            object: textView.textStorage,
            queue: nil
        ) { _ in storageTransactions += 1 }
        defer { NotificationCenter.default.removeObserver(observer) }
        textView.applyUpdateJSON(explicitPatchJSON, notifyDelegate: false)
        let maximumStorageTransactions = 2
        XCTAssertLessThanOrEqual(storageTransactions, maximumStorageTransactions, "The prepend and retained atom metadata must use one transaction each")

        let expected = RenderBridge.renderBlocks(
            fromArray: finalRenderBlocks,
            baseFont: textView.baseFont,
            textColor: textView.baseTextColor
        )
        let trace = try XCTUnwrap(textView.lastApplyUpdateTrace())
        XCTAssertTrue(
            trace.attemptedPatch,
            "the update must exercise the explicit renderPatch path"
        )
        XCTAssertTrue(
            trace.usedPatch,
            "the explicit prepend must retain the compact renderPatch path"
        )
        XCTAssertEqual(textView.textStorage.string, expected.string, "the prepend must not duplicate content")

        let actualAtomOffsets = (0..<textView.textStorage.length).filter { index in
            textView.textStorage.attributes(at: index, effectiveRange: nil)[RenderBridgeAttributes.voidNodeType] != nil
        }
        let expectedAtomOffsets = (0..<expected.length).filter { index in
            expected.attributes(at: index, effectiveRange: nil)[RenderBridgeAttributes.voidNodeType] != nil
        }
        XCTAssertEqual(actualAtomOffsets.count, 2)
        XCTAssertEqual(actualAtomOffsets.count, expectedAtomOffsets.count)

        for (actualOffset, expectedOffset) in zip(actualAtomOffsets, expectedAtomOffsets) {
            let actualAttributes = textView.textStorage.attributes(at: actualOffset, effectiveRange: nil)
            let expectedAttributes = expected.attributes(at: expectedOffset, effectiveRange: nil)
            XCTAssertEqual(
                (actualAttributes[RenderBridgeAttributes.topLevelChildIndex] as? NSNumber)?.intValue,
                (expectedAttributes[RenderBridgeAttributes.topLevelChildIndex] as? NSNumber)?.intValue
            )
            XCTAssertEqual(
                (actualAttributes[RenderBridgeAttributes.docPos] as? NSNumber)?.uint32Value,
                (expectedAttributes[RenderBridgeAttributes.docPos] as? NSNumber)?.uint32Value
            )
        }
        storageTransactions = 0
        XCTAssertTrue(textView.refreshRetainedPositionalMetadata(startingAt: 0, updatedRenderBlocks: finalRenderBlocks))
        XCTAssertEqual(storageTransactions, 0, "Current retained atom positions must not invalidate text layout")

        let authorized = NSAttributedString(attributedString: textView.textStorage)
        assertRetainedSnapshotMetadata(textView.lastAuthorizedAttributedTextStorage, matches: authorized)
        setSelection(in: textView, utf16Range: NSRange(location: 0, length: 0))
        let sessionID = "retained-atom-composition"
        _ = textView.beginExternalTextComposition(sessionId: sessionID)
        let composition = try XCTUnwrap(textView.externalTextComposition)
        assertRetainedSnapshotMetadata(composition.startingAuthorizedAttributedText, matches: authorized)
        _ = textView.updateExternalTextComposition(sessionId: sessionID, text: "preview")
        _ = textView.cancelExternalTextComposition(sessionId: sessionID, cause: "consumer")
        XCTAssertTrue(textView.textStorage.isEqual(to: authorized), "Cancelling composition must preserve current atom metadata")
    }

    func testWrongRenderPatchBaseRecoversAFullNativeSnapshot() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        guard let adapter = EditorV2Registry.adapter(forLegacyId: editorId) else {
            XCTFail("expected adapter")
            return
        }
        _ = EditorV2Shadow.setHtml(id: editorId, html: "<p>Alpha</p>")
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p>")
        let revision = adapter.baseDocumentRevision
        let wrongBase = revision == 0 ? UInt64(1) : UInt64(0)
        let stalePatch: [String: Any] = [
            "documentVersion": String(revision),
            "renderPatch": [
                "baseDocumentVersion": String(wrongBase),
                "startIndex": 0,
                "deleteCount": 1,
                "renderBlocks": [[
                    ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                    ["type": "textRun", "text": "Corrupt", "marks": []],
                    ["type": "blockEnd"]
                ]]
            ]
        ]
        let staleData = try JSONSerialization.data(withJSONObject: stalePatch)
        let staleJSON = try XCTUnwrap(String(data: staleData, encoding: .utf8))

        XCTAssertTrue(textView.applyUpdateJSON(staleJSON, notifyDelegate: false))
        XCTAssertEqual(textView.textStorage.string, "Alpha")
        XCTAssertFalse(textView.lastRenderAppliedPatch())

        let nextUpdate = EditorV2Shadow.insertTextScalar(
            id: editorId,
            scalarPos: 5,
            text: "!"
        )
        XCTAssertTrue(textView.applyUpdateJSON(nextUpdate, notifyDelegate: false))
        XCTAssertEqual(textView.textStorage.string, "Alpha!")
    }

    func testTypingInsideListItemFallsBackToFullRenderAndPreservesTextOrder() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.captureApplyUpdateTraceForTesting = true
        textView.bindEditor(
            id: editorId,
            initialHTML: "<ul><li><p>Alpha</p></li><li><p>Beta</p></li></ul>"
        )

        let alphaRange = (textView.text as NSString).range(of: "Alpha")
        XCTAssertNotEqual(alphaRange.location, NSNotFound)
        setCollapsedSelection(in: textView, utf16Offset: alphaRange.location + alphaRange.length)
        flushMainQueue()

        textView.insertText("!")

        XCTAssertFalse(
            textView.lastRenderAppliedPatch(),
            "list items should bypass the top-level render patch path until list marker patching is made safe"
        )
        XCTAssertEqual(textView.textStorage.string, "Alpha!\nBeta")
        XCTAssertEqual(
            EditorV2Shadow.getHtml(id: editorId),
            "<ul><li><p>Alpha!</p></li><li><p>Beta</p></li></ul>"
        )
    }

    func testReturnInsideListItemFallsBackToFullRenderAndKeepsTypingInNewItem() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 180))
        textView.captureApplyUpdateTraceForTesting = true
        textView.bindEditor(
            id: editorId,
            initialHTML: "<ul><li><p>Alpha</p></li><li><p>Beta</p></li></ul>"
        )

        let alphaRange = (textView.text as NSString).range(of: "Alpha")
        XCTAssertNotEqual(alphaRange.location, NSNotFound)
        setCollapsedSelection(in: textView, utf16Offset: alphaRange.location + alphaRange.length)
        flushMainQueue()

        textView.insertText("\n")

        XCTAssertFalse(
            textView.lastRenderAppliedPatch(),
            "splitting list items should use the full render path to keep caret mapping stable"
        )
        textView.insertText("B")

        XCTAssertEqual(textView.textStorage.string, "Alpha\nB\nBeta")
        XCTAssertEqual(
            EditorV2Shadow.getHtml(id: editorId),
            "<ul><li><p>Alpha</p></li><li><p>B</p></li><li><p>Beta</p></li></ul>"
        )
    }

    func testFullCurrentStateLocalEditUsesSynthesizedTopLevelPatch() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p><p>Beta</p><p>Gamma</p>")

        let updatedDocument = """
        {
        "type": "doc",
        "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Alpha"}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "Better"}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "Gamma"}]}
        ]
        }
        """
        let update = EditorV2Shadow.setJson(id: editorId, json: updatedDocument)
        textView.applyUpdateJSON(update, notifyDelegate: false)

        XCTAssertTrue(
            textView.lastRenderAppliedPatch(),
            "full current-state updates should synthesize a top-level patch when only a local block range changes"
        )
        XCTAssertEqual(textView.textStorage.string, "Alpha\nBetter\nGamma")
    }

    func testIdenticalFullCurrentStateSkipsNativeTextReapply() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p><p>Beta</p><p>Gamma</p>")
        textView.captureApplyUpdateTraceForTesting = true

        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)

        let trace = try XCTUnwrap(textView.lastApplyUpdateTrace())
        XCTAssertFalse(textView.lastRenderAppliedPatch())
        XCTAssertEqual(trace.buildRenderNanos, 0)
        XCTAssertEqual(trace.applyRenderNanos, 0)
        XCTAssertEqual(trace.applyRenderTextMutationNanos, 0)
        XCTAssertEqual(textView.textStorage.string, "Alpha\nBeta\nGamma")
    }

    func testRustDrivenSelectionApplyDoesNotNotifySelectionDelegate() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 320, height: 160))
        let delegate = EditorTextViewDelegateSpy()
        textView.editorDelegate = delegate
        textView.bindEditor(id: editorId, initialHTML: "<p>Alpha</p><p>Beta</p>")
        delegate.selectionChanges.removeAll()
        delegate.receivedUpdates.removeAll()

        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: 8, scalarHead: 8)
        textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        flushMainQueue()

        XCTAssertEqual(delegate.selectionChanges.count, 0)
        XCTAssertEqual(delegate.receivedUpdates.count, 0)
    }

    private enum PublicMixedAtomFixture {
        static let viewport = CGRect(x: 0, y: 0, width: 320, height: 240)
        static let prefixLead = "Before "
        static let originalFragment = "A😀B"
        static let replacementFragment = "👩‍💻"
        static let prefixTail = " after"
        static let atomTail = "Suffix"
        static let finalParagraph = "Tail styled"
        static let transientSuffix = " preview"

        static func prefix(_ fragment: String) -> String { prefixLead + fragment + prefixTail }
        static func text(_ fragment: String) -> String {
            prefix(fragment) + "\n" + RichTextEditorViewTests.retainedMentionLabels.joined() + "\n" + atomTail
                + "\n" + LayoutConstants.objectReplacementCharacter + "\n" + finalParagraph
        }

        static var document: [String: Any] {
            ["type": "doc", "content": [
                ["type": "paragraph", "content": [
                    ["type": "text", "text": prefix(originalFragment), "marks": [["type": "bold"]]]
                ]],
                ["type": "paragraph", "content": RichTextEditorViewTests.retainedMentionLabels.enumerated().map { index, label in
                    ["type": "mention", "attrs": ["id": "retained-mention-\(index)", "label": label]] as [String: Any]
                } + [
                    ["type": "hardBreak"],
                    ["type": "text", "text": atomTail, "marks": [["type": "italic"]]]
                ]],
                ["type": "horizontalRule"],
                ["type": "paragraph", "content": [
                    ["type": "text", "text": finalParagraph, "marks": [["type": "italic"]]]
                ]]
            ]]
        }
    }

    private func withPublicMixedAtomEditor(
        _ body: (UInt64, RichTextEditorView, EditorV2Adapter) throws -> Void
    ) throws {
        let editorId = makeV2Editor(configJson: mentionEditorConfigJson())
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: PublicMixedAtomFixture.viewport)
        let window = hostEditorView(view)
        defer {
            view.textView.onApplyingRustTextForTesting = nil
            view.textView.captureApplyUpdateTraceForTesting = false
            _ = view.textView.resignFirstResponder()
            view.textView.unbindEditor()
            view.removeFromSuperview()
            window.isHidden = true
        }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        defer { adapter.transformNativeFrameForTesting = nil }
        view.editorId = editorId
        let data = try JSONSerialization.data(withJSONObject: PublicMixedAtomFixture.document)
        view.setContent(json: try XCTUnwrap(String(data: data, encoding: .utf8)))
        let roots = try XCTUnwrap(parseJSONObject(EditorV2Shadow.getJson(id: editorId))["content"] as? [[String: Any]])
        XCTAssertEqual(roots.compactMap { $0["type"] as? String },
            ["paragraph", "paragraph", "horizontalRule", "paragraph"], "The actual engine must contain editable prose and mixed atoms")
        let atomParagraph = try XCTUnwrap(roots.first { root in
            (root["content"] as? [[String: Any]])?.first?["type"] as? String == "mention"
        })
        let atoms = try XCTUnwrap(atomParagraph["content"] as? [[String: Any]])
        XCTAssertEqual(atoms.compactMap { $0["type"] as? String }, ["mention", "mention", "hardBreak", "text"])
        XCTAssertEqual(atoms.prefix(Self.retainedMentionLabels.count).compactMap {
            ($0["attrs"] as? [String: Any])?["label"] as? String
        }, Self.retainedMentionLabels)
        XCTAssertEqual(view.textView.textStorage.string, PublicMixedAtomFixture.text(PublicMixedAtomFixture.originalFragment))
        XCTAssertTrue(view.textView.becomeFirstResponder())
        flushMainQueue()
        XCTAssertTrue(view.textView.isFirstResponder)
        XCTAssertTrue(view.textView.window === window)
        XCTAssertTrue(view.textView.ownsNativeBinding(adapter))
        view.textView.captureApplyUpdateTraceForTesting = true
        try body(editorId, view, adapter)
        XCTAssertTrue(view.textView.isFirstResponder)
        XCTAssertTrue(view.textView.window === window)
        XCTAssertTrue(view.textView.ownsNativeBinding(adapter))
    }

    private func assertPublicMixedAtomAction(
        in input: EditorTextView, adapter: EditorV2Adapter, context: String,
        expectedFragment: String, expectedCaret: Int,
        requiresRetainedAtoms: Bool, expectsPatch: Bool, perform: () -> Void
    ) throws {
        let priorRevision = try XCTUnwrap(adapter.cachedAtomicRenderDocumentRevision, context)
        let priorInstalled = try XCTUnwrap(adapter.installedFrameRevision, context)
        let priorBlocks = try XCTUnwrap(adapter.cachedSemanticRenderBlocks, context)
        let priorViewRevision = try XCTUnwrap(input.currentRenderBlocksDocumentVersion, context)
        XCTAssertEqual(priorRevision, priorInstalled, context)
        XCTAssertEqual(priorRevision, priorViewRevision, context)
        XCTAssertTrue(adapter.tableIndex.tableKeys.isEmpty, context)
        XCTAssertTrue(adapter.tableIndex.rootExtents.isEmpty, context)
        XCTAssertNil(adapter.transformNativeFrameForTesting, context)
        var frames: [FfiNativeRenderFrame] = []
        adapter.transformNativeFrameForTesting = { frame in
            frames.append(frame)
            return frame
        }
        defer { adapter.transformNativeFrameForTesting = nil }
        perform()
        flushMainQueue()
        for (index, frame) in frames.enumerated() {
            let attachment = XCTAttachment(string: frame.snapshotJson)
            attachment.name = "\(context).input-frame-\(index)"
            attachment.lifetime = .keepAlways
            add(attachment)
        }
        XCTAssertEqual(frames.count, 1, "\(context): public input must produce one unchanged real frame without adapter recovery")
        let frame = try XCTUnwrap(frames.first, context)
        XCTAssertEqual(frame.tables.kind, .delta, context)
        XCTAssertEqual(frame.tables.baseDocumentRevision, String(priorInstalled), context)
        let snapshot = try XCTUnwrap(EditorV2Adapter.parseAtomicRenderSnapshot(frame.snapshotJson), context)
        let patch = try XCTUnwrap(snapshot.renderObject["renderPatch"] as? [String: Any], context)
        XCTAssertTrue(snapshot.renderObject["renderBlocks"] is NSNull, context)
        XCTAssertEqual(patch["baseDocumentVersion"] as? String, String(priorRevision), context)
        let start = Int(try XCTUnwrap(EditorV2Adapter.uint32Field(patch, "startIndex"), context))
        let deleted = Int(try XCTUnwrap(EditorV2Adapter.uint32Field(patch, "deleteCount"), context))
        let inserted = try XCTUnwrap(patch["renderBlocks"] as? [[[String: Any]]], context)
        XCTAssertFalse(inserted.isEmpty, context)
        XCTAssertLessThanOrEqual(start + deleted, priorBlocks.count, context)
        let adoptedBlocks = try XCTUnwrap(adapter.cachedSemanticRenderBlocks, context)
        XCTAssertEqual(try XCTUnwrap(input.currentRenderBlocks, context) as NSArray, adoptedBlocks as NSArray, context)
        if requiresRetainedAtoms {
            let oldAtomChild = try XCTUnwrap(priorBlocks.firstIndex { block in
                block.contains { $0["nodeType"] as? String == "mention" }
            }, context)
            let newAtomChild = try XCTUnwrap(adoptedBlocks.firstIndex { block in
                block.contains { $0["nodeType"] as? String == "mention" }
            }, context)
            XCTAssertLessThanOrEqual(start + deleted, oldAtomChild, "\(context): the genuine patch must retain the old adjacent atom block")
            XCTAssertLessThanOrEqual(start + inserted.count, newAtomChild, "\(context): metadata refresh must walk the retained mixed atom suffix")
            XCTAssertLessThan(newAtomChild, adoptedBlocks.count - 1, context)
        }
        XCTAssertEqual(input.lastRenderAppliedPatch(), expectsPatch, context)
        XCTAssertEqual(adapter.cachedAtomicRenderJSON, frame.snapshotJson, context)
        XCTAssertEqual(snapshot.documentRevision, priorRevision + 1, context)
        XCTAssertEqual(input.textStorage.string, PublicMixedAtomFixture.text(expectedFragment), context)
        XCTAssertEqual(input.selectedRange, NSRange(location: expectedCaret, length: 0), context)
        let expectedScalar = UInt32((input.textStorage.string as NSString).substring(to: expectedCaret).unicodeScalars.count)
        XCTAssertEqual(snapshot.selection?.anchor, expectedScalar, context)
        XCTAssertEqual(snapshot.selection?.head, expectedScalar, context)
        XCTAssertEqual(adapter.cachedAuthoritativeScalarSelection?.anchor, expectedScalar, context)
        XCTAssertEqual(adapter.cachedAuthoritativeScalarSelection?.head, expectedScalar, context)
        XCTAssertTrue((snapshot.activeState["marks"] as? [String: Bool])?["bold"] == true, context)
        XCTAssertTrue(snapshot.historyState.canUndo, context)
        XCTAssertFalse(snapshot.historyState.canRedo, context)
        XCTAssertFalse(snapshot.documentIsEmpty, context)
        XCTAssertEqual(adapter.cachedScalarLength, snapshot.scalarLength, context)
        XCTAssertNotNil(frame.positionEpoch, context)
        XCTAssertEqual(adapter.positionEpoch.map(String.init), frame.positionEpoch, context)
        var completeDelta = parseJSONObject(frame.snapshotJson)
        XCTAssertEqual(Set(completeDelta.keys), EditorV2Adapter.atomicRenderSnapshotKeys, context)
        completeDelta["renderBlocks"] = adoptedBlocks
        completeDelta["renderPatch"] = NSNull()
        let mounted = NSAttributedString(attributedString: input.textStorage)
        frames.removeAll()
        XCTAssertNotNil(adapter.recoverNativeRender(), context)
        XCTAssertEqual(frames.count, 1, "\(context): independent oracle must capture the genuine same-engine full projection")
        let full = try XCTUnwrap(frames.first, context)
        let fullAttachment = XCTAttachment(string: full.snapshotJson)
        fullAttachment.name = context + ".independent-full"
        fullAttachment.lifetime = .keepAlways
        add(fullAttachment)
        XCTAssertEqual(full.tables.kind, .full, context)
        let fullSnapshot = try XCTUnwrap(EditorV2Adapter.parseAtomicRenderSnapshot(full.snapshotJson), context)
        XCTAssertTrue(fullSnapshot.renderObject["renderPatch"] is NSNull, context)
        let fullBlocks = try XCTUnwrap(fullSnapshot.renderObject["renderBlocks"] as? [[NSDictionary]], context)
        XCTAssertEqual(NSDictionary(dictionary: completeDelta), parseJSONObject(full.snapshotJson) as NSDictionary,
            "\(context): all nine canonical fields must match independent full rendering")
        XCTAssertEqual(adapter.positionEpoch.map(String.init), full.positionEpoch, context)
        XCTAssertTrue(input.textStorage.isEqual(to: mounted), "\(context): independent projection must leave the mounted view untouched")
        let fresh = RenderBridge.renderBlocks(fromArray: fullBlocks, baseFont: input.baseFont,
            textColor: input.baseTextColor, theme: input.theme, atomConfiguration: input.atomRenderConfiguration)
        let reference = styledDrawingReference(for: input, storage: fresh, theme: input.theme)
        XCTAssertEqual(input.textStorage.string, reference.textStorage.string, context)
        XCTAssertEqual(input.textStorage.length, reference.textStorage.length, context)
        XCTAssertTrue((input.textStorage.string as NSString).isEqual(to: reference.textStorage.string), context)
        var mentionPositions: [UInt32] = []
        for label in Self.retainedMentionLabels {
            let range = (reference.textStorage.string as NSString).range(of: label)
            guard range.location != NSNotFound else { XCTFail("\(context): expected actual atom label \(label)"); return }
            let expectedPosition = try XCTUnwrap(RenderBridge.jsonUInt32(
                reference.textStorage.attribute(RenderBridgeAttributes.docPos, at: range.location, effectiveRange: nil)), context)
            mentionPositions.append(expectedPosition)
            for offset in range.location..<NSMaxRange(range) {
                XCTAssertEqual(input.textStorage.attribute(RenderBridgeAttributes.voidNodeType, at: offset, effectiveRange: nil) as? String,
                    "mention", "\(context): every adjacent atom character must retain its type")
                XCTAssertEqual(RenderBridge.jsonUInt32(input.textStorage.attribute(RenderBridgeAttributes.docPos,
                    at: offset, effectiveRange: nil)), expectedPosition, "\(context): atom subrun at UTF16 \(offset)")
            }
        }
        XCTAssertEqual(Set(mentionPositions).count, Self.retainedMentionLabels.count, "\(context): adjacent same-type atoms must remain distinct")
        try preserveValidatedRetainedChildIndexCoverage(from: input.textStorage, in: reference.textStorage, context: context)
        assertRetainedSnapshotMetadata(input.textStorage, matches: reference.textStorage)
        assertRetainedSnapshotMetadata(input.lastAuthorizedAttributedTextStorage, matches: input.textStorage)
        assertTextKitAttributedStorage(in: input, matches: reference.textStorage, compareStyledValues: true)
        assertTextKitAttributedStorage(in: input, matches: input.lastAuthorizedAttributedTextStorage, compareStyledValues: true)
        print("EDITOR_PUBLIC_MIXED_ATOM_FRAME context=\(context) old=\(priorRevision) new=\(snapshot.documentRevision) start=\(start) deleted=\(deleted) inserted=\(inserted.count) retainedRequired=\(requiresRetainedAtoms) usedPatch=\(input.lastRenderAppliedPatch())")
    }

    func testPublicPrefixInputKeepsAdjacentMixedAtomMetadataAndAuthorizedRollback() throws {
        XCTAssertEqual(PublicMixedAtomFixture.originalFragment.unicodeScalars.count,
            PublicMixedAtomFixture.replacementFragment.unicodeScalars.count)
        XCTAssertNotEqual(PublicMixedAtomFixture.originalFragment.utf16.count,
            PublicMixedAtomFixture.replacementFragment.utf16.count)
        try withPublicMixedAtomEditor { editorId, view, adapter in
            let input = view.textView
            for (oldFragment, nextFragment) in [
                (PublicMixedAtomFixture.originalFragment, PublicMixedAtomFixture.replacementFragment),
                (PublicMixedAtomFixture.replacementFragment, PublicMixedAtomFixture.originalFragment)
            ] {
                setSelection(in: input, utf16Range: NSRange(location: PublicMixedAtomFixture.prefixLead.utf16.count,
                    length: oldFragment.utf16.count))
                flushMainQueue()
                try assertPublicMixedAtomAction(in: input, adapter: adapter, context: "replace-\(oldFragment)-with-\(nextFragment)",
                    expectedFragment: nextFragment,
                    expectedCaret: (PublicMixedAtomFixture.prefixLead + nextFragment).utf16.count,
                    requiresRetainedAtoms: true, expectsPatch: true) { input.insertText(nextFragment) }
            }
            let prefixCaret = (PublicMixedAtomFixture.prefixLead + PublicMixedAtomFixture.originalFragment).utf16.count
            setCollapsedSelection(in: input, utf16Offset: prefixCaret)
            flushMainQueue()
            let extendedFragment = PublicMixedAtomFixture.originalFragment + PublicMixedAtomFixture.replacementFragment
            try assertPublicMixedAtomAction(in: input, adapter: adapter, context: "extent-changing-insert",
                expectedFragment: extendedFragment, expectedCaret: prefixCaret + PublicMixedAtomFixture.replacementFragment.utf16.count,
                requiresRetainedAtoms: false, expectsPatch: false) { input.insertText(PublicMixedAtomFixture.replacementFragment) }
            try assertPublicMixedAtomAction(in: input, adapter: adapter, context: "actual-grapheme-delete",
                expectedFragment: PublicMixedAtomFixture.originalFragment, expectedCaret: prefixCaret,
                requiresRetainedAtoms: false, expectsPatch: false) { input.deleteBackward() }

            let document = EditorV2Shadow.getJson(id: editorId)
            let authorized = NSAttributedString(attributedString: input.lastAuthorizedAttributedTextStorage)
            input.performTransientTextMutation {
                input.textStorage.replaceCharacters(in: NSRange(location: input.textStorage.length, length: 0),
                    with: PublicMixedAtomFixture.transientSuffix)
            }
            XCTAssertEqual(input.textStorage.string, authorized.string + PublicMixedAtomFixture.transientSuffix,
                "The rollback control must actually mutate mounted text")
            input.restoreAuthorizedTextSnapshot()
            XCTAssertEqual(input.textStorage.length, authorized.length)
            XCTAssertTrue((input.textStorage.string as NSString).isEqual(to: authorized.string))
            assertTextKitAttributedStorage(in: input, matches: authorized, compareStyledValues: true)
            assertRetainedSnapshotMetadata(input.textStorage, matches: authorized)
            XCTAssertEqual(EditorV2Shadow.getJson(id: editorId), document, "Rollback must not edit the genuine engine document")
            assertRetainedSnapshotMetadata(input.lastAuthorizedAttributedTextStorage, matches: input.textStorage)
            XCTAssertEqual(input.selectedRange, NSRange(location: prefixCaret, length: 0))
        }
    }

    func testPublicPrefixInputRecoversMalformedRetainedAtomPositionBeforePublication() throws {
        try withPublicMixedAtomEditor { _, view, adapter in
            let input = view.textView
            setSelection(in: input, utf16Range: NSRange(location: PublicMixedAtomFixture.prefixLead.utf16.count,
                length: PublicMixedAtomFixture.originalFragment.utf16.count))
            flushMainQueue()
            let label = Self.retainedMentionLabels[0]
            let labelRange = (input.textStorage.string as NSString).range(of: label)
            guard labelRange.location != NSNotFound else { XCTFail("expected the actual first mention label"); return }
            let subrun = NSRange(location: labelRange.location + label.utf16.count / 2, length: 1)
            input.textStorage.addAttributes([
                RenderBridgeAttributes.docPos: NSNumber(value: true), .foregroundColor: UIColor.systemRed
            ], range: subrun)
            XCTAssertEqual(EditorV2Adapter.exactBool(input.textStorage.attribute(RenderBridgeAttributes.docPos,
                at: subrun.location, effectiveRange: nil)), true, "The live fault must be a real boolean metadata subrun")
            XCTAssertNil(RenderBridge.jsonUInt32(input.textStorage.attribute(RenderBridgeAttributes.docPos,
                at: subrun.location, effectiveRange: nil)))
            XCTAssertNotNil(RenderBridge.jsonUInt32(input.textStorage.attribute(RenderBridgeAttributes.docPos,
                at: labelRange.location, effectiveRange: nil)), "The surrounding atom metadata must remain valid")
            var appliedStorages: [NSAttributedString] = []
            XCTAssertNil(input.onApplyingRustTextForTesting)
            input.onApplyingRustTextForTesting = {
                appliedStorages.append(NSAttributedString(attributedString: input.textStorage))
            }
            try assertPublicMixedAtomAction(in: input, adapter: adapter, context: "malformed-live-position-subrun",
                expectedFragment: PublicMixedAtomFixture.replacementFragment,
                expectedCaret: (PublicMixedAtomFixture.prefixLead + PublicMixedAtomFixture.replacementFragment).utf16.count,
                requiresRetainedAtoms: true, expectsPatch: false) { input.insertText(PublicMixedAtomFixture.replacementFragment) }
            input.onApplyingRustTextForTesting = nil
            let firstApplied = try XCTUnwrap(appliedStorages.first)
            let firstLabelRange = (firstApplied.string as NSString).range(of: label)
            guard firstLabelRange.location != NSNotFound else { XCTFail("expected first live apply to retain the actual mention"); return }
            let firstSubrunOffset = firstLabelRange.location + label.utf16.count / 2
            XCTAssertEqual(EditorV2Adapter.exactBool(firstApplied.attribute(RenderBridgeAttributes.docPos,
                at: firstSubrunOffset, effectiveRange: nil)), true,
                "The prefix patch must actually reach the retained malformed subrun before full recovery")
            let lastApplied = try XCTUnwrap(appliedStorages.last)
            assertRetainedSnapshotMetadata(lastApplied, matches: input.textStorage)
            assertTextKitAttributedStorage(in: input, matches: lastApplied, compareStyledValues: true)
            XCTAssertNotNil(RenderBridge.jsonUInt32(input.textStorage.attribute(RenderBridgeAttributes.docPos,
                at: (input.textStorage.string as NSString).range(of: label).location, effectiveRange: nil)))
            setSelection(in: input, utf16Range: NSRange(location: PublicMixedAtomFixture.prefixLead.utf16.count,
                length: PublicMixedAtomFixture.replacementFragment.utf16.count))
            flushMainQueue()
            try assertPublicMixedAtomAction(in: input, adapter: adapter, context: "native-continuation-after-live-recovery",
                expectedFragment: PublicMixedAtomFixture.originalFragment,
                expectedCaret: (PublicMixedAtomFixture.prefixLead + PublicMixedAtomFixture.originalFragment).utf16.count,
                requiresRetainedAtoms: true, expectsPatch: true) { input.insertText(PublicMixedAtomFixture.originalFragment) }
        }
    }

    func testAdapterRenderUpdatesShareParsedRenderBlocksWithTheTextView() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: editorId))
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        defer {
            _ = view.textView.resignFirstResponder()
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.editorId = editorId
        view.setContent(html: (0..<12).map { "<p>Paragraph \($0) keeps 😀 text</p>" }.joined())

        func assertSharedBlocks(_ phase: String) throws {
            let adopted = try XCTUnwrap(adapter.cachedSemanticRenderBlocks, phase)
            let retained = try XCTUnwrap(view.textView.currentRenderBlocks, phase)
            XCTAssertEqual(retained.count, adopted.count, phase)
            for (index, block) in retained.enumerated() {
                XCTAssertEqual(
                    block.withUnsafeBufferPointer { UnsafeRawPointer($0.baseAddress) },
                    adopted[index].withUnsafeBufferPointer { UnsafeRawPointer($0.baseAddress) },
                    "\(phase): block \(index) must reuse the adapter's parsed elements instead of a second copy"
                )
            }
        }

        try assertSharedBlocks("load")
        XCTAssertTrue(view.textView.becomeFirstResponder())
        setCollapsedSelection(in: view.textView, utf16Offset: "Paragraph 0".utf16.count)
        view.textView.insertText("!")
        try assertSharedBlocks("typing patch")

        let detachedCopy = try XCTUnwrap(String(data: Data(try XCTUnwrap(adapter.cachedViewUpdate?.updateJSON).utf8), encoding: .utf8))
        let textBefore = view.textView.textStorage.string
        XCTAssertTrue(view.textView.applyUpdateJSON(detachedCopy, notifyDelegate: false),
            "An equal update that did not come from the adapter must still be parsed and applied")
        XCTAssertEqual(view.textView.textStorage.string, textBefore)
        XCTAssertTrue(view.textView.textStorage.string.hasPrefix("Paragraph 0! keeps 😀 text"))
    }
}
