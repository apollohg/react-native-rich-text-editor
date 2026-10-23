import CoreText
import XCTest

extension RenderBridgeTests {
    private struct FontResolutionRun {
        let text: String
        let marks: [Any]
        let fontName: String
        let fontSize: CGFloat
    }

    private struct BlockFontExpectation {
        let runs: [FontResolutionRun]
        let rootIndex: Int
        let indent: CGFloat
    }

    private func fontResolutionRuns(
        prefix: String,
        fontSize: CGFloat,
        quoted: Bool = false,
        heading: Bool = false
    ) -> [FontResolutionRun] {
        let faces = quoted
            ? ["Courier-BoldOblique", "Courier-Oblique", "Courier-Oblique", "Courier-BoldOblique", "Georgia"]
            : heading
                ? ["Courier-Bold", "Courier-Bold", "Courier-BoldOblique", "Courier-BoldOblique", "Courier-Bold"]
                : ["Courier-Bold", "Courier", "Courier-Oblique", "Courier-BoldOblique", "Courier"]
        let names = ["bold", "plain", "italic", "combined", "link"]
        let marks: [[Any]] = [["bold"], [], ["italic"], ["bold", "italic"],
            [["type": "link", "href": "https://example.com/fonts"]]]
        return names.indices.map { index in
            FontResolutionRun(text: "\(prefix)-\(names[index]) café e\u{301} 東京 👩‍💻 ",
                marks: marks[index], fontName: faces[index], fontSize: fontSize)
        }
    }

    private func assertResolvedFont(
        _ font: UIFont?,
        matches run: FontResolutionRun,
        file: StaticString = #filePath,
        line: UInt = #line
    ) throws {
        let actual = try XCTUnwrap(font, "Missing font for \(run.text)", file: file, line: line)
        let expected = try XCTUnwrap(UIFont(name: run.fontName, size: run.fontSize),
            "The font fixture must resolve its registered face", file: file, line: line)
        XCTAssertEqual(actual.fontName, expected.fontName, run.text, file: file, line: line)
        XCTAssertEqual(actual.fontDescriptor.symbolicTraits, expected.fontDescriptor.symbolicTraits,
            run.text, file: file, line: line)
        XCTAssertEqual(actual.pointSize, expected.pointSize, run.text, file: file, line: line)
        XCTAssertEqual(actual.ascender, expected.ascender, run.text, file: file, line: line)
        XCTAssertEqual(actual.descender, expected.descender, run.text, file: file, line: line)
    }

    func testRepeatedBlockTypographyResolvesFontOncePerAncestryInOneRender() throws {
        let paragraphCount = 8
        let quotedParagraphCount = 2
        let uniqueAncestryCount = 4
        let paragraphFontSize: CGFloat = 19
        let quotedFontSize: CGFloat = 21
        let headingFontSize: CGFloat = 23
        let codeFontSize: CGFloat = 17
        let lineHeight: CGFloat = 30
        let quoteBorderWidth: CGFloat = 4
        let quotePadding: CGFloat = 16
        let letterSpacing: CGFloat = 1
        let theme = EditorTheme(dictionary: ["version": 1, "styles": [
            "text": ["fontFamily": "Courier", "fontSize": paragraphFontSize,
                "lineHeight": lineHeight, "letterSpacing": letterSpacing, "color": "#123456ff"],
            "paragraph": ["marginTop": 0, "marginBottom": 0],
            "blockquote": ["fontSize": quotedFontSize, "fontStyle": "italic",
                "borderLeftWidth": quoteBorderWidth, "paddingLeft": quotePadding],
            "h2": ["fontSize": headingFontSize, "marginTop": 0, "marginBottom": 0],
            "codeBlock": ["fontFamily": "Courier", "fontSize": codeFontSize,
                "paddingLeft": 0, "paddingRight": 0, "paddingTop": 0, "paddingBottom": 0,
                "marginTop": 0, "marginBottom": 0],
            "link": ["color": "#0000ffff"]
        ], "rules": [["path": ["blockquote", "paragraph", "link"],
            "style": ["fontFamily": "Georgia", "fontStyle": "normal"]],
            ["path": ["blockquote", "paragraph"], "style": ["letterSpacing": 0]]]])
        var blocks: [[[String: Any]]] = []
        var expectations: [BlockFontExpectation] = []
        func appendBlock(
            _ nodeType: String,
            prefix: String,
            fontSize: CGFloat,
            quoted: Bool = false,
            heading: Bool = false
        ) {
            let runs = fontResolutionRuns(prefix: prefix, fontSize: fontSize, quoted: quoted, heading: heading)
            var elements: [[String: Any]] = []
            if quoted { elements.append(["type": "blockStart", "nodeType": "blockquote", "depth": 0]) }
            elements.append(["type": "blockStart", "nodeType": nodeType, "depth": quoted ? 1 : 0])
            elements.append(contentsOf: runs.map { ["type": "textRun", "text": $0.text, "marks": $0.marks] })
            elements.append(["type": "blockEnd"])
            if quoted { elements.append(["type": "blockEnd"]) }
            expectations.append(BlockFontExpectation(runs: runs, rootIndex: blocks.count,
                indent: quoted ? quoteBorderWidth + quotePadding : 0))
            blocks.append(elements)
        }
        for index in 0..<paragraphCount {
            appendBlock("paragraph", prefix: "paragraph-\(index)", fontSize: paragraphFontSize)
        }
        for index in 0..<quotedParagraphCount {
            appendBlock("paragraph", prefix: "quote-\(index)", fontSize: quotedFontSize, quoted: true)
        }
        appendBlock("h2", prefix: "heading", fontSize: headingFontSize, heading: true)
        appendBlock("codeBlock", prefix: "code", fontSize: codeFontSize)
        var resolutionCount = 0
        let previousObserver = PreparedProseInstrumentation.tableStageObserverForTesting
        PreparedProseInstrumentation.tableStageObserverForTesting = { stage, start, end in
            if stage == .editorBlockFontResolution { resolutionCount += 1 }
            previousObserver?(stage, start, end)
        }
        defer { PreparedProseInstrumentation.tableStageObserverForTesting = previousObserver }
        let rendered = RenderBridge.renderBlocks(fromArray: blocks as [[NSDictionary]],
            baseFont: baseFont, textColor: textColor, theme: theme)
        let text = rendered.string as NSString
        XCTAssertEqual(rendered.string, expectations.map { $0.runs.map(\.text).joined() }.joined(separator: "\n"))
        for expectation in expectations {
            let firstRun = try XCTUnwrap(expectation.runs.first)
            let blockStart = text.range(of: firstRun.text).location
            XCTAssertNotEqual(blockStart, NSNotFound)
            guard blockStart != NSNotFound else { continue }
            let tagOffset = expectation.rootIndex == 0 ? blockStart : blockStart - 1
            XCTAssertEqual((rendered.attribute(RenderBridgeAttributes.topLevelChildIndex,
                at: tagOffset, effectiveRange: nil) as? NSNumber)?.intValue, expectation.rootIndex,
                "Sparse root metadata must remain on the leading character or actual separator")
            for run in expectation.runs {
                let range = text.range(of: run.text)
                XCTAssertNotEqual(range.location, NSNotFound, "The complete mixed font run must remain present")
                guard range.location != NSNotFound else { continue }
                let attributes = rendered.attributes(at: range.location, effectiveRange: nil)
                try assertResolvedFont(attributes[.font] as? UIFont, matches: run)
                if range.location != 0 {
                    XCTAssertNil(attributes[RenderBridgeAttributes.topLevelChildIndex],
                        "Font reuse must not spread sparse root metadata over unrelated text runs")
                }
                let paragraph = try XCTUnwrap(attributes[.paragraphStyle] as? NSParagraphStyle)
                XCTAssertEqual(paragraph.minimumLineHeight, lineHeight, run.text)
                XCTAssertEqual(paragraph.maximumLineHeight, lineHeight, run.text)
                XCTAssertEqual(paragraph.headIndent, expectation.indent, run.text)
                XCTAssertEqual(paragraph.firstLineHeadIndent, expectation.indent, run.text)
                XCTAssertEqual(paragraph.tailIndent, 0, run.text)
                XCTAssertEqual(attributes[.kern] as? CGFloat, expectation.indent > 0 ? 0 : letterSpacing, run.text)
                let linked = !run.marks.isEmpty && run.marks.first is [String: Any]
                XCTAssertEqual(attributes[.foregroundColor] as? UIColor,
                    EditorTheme.color(from: linked ? "#0000ffff" : "#123456ff"), run.text)
                XCTAssertEqual(attributes[RenderBridgeAttributes.linkHref] as? String,
                    linked ? "https://example.com/fonts" : nil, run.text)
            }
        }
        print("EDITOR_BLOCK_FONT_RESOLUTION count=\(resolutionCount) expected=\(uniqueAncestryCount) blocks=\(blocks.count) runs=\(expectations.reduce(0) { $0 + $1.runs.count }) UTF16=\(rendered.length)")
        XCTAssertGreaterThan(resolutionCount, 0, "The probe must capture actual font resolver work")
        XCTAssertEqual(resolutionCount, uniqueAncestryCount,
            "Repeated rich blocks must reuse only their immutable unmarked font resolution; actual=\(resolutionCount), distinct ancestries=\(uniqueAncestryCount)")
    }

