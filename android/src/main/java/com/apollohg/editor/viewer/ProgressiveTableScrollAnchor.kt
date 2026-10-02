package com.apollohg.editor.viewer

import android.graphics.Rect
import android.view.View
import android.widget.ScrollView
import androidx.core.widget.NestedScrollView

internal fun progressiveTableViewportHeight(view: View): Int {
    if (!view.isAttachedToWindow) return 0
    var parent = view.parent
    var height = 0
    while (parent is View) {
        if (parent is ScrollView || parent is NestedScrollView) {
            if (height == 0) height = parent.height.takeIf { it > 0 } ?: parent.measuredHeight
        } else if (parent.scrollY != 0 || parent.canScrollVertically(-1) || parent.canScrollVertically(1)) {
            return 0
        }
        parent = parent.parent
    }
    return height.takeIf { it > 0 } ?: view.rootView.height.takeIf { it > 0 }
        ?: view.resources.displayMetrics.heightPixels.coerceAtLeast(0)
}

internal class ProgressiveTableScrollAnchor private constructor(
    private val view: View,
    private val scroll: View,
    private val anchor: ProgressiveTableAnchor,
    private val screenY: Int
) {
    fun restore(layout: PreparedProseLayout) {
        if (!view.isAttachedToWindow || !scroll.isAttachedToWindow) return
        val location = IntArray(2).also(view::getLocationOnScreen)
        val delta = location[1] + anchor.resolve(layout) - screenY
        if (delta != 0) scroll.scrollBy(0, delta)
    }

    companion object {
        fun capture(view: View, layout: PreparedProseLayout): ProgressiveTableScrollAnchor? {
            var parent = view.parent
            while (parent is View && parent !is ScrollView && parent !is NestedScrollView) parent = parent.parent
            val scroll = parent as? View ?: return null
            val visible = Rect()
            if (!view.getLocalVisibleRect(visible)) return null
            val location = IntArray(2).also(view::getLocationOnScreen)
            return ProgressiveTableScrollAnchor(view, scroll, ProgressiveTableAnchor.capture(layout, visible.top),
                location[1] + visible.top)
        }
    }
}
