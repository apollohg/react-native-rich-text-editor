package com.apollohg.editor

import org.json.JSONObject

data class EditorEdges(
    val top: Float = 0f,
    val right: Float = 0f,
    val bottom: Float = 0f,
    val left: Float = 0f
) {
    fun scaled(density: Float) = EditorEdges(
        top * density,
        right * density,
        bottom * density,
        left * density
    )
    operator fun plus(other: EditorEdges) = EditorEdges(
        top + other.top,
        right + other.right,
        bottom + other.bottom,
        left + other.left
    )
}

data class EditorCorners(
    val topLeft: Float = 0f,
    val topRight: Float = 0f,
    val bottomRight: Float = 0f,
    val bottomLeft: Float = 0f
) {
    fun scaled(density: Float) = EditorCorners(
        topLeft * density,
        topRight * density,
        bottomRight * density,
        bottomLeft * density
    )
}

data class EditorBoxStyle(
    val backgroundColor: Int? = null,
    val padding: EditorEdges = EditorEdges(),
    val margin: EditorEdges = EditorEdges(),
    val border: EditorEdges = EditorEdges(),
    val borderColors: List<Int> = List(4) { android.graphics.Color.BLACK },
    val corners: EditorCorners = EditorCorners(),
    val borderStyle: String = "solid"
) {
    val inset: EditorEdges get() = padding + border
    val outerInset: EditorEdges get() = inset + margin
    fun scaled(density: Float) = copy(
        padding = padding.scaled(density),
        margin = margin.scaled(density),
        border = border.scaled(density),
        corners = corners.scaled(density)
    )
}

data class EditorElementStyle(
    val text: EditorTextStyle,
    val box: EditorBoxStyle,
    val indent: Float? = null,
    val baseIndentMultiplier: Float? = null,
    val scale: Float? = null,
    val gap: Float? = null,
    val ordered: EditorOrderedListMarkerTheme? = null,
    val checked: EditorElementStyle? = null,
    val resizeMode: String = "contain",
    val size: Float? = null,
    val checkColor: Int? = null,
    val height: Float? = null,
    val declaredProperties: Set<String> = emptySet()
)

private data class EditorStyleRule(val path: List<String>, val style: JSONObject)