    func testBlockTypographyReresolvesChangedFontsThemesAndEnvironmentBetweenRenders() throws {
        let elements: [[String: Any]] = [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "bold", "marks": ["bold"]],
            ["type": "textRun", "text": " plain", "marks": []],
            ["type": "blockEnd"]
        ]
        let firstSize: CGFloat = 19
        let secondSize: CGFloat = 24
        for (family, size, versioned) in [("Courier", firstSize, false), ("Georgia", secondSize, false),
            ("Courier", firstSize, true), ("Georgia", secondSize, true)] {
            let fallback = try XCTUnwrap(UIFont(name: family, size: size))
            let theme = versioned ? EditorTheme(dictionary: ["version": 1,
                "styles": ["text": ["fontFamily": family, "fontSize": size]]]) : nil
            ViewerFontEnvironment.shared.invalidateRegisteredFonts()
            let rendered = RenderBridge.renderElements(fromArray: elements,
                baseFont: fallback, textColor: .brown, theme: theme)
            XCTAssertEqual(rendered.string, "bold plain")
            let marked = try XCTUnwrap(rendered.attribute(.font, at: 0, effectiveRange: nil) as? UIFont)
            let plain = try XCTUnwrap(rendered.attribute(.font, at: "bold".utf16.count, effectiveRange: nil) as? UIFont)
            XCTAssertEqual(plain.fontName, fallback.fontName)
            XCTAssertEqual(plain.pointSize, size)
            XCTAssertEqual(marked.familyName, fallback.familyName)
            XCTAssertEqual(marked.pointSize, size)
            XCTAssertTrue(marked.fontDescriptor.symbolicTraits.contains(.traitBold))
            XCTAssertFalse(plain.fontDescriptor.symbolicTraits.contains(.traitBold),
                "A preceding marked run must not contaminate the cached unmarked block font")
            XCTAssertEqual(rendered.attribute(.foregroundColor, at: 0, effectiveRange: nil) as? UIColor, .brown)
        }
    }

    func testBlockTypographyKeepsMarkerFontAndWidthFreshAcrossEqualListAncestry() throws {
        let markerFontSize: CGFloat = 17
        let markedFontSize: CGFloat = 25
        let markerGap: CGFloat = 6
        let indices = [9, 10]
        let theme = EditorTheme(dictionary: ["version": 1, "styles": [
            "text": ["fontFamily": "Courier", "fontSize": markerFontSize],
            "orderedList": ["indent": 0, "baseIndentMultiplier": 0],
            "paragraph": ["marginTop": 0, "marginBottom": 0],
            "listItem": ["marginTop": 0, "marginBottom": 0],
            "listMarker": ["gap": markerGap], "bold": ["fontSize": markedFontSize]
        ]])
        let elements: [[String: Any]] = indices.flatMap { index in
            [
                ["type": "blockStart", "nodeType": "listItem", "depth": 0,
                    "listContext": ["ordered": true, "index": index,
                        "isFirst": index == indices.first, "isLast": index == indices.last]],
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                ["type": "textRun", "text": "item-\(index)", "marks": ["bold"]],
                ["type": "textRun", "text": " plain", "marks": []],
                ["type": "blockEnd"], ["type": "blockEnd"]
            ]
        }
        let rendered = RenderBridge.renderElements(fromArray: elements,
            baseFont: baseFont, textColor: textColor, theme: theme)
        let markerFont = try XCTUnwrap(UIFont(name: "Courier", size: markerFontSize))
        var widths: [CGFloat] = []
        for index in indices {
            let range = (rendered.string as NSString).range(of: "item-\(index)")
            XCTAssertNotEqual(range.location, NSNotFound)
            guard range.location != NSNotFound else { continue }
            let attributes = rendered.attributes(at: range.location, effectiveRange: nil)
            let marker = try XCTUnwrap(attributes[RenderBridgeAttributes.listMarkerBaseFont] as? UIFont)
            XCTAssertEqual(marker.fontName, markerFont.fontName)
            XCTAssertEqual(marker.pointSize, markerFontSize,
                "A larger bold text run must not replace the list's unmarked marker font")
            XCTAssertEqual((attributes[.font] as? UIFont)?.pointSize, markedFontSize)
            let label = "\(index)."
            XCTAssertEqual(attributes[RenderBridgeAttributes.orderedListMarkerLabel] as? String, label)
            let width = ceil((label as NSString).size(withAttributes: [.font: markerFont]).width) + markerGap
            XCTAssertEqual(attributes[RenderBridgeAttributes.listMarkerWidth] as? CGFloat, width)
            XCTAssertEqual((attributes[.paragraphStyle] as? NSParagraphStyle)?.headIndent, width)
            widths.append(width)
        }
        XCTAssertEqual(widths.count, indices.count)
        XCTAssertGreaterThan(try XCTUnwrap(widths.last), try XCTUnwrap(widths.first),
            "Equal ancestry must still reserve the wider current two-digit ordered marker")
    }

    func testBlockTypographyPreservesCustomAndMissingFamilyEmphasisFallback() throws {
        let fontSize: CGFloat = 19
        let fallback = try XCTUnwrap(UIFont(name: "Courier", size: fontSize))
        let missingFamily = "font-resolution-missing-\(UUID().uuidString)"
        let elements: [[String: Any]] = [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "both", "marks": ["bold", "italic"]],
            ["type": "textRun", "text": " plain", "marks": []], ["type": "blockEnd"]
        ]
        for family in [missingFamily, "AppleColorEmoji"] {
            let theme = EditorTheme(dictionary: ["version": 1,
                "styles": ["text": ["fontFamily": family, "fontSize": fontSize]]])
            let rendered = RenderBridge.renderElements(fromArray: elements,
                baseFont: fallback, textColor: textColor, theme: theme)
            let marked = try XCTUnwrap(rendered.attribute(.font, at: 0, effectiveRange: nil) as? UIFont)
            let plain = try XCTUnwrap(rendered.attribute(.font, at: "both".utf16.count, effectiveRange: nil) as? UIFont)
            XCTAssertTrue(marked.fontDescriptor.symbolicTraits.contains(.traitBold))
            XCTAssertTrue(marked.fontDescriptor.symbolicTraits.contains(.traitItalic))
            XCTAssertEqual(marked.pointSize, fontSize)
            XCTAssertEqual(plain.fontName, family == missingFamily ? fallback.fontName : family)
            XCTAssertFalse(plain.fontDescriptor.symbolicTraits.contains(.traitBold))
            XCTAssertFalse(plain.fontDescriptor.symbolicTraits.contains(.traitItalic))
            if family == "AppleColorEmoji" {
                XCTAssertNotEqual(marked.familyName, plain.familyName,
                    "A family without the requested emphasis must retain the complete existing fallback contract")
            }
        }
        XCTAssertTrue(ViewerFontEnvironment.shared.hasMissingFamilyWarning(missingFamily,
            semanticGeneration: "editor-stylesheet"), "The real missing-family resolver must still run")
    }

    func testBlockTypographyMatchesNativeFontsWrappedGlyphsCaretAndPixels() throws {
        let fontSize: CGFloat = 19
        let lineHeight: CGFloat = 30
        let viewport = CGSize(width: 180, height: 600)
        let bitmapSize = CGSize(width: 180, height: 600)
        let runs = fontResolutionRuns(prefix: "native", fontSize: fontSize)
        let theme = EditorTheme(dictionary: ["version": 1, "styles": [
            "text": ["fontFamily": "Courier", "fontSize": fontSize,
                "lineHeight": lineHeight, "color": "#000000ff"],
            "paragraph": ["marginTop": 0, "marginBottom": 0],
            "link": ["color": "#0000ffff"]
        ]])
        let elements: [[String: Any]] = [["type": "blockStart", "nodeType": "paragraph", "depth": 0]]
            + runs.map { ["type": "textRun", "text": $0.text, "marks": $0.marks] }
            + [["type": "blockEnd"]]
        let rendered = RenderBridge.renderElements(fromArray: elements,
            baseFont: baseFont, textColor: textColor, theme: theme)
        let expected = NSMutableAttributedString()
        let paragraph = NSMutableParagraphStyle()
        paragraph.minimumLineHeight = lineHeight
        paragraph.maximumLineHeight = lineHeight
        for run in runs {
            let linked = run.marks.first is [String: Any]
            var attributes: [NSAttributedString.Key: Any] = [
                .font: try XCTUnwrap(UIFont(name: run.fontName, size: fontSize)),
                .foregroundColor: linked ? UIColor.blue : UIColor.black, .paragraphStyle: paragraph
            ]
            if linked { attributes[.underlineStyle] = NSUnderlineStyle.single.rawValue }
            expected.append(NSAttributedString(string: run.text, attributes: attributes))
        }
        var bitmaps: [Data] = []
        var lineRects: [[CGRect]] = []
        var caretRects: [CGRect] = []
        let caretOffset = runs.prefix(3).reduce(0) { $0 + $1.text.utf16.count }
        for attributed in [rendered, expected] {
            let view = EditorTextView(frame: CGRect(origin: .zero, size: viewport), textContainer: nil)
            view.textContainerInset = .zero
            view.textContainer.lineFragmentPadding = 0
            view.textStorage.setAttributedString(attributed)
            view.layoutIfNeeded()
            view.layoutManager.ensureLayout(for: view.textContainer)
            XCTAssertEqual(view.textStorage.string, expected.string)
            let glyphRange = NSRange(location: 0, length: view.layoutManager.numberOfGlyphs)
            var lines: [CGRect] = []
            view.layoutManager.enumerateLineFragments(forGlyphRange: glyphRange) { _, used, _, _, _ in
                lines.append(used)
            }
            XCTAssertGreaterThan(lines.count, 1, "The custom rich-font fixture must wrap in native TextKit")
            lineRects.append(lines)
            let position = try XCTUnwrap(view.position(from: view.beginningOfDocument, offset: caretOffset))
            caretRects.append(view.caretRect(for: position))
            bitmaps.append(try renderGlyphBitmap(view.layoutManager, glyphRange: glyphRange,
                origin: .zero, size: bitmapSize))
        }
        XCTAssertTrue(layoutRectsMatch(lineRects[0], lineRects[1]),
            "Actual rendered typography must match independent native font metrics: \(lineRects[0]) vs \(lineRects[1])")
        XCTAssertTrue(layoutRectsMatch([caretRects[0]], [caretRects[1]]),
            "Font reuse must retain the native caret after mixed emphasis and Unicode: \(caretRects[0]) vs \(caretRects[1])")
        XCTAssertEqual(bitmaps[0], bitmaps[1], "Custom fonts, Unicode fallback, emphasis and link pixels must match native reference text")
        XCTAssertTrue(bitmaps[0].contains { $0 != 0 }, "Pixel equality requires actual painted text")
    }
}

extension RenderBridgeTests {
    func testStyledBlockAttributesDistributeDistinctContainerIdentitiesAcrossHashes() throws {
        let paragraphCount = 32
        let minimumDistinctHashes = paragraphCount / 2
        let theme = EditorTheme(dictionary: [
            "version": 1, "styles": ["paragraph": ["marginTop": 0, "marginBottom": 0]]
        ])
        let elements: [[String: Any]] = (0..<paragraphCount).flatMap { index in
            [
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                ["type": "textRun", "text": "Paragraph \(index) café e\u{301} 東京 😀 ", "marks": []],
                ["type": "textRun", "text": "bold", "marks": ["bold"]],
                ["type": "textRun", "text": " italic", "marks": ["italic"]],
                ["type": "blockEnd"]
            ]
        }
        let rendered = RenderBridge.renderElements(fromArray: elements,
            baseFont: baseFont, textColor: textColor, theme: theme)
        var identities = Set<ObjectIdentifier>()
        var hashBuckets: [Int: Set<ObjectIdentifier>] = [:]
        rendered.enumerateAttribute(editorStyleBoxesAttribute,
            in: NSRange(location: 0, length: rendered.length)) { value, range, _ in
            guard let value else {
                XCTAssertEqual((rendered.string as NSString).substring(with: range), "\n",
                    "Only the inter-block separator may omit container attributes")
                XCTAssertNotNil(rendered.attribute(RenderBridgeAttributes.blockBoundary,
                    at: range.location, effectiveRange: nil))
                return
            }
            guard let boxes = renderedStyleBoxes(from: value), let box = boxes.first,
                  let attribute = value as? NSObject else {
                XCTFail("Styled paragraph is missing its container attribute at \(range)")
                return
            }
            XCTAssertEqual(boxes.count, 1, "Each top-level paragraph must retain its own box")
            let identity = ObjectIdentifier(box)
            identities.insert(identity)
            hashBuckets[attribute.hash, default: []].insert(identity)
            XCTAssertEqual(box.depth, 0)
            XCTAssertEqual(box.topInset, 0)
            XCTAssertEqual(box.bottomInset, 0)
        }
        XCTAssertEqual(identities.count, paragraphCount, "Equal visual styles must not merge separate containers")
        print("EDITOR_STYLE_BOX_HASH_DISTRIBUTION identities=\(identities.count) distinctHashes=\(hashBuckets.count) largestBucket=\(hashBuckets.values.map(\.count).max() ?? 0)")
        XCTAssertGreaterThanOrEqual(hashBuckets.count, minimumDistinctHashes,
            "Distinct container stacks must not all collide in Foundation's attributed-string dictionary cache")
    }

