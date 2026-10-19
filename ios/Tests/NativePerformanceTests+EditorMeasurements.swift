import UIKit
import XCTest

extension NativePerformanceTests {
    private enum EditorBenchmark {
        static let viewport = CGSize(width: 390, height: 844)
        static let text = "!"
        static let millisecondsPerSecond = 1_000.0
        static let scrollFrameCount = 180
        static let scrollTimeout: TimeInterval = 30
    }

    func testPerformance_coldHostedEditorLargeDocument_fixed() throws {
        try measureColdHostedEditor(autoGrow: false)
    }

    func testPerformance_coldHostedEditorLargeDocument_autoGrow() throws {
        try measureColdHostedEditor(autoGrow: true)
    }

    private func measureColdHostedEditor(autoGrow: Bool) throws {
        let source = NativePerformanceFixtureFactory.largeDocumentJSONString()
        let render = try XCTUnwrap(JSONSerialization.jsonObject(
            with: Data(NativePerformanceFixtureFactory.largeRenderJSON().utf8)
        ) as? [String: Any])
        let expectedText = RenderBridge.renderBlocks(
            fromArray: try XCTUnwrap(render["renderBlocks"] as? [[NSDictionary]]),
            baseFont: baseFont,
            textColor: textColor
        ).string
        measure(metrics: [XCTClockMetric()], options: measureOptions()) {
            autoreleasepool {
                let editorId = makeV2Editor()
                let view = RichTextEditorView(frame: CGRect(origin: .zero, size: EditorBenchmark.viewport))
                let window = hostEditorView(view, size: EditorBenchmark.viewport)
                defer {
                    view.editorId = 0
                    view.removeFromSuperview()
                    window.isHidden = true
                    destroyV2Editor(id: editorId)
                }
                view.heightBehavior = autoGrow ? .autoGrow : .fixed
                view.editorId = editorId
                view.setContent(json: source)
                flushMainQueue()
                NativePerformanceFixtureFactory.layoutHostedEditor(view, autoGrow: autoGrow)
                view.textView.layoutManager.ensureLayout(for: view.textView.textContainer)
                view.textView.layoutIfNeeded()
                XCTAssertEqual(view.textView.textStorage.string, expectedText)
                XCTAssertGreaterThan(view.textView.contentSize.height, EditorBenchmark.viewport.height)
                XCTAssertNotNil(view.textView.selectedTextRange)
                if autoGrow {
                    XCTAssertEqual(view.frame.height, ceil(view.intrinsicContentSize.height), accuracy: 1)
                }
            }
        }
    }

    func testPerformance_typingHostedLargeDocument_fixed() throws {
        try measureHostedTyping(autoGrow: false)
    }

    func testPerformance_typingHostedLargeDocument_autoGrow() throws {
        try measureHostedTyping(autoGrow: true)
    }

    private func measureHostedTyping(autoGrow: Bool) throws {
        let session = try XCTUnwrap(NativePerformanceFixtureFactory.hostedParagraphSplitSessions(count: 1, autoGrow: autoGrow).first)
        defer {
            session.view.editorId = 0
            session.view.removeFromSuperview()
            session.window.isHidden = true
            destroyV2Editor(id: session.editorId)
        }
        let input = session.view.textView
        let originalText = input.textStorage.string
        let insertionOffset = session.splitOffset
        let insertedText = (originalText as NSString).replacingCharacters(
            in: NSRange(location: insertionOffset, length: 0),
            with: EditorBenchmark.text
        )
        var traces: [EditorTextView.ApplyUpdateTrace] = []
        measure(metrics: [XCTClockMetric()], options: measureOptions()) {
            autoreleasepool {
                setSelection(in: input, utf16Range: NSRange(location: insertionOffset, length: 0))
                input.insertText(EditorBenchmark.text)
                XCTAssertEqual(input.textStorage.string, insertedText)
                XCTAssertEqual(input.selectedRange, NSRange(location: insertionOffset + EditorBenchmark.text.utf16.count, length: 0))
                if let trace = input.lastApplyUpdateTrace() { traces.append(trace) }
                input.deleteBackward()
                flushMainQueue()
                NativePerformanceFixtureFactory.layoutHostedEditor(session.view, autoGrow: autoGrow)
                XCTAssertEqual(input.textStorage.string, originalText)
                XCTAssertEqual(input.selectedRange, NSRange(location: insertionOffset, length: 0))
                XCTAssertGreaterThan(input.contentSize.height, EditorBenchmark.viewport.height)
                if let trace = input.lastApplyUpdateTrace() { traces.append(trace) }
            }
        }
        print(ApplyUpdateTraceStats(name: "typingHostedLargeDocument.\(autoGrow ? "autoGrow" : "fixed").breakdown", traces: traces).summaryString())
    }