class EditorStyleSheet private constructor(
    val styles: Map<String, EditorElementStyle>,
    private val rules: List<EditorStyleRule> = emptyList()
) {
    operator fun get(element: String): EditorElementStyle? = styles[canonicalElement(element)]

    fun resolveElement(
        element: String,
        ancestors: List<String> = emptyList()
    ): EditorElementStyle? {
        val name = canonicalMark(canonicalElement(element))
        val matches = matchingRules(name, ancestors)
        val existing = this[name]
        if (matches.isEmpty()) return existing
        var result = existing ?: decodeElement(JSONObject(), defaultBox(name))
        for (rule in matches) {
            result = result.overlaidWith(decodeElement(rule.style, result.box), rule.style)
        }
        return result
    }

    fun resolveText(
        element: String,
        ancestors: List<String> = emptyList(),
        marks: List<String> = emptyList()
    ): EditorTextStyle {
        val name = canonicalElement(element)
        var result = EditorTextStyle().mergedWith(this["text"]?.text)
            .mergedWith(semanticText(name))
        ancestors.forEach {
            result = result.mergedWith(this[it]?.text?.copy(backgroundColor = null))
        }
        result = result.mergedWith(this[name]?.text?.copy(backgroundColor = null))
        matchingRules(name, ancestors).forEach {
            result = result.mergedWith(EditorTextStyle.fromJson(it.style)?.copy(backgroundColor = null))
        }
        val active = marks.map(::canonicalMark).toSet()
        listOf("inlineCode", "bold", "italic", "link", "underline", "strike").filter {
            it in active
        }.forEach {
            result = result.mergedWith(semanticText(it)).mergedWith(this[it]?.text)
            matchingRules(it, ancestors + name).forEach { rule ->
                result = result.mergedWith(EditorTextStyle.fromJson(rule.style))
            }
        }
        return result
    }

    fun box(element: String): EditorBoxStyle = box(element, emptyList())

    fun box(element: String, ancestors: List<String>): EditorBoxStyle {
        var result = this[element]?.box ?: defaultBox(canonicalElement(element))
        for (rule in matchingRules(element, ancestors)) {
            result = decodeElement(rule.style, result).box
        }
        return result
    }

    private fun matchingRules(element: String, ancestors: List<String>): List<EditorStyleRule> {
        val chain = (ancestors + element).map { canonicalMark(canonicalElement(it)) }
        return rules.filter { rule ->
            rule.path.size <= chain.size && chain.takeLast(rule.path.size) == rule.path
        }
    }

    companion object {
        internal fun decodeTheme(root: JSONObject): EditorTheme? {
            if ((root.opt("version") as? Number)?.toDouble() != 1.0 ||
                (root.has("styles") && root.optJSONObject("styles") == null)
            ) {
                return null
            }
            val values = root.optJSONObject("styles") ?: JSONObject()
            if (values.keys().asSequence().any { values.optJSONObject(it) == null }) return null
            val sheet = EditorStyleSheet(
                values.keys().asSequence().associateWith { name ->
                    decodeElement(values.getJSONObject(name), defaultBox(name))
                },
                decodeRules(root)
            )
            val content = sheet.box("content").outerInset
            val marker = sheet["listMarker"]
            val list = sheet["bulletList"]
            return EditorTheme(
                text = sheet["text"]?.text,
                paragraph = sheet["paragraph"]?.text,
                headings = (1..6).associate { "h$it" to sheet.resolveText("h$it") },
                list = EditorListTheme(
                    indent = list?.indent,
                    baseIndentMultiplier = list?.baseIndentMultiplier,
                    itemSpacing = 0f,
                    spacingAfter = 0f,
                    markerColor = marker?.text?.color,
                    markerScale = marker?.scale,
                    markerGap = marker?.gap,
                    orderedMarker = marker?.ordered
                ),
                blockquote = EditorBlockquoteTheme(
                    text = sheet["blockquote"]?.text,
                    indent = 0f,
                    borderWidth = 0f,
                    markerGap = 0f
                ),
                codeBlock = EditorCodeBlockTheme(
                    text = sheet.resolveText("codeBlock"),
                    paddingHorizontal = 0f,
                    paddingVertical = 0f
                ),
                mentions = EditorMentionTheme.fromJson(root.optJSONObject("mentions")),
                horizontalRule = EditorHorizontalRuleTheme(
                    color = sheet.box("horizontalRule").backgroundColor,
                    thickness =
                        sheet["horizontalRule"]?.height ?: 1f,
                    verticalMargin = 0f
                ),
                toolbar = EditorToolbarTheme.fromJson(root.optJSONObject("toolbar")),
                placeholderColor = sheet["placeholder"]?.text?.color,
                backgroundColor = sheet.box("content").backgroundColor,
                contentInsets = EditorContentInsets(
                    content.top,
                    content.right,
                    content.bottom,
                    content.left
                ),
                styleSheet = sheet
            )
        }

        private fun decodeRules(root: JSONObject): List<EditorStyleRule> {
            val values = root.optJSONArray("rules") ?: return emptyList()
            return buildList {
                for (index in 0 until values.length()) {
                    val entry = values.optJSONObject(index) ?: continue
                    val pathValues = entry.optJSONArray("path") ?: continue
                    if (pathValues.length() == 0) continue
                    val path = mutableListOf<String>()
                    for (pathIndex in 0 until pathValues.length()) {
                        val raw = pathValues.opt(pathIndex) as? String ?: break
                        val name = canonicalMark(canonicalElement(raw))
                        if (name !in STYLE_NAMES) break
                        path.add(name)
                    }
                    if (path.size != pathValues.length()) continue
                    val style = entry.optJSONObject("style") ?: continue
                    add(EditorStyleRule(path.toList(), style))
                }
            }
        }

        internal fun decodeElement(
            json: JSONObject,
            fallback: EditorBoxStyle = EditorBoxStyle()
        ): EditorElementStyle {
            fun number(key: String, default: Float) = json.optNullableFloat(key) ?: default
            fun edges(prefix: String, defaults: EditorEdges, suffix: String = "") = EditorEdges(
                number("${prefix}Top$suffix", defaults.top),
                number("${prefix}Right$suffix", defaults.right),
                number("${prefix}Bottom$suffix", defaults.bottom),
                number("${prefix}Left$suffix", defaults.left)
            )
            val box = EditorBoxStyle(
                backgroundColor =
                    parseColor(json.optNullableString("backgroundColor"))
                        ?: fallback.backgroundColor,
                padding = edges("padding", fallback.padding),
                margin = edges("margin", fallback.margin),
                border = edges("border", fallback.border, "Width"),
                borderColors = listOf("Top", "Right", "Bottom", "Left").mapIndexed { index, side ->
                    parseColor(json.optNullableString("border${side}Color"))
                        ?: fallback.borderColors[index]
                },
                corners = EditorCorners(
                    number("borderTopLeftRadius", fallback.corners.topLeft),
                    number("borderTopRightRadius", fallback.corners.topRight),
                    number("borderBottomRightRadius", fallback.corners.bottomRight),
                    number("borderBottomLeftRadius", fallback.corners.bottomLeft)
                ),
                borderStyle = json.optNullableString("borderStyle") ?: fallback.borderStyle
            )
            return EditorElementStyle(
                text = EditorTextStyle.fromJson(json) ?: EditorTextStyle(), box = box,
                indent = json.optNullableFloat(
                    "indent"
                ),
                baseIndentMultiplier = json.optNullableFloat("baseIndentMultiplier"),
                scale = json.optNullableFloat("scale"), gap = json.optNullableFloat("gap"),
                ordered = EditorOrderedListMarkerTheme.fromJson(json.optJSONObject("ordered")),
                checked = json.optJSONObject("checked")?.let { decodeElement(it, box) },
                resizeMode = json.optNullableString("resizeMode") ?: "contain",
                size = json.optNullableFloat(
                    "size"
                ),
                checkColor = parseColor(json.optNullableString("checkColor")),
                height = json.optNullableFloat("height"),
                declaredProperties = json.keys().asSequence().toSet()
            )
        }
    }
}