    func testStyledBoxStacksCoalesceOnlyOrderedIdentitiesAndKeepMutableInsetsVisible() throws {
        let box = EditorStyleBox(["paddingTop": 12, "paddingBottom": 9, "borderLeftWidth": 4])
        let parent = EditorRenderedBox(box: box, depth: 0, leading: 0, trailing: 0)
        let child = EditorRenderedBox(box: box, depth: 1, leading: 4, trailing: 0)
        let stack = EditorRenderedBoxStack(boxes: [parent, child])
        let sameIdentities = EditorRenderedBoxStack(boxes: [parent, child])
        let reversed = EditorRenderedBoxStack(boxes: [child, parent])
        let freshChild = EditorRenderedBox(box: box, depth: child.depth,
            leading: child.leading, trailing: child.trailing)
        let differentContainer = EditorRenderedBoxStack(boxes: [parent, freshChild])
        XCTAssertEqual(stack.hash, sameIdentities.hash, "Equal identity sequences must satisfy Foundation's hash contract")
        XCTAssertTrue(stack.isEqual(sameIdentities))
        XCTAssertFalse(stack.isEqual(reversed), "Changing ancestor order must keep a distinct attribute run")
        XCTAssertFalse(stack.isEqual(differentContainer), "Equal visual values must not merge separate containers")

        let storage = NSMutableAttributedString(string: "ab")
        let firstCharacter = NSRange(location: 0, length: 1)
        let secondCharacter = NSRange(location: 1, length: 1)
        storage.addAttribute(editorStyleBoxesAttribute, value: stack, range: firstCharacter)
        storage.addAttribute(editorStyleBoxesAttribute, value: sameIdentities, range: secondCharacter)
        var effectiveRange = NSRange()
        _ = storage.attribute(editorStyleBoxesAttribute, at: 0, effectiveRange: &effectiveRange)
        XCTAssertEqual(effectiveRange, NSRange(location: 0, length: storage.length),
            "Foundation must coalesce separately allocated stacks with the same descriptor identities")

        let originalHash = stack.hash
        let collapsedTopInset: CGFloat = 13
        let collapsedBottomInset: CGFloat = 7
        parent.topInset = collapsedTopInset
        child.bottomInset = collapsedBottomInset
        XCTAssertEqual(stack.hash, originalHash, "Margin-collapse mutations must not invalidate the attributed-string cache key")
        XCTAssertTrue(stack.isEqual(sameIdentities))
        let mounted = try XCTUnwrap(renderedStyleBoxes(from:
            storage.attribute(editorStyleBoxesAttribute, at: 0, effectiveRange: nil)))
        XCTAssertTrue(mounted[0] === parent)
        XCTAssertTrue(mounted[1] === child)
        XCTAssertEqual(mounted[0].topInset, collapsedTopInset)
        XCTAssertEqual(mounted[1].bottomInset, collapsedBottomInset)
        XCTAssertTrue(stack.renderedBoxes.object(at: 0) as? EditorRenderedBox === parent,
            "The Objective-C mounted-style oracle must receive the actual descriptors")
        XCTAssertTrue(try XCTUnwrap(renderedStyleBoxes(from: [parent, child]))[1] === child,
            "Existing raw-array attributes remain readable")

        storage.addAttribute(editorStyleBoxesAttribute, value: reversed, range: secondCharacter)
        _ = storage.attribute(editorStyleBoxesAttribute, at: 0, effectiveRange: &effectiveRange)
        XCTAssertEqual(effectiveRange, firstCharacter, "Reordered containers must not coalesce")
        storage.addAttribute(editorStyleBoxesAttribute, value: differentContainer, range: secondCharacter)
        _ = storage.attribute(editorStyleBoxesAttribute, at: 1, effectiveRange: &effectiveRange)
        XCTAssertEqual(effectiveRange, secondCharacter, "Distinct sibling containers must retain separate runs")
    }

    func testRender_contextualOuterListIndentUsesItsOwnPrefix() {
        let theme = EditorTheme(dictionary: [
            "version": 1, "styles": [:],
            "rules": [["path": ["blockquote", "bulletList"], "style": ["indent": 50]]]
        ])
        let stack = [
            BlockContext(nodeType: "blockquote", depth: 0, listContext: nil),
            BlockContext(nodeType: "bulletList", depth: 1, listContext: nil),
            BlockContext(nodeType: "listItem", depth: 1, listContext: ["ordered": false]),
            BlockContext(nodeType: "orderedList", depth: 2, listContext: nil),
            BlockContext(nodeType: "listItem", depth: 2, listContext: ["ordered": true, "index": 1]),
            BlockContext(nodeType: "paragraph", depth: 2, listContext: nil)
        ]
        let context = RenderBridge.effectiveBlockContext(stack)!
        let baseline = RenderBridge.paragraphStyleForBlock(context, blockStack: stack, theme: EditorTheme(dictionary: ["version": 1, "styles": [:]]), baseFont: baseFont)
        let resolved = RenderBridge.paragraphStyleForBlock(context, blockStack: stack, theme: theme, baseFont: baseFont)
        XCTAssertEqual(resolved.headIndent - baseline.headIndent, 26)
    }

    func testRender_contextualBoxesPreserveListContainers() throws {
        let theme = EditorTheme(dictionary: [
            "version": 1, "styles": ["paragraph": ["marginBottom": 8]], "rules": [
                ["path": ["listItem", "paragraph"], "style": ["marginBottom": 0]],
                ["path": ["blockquote", "bulletList"], "style": ["paddingLeft": 13]],
                ["path": ["listItem", "listMarker"], "style": ["scale": 1, "gap": 12, "color": "#ff0000ff"]],
                ["path": ["paragraph", "listMarker"], "style": ["gap": 99]],
                ["path": ["blockquote", "bulletList", "listItem", "paragraph"], "style": ["paddingLeft": 7]]
            ]
        ])
        let result = RenderBridge.renderElements(fromJSON: """
        [
        {"type":"blockStart","nodeType":"blockquote","depth":0},
        {"type":"blockStart","nodeType":"listItem","depth":1,"listContext":{"ordered":false,"index":1,"isFirst":true,"isLast":true}},
        {"type":"blockStart","nodeType":"paragraph","depth":1},
        {"type":"textRun","text":"nested","marks":["bold"]},
        {"type":"blockEnd"},{"type":"blockEnd"},{"type":"blockEnd"},
        {"type":"blockStart","nodeType":"paragraph","depth":0},
        {"type":"textRun","text":"top","marks":[]},{"type":"blockEnd"}
        ]
        """, baseFont: baseFont, textColor: textColor, theme: theme)
        let nested = try XCTUnwrap(renderedStyleBoxes(from: result.attribute(editorStyleBoxesAttribute, at: 0, effectiveRange: nil)))
        let top = try XCTUnwrap(renderedStyleBoxes(from: result.attribute(editorStyleBoxesAttribute, at: (result.string as NSString).range(of: "top").location, effectiveRange: nil)))
        XCTAssertEqual(nested.last?.box.margin.bottom, 0)
        XCTAssertEqual(top.last?.box.margin.bottom, 8)
        XCTAssertEqual(nested.last?.box.padding.left, 7)
        XCTAssertEqual(nested[1].box.padding.left, 13)
        XCTAssertEqual(nested.last?.leading, 32)
        XCTAssertEqual(result.attribute(RenderBridgeAttributes.listMarkerGap, at: 0, effectiveRange: nil) as? CGFloat, 12)
        XCTAssertEqual(result.attribute(RenderBridgeAttributes.listMarkerScale, at: 0, effectiveRange: nil) as? CGFloat, 1)
        XCTAssertEqual(result.attribute(RenderBridgeAttributes.listMarkerColor, at: 0, effectiveRange: nil) as? UIColor, .red)
    }

    func testRender_invalidJSON() {
        let result = RenderBridge.renderElements(
            fromJSON: "not valid json",
            baseFont: baseFont,
            textColor: textColor
        )

        XCTAssertEqual(
            result.string, "",
            "Invalid JSON should produce empty attributed string"
        )
    }

    func testRender_emptyArray() {
        let result = RenderBridge.renderElements(
            fromJSON: "[]",
            baseFont: baseFont,
            textColor: textColor
        )

        XCTAssertEqual(
            result.string, "",
            "Empty array should produce empty attributed string"
        )
    }

    /// Test attributesForMarks directly to verify all mark combinations.
    func testAttributesForMarks_noMarks() {
        let attrs = RenderBridge.attributesForMarks([], baseFont: baseFont, textColor: textColor)
        let font = attrs[.font] as? UIFont
        XCTAssertEqual(font, baseFont, "No marks should use base font")
        XCTAssertNil(attrs[.underlineStyle], "No marks should have no underline")
        XCTAssertNil(attrs[.strikethroughStyle], "No marks should have no strikethrough")
    }

    func testAttributesForMarks_strongAlias() {
        // "strong" is an alias for "bold"
        let attrs = RenderBridge.attributesForMarks(
            ["strong"],
            baseFont: baseFont,
            textColor: textColor
        )
        let font = attrs[.font] as? UIFont
        XCTAssertTrue(
            font?.fontDescriptor.symbolicTraits.contains(.traitBold) ?? false,
            "'strong' should produce bold font"
        )
    }

    func testAttributesForMarks_emAlias() {
        // "em" is an alias for "italic"
        let attrs = RenderBridge.attributesForMarks(
            ["em"],
            baseFont: baseFont,
            textColor: textColor
        )
        let font = attrs[.font] as? UIFont
        XCTAssertTrue(
            font?.fontDescriptor.symbolicTraits.contains(.traitItalic) ?? false,
            "'em' should produce italic font"
        )
    }

    func testAttributesForMarks_strikethroughAlias() {
        // "strikethrough" is an alias for "strike"
        let attrs = RenderBridge.attributesForMarks(
            ["strikethrough"],
            baseFont: baseFont,
            textColor: textColor
        )
        let strikethrough = attrs[.strikethroughStyle] as? Int
        XCTAssertEqual(
            strikethrough, NSUnderlineStyle.single.rawValue,
            "'strikethrough' should produce strikethrough style"
        )
    }

    func testAttributesForMarks_allCombined() {
        let attrs = RenderBridge.attributesForMarks(
            ["bold", "italic", "underline", "strike"],
            baseFont: baseFont,
            textColor: textColor
        )
        let font = attrs[.font] as? UIFont
        let traits = font?.fontDescriptor.symbolicTraits ?? []
        XCTAssertTrue(traits.contains(.traitBold), "Should have bold")
        XCTAssertTrue(traits.contains(.traitItalic), "Should have italic")
        XCTAssertEqual(
            attrs[.underlineStyle] as? Int,
            NSUnderlineStyle.single.rawValue,
            "Should have underline"
        )
        XCTAssertEqual(
            attrs[.strikethroughStyle] as? Int,
            NSUnderlineStyle.single.rawValue,
            "Should have strikethrough"
        )
    }

    func testAttributesForMarks_unknownMarkIgnored() {
        let attrs = RenderBridge.attributesForMarks(
            ["unknownMark"],
            baseFont: baseFont,
            textColor: textColor
        )
        let font = attrs[.font] as? UIFont
        XCTAssertEqual(
            font, baseFont,
            "Unknown marks should be ignored, producing base font"
        )
    }

    func testParagraphStyle_depth0() {
        let ctx = BlockContext(nodeType: "paragraph", depth: 0, listContext: nil)
        let style = RenderBridge.paragraphStyleForBlock(ctx, blockStack: [ctx])
        XCTAssertEqual(
            style.firstLineHeadIndent, 0,
            "Depth 0 paragraph should have 0 indentation"
        )
        XCTAssertEqual(
            style.headIndent, 0,
            "Depth 0 paragraph should have 0 head indent"
        )
    }

    func testParagraphStyle_depth2() {
        let ctx = BlockContext(nodeType: "paragraph", depth: 2, listContext: nil)
        let style = RenderBridge.paragraphStyleForBlock(ctx, blockStack: [ctx])
        let expectedIndent: CGFloat = 2 * 24.0  // 2 * indentPerDepth
        XCTAssertEqual(
            style.firstLineHeadIndent, expectedIndent,
            "Depth 2 paragraph should have \(expectedIndent) first line indent"
        )
    }

    func testParagraphStyle_listItem() {
        let listCtx: [String: Any] = [
            "ordered": true,
            "index": 1,
            "total": 3,
            "start": 1,
            "isFirst": true,
            "isLast": false
        ]
        let ctx = BlockContext(nodeType: "listItem", depth: 1, listContext: listCtx)
        let style = RenderBridge.paragraphStyleForBlock(ctx, blockStack: [ctx])

        let baseIndent: CGFloat = 1 * 24.0  // depth * indentPerDepth
        XCTAssertEqual(
            style.firstLineHeadIndent, baseIndent + LayoutConstants.listMarkerWidth,
            "List item first line indent should reserve marker width"
        )
        XCTAssertEqual(
            style.headIndent, baseIndent + LayoutConstants.listMarkerWidth,
            "List item head indent should include marker width"
        )
    }