    func testPerformance_scrollHostedLargeDocument_fixed() throws {
        try measureHostedScroll(autoGrow: false)
    }

    func testPerformance_scrollHostedLargeDocument_autoGrow() throws {
        try measureHostedScroll(autoGrow: true)
    }

    private func measureHostedScroll(autoGrow: Bool) throws {
        let session = try XCTUnwrap(NativePerformanceFixtureFactory.hostedParagraphSplitSessions(count: 1, autoGrow: autoGrow).first)
        let clock = TableTestFrameClock()
        defer {
            clock.close()
            session.view.editorId = 0
            session.view.removeFromSuperview()
            session.window.isHidden = true
            destroyV2Editor(id: session.editorId)
        }
        let input = session.view.textView
        input.layoutManager.ensureLayout(for: input.textContainer)
        input.layoutIfNeeded()
        let originalText = input.textStorage.string
        let originalSelection = input.selectedRange
        let originalHeight = input.contentSize.height
        let scroll: UIScrollView
        if autoGrow {
            let ancestor = UIScrollView(frame: CGRect(origin: .zero, size: EditorBenchmark.viewport))
            let parent = try XCTUnwrap(session.view.superview)
            session.view.removeFromSuperview()
            parent.addSubview(ancestor)
            ancestor.addSubview(session.view)
            ancestor.contentSize = session.view.frame.size
            scroll = ancestor
        } else {
            scroll = input
        }
        let maximumOffset = max(0, scroll.contentSize.height - scroll.bounds.height)
        XCTAssertGreaterThan(maximumOffset, EditorBenchmark.viewport.height)
        var previousTimestamp: Double?
        var framesMs: [Double] = []
        var workMs: [Double] = []
        var largestOffset: CGFloat = 0
        clock.onTick = { link in
            if let previousTimestamp {
                framesMs.append((link.timestamp - previousTimestamp) * EditorBenchmark.millisecondsPerSecond)
            }
            previousTimestamp = link.timestamp
            let startedAt = CACurrentMediaTime()
            let fraction = CGFloat((1 - cos(Double(framesMs.count) / Double(EditorBenchmark.scrollFrameCount) * 2 * .pi)) / 2)
            scroll.setContentOffset(CGPoint(x: 0, y: maximumOffset * fraction), animated: false)
            session.view.layoutIfNeeded()
            largestOffset = max(largestOffset, scroll.contentOffset.y)
            workMs.append((CACurrentMediaTime() - startedAt) * EditorBenchmark.millisecondsPerSecond)
        }
        let deadline = CACurrentMediaTime() + EditorBenchmark.scrollTimeout
        while framesMs.count < EditorBenchmark.scrollFrameCount && CACurrentMediaTime() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(TableTestFrameClock.runLoopSlice))
        }
        clock.onTick = nil
        XCTAssertEqual(framesMs.count, EditorBenchmark.scrollFrameCount)
        XCTAssertEqual(largestOffset, maximumOffset, accuracy: 1)
        XCTAssertEqual(scroll.contentOffset.y, 0, accuracy: 1)
        XCTAssertEqual(input.textStorage.string, originalText)
        XCTAssertEqual(input.selectedRange, originalSelection)
        XCTAssertEqual(input.contentSize.height, originalHeight, accuracy: 1)
        print("EDITOR_SCROLL_PERFORMANCE mode=\(autoGrow ? "autoGrow" : "fixed") framesMs=\(framesMs) workMs=\(workMs) contentHeight=\(originalHeight) maximumOffset=\(maximumOffset)")
    }

    func testPerformance_renderBridgeLargeDocument() {
        let renderJSON = NativePerformanceFixtureFactory.largeRenderJSON()
        // The v2 update carries renderBlocks (the flat renderElements array
        // was a legacy-update shape); render through the blocks entry.
        let renderBlocks = (try? JSONSerialization.jsonObject(with: Data(renderJSON.utf8)))
            .flatMap { $0 as? [String: Any] }
            .flatMap { $0["renderBlocks"] as? [[NSDictionary]] } ?? []
        let options = measureOptions()

        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                let attributed = RenderBridge.renderBlocks(
                    fromArray: renderBlocks,
                    baseFont: baseFont,
                    textColor: textColor
                )

                XCTAssertGreaterThan(attributed.length, 0)
                _ = attributed.string.utf16.count
                if attributed.length > 0 {
                    _ = attributed.attributes(at: min(1, attributed.length - 1), effectiveRange: nil)
                }
            }
        }
    }

    func testPerformance_applyUpdateJSONLargeDocument() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let updateJSON = NativePerformanceFixtureFactory.loadLargeDocument(into: editorId)
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
        textView.captureApplyUpdateTraceForTesting = true
        textView.bindEditor(id: editorId)
        var traceSamples: [EditorTextView.ApplyUpdateTrace] = []

        // Warm the text system before measuring steady-state apply cost.
        textView.applyUpdateJSON(updateJSON, notifyDelegate: false)
        textView.layoutIfNeeded()

        let options = measureOptions()
        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                textView.applyUpdateJSON(updateJSON, notifyDelegate: false)
                textView.layoutIfNeeded()
                if let trace = textView.lastApplyUpdateTrace() {
                    traceSamples.append(trace)
                }
                XCTAssertFalse(textView.text.isEmpty)
                _ = textView.attributedText.length
            }
        }
        print(
            ApplyUpdateTraceStats(
                name: "applyUpdateJSONLargeDocument.breakdown",
                traces: traceSamples
            ).summaryString()
        )
    }

    func testPerformance_typingRoundTripLargeDocument() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        _ = NativePerformanceFixtureFactory.loadLargeDocument(into: editorId)
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
        textView.bindEditor(id: editorId)
        textView.layoutIfNeeded()

        let typingOffset = NativePerformanceFixtureFactory.typingCursorOffset(in: textView)
        setSelection(in: textView, utf16Range: NSRange(location: typingOffset, length: 0))

        let options = measureOptions()
        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                setSelection(in: textView, utf16Range: NSRange(location: typingOffset, length: 0))
                textView.insertText("!")
                textView.deleteBackward()
                XCTAssertFalse(textView.text.isEmpty)
                XCTAssertNotNil(textView.selectedTextRange)
            }
        }
    }

    func testPerformance_paragraphSplitRoundTripLargeDocument() {
        let options = measureOptions()
        let sessions = NativePerformanceFixtureFactory.paragraphSplitSessions(
            count: max(options.iterationCount + 4, 12)
        )
        defer {
            for session in sessions {
                destroyV2Editor(id: session.editorId)
            }
        }

        var remainingSessionIndices = Array(sessions.indices)
        var traceSamples: [EditorTextView.ApplyUpdateTrace] = []

        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                guard let sessionIndex = remainingSessionIndices.first else {
                    XCTFail("expected prebuilt paragraph split sessions")
                    return
                }
                remainingSessionIndices.removeFirst()

                let session = sessions[sessionIndex]
                setSelection(in: session.textView, utf16Range: NSRange(location: session.splitOffset, length: 0))
                session.textView.insertText("\n")
                session.textView.layoutIfNeeded()

                if let trace = session.textView.lastApplyUpdateTrace() {
                    traceSamples.append(trace)
                }
                XCTAssertGreaterThan(session.textView.attributedText.length, session.initialTextLength)
                XCTAssertTrue(
                    session.textView.lastRenderAppliedPatch(),
                    "paragraph split must use the render patch path; trace: \(String(describing: session.textView.lastApplyUpdateTrace()))"
                )
                XCTAssertNotNil(session.textView.selectedTextRange)
            }
        }
        print(
            ApplyUpdateTraceStats(
                name: "paragraphSplitRoundTripLargeDocument.breakdown",
                traces: traceSamples
            ).summaryString()
        )
    }

    func testPerformance_paragraphSplitRoundTripLargeDocument_autoGrow() {
        let options = measureOptions()
        let sessions = NativePerformanceFixtureFactory.paragraphSplitSessions(
            count: max(options.iterationCount + 4, 12),
            autoGrow: true
        )
        defer {
            for session in sessions {
                destroyV2Editor(id: session.editorId)
            }
        }

        var remainingSessionIndices = Array(sessions.indices)
        var traceSamples: [EditorTextView.ApplyUpdateTrace] = []

        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                guard let sessionIndex = remainingSessionIndices.first else {
                    XCTFail("expected prebuilt paragraph split sessions")
                    return
                }
                remainingSessionIndices.removeFirst()

                let session = sessions[sessionIndex]
                setSelection(in: session.textView, utf16Range: NSRange(location: session.splitOffset, length: 0))
                session.textView.insertText("\n")
                session.textView.layoutIfNeeded()

                if let trace = session.textView.lastApplyUpdateTrace() {
                    traceSamples.append(trace)
                }
                XCTAssertGreaterThan(session.textView.attributedText.length, session.initialTextLength)
                XCTAssertTrue(
                    session.textView.lastRenderAppliedPatch(),
                    "auto-grow paragraph split must use the render patch path; trace: \(String(describing: session.textView.lastApplyUpdateTrace()))"
                )
                XCTAssertNotNil(session.textView.selectedTextRange)
            }
        }
        print(
            ApplyUpdateTraceStats(
                name: "paragraphSplitRoundTripLargeDocument.autoGrow.breakdown",
                traces: traceSamples
            ).summaryString()
        )
    }

    func testPerformance_paragraphSplitRoundTripLargeDocument_autoGrowHostedView() {
        let options = measureOptions()
        let sessions = NativePerformanceFixtureFactory.hostedParagraphSplitSessions(
            count: max(options.iterationCount + 4, 12)
        )
        defer {
            for session in sessions {
                session.view.removeFromSuperview()
                session.window.isHidden = true
                destroyV2Editor(id: session.editorId)
            }
        }

        var remainingSessionIndices = Array(sessions.indices)
        var traceSamples: [EditorTextView.ApplyUpdateTrace] = []
        var hostedLayoutTraceSamples: [RichTextEditorView.HostedLayoutTrace] = []

        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                guard let sessionIndex = remainingSessionIndices.first else {
                    XCTFail("expected prebuilt hosted paragraph split sessions")
                    return
                }
                remainingSessionIndices.removeFirst()

                let session = sessions[sessionIndex]
                session.view.captureHostedLayoutTraceForTesting = true
                session.view.resetHostedLayoutTraceForTesting()
                setSelection(in: session.view.textView, utf16Range: NSRange(location: session.splitOffset, length: 0))
                session.view.textView.insertText("\n")
                flushMainQueue()

                let measuredHeight = ceil(session.view.intrinsicContentSize.height)
                session.view.frame.size.height = measuredHeight
                session.view.layoutIfNeeded()

                if let trace = session.view.textView.lastApplyUpdateTrace() {
                    traceSamples.append(trace)
                }
                hostedLayoutTraceSamples.append(session.view.lastHostedLayoutTraceForTesting())
                XCTAssertGreaterThan(session.view.textView.attributedText.length, session.initialTextLength)
                XCTAssertTrue(
                    session.view.textView.lastRenderAppliedPatch(),
                    "hosted auto-grow paragraph split must use the render patch path; trace: \(String(describing: session.view.textView.lastApplyUpdateTrace()))"
                )
                XCTAssertGreaterThan(measuredHeight, 0)
            }
        }
        print(
            ApplyUpdateTraceStats(
                name: "paragraphSplitRoundTripLargeDocument.autoGrowHostedView.breakdown",
                traces: traceSamples
            ).summaryString()
        )
        print(
            HostedLayoutTraceStats(
                name: "paragraphSplitRoundTripLargeDocument.autoGrowHostedView.hostedLayout",
                traces: hostedLayoutTraceSamples
            ).summaryString()
        )
    }

    func testPerformance_selectionScrubLargeDocument() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        _ = NativePerformanceFixtureFactory.loadLargeDocument(into: editorId)
        let textView = EditorTextView(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
        textView.bindEditor(id: editorId)
        textView.layoutIfNeeded()

        let scrubOffsets = NativePerformanceFixtureFactory.selectionScrubOffsets(in: textView, points: 48)
        let options = measureOptions()

        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                for offset in scrubOffsets {
                    setSelection(in: textView, utf16Range: NSRange(location: offset, length: 0))
                }

                let finalOffset = textView.offset(
                    from: textView.beginningOfDocument,
                    to: textView.selectedTextRange?.start ?? textView.endOfDocument
                )
                XCTAssertEqual(finalOffset, scrubOffsets.last ?? 0)
            }
        }
    }

    func testPerformance_remoteSelectionOverlayRefreshMultiPeerLargeDocument() {
        let editorId = makeV2Editor()
        defer { destroyV2Editor(id: editorId) }

        let updateJSON = NativePerformanceFixtureFactory.loadLargeDocument(into: editorId)
        let view = RichTextEditorView(frame: CGRect(x: 0, y: 0, width: 390, height: 844))
        view.editorId = editorId
        view.textView.applyUpdateJSON(updateJSON, notifyDelegate: false)
        view.layoutIfNeeded()

        let selections = NativePerformanceFixtureFactory.remoteSelections(
            editorId: editorId,
            peerCount: 24,
            selectionWidth: 24
        )
        view.setRemoteSelections(selections)
        view.layoutIfNeeded()

        let options = measureOptions()
        measure(metrics: [XCTClockMetric()], options: options) {
            autoreleasepool {
                view.setRemoteSelections(selections)
                view.layoutIfNeeded()
                XCTAssertFalse(view.remoteSelectionOverlaySubviewsForTesting().isEmpty)
                XCTAssertGreaterThanOrEqual(view.remoteSelectionOverlaySubviewsForTesting().count, selections.count)
            }
        }
    }

}
