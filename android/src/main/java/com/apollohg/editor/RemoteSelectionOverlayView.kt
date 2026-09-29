package com.apollohg.editor

import com.apollohg.editor.tables.EditorTableIndex
import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.util.AttributeSet
import android.util.TypedValue
import androidx.appcompat.content.res.AppCompatResources
import com.apollohg.editor.tables.EditorCellSelection
import com.apollohg.editor.tables.resolveEditorCellSelection
import com.apollohg.editor.viewer.RemoteTableCellSelection
import org.json.JSONArray
import org.json.JSONObject

data class RemoteSelectionFrame(val editorId: String, val documentRevision: String) {
    companion object {
        fun fromJson(value: JSONObject?): RemoteSelectionFrame? {
            value ?: return null
            val editorId = canonicalV2U64(value.opt("editorId") as? String) ?: return null
            val revision = canonicalV2U64(value.opt("documentRevision") as? String) ?: return null
            return RemoteSelectionFrame(editorId, revision)
        }

        internal fun installed(adapter: EditorV2Adapter?): RemoteSelectionFrame? {
            val revision = adapter?.installedFrameRevision ?: return null
            return RemoteSelectionFrame(adapter.editorId, revision.toString())
        }
    }
}

data class RemoteCellRectangle(val anchorCell: Long, val headCell: Long) {
    companion object {
        fun fromJson(json: JSONObject?): RemoteCellRectangle? {
            json ?: return null
            val anchorCell = exactV2U32(json.opt("anchorCell") as? Number)?.toLong() ?: return null
            val headCell = exactV2U32(json.opt("headCell") as? Number)?.toLong() ?: return null
            return RemoteCellRectangle(anchorCell, headCell)
        }
    }
}

data class RemoteSelectionDecoration(
    val clientId: String,
    val anchor: Int,
    val head: Int,
    val color: Int,
    val name: String?,
    val isFocused: Boolean,
    val cellRectangle: RemoteCellRectangle? = null,
    val resolvedAt: RemoteSelectionFrame? = null
) {
    companion object {
        fun fromJson(context: Context, json: String?): List<RemoteSelectionDecoration> {
            if (json.isNullOrBlank()) return emptyList()
            val array = try {
                JSONArray(json)
            } catch (_: Throwable) {
                return emptyList()
            }
            val fallbackColor = resolveFallbackColor(context)

            return buildList {
                for (index in 0 until array.length()) {
                    val item = array.optJSONObject(index) ?: continue
                    val clientId = canonicalV2U64(item.opt("clientId") as? String) ?: continue
                    val anchor = exactV2ScalarInt(item.opt("anchor") as? Number) ?: continue
                    val head = exactV2ScalarInt(item.opt("head") as? Number) ?: continue
                    val color = parseColor(item.optString("color", ""), fallbackColor)
                    val resolvedAt = RemoteSelectionFrame.fromJson(item.optJSONObject("resolvedAt"))
                    if (item.has("resolvedAt") && resolvedAt == null) continue
                    add(
                        RemoteSelectionDecoration(
                            clientId = clientId,
                            anchor = anchor,
                            head = head,
                            color = color,
                            name = item.optString("name").takeIf { it.isNotBlank() },
                            isFocused = item.optBoolean("isFocused", false),
                            cellRectangle = RemoteCellRectangle.fromJson(item.optJSONObject("cellRectangle")),
                            resolvedAt = resolvedAt
                        )
                    )
                }
            }
        }

        private fun parseColor(raw: String, fallbackColor: Int): Int = try {
            Color.parseColor(raw)
        } catch (_: Throwable) {
            fallbackColor
        }

        private fun resolveFallbackColor(context: Context): Int {
            val typedValue = TypedValue()
            val attrs = intArrayOf(
                androidx.appcompat.R.attr.colorPrimary,
                androidx.appcompat.R.attr.colorAccent,
                android.R.attr.colorAccent,
                android.R.attr.textColorPrimary
            )
            for (attr in attrs) {
                if (!context.theme.resolveAttribute(attr, typedValue, true)) {
                    continue
                }
                if (typedValue.resourceId != 0) {
                    AppCompatResources.getColorStateList(context, typedValue.resourceId)
                        ?.defaultColor
                        ?.let { return it }
                } else if (typedValue.type in
                    TypedValue.TYPE_FIRST_COLOR_INT..TypedValue.TYPE_LAST_COLOR_INT
                ) {
                    return typedValue.data
                }
            }
            return Color.TRANSPARENT
        }
    }
}

data class RemoteSelectionDebugSnapshot(val clientId: String, val caretRect: RectF?)

class RemoteSelectionOverlayView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : PointerTransparentView(context, attrs, defStyleAttr) {
    private data class CachedSelectionGeometry(
        val clientId: String,
        val selectionPath: Path?,
        val selectionColor: Int,
        val caretRect: RectF?,
        val caretColor: Int
    )

