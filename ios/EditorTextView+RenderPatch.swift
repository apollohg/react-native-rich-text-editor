import os
import UIKit

extension EditorTextView {
    private static let positionalAtomElementTypes: Set<NSString> = [
        "voidInline", "voidBlock", "opaqueInlineAtom", "opaqueBlockAtom"
    ]
    private static let positionalRenderElementKeys = ["docPos", "topLevelChildIndex"]

    struct TopLevelChildMetadata {
        var startOffset: Int
        var containsAttachment: Bool
        var containsPositionAdjustments: Bool
    }

    private struct TopLevelChildMetadataSlice {
        let startIndex: Int
        let entries: [TopLevelChildMetadata]
    }

    private struct RetainedMetadataChange {
        let attribute: NSAttributedString.Key
        let value: NSNumber
        let range: NSRange
    }

    private struct RetainedAtomMetadata {
        let type: String
        let documentPosition: UInt32
        var range: NSRange
    }

    struct ParsedRenderPatch {
        let baseDocumentVersion: UInt64?
        let startIndex: Int
        let deleteCount: Int
        let renderBlocks: [[NSDictionary]]
    }

    enum DerivedRenderPatch {
        case unchanged
        case patch(ParsedRenderPatch)
    }

    func parseRenderBlocks(_ value: Any?) -> [[NSDictionary]]? {
        value as? [[NSDictionary]]
    }

    func parseRenderPatch(_ value: Any?) -> ParsedRenderPatch? {
        guard let raw = value as? [String: Any],
              let startIndex = RenderBridge.jsonInt(raw["startIndex"]),
              let deleteCount = RenderBridge.jsonInt(raw["deleteCount"]),
              let renderBlocks = parseRenderBlocks(raw["renderBlocks"])
        else {
            return nil
        }
        let baseDocumentVersion: UInt64?
        if raw.keys.contains("baseDocumentVersion") {
            guard let parsed = canonicalDocumentVersion(raw["baseDocumentVersion"])
            else { return nil }
            baseDocumentVersion = parsed
        } else {
            baseDocumentVersion = nil
        }

        return ParsedRenderPatch(
            baseDocumentVersion: baseDocumentVersion,
            startIndex: startIndex,
            deleteCount: deleteCount,
            renderBlocks: renderBlocks
        )
    }

    func canonicalDocumentVersion(_ value: Any?) -> UInt64? {
        guard let text = value as? String,
              let version = UInt64(text),
              String(version) == text
        else { return nil }
        return version
    }

    func patchMatchesCurrentRenderBlocks(
        _ patch: ParsedRenderPatch,
        updateDocumentVersion: UInt64?
    ) -> Bool {
        if let baseDocumentVersion = patch.baseDocumentVersion {
            return baseDocumentVersion == currentRenderBlocksDocumentVersion
        }
        return updateDocumentVersion == nil && currentRenderBlocksDocumentVersion == nil
    }

    func retainCurrentRenderBlocks(
        _ blocks: [[NSDictionary]]?,
        documentVersion: UInt64?
    ) {
        currentRenderBlocks = blocks
        currentRenderBlocksDocumentVersion = blocks == nil ? nil : documentVersion
    }

    func invalidateCurrentRenderBlocks() {
        currentRenderBlocks = nil
        currentRenderBlocksDocumentVersion = nil
    }

    func recoverRenderPatchBaseMismatch(notifyDelegate: Bool) -> Bool {
        invalidateCurrentRenderBlocks()
        guard !recoveringRenderPatchBaseMismatch,
              editorId != 0,
              let adapter = EditorV2Registry.adapter(forLegacyId: editorId),
              let recovery = adapter.recoverNativeRender()
        else { return false }
        recoveringRenderPatchBaseMismatch = true
        defer { recoveringRenderPatchBaseMismatch = false }
        return applyUpdateJSON(recovery, notifyDelegate: notifyDelegate)
    }

