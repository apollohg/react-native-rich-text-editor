import ExpoModulesCore
import XCTest

extension RichTextEditorViewTests {
    func testEditorThemeContentInsetsApplyToTextView() {
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        let defaultInset = view.textView.textContainerInset
        let theme = EditorTheme(dictionary: [
            "contentInsets": [
                "top": 12,
                "right": 16,
                "bottom": 20,
                "left": 24
            ]
        ])

        view.applyTheme(theme)

        XCTAssertEqual(view.textView.textContainerInset.top, 12, accuracy: 0.1)
        XCTAssertEqual(view.textView.textContainerInset.left, 24, accuracy: 0.1)
        XCTAssertEqual(view.textView.textContainerInset.bottom, 20, accuracy: 0.1)
        XCTAssertEqual(view.textView.textContainerInset.right, 16, accuracy: 0.1)

        view.applyTheme(nil)

        XCTAssertEqual(view.textView.textContainerInset.top, defaultInset.top, accuracy: 0.1)
        XCTAssertEqual(view.textView.textContainerInset.left, defaultInset.left, accuracy: 0.1)
        XCTAssertEqual(view.textView.textContainerInset.bottom, defaultInset.bottom, accuracy: 0.1)
        XCTAssertEqual(view.textView.textContainerInset.right, defaultInset.right, accuracy: 0.1)
    }

    func testEditorThemeZeroContentInsetsRemoveLeadingTextGutter() {
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.textView.placeholder = "Type here"
        view.textView.applyRenderJSON("""
        [
        {"type":"blockStart","nodeType":"paragraph","depth":0},
        {"type":"textRun","text":"\\u200B","marks":[]},
        {"type":"blockEnd"}
        ]
        """)

        view.applyTheme(EditorTheme(dictionary: [
            "contentInsets": [
                "top": 0,
                "right": 0,
                "bottom": 0,
                "left": 0
            ]
        ]))
        view.layoutIfNeeded()
        view.textView.layoutIfNeeded()

        XCTAssertEqual(view.textView.textContainer.lineFragmentPadding, 0, accuracy: 0.1)
        XCTAssertEqual(view.textView.placeholderFrameForTesting().minX, 0, accuracy: 0.1)
    }

    func testEditorThemeBorderRadiusAppliesToEditorContainer() {
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        let theme = EditorTheme(dictionary: [
            "backgroundColor": "#d7e4ff",
            "borderRadius": 18
        ])

        view.applyTheme(theme)

        XCTAssertEqual(view.layer.cornerRadius, 18, accuracy: 0.1)
        XCTAssertTrue(view.clipsToBounds)

        view.applyTheme(nil)

        XCTAssertEqual(view.layer.cornerRadius, 0, accuracy: 0.1)
        XCTAssertFalse(view.clipsToBounds)
    }

    func testRemoteSelectionOverlayShowsFocusedCaretWithoutBadge() {
        let editorId = makeV2Editor(
            configJson: #"{"initialization":{"type":"localEmpty"},"policy":{"allowBase64Images":true}}"#
        )
        defer { destroyV2Editor(id: editorId) }

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.editorId = editorId
        view.setContent(html: "<p>Hello world</p>")
        view.layoutIfNeeded()

        let docPos = EditorV2Shadow.scalarToDoc(id: editorId, scalar: 6)
        view.setRemoteSelections([
            RemoteSelectionDecoration(
                clientId: "7",
                anchor: docPos,
                head: docPos,
                color: .systemOrange,
                name: "Alice",
                isFocused: true
            )
        ])
        view.layoutIfNeeded()

        let overlaySubviews = view.remoteSelectionOverlaySubviewsForTesting()
        let labels = overlaySubviews.compactMap { $0 as? UILabel }
        let nonLabels = overlaySubviews.filter { !($0 is UILabel) }
        let caretViews = nonLabels.filter { $0.bounds.height > 0 && $0.bounds.width > 0 }

        XCTAssertTrue(labels.isEmpty)
        XCTAssertEqual(nonLabels.count, 1, "expected one caret view for a collapsed focused remote selection")
        XCTAssertEqual(caretViews.count, 1, "expected the collapsed remote caret view to have a visible frame")
    }

    func testRemoteSelectionOverlayShowsFocusedCaretAtEndOfDocument() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.editorId = editorId
        view.setContent(html: "<p>Hello world</p>")
        view.layoutIfNeeded()

        let endDocPos = EditorV2Shadow.scalarToDoc(id: editorId, scalar: 11)
        view.setRemoteSelections([
            RemoteSelectionDecoration(
                clientId: "9",
                anchor: endDocPos,
                head: endDocPos,
                color: .systemGreen,
                name: "Bob",
                isFocused: true
            )
        ])
        view.layoutIfNeeded()

