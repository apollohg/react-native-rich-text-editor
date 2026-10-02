package com.apollohg.editor.viewer

import uniffi.editor_core.FfiTableRecord
import com.apollohg.editor.tables.TableSurfaceCell
import com.apollohg.editor.tables.TableSurfaceSource
import com.apollohg.editor.tables.EditorTableIndex
import com.apollohg.editor.ProseViewerConfiguration
import com.apollohg.editor.ProseViewerError
import com.apollohg.editor.ProseViewerSource
import java.security.MessageDigest
import org.json.JSONArray
import org.json.JSONObject
import uniffi.editor_core.FfiViewerCompileRequest
import uniffi.editor_core.FfiViewerElement
import uniffi.editor_core.FfiViewerTable
import uniffi.editor_core.FfiViewerMark
import uniffi.editor_core.FfiViewerSourceKind
import uniffi.editor_core.viewerCompile

internal data class ViewerListContext(
    val ordered: Boolean,
    /** Rust u32 list index retained exactly for interaction/accessibility consumers. */
    val index: Long,
    val kind: String?,
    val checked: Boolean,
    val isLast: Boolean,
    val isFirst: Boolean = false
)

/** Identifies the nearest list item and its first/final renderable leaf. */
internal data class ViewerListItemBoundary(
    val identity: Int,
    val nestingDepth: Int,
    val isFirstRenderableLeaf: Boolean,
    val isFinalRenderableLeaf: Boolean
)

/**
 * One list-item owner on a leaf's full semantic path, ordered outermost first.
 * Every owner keeps its own marker reservation even when a nested descendant is
 * the nearest list item exposed to interaction/accessibility consumers.
 */
internal data class ViewerListItemAncestor(
    val identity: Int,
    val context: ViewerListContext,
    val nestingDepth: Int,
    val isFirstRenderableLeaf: Boolean,
    val isFinalRenderableLeaf: Boolean
)

internal sealed interface ViewerInline {
    data class Text(val text: String, val marks: List<FfiViewerMark>) : ViewerInline

    /** Rust u32 document position retained exactly; drawing spans never own it. */
    data class Atom(
        val nodeType: String,
        val docPos: Long,
        val attrsJson: String,
        val label: String
    ) : ViewerInline
}

internal data class ViewerContainerAncestor(
    val identity: Int,
    val nodeType: String,
    val firstLeaf: Int,
    val lastLeaf: Int
)

internal data class ViewerBlock(
    val nodeType: String,
    val depth: Int,
    val inBlockquote: Boolean,
    val listContext: ViewerListContext?,
    val listItemBoundary: ViewerListItemBoundary?,
    val inlines: List<ViewerInline>,
    val listItemAncestors: List<ViewerListItemAncestor> = emptyList(),
    val outermostListItemIdentity: Int? = null,
    val outermostListItemIsLast: Boolean = false,
    val isBlockAtom: Boolean = false,
    val containers: List<ViewerContainerAncestor> = emptyList(),
    val language: String? = null,
    val table: FfiViewerTable? = null,
    val frameRecord: FfiTableRecord? = null
) {
    val tableKey: String? get() = frameRecord?.tableKey ?: table?.let { "t${it.tablePos}" }
    fun tableSource(): TableSurfaceSource? =
        frameRecord?.let(TableSurfaceSource::from)
            ?: table?.let(TableSurfaceSource::from)
}

/** Semantic positions live only in [ViewerInline.Atom], never in Android drawing spans. */
internal data class ViewerDocument(
    val semanticKey: String,
    val blocks: List<ViewerBlock>,
    val isEmpty: Boolean,
    val retainedBytes: Long,
    val trailingEmptyTextBlockCount: Int = 0,
    val tableAttributes: Map<String, JSONObject> = emptyMap(),
    val tableRecords: Map<String, FfiViewerTable> = emptyMap(),
    val preferredTextBlockName: String = "paragraph",
    val tablePresentationIdentities: Map<String, String> = emptyMap(),
    val frameIndex: EditorTableIndex? = null
) {
    fun tablePresentationIdentity(tableKey: String): String =
        tablePresentationIdentities[tableKey] ?: tableKey
}

