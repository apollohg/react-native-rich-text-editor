package com.apollohg.editor.viewer

import com.apollohg.editor.tables.TableCellLayoutStore
import java.lang.ref.WeakReference
import org.junit.Assert.assertNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
internal class PreparedCellShapeCatalogTest {
    @Test
    fun `owner synchronization traverses aliased roots once and preserves first shape`() {
        val catalog = PreparedCellShapeCatalog()
        val first = shape("shared-key")
        val later = shape("shared-key")
        val block = PreparedProseBlock(emptyList(), android.graphics.Rect(0, 0, 100, 20))
        var blockReads = 0
        val observed = object : AbstractList<PreparedProseBlock>() {
            override val size get() = 1
            override fun get(index: Int): PreparedProseBlock { blockReads++; return block }
        }
        val parent = owner(first).copy(blocks = observed)
        val duplicateRoots = 8
        blockReads = 0
        catalog.synchronizeOwners(List(duplicateRoots) { parent } + owner(later))
        assertEquals("Aliases must not traverse separate ownership walks", 1, blockReads)
        assertSame("The first live shape with a shared key wins", first, catalog.acquireForBuild(first.key))
        catalog.releaseBuildPins(listOf(first))
        catalog.synchronizeOwners(listOf(owner(later)))
        assertSame("Traversal state must not survive synchronization", later, catalog.acquireForBuild(later.key))
        catalog.releaseBuildPins(listOf(later))
        catalog.synchronizeOwners(emptyList())
        assertNull(catalog.acquireForBuild(first.key))
    }

    @Test
    fun `shared nested stores preserve depth first owners and refresh after eviction`() {
        val catalog = PreparedCellShapeCatalog()
        val nestedShape = shape("shared-nested")
        val laterShape = shape("shared-nested")
        val replacementShape = shape("shared-nested")
        val width = 100
        val store = TableCellLayoutStore(capacity = 1)
        val nested = owner(nestedShape)
        val record = com.apollohg.editor.tables.TableGridRecord("nested", 1, 1, listOf(width.toFloat()),
            listOf(com.apollohg.editor.tables.TableGridCell(0, 0, 0, contentKey = "nested")))
        val surface = com.apollohg.editor.tables.ViewerTableSurface("nested", record, width.toFloat(),
            com.apollohg.editor.tables.TableStyle(), isRightToLeft = false, layoutStore = store) { _, _ -> nested }
        val otherSurface = com.apollohg.editor.tables.ViewerTableSurface("other", width.toFloat(),
            surface.style, false, surface.layout, surface.cells, null)
        val bounds = android.graphics.Rect(0, 0, width, surface.layout.contentHeight.toInt())
        val firstRoot = bareLayout().copy(blocks = listOf(PreparedProseBlock(emptyList(), bounds,
            tableSurface = surface, tableBounds = bounds)))
        val secondRoot = bareLayout().copy(blocks = listOf(PreparedProseBlock(emptyList(), bounds,
            tableSurface = otherSurface, tableBounds = bounds)))
        val roots = listOf(firstRoot, secondRoot, firstRoot)
        val visited = mutableListOf<PreparedProseLayout>()
        val tables = mutableListOf<com.apollohg.editor.tables.ViewerTableSurface>()
        roots.forEachRetainedLayout(visited::add, tables::add)
        assertEquals(3, visited.size)
        assertSame(firstRoot, visited[0])
        assertSame(nested, visited[1])
        assertSame(secondRoot, visited[2])
        assertEquals(2, tables.size)
        assertSame(surface, tables[0])
        assertSame(otherSurface, tables[1])
        catalog.stageForBuild(laterShape)
        catalog.synchronizeOwners(roots + owner(laterShape))
        assertSame("Nested live owner precedes later roots and build pins", nestedShape,
            catalog.acquireForBuild(nestedShape.key))
        catalog.releaseBuildPins(listOf(nestedShape, laterShape))
        store.insert(owner(replacementShape).copy(key = nested.key.copy(semanticKey = "replacement")))
        catalog.synchronizeOwners(roots + owner(laterShape))
        assertSame("A later synchronization traverses the store's new resident", replacementShape,
            catalog.acquireForBuild(replacementShape.key))
        catalog.releaseBuildPins(listOf(replacementShape))
        catalog.synchronizeOwners(surface.cellShapeOwnerLayouts)
        assertSame("Displaced but resident entries remain valid source-neutral shape owners", replacementShape,
            catalog.acquireForBuild(replacementShape.key))
        catalog.releaseBuildPins(listOf(replacementShape))
        catalog.synchronizeOwners(emptyList())
        assertEquals(0, catalog.countForTesting)
    }