private val STYLE_NAMES = setOf(
    "content", "text", "paragraph", "h1", "h2", "h3", "h4", "h5", "h6",
    "blockquote", "codeBlock", "bulletList", "orderedList", "taskList", "listItem",
    "taskItem", "listMarker", "taskCheckbox", "horizontalRule", "image", "link",
    "inlineCode", "bold", "italic", "underline", "strike", "mention", "placeholder"
)

private fun EditorElementStyle.overlaidWith(
    other: EditorElementStyle,
    raw: JSONObject? = null
): EditorElementStyle {
    val keys = other.declaredProperties
    fun sides(prefix: String, current: EditorEdges, next: EditorEdges, suffix: String = "") =
        EditorEdges(
            if ("${prefix}Top$suffix" in keys) next.top else current.top,
            if ("${prefix}Right$suffix" in keys) next.right else current.right,
            if ("${prefix}Bottom$suffix" in keys) next.bottom else current.bottom,
            if ("${prefix}Left$suffix" in keys) next.left else current.left
        )
    val nextChecked = if ("checked" in keys) {
        other.checked?.let {
            (checked ?: EditorElementStyle(EditorTextStyle(), box)).overlaidWith(
                it,
                raw?.optJSONObject("checked")
            )
        }
    } else {
        checked
    }
    val nextOrdered = if ("ordered" in keys) {
        other.ordered?.let { next ->
            val current = ordered ?: EditorOrderedListMarkerTheme()
            val orderedJson = raw?.optJSONObject("ordered")
            EditorOrderedListMarkerTheme(
                schemes = if (orderedJson?.has("schemes") == true) next.schemes else current.schemes,
                suffix = if (orderedJson?.has("suffix") == true) next.suffix else current.suffix
            )
        }
    } else {
        ordered
    }
    return copy(
        text = text.mergedWith(other.text),
        box = box.copy(
            backgroundColor = if ("backgroundColor" in keys) {
                other.box.backgroundColor
            } else {
                box.backgroundColor
            },
            padding = sides("padding", box.padding, other.box.padding),
            margin = sides("margin", box.margin, other.box.margin),
            border = sides("border", box.border, other.box.border, "Width"),
            borderColors = listOf("Top", "Right", "Bottom", "Left").mapIndexed { index, side ->
                if ("border${side}Color" in keys) other.box.borderColors[index]
                else box.borderColors[index]
            },
            corners = EditorCorners(
                if ("borderTopLeftRadius" in keys) other.box.corners.topLeft else box.corners.topLeft,
                if ("borderTopRightRadius" in keys) other.box.corners.topRight else box.corners.topRight,
                if ("borderBottomRightRadius" in keys) {
                    other.box.corners.bottomRight
                } else {
                    box.corners.bottomRight
                },
                if ("borderBottomLeftRadius" in keys) {
                    other.box.corners.bottomLeft
                } else {
                    box.corners.bottomLeft
                }
            ),
            borderStyle = if ("borderStyle" in keys) other.box.borderStyle else box.borderStyle
        ),
        indent = if ("indent" in keys) other.indent else indent,
        baseIndentMultiplier = if ("baseIndentMultiplier" in keys) {
            other.baseIndentMultiplier
        } else {
            baseIndentMultiplier
        },
        scale = if ("scale" in keys) other.scale else scale,
        gap = if ("gap" in keys) other.gap else gap,
        ordered = nextOrdered,
        checked = nextChecked,
        resizeMode = if ("resizeMode" in keys) other.resizeMode else resizeMode,
        size = if ("size" in keys) other.size else size,
        checkColor = if ("checkColor" in keys) other.checkColor else checkColor,
        height = if ("height" in keys) other.height else height,
        declaredProperties = declaredProperties + keys
    )
}

