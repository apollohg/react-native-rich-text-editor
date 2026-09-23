import os
import UIKit

private struct RootTablePositionMapping {
    let extents: [String: RenderBridge.RootTableScalarExtent]
    let tableIDs: Set<String>
}

extension EditorTextView {
    func reserveRootTableHeights(_ heights: [String: CGFloat]) {
        guard !heights.isEmpty, textStorage.length > 0 else { return }
        var changed = false
        let fullRange = NSRange(location: 0, length: textStorage.length)
        let wasApplyingRustState = isApplyingRustState
        isApplyingRustState = true
        defer { isApplyingRustState = wasApplyingRustState }
        textStorage.beginEditing()
        textStorage.enumerateAttribute(
            RenderBridgeAttributes.rootTableScalarExtent,
            in: fullRange,
            options: []
        ) { value, range, _ in
            guard let extent = value as? RenderBridge.RootTableScalarExtent,
                  let tableID = extent.tableID,
                  let height = heights[tableID],
                  height.isFinite,
                  height > 0
            else { return }
            let paragraphRange = (textStorage.string as NSString).paragraphRange(for: range)
            let current = textStorage.attribute(.paragraphStyle, at: range.location, effectiveRange: nil)
                as? NSParagraphStyle
            let trailing = textStorage.attribute(.paragraphStyle, at: NSMaxRange(paragraphRange) - 1, effectiveRange: nil)
                as? NSParagraphStyle
            if let current,
               let trailing,
               abs(current.minimumLineHeight - height) < 0.5,
               abs(current.maximumLineHeight - height) < 0.5,
               abs(trailing.minimumLineHeight - height) < 0.5,
               abs(trailing.maximumLineHeight - height) < 0.5 {
                return
            }
            let paragraph = (current?.mutableCopy() as? NSMutableParagraphStyle) ?? NSMutableParagraphStyle()
            paragraph.minimumLineHeight = height
            paragraph.maximumLineHeight = height
            paragraph.paragraphSpacing = 0
            paragraph.paragraphSpacingBefore = 0
            textStorage.addAttribute(.paragraphStyle, value: paragraph, range: paragraphRange)
            changed = true
        }
        textStorage.endEditing()
        guard changed else { return }
        layoutManager.invalidateLayout(forCharacterRange: fullRange, actualCharacterRange: nil)
        lastAuthorizedTextStorage.setString(textStorage.string)
        lastAuthorizedAttributedTextStorage.setAttributedString(textStorage)
        PositionBridge.invalidateCache(for: self)
    }

    func withImageLoadOwner<T>(_ body: () -> T) -> T {
        guard let imageLoadOwner else { return body() }
        return imageLoadOwner.withCurrent(body)
    }

    func imageLoadingPolicyDidChange() {
        renderAppearanceRevision &+= 1
    }

    enum PositionCacheUpdate {
        case scan
        case invalidate
        case plainText
        case attributed
    }

    @discardableResult
    func applyTheme(_ theme: EditorTheme?) -> Bool {
        if editorId != 0 {
            guard prepareForExternalEditorUpdate() else { return false }
            self.theme = theme
            let previousOffset = contentOffset
            let stateJSON = EditorV2Shadow.getCurrentState(id: editorId)
            applyUpdateJSON(stateJSON, notifyDelegate: false)
            if heightBehavior == .fixed {
                preserveScrollOffset(previousOffset)
            }
        } else {
            self.theme = theme
            refreshTypingAttributesForSelection()
        }
        if heightBehavior == .autoGrow {
            notifyHeightChangeIfNeeded(force: true)
        }
        return true
    }

    @discardableResult
    func applyAtomRenderConfiguration(_ configuration: AtomRenderConfiguration?) -> Bool {
        if editorId != 0 {
            guard prepareForExternalEditorUpdate() else { return false }
            atomRenderConfiguration = configuration
            let previousOffset = contentOffset
            applyUpdateJSON(EditorV2Shadow.getCurrentState(id: editorId), notifyDelegate: false)
            if heightBehavior == .fixed {
                preserveScrollOffset(previousOffset)
            }
        } else {
            atomRenderConfiguration = configuration
        }
        if heightBehavior == .autoGrow {
            notifyHeightChangeIfNeeded(force: true)
        }
        return true
    }