internal data class ProseViewerRequest(
    val source: ProseViewerSource,
    val configuration: ProseViewerConfiguration,
    val nativeFontRevision: Long = 0,
    val fontEnvironmentRevision: Long = 0,
    val attachmentRevision: Long = 0,
    val tableGeometryRevision: Long = 0,
    val tableGeometryPolicy: TableGeometryPolicy = TableGeometryPolicy.EAGER,
    val tableMeasurementViewportHeightPx: Int = 0
) {
    val compiledCacheKey: String by lazy {
        sha256(
            listOf(
                source.value,
                configuration.configJson,
                configuration.imagePolicyJson.orEmpty(),
                if (configuration.imagesEnabled) "1" else "0",
                mentionPrefix(configuration.configJson).orEmpty(),
                source.kind
            ).joinToString("\u001f")
        )
    }
    val themeDigest: String by lazy { sha256(configuration.themeJson.orEmpty()) }

    /** Semantic publication identity; layout/font revisions deliberately do not enter it. */
    val semanticGenerationIdentity: String by lazy {
        sha256(
            listOf(
                source.kind,
                source.value,
                configuration.configJson,
                configuration.themeJson.orEmpty(),
                configuration.imagePolicyJson.orEmpty(),
                if (configuration.imagesEnabled) "1" else "0",
                if (configuration.collapsesWhenEmpty) "1" else "0",
                mentionPrefix.orEmpty()
            ).joinToString("\u001f")
        )
    }

    /** Immutable layout/cache identity including permitted state-only revisions. */
    val generationIdentity: String by lazy {
        sha256(
            listOf(
                semanticGenerationIdentity,
                attachmentRevision.toString(),
                nativeFontRevision.toString(),
                fontEnvironmentRevision.toString(),
                tableGeometryRevision.toString(),
                tableGeometryPolicy.stateValue.toString(),
                tableMeasurementViewportHeightPx.toString()
            ).joinToString("\u001f")
        )
    }
    val mentionPrefix: String? get() = mentionPrefix(configuration.configJson)
}

internal typealias DocumentCompiler = (ProseViewerRequest) -> ViewerDocument