    func mergeRenderBlocks(
        applying patch: ParsedRenderPatch,
        to current: [[NSDictionary]]
    ) -> [[NSDictionary]]? {
        guard patch.startIndex >= 0,
              patch.deleteCount >= 0,
              patch.startIndex <= current.count,
              patch.startIndex + patch.deleteCount <= current.count
        else {
            return nil
        }

        var merged = current
        merged.replaceSubrange(
            patch.startIndex..<(patch.startIndex + patch.deleteCount),
            with: patch.renderBlocks
        )
        return merged
    }

    private func renderBlockEquals(
        _ lhs: [NSDictionary],
        _ rhs: [NSDictionary]
    ) -> Bool {
        guard lhs.count == rhs.count else { return false }
        for (lhsElement, rhsElement) in zip(lhs, rhs) {
            guard renderElementEquals(lhsElement, rhsElement) else { return false }
        }
        return true
    }

    private func positionalAtomMetadata(in renderBlock: [NSDictionary]) -> [(type: String, docPos: NSNumber)]? {
        let nodes = renderBlock.compactMap { element -> (type: String, docPos: NSNumber)? in
            guard let elementType = element.object(forKey: RenderBridge.renderElementTypeKey) as? NSString,
                  Self.positionalAtomElementTypes.contains(elementType),
                  let nodeType = element["nodeType"] as? String,
                  let docPos = RenderBridge.jsonUInt32(element["docPos"])
            else {
                return nil
            }
            return (nodeType, NSNumber(value: docPos))
        }
        return nodes
    }

    func refreshRetainedPositionalMetadata(
        startingAt retainedStart: Int,
        updatedRenderBlocks: [[NSDictionary]]
    ) -> Bool {
        guard let currentTopLevelChildMetadata,
              currentTopLevelChildMetadata.count == updatedRenderBlocks.count,
              retainedStart >= 0,
              retainedStart <= currentTopLevelChildMetadata.count,
              lastAuthorizedAttributedTextStorage.length == textStorage.length
        else {
            return false
        }

        var changes: [RetainedMetadataChange] = []
        for index in retainedStart..<currentTopLevelChildMetadata.count {
            let start = currentTopLevelChildMetadata[index].startOffset
            let end = index + 1 < currentTopLevelChildMetadata.count
                ? currentTopLevelChildMetadata[index + 1].startOffset
                : textStorage.length
            guard start >= 0, end >= start, end <= textStorage.length else { return false }
            let range = NSRange(location: start, length: end - start)
            var childIndexChanged = false
            textStorage.enumerateAttribute(
                RenderBridgeAttributes.topLevelChildIndex,
                in: range,
                options: [.longestEffectiveRangeNotRequired]
            ) { value, _, stop in
                if let currentIndex = self.topLevelChildIndex(from: value), currentIndex != index {
                    childIndexChanged = true
                    stop.pointee = true
                }
            }
            if childIndexChanged {
                changes.append(RetainedMetadataChange(
                    attribute: RenderBridgeAttributes.topLevelChildIndex,
                    value: NSNumber(value: index),
                    range: range
                ))
            }

            guard let expectedAtoms = positionalAtomMetadata(in: updatedRenderBlocks[index]) else {
                return false
            }
            var actualAtoms: [RetainedAtomMetadata] = []
            var atomPositionsAreValid = true
            textStorage.enumerateAttribute(
                RenderBridgeAttributes.voidNodeType,
                in: range,
                options: [.longestEffectiveRangeNotRequired]
            ) { value, typeRange, typeStop in
                guard value is String else { return }
                self.textStorage.enumerateAttributes(
                    in: typeRange,
                    options: [.longestEffectiveRangeNotRequired]
                ) { attributes, nodeRange, stop in
                    guard let type = attributes[RenderBridgeAttributes.voidNodeType] as? String else { return }
                    guard let position = RenderBridge.jsonUInt32(attributes[RenderBridgeAttributes.docPos]) else {
                        atomPositionsAreValid = false
                        stop.pointee = true
                        typeStop.pointee = true
                        return
                    }
                    if let previous = actualAtoms.last,
                       previous.type == type,
                       previous.documentPosition == position,
                       NSMaxRange(previous.range) == nodeRange.location {
                        actualAtoms[actualAtoms.count - 1].range.length += nodeRange.length
                    } else {
                        actualAtoms.append(RetainedAtomMetadata(type: type, documentPosition: position, range: nodeRange))
                    }
                }
            }
            guard atomPositionsAreValid,
                  actualAtoms.count == expectedAtoms.count,
                  zip(actualAtoms, expectedAtoms).allSatisfy({ $0.type == $1.type })
            else {
                return false
            }
            for (actual, expected) in zip(actualAtoms, expectedAtoms)
                where actual.documentPosition != expected.docPos.uint32Value {
                changes.append(RetainedMetadataChange(
                    attribute: RenderBridgeAttributes.docPos,
                    value: expected.docPos,
                    range: actual.range
                ))
            }
        }
        guard !changes.isEmpty else { return true }
        textStorage.beginEditing()
        for change in changes {
            textStorage.addAttribute(change.attribute, value: change.value, range: change.range)
            lastAuthorizedAttributedTextStorage.addAttribute(change.attribute, value: change.value, range: change.range)
        }
        textStorage.endEditing()
        return true
    }