internal fun canonicalElement(name: String): String = when (name) {
    "bullet_list" -> "bulletList"
    "ordered_list" -> "orderedList"
    "task_list" -> "taskList"
    "list_item" -> "listItem"
    "task_item" -> "taskItem"
    "horizontal_rule" -> "horizontalRule"
    "code_block" -> "codeBlock"
    else -> name
}

internal fun canonicalMark(name: String): String = when (name) {
    "code" -> "inlineCode"
    "strong" -> "bold"
    "em" -> "italic"
    "strikethrough" -> "strike"
    else -> name
}

internal fun semanticText(name: String): EditorTextStyle = when (name) {
    "h1", "h2", "h3", "h4", "h5", "h6" -> EditorTextStyle(
        fontSize = listOf(32f, 28f, 24f, 21f, 19f, 17f)[
            name.last().digitToInt() -
                1
        ],
        fontWeight = "700"
    )

    "codeBlock" -> EditorTextStyle(fontFamily = "monospace")

    "inlineCode" -> EditorTextStyle(
        fontFamily = "monospace",
        backgroundColor = LayoutConstants.CODE_BACKGROUND_COLOR
    )

    "bold" -> EditorTextStyle(fontWeight = "700")

    "italic" -> EditorTextStyle(fontStyle = "italic")

    "link" -> EditorTextStyle(
        color = LayoutConstants.DEFAULT_LINK_COLOR,
        textDecorationLine = "underline"
    )

    "underline" -> EditorTextStyle(textDecorationLine = "underline")

    "strike" -> EditorTextStyle(textDecorationLine = "line-through")

    else -> EditorTextStyle()
}

internal fun defaultBox(name: String): EditorBoxStyle = when (name) {
    "codeBlock" -> EditorBoxStyle(
        backgroundColor = LayoutConstants.CODE_BACKGROUND_COLOR,
        padding = EditorEdges(8f, 12f, 8f, 12f),
        corners = EditorCorners(8f, 8f, 8f, 8f)
    )

    "blockquote" -> EditorBoxStyle(
        padding = EditorEdges(left = 10f),
        border = EditorEdges(left = 3f),
        borderColors = List(4) {
            0xFFC7C7CC.toInt()
        }
    )

    "listItem", "taskItem" -> EditorBoxStyle(margin = EditorEdges(bottom = 4f))

    "h1", "h2", "h3", "h4", "h5", "h6" -> EditorBoxStyle(margin = EditorEdges(bottom = 10f))

    "horizontalRule" -> EditorBoxStyle(
        backgroundColor = 0xFFC7C7CC.toInt(),
        margin = EditorEdges(top = 12f, bottom = 12f)
    )

    else -> EditorBoxStyle()
}

internal fun mergeDecoration(previous: String?, next: String?): String? = when {
    next == null -> previous

    next == "none" -> "none"

    else -> listOfNotNull(
        previous?.takeUnless {
            it == "none"
        },
        next
    ).flatMap { it.split(' ') }.distinct().joinToString(" ")
}
