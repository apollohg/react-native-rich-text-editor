package com.apollohg.editor.viewer

import android.graphics.Rect
import com.apollohg.editor.tables.ViewerTableSurface
import java.util.IdentityHashMap

/** A source-neutral local shape. Parent artifacts remain the only persistent owners. */
internal data class PreparedCellShapeKey(
    val contentKey: String,
    val widthPx: Int,
    val densityBits: Int,
    val styleDigest: String,
    val atomGeometryDigest: String,
    val imageGeometryDigest: String,
    val format: Int = 1
)

internal fun cellShapeKey(
    contentKey: String,
    document: ViewerDocument,
    widthPx: Int,
    theme: PreparedProseTheme,
    density: Float,
    nativeFontRevision: Long,
    fontEnvironmentRevision: Long
): PreparedCellShapeKey {
    val safeWidth = widthPx.coerceAtLeast(1)
    fun atomGeometry(current: ViewerDocument): List<String> = buildList {
        current.blocks.forEach { block ->
            val custom = block.inlines.singleOrNull() as? ViewerInline.Atom
            if (block.isBlockAtom && custom != null && theme.viewerAtoms?.nodeTypes?.contains(custom.nodeType) == true) {
                // Resolve by current source position, but keep only local
                // geometry in the digest so an outer prose shift still hits.
                add(
                    "${custom.nodeType}:${theme.viewerAtoms.measurements[custom.docPos.toString()]}:" +
                        theme.viewerAtoms.estimatedHeights[custom.nodeType]
                )
            }
            block.table?.cells?.forEach { addAll(atomGeometry(current.cellDocument(it))) }
        }
    }
    fun imageGeometry(current: ViewerDocument): List<String> = buildList {
        current.blocks.forEach { block ->
            ViewerImageAttachment.sourceAndDeclaredSize(block)?.let { (id, source, declared) ->
                // The source-qualified ID is only a lookup handle. Its document
                // position must not participate in a reusable shape identity.
                add("$source:${declared ?: ViewerImageIntrinsicStore.shared.size(id)}")
            }
            block.table?.cells?.forEach { addAll(imageGeometry(current.cellDocument(it))) }
        }
    }
    val style = listOf(
        theme.density, theme.fontDensity, theme.text, theme.paragraph, theme.headings,
        theme.blockquote, theme.code, theme.insetTopPx, theme.insetRightPx, theme.insetBottomPx,
        theme.insetLeftPx, theme.listIndentPx, theme.listBaseIndentMultiplier, theme.listItemSpacingPx,
        theme.listSpacingAfterPx, theme.listMarkerColor, theme.listMarkerScale, theme.listMarkerGapPx,
        theme.quoteIndentPx, theme.quoteBorderColor, theme.quoteBorderWidthPx, theme.quoteMarkerGapPx,
        theme.codeBackground, theme.codeRadiusPx, theme.codePaddingHorizontalPx, theme.codePaddingVerticalPx,
        theme.ruleColor, theme.ruleThicknessPx, theme.ruleMarginPx, theme.link, theme.mention,
        theme.atomPaddingHorizontalPx, theme.atomPaddingVerticalPx,
        theme.orderedListMarker?.let { marker ->
            marker.schemes.joinToString(",") { it.name } + ":${marker.suffix}"
        },
        theme.sourceTheme?.styleSheet?.shapingDigest()
    ).joinToString("|") + "|$nativeFontRevision|$fontEnvironmentRevision"
    return PreparedCellShapeKey(
        contentKey,
        safeWidth,
        density.toRawBits(),
        sha256(style),
        sha256(atomGeometry(document).joinToString("|")),
        sha256(imageGeometry(document).joinToString("|"))
    )
}

internal class PreparedCellShape internal constructor(
    val key: PreparedCellShapeKey,
    internal val localLayout: PreparedProseLayout
) {
    val retainedBytes: Long get() = localLayout.sourceNeutralWrapperBytes()
    companion object {
        fun fromBound(key: PreparedCellShapeKey, layout: PreparedProseLayout): PreparedCellShape =
            PreparedCellShape(key, layout.localShape())
    }
}

/**
 * Build-scoped resolver. A hit is pinned by the authoritative parent cache
 * until the replacement parent has either been published or discarded.
 */