    private func renderElementEquals(_ lhs: NSDictionary, _ rhs: NSDictionary) -> Bool {
        if lhs.isEqual(rhs) { return true }
        guard let comparableLhs = lhs.mutableCopy() as? NSMutableDictionary,
              let comparableRhs = rhs.mutableCopy() as? NSMutableDictionary
        else { return false }
        comparableLhs.removeObjects(forKeys: Self.positionalRenderElementKeys)
        comparableRhs.removeObjects(forKeys: Self.positionalRenderElementKeys)
        return comparableLhs.isEqual(comparableRhs)
    }

    func deriveRenderPatch(
        from current: [[NSDictionary]],
        to updated: [[NSDictionary]]
    ) -> DerivedRenderPatch {
        let sharedCount = min(current.count, updated.count)

        var prefix = 0
        while prefix < sharedCount, renderBlockEquals(current[prefix], updated[prefix]) {
            prefix += 1
        }

        if prefix == current.count, prefix == updated.count {
            return .unchanged
        }

        var suffix = 0
        while suffix < (sharedCount - prefix),
              renderBlockEquals(
                  current[current.count - suffix - 1],
                  updated[updated.count - suffix - 1]
              ) {
            suffix += 1
        }

        let startIndex = prefix
        let deleteCount = current.count - prefix - suffix
        let endIndex = updated.count - suffix
        let replacementBlocks = Array(updated[startIndex..<endIndex])

        return .patch(
            ParsedRenderPatch(
                baseDocumentVersion: currentRenderBlocksDocumentVersion,
                startIndex: startIndex,
                deleteCount: deleteCount,
                renderBlocks: replacementBlocks
            )
        )
    }

    func topLevelChildIndex(from value: Any?) -> Int? {
        if let number = value as? NSNumber {
            return number.intValue
        }
        return value as? Int
    }

    private func topLevelChildMetadataSlice(
        from attributedString: NSAttributedString
    ) -> TopLevelChildMetadataSlice? {
        guard attributedString.length > 0 else {
            return TopLevelChildMetadataSlice(startIndex: 0, entries: [])
        }

        var entriesByIndex: [Int: TopLevelChildMetadata] = [:]
        var orderedIndexes: [Int] = []

        attributedString.enumerateAttributes(
            in: NSRange(location: 0, length: attributedString.length),
            options: []
        ) { attrs, range, _ in
            guard let index = topLevelChildIndex(from: attrs[RenderBridgeAttributes.topLevelChildIndex]) else {
                return
            }
            if entriesByIndex[index] == nil {
                entriesByIndex[index] = TopLevelChildMetadata(
                    startOffset: range.location,
                    containsAttachment: false,
                    containsPositionAdjustments: false
                )
                orderedIndexes.append(index)
            }
            if attrs[.attachment] != nil {
                entriesByIndex[index]?.containsAttachment = true
            }
            if attrs[RenderBridgeAttributes.syntheticPlaceholder] as? Bool == true
                || attrs[RenderBridgeAttributes.listMarkerContext] != nil {
                entriesByIndex[index]?.containsPositionAdjustments = true
            }
        }

        guard !orderedIndexes.isEmpty else { return nil }
        orderedIndexes.sort()
        guard let startIndex = orderedIndexes.first else { return nil }

        var entries: [TopLevelChildMetadata] = []
        entries.reserveCapacity(orderedIndexes.count)
        for (offset, index) in orderedIndexes.enumerated() {
            guard index == startIndex + offset,
                  let entry = entriesByIndex[index]
            else {
                return nil
            }
            entries.append(entry)
        }

        return TopLevelChildMetadataSlice(startIndex: startIndex, entries: entries)
    }

