package com.apollohg.editor

import android.graphics.Canvas
import android.graphics.Paint
import android.graphics.Path
import android.graphics.RectF
import android.text.style.ReplacementSpan

internal fun resolvedCheckboxStyle(
    sheet: EditorStyleSheet,
    checked: Boolean,
    ancestors: List<String> = emptyList()
): EditorElementStyle {
    val resolved = sheet.resolveElement("taskCheckbox", ancestors)
    val base = EditorElementStyle(
        EditorTextStyle(),
        EditorBoxStyle(
            border = EditorEdges(1f, 1f, 1f, 1f),
            corners = EditorCorners(3f, 3f, 3f, 3f)
        ),
        size = 18f,
        gap = 6f
    )
        .mergedWith(resolved)
    return if (checked) base.mergedWith(resolved?.checked) else base
}

internal fun drawCheckbox(
    canvas: Canvas,
    bounds: RectF,
    box: EditorBoxStyle,
    checked: Boolean,
    checkColor: Int
) {
    EditorBoxDrawing.draw(canvas, bounds, box)
    if (!checked) return
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = checkColor
        style = Paint.Style.STROKE
        strokeWidth =
            maxOf(1f, bounds.width() * 0.1f)
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }
    val path = Path().apply {
        moveTo(bounds.left + bounds.width() * .22f, bounds.top + bounds.height() * .52f)
        lineTo(bounds.left + bounds.width() * .43f, bounds.top + bounds.height() * .72f)
        lineTo(bounds.left + bounds.width() * .8f, bounds.top + bounds.height() * .28f)
    }
    canvas.drawPath(path, paint)
}

internal fun drawTaskCheckbox(
    canvas: Canvas,
    bounds: RectF,
    boxStyle: EditorBoxStyle?,
    checked: Boolean,
    color: Int,
    checkColor: Int = color
) {
    if (boxStyle != null) {
        drawCheckbox(canvas, bounds, boxStyle, checked, checkColor)
        return
    }
    val inset = maxOf(1f, bounds.height() * 0.2f)
    val box = RectF(bounds).apply { inset(inset, inset) }
    val paint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = maxOf(1f, box.width() * 0.1f)
        this.color = color
    }
    canvas.drawRoundRect(box, box.width() * 0.2f, box.width() * 0.2f, paint)
    if (!checked) return
    paint.strokeWidth = maxOf(1.4f, box.width() * 0.12f)
    paint.strokeCap = Paint.Cap.ROUND
    paint.strokeJoin = Paint.Join.ROUND
    val path = Path().apply {
        moveTo(box.left + box.width() * 0.2f, box.centerY())
        lineTo(box.left + box.width() * 0.43f, box.bottom - box.height() * 0.2f)
        lineTo(box.right - box.width() * 0.16f, box.top + box.height() * 0.2f)
    }
    canvas.drawPath(path, paint)
}

internal class EditorCheckboxSpan(
    private val size: Int,
    private val box: EditorBoxStyle?,
    private val checked: Boolean,
    private val checkColor: Int?,
    private val markerColor: Int? = null,
    private val reserveLineHeight: Boolean = true
) : ReplacementSpan() {
    constructor(style: EditorElementStyle, checked: Boolean, density: Float) : this(
        ((style.size ?: 18f) * density).toInt(),
        style.box.scaled(density),
        checked,
        style.checkColor
    )
    override fun getSize(
        paint: Paint,
        text: CharSequence,
        start: Int,
        end: Int,
        fm: Paint.FontMetricsInt?
    ): Int {
        fm?.takeIf { reserveLineHeight }?.let {
            it.ascent = minOf(it.ascent, -size)
            it.top = it.ascent
        }
        return size
    }
    override fun draw(
        canvas: Canvas,
        text: CharSequence,
        start: Int,
        end: Int,
        x: Float,
        top: Int,
        y: Int,
        bottom: Int,
        paint: Paint
    ) {
        val center = y + (paint.fontMetrics.ascent + paint.fontMetrics.descent) / 2f
        drawTaskCheckbox(
            canvas,
            RectF(x, center - size / 2f, x + size, center + size / 2f),
            box,
            checked,
            markerColor ?: paint.color,
            checkColor ?: markerColor ?: paint.color
        )
    }
}
