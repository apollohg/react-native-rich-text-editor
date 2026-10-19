package com.apollohg.editor

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.RectF
import android.os.Build
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver

internal class EditorCaretOverlayView(context: Context, private val input: EditorEditText) :
    PointerTransparentView(context, null, 0),
    ViewTreeObserver.OnPreDrawListener {
    private var observer: ViewTreeObserver? = null
    private var caretBounds: RectF? = null
    private var caretColor = Color.TRANSPARENT
    private var caretVisible = false
    private val caretInvalidation: () -> Boolean = {
        invalidate()
        input.isCaretDrawnSeparately && isShown
    }

    init {
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        observer = viewTreeObserver.also { it.addOnPreDrawListener(this) }
        if (input.onCaretInvalidation == null) {
            input.onCaretInvalidation = caretInvalidation
        }
    }

    override fun onDetachedFromWindow() {
        observer?.takeIf { it.isAlive }?.removeOnPreDrawListener(this)
        observer = null
        disarm()
        super.onDetachedFromWindow()
    }

    override fun onPreDraw(): Boolean {
        refresh()
        return true
    }

    private fun refresh() {
        if (parent !== input.parent || !input.isAttachedToWindow || !input.isShown ||
            !isShown
        ) {
            disarm()
            return
        }
        if (input.onCaretInvalidation == null) {
            input.onCaretInvalidation = caretInvalidation
        }
        if (input.onCaretInvalidation !== caretInvalidation) {
            disarm()
            return
        }
        if (left != input.left || top != input.top || right != input.right ||
            bottom != input.bottom
        ) {
            layout(input.left, input.top, input.right, input.bottom)
        }
        if (scrollX != input.scrollX || scrollY != input.scrollY) {
            scrollTo(input.scrollX, input.scrollY)
        }
        input.updateAtomBoundaryCursorVisibility()
        val bounds = input.caretDrawingBounds(includeBlink = false)
        val separate = canDrawSeparately(bounds)
        if (input.isCaretDrawnSeparately != separate) {
            input.isCaretDrawnSeparately = separate
            input.invalidate()
            invalidate()
        }
        val visible = separate && input.isCaretBlinkVisible && bounds != null
        val color = input.caretColor
        if (caretBounds != bounds || caretColor != color || caretVisible != visible) {
            caretBounds = bounds
            caretColor = color
            caretVisible = visible
            invalidate()
        }
    }

    private fun disarm() {
        var changed = caretBounds != null || caretVisible
        if (input.onCaretInvalidation === caretInvalidation) {
            input.onCaretInvalidation = null
            if (input.isCaretDrawnSeparately) {
                input.isCaretDrawnSeparately = false
                input.invalidate()
                changed = true
            }
        }
        caretBounds = null
        caretVisible = false
        if (changed) invalidate()
    }

    private fun canDrawSeparately(bounds: RectF?): Boolean {
        if (!isHardwareAccelerated || !input.isHardwareAccelerated ||
            input.layerType != View.LAYER_TYPE_NONE || !input.matrix.isIdentity ||
            input.animation != null || (parent as? ViewGroup)?.layoutAnimation != null ||
            input.alpha != 1f || input.elevation != 0f || input.translationZ != 0f ||
            input.clipBounds != null || input.clipToOutline || input.foreground != null ||
            input.hasAccessedOwnOverlay || input.scrollIndicators != 0 ||
            input.isHorizontalScrollBarEnabled || resources.configuration.isScreenRound ||
            !input.isEditable
        ) {
            return false
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !input.isInTouchMode &&
            input.defaultFocusHighlightEnabled
        ) {
            return false
        }
        if (input.placeholderText.isNotEmpty()) {
            if (input.coreReportedDocumentIsEmpty == null &&
                input.externalTextComposition?.latestText.isNullOrEmpty()
            ) {
                return false
            }
            if (input.shouldDisplayPlaceholder()) return false
        }
        val layout = input.layout as? EditorDocumentLayout ?: return false
        if (layout.hasPostCaretDecoration) return false
        if (bounds != null && input.isVerticalScrollBarEnabled) {
            val viewport = RectF(bounds).apply {
                offset(
                    (input.compoundPaddingLeft - input.scrollX).toFloat(),
                    (input.compoundPaddingTop - input.scrollY).toFloat()
                )
            }
            val position = input.verticalScrollbarPosition
            val onLeft = position == View.SCROLLBAR_POSITION_LEFT ||
                (
                    position == View.SCROLLBAR_POSITION_DEFAULT &&
                        input.layoutDirection == View.LAYOUT_DIRECTION_RTL
                    )
            val size = input.verticalScrollbarWidth
            // Include inside/outside styles and tracks even without scroll overflow.
            val edge = if (onLeft) {
                input.paddingLeft + size
            } else {
                input.width -
                    input.paddingRight - size
            }
            if (if (onLeft) viewport.left < edge else viewport.right > edge) return false
        }
        return true
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (!canvas.isHardwareAccelerated || !input.isCaretDrawnSeparately ||
            input.onCaretInvalidation !== caretInvalidation
        ) {
            return
        }
        val saved = canvas.save()
        canvas.translate(input.compoundPaddingLeft.toFloat(), input.compoundPaddingTop.toFloat())
        input.drawCaret(canvas)
        canvas.restoreToCount(saved)
    }
}