private fun validViewerTables(elements: List<FfiViewerElement>, tableRecords: Map<String, FfiViewerTable>, pool: Map<String, JSONObject>): Boolean {
    data class Pending(val element: FfiViewerElement, val depth: Int, val start: Long, val end: Long)
    val pending = java.util.ArrayDeque<Pending>()
    elements.forEach { pending.add(Pending(it, 0, 0, 0xffff_ffffL)) }
    var records = 0L
    var slots = 0L
    val referenced = mutableSetOf<String>()
    while (pending.isNotEmpty()) {
        val entry = pending.removeLast()
        if (++records + pending.size > 7_000_000 || entry.depth > 1024) return false
        val tableId = (entry.element as? FfiViewerElement.Table)?.tableId ?: continue
        if (!referenced.add(tableId)) return false
        val table = tableRecords[tableId] ?: return false
        val rows = table.rows.toLong()
        val columns = table.columns.toLong()
        if (rows > 4_000_000 || columns > 4_000_000) return false
        slots += rows * columns
        if (slots > 4_000_000 || table.tablePos.toLong() < entry.start || table.sourceEnd.toLong() > entry.end ||
            table.sourceEnd <= table.tablePos || table.columnWidths.size.toLong() != columns ||
            table.columnWidths.any { it == 0u } || table.direction !in listOf(null, "ltr", "rtl") ||
            table.readOnlyDescendants != (entry.depth > 0) || !pool.containsKey(table.attrsKey)) return false
        if (table.failure != null) {
            if (rows != 0L || columns != 0L || table.cells.isNotEmpty() || table.sourceRows.isNotEmpty() ||
                table.syntheticRegions.isNotEmpty() || table.compatibilityDiagnostic != null) return false
            continue
        }
        records += table.cells.size + table.sourceRows.size + table.syntheticRegions.size
        if (records > 7_000_000) return false
        var previous = table.tablePos.toLong() + 1
        for (row in table.sourceRows) {
            if (row.sourcePos.toLong() < previous || row.sourceEnd <= row.sourcePos || row.sourceEnd >= table.sourceEnd || !pool.containsKey(row.attrsKey)) return false
            previous = row.sourceEnd.toLong()
        }
        val occupied = mutableSetOf<Long>()
        fun region(row: UInt, column: UInt, rowspan: UInt, colspan: UInt, key: String): Boolean {
            if (rowspan == 0u || colspan == 0u || row.toLong() + rowspan.toLong() > rows || column.toLong() + colspan.toLong() > columns || !pool.containsKey(key)) return false
            for (r in row.toLong() until row.toLong() + rowspan.toLong()) for (c in column.toLong() until column.toLong() + colspan.toLong()) {
                if (!occupied.add(r * columns + c)) return false
            }
            return true
        }
        previous = table.tablePos.toLong() + 1
        var rowIndex = 0
        for (cell in table.cells) {
            if (!region(cell.row, cell.column, cell.rowspan, cell.colspan, cell.attrsKey) || cell.sourcePos.toLong() < previous || cell.sourceEnd <= cell.sourcePos || cell.contentKey.isEmpty()) return false
            while (rowIndex < table.sourceRows.size && table.sourceRows[rowIndex].sourceEnd <= cell.sourcePos) rowIndex++
            val row = table.sourceRows.getOrNull(rowIndex) ?: return false
            if (row.sourcePos >= cell.sourcePos || row.sourceEnd <= cell.sourceEnd) return false
            previous = cell.sourceEnd.toLong()
            cell.elements.forEach { pending.add(Pending(it, entry.depth + 1, cell.sourcePos.toLong() + 1, cell.sourceEnd.toLong() - 1)) }
        }
        for (gap in table.syntheticRegions) if (!region(gap.row, gap.column, gap.rowspan, gap.colspan, gap.attrsKey)) return false
    }
    return referenced == tableRecords.keys
}

internal fun compileWithRust(request: ProseViewerRequest): ViewerDocument {
    val result = viewerCompile(
        FfiViewerCompileRequest(
            sourceKind = if (request.source is ProseViewerSource.Html) FfiViewerSourceKind.HTML else FfiViewerSourceKind.JSON,
            source = request.source.value,
            configJson = request.configuration.configJson,
            imagesEnabled = request.configuration.imagesEnabled,
            mentionPrefix = request.mentionPrefix
        )
    )
    try {
        result.error?.let { throw ProseViewerError.compiler(it.domain, it.code, it.message) }
        val compiled = result.value ?: throw ProseViewerError.compiler("viewer", "MISSING_COMPILED_DOCUMENT", "The compiler returned neither a document nor an error.")
        val semanticKey = compiled.semanticKey()
        if (!semanticKey.matches(Regex("[0-9a-f]{64}"))) {
            throw ProseViewerError.compiler("viewer", "INVALID_SEMANTIC_KEY", "The compiler returned an invalid semantic key.")
        }
        val elements = compiled.elements()
        val tableRecords = mutableMapOf<String, FfiViewerTable>()
        for (record in compiled.tableRecords()) {
            if (tableRecords.put("t${record.tablePos}", record) != null) {
                throw ProseViewerError.compiler("viewer", "INVALID_TABLE_RECORD", "The compiler returned duplicate semantic table records.")
            }
        }
        val tableAttributes = parseTableAttributes(JSONObject(compiled.tableAttributes()))
        if (tableAttributes == null || !validViewerTables(elements, tableRecords, tableAttributes)) {
            throw ProseViewerError.compiler("viewer", "INVALID_TABLE_RECORD", "The compiler returned an invalid semantic record.")
        }
        validateAdmittedAttachments(elements, tableRecords)
        val isEmpty = compiled.isEmpty()
        val preferredTextBlockName = compiled.preferredTextBlockName()
        return ViewerDocument(
            semanticKey = semanticKey,
            blocks = lowerElements(elements, preferredTextBlockName, tableRecords, isEmpty),
            isEmpty = isEmpty,
            retainedBytes = compiled.retainedBytesDecimal().toLongOrNull() ?: 0,
            trailingEmptyTextBlockCount = compiled.trailingEmptyTextBlockCount().toInt(),
            tableAttributes = tableAttributes,
            tableRecords = tableRecords,
            preferredTextBlockName = preferredTextBlockName
        )
    } finally {
        result.destroy()
    }
}

