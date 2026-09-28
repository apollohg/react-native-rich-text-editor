import CoreText
import UIKit
import XCTest

final class TableCellPreparationBenchmarkTests: XCTestCase {
    private enum Benchmark {
        static let rows = 1_000
        static let columns = 20
        static let cellCount = rows * columns
        static let warmupRuns = 1
        static let measuredRuns = 5
        static let changedCellEdits = 200
        static let microsecondsPerSecond = 1_000_000.0
        static let millisecondsPerSecond = 1_000.0
        static let generation = "table-cell-preparation-benchmark"
    }

    func testPrepareDiscardMeasureAndWarmChangedCell() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["PREPARED_PROSE_DEVICE_BENCHMARK"] == "1",
                          "Run through the prepared prose performance scheme.")
        let style = TableStyle()
        let width = style.minColumnWidth - 2 * (style.cellPadding + style.borderWidth)
        let scale = UIScreen.main.scale
        var theme = PreparedProseTheme.resolve(themeJSON: nil)
        theme.contentInsets = .zero
        let texts = (0..<Benchmark.cellCount).map {
            String(format: "R%04dC%04dXY", $0 / Benchmark.columns, $0 % Benchmark.columns)
        }
        func document(_ text: String) -> ViewerDocument {
            ViewerDocument(semanticKey: text, paragraphs: [.init(text: text)], isEmpty: false,
                           retainedBytes: 0).withPreparedTheme(theme)
        }
        let documents = texts.map(document)
        let changed = (0..<Benchmark.changedCellEdits).map {
            document(texts[Benchmark.cellCount / 2] + String(repeating: "x", count: $0 + 1))
        }
        func key(_ document: ViewerDocument) -> ProseLayoutKey {
            ProseLayoutKey(semanticKey: document.semanticKey, widthPixels: Int(width * scale),
                themeDigest: Benchmark.generation, nativeFontRevision: 0, fontEnvironmentRevision: 0,
                displayScale: scale, attachmentRevision: 0, generationIdentity: Benchmark.generation,
                semanticGenerationIdentity: Benchmark.generation)
        }
        let keys = documents.map(key)
        let changedKeys = changed.map(key)
        let engine = CoreTextProseLayoutEngine()
        var prepareSamples: [Double] = []
        var measureSamples: [Double] = []
        var changedSamples: [Double] = []
        var changedHeightChecksum: CGFloat = 0
        for run in 0..<(Benchmark.warmupRuns + Benchmark.measuredRuns) {
            var preparedHeights = [CGFloat](repeating: 0, count: Benchmark.cellCount)
            var measuredHeights = preparedHeights
            let prepareStart = CACurrentMediaTime()
            for index in documents.indices {
                preparedHeights[index] = try autoreleasepool {
                    try engine.prepare(document: documents[index], key: keys[index], widthPoints: width,
                                       displayScale: scale, cellMode: true).size.height
                }
            }
            let prepareDuration = CACurrentMediaTime() - prepareStart
            let measureStart = CACurrentMediaTime()
            for index in texts.indices {
                measuredHeights[index] = autoreleasepool {
                    measurePlainText(texts[index], font: theme.paragraph.font, width: width,
                                     scale: scale, collectLineEnds: false).height
                }
            }
            let measureDuration = CACurrentMediaTime() - measureStart
            for index in texts.indices where preparedHeights[index] != measuredHeights[index] {
                XCTFail("run \(run) cell \(texts[index]) width \(width): prepared \(preparedHeights[index]), measured \(measuredHeights[index])")
                return
            }
            var edits: [Double] = []
            for index in changed.indices {
                let start = CACurrentMediaTime()
                changedHeightChecksum += try autoreleasepool {
                    try engine.prepare(document: changed[index], key: changedKeys[index], widthPoints: width,
                                       displayScale: scale, cellMode: true).size.height
                }
                edits.append((CACurrentMediaTime() - start) * Benchmark.millisecondsPerSecond)
            }
            if run >= Benchmark.warmupRuns {
                prepareSamples.append(prepareDuration * Benchmark.microsecondsPerSecond / Double(Benchmark.cellCount))
                measureSamples.append(measureDuration * Benchmark.microsecondsPerSecond / Double(Benchmark.cellCount))
                changedSamples.append(median(edits))
            }
        }
        print("TABLE_CELL_BENCHMARK platform=ios scale=\(scale) width=\(width) cells=\(Benchmark.cellCount) prepareUs=\(median(prepareSamples)) measureUs=\(median(measureSamples)) changedMs=\(median(changedSamples)) prepareRuns=\(prepareSamples) measureRuns=\(measureSamples) changedRuns=\(changedSamples) changedHeightChecksum=\(changedHeightChecksum)")
    }

    func testExactMeasurementPreservesPlainCellHeightAndLineBreaks() throws {
        let texts = ["", "a", " ", "  ", "word ", " word", "word  word", "a-b/c.d, e! f?",
                     "R0001C0001XY", String(repeating: "x", count: 200), "short", "last  "]
        let engine = CoreTextProseLayoutEngine()
        for fontScale: CGFloat in [1, 1.3, 2] {
            var theme = PreparedProseTheme.resolve(themeJSON: nil, fontScale: fontScale)
            theme.contentInsets = .zero
            for scale: CGFloat in [1, 2, 3] {
                for width: CGFloat in [31, 62, 186] {
                    for text in texts {
                        let document = ViewerDocument(semanticKey: text, paragraphs: [.init(text: text)],
                            isEmpty: false, retainedBytes: 0).withPreparedTheme(theme)
                        let key = ProseLayoutKey(semanticKey: text, widthPixels: Int(width * scale),
                            themeDigest: Benchmark.generation, nativeFontRevision: 0, fontEnvironmentRevision: 0,
                            displayScale: scale, attachmentRevision: 0, generationIdentity: Benchmark.generation,
                            semanticGenerationIdentity: Benchmark.generation)
                        let prepared = try engine.prepare(document: document, key: key, widthPoints: width,
                                                          displayScale: scale, cellMode: true)
                        let measured = measurePlainText(text, font: theme.paragraph.font, width: width, scale: scale)
                        let context = "text=<\(text)> fontScale=\(fontScale) scale=\(scale) width=\(width)"
                        XCTAssertEqual(prepared.size.height, measured.height, context)
                        if !text.isEmpty {
                            let ends = prepared.blocks.flatMap(\.fragments).compactMap { fragment -> Int? in
                                guard fragment.kind == .text, let line = fragment.line else { return nil }
                                let range = CTLineGetStringRange(line)
                                return range.location + range.length
                            }
                            XCTAssertEqual(ends, measured.lineEnds, context)
                        }
                    }
                }
            }
        }
    }

    private func measurePlainText(_ text: String, font: UIFont, width: CGFloat, scale: CGFloat,
                                  collectLineEnds: Bool = true) -> (height: CGFloat, lineEnds: [Int]) {
        if text.isEmpty { return (ceil(font.lineHeight * scale) / scale, []) }
        let attributed = NSAttributedString(string: text, attributes: [
            kCTFontAttributeName as NSAttributedString.Key: CoreTextProseLayoutEngine.coreTextFont(from: font)
        ])
        let framesetter = CTFramesetterCreateWithAttributedString(attributed)
        let size = CTFramesetterSuggestFrameSizeWithConstraints(framesetter, CFRange(location: 0, length: 0),
            nil, CGSize(width: width, height: .greatestFiniteMagnitude), nil)
        let path = CGPath(rect: CGRect(x: 0, y: 0, width: width, height: size.height), transform: nil)
        let frame = CTFramesetterCreateFrame(framesetter, CFRange(location: 0, length: 0), path, nil)
        let lines = CTFrameGetLines(frame) as! [CTLine]
        let height = lines.reduce(CGFloat.zero) { height, line in
            var ascent: CGFloat = 0
            var descent: CGFloat = 0
            var leading: CGFloat = 0
            CTLineGetTypographicBounds(line, &ascent, &descent, &leading)
            return height + ascent + descent + leading
        }
        let ends = collectLineEnds ? lines.map { line -> Int in
            let range = CTLineGetStringRange(line)
            return range.location + range.length
        } : []
        return (ceil(height * scale) / scale, ends)
    }

    private func median(_ samples: [Double]) -> Double {
        let sorted = samples.sorted()
        let middle = sorted.count / 2
        return sorted.count.isMultiple(of: 2) ? (sorted[middle - 1] + sorted[middle]) / 2 : sorted[middle]
    }
}
