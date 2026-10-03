package com.apollohg.editor

import android.content.Intent
import android.graphics.Rect
import android.os.SystemClock
import android.text.Spanned
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.widget.LinearLayout
import androidx.core.view.accessibility.AccessibilityNodeInfoCompat
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.filters.SdkSuppress
import androidx.test.platform.app.InstrumentationRegistry
import com.apollohg.editor.tables.PlainTableFixture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class NativeTableHostTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private data class Widths(val editor: Int, val host: Int, val prepared: Int)

    @Test
    fun tableHostReservesGridHeightAndKeepsFollowingProseMapped() {
        runFixture(dark = false, screenshotName = "native-table-host-light.png", reflow = true)
    }

    @Test
    fun twentyThousandSlotTableRendersAndScrollsOnDevice() {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
            .putExtra(NativeTableHostActivity.EXTRA_PLAIN_ROWS, PlainTableFixture.LARGE_ROWS)
            .putExtra(NativeTableHostActivity.EXTRA_PLAIN_COLUMNS, PlainTableFixture.LARGE_COLUMNS)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitTableLayout(
                scenario,
                "native-table-large-timeout.png",
                timeoutMs = LARGE_TABLE_LAYOUT_TIMEOUT_MS
            )
            scenario.onActivity { activity ->
                val table = requireNotNull(
                    tableHosts(activity.richTextView).single().preparedLayout
                )
                    .blocks.mapNotNull { it.tableSurface }.single()
                assertTrue(
                    "Cold prepared layouts remain bounded",
                    table.cells.count { it.cachedContent != null } <=
                        com.apollohg.editor.tables.TableCellLayoutStore.MAXIMUM_RESIDENT_LAYOUTS
                )
                assertTrue(
                    "Unmounted bytes respect the production budget",
                    table.layoutStore.unmountedRetainedBytes <=
                        com.apollohg.editor.viewer.PREPARED_LAYOUT_UNMOUNTED_BYTE_BUDGET
                )
                assertTrue(
                    "All cell accessibility text survives",
                    table.cells.all {
                        it.accessibilityText.isNotBlank()
                    }
                )
            }
            listOf(0f, LARGE_TABLE_SCROLL_MIDDLE, 1f).forEach { fraction ->
                val preparations = mutableListOf<Int>()
                scenario.onActivity { activity ->
                    activity.richTextView.editorTableSurface.onTableCellPreparedForTesting =
                        { index, _ ->
                            preparations.add(index)
                            Unit
                        }
                    val scroll = activity.richTextView.editorScrollView
                    val bottom = (scroll.getChildAt(0).height - scroll.height).coerceAtLeast(0)
                    scroll.scrollTo(0, (bottom * fraction).toInt())
                }
                instrumentation.waitForIdleSync()
                scenario.onActivity { activity ->
                    val drawing = tableHosts(activity.richTextView).single()
                    val surface = requireNotNull(drawing.preparedLayout).blocks.mapNotNull {
                        it.tableSurface
                    }.single()
                    assertEquals(
                        "every cell keeps measured metadata",
                        PlainTableFixture.LARGE_ROWS * PlainTableFixture.LARGE_COLUMNS,
                        surface.cells.size
                    )
                    val visible = android.graphics.Rect()
                    assertTrue(
                        "the table is on screen at $fraction",
                        drawing.getLocalVisibleRect(visible)
                    )
                    val presented = drawing.presentedTableCells()
                    if (fraction == LARGE_TABLE_SCROLL_MIDDLE) {
                        assertTrue(
                            "Scrolling to an uncached region prepares entering cells",
                            preparations.isNotEmpty()
                        )
                    }
                    preparations.clear()
                    drawing.presentedTableCells()
                    assertTrue("Repeated presentation reuses cells", preparations.isEmpty())
                    activity.richTextView.editorTableSurface.onTableCellPreparedForTesting = null
                    println(
                        "large table at $fraction: ${presented.size} presented, visible $visible"
                    )
                    val bound = PlainTableFixture.maximumPresentedCells(
                        surface.style,
                        visible.width().toFloat(),
                        visible.height().toFloat()
                    )
                    assertTrue(
                        "the presentation stays within the viewport window bound $bound at $fraction: ${presented.size}",
                        presented.size <= bound
                    )
                    assertTrue(
                        "the cell under the viewport centre is presented at $fraction",
                        presented.any {
                            it.bounds.contains(visible.exactCenterX(), visible.exactCenterY())
                        }
                    )
                    val canvasHeight = requireNotNull(drawing.preparedLayout).heightPx
                    assertTrue(
                        "the drawn canvas reaches the visible rows at $fraction: $canvasHeight < ${visible.bottom}",
                        canvasHeight >= visible.bottom
                    )
                }
            }
            val cellIndex = PlainTableFixture.LARGE_ROWS * PlainTableFixture.LARGE_COLUMNS / 2
            var nodeId = 0
            scenario.onActivity { activity ->
                val drawing = tableHosts(activity.richTextView).single()
                val surface = requireNotNull(drawing.preparedLayout).blocks.mapNotNull {
                    it.tableSurface
                }.single()
                nodeId =
                    requireNotNull(
                        drawing.tableAccessibilityLocation(surface, cellIndex)
                    ).cellNodeId
                assertTrue(
                    drawing.accessibilityNodeProvider.performAction(
                        nodeId,
                        AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS,
                        null
                    )
                )
            }
            instrumentation.waitForIdleSync()
            scenario.onActivity { activity ->
                val drawing = tableHosts(activity.richTextView).single()
                assertTrue(
                    drawing.accessibilityNodeProvider.performAction(
                        nodeId,
                        AccessibilityNodeInfo.ACTION_CLICK,
                        null
                    )
                )
            }
            instrumentation.waitForIdleSync()
            var retainedNodeRecords = emptyMap<String, Int>()
            scenario.onActivity { activity ->
                val drawing = tableHosts(activity.richTextView).single()
                val probe = android.graphics.RenderNode("typing-probe")
                val canvas = probe.beginRecording(drawing.width, drawing.height)
                try {
                    drawing.draw(canvas)
                } finally {
                    probe.endRecording()
                    probe.discardDisplayList()
                }
                retainedNodeRecords = drawing.nodeRecordsForTesting.toMap()
            }
            repeat(PlainTableFixture.TYPING_PROBE_CHARACTERS) { keystroke ->
                scenario.onActivity { activity ->
                    val view = activity.richTextView
                    val prepared = mutableListOf<Int>()
                    view.editorTableSurface.onTableCellPreparedForTesting =
                        { index, _ ->
                            prepared.add(index)
                            Unit
                        }
                    try {
                        val input = view.activeTextInput
                        assertTrue(input !== view.editorEditText)
                        assertTrue(
                            requireNotNull(
                                input.onCreateInputConnection(EditorInfo())
                            ).commitText("x", 1)
                        )
                        assertEquals(
                            "keystroke $keystroke prepares only its cell",
                            listOf(cellIndex),
                            prepared
                        )
                    } finally {
                        view.editorTableSurface.onTableCellPreparedForTesting = null
                    }
                }
                instrumentation.waitForIdleSync()
            }
            scenario.onActivity { activity ->
                val drawing = tableHosts(activity.richTextView).single()
                val probe = android.graphics.RenderNode("typing-probe")
                val canvas = probe.beginRecording(drawing.width, drawing.height)
                try {
                    drawing.draw(canvas)
                } finally {
                    probe.endRecording()
                    probe.discardDisplayList()
                }
                for (name in listOf("above", "below")) {
                    assertEquals(
                        "$name is retained over ${PlainTableFixture.TYPING_PROBE_CHARACTERS} keystrokes",
                        retainedNodeRecords[name],
                        drawing.nodeRecordsForTesting[name]
                    )
                }
                assertTrue(
                    "the bound node actually recorded typing",
                    drawing.nodeRecordsForTesting.getValue("boundCell") >
                        retainedNodeRecords.getValue("boundCell")
                )
                val activeInput = activity.richTextView.activeTextInput
                activity.richTextView.editorScrollView.scrollTo(0, 0)
                drawing.presentedTableCells()
                val surface = requireNotNull(drawing.preparedLayout).blocks.mapNotNull {
                    it.tableSurface
                }.single()
                val active = requireNotNull(surface.cell(cellIndex))
                val visible = Rect()
                assertTrue(drawing.getLocalVisibleRect(visible))
                assertTrue(
                    "The bound cell is outside the presentation window",
                    surface.frameOfCell(active).top > visible.bottom
                )
                assertNotNull(
                    "The active input cell stays prepared offscreen",
                    active.cachedContent
                )
                assertTrue(
                    "Scrolling preserves the input binding",
                    activeInput === activity.richTextView.activeTextInput
                )
            }
            instrumentation.saveDeviceScreenshot("native-table-large-scrolled.png")
        }
    }

    @Test
    fun largeTableScreenReaderFocusRevealsEachWalkedRowOnDevice() {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
            .putExtra(NativeTableHostActivity.EXTRA_PLAIN_ROWS, PlainTableFixture.LARGE_ROWS)
            .putExtra(NativeTableHostActivity.EXTRA_PLAIN_COLUMNS, PlainTableFixture.LARGE_COLUMNS)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitTableLayout(
                scenario,
                "native-table-large-a11y-timeout.png",
                timeoutMs = LARGE_TABLE_LAYOUT_TIMEOUT_MS
            )
            PlainTableFixture.ACCESSIBILITY_WALK_ROWS.forEach { row ->
                val column = row % PlainTableFixture.LARGE_COLUMNS
                var id = 0
                scenario.onActivity { activity ->
                    val drawing = tableHosts(activity.richTextView).single()
                    val surface = requireNotNull(drawing.preparedLayout).blocks.mapNotNull {
                        it.tableSurface
                    }.single()
                    id =
                        requireNotNull(
                            drawing.tableAccessibilityLocation(
                                surface,
                                row * PlainTableFixture.LARGE_COLUMNS + column
                            )
                        ) {
                            "cell $row,$column has an accessibility node"
                        }.cellNodeId
                    assertTrue(
                        "focus reaches row $row",
                        drawing.accessibilityNodeProvider.performAction(
                            id,
                            AccessibilityNodeInfo.ACTION_ACCESSIBILITY_FOCUS,
                            null
                        )
                    )
                }
                instrumentation.waitForIdleSync()
                if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
                    awaitCommittedFrame(scenario)
                }
                scenario.onActivity { activity ->
                    val drawing = tableHosts(activity.richTextView).single()
                    val info =
                        requireNotNull(
                            drawing.accessibilityNodeProvider.createAccessibilityNodeInfo(id)
                        )
                    val item =
                        requireNotNull(AccessibilityNodeInfoCompat.wrap(info).collectionItemInfo)
                    val bounds = Rect().also(info::getBoundsInScreen)
                    val metrics = activity.resources.displayMetrics
                    val screen = Rect(0, 0, metrics.widthPixels, metrics.heightPixels)
                    println(
                        (
                            "row $row column $column: focused " +
                                "${info.isAccessibilityFocused} rowIndex ${item.rowIndex} "
                            ) +
                            "title '${item.columnTitle}' screen $bounds of $screen"
                    )
                    assertTrue(
                        "row $row is revealed on screen: $bounds",
                        Rect.intersects(bounds, screen)
                    )
                    assertTrue("row $row keeps focus after its reveal", info.isAccessibilityFocused)
                    assertEquals(row, item.rowIndex)
                    assertEquals(
                        "row $row announces its column header",
                        PlainTableFixture.coordinateText(0, column),
                        item.columnTitle
                    )
                }
            }
        }
    }

    @Test
    fun darkTableHostRendersOnDevice() {
        runFixture(dark = true, screenshotName = "native-table-host-dark.png", reflow = false)
    }

    @Test
    fun draggingSelectedTableHandlePublishesExactCellSelectionOnDevice() {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitTableLayout(scenario, "native-table-selection-layout-timeout.png")
            var fromX = 0f
            var fromY = 0f
            var toX = 0f
            var toY = 0f
            var targetOpening = -1
            lateinit var beforeDocument: String
            var beforeRevision = 0uL
            var beforeHistory: Pair<Boolean?, Boolean?> = null to null
            scenario.onActivity { activity ->
                val adapter = activity.adapter
                val root = activity.richTextView.editorEditText
                val cells = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
                fun point(index: Int) = JSONObject().put("kind", "document")
                    .put("offset", cells.getJSONObject(index).getInt("sourcePos"))
                val selection = JSONObject().put("type", "cell")
                    .put("anchorCell", point(2)).put("headCell", point(3))
                val admitted = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                    UniffiEditorV2Backend.setSelection(adapter.editorId, it)
                }
                assertTrue(
                    "device selection admission=$admitted",
                    admitted is EditorV2CallResult.Ok
                )
                assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
                val drawing = tableHosts(activity.richTextView).single()
                val handles = drawing.selectionHandles()
                assertEquals(2, handles.size)
                val head = handles.single {
                    it.role ==
                        com.apollohg.editor.viewer.TableSelectionHandleRole.HEAD
                }
                targetOpening = cells.getJSONObject(4).getInt("sourcePos")
                val target = drawing.presentedTableCells().single { it.sourceIndex == 4 }
                val location = IntArray(2)
                drawing.getLocationOnScreen(location)
                fromX = location[0] + head.x
                fromY = location[1] + head.y
                toX = location[0] + target.bounds.centerX()
                toY = location[1] + target.bounds.centerY()
                beforeDocument = requireNotNull(adapter.documentJson())
                beforeRevision = adapter.baseDocumentRevision
                beforeHistory = adapter.historyCanUndo() to adapter.historyCanRedo()
            }
            awaitCommittedFrame(scenario)
            instrumentation.saveDeviceScreenshot("native-table-selection-handles-before.png")
            val start = SystemClock.uptimeMillis()
            listOf(
                MotionEvent.obtain(start, start, MotionEvent.ACTION_DOWN, fromX, fromY, 0),
                MotionEvent.obtain(start, start + 30, MotionEvent.ACTION_MOVE, toX, toY, 0),
                MotionEvent.obtain(start, start + 60, MotionEvent.ACTION_UP, toX, toY, 0)
            ).forEach { event ->
                try {
                    instrumentation.sendPointerSync(event)
                } finally {
                    event.recycle()
                }
            }
            instrumentation.waitForIdleSync()
            scenario.onActivity { activity ->
                val adapter = activity.adapter
                val rendered = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
                assertTrue(rendered is EditorV2CallResult.Ok)
                val canonical = JSONObject((rendered as EditorV2CallResult.Ok).value)
                    .getJSONObject("selection")
                assertEquals("cell", canonical.getString("type"))
                assertEquals(targetOpening, canonical.getInt("headCell"))
                assertTrue(activity.richTextView.editorEditText.authoritativeCellSelectionActive)
                assertEquals(beforeDocument, adapter.documentJson())
                assertEquals(beforeRevision, adapter.baseDocumentRevision)
                assertEquals(beforeHistory, adapter.historyCanUndo() to adapter.historyCanRedo())
            }
            awaitCommittedFrame(scenario)
            instrumentation.saveDeviceScreenshot("native-table-selection-handles-after.png")
        }
    }

    @Test
    @SdkSuppress(minSdkVersion = 29)
    fun tappingCellsReusesInputAndRetiresPreviousConnections() {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitTableLayout(scenario, "native-table-cell-timeout.png")
            tapCell(scenario, 2)
            instrumentation.saveDeviceScreenshot("native-table-cell-tapped.png")
            lateinit var cellInput: EditorEditText
            lateinit var firstConnection: InputConnection
            scenario.onActivity { activity ->
                val root = activity.richTextView.editorEditText
                assertEquals(
                    "tapping must not mutate the document",
                    activity.documentBeforeMount,
                    activity.adapter.documentJson()
                )
                assertTrue(
                    (
                        "focus=${activity.currentFocus} " +
                            "active=${activity.richTextView.activeTextInput} "
                        ) +
                        "inputs=${countEditorInputs(
                            activity.richTextView
                        )} rootFocused=${root.hasFocus()} " +
                        "hosts=${tableHosts(activity.richTextView).size} rootText=${root.text} " +
                        (
                            "revision=${activity.adapter.baseDocumentRevision} " +
                                "applied=${root.lastAppliedDocumentVersion} "
                            ) +
                        "tables=${activity.adapter.tableRecordsForTesting.keys} maps=${root.rootTableMapTableIds}",
                    activity.currentFocus is EditorEditText
                )
                cellInput = activity.currentFocus as EditorEditText
                assertTrue("cell tap must focus the reusable cell input", cellInput !== root)
                assertEquals("Alpha", cellInput.text.toString())
                assertEquals(2, countEditorInputs(activity.richTextView))
                cellInput.setSelection(cellInput.text.length)
                firstConnection = requireNotNull(cellInput.onCreateInputConnection(EditorInfo()))
                assertTrue(firstConnection.commitText("😀", 1))
                assertEquals("Alpha😀", cellText(activity, 1, 0))
                assertTrue(firstConnection.commitText("Z", 1))
                assertEquals("Alpha😀Z", cellText(activity, 1, 0))
                assertTrue(firstConnection.deleteSurroundingTextInCodePoints(1, 0))
                assertEquals("Alpha😀", cellText(activity, 1, 0))
                assertTrue(firstConnection.deleteSurroundingTextInCodePoints(1, 0))
                assertEquals("Alpha", cellText(activity, 1, 0))
                assertTrue(firstConnection.commitText("X", 1))
                assertEquals("AlphaX", cellText(activity, 1, 0))
            }
            instrumentation.waitForIdleSync()
            scenario.onActivity { activity ->
                assertEquals("AlphaX", cellText(activity, 1, 0))
                assertEquals("AlphaX", cellInput.text.toString())
                assertSame(cellInput, activity.currentFocus)
            }
            awaitCommittedFrame(scenario)
            instrumentation.saveDeviceScreenshot("native-table-cell-editing.png")
            tapCell(scenario, 3)
            lateinit var secondConnection: InputConnection
            var reflowWidth = 0
            lateinit var beforeReflow: Widths
            scenario.onActivity { activity ->
                assertSame(cellInput, activity.currentFocus)
                assertEquals("Owner", cellInput.text.toString())
                val beforeStale = activity.adapter.documentJson()
                assertFalse(firstConnection.beginBatchEdit())
                firstConnection.commitText("stale", 1)
                assertEquals(beforeStale, activity.adapter.documentJson())
                assertEquals("Owner", cellInput.text.toString())
                cellInput.setSelection(cellInput.text.length)
                secondConnection = requireNotNull(cellInput.onCreateInputConnection(EditorInfo()))
                assertTrue(secondConnection.setComposingText("pending", 1))
                assertEquals(beforeStale, activity.adapter.documentJson())
                beforeReflow = widths(activity.richTextView)
                reflowWidth = (activity.richTextView.width * 0.78f).toInt()
                activity.richTextView.layoutParams = activity.richTextView.layoutParams.apply {
                    width = reflowWidth
                }
            }
            awaitTableLayout(
                scenario,
                "native-table-cell-composition-timeout.png",
                reflowWidth,
                beforeReflow
            )
            scenario.onActivity { activity ->
                assertSame(cellInput, activity.currentFocus)
                assertEquals("Ownerpending", cellInput.text.toString())
                assertEquals("Owner", cellText(activity, 1, 1))
                assertTrue(secondConnection.finishComposingText())
                assertEquals("Ownerpending", cellText(activity, 1, 1))
                assertEquals("AlphaX", cellText(activity, 1, 0))
            }
            awaitCommittedFrame(scenario)
            instrumentation.saveDeviceScreenshot("native-table-cell-composed.png")
            tapFollowingProse(scenario)
            scenario.onActivity { activity ->
                assertSame(activity.richTextView.editorEditText, activity.currentFocus)
                val beforeStale = activity.adapter.documentJson()
                val beforeRootText = activity.richTextView.editorEditText.text.toString()
                assertFalse(secondConnection.beginBatchEdit())
                secondConnection.commitText("stale", 1)
                assertEquals(beforeStale, activity.adapter.documentJson())
                assertEquals(beforeRootText, activity.richTextView.editorEditText.text.toString())
            }
        }
    }

    private fun awaitCommittedFrame(scenario: ActivityScenario<NativeTableHostActivity>) {
        val committed = CountDownLatch(1)
        val callback = Runnable { committed.countDown() }
        scenario.onActivity { activity ->
            val decor = activity.window.decorView
            assertTrue(decor.isHardwareAccelerated)
            decor.viewTreeObserver.registerFrameCommitCallback(callback)
            decor.invalidate()
        }
        try {
            assertTrue("edited cell frame must be submitted", committed.await(3, TimeUnit.SECONDS))
        } finally {
            scenario.onActivity { activity ->
                activity.window.decorView.viewTreeObserver.unregisterFrameCommitCallback(callback)
            }
        }
    }

    private fun cellText(activity: NativeTableHostActivity, row: Int, column: Int): String {
        val document = JSONObject(requireNotNull(activity.adapter.documentJson()))
        return document.getJSONArray("content").getJSONObject(1)
            .getJSONArray("content").getJSONObject(row)
            .getJSONArray("content").getJSONObject(column)
            .getJSONArray("content").getJSONObject(0)
            .getJSONArray("content").getJSONObject(0).getString("text")
    }

    private fun tapCell(scenario: ActivityScenario<NativeTableHostActivity>, cellIndex: Int) {
        val point = instrumentation.tableCellScreenPoint(scenario, cellIndex)
        tap(point.x, point.y)
    }

    private fun tapFollowingProse(scenario: ActivityScenario<NativeTableHostActivity>) {
        fun target(input: EditorEditText): Rect {
            val offset = input.text.indexOf("After table.") + 4
            val layout = requireNotNull(input.layout)
            val line = layout.getLineForOffset(offset)
            val x = input.totalPaddingLeft + layout.getPrimaryHorizontal(offset).toInt()
            return Rect(
                x,
                input.totalPaddingTop + layout.getLineTop(line),
                x + 1,
                input.totalPaddingTop + layout.getLineBottom(line)
            )
        }
        scenario.onActivity { activity ->
            val input = activity.richTextView.editorEditText
            input.requestRectangleOnScreen(target(input), true)
        }
        instrumentation.waitForIdleSync()
        var x = 0f
        var y = 0f
        scenario.onActivity { activity ->
            val input = activity.richTextView.editorEditText
            val bounds = target(input)
            val viewport = Rect()
            assertTrue(input.getLocalVisibleRect(viewport))
            assertTrue(
                "following prose must be visible: $bounds in $viewport",
                bounds.intersect(viewport)
            )
            val location = IntArray(2).also(input::getLocationOnScreen)
            x = location[0] + bounds.exactCenterX()
            y = location[1] + bounds.exactCenterY()
        }
        tap(x, y)
    }

    private fun tap(x: Float, y: Float) {
        val start = SystemClock.uptimeMillis()
        val down = MotionEvent.obtain(start, start, MotionEvent.ACTION_DOWN, x, y, 0)
        val up = MotionEvent.obtain(start, start + 40, MotionEvent.ACTION_UP, x, y, 0)
        try {
            instrumentation.sendPointerSync(down)
            instrumentation.sendPointerSync(up)
            instrumentation.waitForIdleSync()
        } finally {
            down.recycle()
            up.recycle()
        }
    }

    private fun runFixture(dark: Boolean, screenshotName: String, reflow: Boolean) {
        val intent = Intent(instrumentation.targetContext, NativeTableHostActivity::class.java)
            .putExtra(NativeTableHostActivity.EXTRA_DARK, dark)
        ActivityScenario.launch<NativeTableHostActivity>(intent).use { scenario ->
            awaitTableLayout(scenario, "${screenshotName.removeSuffix(".png")}-timeout.png")
            lateinit var before: Widths
            scenario.onActivity { activity ->
                assertTableLayout(activity)
                before = widths(activity.richTextView)
            }
            instrumentation.saveDeviceScreenshot(screenshotName)
            if (reflow) {
                var targetWidth = 0
                scenario.onActivity { activity ->
                    val editor = activity.richTextView
                    val params = editor.layoutParams as LinearLayout.LayoutParams
                    targetWidth = (editor.width * 0.78f).toInt()
                    params.width = targetWidth
                    editor.layoutParams = params
                }
                awaitTableLayout(
                    scenario,
                    "${screenshotName.removeSuffix(".png")}-reflow-timeout.png",
                    targetWidth,
                    before
                )
                scenario.onActivity { activity ->
                    assertTableLayout(activity)
                    val after = widths(activity.richTextView)
                    assertEquals(targetWidth, after.editor)
                    assertTrue("table host width did not reflow", after.host != before.host)
                    assertTrue(
                        "prepared table width did not reflow",
                        after.prepared != before.prepared
                    )
                }
            }
        }
    }

    private fun awaitTableLayout(
        scenario: ActivityScenario<NativeTableHostActivity>,
        timeoutScreenshotName: String,
        expectedEditorWidth: Int? = null,
        previous: Widths? = null,
        timeoutMs: Long = TABLE_LAYOUT_TIMEOUT_MS
    ) {
        val deadline = SystemClock.uptimeMillis() + timeoutMs
        var lastReadinessState = "activity not observed"
        do {
            instrumentation.waitForIdleSync()
            var ready = false
            scenario.onActivity { activity ->
                val editor = activity.richTextView
                val input = editor.editorEditText
                val adapter = activity.adapter
                val host = tableHosts(editor).singleOrNull()
                val prepared = host?.preparedLayout
                ready = editor.width > 0 && host != null && host.height > 0 &&
                    prepared != null && input.layout != null &&
                    !editor.isLayoutRequested && !host.isLayoutRequested &&
                    (expectedEditorWidth == null || editor.width == expectedEditorWidth) &&
                    (
                        previous == null ||
                            (host.width != previous.host && prepared.widthPx != previous.prepared)
                        )
                lastReadinessState = buildString {
                    append("editor=${editor.width}x${editor.height}")
                    append(" expectedEditorWidth=$expectedEditorWidth previous=$previous")
                    append(
                        " frame=${editor.editorContentFrame.width}x${editor.editorContentFrame.height}"
                    )
                    append(
                        " input=${input.width}x${input.height} inputLayout=${input.layout != null}"
                    )
                    append(
                        " hostCount=${tableHosts(editor).size} host=${host?.width}x${host?.height}"
                    )
                    append(" prepared=${prepared?.widthPx}x${prepared?.heightPx}")
                    append(" preparedBlocks=${prepared?.blocks?.size}")
                    append(" preparedTables=${prepared?.blocks?.count { it.tableSurface != null }}")
                    append(" layoutRequested(editor/frame/input/host)=")
                    append(
                        "${editor.isLayoutRequested}/${editor.editorContentFrame.isLayoutRequested}/"
                    )
                    append("${input.isLayoutRequested}/${host?.isLayoutRequested}")
                    append(" rootText=${JSONObject.quote(input.text.toString().take(180))}")
                    append(" cachedRevision=${adapter.cachedAtomicRenderDocumentRevision}")
                    append(" baseRevision=${adapter.baseDocumentRevision}")
                    append(" lastAppliedVersion=${input.lastAppliedDocumentVersion}")
                    append(" cachedMappingIds=${adapter.tableMappingsForTesting?.tables?.keys}")
                    append(" rootMapIds=${input.rootTableMapTableIds}")
                    append(" rootMapVersion=${input.rootTableMapDocumentVersion}")
                    append(" rootMapPresent=${input.rootTablePositionMap != null}")
                }
            }
            if (ready) return
            SystemClock.sleep(16L)
        } while (SystemClock.uptimeMillis() < deadline)
        val screenshot = runCatching { instrumentation.saveDeviceScreenshot(timeoutScreenshotName) }
            .fold(onSuccess = { it.absolutePath }, onFailure = { "failed: $it" })
        error(
            "Native table host did not finish layout: $lastReadinessState; screenshot=$screenshot"
        )
    }

    private fun assertTableLayout(activity: NativeTableHostActivity) {
        val editor = activity.richTextView
        val input = editor.editorEditText
        val content = input.text as Spanned
        val layout = requireNotNull(input.layout)
        val markerOffset = content.indexOf('\u200B')
        val beforeOffset = content.indexOf("Before table.")
        val afterOffset = content.indexOf("After table.")
        assertTrue("root table marker is missing", markerOffset >= 0)
        assertTrue("before prose is missing", beforeOffset >= 0)
        assertTrue("following prose is missing", afterOffset >= 0)

        val tableLine = layout.getLineForOffset(markerOffset)
        val beforeLine = layout.getLineForOffset(beforeOffset)
        val afterLine = layout.getLineForOffset(afterOffset)
        val tableLineHeight = layout.getLineBottom(tableLine) - layout.getLineTop(tableLine)
        val proseLineHeight = layout.getLineBottom(beforeLine) - layout.getLineTop(beforeLine)
        assertTrue(
            "table marker line must reserve grid height: $tableLineHeight <= $proseLineHeight",
            tableLineHeight > proseLineHeight
        )

        val host = tableHosts(editor).single()
        assertTrue(host.isShown)
        assertTrue(host.width > 0 && host.height > proseLineHeight)
        val tableBlock = requireNotNull(host.preparedLayout).blocks
            .single { it.tableSurface != null }
        val surface = requireNotNull(tableBlock.tableSurface)
        assertEquals(null, surface.layout.failure)
        assertEquals(6, surface.cells.size)
        assertTrue(surface.cells.any { it.isHeader })
        assertTrue(surface.cells.any { !it.isHeader })
        val sourceCells = requireNotNull(surface.sourceTable).cells
        assertEquals(2, sourceCells.count { it.header })
        assertTrue(sourceCells.any { it.rowspan == 2 })
        assertTrue(sourceCells.any { it.colspan == 2 })
        val frames = surface.layout.rectangles.values.toList()
        assertEquals(6, frames.size)
        frames.forEach { frame -> assertTrue(frame.width > 0f && frame.height > 0f) }
        frames.forEachIndexed { index, left ->
            frames.drop(index + 1).forEach { right ->
                val overlapWidth = minOf(left.left + left.width, right.left + right.width) -
                    maxOf(left.left, right.left)
                val overlapHeight = minOf(left.top + left.height, right.top + right.height) -
                    maxOf(left.top, right.top)
                assertTrue("table cell frames overlap", overlapWidth <= 0f || overlapHeight <= 0f)
            }
        }
        val followingTop = input.top + input.totalPaddingTop + layout.getLineTop(afterLine)
        val tableBottom = host.top + requireNotNull(tableBlock.tableBounds).bottom
        assertTrue(
            "following prose overlaps the table: $tableBottom > $followingTop",
            tableBottom <= followingTop
        )

        val extent = requireNotNull(
            activity.adapter.tableMappingsForTesting?.tables?.values?.single()?.extent
        )
        assertEquals(
            extent.scalarEnd + 1,
            input.inputScalarAtLocalUtf16(afterOffset, content.toString())
        )
        assertEquals(1, countEditorInputs(editor))
        assertFalse(editor.editorContentFrame.getChildAt(0) === host)
        assertEquals(activity.documentBeforeMount, activity.adapter.documentJson())
        assertEquals(
            activity.historyBeforeMount,
            activity.adapter.historyCanUndo() to activity.adapter.historyCanRedo()
        )
        assertEquals(activity.revisionBeforeMount, activity.adapter.baseDocumentRevision)
    }

    private fun widths(editor: RichTextEditorView): Widths {
        val host = tableHosts(editor).single()
        return Widths(editor.width, host.width, requireNotNull(host.preparedLayout).widthPx)
    }

    private fun countEditorInputs(view: View): Int {
        val self = if (view is EditorEditText) 1 else 0
        val children = view as? ViewGroup ?: return self
        return self +
            (0 until children.childCount).sumOf { countEditorInputs(children.getChildAt(it)) }
    }

    private companion object {
        const val TABLE_LAYOUT_TIMEOUT_MS = 5_000L
        const val LARGE_TABLE_LAYOUT_TIMEOUT_MS = 300_000L
        const val LARGE_TABLE_SCROLL_MIDDLE = 0.5f
    }
}
