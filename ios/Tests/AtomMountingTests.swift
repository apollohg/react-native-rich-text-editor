import XCTest

final class AtomMountingTests: XCTestCase {
    private static let lookupViewport = CGSize(width: 320, height: 240)
    private static let lookupWiderViewportWidth: CGFloat = 400
    private static let lookupParagraphCount = 32

    func testScrollingRichDocumentWithoutAtomsDoesNotRepeatAttachmentEnumeration() throws {
        let editorId = makeV2Editor()
        let editor = RichTextEditorView(frame: CGRect(origin: .zero, size: Self.lookupViewport))
        let window = hostEditorView(editor, size: Self.lookupViewport)
        defer {
            editor.editorId = 0
            destroyV2Editor(id: editorId)
            editor.removeFromSuperview()
            window.isHidden = true
        }
        editor.editorId = editorId
        editor.setContent(html: (0..<Self.lookupParagraphCount).map {
            #"<p><strong>Paragraph \#($0) 👩‍💻</strong> <em>é</em> <a href="https://example.com">linked text</a> fills the scrolling fixture.</p>"#
        }.joined())
        editor.layoutIfNeeded()
        editor.textView.layoutManager.ensureLayout(for: editor.textView.textContainer)
        flushMainQueue()
        XCTAssertTrue(editor.atomLayoutPositions().isEmpty)
        let snapshot = NSAttributedString(attributedString: editor.textView.textStorage)
        let caret = editor.textView.selectedRange
        let height = editor.textView.contentSize.height
        let travel = height - editor.textView.bounds.height
        XCTAssertGreaterThan(travel, editor.textView.bounds.height,
            "The real editor must overflow its viewport before scroll work is measured")
        var scanCount = 0
        var emittedWidths: [CGFloat] = []
        let previousObserver = PreparedProseInstrumentation.tableStageObserverForTesting
        PreparedProseInstrumentation.tableStageObserverForTesting = { stage, start, end in
            if stage == .atomAttachmentEnumeration { scanCount += 1 }
            previousObserver?(stage, start, end)
        }
        defer { PreparedProseInstrumentation.tableStageObserverForTesting = previousObserver }
        editor.onAtomContentWidthChange = { emittedWidths.append($0) }

        for fraction in [CGFloat(0.25), 0.5, 0.75, 1] {
            let target = CGPoint(x: 0, y: travel * fraction)
            editor.textView.contentOffset = target
            XCTAssertEqual(editor.textView.contentOffset.x, target.x)
            XCTAssertEqual(editor.textView.contentOffset.y, target.y, accuracy: 1 / window.screen.scale)
            XCTAssertTrue(editor.atomLayoutPositions().isEmpty)
            XCTAssertEqual(editor.textView.contentSize.height, height,
                "Scrolling an unchanged fully laid-out document must preserve its content height")
        }
        editor.frame.size.width = Self.lookupWiderViewportWidth
        editor.setNeedsLayout()
        editor.layoutIfNeeded()

        XCTAssertGreaterThan(emittedWidths.count, 0, "Actual viewport changes must still publish atom layout width")
        XCTAssertGreaterThan(try XCTUnwrap(emittedWidths.last), try XCTUnwrap(emittedWidths.first),
            "The final width change must still reach atom layout subscribers without an attachment scan")
        XCTAssertEqual(scanCount, 0,
            "Unchanged rich text without atoms must not rescan all attributed runs for each viewport callback; scans=\(scanCount), UTF16=\(snapshot.length)")
        XCTAssertTrue(snapshot.isEqual(to: editor.textView.textStorage), "Scrolling must preserve every rendered attribute")
        XCTAssertEqual(editor.textView.selectedRange, caret)
        XCTAssertTrue(editor.atomLayoutPositions().isEmpty)
    }

