package com.apollohg.editor

import org.json.JSONArray
import org.json.JSONObject

internal fun ulongField(jsonObject: JSONObject, key: String): ULong? =
    canonicalV2U64(jsonObject.opt(key) as? String)?.toULong()

internal fun scalarField(jsonObject: JSONObject, key: String): Int? =
    exactV2ScalarInt(jsonObject.opt(key) as? Number)

internal fun exactBool(value: Any?): Boolean? = value as? Boolean

private fun exactKeys(jsonObject: JSONObject, keys: Set<String>): Boolean {
    val actual = mutableSetOf<String>()
    val iterator = jsonObject.keys()
    while (iterator.hasNext()) actual += iterator.next()
    return actual == keys
}

private fun onlyKeys(jsonObject: JSONObject, keys: Set<String>): Boolean {
    val iterator = jsonObject.keys()
    while (iterator.hasNext()) if (iterator.next() !in keys) return false
    return true
}

private fun validJsonValue(value: Any?): Boolean = when (value) {
    null, JSONObject.NULL, is String, is Boolean -> true

    is Number -> value.toDouble().isFinite()

    is JSONArray -> (0 until value.length()).all { validJsonValue(value.opt(it)) }

    is JSONObject -> {
        val iterator = value.keys()
        var valid = true
        while (iterator.hasNext()) {
            if (!validJsonValue(value.opt(iterator.next()))) valid = false
        }
        valid
    }

    else -> false
}

private fun validRenderMark(value: Any?): Boolean = when (value) {
    is String -> true
    is JSONObject -> value.opt("type") is String && validJsonValue(value)
    else -> false
}

private fun validListContext(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    if (!onlyKeys(
            jsonObject,
            setOf("ordered", "index", "total", "start", "isFirst", "isLast", "kind", "checked")
        )
    ) {
        return false
    }
    if (exactBool(jsonObject.opt("ordered")) == null || scalarField(jsonObject, "index") == null ||
        scalarField(jsonObject, "total") == null || scalarField(jsonObject, "start") == null ||
        exactBool(jsonObject.opt("isFirst")) == null || exactBool(jsonObject.opt("isLast")) == null
    ) {
        return false
    }
    val kind = jsonObject.opt("kind")
    if (kind != null && kind !== JSONObject.NULL && kind !is String) return false
    val checked = jsonObject.opt("checked")
    return checked == null || checked === JSONObject.NULL || exactBool(checked) != null
}

private fun validMentionThemeSection(
    value: Any?,
    stringKeys: Set<String>,
    extraKeys: Set<String>
): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    val numberKeys = setOf("borderWidth", "borderRadius")
    if (!onlyKeys(jsonObject, stringKeys + numberKeys + extraKeys)) return false
    if (stringKeys.any { jsonObject.has(it) && jsonObject.opt(it) !is String }) return false
    if (numberKeys.any {
            jsonObject.has(it) &&
                (
                    jsonObject.opt(it) !is Number ||
                        !(jsonObject.opt(it) as Number).toDouble().isFinite()
                    )
        }
    ) {
        return false
    }
    val weight = jsonObject.opt("fontWeight")
    return weight == null ||
        weight in
        setOf("normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900")
}

private fun validMentionTheme(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    if (!onlyKeys(jsonObject, setOf("node", "suggestions"))) return false

    if (jsonObject.has("node") && !validMentionThemeSection(
            jsonObject.opt("node"),
            setOf("textColor", "backgroundColor", "borderColor"),
            setOf("fontWeight", "style")
        )
    ) {
        return false
    }
    val node = jsonObject.optJSONObject("node")
    if (node?.has("style") == true && !validMentionNodeStyle(node.opt("style"))) return false
    if (!jsonObject.has("suggestions")) return true
    val suggestions = jsonObject.opt("suggestions")
    if (!validMentionThemeSection(
            suggestions,
            setOf("backgroundColor", "borderColor", "shadowColor"),
            setOf("option")
        )
    ) {
        return false
    }
    val option = (suggestions as? JSONObject)?.opt("option") ?: return true
    return validMentionThemeSection(
        option,
        setOf(
            "textColor",
            "secondaryTextColor",
            "backgroundColor",
            "borderColor",
            "highlightedBackgroundColor",
            "highlightedTextColor"
        ),
        setOf("fontWeight")
    )
}