    @Test
    fun `parallel build contexts release evicted shapes before closing`() {
        val catalog = PreparedCellShapeCatalog()
        val workerCount = StaticLayoutAndroidProseLayoutEngine.MAX_TABLE_PREPARATION_WORKERS
        val capacity = 8
        val store = TableCellLayoutStore(capacity = capacity)
        val contexts = List(workerCount) { catalog.newBuildContext() }
        val uniqueCells = TableCellLayoutStore.MAXIMUM_RESIDENT_LAYOUTS + 1
        val references = java.util.concurrent.ConcurrentLinkedQueue<WeakReference<PreparedCellShape>>()
        val failure = AtomicReference<Throwable?>()
        val workers = contexts.mapIndexed { worker, context ->
            thread {
                try {
                    for (index in worker until uniqueCells step workerCount) {
                        val key = shape("parallel-$index").key
                        val prepared = context.resolve(key,
                            { bareLayout().copy(key = bareLayout().key.copy(semanticKey = key.contentKey)) }, { null })
                        references.add(WeakReference(requireNotNull(prepared.cellShape)))
                        store.insert(prepared)
                    }
                } catch (error: Throwable) { failure.set(error) }
            }
        }
        try {
            workers.forEach { it.join() }
            failure.get()?.let { throw it }
            repeat(8) { System.gc(); System.runFinalization() }
            assertEquals(capacity, store.count)
            assertEquals("Only resident cells may retain shapes before worker contexts close",
                capacity, references.count { it.get() != null })
        } finally { contexts.forEach { it.close() } }
    }

    @Test
    fun `open build contexts release unique shapes before closing`() {
        val catalog = PreparedCellShapeCatalog()
        val context = catalog.newBuildContext()
        val worker = context.fork()
        val uniqueCells = TableCellLayoutStore.MAXIMUM_RESIDENT_LAYOUTS + 1
        val gcAttempts = 8
        fun prepare(index: Int): WeakReference<PreparedCellShape> {
            val key = shape("cold-$index").key
            val prepared = (if (index % 2 == 0) context else worker).resolve(key,
                { bareLayout() }, { null })
            return WeakReference(requireNotNull(prepared.cellShape))
        }
        val references = (0 until uniqueCells).map(::prepare)
        repeat(gcAttempts) { System.gc(); System.runFinalization() }
        try {
            references.forEachIndexed { index, reference ->
                assertNull("Unowned cold cell $index must release while both contexts remain open", reference.get())
            }
        } finally {
            worker.close()
            context.close()
        }
    }

    @Test
    fun `concurrent build contexts retain an acquired shape until both close`() {
        val catalog = PreparedCellShapeCatalog()
        val shape = shape("shared")
        catalog.synchronizeOwners(listOf(owner(shape)))
        val first = catalog.newBuildContext()
        val second = catalog.newBuildContext()

        assertSame(shape, first.resolve(shape.key, { error("must hit") }) { it.localLayout }.cellShape)
        assertSame(shape, second.resolve(shape.key, { error("must hit") }) { it.localLayout }.cellShape)
        catalog.synchronizeOwners(emptyList())
        first.close()
        catalog.synchronizeOwners(emptyList())
        assertEquals(1, catalog.countForTesting)
        second.close()
        catalog.synchronizeOwners(emptyList())
        assertEquals(0, catalog.countForTesting)
    }

    @Test
    fun `retired final owner cannot be resurrected by a later build`() {
        val catalog = PreparedCellShapeCatalog()
        val old = shape("retired")
        catalog.synchronizeOwners(listOf(owner(old)))
        catalog.synchronizeOwners(emptyList())
        val context = catalog.newBuildContext()
        var builds = 0

        val result = context.resolve(old.key, {
            builds += 1
            bareLayout()
        }) { error("retired shape must not be offered") }

        assertEquals(1, builds)
        assertEquals(old.key, result.cellShape!!.key)
        context.close()
        catalog.synchronizeOwners(emptyList())
        assertEquals(0, catalog.countForTesting)
    }

    @Test
    fun `binding mismatch stages a replacement without leaking the acquired pin`() {
        val catalog = PreparedCellShapeCatalog()
        val old = shape("mismatch")
        catalog.synchronizeOwners(listOf(owner(old)))
        val context = catalog.newBuildContext()
        var builds = 0

        context.resolve(old.key, {
            builds += 1
            bareLayout()
        }) { null }
        catalog.synchronizeOwners(emptyList())
        context.close()
        context.close()
        catalog.synchronizeOwners(emptyList())

        assertEquals(1, builds)
        assertEquals(0, catalog.countForTesting)
    }