    func testAttachmentLookupTracksAttributeOnlyEditsWithoutDelegateAndMutableHeights() throws {
        let editor = RichTextEditorView(frame: CGRect(origin: .zero, size: Self.lookupViewport))
        let originalText = LayoutConstants.objectReplacementCharacter + " ordinary text"
        editor.textView.textStorage.setAttributedString(NSAttributedString(string: originalText))
        editor.layoutIfNeeded()
        XCTAssertTrue(editor.atomLayoutPositions().isEmpty)
        let attachment = AtomBlockAttachment(atomKey: "counter:live", nodeType: "counter", docPos: 1, reservedHeight: 40)
        let previousDelegate = editor.textView.textStorage.delegate
        editor.textView.textStorage.delegate = nil
        defer { editor.textView.textStorage.delegate = previousDelegate }
        let atomRange = NSRange(location: 0, length: 1)
        editor.textView.textStorage.addAttribute(.attachment, value: attachment, range: atomRange)

        let initial = try XCTUnwrap(editor.atomLayoutPositions().first)
        XCTAssertEqual(initial["key"], attachment.atomKey,
            "Attribute-only attachment edits must invalidate an empty lookup even when the Rust render path disables the storage delegate")
        XCTAssertEqual(initial["height"], Double(attachment.reservedHeight))
        XCTAssertEqual(editor.textView.textStorage.string, originalText,
            "An attribute-only attachment update must not alter the live characters")
        attachment.reservedHeight = 90
        XCTAssertEqual(try XCTUnwrap(editor.atomLayoutPositions().first)["height"], 90.0,
            "Lookup reuse must retain the live attachment reference rather than cache its mutable height")

        editor.textView.textStorage.removeAttribute(.attachment, range: atomRange)
        XCTAssertTrue(editor.atomLayoutPositions().isEmpty, "Removing the last attachment must clear its cached range")
        editor.textView.textStorage.replaceCharacters(in: NSRange(location: 0, length: 0),
            with: NSAttributedString(attachment: attachment))
        XCTAssertEqual(try XCTUnwrap(editor.atomLayoutPositions().first)["key"], attachment.atomKey)
        editor.textView.textStorage.setAttributedString(NSAttributedString(string: "replaced document"))
        XCTAssertTrue(editor.atomLayoutPositions().isEmpty, "A whole-document replacement must discard the old attachment")
    }