private fun validMentionNodeStyle(value: Any?): Boolean {
    val style = value as? JSONObject ?: return false
    val colors =
        setOf("color", "backgroundColor", "textDecorationColor") +
            listOf("Top", "Right", "Bottom", "Left").map { "border${it}Color" }
    val nonnegative =
        listOf("Top", "Right", "Bottom", "Left").map { "border${it}Width" } +
            listOf("Top", "Right", "Bottom", "Left").map { "padding$it" } +
            listOf("TopLeft", "TopRight", "BottomLeft", "BottomRight").map { "border${it}Radius" }
    val enums = mapOf(
        "fontWeight" to
            setOf("normal", "bold", "100", "200", "300", "400", "500", "600", "700", "800", "900"),
        "fontStyle" to setOf("normal", "italic"),
        "borderStyle" to setOf("solid", "dashed", "dotted"),
        "textDecorationLine" to setOf(
            "none",
            "underline",
            "line-through",
            "underline line-through"
        ),
        "textDecorationStyle" to setOf("solid", "double", "dashed", "dotted")
    )
    return style.keys().asSequence().all { key ->
        val field = style.opt(key)
        when {
            key in colors -> field is String && field.matches(Regex("#[0-9a-fA-F]{8}"))

            key in nonnegative -> field is Number && field.toDouble().isFinite() &&
                field.toDouble() >= 0

            key == "fontSize" || key == "lineHeight" ->
                field is Number &&
                    field.toDouble().isFinite() &&
                    field.toDouble() > 0

            key == "letterSpacing" -> field is Number && field.toDouble().isFinite()

            key == "fontFamily" -> field is String && field.isNotBlank()

            key in enums -> field in enums.getValue(key)

            else -> false
        }
    }
}

private fun validRenderElement(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    return when (jsonObject.opt("type") as? String) {
        "textRun" -> exactKeys(jsonObject, setOf("type", "text", "marks")) &&
            jsonObject.opt("text") is String &&
            (jsonObject.opt("marks") as? JSONArray)?.let { marks ->
                (0 until marks.length()).all { validRenderMark(marks.opt(it)) }
            } ==
            true

        "blockStart" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "depth", "listContext", "language")
        ) &&
            jsonObject.opt("nodeType") is String &&
            scalarField(jsonObject, "depth") != null &&
            (!jsonObject.has("listContext") || validListContext(jsonObject.opt("listContext"))) &&
            (
                !jsonObject.has("language") || jsonObject.isNull("language") ||
                    jsonObject.opt("language") is String
                )

        "blockEnd" -> exactKeys(jsonObject, setOf("type"))

        "voidInline" -> onlyKeys(jsonObject, setOf("type", "nodeType", "docPos", "attrs")) &&
            jsonObject.opt("nodeType") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject)

        "voidBlock" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "docPos", "attrs", "atomId")
        ) &&
            jsonObject.opt("nodeType") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject) &&
            (!jsonObject.has("atomId") || jsonObject.opt("atomId") is String)

        "opaqueInlineAtom" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "label", "docPos", "attrs", "mentionTheme")
        ) &&
            jsonObject.opt("nodeType") is String && jsonObject.opt("label") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject) &&
            (!jsonObject.has("mentionTheme") || validMentionTheme(jsonObject.opt("mentionTheme")))

        "opaqueBlockAtom" -> onlyKeys(
            jsonObject,
            setOf("type", "nodeType", "label", "docPos", "attrs")
        ) &&
            jsonObject.opt("nodeType") is String && jsonObject.opt("label") is String &&
            scalarField(jsonObject, "docPos") != null &&
            (!jsonObject.has("attrs") || jsonObject.opt("attrs") is JSONObject)

        else -> false
    }
}

private fun validRenderBlocks(value: Any?): Boolean {
    val blocks = value as? JSONArray ?: return false
    return (0 until blocks.length()).all { blockIndex ->
        val block = blocks.opt(blockIndex) as? JSONArray ?: return@all false
        (0 until block.length()).all { validRenderElement(block.opt(it)) }
    }
}

private fun validRenderPatch(value: Any?): Boolean {
    if (value === JSONObject.NULL) return true
    val patch = value as? JSONObject ?: return false
    return exactKeys(
        patch,
        setOf("baseDocumentVersion", "startIndex", "deleteCount", "renderBlocks")
    ) &&
        canonicalV2U64(patch.opt("baseDocumentVersion") as? String) != null &&
        scalarField(patch, "startIndex") != null && scalarField(patch, "deleteCount") != null &&
        validRenderBlocks(patch.opt("renderBlocks"))
}

private fun validBooleanRecord(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    val iterator = jsonObject.keys()
    while (iterator.hasNext()) if (exactBool(jsonObject.opt(iterator.next())) == null) return false
    return true
}

private fun validStringArray(value: Any?): Boolean {
    val array = value as? JSONArray ?: return false
    return (0 until array.length()).all { array.opt(it) is String }
}

