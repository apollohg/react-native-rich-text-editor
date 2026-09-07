package com.apollohg.editor

import org.json.JSONObject

internal data class NativeToolbarItem(
    val type: ToolbarItemKind,
    val key: String? = null,
    val label: String? = null,
    val icon: NativeToolbarIcon? = null,
    val mark: String? = null,
    val headingLevel: Int? = null,
    val listType: ToolbarListType? = null,
    val command: ToolbarCommand? = null,
    val nodeType: String? = null,
    val isActive: Boolean = false,
    val isDisabled: Boolean = false,
    val placement: ToolbarItemPlacement? = null,
    val presentation: ToolbarGroupPresentation? = null,
    val items: List<NativeToolbarItem> = emptyList(),
    val buttonStyle: EditorToolbarButtonStyle? = null,
    val parentGroupKey: String? = null
) {
    companion object {
        val defaults = listOf(
            NativeToolbarItem(
                ToolbarItemKind.MARK,
                label = "Bold",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.BOLD),
                mark = "bold"
            ),
            NativeToolbarItem(
                ToolbarItemKind.MARK,
                label = "Italic",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.ITALIC),
                mark = "italic"
            ),
            NativeToolbarItem(
                ToolbarItemKind.MARK,
                label = "Underline",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.UNDERLINE),
                mark = "underline"
            ),
            NativeToolbarItem(
                ToolbarItemKind.MARK,
                label = "Strikethrough",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.STRIKE),
                mark = "strike"
            ),
            NativeToolbarItem(
                ToolbarItemKind.BLOCKQUOTE,
                label = "Blockquote",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.BLOCKQUOTE)
            ),
            NativeToolbarItem(ToolbarItemKind.SEPARATOR),
            NativeToolbarItem(
                ToolbarItemKind.LIST,
                label = "Bullet List",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.BULLET_LIST),
                listType = ToolbarListType.BULLET_LIST
            ),
            NativeToolbarItem(
                ToolbarItemKind.LIST,
                label = "Ordered List",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.ORDERED_LIST),
                listType = ToolbarListType.ORDERED_LIST
            ),
            NativeToolbarItem(
                ToolbarItemKind.COMMAND,
                label = "Indent List",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.INDENT_LIST),
                command = ToolbarCommand.INDENT_LIST
            ),
            NativeToolbarItem(
                ToolbarItemKind.COMMAND,
                label = "Outdent List",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.OUTDENT_LIST),
                command = ToolbarCommand.OUTDENT_LIST
            ),
            NativeToolbarItem(
                ToolbarItemKind.NODE,
                label = "Line Break",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.LINE_BREAK),
                nodeType = "hard_break"
            ),
            NativeToolbarItem(
                ToolbarItemKind.NODE,
                label = "Horizontal Rule",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.HORIZONTAL_RULE),
                nodeType = "horizontal_rule"
            ),
            NativeToolbarItem(ToolbarItemKind.SEPARATOR),
            NativeToolbarItem(
                ToolbarItemKind.COMMAND,
                label = "Undo",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.UNDO),
                command = ToolbarCommand.UNDO
            ),
            NativeToolbarItem(
                ToolbarItemKind.COMMAND,
                label = "Redo",
                icon = NativeToolbarIcon(defaultId = ToolbarDefaultIconId.REDO),
                command = ToolbarCommand.REDO
            )
        )

        private fun parseItem(
            rawItem: JSONObject,
            allowGroup: Boolean = true,
            allowSeparator: Boolean = true
        ): NativeToolbarItem? {
            val type = runCatching {
                ToolbarItemKind.fromWireValue(rawItem.getString("type"))
            }.getOrNull() ?: return null
            val key = rawItem.toolbarNullableString("key")
            val placement = rawItem.toolbarNullableString("placement")?.let {
                runCatching { ToolbarItemPlacement.fromWireValue(it) }.getOrNull()
            }
            val parsed = when (type) {
                ToolbarItemKind.SEPARATOR -> {
                    if (!allowSeparator) {
                        null
                    } else {
                        NativeToolbarItem(type = type, key = key, placement = placement)
                    }
                }

                ToolbarItemKind.MARK -> {
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val mark = rawItem.toolbarNullableString("mark") ?: return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    NativeToolbarItem(type, key, label, icon, mark = mark, placement = placement)
                }

                ToolbarItemKind.HEADING -> {
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val level = rawItem.optInt("level", -1)
                    if (level !in 1..6) return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    NativeToolbarItem(
                        type,
                        key,
                        label,
                        icon,
                        headingLevel = level,
                        placement = placement
                    )
                }

                ToolbarItemKind.BLOCKQUOTE -> {
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    NativeToolbarItem(type, key, label, icon, placement = placement)
                }

                ToolbarItemKind.LIST -> {
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val listType = runCatching {
                        ToolbarListType.fromWireValue(rawItem.getString("listType"))
                    }.getOrNull() ?: return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    NativeToolbarItem(
                        type,
                        key,
                        label,
                        icon,
                        listType = listType,
                        placement = placement
                    )
                }

                ToolbarItemKind.COMMAND -> {
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val command = runCatching {
                        ToolbarCommand.fromWireValue(rawItem.getString("command"))
                    }.getOrNull() ?: return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    NativeToolbarItem(
                        type,
                        key,
                        label,
                        icon,
                        command = command,
                        placement = placement
                    )
                }

                ToolbarItemKind.NODE -> {
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val nodeType = rawItem.toolbarNullableString("nodeType") ?: return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    NativeToolbarItem(
                        type,
                        key,
                        label,
                        icon,
                        nodeType = nodeType,
                        placement = placement
                    )
                }

                ToolbarItemKind.ACTION -> {
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val keyValue = rawItem.toolbarNullableString("key") ?: return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    NativeToolbarItem(
                        type = type,
                        key = keyValue,
                        label = label,
                        icon = icon,
                        placement = placement,
                        isActive = rawItem.optBoolean("isActive", false),
                        isDisabled = rawItem.optBoolean("isDisabled", false)
                    )
                }

                ToolbarItemKind.GROUP -> {
                    if (!allowGroup) return null
                    val keyValue = rawItem.toolbarNullableString("key") ?: return null
                    val icon =
                        NativeToolbarIcon.fromJson(rawItem.optJSONObject("icon")) ?: return null
                    val label = rawItem.toolbarNullableString("label") ?: return null
                    val presentation = rawItem.toolbarNullableString("presentation")?.let {
                        runCatching { ToolbarGroupPresentation.fromWireValue(it) }.getOrNull()
                    } ?: ToolbarGroupPresentation.EXPAND
                    val rawChildren = rawItem.optJSONArray("items") ?: return null
                    val children = mutableListOf<NativeToolbarItem>()
                    for (childIndex in 0 until rawChildren.length()) {
                        val rawChild = rawChildren.optJSONObject(childIndex) ?: continue
                        parseItem(rawChild, allowGroup = false, allowSeparator = false)?.let {
                            children += it
                        }
                    }
                    if (children.isEmpty()) return null
                    NativeToolbarItem(
                        type = type,
                        key = keyValue,
                        label = label,
                        icon = icon,
                        placement = placement,
                        presentation = presentation,
                        items = children
                    )
                }
            }
            return parsed?.copy(
                buttonStyle = EditorToolbarButtonStyle.fromJson(
                    rawItem.optJSONObject("buttonStyle")
                )
            )
        }

        fun fromJson(json: String?): List<NativeToolbarItem> {
            if (json.isNullOrBlank()) return defaults
            val rawArray = try {
                org.json.JSONArray(json)
            } catch (_: Exception) {
                return defaults
            }
            val parsed = mutableListOf<NativeToolbarItem>()
            for (index in 0 until rawArray.length()) {
                val rawItem = rawArray.optJSONObject(index) ?: continue
                parseItem(rawItem)?.let { parsed += it }
            }
            return parsed.ifEmpty { defaults }
        }
    }
}
