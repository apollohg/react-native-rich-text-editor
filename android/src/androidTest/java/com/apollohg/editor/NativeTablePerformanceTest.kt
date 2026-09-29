package com.apollohg.editor

import android.app.Activity
import android.graphics.Rect
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.view.Choreographer
import android.view.FrameMetrics
import android.view.View
import android.view.ViewGroup
import android.view.ViewTreeObserver
import android.view.Window
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.tables.PlainTableFixture
import com.apollohg.editor.tables.TableAccessibilityAction
import com.apollohg.editor.tables.TableAccessibilityItem
import com.apollohg.editor.tables.TableCollaborationRelay
import com.apollohg.editor.tables.TableRoomSeed
import com.apollohg.editor.tables.TableToolbarTestItems
import com.apollohg.editor.tables.required
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.PreparedProseInstrumentation
import com.apollohg.editor.viewer.PreparedProseLayoutRegistry
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.max
import kotlin.math.roundToInt
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@LargeTest
class NativeTablePerformanceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val samples = JSONArray()
    private lateinit var activity: Activity
    private lateinit var frameThread: HandlerThread
    private lateinit var frameListener: Window.OnFrameMetricsAvailableListener
    @Volatile private var onFrameMetrics: ((FrameMetrics, Int) -> Unit)? = null
    private val pending = AtomicReference<PendingFrame?>()
    private var refreshHz = 0
    private var widthPx = 0
    private var heightPx = 0

    private data class Fixture(val rows: Int, val columns: Int, val rich: Boolean) {
        val name get() = "${if (rich) "rich-merged" else "plain"}-${rows}x${columns}"

        fun source(): String {
            val source = PlainTableFixture.document(rows, columns, PlainTableFixture::coordinateText)
            if (!rich) return source
            val document = JSONObject(source)
            val tableRows = document.getJSONArray("content").getJSONObject(0).getJSONArray("content")
            repeat(rows) { row ->
                val cells = tableRows.getJSONObject(row).getJSONArray("content")
                repeat(columns) { column ->
                    val content = cells.getJSONObject(column).getJSONArray("content")
                    content.getJSONObject(0).getJSONArray("content").getJSONObject(0)
                        .put("marks", JSONArray().put(JSONObject().put("type", TableToolbarTestItems.STRONG_MARK)))
                    if ((row + column) % RICH_PARAGRAPH_STRIDE == 0) content.put(JSONObject().put("type", "paragraph").put("content", JSONArray().put(
                        JSONObject().put("type", "text").put("text", RICH_TEXT)
                    )))
                }
            }
            val first = tableRows.getJSONObject(0).getJSONArray("content")
            first.getJSONObject(0).put("attrs", JSONObject().put("colspan", MERGE_WIDTH))
            first.remove(1)
            return document.toString()
        }
    }

    private data class Measurement(val durationMs: Double, val stagesMs: Map<String, Double> = emptyMap())

    private class PendingFrame {
        val completed = CountDownLatch(1)
        @Volatile var drawNanos: Long? = null
        @Volatile var metrics: FrameMetrics? = null
        @Volatile var droppedReports = 0
        @Volatile var lastReportedFrameMillis = 0L
        @Volatile var reports = 0
        var startNanos = 0L
        var actionEndNanos = 0L
    }

    private class StageProbe {
        private val spans = linkedMapOf<PreparedProseInstrumentation.TableStage, MutableList<PreparedProseInstrumentation.ViewerWorkSpan>>()

        @Synchronized fun record(stage: PreparedProseInstrumentation.TableStage, start: Long, end: Long) {
            spans.getOrPut(stage) { mutableListOf() }.add(PreparedProseInstrumentation.ViewerWorkSpan(
                start, end, PreparedProseInstrumentation.ViewerWorkKind.LAYOUT
            ))
        }

        @Synchronized fun durationsMs(): Map<String, Double> = spans.mapKeys { it.key.jsonName }
            .mapValues { (_, intervals) -> PreparedProseInstrumentation.viewerWorkNanos(0, Long.MAX_VALUE, intervals) / NANOS_PER_MILLISECOND }
    }

    private fun <T> onMain(body: () -> T): T {
        var result: Result<T>? = null
        instrumentation.runOnMainSync { result = runCatching(body) }
        return requireNotNull(result).getOrThrow()
    }

    private fun withActivity(body: () -> Unit) {
        ActivityScenario.launch(NativeEditorOutsideTapActivity::class.java).use { active ->
            frameThread = HandlerThread("table-frame-metrics").apply { start() }
            try {
                active.onActivity { host ->
                    activity = host
                    host.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    val density = host.resources.displayMetrics.density
                    widthPx = (VIEWPORT_WIDTH * density).roundToInt()
                    heightPx = (VIEWPORT_HEIGHT * density).roundToInt()
                    assertEquals("Protocol requires text scale 1", 1f, host.resources.configuration.fontScale, SCALE_TOLERANCE)
                    val display = host.windowManager.defaultDisplay
                    val mode = requireNotNull(display.supportedModes.firstOrNull {
                        abs(it.refreshRate - REFRESH_HZ) < REFRESH_TOLERANCE
                    }) { "No 60 Hz display mode is available" }
                    host.window.attributes = host.window.attributes.apply { preferredDisplayModeId = mode.modeId }
                    frameListener = Window.OnFrameMetricsAvailableListener { _, metrics, dropped ->
                        onFrameMetrics?.invoke(metrics, dropped)
                        val target = pending.get() ?: return@OnFrameMetricsAvailableListener
                        target.lastReportedFrameMillis = metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP) / NANOS_PER_MILLISECOND_LONG
                        target.reports++
                        val drawNanos = target.drawNanos ?: return@OnFrameMetricsAvailableListener
                        val beforeDraw = PRE_DRAW_STAGES.map(metrics::getMetric)
                        val drawDuration = metrics.getMetric(FrameMetrics.DRAW_DURATION)
                        if (beforeDraw.any { it < 0 } || drawDuration < 0) return@OnFrameMetricsAvailableListener
                        val drawStart = metrics.getMetric(FrameMetrics.INTENDED_VSYNC_TIMESTAMP) + beforeDraw.sum()
                        if (drawNanos >= drawStart && drawNanos < drawStart + drawDuration && target.metrics == null) {
                            target.metrics = FrameMetrics(metrics)
                            target.droppedReports = dropped
                            target.completed.countDown()
                        }
                    }
                    host.window.addOnFrameMetricsAvailableListener(frameListener, Handler(frameThread.looper))
                }
                instrumentation.waitForIdleSync()
                refreshHz = onMain { activity.windowManager.defaultDisplay.refreshRate.roundToInt() }
                body()
            } finally {
                onMain {
                    pending.set(null)
                    activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    onFrameMetrics = null
                    if (::frameListener.isInitialized) activity.window.removeOnFrameMetricsAvailableListener(frameListener)
                    activity.setContentView(FrameLayout(activity))
                    PreparedProseInstrumentation.tableStageObserverForTesting = null
                    PreparedProseInstrumentation.tableWorkObserverForTesting = null
                }
                frameThread.quitSafely()
                frameThread.join(TimeUnit.SECONDS.toMillis(FRAME_TIMEOUT_SECONDS))
                assertFalse("Frame callback thread did not close", frameThread.isAlive)
            }
        }
    }

    private fun layout(view: View) {
        view.measure(View.MeasureSpec.makeMeasureSpec(widthPx, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(heightPx, View.MeasureSpec.EXACTLY))
        view.layout(0, 0, widthPx, heightPx)
    }

    private fun attach(view: View) {
        activity.setContentView(FrameLayout(activity).apply {
            addView(view, FrameLayout.LayoutParams(widthPx, heightPx))
        })
        layout(view)
    }

    private inner class EditorHost(existing: EditorV2Adapter? = null) : AutoCloseable {
        val adapter = existing ?: run {
            val created = UniffiEditorV2Backend.create(config, null).required("create")
            requireNotNull(EditorV2Adapter.attach(UniffiEditorV2Backend,
                JSONObject(created).getString("editorId"), roomBound = false))
        }
        private val token = EditorV2Registry.register(adapter)
        var inputInstances = 0
            private set
        val view = RichTextEditorView(activity).apply {
            onTableCellInputCreated = { inputInstances++ }
            editorId = token
        }
        val drawing get() = view.editorTableSurface.drawingView
        val table get() = requireNotNull(drawing.preparedLayout).blocks.single { it.tableSurface != null }.tableSurface!!

        init { attach(view) }

        fun load(source: String) {
            PreparedProseInstrumentation.measureTableStage(PreparedProseInstrumentation.TableStage.REPLACEMENT_AND_FFI) {
                adapter.callWithEnvelope(JSONObject().put("setJson", JSONObject(source))
                    .put("history", "resetAndClear"), includeBaseRevision = false) {
                    adapter.backend.replaceDocument(adapter.editorId, it)
                }.required("replace fixture")
            }
            assertTrue(view.editorEditText.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(intArrayOf(0, 0)))))
            layout(view)
        }

        fun bind(index: Int): EditorEditText {
            val surface = table
            val cell = requireNotNull(surface.cell(index))
            val frame = surface.frameOfCell(cell)
            view.editorScrollView.scrollTo(0, max(0, (frame.top + frame.height / 2 - heightPx / 2).roundToInt()))
            drawing.setTableLogicalOffset(surface.identity, max(0f, frame.left + frame.width / 2 - widthPx / 2))
            layout(view)
            val accessible = drawing.tableAccessibilityItems().filterIsInstance<TableAccessibilityItem.Table>()
                .single().table.cells.single { it.sourceIndex == index }
            assertTrue("Could not activate cell $index", view.editorTableSurface.activateTableAccessibilityCell(accessible))
            val input = view.activeTextInput
            assertTrue(input !== view.editorEditText)
            assertTrue(input.requestFocus())
            input.setSelection(input.text.length)
            return input
        }

        fun authoritativeBytes(): Long {
            val (metadata, state) = adapter.backend.snapshotExport(adapter.editorId).required("snapshot export")
            return metadata.toByteArray(Charsets.UTF_8).size.toLong() + state.size
        }

        override fun close() {
            view.editorTableSurface.onTableCellPreparedForTesting = null
            drawing.onMountedTableCellsDrawnForTesting = null
            view.editorId = 0L
            (view.parent as? ViewGroup)?.removeView(view)
            releasePairedV2TestEditor(token)
        }
    }

    private fun measure(drawing: PreparedProseDrawingView, action: () -> Unit): Measurement {
        val target = PendingFrame()
        val probe = StageProbe()
        try {
            onMain {
                pending.set(target)
                PreparedProseInstrumentation.tableStageObserverForTesting = probe::record
                drawing.onMountedTableCellsDrawnForTesting = {
                    if (target.drawNanos == null) target.drawNanos = System.nanoTime()
                }
                target.startNanos = System.nanoTime()
                action()
                target.actionEndNanos = System.nanoTime()
                drawing.invalidate()
            }
            val completed = target.completed.await(FRAME_TIMEOUT_SECONDS, TimeUnit.SECONDS)
            assertTrue("No committed frame: drawNanos=${target.drawNanos}, lastFrameMillis=${target.lastReportedFrameMillis}, reports=${target.reports}", completed)
            assertEquals("Frame reports were dropped during the measured commit", 0, target.droppedReports)
            val metrics = requireNotNull(target.metrics)
            val end = metrics.getMetric(FrameMetrics.INTENDED_VSYNC_TIMESTAMP) + metrics.getMetric(FrameMetrics.TOTAL_DURATION)
            assertTrue("Frame completion precedes input", end >= target.startNanos)
            val stages = probe.durationsMs().toMutableMap()
            stages["synchronousAction"] = (target.actionEndNanos - target.startNanos) / NANOS_PER_MILLISECOND
            stages["presentationWait"] = (end - target.actionEndNanos) / NANOS_PER_MILLISECOND
            FRAME_STAGES.forEach { (name, metric) ->
                metrics.getMetric(metric).takeIf { it >= 0 }?.let { stages[name] = it / NANOS_PER_MILLISECOND }
            }
            return Measurement((end - target.startNanos) / NANOS_PER_MILLISECOND, stages)
        } finally {
            onMain {
                drawing.onMountedTableCellsDrawnForTesting = null
                PreparedProseInstrumentation.tableStageObserverForTesting = null
                pending.compareAndSet(target, null)
            }
        }
    }

    private fun append(fixture: Fixture, metric: String, values: List<Measurement>,
                       counters: PreparedProseInstrumentation.TablePerformanceCounters,
                       run: Int = 1, wraps: Int? = null, attributed: List<Boolean>? = null) {
        val hardware = "${Build.HARDWARE} ${Build.FINGERPRINT}".lowercase()
        val stages = JSONObject()
        values.flatMap { it.stagesMs.keys }.toSet().forEach { stage ->
            stages.put(stage, JSONArray(values.map { it.stagesMs[stage] ?: 0.0 }))
        }
        samples.put(JSONObject().put("platform", "android").put("device", Build.MODEL)
            .put("os", "Android ${Build.VERSION.RELEASE} (${Build.DISPLAY})")
            .put("buildType", if (BuildConfig.DEBUG) "debug" else "release")
            .put("physicalDevice", listOf("ranchu", "goldfish", "emulator").none(hardware::contains))
            .put("refreshHz", refreshHz).put("textScale", 1).put("viewportWidth", VIEWPORT_WIDTH)
            .put("viewportHeight", VIEWPORT_HEIGHT).put("overscanViewports", OVERSCAN_VIEWPORTS)
            .put("fixture", fixture.name).put("metric", metric).put("run", run)
            .put("samplesMs", JSONArray(values.map { it.durationMs })).put("stageSamplesMs", stages)
            .put("stageTimingSemantics", "inclusive wall-time unions; native stages include Rust and FFI; preparation includes geometry; frame metrics may overlap native stages")
            .put("authoritativeDocumentBytesRepresentation", "yrs-update-v1-plus-snapshot-metadata; viewer uses compiled-document logical retained bytes")
            .put("counters", counters.json()).apply {
                if (metric == "typing") put("warmupSamplesDiscarded", WARMUP_SAMPLES)
                if (wraps != null) { put("wrapCount", wraps); put("nonWrapCount", values.size - wraps) }
                if (attributed != null) put("tableAttributed", JSONArray(attributed))
            })
        println("TABLE_PERFORMANCE_CASE fixture=${fixture.name} metric=$metric run=$run samples=${values.size} wraps=$wraps")
    }

    private fun cold(fixture: Fixture, source: String) {
        val editorValues = mutableListOf<Measurement>()
        val viewerValues = mutableListOf<Measurement>()
        val editorCounters = PreparedProseInstrumentation.TablePerformanceCounters()
        val viewerCounters = PreparedProseInstrumentation.TablePerformanceCounters()
        repeat(COLD_SAMPLES) {
            val host = onMain { EditorHost() }
            try {
                editorValues += measure(host.drawing) { host.load(source) }
                onMain {
                    editorCounters.observe(host.drawing, inputInstances = host.inputInstances)
                    editorCounters.authoritativeDocumentBytes = max(editorCounters.authoritativeDocumentBytes, host.authoritativeBytes())
                }
            } finally { onMain { host.close() } }
            val registry = PreparedProseLayoutRegistry()
            val viewer = onMain { ProseViewerView(activity, registry).also(::attach) }
            val drawing = onMain { viewerDrawing(viewer) }
            try {
                viewerValues += measure(drawing) {
                    assertTrue(viewer.apply(ProseViewerSource.Json(source), ProseViewerConfiguration(config)))
                    layout(viewer)
                    assertTrue(requireNotNull(drawing.preparedLayout).heightPx > 0)
                }
                onMain {
                    viewerCounters.observe(drawing, registry.layoutRetainedBytesForTesting)
                    viewerCounters.authoritativeDocumentBytes = max(viewerCounters.authoritativeDocumentBytes, registry.compiledDocumentBytesForTesting)
                }
            } finally { onMain { viewer.prepareForReuse(); (viewer.parent as ViewGroup).removeView(viewer) } }
        }
        append(fixture, "editorColdLayout", editorValues, editorCounters)
        append(fixture, "viewerColdLayout", viewerValues, viewerCounters)
    }

    private fun viewerDrawing(viewer: ProseViewerView): PreparedProseDrawingView =
        (0 until viewer.childCount).map(viewer::getChildAt).filterIsInstance<PreparedProseDrawingView>().single()

    private fun measureChange(host: EditorHost, counters: PreparedProseInstrumentation.TablePerformanceCounters,
                              action: () -> Unit): Measurement {
        val keys = onMain { requireNotNull(host.table.sourceTable).cells.map { it.contentKey }.toSet() }
        val prepared = mutableListOf<String>()
        onMain { host.view.editorTableSurface.onTableCellPreparedForTesting = { _, key -> prepared.add(key); Unit } }
        try {
            val result = measure(host.drawing, action)
            onMain {
                prepared.forEach { key ->
                    if (key in keys) counters.unchangedCellRemeasurements++
                    else counters.changedCellRemeasurements++
                }
                counters.observe(host.drawing, inputInstances = host.inputInstances)
            }
            return result
        } finally { onMain { host.view.editorTableSurface.onTableCellPreparedForTesting = null } }
    }

    private fun edit(host: EditorHost, input: EditorEditText,
                     counters: PreparedProseInstrumentation.TablePerformanceCounters,
                     text: String = TYPING_TEXT): Pair<Measurement, Boolean> {
        val cellIndex = onMain { requireNotNull(input.tableCellPositionMap).binding.cellIndex }
        val before = onMain { Triple(requireNotNull(host.table.cell(cellIndex)).contentHeightPx,
            if (text == LINE_BREAK_TEXT) input.text.count { it == LINE_BREAK_TEXT.single() } else input.text.length,
            host.adapter.baseDocumentRevision) }
        val connection = onMain { requireNotNull(input.onCreateInputConnection(EditorInfo())) }
        val result = measureChange(host, counters) {
            assertTrue(connection.commitText(text, 1))
            layout(host.view)
            val after = if (text == LINE_BREAK_TEXT) input.text.count { it == LINE_BREAK_TEXT.single() } else input.text.length
            assertEquals(before.second + text.length, after)
            assertTrue("Native input did not advance document revision", host.adapter.baseDocumentRevision > before.third)
        }
        val wrapped = onMain { requireNotNull(host.table.cell(cellIndex)).contentHeightPx != before.first }
        return result to wrapped
    }

    private fun typing(fixture: Fixture, source: String, run: Int) {
        val host = onMain { EditorHost().also { it.load(source) } }
        try {
            val input = onMain { host.bind(0) }
            repeat(WARMUP_SAMPLES) { edit(host, input, PreparedProseInstrumentation.TablePerformanceCounters()) }
            val counters = PreparedProseInstrumentation.TablePerformanceCounters()
            var wraps = 0
            val values = List(TYPING_SAMPLES) {
                val (value, wrapped) = edit(host, input, counters)
                if (wrapped) wraps++
                value
            }
            counters.authoritativeDocumentBytes = onMain { host.authoritativeBytes() }
            append(fixture, "typing", values, counters, run, wraps)
        } finally { onMain { host.close() } }
    }

    private fun cellChange(fixture: Fixture, source: String, atEnd: Boolean) {
        val host = onMain { EditorHost().also { it.load(source) } }
        try {
            val input = onMain { host.bind(if (atEnd) host.table.cells.lastIndex else 0) }
            measure(host.drawing) {}
            val counters = PreparedProseInstrumentation.TablePerformanceCounters()
            val values = List(BASELINE_SAMPLES) { edit(host, input, counters).first }
            counters.authoritativeDocumentBytes = onMain { host.authoritativeBytes() }
            append(fixture, if (atEnd) "cellChangeEnd" else "cellChangeStart", values, counters)
        } finally { onMain { host.close() } }
    }

    private fun warm(fixture: Fixture, source: String) {
        val registry = PreparedProseLayoutRegistry()
        val viewer = onMain { ProseViewerView(activity, registry).also(::attach) }
        val drawing = onMain { viewerDrawing(viewer) }
        try {
            measure(drawing) {
                assertTrue(viewer.apply(ProseViewerSource.Json(source), ProseViewerConfiguration(config)))
                layout(viewer)
            }
            val before = registry.layoutPreparationCount
            val values = onMain {
                List(WARM_SAMPLES) {
                    viewer.forceLayout()
                    val start = System.nanoTime()
                    viewer.measure(View.MeasureSpec.makeMeasureSpec(widthPx, View.MeasureSpec.EXACTLY),
                        View.MeasureSpec.makeMeasureSpec(0, View.MeasureSpec.UNSPECIFIED))
                    Measurement((System.nanoTime() - start) / NANOS_PER_MILLISECOND)
                }
            }
            val counters = PreparedProseInstrumentation.TablePerformanceCounters()
            onMain {
                counters.unchangedCellRemeasurements = registry.layoutPreparationCount - before
                counters.observe(drawing, registry.layoutRetainedBytesForTesting)
                counters.authoritativeDocumentBytes = registry.compiledDocumentBytesForTesting
            }
            append(fixture, "warmMeasurement", values, counters)
        } finally { onMain { viewer.prepareForReuse(); (viewer.parent as ViewGroup).removeView(viewer) } }
    }

    private fun structural(fixture: Fixture, source: String) {
        val host = onMain { EditorHost().also { it.load(source); it.bind(0) } }
        try {
            measure(host.drawing) {}
            val action = TableAccessibilityAction.ALL.single { it.applicability == "addTableRowAfter" }
            val counters = PreparedProseInstrumentation.TablePerformanceCounters()
            val values = List(BASELINE_SAMPLES) {
                val previous = onMain { host.table.cells.size }
                measureChange(host, counters) {
                    val cell = host.drawing.tableAccessibilityItems().filterIsInstance<TableAccessibilityItem.Table>()
                        .single().table.cells.first()
                    assertTrue(host.view.editorTableSurface.performTableAccessibilityAction(action, cell))
                    layout(host.view)
                    assertEquals(previous + fixture.columns, host.table.cells.size)
                }
            }
            counters.authoritativeDocumentBytes = onMain { host.authoritativeBytes() }
            append(fixture, "structuralCommand", values, counters)
        } finally { onMain { host.close() } }
    }

    private fun remote(fixture: Fixture, source: String) {
        val seed = onMain { TableRoomSeed(config, source) }
        val host = onMain { EditorHost(seed.makeAdapter()) }
        val peer = onMain { seed.makeAdapter() }
        try {
            val relay = onMain {
                TableCollaborationRelay(listOf(host.adapter.editorId, peer.editorId)).also {
                    it.exchangeUntilIdle()
                    requireNotNull(peer.refreshFromRustState(null))
                    assertTrue(host.view.editorEditText.applyUpdateJSON(requireNotNull(host.adapter.refreshFromRustState(null))))
                    layout(host.view)
                }
            }
            measure(host.drawing) {}
            val counters = PreparedProseInstrumentation.TablePerformanceCounters()
            val values = List(BASELINE_SAMPLES) {
                onMain {
                    val revision = peer.baseDocumentRevision
                    requireNotNull(peer.insertText(TYPING_TEXT, 0))
                    assertTrue(peer.baseDocumentRevision > revision)
                }
                measureChange(host, counters) {
                    assertTrue(relay.exchangeUntilIdle().contains(host.adapter.editorId))
                    assertTrue(host.view.editorEditText.applyUpdateJSON(requireNotNull(host.adapter.refreshFromRustState(null))))
                    layout(host.view)
                }
            }
            counters.authoritativeDocumentBytes = onMain { host.authoritativeBytes() }
            append(fixture, "remoteUpdate", values, counters)
        } finally { onMain { peer.destroy(); host.close() } }
    }

    private class WorkProbe {
        private val spans = mutableListOf<PreparedProseInstrumentation.ViewerWorkSpan>()
        @Synchronized fun record(span: PreparedProseInstrumentation.ViewerWorkSpan) { spans.add(span) }
        @Synchronized fun consume(end: Long): List<PreparedProseInstrumentation.ViewerWorkSpan> {
            val result = spans.filter { it.startNanos < end }
            spans.removeAll { it.endNanos <= end }
            return result
        }
    }

    private fun scroll(fixture: Fixture, source: String, horizontal: Boolean) {
        val host = onMain { EditorHost().also { it.load(source) } }
        val work = WorkProbe()
        val values = mutableListOf<Measurement>()
        val attributed = mutableListOf<Boolean>()
        val counters = PreparedProseInstrumentation.TablePerformanceCounters()
        val completed = CountDownLatch(1)
        var previous: Long? = null
        var elapsedNanos = 0L
        var droppedReports = 0
        var callback: Choreographer.FrameCallback? = null
        try {
            measure(host.drawing) {}
            onMain {
                val table = host.table
                val horizontalRange = max(0f, table.layout.contentWidth - table.hostViewportWidth)
                val verticalRange = max(0, host.view.editorScrollView.getChildAt(0).height - host.view.editorScrollView.height)
                PreparedProseInstrumentation.tableWorkObserverForTesting = work::record
                host.drawing.onMountedTableCellsDrawnForTesting = { count ->
                    counters.retainedPresentations = max(counters.retainedPresentations, count)
                    counters.unmountedCacheBytes = max(counters.unmountedCacheBytes, table.layoutStore.unmountedRetainedBytes)
                }
                onFrameMetrics = { metrics, dropped ->
                    if (completed.count != 0L) {
                        droppedReports += dropped
                        val timestamp = metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP)
                        previous?.let { from ->
                            val duration = timestamp - from
                            if (duration > 0) {
                                values.add(Measurement(duration / NANOS_PER_MILLISECOND))
                                attributed.add(PreparedProseInstrumentation.viewerCaused(from, timestamp, work.consume(timestamp),
                                    duration, PreparedProseInstrumentation.NOMINAL_FRAME_PERIOD_NANOS))
                                elapsedNanos += duration
                            }
                        }
                        previous = timestamp
                        if (elapsedNanos >= TRAVERSAL_NANOS) completed.countDown()
                    }
                }
                val start = System.nanoTime()
                val frameCallback = object : Choreographer.FrameCallback {
                    override fun doFrame(frameTime: Long) {
                        if (completed.count != 0L) {
                            val fraction = ((1 - cos((frameTime - start).toDouble() / TRAVERSAL_NANOS * 2 * Math.PI)) / 2).toFloat()
                            if (horizontal) host.drawing.setTableLogicalOffset(table.identity, fraction * horizontalRange)
                            else host.view.editorScrollView.scrollTo(0, (fraction * verticalRange).roundToInt())
                            layout(host.view)
                            host.drawing.invalidate()
                            Choreographer.getInstance().postFrameCallback(this)
                        }
                    }
                }
                callback = frameCallback
                Choreographer.getInstance().postFrameCallback(frameCallback)
            }
            assertTrue("Scroll did not produce 30 seconds of frame reports", completed.await(FRAME_TIMEOUT_SECONDS, TimeUnit.SECONDS))
            assertEquals("Scroll lost FrameMetrics callbacks", 0, droppedReports)
            onMain {
                counters.observe(host.drawing, inputInstances = host.inputInstances)
                counters.authoritativeDocumentBytes = host.authoritativeBytes()
            }
            append(fixture, if (horizontal) "scrollHorizontal" else "scrollVertical", values, counters, attributed = attributed)
        } finally {
            onMain {
                onFrameMetrics = null
                callback?.let { Choreographer.getInstance().removeFrameCallback(it) }
                PreparedProseInstrumentation.tableWorkObserverForTesting = null
                host.close()
            }
        }
    }

    @Test fun everyFixtureIsAdmitted() = withActivity {
        for (rich in listOf(false, true)) {
            for ((rows, columns) in listOf(SMALL_ROWS to SMALL_COLUMNS) + PlainTableFixture.TWENTY_THOUSAND_SLOT_SHAPES) {
                onMain {
                    val fixture = Fixture(rows, columns, rich)
                    EditorHost().use { host ->
                        host.load(fixture.source())
                        assertEquals(fixture.name, rows * columns - if (rich) 1 else 0, host.table.cells.size)
                    }
                }
            }
        }
    }

    @Test fun exportUsesCollectedOutputDirectory() {
        val expected = InstrumentationRegistry.getArguments().getString(ADDITIONAL_OUTPUT_ARGUMENT)?.let(::File)
            ?: requireNotNull(instrumentation.targetContext.externalMediaDirs.firstOrNull())
        val output = saveExport(SMOKE_OUTPUT_FILE)
        assertEquals("Gradle must collect the export before uninstalling the test app",
            expected.canonicalFile, output.parentFile!!.canonicalFile)
    }

    @Test fun structuralCounterRecognizesRebuiltCellAfterItsRowMoves() = withActivity {
        val fixture = Fixture(SMALL_ROWS, SMALL_COLUMNS, false)
        val host = onMain { EditorHost().also { it.load(fixture.source()); it.bind(0) } }
        try {
            measure(host.drawing) {}
            val rebuild = onMain { requireNotNull(host.table.cells[fixture.columns].content.cellPreparation) }
            val previousRevision = onMain { host.adapter.baseDocumentRevision }
            val action = TableAccessibilityAction.ALL.single { it.applicability == "addTableRowAfter" }
            val counters = PreparedProseInstrumentation.TablePerformanceCounters()
            measureChange(host, counters) {
                val cell = host.drawing.tableAccessibilityItems().filterIsInstance<TableAccessibilityItem.Table>()
                    .single().table.cells.first()
                assertTrue(host.view.editorTableSurface.performTableAccessibilityAction(action, cell))
                layout(host.view)
                rebuild()
            }
            assertTrue(onMain { host.adapter.baseDocumentRevision > previousRevision })
            assertEquals("The original second-row content remains unchanged after its row moves",
                1, counters.unchangedCellRemeasurements)
            assertEquals("The inserted empty cells share one newly prepared shape",
                1, counters.changedCellRemeasurements)
        } finally { onMain { host.close() } }
    }

    @Test fun largeStructuralChangesReuseUnchangedGeometry() = withActivity {
        val (rows, columns) = PlainTableFixture.TWENTY_THOUSAND_SLOT_SHAPES.first()
        val fixture = Fixture(rows, columns, false)
        structural(fixture, fixture.source())
        val counters = samples.getJSONObject(0).getJSONObject("counters")
        assertEquals(0, counters.getInt("unchangedCellRemeasurements"))
        assertEquals("All inserted empty rows share one shape and keep separate bindings",
            1, counters.getInt("changedCellRemeasurements"))
    }

    @Test fun largeBoundCellPreparationStartsAfterActivationFrame() = withActivity {
        val (rows, columns) = PlainTableFixture.TWENTY_THOUSAND_SLOT_SHAPES.first()
        val fixture = Fixture(rows, columns, false)
        cellChange(fixture, fixture.source(), atEnd = true)
        val counters = samples.getJSONObject(0).getJSONObject("counters")
        assertEquals(0, counters.getInt("unchangedCellRemeasurements"))
        assertEquals(BASELINE_SAMPLES, counters.getInt("changedCellRemeasurements"))
    }

    @Test fun typingKeepsThePresentedViewportStable() = withActivity {
        val fixture = Fixture(SMALL_ROWS, SMALL_COLUMNS, false)
        val host = onMain { EditorHost().also { it.load(fixture.source()) } }
        try {
            val input = onMain { host.bind(0) }
            repeat(VIEWPORT_STABILITY_WARMUP_SAMPLES) { edit(host, input, PreparedProseInstrumentation.TablePerformanceCounters()) }
            fun viewport(): List<Int> {
                val location = IntArray(2)
                host.view.getLocationOnScreen(location)
                return listOf(location[0], location[1], host.view.top, host.view.height,
                    host.view.editorScrollView.scrollY, activity.window.decorView.scrollY)
            }
            val settleDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(FRAME_TIMEOUT_SECONDS)
            var expected = onMain { viewport() }
            var stableFrames = 0
            while (stableFrames < VIEWPORT_STABLE_FRAMES) {
                check(System.nanoTime() < settleDeadline) { "Initial keyboard viewport did not settle" }
                measure(host.drawing) {}
                val next = onMain { viewport() }
                stableFrames = if (next == expected) stableFrames + 1 else 0
                expected = next
            }
            val observed = mutableListOf<List<Int>>()
            val observer = ViewTreeObserver.OnPreDrawListener {
                observed.add(viewport())
                true
            }
            onMain { host.view.viewTreeObserver.addOnPreDrawListener(observer) }
            try {
                repeat(VIEWPORT_STABILITY_SAMPLES) {
                    edit(host, input, PreparedProseInstrumentation.TablePerformanceCounters())
                    onMain { observed.add(viewport()) }
                }
            } finally { onMain { host.view.viewTreeObserver.removeOnPreDrawListener(observer) } }
            assertEquals("Viewport moved while the typed caret remained visible: expected=$expected, frames=${observed.distinct()}",
                listOf(expected), observed.distinct())
        } finally { onMain { host.close() } }
    }

    @Test fun caretRevealPreservesRectangleThroughTableScroll() = withActivity {
        val fixture = Fixture(SMALL_ROWS, WRAP_SCROLL_COLUMNS, false)
        val host = onMain { EditorHost() }
        var propagated: Rect? = null
        try {
            onMain {
                (host.view.parent as ViewGroup).removeView(host.view)
                val container = object : FrameLayout(activity) {
                    override fun requestChildRectangleOnScreen(child: View, rectangle: Rect, immediate: Boolean): Boolean {
                        propagated = Rect(rectangle)
                        return super.requestChildRectangleOnScreen(child, rectangle, immediate)
                    }
                }
                activity.setContentView(container)
                container.addView(host.view, FrameLayout.LayoutParams(widthPx, heightPx))
                host.load(fixture.source())
            }
            val input = onMain { host.bind(0) }
            measure(host.drawing) {
                val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
                assertTrue(connection.commitText(TYPING_TEXT.repeat(TYPING_SAMPLES), 1))
                layout(host.view)
            }
            onMain {
                host.view.editorScrollView.scrollTo(0, 0)
                val caret = Rect().also(input::getFocusedRect)
                propagated = null
                input.requestRectangleOnScreen(Rect(caret), true)
                assertTrue("Caret reveal must synchronously scroll the table", host.view.editorScrollView.scrollY > 0)
                val expected = Rect(caret)
                host.view.offsetDescendantRectToMyCoords(input, expected)
                assertEquals("Caret bounds changed while propagating through table scroll: local=$caret scroll=${host.view.editorScrollView.scrollY}",
                    expected, propagated)
            }
        } finally { onMain { host.close() } }
    }

    @Test fun typingWrappedCellDoesNotScrollBackToTop() = withActivity {
        val fixture = Fixture(SMALL_ROWS, WRAP_SCROLL_COLUMNS, false)
        val host = onMain { EditorHost().also { it.load(fixture.source()) } }
        try {
            val input = onMain { host.bind(0) }
            repeat(VIEWPORT_STABILITY_WARMUP_SAMPLES) { edit(host, input, PreparedProseInstrumentation.TablePerformanceCounters()) }
            val offsets = mutableListOf<Int>()
            val frames = mutableListOf<String>()
            var editIndex = 0
            val observer = ViewTreeObserver.OnPreDrawListener {
                val location = IntArray(2)
                host.view.getLocationOnScreen(location)
                val scroll = host.view.editorScrollView.scrollY
                offsets.add(scroll - location[1])
                frames.add("edit=$editIndex scroll=$scroll hostY=${location[1]} inputY=${input.y} inputScroll=${input.scrollY}")
                true
            }
            onMain { host.view.viewTreeObserver.addOnPreDrawListener(observer) }
            var wraps = 0
            try {
                repeat(TYPING_SAMPLES) {
                    editIndex = it
                    if (edit(host, input, PreparedProseInstrumentation.TablePerformanceCounters()).second) wraps++
                }
                repeat(BASELINE_SAMPLES) {
                    editIndex = TYPING_SAMPLES + it
                    edit(host, input, PreparedProseInstrumentation.TablePerformanceCounters(), LINE_BREAK_TEXT)
                }
            } finally { onMain { host.view.viewTreeObserver.removeOnPreDrawListener(observer) } }
            assertTrue("Typing must wrap and scroll beyond the initial viewport: wraps=$wraps offsets=${offsets.distinct()}",
                wraps > 0 && offsets.last() > offsets.first())
            val reversal = offsets.zipWithNext().indexOfFirst { (before, after) -> after < before - SCROLL_ROUNDING_TOLERANCE_PX }
            assertEquals("Appending text scrolled backward: ${if (reversal < 0) frames.distinct() else frames.subList(max(0, reversal - VIEWPORT_STABLE_FRAMES), minOf(frames.size, reversal + VIEWPORT_STABLE_FRAMES))}",
                -1, reversal)
        } finally { onMain { host.close() } }
    }

    @Test fun exporterPrimitivesAndNativeStages() = withActivity {
        val fixture = Fixture(SMALL_ROWS, SMALL_COLUMNS, false)
        val source = fixture.source()
        cold(fixture, source)
        typing(fixture, source, 1)
        cellChange(fixture, source, atEnd = true)
        warm(fixture, source)
        structural(fixture, source)
        remote(fixture, source)
        repeat(samples.length()) { index ->
            val sample = samples.getJSONObject(index)
            assertEquals("${sample.getString("metric")} remeasured unchanged cells", 0,
                sample.getJSONObject("counters").getInt("unchangedCellRemeasurements"))
            val durations = sample.getJSONArray("samplesMs")
            repeat(durations.length()) { assertTrue(durations.getDouble(it).isFinite() && durations.getDouble(it) >= 0) }
        }
        val typing = (0 until samples.length()).map(samples::getJSONObject).single { it.getString("metric") == "typing" }
        assertTrue(typing.getInt("wrapCount") > 0)
        assertTrue(typing.getInt("nonWrapCount") > 0)
        listOf("nativeInputAndFFI", "nativeFrameAndFFI", "adapterAdoption", "tablePreparationAndGeometry", "frameTotal").forEach { stage ->
            val durations = typing.getJSONObject("stageSamplesMs").getJSONArray(stage)
            assertEquals(TYPING_SAMPLES, durations.length())
            repeat(durations.length()) { assertTrue("$stage missing for edit $it", durations.getDouble(it) > 0) }
        }
        val changed = (0 until samples.length()).map(samples::getJSONObject).single { it.getString("metric") == "cellChangeEnd" }
        assertEquals(BASELINE_SAMPLES, changed.getJSONObject("counters").getInt("changedCellRemeasurements"))
    }

    @Test fun exportTablePerformance() = withActivity {
        val fixtures = listOf(false, true).flatMap { rich ->
            (listOf(SMALL_ROWS to SMALL_COLUMNS) + PlainTableFixture.TWENTY_THOUSAND_SLOT_SHAPES)
                .map { (rows, columns) -> Fixture(rows, columns, rich) }
        }
        val requested = InstrumentationRegistry.getArguments().getString(FIXTURE_ARGUMENT)?.split(',')?.toSet()
        require(requested == null || requested.all { name -> fixtures.any { it.name == name } }) {
            "Unknown table performance fixtures: $requested"
        }
        for (fixture in fixtures.filter { requested == null || it.name in requested }) {
            val source = fixture.source()
            cold(fixture, source)
            repeat(if (fixture.rich) 1 else TYPING_RUNS) { typing(fixture, source, it + 1) }
            cellChange(fixture, source, atEnd = false)
            cellChange(fixture, source, atEnd = true)
            if (!fixture.rich) {
                warm(fixture, source)
                scroll(fixture, source, horizontal = true)
                scroll(fixture, source, horizontal = false)
                structural(fixture, source)
                remote(fixture, source)
            }
            saveExport()
        }
        val output = saveExport()
        println("TABLE_PERFORMANCE_EXPORT_PATH ${output.absolutePath}")
        instrumentation.sendStatus(0, android.os.Bundle().apply { putString("tablePerformanceExport", output.absolutePath) })
    }

    private fun saveExport(fileName: String = OUTPUT_FILE): File {
        val directory = InstrumentationRegistry.getArguments().getString(ADDITIONAL_OUTPUT_ARGUMENT)?.let(::File)
            ?: requireNotNull(instrumentation.targetContext.externalMediaDirs.firstOrNull())
        check(directory.mkdirs() || directory.isDirectory) { "Cannot create performance export directory: $directory" }
        return File(directory, fileName).also {
            it.writeText(JSONObject().put("samples", samples).toString())
        }
    }

    private companion object {
        const val VIEWPORT_WIDTH = 390
        const val VIEWPORT_HEIGHT = 844
        const val OVERSCAN_VIEWPORTS = 1
        const val SMALL_ROWS = 3
        const val SMALL_COLUMNS = 3
        const val MERGE_WIDTH = 2
        const val RICH_PARAGRAPH_STRIDE = 2
        const val TYPING_RUNS = 5
        const val TYPING_SAMPLES = 500
        const val WARMUP_SAMPLES = 20
        const val VIEWPORT_STABILITY_SAMPLES = 80
        const val VIEWPORT_STABILITY_WARMUP_SAMPLES = 100
        const val VIEWPORT_STABLE_FRAMES = 8
        const val WRAP_SCROLL_COLUMNS = 20
        const val SCROLL_ROUNDING_TOLERANCE_PX = 1
        const val COLD_SAMPLES = 30
        const val WARM_SAMPLES = 1_000
        const val BASELINE_SAMPLES = 10
        const val FRAME_TIMEOUT_SECONDS = 120L
        const val REFRESH_HZ = 60f
        const val REFRESH_TOLERANCE = 0.1f
        const val SCALE_TOLERANCE = 0.001f
        const val NANOS_PER_MILLISECOND_LONG = 1_000_000L
        const val NANOS_PER_MILLISECOND = 1_000_000.0
        const val TRAVERSAL_NANOS = 30_000_000_000L
        const val TYPING_TEXT = "x"
        const val LINE_BREAK_TEXT = "\n"
        const val RICH_TEXT = "café العربية 👩🏽‍💻"
        const val OUTPUT_FILE = "table-performance-android.json"
        const val SMOKE_OUTPUT_FILE = "table-performance-android-storage-smoke.json"
        const val ADDITIONAL_OUTPUT_ARGUMENT = "additionalTestOutputDir"
        const val FIXTURE_ARGUMENT = "tablePerformanceFixtures"
        val PRE_DRAW_STAGES = intArrayOf(FrameMetrics.UNKNOWN_DELAY_DURATION,
            FrameMetrics.INPUT_HANDLING_DURATION, FrameMetrics.ANIMATION_DURATION,
            FrameMetrics.LAYOUT_MEASURE_DURATION)
        val FRAME_STAGES = mapOf("frameTotal" to FrameMetrics.TOTAL_DURATION,
            "frameInput" to FrameMetrics.INPUT_HANDLING_DURATION, "frameLayout" to FrameMetrics.LAYOUT_MEASURE_DURATION,
            "frameDraw" to FrameMetrics.DRAW_DURATION, "frameSync" to FrameMetrics.SYNC_DURATION,
            "frameCommands" to FrameMetrics.COMMAND_ISSUE_DURATION, "frameSwap" to FrameMetrics.SWAP_BUFFERS_DURATION,
            "frameGpu" to FrameMetrics.GPU_DURATION)
        val config: String = JSONObject(PlainTableFixture.CONFIG).apply {
            getJSONObject("schema").apply {
                put("marks", JSONArray().put(JSONObject().put("name", TableToolbarTestItems.STRONG_MARK)))
                val nodes = getJSONArray("nodes")
                repeat(nodes.length()) { index ->
                    val node = nodes.getJSONObject(index)
                    node.remove("htmlTag")
                    if (node.getString("name") in listOf("table", "table_cell", "table_header")) {
                        node.put("attrs", (node.optJSONObject("attrs") ?: JSONObject())
                            .put("class", JSONObject().put("default", JSONObject.NULL)))
                    }
                }
            }
            put("initialization", JSONObject().put("type", "localHtml").put("html", "")
                .put("snapshotScope", JSONObject().put("documentId", "table-performance")
                    .put("lineageId", "native-editor|table-performance")))
        }.toString()
    }
}
