package com.apollohg.editor

import org.json.JSONObject

internal fun editorPerformanceContentInsetPx(style: JSONObject, density: Float): Int =
    (style.getDouble("textInset").toFloat() * density).toInt()