    func testParagraphStyle_listBaseIndentMultiplierCanCollapseTopLevelIndent() {
        let listCtx: [String: Any] = [
            "ordered": false,
            "index": 1,
            "total": 1,
            "start": 1,
            "isFirst": true,
            "isLast": true
        ]
        let topLevelCtx = BlockContext(nodeType: "paragraph", depth: 1, listContext: listCtx)
        let nestedCtx = BlockContext(nodeType: "paragraph", depth: 2, listContext: listCtx)
        let theme = EditorTheme(dictionary: [
            "list": [
                "indent": 24,
                "baseIndentMultiplier": 0
            ]
        ])

        let topLevelStyle = RenderBridge.paragraphStyleForBlock(
            topLevelCtx,
            blockStack: [topLevelCtx],
            theme: theme,
            baseFont: baseFont
        )
        let nestedStyle = RenderBridge.paragraphStyleForBlock(
            nestedCtx,
            blockStack: [nestedCtx],
            theme: theme,
            baseFont: baseFont
        )

        XCTAssertEqual(
            topLevelStyle.firstLineHeadIndent,
            LayoutConstants.listMarkerWidth,
            accuracy: 0.1,
            "Top-level list items should be flush-left apart from the marker gutter"
        )
        XCTAssertEqual(
            topLevelStyle.headIndent,
            LayoutConstants.listMarkerWidth,
            accuracy: 0.1,
            "Wrapped lines should align with the marker gutter when the base indent multiplier is zero"
        )
        XCTAssertEqual(
            nestedStyle.headIndent - topLevelStyle.headIndent,
            24,
            accuracy: 0.1,
            "Nested list levels should still add one indent unit each"
        )
    }

    func testParagraphStyle_unorderedMarkerScaleDoesNotWidenTextGutter() {
        let baseContext = BlockContext(
            nodeType: "listItem",
            depth: 1,
            listContext: [
                "ordered": false,
                "index": 1,
                "total": 1,
                "start": 1,
                "isFirst": true,
                "isLast": true
            ]
        )
        let baseTheme = EditorTheme(dictionary: [
            "list": [
                "indent": 24,
                "markerScale": 1
            ]
        ])
        let scaledTheme = EditorTheme(dictionary: [
            "list": [
                "indent": 24,
                "markerScale": 2
            ]
        ])

        let largeBaseFont = UIFont.systemFont(ofSize: 40)
        let baseStyle = RenderBridge.paragraphStyleForBlock(
            baseContext,
            blockStack: [baseContext],
            theme: baseTheme,
            baseFont: largeBaseFont
        )
        let scaledStyle = RenderBridge.paragraphStyleForBlock(
            baseContext,
            blockStack: [baseContext],
            theme: scaledTheme,
            baseFont: largeBaseFont
        )

        XCTAssertEqual(baseStyle.headIndent, scaledStyle.headIndent, accuracy: 0.1)
        XCTAssertEqual(baseStyle.firstLineHeadIndent, scaledStyle.firstLineHeadIndent, accuracy: 0.1)
    }

    func testParagraphStyle_blockquoteUsesQuoteIndent() {
        let quote = BlockContext(nodeType: "blockquote", depth: 0, listContext: nil)
        let paragraph = BlockContext(nodeType: "paragraph", depth: 1, listContext: nil)
        let theme = EditorTheme(dictionary: [
            "blockquote": [
                "indent": 20,
                "borderColor": "#aa5500",
                "borderWidth": 4,
                "markerGap": 10
            ]
        ])

        let style = RenderBridge.paragraphStyleForBlock(
            paragraph,
            blockStack: [quote, paragraph],
            theme: theme,
            baseFont: baseFont
        )

        XCTAssertEqual(style.firstLineHeadIndent, 20, accuracy: 0.1)
        XCTAssertEqual(style.headIndent, 20, accuracy: 0.1)
    }

