package com.apollohg.editor

import android.app.Activity
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Rect
import android.os.Looper
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.widget.FrameLayout
import com.apollohg.editor.tables.EditorCellSelection
import com.apollohg.editor.tables.TableStyle
import com.apollohg.editor.tables.resolveEditorCellSelection
import com.apollohg.editor.viewer.PreparedProseDrawingView
import com.apollohg.editor.viewer.TableSelectionHandleRole
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.annotation.LooperMode

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], qualifiers = "w1000dp-h1000dp")
internal class EditorCellSelectionAdmissionTest {
    private val config = """{"schema":{"nodes":[{"name":"doc","content":"block+",""" +
        """"role":"doc"},{"name":"paragraph","content":"inline*",""" +
        """"group":"block","role":"textBlock"},{"name":"text",""" +
        """"content":"","group":"inline","role":"text"},""" +
        """{"name":"table","content":"table_row+","group":"block",""" +
        """"role":"block","tableRole":"table"},""" +
        """{"name":"table_row","content":"(table_cell | """ +
        """table_header)*","role":"block","tableRole":"row"},""" +
        """{"name":"table_cell","content":"block+","role":"block",""" +
        """"tableRole":"cell","attrs":{"colspan":{"type":"number",""" +
        """"default":1,"min":1},"rowspan":{"type":"number",""" +
        """"default":1,"min":1},"colwidth":{"default":null}}},""" +
        """{"name":"table_header","content":"block+",""" +
        """"role":"block","tableRole":"header_cell",""" +
        """"attrs":{"colspan":{"type":"number","default":1,""" +
        """"min":1},"rowspan":{"type":"number","default":1,""" +
        """"min":1},"colwidth":{"default":null}}}],"marks":[]},""" +
        """"initialization":{"type":"localEmpty"}}"""
    private val rtlConfig = config.replace(
        "\"tableRole\":\"table\"}",
        "\"tableRole\":\"table\",\"attrs\":{\"dir\":{\"default\":null}}}"
    )
    private val document = """{"type":"doc","content":[{"type":"table",""" +
        """"content":[{"type":"table_row",""" +
        """"content":[{"type":"table_cell",""" +
        """"content":[{"type":"paragraph",""" +
        """"content":[{"type":"text","text":"one"}]}]},""" +
        """{"type":"table_cell","content":[{"type":"paragraph",""" +
        """"content":[{"type":"text","text":"two"}]}]}]}]},""" +
        """{"type":"paragraph","content":[{"type":"text",""" +
        """"text":"after"}]}]}"""