    @Test
    fun `equivalent freshly parsed stylesheet themes have the same shape key`() {
        val themeJson = """{"version":1,"styles":{"paragraph":{"marginBottom":8},"text":{"fontSize":18}},"rules":[{"path":["blockquote","paragraph"],"style":{"paddingLeft":3}}]}"""
        val document = ViewerDocument("document", emptyList(), true, 0)

        val first = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(themeJson, 1f), 1f,
            cellShapeStyleDigest(PreparedProseTheme.resolve(themeJson, 1f), 0, 0)
        )
        val second = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(themeJson, 1f), 1f,
            cellShapeStyleDigest(PreparedProseTheme.resolve(themeJson, 1f), 0, 0)
        )

        assertEquals(first, second)
    }

    @Test
    fun `font scale is a shape dependency even when the caller advances a font revision`() {
        val document = ViewerDocument("document", emptyList(), true, 0)

        val normal = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(null, 1f, 1f), 1f,
            cellShapeStyleDigest(PreparedProseTheme.resolve(null, 1f, 1f), 7, 0)
        )
        val scaled = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(null, 1f, 1.5f), 1f,
            cellShapeStyleDigest(PreparedProseTheme.resolve(null, 1f, 1.5f), 7, 0)
        )

        assertNotEquals(normal, scaled)
    }

    @Test
    fun `ordered list marker formatting is a shape dependency`() {
        val document = ViewerDocument("document", emptyList(), true, 0)
        val decimal = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(
                """{"list":{"orderedMarker":{"schemes":["decimal"],"suffix":"."}}}""",
                1f
            ), 1f,
            cellShapeStyleDigest(PreparedProseTheme.resolve(
                """{"list":{"orderedMarker":{"schemes":["decimal"],"suffix":"."}}}""",
                1f
            ), 0, 0)
        )
        val romanTheme = PreparedProseTheme.resolve(
            """{"list":{"orderedMarker":{"schemes":["upperRoman"],"suffix":")"}}}""",
            1f
        )
        val roman = cellShapeKey("content", document, 100, romanTheme, 1f, cellShapeStyleDigest(romanTheme, 0, 0))

        assertNotEquals(decimal, roman)
    }

    @Test
    fun `parent accounting includes source neutral shape wrappers`() {
        val cache = PreparedProseLayoutCache(byteBudget = 10_000)
        val shape = shape("accounted")
        val artifact = owner(shape)

        cache.value(artifact.key) { artifact }

        assertTrue(
            cache.retainedBytesForTesting >=
                artifact.retainedBytes + shape.key.catalogMetadataBytes() + shape.retainedBytes
        )
    }

    @Test
    fun `failed replacement build retires its final pinned shape without another cache mutation`() {
        val cache = PreparedProseLayoutCache(byteBudget = 10_000)
        val retainedShape = shape("failed-build")
        val retainedParent = owner(retainedShape)
        val replacementKey = retainedParent.key.copy(generationIdentity = "replacement")
        val acquired = CountDownLatch(1)
        val releaseFailure = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>()

        cache.value(retainedParent.key) { retainedParent }
        val build = thread(start = true) {
            runCatching {
                cache.valueWithCellShapeContext(replacementKey) { context ->
                    context.resolve(retainedShape.key, { error("must hit") }) { it.localLayout }
                    acquired.countDown()
                    check(releaseFailure.await(5, TimeUnit.SECONDS))
                    error("replacement failure")
                }
            }.onFailure(failure::set)
        }

        assertTrue(acquired.await(5, TimeUnit.SECONDS))
        cache.removeAllUnmounted()
        assertEquals(1, cache.cellShapeCatalogCountForTesting)
        releaseFailure.countDown()
        build.join(5_000)

        assertTrue(failure.get() is IllegalStateException)
        assertEquals(0, cache.cellShapeCatalogCountForTesting)
        assertEquals(0L, cache.cellShapeCatalogRetainedBytesForTesting)
        assertEquals(0L, cache.retainedBytesForTesting)
    }

    @Test
    fun `mounted-only catalog metadata is not reported as unmounted`() {
        val cache = PreparedProseLayoutCache(byteBudget = 10_000)
        val shape = shape("mounted")
        val key = bareLayout().key
        val artifact = owner(shape)
        val generation = FabricGenerationToken(FabricSurfaceToken(91, 910), key.generationIdentity, 1)

        cache.value(key) { artifact }
        assertSame(
            artifact,
            requireNotNull(cache.acquireForFabricMount(generation, 100, 1, allowCompletedFallback = true))
        )
        cache.removeAllUnmounted()

        assertEquals(0L, cache.retainedBytesForTesting)
        assertEquals(1, cache.cellShapeCatalogCountForTesting)
        cache.releaseLease(generation)
        assertEquals(0, cache.cellShapeCatalogCountForTesting)
    }

    private fun shape(content: String): PreparedCellShape {
        val key = PreparedCellShapeKey(content, 100, 1, "style", "atom", "image")
        return PreparedCellShape(key, bareLayout())
    }

    private fun owner(shape: PreparedCellShape): PreparedProseLayout = bareLayout().copy(cellShape = shape)

    private fun bareLayout(): PreparedProseLayout = PreparedProseLayout(
        ProseLayoutKey("test", 100, "theme", 0, 0, 1, 0, "generation"),
        100,
        1,
        emptyList(),
        retainedBytes = 1
    )
}
