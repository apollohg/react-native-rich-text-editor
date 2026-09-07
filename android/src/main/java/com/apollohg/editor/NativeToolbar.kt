package com.apollohg.editor

import kotlin.math.roundToInt
import org.json.JSONObject

internal fun physicalToolbarBorderWidth(widthDp: Float, density: Float): Int = when {
    widthDp <= 0f -> 0
    else -> maxOf(1, (widthDp * density).roundToInt())
}

internal data class NativeToolbarState(
    val marks: Map<String, Boolean>,
    val nodes: Map<String, Boolean>,
    val commands: Map<String, Boolean>,
    val allowedMarks: Set<String>,
    val insertableNodes: Set<String>,
    val canUndo: Boolean,
    val canRedo: Boolean
) {
    companion object {
        val empty = NativeToolbarState(
            marks = emptyMap(),
            nodes = emptyMap(),
            commands = emptyMap(),
            allowedMarks = emptySet(),
            insertableNodes = emptySet(),
            canUndo = false,
            canRedo = false
        )

        fun fromUpdateJson(updateJson: String): NativeToolbarState? {
            val root = try {
                JSONObject(updateJson)
            } catch (_: Exception) {
                return null
            }
            val activeState = root.optJSONObject("activeState") ?: JSONObject()
            val historyState = root.optJSONObject("historyState") ?: JSONObject()
            return NativeToolbarState(
                marks = boolMap(activeState.optJSONObject("marks")),
                nodes = boolMap(activeState.optJSONObject("nodes")),
                commands = boolMap(activeState.optJSONObject("commands")),
                allowedMarks = stringSet(activeState.optJSONArray("allowedMarks")),
                insertableNodes = stringSet(activeState.optJSONArray("insertableNodes")),
                canUndo = historyState.optBoolean("canUndo", false),
                canRedo = historyState.optBoolean("canRedo", false)
            )
        }

        private fun boolMap(json: JSONObject?): Map<String, Boolean> {
            json ?: return emptyMap()
            val result = mutableMapOf<String, Boolean>()
            val keys = json.keys()
            while (keys.hasNext()) {
                val key = keys.next()
                result[key] = json.optBoolean(key, false)
            }
            return result
        }

        private fun stringSet(array: org.json.JSONArray?): Set<String> {
            array ?: return emptySet()
            val result = linkedSetOf<String>()
            for (index in 0 until array.length()) {
                array.optString(index, null)?.let { result.add(it) }
            }
            return result
        }
    }
}

internal enum class ToolbarCommand(val wireValue: String) {
    INDENT_LIST("indentList"),
    OUTDENT_LIST("outdentList"),
    UNDO("undo"),
    REDO("redo");

    companion object {
        fun fromWireValue(value: String): ToolbarCommand =
            entries.firstOrNull { it.wireValue == value }
                ?: throw IllegalArgumentException("Unknown ToolbarCommand: $value")
    }
}

internal object EditorNodeTypes {
    fun listItemType(listType: String): String = when (listType) {
        "bullet_list", "ordered_list" -> "list_item"
        "task_list" -> "task_item"
        "taskList" -> "taskItem"
        else -> "listItem"
    }

    fun isHardBreak(nodeType: String?): Boolean =
        nodeType == "hardBreak" || nodeType == "hard_break"

    fun isHorizontalRule(nodeType: String?): Boolean =
        nodeType == "horizontalRule" || nodeType == "horizontal_rule"

    fun isListItem(nodeType: String): Boolean = nodeType == "listItem" || nodeType == "list_item" ||
        nodeType == "taskItem" || nodeType == "task_item"

    fun isListContainer(nodeType: String): Boolean =
        nodeType == "bulletList" || nodeType == "bullet_list" ||
            nodeType == "orderedList" || nodeType == "ordered_list" ||
            nodeType == "taskList" || nodeType == "task_list"

    fun preferredHardBreak(insertableNodes: Set<String>): String =
        if (insertableNodes.contains("hard_break")) "hard_break" else "hardBreak"
}

internal enum class ToolbarListType(val wireValue: String) {
    BULLET_LIST("bullet_list"),
    ORDERED_LIST("ordered_list"),
    CAMEL_CASE_BULLET_LIST("bulletList"),
    CAMEL_CASE_ORDERED_LIST("orderedList");

    companion object {
        fun fromWireValue(value: String): ToolbarListType =
            entries.firstOrNull { it.wireValue == value }
                ?: throw IllegalArgumentException("Unknown ToolbarListType: $value")
    }
}

internal enum class ToolbarDefaultIconId(val wireValue: String) {
    BOLD("bold"),
    ITALIC("italic"),
    UNDERLINE("underline"),
    STRIKE("strike"),
    LINK("link"),
    IMAGE("image"),
    H1("h1"),
    H2("h2"),
    H3("h3"),
    H4("h4"),
    H5("h5"),
    H6("h6"),
    BLOCKQUOTE("blockquote"),
    BULLET_LIST("bulletList"),
    ORDERED_LIST("orderedList"),
    INDENT_LIST("indentList"),
    OUTDENT_LIST("outdentList"),
    LINE_BREAK("lineBreak"),
    HORIZONTAL_RULE("horizontalRule"),
    UNDO("undo"),
    REDO("redo");

    companion object {
        fun fromWireValue(value: String): ToolbarDefaultIconId =
            entries.firstOrNull { it.wireValue == value }
                ?: throw IllegalArgumentException("Unknown ToolbarDefaultIconId: $value")
    }
}

internal enum class ToolbarItemKind(val wireValue: String) {
    MARK("mark"),
    HEADING("heading"),
    BLOCKQUOTE("blockquote"),
    LIST("list"),
    COMMAND("command"),
    NODE("node"),
    ACTION("action"),
    GROUP("group"),
    SEPARATOR("separator");

    companion object {
        fun fromWireValue(value: String): ToolbarItemKind =
            entries.firstOrNull { it.wireValue == value }
                ?: throw IllegalArgumentException("Unknown ToolbarItemKind: $value")
    }
}

internal enum class ToolbarGroupPresentation(val wireValue: String) {
    EXPAND("expand"),
    MENU("menu");

    companion object {
        fun fromWireValue(value: String): ToolbarGroupPresentation =
            entries.firstOrNull { it.wireValue == value }
                ?: throw IllegalArgumentException("Unknown ToolbarGroupPresentation: $value")
    }
}

internal enum class ToolbarItemPlacement(val wireValue: String) {
    START("start"),
    SCROLL("scroll"),
    END("end");

    companion object {
        fun fromWireValue(value: String): ToolbarItemPlacement =
            entries.firstOrNull { it.wireValue == value }
                ?: throw IllegalArgumentException("Unknown ToolbarItemPlacement: $value")
    }
}