private fun validActiveState(value: Any?): Boolean {
    val jsonObject = value as? JSONObject ?: return false
    if (!exactKeys(
            jsonObject,
            setOf("marks", "markAttrs", "nodes", "commands", "allowedMarks", "insertableNodes")
        )
    ) {
        return false
    }
    val attrs = jsonObject.opt("markAttrs") as? JSONObject ?: return false
    val attrsIterator = attrs.keys()
    while (attrsIterator.hasNext()) if (attrs.opt(attrsIterator.next()) !is JSONObject) return false
    return validBooleanRecord(jsonObject.opt("marks")) &&
        validBooleanRecord(jsonObject.opt("nodes")) &&
        validBooleanRecord(
            jsonObject.opt("commands")
        ) && validStringArray(jsonObject.opt("allowedMarks")) &&
        validStringArray(jsonObject.opt("insertableNodes"))
}

internal fun scalarSelection(value: Any?): IntArray? {
    val selection = value as? JSONObject ?: return null
    if (selection.opt("type") != "text" ||
        !exactKeys(selection, setOf("type", "anchor", "head", "anchorScalar", "headScalar"))
    ) {
        return null
    }
    if (scalarField(selection, "anchor") == null ||
        scalarField(selection, "head") == null
    ) {
        return null
    }
    return intArrayOf(
        scalarField(selection, "anchorScalar") ?: return null,
        scalarField(selection, "headScalar") ?: return null
    )
}

private fun validSelection(value: Any?): Boolean {
    val selection = value as? JSONObject ?: return false
    return when (selection.opt("type") as? String) {
        "text" -> scalarSelection(selection) != null

        "node" -> exactKeys(selection, setOf("type", "pos", "posScalar")) &&
            scalarField(selection, "pos") != null &&
            scalarField(selection, "posScalar") != null

        "all" -> exactKeys(selection, setOf("type"))

        else -> false
    }
}

internal data class AtomicRenderSnapshot(
    /** Original validated wire payload for controlled-prop delivery. */
    val atomicRenderJson: String,
    val viewUpdateJson: String,
    val documentRevision: ULong,
    val stateRevision: ULong,
    val scalarLength: Int,
    val scalarSelection: IntArray?,
    val activeState: JSONObject,
    val historyState: JSONObject,
    val positionEpoch: String?
)

internal data class PinnedAtomicRenderSnapshot(
    val snapshot: AtomicRenderSnapshot,
    val positionEpoch: String?
)

internal fun parseAtomicRenderSnapshot(json: String): AtomicRenderSnapshot? {
    return try {
        val jsonObject = JSONObject(json)
        val requiredKeys =
            setOf(
                "renderBlocks",
                "renderPatch",
                "selection",
                "activeState",
                "historyState",
                "documentVersion",
                "stateRevision",
                "scalarLength",
                "documentIsEmpty"
            )
        val renderBlocks = jsonObject.opt("renderBlocks")
        val renderPatch = jsonObject.opt("renderPatch")
        val validRenderPayload =
            (validRenderBlocks(renderBlocks) && renderPatch === JSONObject.NULL) ||
                (
                    renderBlocks === JSONObject.NULL && renderPatch is JSONObject &&
                        validRenderPatch(renderPatch)
                    )
        if (!onlyKeys(jsonObject, requiredKeys + "positionEpoch") ||
            requiredKeys.any { !jsonObject.has(it) } ||
            !validRenderPayload ||
            !validSelection(jsonObject.opt("selection")) ||
            !validActiveState(jsonObject.opt("activeState")) ||
            exactBool(jsonObject.opt("documentIsEmpty")) == null
        ) {
            return null
        }
        val history = jsonObject.opt("historyState") as? JSONObject ?: return null
        if (!exactKeys(history, setOf("canUndo", "canRedo")) ||
            exactBool(history.opt("canUndo")) == null ||
            exactBool(history.opt("canRedo")) == null
        ) {
            return null
        }
        val revision = ulongField(jsonObject, "documentVersion") ?: return null
        val state = ulongField(jsonObject, "stateRevision") ?: return null
        val scalarLength = scalarField(jsonObject, "scalarLength") ?: return null
        val scalarSelection = scalarSelection(jsonObject.opt("selection"))
        val positionEpoch = if (jsonObject.has("positionEpoch")) {
            canonicalV2U64(jsonObject.opt("positionEpoch") as? String) ?: return null
        } else {
            null
        }
        jsonObject.remove("positionEpoch")
        val atomicRenderJson = jsonObject.toString()
        jsonObject.remove("scalarLength")
        AtomicRenderSnapshot(
            atomicRenderJson,
            jsonObject.toString(),
            revision,
            state,
            scalarLength,
            scalarSelection,
            JSONObject(jsonObject.getJSONObject("activeState").toString()),
            JSONObject(history.toString()),
            positionEpoch
        )
    } catch (_: Exception) {
        null
    }
}
