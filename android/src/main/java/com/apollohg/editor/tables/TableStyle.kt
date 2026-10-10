package com.apollohg.editor.tables

import com.apollohg.editor.optNullableFloat
import com.apollohg.editor.optNullableString
import com.apollohg.editor.parseColor
import org.json.JSONObject

data class TableStyle(
    val minColumnWidth: Float = 80f,
    val cellPadding: Float = 8f,
    val borderWidth: Float = 1f,
    val borderColor: Int = 0xFFD1D5DB.toInt(),
    val headerBackgroundColor: Int = 0xFFF3F4F6.toInt(),
    val selectionColor: Int = 0x333B82F6,
    val resizeHandleColor: Int = 0xFF3B82F6.toInt()
) {
    fun isValid() = minColumnWidth.isFinite() && minColumnWidth > 0f &&
        cellPadding.isFinite() && cellPadding >= 0f && borderWidth.isFinite() && borderWidth >= 0f

    companion object {
        fun fromJson(json: JSONObject?): TableStyle? {
            if (json == null) return null
            val defaults = TableStyle()
            return TableStyle(
                minColumnWidth = json.optNullableFloat("minColumnWidth") ?: defaults.minColumnWidth,
                cellPadding = json.optNullableFloat("cellPadding") ?: defaults.cellPadding,
                borderWidth = json.optNullableFloat("borderWidth") ?: defaults.borderWidth,
                borderColor =
                    parseColor(json.optNullableString("borderColor")) ?: defaults.borderColor,
                headerBackgroundColor = parseColor(json.optNullableString("headerBackgroundColor"))
                    ?: defaults.headerBackgroundColor,
                selectionColor = parseColor(json.optNullableString("selectionColor"))
                    ?: defaults.selectionColor,
                resizeHandleColor = parseColor(json.optNullableString("resizeHandleColor"))
                    ?: defaults.resizeHandleColor
            ).takeIf { it.isValid() }
        }
    }
}

internal fun TableStyle.physical(scale: Float): TableStyle {
    val unit = scale.takeIf { it.isFinite() && it > 0f } ?: 1f
    return copy(
        minColumnWidth = minColumnWidth * unit,
        cellPadding = cellPadding * unit,
        borderWidth =
            borderWidth * unit
    )
}
