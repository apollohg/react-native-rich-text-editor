package com.apollohg.editor

import android.text.Annotation
import android.text.Spanned
import android.view.View
import com.apollohg.editor.RenderBridge.RenderBuildState
import org.json.JSONArray

internal fun RenderBridge.appendElements(
    state: RenderBuildState,
    elements: JSONArray,
    baseFontSize: Float,
    textColor: Int,
    theme: EditorTheme?,
    density: Float,
    hostView: View?,
    atomConfiguration: AtomRenderConfiguration?,
    topLevelChildIndex: Int? = null
) {
    for (i in 0 until elements.length()) {
        val element = elements.optJSONObject(i) ?: continue
        val type = element.optString("type", "")

        when (type) {
            "textRun" -> {
                val text = element.optString("text", "")
                val marksArray = element.optJSONArray("marks")
                val marks = parseMarks(marksArray)
                appendStyledText(
                    state.result,
                    text,
                    marks,
                    baseFontSize,
                    textColor,
                    state.blockStack,
                    state.pendingLeadingMargins,
                    theme,
                    density
                )
            }

            "voidInline" -> {
                val nodeType = element.optString("nodeType", "")
                appendVoidInline(
                    state.result,
                    nodeType,
                    baseFontSize,
                    textColor,
                    state.blockStack,
                    state.pendingLeadingMargins,
                    theme,
                    density
                )
            }

            "voidBlock" -> {
                val nodeType = element.optString("nodeType", "")
                val attrs = element.optJSONObject("attrs")
                val occurrence = state.atomOccurrences[nodeType] ?: 0
                state.atomOccurrences[nodeType] = occurrence + 1
                val atomId = element.opt("atomId") as? String
                val atomKey = atomId ?: "$nodeType:$occurrence"
                val docPos = exactV2U32(element.opt("docPos") as? Number)?.toInt()
                if (!state.isFirstBlock) {
                    val spacingPx = ((state.nextBlockSpacingBefore ?: 0f) * density).toInt()
                    val separatorStart = state.result.length
                    appendInterBlockNewline(
                        state.result,
                        baseFontSize,
                        textColor,
                        spacingPx,
                        topLevelChildIndex = topLevelChildIndex
                    )
                    if (theme?.styleSheet !=
                        null
                    ) {
                        state.blockStack.filter { it.renderStart == separatorStart }.forEach {
                            it.renderStart =
                                state.result.length
                        }
                    }
                }
                state.isFirstBlock = false
                val spacingBefore = theme?.effectiveTextStyle(nodeType)?.spacingAfter
                    ?: theme?.list?.itemSpacing
                state.replaceNextBlockSpacing(spacingBefore)
                appendVoidBlock(
                    state.result,
                    nodeType,
                    attrs,
                    baseFontSize,
                    textColor,
                    theme,
                    density,
                    spacingBefore,
                    hostView,
                    topLevelChildIndex,
                    atomConfiguration,
                    atomKey,
                    docPos,
                    atomId != null,
                    state.blockStack.isEmpty(),
                    state.reusableImages,
                    state.blockStack.foldIndexed(EditorEdges()) { index, total, block ->
                        total +
                            (
                                theme?.styleSheet?.box(
                                    block.nodeType,
                                    state.blockStack.take(index).map {
                                        it.nodeType
                                    }
                                )?.outerInset?.scaled(density)
                                    ?: EditorEdges()
                                )
                    },
                    state.blockStack.size,
                    state.blockStack.map { it.nodeType }
                )
            }

            "opaqueInlineAtom" -> {
                val nodeType = element.optString("nodeType", "")
                val label = element.optString("label", "?")
                val docPos = exactV2U32(element.opt("docPos") as? Number)?.toLong() ?: continue
                val mentionTheme = EditorMentionTheme.fromJson(
                    element.optJSONObject("mentionTheme")
                )
                appendOpaqueInlineAtom(
                    state.result,
                    nodeType,
                    label,
                    docPos,
                    baseFontSize,
                    textColor,
                    state.blockStack,
                    state.pendingLeadingMargins,
                    theme,
                    mentionTheme,
                    density
                )
            }

            "opaqueBlockAtom" -> {
                val nodeType = element.optString("nodeType", "")
                val label = element.optString("label", "?")
                val docPos = exactV2U32(element.opt("docPos") as? Number)?.toLong() ?: continue
                val blockSpacing = theme?.effectiveTextStyle(nodeType)?.spacingAfter
                if (!state.isFirstBlock) {
                    val spacingPx = ((state.nextBlockSpacingBefore ?: 0f) * density).toInt()
                    val separatorStart = state.result.length
                    appendInterBlockNewline(
                        state.result,
                        baseFontSize,
                        textColor,
                        spacingPx,
                        topLevelChildIndex = topLevelChildIndex
                    )
                    if (theme?.styleSheet !=
                        null
                    ) {
                        state.blockStack.filter { it.renderStart == separatorStart }.forEach {
                            it.renderStart =
                                state.result.length
                        }
                    }
                }
                state.isFirstBlock = false
                state.replaceNextBlockSpacing(blockSpacing)
                appendOpaqueBlockAtom(
                    state.result,
                    nodeType,
                    label,
                    docPos,
                    baseFontSize,
                    textColor,
                    theme,
                    blockSpacing,
                    topLevelChildIndex
                )
            }

            "blockStart" -> {
                val nodeType = element.optString("nodeType", "")
                val depth = element.optInt("depth", 0)
                val listContext = element.optJSONObject("listContext")
                if (theme?.styleSheet != null && listContext?.optBoolean("isFirst") == true) {
                    val listName = if (listContext.optString("kind") ==
                        "task"
                    ) {
                        "taskList"
                    } else if (listContext.optBoolean("ordered")) {
                        "orderedList"
                    } else {
                        "bulletList"
                    }
                    val container = org.json.JSONObject().put(
                        "type",
                        "blockStart"
                    ).put("nodeType", listName).put("depth", depth)
                    appendElements(
                        state,
                        JSONArray().put(
                            container
                        ),
                        baseFontSize,
                        textColor,
                        theme,
                        density,
                        hostView,
                        atomConfiguration,
                        topLevelChildIndex
                    )
                }
                val isListItemContainer = isListItemNodeType(nodeType) && listContext != null
                val isTransparentContainer = isTransparentContainer(nodeType)
                val nestedListItemContainer =
                    isListItemContainer &&
                        state.blockStack.any {
                            isListItemNodeType(it.nodeType) && it.listContext != null
                        }
                val blockSpacing = if (isListItemContainer) {
                    null
                } else {
                    theme?.effectiveTextStyle(nodeType)?.spacingAfter
                        ?: (if (listContext != null) theme?.list?.itemSpacing else null)
                }

                if (!isListItemContainer && !isTransparentContainer) {
                    if (!state.isFirstBlock) {
                        val spacingPx = ((state.nextBlockSpacingBefore ?: 0f) * density).toInt()
                        val nextBlockStack = state.blockStack + BlockContext(
                            nodeType = nodeType,
                            depth = depth,
                            listContext = listContext,
                            topLevelChildIndex = topLevelChildIndex,
                            markerPending = isListItemContainer,
                            renderStart = state.result.length
                        )
                        val inBlockquoteSeparator =
                            blockquoteDepth(nextBlockStack) > 0f &&
                                trailingRenderedContentHasBlockquote(state.result)
                        val separatorStart = state.result.length
                        appendInterBlockNewline(
                            state.result,
                            baseFontSize,
                            textColor,
                            spacingPx,
                            inBlockquote = inBlockquoteSeparator,
                            topLevelChildIndex = topLevelChildIndex
                        )
                        if (theme?.styleSheet !=
                            null
                        ) {
                            state.blockStack.filter {
                                it.renderStart == separatorStart
                            }.forEach {
                                it.renderStart =
                                    state.result.length
                            }
                        }
                    }
                    state.isFirstBlock = false
                    state.replaceNextBlockSpacing(blockSpacing)
                } else if (nestedListItemContainer && theme?.list?.itemSpacing != null) {
                    if (state.pendingListBoundarySpacing == null) {
                        state.replaceNextBlockSpacing(theme.list.itemSpacing)
                    }
                }

                val ctx = BlockContext(
                    nodeType = nodeType,
                    depth = depth,
                    listContext = listContext,
                    topLevelChildIndex = topLevelChildIndex,
                    markerPending = isListItemContainer,
                    renderStart = state.result.length,
                    language = element.optNullableString("language")
                )
                state.blockStack.add(ctx)

                val markerListContext = when {
                    isListItemContainer -> null
                    listContext != null -> listContext
                    else -> consumePendingListMarker(state.blockStack, state.result.length)
                }

                if (markerListContext != null) {
                    val ownerIndex = state.blockStack.indexOfLast { it.listContext != null }
                    val markerAncestors = state.blockStack.take(ownerIndex + 1).map { it.nodeType }
                    val markerStyle = theme?.styleSheet?.resolveElement(
                        "listMarker",
                        markerAncestors
                    )
                    val ordered = markerListContext.optBoolean("ordered", false)
                    val isTask = markerListContext.optString("kind", "") == "task"
                    val visualListDepth = (
                        state.blockStack.count { it.listContext != null } - 1
                        ).coerceAtLeast(0)
                    val presentationLabel = if (ordered && !isTask) {
                        val index = if (!markerListContext.has("index")) {
                            1L
                        } else {
                            exactV2U32(
                                markerListContext.opt("index") as? Number
                            )?.toLong() ?: 0L
                        }
                        OrderedListMarkerFormatter.label(
                            index,
                            visualListDepth,
                            markerStyle?.ordered ?: theme?.list?.orderedMarker
                        )
                    } else {
                        null
                    }
                    val marker = listMarkerString(markerListContext)
                    var markerTextStyle = resolveTextStyle(
                        nodeType,
                        theme,
                        blockquoteDepth(state.blockStack) > 0
                    )
                    theme?.styleSheet?.let { sheet ->
                        val ancestors = state.blockStack.dropLast(1).map { it.nodeType }
                        val resolved = sheet.resolveText(nodeType, ancestors)
                        markerTextStyle = markerTextStyle.copy(
                            fontSize = if (sheet.hasMatchingRuleProperty(nodeType, ancestors, "fontSize")) {
                                resolved.fontSize
                            } else markerTextStyle.fontSize,
                            color = if (sheet.hasMatchingRuleProperty(nodeType, ancestors, "color")) {
                                resolved.color
                            } else markerTextStyle.color
                        )
                    }
                    val markerBaseSize = markerTextStyle.fontSize?.times(density) ?: baseFontSize
                    val resolvedMarkerBaseSize = if (isTask) {
                        markerBaseSize * LayoutConstants.TASK_LIST_MARKER_FONT_SCALE
                    } else {
                        markerBaseSize
                    }
                    val markerColor = markerStyle?.text?.color ?: theme?.list?.markerColor
                        ?: markerTextStyle.color
                        ?: textColor
                    appendStyledText(
                        state.result,
                        marker,
                        emptyList(),
                        resolvedMarkerBaseSize,
                        markerColor,
                        state.blockStack,
                        state.pendingLeadingMargins,
                        null,
                        density,
                        applyBlockSpans = false
                    )
                    val markerStart = state.result.length - marker.length
                    val markerEnd = state.result.length
                    annotateTopLevelChild(state.result, markerStart, markerEnd, topLevelChildIndex)
                    state.result.setSpan(
                        Annotation(NATIVE_LIST_MARKER_ANNOTATION, "1"),
                        markerStart,
                        markerEnd,
                        Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                    )
                    if (isTask) {
                        state.result.setSpan(
                            Annotation(NATIVE_TASK_LIST_MARKER_ANNOTATION, "1"),
                            markerStart,
                            markerEnd,
                            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                        )
                    }
                    val checkbox = if (isTask) {
                        theme?.styleSheet?.let {
                            resolvedCheckboxStyle(
                                it,
                                markerListContext.optBoolean("checked"),
                                markerAncestors
                            )
                        }
                    } else {
                        null
                    }
                    if (checkbox != null && marker.endsWith(' ')) {
                        state.result.setSpan(
                            EditorCheckboxSpan(
                                checkbox,
                                markerListContext.optBoolean("checked"),
                                density
                            ),
                            markerStart,
                            markerEnd - 1,
                            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                        )
                    }
                    val markerGapPx =
                        (
                            checkbox?.gap ?: markerStyle?.gap ?: theme?.list?.markerGap
                                ?: LayoutConstants.LIST_MARKER_TEXT_GAP
                            ) *
                            density
                    if ((ordered || isTask) && marker.endsWith(' ')) {
                        state.result.setSpan(
                            MarkerGapSpan(markerGapPx),
                            markerEnd - 1,
                            markerEnd,
                            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                        )
                    }
                    if (ordered && !isTask && presentationLabel != null && marker.endsWith(' ')) {
                        state.result.setSpan(
                            OrderedListMarkerSpan(presentationLabel, markerColor),
                            markerStart,
                            markerEnd - 1,
                            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                        )
                    }
                    if (!ordered && !isTask) {
                        val markerScale =
                            markerStyle?.scale ?: theme?.list?.markerScale
                                ?: LayoutConstants.UNORDERED_LIST_MARKER_FONT_SCALE
                        val bulletRadius = ((markerBaseSize * markerScale) * 0.16f).coerceAtLeast(
                            2f * density
                        )
                        val markerWidth = if (theme?.styleSheet != null) {
                            (bulletRadius * 2f) + markerGapPx
                        } else {
                            calculateMarkerWidth(density)
                        }
                        state.result.setSpan(
                            CenteredBulletSpan(
                                textColor = markerColor,
                                markerWidthPx = markerWidth,
                                bulletRadiusPx = bulletRadius,
                                bodyFontSizePx = resolvedMarkerBaseSize,
                                markerGapToTextPx = markerGapPx
                            ),
                            markerStart,
                            markerEnd,
                            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                        )
                    }
                    applyLineHeightSpan(
                        builder = state.result,
                        start = markerStart,
                        end = markerEnd,
                        lineHeight = markerTextStyle.lineHeight,
                        density = density
                    )
                }
            }

            "blockEnd" -> {
                if (state.blockStack.isNotEmpty()) {
                    val endedAncestors = state.blockStack.dropLast(1).map { it.nodeType }
                    val endedBlock = state.blockStack.removeAt(state.blockStack.lastIndex)
                    appendTrailingHardBreakPlaceholderIfNeeded(
                        builder = state.result,
                        endedBlock = endedBlock,
                        remainingBlockStack = state.blockStack,
                        baseFontSize = baseFontSize,
                        textColor = textColor,
                        theme = theme,
                        density = density,
                        pendingLeadingMargins = state.pendingLeadingMargins
                    )
                    theme?.styleSheet?.let { sheet ->
                        val start = endedBlock.renderStart
                        if (start <= state.result.length) {
                            val ancestors = state.blockStack.foldIndexed(EditorEdges()) {
                                    index,
                                    total,
                                    block
                                ->
                                total +
                                    sheet.box(
                                        block.nodeType,
                                        state.blockStack.take(index).map {
                                            it.nodeType
                                        }
                                    ).outerInset.scaled(density)
                            }
                            val empty = start == state.result.length
                            val flags = if (empty) {
                                Spanned.SPAN_MARK_MARK
                            } else {
                                Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                            }
                            state.result.setSpan(
                                EditorBlockBoxSpan(
                                    sheet.box(endedBlock.nodeType, endedAncestors).scaled(density),
                                    ancestors,
                                    state.blockStack.size,
                                    endedBlock.nodeType
                                ),
                                start,
                                state.result.length,
                                flags
                            )
                            if (empty && !isTransparentContainer(endedBlock.nodeType) &&
                                !isListItemNodeType(endedBlock.nodeType)
                            ) {
                                val style = EditorTextStyle(
                                    fontSize = baseFontSize / density,
                                    color = textColor
                                )
                                    .mergedWith(
                                        sheet.resolveText(
                                            endedBlock.nodeType,
                                            state.blockStack.map {
                                                it.nodeType
                                            }
                                        )
                                    )
                                state.result.setSpan(
                                    EditorResolvedTextSpan(style, density),
                                    start,
                                    start,
                                    flags
                                )
                            }
                            val alignment = sheet.resolveText(
                                endedBlock.nodeType,
                                state.blockStack.map {
                                    it.nodeType
                                }
                            ).textAlign
                            if (alignment != null &&
                                !isTransparentContainer(endedBlock.nodeType)
                            ) {
                                state.result.applyPhysicalTextAlignment(
                                    alignment,
                                    start,
                                    state.result.length
                                )
                            }
                        }
                    }
                    if (endedBlock.listContext != null) {
                        val spacing = if (endedBlock.listContext.optBoolean("isLast", false)) {
                            theme?.list?.spacingAfter ?: theme?.list?.itemSpacing
                        } else {
                            theme?.list?.itemSpacing
                        }
                        state.addListBoundarySpacing(spacing)
                    }
                    if (endedBlock.nodeType == "codeBlock" &&
                        endedBlock.renderStart < state.result.length
                    ) {
                        state.result.setSpan(
                            CodeBlockMetadataSpan(endedBlock.language),
                            endedBlock.renderStart,
                            state.result.length,
                            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE
                        )
                        state.pendingCodeBlockSpans.add(
                            PendingCodeBlockSpan(
                                start = endedBlock.renderStart,
                                end = state.result.length
                            )
                        )
                    }
                    if (theme?.styleSheet != null &&
                        endedBlock.listContext?.optBoolean("isLast") == true &&
                        state.blockStack.lastOrNull()?.nodeType in
                        setOf("bulletList", "orderedList", "taskList")
                    ) {
                        appendElements(
                            state,
                            JSONArray().put(
                                org.json.JSONObject().put("type", "blockEnd")
                            ),
                            baseFontSize,
                            textColor,
                            theme,
                            density,
                            hostView,
                            atomConfiguration,
                            topLevelChildIndex
                        )
                    }
                }
            }
        }
    }
}