private fun validateAdmittedAttachments(
    elements: List<FfiViewerElement>,
    tableRecords: Map<String, FfiViewerTable>
) {
    var count = 0
    fun countAttachments(elements: List<FfiViewerElement>) {
        elements.forEach { element ->
            val atom = element as? FfiViewerElement.BlockAtom ?: return@forEach
            if (ViewerImageAttachment.sourceAndDeclaredSize(atom.nodeType, u32(atom.docPos), atom.attrsJson) == null) {
                return@forEach
            }
            count += 1
            if (count > ViewerImageAttachment.MAXIMUM_ADMITTED_ATTACHMENTS) {
                throw ProseViewerError.compiler(
                    "viewer",
                    "ATTACHMENT_LIMIT_EXCEEDED",
                    "The document exceeds the maximum admitted image attachment count."
                )
            }
        }
    }

    countAttachments(elements)
    tableRecords.values.forEach { table ->
        table.cells.forEach { cell -> countAttachments(cell.elements) }
    }
}

private fun lowerElements(
    elements: List<FfiViewerElement>,
    preferredTextBlockName: String,
    tableRecords: Map<String, FfiViewerTable>,
    isEmpty: Boolean,
    frameIndex: EditorTableIndex? = null
): List<ViewerBlock> {
    data class Builder(
        val nodeType: String,
        val depth: Int,
        val listContext: ViewerListContext?,
        val listItemIdentity: Int?,
        val listItemContext: ViewerListContext?,
        val identity: Int,
        val language: String? = null,
        val inlines: MutableList<ViewerInline> = mutableListOf()
    )

    val stack = mutableListOf<Builder>()
    val rendered = mutableListOf<ViewerBlock>()
    // A list item's terminal spacing belongs after its own direct leaves,
    // before a child list begins. Descendant leaves are retained only as a
    // fallback for an item whose sole renderable content is nested.
    val directLeavesByListItem = mutableMapOf<Int, MutableList<Int>>()
    val descendantLeavesByListItem = mutableMapOf<Int, MutableList<Int>>()
    val listItemDepths = mutableMapOf<Int, Int>()
    var nextListItemIdentity = 0
    var nextContainerIdentity = 0

    fun nearestListContext(builders: List<Builder>): ViewerListContext? =
        builders.asReversed().firstNotNullOfOrNull {
            it.listContext
        }
    fun listItemAncestors(builders: List<Builder>): List<ViewerListItemAncestor> =
        builders.mapNotNull { builder ->
            val identity = builder.listItemIdentity ?: return@mapNotNull null
            val context = builder.listItemContext ?: return@mapNotNull null
            ViewerListItemAncestor(identity, context, builder.depth, false, false)
        }
    fun appendLeaf(
        nodeType: String,
        depth: Int,
        inlines: List<ViewerInline>,
        ancestors: List<Builder>,
        isBlockAtom: Boolean = false,
        table: FfiViewerTable? = null,
        frameRecord: FfiTableRecord? = null
    ) {
        val itemAncestors = listItemAncestors(ancestors)
        rendered += ViewerBlock(
            nodeType = nodeType,
            depth = depth,
            inBlockquote = ancestors.any { it.nodeType == "blockquote" },
            listContext = nearestListContext(ancestors),
            listItemBoundary = null,
            inlines = inlines,
            isBlockAtom = isBlockAtom,
            table = table,
            frameRecord = frameRecord,
            language = ancestors.lastOrNull()?.language,
            containers = ancestors.filter {
                it.nodeType in CONTAINER_BLOCKS &&
                    it.nodeType != "doc"
            }.map { ViewerContainerAncestor(it.identity, it.nodeType, 0, 0) },
            listItemAncestors = itemAncestors,
            outermostListItemIdentity = itemAncestors.firstOrNull()?.identity,
            outermostListItemIsLast = itemAncestors.firstOrNull()?.context?.isLast == true
        )
        itemAncestors.forEach { ancestor ->
            descendantLeavesByListItem.getOrPut(ancestor.identity) { mutableListOf() } +=
                rendered.lastIndex
        }
        itemAncestors.lastOrNull()?.let { nearest ->
            directLeavesByListItem.getOrPut(nearest.identity) { mutableListOf() } +=
                rendered.lastIndex
        }
    }

    elements.forEach { element ->
        when (element) {
            is FfiViewerElement.Table -> {
                val record = frameIndex?.record(element.tableId)
                val table = tableRecords[element.tableId]
                if (record == null && table == null) throw ProseViewerError.compiler("viewer", "INVALID_TABLE_RECORD", "The compiler returned a dangling semantic table reference.")
                appendLeaf("table", stack.lastOrNull()?.depth ?: 0, emptyList(), stack, true, table, record)
            }
            is FfiViewerElement.BlockStart -> {
                val context = listContext(element.listContextJson)
                if (context?.isFirst == true) {
                    stack +=
                        Builder(
                            if (context.kind ==
                                "task"
                            ) {
                                "taskList"
                            } else if (context.ordered) {
                                "orderedList"
                            } else {
                                "bulletList"
                            },
                            element.depth.toInt(),
                            null,
                            null,
                            null,
                            nextContainerIdentity++
                        )
                }
                val identity = if (context != null) nextListItemIdentity++ else null
                identity?.let { listItemDepths[it] = element.depth.toInt() }
                stack += Builder(
                    element.nodeType,
                    element.depth.toInt(),
                    context,
                    identity,
                    if (identity == null) null else context,
                    nextContainerIdentity++,
                    element.language
                )
            }

            is FfiViewerElement.TextRun -> stack.lastOrNull()?.inlines?.add(
                ViewerInline.Text(element.text, element.marks)
            )

            is FfiViewerElement.InlineAtom -> stack.lastOrNull()?.inlines?.add(
                ViewerInline.Atom(
                    element.nodeType,
                    u32(element.docPos),
                    element.attrsJson,
                    element.label
                )
            )

            is FfiViewerElement.BlockAtom -> appendLeaf(
                element.nodeType,
                stack.lastOrNull()?.depth ?: 0,
                listOf(
                    ViewerInline.Atom(
                        element.nodeType,
                        u32(element.docPos),
                        element.attrsJson,
                        element.label
                    )
                ),
                stack,
                isBlockAtom = true
            )

            FfiViewerElement.BlockEnd -> {
                val builder = stack.removeLastOrNull() ?: return@forEach
                // Containers are represented by inherited context. Every text block,
                // including an empty paragraph, remains a leaf for list boundaries.
                if (builder.nodeType !in CONTAINER_BLOCKS && builder.listItemIdentity == null) {
                    appendLeaf(
                        builder.nodeType,
                        builder.depth,
                        builder.inlines,
                        stack + builder
                    )
                }
                if (builder.listItemContext?.isLast == true &&
                    stack.lastOrNull()?.nodeType in
                    setOf("bulletList", "orderedList", "taskList")
                ) {
                    stack.removeLastOrNull()
                }
            }
        }
    }
    descendantLeavesByListItem.forEach { (identity, descendantLeaves) ->
        val leaves =
            directLeavesByListItem[identity]?.takeIf { it.isNotEmpty() } ?: descendantLeaves
        val first = leaves.firstOrNull() ?: return@forEach
        val final = leaves.last()
        leaves.forEach { index ->
            val updatedAncestors = rendered[index].listItemAncestors.map { ancestor ->
                if (ancestor.identity == identity) {
                    ancestor.copy(
                        nestingDepth = listItemDepths[identity] ?: ancestor.nestingDepth,
                        isFirstRenderableLeaf = index == first,
                        isFinalRenderableLeaf = index == final
                    )
                } else {
                    ancestor
                }
            }
            val nearest = updatedAncestors.lastOrNull()
            rendered[index] = rendered[index].copy(
                listItemBoundary = nearest?.let {
                    ViewerListItemBoundary(
                        it.identity,
                        it.nestingDepth,
                        it.isFirstRenderableLeaf,
                        it.isFinalRenderableLeaf
                    )
                },
                listItemAncestors = updatedAncestors
            )
        }
    }
    val containerLeaves = mutableMapOf<Int, MutableList<Int>>()
    rendered.forEachIndexed { index, block ->
        block.containers.forEach {
            containerLeaves.getOrPut(it.identity) { mutableListOf() } +=
                index
        }
    }
    rendered.indices.forEach { index ->
        rendered[index] = rendered[index].copy(
            containers = rendered[index].containers.map {
                val leaves = containerLeaves.getValue(it.identity)
                it.copy(firstLeaf = leaves.first(), lastLeaf = leaves.last())
            }
        )
    }
    val fallback = if (rendered.isEmpty() &&
        !isEmpty
    ) {
        listOf(ViewerBlock("paragraph", 0, false, null, null, emptyList()))
    } else {
        rendered
    }
    val admittedAttachmentCount = fallback.count { block ->
        block.nodeType == "image" && ViewerImageAttachment.sourceAndDeclaredSize(block) != null
    }
    if (admittedAttachmentCount > ViewerImageAttachment.MAXIMUM_ADMITTED_ATTACHMENTS) {
        throw ProseViewerError.compiler(
            "viewer",
            "ATTACHMENT_LIMIT_EXCEEDED",
            "The document exceeds the maximum admitted image attachment count."
        )
    }
    return fallback

}