    func testAttachmentLookupPreservesMixedRenderRangesAndDuplicateKeyBehavior() throws {
        let editor = RichTextEditorView(frame: CGRect(origin: .zero, size: Self.lookupViewport))
        let tableKey = "lookup-table"
        let rendered = RenderBridge.renderElements(fromArray: [
            ["type": "blockStart", "nodeType": "paragraph", "depth": 0],
            ["type": "textRun", "text": "Before ", "marks": []],
            ["type": "opaqueInlineAtom", "nodeType": "mention", "label": "@Alice", "docPos": 8],
            ["type": "blockEnd"],
            ["type": "opaqueBlockAtom", "nodeType": "card", "label": "Card", "docPos": 10],
            ["type": "voidBlock", "nodeType": "image", "docPos": 11,
                "attrs": ["src": "", "width": 140, "height": 80]],
            ["type": "voidBlock", "nodeType": "horizontal_rule", "docPos": 12],
            ["type": "table", "tableId": tableKey]
        ], baseFont: editor.textView.baseFont, textColor: .label, theme: nil, rootTableIDs: [tableKey])
        editor.textView.textStorage.setAttributedString(rendered)
        XCTAssertTrue(editor.atomLayoutPositions().isEmpty,
            "Opaque labels, images, rules and root table anchors are not React atom host attachments")
        let first = AtomBlockAttachment(atomKey: "counter:duplicate", nodeType: "counter", docPos: 13, reservedHeight: 40)
        let last = AtomBlockAttachment(atomKey: first.atomKey, nodeType: first.nodeType, docPos: 14, reservedHeight: 70)
        let value = NSMutableAttributedString(attributedString: rendered)
        value.append(NSAttributedString(attachment: first))
        value.append(NSAttributedString(string: "\n"))
        let lastOffset = value.length
        value.append(NSAttributedString(attachment: last))
        editor.textView.textStorage.setAttributedString(value)
        editor.layoutIfNeeded()
        XCTAssertEqual(editor.atomLayoutPositions().count, 1,
            "The full attachment dictionary must retain its existing last-key-wins contract")
        XCTAssertEqual(try XCTUnwrap(editor.atomLayoutPositions().first)["height"], Double(last.reservedHeight))
        editor.setAtomHeight(key: first.atomKey, height: 90)
        XCTAssertEqual(first.reservedHeight, 90,
            "The single attachment lookup must continue selecting the first matching key")
        XCTAssertEqual(last.reservedHeight, 70)
        XCTAssertEqual(try XCTUnwrap(editor.atomLayoutPositions().first)["height"], Double(last.reservedHeight))

        let prefix = "Added 👩‍💻\n"
        editor.textView.layoutManager.ensureLayout(for: editor.textView.textContainer)
        let beforePrefix = NSAttributedString(attributedString: editor.textView.textStorage)
        editor.textView.textStorage.replaceCharacters(in: NSRange(location: 0, length: 0), with: prefix)
        let shiftedOffset = lastOffset + prefix.utf16.count
        let shiftedRange = NSRange(location: shiftedOffset, length: 1)
        editor.textView.layoutManager.ensureLayout(forCharacterRange: shiftedRange)
        let glyphs = editor.textView.layoutManager.glyphRange(forCharacterRange: shiftedRange, actualCharacterRange: nil)
        let expectedRect = editor.textView.layoutManager.boundingRect(forGlyphRange: glyphs, in: editor.textView.textContainer)
        let shifted = try XCTUnwrap(editor.atomLayoutPositions().first)
        XCTAssertEqual(shifted["y"] as? Double, Double(editor.textView.textContainerInset.top + expectedRect.minY),
            "Retained atom positions must be recomputed from their updated live range after a structural prefix edit")
        let retained = editor.textView.textStorage.attributedSubstring(from: NSRange(location: prefix.utf16.count, length: rendered.length))
        XCTAssertEqual(retained.string, rendered.string)
        XCTAssertTrue(editor.textView.textStorage.attributedSubstring(from:
            NSRange(location: prefix.utf16.count, length: beforePrefix.length)).isEqual(to: beforePrefix),
            "The retained mixed suffix must preserve every opaque label, image, rule, table marker and attributed style")
        XCTAssertTrue((editor.textView.textStorage.attribute(.attachment, at: shiftedOffset, effectiveRange: nil) as? AtomBlockAttachment) === last)
        XCTAssertEqual(retained.attribute(RenderBridgeAttributes.rootTableMarker, at: rendered.length - 1, effectiveRange: nil) as? String, tableKey)
    }

    func testAttachmentLookupObservesEveryPendingStorageEditInsideOneBatch() throws {
        let editor = RichTextEditorView(frame: CGRect(origin: .zero, size: Self.lookupViewport))
        let original = installAtom(key: "counter:batch", height: 40, in: editor)
        let initial = try XCTUnwrap(editor.atomAttachmentEntries()[original.atomKey])
        XCTAssertTrue(initial.0 === original)
        XCTAssertEqual(initial.1, NSRange(location: 0, length: 1))
        let prefix = "Pending 👩‍💻\n"
        let expectedRange = NSRange(location: prefix.utf16.count, length: 1)
        let replacement = AtomBlockAttachment(atomKey: original.atomKey, nodeType: original.nodeType,
            docPos: original.docPos, reservedHeight: 90)
        do {
            editor.textView.textStorage.beginEditing()
            defer { editor.textView.textStorage.endEditing() }
            editor.textView.textStorage.replaceCharacters(in: NSRange(location: 0, length: 0),
                with: NSAttributedString(string: prefix, attributes: [.font: editor.textView.baseFont]))
            let shifted = try XCTUnwrap(editor.atomAttachmentEntries()[original.atomKey])
            XCTAssertTrue(shifted.0 === original)
            XCTAssertEqual(shifted.1, expectedRange,
                "Reads during a pending edit must see the current UTF16 attachment range before editing notifications are delivered")
            editor.textView.textStorage.removeAttribute(.attachment, range: expectedRange)
            XCTAssertTrue(editor.atomAttachmentEntries().isEmpty,
                "A second edit in the same open batch must not reuse the preceding lookup")
            editor.textView.textStorage.addAttribute(.attachment, value: replacement, range: expectedRange)
            let reattached = try XCTUnwrap(editor.atomAttachmentEntries()[replacement.atomKey])
            XCTAssertTrue(reattached.0 === replacement)
            XCTAssertEqual(reattached.1, expectedRange)
        }
        let final = try XCTUnwrap(editor.atomAttachmentEntries()[replacement.atomKey])
        XCTAssertTrue(final.0 === replacement)
        XCTAssertEqual(final.1, expectedRange)
        XCTAssertEqual(try XCTUnwrap(editor.atomLayoutPositions().first)["height"], Double(replacement.reservedHeight))
    }

