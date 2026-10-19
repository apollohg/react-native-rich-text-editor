package com.apollohg.editor.viewer

import com.apollohg.editor.tables.TableCellLayoutStore
import java.lang.ref.WeakReference
import java.util.Collections
import java.util.IdentityHashMap
import java.util.WeakHashMap

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

private val EMPTY_GEOMETRY_DIGEST: String by lazy { sha256("") }

private fun geometryDigest(parts: List<String>): String =
    if (parts.isEmpty()) EMPTY_GEOMETRY_DIGEST else sha256(parts.joinToString("|"))

internal fun cellShapeStyleDigest(
    theme: PreparedProseTheme,
    nativeFontRevision: Long,
    fontEnvironmentRevision: Long
): String = sha256(
    listOf(
        theme.density, theme.fontDensity, theme.text, theme.paragraph, theme.headings,
        theme.blockquote, theme.code, theme.insetTopPx, theme.insetRightPx, theme.insetBottomPx,
        theme.insetLeftPx,
        theme.listIndentPx,
        theme.listBaseIndentMultiplier,
        theme.listItemSpacingPx,

        theme.listSpacingAfterPx,
        theme.listMarkerColor,
        theme.listMarkerScale,
        theme.listMarkerGapPx,

        theme.quoteIndentPx,
        theme.quoteBorderColor,
        theme.quoteBorderWidthPx,
        theme.quoteMarkerGapPx,

        theme.codeBackground,
        theme.codeRadiusPx,
        theme.codePaddingHorizontalPx,
        theme.codePaddingVerticalPx,

        theme.ruleColor, theme.ruleThicknessPx, theme.ruleMarginPx, theme.link, theme.mention,
        theme.atomPaddingHorizontalPx, theme.atomPaddingVerticalPx,
        theme.orderedListMarker?.let { marker ->
            marker.schemes.joinToString(",") { it.name } + ":${marker.suffix}"
        },
        theme.sourceTheme?.styleSheet?.shapingDigest(),
        theme.tableDirection
    ).joinToString("|") + "|$nativeFontRevision|$fontEnvironmentRevision"
)

internal fun cellShapeKey(
    contentKey: String,
    document: ViewerDocument,
    widthPx: Int,
    theme: PreparedProseTheme,
    density: Float,
    styleDigest: String,
    context: PreparedCellShapeBuildContext
): PreparedCellShapeKey {
    val safeWidth = widthPx.coerceAtLeast(1)
    val digest = context.geometryDigest(document, theme)
    return PreparedCellShapeKey(
        contentKey,
        safeWidth,
        density.toRawBits(),
        styleDigest,
        geometryDigest(digest.atoms),
        geometryDigest(digest.images)
    )
}

internal class PreparedCellGeometryDigest(val atoms: List<String>, val images: List<String>)