    func testParagraphStyle_nestedListItemInsideBlockquoteAddsListIndent() {
        let quote = BlockContext(nodeType: "blockquote", depth: 0, listContext: nil)
        let parentListItem = BlockContext(
            nodeType: "listItem",
            depth: 1,
            listContext: ["ordered": false, "index": 1, "total": 2, "start": 1, "isFirst": true, "isLast": false]
        )
        let parentParagraph = BlockContext(nodeType: "paragraph", depth: 2, listContext: nil)
        let nestedListItem = BlockContext(
            nodeType: "listItem",
            depth: 2,
            listContext: ["ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true]
        )
        let nestedParagraph = BlockContext(nodeType: "paragraph", depth: 3, listContext: nil)

        let parentStyle = RenderBridge.paragraphStyleForBlock(
            parentParagraph,
            blockStack: [quote, parentListItem, parentParagraph],
            theme: nil,
            baseFont: baseFont
        )
        let nestedStyle = RenderBridge.paragraphStyleForBlock(
            nestedParagraph,
            blockStack: [quote, parentListItem, nestedListItem, nestedParagraph],
            theme: nil,
            baseFont: baseFont
        )

        XCTAssertGreaterThan(
            nestedStyle.headIndent,
            parentStyle.headIndent,
            "nested list item inside a blockquote should indent more than its parent item"
        )
        XCTAssertGreaterThan(
            nestedStyle.firstLineHeadIndent,
            parentStyle.firstLineHeadIndent,
            "nested list marker should also move inward inside a blockquote"
        )
    }

    func testParagraphStyle_firstLevelListInsideBlockquoteAddsListIndentInsideQuote() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "blockquote", "depth": 0},
            {"type": "blockStart", "nodeType": "listItem", "depth": 1,
            "listContext": {"ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true}},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 2},
            {"type": "textRun", "text": "Quoted item", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"},
            {"type": "blockEnd"}
        ]
        """
        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor
        )
        let style = result.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle
        let quote = BlockContext(nodeType: "blockquote", depth: 0, listContext: nil)
        let quotedParagraph = BlockContext(nodeType: "paragraph", depth: 1, listContext: nil)
        let quotedListParagraph = BlockContext(
            nodeType: "paragraph",
            depth: 2,
            listContext: ["ordered": false, "index": 1, "total": 1, "start": 1, "isFirst": true, "isLast": true]
        )
        let plainQuotedStyle = RenderBridge.paragraphStyleForBlock(
            quotedParagraph,
            blockStack: [quote, quotedParagraph],
            theme: nil,
            baseFont: baseFont
        )
        let expectedStyle = RenderBridge.paragraphStyleForBlock(
            quotedListParagraph,
            blockStack: [quote, quotedListParagraph],
            theme: nil,
            baseFont: baseFont
        )

        XCTAssertEqual(
            style?.headIndent ?? -1,
            expectedStyle.headIndent,
            accuracy: 0.1,
            "first-level list paragraphs inside a blockquote should keep their extra list indent"
        )
        XCTAssertEqual(
            style?.firstLineHeadIndent ?? -1,
            expectedStyle.firstLineHeadIndent,
            accuracy: 0.1,
            "first-level quoted list markers should keep their extra list indent"
        )
        XCTAssertGreaterThan(
            style?.headIndent ?? -1,
            plainQuotedStyle.headIndent,
            "quoted list text should indent further than plain quoted text"
        )
        XCTAssertGreaterThan(
            style?.firstLineHeadIndent ?? -1,
            plainQuotedStyle.firstLineHeadIndent,
            "quoted list marker gutter should indent further than plain quoted text"
        )
    }

    func testRender_blockquoteAppliesBorderAttributesAndTextTheme() {
        let json = """
        [
            {"type": "blockStart", "nodeType": "blockquote", "depth": 0},
            {"type": "blockStart", "nodeType": "paragraph", "depth": 1},
            {"type": "textRun", "text": "Quoted", "marks": []},
            {"type": "blockEnd"},
            {"type": "blockEnd"}
        ]
        """
        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor,
            theme: EditorTheme(dictionary: [
                "blockquote": [
                    "indent": 20,
                    "borderColor": "#aa5500",
                    "borderWidth": 4,
                    "markerGap": 10,
                    "text": [
                        "color": "#334455"
                    ]
                ]
            ])
        )
        let expectedTextColor = UIColor(
            red: 51.0 / 255.0,
            green: 68.0 / 255.0,
            blue: 85.0 / 255.0,
            alpha: 1
        )
        let expectedBorderColor = UIColor(
            red: 170.0 / 255.0,
            green: 85.0 / 255.0,
            blue: 0.0,
            alpha: 1
        )
        var foundStyledRun = false
        result.enumerateAttributes(
            in: NSRange(location: 0, length: result.length),
            options: []
        ) { attrs, _, stop in
            guard attrs[RenderBridgeAttributes.blockquoteBorderColor] != nil else { return }
            XCTAssertEqual(attrs[.foregroundColor] as? UIColor, expectedTextColor)
            XCTAssertEqual(attrs[RenderBridgeAttributes.blockquoteBorderColor] as? UIColor, expectedBorderColor)
            XCTAssertEqual(
                (attrs[RenderBridgeAttributes.blockquoteBorderWidth] as? NSNumber)?.doubleValue ?? 0,
                4,
                accuracy: 0.1
            )
            XCTAssertEqual(
                (attrs[RenderBridgeAttributes.blockquoteMarkerGap] as? NSNumber)?.doubleValue ?? 0,
                10,
                accuracy: 0.1
            )
            foundStyledRun = true
            stop.pointee = true
        }

        XCTAssertTrue(foundStyledRun, "Expected a rendered run carrying blockquote border attributes")
    }

    func testRender_blockquoteDoesNotInsertExtraLeadingParagraphBreak() {
        let json = """
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
        """
        let result = RenderBridge.renderElements(
            fromJSON: json,
            baseFont: baseFont,
            textColor: textColor
        )

        XCTAssertEqual(result.string, "Hello\nWorld")
    }

    func testListMarker_ordered() {
        let ctx: [String: Any] = ["ordered": true, "index": 3]
        let marker = RenderBridge.listMarkerString(listContext: ctx)
        XCTAssertEqual(marker, "3. ", "Ordered list item 3 should produce '3. '")
    }

    func testListMarker_orderedPreservesExactU32Boundary() {
        let max: [String: Any] = ["ordered": true, "index": NSNumber(value: UInt32.max)]
        XCTAssertEqual(RenderBridge.listMarkerString(listContext: max), "4294967295. ")

        for malformedIndex: Any in [
            NSNumber(value: -1),
            NSNumber(value: 1.5),
            NSNull(),
            "1",
            NSNumber(value: UInt64(UInt32.max) + 1)
        ] {
            let context: [String: Any] = ["ordered": true, "index": malformedIndex]
            XCTAssertEqual(
                RenderBridge.listMarkerString(listContext: context),
                "",
                "present malformed ordered-list index must be rejected: \(malformedIndex)"
            )
        }
    }

    func testListMarker_unordered() {
        let ctx: [String: Any] = ["ordered": false, "index": 1]
        let marker = RenderBridge.listMarkerString(listContext: ctx)
        XCTAssertEqual(marker, "\u{2022} ", "Unordered list should produce bullet + space")
    }

}

extension RenderBridgeTests {
    func testVersionedStylesInheritParagraphLineHeightAndReserveBoxSpace() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"text":{"lineHeight":30,"color":"#123456ff"},"paragraph":{"paddingLeft":12,"paddingRight":7,"borderLeftWidth":4,"borderRightWidth":2,"marginTop":5,"marginBottom":9}}}
        """))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "Styled", "marks": []], ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        let style = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(style.minimumLineHeight, 30)
        XCTAssertEqual(style.headIndent, 16)
        XCTAssertEqual(style.tailIndent, -9)
        XCTAssertEqual(style.paragraphSpacingBefore, 5)
        XCTAssertEqual(style.paragraphSpacing, 9)
        XCTAssertEqual(theme.effectiveTextStyle(for: "paragraph").color, EditorTheme.color(from: "#123456ff"))
    }

    func testVersionedInlineStylesApplyFixedOrderAndExplicitDecorationReset() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"bold":{"color":"#ff0000ff"},"link":{"fontWeight":"400","textDecorationLine":"none"},"strike":{"textDecorationLine":"underline line-through","letterSpacing":2}}}
        """))
        let attrs = RenderBridge.attributesForMarks(["strike", "link", "bold"], baseFont: baseFont, textColor: textColor, theme: theme)
        XCTAssertFalse(try XCTUnwrap(attrs[.font] as? UIFont).fontDescriptor.symbolicTraits.contains(.traitBold))
        XCTAssertEqual(attrs[.kern] as? CGFloat, 2)
        XCTAssertEqual(attrs[.underlineStyle] as? Int, NSUnderlineStyle.single.rawValue)
        XCTAssertEqual(attrs[.strikethroughStyle] as? Int, NSUnderlineStyle.single.rawValue)
    }

    func testVersionedThemeRejectsUnknownVersionAndInvalidStylesShape() {
        XCTAssertNil(EditorTheme.from(json: "{\"version\":true,\"styles\":{}}"))
        XCTAssertNil(EditorTheme.from(json: "{\"version\":1.5,\"styles\":{}}"))
        XCTAssertNil(EditorTheme.from(json: "{\"version\":2,\"styles\":{}}"))
        XCTAssertNil(EditorTheme.from(json: "{\"version\":1,\"styles\":[]}"))
    }
}

extension RenderBridgeTests {
    func testVersionedSiblingMarginsCollapseWithoutCollapsingPadding() throws {
        for (bottom, top, collapsed) in [(12, 20, 20), (-12, -20, -20), (12, -5, 7), (0, -20, -20), (-60, -60, -60)] {
            let theme = try XCTUnwrap(EditorTheme.from(json: """
            {"version":1,"styles":{"paragraph":{"marginTop":80,"marginBottom":\(bottom),"paddingBottom":3,"borderBottomWidth":2},"codeBlock":{"marginTop":\(top),"paddingTop":7,"borderTopWidth":4}}}
            """))
            let rendered = RenderBridge.renderElements(fromArray: [
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                ["type": "textRun", "text": "First", "marks": []], ["type": "blockEnd"],
                ["type": "blockStart", "nodeType": "codeBlock", "depth": 0],
                ["type": "textRun", "text": String(repeating: "Second ", count: 8), "marks": []], ["type": "blockEnd"]
            ], baseFont: baseFont, textColor: textColor, theme: theme)
            let first = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)
            let second = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: (rendered.string as NSString).range(of: "Second").location, effectiveRange: nil) as? NSParagraphStyle)
            XCTAssertEqual(first.paragraphSpacing + second.paragraphSpacingBefore, CGFloat(collapsed + 16), accuracy: 0.01)
            let storage = NSTextStorage(attributedString: rendered)
            let manager = EditorLayoutManager()
            let container = NSTextContainer(size: CGSize(width: 320, height: 1_000))
            manager.addTextContainer(container)
            storage.addLayoutManager(manager)
            manager.ensureLayout(for: container)
            let firstLine = manager.lineFragmentUsedRect(forGlyphAt: 0, effectiveRange: nil)
            let secondGlyph = manager.glyphIndexForCharacter(at: (rendered.string as NSString).range(of: "Second").location)
            let secondLine = manager.lineFragmentUsedRect(forGlyphAt: secondGlyph, effectiveRange: nil)
            XCTAssertEqual(secondLine.minY - firstLine.maxY, CGFloat(collapsed + 16), accuracy: 0.5)
            var wrappedLines: [CGRect] = []
            manager.enumerateLineFragments(forGlyphRange: NSRange(location: secondGlyph, length: manager.numberOfGlyphs - secondGlyph)) { _, used, _, _, _ in
                wrappedLines.append(used)
            }
            XCTAssertGreaterThan(wrappedLines.count, 1)
            for (previous, next) in zip(wrappedLines, wrappedLines.dropFirst()) {
                XCTAssertEqual(next.minY, previous.maxY, accuracy: 0.5)
            }
        }
    }

    func testVersionedSiblingQuoteMarginsCollapseOutsideTheirPadding() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"paragraph":{"marginBottom":12},"blockquote":{"marginTop":20,"marginBottom":14,"paddingTop":3,"paddingBottom":5}}}
        """))
        let elements: [[String: Any]] = ["First", "Second"].flatMap { text in [
            ["type": "blockStart", "nodeType": "blockquote", "depth": 0],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 1],
            ["type": "textRun", "text": text, "marks": []],
            ["type": "blockEnd"], ["type": "blockEnd"]
        ] }
        let rendered = RenderBridge.renderElements(fromArray: elements, baseFont: baseFont, textColor: textColor, theme: theme)
        let first = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)
        let second = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: (rendered.string as NSString).range(of: "Second").location, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(first.paragraphSpacing + second.paragraphSpacingBefore, 28, accuracy: 0.01)
    }

    func testVersionedVoidBlockMarginsCollapseWithAdjacentParagraphs() throws {
        let expectedParagraphGap: CGFloat = 22
        let expectedEmptyToVoidGap: CGFloat = 23
        let expectedVoidToRichGap: CGFloat = 19
        let imageSource = "https://example.com/margins.png"
        let imageSize = CGSize(width: 140, height: 80)
        let imageAttributes: [String: Any] = ["src": imageSource, "width": imageSize.width, "height": imageSize.height]
        for nodeType in ["image", "horizontalRule"] {
            let theme = try XCTUnwrap(EditorTheme.from(json: """
            {"version":1,"styles":{"paragraph":{"marginTop":17,"marginBottom":12,"paddingTop":2,"paddingBottom":3},"\(nodeType)":{"marginTop":20,"marginBottom":8}}}
            """))
            for quoteDepth in [0, 2] {
                let context = "\(nodeType), quote depth \(quoteDepth)"
                var elements: [[String: Any]] = (0..<quoteDepth).map {
                    ["type": "blockStart", "nodeType": "blockquote", "depth": $0]
                }
                elements += [
                    ["type": "blockStart", "nodeType": "paragraph", "depth": quoteDepth],
                    ["type": "textRun", "text": "First ", "marks": ["bold"]],
                    ["type": "textRun", "text": "café", "marks": ["italic"]], ["type": "blockEnd"],
                    ["type": "blockStart", "nodeType": "paragraph", "depth": quoteDepth], ["type": "blockEnd"],
                    ["type": "voidBlock", "nodeType": nodeType, "docPos": 14 + quoteDepth,
                        "attrs": nodeType == "image" ? imageAttributes : [:]],
                    ["type": "blockStart", "nodeType": "paragraph", "depth": quoteDepth],
                    ["type": "textRun", "text": "Last ", "marks": [["type": "link", "href": "https://example.com/margins"]]],
                    ["type": "textRun", "text": "é", "marks": ["bold", "italic"]], ["type": "blockEnd"]
                ]
                elements += (0..<quoteDepth).map { _ in ["type": "blockEnd"] }
                let rendered = RenderBridge.renderElements(fromArray: elements, baseFont: baseFont, textColor: textColor, theme: theme)
                XCTAssertEqual(rendered.string, "First café\n\u{200B}\n\u{FFFC}\nLast é", context)
                let text = rendered.string as NSString
                let emptyIndex = text.range(of: "\u{200B}").location
                let atomIndex = text.range(of: "\u{FFFC}").location
                let lastIndex = text.range(of: "Last").location
                let first = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)
                let empty = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: emptyIndex, effectiveRange: nil) as? NSParagraphStyle)
                let attachment = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: atomIndex, effectiveRange: nil) as? NSParagraphStyle)
                let last = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: lastIndex, effectiveRange: nil) as? NSParagraphStyle)
                XCTAssertEqual(first.paragraphSpacing + empty.paragraphSpacingBefore, expectedParagraphGap, accuracy: 0.01, context)
                XCTAssertEqual(empty.paragraphSpacing + attachment.paragraphSpacingBefore, expectedEmptyToVoidGap, accuracy: 0.01, context)
                XCTAssertEqual(attachment.paragraphSpacing + last.paragraphSpacingBefore, expectedVoidToRichGap, accuracy: 0.01, context)
                XCTAssertEqual(rendered.attribute(RenderBridgeAttributes.syntheticPlaceholder, at: emptyIndex, effectiveRange: nil) as? Bool, true, context)
                let renderedAttachment = try XCTUnwrap(rendered.attribute(.attachment, at: atomIndex, effectiveRange: nil) as? NSTextAttachment)
                if nodeType == "image" {
                    let image = try XCTUnwrap(renderedAttachment as? BlockImageAttachment)
                    XCTAssertEqual(image.source, imageSource, context)
                    XCTAssertEqual(image.preferredWidth, imageSize.width, context)
                    XCTAssertEqual(image.preferredHeight, imageSize.height, context)
                } else { XCTAssertTrue(renderedAttachment is HorizontalRuleAttachment, context) }
                XCTAssertEqual(rendered.attribute(RenderBridgeAttributes.linkHref, at: lastIndex, effectiveRange: nil) as? String, "https://example.com/margins", context)
                let firstFont = try XCTUnwrap(rendered.attribute(.font, at: 0, effectiveRange: nil) as? UIFont)
                let finalFont = try XCTUnwrap(rendered.attribute(.font, at: lastIndex + "Last ".utf16.count, effectiveRange: nil) as? UIFont)
                XCTAssertTrue(firstFont.fontDescriptor.symbolicTraits.contains(.traitBold), context)
                XCTAssertTrue(finalFont.fontDescriptor.symbolicTraits.contains([.traitBold, .traitItalic]), context)
                let firstBoxes = try XCTUnwrap(renderedStyleBoxes(from: rendered.attribute(editorStyleBoxesAttribute, at: 0, effectiveRange: nil)))
                let atomBoxes = renderedStyleBoxes(from: rendered.attribute(editorStyleBoxesAttribute, at: atomIndex, effectiveRange: nil)) ?? []
                XCTAssertEqual(atomBoxes.count, quoteDepth, context)
                for (parent, atomParent) in zip(firstBoxes, atomBoxes) {
                    XCTAssertTrue(parent === atomParent, "Void block must keep its real ancestor identities: \(context)")
                }
                let spacingBox = try XCTUnwrap(rendered.attribute(editorBlockSpacingBoxAttribute, at: atomIndex, effectiveRange: nil) as? EditorRenderedBox)
                XCTAssertEqual(spacingBox.depth, quoteDepth, context)
            }
        }
    }

    func testVersionedQuoteOmitsOnlyFinalParagraphBottomMargin() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"paragraph":{"marginBottom":12,"paddingBottom":3,"borderBottomWidth":2},"blockquote":{"paddingBottom":7,"marginBottom":11}}}
        """))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "blockquote", "depth": 0],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 1],
            ["type": "textRun", "text": "First", "marks": []],
            ["type": "blockEnd"],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 1],
            ["type": "textRun", "text": "Last", "marks": []],
            ["type": "blockEnd"],
            ["type": "blockEnd"],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "Outside", "marks": []],
            ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        let text = rendered.string as NSString
        for (label, expected): (String, CGFloat) in [("First", 17), ("Last", 23), ("Outside", 17)] {
            let index = text.range(of: label).location
            let style = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: index, effectiveRange: nil) as? NSParagraphStyle)
            XCTAssertEqual(style.paragraphSpacing, expected, accuracy: 0.01, label)
        }
        let boxes = try XCTUnwrap(renderedStyleBoxes(from: rendered.attribute(editorStyleBoxesAttribute, at: text.range(of: "Last").location, effectiveRange: nil)))
        XCTAssertEqual(boxes.first?.bottomInset, 12)
        XCTAssertEqual(boxes.last?.box.inset.bottom, 5)
    }

    func testVersionedQuoteKeepsNestedContainerSpacing() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"paragraph":{"marginBottom":12},"blockquote":{"paddingBottom":7}}}
        """))
        for (nodeType, expected): (String, CGFloat) in [("blockquote", 14), ("listItem", 23)] {
            var child: [String: Any] = ["type": "blockStart", "nodeType": nodeType, "depth": 1]
            if nodeType == "listItem" {
                child["listContext"] = ["ordered": false, "index": 1, "isFirst": true, "isLast": true]
            }
            let rendered = RenderBridge.renderElements(fromArray: [
                ["type": "blockStart", "nodeType": "blockquote", "depth": 0], child,
                ["type": "blockStart", "nodeType": "paragraph", "depth": 2],
                ["type": "textRun", "text": "Nested", "marks": []],
                ["type": "blockEnd"], ["type": "blockEnd"], ["type": "blockEnd"]
            ], baseFont: baseFont, textColor: textColor, theme: theme)
            let style = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)
            XCTAssertEqual(style.paragraphSpacing, expected, accuracy: 0.01, nodeType)
        }
    }

    func testStyleBoxResolvesAsymmetricBordersCornersAndImageFit() {
        let box = EditorStyleBox([
            "borderTopWidth": 2, "borderLeftWidth": 4, "borderRightWidth": 0,
            "borderBottomWidth": 3, "paddingLeft": 6, "borderTopLeftRadius": 20,
            "borderTopRightRadius": 0, "resizeMode": "contain"
        ])
        XCTAssertEqual(box.inset.left, 10)
        XCTAssertEqual(box.inset.right, 0)
        XCTAssertEqual(box.radii, [20, 0, 0, 0])
        let bounds = CGRect(x: 0, y: 0, width: 100, height: 100)
        XCTAssertFalse(box.path(in: bounds).contains(CGPoint(x: 1, y: 1)))
        XCTAssertTrue(box.path(in: bounds).contains(CGPoint(x: 99, y: 1)))
        XCTAssertEqual(box.imageRect(CGSize(width: 180, height: 90), in: bounds), CGRect(x: 10, y: 27, width: 90, height: 45))
    }

    func testStyledEmptyBlockRetainsCaretAndDecorationIdentity() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"paragraph":{"backgroundColor":"#ff0000ff","paddingTop":10,"paddingBottom":12}}}
        """))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0], ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        XCTAssertGreaterThan(rendered.length, 0)
        XCTAssertNotNil(rendered.attribute(editorStyleBoxesAttribute, at: 0, effectiveRange: nil))
        XCTAssertEqual(rendered.attribute(RenderBridgeAttributes.syntheticPlaceholder, at: 0, effectiveRange: nil) as? Bool, true)
    }
}

extension RenderBridgeTests {
    func testStyledWrappedParagraphKeepsTextKitCaretInsideAllocatedBox() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"paragraph":{"paddingTop":11,"paddingBottom":13,"paddingLeft":7,"borderTopWidth":2,"borderLeftWidth":4,"marginTop":17,"marginBottom":19}}}
        """))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": String(repeating: "wrapped text ", count: 8), "marks": []],
            ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        let storage = NSTextStorage(attributedString: rendered)
        let manager = EditorLayoutManager()
        let container = NSTextContainer(size: CGSize(width: 160, height: 10_000))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        let first = manager.lineFragmentUsedRect(forGlyphAt: 0, effectiveRange: nil)
        XCTAssertEqual(first.minX, 11)
        XCTAssertEqual(first.minY, 30)
        var count = 0
        manager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: manager.numberOfGlyphs)) { _, used, _, _, _ in
            XCTAssertEqual(used.minX, 11)
            count += 1
        }
        XCTAssertGreaterThan(count, 1)
    }
}