    func testFabricMountIndexesIgnoreAtomAndInternalSubviews() {
        let editor = NativeEditorExpoView()
        let first = UIView()
        let atom = atomChild(key: "counter:1", height: 40)
        let second = UIView()

        editor.mountChildComponentView(first, index: 0)
        editor.mountChildComponentView(atom, index: 1)
        editor.mountChildComponentView(second, index: 2)

        XCTAssertEqual(editor.subviews.count, 3)
        XCTAssertTrue(editor.subviews[0] === editor.richTextView)
        XCTAssertTrue(editor.subviews[1] === first)
        XCTAssertTrue(editor.subviews[2] === second)

        editor.unmountChildComponentView(first, index: 0)
        XCTAssertTrue(editor.subviews[0] === editor.richTextView)
        XCTAssertTrue(editor.subviews[1] === second)
    }

    func testPrefixedReactChildIsReparentedIntoTextView() throws {
        let editor = NativeEditorExpoView()
        editor.frame = CGRect(x: 0, y: 0, width: 320, height: 240)
        installAtom(key: "counter:1", height: 80, in: editor.richTextView)
        editor.layoutIfNeeded()

        let child = atomChild(key: "counter:1", height: 80)
        editor.mountChildComponentView(child, index: 0)

        let container = try XCTUnwrap(child.superview as? AtomHostContainerView)
        XCTAssertTrue(container.superview === editor.richTextView.textView)
        XCTAssertEqual(container.atomKey, "counter:1")
    }

    func testFabricNativeIdMountsReactChildIntoAtomContainer() throws {
        let editor = NativeEditorExpoView()
        editor.frame = CGRect(x: 0, y: 0, width: 320, height: 240)
        installAtom(key: "counter:1", height: 80, in: editor.richTextView)
        editor.layoutIfNeeded()

        let componentViewClass = try XCTUnwrap(
            NSClassFromString("RCTViewComponentView") as? NSObject.Type
        )
        let child = try XCTUnwrap(componentViewClass.init() as? UIView)
        child.frame = CGRect(x: 0, y: 0, width: 100, height: 80)
        child.setValue("prose-atom:counter:1", forKey: "nativeId")
        editor.mountChildComponentView(child, index: 0)

        let container = try XCTUnwrap(child.superview as? AtomHostContainerView)
        XCTAssertTrue(container.superview === editor.richTextView.textView)
    }