    private data class GeometrySnapshot(
        val editorId: Long,
        val text: String,
        val layoutWidth: Int,
        val layoutHeight: Int,
        val baseX: Int,
        val baseY: Int,
        val width: Int,
        val height: Int,
        val selections: List<RemoteSelectionDecoration>,
        val cellSelectionClientIds: Set<String>
    )

    private data class GeometryContext(
        val snapshot: GeometrySnapshot,
        val layout: android.text.Layout,
        val caretWidth: Float
    )

    private var editorView: RichTextEditorView? = null
    private var remoteSelections: List<RemoteSelectionDecoration> = emptyList()
    private var legacyFrame: RemoteSelectionFrame? = null
    private var cachedSnapshot: GeometrySnapshot? = null
    private var cachedGeometry: List<CachedSelectionGeometry> = emptyList()
    private var cellSelectionClientIds: Set<String> = emptySet()
    internal var editorIdOverrideForTesting: Long? = null
    internal var docToScalarResolver: (Long, Int) -> Int = { editorId, docPos ->
        EditorV2Registry.adapterForViewToken(editorId)?.scalarPositionForDoc(docPos) ?: 0
    }
    private val selectionPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
    }
    private val caretPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
    }

    init {
        setWillNotDraw(false)
        isClickable = false
        isFocusable = false
    }

    fun bind(editorView: RichTextEditorView) {
        this.editorView = editorView
        invalidateGeometry()
    }

    fun setRemoteSelections(selections: List<RemoteSelectionDecoration>) {
        val frame = installedFrame()
        if (remoteSelections == selections && legacyFrame == frame) return
        legacyFrame = frame
        remoteSelections = selections
        invalidateGeometry()
        refreshGeometry()
    }

    private fun installedFrame(): RemoteSelectionFrame? = editorView?.let { view ->
        RemoteSelectionFrame.installed(EditorV2Registry.adapterForViewToken(resolvedEditorId(view)))
    }

    private fun currentSelections(): List<RemoteSelectionDecoration> {
        val frame = installedFrame()
        val presentedRevision = editorView?.editorTableSurface?.presentedDocumentRevision?.toString()
        val hasTables = editorView?.let { view ->
            EditorV2Registry.adapterForViewToken(resolvedEditorId(view))?.tableIndex?.tableKeys?.isNotEmpty()
        } == true
        return remoteSelections.filter { selection ->
            if (selection.resolvedAt == null && selection.cellRectangle == null) true
            else (selection.resolvedAt ?: legacyFrame)?.let {
                it == frame && editorView?.editorEditText?.lastAppliedDocumentVersion == it.documentRevision &&
                    (selection.cellRectangle == null || !hasTables || it.documentRevision == presentedRevision)
            } ?: false
        }
    }

    fun refreshGeometry() {
        if (legacyFrame == null) legacyFrame = installedFrame()
        presentRemoteCellSelections()
        ensureGeometry()
        invalidate()
    }

    private fun presentRemoteCellSelections() {
        val editorView = editorView ?: return
        val editorId = resolvedEditorId(editorView)
        val index = EditorV2Registry.adapterForViewToken(editorId)?.tableIndex ?: EditorTableIndex()
        val drawable = if (editorId == 0L) emptyList() else currentSelections().mapNotNull { selection ->
            val rectangle = selection.cellRectangle ?: return@mapNotNull null
            val cells = resolveEditorCellSelection(rectangle.anchorCell, rectangle.headCell, index)
                as? EditorCellSelection.Drawable ?: return@mapNotNull null
            selection.clientId to RemoteTableCellSelection(
                cells.tableId,
                cells.sourceIndices,
                withAlpha(selection.color, SELECTION_ALPHA)
            )
        }
        cellSelectionClientIds = drawable.map { it.first }.toSet()
        editorView.editorTableSurface.presentRemoteCellSelections(drawable.map { it.second })
    }

    fun hasSelectionsOrCachedGeometry(): Boolean =
        remoteSelections.isNotEmpty() || cachedGeometry.isNotEmpty()

    fun debugSnapshotsForTesting(): List<RemoteSelectionDebugSnapshot> =
        ensureGeometry().map { geometry ->
            RemoteSelectionDebugSnapshot(
                clientId = geometry.clientId,
                caretRect = geometry.caretRect?.let(::RectF)
            )
        }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val geometry = ensureGeometry()
        if (geometry.isEmpty()) return

        for (entry in geometry) {
            entry.selectionPath?.let { path ->
                selectionPaint.color = entry.selectionColor
                canvas.drawPath(path, selectionPaint)
            }

            entry.caretRect?.let { caretRect ->
                caretPaint.color = entry.caretColor
                val cornerRadius = maxOf(1f, caretRect.width() / 2f)
                canvas.drawRoundRect(
                    caretRect.left,
                    caretRect.top,
                    caretRect.right,
                    caretRect.bottom,
                    cornerRadius,
                    cornerRadius,
                    caretPaint
                )
            }
        }
    }

    private fun ensureGeometry(): List<CachedSelectionGeometry> {
        val context = buildGeometryContext() ?: run {
            cachedSnapshot = null
            cachedGeometry = emptyList()
            return emptyList()
        }
        if (cachedSnapshot == context.snapshot) {
            return cachedGeometry
        }

        val text = context.snapshot.text
        val editorId = context.snapshot.editorId
        val textSelections = context.snapshot.selections.filter { it.clientId !in context.snapshot.cellSelectionClientIds }
        val geometry = textSelections.map { selection ->
            val startDoc = minOf(selection.anchor, selection.head)
            val endDoc = maxOf(selection.anchor, selection.head)
            val startScalar = docToScalarResolver(editorId, startDoc)
            val endScalar = docToScalarResolver(editorId, endDoc)
            val startUtf16 = PositionBridge.scalarToUtf16(
                startScalar,
                text
            ).coerceIn(0, text.length)
            val endUtf16 = PositionBridge.scalarToUtf16(endScalar, text).coerceIn(0, text.length)

            val selectionPath = if (startUtf16 != endUtf16) {
                Path().apply {
                    context.layout.getSelectionPath(startUtf16, endUtf16, this)
                    offset(context.snapshot.baseX.toFloat(), context.snapshot.baseY.toFloat())
                }
            } else {
                null
            }

            CachedSelectionGeometry(
                clientId = selection.clientId,
                selectionPath = selectionPath,
                selectionColor = withAlpha(selection.color, SELECTION_ALPHA),
                caretRect = caretRectForOffset(
                    endUtf16 = endUtf16,
                    textLength = text.length,
                    layout = context.layout,
                    baseX = context.snapshot.baseX.toFloat(),
                    baseY = context.snapshot.baseY.toFloat(),
                    caretWidth = context.caretWidth,
                    isFocused = selection.isFocused
                ),
                caretColor = selection.color
            )
        }

        cachedSnapshot = context.snapshot
        cachedGeometry = geometry
        return geometry
    }

    private fun buildGeometryContext(): GeometryContext? {
        val editorView = editorView ?: return null
        val editorId = resolvedEditorId(editorView)
        val selections = currentSelections()
        if (editorId == 0L || selections.isEmpty()) return null

        val editText = editorView.editorEditText
        val layout = editText.layout ?: return null
        val text = editText.text?.toString() ?: return null
        val baseX =
            editorView.editorViewport.left + editorView.editorScrollView.left + editText.left +
                editText.compoundPaddingLeft
        val baseY = editorView.editorViewport.top + editorView.editorScrollView.top + editText.top +
            editText.compoundPaddingTop - editorView.editorScrollView.scrollY
        val caretWidth = maxOf(2f, resources.displayMetrics.density)

        return GeometryContext(
            snapshot = GeometrySnapshot(
                editorId = editorId,
                text = text,
                layoutWidth = layout.width,
                layoutHeight = layout.height,
                baseX = baseX,
                baseY = baseY,
                width = width,
                height = height,
                selections = selections,
                cellSelectionClientIds = cellSelectionClientIds
            ),
            layout = layout,
            caretWidth = caretWidth
        )
    }

    private fun invalidateGeometry() {
        cachedSnapshot = null
    }

    private fun caretRectForOffset(
        endUtf16: Int,
        textLength: Int,
        layout: android.text.Layout,
        baseX: Float,
        baseY: Float,
        caretWidth: Float,
        isFocused: Boolean
    ): RectF? {
        if (!isFocused) return null

        val clampedOffset = endUtf16.coerceIn(0, textLength)
        val line = layout.getLineForOffset(clampedOffset)
        val horizontal = layout.getPrimaryHorizontal(clampedOffset)
        val caretLeft = baseX + horizontal
        val caretTop = baseY + layout.editorTextLineTop(line)
        val caretBottom = baseY + layout.editorTextLineBottom(line)
        return RectF(caretLeft, caretTop, caretLeft + caretWidth, caretBottom)
    }

    private fun withAlpha(color: Int, alphaFraction: Float): Int {
        val alpha = (255f * alphaFraction).toInt().coerceIn(0, 255)
        return Color.argb(alpha, Color.red(color), Color.green(color), Color.blue(color))
    }

    private fun resolvedEditorId(editorView: RichTextEditorView): Long =
        editorIdOverrideForTesting ?: editorView.editorId

    internal companion object {
        const val SELECTION_ALPHA = 0.18f
    }
}
