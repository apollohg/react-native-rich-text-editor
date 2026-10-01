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

    func testScopedPlainTextTemplatesPreserveAttributesShapingAndPixels() {
        let engine = CoreTextProseLayoutEngine()
        let theme = PreparedProseTheme.resolve(themeJSON: nil)
        let texts = ["", " ", "  ", "trailing  ", "line\n", "one\ntwo", "a\r\nb", "e\u{301}",
            "👩🏽‍💻 family 👨‍👩‍👧", "العربية 123", "אבג Latin 42", "日本語", "office fi ffi", String(repeating: "wrap ", count: 40)]
        func make(_ inlines: [ViewerInline], _ paint: PreparedTextPaint, _ spacing: CGFloat,
                  _ template: CoreTextProseLayoutEngine.PlainTextPreparation? = nil) -> PreparedAttributedBlock {
            engine.makeAttributedString(inlines, paint: paint, theme: theme,
                warningSemanticGeneration: Benchmark.generation, paragraphSpacing: spacing,
                textPreparation: template)
        }
        func raster(_ line: CTLine) -> Data? {
            let format = UIGraphicsImageRendererFormat()
            format.scale = 1
            let size = CGSize(width: 224, height: 96)
            return UIGraphicsImageRenderer(size: size, format: format).image { image in
                image.cgContext.translateBy(x: 0, y: size.height)
                image.cgContext.scaleBy(x: 1, y: -1)
                image.cgContext.textPosition = CGPoint(x: 8, y: 40)
                CTLineDraw(line, image.cgContext)
            }.pngData()
        }
        func compare(_ expected: PreparedAttributedBlock, _ actual: PreparedAttributedBlock, _ context: String) {
            XCTAssertEqual(expected.string, actual.string, context)
            XCTAssertEqual(expected.retainedBytes, actual.retainedBytes, context)
            XCTAssertEqual(expected.atoms.count, actual.atoms.count, context)
            XCTAssertEqual(expected.semanticRanges.map(\.range), actual.semanticRanges.map(\.range), context)
            XCTAssertEqual(expected.accessibilityRanges.map(\.range), actual.accessibilityRanges.map(\.range), context)
            XCTAssertEqual(expected.accessibilityRanges.map(\.label), actual.accessibilityRanges.map(\.label), context)
            XCTAssertEqual(expected.accessibilityRanges.map(\.role), actual.accessibilityRanges.map(\.role), context)
            expected.string.enumerateAttributes(in: NSRange(location: 0, length: expected.string.length)) { attributes, range, _ in
                var actualRange = NSRange()
                let actualAttributes = actual.string.attributes(at: range.location, effectiveRange: &actualRange)
                XCTAssertEqual(NSDictionary(dictionary: attributes), NSDictionary(dictionary: actualAttributes), context)
                XCTAssertEqual(range, actualRange, context)
            }
            for width: CGFloat in [31, 62, 186] {
                let before = CTTypesetterCreateWithAttributedString(expected.string)
                let after = CTTypesetterCreateWithAttributedString(actual.string)
                var start = 0
                while start < expected.string.length {
                    let count = max(1, CTTypesetterSuggestLineBreak(before, start, Double(width)))
                    XCTAssertEqual(count, max(1, CTTypesetterSuggestLineBreak(after, start, Double(width))), context)
                    let range = CFRange(location: start, length: count)
                    let left = CTTypesetterCreateLine(before, range)
                    let right = CTTypesetterCreateLine(after, range)
                    var leftAscent: CGFloat = 0, leftDescent: CGFloat = 0, leftLeading: CGFloat = 0
                    var rightAscent: CGFloat = 0, rightDescent: CGFloat = 0, rightLeading: CGFloat = 0
                    XCTAssertEqual(CTLineGetTypographicBounds(left, &leftAscent, &leftDescent, &leftLeading),
                        CTLineGetTypographicBounds(right, &rightAscent, &rightDescent, &rightLeading), context)
                    XCTAssertEqual(leftAscent, rightAscent, context)
                    XCTAssertEqual(leftDescent, rightDescent, context)
                    XCTAssertEqual(leftLeading, rightLeading, context)
                    for offset in start...(start + count) {
                        var leftSecondary: CGFloat = 0, rightSecondary: CGFloat = 0
                        XCTAssertEqual(CTLineGetOffsetForStringIndex(left, offset, &leftSecondary),
                            CTLineGetOffsetForStringIndex(right, offset, &rightSecondary), context)
                        XCTAssertEqual(leftSecondary, rightSecondary, context)
                    }
                    let expectedPixels = raster(left), actualPixels = raster(right)
                    XCTAssertNotNil(expectedPixels, context)
                    XCTAssertNotNil(actualPixels, context)
                    XCTAssertEqual(expectedPixels, actualPixels, context)
                    start += count
                }
            }
        }
        let sharedPreparation = CoreTextProseLayoutEngine.PlainTextPreparation()
        for appearance in [UIUserInterfaceStyle.light, .dark] {
            UITraitCollection(userInterfaceStyle: appearance).performAsCurrent {
                for font in [theme.paragraph.font, UIFont.boldSystemFont(ofSize: theme.paragraph.font.pointSize)] {
                    let paint = PreparedTextPaint(font: font, color: .label, lineHeight: nil, spacingAfter: 0)
                    for spacing: CGFloat in [0, 7.25] {
                        let template = sharedPreparation
                        var retained: [(NSAttributedString, NSAttributedString)] = []
                        for text in texts {
                            let inlines: [ViewerInline] = [.text(text: text, marks: [])]
                            let expected = make(inlines, paint, spacing)
                            let actual = make(inlines, paint, spacing, template)
                            compare(expected, actual, "appearance=\(appearance.rawValue) font=\(font.fontName) spacing=\(spacing) text=\(text)")
                            retained.append((actual.string, NSAttributedString(attributedString: actual.string)))
                        }
                        for (actual, snapshot) in retained { XCTAssertEqual(actual, snapshot) }
                        let fallbacks: [[ViewerInline]] = [[], [.text(text: "", marks: [])],
                            [.text(text: "first", marks: []), .text(text: "second", marks: [])],
                            [.text(text: "bold", marks: [.init(markType: "bold", attrsJson: "{}")])],
                            [.atom(nodeType: "hard_break", docPos: 0, attrsJSON: "{}", label: "\n")]]
                        for inlines in fallbacks { compare(make(inlines, paint, spacing), make(inlines, paint, spacing, template), "fallback") }
                        var styled = paint
                        styled.textValues = ["letterSpacing": 2]
                        compare(make([.text(text: "styled", marks: [])], styled, spacing),
                            make([.text(text: "styled", marks: [])], styled, spacing, template), "textValues fallback")
                    }
                }
            }
        }
        let styleTheme = PreparedProseTheme.resolve(themeJSON: ##"{"version":1,"styles":{"paragraph":{"color":"#123456","letterSpacing":2}}}"##)
        XCTAssertNotNil(styleTheme.styleSheet)
        let inlines: [ViewerInline] = [.text(text: "styled fallback", marks: [])]
        let baseline = engine.makeAttributedString(inlines, paint: styleTheme.paragraph, theme: styleTheme,
            warningSemanticGeneration: Benchmark.generation, paragraphSpacing: 0)
        let styled = engine.makeAttributedString(inlines, paint: styleTheme.paragraph, theme: styleTheme,
            warningSemanticGeneration: Benchmark.generation, paragraphSpacing: 0, textPreparation: sharedPreparation)
        compare(baseline, styled, "stylesheet fallback")
        let colors = [UIColor { _ in .red }, UIColor { _ in .red }, UIColor { traits in
            traits.userInterfaceStyle == .dark ? .yellow : .blue
        }]
        for appearance in [UIUserInterfaceStyle.light, .dark, .light] {
            UITraitCollection(userInterfaceStyle: appearance).performAsCurrent {
                for color in colors {
                    let paint = PreparedTextPaint(font: theme.paragraph.font, color: color, lineHeight: nil, spacingAfter: 0)
                    compare(make(inlines, paint, 0), make(inlines, paint, 0, sharedPreparation), "dynamic color identity")
                }
            }
        }
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
