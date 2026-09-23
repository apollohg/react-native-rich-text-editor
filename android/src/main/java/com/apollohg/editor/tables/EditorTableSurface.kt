package com.apollohg.editor.tables

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Rect
import android.text.Annotation
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.style.ReplacementSpan
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import com.apollohg.editor.EditorEditText
import com.apollohg.editor.EditorTextStyle
import com.apollohg.editor.EditorV2Adapter
import com.apollohg.editor.RenderBridge
import com.apollohg.editor.RichTextEditorView
import com.apollohg.editor.viewer.PreparedProseBlock
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.PreparedProseTheme
import com.apollohg.editor.viewer.ProseLayoutKey
import com.apollohg.editor.viewer.StaticLayoutAndroidProseLayoutEngine
import com.apollohg.editor.viewer.ViewerBlock
import com.apollohg.editor.viewer.ViewerDocument

internal class RootTableHeightSpan(val heightPx: Int) : ReplacementSpan() {
    override fun getSize(paint: Paint, text: CharSequence?, start: Int, end: Int,
                         fm: Paint.FontMetricsInt?): Int {
        fm?.let {
            it.ascent = -heightPx
            it.top = it.ascent
            it.descent = 0
            it.bottom = 0
        }
        return 0
    }

    override fun draw(canvas: Canvas, text: CharSequence?, start: Int, end: Int,
                      x: Float, top: Int, y: Int, bottom: Int, paint: Paint) = Unit
}

internal class EditorTableSurface(private val host: RichTextEditorView) {
    private data class Entry(val surface: ViewerTableSurface, val localBounds: Rect,
                             val occupiedHeight: Int)

    val drawingView = PreparedProseDrawingView(host.context).apply {
        isFocusable = false
        linkInteractionsEnabled = false
        mentionInteractionsEnabled = false
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        setBackgroundColor(android.graphics.Color.TRANSPARENT)
    }
    private var entries: Map<String, Entry> = emptyMap()
    private var key: Triple<EditorV2Adapter, ULong, Pair<Int, Long>>? = null
    private var positionedBlocks: List<PreparedProseBlock> = emptyList()

    fun clear() {
        entries = emptyMap()
        key = null
        positionedBlocks = emptyList()
        reserve(emptyMap())
        drawingView.install(null)
        (drawingView.parent as? ViewGroup)?.removeView(drawingView)
    }