    func refreshTopLevelChildMetadata(
        from attributedString: NSAttributedString
    ) {
        guard let slice = topLevelChildMetadataSlice(from: attributedString),
              slice.startIndex == 0
        else {
            currentTopLevelChildMetadata = nil
            return
        }
        currentTopLevelChildMetadata = slice.entries
    }

    private func applyTopLevelChildMetadataPatch(
        _ patch: ParsedRenderPatch,
        replaceRange: NSRange,
        renderedPatchMetadata: TopLevelChildMetadataSlice?,
        renderedPatchLength: Int
    ) {
        guard var currentMetadata = currentTopLevelChildMetadata else {
            currentTopLevelChildMetadata = nil
            return
        }

        let newEntries: [TopLevelChildMetadata]
        if let renderedPatchMetadata,
           renderedPatchMetadata.entries.isEmpty {
            newEntries = []
        } else if let renderedPatchMetadata,
                  renderedPatchMetadata.startIndex == patch.startIndex {
            let patchEntries = renderedPatchMetadata.entries.prefix(patch.renderBlocks.count)
            guard patchEntries.count == patch.renderBlocks.count else {
                currentTopLevelChildMetadata = nil
                return
            }
            newEntries = patchEntries.map { entry in
                TopLevelChildMetadata(
                    startOffset: replaceRange.location + entry.startOffset,
                    containsAttachment: entry.containsAttachment,
                    containsPositionAdjustments: entry.containsPositionAdjustments
                )
            }
        } else {
            currentTopLevelChildMetadata = nil
            return
        }

        guard patch.startIndex >= 0,
              patch.deleteCount >= 0,
              patch.startIndex <= currentMetadata.count,
              patch.startIndex + patch.deleteCount <= currentMetadata.count
        else {
            currentTopLevelChildMetadata = nil
            return
        }

        currentMetadata.replaceSubrange(
            patch.startIndex..<(patch.startIndex + patch.deleteCount),
            with: newEntries
        )

        let delta = renderedPatchLength - replaceRange.length
        if delta != 0 {
            let shiftStart = patch.startIndex + newEntries.count
            for index in shiftStart..<currentMetadata.count {
                currentMetadata[index].startOffset += delta
            }
        }
        let retainedStart = patch.startIndex + newEntries.count
        if retainedStart < currentMetadata.count,
           let trailingBoundary = renderedPatchMetadata?.entries.dropFirst(newEntries.count).first {
            // The inserted separator belongs to the first retained child.
            currentMetadata[retainedStart].startOffset = replaceRange.location + trailingBoundary.startOffset
        } else if patch.startIndex == 0, newEntries.isEmpty, !currentMetadata.isEmpty {
            currentMetadata[0].startOffset = 0
        }

        currentTopLevelChildMetadata = currentMetadata
    }

    private func hasTopLevelChildMetadata() -> Bool {
        currentTopLevelChildMetadata != nil
    }

    private func firstCharacterOffset(forTopLevelChildIndex index: Int) -> Int? {
        guard let currentTopLevelChildMetadata,
              index >= 0,
              index < currentTopLevelChildMetadata.count
        else {
            return nil
        }
        return currentTopLevelChildMetadata[index].startOffset
    }