        let caretViews = view.remoteSelectionOverlaySubviewsForTesting()
            .filter { !($0 is UILabel) && $0.bounds.height > 0 && $0.bounds.width > 0 }
        XCTAssertEqual(caretViews.count, 1, "expected a visible caret view at the end of the document")
    }

    func testRemoteSelectionOverlayUsesCorrectWrappedVisualLine() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 140, height: 220))
        view.editorId = editorId
        view.setContent(html: "<p>Hello world from remote carets</p>")
        view.layoutIfNeeded()

        let targetScalar: UInt32 = 15
        let expectedCaretRect = view.textView.convert(
            view.textView.caretRect(
                for: PositionBridge.scalarToTextView(targetScalar, in: view.textView)
            ),
            to: view
        )
        XCTAssertGreaterThan(expectedCaretRect.minY, 0, "expected the target caret to be on a wrapped visual line")

        let docPos = EditorV2Shadow.scalarToDoc(id: editorId, scalar: targetScalar)
        view.setRemoteSelections([
            RemoteSelectionDecoration(
                clientId: "10",
                anchor: docPos,
                head: docPos,
                color: .systemPurple,
                name: "Wrapped",
                isFocused: true
            )
        ])
        view.layoutIfNeeded()

        let caretView = view.remoteSelectionOverlaySubviewsForTesting()
            .first { !($0 is UILabel) && $0.bounds.height > 0 && $0.bounds.width > 0 }
        XCTAssertNotNil(caretView)
        XCTAssertEqual(caretView?.frame.minY ?? 0, round(expectedCaretRect.minY), accuracy: 1)
    }

    func testRemoteSelectionOverlayHidesCaretAndBadgeForUnfocusedCollapsedSelection() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 200))
        view.editorId = editorId
        view.setContent(html: "<p>Hello world</p>")
        view.layoutIfNeeded()

        let docPos = EditorV2Shadow.scalarToDoc(id: editorId, scalar: 6)
        view.setRemoteSelections([
            RemoteSelectionDecoration(
                clientId: "8",
                anchor: docPos,
                head: docPos,
                color: .systemBlue,
                name: "Alice",
                isFocused: false
            )
        ])
        view.layoutIfNeeded()

        XCTAssertTrue(view.remoteSelectionOverlaySubviewsForTesting().isEmpty)
    }

    func testAccessoryToolbarSwitchesToMentionSuggestionMode() {
        let toolbar = EditorAccessoryToolbarView(frame: .zero)
        let baseHeight = toolbar.intrinsicContentSize.height

        toolbar.apply(mentionTheme: EditorMentionTheme(dictionary: [
            "suggestions": [
                "option": [
                    "backgroundColor": "#d7e4ff",
                    "textColor": "#1a2c48"
                ]
            ]
        ]))

        let didChange = toolbar.setMentionSuggestions([
            NativeMentionSuggestion(dictionary: [
                "key": "alice",
                "title": "Alice Chen",
                "subtitle": "Design",
                "label": "alice",
                "attrs": ["label": "alice"]
            ])!,
            NativeMentionSuggestion(dictionary: [
                "key": "ben",
                "title": "Ben Ortiz",
                "subtitle": "Engineering",
                "label": "ben",
                "attrs": ["label": "ben"]
            ])!
        ], trigger: "@")

        XCTAssertTrue(didChange)
        XCTAssertEqual(toolbar.intrinsicContentSize.height, baseHeight + 2)
        XCTAssertTrue(toolbar.isShowingMentionSuggestions)
        XCTAssertEqual(toolbar.mentionButtonAtForTesting(0)?.titleTextForTesting(), "@alice")
    }

    func testAccessoryToolbarKeepsRetainedMentionButtonsMountedWhileQueryNarrows() {
        let toolbar = EditorAccessoryToolbarView(frame: .zero)
        let alice = NativeMentionSuggestion(dictionary: [
            "key": "alice",
            "title": "Alice Chen",
            "subtitle": "Design",
            "label": "alice",
            "attrs": ["label": "alice"]
        ])!
        let ben = NativeMentionSuggestion(dictionary: [
            "key": "ben",
            "title": "Ben Ortiz",
            "subtitle": "Engineering",
            "label": "ben",
            "attrs": ["label": "ben"]
        ])!

        _ = toolbar.setMentionSuggestions([alice, ben], trigger: "@")
        let retainedButton = toolbar.mentionButtonAtForTesting(0)

        _ = toolbar.setMentionSuggestions([alice], trigger: "@")

        XCTAssertTrue(toolbar.mentionButtonAtForTesting(0) === retainedButton)
    }

    func testNativeEditorUsesZeroHeightAccessoryPlaceholderWhenToolbarIsInline() {
        let view = NativeEditorExpoView()

        view.setToolbarPlacement("inline")

        XCTAssertTrue(view.isUsingAccessoryPlaceholderForTesting())
        XCTAssertFalse(view.isUsingAccessoryToolbarForTesting())
        XCTAssertNotNil(view.inputAccessoryViewForTesting())
        XCTAssertEqual(view.inputAccessoryViewForTesting()?.intrinsicContentSize.height ?? -1, 0)
    }

    func testNativeEditorRestoresToolbarAccessoryWhenSwitchingBackToKeyboardPlacement() {
        let view = NativeEditorExpoView()

        view.setToolbarPlacement("inline")
        XCTAssertTrue(view.isUsingAccessoryPlaceholderForTesting())

        view.setToolbarPlacement("keyboard")

        XCTAssertTrue(view.isUsingAccessoryToolbarForTesting())
        XCTAssertFalse(view.isUsingAccessoryPlaceholderForTesting())
    }

    func testNativeEditorRemovesAccessoryPlaceholderWhenNotEditable() {
        let view = NativeEditorExpoView()

        view.setToolbarPlacement("inline")
        view.setEditable(false)

        XCTAssertNil(view.inputAccessoryViewForTesting())
    }

    func testNativeEditorToolbarFrameTapPreservesNextBlurOnce() {
        let view = NativeEditorExpoView()
        view.setToolbarFrameJson(#"{"x":20,"y":40,"width":100,"height":32}"#)

        XCTAssertFalse(view.shouldPreserveFocusAfterToolbarTouchForTesting())
        XCTAssertFalse(
            view.prepareOutsideTapForFocusHandlingForTesting(
                locationInWindow: CGPoint(x: 30, y: 50)
            )
        )
        XCTAssertTrue(view.shouldPreserveFocusAfterToolbarTouchForTesting())
        XCTAssertTrue(view.consumeToolbarFocusPreservationForTesting())
        XCTAssertFalse(view.shouldPreserveFocusAfterToolbarTouchForTesting())
        XCTAssertFalse(view.consumeToolbarFocusPreservationForTesting())
    }

    func testNativeEditorOutsideTapClearsToolbarPreservation() {
        let view = NativeEditorExpoView()

        view.markRecentToolbarTouchForTesting()
        XCTAssertTrue(view.shouldPreserveFocusAfterToolbarTouchForTesting())

        XCTAssertTrue(
            view.prepareOutsideTapForFocusHandlingForTesting(
                locationInWindow: CGPoint(x: 240, y: 260)
            )
        )
        XCTAssertFalse(view.shouldPreserveFocusAfterToolbarTouchForTesting())
    }

    func testInlineAccessoryPlaceholderRemainsAttachedAfterNativeEdit() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        _ = EditorV2Shadow.setHtml(id: editorId, html: "<p>Hello</p>")
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: 5, scalarHead: 5)

        let view = NativeEditorExpoView()
        view.setEditorId(editorId)
        view.setToolbarPlacement("inline")
        view.richTextView.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)

        view.richTextView.textView.insertText("!")

        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), "<p>Hello!</p>")
        XCTAssertTrue(view.isUsingAccessoryPlaceholderForTesting())
    }

}

