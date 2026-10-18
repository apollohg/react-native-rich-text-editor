import ObjectiveC
import UIKit

// MARK: - PositionBridge

final class PositionBridge {
    private static let highSurrogateRange: ClosedRange<unichar> = 0xD800...0xDBFF
    private static let lowSurrogateRange: ClosedRange<unichar> = 0xDC00...0xDFFF
    private static let utf16ScanChunkLength = 4096

    private struct ScalarDeltaBreakpoint {
        let utf16Offset: Int
        let delta: Int
    }

    private struct UTF16ScalarMap {
        let utf16Length: Int
        let breakpoints: [ScalarDeltaBreakpoint]

        init(utf16Length: Int, orderedBreakpoints: [ScalarDeltaBreakpoint]) {
            var compact: [ScalarDeltaBreakpoint] = []
            for breakpoint in orderedBreakpoints where breakpoint.delta != (compact.last?.delta ?? 0) {
                compact.append(breakpoint)
            }
            self.utf16Length = utf16Length
            breakpoints = compact
        }

        var scalarLength: UInt32 { scalar(atUtf16Offset: utf16Length) }

        func delta(atUtf16Offset offset: Int) -> Int {
            var low = 0
            var high = breakpoints.count
            while low < high {
                let mid = (low + high) / 2
                if breakpoints[mid].utf16Offset <= offset {
                    low = mid + 1
                } else {
                    high = mid
                }
            }
            return low == 0 ? 0 : breakpoints[low - 1].delta
        }

        func scalar(atUtf16Offset offset: Int) -> UInt32 {
            let clamped = min(max(offset, 0), utf16Length)
            return UInt32(clamping: max(0, clamped + delta(atUtf16Offset: clamped)))
        }

        func utf16Offset(atScalar scalar: UInt32, in text: NSString) -> Int {
            guard scalar > 0 else { return 0 }
            guard scalar <= scalarLength else { return utf16Length }
            var low = 0
            var high = utf16Length
            while low < high {
                let mid = (low + high) / 2
                if self.scalar(atUtf16Offset: mid) < scalar {
                    low = mid + 1
                } else {
                    high = mid
                }
            }
            if low > 0, low < text.length,
               highSurrogateRange.contains(text.character(at: low - 1)),
               lowSurrogateRange.contains(text.character(at: low)) {
                return low + 1
            }
            return low
        }

        func replacing(_ range: NSRange, with replacement: UTF16ScalarMap) -> UTF16ScalarMap {
            let start = range.location
            let end = NSMaxRange(range)
            let startDelta = delta(atUtf16Offset: start)
            let utf16Shift = replacement.utf16Length - range.length
            let suffixDelta = startDelta + replacement.delta(atUtf16Offset: replacement.utf16Length)
                - delta(atUtf16Offset: end)
            var ordered = Array(breakpoints.prefix { $0.utf16Offset < start })
            ordered.append(ScalarDeltaBreakpoint(
                utf16Offset: start,
                delta: startDelta + replacement.delta(atUtf16Offset: 0)
            ))
            for breakpoint in replacement.breakpoints where breakpoint.utf16Offset > 0 {
                ordered.append(ScalarDeltaBreakpoint(
                    utf16Offset: start + breakpoint.utf16Offset,
                    delta: startDelta + breakpoint.delta
                ))
            }
            for breakpoint in breakpoints where breakpoint.utf16Offset > end {
                ordered.append(ScalarDeltaBreakpoint(
                    utf16Offset: breakpoint.utf16Offset + utf16Shift,
                    delta: breakpoint.delta + suffixDelta
                ))
            }
            return UTF16ScalarMap(utf16Length: utf16Length + utf16Shift, orderedBreakpoints: ordered)
        }
    }

    private final class TextViewScalarMap: NSObject {
        let map: UTF16ScalarMap

        init(_ map: UTF16ScalarMap) {
            self.map = map
        }
    }

    struct VirtualListMarker {
        let paragraphStartUtf16: Int
        let scalarLength: UInt32
    }

    private struct PositionAdjustments {
        let placeholders: [Int]
        let listMarkers: [VirtualListMarker]
    }

    private static var textViewScalarMapKey: UInt8 = 0
    private static var rootTablePositionMapKey: UInt8 = 0

    static func rootTablePositionMap(in textView: UITextView) -> RootTablePositionMap? {
        objc_getAssociatedObject(textView, &rootTablePositionMapKey) as? RootTablePositionMap
    }

