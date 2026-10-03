package com.apollohg.editor.tables

import android.content.Context
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.View
import android.view.ViewConfiguration
import android.widget.OverScroller
import androidx.core.view.ViewCompat
import kotlin.math.abs
import kotlin.math.roundToInt

internal enum class TableGestureAxis { UNDECIDED, HORIZONTAL, VERTICAL }

internal class TableGestureAxisLock(context: Context) {
    private companion object {
        const val HORIZONTAL_BIAS = 1.25f
    }
    private val touchSlop = ViewConfiguration.get(context).scaledTouchSlop.toFloat()
    var axis = TableGestureAxis.UNDECIDED
        private set

    fun reset() {
        axis = TableGestureAxis.UNDECIDED
    }

    fun update(dx: Float, dy: Float, canConsume: () -> Boolean): TableGestureAxis {
        if (axis != TableGestureAxis.UNDECIDED) return axis
        if (dx * dx + dy * dy <= touchSlop * touchSlop) return axis
        axis = if (abs(dx) > HORIZONTAL_BIAS * abs(dy) && canConsume()) {
            TableGestureAxis.HORIZONTAL
        } else {
            TableGestureAxis.VERTICAL
        }
        return axis
    }
}

internal class TableInteractionController(
    context: Context,
    private val view: View,
    private val owner: () -> ViewerTablePresentationOwner,
    private val snapshot: () -> ViewerTablePresentationSnapshot?,
    private val offsetChanged: () -> Unit,
    private val startNested: (Int) -> Boolean,
    private val stopNested: (Int) -> Unit,
    private val preScroll: (Int, IntArray, IntArray, Int) -> Boolean,
    private val postScroll: (Int, Int, IntArray, IntArray, Int) -> Unit,
    private val preFling: (Float) -> Boolean,
    private val fling: (Float, Boolean) -> Boolean
) {
    private companion object {
        const val VELOCITY_UNITS = 1000
    }

    private val axisLock = TableGestureAxisLock(context)
    private val minimumFlingVelocity = ViewConfiguration.get(context).scaledMinimumFlingVelocity
    private val maximumFlingVelocity = ViewConfiguration.get(context).scaledMaximumFlingVelocity
    private val scroller = OverScroller(context)
    private var velocity: VelocityTracker? = null
    private var pointerId = MotionEvent.INVALID_POINTER_ID
    private var downX = 0f
    private var downY = 0f
    private var lastX = 0f
    private var surfaces = emptyList<ViewerTableSurface>()
    private var flingSurfaces = emptyList<ViewerTableSurface>()
    private var lastFlingX = 0
    private var nestedWindowX = 0
    private var gestureGeneration = 0L
    private val stoppingNestedTypes = mutableSetOf<Int>()

    val ownsHorizontalGesture: Boolean get() = axisLock.axis == TableGestureAxis.HORIZONTAL
    val tracksGesture: Boolean get() = pointerId != MotionEvent.INVALID_POINTER_ID

    fun hasTableAt(x: Float, y: Float): Boolean = surfacesAt(x, y).isNotEmpty()

    fun canConsumeAt(x: Float, y: Float, dx: Float): Boolean =
        surfacesAt(x, y).any { owner().canConsumePhysical(-dx, it) }

    fun onTouch(event: MotionEvent, contentX: Float, contentY: Float): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                resetGesture()
                finishFling()
                surfaces = surfacesAt(contentX, contentY)
                if (surfaces.isEmpty() || event.pointerCount != 1) return false
                pointerId = event.getPointerId(0)
                downX = event.x
                downY = event.y
                lastX = downX
                nestedWindowX = 0
                velocity = VelocityTracker.obtain().also { it.addMovement(event) }
                return true
            }

            MotionEvent.ACTION_POINTER_DOWN,
            MotionEvent.ACTION_POINTER_UP,
            MotionEvent.ACTION_CANCEL -> {
                val owned = ownsHorizontalGesture
                resetGesture()
                return owned
            }

            MotionEvent.ACTION_MOVE -> {
                if (pointerId == MotionEvent.INVALID_POINTER_ID) return false
                val generation = gestureGeneration
                val index = event.findPointerIndex(pointerId)
                if (index < 0) {
                    val owned = ownsHorizontalGesture
                    resetGesture()
                    return owned
                }
                val x = event.getX(index)
                val y = event.getY(index)
                if (axisLock.axis == TableGestureAxis.UNDECIDED) {
                    val dx = x - downX
                    val dy = y - downY
                    val axis = axisLock.update(dx, dy) {
                        surfaces.any { owner().canConsumePhysical(-dx, it) }
                    }
                    if (generation != gestureGeneration) return true
                    if (axis == TableGestureAxis.UNDECIDED) {
                        addVelocity(event)
                        return true
                    }
                    if (axis == TableGestureAxis.HORIZONTAL) {
                        startNested(ViewCompat.TYPE_TOUCH)
                        if (generation != gestureGeneration) return true
                        view.parent?.requestDisallowInterceptTouchEvent(true)
                        if (generation != gestureGeneration) return true
                    }
                }
                if (axisLock.axis != TableGestureAxis.HORIZONTAL) {
                    addVelocity(event)
                    return false
                }
                val delta = (lastX - x).roundToInt()
                val consumed = IntArray(2)
                val preOffset = IntArray(2)
                preScroll(delta, consumed, preOffset, ViewCompat.TYPE_TOUCH)
                if (generation != gestureGeneration) return true
                val local = scrollLocal((delta - consumed[0]).toFloat())
                if (generation != gestureGeneration) return true
                val remaining = delta - consumed[0] - local.roundToInt()
                val postConsumed = IntArray(2)
                val postOffset = IntArray(2)
                postScroll(
                    local.roundToInt(),
                    remaining,
                    postConsumed,
                    postOffset,
                    ViewCompat.TYPE_TOUCH
                )
                if (generation != gestureGeneration) return true
                val windowShift = preOffset[0] + postOffset[0]
                nestedWindowX += windowShift
                lastX = x - windowShift
                addVelocity(event)
                return true
            }

            MotionEvent.ACTION_UP -> {
                val owned = ownsHorizontalGesture
                if (owned) {
                    val generation = gestureGeneration
                    addVelocity(event)
                    velocity?.computeCurrentVelocity(VELOCITY_UNITS, maximumFlingVelocity.toFloat())
                    val horizontalVelocity = -(velocity?.getXVelocity(pointerId) ?: 0f)
                    if (abs(horizontalVelocity) >= minimumFlingVelocity &&
                        !preFling(horizontalVelocity) &&
                        generation == gestureGeneration
                    ) {
                        val canConsume = surfaces.any {
                            owner().canConsumePhysical(horizontalVelocity, it)
                        }
                        fling(horizontalVelocity, canConsume)
                        if (generation != gestureGeneration) return true
                        if (canConsume) {
                            flingSurfaces = surfaces
                            scroller.fling(
                                0,
                                0,
                                horizontalVelocity.roundToInt(),
                                0,
                                -Int.MAX_VALUE,
                                Int.MAX_VALUE,
                                0,
                                0
                            )
                            lastFlingX = 0
                            startNested(ViewCompat.TYPE_NON_TOUCH)
                            if (generation != gestureGeneration) return true
                            view.postInvalidateOnAnimation()
                        }
                    }
                }
                resetGesture()
                return owned
            }
        }
        return false
    }

    fun computeScroll() {
        if (scroller.isFinished) {
            if (flingSurfaces.isNotEmpty()) finishFling()
            return
        }
        if (!scroller.computeScrollOffset()) {
            finishFling()
            return
        }
        val generation = gestureGeneration
        val current = scroller.currX
        val delta = current - lastFlingX
        lastFlingX = current
        val consumed = IntArray(2)
        val windowOffset = IntArray(2)
        preScroll(delta, consumed, windowOffset, ViewCompat.TYPE_NON_TOUCH)
        if (generation != gestureGeneration) return
        val local = scrollLocal((delta - consumed[0]).toFloat(), flingSurfaces).roundToInt()
        if (generation != gestureGeneration) return
        val remaining = delta - consumed[0] - local
        val postConsumed = IntArray(2)
        postScroll(
            local,
            remaining,
            postConsumed,
            windowOffset,
            ViewCompat.TYPE_NON_TOUCH
        )
        if (generation != gestureGeneration) return
        if (remaining != 0 && postConsumed[0] == 0) {
            startNested(ViewCompat.TYPE_TOUCH)
            if (generation != gestureGeneration) return
            fling(scroller.currVelocity * remaining.sign(), false)
            stopNestedSafely(ViewCompat.TYPE_TOUCH)
            if (generation == gestureGeneration) finishFling()
        } else {
            view.postInvalidateOnAnimation()
        }
    }

    fun cancel() {
        resetGesture()
        finishFling()
    }

    private fun resetGesture() {
        val owned = axisLock.axis == TableGestureAxis.HORIZONTAL
        velocity?.recycle()
        velocity = null
        pointerId = MotionEvent.INVALID_POINTER_ID
        gestureGeneration++
        val generation = gestureGeneration
        axisLock.reset()
        surfaces = emptyList()
        if (owned) view.parent?.requestDisallowInterceptTouchEvent(false)
        if (generation == gestureGeneration) stopNestedSafely(ViewCompat.TYPE_TOUCH)
    }

    private fun finishFling() {
        scroller.abortAnimation()
        flingSurfaces = emptyList()
        stopNestedSafely(ViewCompat.TYPE_NON_TOUCH)
        stopNestedSafely(ViewCompat.TYPE_TOUCH)
    }

    private fun stopNestedSafely(type: Int) {
        if (!stoppingNestedTypes.add(type)) return
        try {
            stopNested(type)
        } finally {
            stoppingNestedTypes.remove(type)
        }
    }

    private fun scrollLocal(delta: Float, targets: List<ViewerTableSurface> = surfaces): Float {
        var remaining = delta
        targets.forEach { surface ->
            remaining -= owner().scrollPhysical(remaining, surface)
        }
        if (remaining != delta) offsetChanged()
        return delta - remaining
    }

    private fun surfacesAt(x: Float, y: Float): List<ViewerTableSurface> =
        snapshot()?.tables.orEmpty().asReversed().filter { presented ->
            presented.bounds.contains(x, y) && presented.clip.contains(x, y)
        }.map { it.surface }.distinct()

    private fun Int.sign(): Float = if (this < 0) -1f else 1f

    private fun addVelocity(event: MotionEvent) {
        val adjusted = MotionEvent.obtain(event)
        adjusted.offsetLocation(nestedWindowX.toFloat(), 0f)
        velocity?.addMovement(adjusted)
        adjusted.recycle()
    }
}