extension RichTextEditorViewTests {
    func testVersionedThemeClearingPreservesDocumentAndSelection() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 220))
        view.editorId = editorId
        view.setContent(html: "<p>Hello</p>")
        EditorV2Shadow.setSelectionScalar(id: editorId, scalarAnchor: 2, scalarHead: 2)
        view.textView.applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
        let text = view.textView.text
        let selection = view.textView.selectedRange
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"content":{"borderTopLeftRadius":20,"borderTopWidth":3,"paddingLeft":8},"paragraph":{"backgroundColor":"#ff0000ff","paddingLeft":9},"placeholder":{"fontSize":24}}}
        """))
        XCTAssertTrue(view.applyTheme(theme))
        view.layoutIfNeeded()
        XCTAssertEqual(view.textView.text, text)
        XCTAssertEqual(view.textView.selectedRange, selection)
        XCTAssertNotNil(view.layer.mask)
        XCTAssertTrue(view.applyTheme(nil))
        view.layoutIfNeeded()
        XCTAssertNil(view.layer.mask)
        XCTAssertNil(view.textView.styleContentView.box)
        XCTAssertNil(view.textView.textStorage.attribute(editorStyleBoxesAttribute, at: 0, effectiveRange: nil))
        XCTAssertEqual(view.textView.text, text)
        XCTAssertEqual(view.textView.selectedRange, selection)
    }
}


extension RichTextEditorViewTests {
    func testDocumentStyleBackgroundStaysBoundedAndPreservesScrolledPixels() throws {
        let viewport = CGSize(width: 320, height: 220)
        let documentHeight: CGFloat = 142_016
        let window = makeTestWindow(frame: CGRect(origin: .zero, size: viewport))
        let scroll = UIScrollView(frame: window.bounds)
        scroll.contentInsetAdjustmentBehavior = .never
        scroll.contentSize = CGSize(width: viewport.width, height: documentHeight)
        window.addSubview(scroll)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        let background = EditorStyleBoxView()
        background.backgroundColor = .clear
        background.box = EditorStyleBox([
            "backgroundColor": "#fafafaff", "borderWidth": 4, "borderColor": "#123456ff",
            "borderTopLeftRadius": 24, "borderBottomRightRadius": 32
        ])
        scroll.addSubview(background)
        let document = CGRect(origin: .zero, size: scroll.contentSize)
        background.documentBounds = document
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.preferredRange = .standard
        for offset in [CGFloat.zero, documentHeight / 2, documentHeight - viewport.height] {
            scroll.contentOffset = CGPoint(x: 0, y: offset)
            XCTAssertEqual(background.frame, CGRect(origin: scroll.contentOffset, size: viewport))
            XCTAssertEqual(background.bounds.size, viewport)
            let renderer = UIGraphicsImageRenderer(size: viewport, format: format)
            let actual = renderer.image { _ in background.draw(background.bounds) }
            let attachment = XCTAttachment(image: actual)
            attachment.name = "Document background at offset \(Int(offset))"
            attachment.lifetime = .keepAlways
            add(attachment)
            let expected = renderer.image { context in
                background.box?.draw(in: document.offsetBy(dx: 0, dy: -offset), context: context.cgContext)
            }
            XCTAssertEqual(try XCTUnwrap(actual.cgImage?.dataProvider?.data) as Data,
                           try XCTUnwrap(expected.cgImage?.dataProvider?.data) as Data,
                           "Background, borders and corners remain anchored at document offset \(offset)")
        }
    }

    func testAutoGrowStyleBackgroundTracksAncestorViewportAndVisibility() {
        let viewport = CGSize(width: 320, height: 220)
        let documentHeight: CGFloat = 142_016
        let window = makeTestWindow(frame: CGRect(origin: .zero, size: viewport))
        let scroll = UIScrollView(frame: window.bounds)
        scroll.contentInsetAdjustmentBehavior = .never
        scroll.contentSize = CGSize(width: viewport.width, height: documentHeight)
        let content = UIView(frame: CGRect(origin: .zero, size: scroll.contentSize))
        let background = EditorStyleBoxView()
        content.addSubview(background)
        background.documentBounds = content.bounds
        XCTAssertEqual(background.bounds.size, .zero, "Detached content must have no backing")
        scroll.addSubview(content)
        window.addSubview(scroll)
        window.makeKeyAndVisible()
        defer { window.isHidden = true }
        for offset in [CGFloat.zero, documentHeight / 2, documentHeight - viewport.height] {
            scroll.contentOffset = CGPoint(x: 0, y: offset)
            XCTAssertEqual(background.frame, CGRect(origin: scroll.contentOffset, size: viewport))
        }
        scroll.contentOffset = .zero
        let originalCenter = content.center
        content.center.x += viewport.width / 2
        XCTAssertEqual(background.bounds.width, viewport.width / 2, "Moving an ancestor clips the backing immediately")
        content.center = originalCenter
        content.transform = CGAffineTransform(scaleX: 2, y: 2)
        XCTAssertEqual(background.bounds.width, viewport.width / 2, "Ancestor transforms refresh the visible geometry")
        content.transform = .identity
        XCTAssertEqual(background.bounds.size, viewport)
        content.isHidden = true
        XCTAssertEqual(background.bounds.size, .zero)
        content.isHidden = false
        XCTAssertEqual(background.bounds.size, viewport, "Unhiding must restore the backing without a scroll")
        content.alpha = 0
        XCTAssertEqual(background.bounds.size, .zero)
        content.alpha = 1
        XCTAssertEqual(background.bounds.size, viewport)
        scroll.frame.size.width /= 2
        XCTAssertEqual(background.bounds.width, scroll.bounds.width)
        scroll.clipsToBounds = false
        XCTAssertEqual(background.bounds.width, viewport.width, "Removing ancestor clipping reveals the full window")
        scroll.clipsToBounds = true
        XCTAssertEqual(background.bounds.width, scroll.bounds.width)
        scroll.frame.size.width = viewport.width
        scroll.bounds.size.height /= 2
        XCTAssertEqual(background.bounds.height, scroll.bounds.height, "Ancestor resizing must bound the backing")
        content.removeFromSuperview()
        XCTAssertEqual(background.bounds.size, .zero)
        scroll.addSubview(content)
        XCTAssertEqual(background.bounds.height, scroll.bounds.height, "Reattachment restores observations")
        scroll.contentOffset = .zero
        XCTAssertEqual(background.frame.origin, .zero)
    }
}