    private func performPostApplyMaintenance(forceHeightNotify: Bool = false) -> PostApplyTrace {
        let totalStartedAt = DispatchTime.now().uptimeNanoseconds

        let typingAttributesStartedAt = totalStartedAt
        refreshTypingAttributesForSelection()
        let typingAttributesNanos = DispatchTime.now().uptimeNanoseconds - typingAttributesStartedAt

        let heightNotifyStartedAt = DispatchTime.now().uptimeNanoseconds
        lastHeightNotifyMeasureNanosForTesting = 0
        lastHeightNotifyCallbackNanosForTesting = 0
        lastHeightNotifyEnsureLayoutNanosForTesting = 0
        lastHeightNotifyUsedRectNanosForTesting = 0
        lastHeightNotifyContentSizeNanosForTesting = 0
        lastHeightNotifySizeThatFitsNanosForTesting = 0
        if heightBehavior == .autoGrow {
            invalidateAutoGrowHeightMeasurement()
            if forceHeightNotify || window == nil {
                notifyHeightChangeIfNeeded(force: forceHeightNotify)
            } else {
                setNeedsLayout()
            }
        }
        let heightNotifyNanos = DispatchTime.now().uptimeNanoseconds - heightNotifyStartedAt

        let selectionOrContentStartedAt = DispatchTime.now().uptimeNanoseconds
        onSelectionOrContentMayChange?()
        let selectionOrContentCallbackNanos =
            DispatchTime.now().uptimeNanoseconds - selectionOrContentStartedAt

        return PostApplyTrace(
            totalNanos: DispatchTime.now().uptimeNanoseconds - totalStartedAt,
            typingAttributesNanos: typingAttributesNanos,
            heightNotifyNanos: heightNotifyNanos,
            heightNotifyMeasureNanos: lastHeightNotifyMeasureNanosForTesting,
            heightNotifyCallbackNanos: lastHeightNotifyCallbackNanosForTesting,
            heightNotifyEnsureLayoutNanos: lastHeightNotifyEnsureLayoutNanosForTesting,
            heightNotifyUsedRectNanos: lastHeightNotifyUsedRectNanosForTesting,
            heightNotifyContentSizeNanos: lastHeightNotifyContentSizeNanosForTesting,
            heightNotifySizeThatFitsNanos: lastHeightNotifySizeThatFitsNanosForTesting,
            selectionOrContentCallbackNanos: selectionOrContentCallbackNanos
        )
    }