internal fun ViewerDocument.cellSupportsBackgroundPreparation(cell: TableSurfaceCell, tableId: String): Boolean {
    var depth = 0
    var plain = true
    for (element in cell.elements) {
        when (element) {
            is FfiViewerElement.BlockStart -> {
                if (element.nodeType == "image") plain = false
                depth++
            }
            FfiViewerElement.BlockEnd -> if (depth == 0) plain = false else depth--
            is FfiViewerElement.TextRun -> if (depth == 0) plain = false
            is FfiViewerElement.InlineAtom, is FfiViewerElement.BlockAtom, is FfiViewerElement.Table -> plain = false
        }
        if (!plain) break
    }
    if (plain && depth == 0) return true
    return cellDocument(cell, tableId).blocks.all { block ->
        !block.isBlockAtom && block.nodeType != "image" && block.tableKey == null && block.inlines.none { it is ViewerInline.Atom }
    }
}

internal const val INVALID_CELL_SOURCE_INDEX = -1
private const val CELL_CONTENT_HASH_LENGTH = 64

private fun cellSemanticKey(parent: String, tableId: String, cell: TableSurfaceCell): String =
    "$parent:$tableId:${cell.sourceIndex}:${cell.contentKey}"

internal class PlainTableCellDocument private constructor(
    val semanticKey: String,
    private val originalText: String?,
    private val depth: UShort,
    private val language: String?,
    private val preferredTextBlockName: String
) {
    val text: String get() = originalText.orEmpty()

    fun materialize(): ViewerDocument {
        val elements = buildList {
            add(FfiViewerElement.BlockStart("paragraph", language, depth, null))
            originalText?.let { add(FfiViewerElement.TextRun(it, emptyList())) }
            add(FfiViewerElement.BlockEnd)
        }
        return ViewerDocument(semanticKey,
            lowerElements(elements, preferredTextBlockName, emptyMap(), false),
            false, 0, preferredTextBlockName = preferredTextBlockName)
    }

    companion object {
        private const val EMPTY_PARAGRAPH_ELEMENTS = 2
        private const val TEXT_PARAGRAPH_ELEMENTS = 3

        fun capture(parent: ViewerDocument, cell: TableSurfaceCell, tableId: String): PlainTableCellDocument? {
            val elements = cell.elements
            if (elements.size != EMPTY_PARAGRAPH_ELEMENTS && elements.size != TEXT_PARAGRAPH_ELEMENTS) return null
            val start = elements.first() as? FfiViewerElement.BlockStart ?: return null
            if (start.nodeType != "paragraph" || start.listContextJson != null || elements.last() != FfiViewerElement.BlockEnd) return null
            val text = if (elements.size == TEXT_PARAGRAPH_ELEMENTS) {
                val run = elements[1] as? FfiViewerElement.TextRun ?: return null
                if (run.marks.isNotEmpty()) return null
                run.text
            } else null
            return PlainTableCellDocument(cellSemanticKey(parent.semanticKey, tableId, cell),
                text, start.depth, start.language, parent.preferredTextBlockName)
        }
    }
}