extension RenderBridgeTests {
    func testInlineLineHeightExpandsOnlyItsTextKitLine() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"bold":{"lineHeight":60},"paragraph":{"marginBottom":0}}}
        """))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "Tall", "marks": ["bold"]], ["type": "blockEnd"],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "Short", "marks": []], ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        let storage = NSTextStorage(attributedString: rendered)
        let manager = EditorLayoutManager()
        let container = NSTextContainer(size: CGSize(width: 200, height: 500))
        manager.addTextContainer(container); storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        XCTAssertEqual(manager.lineFragmentUsedRect(forGlyphAt: 0, effectiveRange: nil).height, 60)
        XCTAssertLessThan(manager.lineFragmentUsedRect(forGlyphAt: manager.numberOfGlyphs - 1, effectiveRange: nil).height, 60)
    }
}

extension RenderBridgeTests {
    func testAtomicAdmissionAcceptsCoreLanguageAndRejectsMalformedLanguage() throws {
        let config = #"{"schema":{"nodes":[{"name":"doc","content":"block+","role":"doc"},{"name":"paragraph","content":"inline*","group":"block","role":"textBlock"},{"name":"codeBlock","content":"text*","group":"block","role":"textBlock","attrs":{"language":{"default":null}}},{"name":"text","group":"inline","role":"text"}],"marks":[]},"initialization":{"type":"localEmpty"}}"#
        let editorId = makeV2Editor(configJson: config)
        defer { destroyV2Editor(id: editorId) }
        _ = EditorV2Shadow.setJson(id: editorId, json: #"{"type":"doc","content":[{"type":"codeBlock","attrs":{"language":"rust"},"content":[{"type":"text","text":"let value = 1;"}]}]}"#)
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: String(editorId), mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(raw))
        var object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
        var blocks = try XCTUnwrap(object["renderBlocks"] as? [[[String: Any]]])
        for language: Any in [42, true, ["bad"]] {
            blocks[0][0]["language"] = language
            object["renderBlocks"] = blocks
            let data = try JSONSerialization.data(withJSONObject: object)
            XCTAssertNil(EditorV2Adapter.parseAtomicRenderSnapshot(try XCTUnwrap(String(data: data, encoding: .utf8))))
        }
    }

    func testAtomicAdmissionValidatesRichMentionStyleWithoutOpeningUnknownKeys() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let raw = try XCTUnwrap(editorV2RenderUpdate(editorId: String(editorId), mirrorScalarAnchor: nil, mirrorScalarHead: nil).value)
        var object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [String: Any])
        func snapshot(style: Any) throws -> String {
            object["renderBlocks"] = [[
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                [
                    "type": "opaqueInlineAtom", "nodeType": "mention", "label": "Jay", "docPos": 1,
                    "mentionTheme": ["node": ["style": style]]
                ],
                ["type": "blockEnd"]
            ]]
            let data = try JSONSerialization.data(withJSONObject: object)
            return try XCTUnwrap(String(data: data, encoding: .utf8))
        }
        XCTAssertNotNil(EditorV2Adapter.parseAtomicRenderSnapshot(try snapshot(style: ["fontSize": 20, "color": "#123456ff", "borderLeftWidth": 3, "borderTopRightRadius": 8, "paddingLeft": 0, "paddingTop": 8])))
        for style: Any in [[], ["unknown": 1], ["fontSize": -1], ["borderLeftWidth": true], ["color": "bad-color"], ["fontStyle": "oblique"], ["paddingTop": -1], ["paddingRight": true]] {
            XCTAssertNil(EditorV2Adapter.parseAtomicRenderSnapshot(try snapshot(style: style)))
        }
    }
}

extension RenderBridgeTests {
    func testRichDocumentLayoutAvoidsWholeDocumentMentionLookupsForPlainGlyphs() throws {
        let update = try XCTUnwrap(JSONSerialization.jsonObject(
            with: Data(NativePerformanceFixtureFactory.largeRenderJSON().utf8)
        ) as? [String: Any])
        let rendered = RenderBridge.renderBlocks(
            fromArray: try XCTUnwrap(update["renderBlocks"] as? [[NSDictionary]]),
            baseFont: baseFont,
            textColor: textColor
        )
        let storage = AttributeLookupTextStorage()
        storage.setAttributedString(rendered)
        let manager = EditorLayoutManager()
        let container = NSTextContainer(size: CGSize(width: 390, height: CGFloat.greatestFiniteMagnitude))
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        XCTAssertEqual(storage.string, rendered.string)
        XCTAssertGreaterThan(manager.usedRect(for: container).height, 844)
        XCTAssertEqual(
            storage.absentMentionLongestLookupCount,
            0,
            "Plain glyph generation and line breaking must not scan unrelated rich attribute runs across the entire document"
        )
    }

    func testMentionGeometrySurvivesSplitGlyphRunsAndWrapping() throws {
        let mentionLabel = "Jay Den"
        let theme = try XCTUnwrap(EditorTheme.from(json: #"{"version":1,"styles":{"mention":{"paddingLeft":6,"paddingRight":6}}}"#))
        let rendered = NSMutableAttributedString(attributedString: RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "opaqueInlineAtom", "nodeType": "mention", "label": mentionLabel, "docPos": 1],
            ["type": "textRun", "text": "! trailing text wraps beyond the chip", "marks": []],
            ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme))
        let chip = try XCTUnwrap(rendered.attribute(editorMentionBoxAttribute, at: 0, effectiveRange: nil) as? EditorMentionRenderedBox)
        let splitRun = NSRange(location: 3, length: mentionLabel.utf16.count - 3)
        rendered.addAttribute(.font, value: UIFont.italicSystemFont(ofSize: baseFont.pointSize), range: splitRun)
        rendered.addAttribute(.foregroundColor, value: UIColor.systemRed, range: splitRun)
        let storage = AttributeLookupTextStorage()
        storage.setAttributedString(rendered)
        let manager = EditorLayoutManager()
        let container = NSTextContainer(size: CGSize(width: chip.size.width + baseFont.pointSize, height: 500))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        let followingGlyph = manager.glyphIndexForCharacter(at: mentionLabel.utf16.count)
        XCTAssertEqual(manager.location(forGlyphAt: followingGlyph).x, chip.size.width, accuracy: 1)
        let chipLine = manager.lineFragmentRect(forGlyphAt: 0, effectiveRange: nil)
        for offset in 0..<mentionLabel.utf16.count {
            let glyph = manager.glyphIndexForCharacter(at: offset)
            XCTAssertEqual(manager.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil), chipLine)
        }
        let finalGlyph = manager.glyphIndexForCharacter(at: storage.length - 1)
        XCTAssertGreaterThan(manager.lineFragmentRect(forGlyphAt: finalGlyph, effectiveRange: nil).minY, chipLine.minY)
        XCTAssertEqual(storage.string, rendered.string)
        XCTAssertEqual(storage.absentMentionLongestLookupCount, 0, "Ordinary trailing glyphs must use local lookups")
    }

    func testRichMentionReservesChipGeometryWithoutChangingTextOffsets() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: #"{"version":1,"styles":{"mention":{"fontSize":20,"borderLeftWidth":7,"borderRightWidth":9,"borderTopWidth":3,"borderBottomWidth":5}}}"#))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "opaqueInlineAtom", "nodeType": "mention", "label": "Jay Den", "docPos": 1],
            ["type": "textRun", "text": "!", "marks": []], ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        XCTAssertEqual(rendered.string, "Jay Den!")
        let storage = NSTextStorage(attributedString: rendered)
        let manager = EditorLayoutManager()
        let container = NSTextContainer(size: CGSize(width: 300, height: 500))
        container.lineFragmentPadding = 0
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        let font = try XCTUnwrap(rendered.attribute(.font, at: 0, effectiveRange: nil) as? UIFont)
        let labelSize = ("Jay Den" as NSString).size(withAttributes: [.font: font])
        let following = manager.location(forGlyphAt: manager.glyphIndexForCharacter(at: 7))
        XCTAssertEqual(following.x, ceil(labelSize.width) + 12 + 7 + 9, accuracy: 1)
        XCTAssertGreaterThanOrEqual(manager.lineFragmentUsedRect(forGlyphAt: 0, effectiveRange: nil).height, ceil(labelSize.height) + 8 + 3 + 5)
        XCTAssertEqual(storage.string, "Jay Den!")
    }
}

extension RenderBridgeTests {
    func testCodeBlockVerticalInsetsApplyOnlyAtOuterBoundaries() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: #"{"version":1,"styles":{"codeBlock":{"paddingTop":12,"paddingBottom":12,"marginTop":12,"marginBottom":12}}}"#))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "codeBlock", "depth": 0],
            ["type": "textRun", "text": "first\nsecond\nthird", "marks": []], ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        let middle = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: 6, effectiveRange: nil) as? NSParagraphStyle)
        XCTAssertEqual(middle.paragraphSpacingBefore, 0)
        XCTAssertEqual(middle.paragraphSpacing, 0)
        let storage = NSTextStorage(attributedString: rendered)
        let manager = EditorLayoutManager()
        let container = NSTextContainer(size: CGSize(width: 300, height: 500))
        manager.addTextContainer(container)
        storage.addLayoutManager(manager)
        manager.ensureLayout(for: container)
        var lines: [CGRect] = []
        manager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: manager.numberOfGlyphs)) { _, used, _, _, _ in lines.append(used) }
        XCTAssertEqual(lines.count, 3)
        XCTAssertEqual(lines[1].minY - lines[0].minY, lines[0].height, accuracy: 1)
        XCTAssertEqual(lines[2].minY - lines[1].minY, lines[1].height, accuracy: 1)
    }

    func testQuoteDecorationExcludesPreviousParagraphSeparator() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: ##"{"version":1,"styles":{"blockquote":{"borderLeftWidth":3,"borderLeftColor":"#123456ff","paddingTop":8}}}"##))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "Before", "marks": []], ["type": "blockEnd"],
            ["type": "blockStart", "nodeType": "blockquote", "depth": 0],
            ["type": "blockStart", "nodeType": "paragraph", "depth": 1],
            ["type": "textRun", "text": "Quote", "marks": []], ["type": "blockEnd"], ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        XCTAssertEqual(rendered.string, "Before\nQuote")
        let previousBoxes = renderedStyleBoxes(from: rendered.attribute(editorStyleBoxesAttribute, at: 6, effectiveRange: nil)) ?? []
        XCTAssertFalse(previousBoxes.contains { $0.box.color("borderLeftColor") == EditorTheme.color(from: "#123456ff") })
        let quoteBoxes = renderedStyleBoxes(from: rendered.attribute(editorStyleBoxesAttribute, at: 7, effectiveRange: nil)) ?? []
        XCTAssertTrue(quoteBoxes.contains { $0.box.color("borderLeftColor") == EditorTheme.color(from: "#123456ff") })
    }
}

extension RenderBridgeTests {
    func testRoundedBorderFillsCornersAroundRoundedInterior() throws {
        let colorSpace = CGColorSpaceCreateDeviceRGB()
        let context = try XCTUnwrap(CGContext(data: nil, width: 100, height: 100, bitsPerComponent: 8, bytesPerRow: 400, space: colorSpace, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let box = EditorStyleBox(["borderWidth": 10, "borderRadius": 30, "borderColor": UIColor.red, "backgroundColor": UIColor.white])
        box.draw(in: CGRect(x: 0, y: 0, width: 100, height: 100), context: context)
        let pixels = try XCTUnwrap(context.data).assumingMemoryBound(to: UInt8.self)
        XCTAssertEqual(pixels[(13 * 100 + 13) * 4 + 1], 0)
        XCTAssertEqual(pixels[(30 * 100 + 30) * 4 + 1], 255)
        XCTAssertEqual(pixels[3], 0)
    }
}

func renderGlyphBitmap(
    _ manager: NSLayoutManager, glyphRange: NSRange, origin: CGPoint,
    size: CGSize, scale: CGFloat = 1
) throws -> Data {
    let bytesPerPixel = 4
    let width = Int(size.width * scale)
    let height = Int(size.height * scale)
    let bytesPerRow = width * bytesPerPixel
    let context = try XCTUnwrap(CGContext(data: nil, width: width, height: height,
        bitsPerComponent: 8, bytesPerRow: bytesPerRow, space: CGColorSpaceCreateDeviceRGB(),
        bitmapInfo: CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue))
    context.translateBy(x: 0, y: CGFloat(height))
    context.scaleBy(x: scale, y: -scale)
    UIGraphicsPushContext(context)
    defer { UIGraphicsPopContext() }
    manager.drawGlyphs(forGlyphRange: glyphRange, at: origin)
    return Data(bytes: try XCTUnwrap(context.data), count: bytesPerRow * height)
}

extension RenderBridgeTests {
    func testDrawingUndecoratedContainerKeepsDistantLayoutLazy() throws {
        let theme = EditorTheme(dictionary: ["version": 1, "styles": [
            "text": ["fontSize": 16, "lineHeight": 24, "color": "#0000ffff"],
            "blockquote": ["borderWidth": 4, "borderTopWidth": 0, "borderLeftWidth": 0,
                "borderBottomWidth": 0, "borderRightWidth": 0, "paddingTop": 7,
                "paddingBottom": 9, "paddingLeft": 11, "paddingRight": 13,
                "marginTop": 5, "marginBottom": 6],
            "codeBlock": ["backgroundColor": "#ff0000ff", "borderWidth": 0,
                "borderRadius": 0, "paddingTop": 3, "paddingBottom": 5,
                "paddingLeft": 4, "paddingRight": 6, "marginBottom": 8],
            "h1": ["fontSize": 16, "lineHeight": 24, "borderLeftWidth": 3,
                "paddingLeft": 7, "marginTop": 4, "marginBottom": 8],
            "paragraph": ["marginBottom": 6, "paddingLeft": 5]
        ]])
        let headingText = "Border only"
        let distantParagraphCount = 2_000
        var elements: [[String: Any]] = [
            ["type": "blockStart", "nodeType": "blockquote", "depth": 0],
            ["type": "blockStart", "nodeType": "codeBlock", "depth": 1],
            ["type": "textRun", "text": "Wrapped café e\u{301} 東京 😀 code with enough text to occupy several visible lines.", "marks": []],
            ["type": "blockEnd"],
            ["type": "blockStart", "nodeType": "h1", "depth": 1],
            ["type": "textRun", "text": headingText, "marks": []], ["type": "blockEnd"]
        ]
        for index in 0..<distantParagraphCount {
            elements.append(contentsOf: [
                ["type": "blockStart", "nodeType": "paragraph", "depth": 1],
                ["type": "textRun", "text": "Distant paragraph \(index) with wrapped Unicode café 東京 😀 content.", "marks": []],
                ["type": "blockEnd"]
            ])
        }
        elements.append(["type": "blockEnd"])
        let bitmapSize = CGSize(width: 300, height: 240)
        let bytesPerPixel = 4
        let drawOrigin = CGPoint(x: 0, y: 20)
        var bitmaps: [Data] = []
        var visibleLineRects: [[CGRect]] = []
        var headingLocations: [CGPoint] = []
        for eagerLayout in [false, true] {
            let rendered = RenderBridge.renderElements(fromArray: elements,
                baseFont: baseFont, textColor: textColor, theme: theme)
            let boxes = try XCTUnwrap(renderedStyleBoxes(from: rendered.attribute(editorStyleBoxesAttribute,
                at: 0, effectiveRange: nil)))
            let parent = try XCTUnwrap(boxes.first { $0.depth == 0 })
            XCTAssertNil(parent.box.color("backgroundColor"))
            XCTAssertEqual(parent.box.borders, .zero, "Side overrides must cancel the shorthand border")
            let storage = NSTextStorage(attributedString: rendered)
            let manager = EditorLayoutManager()
            manager.allowsNonContiguousLayout = true
            let container = NSTextContainer(size: CGSize(width: bitmapSize.width,
                height: CGFloat.greatestFiniteMagnitude))
            container.lineFragmentPadding = 0
            manager.addTextContainer(container)
            storage.addLayoutManager(manager)
            let headingRange = (storage.string as NSString).range(of: headingText)
            XCTAssertNotEqual(headingRange.location, NSNotFound)
            let visibleCharacters = NSRange(location: 0, length: NSMaxRange(headingRange))
            manager.ensureLayout(forCharacterRange: visibleCharacters)
            let visibleGlyphs = manager.glyphRange(forCharacterRange: visibleCharacters,
                actualCharacterRange: nil)
            if eagerLayout {
                manager.ensureLayout(for: container)
            } else {
                XCTAssertLessThan(manager.firstUnlaidCharacterIndex(), storage.length / 2,
                    "The fixture must leave the distant container tail unlaid before drawing")
            }
            let bitmap = try renderGlyphBitmap(manager, glyphRange: visibleGlyphs,
                origin: drawOrigin, size: bitmapSize)
            if !eagerLayout {
                XCTAssertLessThan(manager.firstUnlaidCharacterIndex(), storage.length / 2,
                    "Drawing an undecorated parent must not lay out its distant children")
            }
            bitmaps.append(bitmap)
            var lines: [CGRect] = []
            manager.enumerateLineFragments(forGlyphRange: visibleGlyphs) { _, used, _, _, _ in
                lines.append(used)
            }
            visibleLineRects.append(lines)
            let headingGlyph = manager.glyphIndexForCharacter(at: headingRange.location)
            headingLocations.append(manager.location(forGlyphAt: headingGlyph))
            let headingBoxes = try XCTUnwrap(renderedStyleBoxes(from: storage.attribute(editorStyleBoxesAttribute,
                at: headingRange.location, effectiveRange: nil)))
            let headingBox = try XCTUnwrap(headingBoxes.first { $0.depth == 1 })
            let headingLine = manager.lineFragmentUsedRect(forGlyphAt: headingGlyph, effectiveRange: nil)
            let borderX = Int(drawOrigin.x + headingBox.leading + container.lineFragmentPadding
                + headingBox.box.borders.left / 2)
            let borderY = Int(drawOrigin.y + headingLine.midY)
            guard borderX >= 0, borderX < Int(bitmapSize.width), borderY >= 0,
                  borderY < Int(bitmapSize.height) else {
                XCTFail("The heading's actual left border must lie inside the visible bitmap")
                continue
            }
            let borderPixelOffset = (borderY * Int(bitmapSize.width) + borderX) * bytesPerPixel
            XCTAssertEqual(Array(bitmap[borderPixelOffset..<(borderPixelOffset + bytesPerPixel)]),
                [0, 0, 0, 255], "A border without explicit color must paint black at its actual heading edge")
        }
        XCTAssertEqual(bitmaps[0], bitmaps[1], "Lazy drawing must preserve text, background and default black border pixels")
        XCTAssertTrue(layoutRectsMatch(visibleLineRects[0], visibleLineRects[1]),
            "Lazy drawing must preserve visible line geometry: \(visibleLineRects[0]) vs \(visibleLineRects[1])")
        XCTAssertEqual(headingLocations[0], headingLocations[1])
        for (name, channels) in [("code background", [255, 0, 0]), ("Unicode text", [0, 0, 255])] {
            XCTAssertTrue(stride(from: 0, to: bitmaps[0].count, by: bytesPerPixel).contains { offset in
                (0..<channels.count).allSatisfy { bitmaps[0][offset + $0] == UInt8(channels[$0]) }
                    && bitmaps[0][offset + 3] == 255
            }, "The visible fixture must paint its \(name)")
        }
    }
}

extension RenderBridgeTests {
    func testMentionPaddingOverridesPreserveUnspecifiedDefaults() {
        let partial = EditorMentionRenderedBox(box: EditorStyleBox([
            "paddingLeft": 0, "paddingTop": 10, "borderRightWidth": 2
        ]))
        XCTAssertEqual(partial.padding, UIEdgeInsets(top: 10, left: 0, bottom: 4, right: 8))
        let zero = EditorMentionRenderedBox(box: EditorStyleBox([
            "paddingTop": 0, "paddingRight": 0, "paddingBottom": 0, "paddingLeft": 0
        ]))
        XCTAssertEqual(zero.padding, .zero)
    }

    func testZeroPaddingMentionKeepsOnlyItsLabelWidth() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: ##"{"version":1,"styles":{"mention":{"paddingTop":0,"paddingRight":0,"paddingBottom":0,"paddingLeft":0,"backgroundColor":"#00000000"}}}"##))
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "opaqueInlineAtom", "nodeType": "mention", "label": "@alice", "docPos": 1],
            ["type": "textRun", "text": "!", "marks": []], ["type": "blockEnd"]
        ], baseFont: baseFont, textColor: textColor, theme: theme)
        let chip = try XCTUnwrap(rendered.attribute(editorMentionBoxAttribute, at: 0, effectiveRange: nil) as? EditorMentionRenderedBox)
        XCTAssertEqual(chip.size.width, ceil(try XCTUnwrap(chip.label).size().width))
        XCTAssertEqual(chip.size.height, ceil(try XCTUnwrap(chip.label).size().height))
        XCTAssertEqual(rendered.string, "@alice!")
    }
}
extension RenderBridgeTests {
    func testMentionTextAlignsWithSurroundingTextRegardlessOfPadding() throws {
        for (top, bottom, surrounded) in [(0, 0, false), (2, 2, false), (4, 4, false), (12, 2, false), (40, 0, false), (0, 40, false), (40, 0, true), (0, 40, true)] {
            let theme = try XCTUnwrap(EditorTheme.from(json: """
                {"version":1,"styles":{"text":{"fontSize":17,"lineHeight":26,"color":"#ff0000ff"},"mention":{"color":"#0000ffff","backgroundColor":"#00ff00ff","paddingTop":\(top),"paddingBottom":\(bottom),"paddingLeft":0,"paddingRight":0}}}
                """))
            var elements: [[String: Any]] = [
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                ["type": "textRun", "text": "alice ", "marks": []],
                ["type": "opaqueInlineAtom", "nodeType": "mention", "label": "alice", "docPos": 7],
                ["type": "textRun", "text": " alice", "marks": []],
                ["type": "blockEnd"]
            ]
            if surrounded {
                elements.insert(contentsOf: [
                    ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                    ["type": "textRun", "text": "before", "marks": []], ["type": "blockEnd"]
                ], at: 0)
                elements.append(contentsOf: [
                    ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
                    ["type": "textRun", "text": "after", "marks": []], ["type": "blockEnd"]
                ])
            }
            let rendered = RenderBridge.renderElements(fromArray: elements, baseFont: baseFont, textColor: textColor, theme: theme)
            let storage = NSTextStorage(attributedString: rendered)
            if surrounded {
                storage.addAttribute(.foregroundColor, value: UIColor.black, range: NSRange(location: 0, length: 6))
                storage.addAttribute(.foregroundColor, value: UIColor.black, range: NSRange(location: storage.length - 5, length: 5))
            }
            let manager = EditorLayoutManager()
            let container = NSTextContainer(size: CGSize(width: 300, height: 240))
            container.lineFragmentPadding = 0
            manager.addTextContainer(container)
            storage.addLayoutManager(manager)
            manager.ensureLayout(for: container)
            let pixels = try renderGlyphBitmap(manager, glyphRange: manager.glyphRange(for: container),
                origin: CGPoint(x: 0, y: 20), size: CGSize(width: 300, height: 240), scale: 2)
            var redRows = Set<Int>()
            var blueRows = Set<Int>()
            var greenRows = Set<Int>()
            for y in 0..<480 {
                for x in 0..<600 {
                    let offset = y * 2400 + x * 4
                    if pixels[offset] > 128 && pixels[offset + 2] < 32 { redRows.insert(y) }
                    if pixels[offset + 1] > 128 && pixels[offset] < 32 && pixels[offset + 2] < 32 { greenRows.insert(y) }
                    if pixels[offset + 2] > 128 && pixels[offset] < 32 { blueRows.insert(y) }
                }
            }
            let lineGlyph = manager.glyphIndexForCharacter(at: surrounded ? 7 : 0)
            let line = manager.lineFragmentUsedRect(forGlyphAt: lineGlyph, effectiveRange: nil)
            if surrounded {
                let nextGlyph = manager.glyphIndexForCharacter(at: storage.length - 1)
                let nextLine = manager.lineFragmentUsedRect(forGlyphAt: nextGlyph, effectiveRange: nil)
                XCTAssertGreaterThanOrEqual(nextLine.minY, line.maxY)
            }
            XCTAssertGreaterThanOrEqual(Double(try XCTUnwrap(greenRows.min())), floor((20 + line.minY) * 2) - 1, "top=\(top), bottom=\(bottom)")
            XCTAssertLessThanOrEqual(Double(try XCTUnwrap(greenRows.max())), ceil((20 + line.maxY) * 2) + 1, "top=\(top), bottom=\(bottom)")
            XCTAssertEqual(Double(try XCTUnwrap(redRows.min())), Double(try XCTUnwrap(blueRows.min())), accuracy: 1, "top=\(top), bottom=\(bottom)")
            XCTAssertEqual(Double(try XCTUnwrap(redRows.max())), Double(try XCTUnwrap(blueRows.max())), accuracy: 1, "top=\(top), bottom=\(bottom)")
        }
    }
}

extension RenderBridgeTests {
    func testDocumentAlignmentOverridesThemeAndResetRebuildsParagraphStyle() throws {
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"paragraph":{"textAlign":"center","paddingLeft":9},"h2":{"textAlign":"center"}}}
        """))
        for nodeType in ["paragraph", "h2"] {
            for text in ["שלום", ""] {
                for (alignment, expected) in [("left", NSTextAlignment.left), ("center", .center), ("right", .right)] {
                    let elements: [[String: Any]] = [
                        ["type": "blockStart", "nodeType": nodeType, "depth": 0, "textAlign": alignment],
                        ["type": "textRun", "text": text, "marks": []], ["type": "blockEnd"]
                    ]
                    let rendered = RenderBridge.renderElements(fromArray: elements, baseFont: baseFont, textColor: textColor, theme: theme)
                    let style = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)
                    XCTAssertEqual(style.alignment, expected, "\(nodeType) text=\(text) alignment=\(alignment)")
                    if nodeType == "paragraph" { XCTAssertEqual(style.headIndent, 9) }
                    var reset = elements
                    reset[0]["textAlign"] = NSNull()
                    let inherited = RenderBridge.renderElements(fromArray: reset, baseFont: baseFont, textColor: textColor, theme: theme)
                    XCTAssertEqual((inherited.attribute(.paragraphStyle, at: 0, effectiveRange: nil) as? NSParagraphStyle)?.alignment, .center)
                }
            }
        }
    }
}

