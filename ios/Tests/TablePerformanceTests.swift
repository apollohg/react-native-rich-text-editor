import CoreText
import Darwin
import QuartzCore
import UIKit
import XCTest

final class TablePerformanceTests: XCTestCase {
    private enum Benchmark {
        static let viewport = CGSize(width: 390, height: 844)
        static let typingRuns = 5
        static let typingSamples = 500
        static let warmupSamples = 20
        static let coldSamples = 30
        static let warmSamples = 1_000
        static let warmPercentile = 0.99
        static let warmBudgetMs = 1.0
        static let baselineSamples = 10
        static let traversalSeconds = 30.0
        static let millisecondsPerSecond = 1_000.0
        static let nanosecondsPerSecond = 1_000_000_000.0
        static let nanosecondsPerMillisecond = nanosecondsPerSecond / millisecondsPerSecond
        static let text = "x"
        static let richParagraphStride = 2
        static let marker = "TABLE_PERFORMANCE_EXPORT "
        static let schema = TableInputTestSchema.strongMarkTableConfig.replacingOccurrences(
            of: #""initialization":{"type":"localEmpty"}"#,
            with: #""initialization":{"type":"localHtml","html":"","snapshotScope":{"documentId":"table-performance","lineageId":"native-editor|table-performance"}}"#
        )
    }

    private struct Fixture {
        let rows: Int
        let columns: Int
        let rich: Bool
        var name: String { "\(rich ? "rich-merged" : "plain")-\(rows)x\(columns)" }

        func source() throws -> String {
            var tableRows: [[String: Any]] = []
            let mergeWidth = 2
            for row in 0..<rows {
                var cells: [[String: Any]] = []
                for column in 0..<columns {
                    if rich && row == 0 && column == 1 { continue }
                    let label = String(format: "R%04dC%04dXY", row, column)
                    var text: [String: Any] = ["type": "text", "text": label]
                    if rich { text["marks"] = [["type": TableToolbarTestItems.strongMark]] }
                    var content: [[String: Any]] = [["type": "paragraph", "content": [text]]]
                    if rich && (row + column).isMultiple(of: Benchmark.richParagraphStride) {
                        content.append(["type": "paragraph", "content": [["type": "text", "text": "café العربية 👩🏽‍💻"]]])
                    }
                    var cell: [String: Any] = ["type": row == 0 ? "table_header" : "table_cell", "content": content]
                    if rich && row == 0 && column == 0 { cell["attrs"] = ["colspan": mergeWidth] }
                    cells.append(cell)
                }
                tableRows.append(["type": "table_row", "content": cells])
            }
            return String(decoding: try JSONSerialization.data(withJSONObject: ["type": "doc",
                "content": [["type": "table", "content": tableRows]]], options: [.sortedKeys]), as: UTF8.self)
        }
    }

    private final class WorkProbe {
        private let lock = NSLock()
        private var spans: [PreparedProseInstrumentation.ViewerWorkSpan] = []

        func record(_ span: PreparedProseInstrumentation.ViewerWorkSpan) {
            lock.lock(); defer { lock.unlock() }
            spans.append(span)
        }

        func consume(through end: UInt64) -> [PreparedProseInstrumentation.ViewerWorkSpan] {
            lock.lock(); defer { lock.unlock() }
            let current = spans.filter { $0.startNanos < end }
            spans.removeAll { $0.endNanos <= end }
            return current
        }
    }

    private struct Measurement {
        let durationMs: Double
        var stagesMs: [String: Double] = [:]
        var presentation: (commit: Double, displayed: Double, measured: Double)?
    }

    private enum MeasurementEndpoint {
        case exactLayout
        case displayedFrameAfterCommit
    }

    private final class StageProbe {
        private let lock = NSLock()
        private var spans: [PreparedProseInstrumentation.TableStage: [PreparedProseInstrumentation.ViewerWorkSpan]] = [:]

        func record(_ stage: PreparedProseInstrumentation.TableStage, start: UInt64, end: UInt64) {
            lock.lock(); defer { lock.unlock() }
            spans[stage, default: []].append(.init(startNanos: start, endNanos: end, kind: .layout))
        }

        func durationsMs() -> [String: Double] {
            lock.lock(); defer { lock.unlock() }
            return Dictionary(uniqueKeysWithValues: spans.map { stage, intervals in
                (stage.rawValue, Double(PreparedProseInstrumentation.viewerWorkNanos(0, UInt64.max, intervals)) / Benchmark.nanosecondsPerMillisecond)
            })
        }
    }

    private struct Sample: Encodable {
        let platform = "ios"
        let device: String
        let os = ProcessInfo.processInfo.operatingSystemVersionString
        let buildType: String
        let physicalDevice: Bool
        let refreshHz: Int
        let textScale = 1
        let viewportWidth = Int(Benchmark.viewport.width)
        let viewportHeight = Int(Benchmark.viewport.height)
        let overscanViewports = 1
        let fixture: String
        let metric: String
        let run: Int
        let samplesMs: [Double]
        let stageSamplesMs: [String: [Double]]
        let stageTimingSemantics = "inclusive wall-time unions through the full frame drain, not a decomposition of cold-layout duration; native stages include Rust and FFI; preparation includes geometry; presentationWait ends at the first display-clock timestamp after draw transaction completion, not a content-specific GPU presentation acknowledgment; postLayoutPresentationWait is diagnostic and excluded from cold-layout duration"
        let warmupSamplesDiscarded: Int?
        let tableAttributed: [Bool]?
        let wrapCount: Int?
        let nonWrapCount: Int?
        let authoritativeDocumentBytesRepresentation = "yrs-update-v1-plus-snapshot-metadata; viewer uses compiled-document logical retained bytes"
        let counters: PreparedProseInstrumentation.TablePerformanceCounters
    }

    private final class EditorHost {
        let id: UInt64
        let adapter: EditorV2Adapter
        let window: UIWindow
        let view: RichTextEditorView
        let surface: EditorTableSurface
        let drawing: PreparedProseDrawingView