    func applyAttributedRender(
        _ attrStr: NSAttributedString,
        replaceRange: NSRange? = nil,
        usedPatch: Bool,
        positionCacheUpdate: PositionCacheUpdate = .scan,
        authorizedReplaceRange: NSRange? = nil,
        authorizedReplacementText: String? = nil,
        authorizedReplacementAttributedText: NSAttributedString? = nil
    ) -> ApplyRenderTrace {
        let totalStartedAt = DispatchTime.now().uptimeNanoseconds
        let replaceUtf16Length = replaceRange?.length ?? textStorage.length
        let replacementUtf16Length = attrStr.length
        let shouldUseSmallPatchTextMutation =
            replaceRange != nil && shouldUseSmallPatchTextMutation(for: attrStr, replaceRange: replaceRange)
        isApplyingRustState = true
        let textMutationStartedAt = DispatchTime.now().uptimeNanoseconds
        let beginEditingStartedAt = DispatchTime.now().uptimeNanoseconds
        textStorage.beginEditing()
        let beginEditingNanos = DispatchTime.now().uptimeNanoseconds - beginEditingStartedAt
        var stringMutationNanos: UInt64 = 0
        var attributeMutationNanos: UInt64 = 0
        let previousTextStorageDelegate = textStorage.delegate
        textStorage.delegate = nil
        delegate = nil
        defer {
            textStorage.delegate = previousTextStorageDelegate
            ensureInternalTextViewDelegate()
        }
        if let replaceRange {
            if shouldUseSmallPatchTextMutation {
                let stringMutationStartedAt = DispatchTime.now().uptimeNanoseconds
                textStorage.replaceCharacters(in: replaceRange, with: attrStr.string)
                stringMutationNanos =
                    DispatchTime.now().uptimeNanoseconds - stringMutationStartedAt
                let destinationRange = NSRange(location: replaceRange.location, length: attrStr.length)
                let attributeMutationStartedAt = DispatchTime.now().uptimeNanoseconds
                applyAttributes(from: attrStr, to: destinationRange)
                attributeMutationNanos =
                    DispatchTime.now().uptimeNanoseconds - attributeMutationStartedAt
            } else {
                let stringMutationStartedAt = DispatchTime.now().uptimeNanoseconds
                textStorage.replaceCharacters(in: replaceRange, with: attrStr)
                stringMutationNanos =
                    DispatchTime.now().uptimeNanoseconds - stringMutationStartedAt
            }
        } else {
            let stringMutationStartedAt = DispatchTime.now().uptimeNanoseconds
            textStorage.setAttributedString(attrStr)
            stringMutationNanos =
                DispatchTime.now().uptimeNanoseconds - stringMutationStartedAt
        }
        onApplyingRustTextForTesting?()
        let endEditingStartedAt = DispatchTime.now().uptimeNanoseconds
        textStorage.endEditing()
        let endEditingNanos = DispatchTime.now().uptimeNanoseconds - endEditingStartedAt
        let textMutationNanos = DispatchTime.now().uptimeNanoseconds - textMutationStartedAt
        let authorizedTextStartedAt = DispatchTime.now().uptimeNanoseconds
        let snapshotReplaceRange = authorizedReplaceRange ?? replaceRange
        let snapshotReplacementText = authorizedReplacementText ?? attrStr.string
        let snapshotReplacementAttributedText = authorizedReplacementAttributedText ?? attrStr
        if let snapshotReplaceRange,
           snapshotReplaceRange.location >= 0,
           snapshotReplaceRange.location + snapshotReplaceRange.length <= lastAuthorizedTextStorage.length {
            lastAuthorizedTextStorage.replaceCharacters(
                in: snapshotReplaceRange,
                with: snapshotReplacementText
            )
            lastAuthorizedAttributedTextStorage.replaceCharacters(
                in: snapshotReplaceRange,
                with: snapshotReplacementAttributedText
            )
        } else {
            lastAuthorizedTextStorage.setString(replaceRange == nil ? snapshotReplacementText : textStorage.string)
            let fallbackAttributedSnapshot = replaceRange == nil
                ? snapshotReplacementAttributedText
                : NSAttributedString(attributedString: textStorage)
            lastAuthorizedAttributedTextStorage.setAttributedString(fallbackAttributedSnapshot)
        }
        let authorizedTextNanos = DispatchTime.now().uptimeNanoseconds - authorizedTextStartedAt
        let cacheInvalidationStartedAt = DispatchTime.now().uptimeNanoseconds
        lastRenderAppliedPatchForTesting = usedPatch
        switch positionCacheUpdate {
        case .plainText:
            guard let replaceRange else {
                PositionBridge.invalidateCache(for: self)
                break
            }
            let patchedPositionCache = PositionBridge.applyPlainTextPatchIfPossible(
                for: self,
                replaceRange: replaceRange,
                replacementText: attrStr.string
            )
            if !patchedPositionCache {
                PositionBridge.invalidateCache(for: self)
            }
        case .attributed:
            guard let replaceRange else {
                PositionBridge.invalidateCache(for: self)
                break
            }
            let patchedPositionCache = PositionBridge.applyAttributedPatchIfPossible(
                for: self,
                replaceRange: replaceRange,
                replacement: attrStr
            )
            if !patchedPositionCache {
                PositionBridge.invalidateCache(for: self)
            }
        case .invalidate:
            PositionBridge.invalidateCache(for: self)
        case .scan:
            let canPatchPositionCache = if let replaceRange {
                replaceRange.location >= 0
                    && !textStorageRangeContainsAttachment(replaceRange)
                    && !attributedStringContainsAttachment(attrStr)
            } else {
                false
            }
            if let replaceRange, canPatchPositionCache {
                let patchedPositionCache: Bool
                if !textStorageRangeContainsPositionAdjustments(replaceRange),
                   !attributedStringContainsPositionAdjustments(attrStr) {
                    patchedPositionCache = PositionBridge.applyPlainTextPatchIfPossible(
                        for: self,
                        replaceRange: replaceRange,
                        replacementText: attrStr.string
                    )
                } else {
                    patchedPositionCache = PositionBridge.applyAttributedPatchIfPossible(
                        for: self,
                        replaceRange: replaceRange,
                        replacement: attrStr
                    )
                }

                if !patchedPositionCache {
                    PositionBridge.invalidateCache(for: self)
                }
            } else {
                PositionBridge.invalidateCache(for: self)
            }
        }
        let cacheInvalidationNanos = DispatchTime.now().uptimeNanoseconds - cacheInvalidationStartedAt
        isApplyingRustState = false
        scheduleCodeHighlighting()
        return ApplyRenderTrace(
            totalNanos: DispatchTime.now().uptimeNanoseconds - totalStartedAt,
            replaceUtf16Length: replaceUtf16Length,
            replacementUtf16Length: replacementUtf16Length,
            textMutationNanos: textMutationNanos,
            beginEditingNanos: beginEditingNanos,
            endEditingNanos: endEditingNanos,
            stringMutationNanos: stringMutationNanos,
            attributeMutationNanos: attributeMutationNanos,
            authorizedTextNanos: authorizedTextNanos,
            cacheInvalidationNanos: cacheInvalidationNanos,
            usedSmallPatchTextMutation: shouldUseSmallPatchTextMutation
        )
    }