extension RenderBridgeTests {
    func testPhysicalAlignmentMovesRTLTextKitLayoutWithoutTheme() throws {
        var origins: [CGFloat] = []
        for alignment in ["left", "center", "right"] {
            let rendered = RenderBridge.renderElements(fromArray: [
                ["type": "blockStart", "nodeType": "paragraph", "depth": 0, "textAlign": alignment],
                ["type": "textRun", "text": "שלום", "marks": []], ["type": "blockEnd"]
            ], baseFont: baseFont, textColor: textColor)
            let storage = NSTextStorage(attributedString: rendered)
            let manager = EditorLayoutManager()
            let container = NSTextContainer(size: CGSize(width: 300, height: 1_000))
            container.lineFragmentPadding = 0
            manager.addTextContainer(container)
            storage.addLayoutManager(manager)
            manager.ensureLayout(for: container)
            origins.append(manager.lineFragmentUsedRect(forGlyphAt: 0, effectiveRange: nil).minX)
        }
        XCTAssertLessThan(origins[0], origins[1])
        XCTAssertLessThan(origins[1], origins[2])
    }
}

extension RenderBridgeTests {
    func testAlignedTextKitCaretAndHitTestingFollowRebuiltEmptyParagraphs() throws {
        for text in ["abc", ""] {
            var caretOrigins: [CGFloat] = []
            for alignment in ["left", "center", "right"] {
                let rendered = RenderBridge.renderElements(fromArray: [
                    ["type": "blockStart", "nodeType": "paragraph", "depth": 0, "textAlign": alignment],
                    ["type": "textRun", "text": text, "marks": []], ["type": "blockEnd"]
                ], baseFont: baseFont, textColor: textColor)
                let view = UITextView(frame: CGRect(x: 0, y: 0, width: 300, height: 100))
                view.textContainerInset = .zero
                view.textContainer.lineFragmentPadding = 0
                view.attributedText = rendered
                view.layoutIfNeeded()
                let caret = view.caretRect(for: view.beginningOfDocument)
                caretOrigins.append(caret.minX)
                if !text.isEmpty {
                    let hit = try XCTUnwrap(view.closestPosition(to: CGPoint(x: caret.minX, y: caret.midY)))
                    XCTAssertEqual(view.offset(from: view.beginningOfDocument, to: hit), 0, alignment)
                }
            }
            XCTAssertLessThan(caretOrigins[0], caretOrigins[1], text)
            XCTAssertLessThan(caretOrigins[1], caretOrigins[2], text)
        }
    }
}