    static func setRootTablePositionMap(_ map: RootTablePositionMap?, in textView: UITextView) {
        objc_setAssociatedObject(textView, &rootTablePositionMapKey, map, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
    }

    // MARK: - UTF-16 <-> Scalar Conversion

    static func textViewToScalar(_ position: UITextPosition, in textView: UITextView) -> UInt32 {
        let utf16Offset = textView.offset(from: textView.beginningOfDocument, to: position)
        return utf16OffsetToScalar(utf16Offset, in: textView)
    }

    static func scalarToTextView(_ scalar: UInt32, in textView: UITextView) -> UITextPosition {
        let utf16Offset = scalarToUtf16Offset(scalar, in: textView)
        return textView.position(
            from: textView.beginningOfDocument,
            offset: utf16Offset
        ) ?? textView.endOfDocument
    }

    static func utf16OffsetToScalar(_ utf16Offset: Int, in textView: UITextView) -> UInt32 {
        let local = textViewScalarMap(for: textView).scalar(
            atUtf16Offset: min(utf16Offset, textView.textStorage.length)
        )
        return rootTablePositionMap(in: textView)?.globalBoundary(local: local) ?? local
    }

    static func utf16OffsetToScalar(_ utf16Offset: Int, in attributedString: NSAttributedString) -> UInt32 {
        adjustedScalarMap(for: attributedString).scalar(atUtf16Offset: utf16Offset)
    }

    static func scalarToUtf16Offset(_ scalar: UInt32, in textView: UITextView) -> Int {
        textViewScalarMap(for: textView).utf16Offset(
            atScalar: rootTablePositionMap(in: textView)?.localBoundary(global: scalar) ?? scalar,
            in: textView.textStorage.mutableString
        )
    }

    static func isScalarPositionRepresentable(_ scalar: UInt32, in textView: UITextView) -> Bool {
        rootTablePositionMap(in: textView)?.isPositionRepresentable(scalar) ?? true
    }

    static func hasRootTableScalarExtents(in textView: UITextView) -> Bool {
        rootTablePositionMap(in: textView)?.hasTables == true
    }

    static func isScalarRangeRepresentable(from: UInt32, to: UInt32, in textView: UITextView) -> Bool {
        guard from <= to else { return false }
        return rootTablePositionMap(in: textView)?.isRangeRepresentable(from: from, to: to) ?? true
    }

    static func isRootTextInputRangeSafe(from: UInt32, to: UInt32, in textView: UITextView) -> Bool {
        guard from <= to else { return false }
        return rootTablePositionMap(in: textView)?.isInputRangeSafe(from: from, to: to) ?? true
    }

    static func scalarToUtf16Offset(_ scalar: UInt32, in attributedString: NSAttributedString) -> Int {
        adjustedScalarMap(for: attributedString).utf16Offset(atScalar: scalar, in: utf16Text(of: attributedString))
    }

    static func utf16OffsetToScalar(_ utf16Offset: Int, in text: String) -> UInt32 {
        scalarMap(for: text as NSString).scalar(atUtf16Offset: utf16Offset)
    }

    static func scalarToUtf16Offset(_ scalar: UInt32, in text: String) -> Int {
        let utf16Text = text as NSString
        return scalarMap(for: utf16Text).utf16Offset(atScalar: scalar, in: utf16Text)
    }

    // MARK: - Grapheme Boundary Snapping

    static func snapToGraphemeBoundary(_ utf16Offset: Int, in text: String, biasForward: Bool = true) -> Int {
        guard !text.isEmpty else { return 0 }

        let nsString = text as NSString
        let clampedOffset = min(max(utf16Offset, 0), nsString.length)

        if clampedOffset == 0 || clampedOffset == nsString.length {
            return clampedOffset
        }

        let range = nsString.rangeOfComposedCharacterSequence(at: clampedOffset)

        if range.location == clampedOffset {
            return clampedOffset
        }

        return biasForward ? NSMaxRange(range) : range.location
    }

    // MARK: - UITextRange <-> Scalar Range

    static func textRangeToScalarRange(
        _ range: UITextRange,
        in textView: UITextView
    ) -> (from: UInt32, to: UInt32) {
        let from = textViewToScalar(range.start, in: textView)
        let to = textViewToScalar(range.end, in: textView)
        return (from: min(from, to), to: max(from, to))
    }

    static func scalarRangeToTextRange(
        from: UInt32,
        to: UInt32,
        in textView: UITextView
    ) -> UITextRange? {
        let startPos = scalarToTextView(from, in: textView)
        let endPos = scalarToTextView(to, in: textView)
        return textView.textRange(from: startPos, to: endPos)
    }

    // MARK: - Cursor Scalar Offset (Convenience)

    static func cursorScalarOffset(in textView: UITextView) -> UInt32 {
        if let editorTextView = textView as? EditorTextView,
           let selection = editorTextView.currentLogicalScalarSelection() {
            return selection.head
        }
        guard let selectedRange = textView.selectedTextRange else { return 0 }
        return textViewToScalar(selectedRange.end, in: textView)
    }

    static func virtualListMarker(
        atUtf16Offset utf16Offset: Int,
        in textView: UITextView
    ) -> VirtualListMarker? {
        let storage = textView.textStorage
        guard utf16Offset >= 0, utf16Offset < storage.length else { return nil }
        let paragraphRange = (storage.string as NSString).paragraphRange(
            for: NSRange(location: utf16Offset, length: 0)
        )
        guard paragraphRange.location == utf16Offset,
              !RenderBridge.isListContinuationParagraph(utf16Offset, in: storage)
        else { return nil }

        var firstRun = NSRange(location: 0, length: 0)
        _ = storage.attributes(at: utf16Offset, effectiveRange: &firstRun)
        // Preserve the original run start when the query clips an inherited run.
        let start = firstRun.location < utf16Offset ? NSMaxRange(firstRun) : utf16Offset
        let paragraphEnd = NSMaxRange(paragraphRange)
        guard start < paragraphEnd else { return nil }

        var marker: VirtualListMarker?
        storage.enumerateAttributes(
            in: NSRange(location: start, length: paragraphEnd - start),
            options: [.longestEffectiveRangeNotRequired]
        ) { attrs, range, stop in
            guard range.length > 0,
                  let context = attrs[RenderBridgeAttributes.listMarkerContext] as? [String: Any]
            else { return }
            marker = VirtualListMarker(
                paragraphStartUtf16: utf16Offset,
                scalarLength: UInt32(RenderBridge.listMarkerString(listContext: context).unicodeScalars.count)
            )
            stop.pointee = true
        }
        return marker
    }

    static func invalidateCache(for textView: UITextView) {
        objc_setAssociatedObject(
            textView,
            &textViewScalarMapKey,
            nil,
            .OBJC_ASSOCIATION_RETAIN_NONATOMIC
        )
    }

    @discardableResult
    static func applyAttributedPatchIfPossible(
        for textView: UITextView,
        replaceRange: NSRange,
        replacement: NSAttributedString
    ) -> Bool {
        patchTextViewScalarMap(for: textView, replaceRange: replaceRange) {
            adjustedScalarMap(for: replacement)
        }
    }

    @discardableResult
    static func applyPlainTextPatchIfPossible(
        for textView: UITextView,
        replaceRange: NSRange,
        replacementText: String
    ) -> Bool {
        patchTextViewScalarMap(for: textView, replaceRange: replaceRange) {
            scalarMap(for: replacementText as NSString)
        }
    }

    private static func patchTextViewScalarMap(
        for textView: UITextView,
        replaceRange: NSRange,
        replacementMap: () -> UTF16ScalarMap
    ) -> Bool {
        guard let cached = objc_getAssociatedObject(textView, &textViewScalarMapKey) as? TextViewScalarMap,
              !hasRootTableScalarExtents(in: textView),
              replaceRange.location >= 0,
              replaceRange.length >= 0,
              NSMaxRange(replaceRange) <= cached.map.utf16Length
        else {
            return false
        }
        storeTextViewScalarMap(cached.map.replacing(replaceRange, with: replacementMap()), in: textView)
        return true
    }

    private static func storeTextViewScalarMap(_ map: UTF16ScalarMap, in textView: UITextView) {
        objc_setAssociatedObject(
            textView,
            &textViewScalarMapKey,
            TextViewScalarMap(map),
            .OBJC_ASSOCIATION_RETAIN_NONATOMIC
        )
    }

    private static func textViewScalarMap(for textView: UITextView) -> UTF16ScalarMap {
        if let cached = objc_getAssociatedObject(textView, &textViewScalarMapKey) as? TextViewScalarMap {
            return cached.map
        }
        let map = adjustedScalarMap(for: textView.textStorage)
        storeTextViewScalarMap(map, in: textView)
        return map
    }

    private static func utf16Text(of attributedString: NSAttributedString) -> NSString {
        (attributedString as? NSMutableAttributedString)?.mutableString ?? attributedString.string as NSString
    }

    private static func adjustedScalarMap(for attributedString: NSAttributedString) -> UTF16ScalarMap {
        let text = utf16Text(of: attributedString)
        let adjustments = positionAdjustments(in: attributedString, text: text)
        return scalarMap(for: text, placeholders: adjustments.placeholders, listMarkers: adjustments.listMarkers)
    }

    private static func scalarMap(
        for text: NSString,
        placeholders: [Int] = [],
        listMarkers: [VirtualListMarker] = []
    ) -> UTF16ScalarMap {
        let utf16Length = text.length
        var changes = surrogatePairEnds(in: text).map { (utf16Offset: $0, change: -1) }
        for placeholderOffset in placeholders {
            let startOffset = min(max(placeholderOffset + 1, 0), utf16Length + 1)
            if startOffset <= utf16Length {
                changes.append((utf16Offset: startOffset, change: -1))
            }
        }
        for marker in listMarkers {
            changes.append((
                utf16Offset: min(max(marker.paragraphStartUtf16, 0), utf16Length),
                change: Int(marker.scalarLength)
            ))
        }
        changes.sort { $0.utf16Offset < $1.utf16Offset }
        var ordered: [ScalarDeltaBreakpoint] = []
        var runningDelta = 0
        for change in changes {
            runningDelta += change.change
            let breakpoint = ScalarDeltaBreakpoint(utf16Offset: change.utf16Offset, delta: runningDelta)
            if ordered.last?.utf16Offset == change.utf16Offset {
                ordered[ordered.count - 1] = breakpoint
            } else {
                ordered.append(breakpoint)
            }
        }
        return UTF16ScalarMap(utf16Length: utf16Length, orderedBreakpoints: ordered)
    }

    private static func surrogatePairEnds(in text: NSString) -> [Int] {
        let utf16Length = text.length
        var ends: [Int] = []
        var previousWasHighSurrogate = false
        withUnsafeTemporaryAllocation(of: unichar.self, capacity: utf16ScanChunkLength) { buffer in
            guard let units = buffer.baseAddress else { return }
            var location = 0
            while location < utf16Length {
                let count = min(utf16ScanChunkLength, utf16Length - location)
                text.getCharacters(units, range: NSRange(location: location, length: count))
                for index in 0..<count {
                    let unit = units[index]
                    if previousWasHighSurrogate, lowSurrogateRange.contains(unit) {
                        ends.append(location + index + 1)
                        previousWasHighSurrogate = false
                    } else {
                        previousWasHighSurrogate = highSurrogateRange.contains(unit)
                    }
                }
                location += count
            }
        }
        return ends
    }

    private static func positionAdjustments(
        in attributedString: NSAttributedString,
        text nsString: NSString
    ) -> PositionAdjustments {
        guard attributedString.length > 0 else {
            return PositionAdjustments(placeholders: [], listMarkers: [])
        }

        var placeholders: [Int] = []
        var markers: [VirtualListMarker] = []
        var seenStarts = Set<Int>()
        let fullRange = NSRange(location: 0, length: attributedString.length)

        attributedString.enumerateAttributes(
            in: fullRange,
            options: [.longestEffectiveRangeNotRequired]
        ) { attrs, range, _ in
            guard range.length > 0 else { return }

            if attrs[RenderBridgeAttributes.syntheticPlaceholder] as? Bool == true {
                placeholders.append(range.location)
            }

            guard let listContext = attrs[RenderBridgeAttributes.listMarkerContext] as? [String: Any] else {
                return
            }

            let paragraphStart = nsString.paragraphRange(
                for: NSRange(location: range.location, length: 0)
            ).location
            guard !RenderBridge.isListContinuationParagraph(
                paragraphStart,
                in: attributedString
            ) else {
                return
            }
            guard seenStarts.insert(paragraphStart).inserted else { return }

            let markerLength = UInt32(
                RenderBridge.listMarkerString(listContext: listContext).unicodeScalars.count
            )
            markers.append(
                VirtualListMarker(
                    paragraphStartUtf16: paragraphStart,
                    scalarLength: markerLength
                )
            )
        }

        return PositionAdjustments(
            placeholders: placeholders,
            listMarkers: markers.sorted { $0.paragraphStartUtf16 < $1.paragraphStartUtf16 }
        )
    }

}

// MARK: - v2 position envelopes
//
// The v2 native transaction bridge addresses positions as scalar/utf16
// offsets with an optional affinity. These helpers build the data-only
// envelope shapes (`native_transaction_bridge.rs`: PositionEnvelope,
// RangeEnvelope, SelectionEnvelope) from the native view's scalar currency.
enum EditorV2PositionBridge {
    static func positionEnvelope(scalar: UInt32, affinity: String? = nil) -> [String: Any] {
        var envelope: [String: Any] = ["offset": Int(scalar), "kind": "scalar"]
        if let affinity {
            envelope["affinity"] = affinity
        }
        return envelope
    }

    static func rangeEnvelope(from: UInt32, to: UInt32) -> [String: Any] {
        [
            "from": positionEnvelope(scalar: from),
            "to": positionEnvelope(scalar: to)
        ]
    }

    static func textSelectionEnvelope(anchor: UInt32, head: UInt32, affinity: String? = nil) -> [String: Any] {
        [
            "type": "text",
            "anchor": positionEnvelope(scalar: anchor, affinity: affinity),
            "head": positionEnvelope(scalar: head, affinity: affinity)
        ]
    }

    static func scalarLength(of text: String) -> UInt32 {
        UInt32(text.unicodeScalars.count)
    }
}