    /// Apply a full render update from Rust to the text view.
    ///
    /// Parses the update JSON, converts render elements to NSAttributedString
    /// via RenderBridge, and replaces the text view's content.
    ///
    /// - Parameter updateJSON: The JSON string from editor_insert_text, etc.
    @discardableResult
    func applyUpdateJSON(_ updateJSON: String, notifyDelegate: Bool = true) -> Bool {
        if let onProjectedUpdate {
            return onProjectedUpdate(updateJSON, notifyDelegate)
        }
        ensureInternalTextViewDelegate()
        let totalStartedAt = DispatchTime.now().uptimeNanoseconds
        let parseStartedAt = totalStartedAt
        guard let data = updateJSON.data(using: .utf8),
              let update = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else { return false }
        let parseNanos = DispatchTime.now().uptimeNanoseconds - parseStartedAt
        resetPendingNativeTextMutationState()

        let renderElements = update["renderElements"] as? [[String: Any]]
        guard let rootTablePositionMapping = rootTablePositionMapping(from: update) else {
            return false
        }
        let rootTableScalarExtents = rootTablePositionMapping.extents
        let selectionFromUpdate = (update["selection"] as? [String: Any])
            .map(self.selectionSummary(from:)) ?? "none"
        Self.updateLog.debug(
            "[applyUpdateJSON.begin] renderCount=\(renderElements?.count ?? 0) updateSelection=\(selectionFromUpdate, privacy: .public) before=\(self.textSnapshotSummary(), privacy: .public)"
        )
        let resolveRenderBlocksStartedAt = DispatchTime.now().uptimeNanoseconds
        let updateDocumentVersion = canonicalDocumentVersion(update["documentVersion"])
        let renderBlocks = parseRenderBlocks(update["renderBlocks"])
        let explicitRenderPatch = parseRenderPatch(update["renderPatch"])
        let resolvedRenderBlocks = renderBlocks
            ?? explicitRenderPatch.flatMap { patch in
                guard patchMatchesCurrentRenderBlocks(
                    patch,
                    updateDocumentVersion: updateDocumentVersion
                ) else { return nil }
                return currentRenderBlocks.flatMap { mergeRenderBlocks(applying: patch, to: $0) }
            }
        let resolveRenderBlocksNanos =
            DispatchTime.now().uptimeNanoseconds - resolveRenderBlocksStartedAt
        if renderBlocks == nil,
           renderElements == nil,
           explicitRenderPatch != nil,
           resolvedRenderBlocks == nil {
            return recoverRenderPatchBaseMismatch(notifyDelegate: notifyDelegate)
        }
        let incomingRootTableIDs = rootTableIDs(
            renderBlocks: resolvedRenderBlocks,
            renderElements: renderElements
        )
        guard incomingRootTableIDs.isSubset(of: rootTablePositionMapping.tableIDs) else {
            return false
        }
        let currentHasRootTableMarkers = PositionBridge.hasRootTableScalarExtents(in: self)

        let derivedRenderPatch: DerivedRenderPatch? =
            if let currentRenderBlocks,
            let resolvedRenderBlocks {
                deriveRenderPatch(from: currentRenderBlocks, to: resolvedRenderBlocks)
            } else {
                nil
            }
        let renderPatch: ParsedRenderPatch? = if let explicitRenderPatch {
            explicitRenderPatch
        } else if case let .patch(derivedPatch)? = derivedRenderPatch {
            derivedPatch
        } else {
            nil
        }
        let tablePositionSnapshotIsUnchanged = if let updateDocumentVersion,
            let currentRenderBlocksDocumentVersion {
            updateDocumentVersion == currentRenderBlocksDocumentVersion
        } else {
            false
        }
        let shouldSkipRender = if case .unchanged? = derivedRenderPatch {
            (rootTableScalarExtents.isEmpty && !currentHasRootTableMarkers
                || tablePositionSnapshotIsUnchanged)
                && textStorage.string == lastAuthorizedText
                && lastAppliedRenderAppearanceRevision == renderAppearanceRevision
        } else {
            false
        }

        let patchTrace: PatchApplyTrace? = if !shouldSkipRender
            && rootTableScalarExtents.isEmpty
            && !currentHasRootTableMarkers
            && textStorage.string == lastAuthorizedText
            && lastAppliedRenderAppearanceRevision == renderAppearanceRevision {
            renderPatch.map(applyRenderPatchIfPossible)
        } else {
            nil
        }
        let appliedPatch = patchTrace?.applied == true
        var usedSmallPatchTextMutation = patchTrace?.usedSmallPatchTextMutation ?? false
        var applyRenderReplaceUtf16Length = patchTrace?.applyRenderReplaceUtf16Length ?? 0
        var applyRenderReplacementUtf16Length =
            patchTrace?.applyRenderReplacementUtf16Length ?? 0
        var buildRenderNanos = patchTrace?.buildRenderNanos ?? 0
        var applyRenderNanos = patchTrace?.applyRenderNanos ?? 0
        var applyRenderTextMutationNanos = patchTrace?.applyRenderTextMutationNanos ?? 0
        var applyRenderBeginEditingNanos = patchTrace?.applyRenderBeginEditingNanos ?? 0
        var applyRenderEndEditingNanos = patchTrace?.applyRenderEndEditingNanos ?? 0
        var applyRenderStringMutationNanos = patchTrace?.applyRenderStringMutationNanos ?? 0
        var applyRenderAttributeMutationNanos =
            patchTrace?.applyRenderAttributeMutationNanos ?? 0
        var applyRenderAuthorizedTextNanos = patchTrace?.applyRenderAuthorizedTextNanos ?? 0
        var applyRenderCacheInvalidationNanos = patchTrace?.applyRenderCacheInvalidationNanos ?? 0
        if shouldSkipRender {
            lastRenderAppliedPatchForTesting = false
            if let resolvedRenderBlocks {
                retainCurrentRenderBlocks(
                    resolvedRenderBlocks,
                    documentVersion: updateDocumentVersion
                )
            }
        } else if !appliedPatch {
            let buildStartedAt = DispatchTime.now().uptimeNanoseconds
            let attrStr: NSAttributedString
            if let resolvedRenderBlocks {
                attrStr = withImageLoadOwner {
                    RenderBridge.renderBlocks(
                        fromArray: resolvedRenderBlocks,
                        baseFont: baseFont,
                        textColor: baseTextColor,
                        theme: theme,
                        atomConfiguration: atomRenderConfiguration,
                        rootTableScalarExtents: rootTableScalarExtents,
                        rootTableIDs: rootTablePositionMapping.tableIDs
                    )
                }
                retainCurrentRenderBlocks(
                    resolvedRenderBlocks,
                    documentVersion: updateDocumentVersion
                )
            } else if let renderElements {
                attrStr = withImageLoadOwner {
                    RenderBridge.renderElements(
                        fromArray: renderElements,
                        baseFont: baseFont,
                        textColor: baseTextColor,
                        theme: theme,
                        atomConfiguration: atomRenderConfiguration,
                        rootTableScalarExtents: rootTableScalarExtents,
                        rootTableIDs: rootTablePositionMapping.tableIDs
                    )
                }
                invalidateCurrentRenderBlocks()
            } else {
                return false
            }
            buildRenderNanos = DispatchTime.now().uptimeNanoseconds - buildStartedAt
            let applyTrace = applyAttributedRender(
                attrStr,
                usedPatch: false,
                positionCacheUpdate: .invalidate
            )
            refreshTopLevelChildMetadata(from: attrStr)
            applyRenderReplaceUtf16Length = applyTrace.replaceUtf16Length
            applyRenderReplacementUtf16Length = applyTrace.replacementUtf16Length
            applyRenderNanos = applyTrace.totalNanos
            applyRenderTextMutationNanos = applyTrace.textMutationNanos
            applyRenderBeginEditingNanos = applyTrace.beginEditingNanos
            applyRenderEndEditingNanos = applyTrace.endEditingNanos
            applyRenderStringMutationNanos = applyTrace.stringMutationNanos
            applyRenderAttributeMutationNanos = applyTrace.attributeMutationNanos
            applyRenderAuthorizedTextNanos = applyTrace.authorizedTextNanos
            applyRenderCacheInvalidationNanos = applyTrace.cacheInvalidationNanos
            usedSmallPatchTextMutation = applyTrace.usedSmallPatchTextMutation
            lastAppliedRenderAppearanceRevision = renderAppearanceRevision
        } else if let resolvedRenderBlocks {
            retainCurrentRenderBlocks(
                resolvedRenderBlocks,
                documentVersion: updateDocumentVersion
            )
            lastAppliedRenderAppearanceRevision = renderAppearanceRevision
        }
        if appliedPatch,
           let renderPatch,
           let resolvedRenderBlocks,
           !refreshRetainedPositionalMetadata(
               startingAt: renderPatch.startIndex + renderPatch.renderBlocks.count,
               updatedRenderBlocks: resolvedRenderBlocks
           ) {
            // The preflight is deliberately conservative: a partial metadata
            // refresh must never leave a retained atom targeting stale state.
            // The next update will take the full safe path after cache reset.
            currentTopLevelChildMetadata = nil
        }

        // The core is the authority on empty state; adopt it before the
        // placeholder is reconsidered.
        coreReportedDocumentIsEmpty = update["documentIsEmpty"] as? Bool
        refreshPlaceholderVisibility()
        Self.updateLog.debug(
            "[applyUpdateJSON.rendered] mode=\(appliedPatch ? "patch" : "full", privacy: .public) after=\(self.textSnapshotSummary(), privacy: .public)"
        )

        let selectionTrace: SelectionApplyTrace
        if let selection = update["selection"] as? [String: Any] {
            setAuthoritativeCellSelectionActive(selection["type"] as? String == "cell")
            let representable = rootSelectionIsRepresentable(selection)
            setRootTableSelectionRepresentable(representable)
            if representable {
                selectionTrace = applySelectionFromJSON(selection)
            } else {
                selectionTrace = SelectionApplyTrace(
                    totalNanos: 0,
                    resolveNanos: 0,
                    assignmentNanos: 0,
                    chromeNanos: 0
                )
            }
        } else {
            selectionTrace = SelectionApplyTrace(
                totalNanos: 0,
                resolveNanos: 0,
                assignmentNanos: 0,
                chromeNanos: 0
            )
        }
        let postApplyTrace = performPostApplyMaintenance()
        let postApplyNanos = postApplyTrace.totalNanos

        if captureApplyUpdateTraceForTesting {
            lastApplyUpdateTraceForTesting = ApplyUpdateTrace(
                attemptedPatch: renderPatch != nil,
                patchStartIndex: renderPatch?.startIndex,
                patchDeleteCount: renderPatch?.deleteCount,
                patchRenderBlockCount: renderPatch?.renderBlocks.count,
                usedPatch: appliedPatch,
                usedSmallPatchTextMutation: usedSmallPatchTextMutation,
                applyRenderReplaceUtf16Length: applyRenderReplaceUtf16Length,
                applyRenderReplacementUtf16Length: applyRenderReplacementUtf16Length,
                parseNanos: parseNanos,
                resolveRenderBlocksNanos: resolveRenderBlocksNanos,
                patchEligibilityNanos: patchTrace?.eligibilityNanos ?? 0,
                patchTrimNanos: patchTrace?.trimNanos ?? 0,
                patchMetadataNanos: patchTrace?.metadataNanos ?? 0,
                buildRenderNanos: buildRenderNanos,
                applyRenderNanos: applyRenderNanos,
                selectionNanos: selectionTrace.totalNanos,
                postApplyNanos: postApplyNanos,
                totalNanos: DispatchTime.now().uptimeNanoseconds - totalStartedAt,
                applyRenderTextMutationNanos: applyRenderTextMutationNanos,
                applyRenderBeginEditingNanos: applyRenderBeginEditingNanos,
                applyRenderEndEditingNanos: applyRenderEndEditingNanos,
                applyRenderStringMutationNanos: applyRenderStringMutationNanos,
                applyRenderAttributeMutationNanos: applyRenderAttributeMutationNanos,
                applyRenderAuthorizedTextNanos: applyRenderAuthorizedTextNanos,
                applyRenderCacheInvalidationNanos: applyRenderCacheInvalidationNanos,
                selectionResolveNanos: selectionTrace.resolveNanos,
                selectionAssignmentNanos: selectionTrace.assignmentNanos,
                selectionChromeNanos: selectionTrace.chromeNanos,
                postApplyTypingAttributesNanos: postApplyTrace.typingAttributesNanos,
                postApplyHeightNotifyNanos: postApplyTrace.heightNotifyNanos,
                postApplyHeightNotifyMeasureNanos: postApplyTrace.heightNotifyMeasureNanos,
                postApplyHeightNotifyCallbackNanos: postApplyTrace.heightNotifyCallbackNanos,
                postApplyHeightNotifyEnsureLayoutNanos: postApplyTrace.heightNotifyEnsureLayoutNanos,
                postApplyHeightNotifyUsedRectNanos: postApplyTrace.heightNotifyUsedRectNanos,
                postApplyHeightNotifyContentSizeNanos: postApplyTrace.heightNotifyContentSizeNanos,
                postApplyHeightNotifySizeThatFitsNanos: postApplyTrace.heightNotifySizeThatFitsNanos,
                postApplySelectionOrContentCallbackNanos:
                postApplyTrace.selectionOrContentCallbackNanos
            )
        }
        recordAuthorizedSelectionIfPossible()
        Self.updateLog.debug(
            "[applyUpdateJSON.end] finalSelection=\(self.selectionSummary(), privacy: .public) textState=\(self.textSnapshotSummary(), privacy: .public)"
        )

        if notifyDelegate {
            editorDelegate?.editorTextView(self, didReceiveUpdate: updateJSON)
        }
        onAuthoritativeRenderApplied?(updateJSON)
        return true
    }

