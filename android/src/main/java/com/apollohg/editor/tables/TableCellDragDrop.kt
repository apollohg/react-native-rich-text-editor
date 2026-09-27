package com.apollohg.editor.tables

import android.graphics.Canvas
import android.graphics.Path
import android.graphics.Point
import android.graphics.RectF
import android.view.View
import kotlin.math.ceil
import kotlin.math.roundToInt
import com.apollohg.editor.EditorClipboardPayload
import com.apollohg.editor.EditorV2Adapter

internal data class TableCellDragSource(
    val tableId: String,
    val anchor: Int,
    val head: Int,
    val sourcePositions: Set<Int>
)

internal class TableCellDragState(
    val editorId: Long,
    val adapter: EditorV2Adapter,
    val documentRevision: ULong,
    val source: TableCellDragSource,
    val payload: EditorClipboardPayload,
    val movable: Boolean
)

internal class TableCellDragShadow(
    private val drawing: View,
    private val cellRects: List<RectF>,
    private val touchX: Float,
    private val touchY: Float
) : View.DragShadowBuilder(drawing) {
    val bounds: RectF = RectF(cellRects.first()).apply { cellRects.drop(1).forEach(::union) }

    override fun onProvideShadowMetrics(outShadowSize: Point, outShadowTouchPoint: Point) {
        val width = ceil(bounds.width()).toInt().coerceAtLeast(MINIMUM_SHADOW_SIZE)
        val height = ceil(bounds.height()).toInt().coerceAtLeast(MINIMUM_SHADOW_SIZE)
        outShadowSize.set(width, height)
        outShadowTouchPoint.set(
            (touchX - bounds.left).roundToInt().coerceIn(0, width),
            (touchY - bounds.top).roundToInt().coerceIn(0, height)
        )
    }

    override fun onDrawShadow(canvas: Canvas) {
        canvas.translate(-bounds.left, -bounds.top)
        canvas.clipPath(Path().apply { cellRects.forEach { addRect(it, Path.Direction.CW) } })
        drawing.draw(canvas)
    }

    private companion object {
        const val MINIMUM_SHADOW_SIZE = 1
    }
}
