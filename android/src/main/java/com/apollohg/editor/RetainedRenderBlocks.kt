package com.apollohg.editor

import org.json.JSONArray
import org.json.JSONObject

private const val RENDER_ELEMENT_TYPE_FIELD = "type"
private const val RENDER_NODE_TYPE_FIELD = "nodeType"
private const val RENDER_TABLE_ID_FIELD = "tableId"
internal const val RENDER_DOC_POSITION_FIELD = "docPos"
private const val TABLE_ELEMENT_TYPE = "table"
private const val VOID_BLOCK_ELEMENT_TYPE = "voidBlock"
private val ATOM_ELEMENT_TYPES = setOf("opaqueInlineAtom", "opaqueBlockAtom")

internal class RetainedAtomNode(
    val nodeType: String,
    val isVoidBlock: Boolean,
    val documentPosition: Long?
)

internal class RetainedRenderBlock private constructor(
    private val encoded: ByteArray,
    val tableIds: List<String>?,
    val atomNodes: List<RetainedAtomNode>?
) {
    fun parse(): JSONArray = JSONArray(String(encoded, Charsets.UTF_8))

    fun contentEquals(other: RetainedRenderBlock): Boolean =
        this === other || encoded.contentEquals(other.encoded)

    companion object {
        fun from(block: JSONArray): RetainedRenderBlock {
            var tableIds: MutableList<String>? = null
            var atomNodes: MutableList<RetainedAtomNode>? = null
            var malformedTable = false
            var nonObjectElement = false
            for (index in 0 until block.length()) {
                val element = block.opt(index) as? JSONObject
                if (element == null) {
                    nonObjectElement = true
                    continue
                }
                val type = element.optString(RENDER_ELEMENT_TYPE_FIELD)
                if (type == TABLE_ELEMENT_TYPE) {
                    val tableId = element.opt(RENDER_TABLE_ID_FIELD) as? String
                    if (tableId == null) {
                        malformedTable = true
                    } else {
                        (tableIds ?: mutableListOf<String>().also { tableIds = it }).add(tableId)
                    }
                } else if (type == VOID_BLOCK_ELEMENT_TYPE || type in ATOM_ELEMENT_TYPES) {
                    (atomNodes ?: mutableListOf<RetainedAtomNode>().also { atomNodes = it }).add(
                        RetainedAtomNode(
                            element.optString(RENDER_NODE_TYPE_FIELD),
                            type == VOID_BLOCK_ELEMENT_TYPE,
                            exactV2U32(element.opt(RENDER_DOC_POSITION_FIELD) as? Number)?.toLong()
                        )
                    )
                }
            }
            return RetainedRenderBlock(
                block.toString().toByteArray(Charsets.UTF_8),
                if (malformedTable) null else tableIds ?: emptyList(),
                if (nonObjectElement) null else atomNodes ?: emptyList()
            )
        }
    }
}

internal class ResolvedRenderBlocks private constructor(
    val blocks: List<RetainedRenderBlock>,
    private val parsed: Array<JSONArray?>
) {
    val size: Int get() = blocks.size

    fun block(index: Int): JSONArray =
        parsed[index] ?: blocks[index].parse().also { parsed[index] = it }

    fun toJsonArray(): JSONArray = JSONArray().also { array ->
        for (index in blocks.indices) array.put(block(index))
    }

    fun contentEquals(other: List<RetainedRenderBlock>): Boolean =
        blocks.size == other.size && blocks.indices.all { blocks[it].contentEquals(other[it]) }

    companion object {
        fun retained(blocks: List<RetainedRenderBlock>): ResolvedRenderBlocks =
            ResolvedRenderBlocks(blocks, arrayOfNulls(blocks.size))

        fun from(renderBlocks: JSONArray): ResolvedRenderBlocks? {
            val parsed = arrayOfNulls<JSONArray>(renderBlocks.length())
            for (index in parsed.indices) {
                parsed[index] = renderBlocks.opt(index) as? JSONArray ?: return null
            }
            return ResolvedRenderBlocks(
                parsed.map {
                    RetainedRenderBlock.from(requireNotNull(it))
                },
                parsed
            )
        }

        fun merge(
            current: List<RetainedRenderBlock>,
            patch: ParsedRenderPatch
        ): ResolvedRenderBlocks? {
            if (patch.startIndex < 0 || patch.deleteCount < 0 ||
                patch.startIndex + patch.deleteCount > current.size
            ) {
                return null
            }
            val inserted = from(patch.renderBlocks) ?: return null
            val retainedSuffix = patch.startIndex + patch.deleteCount
            val parsed = arrayOfNulls<JSONArray>(current.size - patch.deleteCount + inserted.size)
            for (index in 0 until inserted.size) {
                parsed[patch.startIndex + index] = inserted.block(index)
            }
            return ResolvedRenderBlocks(
                current.subList(0, patch.startIndex) + inserted.blocks +
                    current.subList(retainedSuffix, current.size),
                parsed
            )
        }
    }
}