    private func replacementRangeForRenderPatch(
        startIndex: Int,
        deleteCount: Int
    ) -> NSRange? {
        let startLocation: Int
        if let resolvedStart = firstCharacterOffset(forTopLevelChildIndex: startIndex) {
            startLocation = resolvedStart
        } else if deleteCount == 0 {
            startLocation = textStorage.length
        } else {
            return nil
        }

        let endIndexExclusive = startIndex + deleteCount
        let endLocation = firstCharacterOffset(forTopLevelChildIndex: endIndexExclusive)
            ?? textStorage.length
        guard startLocation <= endLocation else { return nil }
        return NSRange(location: startLocation, length: endLocation - startLocation)
    }

    private func singleParagraphBoxStack(
        in attributedString: NSAttributedString, childIndex: Int
    ) -> EditorRenderedBoxStack? {
        var retained: EditorRenderedBoxStack?
        var valid = true
        attributedString.enumerateAttributes(in: NSRange(location: 0, length: attributedString.length)) { attrs, _, stop in
            if let value = attrs[RenderBridgeAttributes.topLevelChildIndex],
               self.topLevelChildIndex(from: value) != childIndex { valid = false }
            if attrs[RenderBridgeAttributes.blockBoundary] != nil { return }
            guard attrs[RenderBridgeAttributes.blockNodeType] as? String == RenderBridge.styledParagraphNodeType,
                  attrs[RenderBridgeAttributes.voidNodeType] == nil,
                  let stack = attrs[editorStyleBoxesAttribute] as? EditorRenderedBoxStack,
                  let boxes = renderedStyleBoxes(from: stack), boxes.count == 1,
                  boxes.first?.depth == 0 else {
                valid = false
                stop.pointee = true
                return
            }
            if let retained, !retained.isEqual(stack) {
                valid = false
                stop.pointee = true
            } else {
                retained = stack
            }
        }
        return valid ? retained : nil
    }

    private func reusingStyledParagraphBox(
        for patch: ParsedRenderPatch, replacing range: NSRange, with replacement: NSAttributedString
    ) -> NSAttributedString? {
        guard theme?.styleSheet != nil, isFirstResponder, isEditable, isInterceptingInput,
              !isComposing, markedTextRange == nil, externalTextComposition == nil,
              tableCellPositionMap == nil,
              patch.deleteCount == 1, patch.renderBlocks.count == 1,
              let currentRenderBlocks, currentRenderBlocks.indices.contains(patch.startIndex),
              currentRenderBlocks[patch.startIndex].first?["nodeType"] as? String == RenderBridge.styledParagraphNodeType,
              patch.renderBlocks.first?.first?["nodeType"] as? String == RenderBridge.styledParagraphNodeType,
              shouldUseSmallPatchTextMutation(for: replacement, replaceRange: range),
              let oldStack = singleParagraphBoxStack(in: textStorage.attributedSubstring(from: range), childIndex: patch.startIndex),
              let newStack = singleParagraphBoxStack(in: replacement, childIndex: patch.startIndex),
              let oldBox = renderedStyleBoxes(from: oldStack)?.first,
              let newBox = renderedStyleBoxes(from: newStack)?.first,
              NSDictionary(dictionary: oldBox.box.values).isEqual(to: newBox.box.values),
              oldBox.depth == newBox.depth, oldBox.leading == newBox.leading,
              oldBox.trailing == newBox.trailing, oldBox.topInset == newBox.topInset,
              oldBox.bottomInset == newBox.bottomInset else { return nil }
        let result = NSMutableAttributedString(attributedString: replacement)
        result.enumerateAttribute(editorStyleBoxesAttribute, in: NSRange(location: 0, length: result.length)) { value, range, _ in
            if value != nil { result.addAttribute(editorStyleBoxesAttribute, value: oldStack, range: range) }
        }
        return result
    }

