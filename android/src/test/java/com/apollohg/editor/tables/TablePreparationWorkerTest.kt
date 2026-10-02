package com.apollohg.editor.tables

import com.apollohg.editor.viewer.PreparedProseLayout
import com.apollohg.editor.viewer.ProseLayoutKey
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicIntegerArray
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34])
class TablePreparationWorkerTest {
    private val cellCount = 1_000
    private val width = 120f
    private val record = TableGridRecord("workers", 1, cellCount, listOf(width),
        List(cellCount) { TableGridCell(it, it, 0, contentKey = "cell-$it") })
    private val indices = record.cells.mapTo(mutableSetOf()) { it.sourceIndex }

    private fun content(cell: TableGridCell, width: Float) = PreparedProseLayout(
        ProseLayoutKey(cell.contentKey, width.toInt(), "workers", 0, 0, 1, 0, "workers"),
        width.toInt(), cell.sourceIndex + 1, emptyList(), retainedBytes = 100L)

    private fun prepare(workers: List<(TableGridCell, Float) -> PreparedProseLayout>) = ViewerTableSurface(
        "workers", record, width, TableStyle(), false,
        prepareCellWorkers = workers, parallelCellIndices = indices,
        transientCellIndices = indices, prepareCell = ::content)

    @Test fun callerParticipatesAndEveryCellIsCapturedOnceInSourceOrder() {
        val caller = Thread.currentThread()
        val visits = AtomicIntegerArray(cellCount)
        val workerThreads = arrayOfNulls<Thread>(2)
        val started = CountDownLatch(2)
        val surface = prepare(List(2) { worker ->
            { cell, width ->
                val thread = Thread.currentThread()
                workerThreads[worker]?.let { assertSame("A worker owns one engine on one thread", it, thread) }
                if (workerThreads[worker] == null) {
                    started.countDown()
                    assertTrue(started.await(TIMEOUT_SECONDS, TimeUnit.SECONDS))
                }
                workerThreads[worker] = thread
                visits.incrementAndGet(cell.sourceIndex)
                content(cell, width)
            }
        })
        assertSame("The synchronous caller must perform one worker's share", caller, workerThreads[0])
        assertNotSame(caller, workerThreads[1])
        assertEquals(indices.toList(), surface.cells.map { it.sourceIndex })
        surface.cells.forEach { cell ->
            assertEquals("cell ${cell.sourceIndex} must be prepared exactly once", 1, visits[cell.sourceIndex])
            assertEquals(cell.sourceIndex + 1, cell.contentHeightPx)
        }
        val sequential = prepare(emptyList())
        assertEquals(sequential.layout, surface.layout)
        assertEquals(sequential.retainedBytes, surface.retainedBytes)
        assertEquals(sequential.layoutStore.count, surface.layoutStore.count)
    }

    @Test fun callerFailureWaitsForBackgroundCompletionBeforeClosingWorkerContexts() {
        val backgroundStarted = CountDownLatch(1)
        val callerFailed = CountDownLatch(1)
        val releaseBackground = CountDownLatch(1)
        val backgroundFinished = AtomicBoolean()
        val contextsClosed = AtomicBoolean()
        val callerExecutor = Executors.newSingleThreadExecutor()
        val failure = IllegalStateException("caller worker failure")
        try {
            val result = CompletableFuture.runAsync({
                try {
                    prepare(listOf(
                        { _, _ ->
                            assertTrue(backgroundStarted.await(TIMEOUT_SECONDS, TimeUnit.SECONDS))
                            callerFailed.countDown()
                            throw failure
                        },
                        { cell, width ->
                            backgroundStarted.countDown()
                            assertTrue(releaseBackground.await(TIMEOUT_SECONDS, TimeUnit.SECONDS))
                            assertFalse("Worker contexts must remain open during outstanding work", contextsClosed.get())
                            backgroundFinished.set(true)
                            content(cell, width)
                        }
                    ))
                } finally { contextsClosed.set(true) }
            }, callerExecutor)
            assertTrue(callerFailed.await(TIMEOUT_SECONDS, TimeUnit.SECONDS))
            assertFalse("A failed caller must still join background work", result.isDone)
            releaseBackground.countDown()
            val thrown = runCatching { result.get(TIMEOUT_SECONDS, TimeUnit.SECONDS) }.exceptionOrNull()
            assertNotNull(thrown)
            assertTrue(generateSequence(thrown) { it.cause }.any { it === failure })
            assertTrue(backgroundFinished.get())
            assertTrue(contextsClosed.get())
        } finally {
            releaseBackground.countDown()
            callerExecutor.shutdownNow()
        }
    }

    private companion object { const val TIMEOUT_SECONDS = 5L }
}