        init(id: UInt64? = nil, viewport: CGSize = Benchmark.viewport,
             appearance: UIUserInterfaceStyle = .unspecified) throws {
            window = makeTestWindow(frame: CGRect(origin: .zero, size: viewport))
            window.overrideUserInterfaceStyle = appearance
            view = RichTextEditorView(frame: window.bounds)
            self.id = id ?? makeV2Editor(configJson: Benchmark.schema)
            adapter = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: self.id))
            window.addSubview(view)
            window.makeKeyAndVisible()
            view.bindEditor(id: self.id, initialUpdateJSON: try XCTUnwrap(adapter.initialUpdateJSON()))
            surface = try XCTUnwrap(view.subviews.compactMap { $0 as? EditorTableSurface }.first)
            drawing = try XCTUnwrap(surface.subviews.compactMap { $0 as? PreparedProseDrawingView }.first)
        }

        func close() {
            surface.onTableCellPreparedForTesting = nil
            drawing.onMountedTableCellsDrawnForTesting = nil
            view.activeTextInput.resignFirstResponder()
            view.bindEditor(id: 0, initialUpdateJSON: nil)
            window.isHidden = true
            destroyV2Editor(id: id)
        }

        func load(_ source: String) throws {
            let document = try JSONSerialization.jsonObject(with: Data(source.utf8))
            let result = PreparedProseInstrumentation.measureTableStage(.replacementAndFFI) {
                adapter.callWithEnvelope(["setJson": document, "history": "resetAndClear"], includeBaseRevision: false) {
                    editorV2ReplaceDocument(editorId: adapter.editorId, requestJson: $0)
                }
            }
            XCTAssertNil(result.error, "fixture replacement: \(String(describing: result.error))")
            _ = try XCTUnwrap(result.value, "fixture replacement was rejected")
            let update = try XCTUnwrap(adapter.refreshFromRustState(mirrorSelection: (0, 0)))
            XCTAssertTrue(view.textView.applyUpdateJSON(update))
            view.layoutIfNeeded()
        }

        func table() throws -> ViewerTableSurface {
            try XCTUnwrap(drawing.layout?.blocks.compactMap(\.tableSurface).first)
        }

        func bind(_ cellIndex: Int) throws -> EditorTextView {
            let table = try table()
            let cell = try XCTUnwrap(table.cell(sourceIndex: cellIndex))
            let frame = table.frame(ofCell: cell)
            let scroll = view.textView
            let minimum = -scroll.adjustedContentInset.top
            let maximum = max(minimum, scroll.contentSize.height - scroll.bounds.height + scroll.adjustedContentInset.bottom)
            scroll.contentOffset.y = min(max(frame.midY - scroll.bounds.height / 2, minimum), maximum)
            let horizontal = max(0, frame.midX - Benchmark.viewport.width / 2)
            _ = drawing.scrollTables(in: [table.scrollIdentity], by: -horizontal)
            view.layoutIfNeeded()
            let contentRect = try XCTUnwrap(surface.cellFrame(tableID: table.identity, cellIndex: UInt32(cellIndex)))
            let bound = view.bindTableCell(tableID: table.identity, cellIndex: UInt32(cellIndex), contentRect: contentRect)
            let input = try XCTUnwrap(bound ? view.activeTextInput as? TableCellInputTextView : nil,
                                     "Table cell \(cellIndex) rejected its input binding")
            XCTAssertTrue(input.becomeFirstResponder())
            input.selectedRange = NSRange(location: input.textStorage.length, length: 0)
            return input
        }

        func authoritativeBytes() throws -> Int {
            let result = editorV2SnapshotExport(editorId: adapter.editorId)
            XCTAssertNil(result.error)
            let snapshot = try XCTUnwrap(result.value)
            return snapshot.encodedState.count + snapshot.metadataJson.utf8.count
        }
    }

    private var samples: [Sample] = []
    private var clock: TableTestFrameClock!

    func testInputCounterBelongsToTheMeasuredEditor() throws {
        let first = try EditorHost()
        defer { first.close() }
        let second = try EditorHost()
        defer { second.close() }
        try first.load(Fixture(rows: 3, columns: 3, rich: false).source())
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        counters.observe(first.drawing, cellInputs: first.view.textInputs)
        XCTAssertEqual(counters.maxCellInputInstances, 1, "Another editor must not inflate this editor's input count")
    }

    func testWarmLargeTableMeasurementsMeetBudget() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        for (rows, columns) in [(1_000, 20), (100, 200)] {
            try autoreleasepool {
                let fixture = Fixture(rows: rows, columns: columns, rich: false)
                try warm(fixture, source: fixture.source())
                let sample = try XCTUnwrap(samples.last)
                let ordered = sample.samplesMs.sorted()
                let index = Int(ceil(Double(ordered.count) * Benchmark.warmPercentile)) - 1
                XCTAssertEqual(ordered.count, Benchmark.warmSamples)
                XCTAssertEqual(sample.counters.unchangedCellRemeasurements, 0)
                XCTAssertLessThanOrEqual(ordered[index], Benchmark.warmBudgetMs,
                    "\(fixture.name) unchanged warm measurement p99")
            }
        }
    }

    func testEveryFixtureIsAdmitted() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        for rich in [false, true] {
            for (rows, columns) in [(3, 3), (1_000, 20), (100, 200)] {
                try autoreleasepool {
                    let fixture = Fixture(rows: rows, columns: columns, rich: rich)
                    let host = try EditorHost()
                    defer { host.close() }
                    try host.load(fixture.source())
                    XCTAssertEqual(try host.table().cells.count, rows * columns - (rich ? 1 : 0), fixture.name)
                    _ = try measure(host.drawing) {}
                }
            }
        }
    }

    func testEditorPreparationEmitsLayoutWork() throws {
        let host = try EditorHost()
        defer { host.close() }
        let work = WorkProbe()
        PreparedProseInstrumentation.tableWorkObserverForTesting = work.record
        defer { PreparedProseInstrumentation.tableWorkObserverForTesting = nil }
        try host.load(Fixture(rows: 3, columns: 3, rich: false).source())
        let spans = work.consume(through: DispatchTime.now().uptimeNanoseconds)
        XCTAssertTrue(spans.contains { $0.kind == .layout && $0.endNanos > $0.startNanos },
                      "Editor table preparation must participate in delayed-frame attribution")
    }

    func testNativeStagesObserveReplacementAndTyping() throws {
        let host = try EditorHost()
        defer { host.close() }
        var stages = Set<PreparedProseInstrumentation.TableStage>()
        PreparedProseInstrumentation.tableStageObserverForTesting = { stage, start, end in
            XCTAssertGreaterThan(end, start)
            stages.insert(stage)
        }
        defer { PreparedProseInstrumentation.tableStageObserverForTesting = nil }
        try host.load(Fixture(rows: 3, columns: 3, rich: false).source())
        let input = try host.bind(0)
        input.insertText(Benchmark.text)
        host.view.layoutIfNeeded()
        for stage: PreparedProseInstrumentation.TableStage in [
            .replacementAndFFI, .nativeInputAndFFI, .nativeFrameAndFFI,
            .adapterAdoption, .tablePreparationAndGeometry
        ] {
            XCTAssertTrue(stages.contains(stage), "Missing native timing stage: \(stage)")
        }
    }

    func testExportTablePerformance() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["PREPARED_PROSE_DEVICE_BENCHMARK"] == "1",
                          "Run through NativeEditorPreparedProsePerformance.")
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        for rich in [false, true] {
            for (rows, columns) in [(3, 3), (1_000, 20), (100, 200)] {
                let fixture = Fixture(rows: rows, columns: columns, rich: rich)
                let source = try fixture.source()
                try cold(fixture, source: source)
                let runs = rich ? 1 : Benchmark.typingRuns
                for run in 1...runs { try autoreleasepool { try typing(fixture, source: source, run: run) } }
                for atEnd in [false, true] { try autoreleasepool { try cellChange(fixture, source: source, atEnd: atEnd) } }
                if !rich {
                    try autoreleasepool { try warm(fixture, source: source) }
                    for horizontal in [true, false] {
                        try autoreleasepool { try scroll(fixture, source: source, horizontal: horizontal) }
                    }
                    try autoreleasepool { try structural(fixture, source: source) }
                    try autoreleasepool { try remote(fixture, source: source) }
                }
                _ = try saveExport()
            }
        }
        let data = try saveExport()
        let attachment = XCTAttachment(data: data, uniformTypeIdentifier: "public.json")
        attachment.name = "table-performance-ios.json"
        attachment.lifetime = .keepAlways
        add(attachment)
        print(Benchmark.marker + String(decoding: data, as: UTF8.self))
    }

    private func saveExport() throws -> Data {
        let data = try JSONEncoder().encode(["samples": samples])
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("table-performance-ios.json")
        try data.write(to: url, options: .atomic)
        return data
    }

    func testExporterPrimitives() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["PREPARED_PROSE_DEVICE_BENCHMARK"] == "1",
                          "Run through NativeEditorPreparedProsePerformance.")
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 3, columns: 3, rich: false)
        let source = try fixture.source()
        try cold(fixture, source: source)
        for sample in samples {
            XCTAssertEqual(sample.samplesMs, sample.stageSamplesMs["synchronousAction"],
                "Cold source compilation and exact layout must end before presentation waiting: \(sample.metric)")
        }
        try autoreleasepool { try typing(fixture, source: source, run: 1) }
        try autoreleasepool { try cellChange(fixture, source: source, atEnd: true) }
        try autoreleasepool { try warm(fixture, source: source) }
        try autoreleasepool { try structural(fixture, source: source) }
        try autoreleasepool { try remote(fixture, source: source) }
        XCTAssertTrue(samples.allSatisfy { $0.samplesMs.allSatisfy { $0.isFinite && $0 >= 0 } })
        XCTAssertEqual(samples.first { $0.metric == "cellChangeEnd" }?.counters.changedCellRemeasurements,
                       Benchmark.baselineSamples)
        XCTAssertTrue(samples.allSatisfy { $0.counters.unchangedCellRemeasurements == 0 })
        let typing = try XCTUnwrap(samples.first { $0.metric == "typing" })
        XCTAssertGreaterThan(try XCTUnwrap(typing.wrapCount), 0)
        XCTAssertGreaterThan(try XCTUnwrap(typing.nonWrapCount), 0)
    }

    func testLargeTableHorizontalScrollPresentation() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["PREPARED_PROSE_DEVICE_BENCHMARK"] == "1",
                          "Run through NativeEditorPreparedProsePerformance.")
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 1_000, columns: 20, rich: false)
        let scene = try XCTUnwrap(UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first)
        let viewport = scene.screen.bounds.size
        print("TABLE_HORIZONTAL_VISUAL_PROBE_STARTED viewport=\(viewport)")
        try scroll(fixture, source: fixture.source(), horizontal: true, viewport: viewport)
        XCTAssertFalse(samples.isEmpty)
        XCTAssertTrue(samples.allSatisfy { $0.samplesMs.allSatisfy { $0.isFinite && $0 >= 0 } })
    }

    func testDefaultHeaderKeepsTypedTextReadableAcrossAppearances() throws {
        let minimumTextContrast: CGFloat = 4.5
        let proMaxViewport = CGSize(width: 440, height: 956)
        func luminance(_ color: UIColor, traits: UITraitCollection) -> CGFloat {
            var red: CGFloat = 0, green: CGFloat = 0, blue: CGFloat = 0, alpha: CGFloat = 0
            XCTAssertTrue(color.resolvedColor(with: traits).getRed(&red, green: &green, blue: &blue, alpha: &alpha))
            func linear(_ value: CGFloat) -> CGFloat {
                value <= 0.04045 ? value / 12.92 : pow((value + 0.055) / 1.055, 2.4)
            }
            return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
        }
        let host = try EditorHost(viewport: proMaxViewport, appearance: .light)
        defer { host.close() }
        try host.load(Fixture(rows: 3, columns: 3, rich: false).source())
        let input = try host.bind(0)
        for appearance in [UIUserInterfaceStyle.light, .dark, .light] {
            host.window.overrideUserInterfaceStyle = appearance
            host.view.setNeedsLayout()
            host.view.layoutIfNeeded()
            let insertion = input.textStorage.length
            input.insertText(" typed header")
            host.view.layoutIfNeeded()
            CATransaction.flush()
            let table = try host.table()
            let cell = try XCTUnwrap(table.cell(sourceIndex: 0))
            XCTAssertTrue(cell.isHeader)
            XCTAssertEqual(input.traitCollection.userInterfaceStyle, appearance)
            let line = try XCTUnwrap(cell.content.blocks.flatMap(\.fragments).compactMap(\.line).first)
            let run = try XCTUnwrap((CTLineGetGlyphRuns(line) as? [CTRun])?.first)
            let attributes = try XCTUnwrap(CTRunGetAttributes(run) as? [NSAttributedString.Key: Any])
            let preparedColor = try unwrapCoreTextAttribute(
                XCTUnwrap(attributes[kCTForegroundColorAttributeName as NSAttributedString.Key]), as: CGColor.self)
            XCTAssertEqual(UIColor(cgColor: preparedColor), UIColor.label.resolvedColor(with: input.traitCollection))
            let background = luminance(table.style.headerBackgroundColor, traits: input.traitCollection)
            for index in [0, insertion] {
                let foreground = try XCTUnwrap(input.textStorage.attribute(.foregroundColor, at: index, effectiveRange: nil) as? UIColor)
                let text = luminance(foreground, traits: input.traitCollection)
                let contrast = (max(text, background) + 0.05) / (min(text, background) + 0.05)
                XCTAssertGreaterThanOrEqual(contrast, minimumTextContrast,
                    "appearance=\(appearance.rawValue), character=\(index): bound header text must contrast with its painted background")
            }
            let image = try captureWindow(host.window, name: "typed-header-appearance-\(appearance.rawValue)")
            let cellFrame = try XCTUnwrap(host.surface.cellFrame(tableID: table.identity, cellIndex: 0))
            let point = host.surface.convert(CGPoint(x: cellFrame.minX - table.style.cellPadding / 2,
                                                     y: cellFrame.minY - table.style.cellPadding / 2), to: host.window)
            let bitmap = try XCTUnwrap(image.cgImage)
            let channelCount = 4
            var pixels = [UInt8](repeating: 0, count: bitmap.width * bitmap.height * channelCount)
            try pixels.withUnsafeMutableBytes { buffer in
                let context = try XCTUnwrap(CGContext(data: buffer.baseAddress, width: bitmap.width, height: bitmap.height,
                    bitsPerComponent: 8, bytesPerRow: bitmap.width * channelCount, space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue))
                context.draw(bitmap, in: CGRect(x: 0, y: 0, width: bitmap.width, height: bitmap.height))
            }
            let pixelIndex = (Int(point.y * image.scale) * bitmap.width + Int(point.x * image.scale)) * channelCount
            let channelMaximum: CGFloat = 255
            let painted = UIColor(red: CGFloat(pixels[pixelIndex]) / channelMaximum,
                green: CGFloat(pixels[pixelIndex + 1]) / channelMaximum,
                blue: CGFloat(pixels[pixelIndex + 2]) / channelMaximum, alpha: 1)
            XCTAssertEqual(pixels[pixelIndex + 3], UInt8(channelMaximum), "The composited header sample remains opaque")
            XCTAssertEqual(luminance(painted, traits: input.traitCollection), background, accuracy: 0.01,
                "The header bitmap must use the mounted view's appearance")
        }
    }

    func testHeaderColorDefaultsAndExplicitThemeOverrides() throws {
        let light = UITraitCollection(userInterfaceStyle: .light)
        let dark = UITraitCollection(userInterfaceStyle: .dark)
        for style in [TableStyle(), try XCTUnwrap(EditorTheme(dictionary: ["table": ["cellPadding": 9]]).table)] {
            XCTAssertEqual(style.headerBackgroundColor.resolvedColor(with: light), EditorTheme.color(from: "#F3F4F6"))
            XCTAssertEqual(style.headerBackgroundColor.resolvedColor(with: dark), UIColor.secondarySystemBackground.resolvedColor(with: dark))
            XCTAssertEqual(style.headerBackgroundColor.resolvedColor(with: light).cgColor.alpha, 1)
            XCTAssertEqual(style.headerBackgroundColor.resolvedColor(with: dark).cgColor.alpha, 1)
        }
        let explicit = try XCTUnwrap(EditorTheme(dictionary: ["table": ["headerBackgroundColor": "#e8f0f5"]]).table)
        for traits in [light, dark] {
            XCTAssertEqual(explicit.headerBackgroundColor.resolvedColor(with: traits), EditorTheme.color(from: "#e8f0f5"))
        }
    }

    func testStructuralCounterRecognizesRebuiltCellAfterItsRowMoves() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 3, columns: 3, rich: false)
        let host = try EditorHost()
        defer { host.close() }
        try host.load(fixture.source())
        _ = try host.bind(0)
        _ = try measure(host.drawing) {}
        let rebuild = try XCTUnwrap(host.table().cells[fixture.columns].content.cellPreparation)
        let previousRevision = host.adapter.baseDocumentRevision
        let command = try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "addRowAfter" }).command
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        _ = try measureChange(host, counters: &counters) {
            let update = try XCTUnwrap(host.adapter.commandAtSelection(command, anchor: 0, head: 0))
            XCTAssertTrue(host.view.textView.applyUpdateJSON(update))
            host.view.layoutIfNeeded()
            _ = rebuild()
        }
        XCTAssertGreaterThan(host.adapter.baseDocumentRevision, previousRevision)
        XCTAssertEqual(counters.unchangedCellRemeasurements, 1,
            "The rebuilt original second-row cell is unchanged after moving to the third row")
        XCTAssertEqual(counters.changedCellRemeasurements, 1,
            "The inserted empty cells share one newly prepared shape")
    }

    func testTypingBeyondViewportKeepsTheWholeRowAligned() throws {
        try checkRowScroll(pasteNewlines: false)
    }

    func testPastingNewlinesBeyondViewportKeepsTheWholeRowAligned() throws {
        try checkRowScroll(pasteNewlines: true)
    }

    func testWrappingBeyondViewportKeepsTheWholeRowAligned() throws {
        try checkRowScroll(pasteNewlines: false, wraps: true)
    }

    func testKeyboardTypingScrollsTheRootAndAutoGrowAncestor() throws {
        for autoGrow in [false, true] {
            try checkRowScroll(pasteNewlines: false, autoGrow: autoGrow, keyboard: true)
        }
    }

    func testExplicitParagraphSpacingKeepsTheWholeRowAligned() throws {
        for spacing in [0, 12] {
            try checkRowScroll(pasteNewlines: true, theme: EditorTheme(dictionary: ["paragraph": ["spacingAfter": spacing]]))
        }
    }

    func testCellLineBreaksFitTheSharedPreparedRow() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        var config = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(Benchmark.schema.utf8)) as? [String: Any])
        var schema = try XCTUnwrap(config["schema"] as? [String: Any])
        var nodes = try XCTUnwrap(schema["nodes"] as? [[String: Any]])
        nodes += [
            ["name": "codeBlock", "content": "text*", "group": "block", "role": "textBlock"],
            ["name": "h1", "content": "inline*", "group": "block", "role": "textBlock"],
            ["name": "blockquote", "content": "block+", "group": "block", "role": "block"],
            ["name": "bulletList", "content": "listItem+", "group": "block", "role": "list"],
            ["name": "listItem", "content": "block+", "role": "listItem"],
            ["name": "hardBreak", "content": "", "group": "inline", "role": "hardBreak", "isVoid": true]
        ]
        schema["nodes"] = nodes
        config["schema"] = schema
        let configJSON = String(decoding: try JSONSerialization.data(withJSONObject: config), as: UTF8.self)
        let paragraph: [String: Any] = ["type": "paragraph", "content": [["type": "text", "text": "a\n\nb\n"]]]
        let splitCRLF: [String: Any] = ["type": "paragraph", "content": [
            ["type": "text", "text": "a\r", "marks": [["type": TableToolbarTestItems.strongMark]]],
            ["type": "text", "text": "\nb\n"]
        ]]
        let hardBreak: [String: Any] = ["type": "paragraph", "content": [
            ["type": "text", "text": "a\n"], ["type": "hardBreak"]
        ]]
        let listParagraph: [String: Any] = ["type": "paragraph", "content": [["type": "text", "text": "List line"]]]
        let listParagraphs = Array(repeating: listParagraph, count: 20)
        let nestedList: [String: Any] = ["type": "bulletList", "content": [["type": "listItem", "content": [listParagraph]]]]
        let quotedHeadings: [String: Any] = ["type": "blockquote", "content": Array(repeating:
            ["type": "h1", "content": [["type": "text", "text": "Heading"]]], count: 10)]
        let contents: [[String: Any]] = [paragraph, splitCRLF, hardBreak, quotedHeadings,
            ["type": "bulletList", "content": [["type": "listItem", "content": [listParagraph, nestedList, listParagraph]]]],
            ["type": "bulletList", "content": [["type": "listItem", "content": [hardBreak]], ["type": "listItem", "content": [listParagraph]]]],
            ["type": "bulletList", "content": listParagraphs.map { ["type": "listItem", "content": [$0]] }],
            ["type": "bulletList", "content": [["type": "listItem", "content": listParagraphs]]],
            ["type": "bulletList", "content": [["type": "listItem", "content": [hardBreak]]]],
            ["type": "blockquote", "content": [["type": "codeBlock", "content": [["type": "text", "text": "a\nb\n"]]]]],
            ["type": "blockquote", "content": [["type": "h1", "content": [["type": "text", "text": "a\nb\n"]]]]]
        ]
        let themes: [[String: Any]] = [[:], ["text": ["spacingAfter": 0]],
            ["paragraph": ["spacingAfter": 20]],
            ["paragraph": ["spacingAfter": 12], "list": ["itemSpacing": 3]],
            ["blockquote": ["text": ["spacingAfter": 20]], "headings": ["h1": ["fontSize": 32, "fontWeight": "700"]]],
            ["version": 1, "styles": ["paragraph": ["marginTop": 3, "marginBottom": 7]]]
        ]
        for theme in themes {
            let host = try EditorHost(id: makeV2Editor(configJson: configJSON))
            defer { host.close() }
            XCTAssertTrue(host.view.applyTheme(EditorTheme(dictionary: theme)))
            for content in contents {
                let source: [String: Any] = ["type": "doc", "content": [["type": "table", "content": [
                    ["type": "table_row", "content": [["type": "table_cell", "content": [content]]]]
                ]]]]
                try host.load(String(decoding: try JSONSerialization.data(withJSONObject: source), as: UTF8.self))
                let input = try host.bind(0)
                _ = try measure(host.drawing) {}
                let diagnostic = "content=\(content), theme=\(theme), input=\(input.bounds)"
                XCTAssertEqual(NSMaxRange(input.layoutManager.glyphRange(for: input.textContainer)),
                               input.layoutManager.numberOfGlyphs, diagnostic)
                let caret = input.caretRect(for: try XCTUnwrap(input.selectedTextRange).end)
                XCTAssertGreaterThanOrEqual(caret.height, input.baseFont.lineHeight / 2, diagnostic)
                XCTAssertLessThanOrEqual(caret.maxY, input.bounds.height + 1, diagnostic)
                let cell = try XCTUnwrap(host.table().cell(sourceIndex: 0))
                let fragments = cell.content.blocks.flatMap(\.fragments)
                if content["type"] as? String == "bulletList" {
                    let preparedLines = fragments.filter { $0.kind == .text }.map { $0.origin.y }
                    var nativeLines: [CGFloat] = []
                    var visibleLineIndices: [Int] = []
                    input.layoutManager.enumerateLineFragments(forGlyphRange: NSRange(location: 0, length: input.layoutManager.numberOfGlyphs)) { rect, _, _, range, _ in
                        let characters = input.layoutManager.characterRange(forGlyphRange: range, actualGlyphRange: nil)
                        let text = (input.textStorage.string as NSString).substring(with: characters)
                        let whitespace = CharacterSet.whitespacesAndNewlines.union(CharacterSet(charactersIn: String(EditorTextView.emptyBlockPlaceholderScalar)))
                        if !text.trimmingCharacters(in: whitespace).isEmpty { visibleLineIndices.append(nativeLines.count) }
                        nativeLines.append(rect.minY + input.layoutManager.location(forGlyphAt: range.location).y)
                    }
                    XCTAssertEqual(preparedLines.count, nativeLines.count, diagnostic)
                    for (previous, index) in zip(visibleLineIndices, visibleLineIndices.dropFirst()) where preparedLines.indices.contains(index) {
                        XCTAssertEqual(preparedLines[index] - preparedLines[previous],
                                       nativeLines[index] - nativeLines[previous],
                                       accuracy: 1, "line=\(index), \(diagnostic)")
                    }
                }
                if let marker = fragments.first(where: { $0.kind == .marker }),
                   let firstLine = fragments.first(where: { $0.kind == .text }) {
                    XCTAssertEqual(marker.bounds.midY, firstLine.bounds.midY, accuracy: 1, diagnostic)
                }
            }
        }
    }

    private func checkRowScroll(pasteNewlines: Bool, wraps: Bool = false,
                               autoGrow: Bool = false, keyboard: Bool = false, theme: EditorTheme? = nil) throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let host = try EditorHost()
        defer { host.close() }
        if let theme { XCTAssertTrue(host.view.applyTheme(theme)) }
        let scroll: UIScrollView
        if autoGrow {
            let ancestor = UIScrollView(frame: host.window.bounds)
            host.view.removeFromSuperview()
            host.window.addSubview(ancestor)
            ancestor.addSubview(host.view)
            let minimumHeight = host.window.bounds.height
            host.view.onHeightMayChange = { [weak view = host.view, weak ancestor] height in
                guard let view, let ancestor else { return }
                let resolved = max(minimumHeight, height)
                view.frame.size.height = resolved
                ancestor.contentSize = CGSize(width: view.bounds.width, height: resolved)
            }
            host.view.heightBehavior = .autoGrow
            scroll = ancestor
        } else {
            scroll = host.view.textView
        }
        try host.load(Fixture(rows: 3, columns: 3, rich: false).source())
        let input = try host.bind(0)
        let keyboardHeight: CGFloat = 300
        if keyboard {
            if autoGrow { scroll.contentInset.bottom = keyboardHeight }
            let frame = host.window.convert(CGRect(x: 0, y: host.window.bounds.maxY - keyboardHeight,
                width: host.window.bounds.width, height: keyboardHeight), to: host.window.screen.coordinateSpace)
            NotificationCenter.default.post(name: UIResponder.keyboardWillChangeFrameNotification, object: nil,
                userInfo: [UIResponder.keyboardFrameEndUserInfoKey: NSValue(cgRect: frame),
                           UIResponder.keyboardAnimationDurationUserInfoKey: 0])
        }
        defer {
            if keyboard { NotificationCenter.default.post(name: UIResponder.keyboardWillHideNotification, object: nil) }
        }
        _ = try measure(host.drawing) {}
        let initialOffset = scroll.contentOffset.y
        let overflowLines = 10
        let lineCount = Int(ceil(scroll.bounds.height / input.baseFont.lineHeight)) + overflowLines
        for line in 0..<lineCount {
            _ = try measure(host.drawing) {
                if wraps {
                    input.insertText("wrapped text line \(line) ")
                } else if pasteNewlines {
                    input.insertText("\nline \(line)")
                } else {
                    input.insertText("\n")
                    input.insertText("line \(line)")
                }
                host.view.layoutIfNeeded()
            }
        }
        let table = try host.table()
        let edited = try XCTUnwrap(table.cell(sourceIndex: 0))
        let adjacent = try XCTUnwrap(table.cell(sourceIndex: 1))
        XCTAssertGreaterThan(table.frame(ofCell: edited).height, scroll.bounds.height)
        XCTAssertEqual(table.frame(ofCell: edited).height, table.frame(ofCell: adjacent).height)
        XCTAssertEqual(NSMaxRange(input.layoutManager.glyphRange(for: input.textContainer)), input.layoutManager.numberOfGlyphs,
                       "The shared row must contain every native input glyph")
        XCTAssertEqual(input.contentOffset.y, 0, accuracy: 1,
                       "The active cell must not scroll its text independently of its row")
        XCTAssertGreaterThan(scroll.contentOffset.y, initialOffset,
                             "Caret reveal must move the whole document row")
        let selection = try XCTUnwrap(input.selectedTextRange)
        XCTAssertEqual(input.selectedRange, NSRange(location: input.textStorage.length, length: 0))
        XCTAssertGreaterThanOrEqual(input.caretRect(for: selection.end).height, input.baseFont.lineHeight / 2)
        XCTAssertLessThanOrEqual(input.caretRect(for: selection.end).maxY, input.bounds.height + 1)
        let caret = scroll.convert(input.caretRect(for: selection.end), from: input)
        let visibleBottom = scroll.bounds.maxY - scroll.adjustedContentInset.bottom
        XCTAssertLessThanOrEqual(caret.maxY, visibleBottom + 1,
                                 "The typed caret must remain above the keyboard")
        if wraps { _ = try captureWindow(host.window, name: "typed-tall-row") }
        let manualOffset = max(-scroll.adjustedContentInset.top, scroll.contentOffset.y - scroll.bounds.height / 2)
        scroll.setContentOffset(CGPoint(x: scroll.contentOffset.x, y: manualOffset), animated: false)
        _ = try measure(host.drawing) {}
        _ = try measure(host.drawing) {}
        XCTAssertEqual(scroll.contentOffset.y, manualOffset, accuracy: 1,
                       "Ordinary scrolling must not snap back to the caret")
        let map = try XCTUnwrap(input.tableCellPositionMap)
        let first = try XCTUnwrap(map.globalScalar(forLocalScalar: 0))
        let last = try XCTUnwrap(map.globalScalar(forLocalScalar: UInt32(input.textStorage.string.unicodeScalars.count)))
        _ = input.applySelectionFromJSON(["type": "text", "anchor": NSNumber(value: last), "head": NSNumber(value: first),
                                         "anchorScalar": NSNumber(value: last), "headScalar": NSNumber(value: first)])
        input.textViewDidChangeSelection(input)
        _ = try measure(host.drawing) {}
        XCTAssertEqual(input.currentLogicalScalarSelection()?.head, first)
        let headCaret = scroll.convert(input.caretRect(for: try XCTUnwrap(input.selectedTextRange).start), from: input)
        XCTAssertGreaterThanOrEqual(headCaret.minY, scroll.bounds.minY + scroll.adjustedContentInset.top - 1,
                                   "A backward selection must reveal its head")
        XCTAssertLessThanOrEqual(headCaret.maxY, scroll.bounds.maxY - scroll.adjustedContentInset.bottom + 1)
        input.selectedRange = NSRange(location: input.textStorage.length / 2, length: 0)
        let revision = host.adapter.baseDocumentRevision
        input.setMarkedText("仮\n文", selectedRange: NSRange(location: 3, length: 0))
        _ = try measure(host.drawing) {}
        XCTAssertNotNil(input.markedTextRange)
        XCTAssertEqual(host.adapter.baseDocumentRevision, revision, "Visual reveal must not commit composition")
    }

    private func captureWindow(_ window: UIWindow, name: String) throws -> UIImage {
        let image = UIGraphicsImageRenderer(bounds: window.bounds).image { context in
            window.layer.render(in: context.cgContext)
        }
        let attachment = XCTAttachment(image: image)
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
        try XCTUnwrap(image.pngData()).write(to: FileManager.default.temporaryDirectory.appendingPathComponent("\(name).png"))
        return image
    }

    func testEndCellActivationAfterAnotherHostStaysWithinScrollBounds() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 1_000, columns: 20, rich: false)
        let source = try fixture.source()
        try cellChange(fixture, source: source, atEnd: false)
        let host = try EditorHost()
        defer { host.close() }
        try host.load(source)
        let scroll = host.view.textView
        func scrollState() -> String {
            "offset=\(scroll.contentOffset.y) presentation=\(String(describing: scroll.layer.presentation()?.bounds.origin.y)) size=\(scroll.contentSize.height) bounds=\(scroll.bounds.height) inset=\(scroll.adjustedContentInset) keyboard=\(scroll.keyboardBottomInset) drawing=\(host.drawing.bounds.origin.y)"
        }
        var trace: [String] = []
        let notifications = [UIResponder.keyboardWillChangeFrameNotification,
            UIResponder.keyboardDidChangeFrameNotification, UIResponder.keyboardWillHideNotification]
        let observers = notifications.map { name in
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { notification in
                trace.append("\(notification.name.rawValue) \(scrollState())")
            }
        }
        defer { observers.forEach(NotificationCenter.default.removeObserver) }
        let input = try host.bind(fixture.rows * fixture.columns - 1)
        trace.append("bound \(scrollState())")
        let minimum = -scroll.adjustedContentInset.top
        let maximum = max(minimum, scroll.contentSize.height - scroll.bounds.height + scroll.adjustedContentInset.bottom)
        XCTAssertLessThanOrEqual(scroll.contentOffset.y, maximum, trace.joined(separator: "\n"))
        _ = try measure(host.drawing) {}
        trace.append("presented \(scrollState())")
        let revision = host.adapter.baseDocumentRevision
        var idleCounters = PreparedProseInstrumentation.TablePerformanceCounters()
        _ = try measureChange(host, counters: &idleCounters) {}
        trace.append("idle \(scrollState()) unchanged=\(idleCounters.unchangedCellRemeasurements)")
        XCTAssertEqual(host.adapter.baseDocumentRevision, revision)
        var editCounters = PreparedProseInstrumentation.TablePerformanceCounters()
        _ = try edit(host, input: input, counters: &editCounters, sample: "end-cell activation after host replacement")
        trace.append("edited \(scrollState()) unchanged=\(editCounters.unchangedCellRemeasurements)")
        print("TABLE_ACTIVATION_TRACE " + trace.joined(separator: "\n"))
        XCTAssertEqual(idleCounters.unchangedCellRemeasurements, 0, trace.joined(separator: "\n"))
        XCTAssertEqual(editCounters.unchangedCellRemeasurements, 0, trace.joined(separator: "\n"))
    }

    func testLargeStructuralChangesReuseUnchangedGeometry() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 1_000, columns: 20, rich: false)
        try structural(fixture, source: fixture.source())
        let sample = try XCTUnwrap(samples.first { $0.metric == "structuralCommand" })
        XCTAssertEqual(sample.counters.unchangedCellRemeasurements, 0)
        XCTAssertEqual(sample.counters.changedCellRemeasurements, 1,
            "All inserted empty rows share one shape and keep separate bindings")
    }

    func testLargeTableStyleBackgroundHasViewportSizedBacking() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let host = try EditorHost()
        defer { host.close() }
        try host.load(Fixture(rows: 1_000, columns: 20, rich: true).source())
        let background = host.view.textView.styleContentView
        XCTAssertFalse(background.isOpaque, "A transparent or rounded background must clear its backing")
        _ = try clock.present(host.drawing) {}
        XCTAssertGreaterThan(background.bounds.height, 0)
        XCTAssertGreaterThan(background.bounds.width, 0)
        XCTAssertLessThanOrEqual(background.bounds.height, host.window.bounds.height,
                                 "The stylesheet background must not allocate a document-height bitmap")
        XCTAssertLessThanOrEqual(background.bounds.width, host.window.bounds.width)
    }

    func testLargeTableColdLayout() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["PREPARED_PROSE_DEVICE_BENCHMARK"] == "1",
                          "Run through NativeEditorPreparedProsePerformance.")
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 1_000, columns: 20, rich: false)
        try cold(fixture, source: fixture.source())
        _ = try saveExport()
    }

    func testLargeTableTypingPreservesIncrementalPreparation() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["PREPARED_PROSE_DEVICE_BENCHMARK"] == "1",
                          "Run through NativeEditorPreparedProsePerformance.")
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        for rich in [false, true] {
            let fixture = Fixture(rows: 1_000, columns: 20, rich: rich)
            let source = try fixture.source()
            for run in 1...(rich ? 1 : Benchmark.typingRuns) {
                try autoreleasepool { try typing(fixture, source: source, run: run) }
                let sample = try XCTUnwrap(samples.last)
                XCTAssertEqual(sample.samplesMs.count, Benchmark.typingSamples)
                XCTAssertEqual(sample.counters.unchangedCellRemeasurements, 0, fixture.name)
                XCTAssertEqual(sample.counters.changedCellRemeasurements, Benchmark.typingSamples, fixture.name)
                XCTAssertGreaterThan(try XCTUnwrap(sample.wrapCount), 0, fixture.name)
                XCTAssertGreaterThan(try XCTUnwrap(sample.nonWrapCount), 0, fixture.name)
                _ = try saveExport()
            }
        }
    }

    func testLargeBoundCellPreparationStartsAfterActivationFrame() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 1_000, columns: 20, rich: false)
        let source = try fixture.source()
        try cellChange(fixture, source: source, atEnd: true)
        for sample in samples {
            XCTAssertEqual(sample.counters.unchangedCellRemeasurements, 0, sample.metric)
        }
        XCTAssertEqual(samples.first { $0.metric == "cellChangeEnd" }?.counters.changedCellRemeasurements,
                       Benchmark.baselineSamples)
    }

    func testInputTimingUsesDisplayedFrameAfterCommit() throws {
        clock = TableTestFrameClock()
        defer { clock.close(); clock = nil }
        let host = try EditorHost()
        defer { host.close() }
        let fixture = Fixture(rows: 3, columns: 3, rich: false)
        try host.load(fixture.source())
        let input = try host.bind(fixture.rows * fixture.columns / 2)
        _ = try measure(host.drawing) {}
        for edit in 0..<Benchmark.baselineSamples {
            let measurement = try measure(host.drawing) {
                input.insertText(Benchmark.text)
                host.view.layoutIfNeeded()
            }
            let frame = try XCTUnwrap(measurement.presentation)
            XCTAssertGreaterThanOrEqual(frame.displayed, frame.commit,
                "Edit \(edit): a delayed callback for a pre-commit frame cannot acknowledge this edit")
            XCTAssertEqual(frame.measured, frame.displayed,
                "Edit \(edit): a scheduled future frame is not a displayed frame")
        }
    }

    private func measure(_ drawing: PreparedProseDrawingView, endpoint: MeasurementEndpoint = .displayedFrameAfterCommit,
                         action: () throws -> Void) throws -> Measurement {
        let stages = StageProbe()
        PreparedProseInstrumentation.tableStageObserverForTesting = stages.record
        defer { PreparedProseInstrumentation.tableStageObserverForTesting = nil }
        let frame = try clock.present(drawing, action: action)
        let start = frame.start
        let actionEnd = frame.actionEnd
        let end = frame.displayed
        var measured = stages.durationsMs()
        measured["synchronousAction"] = (actionEnd - start) * Benchmark.millisecondsPerSecond
        let measurementEnd: Double
        switch endpoint {
        case .exactLayout:
            measurementEnd = actionEnd
            measured["postLayoutPresentationWait"] = (end - actionEnd) * Benchmark.millisecondsPerSecond
        case .displayedFrameAfterCommit:
            measurementEnd = end
            measured["presentationWait"] = (end - actionEnd) * Benchmark.millisecondsPerSecond
        }
        return Measurement(durationMs: (measurementEnd - start) * Benchmark.millisecondsPerSecond, stagesMs: measured,
            presentation: (frame.commit, frame.displayed, end))
    }

    private func append(_ fixture: Fixture, metric: String, run: Int = 1, values: [Measurement],
                        counters: PreparedProseInstrumentation.TablePerformanceCounters,
                        attributed: [Bool]? = nil, wraps: Int? = nil) {
        var system = utsname()
        uname(&system)
        let machineCapacity = MemoryLayout.size(ofValue: system.machine)
        let device = withUnsafePointer(to: &system.machine) {
            $0.withMemoryRebound(to: CChar.self, capacity: machineCapacity) { String(cString: $0) }
        }
        #if DEBUG
        let buildType = "debug"
        #else
        let buildType = "release"
        #endif
        #if targetEnvironment(simulator)
        let physical = false
        #else
        let physical = true
        #endif
        let stageNames = Set(values.flatMap { $0.stagesMs.keys })
        let stageSamples = Dictionary(uniqueKeysWithValues: stageNames.map { stage in
            (stage, values.map { $0.stagesMs[stage] ?? 0 })
        })
        samples.append(Sample(device: device, buildType: buildType, physicalDevice: physical,
            refreshHz: UIScreen.main.maximumFramesPerSecond, fixture: fixture.name, metric: metric, run: run,
            samplesMs: values.map(\.durationMs), stageSamplesMs: stageSamples,
            warmupSamplesDiscarded: metric == "typing" ? Benchmark.warmupSamples : nil,
            tableAttributed: attributed, wrapCount: wraps, nonWrapCount: wraps.map { values.count - $0 }, counters: counters))
        print("TABLE_PERFORMANCE_CASE fixture=\(fixture.name) metric=\(metric) run=\(run) samples=\(values.count) wraps=\(wraps.map(String.init) ?? "n/a")")
    }

    private func cold(_ fixture: Fixture, source: String) throws {
        var editor: [Measurement] = []
        var viewer: [Measurement] = []
        var editorCounters = PreparedProseInstrumentation.TablePerformanceCounters()
        var viewerCounters = PreparedProseInstrumentation.TablePerformanceCounters()
        for _ in 0..<Benchmark.coldSamples {
            try autoreleasepool {
                let host = try EditorHost()
                defer { host.close() }
                editor.append(try measure(host.drawing, endpoint: .exactLayout) { try host.load(source) })
                editorCounters.observe(host.drawing, cellInputs: host.view.textInputs)
                editorCounters.authoritativeDocumentBytes = max(editorCounters.authoritativeDocumentBytes, try host.authoritativeBytes())
            }
            try autoreleasepool {
                let registry = PreparedProseLayoutRegistry()
                let window = makeTestWindow(frame: CGRect(origin: .zero, size: Benchmark.viewport))
                let view = ProseViewerView(frame: window.bounds, layoutRegistry: registry)
                window.addSubview(view)
                window.makeKeyAndVisible()
                defer { view.prepareForReuse(); window.isHidden = true }
                viewer.append(try measure(view.drawingViewForTesting, endpoint: .exactLayout) {
                    XCTAssertTrue(view.apply(source: .json(source), configuration: .init(configJSON: Benchmark.schema)))
                    let exact = view.intrinsicContentSize
                    XCTAssertGreaterThan(exact.height, 0)
                    view.layoutIfNeeded()
                    XCTAssertEqual(view.drawingViewForTesting.layout?.size.height, exact.height)
                })
                viewerCounters.observe(view.drawingViewForTesting, additionalUnmountedBytes: registry.layoutCache.unmountedRetainedBytesForTesting)
                viewerCounters.authoritativeDocumentBytes = max(viewerCounters.authoritativeDocumentBytes, registry.compiledDocumentBytesForTesting)
            }
        }
        append(fixture, metric: "editorColdLayout", values: editor, counters: editorCounters)
        append(fixture, metric: "viewerColdLayout", values: viewer, counters: viewerCounters)
    }

    private func measureChange(_ host: EditorHost,
                               counters: inout PreparedProseInstrumentation.TablePerformanceCounters,
                               action: () throws -> Void) throws -> Measurement {
        let oldKeys = Set(try XCTUnwrap(host.table().sourceTable).cells.map(\.contentKey))
        var prepared: [(Int, String)] = []
        host.surface.onTableCellPreparedForTesting = { prepared.append(($0, $1)) }
        defer { host.surface.onTableCellPreparedForTesting = nil }
        let duration = try measure(host.drawing, action: action)
        var unchanged: [Int] = []
        for (index, key) in prepared {
            if oldKeys.contains(key) {
                counters.unchangedCellRemeasurements += 1
                unchanged.append(index)
            }
            else { counters.changedCellRemeasurements += 1 }
        }
        if !unchanged.isEmpty { print("TABLE_UNCHANGED_PREPARATION indices=\(unchanged)") }
        counters.observe(host.drawing, cellInputs: host.view.textInputs)
        return duration
    }

    private func edit(_ host: EditorHost, input: EditorTextView,
                      counters: inout PreparedProseInstrumentation.TablePerformanceCounters,
                      sample: String) throws -> (Measurement, Bool) {
        func requireFocus(_ phase: String) throws {
            _ = try XCTUnwrap(input.isFirstResponder ? input : nil,
                "\(sample) \(phase): editor=\(host.id), activeInput=\(host.view.activeTextInput === input), attached=\(input.window != nil), keyWindow=\(input.window?.isKeyWindow == true), hidden=\(input.isHidden)")
        }
        try requireFocus("before input")
        let cellIndex = try XCTUnwrap(input.tableCellPositionMap).binding.cellIndex
        let oldHeight = try XCTUnwrap(try host.table().cell(sourceIndex: Int(cellIndex))).contentSize.height
        let previousRevision = host.adapter.baseDocumentRevision
        let previousLength = input.textStorage.length
        let duration = try measureChange(host, counters: &counters) {
            input.insertText(Benchmark.text)
            host.view.layoutIfNeeded()
        }
        try requireFocus("after frame")
        XCTAssertEqual(input.textStorage.length, previousLength + Benchmark.text.utf16.count)
        XCTAssertGreaterThan(host.adapter.baseDocumentRevision, previousRevision)
        let height = try XCTUnwrap(try host.table().cell(sourceIndex: Int(cellIndex))).contentSize.height
        return (duration, height != oldHeight)
    }

    private func typing(_ fixture: Fixture, source: String, run: Int) throws {
        let host = try EditorHost()
        defer { host.close() }
        try host.load(source)
        let input = try host.bind(0)
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        for index in 0..<Benchmark.warmupSamples {
            _ = try edit(host, input: input, counters: &counters, sample: "\(fixture.name) run=\(run) warmup=\(index)")
        }
        counters = .init()
        var values: [Measurement] = []
        var wraps = 0
        for index in 0..<Benchmark.typingSamples {
            let (duration, wrapped) = try edit(host, input: input, counters: &counters,
                sample: "\(fixture.name) run=\(run) sample=\(index)")
            values.append(duration)
            if wrapped { wraps += 1 }
        }
        counters.authoritativeDocumentBytes = try host.authoritativeBytes()
        append(fixture, metric: "typing", run: run, values: values, counters: counters, wraps: wraps)
    }

    private func cellChange(_ fixture: Fixture, source: String, atEnd: Bool) throws {
        let host = try EditorHost()
        defer { host.close() }
        try host.load(source)
        let input = try host.bind(atEnd ? (try host.table()).cells.count - 1 : 0)
        _ = try measure(host.drawing) {}
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        var values: [Measurement] = []
        for index in 0..<Benchmark.baselineSamples {
            values.append(try edit(host, input: input, counters: &counters,
                sample: "\(fixture.name) cellChange atEnd=\(atEnd) sample=\(index)").0)
        }
        counters.authoritativeDocumentBytes = try host.authoritativeBytes()
        append(fixture, metric: atEnd ? "cellChangeEnd" : "cellChangeStart", values: values, counters: counters)
    }

    private func warm(_ fixture: Fixture, source: String) throws {
        let registry = PreparedProseLayoutRegistry()
        let window = makeTestWindow(frame: CGRect(origin: .zero, size: Benchmark.viewport))
        let view = ProseViewerView(frame: window.bounds, layoutRegistry: registry)
        window.addSubview(view)
        window.makeKeyAndVisible()
        defer { view.prepareForReuse(); window.isHidden = true }
        _ = try measure(view.drawingViewForTesting) {
            XCTAssertTrue(view.apply(source: .json(source), configuration: .init(configJSON: Benchmark.schema)))
            view.layoutIfNeeded()
        }
        let baseline = registry.layoutPreparationCount
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        let values = (0..<Benchmark.warmSamples).map { _ -> Measurement in
            let start = CACurrentMediaTime()
            _ = view.intrinsicContentSize
            return Measurement(durationMs: (CACurrentMediaTime() - start) * Benchmark.millisecondsPerSecond)
        }
        counters.unchangedCellRemeasurements = registry.layoutPreparationCount - baseline
        counters.observe(view.drawingViewForTesting, additionalUnmountedBytes: registry.layoutCache.unmountedRetainedBytesForTesting)
        counters.authoritativeDocumentBytes = registry.compiledDocumentBytesForTesting
        append(fixture, metric: "warmMeasurement", values: values, counters: counters)
    }

    private func scroll(_ fixture: Fixture, source: String, horizontal: Bool,
                        viewport: CGSize = Benchmark.viewport) throws {
        let host = try EditorHost(viewport: viewport)
        defer { host.close() }
        try host.load(source)
        _ = try measure(host.drawing) {}
        let table = try host.table()
        let horizontalRange = max(0, table.bounds.width - table.hostViewportWidth)
        let verticalRange = max(0, host.view.textView.contentSize.height - host.view.bounds.height)
        var values: [Measurement] = []
        var attributed: [Bool] = []
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        var previous: Double?
        var traversalMs = 0.0
        let work = WorkProbe()
        PreparedProseInstrumentation.tableWorkObserverForTesting = work.record
        let start = CACurrentMediaTime()
        var finished = false
        host.drawing.onMountedTableCellsDrawnForTesting = { count in
            counters.retainedPresentations = max(counters.retainedPresentations, count)
            counters.unmountedCacheBytes = max(counters.unmountedCacheBytes, table.layoutStore.unmountedRetainedBytes)
        }
        clock.onTick = { link in
            if let previous {
                let duration = link.timestamp - previous
                values.append(Measurement(durationMs: duration * Benchmark.millisecondsPerSecond))
                traversalMs += duration * Benchmark.millisecondsPerSecond
                let from = UInt64(previous * Benchmark.nanosecondsPerSecond)
                let to = UInt64(link.timestamp * Benchmark.nanosecondsPerSecond)
                attributed.append(PreparedProseInstrumentation.viewerCaused(from, to,
                    work.consume(through: to), rawDeltaNanos: to - from,
                    nominalFramePeriodNanos: PreparedProseInstrumentation.nominalFramePeriodNanos))
            }
            previous = link.timestamp
            let elapsed = link.timestamp - start
            if traversalMs >= Benchmark.traversalSeconds * Benchmark.millisecondsPerSecond { finished = true; return }
            let fraction = CGFloat((1 - cos(elapsed / Benchmark.traversalSeconds * 2 * .pi)) / 2)
            if horizontal {
                let current = host.drawing.tableLogicalOffset(for: table.identity)
                _ = host.drawing.scrollTables(in: [table.scrollIdentity], by: current - fraction * horizontalRange)
            } else {
                host.view.textView.contentOffset.y = fraction * verticalRange
            }
            host.view.layoutIfNeeded()
            host.drawing.setNeedsDisplay()
        }
        defer {
            clock.onTick = nil
            host.drawing.onMountedTableCellsDrawnForTesting = nil
            PreparedProseInstrumentation.tableWorkObserverForTesting = nil
        }
        while !finished { RunLoop.main.run(until: Date().addingTimeInterval(TableTestFrameClock.runLoopSlice)) }
        counters.observe(host.drawing, cellInputs: host.view.textInputs)
        counters.authoritativeDocumentBytes = try host.authoritativeBytes()
        append(fixture, metric: horizontal ? "scrollHorizontal" : "scrollVertical", values: values,
               counters: counters, attributed: attributed)
    }

    private func structural(_ fixture: Fixture, source: String) throws {
        let host = try EditorHost()
        defer { host.close() }
        try host.load(source)
        _ = try host.bind(0)
        _ = try measure(host.drawing) {}
        var values: [Measurement] = []
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        let command = try XCTUnwrap(TableAccessibilityAction.all.first { $0.key == "addRowAfter" }).command
        for _ in 0..<Benchmark.baselineSamples {
            let previousCellCount = try host.table().cells.count
            values.append(try measureChange(host, counters: &counters) {
                let update = try XCTUnwrap(host.adapter.commandAtSelection(command, anchor: 0, head: 0),
                                          "structural command: \(host.adapter.debugNotes)")
                XCTAssertTrue(host.view.textView.applyUpdateJSON(update))
                host.view.layoutIfNeeded()
            })
            XCTAssertEqual(try host.table().cells.count, previousCellCount + fixture.columns)
            counters.observe(host.drawing, cellInputs: host.view.textInputs)
        }
        counters.authoritativeDocumentBytes = try host.authoritativeBytes()
        append(fixture, metric: "structuralCommand", values: values, counters: counters)
    }

    private func remote(_ fixture: Fixture, source: String) throws {
        let seed = try TableRoomSeed(localConfigJson: Benchmark.schema, documentJson: source)
        let host = try EditorHost(id: seed.makeEditor())
        defer { host.close() }
        let peerID = seed.makeEditor()
        defer { destroyV2Editor(id: peerID) }
        let peer = try XCTUnwrap(EditorV2Registry.adapter(forLegacyId: peerID))
        let relay = try TableCollaborationRelay(editorIds: [host.adapter.editorId, peer.editorId])
        _ = try relay.exchangeUntilIdle()
        XCTAssertNotNil(peer.refreshFromRustState(mirrorSelection: nil))
        XCTAssertTrue(host.view.textView.applyUpdateJSON(try XCTUnwrap(host.adapter.refreshFromRustState(mirrorSelection: nil))))
        host.view.layoutIfNeeded()
        _ = try measure(host.drawing) {}
        var values: [Measurement] = []
        var counters = PreparedProseInstrumentation.TablePerformanceCounters()
        for _ in 0..<Benchmark.baselineSamples {
            let previousRevision = peer.baseDocumentRevision
            XCTAssertNotNil(peer.insertText(Benchmark.text, atScalar: 0))
            XCTAssertGreaterThan(peer.baseDocumentRevision, previousRevision, "peer must commit the measured edit: \(peer.debugNotes)")
            values.append(try measureChange(host, counters: &counters) {
                XCTAssertTrue(try relay.exchangeUntilIdle().contains(host.adapter.editorId))
                XCTAssertTrue(host.view.textView.applyUpdateJSON(try XCTUnwrap(host.adapter.refreshFromRustState(mirrorSelection: nil))))
                host.view.layoutIfNeeded()
            })
            counters.observe(host.drawing, cellInputs: host.view.textInputs)
        }
        counters.authoritativeDocumentBytes = try host.authoritativeBytes()
        append(fixture, metric: "remoteUpdate", values: values, counters: counters)
    }
}