private data class PreparedCellGeometryDigestKey(
    val semanticKey: String,
    val atomGeneration: String?,
    val atomRevision: String?
) {
    constructor(document: ViewerDocument, theme: PreparedProseTheme) : this(
        document.semanticKey,
        theme.viewerAtoms?.generation,
        theme.viewerAtoms?.revision
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

internal class PreparedCellShapeBuildContext internal constructor(
    private val catalog: PreparedCellShapeCatalog
) {
    private val resolved = linkedMapOf<PreparedCellShapeKey, WeakReference<PreparedCellShape>>()
    private val pins = WeakHashMap<PreparedCellShape, Unit>()
    private val geometryDigests =
        mutableMapOf<PreparedCellGeometryDigestKey, PreparedCellGeometryDigest>()
    private var closed = false
    internal var geometryBlockVisitsForTesting = 0
        private set

    fun fork(): PreparedCellShapeBuildContext = catalog.newBuildContext()

    fun geometryDigest(
        document: ViewerDocument,
        theme: PreparedProseTheme
    ): PreparedCellGeometryDigest {
        geometryDigests[PreparedCellGeometryDigestKey(document, theme)]?.let { return it }
        val atoms = mutableListOf<String>()
        val images = mutableListOf<String>()
        val starts = mutableMapOf<String, Pair<Int, Int>>()
        document.forEachBlockInSourceOrder({ current, index, block ->
            if (index == 0) starts[current.semanticKey] = atoms.size to images.size
            geometryBlockVisitsForTesting += 1
            val custom = block.inlines.singleOrNull() as? ViewerInline.Atom
            if (block.isBlockAtom && custom != null &&
                theme.viewerAtoms?.nodeTypes?.contains(custom.nodeType) == true
            ) {
                atoms.add(
                    "${custom.nodeType}:" +
                        "${theme.viewerAtoms.measurements[custom.docPos.toString()]}:" +
                        theme.viewerAtoms.estimatedHeights[custom.nodeType]
                )
            }
            ViewerImageAttachment.sourceAndDeclaredSize(block)?.let { (id, source, declared) ->
                images.add("$source:${declared ?: ViewerImageIntrinsicStore.shared.size(id)}")
            }
        }, finishing = { current ->
            val (atomStart, imageStart) = starts[current.semanticKey] ?: (atoms.size to images.size)
            geometryDigests[PreparedCellGeometryDigestKey(current, theme)] =
                PreparedCellGeometryDigest(
                    atoms.subList(atomStart, atoms.size).toList(),
                    images.subList(imageStart, images.size).toList()
                )
        })
        return geometryDigests.getValue(PreparedCellGeometryDigestKey(document, theme))
    }

    fun resolve(
        key: PreparedCellShapeKey,
        build: () -> PreparedProseLayout,
        retainShape: Boolean = true,
        bind: (PreparedCellShape) -> PreparedProseLayout?
    ): PreparedProseLayout = requireNotNull(
        resolution(key, { completedPreparation(build()) }, retainShape) {
            completedPreparation(bind(it))
        }.runToCompletion()
    )

    fun resolution(
        key: PreparedCellShapeKey,
        build: () -> Preparation,
        retainShape: Boolean = true,
        bind: (PreparedCellShape) -> Preparation
    ): Preparation = iterator {
        yield(PreparationStep.Complete(resolvedLayout(key, build, retainShape, bind)))
    }

    private suspend fun SequenceScope<PreparationStep>.resolvedLayout(
        key: PreparedCellShapeKey,
        build: () -> Preparation,
        retainShape: Boolean,
        bind: (PreparedCellShape) -> Preparation
    ): PreparedProseLayout {
        if (closed) return requireNotNull(await(build()))
        cachedShape(key)?.let { shape ->
            await(bind(shape))?.let { return it.copy(cellShape = shape) }
        }
        val fresh = requireNotNull(await(build()))
        if (!retainShape) return fresh
        val shape = PreparedCellShape.fromBound(key, fresh)
        catalog.stageForBuild(shape)
        remember(shape)
        return fresh.copy(cellShape = shape)
    }

    fun resolveCached(
        key: PreparedCellShapeKey,
        bind: (PreparedCellShape) -> PreparedProseLayout?
    ): PreparedProseLayout? {
        val shape = cachedShape(key) ?: return null
        return bind(shape)?.copy(cellShape = shape)
    }

    private fun cachedShape(key: PreparedCellShapeKey): PreparedCellShape? = if (closed) {
        null
    } else {
        resolved[key]?.get() ?: catalog.acquireForBuild(key)?.also(::remember)
    }

    private fun remember(shape: PreparedCellShape) {
        resolved.remove(shape.key)?.get()?.let(::release)
        while (resolved.size >= TableCellLayoutStore.MAXIMUM_RESIDENT_LAYOUTS) {
            val iterator = resolved.entries.iterator()
            val previous = iterator.next().value.get()
            iterator.remove()
            previous?.let(::release)
        }
        resolved[shape.key] = WeakReference(shape)
        pins[shape] = Unit
    }

    private fun release(shape: PreparedCellShape) {
        if (pins.remove(shape) != null) catalog.releaseBuildPins(listOf(shape))
    }

    internal fun close() {
        if (closed) return
        closed = true
        catalog.releaseBuildPins(pins.keys.toList())
        pins.clear()
        resolved.clear()
        geometryDigests.clear()
    }
}

/** Index only shapes reachable from parent artifacts retained by the parent cache. */
internal class PreparedCellShapeCatalog {
    private val lock = Any()
    private var entries: Map<PreparedCellShapeKey, WeakReference<PreparedCellShape>> = emptyMap()
    private val buildPins = WeakHashMap<PreparedCellShape, Int>()

    fun newBuildContext(): PreparedCellShapeBuildContext = PreparedCellShapeBuildContext(this)

    fun acquireForBuild(key: PreparedCellShapeKey): PreparedCellShape? = synchronized(lock) {
        entries[key]?.get()?.also { shape -> buildPins[shape] = (buildPins[shape] ?: 0) + 1 }
    }

    fun stageForBuild(shape: PreparedCellShape) = synchronized(lock) {
        buildPins[shape] = (buildPins[shape] ?: 0) + 1
    }

    fun synchronizeOwners(liveLayouts: Collection<PreparedProseLayout>) = synchronized(lock) {
        val next = linkedMapOf<PreparedCellShapeKey, PreparedCellShape>()
        liveLayouts.forEachRetainedLayout(visit = { layout ->
            layout.cellShape?.let { next.putIfAbsent(it.key, it) }
        })
        buildPins.keys.forEach { shape -> next.putIfAbsent(shape.key, shape) }
        entries = next.mapValues { WeakReference(it.value) }
    }

    fun releaseBuildPins(shapes: Collection<PreparedCellShape>) = synchronized(lock) {
        shapes.forEach { shape ->
            val count = buildPins[shape] ?: return@forEach
            if (count == 1) buildPins.remove(shape) else buildPins[shape] = count - 1
        }
    }

    internal val retainedBytes: Long get() = synchronized(lock) {
        entries = entries.filterValues { it.get() != null }
        entries.keys.sumOf { it.catalogMetadataBytes() }
    }
    internal val countForTesting: Int get() = synchronized(lock) {
        entries = entries.filterValues { it.get() != null }
        entries.size
    }
}

internal fun PreparedCellShapeKey.catalogMetadataBytes(): Long =
    112L + contentKey.length * 2L + styleDigest.length * 2L +
        atomGeometryDigest.length * 2L + imageGeometryDigest.length * 2L

internal fun PreparedProseLayout.cellShapeCatalogBytes(): Long {
    if (!hasTableSurfaces) {
        return cellShape?.let { it.key.catalogMetadataBytes() + it.retainedBytes } ?: 0L
    }
    val shapes = Collections.newSetFromMap(IdentityHashMap<PreparedCellShape, Boolean>())
    forEachRetainedLayout(visit = { layout -> layout.cellShape?.let { shapes.add(it) } })
    return shapes.sumOf { it.key.catalogMetadataBytes() + it.retainedBytes }
}

private fun PreparedProseLayout.sourceNeutralWrapperBytes(): Long = 256L + blocks.sumOf { block ->
    224L + block.fragments.size * 48L +
        if (block.imageAttachment == null) 0L else 128L
} + interactions.size * 192L + imageAttachments.size * 128L + viewerAtoms.size * 128L

/** Remove all source-qualified metadata before retaining a local shape. */
private fun PreparedProseLayout.localShape(): PreparedProseLayout = copy(
    key = key.copy(
        semanticKey = "cell-shape",
        generationIdentity = "cell-shape",
        semanticGenerationIdentity = "cell-shape"
    ),
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
    retainedBytes = ownRetainedBytes,
    tableRetainedBytesAtPreparation = 0L,
    interactions = interactions.map { it.copy(docPos = null, attrsJson = null) },
    accessibilityNodes = emptyList(),
    imageAttachments = imageAttachments.map { it.copy(id = "", ordinal = -1) },
    viewerAtoms = viewerAtoms.map { it.copy(docPos = 0, attrsJson = "") },
    cellShape = null,
    cellPreparation = null
)