    private func rootTablePositionMapping(
        from update: [String: Any]
    ) -> RootTablePositionMapping? {
        guard tableCellPositionMap == nil else {
            return .init(extents: [:], tableIDs: [])
        }
        guard let mappings = update["tableInputMappings"] as? [String: Any],
              let tables = mappings["tables"] as? [String: Any]
        else { return .init(extents: [:], tableIDs: []) }

        var extents: [String: RenderBridge.RootTableScalarExtent] = [:]
        var tableIDs = Set<String>()
        for (tableID, rawTable) in tables {
            guard let table = rawTable as? [String: Any],
                  Set(table.keys) == ["extent", "cells"],
                  let cells = table["cells"] as? [[String: Any]]
            else { return nil }
            tableIDs.insert(tableID)
            if table["extent"] is NSNull {
                guard cells.allSatisfy({ cell in
                    guard let blocks = cell["blocks"] as? [[String: Any]],
                          let excluded = cell["excluded"] as? [[String: Any]]
                    else { return false }
                    return blocks.isEmpty && excluded.allSatisfy { $0["extent"] is NSNull }
                }) else { return nil }
                continue
            }
            guard let rawExtent = table["extent"] as? [String: Any],
                  Set(rawExtent.keys) == ["scalarStart", "scalarEnd"],
                  let scalarStart = v2ExactUInt32(rawExtent["scalarStart"] as? NSNumber),
                  let scalarEnd = v2ExactUInt32(rawExtent["scalarEnd"] as? NSNumber),
                  scalarStart <= scalarEnd
            else { return nil }
            extents[tableID] = .init(scalarStart: scalarStart, scalarEnd: scalarEnd)
        }
        return .init(extents: extents, tableIDs: tableIDs)
    }

