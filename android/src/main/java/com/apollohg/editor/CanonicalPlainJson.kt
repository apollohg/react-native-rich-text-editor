package com.apollohg.editor

import org.json.JSONObject

internal object CanonicalPlainJson {
    private const val MAX_CERTIFICATE_DEPTH = 64
    private const val TYPE_MEMBER = "\"type\":"
    private const val TEXT_MEMBER = "\"text\":"
    private const val CONTENT_MEMBER = "\"content\":"
    private const val TYPE_FIELD = 1
    private const val TEXT_FIELD = 2
    private const val CONTENT_FIELD = 4

    fun accepts(source: String): Boolean = Cursor(source).accepts()

    private class Cursor(private val source: String) {
        private var position = 0

        fun accepts(): Boolean = node(0) && position == source.length

        private fun node(depth: Int): Boolean {
            if (depth > MAX_CERTIFICATE_DEPTH || !take('{')) return false
            if (take('}')) return true
            var fields = 0
            while (true) {
                val field = when {
                    take(TYPE_MEMBER) -> TYPE_FIELD
                    take(TEXT_MEMBER) -> TEXT_FIELD
                    take(CONTENT_MEMBER) -> CONTENT_FIELD
                    else -> return false
                }
                if (fields and field != 0) return false
                fields = fields or field
                if (field == CONTENT_FIELD) {
                    if (!content(depth)) return false
                } else if (!string()) {
                    return false
                }
                if (take('}')) return true
                if (!take(',')) return false
            }
        }

        private fun content(depth: Int): Boolean {
            if (!take('[')) return false
            if (take(']')) return true
            while (true) {
                if (!node(depth + 1)) return false
                if (take(']')) return true
                if (!take(',')) return false
            }
        }

        private fun string(): Boolean {
            if (!take('"')) return false
            while (position < source.length) {
                val character = source[position++]
                if (character == '"') return true
                // Android JSONStringer escapes slash and controls as well as JSON escapes.
                if (character !in ' '..'~' || character == '\\' || character == '/') return false
            }
            return false
        }

        private fun take(character: Char): Boolean {
            if (source.getOrNull(position) != character) return false
            position++
            return true
        }

        private fun take(token: String): Boolean {
            if (!source.startsWith(token, position)) return false
            position += token.length
            return true
        }
    }
}

private const val JSON_REPLACEMENT_FIELD = "setJson"
private const val HISTORY_FIELD = "history"
private const val RESET_HISTORY = "resetAndClear"
private const val JSON_REPLACEMENT_PREFIX = "{\"$JSON_REPLACEMENT_FIELD\":"
private const val JSON_REPLACEMENT_SUFFIX = ",\"$HISTORY_FIELD\":\"$RESET_HISTORY\"}"

// Validate before mutation; serialize only after request-ID admission.
internal fun prepareJsonReplacementPayload(source: String): () -> String {
    if (CanonicalPlainJson.accepts(source)) {
        return { "$JSON_REPLACEMENT_PREFIX$source$JSON_REPLACEMENT_SUFFIX" }
    }
    val document = JSONObject(source)
    return {
        JSONObject().put(
            JSON_REPLACEMENT_FIELD,
            document
        ).put(HISTORY_FIELD, RESET_HISTORY).toString()
    }
}