    func applyRenderPatchIfPossible(_ patch: ParsedRenderPatch) -> PatchApplyTrace {
        let eligibilityStartedAt = DispatchTime.now().uptimeNanoseconds
        guard hasTopLevelChildMetadata(),
              var fullReplaceRange = replacementRangeForRenderPatch(
                  startIndex: patch.startIndex,
                  deleteCount: patch.deleteCount
              )
        else {
            return PatchApplyTrace(
                applied: false,
                eligibilityNanos: DispatchTime.now().uptimeNanoseconds - eligibilityStartedAt,
                trimNanos: 0,
                metadataNanos: 0,
                buildRenderNanos: 0,
                applyRenderNanos: 0,
                applyRenderReplaceUtf16Length: 0,
                applyRenderReplacementUtf16Length: 0,
                applyRenderTextMutationNanos: 0,
                applyRenderBeginEditingNanos: 0,
                applyRenderEndEditingNanos: 0,
                applyRenderStringMutationNanos: 0,
                applyRenderAttributeMutationNanos: 0,
                applyRenderAuthorizedTextNanos: 0,
                applyRenderCacheInvalidationNanos: 0,
                usedSmallPatchTextMutation: false
            )
        }

        if patch.startIndex == 0, patch.deleteCount > 0, patch.renderBlocks.isEmpty,
           NSMaxRange(fullReplaceRange) < textStorage.length,
           (textStorage.string as NSString).substring(with: NSRange(location: NSMaxRange(fullReplaceRange), length: 1)) == "\n" {
            // Removing the first child also removes the retained child's separator.
            fullReplaceRange.length += 1
        }

        let buildStartedAt = DispatchTime.now().uptimeNanoseconds
        let attrStr = withImageLoadOwner {
            RenderBridge.renderBlocks(
                fromArray: patch.renderBlocks,
                startIndex: patch.startIndex,
                includeLeadingInterBlockSeparator: patch.startIndex > 0,
                // Replacement ranges already extend to the following child
                // boundary, whose separator is rendered by that retained
                // child. A trailing separator is therefore needed only when
                // inserting before the first existing child.
                includeTrailingInterBlockSeparator: patch.startIndex == 0
                    && patch.deleteCount == 0
                    && !(currentTopLevelChildMetadata?.isEmpty ?? true),
                baseFont: baseFont,
                textColor: baseTextColor,
                theme: theme,
                atomConfiguration: atomRenderConfiguration
            )
        }
        let buildRenderNanos = DispatchTime.now().uptimeNanoseconds - buildStartedAt
        let renderedPatchMetadata = topLevelChildMetadataSlice(from: attrStr)
        let renderedPatchContainsAttachment =
            renderedPatchMetadata?.entries.contains(where: \.containsAttachment)
                ?? attributedStringContainsAttachment(attrStr)
        let renderedPatchContainsListMarkerContext =
            attributedStringContainsListMarkerContext(attrStr)
        let renderedPatchContainsPositionAdjustments =
            renderedPatchMetadata?.entries.contains(where: \.containsPositionAdjustments)
                ?? attributedStringContainsPositionAdjustments(attrStr)
        guard !topLevelChildrenContainAttachment(
            startIndex: patch.startIndex,
            deleteCount: patch.deleteCount
        ),
            !renderedPatchContainsAttachment
        else {
            return PatchApplyTrace(
                applied: false,
                eligibilityNanos: DispatchTime.now().uptimeNanoseconds - eligibilityStartedAt,
                trimNanos: 0,
                metadataNanos: 0,
                buildRenderNanos: buildRenderNanos,
                applyRenderNanos: 0,
                applyRenderReplaceUtf16Length: 0,
                applyRenderReplacementUtf16Length: 0,
                applyRenderTextMutationNanos: 0,
                applyRenderBeginEditingNanos: 0,
                applyRenderEndEditingNanos: 0,
                applyRenderStringMutationNanos: 0,
                applyRenderAttributeMutationNanos: 0,
                applyRenderAuthorizedTextNanos: 0,
                applyRenderCacheInvalidationNanos: 0,
                usedSmallPatchTextMutation: false
            )
        }
        guard !textStorageRangeContainsListMarkerContext(fullReplaceRange),
              !renderedPatchContainsListMarkerContext
        else {
            return PatchApplyTrace(
                applied: false,
                eligibilityNanos: DispatchTime.now().uptimeNanoseconds - eligibilityStartedAt,
                trimNanos: 0,
                metadataNanos: 0,
                buildRenderNanos: buildRenderNanos,
                applyRenderNanos: 0,
                applyRenderReplaceUtf16Length: 0,
                applyRenderReplacementUtf16Length: 0,
                applyRenderTextMutationNanos: 0,
                applyRenderBeginEditingNanos: 0,
                applyRenderEndEditingNanos: 0,
                applyRenderStringMutationNanos: 0,
                applyRenderAttributeMutationNanos: 0,
                applyRenderAuthorizedTextNanos: 0,
                applyRenderCacheInvalidationNanos: 0,
                usedSmallPatchTextMutation: false
            )
        }
        let eligibilityNanos =
            DispatchTime.now().uptimeNanoseconds - eligibilityStartedAt - buildRenderNanos
        let positionCacheUpdate: PositionCacheUpdate =
            if topLevelChildrenContainPositionAdjustments(
                startIndex: patch.startIndex,
                deleteCount: patch.deleteCount
            ) || renderedPatchContainsPositionAdjustments {
                .attributed
            } else {
                .plainText
            }
        let trimStartedAt = DispatchTime.now().uptimeNanoseconds
        let reusedParagraph = reusingStyledParagraphBox(for: patch, replacing: fullReplaceRange, with: attrStr)
        let patchRender = reusedParagraph ?? attrStr
        let patchToApply = trimmedAttributedPatch(replacing: fullReplaceRange, with: patchRender,
            allowingExistingFontFallback: reusedParagraph != nil)
        let trimNanos = DispatchTime.now().uptimeNanoseconds - trimStartedAt
        let applyTrace = applyAttributedRender(
            patchToApply.replacement,
            replaceRange: patchToApply.replaceRange,
            usedPatch: true,
            positionCacheUpdate: positionCacheUpdate,
            authorizedReplaceRange: fullReplaceRange,
            authorizedReplacementAttributedText: patchRender,
            allowsEmptyNativeTextReplacement: reusedParagraph != nil
        )
        if theme?.styleSheet != nil {
            textStorage.beginEditing()
            RenderBridge.collapseStyledSiblingMargins(in: textStorage) { value, range in
                self.lastAuthorizedAttributedTextStorage.addAttribute(.paragraphStyle, value: value, range: range)
            }
            textStorage.endEditing()
        }
        let metadataStartedAt = DispatchTime.now().uptimeNanoseconds
        applyTopLevelChildMetadataPatch(
            patch,
            replaceRange: fullReplaceRange,
            renderedPatchMetadata: renderedPatchMetadata,
            renderedPatchLength: attrStr.length
        )
        let metadataNanos = DispatchTime.now().uptimeNanoseconds - metadataStartedAt
        return PatchApplyTrace(
            applied: true,
            eligibilityNanos: eligibilityNanos,
            trimNanos: trimNanos,
            metadataNanos: metadataNanos,
            buildRenderNanos: buildRenderNanos,
            applyRenderNanos: applyTrace.totalNanos,
            applyRenderReplaceUtf16Length: applyTrace.replaceUtf16Length,
            applyRenderReplacementUtf16Length: applyTrace.replacementUtf16Length,
            applyRenderTextMutationNanos: applyTrace.textMutationNanos,
            applyRenderBeginEditingNanos: applyTrace.beginEditingNanos,
            applyRenderEndEditingNanos: applyTrace.endEditingNanos,
            applyRenderStringMutationNanos: applyTrace.stringMutationNanos,
            applyRenderAttributeMutationNanos: applyTrace.attributeMutationNanos,
            applyRenderAuthorizedTextNanos: applyTrace.authorizedTextNanos,
            applyRenderCacheInvalidationNanos: applyTrace.cacheInvalidationNanos,
            usedSmallPatchTextMutation: applyTrace.usedSmallPatchTextMutation
        )
    }

}