    private fun selectCells(
        adapter: EditorV2Adapter,
        anchorIndex: Int = 0,
        headIndex: Int = 1
    ): Pair<Int, Int> {
        val cells = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
        val anchor = cells.getJSONObject(anchorIndex).getInt("sourcePos")
        val head = cells.getJSONObject(headIndex).getInt("sourcePos")
        fun point(docPos: Int) =
            JSONObject().put("offset", requireNotNull(adapter.scalarPositionForDoc(docPos + 2)))
                .put("kind", "scalar")
        val selection = JSONObject().put("type", "cell")
            .put("anchorCell", point(anchor)).put("headCell", point(head))
        val result = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
            UniffiEditorV2Backend.setSelection(adapter.editorId, it)
        }
        assertTrue("engine rejected cell selection: $result", result is EditorV2CallResult.Ok)
        return anchor to head
    }

    private fun withMountedSelection(
        content: String,
        width: Int,
        anchorIndex: Int,
        headIndex: Int,
        tableStyle: TableStyle? = null,
        schemaConfig: String = config,
        exactSelection: Boolean = true,
        backend: EditorV2Backend = UniffiEditorV2Backend,
        roomBound: Boolean = false,
        block: (RichTextEditorView, EditorV2Adapter, PreparedProseDrawingView) -> Unit
    ) {
        val created = UniffiEditorV2Backend.create(schemaConfig, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                backend,
                JSONObject(created.value).getString("editorId"),
                roomBound
            )
        )
        val token = EditorV2Registry.register(adapter)
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        val view = RichTextEditorView(activity)
        activity.setContentView(view, FrameLayout.LayoutParams(width, 500))
        try {
            val initial = requireNotNull(adapter.setContentJson(content))
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            if (tableStyle != null) view.applyTheme(EditorTheme(table = tableStyle))
            view.measure(
                View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, width, 500)
            activity.window.decorView.measure(
                View.MeasureSpec.makeMeasureSpec(width, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            activity.window.decorView.layout(0, 0, width, 500)
            if (exactSelection) {
                val cells = adapter.tableRecordsForTesting.values.minBy { it.getInt("tablePos") }
                    .getJSONArray("cells")
                fun point(index: Int) = JSONObject().put("kind", "document")
                    .put("offset", cells.getJSONObject(index).getInt("sourcePos"))
                val selection = JSONObject().put("type", "cell")
                    .put("anchorCell", point(anchorIndex)).put("headCell", point(headIndex))
                val admitted = adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                    UniffiEditorV2Backend.setSelection(adapter.editorId, it)
                }
                assertTrue(
                    "engine rejected exact selection: $admitted",
                    admitted is EditorV2CallResult.Ok
                )
            } else {
                selectCells(adapter, anchorIndex, headIndex)
            }
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
            val drawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            assertTrue("selection fixture must have a visible window", drawing.isShown)
            block(view, adapter, drawing)
        } finally {
            view.editorId = 0L
            controller.pause().stop().destroy()
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    private fun dragHandle(
        view: RichTextEditorView,
        fromX: Float,
        fromY: Float,
        toX: Float,
        toY: Float
    ) {
        dragHandleThrough(view, fromX, fromY, listOf(toX to toY))
    }

    private fun dragHandleThrough(
        view: RichTextEditorView,
        fromX: Float,
        fromY: Float,
        targets: List<Pair<Float, Float>>
    ) {
        val events = mutableListOf(
            MotionEvent.obtain(
                0,
                0,
                MotionEvent.ACTION_DOWN,
                fromX,
                fromY,
                0
            )
        )
        targets.forEachIndexed { index, target ->
            events += MotionEvent.obtain(
                0,
                20L * (index + 1),
                MotionEvent.ACTION_MOVE,
                target.first,
                target.second,
                0
            )
        }
        val last = targets.last()
        events += MotionEvent.obtain(
            0,
            20L * (targets.size + 1),
            MotionEvent.ACTION_UP,
            last.first,
            last.second,
            0
        )
        events.forEach { event ->
            try {
                assertTrue(view.editorContentFrame.dispatchTouchEvent(event))
            } finally {
                event.recycle()
            }
        }
    }

    private fun engineSelection(adapter: EditorV2Adapter): JSONObject {
        val result = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
        assertTrue(result is EditorV2CallResult.Ok)
        return JSONObject((result as EditorV2CallResult.Ok).value).getJSONObject("selection")
    }

    private val threeCellDocument: String get() = JSONObject(document).apply {
        getJSONArray("content").getJSONObject(0).getJSONArray("content")
            .getJSONObject(0).getJSONArray("content")
            .put(
                JSONObject(
                    """{"type":"table_cell","content":[{"type":"paragraph",""" +
                        """"content":[{"type":"text","text":"three"}]}]}"""
                )
            )
    }.toString()

    private val gridDocument = """{"type":"doc","content":[{"type":"table",""" +
        """"content":[{"type":"table_row",""" +
        """"content":[{"type":"table_cell",""" +
        """"content":[{"type":"paragraph",""" +
        """"content":[{"type":"text","text":"A"}]}]},""" +
        """{"type":"table_cell","content":[{"type":"paragraph",""" +
        """"content":[{"type":"text","text":"B"}]}]}]},""" +
        """{"type":"table_row","content":[{"type":"table_cell",""" +
        """"content":[{"type":"paragraph",""" +
        """"content":[{"type":"text","text":"C"}]}]},""" +
        """{"type":"table_cell","content":[{"type":"paragraph",""" +
        """"content":[{"type":"text","text":"D"}]}]}]}]}]}"""
    private val emptyCellDocument = """{"type":"doc","content":[{"type":"table",""" +
        """"content":[{"type":"table_row",""" +
        """"content":[{"type":"table_cell",""" +
        """"content":[{"type":"paragraph"}]}]}]}]}"""
    private val scrollableGridDocument: String get() = JSONObject(gridDocument).apply {
        val rows = getJSONArray("content").getJSONObject(0).getJSONArray("content")
        val template = JSONObject(rows.getJSONObject(0).toString())
        rows.remove(1)
        repeat(39) { rows.put(JSONObject(template.toString())) }
        for (index in 0 until rows.length()) {
            val cells = rows.getJSONObject(index).getJSONArray("content")
            for (column in 0 until cells.length()) {
                cells.getJSONObject(column).put(
                    "attrs",
                    JSONObject().put("colwidth", org.json.JSONArray().put(180))
                )
            }
        }
    }.toString()

    @Test
    fun `reverse anchor drag crosses head and publishes canonical cell selection`() =
        withMountedSelection(threeCellDocument, 900, 2, 1) { view, adapter, drawing ->
            val openings = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
            val handles = drawing.selectionHandles()
            assertEquals(2, handles.size)
            val anchor = handles.single { it.role == TableSelectionHandleRole.ANCHOR }
            val head = handles.single { it.role == TableSelectionHandleRole.HEAD }
            assertNotEquals(anchor.x, head.x)
            val first = drawing.presentedTableCells().first { it.sourceIndex == 0 }
            val second = drawing.presentedTableCells().first { it.sourceIndex == 1 }
            val beforeDocument = adapter.documentJson()
            val beforeRevision = adapter.baseDocumentRevision
            val beforeEpoch = adapter.positionEpoch
            val beforeHistory = adapter.cachedHistoryState.toString()
            val published = mutableListOf<JSONObject>()
            view.editorEditText.editorListener = object : EditorEditText.EditorListener {
                override fun onEditorUpdate(updateJSON: String) {
                    published += JSONObject(updateJSON).getJSONObject("selection")
                }
                override fun onSelectionChanged(anchor: Int, head: Int) = Unit
            }
            dragHandleThrough(
                view,
                anchor.x + drawing.left,
                anchor.y + drawing.top,
                listOf(
                    second.bounds.centerX() + drawing.left to second.bounds.centerY() + drawing.top,
                    first.bounds.centerX() + drawing.left to first.bounds.centerY() + drawing.top
                )
            )
            val selection = engineSelection(adapter)
            assertEquals(
                openings.getJSONObject(0).getInt("sourcePos"),
                selection.getInt("anchorCell")
            )
            assertEquals(
                openings.getJSONObject(1).getInt("sourcePos"),
                selection.getInt("headCell")
            )
            assertEquals(2, published.size)
            assertEquals(
                openings.getJSONObject(1).getInt("sourcePos"),
                published[0].getInt("anchorCell")
            )
            assertTrue(
                "no publication; epoch=$beforeEpoch -> ${adapter.positionEpoch} " +
                    "rootCell=${view.editorEditText.authoritativeCellSelectionActive} " +
                    "trace=${view.editorEditText.imeTraceSnapshotForTesting()} " +
                    "notes=${adapter.debugNotes}",
                published.isNotEmpty()
            )
            assertEquals(selection.toString(), published.last().toString())
            assertEquals(beforeDocument, adapter.documentJson())
            assertEquals(beforeRevision, adapter.baseDocumentRevision)
            assertEquals(beforeHistory, adapter.cachedHistoryState.toString())
        }

    @Test
    fun `single cell has two distinguishable handles and deterministic overlapping hit`() =
        withMountedSelection(
            emptyCellDocument,
            50,
            0,
            0,
            tableStyle = TableStyle(minColumnWidth = 32f, cellPadding = 0f)
        ) { _, _, drawing ->
            val handles = drawing.selectionHandles()
            assertEquals(2, handles.size)
            assertEquals(
                setOf(TableSelectionHandleRole.ANCHOR, TableSelectionHandleRole.HEAD),
                handles.map { it.role }.toSet()
            )
            assertNotEquals(handles[0].x, handles[1].x)
            assertNotEquals(handles[0].y, handles[1].y)
            val midpointX = (handles[0].x + handles[1].x) / 2f
            val midpointY = (handles[0].y + handles[1].y) / 2f
            val radius = 24f * drawing.resources.displayMetrics.density
            assertTrue(
                "fixture must overlap both 48dp hit targets: $handles midpoint=$midpointX,$midpointY radius=$radius",
                handles.all { kotlin.math.hypot(it.x - midpointX, it.y - midpointY) < radius }
            )
            assertEquals(
                TableSelectionHandleRole.ANCHOR,
                drawing.hitSelectionHandle(midpointX, midpointY)?.role
            )
        }

    @Test
    fun `anti diagonal selection keeps handles on effective union corners`() =
        withMountedSelection(gridDocument, 600, 1, 2) { _, adapter, drawing ->
            val selection = engineSelection(adapter)
            val resolved = resolveEditorCellSelection(selection, adapter.tableIndex)
                as EditorCellSelection.Drawable
            assertEquals(4, resolved.sourceIndices.size)
            val handles = drawing.selectionHandles()
            assertEquals(2, handles.size)
            val cells = drawing.presentedTableCells()
            val left = cells.minOf { it.bounds.left }
            val right = cells.maxOf { it.bounds.right }
            assertTrue(handles.any { it.x < left + 20f })
            assertTrue(handles.any { it.x > right - 20f })
        }

    @Test
    fun `room head drag publishes the dragged cells as presence`() {
        val backend = RecordingAwarenessBackend()
        withMountedSelection(gridDocument, 600, 0, 1, backend = backend, roomBound = true) {
                view,
                adapter,
                drawing
            ->
            val cells = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
            val anchor = cells.getJSONObject(0).getInt("sourcePos")
            val target = cells.getJSONObject(3).getInt("sourcePos")
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val cell = drawing.presentedTableCells().first { it.sourceIndex == 3 }
            backend.selections.clear()

            dragHandle(
                view,
                head.x + drawing.left,
                head.y + drawing.top,
                cell.bounds.centerX() + drawing.left,
                cell.bounds.centerY() + drawing.top
            )

            assertEquals(target, engineSelection(adapter).getInt("headCell"))
            assertTrue("${backend.selections}", backend.selections.isNotEmpty())
            assertTrue(
                "${backend.selections}",
                backend.selections.last().isCellPresence(anchor, target)
            )
        }
    }

    @Test
    fun `room native selection only table command publishes cell presence`() {
        val backend = RecordingAwarenessBackend()
        withMountedSelection(gridDocument, 600, 0, 0, backend = backend, roomBound = true) {
                _,
                adapter,
                _
            ->
            assertNotNull("the mounted view owns native intents", adapter.nativeOwnerId)
            val cells = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
            val first = cells.getJSONObject(0).getInt("sourcePos")
            val second = cells.getJSONObject(1).getInt("sourcePos")
            val caret = requireNotNull(adapter.scalarPositionForDoc(first + 2))
            val before = adapter.documentJson()
            backend.selections.clear()

            assertNotNull(
                adapter.commandAtSelection(
                    JSONObject().put("type", "selectTableRows"),
                    caret,
                    caret
                )
            )

            assertEquals(before, adapter.documentJson())
            assertTrue(
                "${backend.selections}",
                backend.selections.last().isCellPresence(first, second)
            )
        }
    }

    @Test
    fun `head drag addresses nested only outer cell without selecting its descendant`() =
        withMountedSelection(EditorTableSurfaceMountTest.nestedTableDocument, 900, 0, 2) {
                view,
                adapter,
                drawing
            ->
            val outer = adapter.tableRecordsForTesting.values.minBy { it.getInt("tablePos") }
            val openings = outer.getJSONArray("cells")
            val nestedOnly = openings.getJSONObject(1).getInt("sourcePos")
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val target = drawing.presentedTableCells().first {
                it.surface.editorTableId == outer.getString("sourceId") &&
                    it.sourceIndex == 1
            }
            val before = adapter.documentJson()
            dragHandle(
                view,
                head.x + drawing.left,
                head.y + drawing.top,
                target.bounds.centerX() + drawing.left,
                target.bounds.centerY() + drawing.top
            )
            val selection = engineSelection(adapter)
            assertEquals(
                openings.getJSONObject(0).getInt("sourcePos"),
                selection.getInt("anchorCell")
            )
            assertEquals(nestedOnly, selection.getInt("headCell"))
            assertEquals(before, adapter.documentJson())
        }

    @Test
    fun `rtl handle geometry mirrors without reversing source endpoint roles`() {
        val rtl = JSONObject(gridDocument).apply {
            getJSONArray("content").getJSONObject(0)
                .put("attrs", JSONObject().put("dir", "rtl"))
        }.toString()
        withMountedSelection(rtl, 600, 0, 3, schemaConfig = rtlConfig) { view, adapter, drawing ->
            val handles = drawing.selectionHandles()
            assertEquals(2, handles.size)
            val anchor = handles.single { it.role == TableSelectionHandleRole.ANCHOR }
            val head = handles.single { it.role == TableSelectionHandleRole.HEAD }
            assertTrue(anchor.x > head.x)
            val cells = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
            assertEquals(cells.getJSONObject(0).getInt("sourcePos"), anchor.sourcePosition)
            assertEquals(cells.getJSONObject(3).getInt("sourcePos"), head.sourcePosition)
            val target = drawing.presentedTableCells().first {
                it.sourceIndex == 1
            }
            dragHandle(
                view,
                head.x + drawing.left,
                head.y + drawing.top,
                target.bounds.centerX() + drawing.left,
                target.bounds.centerY() + drawing.top
            )
            assertEquals(
                cells.getJSONObject(1).getInt("sourcePos"),
                engineSelection(adapter).getInt("headCell")
            )
        }
    }

    @Test
    fun `merged span closure keeps both handles on selected real union`() {
        val merged = JSONObject(gridDocument).apply {
            val rows = getJSONArray("content").getJSONObject(0).getJSONArray("content")
            val firstRow = rows.getJSONObject(0).getJSONArray("content")
            firstRow.getJSONObject(0).put("attrs", JSONObject().put("colspan", 2))
            firstRow.remove(1)
        }.toString()
        withMountedSelection(merged, 600, 0, 2) { _, adapter, drawing ->
            val selection = engineSelection(adapter)
            val resolved = resolveEditorCellSelection(selection, adapter.tableIndex)
                as EditorCellSelection.Drawable
            assertEquals(3, resolved.sourceIndices.size)
            assertEquals(2, drawing.selectionHandles().size)
            assertEquals(
                resolved.sourceIndices,
                drawing.presentedTableCells().map { it.sourceIndex }.toSet()
            )
        }
    }

    @Test
    fun `synthetic gap never becomes a drag target`() {
        val irregular = JSONObject(gridDocument).apply {
            getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(1).getJSONArray("content").remove(1)
        }.toString()
        withMountedSelection(irregular, 600, 0, 1) { view, adapter, drawing ->
            val record = adapter.tableRecordsForTesting.values.single()
            assertTrue(
                requireNotNull(
                    adapter.tableIndex.record(record.getString("sourceId"))
                ).syntheticRegions.isNotEmpty()
            )
            val cells = drawing.presentedTableCells()
            val upperRight = cells.first { it.sourceIndex == 1 }
            val lowerLeft = cells.first { it.sourceIndex == 2 }
            val x = upperRight.bounds.centerX()
            val y = lowerLeft.bounds.centerY()
            assertNull(drawing.selectedTableCellAt(x, y, record.getString("sourceId")))
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val before = engineSelection(adapter).toString()
            dragHandle(
                view,
                head.x + drawing.left,
                head.y + drawing.top,
                x + drawing.left,
                y + drawing.top
            )
            assertEquals(before, engineSelection(adapter).toString())
        }
    }

    @Test
    fun `ragged selected union places both handles on occupied cell edges`() {
        val irregular = JSONObject(gridDocument).apply {
            getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(1).getJSONArray("content").remove(1)
        }.toString()
        withMountedSelection(irregular, 600, 1, 2) { _, adapter, drawing ->
            val selection = engineSelection(adapter)
            val selected = resolveEditorCellSelection(selection, adapter.tableIndex)
                as EditorCellSelection.Drawable
            assertEquals(3, selected.sourceIndices.size)
            val handles = drawing.selectionHandles()
            assertEquals(2, handles.size)
            assertTrue(
                handles.all { handle ->
                    drawing.presentedTableCells().any { cell ->
                        cell.sourceIndex in selected.sourceIndices &&
                            cell.bounds.contains(handle.x, handle.y)
                    }
                }
            )
        }
    }

    @Test
    fun `document replacement cancels held handle before same-position target can be reused`() =
        withMountedSelection(threeCellDocument, 900, 0, 1) { view, adapter, drawing ->
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val third = drawing.presentedTableCells().last()
            val down = MotionEvent.obtain(
                0,
                0,
                MotionEvent.ACTION_DOWN,
                head.x + drawing.left,
                head.y + drawing.top,
                0
            )
            try {
                assertTrue(view.editorContentFrame.dispatchTouchEvent(down))
            } finally {
                down.recycle()
            }
            val previousRevision = adapter.baseDocumentRevision
            val replacement = threeCellDocument.replace("three", "fresh")
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    replaceTableDocumentExternallyForTest(adapter, replacement)
                )
            )
            assertTrue(adapter.baseDocumentRevision > previousRevision)
            val after = adapter.documentJson()
            val move = MotionEvent.obtain(
                0,
                20,
                MotionEvent.ACTION_MOVE,
                third.bounds.centerX() + drawing.left,
                third.bounds.centerY() + drawing.top,
                0
            )
            val up = MotionEvent.obtain(
                0,
                30,
                MotionEvent.ACTION_UP,
                third.bounds.centerX() + drawing.left,
                third.bounds.centerY() + drawing.top,
                0
            )
            try {
                view.editorContentFrame.dispatchTouchEvent(move)
                view.editorContentFrame.dispatchTouchEvent(up)
            } finally {
                move.recycle()
                up.recycle()
            }
            assertEquals(after, adapter.documentJson())
            assertNotEquals(
                drawing.tableCellDocumentPosition!!.invoke(
                    third.surface.editorTableId!!,
                    third.sourceIndex
                ),
                engineSelection(adapter).optInt("headCell", -1)
            )
        }

    @Test
    fun `foreign cell selection invalidates held endpoint owner`() =
        withMountedSelection(threeCellDocument, 900, 0, 1) { view, adapter, drawing ->
            val record = adapter.tableRecordsForTesting.values.single()
            val cells = record.getJSONArray("cells")
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val down = MotionEvent.obtain(
                0,
                0,
                MotionEvent.ACTION_DOWN,
                head.x + drawing.left,
                head.y + drawing.top,
                0
            )
            try {
                assertTrue(view.editorContentFrame.dispatchTouchEvent(down))
            } finally {
                down.recycle()
            }
            fun point(index: Int) = JSONObject().put("kind", "document")
                .put("offset", cells.getJSONObject(index).getInt("sourcePos"))
            val external = JSONObject().put("type", "cell")
                .put("anchorCell", point(1)).put("headCell", point(2))
            assertTrue(
                adapter.callWithEnvelope(JSONObject().put("selection", external)) {
                    UniffiEditorV2Backend.setSelection(adapter.editorId, it)
                } is EditorV2CallResult.Ok
            )
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
            val first = drawing.presentedTableCells().first()
            val move = MotionEvent.obtain(
                0,
                20,
                MotionEvent.ACTION_MOVE,
                first.bounds.centerX() + drawing.left,
                first.bounds.centerY() + drawing.top,
                0
            )
            try {
                view.editorContentFrame.dispatchTouchEvent(move)
            } finally {
                move.recycle()
            }
            assertEquals(
                cells.getJSONObject(1).getInt("sourcePos"),
                engineSelection(adapter).getInt("anchorCell")
            )
            assertEquals(
                cells.getJSONObject(2).getInt("sourcePos"),
                engineSelection(adapter).getInt("headCell")
            )
        }

    @Test
    fun `cancel releases captured pointer while a nonzero pointer can complete a new drag`() =
        withMountedSelection(threeCellDocument, 900, 0, 1) { view, adapter, drawing ->
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val third = drawing.presentedTableCells().last()
            fun event(
                action: Int,
                time: Long,
                x: Float,
                y: Float,
                pointerId: Int = 7
            ): MotionEvent {
                val properties = arrayOf(
                    MotionEvent.PointerProperties().apply {
                        id = pointerId
                        toolType = MotionEvent.TOOL_TYPE_FINGER
                    }
                )
                val coordinates = arrayOf(
                    MotionEvent.PointerCoords().apply {
                        this.x = x
                        this.y = y
                        pressure = 1f
                        size = 1f
                    }
                )
                return MotionEvent.obtain(
                    0, time, action, 1, properties, coordinates,
                    0, 0, 1f, 1f, 31, 0, InputDevice.SOURCE_TOUCHSCREEN, 0
                )
            }
            fun otherPointer(action: Int, time: Long, x: Float, y: Float): MotionEvent {
                val properties = arrayOf(7, 8).map { pointerId ->
                    MotionEvent.PointerProperties().apply {
                        id = pointerId
                        toolType = MotionEvent.TOOL_TYPE_FINGER
                    }
                }.toTypedArray()
                val coordinates = arrayOf(0f, 10f).map { offset ->
                    MotionEvent.PointerCoords().apply {
                        this.x = x + offset
                        this.y = y
                        pressure = 1f
                        size = 1f
                    }
                }.toTypedArray()
                return MotionEvent.obtain(
                    0, time, action, 2, properties, coordinates,
                    0, 0, 1f, 1f, 31, 0, InputDevice.SOURCE_TOUCHSCREEN, 0
                )
            }
            val fromX = head.x + drawing.left
            val fromY = head.y + drawing.top
            val toX = third.bounds.centerX() + drawing.left
            val toY = third.bounds.centerY() + drawing.top
            listOf(
                event(MotionEvent.ACTION_DOWN, 0, fromX, fromY),
                event(MotionEvent.ACTION_CANCEL, 10, fromX, fromY),
                event(MotionEvent.ACTION_MOVE, 20, toX, toY)
            ).forEach { motion ->
                try {
                    view.editorContentFrame.dispatchTouchEvent(motion)
                } finally {
                    motion.recycle()
                }
            }
            assertEquals(
                adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
                    .getJSONObject(1).getInt("sourcePos"),
                engineSelection(adapter).getInt("headCell")
            )
            listOf(
                event(MotionEvent.ACTION_DOWN, 25, fromX, fromY),
                otherPointer(
                    MotionEvent.ACTION_POINTER_DOWN or
                        (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT),
                    30,
                    fromX,
                    fromY
                ),
                event(MotionEvent.ACTION_MOVE, 35, toX, toY)
            ).forEach { motion ->
                try {
                    view.editorContentFrame.dispatchTouchEvent(motion)
                } finally {
                    motion.recycle()
                }
            }
            assertEquals(
                "second pointer must cancel the captured drag",
                adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
                    .getJSONObject(1).getInt("sourcePos"),
                engineSelection(adapter).getInt("headCell")
            )
            listOf(
                event(MotionEvent.ACTION_DOWN, 40, fromX, fromY),
                event(MotionEvent.ACTION_MOVE, 50, toX, toY, pointerId = 8),
                event(MotionEvent.ACTION_MOVE, 60, toX, toY)
            ).forEach { motion ->
                try {
                    view.editorContentFrame.dispatchTouchEvent(motion)
                } finally {
                    motion.recycle()
                }
            }
            assertEquals(
                "lost pointer must cancel the captured drag",
                adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
                    .getJSONObject(1).getInt("sourcePos"),
                engineSelection(adapter).getInt("headCell")
            )
            listOf(
                event(MotionEvent.ACTION_DOWN, 70, fromX, fromY),
                event(MotionEvent.ACTION_MOVE, 90, toX, toY),
                event(MotionEvent.ACTION_UP, 100, toX, toY)
            ).forEach { motion ->
                try {
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(motion))
                } finally {
                    motion.recycle()
                }
            }
            assertEquals(
                drawing.tableCellDocumentPosition!!.invoke(
                    third.surface.editorTableId!!,
                    third.sourceIndex
                ),
                engineSelection(adapter).getInt("headCell")
            )
        }

    @Test
    fun `read only and pending composition hide actionable handles`() =
        withMountedSelection(document, 600, 0, 1) { view, adapter, drawing ->
            val root = view.editorEditText
            assertEquals(2, drawing.selectionHandles().size)
            root.isEditable = false
            view.applyTheme(EditorTheme())
            assertTrue(drawing.selectionHandles().isEmpty())
            root.isEditable = true
            root.externalTextComposition = ExternalTextCompositionState(
                "pending",
                "x",
                0,
                0,
                root.lastAuthorizedText,
                root.lastAuthorizedRenderedText
            )
            view.applyTheme(EditorTheme())
            assertTrue(root.hasPendingCompositionForExternalRefresh())
            assertTrue(drawing.selectionHandles().isEmpty())
            root.externalTextComposition = null
            view.applyTheme(EditorTheme())
            assertEquals(2, drawing.selectionHandles().size)
            root.isEnabled = false
            view.applyTheme(EditorTheme())
            assertTrue(drawing.selectionHandles().isEmpty())
            assertEquals("cell", engineSelection(adapter).getString("type"))
        }

    @Test
    fun `rebind cancels held handle without restoring or retargeting selection`() =
        withMountedSelection(threeCellDocument, 900, 0, 1) { view, adapter, drawing ->
            val head = drawing.selectionHandles().single {
                it.role == TableSelectionHandleRole.HEAD
            }
            val third = drawing.presentedTableCells().last()
            val before = engineSelection(adapter).toString()
            val documentBefore = adapter.documentJson()
            val down = MotionEvent.obtain(
                0,
                0,
                MotionEvent.ACTION_DOWN,
                head.x + drawing.left,
                head.y + drawing.top,
                0
            )
            try {
                assertTrue(view.editorContentFrame.dispatchTouchEvent(down))
            } finally {
                down.recycle()
            }
            val token = view.editorId
            view.editorId = 0L
            view.editorId = token
            val move = MotionEvent.obtain(
                0,
                20,
                MotionEvent.ACTION_MOVE,
                third.bounds.centerX() + drawing.left,
                third.bounds.centerY() + drawing.top,
                0
            )
            try {
                view.editorContentFrame.dispatchTouchEvent(move)
            } finally {
                move.recycle()
            }
            assertEquals(before, engineSelection(adapter).toString())
            assertEquals(documentBefore, adapter.documentJson())
        }

    @Test
    fun `detach cancels held handle and queued frame`() = withMountedSelection(
        JSONObject(threeCellDocument).apply {
            val cells = getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getJSONArray("content")
            for (index in 0 until cells.length()) {
                cells.getJSONObject(index).put(
                    "attrs",
                    JSONObject().put("colwidth", org.json.JSONArray().put(200))
                )
            }
        }.toString(),
        300,
        0,
        0
    ) { view, adapter, drawing ->
        val activity = Robolectric.buildActivity(Activity::class.java).setup()
        try {
            val container = FrameLayout(activity.get())
            (view.parent as? ViewGroup)?.removeView(view)
            container.addView(view, FrameLayout.LayoutParams(300, 500))
            activity.get().setContentView(container)
            container.measure(
                View.MeasureSpec.makeMeasureSpec(300, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            container.layout(0, 0, 300, 500)
            val surface = drawing.preparedLayout!!.blocks.single().tableSurface!!
            assertTrue(surface.bounds.width() > surface.hostViewportWidth)
            val head = drawing.selectionHandles().single {
                it.role ==
                    TableSelectionHandleRole.HEAD
            }
            val down = MotionEvent.obtain(
                0,
                0,
                MotionEvent.ACTION_DOWN,
                head.x + drawing.left,
                head.y + drawing.top,
                0
            )
            val move = MotionEvent.obtain(
                0,
                20,
                MotionEvent.ACTION_MOVE,
                view.editorScrollView.width - 8f,
                head.y + drawing.top,
                0
            )
            try {
                assertTrue(view.editorContentFrame.dispatchTouchEvent(down))
                assertTrue(view.editorContentFrame.dispatchTouchEvent(move))
            } finally {
                down.recycle()
                move.recycle()
            }
            val before = engineSelection(adapter).toString()
            val offset = drawing.tablePhysicalOffsetForTesting(surface.identity)
            container.removeView(view)
            Shadows.shadowOf(Looper.getMainLooper()).idleFor(250, TimeUnit.MILLISECONDS)
            assertEquals(before, engineSelection(adapter).toString())
            assertEquals(offset, drawing.tablePhysicalOffsetForTesting(surface.identity))
        } finally {
            activity.pause().stop().destroy()
        }
    }

    @Test
    fun `imported nested selection and unavailable projection expose no outer handles`() =
        withMountedSelection(EditorTableSurfaceMountTest.nestedTableDocument, 900, 0, 2) {
                view,
                adapter,
                drawing
            ->
            assertEquals(2, drawing.selectionHandles().size)
            val originalIndex = adapter.tableIndex
            adapter.tableIndex = originalIndex.replacingRecordsForTesting {
                if (it.host ==
                    null
                ) {
                    it.copy(failure = uniffi.editor_core.TableRenderFailure.INVALID_STRUCTURE)
                } else {
                    it
                }
            }
            view.editorEditText.onSelectionOrContentMayChange?.invoke()
            assertTrue(drawing.selectionHandles().isEmpty())
            adapter.tableIndex = originalIndex
            view.editorEditText.onSelectionOrContentMayChange?.invoke()
            assertEquals(2, drawing.selectionHandles().size)
            val nested = adapter.tableRecordsForTesting.values.maxBy { it.getInt("tablePos") }
            val opening = nested.getJSONArray("cells").getJSONObject(0).getInt("sourcePos")
            val point = JSONObject().put("kind", "document").put("offset", opening)
            val selection = JSONObject().put("type", "cell")
                .put("anchorCell", point).put("headCell", point)
            assertTrue(
                adapter.callWithEnvelope(JSONObject().put("selection", selection)) {
                    UniffiEditorV2Backend.setSelection(adapter.editorId, it)
                } is EditorV2CallResult.Ok
            )
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
            assertTrue(drawing.selectionHandles().isEmpty())
        }

    @Test
    fun `scrolled table clips handles at their real positions without edge substitutes`() {
        val wide = JSONObject(document).apply {
            val cells = getJSONArray("content").getJSONObject(0).getJSONArray("content")
                .getJSONObject(0).getJSONArray("content")
            for (index in 0 until cells.length()) {
                cells.getJSONObject(index).put(
                    "attrs",
                    JSONObject().put("colwidth", org.json.JSONArray().put(600))
                )
            }
        }.toString()
        withMountedSelection(wide, 600, 0, 1) { _, _, drawing ->
            val surface = drawing.preparedLayout!!.blocks.single().tableSurface!!
            assertTrue(surface.bounds.width() > surface.hostViewportWidth)
            assertEquals(
                listOf(TableSelectionHandleRole.ANCHOR),
                drawing.selectionHandles().map { it.role }
            )
            val visibleAnchor = drawing.selectionHandles().single()
            assertEquals(
                TableSelectionHandleRole.ANCHOR,
                drawing.hitSelectionHandle(visibleAnchor.x, visibleAnchor.y)?.role
            )
            drawing.setTableLogicalOffset(surface.identity, 300f)
            assertTrue(drawing.selectionHandles().isEmpty())
            drawing.setTableLogicalOffset(surface.identity, 600f)
            assertEquals(
                listOf(TableSelectionHandleRole.HEAD),
                drawing.selectionHandles().map { it.role }
            )
            val visibleHead = drawing.selectionHandles().single()
            assertEquals(
                TableSelectionHandleRole.HEAD,
                drawing.hitSelectionHandle(visibleHead.x, visibleHead.y)?.role
            )
        }
    }

    @Test
    @LooperMode(LooperMode.Mode.PAUSED)
    fun `held pointer outside visible host cannot select an unexposed real row`() =
        withMountedSelection(scrollableGridDocument, 300, 0, 0) { view, adapter, drawing ->
            val activity = Robolectric.buildActivity(Activity::class.java).setup()
            try {
                val container = FrameLayout(activity.get())
                (view.parent as? ViewGroup)?.removeView(view)
                container.addView(view, FrameLayout.LayoutParams(300, 500))
                activity.get().setContentView(container)
                container.measure(
                    View.MeasureSpec.makeMeasureSpec(300, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
                )
                container.layout(0, 0, 300, 500)
                val visible = Rect()
                assertTrue(drawing.getLocalVisibleRect(visible))
                val tableId = adapter.tableRecordsForTesting.keys.single()
                val invisible = drawing.presentedTableCells().first { cell ->
                    cell.bounds.centerY() > visible.bottom + 24f &&
                        cell.bounds.centerX() < visible.right
                }
                val x = invisible.bounds.centerX()
                val y = invisible.bounds.centerY()
                assertTrue(invisible.clip.contains(x, y))
                assertNull(
                    "unexposed real row is not a drag target",
                    drawing.selectedTableCellAt(x, y, tableId)
                )
                val head = drawing.selectionHandles().single {
                    it.role ==
                        TableSelectionHandleRole.HEAD
                }
                val initial = engineSelection(adapter).toString()
                val down = MotionEvent.obtain(
                    0,
                    0,
                    MotionEvent.ACTION_DOWN,
                    head.x + drawing.left,
                    head.y + drawing.top,
                    0
                )
                val move = MotionEvent.obtain(
                    0,
                    20,
                    MotionEvent.ACTION_MOVE,
                    x + drawing.left,
                    y + drawing.top,
                    0
                )
                val up = MotionEvent.obtain(
                    0,
                    30,
                    MotionEvent.ACTION_UP,
                    x + drawing.left,
                    y + drawing.top,
                    0
                )
                try {
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(down))
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(move))
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(up))
                } finally {
                    down.recycle()
                    move.recycle()
                    up.recycle()
                }
                assertEquals(initial, engineSelection(adapter).toString())
            } finally {
                activity.pause().stop().destroy()
            }
        }

    @Test
    fun `visible handle halo owns drag only within halo and host`() =
        withMountedSelection(threeCellDocument, 300, 0, 1) { view, adapter, drawing ->
            val activity = Robolectric.buildActivity(Activity::class.java).setup()
            try {
                val container = FrameLayout(activity.get())
                (view.parent as? ViewGroup)?.removeView(view)
                container.addView(view, FrameLayout.LayoutParams(300, 500))
                activity.get().setContentView(container)
                container.measure(
                    View.MeasureSpec.makeMeasureSpec(300, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
                )
                container.layout(0, 0, 300, 500)
                val head = drawing.selectionHandles().single {
                    it.role ==
                        TableSelectionHandleRole.HEAD
                }
                val table = requireNotNull(drawing.selectedTableViewport(head.tableId))
                val radius = 24f * drawing.resources.displayMetrics.density
                val haloY = table.bottom + (radius - (table.bottom - head.y)) / 2f
                val visible = Rect()
                assertTrue(drawing.getLocalVisibleRect(visible))
                assertTrue(haloY < visible.bottom)
                assertTrue(haloY > table.bottom)
                assertNull(drawing.hitSelectionHandle(head.x, table.bottom + radius))
                val target = drawing.presentedTableCells().last()
                val before = adapter.documentJson()
                dragHandle(
                    view,
                    head.x + drawing.left,
                    haloY + drawing.top,
                    target.bounds.centerX() + drawing.left,
                    target.bounds.centerY() + drawing.top
                )
                assertEquals(
                    drawing.tableCellDocumentPosition!!.invoke(
                        target.surface.editorTableId!!,
                        target.sourceIndex
                    ),
                    engineSelection(adapter).getInt("headCell")
                )
                assertEquals(before, adapter.documentJson())
                val movedHead = drawing.selectionHandles().single {
                    it.role == TableSelectionHandleRole.HEAD
                }
                val clippedBottom = (movedHead.y + radius / 4f).toInt()
                val outsideHostY = movedHead.y + radius / 2f
                assertTrue(outsideHostY > clippedBottom)
                assertTrue(kotlin.math.abs(outsideHostY - movedHead.y) < radius)
                assertEquals(
                    TableSelectionHandleRole.HEAD,
                    drawing.hitSelectionHandle(movedHead.x, outsideHostY)?.role
                )
                container.layout(0, 0, 300, clippedBottom)
                val clipped = Rect()
                assertTrue(drawing.getLocalVisibleRect(clipped))
                assertTrue(clipped.bottom < outsideHostY)
                assertNull(
                    "halo beyond host clipping is not actionable",
                    drawing.hitSelectionHandle(movedHead.x, outsideHostY)
                )
            } finally {
                activity.pause().stop().destroy()
            }
        }

    @Test
    @LooperMode(LooperMode.Mode.PAUSED)
    fun `held handle scrolls table and document on frames then stops on cancel`() {
        withMountedSelection(scrollableGridDocument, 300, 0, 0) { view, adapter, drawing ->
            val activity = Robolectric.buildActivity(Activity::class.java).setup()
            try {
                val container = FrameLayout(activity.get())
                (view.parent as? ViewGroup)?.removeView(view)
                container.addView(view, FrameLayout.LayoutParams(300, 500))
                activity.get().setContentView(container)
                container.measure(
                    View.MeasureSpec.makeMeasureSpec(300, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
                )
                container.layout(0, 0, 300, 500)
                val surface = drawing.preparedLayout!!.blocks.single().tableSurface!!
                val initialHorizontal = drawing.tablePhysicalOffsetForTesting(surface.identity)
                val initialVertical = view.editorScrollView.scrollY
                assertTrue(surface.bounds.width() > surface.hostViewportWidth)
                assertTrue(view.editorScrollView.canScrollVertically(1))
                val head = drawing.selectionHandles().single {
                    it.role ==
                        TableSelectionHandleRole.HEAD
                }
                val visibleBefore = Rect()
                assertTrue(drawing.getLocalVisibleRect(visibleBefore))
                val before = adapter.documentJson()
                val revision = adapter.baseDocumentRevision
                val down = MotionEvent.obtain(
                    0,
                    0,
                    MotionEvent.ACTION_DOWN,
                    head.x + drawing.left,
                    head.y + drawing.top,
                    0
                )
                val move = MotionEvent.obtain(
                    0,
                    20,
                    MotionEvent.ACTION_MOVE,
                    visibleBefore.right - 8f + drawing.left,
                    visibleBefore.bottom - 8f + drawing.top,
                    0
                )
                try {
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(down))
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(move))
                } finally {
                    down.recycle()
                    move.recycle()
                }
                val headAfterMove = engineSelection(adapter).getInt("headCell")
                val looper = Shadows.shadowOf(Looper.getMainLooper())
                var headAfterFrames = headAfterMove
                repeat(128) {
                    if (drawing.tablePhysicalOffsetForTesting(
                            surface.identity
                        ) == initialHorizontal ||
                        view.editorScrollView.scrollY == initialVertical ||
                        headAfterFrames == headAfterMove
                    ) {
                        looper.runOneTask()
                        headAfterFrames = engineSelection(adapter).getInt("headCell")
                    }
                }
                assertTrue(
                    "horizontal frame scroll did not advance",
                    drawing.tablePhysicalOffsetForTesting(surface.identity) > initialHorizontal
                )
                assertTrue(
                    "vertical frame scroll did not advance",
                    view.editorScrollView.scrollY > initialVertical
                )
                val maxScroll = view.editorScrollView.getChildAt(0).height -
                    view.editorScrollView.height
                assertTrue(
                    "fixture reached document bottom before rehit",
                    view.editorScrollView.scrollY < maxScroll
                )
                val tableId = adapter.tableRecordsForTesting.keys.single()
                val visibleAfter = Rect()
                assertTrue(drawing.getLocalVisibleRect(visibleAfter))
                val pointedCell = drawing.selectedTableCellAt(
                    visibleAfter.right - 8f,
                    visibleAfter.bottom - 8f,
                    tableId
                )
                assertNotNull(
                    "frame rehit lost visible real cell: visible=$visibleAfter " +
                        "scroll=${view.editorScrollView.scrollY}",
                    pointedCell
                )
                val scrolledSelection = engineSelection(adapter)
                assertNotEquals(
                    "frame scrolling must select a newly reached cell",
                    headAfterMove,
                    scrolledSelection.getInt("headCell")
                )
                assertEquals(pointedCell, scrolledSelection.getInt("headCell"))
                assertEquals(
                    adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
                        .getJSONObject(0).getInt("sourcePos"),
                    scrolledSelection.getInt("anchorCell")
                )
                val cancel = MotionEvent.obtain(0, 300, MotionEvent.ACTION_CANCEL, 0f, 0f, 0)
                try {
                    view.editorContentFrame.dispatchTouchEvent(cancel)
                } finally {
                    cancel.recycle()
                }
                val stoppedHorizontal = drawing.tablePhysicalOffsetForTesting(surface.identity)
                val stoppedVertical = view.editorScrollView.scrollY
                Shadows.shadowOf(Looper.getMainLooper()).idleFor(250, TimeUnit.MILLISECONDS)
                assertEquals(
                    stoppedHorizontal,
                    drawing.tablePhysicalOffsetForTesting(surface.identity)
                )
                assertEquals(stoppedVertical, view.editorScrollView.scrollY)
                assertEquals(before, adapter.documentJson())
                assertEquals(revision, adapter.baseDocumentRevision)
                val real = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
                val positions = (0 until real.length()).map {
                    real.getJSONObject(it).getInt("sourcePos")
                }.toSet()
                assertTrue(engineSelection(adapter).getInt("headCell") in positions)
            } finally {
                activity.pause().stop().destroy()
            }
        }
    }

    @Test
    @LooperMode(LooperMode.Mode.PAUSED)
    fun `held handle keeps scrolling from a padded already scrolled viewport`() =
        withMountedSelection(scrollableGridDocument, 300, 8, 8) { view, adapter, drawing ->
            val activity = Robolectric.buildActivity(Activity::class.java).setup()
            try {
                val container = FrameLayout(activity.get())
                (view.parent as? ViewGroup)?.removeView(view)
                container.addView(view, FrameLayout.LayoutParams(300, 500))
                activity.get().setContentView(container)
                view.editorScrollView.setPadding(0, 24, 0, 16)
                container.measure(
                    View.MeasureSpec.makeMeasureSpec(300, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
                )
                container.layout(0, 0, 300, 500)
                view.editorScrollView.scrollTo(0, 120)
                assertEquals(120, view.editorScrollView.scrollY)
                val head = drawing.selectionHandles().single {
                    it.role ==
                        TableSelectionHandleRole.HEAD
                }
                val originalAnchor = engineSelection(adapter).getInt("anchorCell")
                val down = MotionEvent.obtain(
                    0,
                    0,
                    MotionEvent.ACTION_DOWN,
                    head.x + drawing.left,
                    head.y + drawing.top,
                    0
                )
                val move = MotionEvent.obtain(
                    0,
                    20,
                    MotionEvent.ACTION_MOVE,
                    view.editorScrollView.width - 8f,
                    view.editorScrollView.scrollY + view.editorScrollView.height - 8f -
                        view.editorContentFrame.top,
                    0
                )
                try {
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(down))
                    assertTrue(view.editorContentFrame.dispatchTouchEvent(move))
                } finally {
                    down.recycle()
                    move.recycle()
                }
                val looper = Shadows.shadowOf(Looper.getMainLooper())
                repeat(32) {
                    if (view.editorScrollView.scrollY == 120) looper.runOneTask()
                }
                val firstScroll = view.editorScrollView.scrollY
                assertTrue("first held frames did not scroll", firstScroll > 120)
                val maxScroll = view.editorScrollView.getChildAt(0).height -
                    view.editorScrollView.height + view.editorScrollView.paddingBottom
                assertTrue(
                    "fixture exhausted scroll range before second frame",
                    firstScroll < maxScroll
                )
                repeat(32) {
                    if (view.editorScrollView.scrollY == firstScroll) looper.runOneTask()
                }
                assertTrue(
                    (
                        "held pointer stopped: first=$firstScroll " +
                            "second=${view.editorScrollView.scrollY} "
                        ) +
                        "max=$maxScroll " +
                        "selection=${engineSelection(
                            adapter
                        )} handles=${drawing.selectionHandles()}",
                    view.editorScrollView.scrollY > firstScroll
                )
                assertEquals(originalAnchor, engineSelection(adapter).getInt("anchorCell"))
                val cancel = MotionEvent.obtain(0, 600, MotionEvent.ACTION_CANCEL, 0f, 0f, 0)
                try {
                    view.editorContentFrame.dispatchTouchEvent(cancel)
                } finally {
                    cancel.recycle()
                }
            } finally {
                activity.pause().stop().destroy()
            }
        }

    @Test
    fun `engine generated cell selection survives atomic Android admission`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        try {
            assertNotNull(adapter.setContentJson(document))
            val (anchor, head) = selectCells(adapter)
            assertNotNull(adapter.refreshFromRustState(null))
            val raw = requireNotNull(adapter.cachedAtomicRenderJson)
            val wire = JSONObject(raw).getJSONObject("selection")
            assertEquals(setOf("type", "anchorCell", "headCell"), wire.keys().asSequence().toSet())
            assertEquals(anchor, wire.getInt("anchorCell"))
            assertEquals(head, wire.getInt("headCell"))
            assertNotNull(
                "Android rejects genuine engine selection",
                parseAtomicRenderSnapshot(raw)
            )
            val mismatched = JSONObject(raw).put(
                "selection",
                JSONObject(wire.toString())
                    .put("headCell", head + 10000)
            )
            assertNull(
                "cell opening outside the admitted table",
                resolveEditorCellSelection(
                    mismatched.getJSONObject("selection"),
                    adapter.tableIndex
                )
            )
            assertNull(
                parseAtomicRenderSnapshot(
                    JSONObject(raw).put(
                        "selection",
                        JSONObject(wire.toString())
                            .put("anchorScalar", 0)
                    ).toString()
                )
            )
            assertNull(
                parseAtomicRenderSnapshot(
                    JSONObject(raw).put(
                        "selection",
                        JSONObject(wire.toString())
                            .put("headCell", "9")
                    ).toString()
                )
            )
            assertTrue(
                resolveEditorCellSelection(wire, adapter.tableIndex) is EditorCellSelection.Drawable
            )
            val unavailable = adapter.tableIndex.replacingRecordsForTesting {
                it.copy(failure = uniffi.editor_core.TableRenderFailure.INVALID_STRUCTURE)
            }
            assertTrue(
                "preserved selection must survive an unavailable projection",
                resolveEditorCellSelection(wire, unavailable) is EditorCellSelection.Unavailable
            )
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `merged multirow rectangle closes over spans in either layout direction`() {
        val merged = """{"type":"doc","content":[{"type":"table",""" +
            """"content":[{"type":"table_row",""" +
            """"content":[{"type":"table_cell","attrs":{"colspan":2},""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"A"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"B"}]}]}]},""" +
            """{"type":"table_row","content":[{"type":"table_cell",""" +
            """"content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"C"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"D"}]}]},""" +
            """{"type":"table_cell","content":[{"type":"paragraph",""" +
            """"content":[{"type":"text","text":"E"}]}]}]}]}]}"""
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        try {
            assertNotNull(adapter.setContentJson(merged))
            selectCells(adapter, anchorIndex = 1, headIndex = 3)
            val raw = (
                UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
                    as EditorV2CallResult.Ok
                ).value
            val selection = JSONObject(raw).getJSONObject("selection")
            val record = adapter.tableRecordsForTesting.values.single()
            val realCells = record.getJSONArray("cells")
            val expected = (0 until realCells.length()).toSet()
            assertEquals(5, expected.size)
            assertEquals(
                expected,
                (
                    resolveEditorCellSelection(selection, adapter.tableIndex)
                        as EditorCellSelection.Drawable
                    ).sourceIndices
            )
            val rtl = adapter.tableIndex.replacingRecordsForTesting { it.copy(direction = "rtl") }
            assertEquals(
                expected,
                (
                    resolveEditorCellSelection(
                        selection,
                        rtl
                    ) as EditorCellSelection.Drawable
                    ).sourceIndices
            )
            val unavailable = adapter.tableIndex.replacingRecordsForTesting {
                it.copy(failure = uniffi.editor_core.TableRenderFailure.INVALID_STRUCTURE)
            }
            assertTrue(
                resolveEditorCellSelection(
                    selection,
                    unavailable
                ) is EditorCellSelection.Unavailable
            )
            val separateTables = JSONObject(document).apply {
                val content = getJSONArray("content")
                content.put(JSONObject(content.getJSONObject(0).toString()))
            }
            assertNotNull(adapter.setContentJson(separateTables.toString()))
            val tableRecords = adapter.tableRecordsForTesting
            assertEquals(2, tableRecords.size)
            val openings = tableRecords.values.map {
                it.getJSONArray("cells").getJSONObject(0).getInt("sourcePos")
            }
            val crossTable = JSONObject().put("type", "cell")
                .put("anchorCell", openings[0]).put("headCell", openings[1])
            assertNull(resolveEditorCellSelection(crossTable, adapter.tableIndex))
            val atomic = JSONObject(requireNotNull(adapter.cachedAtomicRenderJson))
                .put("selection", crossTable)
            assertNotNull(parseAtomicRenderSnapshot(atomic.toString()))
            assertNull(resolveEditorCellSelection(crossTable, adapter.tableIndex))
        } finally {
            adapter.destroy()
        }
    }

    @Test
    fun `cell selection blocks stale root caret and input without a document write`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        val token = EditorV2Registry.register(adapter)
        try {
            val initial = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            view.measure(
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, 600, 500)
            val root = view.editorEditText
            root.requestFocus()
            val staleConnection = requireNotNull(root.onCreateInputConnection(EditorInfo()))
            val before = adapter.documentJson()
            val (_, headOpening) = selectCells(adapter)
            val selected = requireNotNull(adapter.refreshFromRustState(null))
            assertTrue(root.applyUpdateJSON(selected))
            assertTrue(root.rootTableSelectionInputBlocked)
            assertTrue("cell selection must preserve editor focus", root.hasFocus())
            assertEquals(null, root.caretRect())

            root.setSelection(root.text.length)
            staleConnection.commitText("stale", 1)

            assertTrue(
                "cell authority was replaced by stale root caret",
                root.rootTableSelectionInputBlocked
            )
            assertEquals(
                "cell",
                JSONObject(requireNotNull(adapter.selectionJson())).optString("type", "")
            )
            assertEquals(before, adapter.documentJson())

            val textScalar = requireNotNull(adapter.scalarPositionForDoc(headOpening + 2))
            val point = JSONObject().put("offset", textScalar).put("kind", "scalar")
            val textSelection = JSONObject().put("type", "text")
                .put("anchor", point).put("head", point)
            val textResult = adapter.callWithEnvelope(
                JSONObject().put("selection", textSelection)
            ) {
                UniffiEditorV2Backend.setSelection(adapter.editorId, it)
            }
            assertTrue(textResult is EditorV2CallResult.Ok)
            assertTrue(root.applyUpdateJSON(requireNotNull(adapter.refreshFromRustState(null))))
            assertFalse(
                "external text-in-cell retained cell authority",
                root.authoritativeCellSelectionActive
            )
            assertTrue(
                "root input must remain blocked for cell text",
                root.rootTableSelectionInputBlocked
            )
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `engine rectangle paints real cell frames from prepared geometry`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        val token = EditorV2Registry.register(adapter)
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        val view = RichTextEditorView(activity)
        activity.setContentView(view, FrameLayout.LayoutParams(600, 500))
        try {
            val threeCells = JSONObject(document).apply {
                getJSONArray("content").getJSONObject(0).getJSONArray("content")
                    .getJSONObject(0).getJSONArray("content")
                    .put(
                        JSONObject(
                            """{"type":"table_cell","content":[{"type":"paragraph",""" +
                                """"content":[{"type":"text","text":"three"}]}]}"""
                        )
                    )
            }
            val initial = requireNotNull(adapter.setContentJson(threeCells.toString()))
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            val selectionColor = 0x66A54122
            view.applyTheme(EditorTheme(table = TableStyle(selectionColor = selectionColor)))
            view.measure(
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, 600, 500)
            activity.window.decorView.measure(
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            activity.window.decorView.layout(0, 0, 600, 500)
            fun paint(): List<Int> {
                val drawing = (0 until view.editorContentFrame.childCount)
                    .map { view.editorContentFrame.getChildAt(it) }
                    .filterIsInstance<PreparedProseDrawingView>().single()
                drawing.measure(
                    View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                    View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
                )
                drawing.layout(0, 0, 600, 500)
                val bitmap = Bitmap.createBitmap(600, 500, Bitmap.Config.ARGB_8888)
                assertTrue("paint fixture must have a visible window", drawing.isShown)
                drawing.draw(Canvas(bitmap))
                val block = drawing.preparedLayout!!.blocks.single()
                val bounds = block.tableBounds!!
                val samples = block.tableSurface!!.cells.map { cell ->
                    bitmap.getPixel(
                        (
                            bounds.left + block.tableSurface!!.frameOfCell(cell).left +
                                block.tableSurface!!.frameOfCell(cell).width -
                                20
                            ).toInt(),
                        (
                            bounds.top + block.tableSurface!!.frameOfCell(cell).top +
                                block.tableSurface!!.frameOfCell(cell).height / 2
                            ).toInt()
                    )
                }
                bitmap.recycle()
                return samples
            }
            val before = paint()
            selectCells(adapter)
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
            val selectedDrawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            assertEquals(2, selectedDrawing.selectedTableCellSourceIndices.values.single().size)
            val after = paint()
            assertEquals(3, after.size)
            assertNotEquals("first selected frame unchanged", before[0], after[0])
            assertNotEquals("second selected frame unchanged", before[1], after[1])
            assertEquals("unselected real cell changed", before[2], after[2])
            fun assertThemeColor(actual: Int) {
                assertEquals(Color.alpha(selectionColor), Color.alpha(actual))
                assertTrue(kotlin.math.abs(Color.red(selectionColor) - Color.red(actual)) <= 2)
                assertTrue(kotlin.math.abs(Color.green(selectionColor) - Color.green(actual)) <= 2)
                assertTrue(kotlin.math.abs(Color.blue(selectionColor) - Color.blue(actual)) <= 2)
            }
            assertThemeColor(after[0])
            assertThemeColor(after[1])
        } finally {
            view.editorId = 0L
            controller.pause().stop().destroy()
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `mounted head handle extends authoritative selection without editing document`() {
        withMountedSelection(threeCellDocument, 900, 0, 1, exactSelection = false) {
                view,
                adapter,
                drawing
            ->
            val openings = adapter.tableRecordsForTesting.values.single().getJSONArray("cells")
            val block = drawing.preparedLayout!!.blocks.single()
            val table = block.tableSurface!!
            val bounds = block.tableBounds!!
            val second = table.frameOfCell(1)!!
            val third = table.frameOfCell(2)!!
            val fromX = bounds.left + second.left + second.width - 8f
            val fromY = bounds.top + second.top + second.height - 8f
            val toX = bounds.left + third.left + third.width / 2f
            val toY = bounds.top + third.top + third.height / 2f
            val before = adapter.documentJson()
            val revision = adapter.baseDocumentRevision
            listOf(
                MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, fromX, fromY, 0),
                MotionEvent.obtain(0, 20, MotionEvent.ACTION_MOVE, toX, toY, 0),
                MotionEvent.obtain(0, 30, MotionEvent.ACTION_UP, toX, toY, 0)
            ).forEach { event ->
                try {
                    view.editorContentFrame.dispatchTouchEvent(event)
                } finally {
                    event.recycle()
                }
            }
            val render = UniffiEditorV2Backend.renderUpdate(adapter.editorId, null, null)
            assertTrue(render is EditorV2CallResult.Ok)
            val selection = JSONObject(
                (render as EditorV2CallResult.Ok).value
            ).getJSONObject("selection")
            assertEquals("cell", selection.getString("type"))
            assertEquals(
                openings.getJSONObject(0).getInt("sourcePos"),
                selection.getInt("anchorCell")
            )
            assertEquals(
                openings.getJSONObject(2).getInt("sourcePos"),
                selection.getInt("headCell")
            )
            assertEquals(before, adapter.documentJson())
            assertEquals(revision, adapter.baseDocumentRevision)
        }
    }

    @Test
    fun `selection from focused cell retires its connection and retains editor focus`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        val token = EditorV2Registry.register(adapter)
        try {
            val initial = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            view.measure(
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, 600, 500)
            val drawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            drawing.measure(
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            drawing.layout(0, 0, 600, 500)
            val block = drawing.preparedLayout!!.blocks.single()
            val cell = block.tableSurface!!.cells.first()
            val bounds = block.tableBounds!!
            val x =
                bounds.left + block.tableSurface!!.frameOfCell(
                    cell
                ).left + cell.contentOrigin.first +
                    8f
            val y =
                bounds.top + block.tableSurface!!.frameOfCell(
                    cell
                ).top + cell.contentOrigin.second +
                    8f
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
            val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
            try {
                assertTrue(view.dispatchTouchEvent(down))
                assertTrue(view.dispatchTouchEvent(up))
            } finally {
                down.recycle()
                up.recycle()
            }
            val input = view.activeTextInput
            assertTrue(input !== view.editorEditText)
            assertTrue(input.hasFocus())
            val staleConnection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
            val before = adapter.documentJson()

            selectCells(adapter)
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )

            assertTrue(view.activeTextInput === view.editorEditText)
            assertTrue(view.editorEditText.hasFocus())
            staleConnection.commitText("stale", 1)
            assertEquals(before, adapter.documentJson())
            assertEquals(
                "cell",
                JSONObject(requireNotNull(adapter.selectionJson())).optString("type", "")
            )
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }

    @Test
    fun `tapping a selected cell returns to an engine text selection`() {
        val created = UniffiEditorV2Backend.create(config, null) as EditorV2CallResult.Ok
        val adapter = requireNotNull(
            EditorV2Adapter.attach(
                UniffiEditorV2Backend,
                JSONObject(created.value).getString("editorId"),
                false
            )
        )
        val token = EditorV2Registry.register(adapter)
        try {
            val initial = requireNotNull(adapter.setContentJson(document))
            val view = RichTextEditorView(RuntimeEnvironment.getApplication())
            view.editorId = token
            assertTrue(view.editorEditText.applyUpdateJSON(initial))
            view.measure(
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            view.layout(0, 0, 600, 500)
            selectCells(adapter)
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
            val drawing = (0 until view.editorContentFrame.childCount)
                .map { view.editorContentFrame.getChildAt(it) }
                .filterIsInstance<PreparedProseDrawingView>().single()
            drawing.measure(
                View.MeasureSpec.makeMeasureSpec(600, View.MeasureSpec.EXACTLY),
                View.MeasureSpec.makeMeasureSpec(500, View.MeasureSpec.EXACTLY)
            )
            drawing.layout(0, 0, 600, 500)
            val block = drawing.preparedLayout!!.blocks.single()
            val cell = block.tableSurface!!.cells.last()
            val bounds = block.tableBounds!!
            val x =
                bounds.left + block.tableSurface!!.frameOfCell(
                    cell
                ).left + cell.contentOrigin.first +
                    8f
            val y =
                bounds.top + block.tableSurface!!.frameOfCell(
                    cell
                ).top + cell.contentOrigin.second +
                    8f
            val down = MotionEvent.obtain(0, 0, MotionEvent.ACTION_DOWN, x, y, 0)
            val up = MotionEvent.obtain(0, 10, MotionEvent.ACTION_UP, x, y, 0)
            try {
                assertTrue(view.dispatchTouchEvent(down))
                assertTrue(view.dispatchTouchEvent(up))
            } finally {
                down.recycle()
                up.recycle()
            }
            assertTrue(view.activeTextInput !== view.editorEditText)
            assertEquals(
                "text",
                JSONObject(requireNotNull(adapter.selectionJson())).optString("type", "")
            )
            assertFalse(view.editorEditText.authoritativeCellSelectionActive)
            assertTrue(drawing.selectedTableCellSourceIndices.isEmpty())

            val before = adapter.documentJson()
            selectCells(adapter)
            assertTrue(
                view.editorEditText.applyUpdateJSON(
                    requireNotNull(adapter.refreshFromRustState(null))
                )
            )
            val root = view.editorEditText
            val prose = root.text.toString().indexOf("after")
            val line = root.layout.getLineForOffset(prose)
            val proseX = root.left + root.totalPaddingLeft + 16f
            val proseY = root.top + root.totalPaddingTop +
                (root.layout.getLineTop(line) + root.layout.getLineBottom(line)) / 2f
            val proseDown = MotionEvent.obtain(20, 20, MotionEvent.ACTION_DOWN, proseX, proseY, 0)
            val proseUp = MotionEvent.obtain(20, 30, MotionEvent.ACTION_UP, proseX, proseY, 0)
            try {
                assertTrue(view.dispatchTouchEvent(proseDown))
                assertTrue(view.dispatchTouchEvent(proseUp))
            } finally {
                proseDown.recycle()
                proseUp.recycle()
            }
            assertTrue(view.activeTextInput === root)
            assertEquals(
                "text",
                JSONObject(requireNotNull(adapter.selectionJson())).optString("type", "")
            )
            assertFalse(root.authoritativeCellSelectionActive)
            assertFalse(root.rootTableSelectionInputBlocked)
            assertEquals(before, adapter.documentJson())
        } finally {
            EditorV2Registry.remove(adapter.editorId)
            adapter.destroy()
        }
    }
}