    private func rootTableIDs(
        renderBlocks: [[[String: Any]]]?,
        renderElements: [[String: Any]]?
    ) -> Set<String> {
        let elements = renderBlocks?.flatMap { $0 } ?? renderElements ?? []
        return Set(elements.compactMap { element in
            element["type"] as? String == "table" ? element["tableId"] as? String : nil
        })
    }

    private func rootSelectionIsRepresentable(_ selection: [String: Any]) -> Bool {
        guard tableCellPositionMap == nil else { return true }
        guard PositionBridge.hasRootTableScalarExtents(in: self) else { return true }
        switch selection["type"] as? String {
        case "text":
            guard let anchor = v2ExactUInt32(selection["anchorScalar"] as? NSNumber),
                  let head = v2ExactUInt32(selection["headScalar"] as? NSNumber)
            else { return false }
            if anchor == head {
                return PositionBridge.isScalarPositionRepresentable(anchor, in: self)
            }
            return PositionBridge.isScalarRangeRepresentable(
                from: min(anchor, head),
                to: max(anchor, head),
                in: self
            )
        case "node":
            guard let scalar = v2ExactUInt32(selection["posScalar"] as? NSNumber),
                  scalar < UInt32.max
            else { return false }
            return PositionBridge.isScalarRangeRepresentable(
                from: scalar,
                to: scalar + 1,
                in: self
            ) && PositionBridge.isRootTextInputRangeSafe(
                from: scalar,
                to: scalar + 1,
                in: self
            )
        default:
            return false
        }
    }

