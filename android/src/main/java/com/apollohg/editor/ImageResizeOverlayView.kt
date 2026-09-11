package com.apollohg.editor

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Rect
import android.graphics.RectF
import android.os.Build
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.View
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.max

internal class ImageResizeOverlayView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {
    private enum class Corner {
        TOP_LEFT,
        TOP_RIGHT,
        BOTTOM_LEFT,
        BOTTOM_RIGHT
    }

    private data class DragState(
        val corner: Corner,
        val originalRect: RectF,
        val docPos: Int,
        val span: BlockImageSpan,
        val downX: Float,
        val downY: Float,
        val originalSize: Pair<Int, Int>,
        val fixedWidthPx: Float,
        val fixedHeightPx: Float,
        val maximumWidthPx: Float,
        var previewRect: RectF,
        var previewContentSize: Pair<Float, Float>
    )

    private var editorView: RichTextEditorView? = null
    private var currentGeometry: EditorEditText.SelectedImageGeometry? = null
    private var dragState: DragState? = null

    private val density = resources.displayMetrics.density
    private val handleRadiusPx = 10f * density
    private val handleTouchRadiusPx = 24f * density
    private val minimumImageSizePx = 48f * density
    private val borderPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#0A84FF")
        style = Paint.Style.STROKE
        strokeWidth = max(2f, density)
    }
    private val handleFillPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        style = Paint.Style.FILL
    }
    private val handleStrokePaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.parseColor("#0A84FF")
        style = Paint.Style.STROKE
        strokeWidth = max(2f, density)
    }

    init {
        setWillNotDraw(false)
        visibility = INVISIBLE
    }

    fun bind(editorView: RichTextEditorView) {
        if (this.editorView !== editorView) cancelActiveResize()
        this.editorView = editorView
    }

    fun refresh() {
        val nextGeometry = editorView?.selectedImageGeometry()
        if (nextGeometry == null) {
            cancelActiveResize()
            return
        }
        val selectedSpan = editorView?.selectedImageSpanForResize()
        if (currentGeometry?.docPos != nextGeometry.docPos ||
            dragState?.span?.let { it !== selectedSpan } == true
        ) {
            cancelActiveResize()
        }
        currentGeometry = nextGeometry
        visibility = VISIBLE
        bringToFront()
        updateSystemGestureExclusionRects()
        invalidate()
    }

    fun cancelActiveResize() {
        dragState?.let { state -> editorView?.restoreImageResizePreview(state.span) }
        dragState = null
        currentGeometry = null
        parent?.requestDisallowInterceptTouchEvent(false)
        visibility = INVISIBLE
        updateSystemGestureExclusionRects()
        invalidate()
    }

    fun visibleRectForTesting(): RectF? = currentGeometry?.rect?.let(::RectF)

    fun simulateResizeForTesting(widthPx: Float, heightPx: Float) {
        val geometry = currentGeometry ?: return
        editorView?.resizeImage(geometry.docPos, widthPx, heightPx)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val geometry = currentGeometry ?: return
        canvas.drawRoundRect(geometry.rect, 8f * density, 8f * density, borderPaint)
        for (corner in Corner.entries) {
            val center = handleCenter(corner, geometry.rect)
            canvas.drawCircle(center.x, center.y, handleRadiusPx, handleFillPaint)
            canvas.drawCircle(center.x, center.y, handleRadiusPx, handleStrokePaint)
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        val geometry = currentGeometry ?: return false

        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                val corner = cornerAt(event.x, event.y, geometry.rect) ?: return false
                val span = editorView?.selectedImageSpanForResize() ?: return false
                dragState = DragState(
                    corner = corner,
                    originalRect = RectF(geometry.rect),
                    docPos = geometry.docPos,
                    span = span,
                    downX = event.x,
                    downY = event.y,
                    originalSize = span.currentSizePx(),
                    fixedWidthPx = geometry.rect.width() - span.currentSizePx().first,
                    fixedHeightPx = geometry.rect.height() - span.currentSizePx().second,
                    maximumWidthPx = editorView?.maximumImageWidthPx() ?: geometry.rect.width(),
                    previewRect = RectF(geometry.rect),
                    previewContentSize = span.currentSizePx().let {
                        it.first.toFloat() to
                            it.second.toFloat()
                    }
                )
                parent?.requestDisallowInterceptTouchEvent(true)
                return true
            }

            MotionEvent.ACTION_MOVE -> {
                val state = dragState ?: return false
                updatePreview(state, event.x, event.y)
                return true
            }

            MotionEvent.ACTION_UP, MotionEvent.ACTION_CANCEL -> {
                val state = dragState ?: return false
                if (event.actionMasked == MotionEvent.ACTION_UP) {
                    updatePreview(state, event.x, event.y)
                    val previewSize = state.span.currentSizePx()
                    if (previewSize != state.originalSize) {
                        editorView?.resizeImage(
                            state.docPos,
                            state.previewContentSize.first,
                            state.previewContentSize.second
                        )
                    }
                    editorView?.restoreImageResizePreview(state.span)
                } else {
                    editorView?.restoreImageResizePreview(state.span)
                }
                dragState = null
                parent?.requestDisallowInterceptTouchEvent(false)
                post { refresh() }
                return true
            }
        }

        return false
    }

    private fun updatePreview(state: DragState, x: Float, y: Float) {
        val deltaX = x - state.downX
        val deltaY = y - state.downY
        if (deltaX == 0f && deltaY == 0f) {
            val originalContentSize = state.originalSize.let {
                it.first.toFloat() to it.second.toFloat()
            }
            if (state.previewContentSize == originalContentSize) return
            val boundEditor = editorView ?: return
            if (!boundEditor.previewImageResize(
                    state.span,
                    originalContentSize.first,
                    originalContentSize.second
                )
            ) {
                return
            }
            state.previewRect = RectF(state.originalRect)
            state.previewContentSize = originalContentSize
            currentGeometry = boundEditor.selectedImageGeometry()
                ?.takeIf { it.docPos == state.docPos }
                ?: EditorEditText.SelectedImageGeometry(state.docPos, state.originalRect)
            updateSystemGestureExclusionRects()
            invalidate()
            return
        }
        val (nextRect, contentSize) = resizedRect(
            originalRect = state.originalRect,
            originalContentSize = state.originalSize,
            fixedWidthPx = state.fixedWidthPx,
            fixedHeightPx = state.fixedHeightPx,
            corner = state.corner,
            deltaX = deltaX,
            deltaY = deltaY,
            maximumWidthPx = state.maximumWidthPx
        )
        val boundEditor = editorView ?: return
        if (!boundEditor.previewImageResize(
                state.span,
                contentSize.first,
                contentSize.second
            )
        ) {
            return
        }
        state.previewRect = RectF(nextRect)
        state.previewContentSize = contentSize
        currentGeometry = boundEditor.selectedImageGeometry()
            ?.takeIf { it.docPos == state.docPos }
            ?: EditorEditText.SelectedImageGeometry(state.docPos, nextRect)
        updateSystemGestureExclusionRects()
        invalidate()
    }

    override fun onDetachedFromWindow() {
        cancelActiveResize()
        super.onDetachedFromWindow()
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        updateSystemGestureExclusionRects()
    }

    private fun cornerAt(x: Float, y: Float, rect: RectF): Corner? =
        Corner.entries.firstOrNull { corner ->
            val center = handleCenter(corner, rect)
            val dx = x - center.x
            val dy = y - center.y
            (dx * dx) + (dy * dy) <= handleTouchRadiusPx * handleTouchRadiusPx
        }

    private fun updateSystemGestureExclusionRects() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val geometry = currentGeometry
        val nextRects = if (geometry == null || visibility != VISIBLE || width == 0 ||
            height == 0
        ) {
            emptyList()
        } else {
            Corner.entries.map { corner ->
                val center = handleCenter(corner, geometry.rect)
                Rect(
                    floor(center.x - handleTouchRadiusPx).toInt().coerceIn(0, width),
                    floor(center.y - handleTouchRadiusPx).toInt().coerceIn(0, height),
                    ceil(center.x + handleTouchRadiusPx).toInt().coerceIn(0, width),
                    ceil(center.y + handleTouchRadiusPx).toInt().coerceIn(0, height)
                )
            }
        }
        if (systemGestureExclusionRects != nextRects) {
            systemGestureExclusionRects = nextRects
        }
    }

    private fun handleCenter(corner: Corner, rect: RectF) = when (corner) {
        Corner.TOP_LEFT -> android.graphics.PointF(rect.left, rect.top)
        Corner.TOP_RIGHT -> android.graphics.PointF(rect.right, rect.top)
        Corner.BOTTOM_LEFT -> android.graphics.PointF(rect.left, rect.bottom)
        Corner.BOTTOM_RIGHT -> android.graphics.PointF(rect.right, rect.bottom)
    }

    private fun anchorPoint(corner: Corner, rect: RectF) = when (corner) {
        Corner.TOP_LEFT -> android.graphics.PointF(rect.right, rect.bottom)
        Corner.TOP_RIGHT -> android.graphics.PointF(rect.left, rect.bottom)
        Corner.BOTTOM_LEFT -> android.graphics.PointF(rect.right, rect.top)
        Corner.BOTTOM_RIGHT -> android.graphics.PointF(rect.left, rect.top)
    }

    private fun resizedRect(
        originalRect: RectF,
        originalContentSize: Pair<Int, Int>,
        fixedWidthPx: Float,
        fixedHeightPx: Float,
        corner: Corner,
        deltaX: Float,
        deltaY: Float,
        maximumWidthPx: Float?
    ): Pair<RectF, Pair<Float, Float>> {
        val originalWidth = max(originalContentSize.first.toFloat(), 1f)
        val originalHeight = max(originalContentSize.second.toFloat(), 1f)
        val aspectRatio = max(originalWidth / originalHeight, 0.1f)
        val signedDx = if (corner == Corner.TOP_RIGHT ||
            corner == Corner.BOTTOM_RIGHT
        ) {
            deltaX
        } else {
            -deltaX
        }
        val signedDy = if (corner == Corner.BOTTOM_LEFT ||
            corner == Corner.BOTTOM_RIGHT
        ) {
            deltaY
        } else {
            -deltaY
        }
        val projectedScale = 1f +
            ((signedDx * originalWidth) + (signedDy * originalHeight)) /
            ((originalWidth * originalWidth) + (originalHeight * originalHeight))
        val scale = max(
            projectedScale,
            max(minimumImageSizePx / originalWidth, minimumImageSizePx / originalHeight)
        )
        val unclampedWidth = max(minimumImageSizePx, originalWidth * scale)
        val unclampedHeight = max(minimumImageSizePx / aspectRatio, unclampedWidth / aspectRatio)
        val (width, height) = editorView?.let { boundEditor ->
            maximumWidthPx?.let { maxWidth ->
                boundEditor.clampImageSize(
                    widthPx = unclampedWidth,
                    heightPx = unclampedHeight,
                    maximumWidthPx = (maxWidth - fixedWidthPx).coerceAtLeast(minimumImageSizePx)
                )
            } ?: boundEditor.clampImageSize(unclampedWidth, unclampedHeight)
        } ?: (unclampedWidth to unclampedHeight)
        val anchor = anchorPoint(corner, originalRect)

        val visualWidth = width + fixedWidthPx
        val visualHeight = height + fixedHeightPx
        val rect = when (corner) {
            Corner.TOP_LEFT -> RectF(
                anchor.x - visualWidth,
                anchor.y - visualHeight,
                anchor.x,
                anchor.y
            )

            Corner.TOP_RIGHT -> RectF(
                anchor.x,
                anchor.y - visualHeight,
                anchor.x + visualWidth,
                anchor.y
            )

            Corner.BOTTOM_LEFT -> RectF(
                anchor.x - visualWidth,
                anchor.y,
                anchor.x,
                anchor.y + visualHeight
            )

            Corner.BOTTOM_RIGHT -> RectF(
                anchor.x,
                anchor.y,
                anchor.x + visualWidth,
                anchor.y + visualHeight
            )
        }
        return rect to (width to height)
    }
}
