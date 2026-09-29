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
        static let baselineSamples = 10
        static let traversalSeconds = 30.0
        static let millisecondsPerSecond = 1_000.0
        static let nanosecondsPerSecond = 1_000_000_000.0
        static let nanosecondsPerMillisecond = nanosecondsPerSecond / millisecondsPerSecond
        static let frameTimeout = 120.0
        static let runLoopSlice = 0.005
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

    private final class FrameClock: NSObject {
        private var link: CADisplayLink!
        var onTick: ((CADisplayLink) -> Void)?

        override init() {
            super.init()
            link = CADisplayLink(target: self, selector: #selector(tick(_:)))
            PreparedProseInstrumentation.configureBenchmarkCadence(link)
            link.add(to: .main, forMode: .common)
        }

        func close() { onTick = nil; link.invalidate() }

        @objc private func tick(_ link: CADisplayLink) {
            onTick?(link)
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
        let stageTimingSemantics = "inclusive wall-time unions; native stages include Rust and FFI; preparation includes geometry; presentationWait includes scheduling, render server and GPU without separating them"
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
        let window = UIWindow(frame: CGRect(origin: .zero, size: Benchmark.viewport))
        let view = RichTextEditorView(frame: CGRect(origin: .zero, size: Benchmark.viewport))
        let surface: EditorTableSurface
        let drawing: PreparedProseDrawingView

        init(id: UInt64? = nil) throws {
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
            view.textView.contentOffset.y = max(0, frame.midY - Benchmark.viewport.height / 2)
            let horizontal = max(0, frame.midX - Benchmark.viewport.width / 2)
            _ = drawing.scrollTables(in: [table.scrollIdentity], by: -horizontal)
            view.layoutIfNeeded()
            let contentRect = try XCTUnwrap(surface.cellFrame(tableID: table.identity, cellIndex: UInt32(cellIndex)))
            XCTAssertTrue(view.bindTableCell(tableID: table.identity, cellIndex: UInt32(cellIndex), contentRect: contentRect))
            let input = view.activeTextInput
            XCTAssertTrue(input is TableCellInputTextView)
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
    private var clock: FrameClock!

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

    func testEveryFixtureIsAdmitted() throws {
        for rich in [false, true] {
            for (rows, columns) in [(3, 3), (1_000, 20), (100, 200)] {
                try autoreleasepool {
                    let fixture = Fixture(rows: rows, columns: columns, rich: rich)
                    let host = try EditorHost()
                    defer { host.close() }
                    try host.load(fixture.source())
                    XCTAssertEqual(try host.table().cells.count, rows * columns - (rich ? 1 : 0), fixture.name)
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
        clock = FrameClock()
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
        clock = FrameClock()
        defer { clock.close(); clock = nil }
        let fixture = Fixture(rows: 3, columns: 3, rich: false)
        let source = try fixture.source()
        try cold(fixture, source: source)
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

    func testLargeBoundCellPreparationStartsAfterActivationFrame() throws {
        clock = FrameClock()
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

    private func measure(_ drawing: PreparedProseDrawingView, action: () throws -> Void) throws -> Measurement {
        let stages = StageProbe()
        PreparedProseInstrumentation.tableStageObserverForTesting = stages.record
        var commitTime: Double?
        var presented: Double?
        drawing.onMountedTableCellsDrawnForTesting = { _ in
            CATransaction.setCompletionBlock { if commitTime == nil { commitTime = CACurrentMediaTime() } }
        }
        clock.onTick = { link in
            if commitTime != nil { presented = link.targetTimestamp }
        }
        defer {
            drawing.onMountedTableCellsDrawnForTesting = nil
            clock.onTick = nil
            PreparedProseInstrumentation.tableStageObserverForTesting = nil
        }
        let start = CACurrentMediaTime()
        try action()
        let actionEnd = CACurrentMediaTime()
        drawing.setNeedsDisplay()
        CATransaction.flush()
        let deadline = start + Benchmark.frameTimeout
        while presented == nil && CACurrentMediaTime() < deadline {
            RunLoop.main.run(until: Date().addingTimeInterval(Benchmark.runLoopSlice))
        }
        let end = try XCTUnwrap(presented, "No presented frame followed the dirty table transaction")
        var measured = stages.durationsMs()
        measured["synchronousAction"] = (actionEnd - start) * Benchmark.millisecondsPerSecond
        measured["presentationWait"] = (end - actionEnd) * Benchmark.millisecondsPerSecond
        return Measurement(durationMs: (end - start) * Benchmark.millisecondsPerSecond, stagesMs: measured)
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
                editor.append(try measure(host.drawing) { try host.load(source) })
                editorCounters.observe(host.drawing, cellInputs: host.view.textInputs)
                editorCounters.authoritativeDocumentBytes = max(editorCounters.authoritativeDocumentBytes, try host.authoritativeBytes())
            }
            try autoreleasepool {
                let registry = PreparedProseLayoutRegistry()
                let window = UIWindow(frame: CGRect(origin: .zero, size: Benchmark.viewport))
                let view = ProseViewerView(frame: window.bounds, layoutRegistry: registry)
                window.addSubview(view)
                window.makeKeyAndVisible()
                defer { view.prepareForReuse(); window.isHidden = true }
                viewer.append(try measure(view.drawingViewForTesting) {
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
        let oldKeys = Set(try host.table().cells.map { $0.contentKey.semanticKey })
        var prepared: [Int] = []
        host.surface.onTableCellPreparedForTesting = { prepared.append($0) }
        defer { host.surface.onTableCellPreparedForTesting = nil }
        let duration = try measure(host.drawing, action: action)
        let next = try host.table()
        var unchanged: [Int] = []
        for index in prepared {
            let key = try XCTUnwrap(next.cell(sourceIndex: index)).contentKey.semanticKey
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
                      counters: inout PreparedProseInstrumentation.TablePerformanceCounters) throws -> (Measurement, Bool) {
        let cellIndex = try XCTUnwrap(input.tableCellPositionMap).binding.cellIndex
        let oldHeight = try XCTUnwrap(try host.table().cell(sourceIndex: Int(cellIndex))).contentSize.height
        let previousRevision = host.adapter.baseDocumentRevision
        let previousLength = input.textStorage.length
        let duration = try measureChange(host, counters: &counters) {
            input.insertText(Benchmark.text)
            host.view.layoutIfNeeded()
        }
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
        for _ in 0..<Benchmark.warmupSamples { _ = try edit(host, input: input, counters: &counters) }
        counters = .init()
        var values: [Measurement] = []
        var wraps = 0
        for _ in 0..<Benchmark.typingSamples {
            let (duration, wrapped) = try edit(host, input: input, counters: &counters)
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
        for _ in 0..<Benchmark.baselineSamples { values.append(try edit(host, input: input, counters: &counters).0) }
        counters.authoritativeDocumentBytes = try host.authoritativeBytes()
        append(fixture, metric: atEnd ? "cellChangeEnd" : "cellChangeStart", values: values, counters: counters)
    }

    private func warm(_ fixture: Fixture, source: String) throws {
        let registry = PreparedProseLayoutRegistry()
        let window = UIWindow(frame: CGRect(origin: .zero, size: Benchmark.viewport))
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

    private func scroll(_ fixture: Fixture, source: String, horizontal: Bool) throws {
        let host = try EditorHost()
        defer { host.close() }
        try host.load(source)
        _ = try measure(host.drawing) {}
        let table = try host.table()
        let horizontalRange = max(0, table.bounds.width - table.hostViewportWidth)
        let verticalRange = max(0, host.view.textView.contentSize.height - Benchmark.viewport.height)
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
        while !finished { RunLoop.main.run(until: Date().addingTimeInterval(Benchmark.runLoopSlice)) }
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