internal class PreparedCellShapeBuildContext internal constructor(
    private val catalog: PreparedCellShapeCatalog
) {
    private val resolved = mutableMapOf<PreparedCellShapeKey, PreparedCellShape>()
    private val pins = IdentityHashMap<PreparedCellShape, Unit>()
    private var closed = false

    fun resolve(
        key: PreparedCellShapeKey,
        build: () -> PreparedProseLayout,
        bind: (PreparedCellShape) -> PreparedProseLayout?
    ): PreparedProseLayout {
        val acquired = resolved[key] ?: catalog.acquireForBuild(key)?.also { shape ->
            resolved[key] = shape
            pins[shape] = Unit
        }
        acquired?.let { shape -> bind(shape)?.let { return it.copy(cellShape = shape) } }
        val fresh = build()
        val shape = PreparedCellShape.fromBound(key, fresh)
        resolved[key] = shape
        pins[shape] = Unit
        catalog.stageForBuild(shape)
        return fresh.copy(cellShape = shape)
    }

    internal fun close() {
        if (closed) return
        closed = true
        catalog.releaseBuildPins(pins.keys.toList())
        pins.clear()
        resolved.clear()
    }
}

/** Index only shapes reachable from parent artifacts retained by the parent cache. */
internal class PreparedCellShapeCatalog {
    private val lock = Any()
    private var entries: Map<PreparedCellShapeKey, PreparedCellShape> = emptyMap()
    private val buildPins = IdentityHashMap<PreparedCellShape, Int>()

    fun newBuildContext(): PreparedCellShapeBuildContext = PreparedCellShapeBuildContext(this)

    fun acquireForBuild(key: PreparedCellShapeKey): PreparedCellShape? = synchronized(lock) {
        entries[key]?.also { shape -> buildPins[shape] = (buildPins[shape] ?: 0) + 1 }
    }

    fun stageForBuild(shape: PreparedCellShape) = synchronized(lock) {
        buildPins[shape] = (buildPins[shape] ?: 0) + 1
    }

    fun synchronizeOwners(liveLayouts: Collection<PreparedProseLayout>) = synchronized(lock) {
        val next = linkedMapOf<PreparedCellShapeKey, PreparedCellShape>()
        liveLayouts.forEach { layout -> layout.collectCellShapes(next) }
        buildPins.keys.forEach { shape -> next.putIfAbsent(shape.key, shape) }
        entries = next
    }

    fun releaseBuildPins(shapes: Collection<PreparedCellShape>) = synchronized(lock) {
        shapes.forEach { shape ->
            val count = buildPins[shape] ?: return@forEach
            if (count == 1) buildPins.remove(shape) else buildPins[shape] = count - 1
        }
    }

    internal val retainedBytes: Long get() = synchronized(lock) {
        entries.keys.sumOf { it.catalogMetadataBytes() }
    }
    internal val countForTesting: Int get() = synchronized(lock) { entries.size }
}

internal fun PreparedCellShapeKey.catalogMetadataBytes(): Long =
    112L + contentKey.length * 2L + styleDigest.length * 2L +
        atomGeometryDigest.length * 2L + imageGeometryDigest.length * 2L

private fun PreparedProseLayout.collectCellShapes(
    destination: MutableMap<PreparedCellShapeKey, PreparedCellShape>
) {
    cellShape?.let { destination.putIfAbsent(it.key, it) }
    blocks.forEach { block ->
        block.tableSurface?.cells?.forEach { it.content.collectCellShapes(destination) }
    }
}

internal fun PreparedProseLayout.cellShapeCatalogBytes(): Long {
    val shapes = linkedMapOf<PreparedCellShapeKey, PreparedCellShape>()
    collectCellShapes(shapes)
    return shapes.entries.sumOf { (key, shape) -> key.catalogMetadataBytes() + shape.retainedBytes }
}

private fun PreparedProseLayout.sourceNeutralWrapperBytes(): Long =
    256L + blocks.sumOf { block ->
        224L + block.fragments.size * 48L +
            if (block.imageAttachment == null) 0L else 128L
    } + interactions.size * 192L + imageAttachments.size * 128L + viewerAtoms.size * 128L

/** Remove all source-qualified metadata before retaining a local shape. */
private fun PreparedProseLayout.localShape(): PreparedProseLayout = copy(
    key = key.copy(semanticKey = "cell-shape", generationIdentity = "cell-shape", semanticGenerationIdentity = "cell-shape"),
    blocks = blocks.map { block ->
        block.copy(
            fragments = block.fragments.map { fragment ->
                if (fragment.kind == PreparedProseFragmentKind.ATOM) {
                    fragment.copy(atomDocPos = null, atomAttrsJson = null)
                } else {
                    fragment
                }
            },
            imageAttachment = block.imageAttachment?.copy(id = "", ordinal = -1),
            tableSurface = null
        )
    },
    interactions = interactions.map { it.copy(docPos = null, attrsJson = null) },
    accessibilityNodes = emptyList(),
    imageAttachments = imageAttachments.map { it.copy(id = "", ordinal = -1) },
    viewerAtoms = viewerAtoms.map { it.copy(docPos = 0, attrsJson = "") },
    cellShape = null
)
