package com.apollohg.editor.tables

import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.ProseLayoutKey
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

class TableCellLayoutStoreTest {
    private fun layout(name: String, bytes: Long = CELL_BYTES) = PreparedProseLayout(
        ProseLayoutKey(name, 100, "store-test", 0, 0, 1, 0, "store-test"),
        100,
        20,
        emptyList(),
        retainedBytes = bytes
    )

    @Test fun testStoreEvictsByRetainedBytes() {
        val store = TableCellLayoutStore(byteBudget = CELL_BYTES * 2)
        val first = layout("first")
        val second = layout("second")
        val third = layout("third")
        store.insert(first)
        store.insert(second)
        assertSame(first, store.value(first.key) { error("Resident cell was rebuilt") })
        store.insert(third)
        assertNull("Least-recent cell must be evicted", store.peek(second.key))
        assertSame(first, store.peek(first.key))
        assertSame(third, store.peek(third.key))
        assertEquals(CELL_BYTES * 2, store.unmountedRetainedBytes)
    }

    @Test fun testActiveInputCellStaysPreparedOffscreen() {
        val store = TableCellLayoutStore(byteBudget = CELL_BYTES)
        val active = layout("active")
        store.pin(active.key)
        store.insert(active)
        repeat(SCROLL_CELLS) { store.insert(layout("scroll-$it")) }
        assertSame("Scrolling must not evict the input cell", active, store.peek(active.key))
        assertTrue(store.unmountedRetainedBytes <= CELL_BYTES)
        store.unpin(active.key)
        assertNull("Unbinding restores normal eviction", store.peek(active.key))
    }

    @Test fun testOversizedUnmountedLayoutIsReturnedWithoutRetention() {
        val store = TableCellLayoutStore(byteBudget = CELL_BYTES)
        val oversized = layout("oversized", CELL_BYTES + 1)
        assertSame(oversized, store.value(oversized.key) { oversized })
        assertNull(store.peek(oversized.key))
        assertEquals(0L, store.unmountedRetainedBytes)
    }

    @Test fun testRebuiltReusedCellKeepsItsStoreKey() {
        val store = TableCellLayoutStore()
        val retainedKey = layout("before-rebind").key
        val rebuilt = layout("after-rebind")
        assertSame(rebuilt, store.value(retainedKey) { rebuilt })
        assertSame(
            "A reused cell keeps its lookup identity after eviction",
            rebuilt,
            store.peek(retainedKey)
        )
        assertSame(
            rebuilt,
            store.value(retainedKey) {
                error("The rebuilt cell was lost under a different artifact key")
            }
        )
    }

    @Test fun testBulkChargePreservesAliasesDuplicatesAndEvictionOrder() {
        val store = TableCellLayoutStore(byteBudget = CELL_BYTES * 3, capacity = 2)
        val lookup = layout("lookup").key
        val aliased = layout("different-layout-key", CELL_BYTES * 2)
        val second = layout("second")
        store.insert(aliased, lookup)
        store.insert(second)
        val keys = listOf(second.key, lookup, lookup, layout("missing").key)
        val revision = store.revision
        val bytes = store.unmountedRetainedBytes
        assertEquals(
            "Each mapped occurrence retains its original charge",
            keys.sumOf {
                store.peek(it)?.retainedBytes ?: 0L
            },
            store.retainedBytes(keys.asSequence())
        )
        assertEquals(
            "Aliased duplicate entries are counted twice",
            CELL_BYTES * 5,
            store.retainedBytes(keys.asSequence())
        )
        assertEquals("Accounting must not mutate store revision", revision, store.revision)
        assertEquals(
            "Accounting must not change admission charges",
            bytes,
            store.unmountedRetainedBytes
        )
        val third = layout("third")
        store.insert(third)
        assertNull("Reading the alias must not promote the oldest entry", store.peek(lookup))
        assertSame(second, store.peek(second.key))
        assertSame(third, store.peek(third.key))
    }

    private companion object {
        const val CELL_BYTES = 100L
        const val SCROLL_CELLS = 20
    }
}