extension RenderBridgeTests {
    func testAdjacentDocumentAlignmentsRenderIndependentlyAndCaptureNativeView() throws {
        struct BlockCase {
            let nodeType: String
            let text: String
            let alignment: String
            let expected: NSTextAlignment
        }
        let blocks: [BlockCase] = [
            .init(nodeType: "h2", text: "Left aligned heading", alignment: "left", expected: .left),
            .init(nodeType: "paragraph", text: "Physical left: Hello שלום", alignment: "left", expected: .left),
            .init(nodeType: "h2", text: "Centred heading", alignment: "center", expected: .center),
            .init(nodeType: "paragraph", text: "Centre: Hello שלום", alignment: "center", expected: .center),
            .init(nodeType: "h2", text: "Right aligned heading", alignment: "right", expected: .right),
            .init(nodeType: "paragraph", text: "Physical right: Hello שלום", alignment: "right", expected: .right)
        ]
        let elements: [[String: Any]] = blocks.flatMap { block in [
            ["type": "blockStart", "nodeType": block.nodeType, "depth": 0, "textAlign": block.alignment],
            ["type": "textRun", "text": block.text, "marks": []], ["type": "blockEnd"]
        ] }
        let rendered = RenderBridge.renderElements(fromArray: elements, baseFont: baseFont, textColor: .black)
        for block in blocks {
            let offset = (rendered.string as NSString).range(of: block.text).location
            let style = try XCTUnwrap(rendered.attribute(.paragraphStyle, at: offset, effectiveRange: nil) as? NSParagraphStyle)
            XCTAssertEqual(style.alignment, block.expected, block.text)
        }
        let view = UITextView(frame: CGRect(x: 0, y: 0, width: 390, height: 360))
        view.backgroundColor = .white
        view.textContainerInset = UIEdgeInsets(top: 20, left: 16, bottom: 20, right: 16)
        view.attributedText = rendered
        view.layoutIfNeeded()
        let image = UIGraphicsImageRenderer(bounds: view.bounds).image { context in
            view.layer.render(in: context.cgContext)
        }
        let attachment = XCTAttachment(image: image)
        attachment.name = "iOS paragraph and heading physical alignment"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
