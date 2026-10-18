import os
import UIKit

extension EditorTextView {
    private static let textKitOriginalFontAttribute = NSAttributedString.Key("NSOriginalFont")

    func shouldUseSmallPatchTextMutation(
        for attributedString: NSAttributedString,
        replaceRange: NSRange?,
        allowsEmptyNativeTextReplacement: Bool = false
    ) -> Bool {
        (attributedString.length > 0
            || (allowsEmptyNativeTextReplacement && (replaceRange?.length ?? 0) > 0))
            && attributedString.length <= 512
            && (replaceRange?.length ?? 0) <= 512
            && !attributedStringContainsAttachment(attributedString)
    }

    private func attributesEqualForPatchTrimming(
        _ lhs: [NSAttributedString.Key: Any],
        _ rhs: [NSAttributedString.Key: Any],
        allowingExistingFontFallback: Bool = false
    ) -> Bool {
        if NSDictionary(dictionary: lhs).isEqual(to: rhs) { return true }
        // Only unchanged prefix/suffix characters may retain UIKit's optional fallback font.
        guard allowingExistingFontFallback,
              let originalFont = lhs[Self.textKitOriginalFontAttribute] as? UIFont,
              let nativeFont = lhs[.font] as? UIFont,
              let renderedFont = rhs[.font] as? UIFont,
              !nativeFont.isEqual(originalFont), originalFont.isEqual(renderedFont),
              rhs[Self.textKitOriginalFontAttribute] == nil else { return false }
        var originalAttributes = lhs
        originalAttributes[.font] = originalFont
        originalAttributes.removeValue(forKey: Self.textKitOriginalFontAttribute)
        // Every style and structural key, including docPos/child index, still matches exactly.
        return NSDictionary(dictionary: originalAttributes).isEqual(to: rhs)
    }

    func applyNativeTextReplacementIfPossible(
        replacing replaceRange: NSRange,
        with attributedString: NSAttributedString
    ) -> Bool {
        guard isApplyingRustState, isFirstResponder, isEditable, isInterceptingInput,
              !isComposing, markedTextRange == nil, externalTextComposition == nil,
              tableCellPositionMap == nil,
              !textStorageRangeContainsAttachment(replaceRange),
              !textStorageRangeContainsPositionAdjustments(replaceRange),
              !attributedStringContainsPositionAdjustments(attributedString),
              let change = plainTextReplacement(
                  from: (textStorage.string as NSString).substring(with: replaceRange),
                  to: attributedString.string
              )
        else { return false }

        let nativeRange = NSRange(
            location: replaceRange.location + change.range.location,
            length: change.range.length
        )
        let replacesSelection = selectedRange == nativeRange
        let deletesBeforeCaret = change.text.isEmpty && selectedRange.length == 0
            && selectedRange.location == NSMaxRange(nativeRange)
        guard replacesSelection || deletesBeforeCaret,
              let start = position(from: beginningOfDocument, offset: nativeRange.location),
              let end = position(from: start, offset: nativeRange.length),
              let range = textRange(from: start, to: end)
        else { return false }

        // UIKit advances the accepted caret without a second keyboard selection notification.
        replace(range, withText: change.text)
        return true
    }

    func applyAttributes(from attributedString: NSAttributedString, to destinationRange: NSRange) {
        guard attributedString.length == destinationRange.length else { return }
        var changedRuns: [(attributes: [NSAttributedString.Key: Any], range: NSRange)] = []
        let sourceRange = NSRange(location: 0, length: attributedString.length)
        attributedString.enumerateAttributes(
            in: sourceRange,
            options: [.longestEffectiveRangeNotRequired]
        ) { attrs, range, _ in
            let targetRange = NSRange(location: destinationRange.location + range.location, length: range.length)
            var currentRange = NSRange()
            let current = textStorage.attributes(at: targetRange.location, effectiveRange: &currentRange)
            if NSMaxRange(currentRange) < NSMaxRange(targetRange)
                || !attributesEqualForPatchTrimming(current, attrs) {
                changedRuns.append((attrs, targetRange))
            }
        }
        for run in changedRuns {
            textStorage.setAttributes(run.attributes, range: run.range)
        }
    }

