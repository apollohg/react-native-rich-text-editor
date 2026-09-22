package com.apollohg.editor.viewer

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
            "content", document, 100, PreparedProseTheme.resolve(themeJson, 1f), 1f, 0, 0
        )
        val second = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(themeJson, 1f), 1f, 0, 0
        )

        assertEquals(first, second)
    }

    @Test
    fun `font scale is a shape dependency even when the caller advances a font revision`() {
        val document = ViewerDocument("document", emptyList(), true, 0)

        val normal = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(null, 1f, 1f), 1f, 7, 0
        )
        val scaled = cellShapeKey(
            "content", document, 100, PreparedProseTheme.resolve(null, 1f, 1.5f), 1f, 7, 0
        )

        assertNotEquals(normal, scaled)
    }

    @Test
    fun `ordered list marker formatting is a shape dependency`() {
        val document = ViewerDocument("document", emptyList(), true, 0)
        val decimal = cellShapeKey(
            "content",
            document,
            100,
            PreparedProseTheme.resolve(
                """{"list":{"orderedMarker":{"schemes":["decimal"],"suffix":"."}}}""",
                1f
            ),
            1f,
            0,
            0
        )
        val roman = cellShapeKey(
            "content",
            document,
            100,
            PreparedProseTheme.resolve(
                """{"list":{"orderedMarker":{"schemes":["upperRoman"],"suffix":")"}}}""",
                1f
            ),
            1f,
            0,
            0
        )

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