    fun refresh() {
        val input = host.editorEditText
        val adapter = input.v2Driver as? EditorV2Adapter
        val revision = adapter?.cachedAtomicRenderDocumentRevision
        val width = (input.measuredWidth - input.compoundPaddingLeft - input.compoundPaddingRight)
            .coerceAtLeast(0)
        val markers = markers(input)
        val admittedMappings = adapter?.cachedTableInputMappings?.tables
        if (adapter == null || revision == null ||
            input.lastAppliedDocumentVersion != revision.toString() ||
            input.rootTableMapDocumentVersion != revision.toString() ||
            input.rootTableMapPositionEpoch != adapter.positionEpoch ||
            admittedMappings?.keys != input.rootTableMapTableIds ||
            admittedMappings?.mapNotNull { (id, table) -> table.extent?.let { id to it } }?.toMap() != input.rootTableMapExtents ||
            input.rootTableMapExtents.keys != markers.keys ||
            input.rootTablePositionMap == null || markers.isEmpty() || width <= 0 ||
            adapter.cachedTableRecords.keys.containsAll(markers.keys).not()
        ) {
            if (entries.isNotEmpty() || drawingView.parent != null) clear()
            return
        }
        val nextKey = Triple(adapter, revision, width to input.renderAppearanceRevision)
        if (key != nextKey) {
            val records = lowerEditorTableRecords(adapter.cachedTableRecords) ?: run { clear(); return }
            val density = input.resources.displayMetrics.density
            val base = EditorTextStyle(fontSize = input.baseFontSize / density,
                color = input.baseTextColor)
            val theme = input.theme?.copy(text = base.mergedWith(input.theme?.text))
                ?: com.apollohg.editor.EditorTheme(text = base)
            val preparedTheme = PreparedProseTheme.resolve(null, density,
                semanticGeneration = "editor-table", editorTheme = theme)
                .copy(insetTopPx = 0, insetRightPx = 0, insetBottomPx = 0, insetLeftPx = 0)
            val engine = StaticLayoutAndroidProseLayoutEngine()
            val prepared = markers.mapNotNull { (id, _) ->
                val table = records[id] ?: return@mapNotNull null
                val semantic = "editor-table-$id-$revision"
                val document = ViewerDocument(semantic,
                    listOf(ViewerBlock("table", 0, false, null, null, emptyList(), table = table)),
                    false, 256, tableAttributes = adapter.cachedTableAttributes,
                    tableRecords = records)
                val layoutKey = ProseLayoutKey(semantic, width, "editor-table-${input.renderAppearanceRevision}",
                    0, 0, density.toBits().toLong(), revision.toLong(), semantic)
                val result = engine.prepare(document, layoutKey, preparedTheme, width, density, false)
                val block = result.blocks.firstOrNull { it.tableSurface != null }
                    ?: return@mapNotNull null
                val bounds = block.tableBounds ?: return@mapNotNull null
                if (result.error != null || result.heightPx <= 0) return@mapNotNull null
                id to Entry(requireNotNull(block.tableSurface), bounds, result.heightPx)
            }.toMap()
            entries = prepared
            key = nextKey
        }
        reserve(entries.mapValues { it.value.occupiedHeight })
        if (entries.isEmpty()) {
            drawingView.install(null)
            (drawingView.parent as? ViewGroup)?.removeView(drawingView)
            return
        }
        if (drawingView.parent !== host.editorContentFrame) {
            (drawingView.parent as? ViewGroup)?.removeView(drawingView)
            host.editorContentFrame.addView(drawingView,
                FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.MATCH_PARENT))
        }
        updateGeometry()
    }

    fun updateGeometry() {
        val input = host.editorEditText
        if (entries.isEmpty()) return
        val layout = input.layout
        val markers = markers(input)
        val blocks = markers.mapNotNull { (id, start) ->
            val entry = entries[id] ?: return@mapNotNull null
            val line = layout.getLineForOffset(start)
            val x = input.left + input.totalPaddingLeft + entry.localBounds.left
            val y = input.top + input.totalPaddingTop + layout.getLineTop(line) + entry.localBounds.top
            val rect = Rect(x, y, x + entry.localBounds.width(), y + entry.localBounds.height())
            PreparedProseBlock(emptyList(), rect, tableSurface = entry.surface, tableBounds = rect)
        }.sortedBy { it.bounds.top }
        if (blocks == positionedBlocks && drawingView.preparedLayout != null) return
        positionedBlocks = blocks
        val width = host.editorContentFrame.width.coerceAtLeast(input.measuredWidth).coerceAtLeast(1)
        val height = host.editorContentFrame.height.coerceAtLeast(input.measuredHeight).coerceAtLeast(1)
        val key = ProseLayoutKey("editor-table-canvas", width, "editor-table-canvas", 0, 0,
            input.resources.displayMetrics.density.toBits().toLong(), 0, "editor-table-canvas")
        drawingView.install(PreparedProseLayout(key, width, height, blocks,
            retainedBytes = blocks.sumOf { it.retainedBytes }))
    }

    private fun markers(input: EditorEditText): Map<String, Int> = markers(input.text)

    private fun markers(text: Spanned): Map<String, Int> {
        return text.getSpans(0, text.length, Annotation::class.java)
            .filter { it.key == RenderBridge.NATIVE_ROOT_TABLE_MARKER_ANNOTATION }
            .associate { it.value to text.getSpanStart(it) }
    }

    private fun reserve(heights: Map<String, Int>) {
        val input = host.editorEditText
        fun apply(text: android.text.Spannable): Boolean {
            val markers = markers(text)
            var changed = false
            text.getSpans(0, text.length, RootTableHeightSpan::class.java).forEach { span ->
                val start = text.getSpanStart(span)
                val expected = markers.entries.firstOrNull { it.value == start }?.key?.let(heights::get)
                if (expected != span.heightPx) {
                    text.removeSpan(span)
                    changed = true
                }
            }
            markers.forEach { (id, start) ->
                val height = heights[id] ?: return@forEach
                val existing = text.getSpans(start, start + 1, RootTableHeightSpan::class.java)
                    .any { text.getSpanStart(it) == start && it.heightPx == height }
                if (!existing) {
                    text.setSpan(RootTableHeightSpan(height), start, start + 1,
                        Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
                    changed = true
                }
            }
            return changed
        }
        val authorized = SpannableStringBuilder(input.lastAuthorizedRenderedText ?: input.lastAuthorizedText)
        if (apply(authorized)) input.lastAuthorizedRenderedText = authorized
        if (apply(input.text)) {
            input.invalidateTextLayout()
        }
    }
}