    func attributedStringContainsAttachment(_ attributedString: NSAttributedString) -> Bool {
        guard attributedString.length > 0 else { return false }
        var hasAttachment = false
        attributedString.enumerateAttribute(
            .attachment,
            in: NSRange(location: 0, length: attributedString.length),
            options: [.longestEffectiveRangeNotRequired]
        ) { value, _, stop in
            if value != nil {
                hasAttachment = true
                stop.pointee = true
            }
        }
        return hasAttachment
    }

    func attributedStringContainsPositionAdjustments(_ attributedString: NSAttributedString) -> Bool {
        guard attributedString.length > 0 else { return false }
        var hasAdjustments = false
        attributedString.enumerateAttributes(
            in: NSRange(location: 0, length: attributedString.length),
            options: [.longestEffectiveRangeNotRequired]
        ) { attrs, _, stop in
            if attrs[RenderBridgeAttributes.syntheticPlaceholder] as? Bool == true
                || attrs[RenderBridgeAttributes.listMarkerContext] != nil {
                hasAdjustments = true
                stop.pointee = true
            }
        }
        return hasAdjustments
    }

    func attributedStringContainsListMarkerContext(_ attributedString: NSAttributedString) -> Bool {
        guard attributedString.length > 0 else { return false }
        var hasListMarkerContext = false
        attributedString.enumerateAttribute(
            RenderBridgeAttributes.listMarkerContext,
            in: NSRange(location: 0, length: attributedString.length),
            options: [.longestEffectiveRangeNotRequired]
        ) { value, _, stop in
            if value != nil {
                hasListMarkerContext = true
                stop.pointee = true
            }
        }
        return hasListMarkerContext
    }

    func textStorageRangeContainsPositionAdjustments(_ range: NSRange) -> Bool {
        guard range.length > 0,
              range.location >= 0,
              range.location + range.length <= textStorage.length
        else {
            return false
        }

        var hasAdjustments = false
        textStorage.enumerateAttributes(
            in: range,
            options: [.longestEffectiveRangeNotRequired]
        ) { attrs, _, stop in
            if attrs[RenderBridgeAttributes.syntheticPlaceholder] as? Bool == true
                || attrs[RenderBridgeAttributes.listMarkerContext] != nil {
                hasAdjustments = true
                stop.pointee = true
            }
        }
        return hasAdjustments
    }

    func textStorageRangeContainsListMarkerContext(_ range: NSRange) -> Bool {
        guard range.length > 0,
              range.location >= 0,
              range.location + range.length <= textStorage.length
        else {
            return false
        }

        var hasListMarkerContext = false
        textStorage.enumerateAttribute(
            RenderBridgeAttributes.listMarkerContext,
            in: range,
            options: [.longestEffectiveRangeNotRequired]
        ) { value, _, stop in
            if value != nil {
                hasListMarkerContext = true
                stop.pointee = true
            }
        }
        return hasListMarkerContext
    }

    func textStorageRangeContainsAttachment(_ range: NSRange) -> Bool {
        guard range.length > 0,
              range.location >= 0,
              range.location + range.length <= textStorage.length
        else {
            return false
        }

        var hasAttachment = false
        textStorage.enumerateAttribute(
            .attachment,
            in: range,
            options: [.longestEffectiveRangeNotRequired]
        ) { value, _, stop in
            if value != nil {
                hasAttachment = true
                stop.pointee = true
            }
        }
        return hasAttachment
    }

    func topLevelChildrenContainAttachment(
        startIndex: Int,
        deleteCount: Int
    ) -> Bool {
        guard deleteCount > 0,
              let currentTopLevelChildMetadata,
              startIndex >= 0,
              startIndex + deleteCount <= currentTopLevelChildMetadata.count
        else {
            return false
        }
        return currentTopLevelChildMetadata[startIndex..<(startIndex + deleteCount)]
            .contains(where: \.containsAttachment)
    }

    func topLevelChildrenContainPositionAdjustments(
        startIndex: Int,
        deleteCount: Int
    ) -> Bool {
        guard deleteCount > 0,
              let currentTopLevelChildMetadata,
              startIndex >= 0,
              startIndex + deleteCount <= currentTopLevelChildMetadata.count
        else {
            return false
        }
        return currentTopLevelChildMetadata[startIndex..<(startIndex + deleteCount)]
            .contains(where: \.containsPositionAdjustments)
    }

