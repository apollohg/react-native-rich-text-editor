package com.apollohg.editor

import android.content.Context
import android.graphics.Typeface
import org.json.JSONObject

internal data class NativeToolbarIcon(
    val defaultId: ToolbarDefaultIconId? = null,
    val glyphText: String? = null,
    val fallbackText: String? = null,
    val materialIconName: String? = null
) {
    companion object {
        private val defaultGlyphs = mapOf(
            ToolbarDefaultIconId.BOLD to "B",
            ToolbarDefaultIconId.ITALIC to "I",
            ToolbarDefaultIconId.UNDERLINE to "U",
            ToolbarDefaultIconId.STRIKE to "S",
            ToolbarDefaultIconId.LINK to "🔗",
            ToolbarDefaultIconId.IMAGE to "🖼",
            ToolbarDefaultIconId.H1 to "H1",
            ToolbarDefaultIconId.H2 to "H2",
            ToolbarDefaultIconId.H3 to "H3",
            ToolbarDefaultIconId.H4 to "H4",
            ToolbarDefaultIconId.H5 to "H5",
            ToolbarDefaultIconId.H6 to "H6",
            ToolbarDefaultIconId.BLOCKQUOTE to "❝",
            ToolbarDefaultIconId.BULLET_LIST to "•≡",
            ToolbarDefaultIconId.ORDERED_LIST to "1.",
            ToolbarDefaultIconId.INDENT_LIST to "→",
            ToolbarDefaultIconId.OUTDENT_LIST to "←",
            ToolbarDefaultIconId.LINE_BREAK to "↵",
            ToolbarDefaultIconId.HORIZONTAL_RULE to "—",
            ToolbarDefaultIconId.UNDO to "↩",
            ToolbarDefaultIconId.REDO to "↪"
        )
        private val defaultMaterialIcons = mapOf(
            ToolbarDefaultIconId.BOLD to "format-bold",
            ToolbarDefaultIconId.ITALIC to "format-italic",
            ToolbarDefaultIconId.UNDERLINE to "format-underlined",
            ToolbarDefaultIconId.STRIKE to "strikethrough-s",
            ToolbarDefaultIconId.LINK to "link",
            ToolbarDefaultIconId.IMAGE to "image",
            ToolbarDefaultIconId.BLOCKQUOTE to "format-quote",
            ToolbarDefaultIconId.BULLET_LIST to "format-list-bulleted",
            ToolbarDefaultIconId.ORDERED_LIST to "format-list-numbered",
            ToolbarDefaultIconId.INDENT_LIST to "format-indent-increase",
            ToolbarDefaultIconId.OUTDENT_LIST to "format-indent-decrease",
            ToolbarDefaultIconId.LINE_BREAK to "keyboard-return",
            ToolbarDefaultIconId.HORIZONTAL_RULE to "horizontal-rule",
            ToolbarDefaultIconId.H1 to "title",
            ToolbarDefaultIconId.H2 to "title",
            ToolbarDefaultIconId.H3 to "title",
            ToolbarDefaultIconId.H4 to "title",
            ToolbarDefaultIconId.H5 to "title",
            ToolbarDefaultIconId.H6 to "title",
            ToolbarDefaultIconId.UNDO to "undo",
            ToolbarDefaultIconId.REDO to "redo"
        )

        fun fromJson(raw: JSONObject?): NativeToolbarIcon? {
            raw ?: return null
            return when (raw.optString("type")) {
                "default" -> {
                    val id = runCatching {
                        ToolbarDefaultIconId.fromWireValue(raw.getString("id"))
                    }.getOrNull() ?: return null
                    NativeToolbarIcon(defaultId = id)
                }

                "glyph" -> {
                    val text = raw.optString("text")
                    if (text.isBlank()) null else NativeToolbarIcon(glyphText = text)
                }

                "platform" -> {
                    val materialName = raw.optJSONObject("android")
                        ?.takeIf { it.optString("type") == "material" }
                        ?.toolbarNullableString("name")
                    val fallback = raw.toolbarNullableString("fallbackText")
                    if (materialName.isNullOrBlank() && fallback.isNullOrBlank()) {
                        null
                    } else {
                        NativeToolbarIcon(
                            fallbackText = fallback,
                            materialIconName = materialName
                        )
                    }
                }

                else -> null
            }
        }

        fun defaultMaterialIconName(defaultId: ToolbarDefaultIconId?): String? =
            defaultId?.let { defaultMaterialIcons[it] }
    }

    fun resolvedGlyphText(): String = glyphText?.takeIf { it.isNotBlank() }
        ?: fallbackText?.takeIf { it.isNotBlank() }
        ?: defaultId?.let { defaultGlyphs[it] }
        ?: "?"

    fun resolvedMaterialIconName(): String? = materialIconName?.takeIf { it.isNotBlank() }
        ?: Companion.defaultMaterialIconName(defaultId)
}

internal object MaterialIconRegistry {
    private const val FONT_ASSET_PATH = "editor-icons/MaterialIcons.ttf"
    private const val GLYPHMAP_ASSET_PATH = "editor-icons/MaterialIcons.json"

    @Volatile
    private var typeface: Typeface? = null

    @Volatile
    private var glyphMap: Map<String, String>? = null

    fun typeface(context: Context): Typeface? {
        val cached = typeface
        if (cached != null) return cached
        return runCatching {
            Typeface.createFromAsset(context.assets, FONT_ASSET_PATH)
        }.getOrNull()?.also { loaded ->
            typeface = loaded
        }
    }

    fun glyphForName(context: Context, name: String?): String? {
        if (name.isNullOrBlank()) return null
        val map = glyphMap ?: loadGlyphMap(context).also { loaded ->
            glyphMap = loaded
        }
        return map[name]
    }

    private fun loadGlyphMap(context: Context): Map<String, String> {
        val assetText = runCatching {
            context.assets.open(GLYPHMAP_ASSET_PATH).bufferedReader().use { it.readText() }
        }.getOrNull() ?: return emptyMap()

        val json = runCatching { JSONObject(assetText) }.getOrNull() ?: return emptyMap()
        val result = linkedMapOf<String, String>()
        val keys = json.keys()
        while (keys.hasNext()) {
            val key = keys.next()
            val codePoint = json.optInt(key, -1)
            if (codePoint > 0) {
                result[key] = String(Character.toChars(codePoint))
            }
        }
        return result
    }
}

internal data class NativeToolbarResolvedIcon(val text: String, val typeface: Typeface? = null)

internal fun NativeToolbarIcon.resolveForAndroid(context: Context): NativeToolbarResolvedIcon {
    val materialName = resolvedMaterialIconName()
    val materialGlyph = MaterialIconRegistry.glyphForName(context, materialName)
    val materialTypeface = MaterialIconRegistry.typeface(context)
    if (materialGlyph != null && materialTypeface != null) {
        return NativeToolbarResolvedIcon(
            text = materialGlyph,
            typeface = materialTypeface
        )
    }

    return NativeToolbarResolvedIcon(
        text = resolvedGlyphText(),
        typeface = null
    )
}

internal fun JSONObject.toolbarNullableString(key: String): String? {
    if (!has(key) || isNull(key)) return null
    return optString(key).takeUnless { it == "null" }
}