    /// Apply a render JSON string (just render elements, no update wrapper).
    ///
    /// Used for initial content loading (set_html / set_json return render
    /// elements directly, not wrapped in an EditorUpdate).
    func applyRenderJSON(_ renderJSON: String) {
        ensureInternalTextViewDelegate()
        resetPendingNativeTextMutationState()
        Self.updateLog.debug(
            "[applyRenderJSON.begin] before=\(self.textSnapshotSummary(), privacy: .public)"
        )
        let attrStr = withImageLoadOwner {
            RenderBridge.renderElements(
                fromJSON: renderJSON,
                baseFont: baseFont,
                textColor: baseTextColor,
                theme: theme,
                atomConfiguration: atomRenderConfiguration
            )
        }
        _ = applyAttributedRender(attrStr, usedPatch: false)
        invalidateCurrentRenderBlocks()
        refreshTopLevelChildMetadata(from: attrStr)
        lastAppliedRenderAppearanceRevision = renderAppearanceRevision

        // A bare render carries no editor update, so there is no authoritative
        // empty state to adopt; fall back until the next update supplies one.
        coreReportedDocumentIsEmpty = nil
        refreshPlaceholderVisibility()
        _ = performPostApplyMaintenance()
        recordAuthorizedSelectionIfPossible()
        Self.updateLog.debug(
            "[applyRenderJSON.end] after=\(self.textSnapshotSummary(), privacy: .public)"
        )
    }

}
