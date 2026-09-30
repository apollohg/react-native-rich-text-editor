import ObjectiveC
import UIKit

// MARK: - PositionBridge

final class PositionBridge {

    private struct StringConversionTable {
        let utf16ToScalar: [UInt32]
        let scalarToUtf16: [Int]
    }

    private final class TextViewConversionTable: NSObject {
        let adjustedUtf16ToScalar: [UInt32]

        init(
            adjustedUtf16ToScalar: [UInt32]
        ) {
            self.adjustedUtf16ToScalar = adjustedUtf16ToScalar
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

    private static var textViewConversionTableKey: UInt8 = 0
    private static var rootTablePositionMapKey: UInt8 = 0

    static func rootTablePositionMap(in textView: UITextView) -> RootTablePositionMap? {
        objc_getAssociatedObject(textView, &rootTablePositionMapKey) as? RootTablePositionMap
    }

    static func setRootTablePositionMap(_ map: RootTablePositionMap?, in textView: UITextView) {
        objc_setAssociatedObject(textView, &rootTablePositionMapKey, map, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
    }

    private static let stringTableLock = NSLock()
    private static var lastStringTableText = ""
    private static var lastStringTable: StringConversionTable?

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
        let text = textView.text ?? ""
        let conversionTable = textViewConversionTable(for: textView)
        guard !conversionTable.adjustedUtf16ToScalar.isEmpty else { return 0 }
        let clampedOffset = min(
            max(utf16Offset, 0),
            min((text as NSString).length, conversionTable.adjustedUtf16ToScalar.count - 1)
        )
        let local = conversionTable.adjustedUtf16ToScalar[clampedOffset]
        return rootTablePositionMap(in: textView)?.globalBoundary(local: local) ?? local
    }

    static func utf16OffsetToScalar(_ utf16Offset: Int, in attributedString: NSAttributedString) -> UInt32 {
        let conversionTable = adjustedConversionTable(for: attributedString)
        guard !conversionTable.isEmpty else { return 0 }
        let clampedOffset = min(max(utf16Offset, 0), conversionTable.count - 1)
        return conversionTable[clampedOffset]
    }

    static func scalarToUtf16Offset(_ scalar: UInt32, in textView: UITextView) -> Int {
        let conversionTable = textViewConversionTable(for: textView)
        let utf16ToScalar = conversionTable.adjustedUtf16ToScalar
        return scalarToUtf16Offset(rootTablePositionMap(in: textView)?.localBoundary(global: scalar) ?? scalar,
                                   inAdjustedUtf16ToScalarTable: utf16ToScalar)
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
        let utf16ToScalar = adjustedConversionTable(for: attributedString)
        return scalarToUtf16Offset(scalar, inAdjustedUtf16ToScalarTable: utf16ToScalar)
    }

    private static func scalarToUtf16Offset(
        _ scalar: UInt32,
        inAdjustedUtf16ToScalarTable utf16ToScalar: [UInt32]
    ) -> Int {
        guard scalar > 0, !utf16ToScalar.isEmpty else {
            return 0
        }

        if let last = utf16ToScalar.last, scalar > last {
            return utf16ToScalar.count - 1
        }

        var low = 0
        var high = utf16ToScalar.count - 1
        while low < high {
            let mid = (low + high) / 2
            if utf16ToScalar[mid] < scalar {
                low = mid + 1
            } else {
                high = mid
            }
        }

        return low
    }

    static func utf16OffsetToScalar(_ utf16Offset: Int, in text: String) -> UInt32 {
        let conversionTable = stringConversionTable(for: text)
        let clampedOffset = min(max(utf16Offset, 0), conversionTable.utf16ToScalar.count - 1)
        return conversionTable.utf16ToScalar[clampedOffset]
    }

    static func scalarToUtf16Offset(_ scalar: UInt32, in text: String) -> Int {
        let conversionTable = stringConversionTable(for: text)
        guard scalar > 0 else { return 0 }
        let scalarIndex = min(Int(scalar), conversionTable.scalarToUtf16.count - 1)
        return conversionTable.scalarToUtf16[scalarIndex]
    }

    // MARK: - Grapheme Boundary Snapping

    static func snapToGraphemeBoundary(_ utf16Offset: Int, in text: String) -> Int {
        guard !text.isEmpty else { return 0 }

        let nsString = text as NSString
        let clampedOffset = min(max(utf16Offset, 0), nsString.length)

        if clampedOffset == 0 || clampedOffset == nsString.length {
            return clampedOffset
        }

        // composedCharacterSequence(at:) returns the full grapheme cluster range
        // containing the given UTF-16 index. We snap to the end of that range
        // (forward bias) since that's what a user moving the cursor expects.
        let range = nsString.rangeOfComposedCharacterSequence(at: clampedOffset)

        if range.location == clampedOffset {
            return clampedOffset
        }

        return NSMaxRange(range)
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
        virtualListMarkers(in: textView.textStorage).first { $0.paragraphStartUtf16 == utf16Offset }
    }

    static func invalidateCache(for textView: UITextView) {
        objc_setAssociatedObject(
            textView,
            &textViewConversionTableKey,
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
        guard let cached = objc_getAssociatedObject(textView, &textViewConversionTableKey) as? TextViewConversionTable else {
            return false
        }
        guard !hasRootTableScalarExtents(in: textView) else { return false }

        let oldAdjusted = cached.adjustedUtf16ToScalar
        let oldUtf16Count = max(0, oldAdjusted.count - 1)
        guard replaceRange.location >= 0,
              replaceRange.length >= 0,
              replaceRange.location + replaceRange.length <= oldUtf16Count
        else {
            return false
        }

        let startOffset = replaceRange.location
        let endOffset = replaceRange.location + replaceRange.length
        let replacementAdjusted = adjustedConversionTable(for: replacement)
        let patched = patchedAdjustedConversionTable(
            oldAdjusted: oldAdjusted,
            startOffset: startOffset,
            endOffset: endOffset,
            replacementAdjusted: replacementAdjusted
        )

        objc_setAssociatedObject(
            textView,
            &textViewConversionTableKey,
            TextViewConversionTable(adjustedUtf16ToScalar: patched),
            .OBJC_ASSOCIATION_RETAIN_NONATOMIC
        )
        return true
    }

    @discardableResult
    static func applyPlainTextPatchIfPossible(
        for textView: UITextView,
        replaceRange: NSRange,
        replacementText: String
    ) -> Bool {
        guard let cached = objc_getAssociatedObject(textView, &textViewConversionTableKey) as? TextViewConversionTable else {
            return false
        }
        guard !hasRootTableScalarExtents(in: textView) else { return false }

        let oldAdjusted = cached.adjustedUtf16ToScalar
        let oldUtf16Count = max(0, oldAdjusted.count - 1)
        guard replaceRange.location >= 0,
              replaceRange.length >= 0,
              replaceRange.location + replaceRange.length <= oldUtf16Count
        else {
            return false
        }

        let startOffset = replaceRange.location
        let endOffset = replaceRange.location + replaceRange.length
        let replacementBase = stringConversionTable(for: replacementText).utf16ToScalar
        let patched = patchedAdjustedConversionTable(
            oldAdjusted: oldAdjusted,
            startOffset: startOffset,
            endOffset: endOffset,
            replacementAdjusted: replacementBase
        )

        objc_setAssociatedObject(
            textView,
            &textViewConversionTableKey,
            TextViewConversionTable(adjustedUtf16ToScalar: patched),
            .OBJC_ASSOCIATION_RETAIN_NONATOMIC
        )
        return true
    }

    private static func patchedAdjustedConversionTable(
        oldAdjusted: [UInt32],
        startOffset: Int,
        endOffset: Int,
        replacementAdjusted: [UInt32]
    ) -> [UInt32] {
        let startScalar = Int32(oldAdjusted[startOffset])
        let deletedScalarCount = Int32(oldAdjusted[endOffset]) - startScalar
        let replacementScalarCount = Int32(replacementAdjusted.last ?? 0)
        let scalarDelta = replacementScalarCount - deletedScalarCount
        let replacement = replacementAdjusted.map { value in
            UInt32(max(0, Int32(value) + startScalar))
        }
        let prefix = Array(oldAdjusted[..<startOffset])
        let suffix = oldAdjusted[(endOffset + 1)...].map { value in
            UInt32(max(0, Int32(value) + scalarDelta))
        }
        return prefix + replacement + suffix
    }

    private static func stringConversionTable(for text: String) -> StringConversionTable {
        stringTableLock.lock()
        if lastStringTableText == text, let lastStringTable {
            stringTableLock.unlock()
            return lastStringTable
        }
        stringTableLock.unlock()

        let utf16Count = text.utf16.count
        let scalarCount = text.unicodeScalars.count
        var utf16ToScalar = Array(repeating: UInt32(0), count: utf16Count + 1)
        var scalarToUtf16 = Array(repeating: 0, count: scalarCount + 1)
        var utf16Pos = 0
        var scalarPos = 0

        for scalar in text.unicodeScalars {
            let nextUtf16Pos = utf16Pos + scalar.utf16.count
            scalarPos += 1
            if nextUtf16Pos > utf16Pos {
                for offset in (utf16Pos + 1)...nextUtf16Pos {
                    utf16ToScalar[offset] = UInt32(scalarPos)
                }
            }
            scalarToUtf16[scalarPos] = nextUtf16Pos
            utf16Pos = nextUtf16Pos
        }

        let conversionTable = StringConversionTable(
            utf16ToScalar: utf16ToScalar,
            scalarToUtf16: scalarToUtf16
        )

        stringTableLock.lock()
        lastStringTableText = text
        lastStringTable = conversionTable
        stringTableLock.unlock()

        return conversionTable
    }

    private static func adjustedConversionTable(for attributedString: NSAttributedString) -> [UInt32] {
        let baseTable = stringConversionTable(for: attributedString.string)
        let adjustments = positionAdjustments(in: attributedString)
        return adjustedUtf16ToScalar(
            baseUtf16ToScalar: baseTable.utf16ToScalar,
            placeholders: adjustments.placeholders,
            listMarkers: adjustments.listMarkers
        )
    }

    private static func textViewConversionTable(for textView: UITextView) -> TextViewConversionTable {
        if let cached = objc_getAssociatedObject(textView, &textViewConversionTableKey) as? TextViewConversionTable {
            return cached
        }

        let text = textView.text ?? ""
        let baseTable = stringConversionTable(for: text)
        let adjustments = positionAdjustments(in: textView.textStorage)
        let adjustedUtf16ToScalar = adjustedUtf16ToScalar(
            baseUtf16ToScalar: baseTable.utf16ToScalar,
            placeholders: adjustments.placeholders,
            listMarkers: adjustments.listMarkers
        )
        let conversionTable = TextViewConversionTable(
            adjustedUtf16ToScalar: adjustedUtf16ToScalar
        )
        objc_setAssociatedObject(
            textView,
            &textViewConversionTableKey,
            conversionTable,
            .OBJC_ASSOCIATION_RETAIN_NONATOMIC
        )
        return conversionTable
    }

    private static func adjustedUtf16ToScalar(
        baseUtf16ToScalar: [UInt32],
        placeholders: [Int],
        listMarkers: [VirtualListMarker] = [],
    ) -> [UInt32] {
        let utf16Count = max(0, baseUtf16ToScalar.count - 1)
        var deltas = Array(repeating: Int64(0), count: utf16Count + 2)

        for placeholderOffset in placeholders {
            let startOffset = min(max(placeholderOffset + 1, 0), utf16Count + 1)
            if startOffset <= utf16Count {
                deltas[startOffset] -= 1
            }
        }

        for marker in listMarkers {
            let startOffset = min(max(marker.paragraphStartUtf16, 0), utf16Count)
            deltas[startOffset] += Int64(marker.scalarLength)
        }

        var adjustedUtf16ToScalar = Array(repeating: UInt32(0), count: utf16Count + 1)
        var runningDelta: Int64 = 0
        for offset in 0...utf16Count {
            runningDelta += deltas[offset]
            let adjustedValue = Int64(baseUtf16ToScalar[offset]) + runningDelta
            adjustedUtf16ToScalar[offset] = UInt32(max(0, min(adjustedValue, Int64(UInt32.max))))
        }
        return adjustedUtf16ToScalar
    }

    private static func adjustedUtf16ToScalar(
        baseUtf16ToScalar: [UInt32],
        listMarkers: [VirtualListMarker]
    ) -> [UInt32] {
        adjustedUtf16ToScalar(baseUtf16ToScalar: baseUtf16ToScalar, placeholders: [], listMarkers: listMarkers)
    }

    private static func virtualListMarkers(in attributedString: NSAttributedString) -> [VirtualListMarker] {
        positionAdjustments(in: attributedString).listMarkers
    }

    private static func positionAdjustments(in attributedString: NSAttributedString) -> PositionAdjustments {
        guard attributedString.length > 0 else {
            return PositionAdjustments(placeholders: [], listMarkers: [])
        }

        let nsString = attributedString.string as NSString
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

    private static func virtualListMarkers(in textStorage: NSTextStorage) -> [VirtualListMarker] {
        virtualListMarkers(in: textStorage as NSAttributedString)
    }

    private static func syntheticPlaceholderOffsets(in attributedString: NSAttributedString) -> [Int] {
        positionAdjustments(in: attributedString).placeholders
    }

    private static func syntheticPlaceholderOffsets(in textStorage: NSTextStorage) -> [Int] {
        syntheticPlaceholderOffsets(in: textStorage as NSAttributedString)
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