internal fun cellSemanticSourceIndex(semanticKey: String, validateContentHash: Boolean = true): Int {
    val hashSeparator = semanticKey.length - CELL_CONTENT_HASH_LENGTH - 1
    if (hashSeparator <= 0 || semanticKey[hashSeparator] != ':') return INVALID_CELL_SOURCE_INDEX
    if (validateContentHash) {
        for (index in hashSeparator + 1 until semanticKey.length) {
            val char = semanticKey[index]
            if (char !in '0'..'9' && char !in 'a'..'f') return INVALID_CELL_SOURCE_INDEX
        }
    }
    val indexStart = semanticKey.lastIndexOf(':', hashSeparator - 1) + 1
    if (indexStart <= 0 || indexStart == hashSeparator) return INVALID_CELL_SOURCE_INDEX
    if (semanticKey[indexStart] == '0' && indexStart + 1 != hashSeparator) return INVALID_CELL_SOURCE_INDEX
    var sourceIndex = 0
    for (index in indexStart until hashSeparator) {
        val char = semanticKey[index]
        if (char !in '0'..'9') return INVALID_CELL_SOURCE_INDEX
        val digit = char - '0'
        if (sourceIndex > (Int.MAX_VALUE - digit) / 10) return INVALID_CELL_SOURCE_INDEX
        sourceIndex = sourceIndex * 10 + digit
    }
    return sourceIndex
}

