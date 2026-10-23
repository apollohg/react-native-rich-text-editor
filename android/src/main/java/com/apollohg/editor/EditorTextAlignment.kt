package com.apollohg.editor

internal enum class EditorTextAlignment(val wireValue: String) {
    LEFT("left"),
    CENTER("center"),
    RIGHT("right");

    companion object {
        fun fromWireValue(value: String): EditorTextAlignment? =
            entries.firstOrNull { it.wireValue == value }
    }
}