    func trimmedAttributedPatch(
        replacing fullReplaceRange: NSRange,
        with replacement: NSAttributedString,
        allowingExistingFontFallback: Bool = false
    ) -> (replaceRange: NSRange, replacement: NSAttributedString) {
        guard fullReplaceRange.length > 0 else {
            return (fullReplaceRange, replacement)
        }

        let existing = textStorage.attributedSubstring(from: fullReplaceRange)
        let existingRawString = existing.string
        let replacementRawString = replacement.string
        let existingString = existingRawString as NSString
        let replacementString = replacementRawString as NSString
        let sharedLength = min(existing.length, replacement.length)

        var prefix = 0
        while prefix < sharedLength {
            var existingRange = NSRange()
            let existingAttrs = existing.attributes(
                at: prefix,
                longestEffectiveRange: &existingRange,
                in: NSRange(location: prefix, length: sharedLength - prefix)
            )
            var replacementRange = NSRange()
            let replacementAttrs = replacement.attributes(
                at: prefix,
                longestEffectiveRange: &replacementRange,
                in: NSRange(location: prefix, length: sharedLength - prefix)
            )
            guard attributesEqualForPatchTrimming(existingAttrs, replacementAttrs,
                allowingExistingFontFallback: allowingExistingFontFallback) else { break }
            let runEnd = min(NSMaxRange(existingRange), NSMaxRange(replacementRange), sharedLength)
            while prefix < runEnd,
                  existingString.character(at: prefix) == replacementString.character(at: prefix) {
                prefix += 1
            }
            if prefix < runEnd {
                break
            }
        }

        var suffix = 0
        while suffix < (sharedLength - prefix) {
            let existingIndex = existing.length - suffix - 1
            let replacementIndex = replacement.length - suffix - 1
            var existingRange = NSRange()
            let existingAttrs = existing.attributes(
                at: existingIndex,
                longestEffectiveRange: &existingRange,
                in: NSRange(location: prefix, length: existingIndex - prefix + 1)
            )
            var replacementRange = NSRange()
            let replacementAttrs = replacement.attributes(
                at: replacementIndex,
                longestEffectiveRange: &replacementRange,
                in: NSRange(location: prefix, length: replacementIndex - prefix + 1)
            )
            guard attributesEqualForPatchTrimming(existingAttrs, replacementAttrs,
                allowingExistingFontFallback: allowingExistingFontFallback) else { break }
            let maxComparableLength = min(
                existingIndex - max(existingRange.location, prefix) + 1,
                replacementIndex - max(replacementRange.location, prefix) + 1,
                sharedLength - prefix - suffix
            )
            var matchedLength = 0
            while matchedLength < maxComparableLength,
                  existingString.character(at: existingIndex - matchedLength)
                  == replacementString.character(at: replacementIndex - matchedLength) {
                matchedLength += 1
            }
            suffix += matchedLength
            if matchedLength < maxComparableLength {
                break
            }
        }
        prefix = sharedUtf16ScalarBoundary(atOrBefore: prefix, in: existingRawString, and: replacementRawString)
        while suffix > 0 {
            let existingSuffixStart = existing.length - suffix
            let replacementSuffixStart = replacement.length - suffix
            if suffix <= sharedLength - prefix,
               isUtf16ScalarBoundary(existingSuffixStart, in: existingRawString),
               isUtf16ScalarBoundary(replacementSuffixStart, in: replacementRawString) {
                break
            }
            suffix -= 1
        }

        guard prefix > 0 || suffix > 0 else {
            return (fullReplaceRange, replacement)
        }

        let trimmedReplaceRange = NSRange(
            location: fullReplaceRange.location + prefix,
            length: fullReplaceRange.length - prefix - suffix
        )
        let trimmedReplacementRange = NSRange(
            location: prefix,
            length: replacement.length - prefix - suffix
        )
        return (
            trimmedReplaceRange,
            replacement.attributedSubstring(from: trimmedReplacementRange)
        )
    }

}
