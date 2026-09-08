import ExpoModulesCore
import XCTest

extension RichTextEditorViewTests {
    func testImageTouchRemainsInTextViewScrollHierarchy() {
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
        <p>Hello</p><img src="https://example.com/cat.png" width="140" height="80"><p>After</p>
        """ + String(repeating: "<p>More content</p>", count: 20))
        view.layoutIfNeeded()
        guard let imageRange = firstImageRange(in: view.textView) else {
            return XCTFail("expected image attachment")
        }
        view.textView.contentOffset = .zero
        let rect = renderedRect(in: view.textView, utf16Range: imageRange)
        let point = CGPoint(x: rect.midX, y: rect.midY)
        guard let hitView = view.hitTest(point, with: nil) else {
            return XCTFail("expected an image touch target")
        }
        XCTAssertTrue(view.textView.isScrollEnabled)
        XCTAssertTrue(
            hitView.isDescendant(of: view.textView),
            "image touches must reach the text view's scroll recognizer"
        )
    }

    func testImageTapHitTestingUsesScrolledTextViewCoordinates() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        view.setContent(html: String(repeating: "<p>Before</p>", count: 12) + """
        <img src="https://example.com/cat.png" width="140" height="80"><p>After</p>
        """ + String(repeating: "<p>After</p>", count: 12))
        view.layoutIfNeeded()
        guard let imageRange = firstImageRange(in: view.textView) else {
            return XCTFail("expected image attachment")
        }
        view.textView.contentOffset = .zero
        let rect = renderedRect(in: view.textView, utf16Range: imageRange)
        view.textView.contentOffset = CGPoint(x: 0, y: rect.minY - 40)
        view.layoutIfNeeded()
        let point = CGPoint(x: rect.midX, y: rect.midY)
        XCTAssertEqual(view.textView.imageAttachmentRange(at: point), imageRange)
        XCTAssertNil(view.textView.imageAttachmentRange(at: CGPoint(x: rect.midX, y: rect.maxY + 40)))
        let pointInEditor = view.textView.convert(point, to: view)
        XCTAssertTrue(view.imageTapOverlayInterceptsPointForTesting(pointInEditor))
        XCTAssertTrue(view.hitTest(pointInEditor, with: nil)?.isDescendant(of: view.textView) == true)
    }

    func testImageSelectionSurvivesKeyboardThemeRefresh() {
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
        <p>Hello</p><img src="https://example.com/cat.png" width="140" height="80"><p>After</p>
        """)
        view.layoutIfNeeded()
        guard let imageRange = firstImageRange(in: view.textView) else {
            return XCTFail("expected image attachment")
        }
        view.textView.contentOffset = .zero
        let rect = renderedRect(in: view.textView, utf16Range: imageRange)
        XCTAssertTrue(view.tapImageOverlayForTesting(at: CGPoint(x: rect.midX, y: rect.midY)))
        flushMainQueue()
        flushMainQueue()
        assertSelectedUtf16Range(in: view.textView, imageRange)
        XCTAssertEqual(currentSelection(in: editorId)["type"] as? String, "node")
        XCTAssertTrue(view.applyTheme(EditorTheme(dictionary: [
            "contentInsets": ["bottom": 320]
        ])))
        flushMainQueue()
        view.layoutIfNeeded()
        assertSelectedUtf16Range(in: view.textView, imageRange)
        XCTAssertNotNil(view.imageResizeOverlayRectForTesting())
        XCTAssertTrue(view.textView.isScrollEnabled)
    }

    func testSelectedImageOverlayHidesWhenEditorLosesFocus() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        let viewController = UIViewController()
        window.rootViewController = viewController
        window.makeKeyAndVisible()

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        view.setContent(html: """
        <p>Hello</p><img src="https://example.com/cat.png" width="140" height="80"><p></p>
        """)
        viewController.view.addSubview(view)
        view.layoutIfNeeded()

        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }

        guard let imageRange = firstImageRange(in: view.textView) else {
            XCTFail("expected an image attachment in the rendered text")
            return
        }

        XCTAssertTrue(view.textView.becomeFirstResponder())
        setSelection(in: view.textView, utf16Range: imageRange)
        flushMainQueue()
        view.layoutIfNeeded()

        XCTAssertNotNil(view.imageResizeOverlayRectForTesting())

        XCTAssertTrue(view.textView.resignFirstResponder())
        view.refreshSelectionVisualStateForTesting()
        flushMainQueue()
        view.layoutIfNeeded()

        XCTAssertNil(view.imageResizeOverlayRectForTesting())
    }

    func testDeferredImageTapSelectionWinsAfterUIKitCaretPlacement() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 320, height: 480))
        let viewController = UIViewController()
        window.rootViewController = viewController
        window.makeKeyAndVisible()

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        view.setContent(html: """
        <p>Hello</p><img src="https://example.com/cat.png" width="140" height="80"><p></p>
        """)
        viewController.view.addSubview(view)
        view.layoutIfNeeded()

        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }

        guard let imageRange = firstImageRange(in: view.textView) else {
            XCTFail("expected an image attachment in the rendered text")
            return
        }

        let imageRect = renderedRect(in: view.textView, utf16Range: imageRange)
        XCTAssertTrue(view.textView.becomeFirstResponder())
        setCollapsedSelection(in: view.textView, utf16Offset: 0)
        flushMainQueue()
        view.layoutIfNeeded()

        XCTAssertTrue(
            view.tapImageOverlayForTesting(
                at: CGPoint(x: imageRect.midX, y: imageRect.midY)
            )
        )

        // Mirror UIKit collapsing the image selection back to a caret.
        setCollapsedSelection(in: view.textView, utf16Offset: imageRange.location + 1)
        view.textView.textViewDidChangeSelection(view.textView)
        flushMainQueue()
        view.layoutIfNeeded()

        let selectedRange = view.textView.selectedTextRange
        let startOffset = view.textView.offset(
            from: view.textView.beginningOfDocument,
            to: selectedRange?.start ?? view.textView.endOfDocument
        )
        let endOffset = view.textView.offset(
            from: view.textView.beginningOfDocument,
            to: selectedRange?.end ?? view.textView.endOfDocument
        )

        XCTAssertEqual(startOffset, imageRange.location)
        XCTAssertEqual(endOffset, imageRange.location + imageRange.length)
        XCTAssertNotNil(view.imageResizeOverlayRectForTesting())
    }

    func testImageTapOverlayInterceptsImagePointsOnly() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        view.editorId = editorId
        view.setContent(html: """
        <p>Hello</p><img src="https://example.com/cat.png" width="140" height="80"><p></p>
        """)
        view.layoutIfNeeded()

        guard let imageRange = firstImageRange(in: view.textView) else {
            XCTFail("expected an image attachment in the rendered text")
            return
        }

        let imageRect = renderedRect(in: view.textView, utf16Range: imageRange)
        let imageTapPoint = CGPoint(x: imageRect.midX, y: imageRect.midY)

        XCTAssertTrue(view.imageTapOverlayInterceptsPointForTesting(imageTapPoint))
        XCTAssertFalse(
            view.imageTapOverlayInterceptsPointForTesting(
                CGPoint(x: imageRect.midX, y: imageRect.maxY + 24)
            )
        )
    }

    func testOversizedImageResizeClampsToContentWidthAndKeepsAutoGrowHeightBounded() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 0))
        let window = hostEditorView(view)
        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.heightBehavior = .autoGrow
        view.editorId = editorId
        view.setContent(html: """
        <p>Hello</p><img src="https://example.com/cat.png" width="140" height="80"><p></p>
        """)
        view.layoutIfNeeded()

        guard let imageRange = firstImageRange(in: view.textView) else {
            XCTFail("expected an image attachment in the rendered text")
            return
        }

        XCTAssertTrue(view.textView.becomeFirstResponder())
        setSelection(in: view.textView, utf16Range: imageRange)
        flushMainQueue()
        view.layoutIfNeeded()

        let maximumWidth = view.maximumImageWidthForTesting()
        let expectedHeight = max(48, maximumWidth / 2)

        view.resizeSelectedImageForTesting(width: 4_000, height: 2_000)
        flushMainQueue()
        view.layoutIfNeeded()

        let html = EditorV2Shadow.getHtml(id: editorId)
        XCTAssertTrue(
            html.contains("width=\"\(Int(maximumWidth.rounded()))\""),
            "oversized image width should clamp to the editor content width, got: \(html)"
        )
        XCTAssertTrue(
            html.contains("height=\"\(Int(expectedHeight.rounded()))\""),
            "oversized image height should preserve aspect ratio after clamping, got: \(html)"
        )

        let overlayRect = view.imageResizeOverlayRectForTesting()
        XCTAssertEqual(overlayRect?.width ?? 0, maximumWidth, accuracy: 1.0)
        XCTAssertEqual(overlayRect?.height ?? 0, expectedHeight, accuracy: 1.0)
        XCTAssertLessThan(view.intrinsicContentSize.height, 400)
    }

    func testImageResizePreviewReflowsContentAndDefersDocumentMutationUntilCommit() {
        let editorId = makeV2Editor(
            configJson: #"{"initialization":{"type":"localEmpty"},"policy":{"allowBase64Images":true}}"#
        )
        defer { destroyV2Editor(id: editorId) }

        let image = UIGraphicsImageRenderer(size: CGSize(width: 140, height: 80)).image { context in
            UIColor.systemTeal.setFill()
            context.fill(CGRect(x: 0, y: 0, width: 140, height: 80))
        }
        let dataUri = "data:image/png;base64," + image.pngData()!.base64EncodedString()

        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 0))
        let window = hostEditorView(view)
        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.heightBehavior = .autoGrow
        view.editorId = editorId
        view.setContent(json: """
        {
        "type": "doc",
        "content": [
            {
            "type": "paragraph",
            "content": [
                {
                "type": "text",
                "text": "Hello"
                }
            ]
            },
            {
            "type": "image",
            "attrs": {
                "src": "\(dataUri)",
                "width": 140,
                "height": 80
            }
            },
            {
            "type": "paragraph",
            "content": [{"type": "text", "text": "After"}]
            }
        ]
        }
        """)
        view.layoutIfNeeded()

        guard let imageRange = firstImageRange(in: view.textView) else {
            XCTFail("expected an image attachment in the rendered text")
            return
        }

        XCTAssertTrue(view.textView.becomeFirstResponder())
        setSelection(in: view.textView, utf16Range: imageRange)
        flushMainQueue()
        view.layoutIfNeeded()

        let initialHtml = EditorV2Shadow.getHtml(id: editorId)
        let initialHeight = view.intrinsicContentSize.height
        let followingRange = (view.textView.text as NSString).range(of: "After")
        let initialFollowingRect = renderedRect(in: view.textView, utf16Range: followingRange)
        let maximumWidth = view.maximumImageWidthForTesting()

        view.previewResizeSelectedImageForTesting(width: 4_000, height: 2_000)
        flushMainQueue()
        view.layoutIfNeeded()

        XCTAssertGreaterThan(
            renderedRect(in: view.textView, utf16Range: followingRange).minY,
            initialFollowingRect.minY,
            "following content should move during the drag"
        )
        XCTAssertEqual(
            EditorV2Shadow.getHtml(id: editorId),
            initialHtml,
            "preview resizing should not mutate the document before the gesture commits"
        )
        XCTAssertGreaterThan(
            view.intrinsicContentSize.height,
            initialHeight,
            "the editor should grow with the live image preview"
        )
        XCTAssertEqual(
            view.imageResizeOverlayRectForTesting()?.height ?? 0,
            renderedRect(in: view.textView, utf16Range: imageRange).height,
            accuracy: 1.0
        )
        XCTAssertEqual(view.imageResizeOverlayRectForTesting()?.width ?? 0, maximumWidth, accuracy: 1.0)

        view.commitPreviewResizeForTesting()
        flushMainQueue()
        view.layoutIfNeeded()

        let committedHtml = EditorV2Shadow.getHtml(id: editorId)
        XCTAssertTrue(committedHtml.contains("width=\"\(Int(maximumWidth.rounded()))\""))
        XCTAssertNotEqual(committedHtml, initialHtml)
        XCTAssertFalse(view.textView.isPreviewingImageResize)
    }

    func testImageResizeCancellationRestoresLayoutWithoutChangingDocument() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.editorId = editorId
        view.setContent(html: "<p>Before</p><img src=\"https://example.com/image.png\" width=\"140\" height=\"80\"><p>After</p>")
        view.layoutIfNeeded()
        guard let imageRange = firstImageRange(in: view.textView) else {
            return XCTFail("expected image attachment")
        }
        XCTAssertTrue(view.textView.becomeFirstResponder())
        setSelection(in: view.textView, utf16Range: imageRange)
        flushMainQueue()
        let initialRect = renderedRect(in: view.textView, utf16Range: imageRange)
        let initialHtml = EditorV2Shadow.getHtml(id: editorId)
        let before = XCTAttachment(image: UIGraphicsImageRenderer(bounds: view.bounds).image { context in
            view.layer.render(in: context.cgContext)
        })
        before.name = "image-resize-before"
        before.lifetime = .keepAlways
        add(before)
        view.previewResizeSelectedImageForTesting(width: 240, height: 140)
        view.layoutIfNeeded()
        let during = XCTAttachment(image: UIGraphicsImageRenderer(bounds: view.bounds).image { context in
            view.layer.render(in: context.cgContext)
        })
        during.name = "image-resize-during"
        during.lifetime = .keepAlways
        add(during)
        XCTAssertGreaterThan(renderedRect(in: view.textView, utf16Range: imageRange).height, initialRect.height)
        view.allowImageResizing = false
        flushMainQueue()
        view.layoutIfNeeded()
        XCTAssertEqual(renderedRect(in: view.textView, utf16Range: imageRange), initialRect)
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHtml)
        XCTAssertNil(view.imageResizeOverlayRectForTesting())
        XCTAssertFalse(view.textView.isPreviewingImageResize)
    }

    func testImageResizeProjectionShrinksOnEitherAxisAndPreservesAspectRatioAtMinimum() {
        let size = CGSize(width: 200, height: 100)
        for corner in ImageResizeOverlayView.Corner.allCases {
            let x: CGFloat = (corner == .topRight || corner == .bottomRight) ? 1 : -1
            let y: CGFloat = (corner == .bottomLeft || corner == .bottomRight) ? 1 : -1
            let horizontal = ImageResizeOverlayView.resizedSize(
                from: size, corner: corner, translation: CGPoint(x: -20 * x, y: 0)
            )
            XCTAssertLessThan(horizontal.width, size.width)
            let vertical = ImageResizeOverlayView.resizedSize(
                from: size, corner: corner, translation: CGPoint(x: 0, y: -20 * y)
            )
            XCTAssertLessThan(vertical.height, size.height)
            let minimum = ImageResizeOverlayView.resizedSize(
                from: size, corner: corner, translation: CGPoint(x: -1000 * x, y: -1000 * y)
            )
            XCTAssertGreaterThanOrEqual(minimum.height, 48)
            XCTAssertEqual(minimum.width / minimum.height, 2, accuracy: 0.001)
            let diagonal = ImageResizeOverlayView.resizedSize(
                from: size, corner: corner, translation: CGPoint(x: 20 * x, y: 10 * y)
            )
            XCTAssertEqual(diagonal.width, 220, accuracy: 0.001)
            XCTAssertEqual(diagonal.height, 110, accuracy: 0.001)
        }
    }

    func testImageResizeCancellationRestoresNaturalDimensionsAfterAttachmentMoves() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.editorId = editorId
        view.setContent(html: "<p>A</p><img src=\"https://example.com/image.png\"><p>After</p>")
        view.layoutIfNeeded()
        guard let imageRange = firstImageRange(in: view.textView),
              let attachment = view.textView.textStorage.attribute(.attachment, at: imageRange.location, effectiveRange: nil) as? BlockImageAttachment
        else { return XCTFail("expected image attachment") }
        XCTAssertNil(attachment.preferredWidth)
        XCTAssertNil(attachment.preferredHeight)
        XCTAssertTrue(view.textView.becomeFirstResponder())
        setSelection(in: view.textView, utf16Range: imageRange)
        flushMainQueue()
        view.previewResizeSelectedImageForTesting(width: 240, height: 140)
        XCTAssertNotNil(attachment.preferredWidth)
        let update = EditorV2Shadow.replaceHtml(
            id: editorId,
            html: "<p>A longer preceding paragraph</p><img src=\"https://example.com/image.png\"><p>After</p>"
        )
        var patchUpdate = parseJSONObject(update)
        let blocks = try XCTUnwrap((patchUpdate["renderPatch"] as? [String: Any])?["renderBlocks"] as? [[[String: Any]]])
        patchUpdate["renderBlocks"] = blocks
        patchUpdate["renderPatch"] = ["startIndex": 0, "deleteCount": 1, "renderBlocks": [blocks[0]]]
        let patchData = try JSONSerialization.data(withJSONObject: patchUpdate)
        view.textView.applyUpdateJSON(try XCTUnwrap(String(data: patchData, encoding: .utf8)), notifyDelegate: false)
        flushMainQueue()
        view.layoutIfNeeded()
        guard let movedRange = firstImageRange(in: view.textView),
              let movedAttachment = view.textView.textStorage.attribute(.attachment, at: movedRange.location, effectiveRange: nil) as? BlockImageAttachment
        else { return XCTFail("expected retained image attachment") }
        XCTAssertTrue(movedAttachment === attachment)
        XCTAssertNotEqual(movedRange.location, imageRange.location)
        XCTAssertNil(movedAttachment.preferredWidth)
        XCTAssertNil(movedAttachment.preferredHeight)
        XCTAssertFalse(view.textView.isPreviewingImageResize)
        XCTAssertFalse(EditorV2Shadow.getHtml(id: editorId).contains("width="))
    }

    func testImageResizeDragPreservesStyledImageProportionsAndScrollPosition() throws {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
        let window = hostEditorView(view)
        defer {
            view.removeFromSuperview()
            window.isHidden = true
        }
        view.editorId = editorId
        view.setContent(html: "<p>Before</p><img src=\"https://example.com/image.png\" width=\"140\" height=\"80\"><p>After</p>" + String(repeating: "<p>More content</p>", count: 30))
        let theme = try XCTUnwrap(EditorTheme.from(json: """
        {"version":1,"styles":{"image":{"paddingLeft":10,"paddingRight":10,"paddingTop":10,"paddingBottom":10}}}
        """))
        XCTAssertTrue(view.applyTheme(theme))
        view.layoutIfNeeded()
        let range = try XCTUnwrap(firstImageRange(in: view.textView))
        let attachment = try XCTUnwrap(view.textView.textStorage.attribute(.attachment, at: range.location, effectiveRange: nil) as? BlockImageAttachment)
        XCTAssertTrue(view.textView.becomeFirstResponder())
        setSelection(in: view.textView, utf16Range: range)
        flushMainQueue()
        view.textView.contentOffset = CGPoint(x: 0, y: 20)
        let initialHtml = EditorV2Shadow.getHtml(id: editorId)
        view.previewImageResizeDragForTesting(corner: .bottomRight, translation: CGPoint(x: 70, y: 40))
        view.layoutIfNeeded()
        XCTAssertEqual(try XCTUnwrap(attachment.preferredWidth), 210, accuracy: 0.5)
        XCTAssertEqual(try XCTUnwrap(attachment.preferredHeight), 120, accuracy: 0.5)
        XCTAssertEqual(view.imageResizeOverlayRectForTesting()?.width ?? 0, 230, accuracy: 1)
        XCTAssertEqual(view.textView.contentOffset.y, 20, accuracy: 0.5)
        XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHtml)
        view.previewImageResizeDragForTesting(corner: .bottomRight, translation: CGPoint(x: 35, y: 20))
        XCTAssertEqual(try XCTUnwrap(attachment.preferredWidth), 175, accuracy: 0.5)
        XCTAssertEqual(try XCTUnwrap(attachment.preferredHeight), 100, accuracy: 0.5)
        let previewRect = view.imageResizeOverlayRectForTesting()
        view.commitPreviewResizeForTesting()
        flushMainQueue()
        view.layoutIfNeeded()
        XCTAssertEqual(view.imageResizeOverlayRectForTesting()?.width ?? 0, previewRect?.width ?? 0, accuracy: 1)
        let committedHtml = EditorV2Shadow.getHtml(id: editorId)
        XCTAssertTrue(committedHtml.contains("width=\"175\""), committedHtml)
        XCTAssertTrue(committedHtml.contains("height=\"100\""), committedHtml)
        assertSelectedUtf16Range(in: view.textView, range)
    }

    func testImageResizeReturningToStartDoesNotCommit() throws {
        for (width, height) in [(30, 20), (1000, 777)] {
            let editorId = makeV2Editor()
            defer { destroyV2Editor(id: editorId) }
            let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 320, height: 240))
            let window = hostEditorView(view)
            defer {
                view.removeFromSuperview()
                window.isHidden = true
            }
            view.editorId = editorId
            view.setContent(html: "<img src=\"https://example.com/image.png\" width=\"\(width)\" height=\"\(height)\"><p>After</p>")
            view.layoutIfNeeded()
            let range = try XCTUnwrap(firstImageRange(in: view.textView))
            XCTAssertTrue(view.textView.becomeFirstResponder())
            setSelection(in: view.textView, utf16Range: range)
            flushMainQueue()
            let initialHtml = EditorV2Shadow.getHtml(id: editorId)
            let initialRect = renderedRect(in: view.textView, utf16Range: range)
            view.previewImageResizeDragForTesting(corner: .bottomRight, translation: CGPoint(x: -15, y: -10))
            XCTAssertNotEqual(renderedRect(in: view.textView, utf16Range: range).width, initialRect.width)
            view.previewImageResizeDragForTesting(corner: .bottomRight, translation: .zero)
            view.commitPreviewResizeForTesting()
            flushMainQueue()
            XCTAssertEqual(renderedRect(in: view.textView, utf16Range: range), initialRect)
            XCTAssertEqual(EditorV2Shadow.getHtml(id: editorId), initialHtml)
        }
    }

}