    func testChildrenBindToAttachmentsByKeyInsteadOfMountOrder() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 300))
        installAtoms(
            [
                AtomBlockAttachment(
                    atomKey: "counter:1",
                    nodeType: "counter",
                    docPos: 1,
                    reservedHeight: 60
                ),
                AtomBlockAttachment(
                    atomKey: "counter:2",
                    nodeType: "counter",
                    docPos: 2,
                    reservedHeight: 90
                )
            ],
            in: editor
        )

        let second = atomChild(key: "counter:2", height: 90)
        let first = atomChild(key: "counter:1", height: 60)
        editor.mountAtomChild(second, atomKey: "counter:2")
        editor.mountAtomChild(first, atomKey: "counter:1")

        XCTAssertTrue(try XCTUnwrap(editor.atomHostContainer(for: "counter:1")).hostedView === first)
        XCTAssertTrue(try XCTUnwrap(editor.atomHostContainer(for: "counter:2")).hostedView === second)

        let unmatched = atomChild(key: "missing", height: 50)
        editor.mountAtomChild(unmatched, atomKey: "missing")
        XCTAssertTrue(try XCTUnwrap(editor.atomHostContainer(for: "missing")).isHidden)
    }

    func testChildHeightChangeUpdatesSpacerAndInvalidatesLayoutOnce() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let attachment = installAtom(key: "counter:1", height: 40, in: editor)
        let child = atomChild(key: "counter:1", height: 96)

        editor.mountAtomChild(child, atomKey: "counter:1")
        let invalidations = editor.atomLayoutInvalidationCountForTesting
        try XCTUnwrap(editor.atomHostContainer(for: "counter:1")).layoutIfNeeded()

        XCTAssertEqual(attachment.reservedHeight, 96)
        XCTAssertEqual(editor.measuredAtomHeight(for: "counter:1"), 96)
        XCTAssertEqual(invalidations, 1)
        XCTAssertEqual(editor.atomLayoutInvalidationCountForTesting, invalidations)
    }

    func testUnmeasuredChildKeepsEstimateUntilReactSuppliesBounds() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let attachment = installAtom(key: "counter:1", height: 40, in: editor)
        let child = UIView(frame: .zero)
        editor.mountAtomChild(child, atomKey: "counter:1")
        let container = try XCTUnwrap(editor.atomHostContainer(for: "counter:1"))
        container.layoutIfNeeded()

        XCTAssertNil(editor.measuredAtomHeight(for: "counter:1"))
        XCTAssertEqual(attachment.reservedHeight, 40)

        child.frame = CGRect(x: 0, y: 0, width: 280, height: 0)
        container.setNeedsLayout()
        container.layoutIfNeeded()

        XCTAssertEqual(editor.measuredAtomHeight(for: "counter:1"), 0)
        XCTAssertEqual(attachment.reservedHeight, 0)
    }

    func testChildCanCollapseToZeroAndExpandAgain() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let attachment = installAtom(key: "counter:1", height: 40, in: editor)
        let child = atomChild(key: "counter:1", height: 96)
        editor.mountAtomChild(child, atomKey: "counter:1")
        let container = try XCTUnwrap(editor.atomHostContainer(for: "counter:1"))

        for height in [CGFloat(0), 72] {
            child.frame.size.height = height
            container.setNeedsLayout()
            container.layoutIfNeeded()

            XCTAssertEqual(editor.measuredAtomHeight(for: "counter:1"), height)
            XCTAssertEqual(attachment.reservedHeight, height)
        }
    }

    func testChildMountedBeforeAttachmentSuppliesMeasuredHeightToLaterRender() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        XCTAssertTrue(editor.applyAtomRenderConfiguration(AtomRenderConfiguration(
            registeredNodeTypes: ["counter"],
            estimatedHeights: ["counter": 40],
            measuredHeights: [:]
        )))

        let child = atomChild(key: "counter:0", height: 96)
        editor.mountAtomChild(child, atomKey: "counter:0")

        XCTAssertEqual(editor.measuredAtomHeight(for: "counter:0"), 96)
        let rendered = RenderBridge.renderElements(
            fromJSON: """
            [{"type":"voidBlock","nodeType":"counter","docPos":1}]
            """,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: nil,
            atomConfiguration: editor.textView.atomRenderConfiguration
        )
        let attachment = try XCTUnwrap(
            rendered.attribute(.attachment, at: 0, effectiveRange: nil) as? AtomBlockAttachment
        )

        XCTAssertEqual(attachment.reservedHeight, 96)
        editor.textView.attributedText = rendered
        XCTAssertTrue(editor.unmountAtomChild(child))
        XCTAssertEqual(attachment.reservedHeight, 40)
    }

    func testAtomContainerUsesAttachmentLayoutRectIncludingTextContainerInset() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let attachment = installAtom(key: "counter:1", height: 72, in: editor)
        let child = atomChild(key: "counter:1", height: 72)
        editor.mountAtomChild(child, atomKey: "counter:1")
        editor.layoutIfNeeded()

        let characterRange = NSRange(location: 0, length: 1)
        editor.textView.layoutManager.ensureLayout(for: editor.textView.textContainer)
        let glyphRange = editor.textView.layoutManager.glyphRange(
            forCharacterRange: characterRange,
            actualCharacterRange: nil
        )
        let attachmentRect = editor.textView.layoutManager.boundingRect(
            forGlyphRange: glyphRange,
            in: editor.textView.textContainer
        )
        let padding = editor.textView.textContainer.lineFragmentPadding
        let expected = CGRect(
            x: editor.textView.textContainerInset.left + padding,
            y: editor.textView.textContainerInset.top + attachmentRect.minY,
            width: editor.textView.textContainer.size.width - (padding * 2),
            height: attachment.reservedHeight
        )

        XCTAssertEqual(try XCTUnwrap(editor.atomHostContainer(for: "counter:1")).frame, expected)
    }

    func testAtomContainerPreservesRenderedParagraphSpacing() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let value = RenderBridge.renderElements(
            fromJSON: """
            [
                {"type":"blockStart","nodeType":"paragraph","depth":0},
                {"type":"textRun","text":"Above","marks":[]},
                {"type":"blockEnd"},
                {"type":"voidBlock","nodeType":"counterCard","docPos":7}
            ]
            """,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: EditorTheme(dictionary: [
                "paragraph": ["spacingAfter": 18]
            ]),
            atomConfiguration: AtomRenderConfiguration(
                registeredNodeTypes: ["counterCard"],
                estimatedHeights: ["counterCard": 72],
                measuredHeights: [:]
            )
        )
        editor.textView.textStorage.setAttributedString(value)
        let attachmentRange = (value.string as NSString).range(of: "\u{FFFC}")
        let attachment = try XCTUnwrap(
            value.attribute(.attachment, at: attachmentRange.location, effectiveRange: nil)
                as? AtomBlockAttachment
        )
        let child = atomChild(key: attachment.atomKey, height: 72)
        editor.mountAtomChild(child, atomKey: attachment.atomKey)
        editor.layoutIfNeeded()

        let layoutManager = editor.textView.layoutManager
        layoutManager.ensureLayout(for: editor.textView.textContainer)
        let precedingGlyph = layoutManager.glyphIndexForCharacter(at: 0)
        let precedingLineRect = layoutManager.lineFragmentUsedRect(
            forGlyphAt: precedingGlyph,
            effectiveRange: nil
        )
        let precedingLineBottom = editor.textView.textContainerInset.top
            + precedingLineRect.maxY
        let atomTop = try XCTUnwrap(editor.atomHostContainer(for: attachment.atomKey)).frame.minY

        XCTAssertEqual(atomTop - precedingLineBottom, 18, accuracy: 0.5)
    }

    func testAtomContainerUsesAtomThemeSpacingBeforeFollowingParagraph() throws {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 320))
        let value = RenderBridge.renderElements(
            fromJSON: """
            [
                {"type":"voidBlock","nodeType":"counterCard","docPos":1},
                {"type":"blockStart","nodeType":"paragraph","depth":0},
                {"type":"textRun","text":"Below","marks":[]},
                {"type":"blockEnd"}
            ]
            """,
            baseFont: .systemFont(ofSize: 16),
            textColor: .label,
            theme: EditorTheme(dictionary: [
                "text": ["spacingAfter": 11],
                "paragraph": ["spacingAfter": 29]
            ]),
            atomConfiguration: AtomRenderConfiguration(
                registeredNodeTypes: ["counterCard"],
                estimatedHeights: ["counterCard": 72],
                measuredHeights: [:]
            )
        )
        editor.textView.textStorage.setAttributedString(value)
        let attachmentRange = (value.string as NSString).range(of: "\u{FFFC}")
        let attachment = try XCTUnwrap(
            value.attribute(.attachment, at: attachmentRange.location, effectiveRange: nil)
                as? AtomBlockAttachment
        )
        editor.mountAtomChild(
            atomChild(key: attachment.atomKey, height: 72),
            atomKey: attachment.atomKey
        )
        editor.layoutIfNeeded()

        let layoutManager = editor.textView.layoutManager
        layoutManager.ensureLayout(for: editor.textView.textContainer)
        let atomBottom = try XCTUnwrap(
            editor.atomHostContainer(for: attachment.atomKey)
        ).frame.maxY
        let followingCharacter = (value.string as NSString).range(of: "Below").location
        let followingGlyph = layoutManager.glyphIndexForCharacter(at: followingCharacter)
        let followingLineRect = layoutManager.lineFragmentUsedRect(
            forGlyphAt: followingGlyph,
            effectiveRange: nil
        )
        let followingLineTop = editor.textView.textContainerInset.top + followingLineRect.minY

        XCTAssertEqual(followingLineTop - atomBottom, 11, accuracy: 0.5)
    }

    func testUnmountClearsHostAndMeasuredHeight() {
        let editor = NativeEditorExpoView()
        editor.frame = CGRect(x: 0, y: 0, width: 320, height: 240)
        let attachment = installAtom(key: "counter:1", height: 40, in: editor.richTextView)
        let child = atomChild(key: "counter:1", height: 90)
        editor.mountChildComponentView(child, index: 0)

        editor.unmountChildComponentView(child, index: 0)

        XCTAssertNil(editor.richTextView.atomHostContainer(for: "counter:1"))
        XCTAssertNil(editor.richTextView.measuredAtomHeight(for: "counter:1"))
        XCTAssertNil(child.superview)
        XCTAssertEqual(attachment.reservedHeight, 40)
    }

    func testUnmountWithoutEstimateKeepsMeasuredHeightWhenNoFallbackExists() {
        let editor = NativeEditorExpoView()
        editor.frame = CGRect(x: 0, y: 0, width: 320, height: 240)
        let attachment = installAtom(key: "counter:1", height: 0, in: editor.richTextView)
        XCTAssertTrue(editor.richTextView.applyAtomRenderConfiguration(
            AtomRenderConfiguration(
                registeredNodeTypes: ["counter"],
                estimatedHeights: [:],
                measuredHeights: [:]
            )
        ))
        let child = atomChild(key: "counter:1", height: 240)
        editor.mountChildComponentView(child, index: 0)
        XCTAssertEqual(attachment.reservedHeight, 240)

        editor.unmountChildComponentView(child, index: 0)

        XCTAssertEqual(attachment.reservedHeight, 240)
    }

    func testAtomContentWidthEventOnlyFiresWhenWidthChanges() {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        var widths: [CGFloat] = []
        editor.onAtomContentWidthChange = { widths.append($0) }

        editor.layoutIfNeeded()
        editor.layoutIfNeeded()
        editor.frame.size.width = 400
        editor.layoutIfNeeded()

        XCTAssertEqual(widths.count, 2)
        XCTAssertNotEqual(widths[0], widths[1])
    }

    func testAtomLayoutReportsReservedHeightAndScrollAtStableWidth() {
        let editor = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        installAtom(key: "counter:1", height: 90, in: editor)
        editor.layoutIfNeeded()
        let positions = editor.atomLayoutPositions()
        XCTAssertEqual(positions.count, 1)
        XCTAssertEqual(positions[0]["key"], "counter:1")
        XCTAssertEqual(positions[0]["height"], 90.0)
        var widths: [CGFloat] = []
        editor.onAtomContentWidthChange = { widths.append($0) }
        editor.emitAtomContentWidthIfAvailable(force: true)
        editor.textView.contentOffset.y = 30
        XCTAssertGreaterThan(widths.count, 1)
        XCTAssertEqual(widths.first, widths.last)
        XCTAssertEqual(editor.atomLayoutPositions(), positions)
    }

    @discardableResult
    private func installAtom(
        key: String,
        height: CGFloat,
        in editor: RichTextEditorView
    ) -> AtomBlockAttachment {
        let attachment = AtomBlockAttachment(
            atomKey: key,
            nodeType: "counter",
            docPos: 1,
            reservedHeight: height
        )
        installAtoms([attachment], in: editor)
        return attachment
    }

    private func installAtoms(
        _ attachments: [AtomBlockAttachment],
        in editor: RichTextEditorView
    ) {
        let value = NSMutableAttributedString()
        for attachment in attachments {
            value.append(NSAttributedString(attachment: attachment))
        }
        editor.textView.textStorage.setAttributedString(value)
        editor.setNeedsLayout()
    }

    private func atomChild(key: String, height: CGFloat) -> UIView {
        let child = UIView(frame: CGRect(x: 0, y: 0, width: 100, height: height))
        child.accessibilityIdentifier = "prose-atom:\(key)"
        return child
    }
}
