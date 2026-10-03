package com.apollohg.editor.viewer

import android.graphics.Rect
import android.os.Process
import com.apollohg.editor.ProseViewerError
import com.apollohg.editor.tables.ViewerTableSurface
import java.util.concurrent.Executor
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

internal class ProgressiveTableMeasurementController(
    private val executor: Executor = background,
    private val deliver: (() -> Unit) -> Unit,
    private val publish: (PreparedProseLayout) -> Unit
) {
    private class Work {
        val cancelled = AtomicBoolean()
        var task: Runnable? = null
    }

    private var layout: PreparedProseLayout? = null
    private var work: Work? = null
    private var publishing = false

    fun install(artifact: PreparedProseLayout?) {
        if (publishing) {
            layout = artifact
            return
        }
        if (layout === artifact) return
        val previous = layout
        val sameSurfaces = previous != null && artifact != null && previous.key == artifact.key &&
            previous.blocks.mapNotNull { it.tableSurface } ==
            artifact.blocks.mapNotNull { it.tableSurface }
        if (!sameSurfaces) cancel()
        layout = artifact
    }

    fun cancel() {
        work?.let {
            it.cancelled.set(true)
            it.task?.let { task -> (executor as? ThreadPoolExecutor)?.remove(task) }
        }
        work = null
    }

    val hasPendingMeasurements: Boolean get() = layout?.blocks?.any {
        it.tableSurface?.hasPendingMeasurements ==
            true
    } ==
        true

    fun prepareCell(identity: String, sourceIndex: Int, revealViewportHeightPx: Int = 0) {
        val current = layout ?: return
        val block = current.blocks.firstOrNull { it.tableSurface?.identity == identity } ?: return
        val surface = requireNotNull(block.tableSurface)
        var measured = surface
        do {
            val previous = measured
            val frame = measured.frameOfCell(sourceIndex) ?: return
            val bottom = frame.top + frame.height
            val top = minOf(frame.top, (bottom - revealViewportHeightPx).coerceAtLeast(0f))
            measured = measured.measuringViewport(top, bottom)
        } while (measured !== previous)
        if (measured !==
            surface
        ) {
            publishLayout(current.replacingTableSurfaces(mapOf(identity to measured)))
        }
    }

    fun prepareViewport(viewport: Rect): Boolean {
        val current = layout ?: return false
        if (viewport.isEmpty) return false
        var next = current
        do {
            val prior = next
            val replacements = next.blocks.mapNotNull { block ->
                val surface =
                    block.tableSurface?.takeIf { it.hasPendingMeasurements }
                        ?: return@mapNotNull null
                val bounds = requireNotNull(block.tableBounds)
                if (bounds.bottom <= viewport.top ||
                    bounds.top >= viewport.bottom
                ) {
                    return@mapNotNull null
                }
                val measured = surface.measuringViewport(
                    (viewport.top - bounds.top).toFloat(),
                    (
                        viewport.bottom -
                            bounds.top
                        ).toFloat()
                )
                if (measured === surface) null else surface.identity to measured
            }.toMap()
            next = next.replacingTableSurfaces(replacements)
        } while (next !== prior)
        if (next === current) return false
        publishLayout(next)
        return true
    }

    fun start() {
        val current = layout ?: return
        if (work != null ||
            current.blocks.none { it.tableSurface?.hasPendingMeasurements == true }
        ) {
            return
        }
        val token = Work()
        work = token
        val task = Runnable {
            val replacements = mutableMapOf<String, ViewerTableSurface>()
            var failure: ProseViewerError? = null
            try {
                for (block in current.blocks) {
                    val surface =
                        block.tableSurface?.takeIf { it.hasPendingMeasurements } ?: continue
                    val measured =
                        surface.measuringRemaining(token.cancelled::get) ?: return@Runnable
                    replacements[surface.identity] = measured
                }
            } catch (error: ProseViewerError) {
                failure = error
            }
            if (token.cancelled.get()) return@Runnable
            deliver {
                if (work !== token || token.cancelled.get()) return@deliver
                work = null
                val latest = layout ?: return@deliver
                val error = failure
                val next = try {
                    if (error != null) {
                        PreparedProseLayout.error(latest.key, latest.widthPx, error)
                    } else {
                        latest.replacingTableSurfaces(replacements)
                    }
                } catch (layoutError: ProseViewerError) {
                    PreparedProseLayout.error(latest.key, latest.widthPx, layoutError)
                }
                publishLayout(next)
            }
        }
        token.task = task
        executor.execute(task)
    }

    private fun publishLayout(next: PreparedProseLayout) {
        layout = next
        publishing = true
        try {
            publish(next)
        } finally {
            publishing = false
        }
    }

    companion object {
        private val background = ThreadPoolExecutor(
            1,
            1,
            0L,
            TimeUnit.MILLISECONDS,
            LinkedBlockingQueue(),
            { runnable ->
                Thread({
                    Process.setThreadPriority(Process.THREAD_PRIORITY_BACKGROUND)
                    runnable.run()
                }, "table-measurement").apply { isDaemon = true }
            }
        )
    }
}