internal fun ViewerDocument.cellDocument(cell: TableSurfaceCell, tableId: String): ViewerDocument {
    val elements = if (frameIndex == null) cell.elements else cell.elements.map { element ->
        fun absolute(relative: UInt): UInt = requireNotNull(frameIndex.absoluteDocPos(tableId, cell.sourceIndex, relative))
        when (element) {
            is FfiViewerElement.InlineAtom -> element.copy(docPos = absolute(element.docPos))
            is FfiViewerElement.BlockAtom -> element.copy(docPos = absolute(element.docPos))
            else -> element
        }
    }
    val nestedKeys = elements.filterIsInstance<FfiViewerElement.Table>().mapTo(mutableSetOf()) { it.tableId }
    val nestedIndex = if (nestedKeys.isEmpty()) null else frameIndex?.subtree(nestedKeys)
    val records = linkedMapOf<String, FfiViewerTable>()
    val pending = java.util.ArrayDeque(nestedKeys)
    while (pending.isNotEmpty()) {
        val key = pending.removeLast()
        if (key in records) continue
        val record = tableRecords[key] ?: continue
        records[key] = record
        record.cells.flatMap { it.elements }.filterIsInstance<FfiViewerElement.Table>().forEach { pending.add(it.tableId) }
    }
    val attributes = records.values.flatMapTo(mutableSetOf()) { record ->
        listOf(record.attrsKey) + record.sourceRows.map { it.attrsKey } + record.cells.map { it.attrsKey }
    } + nestedIndex?.attributeObjects.orEmpty().keys
    val identities = records.keys + nestedIndex?.tableKeys.orEmpty()
    return copy(
        semanticKey = cellSemanticKey(semanticKey, tableId, cell),
        blocks = lowerElements(elements, preferredTextBlockName, tableRecords, elements.isEmpty(), frameIndex),
        isEmpty = cell.elements.isEmpty(),
        retainedBytes = 0,
        trailingEmptyTextBlockCount = 0,
        tableRecords = records,
        tableAttributes = attributes.mapNotNull { key -> tableAttributes[key]?.let { key to it } }.toMap(),
        frameIndex = nestedIndex,
        tablePresentationIdentities = identities.mapNotNull { key -> tablePresentationIdentities[key]?.let { key to it } }.toMap()
    )
}

