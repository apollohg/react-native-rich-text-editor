package com.apollohg.editor

import android.app.Instrumentation
import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.view.Choreographer
import android.view.FrameMetrics
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.view.WindowManager
import android.widget.FrameLayout
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.LargeTest
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.viewer.PreparedProseInstrumentation
import java.io.BufferedReader
import java.io.File
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.cos
import kotlin.math.roundToInt
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
@LargeTest
class NativeDevicePerformanceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val testContext: Context = instrumentation.context
    private val targetContext: Context = instrumentation.targetContext
    private val baseFontSize = 16f
    private val textColor = Color.BLACK

    @Test
    fun performance_preparedProseCorpusGates_pixel7() {
        val corpus =
            JSONObject(
                testContext.assets.open(
                    "viewer-performance-corpus.json"
                ).bufferedReader().use(BufferedReader::readText)
            )
        val configuration = PreparedProseBenchmarkConfiguration.load(testContext)
        val byId = buildMap {
            val documents = corpus.getJSONArray("documents")
            for (index in 0 until documents.length()) {
                val entry = documents.getJSONObject(index)
                put(
                    entry.getString("id"),
                    PreparedProseRecyclerHarness.Entry(
                        entry.getString("id"),
                        entry.getJSONObject("contentJSON").toString()
                    )
                )
            }
        }
        val warmWindows = corpus.getJSONArray("warmWindows").let { windows ->
            List(windows.length()) { index ->
                windows.getJSONObject(index).let { window ->
                    fun ids(name: String) = window.getJSONArray(name).let { values ->
                        List(values.length()) { valueIndex -> values.getString(valueIndex) }
                    }
                    PreparedProseRecyclerHarness.WarmWindow(
                        id = window.getString("id"),
                        primeIds = ids("primeIds"),
                        warmIds = ids("warmIds")
                    )
                }
            }
        }
        ActivityScenario.launch(NativeEditorOutsideTapActivity::class.java).use { scenario ->
            lateinit var harness: PreparedProseRecyclerHarness
            scenario.onActivity { activity ->
                val viewportWidth = PreparedProseRecyclerHarness.viewportWidthPx(activity)
                val viewportHeight = PreparedProseRecyclerHarness.viewportHeightPx(activity)
                activity.setContentView(
                    FrameLayout(activity).apply {
                        addView(
                            PreparedProseRecyclerHarness(activity, configuration).also {
                                harness =
                                    it
                            },
                            FrameLayout.LayoutParams(viewportWidth, viewportHeight)
                        )
                    }
                )
            }
            instrumentation.waitForIdleSync()
            PreparedProseInstrumentation.beginBenchmark()
            // ActivityScenario only supplies the real attached window. Each
            // literal window drives itself through RecyclerView's normal
            // smooth-scroll/bind/recycle lifecycle before its warm revisit.
            harness.traverseWindows(
                instrumentation,
                warmWindows,
                byId,
                PreparedProseInstrumentation.TraversalPhase.COLD,
                imagesEnabled = true
            )
            harness.traverseWindows(
                instrumentation,
                warmWindows,
                byId,
                PreparedProseInstrumentation.TraversalPhase.IMAGES_DISABLED,
                imagesEnabled = false
            )
            check(harness.exportBeforeReset().isNotEmpty()) {
                "pre-reset prepared prose evidence must export"
            }
            instrumentation.runOnMainSync { harness.resetCacheWhileMounted() }
        }
        val export = PreparedProseInstrumentation.exportJson()
        reportPreparedProseExport(export)
        PreparedProsePerformanceGates.assertPasses(
            export,
            expectedDocuments = 1_000,
            expectedWindows = warmWindows
        )
    }

    @Test
    fun performance_renderBridgeLargeDocument() {
        val renderJson = NativePerformanceFixtureFactory.largeRenderJson()

        val stats = measureOperation("renderBridgeLargeDocument") {
            val spannable = instrumentation.runOnMainSyncWithResult {
                RenderBridge.buildSpannable(
                    renderJson,
                    baseFontSize,
                    textColor,
                    density = targetContext.resources.displayMetrics.density
                )
            }

            assertTrue("rendered spannable should not be empty", spannable.isNotEmpty())
            if (spannable.isNotEmpty()) {
                instrumentation.runOnMainSyncWithResult {
                    spannable.getSpans(0, minOf(spannable.length, 1), Any::class.java)
                }
            }
        }

        reportStats(stats)
        assertTrue("average render time should be positive", stats.averageMillis > 0.0)
    }

    @Test
    fun performance_applyUpdateJsonLargeDocument() {
        val updateJson = NativePerformanceFixtureFactory.largeUpdateJson()
        val editText = instrumentation.runOnMainSyncWithResult {
            EditorEditText(targetContext).apply {
                layoutParams = ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT
                )
                setBaseStyle(baseFontSize, textColor, Color.WHITE)
                captureApplyUpdateTraceForTesting = true
                applyUpdateJSON(updateJson, notifyListener = false)
                layoutView(
                    this,
                    widthPx = 1080,
                    heightPx = 2400,
                    heightMode = View.MeasureSpec.AT_MOST
                )
            }
        }
        val traces = mutableListOf<EditorEditText.ApplyUpdateTrace>()

        val stats = measureOperation("applyUpdateJsonLargeDocument") {
            instrumentation.runOnMainSyncWithResult {
                editText.applyUpdateJSON(updateJson, notifyListener = false)
                layoutView(
                    editText,
                    widthPx = 1080,
                    heightPx = 2400,
                    heightMode = View.MeasureSpec.AT_MOST
                )
                editText.lastApplyUpdateTrace()?.let(traces::add)
            }

            assertFalse("edit text should contain rendered content", editText.text.isNullOrEmpty())
            assertNotNull("edit text should have a layout after applying update", editText.layout)
        }

        reportStats(stats)
        reportTraceStats(stats.name, traces.takeLast(MEASURED_ITERATIONS))
        assertTrue("average applyUpdateJSON time should be positive", stats.averageMillis > 0.0)
    }

    @Test
    fun performance_applyRenderPatchLargeDocument() {
        val initialUpdateJson = NativePerformanceFixtureFactory.largeUpdateJson()
        val patchedUpdateJson = NativePerformanceFixtureFactory.largePatchedUpdateJson()
        val editText = instrumentation.runOnMainSyncWithResult {
            EditorEditText(targetContext).apply {
                layoutParams = ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT
                )
                setBaseStyle(baseFontSize, textColor, Color.WHITE)
                captureApplyUpdateTraceForTesting = true
            }
        }
        val traces = mutableListOf<EditorEditText.ApplyUpdateTrace>()

        val stats = measureOperation(
            name = "applyRenderPatchLargeDocument",
            beforeEach = {
                instrumentation.runOnMainSyncWithResult {
                    editText.applyUpdateJSON(initialUpdateJson, notifyListener = false)
                    layoutView(
                        editText,
                        widthPx = 1080,
                        heightPx = 2400,
                        heightMode = View.MeasureSpec.AT_MOST
                    )
                }
            }
        ) {
            instrumentation.runOnMainSyncWithResult {
                editText.applyUpdateJSON(patchedUpdateJson, notifyListener = false)
                layoutView(
                    editText,
                    widthPx = 1080,
                    heightPx = 2400,
                    heightMode = View.MeasureSpec.AT_MOST
                )
                editText.lastApplyUpdateTrace()?.let(traces::add)
            }

            assertFalse("edit text should contain rendered content", editText.text.isNullOrEmpty())
        }

        reportStats(stats)
        reportTraceStats(stats.name, traces.takeLast(MEASURED_ITERATIONS))
        assertTrue("average patch apply time should be positive", stats.averageMillis > 0.0)
    }

    @Test
    fun performance_typingRoundTripLargeDocument() {
        val (adapter, editorId) = createV2Editor()
        try {
            NativePerformanceFixtureFactory.loadLargeDocumentIntoEditor(adapter)
            val editText = instrumentation.runOnMainSyncWithResult {
                EditorEditText(targetContext).apply {
                    layoutParams = ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.WRAP_CONTENT
                    )
                    setBaseStyle(baseFontSize, textColor, Color.WHITE)
                    captureApplyUpdateTraceForTesting = true
                    bindEditor(editorId)
                    layoutView(
                        this,
                        widthPx = 1080,
                        heightPx = 2400,
                        heightMode = View.MeasureSpec.AT_MOST
                    )
                }
            }
            val typingOffset = NativePerformanceFixtureFactory.typingCursorOffset(
                editText.text ?: ""
            )
            val originalText = editText.text.toString()
            val traces = mutableListOf<EditorEditText.ApplyUpdateTrace>()
            val layoutBuildCounts = mutableListOf<Int>()

            val stats = measureOperation("typingRoundTripLargeDocument") {
                instrumentation.runOnMainSyncWithResult {
                    editText.setSelection(typingOffset)
                    val beforeLayoutBuilds = editText.documentLayoutBuildCount
                    val beforeRevision = adapter.baseDocumentRevision
                    editText.handleTextCommit("!")
                    assertEquals(
                        "typing must insert into the authoritative visible document",
                        originalText.substring(0, typingOffset) + "!" +
                            originalText.substring(typingOffset),
                        editText.text.toString()
                    )
                    assertTrue(
                        "typing must advance Rust revision",
                        adapter.baseDocumentRevision > beforeRevision
                    )
                    editText.lastApplyUpdateTrace()?.let(traces::add)
                    editText.handleDelete(1, 0)
                    assertEquals(
                        "backspace must restore the original content",
                        originalText,
                        editText.text.toString()
                    )
                    editText.lastApplyUpdateTrace()?.let(traces::add)
                    layoutView(
                        editText,
                        widthPx = 1080,
                        heightPx = 2400,
                        heightMode = View.MeasureSpec.AT_MOST
                    )
                    layoutBuildCounts.add(editText.documentLayoutBuildCount - beforeLayoutBuilds)
                }

                assertFalse(
                    "edit text should contain rendered content",
                    editText.text.isNullOrEmpty()
                )
                assertNotNull("edit text should have a layout after typing", editText.layout)
            }

            reportStats(stats)
            reportTraceStats(stats.name, traces.takeLast(MEASURED_ITERATIONS * TYPING_OPERATIONS))
            val patch = JSONObject(requireNotNull(adapter.cachedViewUpdateJson))
                .optJSONObject("renderPatch")
            reportSummary(
                "typingRoundTripLargeDocument layoutBuilds=${layoutBuildCounts.takeLast(
                    MEASURED_ITERATIONS
                )} " +
                    "patchStart=${patch?.optInt("startIndex")} " +
                    "patchDelete=${patch?.optInt("deleteCount")} " +
                    "patchInsert=${patch?.optJSONArray("renderBlocks")?.length()}"
            )
            assertTrue("average typing time should be positive", stats.averageMillis > 0.0)
        } finally {
            releasePairedV2TestEditor(editorId)
        }
    }

    @Test
    fun performance_paragraphSplitRoundTripLargeDocument() {
        val (adapter, editorId) = createV2Editor()
        try {
            val largeDocumentJson = NativePerformanceFixtureFactory.largeDocumentJson()
            NativePerformanceFixtureFactory.loadLargeDocumentIntoEditor(adapter)
            val editText = instrumentation.runOnMainSyncWithResult {
                EditorEditText(targetContext).apply {
                    layoutParams = ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.WRAP_CONTENT
                    )
                    setBaseStyle(baseFontSize, textColor, Color.WHITE)
                    captureApplyUpdateTraceForTesting = true
                    bindEditor(editorId)
                    layoutView(
                        this,
                        widthPx = 1080,
                        heightPx = 2400,
                        heightMode = View.MeasureSpec.AT_MOST
                    )
                }
            }
            val splitOffset = NativePerformanceFixtureFactory.typingCursorOffset(editText.text)
            val originalText = editText.text.toString()
            val expectedSplitText = originalText.substring(0, splitOffset) + "\n" +
                originalText.substring(splitOffset)
            val traces = mutableListOf<EditorEditText.ApplyUpdateTrace>()

            val stats = measureOperation(
                name = "paragraphSplitRoundTripLargeDocument",
                beforeEach = {
                    instrumentation.runOnMainSyncWithResult {
                        adapter.setContentJson(largeDocumentJson)?.let { update ->
                            editText.applyUpdateJSON(update, notifyListener = false)
                        }
                        editText.setSelection(splitOffset)
                        layoutView(
                            editText,
                            widthPx = 1080,
                            heightPx = 2400,
                            heightMode = View.MeasureSpec.AT_MOST
                        )
                    }
                }
            ) {
                instrumentation.runOnMainSyncWithResult {
                    val beforeRevision = adapter.baseDocumentRevision
                    editText.handleTextCommit("\n")
                    if (editText.text.toString() != expectedSplitText) {
                        File(targetContext.filesDir, "editor-performance-split-document.json")
                            .writeText(requireNotNull(adapter.documentJson()))
                        File(targetContext.filesDir, "editor-performance-split-render.json")
                            .writeText(requireNotNull(adapter.cachedAtomicRenderJson))
                        File(targetContext.filesDir, "editor-performance-split-source.json")
                            .writeText(largeDocumentJson)
                        reportSummary(
                            "split offset=$splitOffset " +
                                "revision=$beforeRevision->${adapter.baseDocumentRevision} " +
                                "selection=${editText.selectionStart}..${editText.selectionEnd} " +
                                "trace=${editText.imeTraceSnapshotForTesting().takeLast(8)}"
                        )
                    }
                    assertEquals(
                        "return must split the midpoint paragraph and preserve all text",
                        expectedSplitText,
                        editText.text.toString()
                    )
                    assertEquals(
                        "return must move the caret after the split",
                        splitOffset + 1,
                        editText.selectionStart
                    )
                    assertEquals(
                        "return must leave a collapsed caret",
                        editText.selectionStart,
                        editText.selectionEnd
                    )
                    assertTrue(
                        "return must advance Rust revision",
                        adapter.baseDocumentRevision > beforeRevision
                    )
                    layoutView(
                        editText,
                        widthPx = 1080,
                        heightPx = 2400,
                        heightMode = View.MeasureSpec.AT_MOST
                    )
                    editText.lastApplyUpdateTrace()?.let(traces::add)
                }

                assertNotNull(
                    "edit text should have a layout after paragraph split",
                    editText.layout
                )
            }

            reportStats(stats)
            reportTraceStats(stats.name, traces.takeLast(MEASURED_ITERATIONS))
            assertTrue("average paragraph split time should be positive", stats.averageMillis > 0.0)
        } finally {
            releasePairedV2TestEditor(editorId)
        }
    }

    @Test
    fun performance_selectionScrubLargeDocument() {
        val (adapter, editorId) = createV2Editor()
        try {
            NativePerformanceFixtureFactory.loadLargeDocumentIntoEditor(adapter)
            val editText = instrumentation.runOnMainSyncWithResult {
                EditorEditText(targetContext).apply {
                    layoutParams = ViewGroup.LayoutParams(
                        ViewGroup.LayoutParams.MATCH_PARENT,
                        ViewGroup.LayoutParams.WRAP_CONTENT
                    )
                    setBaseStyle(baseFontSize, textColor, Color.WHITE)
                    bindEditor(editorId)
                    layoutView(
                        this,
                        widthPx = 1080,
                        heightPx = 2400,
                        heightMode = View.MeasureSpec.AT_MOST
                    )
                }
            }
            val scrubOffsets = NativePerformanceFixtureFactory.selectionScrubOffsets(
                editText.text ?: "",
                points = 48
            )

            val stats = measureOperation("selectionScrubLargeDocument") {
                instrumentation.runOnMainSyncWithResult {
                    for (offset in scrubOffsets) {
                        editText.setSelection(offset)
                    }
                }

                assertEquals(
                    "selection should land on the final scrub offset",
                    scrubOffsets.last(),
                    editText.selectionStart
                )
            }

            reportStats(stats)
            assertTrue("average selection scrub time should be positive", stats.averageMillis > 0.0)
        } finally {
            releasePairedV2TestEditor(editorId)
        }
    }

    @Test
    fun performance_hostedEditorLoadAndScrollLargeDocument() {
        val document = JSONObject(NativePerformanceFixtureFactory.largeDocumentJson())
        val coldLoads = mutableListOf<Long>()
        val scrollFrames = mutableListOf<Long>()
        val scrollDraws = mutableListOf<Long>()
        val scrollLayouts = mutableListOf<Long>()
        val completed = CountDownLatch(1)
        val frameThread = HandlerThread("editor-performance-frames").apply { start() }
        var token = 0L
        var host: RichTextEditorView? = null
        var window: Window? = null
        var callback: Choreographer.FrameCallback? = null
        var lastFrameTimestamp: Long? = null
        val scrollStart = AtomicReference<Long?>(null)
        val frameListener = Window.OnFrameMetricsAvailableListener { _, metrics, dropped ->
            val start = scrollStart.get() ?: return@OnFrameMetricsAvailableListener
            val timestamp = metrics.getMetric(FrameMetrics.VSYNC_TIMESTAMP)
            if (timestamp < start) return@OnFrameMetricsAvailableListener
            synchronized(scrollFrames) {
                assertEquals("scroll frame metrics must not be dropped", 0, dropped)
                lastFrameTimestamp?.let { previous ->
                    if (timestamp > previous) scrollFrames.add(timestamp - previous)
                }
                lastFrameTimestamp = timestamp
                scrollDraws.add(metrics.getMetric(FrameMetrics.DRAW_DURATION))
                scrollLayouts.add(metrics.getMetric(FrameMetrics.LAYOUT_MEASURE_DURATION))
            }
        }
        try {
            ActivityScenario.launch(NativeEditorOutsideTapActivity::class.java).use { scenario ->
                scenario.onActivity { activity ->
                    window = activity.window
                    activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                    activity.window.addOnFrameMetricsAvailableListener(
                        frameListener,
                        Handler(frameThread.looper)
                    )
                    repeat(MEASURED_ITERATIONS) {
                        host?.editorEditText?.unbindEditor()
                        if (token != 0L) releasePairedV2TestEditor(token)
                        token = createV2Editor().second
                        val editor = RichTextEditorView(activity).apply {
                            editorId = token
                            configure(
                                textSizePx = baseFontSize * resources.displayMetrics.density,
                                textColor = textColor,
                                backgroundColor = Color.WHITE
                            )
                        }
                        host = editor
                        activity.setContentView(editor)
                        val width = PreparedProseRecyclerHarness.viewportWidthPx(activity)
                        val height = PreparedProseRecyclerHarness.viewportHeightPx(activity)
                        val startedAt = System.nanoTime()
                        editor.setContent(document)
                        layoutView(editor, width, height, View.MeasureSpec.EXACTLY)
                        coldLoads.add(System.nanoTime() - startedAt)
                        assertTrue(
                            "cold load must produce the large document",
                            editor.editorEditText.text.length > MINIMUM_LARGE_TEXT_LENGTH
                        )
                        assertTrue(
                            "cold load must exceed the viewport",
                            editor.editorScrollView.canScrollVertically(1)
                        )
                    }
                }
                instrumentation.waitForIdleSync()
                reportStats(TimingStats("hostedEditorColdLoadLargeDocument", coldLoads))
                val initialLayoutBuilds = instrumentation.runOnMainSyncWithResult {
                    requireNotNull(host).editorEditText.documentLayoutBuildCount
                }
                instrumentation.runOnMainSync {
                    val editor = requireNotNull(host)
                    val scroll = editor.editorScrollView
                    val range = scroll.getChildAt(0).height - scroll.height
                    val startedAt = System.nanoTime()
                    scrollStart.set(startedAt)
                    val traversal = object : Choreographer.FrameCallback {
                        override fun doFrame(frameTimeNanos: Long) {
                            val elapsed = frameTimeNanos - startedAt
                            val fraction =
                                (
                                    1.0 -
                                        cos(
                                            elapsed.toDouble() / SCROLL_TRAVERSAL_NANOS * 2.0 *
                                                Math.PI
                                        )
                                    ) /
                                    2.0
                            scroll.scrollTo(0, (range * fraction).roundToInt())
                            editor.invalidate()
                            if (elapsed < SCROLL_TRAVERSAL_NANOS) {
                                Choreographer.getInstance().postFrameCallback(this)
                            } else {
                                completed.countDown()
                            }
                        }
                    }
                    callback = traversal
                    Choreographer.getInstance().postFrameCallback(traversal)
                }
                assertTrue(
                    "hosted scrolling must complete",
                    completed.await(SCROLL_TIMEOUT_SECONDS, TimeUnit.SECONDS)
                )
                instrumentation.waitForIdleSync()
                synchronized(scrollFrames) {
                    assertTrue(
                        "scroll must produce real frame evidence",
                        scrollFrames.size >= MINIMUM_SCROLL_FRAMES
                    )
                    reportStats(
                        TimingStats("hostedEditorScrollFrameIntervals", scrollFrames.toList())
                    )
                    reportStats(TimingStats("hostedEditorScrollDraw", scrollDraws.toList()))
                    reportStats(TimingStats("hostedEditorScrollLayout", scrollLayouts.toList()))
                }
                instrumentation.runOnMainSyncWithResult {
                    assertEquals(
                        "scrolling must reuse the prepared document layout",
                        initialLayoutBuilds,
                        requireNotNull(host).editorEditText.documentLayoutBuildCount
                    )
                }
            }
        } finally {
            instrumentation.runOnMainSync {
                callback?.let { Choreographer.getInstance().removeFrameCallback(it) }
                window?.removeOnFrameMetricsAvailableListener(frameListener)
                window?.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                host?.editorEditText?.unbindEditor()
                if (token != 0L) releasePairedV2TestEditor(token)
            }
            frameThread.quitSafely()
            frameThread.join(TimeUnit.SECONDS.toMillis(SCROLL_TIMEOUT_SECONDS))
            assertFalse("editor frame collector must close", frameThread.isAlive)
        }
    }

    @Test
    fun performance_remoteSelectionOverlayRefreshMultiPeerLargeDocument() {
        val updateJson = NativePerformanceFixtureFactory.largeUpdateJson()
        val richTextView = instrumentation.runOnMainSyncWithResult {
            RichTextEditorView(targetContext).apply {
                configure(
                    textSizePx = baseFontSize,
                    textColor = textColor,
                    backgroundColor = Color.WHITE
                )
                setRemoteSelectionEditorIdForTesting(1L)
                setRemoteSelectionScalarResolverForTesting { _, docPos -> docPos }
                editorEditText.applyUpdateJSON(updateJson, notifyListener = false)
                layoutView(
                    this,
                    widthPx = 1080,
                    heightPx = 1600,
                    heightMode = View.MeasureSpec.EXACTLY
                )
            }
        }

        val totalScalar = richTextView.editorEditText.text?.length ?: 0
        val selections = NativePerformanceFixtureFactory.remoteSelections(
            totalScalar,
            peerCount = 24,
            selectionWidth = 24
        )
        instrumentation.runOnMainSyncWithResult {
            richTextView.setRemoteSelections(selections)
            layoutView(
                richTextView,
                widthPx = 1080,
                heightPx = 1600,
                heightMode = View.MeasureSpec.EXACTLY
            )
        }

        val bitmap = Bitmap.createBitmap(1080, 1600, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)

        val stats = measureOperation("remoteSelectionOverlayRefreshMultiPeerLargeDocument") {
            val hasVisibleCaret = instrumentation.runOnMainSyncWithResult {
                bitmap.eraseColor(Color.TRANSPARENT)
                richTextView.setRemoteSelections(selections)
                layoutView(
                    richTextView,
                    widthPx = 1080,
                    heightPx = 1600,
                    heightMode = View.MeasureSpec.EXACTLY
                )
                richTextView.draw(canvas)
                richTextView.remoteSelectionDebugSnapshotsForTesting().any { it.caretRect != null }
            }

            assertTrue("remote selection overlay should resolve visible carets", hasVisibleCaret)
            assertEquals(
                "expected one snapshot per peer",
                selections.size,
                richTextView.remoteSelectionDebugSnapshotsForTesting().size
            )
        }

        reportStats(stats)
        assertTrue(
            "average remote selection refresh time should be positive",
            stats.averageMillis > 0.0
        )
    }

    private fun createV2Editor(): Pair<EditorV2Adapter, Long> =
        createPairedV2TestEditor().also { (adapter, _) ->
            adapter.onAutonomousError = { failure ->
                error("performance operation failed: ${failure.code}: ${failure.message}")
            }
        }

    private fun measureOperation(
        name: String,
        warmupIterations: Int = WARMUP_ITERATIONS,
        measuredIterations: Int = MEASURED_ITERATIONS,
        beforeEach: (() -> Unit)? = null,
        block: () -> Unit
    ): TimingStats {
        repeat(warmupIterations) {
            beforeEach?.invoke()
            block()
        }

        val samples = MutableList(measuredIterations) {
            beforeEach?.invoke()
            val startedAt = System.nanoTime()
            block()
            System.nanoTime() - startedAt
        }

        return TimingStats(name, samples)
    }

    private fun reportStats(stats: TimingStats) {
        reportSummary(stats.summaryString(tag = "NativeDevicePerformanceTest"))
    }

    private fun reportTraceStats(name: String, traces: List<EditorEditText.ApplyUpdateTrace>) {
        assertTrue("$name must capture update stage evidence", traces.isNotEmpty())
        reportSummary(
            ApplyUpdateTraceStats("$name.breakdown", traces)
                .summaryString(tag = "NativeDevicePerformanceTest")
        )
    }

    private fun reportSummary(summary: String) {
        instrumentation.sendStatus(
            0,
            Bundle().apply {
                putString(
                    Instrumentation.REPORT_KEY_STREAMRESULT,
                    summary + "\n"
                )
            }
        )
    }

    /** Persist evidence before any numerical assertion can terminate the test. */
    private fun reportPreparedProseExport(export: String) {
        val bytes = export.toByteArray(Charsets.UTF_8)
        val output = File(targetContext.filesDir, "prepared-prose-performance-export.json")
        output.writeBytes(bytes)
        val sha256 = MessageDigest.getInstance("SHA-256")
            .digest(bytes)
            .joinToString("") { "%02x".format(it) }
        instrumentation.sendStatus(
            0,
            Bundle().apply {
                putString(
                    Instrumentation.REPORT_KEY_STREAMRESULT,
                    "[PreparedProseExport] package=${targetContext.packageName} " +
                        "path=${output.absolutePath} bytes=${bytes.size} sha256=$sha256\n"
                )
            }
        )
    }

    private companion object {
        const val WARMUP_ITERATIONS = 3
        const val MEASURED_ITERATIONS = 5
        const val TYPING_OPERATIONS = 2
        const val MINIMUM_LARGE_TEXT_LENGTH = 10_000
        const val SCROLL_TRAVERSAL_NANOS = 5_000_000_000L
        const val SCROLL_TIMEOUT_SECONDS = 15L
        const val MINIMUM_SCROLL_FRAMES = 30
    }
}
