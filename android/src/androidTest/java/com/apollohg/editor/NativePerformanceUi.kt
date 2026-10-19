package com.apollohg.editor

import android.app.Instrumentation
import android.view.View

internal fun <T> Instrumentation.runOnMainSyncWithResult(block: () -> T): T {
    var result: Result<T>? = null
    runOnMainSync { result = runCatching(block) }
    return requireNotNull(result).getOrThrow()
}

internal fun layoutView(view: View, widthPx: Int, heightPx: Int, heightMode: Int) {
    view.measure(
        View.MeasureSpec.makeMeasureSpec(widthPx, View.MeasureSpec.EXACTLY),
        View.MeasureSpec.makeMeasureSpec(heightPx, heightMode)
    )
    view.layout(0, 0, view.measuredWidth, view.measuredHeight)
}