private val CONTAINER_BLOCKS = setOf(
    "doc",
    "blockquote",
    "bulletList",
    "bullet_list",
    "orderedList",
    "ordered_list",
    "taskList",
    "task_list",
    "listItem",
    "list_item",
    "taskItem",
    "task_item"
)

internal fun listContext(json: String?): ViewerListContext? = runCatching {
    json ?: return@runCatching null
    val value = JSONObject(json)
    val index = if (value.has("index")) u32(value.opt("index")) else 1L
    ViewerListContext(
        value.optBoolean("ordered"),
        index,
        value.optionalString("kind"),
        value.optBoolean("checked"),
        value.optBoolean("isLast"),
        value.optBoolean("isFirst")
    )
}.getOrNull()

/**
 * `optString(key, null)` cannot express an absent value: its fallback is
 * declared non-null, and a present JSON null coerces to the string "null".
 */
internal fun JSONObject.optionalString(key: String): String? =
    if (isNull(key)) null else optString(key)

/** JSON and UniFFI may expose Rust u32 values through different Kotlin number types. */
private fun u32(value: Any?): Long {
    val parsed = value?.toString()?.toLongOrNull()
        ?: throw IllegalArgumentException("Expected an unsigned 32-bit semantic value.")
    require(parsed in 0L..0xFFFF_FFFFL) { "Semantic value is outside Rust u32 range." }
    return parsed
}

private fun mentionPrefix(configJson: String): String? = runCatching {
    val root = JSONObject(configJson)
    root.optJSONObject("mentions")?.optionalString("prefix") ?: root.optionalString("mentionPrefix")
}.getOrNull()

internal fun sha256(value: String): String = MessageDigest.getInstance(
    "SHA-256"
).digest(value.toByteArray(Charsets.UTF_8)).joinToString("") {
    "%02x".format(it)
}

private fun parseTableAttributes(value: Any?): Map<String, JSONObject>? {
    val raw = if (value == null) JSONObject() else value as? JSONObject ?: return null
    val pool = mutableMapOf<String, JSONObject>()
    val unique = mutableSetOf<String>()
    var bytes = 0L
    var entries = 0
    for (key in raw.keys()) {
        val json = raw.opt(key) as? String ?: return null
        entries++
        bytes += json.toByteArray(Charsets.UTF_8).size
        if (!Regex("^[0-9a-f]{64}$").matches(key) || entries > 7_000_000 || !unique.add(json) || bytes > 192L * 1024 * 1024) return null
        val root = try { JSONObject(json) } catch (_: Exception) { return null }
        val pending = java.util.ArrayDeque<Pair<Any, Int>>()
        pending.add(root to 0)
        var work = 0
        while (pending.isNotEmpty()) {
            val (item, depth) = pending.removeLast()
            if (++work > json.length || depth > 1024) return null
            when (item) {
                is Number -> if (!item.toDouble().isFinite()) return null
                is JSONObject -> item.keys().forEach { pending.add(item.get(it) to depth + 1) }
                is JSONArray -> for (index in 0 until item.length()) pending.add(item.get(index) to depth + 1)
            }
        }
        pool[key] = root
    }
    return pool.toMap()
}
